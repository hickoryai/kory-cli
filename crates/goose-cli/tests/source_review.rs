//! `/review` against a stub Kory Source server. The stub checks the exact requests the CLI
//! builds, hands out `pending_work`, and records the answers; a fake seat stands in for
//! router-acp so the round-trip runs without a model.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::future::BoxFuture;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use goose_cli::source_review::client::{Client, Settings, SettingsError};
use goose_cli::source_review::seat::{Completion, Failure, SeatCompleter};
use goose_cli::source_review::target::{PrHeads, Target};
use goose_cli::source_review::{render_outcome, Cadence, Outcome, Review};

const TOKEN: &str = "kst_test_token";
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const MOVED_HEAD: &str = "fedcba9876543210fedcba9876543210fedcba98";

#[derive(Clone, Copy)]
enum Ending {
    Succeeds,
    FailsWith(&'static str),
}

struct Stub {
    base: String,
    submit_status: StatusCode,
    submit_error: Option<Value>,
    pending_item: bool,
    ending: Ending,
    answer_failures: usize,
    submits: Vec<(Option<String>, Value)>,
    answers: Vec<(String, Option<String>, Value)>,
    answered: bool,
}

type Shared = Arc<Mutex<Stub>>;

fn auth(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
}

fn job(stub: &Stub) -> Value {
    let base = &stub.base;
    let mut view = json!({
        "id": "rev_1",
        "state": "running",
        "stage": "reviewing",
        "repo": "hickoryai/goose",
        "pr_number": 123,
        "requested_head_sha": HEAD,
        "reviewed_head_sha": null,
        "deadline_at": null,
        "findings": null,
        "models": {"requested": [], "actual": []},
        "error": null,
        "pending_work": [],
        "status_url": format!("{base}/api/reviews/rev_1"),
        "report_url": format!("{base}/api/reviews/rev_1/report"),
    });
    if !stub.answered {
        if stub.pending_item {
            view["pending_work"] = json!([{
                "work_id": "wrk_1",
                "prompt": "Review this diff.",
                "model": "auto",
                "timeout_seconds": 900,
            }]);
        }
        return view;
    }
    view["models"] = json!({"requested": ["auto"], "actual": ["grok/grok-4.6"]});
    match stub.ending {
        Ending::Succeeds => {
            view["state"] = json!("succeeded");
            view["stage"] = json!("done");
            view["reviewed_head_sha"] = json!(HEAD);
            view["summary"] = json!("Adds a divide helper.");
            view["coverage"] = json!({
                "changed_files": 2, "reviewed_files": 1, "reviewed_paths": ["src/app.py"],
                "skipped": [{"path": "assets/logo.png", "reason": "patch_unavailable"}],
                "note": "",
            });
            view["findings"] = json!([{
                "id": "f1", "path": "src/app.py", "line": 12, "end_line": null,
                "severity": "blocker", "category": "bug", "title": "Division by zero",
                "body": "`b` can be 0.", "confidence": 0.95, "suggestion": null,
                "source_pass": "main", "registry_ids": ["REG-7", "DEC-12"],
            }]);
        }
        Ending::FailsWith(code) => {
            view["state"] = json!("failed");
            view["stage"] = json!("done");
            view["error"] = json!({"code": code, "message": format!("stub {code}")});
        }
    }
    view
}

async fn submit(
    State(stub): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let mut stub = stub.lock().unwrap();
    stub.submits.push((auth(&headers), body));
    if let Some(error) = stub.submit_error.clone() {
        return (stub.submit_status, Json(error));
    }
    let mut view = job(&stub);
    view["state"] = json!("queued");
    view["pending_work"] = json!([]);
    (StatusCode::ACCEPTED, Json(view))
}

async fn status(State(stub): State<Shared>, UrlPath(id): UrlPath<String>) -> Json<Value> {
    assert_eq!(id, "rev_1");
    let mut stub = stub.lock().unwrap();
    // A job with no work to hand out finishes on its own, as a server-side timeout would.
    if !stub.pending_item {
        stub.answered = true;
    }
    Json(job(&stub))
}

async fn answer(
    State(stub): State<Shared>,
    UrlPath((id, work_id)): UrlPath<(String, String)>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    assert_eq!(id, "rev_1");
    let mut stub = stub.lock().unwrap();
    stub.answers.push((work_id.clone(), auth(&headers), body));
    if stub.answer_failures > 0 {
        stub.answer_failures -= 1;
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "busy"})),
        );
    }
    stub.answered = true;
    (
        StatusCode::OK,
        Json(json!({"work_id": work_id, "state": "answered"})),
    )
}

