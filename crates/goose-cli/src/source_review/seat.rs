use std::path::PathBuf;
use std::time::Duration;

use futures::future::BoxFuture;
use goose::conversation::message::Message;
use goose::providers::base::Provider;
use goose_providers::model::ModelConfig;

use super::client::{WorkAnswer, WorkErrorCode};

/// The longest one review completion may run, matching the Kory Code relay.
pub const COMPLETION_TIMEOUT_CAP: Duration = Duration::from_secs(900);

const UNPINNED: &[&str] = &[
    "",
    "auto",
    "default",
    "router-default",
    "router-acp-default",
];

pub fn is_pinned(model: &str) -> bool {
    !UNPINNED.contains(&model.trim())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    Text {
        text: String,
        actual_model: Option<String>,
    },
    Failed {
        failure: Failure,
        actual_model: Option<String>,
        detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    SeatExhausted,
    WorkstationOutage,
    Timeout,
    ModelMismatch,
    RouterFailed { retryable: bool },
}

/// Runs one Kory Source work item's prompt on the engineer's own seats.
pub trait SeatCompleter: Send + Sync {
    /// Ready to take work, or why this workstation cannot run any.
    fn check(&self) -> anyhow::Result<()>;

    fn complete(
        &self,
        prompt: String,
        model: String,
        timeout: Duration,
    ) -> BoxFuture<'static, Completion>;
}

/// The body to post for a finished call, following the Kory Code relay's outcome map.
pub fn answer_for(requested_model: &str, completion: Completion) -> WorkAnswer {
    let error = |error, actual_model| WorkAnswer::Error {
        error,
        actual_model,
    };
    match completion {
        Completion::Text { text, actual_model } => {
            if is_pinned(requested_model) && actual_model.as_deref() != Some(requested_model) {
                error(WorkErrorCode::ModelMismatch, actual_model)
            } else if text.trim().is_empty() {
                error(WorkErrorCode::Timeout, actual_model)
            } else {
                WorkAnswer::Text {
                    text,
                    actual_model: actual_model.unwrap_or_else(|| "unknown".to_string()),
                }
            }
        }
        Completion::Failed {
            failure,
            actual_model,
            ..
        } => {
            let code = match failure {
                Failure::SeatExhausted => WorkErrorCode::SeatExhausted,
                Failure::WorkstationOutage | Failure::RouterFailed { retryable: true } => {
                    WorkErrorCode::WorkstationOutage
                }
                Failure::ModelMismatch => WorkErrorCode::ModelMismatch,
                Failure::Timeout | Failure::RouterFailed { retryable: false } => {
                    WorkErrorCode::Timeout
                }
            };
            error(code, actual_model)
        }
    }
}

pub fn classify_router_error(message: &str) -> Failure {
    let message = message.to_lowercase();
    let any = |needles: &[&str]| needles.iter().any(|needle| message.contains(needle));
    if any(&[
        "enoent",
        "not installed",
        "no such file",
        "econnrefused",
        "econnreset",
        "connection refused",
        "connection reset",
        "broken pipe",
        "socket hang up",
        "agent process exited",
    ]) {
        Failure::WorkstationOutage
    } else if any(&[
        "seat",
        "quota",
        "rate limit",
        "usage cap",
        "cordon",
        "exhausted",
    ]) {
        Failure::SeatExhausted
    } else if any(&["timed out", "timeout"]) {
        Failure::Timeout
    } else {
        Failure::RouterFailed { retryable: false }
    }
}

/// Strips router-acp's routing disclosure (a `> router-acp · ...` blockquote line and its
/// `> why:`-style details) and returns the visible text with the last model it names.
pub fn split_disclosure(raw: &str) -> (String, Option<String>) {
    const DETAILS: &[&str] = &["why: ", "effort: ", "note: ", "skipped "];
    let mut kept = Vec::new();
    let mut actual = None;
    let mut in_disclosure = false;
    for line in raw.lines() {
        let quoted = line.trim_start().trim_start_matches('>').trim_start();
        if let Some(headline) = quoted.strip_prefix("router-acp · ") {
            in_disclosure = true;
            if let Some(model) = routed_model(headline) {
                actual = Some(model);
            }
            continue;
        }
        let is_detail = line.trim_start().starts_with('>')
            && (DETAILS.iter().any(|d| quoted.starts_with(d)) || quoted.contains(" is cordoned:"));
        if in_disclosure && is_detail {
            continue;
        }
        in_disclosure = false;
        kept.push(line);
    }
    (kept.join("\n").trim().to_string(), actual)
}

/// The candidate a disclosure headline routed to: `switched a → b` or `x → b`, but not a
/// `delegate_task` frame, which names a tool's model rather than this session's.
fn routed_model(headline: &str) -> Option<String> {
    let headline = headline
        .strip_prefix("switched ")
        .or_else(|| headline.strip_prefix("failover: "))
        .unwrap_or(headline);
    if headline.starts_with("delegate_task") {
        return None;
    }
    let (_, after) = headline.split_once(" → ")?;
    let model = after.split_whitespace().next()?;
    is_pinned(model).then(|| model.to_string())
}

/// router-acp's own config, layered over the user's accounts, for one prompt that is the
/// whole prompt the model sees: no pre-classifier, ticket fetch, skills, or delegation, and a
/// throwaway state store. Disclosure rides the text (`chunk`) because goose's ACP provider
/// does not surface `_meta`, and the routed model is read from that notice.
fn completion_overlay(store: &std::path::Path) -> String {
    format!(
        "router: auto\n\
         disclosure: chunk\n\
         pre_classifier:\n  enabled: false\n\
         ticket_context: null\n\
         skill_routing: null\n\
         delegation:\n  inject_prompt: false\n\
         state_file: {}\n",
        serde_json::to_string(&store.display().to_string()).expect("strings serialize")
    )
}

/// One fresh router-acp session per work item, in a temporary project whose overlay keeps
/// the router from adding anything to the prompt.
pub struct RouterSeat;

impl SeatCompleter for RouterSeat {
    fn check(&self) -> anyhow::Result<()> {
        goose::providers::router_acp::router_binary().map(drop)
    }

    fn complete(
        &self,
        prompt: String,
        model: String,
        timeout: Duration,
    ) -> BoxFuture<'static, Completion> {
        Box::pin(async move {
            match tokio::time::timeout(timeout, run_router(prompt, model)).await {
                Ok(completion) => completion,
                Err(_) => Completion::Failed {
                    failure: Failure::Timeout,
                    actual_model: None,
                    detail: format!("no answer within {}s", timeout.as_secs()),
                },
            }
        })
    }
}

