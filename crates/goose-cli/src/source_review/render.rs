use std::io::Write;

use console::style;
use url::Url;

use super::client::{ApiError, Finding, Job, TOKEN_KEY, URL_KEY};
use super::target::Target;
use super::Outcome;

fn short(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

pub(super) fn submitting(target: &Target, head_sha: &str, service: &Url, out: &mut dyn Write) {
    let _ = writeln!(
        out,
        "{} {target} at {} via {service}",
        style("Kory Source review").bold(),
        short(head_sha),
    );
}

pub(super) fn submitted(job: &Job, out: &mut dyn Write) {
    let note = if job.deduplicated {
        " (already running for this head; following it)"
    } else {
        ""
    };
    let _ = writeln!(out, "  job {}{note}", job.id);
}

/// Prints a progress line whenever the job's state, stage, or running call count changes.
#[derive(Default)]
pub(super) struct Progress {
    last: Option<String>,
}

impl Progress {
    pub(super) fn report(&mut self, job: &Job, running_calls: usize, out: &mut dyn Write) {
        let mut line = match job.stage.as_deref().filter(|stage| !stage.is_empty()) {
            Some(stage) if job.state == "running" => format!("running · {stage}"),
            _ => job.state.clone(),
        };
        match running_calls {
            0 => {}
            1 => line.push_str(" · running model call 1"),
            n => line.push_str(&format!(" · running {n} model calls")),
        }
        if self.last.as_deref() != Some(line.as_str()) {
            let _ = writeln!(out, "  {}", style(&line).dim());
            self.last = Some(line);
        }
    }
}

pub fn render_outcome(outcome: &Outcome, out: &mut dyn Write) {
    let fail = |out: &mut dyn Write, headline: &str, detail: &str| {
        let _ = writeln!(out, "{} {headline}", style("✗").red().bold());
        if !detail.is_empty() {
            let _ = writeln!(out, "  {detail}");
        }
    };
    match outcome {
        Outcome::NotConfigured(detail) => fail(
            out,
            "Kory Source review is not configured.",
            &format!(
                "{detail}. Set {URL_KEY} to the service URL and {TOKEN_KEY} to your engineer \
                 review token (environment, or `kory configure`)."
            ),
        ),
        Outcome::InvalidTarget(detail) => fail(out, "Cannot tell which PR to review.", detail),
        Outcome::WorkstationUnreachable(detail) => fail(
            out,
            "Workstation unreachable: router-acp is not available, so this review's model calls \
             cannot run on your seats. Nothing was submitted.",
            detail,
        ),
        Outcome::HeadLookupFailed(detail) => {
            fail(out, "Could not read the PR head from GitHub.", detail)
        }
        Outcome::ServiceUnreachable(detail) => {
            fail(out, "Kory Source service unreachable.", detail)
        }
        Outcome::AuthFailed(error) => fail(
            out,
            "Kory Source auth failure: the review token was refused.",
            &format!("{}. Check {TOKEN_KEY}.", describe(error)),
        ),
        Outcome::Rejected {
            target,
            head_sha,
            error,
        } => match error {
            ApiError::Http {
                code: Some(code),
                current_head_sha,
                ..
            } if code == "head_sha_mismatch" => fail(
                out,
                &format!("head_sha_mismatch: {target} moved before the review started."),
                &format!(
                    "Requested {}, but the PR head is {}. Nothing was reviewed; run /review again.",
                    short(head_sha),
                    current_head_sha.as_deref().map(short).unwrap_or("unknown"),
                ),
            ),
            error => fail(
                out,
                "Kory Source did not start the review.",
                &describe(error),
            ),
        },
        Outcome::Cancelled { job_id } => {
            let _ = writeln!(out, "{} Review cancelled.", style("■").yellow());
            if let Some(id) = job_id {
                let _ = writeln!(
                    out,
                    "  Kory Source ends job {id} on its own once its unanswered model calls time out."
                );
            }
        }
        Outcome::DeadlinePassed(job) => {
            fail(
                out,
                "The review passed its deadline without finishing.",
                &format!("Job {} is still {}.", job.id, job.state),
            );
            job_footer(job, out);
        }
        Outcome::Finished(job) => finished(job, out),
    }
}

fn describe(error: &ApiError) -> String {
    match error {
        ApiError::Unreachable(detail) => detail.clone(),
        ApiError::Http {
            status,
            code,
            message,
            ..
        } => match code {
            Some(code) if message.is_empty() => format!("HTTP {status} {code}"),
            Some(code) => format!("HTTP {status} {code}: {message}"),
            None => format!("HTTP {status}: {message}"),
        },
    }
}

fn failure_headline(code: &str) -> String {
    match code {
        "seat_exhausted" => {
            "seat_exhausted: your model seats were full on every attempt. Retry later.".into()
        }
        "workstation_outage" => "workstation_outage: this workstation could not run the review's \
             model calls on every attempt. Check router-acp, then retry."
            .into(),
        "model_timeout" => "model_timeout: a review model call got no answer within its timeout."
            .into(),
        "head_sha_mismatch" => {
            "head_sha_mismatch: the PR moved before capture. Nothing was reviewed; run /review again."
                .into()
        }
        "model_mismatch" => {
            "model_mismatch: a pinned model was not the model that ran.".into()
        }
        code => format!("{code}: the review failed."),
    }
}

fn finished(job: &Job, out: &mut dyn Write) {
    match job.state.as_str() {
        "succeeded" => {
            let reviewed = job.reviewed_head_sha.as_deref().unwrap_or("unknown");
            let _ = writeln!(
                out,
                "{} Reviewed {} at {reviewed}",
                style("✓").green().bold(),
                job_label(job),
            );
            findings(job.findings.as_deref().unwrap_or_default(), out);
            if let Some(summary) = job.summary.as_deref().filter(|s| !s.trim().is_empty()) {
                let _ = writeln!(out, "\nSummary\n  {}", summary.trim().replace('\n', "\n  "));
            }
            if let Some(coverage) = &job.coverage {
                let _ = writeln!(
                    out,
                    "\nCoverage: {} of {} changed files reviewed",
                    coverage.reviewed_files, coverage.changed_files
                );
                for skipped in &coverage.skipped {
                    let _ = writeln!(out, "  skipped {} ({})", skipped.path, skipped.reason);
                }
            }
        }
        "interrupted" => {
            let _ = writeln!(
                out,
                "{} The review of {} was interrupted before it finished. Run /review again.",
                style("✗").red().bold(),
                job_label(job),
            );
        }
        _ => {
            let error = job.error.clone().unwrap_or_default();
            let code = if error.code.is_empty() {
                "review_failed"
            } else {
                error.code.as_str()
            };
            let _ = writeln!(
                out,
                "{} {}",
                style("✗").red().bold(),
                failure_headline(code)
            );
            if !error.message.is_empty() {
                let _ = writeln!(out, "  {}", error.message);
            }
        }
    }
    job_footer(job, out);
}

fn job_label(job: &Job) -> String {
    format!("{}#{}", job.repo, job.pr_number)
}

fn findings(findings: &[Finding], out: &mut dyn Write) {
    if findings.is_empty() {
        let _ = writeln!(out, "\nNo findings.");
        return;
    }
    let _ = writeln!(out, "\nFindings ({})", findings.len());
    for finding in findings {
        let location = match (&finding.path, finding.line, finding.end_line) {
            (Some(path), Some(line), Some(end)) if end != line => format!("{path}:{line}-{end}"),
            (Some(path), Some(line), _) => format!("{path}:{line}"),
            (Some(path), None, _) => path.clone(),
            (None, ..) => "(no file)".to_string(),
        };
        let category = finding
            .category
            .as_deref()
            .map(|c| format!(" ({c})"))
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "  [{}] {location} — {}{category}",
            style(&finding.severity).bold(),
            finding.title,
        );
        if !finding.registry_ids.is_empty() {
            let _ = writeln!(out, "    registry: {}", finding.registry_ids.join(", "));
        }
        for line in finding.body.trim().lines() {
            let _ = writeln!(out, "    {line}");
        }
        if let Some(suggestion) = finding.suggestion.as_deref().filter(|s| !s.is_empty()) {
            let _ = writeln!(out, "    suggestion: {}", suggestion.trim());
        }
    }
}

/// Requested vs actual models and the report link, shown for every finished job.
fn job_footer(job: &Job, out: &mut dyn Write) {
    let list = |models: &[String]| {
        if models.is_empty() {
            "none".to_string()
        } else {
            models.join(", ")
        }
    };
    let _ = writeln!(
        out,
        "\nModels: requested {} · actual {}",
        list(&job.models.requested),
        list(&job.models.actual)
    );
    if let Some(url) = &job.report_url {
        let _ = writeln!(out, "Report: {url}");
    }
}
