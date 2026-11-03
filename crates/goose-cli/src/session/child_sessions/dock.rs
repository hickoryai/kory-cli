//! The pane under the session's output, laid out like Claude Code's footer:
//!
//! ```text
//!   ✻ working 1m 12s · 1 queued                    ━━━━━━━━╌╌╌╌╌╌╌╌╌╌╌╌ 40% 102k/258k
//!   ─────────────────────────────────────────────────────────────────────────────────────
//!   › the message being written
//!   ─────────────────────────────────────────────────────────────────────────────────────
//!   12 running                         Enter queues · Ctrl+Enter interrupts · ↓ children
//!     opus[1m] [6m] jam-packet-map  01a19fae  → Read file …
//! ```
//!
//! A status and context line, the input box between two rules, and the session's running
//! children. The relay (`relay.rs`) draws it below the output and takes it off the screen
//! before every write, so none of it ever enters the terminal's history; nothing here
//! positions the cursor or reserves rows. The pane reads no input itself: the session feeds
//! it keys, at the prompt and while a turn runs alike, so what is typed during a turn is still
//! there when the turn ends.
//!
//! During a turn Enter queues the message; the session hands queued messages to the running
//! turn one at a time (`next_steer`, `finish_steer`) and sends whatever is left when the turn
//! ends. Up on an empty input line takes the newest undelivered message back for editing;
//! Down moves into the children.

use super::relay;
use super::router;
use super::view::{self, ChildRow, ChildStatus};
use super::Key;
use console::style;
use goose::providers::router_acp::RouterStateSource;
use goose::session::SessionManager;
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};
use tokio_util::task::AbortOnDropHandle;
use unicode_width::UnicodeWidthChar;

const REPAINT: Duration = Duration::from_secs(1);
const QUERY_TIMEOUT: Duration = Duration::from_secs(5);
/// Child rows the pane shows; more collapse into a "+N more" row.
const MAX_ROWS: usize = 6;
/// Rows a long message may wrap onto; a longer one shows the rows around the caret.
const MAX_INPUT_ROWS: usize = 6;
/// The input box's "› " gutter.
const GUTTER: usize = 2;

/// The live pane, so a full-screen view or child process can hand the screen back and forth.
static ACTIVE: OnceLock<Mutex<Weak<Shared>>> = OnceLock::new();

/// The message being written.
#[derive(Default)]
struct InputLine {
    text: Vec<char>,
    cursor: usize,
    /// A bracketed paste in progress: its text goes in whole when it ends.
    paste: Option<String>,
}

/// What a key typed into the input box did.
#[derive(Debug, PartialEq, Eq)]
pub enum Typed {
    Edited,
    /// Enter during a turn: the message is queued for it, without interrupting it.
    Queued,
    /// Ctrl+Enter during a turn: end the turn now and send what is queued.
    SendNow,
    /// Enter at the prompt: the message to send.
    Submitted(String),
    Ignored,
}

#[derive(Default)]
struct State {
    rows: Vec<ChildRow>,
    /// Router sessions list their children (or that none run) at all times.
    always_list: bool,
    selected: Option<usize>,
    /// The first child row shown; follows the selection through a longer list.
    offset: usize,
    input: InputLine,
    /// Messages not yet delivered, oldest first.
    queued: Vec<String>,
    /// The queued message being handed to the running turn; counted as queued until then.
    steering: Option<String>,
    /// The terminal size the pane was last painted for.
    size: (usize, usize),
    focus_requested: bool,
    /// When the running turn started; `None` at the prompt.
    turn_started: Option<Instant>,
    /// A session-wide state the status line leads with, e.g. `shutting down 1/3`.
    status: Option<String>,
    /// Tokens in the context and the model's limit.
    context: Option<(usize, usize)>,
    /// A one-time notice under the input box, cleared by the next key (e.g. "Ctrl-C again
    /// to exit").
    notice: Option<String>,
    /// A question is open in the transcript above and takes the keys, not the input box.
    question: bool,
}

struct Shared {
    state: Mutex<State>,
}

pub struct Dock {
    shared: Arc<Shared>,
    _task: AbortOnDropHandle<()>,
}

/// Where the pane's children come from: goose's own subagents and router-acp's delegates.
pub struct Sources {
    pub session_manager: Arc<SessionManager>,
    pub session_id: String,
    pub router: Option<(RouterStateSource, String)>,
}

