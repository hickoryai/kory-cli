//! The pane under the session's output, laid out like Claude Code's footer:
//!
//! ```text
//!   ◓ working 1m 12s · 1 queued ────────────────── ━━━━━━━━╌╌╌╌╌╌╌╌╌╌╌╌ 40% 102k/258k
//!   › the message being written
//!   ─────────────────────────────────────────────────────────────────────────────────────
//!   12 running                         Enter queues · Ctrl+Enter interrupts · ↓ children
//!     opus[1m] [6m] jam-packet-map  01a19fae  → Read file …
//! ```
//!
//! The input box between two rules, the upper one carrying the status on its left and the
//! context bar on its right, and the session's running
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
use goose::acp::session_activity::{self, SessionActivity, SessionTurn};
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
/// Columns of rule always shown between the status and the context bar.
const MIN_RULE: usize = 3;
/// The working spinner's frames and pace: the thinking spinner's (cliclack's), which the pane's
/// status line replaces.
const SPINNER: [&str; 4] = ["◒", "◐", "◓", "◑"];
const SPINNER_FRAME: Duration = Duration::from_millis(100);

/// A child pane on screen is working, so the spinner needs drawing on.
static CHILD_WORKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A finished turn: how long it ran and the local time it ended, `1h 30m (12:34am)`.
fn turn_stamp(elapsed: Duration, ended: chrono::DateTime<chrono::Local>) -> String {
    format!("{} ({})", format_elapsed(elapsed), ended.format("%-I:%M%P"))
}

/// The working spinner `elapsed` into a turn.
fn spinner_frame(elapsed: Duration) -> &'static str {
    SPINNER[(elapsed.as_millis() / SPINNER_FRAME.as_millis()) as usize % SPINNER.len()]
}

/// Whether the session's pane is up: its status line then shows that the agent works, and the
/// thinking spinner above it would say so twice.
pub fn is_up() -> bool {
    with_active(|_| ()).is_some()
}

/// Enter in a child session's input box.
const CHILD_TAKES_NO_MESSAGES: &str = "goose subagents take no messages · Esc returns";

/// The live pane, so a full-screen view or child process can hand the screen back and forth.
static ACTIVE: OnceLock<Mutex<Weak<Shared>>> = OnceLock::new();

/// The pane of the child session on screen: what is typed goes there.
static CHILD_PANE: Mutex<Weak<ChildPane>> = Mutex::new(Weak::new());

/// `pane` takes what is typed from now on, or the session's own does again.
pub fn focus_child(pane: Option<&Arc<ChildPane>>) {
    *CHILD_PANE.lock().unwrap() = pane.map(Arc::downgrade).unwrap_or_default();
}

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
    /// How long the last turn took and when it ended (`1h 30m (12:34am)`), shown at the prompt
    /// where `working` was.
    finished: Option<String>,
    /// The existing router activity notification, independent of local prompt/output state.
    router_activity: Option<SessionActivity>,
    /// A session-wide state the status line leads with, e.g. `shutting down 1/3`.
    status: Option<String>,
    /// Tokens in the context and the model's limit.
    context: Option<(usize, usize)>,
    /// A one-time notice under the input box, cleared by the next key (e.g. "Ctrl-C again
    /// to exit").
    notice: Option<String>,
    /// A question is open in the transcript above and takes the keys, not the input box.
    question: bool,
    /// The goal the session is held to, shown on the status line.
    goal: Option<goose::providers::base::SessionGoal>,
    /// ↑ selected the goal: the next key edits or removes it, or moves on.
    goal_selected: bool,
    /// Counts edits made by the key reader, for a prompt that tracks what was typed.
    edits: u64,
    /// Nothing is sent now (a shutdown is under way): Enter goes to the session, which says
    /// why, instead of the queue.
    closed: bool,
}

/// What a key does to a selected goal.
#[derive(Debug, PartialEq, Eq)]
pub enum GoalKey {
    /// Enter: its objective, to edit in the input box.
    Edit(String),
    /// Backspace or Delete.
    Remove,
    /// ↑ again: on to the tool calls.
    ToolCalls,
    /// ↓ or Esc.
    Back,
    Stay,
}

impl State {
    fn working(&self) -> bool {
        self.router_activity
            .as_ref()
            .map_or(self.turn_started.is_some(), |activity| activity.busy)
    }

    /// Why the router holds the running turn.
    fn paused(&self) -> Option<&str> {
        let activity = self.router_activity.as_ref().filter(|a| a.busy)?;
        activity.paused.as_deref()
    }

    /// The second a held turn's timer shows. Its line has no spinner, so it is drawn again only
    /// when this moves on.
    fn paused_second(&self) -> Option<u64> {
        self.paused()?;
        match self.router_activity.as_ref()?.turn {
            Some(SessionTurn::Running(started)) => Some(started.elapsed().as_secs()),
            _ => None,
        }
    }

    fn spinner_turns(&self) -> bool {
        self.working() && self.paused().is_none()
    }
}

struct Shared {
    state: Mutex<State>,
}

pub struct Dock {
    shared: Arc<Shared>,
    _task: AbortOnDropHandle<()>,
    /// The account bar above the transcript, for a router session.
    _accounts: Option<super::accounts::AccountBar>,
}

/// Where the pane's children come from: goose's own subagents and router-acp's delegates.
pub struct Sources {
    pub session_manager: Arc<SessionManager>,
    pub session_id: String,
    pub router: Option<(RouterStateSource, String)>,
    /// The session's agent, whose provider runs its background work (terminals, monitors).
    pub agent: Option<Arc<goose::agents::Agent>>,
}