impl Stub {
    async fn start(configure: impl FnOnce(&mut Stub)) -> Shared {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut stub = Stub {
            base: format!("http://{}", listener.local_addr().unwrap()),
            submit_status: StatusCode::ACCEPTED,
            submit_error: None,
            pending_item: true,
            ending: Ending::Succeeds,
            answer_failures: 0,
            submits: vec![],
            answers: vec![],
            answered: false,
        };
        configure(&mut stub);
        let shared = Arc::new(Mutex::new(stub));
        let app = Router::new()
            .route("/api/reviews", post(submit))
            .route("/api/reviews/{id}", get(status))
            .route("/api/reviews/{id}/work/{work_id}", post(answer))
            .with_state(shared.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        shared
    }
}

struct FixedHead;

impl PrHeads for FixedHead {
    fn head_sha<'a>(&'a self, target: &'a Target) -> BoxFuture<'a, anyhow::Result<String>> {
        assert_eq!(target.to_string(), "hickoryai/goose#123");
        Box::pin(async { Ok(HEAD.to_string()) })
    }
}

type Calls = Arc<Mutex<Vec<(String, String, Duration)>>>;

/// A seat that answers every prompt with `reply`, or never answers when it is `None`.
struct FakeSeat {
    available: bool,
    reply: Option<Completion>,
    calls: Calls,
}

impl FakeSeat {
    fn answering(reply: Option<Completion>) -> (Arc<Self>, Calls) {
        let calls = Calls::default();
        let seat = Arc::new(Self {
            available: true,
            reply,
            calls: calls.clone(),
        });
        (seat, calls)
    }
}

impl SeatCompleter for FakeSeat {
    fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.available, "router-acp: not found in PATH");
        Ok(())
    }

    fn complete(
        &self,
        prompt: String,
        model: String,
        timeout: Duration,
    ) -> BoxFuture<'static, Completion> {
        self.calls.lock().unwrap().push((prompt, model, timeout));
        let reply = self.reply.clone();
        Box::pin(async move {
            match reply {
                Some(reply) => reply,
                None => futures::future::pending().await,
            }
        })
    }
}

fn grok_text() -> Option<Completion> {
    Some(Completion::Text {
        text: "{\"findings\": []}".into(),
        actual_model: Some("grok/grok-4.6".into()),
    })
}

fn checkout() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(args)
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    git(&[
        "remote",
        "add",
        "upstream",
        "https://github.com/aaif-goose/goose.git",
    ]);
    git(&[
        "remote",
        "add",
        "origin",
        "git@github.com:hickoryai/goose.git",
    ]);
    dir
}

async fn review(
    base: &str,
    seat: Arc<FakeSeat>,
    argument: &str,
    cwd: &Path,
    cancel: &CancellationToken,
) -> (Outcome, String) {
    let client = Client::new(Settings::new(base, TOKEN.to_string()).unwrap()).unwrap();
    let review = Review::new(client, Arc::new(FixedHead), seat).with_cadence(Cadence {
        running: Duration::from_millis(20),
        queued: Duration::from_millis(20),
    });
    let mut out = Vec::new();
    let outcome = review.run(argument, cwd, cancel, &mut out).await;
    render_outcome(&outcome, &mut out);
    (outcome, String::from_utf8(out).unwrap())
}

