//! Progress while an ACP agent answers `initialize`. router-acp holds its `initialize` reply
//! until every provider adapter it starts has been probed (up to its `probe_timeout_ms`, two
//! minutes by default), and a slow adapter used to leave the session on a blank screen for that
//! whole time. After [`WARN_AFTER`] the session is told it is still starting, and why when the
//! agent's stderr names a cause. Failed verification is reported when observed; elapsed time
//! alone does not establish that an adapter failed.

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub const WARN_AFTER: Duration = Duration::from_secs(10);
/// router-acp's default `probe_timeout_ms`.
pub const FAIL_AFTER: Duration = Duration::from_secs(120);
/// How often the waiting message is refreshed.
pub const TICK: Duration = Duration::from_secs(5);
/// After a slow start, how long to wait past the reply for stderr to name a failed provider.
pub const NAME_GRACE: Duration = Duration::from_millis(500);

/// A failure message that names the provider(s), rather than only the elapsed time.
pub fn names_a_provider(text: &str) -> bool {
    text.contains(" failed verification;")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// Still waiting: one line, refreshed in place.
    Waiting(String),
    /// Verification failed, initialization failed, or startup is taking unusually long.
    Failed(String),
    /// `initialize` succeeded after a waiting message was shown.
    Done(String),
}

type Hook = Box<dyn Fn(Progress) + Send + Sync>;
static HOOK: OnceLock<Hook> = OnceLock::new();
static CAUSES: Mutex<Causes> = Mutex::new(Causes::new());

/// Where progress is shown. The CLI installs one; without it progress only reaches the log.
pub fn set_hook(hook: impl Fn(Progress) + Send + Sync + 'static) {
    let _ = HOOK.set(Box::new(hook));
}

pub(crate) fn report(progress: Progress) {
    tracing::info!(?progress, "agent startup progress");
    if let Some(hook) = HOOK.get() {
        hook(progress);
    }
}

/// One line of the agent's stderr, for causes.
pub(crate) fn observe_stderr(line: &str) {
    CAUSES.lock().unwrap().observe(line);
}

pub(crate) fn snapshot() -> Causes {
    CAUSES.lock().unwrap().clone()
}

/// A new wait starts with no causes from an earlier agent.
pub(crate) fn reset() {
    *CAUSES.lock().unwrap() = Causes::new();
}

pub(crate) fn completion<T, E>(
    agent: &str,
    elapsed: Duration,
    reply: &Result<Result<T, E>, tokio::sync::oneshot::error::RecvError>,
) -> Progress {
    let seconds = elapsed.as_secs();
    match reply {
        Ok(Ok(_)) => Progress::Done(format!("{agent} started after {seconds}s")),
        Ok(Err(_)) => Progress::Failed(format!("{agent} initialization failed after {seconds}s")),
        Err(_) => Progress::Failed(format!(
            "{agent} initialization was cancelled after {seconds}s"
        )),
    }
}

/// What the agent's stderr said about its provider adapters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Causes {
    /// Targets (e.g. `codex@<id>`) router-acp reported failing verification, in order.
    pub failed: Vec<String>,
    /// Targets whose adapter router-acp started.
    pub started: Vec<String>,
}

impl Causes {
    const fn new() -> Self {
        Self {
            failed: Vec::new(),
            started: Vec::new(),
        }
    }

    fn observe(&mut self, raw: &str) {
        let line = strip_ansi(raw);
        let Some(target) = field(&line, "target") else {
            return;
        };
        let target = target.to_string();
        if line.contains("failed verification") {
            if !self.failed.contains(&target) {
                self.failed.push(target);
            }
        } else if line.contains("interposing adapter") && !self.started.contains(&target) {
            self.started.push(target);
        }
    }
}

/// The message for `elapsed` into the wait, or none before [`WARN_AFTER`].
pub fn message(agent: &str, elapsed: Duration, causes: &Causes) -> Option<Progress> {
    if elapsed < WARN_AFTER {
        return None;
    }
    let seconds = elapsed.as_secs();
    if !causes.failed.is_empty() {
        return Some(Progress::Failed(format!(
            "{agent}: {} failed verification; waiting for initialization to finish",
            names(&causes.failed)
        )));
    }
    if elapsed >= FAIL_AFTER {
        return Some(Progress::Failed(format!(
            "{agent} has not finished starting its provider adapters after {seconds}s; still waiting for it"
        )));
    }
    let detail = if causes.started.is_empty() {
        String::new()
    } else {
        format!(" ({})", names(&causes.started))
    };
    Some(Progress::Waiting(format!(
        "{agent} is still starting provider adapters{detail}: {seconds}s…"
    )))
}

