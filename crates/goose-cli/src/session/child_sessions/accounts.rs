//! The account bar pinned above a router session's transcript: every router-acp account's
//! status on the left, the account the session runs on now on the right, a white rule under
//! them.
//!
//! ```text
//!   ● claude x2 ⏸ claude: w@example.com (11-25) ■ claude: b@example.com ● grok  ▶ claude: a@x
//!   ─────────────────────────────────────────────────────────────────────────────────────────
//! ```
//!
//! Three states, each in its own true color (independent of the terminal's palette):
//! available accounts are counted per provider (green ●, `● grok` for one, `● claude x3` for
//! several); a signed-in account the router holds back for a limit is named with its reset
//! (amber ⏸); a signed-out account is named (red ■). The data comes from router-acp's
//! sanitized `account-status` and `state-query v1 session` readers, polled off the render path.

use super::relay;
use goose::providers::router_acp::{AccountState, RouterAccount, RouterStateSource};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::task::AbortOnDropHandle;

const POLL: Duration = Duration::from_secs(10);
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const GREEN: &str = "\x1b[38;2;63;185;80m";
const AMBER: &str = "\x1b[38;2;210;153;34m";
const RED: &str = "\x1b[38;2;229;72;77m";
const WHITE: &str = "\x1b[97m";
const DEFAULT_FG: &str = "\x1b[39m";
const UP: &str = "●";
const LIMITED: &str = "⏸";
const DOWN: &str = "■";
const ACTIVE: &str = "▶";

#[derive(Default)]
struct Snapshot {
    accounts: Vec<RouterAccount>,
    /// The router agent (`claude@<id>`) the session is routed to.
    active: Option<String>,
}

/// Polls the accounts while alive and draws them as the relay's header.
pub struct AccountBar {
    _poller: AbortOnDropHandle<()>,
}

impl AccountBar {
    pub fn start(source: RouterStateSource, session_id: String) -> Self {
        let latest = Arc::new(Mutex::new(Snapshot::default()));
        let painted = latest.clone();
        relay::set_header(Some(Arc::new(move |cols, _lines| {
            let snapshot = painted.lock().unwrap();
            header_lines(&snapshot.accounts, snapshot.active.as_deref(), cols)
        })));
        let poller = tokio::spawn(async move {
            loop {
                // Both readers run on every refresh, so a sign-in, sign-out or limit anywhere
                // shows within one interval. A failed read keeps the last known state rather
                // than blanking the bar.
                if let Ok(accounts) = source.accounts(QUERY_TIMEOUT).await {
                    latest.lock().unwrap().accounts = accounts;
                }
                if let Ok(active) = source.session_agent(&session_id, QUERY_TIMEOUT).await {
                    latest.lock().unwrap().active = active;
                }
                relay::repaint();
                tokio::time::sleep(POLL).await;
            }
        });
        Self {
            _poller: AbortOnDropHandle::new(poller),
        }
    }
}

impl Drop for AccountBar {
    fn drop(&mut self) {
        relay::set_header(None);
    }
}

/// The bar's two rows for a terminal `cols` wide; none before any account is known. Like the
/// pane's rows, neither reaches the last column, so the terminal never wraps one.
pub fn header_lines(accounts: &[RouterAccount], active: Option<&str>, cols: usize) -> Vec<String> {
    if accounts.is_empty() || cols < 4 {
        return Vec::new();
    }
    // One leading space and the last column kept free.
    let width = cols - 2;
    let left = status_text(accounts);
    let right = active
        .and_then(|id| accounts.iter().find(|account| account.id == id))
        .map(|account| format!("{GREEN}{ACTIVE}{DEFAULT_FG} {}: {}", account.provider, account.label))
        .unwrap_or_default();
    let right_width = console::measure_text_width(&right);
    let line = if right.is_empty() || right_width + 2 > width {
        console::truncate_str(&left, width, "…").into_owned()
    } else {
        let left = console::truncate_str(&left, width - right_width - 1, "…").into_owned();
        let gap = width - console::measure_text_width(&left) - right_width;
        format!("{left}{}{right}", " ".repeat(gap))
    };
    vec![
        format!(" {line}"),
        format!("{WHITE}{}{DEFAULT_FG}", "─".repeat(cols - 1)),
    ]
}

/// Per provider in the order first listed: its available accounts (`● grok`, `● claude x3`),
/// then each limited one with its reset day, then each signed-out one.
fn status_text(accounts: &[RouterAccount]) -> String {
    let mut providers: Vec<&str> = Vec::new();
    for account in accounts {
        if !providers.contains(&account.provider.as_str()) {
            providers.push(&account.provider);
        }
    }
    let mut parts = Vec::new();
    for provider in providers {
        let of: Vec<&RouterAccount> = accounts.iter().filter(|a| a.provider == provider).collect();
        let up = of.iter().filter(|a| a.state == AccountState::Available).count();
        match up {
            0 => {}
            1 => parts.push(format!("{GREEN}{UP}{DEFAULT_FG} {provider}")),
            _ => parts.push(format!("{GREEN}{UP}{DEFAULT_FG} {provider} x{up}")),
        }
        for account in &of {
            if let AccountState::Limited { resets_at } = &account.state {
                let reset = resets_at
                    .as_deref()
                    .and_then(reset_day)
                    .map(|day| format!(" ({day})"))
                    .unwrap_or_default();
                parts.push(format!("{AMBER}{LIMITED}{DEFAULT_FG} {provider}: {}{reset}", account.label));
            }
        }
        for account in of.iter().filter(|a| a.state == AccountState::SignedOut) {
            parts.push(format!("{RED}{DOWN}{DEFAULT_FG} {provider}: {}", account.label));
        }
    }
    parts.join(" ")
}