impl Dock {
    /// The pane, drawn through the relay; `None` when the terminal cannot host it.
    pub fn start(sources: Sources) -> Option<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                always_list: sources.router.is_some(),
                size: terminal_size(),
                ..State::default()
            }),
        });
        let weak = Arc::downgrade(&shared);
        let painter: relay::Painter = Arc::new(move |cols, lines| match weak.upgrade() {
            Some(shared) => {
                let mut state = shared.state.lock().unwrap();
                state.size = (cols, lines);
                follow_selection(&mut state);
                pane(&state, cols, lines)
            }
            None => Vec::new(),
        });
        if !relay::install(painter) {
            return None;
        }
        write_raw("\x1b[?2004h");
        *ACTIVE.get_or_init(Mutex::default).lock().unwrap() = Arc::downgrade(&shared);
        let painter = shared.clone();
        let task = tokio::spawn(async move {
            loop {
                let turn_active = painter.state.lock().unwrap().turn_started.is_some();
                painter.update(load_rows(&sources, turn_active).await);
                tokio::time::sleep(REPAINT).await;
            }
        });
        Some(Self {
            shared,
            _task: AbortOnDropHandle::new(task),
        })
    }

    /// A turn starts: Enter now queues instead of sending, and the status line counts time.
    pub fn begin_turn(&self) {
        self.shared.state.lock().unwrap().turn_started = Some(Instant::now());
        relay::repaint();
    }

    /// Back to the prompt. What was typed and never sent stays in the input box.
    pub fn end_turn(&self) {
        self.shared.state.lock().unwrap().turn_started = None;
        relay::repaint();
    }

    pub fn set_context(&self, context: Option<(usize, usize)>) {
        self.shared.state.lock().unwrap().context = context;
        relay::repaint();
    }

    /// A line under the input box until the next key.
    pub fn set_notice(&self, notice: Option<String>) {
        self.shared.state.lock().unwrap().notice = notice;
        relay::repaint();
    }

    /// Messages queued during the last turn and never delivered, joined into the next one to
    /// send.
    pub fn take_queued(&self) -> Option<String> {
        let queued = {
            let mut state = self.shared.state.lock().unwrap();
            let mut queued: Vec<String> = state.steering.take().into_iter().collect();
            queued.append(&mut state.queued);
            queued
        };
        relay::repaint();
        (!queued.is_empty()).then(|| queued.join("\n\n"))
    }

    /// The oldest queued message, to hand to the running turn; none while one is on its way.
    pub fn next_steer(&self) -> Option<String> {
        let mut state = self.shared.state.lock().unwrap();
        if state.steering.is_some() || state.queued.is_empty() {
            return None;
        }
        let text = state.queued.remove(0);
        state.steering = Some(text.clone());
        Some(text)
    }

    /// The message from `next_steer` reached the turn, or goes back to the front of the queue.
    pub fn finish_steer(&self, delivered: bool) {
        {
            let mut state = self.shared.state.lock().unwrap();
            if let Some(text) = state.steering.take().filter(|_| !delivered) {
                state.queued.insert(0, text);
            }
        }
        relay::repaint();
    }

    /// Messages that left the queue but never reached the turn go back to its front.
    pub fn requeue(&self, texts: Vec<String>) {
        if texts.is_empty() {
            return;
        }
        self.shared.state.lock().unwrap().queued.splice(0..0, texts);
        relay::repaint();
    }

    pub fn is_steering(&self) -> bool {
        self.shared.state.lock().unwrap().steering.is_some()
    }

    pub fn has_queued(&self) -> bool {
        !self.shared.state.lock().unwrap().queued.is_empty()
    }

    /// A key for the input box (the caller handles the children's keys first).
    pub fn type_key(&self, key: Key) -> Typed {
        let typed = {
            let mut state = self.shared.state.lock().unwrap();
            state.notice = None;
            let turn = state.turn_started.is_some();
            let state = &mut *state;
            let input = &mut state.input;
            if let Some(paste) = input.paste.as_mut() {
                match key {
                    Key::Char(c) => paste.push(c),
                    Key::Newline | Key::Enter => paste.push('\n'),
                    Key::PasteEnd => {
                        let text = input.paste.take().unwrap_or_default();
                        input.insert(&text);
                    }
                    _ => {}
                }
                Typed::Edited
            } else {
                match key {
                    Key::Char(c) => {
                        input.insert(&c.to_string());
                        Typed::Edited
                    }
                    Key::Newline => {
                        input.insert("\n");
                        Typed::Edited
                    }
                    Key::PasteStart => {
                        input.paste = Some(String::new());
                        Typed::Edited
                    }
                    Key::Backspace if input.cursor > 0 => {
                        input.cursor -= 1;
                        input.text.remove(input.cursor);
                        Typed::Edited
                    }
                    Key::Delete if input.cursor < input.text.len() => {
                        input.text.remove(input.cursor);
                        Typed::Edited
                    }
                    Key::Left => {
                        input.cursor = input.cursor.saturating_sub(1);
                        Typed::Edited
                    }
                    Key::Right => {
                        input.cursor = (input.cursor + 1).min(input.text.len());
                        Typed::Edited
                    }
                    Key::WordLeft => {
                        input.cursor = input.word_start();
                        Typed::Edited
                    }
                    Key::WordRight => {
                        input.cursor = input.word_end();
                        Typed::Edited
                    }
                    Key::Home => {
                        input.cursor = input.line_start();
                        Typed::Edited
                    }
                    Key::End => {
                        input.cursor = input.line_end();
                        Typed::Edited
                    }
                    Key::KillLine => {
                        let start = input.line_start();
                        input.text.drain(start..input.cursor);
                        input.cursor = start;
                        Typed::Edited
                    }
                    Key::KillToEnd => {
                        let end = input.line_end();
                        input.text.drain(input.cursor..end);
                        Typed::Edited
                    }
                    Key::KillWord => {
                        let start = input.word_start();
                        input.text.drain(start..input.cursor);
                        input.cursor = start;
                        Typed::Edited
                    }
                    Key::Escape => {
                        *input = InputLine::default();
                        Typed::Edited
                    }
                    // The newest undelivered message comes back for editing; Enter queues it
                    // again.
                    Key::Up if input.text.is_empty() && turn => match state.queued.pop() {
                        Some(text) => {
                            input.set(&text);
                            Typed::Edited
                        }
                        None => Typed::Ignored,
                    },
                    Key::Enter | Key::SendNow if !turn => Typed::Submitted(input.take()),
                    Key::Enter | Key::SendNow => {
                        let text = input.take();
                        if !text.trim().is_empty() {
                            state.queued.push(text.trim().to_string());
                        }
                        match key {
                            Key::SendNow if !state.queued.is_empty() => Typed::SendNow,
                            _ if text.trim().is_empty() => Typed::Ignored,
                            _ => Typed::Queued,
                        }
                    }
                    _ => Typed::Ignored,
                }
            }
        };
        relay::repaint();
        typed
    }

    /// ↑/↓ inside a message that spans rows; false at its first or last row, where the key
    /// means something else (history, tool calls, the children).
    pub fn move_vertical(&self, delta: isize) -> bool {
        let moved = {
            let mut state = self.shared.state.lock().unwrap();
            let width = input_width(state.size.0);
            let input = &mut state.input;
            let layout = layout(&input.text, input.cursor, width);
            let (row, _) = layout.caret;
            match row
                .checked_add_signed(delta)
                .filter(|row| *row < layout.rows.len())
            {
                Some(target) => {
                    let column = width_of(&input.text[layout.rows[row].0..input.cursor]);
                    let (start, end) = layout.rows[target];
                    let mut cursor = start;
                    while cursor < end && width_of(&input.text[start..=cursor]) <= column {
                        cursor += 1;
                    }
                    input.cursor = cursor;
                    true
                }
                None => false,
            }
        };
        if moved {
            relay::repaint();
        }
        moved
    }

    /// What the input box holds and where its caret is (a character index).
    pub fn input(&self) -> (String, usize) {
        let state = self.shared.state.lock().unwrap();
        (state.input.text.iter().collect(), state.input.cursor)
    }

    pub fn set_input(&self, text: &str, cursor: usize) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.input.set(text);
            state.input.cursor = cursor.min(state.input.text.len());
        }
        relay::repaint();
    }

    /// What the status line leads with until cleared with `None`.
    pub fn set_status(&self, status: Option<String>) {
        self.shared.state.lock().unwrap().status = status;
        relay::repaint();
    }

    /// The text typed into the input box so far.
    pub fn input_text(&self) -> String {
        self.input().0
    }

    pub fn clear_input(&self) {
        self.shared.state.lock().unwrap().input = InputLine::default();
        relay::repaint();
    }

    pub fn has_rows(&self) -> bool {
        !self.shared.state.lock().unwrap().rows.is_empty()
    }

    pub fn selected(&self) -> Option<ChildRow> {
        let state = self.shared.state.lock().unwrap();
        state
            .selected
            .and_then(|index| state.rows.get(index).cloned())
    }

    pub fn is_selecting(&self) -> bool {
        self.shared.state.lock().unwrap().selected.is_some()
    }

    /// Moves the selection by `delta` rows; moving up past the first row ends the selection.
    /// Returns whether a row is still selected.
    pub fn step(&self, delta: isize) -> bool {
        let selecting = {
            let mut state = self.shared.state.lock().unwrap();
            state.selected = next_selection(state.selected, state.rows.len(), delta);
            follow_selection(&mut state);
            state.selected.is_some()
        };
        relay::repaint();
        selecting
    }

    pub fn clear_selection(&self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.selected = None;
            follow_selection(&mut state);
        }
        relay::repaint();
    }
}

