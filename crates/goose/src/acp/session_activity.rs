//! The active ACP session's title and busy state, as the downstream agent
//! reports them through `session_info_update` (router-acp: `_meta.routerAcp.busy` and
//! `_meta.routerAcp.paused`).
//! The CLI reads it for the terminal title and footer. Only router-reported
//! transitions start or stop the timer; output, silence and a pause do not change it.

use serde_json::Value;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Longest pause label shown.
const PAUSE_LABEL_CHARS: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionTurn {
    Running(Instant),
    Finished {
        elapsed: Duration,
        ended: chrono::DateTime<chrono::Local>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionActivity {
    pub title: Option<String>,
    pub busy: bool,
    pub turn: Option<SessionTurn>,
    /// Why the router holds the running turn (`offline`, `usage cap`, …) until it can go on.
    pub paused: Option<String>,
}

/// What one `_meta.routerAcp` object says. An absent key says nothing, so a router that does
/// not know about pauses never reports one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RouterReport {
    busy: Option<bool>,
    paused: Option<Option<String>>,
}

impl RouterReport {
    pub(crate) fn parse(router: &Value) -> Self {
        Self {
            busy: router.get("busy").and_then(Value::as_bool),
            paused: router.get("paused").and_then(|paused| match paused {
                Value::Null => Some(None),
                Value::Object(paused) => Some(Some(
                    ["label", "reason"]
                        .iter()
                        .filter_map(|key| paused.get(*key)?.as_str())
                        .map(pause_label)
                        .find(|label| !label.is_empty())
                        .unwrap_or_else(|| "paused".to_string()),
                )),
                _ => None,
            }),
        }
    }
}

/// The router's label as one short line that cannot carry terminal control sequences.
fn pause_label(label: &str) -> String {
    let shown: String = label
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !c.is_control() && !is_text_direction_or_hidden(*c))
        .collect();
    let words = shown.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut short: String = words.chars().take(PAUSE_LABEL_CHARS).collect();
    short.truncate(short.trim_end().len());
    short
}

fn is_text_direction_or_hidden(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')
}

static ACTIVITY: LazyLock<Mutex<(Option<String>, SessionActivity)>> =
    LazyLock::new(Default::default);

pub fn current() -> SessionActivity {
    ACTIVITY
        .lock()
        .map(|cached| cached.1.clone())
        .unwrap_or_default()
}

/// Whether the router holds the running turn until it can go on.
pub fn is_paused() -> bool {
    ACTIVITY
        .lock()
        .is_ok_and(|cached| cached.1.paused.is_some())
}

pub(crate) fn update(session_id: &str, title: Option<String>, report: RouterReport) {
    if let Ok(mut cached) = ACTIVITY.lock() {
        let (id, activity) = &mut *cached;
        if id.as_deref() != Some(session_id) {
            *id = Some(session_id.to_owned());
            *activity = SessionActivity::default();
        }
        activity.update(title, report, Instant::now(), chrono::Local::now());
    }
}

