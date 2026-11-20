//! The active ACP session's title and busy state, as the downstream agent
//! reports them through `session_info_update` (router-acp: `_meta.routerAcp.busy`).
//! The CLI reads it for the terminal title and footer. Only router-reported
//! transitions start or stop the timer; output and silence do not change it.

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

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
}

static ACTIVITY: LazyLock<Mutex<(Option<String>, SessionActivity)>> =
    LazyLock::new(Default::default);

pub fn current() -> SessionActivity {
    ACTIVITY
        .lock()
        .map(|cached| cached.1.clone())
        .unwrap_or_default()
}

pub(crate) fn update(session_id: &str, title: Option<String>, busy: Option<bool>) {
    if let Ok(mut cached) = ACTIVITY.lock() {
        let (id, activity) = &mut *cached;
        if id.as_deref() != Some(session_id) {
            *id = Some(session_id.to_owned());
            *activity = SessionActivity::default();
        }
        activity.update(title, busy, Instant::now(), chrono::Local::now());
    }
}

impl SessionActivity {
    fn update(
        &mut self,
        title: Option<String>,
        busy: Option<bool>,
        now: Instant,
        wall_time: chrono::DateTime<chrono::Local>,
    ) {
        if let Some(title) = title {
            self.title = Some(title);
        }
        if let Some(busy) = busy {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_updates_keep_the_other_field() {
        let mut activity = SessionActivity::default();
        let now = Instant::now();
        let wall = chrono::Local::now();
        activity.update(Some("Chordzy [opus]".into()), Some(true), now, wall);
        activity.update(None, Some(false), now, wall);
        assert_eq!(activity.title.as_deref(), Some("Chordzy [opus]"));
        assert!(!activity.busy);
        activity.update(Some("Chordzy [sonnet]".into()), None, now, wall);
        assert_eq!(activity.title.as_deref(), Some("Chordzy [sonnet]"));
        assert!(!activity.busy);
    }

    #[test]
    fn only_router_transitions_start_and_finish_a_turn() {
        let mut activity = SessionActivity::default();
        let start = Instant::now();
        let wall = chrono::Local::now();
        activity.update(None, Some(true), start, wall);
        let later = start + Duration::from_secs(600);
        activity.update(Some("repinned".into()), Some(true), later, wall);
        activity.update(None, None, later, wall);
        assert_eq!(activity.turn, Some(SessionTurn::Running(start)));
        activity.update(None, Some(false), later, wall);
        let finished = Some(SessionTurn::Finished {
            elapsed: Duration::from_secs(600),
            ended: wall,
        });
        assert_eq!(activity.turn, finished);
        activity.update(None, Some(false), later + Duration::from_secs(60), wall);
        assert_eq!(activity.turn, finished);
        activity.update(None, Some(true), later, wall);
        assert_eq!(activity.turn, Some(SessionTurn::Running(later)));
    }

    #[test]
    fn another_session_does_not_inherit_the_previous_turn() {
        update("first", None, Some(true));
        update("second", None, Some(false));
        assert_eq!(current(), SessionActivity::default());
    }
}