impl Drop for Dock {
    fn drop(&mut self) {
        write_raw("\x1b[?2004l");
        relay::remove_painter();
    }
}

impl InputLine {
    fn insert(&mut self, text: &str) {
        for c in text.chars() {
            let c = if c == '\r' { '\n' } else { c };
            self.text.insert(self.cursor, c);
            self.cursor += 1;
        }
    }

    fn set(&mut self, text: &str) {
        self.text = text.chars().collect();
        self.cursor = self.text.len();
        self.paste = None;
    }

    fn take(&mut self) -> String {
        std::mem::take(self).text.into_iter().collect()
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor]
            .iter()
            .rposition(|c| *c == '\n')
            .map_or(0, |index| index + 1)
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .iter()
            .position(|c| *c == '\n')
            .map_or(self.text.len(), |index| self.cursor + index)
    }

    fn word_start(&self) -> usize {
        let mut index = self.cursor;
        while index > 0 && self.text[index - 1].is_whitespace() {
            index -= 1;
        }
        while index > 0 && !self.text[index - 1].is_whitespace() {
            index -= 1;
        }
        index
    }

    fn word_end(&self) -> usize {
        let mut index = self.cursor;
        let len = self.text.len();
        while index < len && self.text[index].is_whitespace() {
            index += 1;
        }
        while index < len && !self.text[index].is_whitespace() {
            index += 1;
        }
        index
    }
}

