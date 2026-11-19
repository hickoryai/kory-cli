//! The account bar pinned above a router session's transcript: every router-acp account's
//! status on the left, the account the session runs on now on the right, a white rule under
//! them.
//!
//! ```text
//!   ● claude x3 ■ claude: a@example.com ● codex x2 ● grok            ▶ claude: b@example.com
//!   ─────────────────────────────────────────────────────────────────────────────────────────
//! ```
//!
//! Accounts the router would route to now are counted per provider (green circle); each one
//! it would not (signed out, at a limit, cordoned) is named (red square). The data comes from
//! router-acp's sanitized `account-status` and `state-query v1 session` readers, polled off the
//! render path.

use super::relay;
use goose::providers::router_acp::{RouterAccount, RouterStateSource};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_util::task::AbortOnDropHandle;

const POLL: Duration = Duration::from_secs(10);
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const WHITE: &str = "\x1b[97m";
const DEFAULT_FG: &str = "\x1b[39m";
const UP: &str = "●";
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
                // A failed read keeps the last known state rather than blanking the bar.
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

/// The bar's two rows for a terminal `cols` wide; none before any account is known.
pub fn header_lines(accounts: &[RouterAccount], active: Option<&str>, cols: usize) -> Vec<String> {
    if accounts.is_empty() || cols < 2 {
        return Vec::new();
    }
    let left = status_text(accounts);
    let right = active
        .and_then(|id| accounts.iter().find(|account| account.id == id))
        .map(|account| format!("{GREEN}{ACTIVE}{DEFAULT_FG} {}: {}", account.provider, account.label))
        .unwrap_or_default();
    let width = cols - 1;
    let right_width = console::measure_text_width(&right);
    let line = if right_width + 2 > width {
        console::truncate_str(&left, width, "…").into_owned()
    } else {
        let room = width - right_width - 1;
        let left = console::truncate_str(&left, room, "…").into_owned();
        let gap = width - console::measure_text_width(&left) - right_width;
        format!("{left}{}{right}", " ".repeat(gap))
    };
    vec![
        format!(" {line}"),
        format!("{WHITE}{}{DEFAULT_FG}", "─".repeat(cols)),
    ]
}

/// `● claude x3 ■ claude: a@example.com ● grok`: per provider in the order first listed, its
/// routable accounts (the name alone for one, ` x<n>` for several), then each one that is not.
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
        let up = of.iter().filter(|a| a.available).count();
        match up {
            0 => {}
            1 => parts.push(format!("{GREEN}{UP}{DEFAULT_FG} {provider}")),
            _ => parts.push(format!("{GREEN}{UP}{DEFAULT_FG} {provider} x{up}")),
        }
        for down in of.iter().filter(|a| !a.available) {
            parts.push(format!("{RED}{DOWN}{DEFAULT_FG} {provider}: {}", down.label));
        }
    }
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str, label: &str, available: bool) -> RouterAccount {
        RouterAccount {
            id: id.into(),
            provider: id.split('@').next().unwrap().into(),
            label: label.into(),
            available,
        }
    }

    fn fleet() -> Vec<RouterAccount> {
        vec![
            account("claude@1", "a@x.com", true),
            account("codex@1", "c@x.com", true),
            account("claude@2", "b@x.com", false),
            account("claude@3", "d@x.com", true),
            account("grok@1", "g@x.com", true),
            account("claude@4", "e@x.com", true),
            account("codex@2", "f@x.com", true),
        ]
    }

    #[test]
    fn routable_accounts_count_per_provider_and_the_rest_are_named() {
        let text = console::strip_ansi_codes(&status_text(&fleet())).into_owned();
        assert_eq!(text, "● claude x3 ■ claude: b@x.com ● codex x2 ● grok");
        let colored = status_text(&fleet());
        assert!(colored.contains(&format!("{GREEN}{UP}")) && colored.contains(&format!("{RED}{DOWN}")));
    }

    #[test]
    fn a_provider_with_no_routable_account_shows_only_its_names() {
        let text = status_text(&[account("grok@1", "g@x.com", false)]);
        assert_eq!(console::strip_ansi_codes(&text), "■ grok: g@x.com");
    }

    #[test]
    fn the_active_account_is_right_justified_above_a_white_rule() {
        let lines = header_lines(&fleet(), Some("claude@3"), 80);
        assert_eq!(lines.len(), 2);
        let top = console::strip_ansi_codes(&lines[0]).into_owned();
        assert!(top.starts_with(" ● claude x3"), "{top}");
        assert!(top.ends_with("▶ claude: d@x.com"), "{top}");
        assert_eq!(console::measure_text_width(&lines[0]), 80);
        assert!(lines[1].starts_with(WHITE));
        assert_eq!(console::measure_text_width(&lines[1]), 80);
    }

    #[test]
    fn a_narrow_terminal_shortens_the_status_and_keeps_the_active_account() {
        let lines = header_lines(&fleet(), Some("claude@3"), 40);
        let top = console::strip_ansi_codes(&lines[0]).into_owned();
        assert!(console::measure_text_width(&top) <= 40, "{top}");
        assert!(top.ends_with("▶ claude: d@x.com") && top.contains('…'), "{top}");
    }

    #[test]
    fn nothing_is_drawn_before_any_account_is_known() {
        assert!(header_lines(&[], Some("claude@1"), 80).is_empty());
        let unknown = header_lines(&fleet(), Some("claude@9"), 80);
        assert!(!console::strip_ansi_codes(&unknown[0]).contains('▶'));
    }
}