#[tokio::test]
async fn a_review_answers_its_work_on_the_seat_and_shows_the_result() {
    let stub = Stub::start(|s| s.answer_failures = 1).await;
    let base = stub.lock().unwrap().base.clone();
    let (seat, calls) = FakeSeat::answering(grok_text());
    let repo = checkout();

    let (outcome, out) = review(&base, seat, "#123", repo.path(), &CancellationToken::new()).await;

    let stub = stub.lock().unwrap();
    assert_eq!(
        stub.submits,
        vec![(
            Some(format!("Bearer {TOKEN}")),
            json!({"repo": "hickoryai/goose", "pr_number": 123, "head_sha": HEAD}),
        )]
    );
    // One model run; the first post hits a 503 and the saved body is sent again.
    assert_eq!(
        *calls.lock().unwrap(),
        vec![(
            "Review this diff.".to_string(),
            "auto".to_string(),
            Duration::from_secs(900)
        )]
    );
    let posted = json!({"text": "{\"findings\": []}", "actual_model": "grok/grok-4.6"});
    let bearer = Some(format!("Bearer {TOKEN}"));
    assert_eq!(
        stub.answers,
        vec![
            ("wrk_1".to_string(), bearer.clone(), posted.clone()),
            ("wrk_1".to_string(), bearer, posted),
        ]
    );

    let Outcome::Finished(job) = outcome else {
        panic!("unexpected outcome: {outcome:?}\n{out}");
    };
    assert_eq!(job.models.requested, vec!["auto"]);
    assert_eq!(job.models.actual, vec!["grok/grok-4.6"]);
    for expected in [
        "Kory Source review hickoryai/goose#123 at 0123456789ab",
        "running · reviewing · running model call 1",
        &format!("Reviewed hickoryai/goose#123 at {HEAD}"),
        "[blocker] src/app.py:12 — Division by zero (bug)",
        "registry: REG-7, DEC-12",
        "Coverage: 1 of 2 changed files reviewed",
        "skipped assets/logo.png (patch_unavailable)",
        "Models: requested auto · actual grok/grok-4.6",
        &format!("Report: {base}/api/reviews/rev_1/report"),
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }
}

#[tokio::test]
async fn a_full_seat_posts_seat_exhausted_and_shows_it() {
    let stub = Stub::start(|s| s.ending = Ending::FailsWith("seat_exhausted")).await;
    let base = stub.lock().unwrap().base.clone();
    let (seat, _) = FakeSeat::answering(Some(Completion::Failed {
        failure: Failure::SeatExhausted,
        actual_model: None,
        detail: "every candidate is cordoned".into(),
    }));

    let (_, out) = review(
        &base,
        seat,
        "hickoryai/goose#123",
        Path::new("/"),
        &CancellationToken::new(),
    )
    .await;

    assert_eq!(
        stub.lock().unwrap().answers[0].2,
        json!({"error": "seat_exhausted"})
    );
    assert!(
        out.contains("seat_exhausted: your model seats were full"),
        "{out}"
    );
}

#[tokio::test]
async fn an_unanswered_call_shows_model_timeout() {
    let stub = Stub::start(|s| {
        s.pending_item = false;
        s.ending = Ending::FailsWith("model_timeout");
    })
    .await;
    let base = stub.lock().unwrap().base.clone();
    let (seat, _) = FakeSeat::answering(grok_text());

    let (_, out) = review(
        &base,
        seat,
        "hickoryai/goose#123",
        Path::new("/"),
        &CancellationToken::new(),
    )
    .await;

    assert!(
        out.contains("model_timeout: a review model call got no answer"),
        "{out}"
    );
    assert!(
        out.contains("Models: requested auto · actual grok/grok-4.6"),
        "{out}"
    );
}