/// An interactive session takes over the terminal before it prints its header, so the header
/// opens the transcript the pane is drawn under.
pub fn begin_session() {
    if is_capable() {
        relay::begin();
    }
}

/// The kitty keyboard protocol's "disambiguate" level for as long as it lives: Ctrl+Enter
/// arrives as its own key. Ctrl-C then arrives as a key too, so its holder must cancel on
/// `Key::Interrupt` itself.
pub struct ExtendedKeys;

impl ExtendedKeys {
    pub fn enable() -> Option<Self> {
        is_capable().then(|| {
            write_raw("\x1b[>1u");
            Self
        })
    }
}

impl Drop for ExtendedKeys {
    fn drop(&mut self) {
        write_raw("\x1b[<u");
    }
}

pub fn is_capable() -> bool {
    super::terminal::is_capable()
}

/// From the line editor's key handler: true when the pane has children to move into.
pub fn request_focus() -> bool {
    with_active(|shared| {
        let mut state = shared.state.lock().unwrap();
        state.focus_requested = !state.rows.is_empty();
        state.focus_requested
    })
    .unwrap_or(false)
}

/// The prompt's ↓ key on an empty line asks for the children; the input reader takes the
/// request once the line editor has returned.
pub fn take_focus_request() -> bool {
    with_active(|shared| std::mem::take(&mut shared.state.lock().unwrap().focus_requested))
        .unwrap_or(false)
}

/// A full-screen view or an external program takes the whole screen; the pane leaves it until
/// `resume`.
pub fn suspend() {
    relay::suspend();
}

pub fn resume() {
    relay::resume();
}

/// The pane off the screen for as long as it lives: around a prompt that draws its own
/// question (a tool confirmation) or a program that takes the terminal.
pub struct Hidden;

impl Hidden {
    pub fn new() -> Self {
        with_active(|shared| shared.state.lock().unwrap().question = true);
        suspend();
        Self
    }
}