/// `codex@abcd…, grok@ef01…` — targets with their account ids shortened.
fn names(targets: &[String]) -> String {
    targets
        .iter()
        .map(|target| match target.split_once('@') {
            Some((provider, id)) if id.chars().count() > 8 => {
                format!("{provider}@{}…", id.chars().take(8).collect::<String>())
            }
            _ => target.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The value of `key=` in a tracing line, up to the next space.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let at = line.find(&format!("{key}="))? + key.len() + 1;
    let value = line.get(at..)?.split_whitespace().next()?;
    Some(value.trim_matches('"')).filter(|v| !v.is_empty())
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAILED: &str = "\x1b[2m2026-11-19T15:57:17.369790Z\x1b[0m \x1b[33m WARN\x1b[0m \x1b[2mrouter_acp::session\x1b[0m\x1b[2m:\x1b[0m downstream target failed verification \x1b[3mtarget\x1b[0m\x1b[2m=\x1b[0mcodex@d16ac4a2639a4d9aa6ed0038ffd9c271 \x1b[3mreason\x1b[0m\x1b[2m=\x1b[0mtarget codex@d16ac4a2639a4d9aa6ed0038ffd9c271 did not finish initialize/probe within 120000ms";
    const STARTED: &str = "2026-11-19T15:55:17.325495Z  INFO router_acp::llm_proxy: interposing adapter inference traffic target=grok@752deed240aa env=";

    fn causes(lines: &[&str]) -> Causes {
        let mut causes = Causes::default();
        for line in lines {
            causes.observe(line);
        }
        causes
    }

    #[test]
    fn router_stderr_names_the_started_and_failed_targets() {
        let causes = causes(&[STARTED, FAILED, FAILED, "unrelated line"]);
        assert_eq!(causes.started, ["grok@752deed240aa"]);
        assert_eq!(causes.failed, ["codex@d16ac4a2639a4d9aa6ed0038ffd9c271"]);
    }

    #[test]
    fn nothing_is_shown_for_the_first_ten_seconds() {
        assert_eq!(
            message("router-acp", Duration::from_secs(9), &Causes::default()),
            None
        );
    }

    #[test]
    fn a_slow_start_warns_with_the_elapsed_time_and_the_adapters_it_started() {
        let Some(Progress::Waiting(text)) =
            message("router-acp", Duration::from_secs(40), &causes(&[STARTED]))
        else {
            panic!("expected a waiting message");
        };
        assert!(text.contains("grok@752deed2…"), "{text}");
        assert!(text.contains("40s"), "{text}");

        let Some(Progress::Waiting(text)) = message(
            "router-acp",
            Duration::from_secs(40),
            &causes(&["interposing adapter target=codex@開発者用アカウント名"]),
        ) else {
            panic!("expected a waiting message for a Unicode account");
        };
        assert!(text.contains("codex@開発者用アカウン…"), "{text}");
    }

    #[test]
    fn a_failed_verification_is_named_as_an_error() {
        let Some(Progress::Failed(text)) = message(
            "router-acp",
            Duration::from_secs(30),
            &causes(&[STARTED, FAILED]),
        ) else {
            panic!("expected a failure");
        };
        assert!(text.contains("codex@d16ac4a2…"), "{text}");
        assert!(!text.contains("120"), "no timeout was observed: {text}");
    }

    #[test]
    fn only_a_failure_naming_the_provider_replaces_a_generic_one() {
        let generic = message("router-acp", Duration::from_secs(120), &Causes::default());
        let named = message("router-acp", Duration::from_secs(121), &causes(&[FAILED]));
        let (Some(Progress::Failed(generic)), Some(Progress::Failed(named))) = (generic, named)
        else {
            panic!("expected failures");
        };
        assert!(!names_a_provider(&generic), "{generic}");
        assert!(
            names_a_provider(&named) && named.contains("codex@d16ac4a2…"),
            "{named}"
        );
    }

    #[test]
    fn past_the_timeout_with_no_named_cause_it_still_says_so() {
        let Some(Progress::Failed(text)) =
            message("router-acp", Duration::from_secs(130), &Causes::default())
        else {
            panic!("expected a failure");
        };
        assert!(text.contains("after 130s"), "{text}");
    }

    #[tokio::test]
    async fn initialization_reports_success_only_for_a_successful_reply() {
        for result in [Some(Ok(())), Some(Err("initialize rejected")), None] {
            let succeeded = matches!(result, Some(Ok(())));
            let (sender, receiver) = tokio::sync::oneshot::channel();
            if let Some(result) = result {
                sender.send(result).unwrap();
            } else {
                drop(sender);
            }
            let reply = receiver.await;
            let progress = completion("router-acp", Duration::from_secs(130), &reply);
            assert_eq!(matches!(progress, Progress::Done(_)), succeeded);
            if !succeeded {
                assert!(matches!(progress, Progress::Failed(_)));
            }
        }
    }
}