#[tokio::test]
async fn a_moved_head_shows_head_sha_mismatch() {
    let stub = Stub::start(|s| {
        s.submit_status = StatusCode::CONFLICT;
        s.submit_error = Some(json!({"error": {
            "code": "head_sha_mismatch", "message": "moved", "current_head_sha": MOVED_HEAD,
        }}));
    })
    .await;
    let base = stub.lock().unwrap().base.clone();
    let (seat, calls) = FakeSeat::answering(grok_text());

    let (outcome, out) = review(
        &base,
        seat,
        "hickoryai/goose#123",
        Path::new("/"),
        &CancellationToken::new(),
    )
    .await;

    assert!(matches!(outcome, Outcome::Rejected { .. }), "{outcome:?}");
    assert!(out.contains("head_sha_mismatch"), "{out}");
    assert!(
        out.contains("Requested 0123456789ab, but the PR head is fedcba987654"),
        "{out}"
    );
    assert!(calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_refused_token_shows_an_auth_failure() {
    let stub = Stub::start(|s| {
        s.submit_status = StatusCode::UNAUTHORIZED;
        s.submit_error = Some(json!({"error": "unauthorized"}));
    })
    .await;
    let base = stub.lock().unwrap().base.clone();
    let (seat, _) = FakeSeat::answering(grok_text());

    let (outcome, out) = review(
        &base,
        seat,
        "hickoryai/goose#123",
        Path::new("/"),
        &CancellationToken::new(),
    )
    .await;

    assert!(matches!(outcome, Outcome::AuthFailed(_)), "{outcome:?}");
    assert!(out.contains("auth failure"), "{out}");
    assert!(out.contains("HTTP 401: unauthorized"), "{out}");
}

#[tokio::test]
async fn a_service_that_is_down_shows_unreachable() {
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let (seat, _) = FakeSeat::answering(grok_text());

    let (outcome, out) = review(
        &format!("http://{closed}"),
        seat,
        "hickoryai/goose#123",
        Path::new("/"),
        &CancellationToken::new(),
    )
    .await;

    assert!(
        matches!(outcome, Outcome::ServiceUnreachable(_)),
        "{outcome:?}"
    );
    assert!(out.contains("Kory Source service unreachable"), "{out}");
}

#[tokio::test]
async fn a_workstation_without_router_acp_submits_nothing() {
    let stub = Stub::start(|_| {}).await;
    let base = stub.lock().unwrap().base.clone();
    let seat = Arc::new(FakeSeat {
        available: false,
        reply: None,
        calls: Calls::default(),
    });

    let (_, out) = review(
        &base,
        seat,
        "hickoryai/goose#123",
        Path::new("/"),
        &CancellationToken::new(),
    )
    .await;

    assert!(stub.lock().unwrap().submits.is_empty());
    assert!(out.contains("Workstation unreachable"), "{out}");
    assert!(out.contains("router-acp: not found in PATH"), "{out}");
}

#[tokio::test]
async fn cancel_stops_the_running_call_and_posts_nothing() {
    let stub = Stub::start(|_| {}).await;
    let base = stub.lock().unwrap().base.clone();
    let (seat, calls) = FakeSeat::answering(None);
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let started = calls.clone();
    tokio::spawn(async move {
        while started.lock().unwrap().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        trigger.cancel();
    });

    let (outcome, out) = review(&base, seat, "hickoryai/goose#123", Path::new("/"), &cancel).await;

    assert_eq!(
        outcome,
        Outcome::Cancelled {
            job_id: Some("rev_1".into())
        }
    );
    assert!(stub.lock().unwrap().answers.is_empty());
    assert!(out.contains("Review cancelled."), "{out}");
}

#[test]
fn missing_settings_show_not_configured() {
    let _env = env_lock::lock_env([
        ("KORY_SOURCE_URL", None::<&str>),
        ("KORY_SOURCE_REVIEW_TOKEN", None),
        ("GOOSE_DISABLE_KEYRING", Some("1")),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let config =
        goose::config::Config::new(dir.path().join("config.yaml"), "kory-review-test").unwrap();
    let error = Settings::from_config(&config).unwrap_err();
    assert_eq!(
        error,
        SettingsError::Missing {
            missing: "KORY_SOURCE_URL and KORY_SOURCE_REVIEW_TOKEN".into()
        }
    );

    let mut out = Vec::new();
    render_outcome(&Outcome::NotConfigured(error.to_string()), &mut out);
    let out = String::from_utf8(out).unwrap();
    assert!(
        out.contains("Kory Source review is not configured."),
        "{out}"
    );
    assert!(
        out.contains("KORY_SOURCE_URL and KORY_SOURCE_REVIEW_TOKEN not set"),
        "{out}"
    );
}
