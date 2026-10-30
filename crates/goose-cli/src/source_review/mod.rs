//! `/review`: a Kory Source review of one pull request, run from the terminal. Kory Source
//! runs no model inference, so this client answers each `pending_work` item on the
//! engineer's own router-acp seats. Nothing is ever written to GitHub.

pub mod client;
mod render;
pub mod seat;
pub mod target;

use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use goose::config::Config;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use client::{ApiError, Client, Job, Settings, WorkAnswer, WorkErrorCode, WorkItem};
use seat::{answer_for, RouterSeat, SeatCompleter, COMPLETION_TIMEOUT_CAP};
use target::{GhPrHeads, PrHeads, Target};

pub use render::render_outcome;

/// How far past `deadline_at` to keep polling for the server's own `deadline_exceeded`.
const DEADLINE_GRACE: Duration = Duration::from_secs(30);
/// Consecutive failed status reads tolerated before the service counts as unreachable.
const STATUS_RETRIES: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    NotConfigured(String),
    InvalidTarget(String),
    WorkstationUnreachable(String),
    HeadLookupFailed(String),
    ServiceUnreachable(String),
    AuthFailed(ApiError),
    Rejected {
        target: Target,
        head_sha: String,
        error: ApiError,
    },
    Finished(Job),
    DeadlinePassed(Job),
    Cancelled {
        job_id: Option<String>,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct Cadence {
    pub running: Duration,
    pub queued: Duration,
}

impl Default for Cadence {
    fn default() -> Self {
        Self {
            running: Duration::from_secs(2),
            queued: Duration::from_secs(5),
        }
    }
}

/// The local seat queue: calls running at once, and items allowed to wait for one.
#[derive(Debug, Clone, Copy)]
pub struct SeatLimits {
    pub concurrency: usize,
    pub queue: usize,
}

impl Default for SeatLimits {
    fn default() -> Self {
        Self {
            concurrency: 2,
            queue: 8,
        }
    }
}

pub struct Review {
    client: Client,
    heads: Arc<dyn PrHeads>,
    seats: Arc<dyn SeatCompleter>,
    cadence: Cadence,
    limits: SeatLimits,
}

/// Runs `/review <argument>` with goose's configuration, the engineer's `gh` login, and
/// router-acp seats, then prints the result.
pub async fn run_command(
    argument: &str,
    cwd: &Path,
    cancel: &CancellationToken,
    out: &mut (dyn Write + Send),
) -> Outcome {
    let outcome = match Settings::from_config(Config::global()) {
        Ok(settings) => match Client::new(settings) {
            Ok(client) => {
                Review::new(client, Arc::new(GhPrHeads), Arc::new(RouterSeat))
                    .run(argument, cwd, cancel, out)
                    .await
            }
            Err(error) => Outcome::NotConfigured(error.to_string()),
        },
        Err(error) => Outcome::NotConfigured(error.to_string()),
    };
    render_outcome(&outcome, out);
    outcome
}

impl Review {
    pub fn new(client: Client, heads: Arc<dyn PrHeads>, seats: Arc<dyn SeatCompleter>) -> Self {
        Self {
            client,
            heads,
            seats,
            cadence: Cadence::default(),
            limits: SeatLimits::default(),
        }
    }

    pub fn with_cadence(mut self, cadence: Cadence) -> Self {
        self.cadence = cadence;
        self
    }

    pub fn with_seat_limits(mut self, limits: SeatLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Submits the review and answers its work until the job ends or `cancel` fires.
    /// Progress goes to `out`; the result is returned for [`render_outcome`].
    pub async fn run(
        &self,
        argument: &str,
        cwd: &Path,
        cancel: &CancellationToken,
        out: &mut (dyn Write + Send),
    ) -> Outcome {
        let job_id = OnceLock::new();
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Outcome::Cancelled { job_id: job_id.get().cloned() },
            outcome = self.submit_and_follow(argument, cwd, &job_id, out) => outcome,
        }
    }

    async fn submit_and_follow(
        &self,
        argument: &str,
        cwd: &Path,
        job_id: &OnceLock<String>,
        out: &mut (dyn Write + Send),
    ) -> Outcome {
        let target = match target::resolve(argument, cwd).await {
            Ok(target) => target,
            Err(error) => return Outcome::InvalidTarget(format!("{error:#}")),
        };
        if let Err(error) = self.seats.check() {
            return Outcome::WorkstationUnreachable(format!("{error:#}"));
        }
        let head_sha = match self.heads.head_sha(&target).await {
            Ok(sha) => sha,
            Err(error) => return Outcome::HeadLookupFailed(format!("{error:#}")),
        };
        render::submitting(&target, &head_sha, self.client.base_url(), out);
        let job = match self
            .client
            .submit(&target.repo, target.pr_number, &head_sha)
            .await
        {
            Ok(job) => job,
            Err(error) => {
                return match error {
                    ApiError::Unreachable(detail) => Outcome::ServiceUnreachable(detail),
                    error if error.is_auth() => Outcome::AuthFailed(error),
                    error => Outcome::Rejected {
                        target,
                        head_sha,
                        error,
                    },
                }
            }
        };
        let _ = job_id.set(job.id.clone());
        render::submitted(&job, out);
        self.follow(job, out).await
    }

    async fn follow(&self, mut job: Job, out: &mut (dyn Write + Send)) -> Outcome {
        let mut work = Work::default();
        let mut progress = render::Progress::default();
        let mut status_failures = 0;
        loop {
            if job.is_terminal() {
                return Outcome::Finished(job);
            }
            if past_deadline(&job) {
                return Outcome::DeadlinePassed(job);
            }
            self.schedule(&job, &mut work);
            progress.report(&job, work.running.len(), out);
            let interval = if job.state == "queued" {
                self.cadence.queued
            } else {
                self.cadence.running
            };
            tokio::select! {
                Some(done) = work.running.join_next(), if !work.running.is_empty() => work.finish(done),
                _ = tokio::time::sleep(interval) => {}
            }
            while let Some(done) = work.running.try_join_next() {
                work.finish(done);
            }
            if let Err(error) = self.post_answers(&job.id, &mut work).await {
                return Outcome::AuthFailed(error);
            }
            match self.client.status(&job.id).await {
                Ok(next) => {
                    job = next;
                    status_failures = 0;
                }
                Err(error) if error.is_auth() => return Outcome::AuthFailed(error),
                Err(error) if error.is_transient() && status_failures < STATUS_RETRIES => {
                    status_failures += 1;
                }
                Err(error) => return Outcome::ServiceUnreachable(error.to_string()),
            }
        }
    }

    /// Starts new work items while seats are free. Each `work_id` is taken once: it waits,
    /// runs, or is answered, and is never run again.
    fn schedule(&self, job: &Job, work: &mut Work) {
        let pending: HashSet<&str> = job
            .pending_work
            .iter()
            .map(|item| item.work_id.as_str())
            .collect();
        work.waiting
            .retain(|item| pending.contains(item.work_id.as_str()));
        for item in &job.pending_work {
            if work.seen.insert(item.work_id.clone()) {
                work.waiting.push_back(item.clone());
            }
        }
        while work.running.len() < self.limits.concurrency {
            let Some(item) = work.waiting.pop_front() else {
                break;
            };
            let timeout = Duration::try_from_secs_f64(item.timeout_seconds)
                .unwrap_or(COMPLETION_TIMEOUT_CAP)
                .min(COMPLETION_TIMEOUT_CAP);
            let call = self
                .seats
                .complete(item.prompt, item.model.clone(), timeout);
            work.running.spawn(async move {
                let answer = answer_for(&item.model, call.await);
                (item.work_id, answer)
            });
        }
        // A full queue is seat exhaustion, reported for the oldest item so the server's
        // retry backoff applies rather than a wait with no owner.
        if work.waiting.len() > self.limits.queue {
            if let Some(item) = work.waiting.pop_front() {
                work.unsent.push((
                    item.work_id,
                    WorkAnswer::Error {
                        error: WorkErrorCode::SeatExhausted,
                        actual_model: None,
                    },
                ));
            }
        }
    }

    /// Posts every saved answer. A transient failure keeps the body for the next poll;
    /// an item the server no longer accepts drops it. Only an auth failure stops the review.
    async fn post_answers(&self, job_id: &str, work: &mut Work) -> Result<(), ApiError> {
        let mut kept = Vec::new();
        for (work_id, answer) in std::mem::take(&mut work.unsent) {
            match self.client.answer(job_id, &work_id, &answer).await {
                Ok(()) => {}
                Err(error) if error.is_auth() => return Err(error),
                Err(error) if error.is_transient() => kept.push((work_id, answer)),
                Err(_) => {}
            }
        }
        work.unsent = kept;
        Ok(())
    }
}

#[derive(Default)]
struct Work {
    seen: HashSet<String>,
    waiting: VecDeque<WorkItem>,
    running: JoinSet<(String, WorkAnswer)>,
    unsent: Vec<(String, WorkAnswer)>,
}

impl Work {
    fn finish(&mut self, done: Result<(String, WorkAnswer), tokio::task::JoinError>) {
        if let Ok(answer) = done {
            self.unsent.push(answer);
        }
    }
}

fn past_deadline(job: &Job) -> bool {
    let Some(deadline) = job.deadline_at else {
        return false;
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    now > deadline + DEADLINE_GRACE.as_secs_f64()
}