impl Default for Hidden {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Hidden {
    fn drop(&mut self) {
        with_active(|shared| shared.state.lock().unwrap().question = false);
        resume();
    }
}

fn with_active<R>(f: impl FnOnce(&Shared) -> R) -> Option<R> {
    let shared = ACTIVE.get()?.lock().unwrap().upgrade()?;
    Some(f(&shared))
}

impl Shared {
    /// New rows from a load; `None` (a failed query) keeps the rows already shown.
    fn update(&self, rows: Option<Vec<ChildRow>>) {
        if let Some(rows) = rows {
            let mut state = self.state.lock().unwrap();
            if let Some(selected) = state.selected {
                let id = state.rows.get(selected).map(|row| row.id.clone());
                state.selected = id
                    .and_then(|id| rows.iter().position(|row| row.id == id))
                    .or(Some(selected.min(rows.len().saturating_sub(1))))
                    .filter(|_| !rows.is_empty());
            }
            state.rows = rows;
            follow_selection(&mut state);
        }
        relay::repaint();
    }
}

fn next_selection(selected: Option<usize>, rows: usize, delta: isize) -> Option<usize> {
    let last = rows.saturating_sub(1);
    match selected {
        _ if rows == 0 => None,
        None if delta > 0 => Some(0),
        None => None,
        Some(0) if delta < 0 => None,
        Some(index) => Some(index.saturating_add_signed(delta).min(last)),
    }
}

/// Child rows that fit under the input box in a terminal `lines` tall.
fn child_slots(state: &State, cols: usize, lines: usize) -> usize {
    let input = layout(&state.input.text, state.input.cursor, input_width(cols))
        .rows
        .len()
        .min(MAX_INPUT_ROWS);
    // Status line, two rules, the input, the children's summary, and two rows of output.
    let fixed = 3 + input + 1 + 2 + usize::from(state.notice.is_some());
    lines.saturating_sub(fixed).min(MAX_ROWS)
}

/// How many child rows show, and whether the last slot instead says what is out of view.
fn window(state: &State, slots: usize) -> (usize, bool) {
    let overflow = state.rows.len() > slots;
    (
        if overflow {
            slots.saturating_sub(1)
        } else {
            state.rows.len()
        },
        overflow,
    )
}

/// Scrolls the list so the selected child is shown; without a selection it shows the top.
fn follow_selection(state: &mut State) {
    let (cols, lines) = state.size;
    let (shown, _) = window(state, child_slots(state, cols, lines));
    state.offset = match state.selected {
        None => 0,
        Some(selected) if selected < state.offset => selected,
        Some(selected) if shown > 0 && selected >= state.offset + shown => selected + 1 - shown,
        Some(_) => state.offset,
    };
    state.offset = state.offset.min(state.rows.len().saturating_sub(shown));
}

/// Every line of the pane, top first, for a terminal `cols` wide and `lines` tall.
fn pane(state: &State, cols: usize, lines: usize) -> Vec<String> {
    let width = cols.saturating_sub(1);
    let rule = style("─".repeat(width)).dim().to_string();
    let mut out = vec![status_line(state, width), rule.clone()];
    if state.question {
        out.push(format!(
            "{} {}",
            style("↑").yellow().bold(),
            style("Answer the question above: ↑↓ to move, Enter to choose, Esc to cancel").yellow()
        ));
    } else {
        out.extend(input_rows(state, cols));
    }
    out.push(rule);
    if let Some(notice) = &state.notice {
        out.push(style(format!("  {notice}")).yellow().to_string());
    }
    let slots = child_slots(state, cols, lines);
    out.push(children_summary(state, width));
    let (shown, overflow) = window(state, slots);
    for slot in 0..shown {
        let index = state.offset + slot;
        if let Some(child) = state.rows.get(index) {
            out.push(view::child_row(child, state.selected == Some(index), width));
        }
    }
    if overflow && slots > 0 {
        let above = state.offset;
        let below = state.rows.len().saturating_sub(state.offset + shown);
        let mut parts = Vec::new();
        if above > 0 {
            parts.push(format!("↑ {above} above"));
        }
        if below > 0 {
            parts.push(format!("↓ {below} more"));
        }
        parts.push("/children for all".to_string());
        out.push(style(format!("  {}", parts.join(" · "))).dim().to_string());
    }
    out
}

/// What the session is doing on the left, the context window on the right.
fn status_line(state: &State, width: usize) -> String {
    let mut left = Vec::new();
    if let Some(status) = &state.status {
        left.push(style(status.clone()).yellow().to_string());
    }
    if let Some(started) = state.turn_started {
        left.push(format!(
            "{} {}",
            style("✻ working").cyan(),
            style(format_elapsed(started.elapsed())).dim()
        ));
    }
    let queued = state.queued.len() + usize::from(state.steering.is_some());
    if queued > 0 {
        left.push(style(format!("{queued} queued")).dim().to_string());
    }
    let left = format!("  {}", left.join(&style(" · ").dim().to_string()));
    let right = state
        .context
        .map(|(used, limit)| context_bar(used, limit))
        .unwrap_or_default();
    spread(&left, &right, width)
}

/// `left` and `right` on one line, `right` flush right; `left` gives way when both do not fit.
fn spread(left: &str, right: &str, width: usize) -> String {
    let right_width = console::measure_text_width(right);
    let room = width.saturating_sub(right_width + 1);
    let left = console::truncate_str(left, room, "…");
    let pad = width.saturating_sub(console::measure_text_width(&left) + right_width);
    format!("{left}{}{right}", " ".repeat(pad))
}

/// The context window's fill, as the status line shows it: `━━━━╌╌╌╌ 40% 102k/258k`.
pub fn context_bar(used: usize, limit: usize) -> String {
    if limit == 0 {
        return style("context unavailable").dim().to_string();
    }
    let percentage = (((used as f64 / limit as f64) * 100.0).round() as usize).min(100);
    let bar_width = 20;
    let filled = (((percentage as f64 / 100.0) * bar_width as f64).round() as usize).min(bar_width);
    let bar = format!("{}{}", "━".repeat(filled), "╌".repeat(bar_width - filled));
    let bar = if percentage < 50 {
        style(bar).green().dim()
    } else if percentage < 85 {
        style(bar).yellow()
    } else {
        style(bar).red()
    };
    format!(
        "{bar} {}",
        style(format!("{percentage}% {}/{}", tokens(used), tokens(limit))).dim()
    )
}

fn tokens(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m {:02}s", secs / 60, secs % 60),
        _ => format!("{}h {:02}m", secs / 3600, secs / 60 % 60),
    }
}