/// `2026-11-25T04:00:00Z` → `11-25`, in local time.
fn reset_day(resets_at: &str) -> Option<String> {
    let at = chrono::DateTime::parse_from_rfc3339(resets_at).ok()?;
    Some(at.with_timezone(&chrono::Local).format("%m-%d").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, label: &str, state: AccountState) -> RouterAccount {
        RouterAccount {
            id: id.into(),
            provider: id.split('@').next().unwrap().into(),
            label: label.into(),
            state,
        }
    }

    fn fleet() -> Vec<RouterAccount> {
        let limited = AccountState::Limited {
            resets_at: Some("2026-11-25T12:00:00+00:00".into()),
        };
        vec![
            account("claude@1", "a@x.com", AccountState::Available),
            account("codex@1", "c@x.com", AccountState::Available),
            account("claude@2", "b@x.com", AccountState::SignedOut),
            account("claude@3", "d@x.com", AccountState::Available),
            account("grok@1", "g@x.com", AccountState::Available),
            account("claude@4", "e@x.com", AccountState::Available),
            account("codex@2", "f@x.com", AccountState::Available),
            account("claude@5", "w@x.com", limited),
        ]
    }

    #[test]
    fn available_accounts_count_and_limited_and_signed_out_ones_are_named() {
        let text = console::strip_ansi_codes(&status_text(&fleet())).into_owned();
        let day = reset_day("2026-11-25T12:00:00+00:00").unwrap();
        assert_eq!(
            text,
            format!("● claude x3 ⏸ claude: w@x.com ({day}) ■ claude: b@x.com ● codex x2 ● grok")
        );
        let colored = status_text(&fleet());
        assert!(colored.contains(&format!("{GREEN}{UP}")));
        assert!(colored.contains(&format!("{AMBER}{LIMITED}")));
        assert!(colored.contains(&format!("{RED}{DOWN}")));
        // True color, never the terminal palette's green/yellow/red.
        for code in [GREEN, AMBER, RED] {
            assert!(code.starts_with("\x1b[38;2;"), "{code:?}");
        }
    }

    #[test]
    fn a_signed_in_account_at_its_limit_is_amber_not_red() {
        let limited = account(
            "claude@1",
            "w@x.com",
            AccountState::Limited { resets_at: None },
        );
        let text = status_text(&[limited]);
        assert!(text.starts_with(AMBER) && !text.contains(RED), "{text:?}");
        assert_eq!(console::strip_ansi_codes(&text), "⏸ claude: w@x.com");
    }

    #[test]
    fn a_provider_with_no_available_account_shows_only_its_names() {
        let text = status_text(&[account("grok@1", "g@x.com", AccountState::SignedOut)]);
        assert_eq!(console::strip_ansi_codes(&text), "■ grok: g@x.com");
    }

    #[test]
    fn the_active_account_is_right_justified_above_a_white_rule() {
        let lines = header_lines(&fleet(), Some("claude@3"), 120);
        assert_eq!(lines.len(), 2, "one text row and the rule, nothing more");
        let top = console::strip_ansi_codes(&lines[0]).into_owned();
        assert!(top.starts_with(" ● claude x3"), "{top}");
        assert!(top.ends_with("▶ claude: d@x.com"), "{top}");
        assert!(lines[0].contains(&format!("{GREEN}{ACTIVE}")));
        // Neither row reaches the last column, so the terminal never wraps it.
        assert_eq!(console::measure_text_width(&lines[0]), 119);
        assert!(lines[1].starts_with(WHITE));
        assert_eq!(console::measure_text_width(&lines[1]), 119);
    }

    #[test]
    fn a_narrow_terminal_shortens_the_status_and_keeps_the_active_account() {
        let lines = header_lines(&fleet(), Some("claude@3"), 40);
        let top = console::strip_ansi_codes(&lines[0]).into_owned();
        assert!(console::measure_text_width(&top) < 40, "{top}");
        assert!(top.ends_with("▶ claude: d@x.com") && top.contains('…'), "{top}");
    }

    #[test]
    fn an_unrouted_session_shows_nothing_on_the_right() {
        assert!(header_lines(&[], Some("claude@1"), 80).is_empty());
        for active in [None, Some("claude@9")] {
            let lines = header_lines(&fleet(), active, 80);
            let top = console::strip_ansi_codes(&lines[0]).into_owned();
            assert!(!top.contains('▶'), "{top}");
            assert!(console::measure_text_width(&top) < 80, "{top}");
        }
    }
}