async fn run_router(prompt: String, model: String) -> Completion {
    let failed = |failure, detail: String| Completion::Failed {
        failure,
        actual_model: None,
        detail,
    };
    let workdir = match tempfile::Builder::new().prefix("kory-review-").tempdir() {
        Ok(dir) => dir,
        Err(error) => return failed(Failure::WorkstationOutage, error.to_string()),
    };
    let overlay: PathBuf = workdir.path().join(".router-acp/router.yaml");
    let written = std::fs::create_dir_all(workdir.path().join(".router-acp")).and_then(|_| {
        std::fs::write(
            &overlay,
            completion_overlay(&workdir.path().join("completion.db")),
        )
    });
    if let Err(error) = written {
        return failed(Failure::WorkstationOutage, error.to_string());
    }
    let candidate = is_pinned(&model).then(|| model.trim().to_string());
    let provider = match goose::providers::router_acp::connect_completion_session(
        workdir.path().to_path_buf(),
        overlay,
        candidate,
    )
    .await
    {
        Ok(provider) => provider,
        Err(error) => {
            let detail = format!("{error:#}");
            return failed(classify_router_error(&detail), detail);
        }
    };
    let messages = [Message::user().with_text(prompt)];
    let model_config = ModelConfig::new(goose::acp::ACP_CURRENT_MODEL);
    let result = provider.complete(&model_config, "", &messages, &[]).await;
    drop(provider);
    match result {
        Ok((message, _usage)) => {
            let (text, actual_model) = split_disclosure(&message.as_concat_text());
            Completion::Text { text, actual_model }
        }
        Err(error) => {
            let detail = error.to_string();
            failed(classify_router_error(&detail), detail)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str, actual: Option<&str>) -> Completion {
        Completion::Text {
            text: text.into(),
            actual_model: actual.map(Into::into),
        }
    }

    fn failed(failure: Failure) -> Completion {
        Completion::Failed {
            failure,
            actual_model: None,
            detail: String::new(),
        }
    }

    fn error(code: WorkErrorCode, actual: Option<&str>) -> WorkAnswer {
        WorkAnswer::Error {
            error: code,
            actual_model: actual.map(Into::into),
        }
    }

    #[test]
    fn outcomes_map_like_the_kory_code_relay() {
        assert_eq!(
            answer_for("auto", text("ok", Some("grok/grok-4.6"))),
            WorkAnswer::Text {
                text: "ok".into(),
                actual_model: "grok/grok-4.6".into()
            }
        );
        assert_eq!(
            answer_for("auto", text("ok", None)),
            WorkAnswer::Text {
                text: "ok".into(),
                actual_model: "unknown".into()
            }
        );
        assert_eq!(
            answer_for("auto", text("  ", Some("a/b"))),
            error(WorkErrorCode::Timeout, Some("a/b"))
        );
        assert_eq!(
            answer_for("grok/grok-4.6", text("ok", Some("claude/opus"))),
            error(WorkErrorCode::ModelMismatch, Some("claude/opus"))
        );
        assert_eq!(
            answer_for("grok/grok-4.6", text("ok", None)),
            error(WorkErrorCode::ModelMismatch, None)
        );
        for (failure, code) in [
            (Failure::SeatExhausted, WorkErrorCode::SeatExhausted),
            (Failure::WorkstationOutage, WorkErrorCode::WorkstationOutage),
            (
                Failure::RouterFailed { retryable: true },
                WorkErrorCode::WorkstationOutage,
            ),
            (Failure::Timeout, WorkErrorCode::Timeout),
            (Failure::ModelMismatch, WorkErrorCode::ModelMismatch),
            (
                Failure::RouterFailed { retryable: false },
                WorkErrorCode::Timeout,
            ),
        ] {
            assert_eq!(answer_for("auto", failed(failure)), error(code, None));
        }
    }

    #[test]
    fn router_errors_classify_by_cause() {
        assert_eq!(
            classify_router_error("spawn router-acp ENOENT"),
            Failure::WorkstationOutage
        );
        assert_eq!(
            classify_router_error("Connection refused (os error 111)"),
            Failure::WorkstationOutage
        );
        assert_eq!(
            classify_router_error("every candidate is cordoned: usage cap"),
            Failure::SeatExhausted
        );
        assert_eq!(classify_router_error("prompt timed out"), Failure::Timeout);
        assert_eq!(
            classify_router_error("invalid params"),
            Failure::RouterFailed { retryable: false }
        );
    }

    #[test]
    fn disclosure_lines_name_the_model_and_leave_the_text() {
        let raw = "> router-acp · pareto-code → grok/grok-4.6 · task Feature (complexity 0.40)\n> why: cheapest\n> effort: no automatic or explicit request\n\n{\"summary\": \"x\"}";
        assert_eq!(
            split_disclosure(raw),
            (
                "{\"summary\": \"x\"}".to_string(),
                Some("grok/grok-4.6".to_string())
            )
        );
        let switched = "router-acp · switched codex/gpt-6 → claude/opus · usage cordon\nhello";
        assert_eq!(
            split_disclosure(switched),
            ("hello".to_string(), Some("claude/opus".to_string()))
        );
        assert_eq!(
            split_disclosure("plain answer\n> a quote"),
            ("plain answer\n> a quote".to_string(), None)
        );
    }

    #[test]
    fn the_overlay_keeps_the_prompt_as_sent() {
        let overlay = completion_overlay(std::path::Path::new("/tmp/x/completion.db"));
        assert!(overlay.contains("pre_classifier:\n  enabled: false\n"));
        assert!(overlay.contains("delegation:\n  inject_prompt: false\n"));
        assert!(overlay.ends_with("state_file: \"/tmp/x/completion.db\"\n"));
    }
}