impl SessionActivity {
    fn update(
        &mut self,
        title: Option<String>,
        report: RouterReport,
        now: Instant,
        wall_time: chrono::DateTime<chrono::Local>,
    ) {
        if let Some(title) = title {
            self.title = Some(title);
        }
        if let Some(busy) = report.busy {
            if busy && !self.busy {
                self.turn = Some(SessionTurn::Running(now));
            } else if !busy && self.busy {
                if let Some(SessionTurn::Running(started)) = self.turn {
                    self.turn = Some(SessionTurn::Finished {
                        elapsed: now.saturating_duration_since(started),
                        ended: wall_time,
                    });
                }
            }
            self.busy = busy;
        }
        if let Some(paused) = report.paused {
            self.paused = paused;
        }
        if !self.busy {
            self.paused = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn busy(busy: bool) -> RouterReport {
        RouterReport {
            busy: Some(busy),
            paused: None,
        }
    }

    #[test]
    fn partial_updates_keep_the_other_field() {
        let mut activity = SessionActivity::default();
        let now = Instant::now();
        let wall = chrono::Local::now();
        activity.update(Some("Chordzy [opus]".into()), busy(true), now, wall);
        activity.update(None, busy(false), now, wall);
        assert_eq!(activity.title.as_deref(), Some("Chordzy [opus]"));
        assert!(!activity.busy);
        activity.update(
            Some("Chordzy [sonnet]".into()),
            RouterReport::default(),
            now,
            wall,
        );
        assert_eq!(activity.title.as_deref(), Some("Chordzy [sonnet]"));
        assert!(!activity.busy);
    }

    #[test]
    fn only_router_transitions_start_and_finish_a_turn() {
        let mut activity = SessionActivity::default();
        let start = Instant::now();
        let wall = chrono::Local::now();
        activity.update(None, busy(true), start, wall);
        let later = start + Duration::from_secs(600);
        activity.update(Some("repinned".into()), busy(true), later, wall);
        activity.update(None, RouterReport::default(), later, wall);
        assert_eq!(activity.turn, Some(SessionTurn::Running(start)));
        activity.update(None, busy(false), later, wall);
        let finished = Some(SessionTurn::Finished {
            elapsed: Duration::from_secs(600),
            ended: wall,
        });
        assert_eq!(activity.turn, finished);
        activity.update(None, busy(false), later + Duration::from_secs(60), wall);
        assert_eq!(activity.turn, finished);
        activity.update(None, busy(true), later, wall);
        assert_eq!(activity.turn, Some(SessionTurn::Running(later)));
    }

    #[test]
    fn a_pause_keeps_the_turn_and_its_timer_until_an_explicit_null_or_the_turn_ends() {
        let mut activity = SessionActivity::default();
        let start = Instant::now();
        let wall = chrono::Local::now();
        let report = |value| RouterReport::parse(&value);
        activity.update(None, report(json!({"busy": true})), start, wall);
        assert_eq!(activity.paused, None);

        let hour = start + Duration::from_secs(3600);
        activity.update(
            None,
            report(json!({"busy": true, "paused": {"reason": "offline", "label": "offline", "since": "2026-11-25T11:00:00Z"}})),
            hour,
            wall,
        );
        assert_eq!(activity.paused.as_deref(), Some("offline"));
        assert_eq!(activity.turn, Some(SessionTurn::Running(start)));
        // The title alone, and a report that does not mention the pause, say nothing about it.
        activity.update(
            Some("Chordzy [opus]".into()),
            RouterReport::default(),
            hour,
            wall,
        );
        activity.update(None, report(json!({"busy": true})), hour, wall);
        assert_eq!(activity.paused.as_deref(), Some("offline"));
        // Another reason replaces it.
        activity.update(
            None,
            report(
                json!({"busy": true, "paused": {"reason": "no_candidates", "label": "usage cap"}}),
            ),
            hour,
            wall,
        );
        assert_eq!(activity.paused.as_deref(), Some("usage cap"));

        activity.update(
            None,
            report(json!({"busy": true, "paused": null})),
            hour,
            wall,
        );
        assert_eq!(activity.paused, None);
        assert_eq!(activity.turn, Some(SessionTurn::Running(start)));

        activity.update(
            None,
            report(json!({"busy": true, "paused": {"label": "offline"}})),
            hour,
            wall,
        );
        activity.update(None, report(json!({"busy": false})), hour, wall);
        assert_eq!(activity.paused, None);
        assert_eq!(
            activity.turn,
            Some(SessionTurn::Finished {
                elapsed: Duration::from_secs(3600),
                ended: wall
            })
        );
        // An idle session is never paused.
        activity.update(
            None,
            report(json!({"paused": {"label": "offline"}})),
            hour,
            wall,
        );
        assert_eq!(activity.paused, None);
    }

    #[test]
    fn a_pause_label_is_one_short_line_whatever_the_router_sends() {
        let label = |value| RouterReport::parse(&json!({ "paused": value })).paused;
        let shown = |text: &str| Some(Some(text.to_string()));
        assert_eq!(
            label(json!({"reason": "some_new_reason", "label": "brand new reason"})),
            shown("brand new reason")
        );
        assert_eq!(
            label(json!({"label": " \u{1b}]0;x\u{7}\n\u{202e}signed \t out\u{9b} "})),
            shown("]0;x signed out")
        );
        assert_eq!(
            label(json!({"label": "\u{1b}\n", "reason": "offline"})),
            shown("offline")
        );
        assert_eq!(label(json!({})), shown("paused"));
        let Some(Some(long)) = label(json!({"label": "x".repeat(500)})) else {
            panic!("a label");
        };
        assert_eq!(long.chars().count(), PAUSE_LABEL_CHARS);
        assert_eq!(label(Value::Null), Some(None));
        assert_eq!(label(json!(true)), None);
        assert_eq!(RouterReport::parse(&json!({"busy": true})).paused, None);
    }

    #[test]
    fn another_session_does_not_inherit_the_previous_turn() {
        update(
            "first",
            None,
            RouterReport::parse(&json!({"busy": true, "paused": {"label": "offline"}})),
        );
        update("second", None, busy(false));
        assert_eq!(current(), SessionActivity::default());
    }
}