impl Dock {
    /// The pane, drawn through the relay; `None` when the terminal cannot host it.
    pub fn start(sources: Sources) -> Option<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                always_list: sources.router.is_some(),
                router_activity: sources.router.as_ref().map(|_| session_activity::current()),
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
        let accounts = sources
            .router
            .clone()
            .map(|(source, session_id)| super::accounts::AccountBar::start(source, session_id));
        *ACTIVE.get_or_init(Mutex::default).lock().unwrap() = Arc::downgrade(&shared);
        let painter = shared.clone();
        let ticking = Arc::downgrade(&shared);
        let context_agent = sources.agent.clone();
        let context_session = sources.session_id.clone();
        let context_painter = shared.clone();
        let task = tokio::spawn(async move {
            // Router wakes keep running while the local prompt waits for input.
            // Child queries must not hold up the provider's context display.
            let _context = context_agent.map(|agent| {
                AbortOnDropHandle::new(tokio::spawn(async move {
                    let mut ticks = tokio::time::interval(super::super::CONTEXT_REFRESH);
                    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    loop {
                        ticks.tick().await;
                        context_painter
                            .refresh_context(&agent, &context_session)
                            .await;
                    }
                }))
            });
            // The spinner turns while the session, or a child on screen, works.
            let _ticker = AbortOnDropHandle::new(tokio::spawn(async move {
                let mut paused_second = None;
                loop {
                    tokio::time::sleep(SPINNER_FRAME).await;
                    let Some(shared) = ticking.upgrade() else {
                        return;
                    };
                    let (spins, changed, pause) = {
                        let mut state = shared.state.lock().unwrap();
                        let mut changed = false;
                        let mut pause = None;
                        if let Some(activity) = &mut state.router_activity {
                            let latest = session_activity::current();
                            changed = *activity != latest;
                            if activity.paused != latest.paused {
                                pause = Some(latest.paused.clone());
                            }
                            *activity = latest;
                        }
                        let second = state.paused_second();
                        changed |= second != paused_second;
                        paused_second = second;
                        (state.spinner_turns(), changed, pause)
                    };
                    if let Some(reason) = pause {
                        super::super::program_status::report_pause(reason.as_deref());
                    }
                    if spins || changed || CHILD_WORKING.load(std::sync::atomic::Ordering::Relaxed)
                    {
                        relay::repaint();
                    }
                }
            }));
            loop {
                let turn_active = painter.state.lock().unwrap().working();
                let goal = load_goal(&sources).await;
                {
                    let mut state = painter.state.lock().unwrap();
                    if goal.is_none() {
                        state.goal_selected = false;
                    }
                    state.goal = goal;
                }
                painter.update(load_rows(&sources, turn_active).await);
                tokio::time::sleep(REPAINT).await;
            }
        });
        Some(Self {
            shared,
            _task: AbortOnDropHandle::new(task),
            _accounts: accounts,
        })
    }

    /// A turn starts (or goes on): Enter now queues instead of sending, and the status line
    /// counts time.
    pub fn begin_turn(&self) {
        let mut state = self.shared.state.lock().unwrap();
        state.turn_started.get_or_insert_with(Instant::now);
        state.finished = None;
        drop(state);
        relay::repaint();
    }

    /// Back to the prompt. What was typed and never sent stays in the input box. A turn that
    /// was running leaves how long it took and when it ended on the status line, and the same
    /// stamp in the transcript (`🕐 1h 30m (12:34am)`), so the history shows where it stopped.
    pub fn end_turn(&self) {
        let mut state = self.shared.state.lock().unwrap();
        if let Some(started) = state.turn_started.take() {
            // Returning to the local prompt does not end a router-owned turn.
            if state.router_activity.is_none() {
                let stamp = turn_stamp(started.elapsed(), chrono::Local::now());
                state.finished = Some(stamp.clone());
                drop(state);
                super::super::output::print_turn_stamp(&stamp);
            }
        }
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

    /// The oldest queued message, to hand to the running turn; none while one is on its way or
    /// while the router holds the turn, which cannot take one until it goes on.
    pub fn next_steer(&self) -> Option<String> {
        let mut state = self.shared.state.lock().unwrap();
        if state.steering.is_some() || state.queued.is_empty() || state.paused().is_some() {
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
        let typed = type_into(&mut self.shared.state.lock().unwrap(), key);
        relay::repaint();
        typed
    }

    /// What the key reader does with a key itself. Everything the input box, the queue and the
    /// children list answer alone needs nothing from the session, so it shows at once however
    /// busy the session is; the rest is handed to the session.
    pub fn direct_keys(&self) -> super::terminal::DirectKeys {
        let shared = Arc::downgrade(&self.shared);
        Arc::new(move |key| match shared.upgrade() {
            Some(shared) => shared.direct_key(key),
            None => Some(key),
        })
    }

    /// Changes whenever the key reader edits the input box.
    pub fn edits(&self) -> u64 {
        self.shared.state.lock().unwrap().edits
    }

    /// Brings the context bar up to date without waiting for it.
    pub fn update_context(&self, agent: Arc<goose::agents::Agent>, session_id: String) {
        let shared = self.shared.clone();
        tokio::spawn(async move { shared.refresh_context(&agent, &session_id).await });
    }

    /// Whether messages can be sent now; while not, the session answers Enter.
    pub fn set_closed(&self, closed: bool) {
        self.shared.state.lock().unwrap().closed = closed;
    }
}

/// A key that edits the text being written, whatever else the pane shows.
fn edits_text(key: Key) -> bool {
    matches!(
        key,
        Key::Char(_)
            | Key::Newline
            | Key::PasteStart
            | Key::Backspace
            | Key::Delete
            | Key::Left
            | Key::Right
            | Key::WordLeft
            | Key::WordRight
            | Key::Home
            | Key::End
            | Key::KillLine
            | Key::KillToEnd
            | Key::KillWord
    )
}

impl Shared {
    fn direct_key(&self, key: Key) -> Option<Key> {
        if super::terminal::screen_is_open() {
            return Some(key);
        }
        let child = CHILD_PANE.lock().unwrap().upgrade();
        let left = match child {
            Some(child) => child.direct_key(key),
            None => direct_key(&mut self.state.lock().unwrap(), key),
        };
        if left != Some(key) {
            relay::repaint();
        }
        left
    }
}

/// Acts on `key` from `state` alone, or returns what the session has to act on.
fn direct_key(state: &mut State, key: Key) -> Option<Key> {
    // Questions and the selected goal answer their own keys.
    if state.question || state.goal_selected {
        return Some(key);
    }
    let turn = state.turn_started.is_some();
    let pasting = state.input.paste.is_some();
    if !pasting {
        let empty = state.input.text.is_empty();
        match key {
            key if edits_text(key) => {}
            Key::Escape if state.selected.is_some() => {
                state.selected = None;
                follow_selection(state);
                return None;
            }
            Key::Escape => {}
            Key::Up if turn && state.selected.is_some() => {
                select(state, -1);
                return None;
            }
            Key::Up if turn && move_vertical(state, -1) => return None,
            // ↑ reaches the goal first; the tool calls after it are the session's.
            Key::Up if turn && state.queued.is_empty() && empty && state.goal.is_some() => {
                state.goal_selected = true;
                return None;
            }
            Key::Up if turn && !state.queued.is_empty() => {}
            Key::Down if turn && state.selected.is_none() && move_vertical(state, 1) => {
                return None
            }
            Key::Down if turn && !state.rows.is_empty() => {
                select(state, 1);
                return None;
            }
            // A command is the session's to act on, and so is Enter on a child.
            Key::Enter if turn && state.selected.is_none() && !state.closed => {
                let first = state.input.text.iter().find(|c| !c.is_whitespace());
                if first == Some(&'/') {
                    return Some(key);
                }
            }
            key => return Some(key),
        }
    }
    state.edits += 1;
    match type_into(state, key) {
        Typed::Queued => Some(Key::MessageQueued),
        _ => None,
    }
}

fn select(state: &mut State, delta: isize) {
    state.selected = next_selection(state.selected, state.rows.len(), delta);
    follow_selection(state);
}

/// ↑/↓ inside a message that spans rows; false at its first or last row.
fn move_vertical(state: &mut State, delta: isize) -> bool {
    let width = input_width(state.size.0);
    let input = &mut state.input;
    let layout = layout(&input.text, input.cursor, width);
    let (row, _) = layout.caret;
    let Some(target) = row
        .checked_add_signed(delta)
        .filter(|row| *row < layout.rows.len())
    else {
        return false;
    };
    let column = width_of(&input.text[layout.rows[row].0..input.cursor]);
    let (start, end) = layout.rows[target];
    let mut cursor = start;
    while cursor < end && width_of(&input.text[start..=cursor]) <= column {
        cursor += 1;
    }
    input.cursor = cursor;
    true
}

/// A key typed into `state`'s input box.
fn type_into(state: &mut State, key: Key) -> Typed {
    {
        state.notice = None;
        let turn = state.turn_started.is_some();
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
    }
}

impl Dock {
    /// ↑/↓ inside a message that spans rows; false at its first or last row, where the key
    /// means something else (history, tool calls, the children).
    pub fn move_vertical(&self, delta: isize) -> bool {
        let moved = move_vertical(&mut self.shared.state.lock().unwrap(), delta);
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

    /// Child `index` of the pane's list (a clicked row).
    pub fn row(&self, index: usize) -> Option<ChildRow> {
        self.shared.state.lock().unwrap().rows.get(index).cloned()
    }

    pub fn is_selecting(&self) -> bool {
        self.shared.state.lock().unwrap().selected.is_some()
    }

    /// Moves the selection by `delta` rows; moving up past the first row ends the selection.
    /// Returns whether a row is still selected.
    pub fn step(&self, delta: isize) -> bool {
        let selecting = {
            let mut state = self.shared.state.lock().unwrap();
            select(&mut state, delta);
            state.selected.is_some()
        };
        relay::repaint();
        selecting
    }

    /// ↑ on an empty input box: selects the goal when there is one.
    pub fn select_goal(&self) -> bool {
        let selected = {
            let mut state = self.shared.state.lock().unwrap();
            state.goal_selected = state.goal.is_some() && !state.goal_selected;
            state.goal_selected
        };
        relay::repaint();
        selected
    }

    pub fn is_goal_selected(&self) -> bool {
        self.shared.state.lock().unwrap().goal_selected
    }

    /// A router session's goal can change mid-turn through `_router/goal`.
    pub fn is_routed(&self) -> bool {
        self.shared.state.lock().unwrap().always_list
    }

    /// A key while the goal is selected; anything but `Stay` ends the selection.
    pub fn goal_key(&self, key: Key) -> GoalKey {
        let action = {
            let mut state = self.shared.state.lock().unwrap();
            let objective = state.goal.as_ref().map(|goal| goal.objective.clone());
            let action = match (key, objective) {
                (Key::Enter, Some(objective)) => GoalKey::Edit(objective),
                (Key::Backspace | Key::Delete, Some(_)) => GoalKey::Remove,
                (Key::Up, _) => GoalKey::ToolCalls,
                (Key::Down | Key::Escape, _) | (_, None) => GoalKey::Back,
                _ => GoalKey::Stay,
            };
            if action != GoalKey::Stay {
                state.goal_selected = false;
            }
            action
        };
        relay::repaint();
        action
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
    async fn refresh_context(&self, agent: &goose::agents::Agent, session_id: &str) {
        if let Ok(context) = super::super::load_context_usage(agent, session_id).await {
            let changed = {
                let mut state = self.state.lock().unwrap();
                let changed = state.context != context;
                state.context = context;
                changed
            };
            if changed {
                relay::repaint();
            }
        }
    }

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
    // Two rules, the input, the children's summary, and two rows of output.
    let fixed = 2 + input + 1 + 2 + usize::from(state.notice.is_some());
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
    let mut out = input_box(state, cols);
    if let Some(notice) = &state.notice {
        out.push(style(format!("  {notice}")).yellow().to_string());
    }
    let slots = child_slots(state, cols, lines);
    out.push(children_summary(state, width));
    let (shown, overflow) = window(state, slots);
    for slot in 0..shown {
        let index = state.offset + slot;
        if let Some(child) = state.rows.get(index) {
            let selected = state.selected == Some(index);
            let row = view::child_row(child, selected, width);
            // Gray until selected or under the pointer (the relay draws that white); a click
            // opens it.
            let row = if selected {
                row
            } else {
                style(console::strip_ansi_codes(&row).into_owned())
                    .dim()
                    .to_string()
            };
            out.push(format!("{}{row}", relay::child_row_mark(index)));
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

/// The input box between its two rules, the upper one carrying the status and context bar.
fn input_box(state: &State, cols: usize) -> Vec<String> {
    let width = cols.saturating_sub(1);
    let mut out = vec![top_rule(state, width)];
    if state.question {
        out.push(format!(
            "{} {}",
            style("↑").yellow().bold(),
            style("Answer the question above: ↑↓ to move, Enter to choose, Esc to cancel").yellow()
        ));
    } else {
        out.extend(input_rows(state, cols));
    }
    out.push(style("─".repeat(width)).dim().to_string());
    out
}

/// The pane of a child session on screen: the session's own status rule and input box, with
/// nothing under them, since a child has no children.
pub struct ChildPane {
    state: Mutex<State>,
    /// Whether Enter sends what is typed to the child (router-acp delegates take messages;
    /// goose's own subagents do not).
    sends: bool,
}

impl ChildPane {
    pub fn new(sends: bool) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                size: terminal_size(),
                ..State::default()
            }),
            sends,
        })
    }

    /// A state the status line leads with: how the last message was delivered, or why not.
    pub fn set_status(&self, status: Option<String>) {
        self.state.lock().unwrap().status = status;
        relay::repaint();
    }

    /// The pane's painter for the relay.
    pub fn painter(self: &Arc<Self>) -> relay::Painter {
        let weak = Arc::downgrade(self);
        Arc::new(move |cols, lines| match weak.upgrade() {
            Some(pane) => {
                let mut state = pane.state.lock().unwrap();
                state.size = (cols, lines);
                input_box(&state, cols)
            }
            None => Vec::new(),
        })
    }

    /// The child's turn: running since `started` (the status line counts its time), or not.
    pub fn set_turn(&self, started: Option<Instant>) {
        self.state.lock().unwrap().turn_started = started;
        CHILD_WORKING.store(started.is_some(), std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_context(&self, context: Option<(usize, usize)>) {
        self.state.lock().unwrap().context = context;
    }

    pub fn input_is_empty(&self) -> bool {
        self.state.lock().unwrap().input.text.is_empty()
    }

    /// What the key reader does itself with a key typed at this pane.
    fn direct_key(&self, key: Key) -> Option<Key> {
        let mut state = self.state.lock().unwrap();
        let edits = state.input.paste.is_some()
            || edits_text(key)
            || (key == Key::Escape && !state.input.text.is_empty());
        if !edits {
            return Some(key);
        }
        state.status = None;
        type_into(&mut state, key);
        None
    }

    /// A key for the input box. Enter (or Ctrl+Enter) hands back the message to send, whether
    /// or not the child is working: it never waits in a queue here. A child that takes no
    /// messages keeps the text and says so on the status line.
    pub fn type_key(&self, key: Key) -> Typed {
        let mut state = self.state.lock().unwrap();
        state.status = None;
        if matches!(key, Key::Enter | Key::SendNow) && state.input.paste.is_none() {
            if state.input.text.iter().all(|c| c.is_whitespace()) {
                return Typed::Ignored;
            }
            if !self.sends {
                state.status = Some(CHILD_TAKES_NO_MESSAGES.to_string());
                return Typed::Ignored;
            }
            return Typed::Submitted(state.input.take().trim().to_string());
        }
        type_into(&mut state, key)
    }
}

impl Drop for ChildPane {
    fn drop(&mut self) {
        CHILD_WORKING.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// The goal on the status line: its state and its objective's first line, reversed while
/// selected.
fn goal_label(goal: &goose::providers::base::SessionGoal, selected: bool) -> String {
    let objective = goal.objective.lines().next().unwrap_or_default();
    let text = match goal.status.as_str() {
        "met" => format!("✓ goal met: {objective}"),
        "unmeetable" => format!("✗ goal unmeetable: {objective}"),
        "stalled" => format!("⚠ goal stalled: {objective}"),
        _ => format!("◎ goal: {objective}"),
    };
    if selected {
        style(text).reverse().to_string()
    } else {
        match goal.status.as_str() {
            "met" => style(text).green().to_string(),
            "unmeetable" | "stalled" => style(text).yellow().to_string(),
            _ => style(text).magenta().to_string(),
        }
    }
}

/// The rule over the input box: what the session is doing on its left, the context window on
/// its right.
fn top_rule(state: &State, width: usize) -> String {
    let mut left = Vec::new();
    if let Some(status) = &state.status {
        left.push(style(status.clone()).yellow().to_string());
    }
    let (started, finished) = match &state.router_activity {
        Some(activity) => match &activity.turn {
            Some(SessionTurn::Running(started)) => (Some(*started), None),
            Some(SessionTurn::Finished { elapsed, ended }) => {
                (None, Some(turn_stamp(*elapsed, *ended)))
            }
            None => (None, None),
        },
        None => (state.turn_started, state.finished.clone()),
    };
    if let Some(started) = started {
        let status = match state.paused() {
            Some(reason) => style(format!("⏸ {reason}")).yellow().to_string(),
            None => super::super::output::sapling_green(format!(
                "{} working",
                spinner_frame(started.elapsed())
            )),
        };
        left.push(format!(
            "{status} {}",
            style(format_elapsed(started.elapsed())).dim()
        ));
    } else if let Some(finished) = finished {
        // The last turn's length and end, where `working` was.
        left.push(format!(
            "{} {}",
            super::super::output::sapling_green("⏹"),
            style(finished).dim()
        ));
    }
    let queued = state.queued.len() + usize::from(state.steering.is_some());
    if queued > 0 {
        left.push(style(format!("{queued} queued")).dim().to_string());
    }
    if let Some(goal) = &state.goal {
        left.push(goal_label(goal, state.goal_selected));
    }
    let left = left.join(&style(" · ").dim().to_string());
    let left = if left.is_empty() {
        left
    } else {
        format!("  {left} ")
    };
    let right = state
        .context
        .map(|(used, limit)| format!(" {}", context_bar(used, limit)))
        .unwrap_or_default();
    let right_width = console::measure_text_width(&right);
    // The status gives way to the bar, and always leaves a stretch of rule between them.
    let room = width.saturating_sub(right_width + MIN_RULE);
    let left = console::truncate_str(&left, room, "… ");
    let fill = width.saturating_sub(console::measure_text_width(&left) + right_width);
    format!("{left}{}{right}", style("─".repeat(fill)).dim())
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
    } else if state.goal_selected {
        "goal: Enter edit · Delete remove · ↑ tool calls · Esc back".to_string()
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
                format!(
                    "{} ",
                    style(super::super::output::sapling_green("›")).bold()
                )
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
        let logs = router::recent_logs(&[], true);
        let delegates = source
            .delegates(std::slice::from_ref(root), &logs, QUERY_TIMEOUT)
            .await
            .ok()?;
        rows.extend(router::dock_rows(&delegates, root, chrono::Utc::now()));
    }
    if let Some(agent) = &sources.agent {
        if let Ok(provider) = agent.provider(&sources.session_id).await {
            rows.extend(background_rows(provider.background_tasks()));
        }
    }
    rows.retain(|row| row.status == ChildStatus::Running);
    rows.reverse();
    Some(rows)
}

async fn load_goal(sources: &Sources) -> Option<goose::providers::base::SessionGoal> {
    let agent = sources.agent.as_ref()?;
    let provider = agent.provider(&sources.session_id).await.ok()?;
    provider.session_goal()
}

/// Background work as rows of the children list: its kind where a child shows its model,
/// how long it has run, the command, and the last line it printed. A task that ends leaves
/// the list the way a finished child does.
fn background_rows(tasks: Vec<goose::providers::base::BackgroundTask>) -> Vec<ChildRow> {
    tasks
        .into_iter()
        .map(|task| ChildRow {
            id: format!("{}{}", super::super::background_view::ROW_PREFIX, task.id),
            label: task.command,
            status: ChildStatus::Running,
            model: task.kind,
            started_at: Some(task.started_at),
            activity: task.last_line,
        })
        .collect()
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

    struct ContextProvider(Mutex<Option<(usize, usize)>>);

    #[async_trait::async_trait]
    impl goose::providers::base::Provider for ContextProvider {
        fn get_name(&self) -> &str {
            "context-test"
        }
        fn manages_own_context(&self) -> bool {
            true
        }
        fn context_tokens(&self) -> Option<usize> {
            self.0.lock().unwrap().map(|(used, _)| used)
        }
        async fn get_context_limit(&self, _model: &str, _override_limit: Option<usize>) -> usize {
            self.0.lock().unwrap().map_or(1_000_000, |(_, size)| size)
        }
        async fn stream(
            &self,
            _model: &goose_providers::model::ModelConfig,
            _system: &str,
            _messages: &[goose::conversation::message::Message],
            _tools: &[rmcp::model::Tool],
        ) -> Result<goose::providers::base::MessageStream, goose_providers::errors::ProviderError>
        {
            panic!("context refresh must never prompt the provider")
        }
    }

    #[tokio::test]
    async fn context_refresh_at_the_prompt_tracks_autonomous_turns_and_handoffs() {
        let session = super::super::super::tests::session_with_loader(None, false).await;
        let provider = Arc::new(ContextProvider(Mutex::new(Some((220_000, 1_000_000)))));
        session
            .agent
            .update_provider(
                provider.clone(),
                goose_providers::model::ModelConfig::new("context-test"),
                &session.session_id,
            )
            .await
            .unwrap();
        let dock = typing();
        // No local turn is started. The persistent footer still consumes every update.
        for (report, expected) in [
            (Some((220_000, 1_000_000)), "22% 220k/1.0M"),
            (Some((890_000, 1_000_000)), "89% 890k/1.0M"),
            (Some((40_000, 200_000)), "20% 40k/200k"),
            (Some((0, 200_000)), "0% 0/200k"),
        ] {
            *provider.0.lock().unwrap() = report;
            dock.shared
                .refresh_context(&session.agent, &session.session_id)
                .await;
            let state = dock.shared.state.lock().unwrap();
            assert!(state.turn_started.is_none());
            let lines = painted(&state, 100, 40);
            assert!(lines[0].contains(expected), "{lines:?}");
        }
        dock.shared
            .refresh_context(&session.agent, "missing-session")
            .await;
        assert_eq!(
            dock.shared.state.lock().unwrap().context,
            Some((0, 200_000))
        );
        *provider.0.lock().unwrap() = None;
        dock.shared
            .refresh_context(&session.agent, &session.session_id)
            .await;
        assert_eq!(dock.shared.state.lock().unwrap().context, None);
    }

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
            _accounts: None,
        }
    }

    fn in_turn() -> Dock {
        let dock = typing();
        dock.begin_turn();
        dock
    }

    #[tokio::test]
    async fn router_activity_overrides_local_prompt_completion_and_old_finished_stamps() {
        let dock = in_turn();
        dock.end_turn();
        let started = Instant::now() - Duration::from_secs(600);
        dock.shared.state.lock().unwrap().router_activity = Some(SessionActivity {
            busy: true,
            turn: Some(SessionTurn::Running(started)),
            ..SessionActivity::default()
        });
        // Router wakes do not have a local prompt in flight. Silence does not stop them.
        dock.end_turn();
        let state = dock.shared.state.lock().unwrap();
        let line = &painted(&state, 100, 20)[0];
        assert!(line.contains("working"), "{line}");
        assert!(!line.contains('⏹'));
        assert!(state.working());
        drop(state);

        // Only the router's completed transition makes the footer show a stop time,
        // even if the local request is still unwinding.
        dock.begin_turn();
        let ended = chrono::Local::now();
        dock.shared.state.lock().unwrap().router_activity = Some(SessionActivity {
            turn: Some(SessionTurn::Finished {
                elapsed: Duration::from_secs(600),
                ended,
            }),
            ..SessionActivity::default()
        });
        let state = dock.shared.state.lock().unwrap();
        let line = &painted(&state, 100, 20)[0];
        assert!(
            line.contains(&turn_stamp(Duration::from_secs(600), ended)),
            "{line}"
        );
        assert!(line.contains('⏹') && !line.contains("working"));
        assert!(!state.working());
    }

    fn routed(paused: Option<&str>, started: Instant) -> Option<SessionActivity> {
        Some(SessionActivity {
            busy: true,
            turn: Some(SessionTurn::Running(started)),
            paused: paused.map(str::to_string),
            ..SessionActivity::default()
        })
    }

    #[test]
    fn a_held_turn_shows_why_instead_of_working_and_goes_back_with_the_same_timer() {
        let started = Instant::now() - Duration::from_secs(72);
        let mut state = State {
            context: Some((102_000, 258_000)),
            router_activity: routed(Some("usage cap"), started),
            ..State::default()
        };
        let line = pane(&state, 100, 20)[0].clone();
        let yellow = style("⏸ usage cap").yellow().to_string();
        assert!(line.contains(&yellow), "{line:?}");
        let plain_line = console::strip_ansi_codes(&line).into_owned();
        assert!(
            plain_line.starts_with("  ⏸ usage cap 1m 12s ─") && !plain_line.contains("working"),
            "{plain_line}"
        );
        assert!(
            SPINNER.iter().all(|frame| !plain_line.contains(frame)),
            "{plain_line}"
        );
        // Nothing turns, and the line is due again only when its second moves on.
        assert!(state.working() && !state.spinner_turns());
        assert_eq!(state.paused_second(), Some(72));
        state.router_activity = routed(Some("usage cap"), started - Duration::from_secs(1));
        assert_eq!(state.paused_second(), Some(73));
        assert!(painted(&state, 100, 20)[0].contains("⏸ usage cap 1m 13s"));

        state.router_activity = routed(None, started);
        assert!(painted(&state, 100, 20)[0].contains(" working 1m 12s"));
        assert!(state.spinner_turns());
        assert_eq!(state.paused_second(), None);
    }

    #[test]
    fn a_long_pause_reason_gives_way_to_the_context_bar() {
        let state = State {
            context: Some((160_000, 1_000_000)),
            router_activity: routed(Some(&"x".repeat(40)), Instant::now()),
            ..State::default()
        };
        let line = painted(&state, 60, 20)[0].clone();
        assert!(
            line.contains("… ───") && line.ends_with("16% 160k/1.0M"),
            "{line}"
        );
    }

    #[tokio::test]
    async fn messages_typed_while_the_turn_is_held_stay_queued_until_it_goes_on() {
        let dock = in_turn();
        let started = Instant::now();
        dock.shared.state.lock().unwrap().router_activity = routed(Some("offline"), started);
        typed(&dock, "first");
        assert_eq!(dock.type_key(Key::Enter), Typed::Queued);
        assert_eq!(
            dock.next_steer(),
            None,
            "a held turn is not offered a message"
        );
        assert!(dock.has_queued());
        let line = painted(&dock.shared.state.lock().unwrap(), 100, 20)[0].clone();
        assert!(line.contains("⏸ offline 0s · 1 queued"), "{line}");

        dock.shared.state.lock().unwrap().router_activity = routed(None, started);
        assert_eq!(dock.next_steer().as_deref(), Some("first"));
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
        // The context bar is part of the rule over the input box, not a row of its own.
        assert!(
            lines[0].starts_with("───") && lines[0].ends_with("40% 102k/258k"),
            "{lines:?}"
        );
        assert!(lines[2].starts_with("───"), "{lines:?}");
        assert_eq!(lines[1], "› hi");
        assert!(
            lines[3].contains("2 running") && lines[3].ends_with("↓ children"),
            "{lines:?}"
        );
        assert!(lines[4].contains("opus 5.5") && lines[5].contains("opus 5.5"));
        assert_eq!(lines.len(), 6);
        let styled = pane(&state, 80, 40);
        assert!(
            styled
                .iter()
                .all(|line| console::measure_text_width(line) < 80),
            "the last column stays free: {lines:?}"
        );
    }

    #[tokio::test]
    async fn a_finished_turn_says_how_long_it_ran_and_when_it_stopped() {
        use chrono::TimeZone;
        let at = |h, m| {
            chrono::Local
                .with_ymd_and_hms(2026, 11, 6, h, m, 0)
                .unwrap()
        };
        assert_eq!(
            turn_stamp(Duration::from_secs(5400), at(0, 34)),
            "1h 30m (12:34am)"
        );
        assert_eq!(
            turn_stamp(Duration::from_secs(75), at(14, 5)),
            "1m 15s (2:05pm)"
        );

        let dock = typing();
        dock.begin_turn();
        dock.end_turn();
        let lines = painted(&dock.shared.state.lock().unwrap(), 100, 40);
        assert!(
            lines[0].starts_with("  ⏹ 0s (") && !lines[0].contains("working"),
            "{lines:?}"
        );
        // Ending again (the prompt loop does at every prompt) keeps the turn's stamp.
        let stamp = dock.shared.state.lock().unwrap().finished.clone();
        dock.end_turn();
        assert_eq!(dock.shared.state.lock().unwrap().finished, stamp);
        dock.begin_turn();
        let lines = painted(&dock.shared.state.lock().unwrap(), 100, 40);
        assert!(
            lines[0].contains(" working") && !lines[0].contains('⏹'),
            "{lines:?}"
        );
    }

    #[test]
    fn the_working_spinner_turns_at_the_thinking_spinners_pace() {
        let frames: Vec<&str> = (0..6)
            .map(|n| spinner_frame(Duration::from_millis(n * 100 + 50)))
            .collect();
        assert_eq!(frames, ["◒", "◐", "◓", "◑", "◒", "◐"]);
    }

    #[test]
    fn working_and_the_selected_child_are_the_saplings_green() {
        let green = |text: &str| super::super::super::output::sapling_green(text);
        let state = State {
            rows: vec![row("a", ChildStatus::Running)],
            selected: Some(0),
            turn_started: Some(Instant::now()),
            ..State::default()
        };
        let lines = pane(&state, 80, 40);
        let working = SPINNER
            .iter()
            .any(|frame| lines[0].contains(&green(&format!("{frame} working"))));
        assert!(working && !lines[0].contains('✻'), "{lines:?}");
        assert!(lines[1].contains(&green("›")), "{lines:?}");
        let selected = lines.iter().find(|line| line.contains("opus 5.5")).unwrap();
        let opening = green("").replace("\x1b[39m", "");
        assert!(
            selected.contains(&format!("{opening}› opus 5.5")) && !selected.contains("\x1b[36"),
            "{selected:?}"
        );
    }

    #[test]
    fn a_child_pane_is_the_sessions_input_box_with_nothing_under_it() {
        let child = ChildPane::new(true);
        child.set_turn(Some(Instant::now() - Duration::from_secs(72)));
        child.set_context(Some((102_000, 258_000)));
        for c in "hi".chars() {
            child.type_key(Key::Char(c));
        }
        let lines = plain(&(child.painter())(80, 40));
        // The same rule, timer and context bar as the session's own pane...
        let parent = State {
            turn_started: child.state.lock().unwrap().turn_started,
            context: Some((102_000, 258_000)),
            input: input("hi", 2),
            ..State::default()
        };
        assert_eq!(lines[..], painted(&parent, 80, 40)[..3]);
        assert!(lines[0].contains(" working 1m 12s") && lines[0].ends_with("40% 102k/258k"));
        // ...and no children summary, keys or rows below it.
        assert_eq!(lines.len(), 3, "{lines:?}");
    }

    #[test]
    fn enter_in_a_child_sends_at_once_even_while_it_works() {
        let child = ChildPane::new(true);
        child.set_turn(Some(Instant::now()));
        for c in "go on".chars() {
            child.type_key(Key::Char(c));
        }
        assert_eq!(
            child.type_key(Key::Enter),
            Typed::Submitted("go on".to_string())
        );
        assert!(child.input_is_empty());
        assert_eq!(child.type_key(Key::Enter), Typed::Ignored);
        let lines = plain(&(child.painter())(80, 40));
        assert!(!lines[0].contains("queued"), "{lines:?}");
    }

    #[test]
    fn enter_in_a_subagent_queues_nothing_and_keeps_the_text() {
        let child = ChildPane::new(false);
        child.set_turn(Some(Instant::now()));
        child.type_key(Key::Char('x'));
        assert_eq!(child.type_key(Key::Enter), Typed::Ignored);
        let lines = plain(&(child.painter())(80, 40));
        assert!(lines[0].contains("take no messages"), "{lines:?}");
        assert!(!lines[0].contains("queued"), "{lines:?}");
        assert_eq!(lines[1], "› x");
        child.type_key(Key::Escape);
        assert!(child.input_is_empty());
        let lines = plain(&(child.painter())(80, 40));
        assert!(!lines[0].contains("take no messages"), "{lines:?}");
    }

    #[test]
    fn a_turn_shows_its_time_and_queue_on_the_rule_over_the_input() {
        let mut state = State {
            turn_started: Some(Instant::now() - Duration::from_secs(75)),
            queued: vec!["x".into()],
            status: Some("shutting down 1/3".into()),
            context: Some((160_000, 1_000_000)),
            ..State::default()
        };
        let lines = painted(&state, 120, 40);
        assert!(
            lines[0].contains("shutting down 1/3 · ")
                && lines[0].contains(" working 1m 15s · 1 queued ──")
                && lines[0].ends_with("16% 160k/1.0M"),
            "{lines:?}"
        );
        assert!(
            lines[3].ends_with("Enter queues · Ctrl+Enter interrupts"),
            "{lines:?}"
        );
        // A status too long for the row gives way; the bar and some rule stay.
        let narrow = painted(&state, 60, 40);
        assert!(
            narrow[0].contains("… ───") && narrow[0].ends_with("16% 160k/1.0M"),
            "{narrow:?}"
        );
        state.turn_started = None;
        assert!(painted(&state, 120, 40)[3].ends_with("Enter sends · Ctrl+J newline"));
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
        // Eleven lines: two rules, one input row, the summary and two output rows leave five
        // child slots, the last of them saying what is out of view.
        state.size = (80, 11);
        assert_eq!(child_slots(&state, 80, 11), 5);
        for _ in 0..6 {
            state.selected = next_selection(state.selected, state.rows.len(), 1);
            follow_selection(&mut state);
        }
        assert_eq!((state.selected, state.offset), (Some(5), 2));
        let lines = painted(&state, 80, 11);
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

    /// Waits, without letting the runtime run anything else, until `shown` holds.
    #[cfg(unix)]
    fn blocked_until(what: &str, shown: impl Fn() -> bool) -> Duration {
        let started = Instant::now();
        while !shown() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "never happened: {what}"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        started.elapsed()
    }

    /// The session's one task is stuck in a slow stream item or refresh for the whole test: it
    /// never asks for a key. What is typed still shows, and Enter still queues.
    #[cfg(unix)]
    #[tokio::test]
    async fn typing_shows_at_once_while_the_session_is_blocked() {
        use std::os::fd::AsRawFd;
        let (mut master, slave) = crate::session::child_sessions::terminal::test_terminal();
        let dock = in_turn();
        let mut keys = crate::session::child_sessions::terminal::KeyListener::start_on(
            slave.as_raw_fd(),
            Some(dock.direct_keys()),
        )
        .unwrap();

        let mut slowest = Duration::ZERO;
        let mut text = String::new();
        for c in "a message typed".chars() {
            text.push(c);
            master.write_all(c.to_string().as_bytes()).unwrap();
            let shown = blocked_until("the typed character shows", || dock.input_text() == text);
            slowest = slowest.max(shown);
        }
        println!("slowest key while the session is blocked: {slowest:?}");
        assert!(slowest < Duration::from_secs(1), "{slowest:?}");
        master.write_all(b"\r").unwrap();
        blocked_until("the message is queued", || {
            queued(&dock).0 == ["a message typed"] && dock.input_text().is_empty()
        });
        // ↑ takes it back, also without the session.
        master.write_all(b"\x1b[A").unwrap();
        blocked_until("the message is back", || dock.input_text() == text);
        // The session learns that a message was queued once it looks.
        assert_eq!(keys.next().await, Some(Key::MessageQueued));

        // Tab is the session's. What is typed after it waits until the session handled it, so
        // it can mean what it means in the view Tab opens.
        master.write_all(b"\tx").unwrap();
        assert_eq!(keys.next().await, Some(Key::Open));
        assert_eq!(keys.next().await, Some(Key::Char('x')));
        assert_eq!(dock.input_text(), text);
        keys.handled();
        master.write_all(b"y").unwrap();
        blocked_until("typing shows again", || {
            dock.input_text() == format!("{text}y")
        });
    }

    /// The longest a key takes to show while the task that takes keys does `work` for each
    /// stream item, with the key reader editing the input box itself or without.
    #[cfg(unix)]
    fn key_latency(direct: bool, work: impl Fn() + Send + 'static) -> Duration {
        use std::os::fd::AsRawFd;
        let (mut master, slave) = crate::session::child_sessions::terminal::test_terminal();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (ready, dock) = std::sync::mpsc::channel();
        let stopping = stop.clone();
        let session = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let dock = Arc::new(in_turn());
                    let mut keys = crate::session::child_sessions::terminal::KeyListener::start_on(
                        slave.as_raw_fd(),
                        direct.then(|| dock.direct_keys()),
                    )
                    .unwrap();
                    ready.send(dock.clone()).unwrap();
                    while !stopping.load(std::sync::atomic::Ordering::Relaxed) {
                        tokio::select! {
                            biased;
                            key = keys.next() => { dock.type_key(key.unwrap()); }
                            _ = tokio::task::yield_now() => work(),
                        }
                    }
                });
        });
        let dock = dock.recv().unwrap();
        let mut slowest = Duration::ZERO;
        for n in 1..=4usize {
            std::thread::sleep(Duration::from_millis(37));
            master.write_all(b"k").unwrap();
            let shown = blocked_until("the key shows", || dock.input_text().len() >= n);
            slowest = slowest.max(shown);
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        session.join().unwrap();
        slowest
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "prints measurements"]
    fn bench_key_latency_while_the_session_is_busy() {
        for busy in [0, 100, 1_000, 5_000].map(Duration::from_millis) {
            let session = key_latency(false, move || std::thread::sleep(busy));
            let reader = key_latency(true, move || std::thread::sleep(busy));
            println!("busy {busy:?} per item: shown by the session {session:?}, by the key reader {reader:?}");
        }
    }

    /// The session's task goes through hundreds of thousands of messages again and again, as
    /// opening the tool calls of a whole held conversation did.
    #[cfg(unix)]
    #[test]
    fn typing_shows_at_once_beside_work_on_a_huge_conversation() {
        use goose::conversation::message::Message;
        let messages: Vec<Message> = (0..300_000)
            .map(|n| {
                Message::assistant().with_tool_request(
                    format!("call-{n}"),
                    Ok(rmcp::model::CallToolRequestParams::new("Terminal")),
                )
            })
            .collect();
        let slowest = key_latency(true, move || {
            assert_eq!(
                crate::session::tool_calls::collect(&messages).len(),
                300_000
            );
            std::thread::sleep(Duration::from_millis(1_500));
        });
        assert!(slowest < Duration::from_secs(1), "{slowest:?}");
    }

    #[test]
    fn the_key_reader_leaves_to_the_session_what_only_it_can_do() {
        let mut state = State {
            turn_started: Some(Instant::now()),
            rows: vec![row("a", ChildStatus::Running)],
            ..State::default()
        };
        let press = direct_key;
        for key in [
            Key::Open,
            Key::Interrupt,
            Key::SendNow,
            Key::Activate,
            Key::Up,
        ] {
            assert_eq!(press(&mut state, key), Some(key));
        }
        // A command is the session's, and stays in the box until it took it.
        for c in "/shutdown".chars() {
            assert_eq!(press(&mut state, Key::Char(c)), None);
        }
        assert_eq!(press(&mut state, Key::Enter), Some(Key::Enter));
        assert_eq!(state.input.text.len(), "/shutdown".len());
        assert_eq!(press(&mut state, Key::Escape), None);
        assert!(state.input.text.is_empty());
        // The children list answers the arrows; Enter on a child opens it.
        assert_eq!(press(&mut state, Key::Down), None);
        assert_eq!(state.selected, Some(0));
        assert_eq!(press(&mut state, Key::Enter), Some(Key::Enter));
        assert_eq!(press(&mut state, Key::Up), None);
        assert_eq!(state.selected, None);
        // A goal takes the keys once ↑ reached it.
        state.goal = Some(goal("active"));
        assert_eq!(press(&mut state, Key::Up), None);
        assert!(state.goal_selected);
        assert_eq!(press(&mut state, Key::Delete), Some(Key::Delete));
        state.goal_selected = false;
        // While nothing can be sent, Enter keeps the text for the session to answer.
        state.closed = true;
        press(&mut state, Key::Char('x'));
        assert_eq!(press(&mut state, Key::Enter), Some(Key::Enter));
        assert!(state.queued.is_empty() && state.input.text == ['x']);
        state.closed = false;
        assert_eq!(press(&mut state, Key::Enter), Some(Key::MessageQueued));
        assert_eq!(state.queued, ["x"]);
        // A question on screen takes every key.
        state.question = true;
        assert_eq!(press(&mut state, Key::Char('y')), Some(Key::Char('y')));
        // At the prompt Enter submits and the arrows walk the history: the session's.
        let mut state = State::default();
        press(&mut state, Key::Char('h'));
        for key in [Key::Enter, Key::Up, Key::Down, Key::Eof] {
            assert_eq!(press(&mut state, key), Some(key));
        }
        assert_eq!(state.input.text, ['h']);
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

    /// Background work sits in the children list: its kind where a child shows its model,
    /// the time it has run and its command.
    #[test]
    fn background_work_is_listed_with_its_kind_time_and_command() {
        let rows = background_rows(vec![goose::providers::base::BackgroundTask {
            id: "term-3".into(),
            kind: "background".into(),
            command: "cargo build --release".into(),
            started_at: chrono::Utc::now() - chrono::Duration::minutes(3),
            last_line: "Compiling goose".into(),
        }]);
        let state = State {
            rows,
            ..State::default()
        };
        let lines = painted(&state, 100, 40);
        let row = lines
            .iter()
            .find(|line| line.contains("cargo build"))
            .expect("the task is listed");
        assert!(
            row.contains("background [3m] cargo build --release"),
            "{row}"
        );
        assert!(row.contains("Compiling goose"), "{row}");
        assert_eq!(
            super::super::super::background_view::task_id(&state.rows[0]),
            Some("term-3")
        );
    }

    fn goal(status: &str) -> goose::providers::base::SessionGoal {
        goose::providers::base::SessionGoal {
            objective: "ship the release\nwith smokes green".into(),
            status: status.into(),
        }
    }

    /// The goal sits next to "working" above the input; ↑ selects it, and from there Enter
    /// edits it, Delete removes it, ↑ moves on to the tool calls and Esc goes back.
    #[tokio::test]
    async fn the_goal_shows_by_working_and_up_selects_it() {
        let dock = in_turn();
        assert!(!dock.select_goal(), "no goal to select");
        dock.shared.state.lock().unwrap().goal = Some(goal("active"));
        let lines = painted(&dock.shared.state.lock().unwrap(), 100, 40);
        assert!(
            lines[0].contains(" working 0s · ◎ goal: ship the release"),
            "{lines:?}"
        );
        assert!(!lines[0].contains("smokes"), "its first line: {lines:?}");

        assert!(dock.select_goal());
        let lines = painted(&dock.shared.state.lock().unwrap(), 100, 40);
        assert!(
            lines[3].contains("goal: Enter edit · Delete remove"),
            "{lines:?}"
        );
        assert_eq!(dock.goal_key(Key::Char('x')), GoalKey::Stay);
        assert!(dock.is_goal_selected());
        assert_eq!(
            dock.goal_key(Key::Enter),
            GoalKey::Edit("ship the release\nwith smokes green".into())
        );
        assert!(!dock.is_goal_selected());
        dock.select_goal();
        assert_eq!(dock.goal_key(Key::Delete), GoalKey::Remove);
        dock.select_goal();
        assert_eq!(dock.goal_key(Key::Up), GoalKey::ToolCalls);
        dock.select_goal();
        assert_eq!(dock.goal_key(Key::Escape), GoalKey::Back);

        dock.shared.state.lock().unwrap().goal = Some(goal("met"));
        let lines = painted(&dock.shared.state.lock().unwrap(), 100, 40);
        assert!(
            lines[0].contains("✓ goal met: ship the release"),
            "{lines:?}"
        );
    }

    #[test]
    fn a_router_pane_says_when_no_children_run() {
        let state = State {
            always_list: true,
            ..State::default()
        };
        assert!(painted(&state, 80, 40)[3].contains("no children running"));
    }
}