/// The children's count on the left and the keys that apply now on the right.
fn children_summary(state: &State, width: usize) -> String {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for row in &state.rows {
        let label = row.status.label();
        match counts.iter_mut().find(|(name, _)| *name == label) {
            Some((_, count)) => *count += 1,
            None => counts.push((label, 1)),
        }
    }
    let summary = if counts.is_empty() {
        if state.always_list {
            "no children running".to_string()
        } else {
            String::new()
        }
    } else {
        counts
            .iter()
            .map(|(name, count)| format!("{count} {name}"))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let keys = if state.question {
        String::new()
    } else if state.selected.is_some() {
        "↑↓ select · Enter open · Esc back".to_string()
    } else {
        let mut keys = vec![if state.turn_started.is_some() {
            "Enter queues · Ctrl+Enter interrupts"
        } else {
            "Enter sends · Ctrl+J newline"
        }];
        if !state.rows.is_empty() {
            keys.push("↓ children");
        }
        keys.join(" · ")
    };
    spread(
        &style(format!("  {summary}")).dim().to_string(),
        &style(keys).dim().to_string(),
        width,
    )
}

/// Columns a message's text gets behind the gutter, keeping the terminal's last column free.
fn input_width(cols: usize) -> usize {
    cols.saturating_sub(GUTTER + 1).max(1)
}

fn char_width(c: char) -> usize {
    match c {
        '\t' => 1,
        c => c.width().unwrap_or(0),
    }
}

fn width_of(text: &[char]) -> usize {
    text.iter().map(|c| char_width(*c)).sum()
}

/// A message laid out in rows `width` columns wide: each row's character range (line breaks
/// excluded) and the caret's row and offset in it.
struct InputLayout {
    rows: Vec<(usize, usize)>,
    caret: (usize, usize),
}

fn layout(text: &[char], cursor: usize, width: usize) -> InputLayout {
    let mut rows = Vec::new();
    let (mut start, mut used, mut index) = (0, 0, 0);
    while index < text.len() {
        let c = text[index];
        if c == '\n' {
            rows.push((start, index));
            start = index + 1;
            used = 0;
            index += 1;
            continue;
        }
        let w = char_width(c);
        if used + w > width && index > start {
            rows.push((start, index));
            start = index;
            used = 0;
            continue;
        }
        used += w;
        index += 1;
    }
    rows.push((start, text.len()));
    let row = rows
        .iter()
        .rposition(|(start, _)| *start <= cursor)
        .unwrap_or(0);
    let mut caret = (row, cursor - rows[row].0);
    // A caret after a full last row goes to a row of its own.
    if cursor == text.len() && width_of(&text[rows[row].0..cursor]) >= width {
        rows.push((text.len(), text.len()));
        caret = (rows.len() - 1, 0);
    }
    InputLayout { rows, caret }
}

/// The input box's rows: the message behind the "› " gutter with its caret in reverse video.
/// A message taller than the box shows the rows around the caret.
fn input_rows(state: &State, cols: usize) -> Vec<String> {
    let input = &state.input;
    let layout = layout(&input.text, input.cursor, input_width(cols));
    let (caret_row, caret_offset) = layout.caret;
    let total = layout.rows.len();
    let shown = total.min(MAX_INPUT_ROWS);
    let first = caret_row.saturating_sub(shown - 1).min(total - shown);
    (first..first + shown)
        .map(|row| {
            let (start, end) = layout.rows[row];
            let mut text = if row == 0 {
                format!("{} ", style("›").cyan().bold())
            } else {
                " ".repeat(GUTTER)
            };
            for (offset, c) in input.text[start..end].iter().enumerate() {
                let c = if *c == '\t' { ' ' } else { *c };
                if row == caret_row && offset == caret_offset {
                    text.push_str(&style(c).reverse().to_string());
                } else {
                    text.push(c);
                }
            }
            if row == caret_row && caret_offset == end - start {
                text.push_str(&style(' ').reverse().to_string());
            }
            text
        })
        .collect()
}

/// The pane lists running children only, newest first; `/children` lists the rest. `None`
/// when a source could not be read (router-acp's state query times out under load), so a
/// failed read never blanks a list of children that are still running.
async fn load_rows(sources: &Sources, turn_active: bool) -> Option<Vec<ChildRow>> {
    let mut rows =
        super::goose_children(&sources.session_manager, &sources.session_id, turn_active)
            .await
            .ok()?;
    if let Some((source, root)) = &sources.router {
        let delegates = source
            .delegates(std::slice::from_ref(root), QUERY_TIMEOUT)
            .await
            .ok()?;
        rows.extend(router::dock_rows(&delegates, root, chrono::Utc::now()));
    }
    rows.retain(|row| row.status == ChildStatus::Running);
    rows.reverse();
    Some(rows)
}

fn terminal_size() -> (usize, usize) {
    let (rows, cols) = console::Term::stdout().size();
    (cols as usize, rows as usize)
}

fn write_raw(text: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, status: ChildStatus) -> ChildRow {
        ChildRow {
            id: id.into(),
            label: id.into(),
            status,
            model: "opus 5.5".to_string(),
            started_at: None,
            activity: String::new(),
        }
    }

    fn typing() -> Dock {
        Dock {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    size: (80, 40),
                    ..State::default()
                }),
            }),
            _task: AbortOnDropHandle::new(tokio::spawn(async {})),
        }
    }

    fn in_turn() -> Dock {
        let dock = typing();
        dock.begin_turn();
        dock
    }

    fn plain(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .map(|line| console::strip_ansi_codes(line).trim_end().to_string())
            .collect()
    }

    fn painted(state: &State, cols: usize, lines: usize) -> Vec<String> {
        plain(&pane(state, cols, lines))
    }

    fn input(text: &str, cursor: usize) -> InputLine {
        InputLine {
            text: text.chars().collect(),
            cursor,
            paste: None,
        }
    }

    #[test]
    fn the_pane_puts_status_and_context_over_the_input_and_children_under_it() {
        let state = State {
            rows: vec![
                row("a", ChildStatus::Running),
                row("b", ChildStatus::Running),
            ],
            always_list: true,
            context: Some((102_000, 258_000)),
            input: input("hi", 2),
            ..State::default()
        };
        let lines = painted(&state, 80, 40);
        assert!(lines[0].ends_with("40% 102k/258k"), "{lines:?}");
        assert!(
            lines[1].starts_with("───") && lines[3].starts_with("───"),
            "{lines:?}"
        );
        assert_eq!(lines[2], "› hi");
        assert!(
            lines[4].contains("2 running") && lines[4].ends_with("↓ children"),
            "{lines:?}"
        );
        assert!(lines[5].contains("opus 5.5") && lines[6].contains("opus 5.5"));
        assert_eq!(lines.len(), 7);
        let styled = pane(&state, 80, 40);
        assert!(
            styled
                .iter()
                .all(|line| console::measure_text_width(line) < 80),
            "the last column stays free: {lines:?}"
        );
    }

    #[test]
    fn a_turn_shows_its_time_and_queue_on_the_status_line() {
        let mut state = State {
            turn_started: Some(Instant::now() - Duration::from_secs(75)),
            queued: vec!["x".into()],
            status: Some("shutting down 1/3".into()),
            ..State::default()
        };
        let lines = painted(&state, 80, 40);
        assert!(
            lines[0].contains("shutting down 1/3 · ✻ working 1m 15s · 1 queued"),
            "{lines:?}"
        );
        assert!(
            lines[4].ends_with("Enter queues · Ctrl+Enter interrupts"),
            "{lines:?}"
        );
        state.turn_started = None;
        assert!(painted(&state, 80, 40)[4].ends_with("Enter sends · Ctrl+J newline"));
    }

    #[test]
    fn the_list_scrolls_to_keep_the_selection_in_view() {
        let mut state = State {
            rows: (0..10)
                .map(|i| row(&i.to_string(), ChildStatus::Running))
                .collect(),
            size: (80, 12),
            ..State::default()
        };
        // Twelve lines: status, two rules, one input row, the summary and two output rows
        // leave five child slots, the last of them saying what is out of view.
        assert_eq!(child_slots(&state, 80, 12), 5);
        for _ in 0..6 {
            state.selected = next_selection(state.selected, state.rows.len(), 1);
            follow_selection(&mut state);
        }
        assert_eq!((state.selected, state.offset), (Some(5), 2));
        let lines = painted(&state, 80, 12);
        assert!(
            lines.iter().any(|line| line.contains("› opus 5.5")),
            "{lines:?}"
        );
        assert!(
            lines.last().unwrap().contains("↑ 2 above · ↓ 4 more"),
            "{lines:?}"
        );
        state.selected = None;
        follow_selection(&mut state);
        assert_eq!(state.offset, 0);
    }

    #[test]
    fn up_from_the_first_row_leaves_the_children() {
        assert_eq!(next_selection(None, 2, 1), Some(0));
        assert_eq!(next_selection(Some(0), 2, 1), Some(1));
        assert_eq!(next_selection(Some(1), 2, 1), Some(1));
        assert_eq!(next_selection(Some(1), 2, -1), Some(0));
        assert_eq!(next_selection(Some(0), 2, -1), None);
        assert_eq!(next_selection(None, 0, 1), None);
    }

    fn typed(dock: &Dock, text: &str) {
        for c in text.chars() {
            dock.type_key(Key::Char(c));
        }
    }

    fn queued(dock: &Dock) -> (Vec<String>, Option<String>) {
        let state = dock.shared.state.lock().unwrap();
        (state.queued.clone(), state.steering.clone())
    }

    #[tokio::test]
    async fn enter_submits_at_the_prompt_and_queues_during_a_turn() {
        let dock = typing();
        typed(&dock, "hello");
        assert_eq!(dock.type_key(Key::Enter), Typed::Submitted("hello".into()));
        assert_eq!(dock.input_text(), "");

        let dock = in_turn();
        typed(&dock, "first");
        assert_eq!(dock.type_key(Key::Enter), Typed::Queued);
        assert_eq!(
            dock.type_key(Key::Enter),
            Typed::Ignored,
            "an empty line queues nothing"
        );
        typed(&dock, "secnd");
        dock.type_key(Key::Left);
        dock.type_key(Key::Left);
        dock.type_key(Key::Char('o'));
        assert_eq!(dock.type_key(Key::SendNow), Typed::SendNow);
        assert_eq!(dock.take_queued().as_deref(), Some("first\n\nsecond"));
        assert_eq!(dock.take_queued(), None);
    }

    #[tokio::test]
    async fn text_typed_during_a_turn_stays_for_the_prompt() {
        let dock = in_turn();
        typed(&dock, "half a thought");
        dock.end_turn();
        assert_eq!(dock.input_text(), "half a thought");
        assert_eq!(
            dock.type_key(Key::Enter),
            Typed::Submitted("half a thought".into())
        );
    }

    #[tokio::test]
    async fn queued_messages_reach_the_turn_one_at_a_time() {
        let dock = in_turn();
        typed(&dock, "first");
        dock.type_key(Key::Enter);
        typed(&dock, "second");
        dock.type_key(Key::Enter);

        assert_eq!(dock.next_steer().as_deref(), Some("first"));
        assert_eq!(dock.next_steer(), None, "one message on its way at a time");
        let status = painted(&dock.shared.state.lock().unwrap(), 80, 40)[0].clone();
        assert!(
            status.contains("2 queued"),
            "the one on its way still counts: {status}"
        );

        dock.finish_steer(false);
        assert_eq!(queued(&dock), (vec!["first".into(), "second".into()], None));
        assert_eq!(
            dock.next_steer().as_deref(),
            Some("first"),
            "a declined one goes first"
        );
        dock.finish_steer(true);
        assert_eq!(queued(&dock), (vec!["second".into()], None));

        assert_eq!(dock.next_steer().as_deref(), Some("second"));
        typed(&dock, "third");
        dock.type_key(Key::Enter);
        assert_eq!(
            dock.take_queued().as_deref(),
            Some("second\n\nthird"),
            "a turn that ends sends what never arrived"
        );
        dock.requeue(vec!["lost".into()]);
        assert_eq!(dock.take_queued().as_deref(), Some("lost"));
    }

    #[tokio::test]
    async fn up_on_an_empty_line_takes_back_the_newest_queued_message() {
        let dock = in_turn();
        assert_eq!(dock.type_key(Key::Up), Typed::Ignored, "nothing queued");
        typed(&dock, "one");
        dock.type_key(Key::Enter);
        typed(&dock, "two");
        dock.type_key(Key::Enter);

        assert_eq!(dock.type_key(Key::Up), Typed::Edited);
        assert_eq!(queued(&dock).0, ["one"]);
        assert_eq!(
            dock.type_key(Key::Up),
            Typed::Ignored,
            "a line being edited keeps it"
        );
        typed(&dock, "!");
        assert_eq!(dock.input(), ("two!".to_string(), 4));
        assert_eq!(dock.type_key(Key::Enter), Typed::Queued);
        assert_eq!(queued(&dock).0, ["one", "two!"]);
    }

    #[tokio::test]
    async fn line_editing() {
        let dock = typing();
        typed(&dock, "one two");
        dock.type_key(Key::Newline);
        typed(&dock, "three four");
        assert_eq!(dock.input_text(), "one two\nthree four");
        dock.type_key(Key::KillWord);
        assert_eq!(dock.input_text(), "one two\nthree ");
        dock.type_key(Key::Home);
        assert_eq!(
            dock.input().1,
            8,
            "Home goes to the start of the caret's line"
        );
        dock.type_key(Key::KillToEnd);
        assert_eq!(dock.input_text(), "one two\n");
        dock.type_key(Key::Backspace);
        dock.type_key(Key::WordLeft);
        assert_eq!(dock.input().1, 4);
        dock.type_key(Key::Delete);
        assert_eq!(dock.input_text(), "one wo");
        dock.type_key(Key::End);
        dock.type_key(Key::KillLine);
        assert_eq!(dock.input(), (String::new(), 0));
    }

    #[tokio::test]
    async fn a_paste_goes_in_whole_with_its_line_breaks() {
        let dock = typing();
        dock.type_key(Key::PasteStart);
        typed(&dock, "a");
        dock.type_key(Key::Newline);
        // Enter inside a paste is a line break, never a send.
        assert_eq!(dock.type_key(Key::Enter), Typed::Edited);
        typed(&dock, "b");
        assert_eq!(dock.input_text(), "", "nothing until the paste ends");
        dock.type_key(Key::PasteEnd);
        assert_eq!(dock.input(), ("a\n\nb".to_string(), 4));
    }

    #[test]
    fn a_long_message_wraps_and_keeps_the_caret_in_view() {
        let text: String = ('a'..='z').cycle().take(50).collect();
        let state = State {
            input: input(&text, 50),
            ..State::default()
        };
        // 20 columns: 17 for text behind the gutter, the last column free.
        let rows = plain(&input_rows(&state, 20));
        assert_eq!(rows.len(), 3);
        assert_eq!(rows.concat().replace(['›', ' '], ""), text);
        assert!(rows
            .iter()
            .all(|row| console::measure_text_width(row) <= 19));

        let tall = State {
            input: input(&"x\n".repeat(10), 0),
            ..State::default()
        };
        let rows = plain(&input_rows(&tall, 80));
        assert_eq!(rows.len(), MAX_INPUT_ROWS);
        assert_eq!(rows[0], "› x", "the caret's row is in view");
    }

    #[tokio::test]
    async fn up_and_down_move_between_the_rows_of_a_message() {
        let dock = typing();
        typed(&dock, "first line");
        dock.type_key(Key::Newline);
        typed(&dock, "second");
        assert!(dock.move_vertical(-1));
        assert_eq!(dock.input().1, 6, "the same column on the row above");
        assert!(!dock.move_vertical(-1), "the first row: ↑ means history");
        assert!(dock.move_vertical(1));
        assert_eq!(dock.input().1, 17);
        assert!(!dock.move_vertical(1));
    }

    #[test]
    fn the_caret_after_a_full_row_starts_the_next() {
        let text: Vec<char> = "abcd".chars().collect();
        let full = layout(&text, 4, 4);
        assert_eq!(full.rows, [(0, 4), (4, 4)]);
        assert_eq!(full.caret, (1, 0));
        assert_eq!(layout(&text, 2, 4).caret, (0, 2));
    }

    #[tokio::test]
    async fn a_failed_load_keeps_the_children_already_shown() {
        let dock = typing();
        let rows = |dock: &Dock| dock.shared.state.lock().unwrap().rows.len();
        dock.shared.update(Some(vec![
            row("a", ChildStatus::Running),
            row("b", ChildStatus::Running),
        ]));
        assert_eq!(rows(&dock), 2);
        dock.shared.update(None);
        assert_eq!(rows(&dock), 2, "a timed-out query is not an empty list");
        dock.shared.update(Some(Vec::new()));
        assert_eq!(rows(&dock), 0);
    }

    #[test]
    fn a_router_pane_says_when_no_children_run() {
        let state = State {
            always_list: true,
            ..State::default()
        };
        assert!(painted(&state, 80, 40)[4].contains("no children running"));
    }
}
