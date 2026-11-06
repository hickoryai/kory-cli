//! One background task, full screen: its command, how long it has run, and its output as it
//! grows, following the end unless scrolled back. Opened from the pane's children list.

use super::child_sessions::{ChildRow, Key, Screen};
use console::style;
use goose::providers::base::Provider;
use std::sync::Arc;

/// Rows of a child id that names a background task rather than a child session.
pub const ROW_PREFIX: &str = "bg:";

pub fn task_id(row: &ChildRow) -> Option<&str> {
    row.id.strip_prefix(ROW_PREFIX)
}

pub struct BackgroundView {
    screen: Screen,
    provider: Arc<dyn Provider>,
    id: String,
    kind: String,
    command: String,
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    output: String,
    /// Rows scrolled back from the end; 0 follows the output.
    back: usize,
    /// Set once the task is no longer running (it ended or was stopped).
    ended: bool,
    notice: Option<String>,
}

impl BackgroundView {
    pub fn open(provider: Arc<dyn Provider>, row: &ChildRow) -> Option<Self> {
        let id = task_id(row)?.to_string();
        let mut view = Self {
            screen: Screen::enter(),
            provider,
            id,
            kind: row.model.clone(),
            command: row.label.clone(),
            started_at: row.started_at,
            output: String::new(),
            back: 0,
            ended: false,
            notice: None,
        };
        view.refresh();
        Some(view)
    }

    /// Reads the output again and draws.
    pub fn refresh(&mut self) {
        self.output = self
            .provider
            .background_output(&self.id)
            .unwrap_or_default();
        self.ended = !self
            .provider
            .background_tasks()
            .iter()
            .any(|task| task.id == self.id);
        self.draw();
    }

    /// A terminal this session hosts can be stopped here; work the agent's own tools run
    /// (a monitor, a backgrounded shell) only the agent can stop.
    fn stoppable(&self) -> bool {
        matches!(self.kind.as_str(), "background" | "terminal")
    }

    /// False when the view closes.
    pub fn key(&mut self, key: Key) -> bool {
        let (_, rows) = self.screen.size();
        let page = rows.saturating_sub(4).max(1);
        match key {
            Key::Escape | Key::Char('q') | Key::Open => return false,
            Key::Up => self.back += 1,
            Key::Down => self.back = self.back.saturating_sub(1),
            Key::PageUp => self.back += page,
            Key::PageDown => self.back = self.back.saturating_sub(page),
            Key::End => self.back = 0,
            Key::Char('s') if !self.ended && self.stoppable() => {
                self.notice = Some(if self.provider.stop_background(&self.id) {
                    "stopping…".to_string()
                } else {
                    "it is no longer running".to_string()
                });
            }
            _ => {}
        }
        self.refresh();
        true
    }

    fn draw(&mut self) {
        let (cols, rows) = self.screen.size();
        let width = cols.saturating_sub(1);
        let elapsed = self
            .started_at
            .map(|started| {
                let seconds = (chrono::Utc::now() - started).num_seconds().max(0);
                format!(" · {}m {:02}s", seconds / 60, seconds % 60)
            })
            .unwrap_or_default();
        let state = if self.ended {
            style("ended").yellow().to_string()
        } else {
            style("running").green().to_string()
        };
        let header = format!(
            "{} {}{} · {}",
            style(&self.kind).cyan().bold(),
            state,
            style(elapsed).dim(),
            self.command
        );
        let keys = if self.ended {
            "↑↓ PgUp/PgDn scroll · End follow · q back"
        } else if self.stoppable() {
            "↑↓ PgUp/PgDn scroll · End follow · s stop · q back"
        } else {
            "↑↓ PgUp/PgDn scroll · End follow · q back · the agent runs this one: ask it to stop it"
        };
        let footer = match &self.notice {
            Some(notice) => format!("{} · {}", style(notice).yellow(), style(keys).dim()),
            None => style(keys).dim().to_string(),
        };
        let body_rows = rows.saturating_sub(3);
        let lines: Vec<&str> = self.output.lines().collect();
        self.back = self.back.min(lines.len().saturating_sub(body_rows));
        let end = lines.len() - self.back;
        let start = end.saturating_sub(body_rows);
        let mut frame = vec![
            console::truncate_str(&header, width, "…").into_owned(),
            style("─".repeat(width)).dim().to_string(),
        ];
        for line in &lines[start..end] {
            frame.push(console::truncate_str(line, width, "…").into_owned());
        }
        if lines.is_empty() {
            frame.push(style("(no output yet)").dim().to_string());
        }
        while frame.len() < rows.saturating_sub(1) {
            frame.push(String::new());
        }
        frame.push(console::truncate_str(&footer, width, "…").into_owned());
        self.screen.draw(&frame);
    }
}
