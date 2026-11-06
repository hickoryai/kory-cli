//! Read-only navigator for the subagent sessions a parent session delegates to.
//! Opened with Tab while a turn runs, with `/children` between turns, or on a child chosen
//! from the dock (`dock.rs`), the persistent child list under the prompt.
//!
//! A child chosen from either is shown the way the session shows itself: through the relay,
//! its transcript rendered by the session's own renderer and pinned under it the same status
//! rule (working time, context bar) and input box. A child has no children, so nothing is
//! drawn below its input box. Esc on an empty input box goes back.

pub mod dock;
mod keys;
#[cfg(unix)]
mod relay;
#[cfg(not(unix))]
#[path = "relay_unsupported.rs"]
mod relay;
pub mod router;
#[cfg(unix)]
mod terminal;
#[cfg(not(unix))]
#[path = "terminal_unsupported.rs"]
mod terminal;
mod view;

use super::output;
use super::tool_calls::{self, ToolCall, ToolCallsView};
use console::style;
use goose::agents::state_machine::{subagent_states, SubagentState};
use goose::conversation::message::{Message, MessageContent};
use goose::providers::router_acp::{RouterStateSource, DELEGATE_ID_SEPARATOR};
use goose::session::SessionManager;
use goose::utils::safe_truncate;
pub use keys::Key;
pub use relay::{take_activation, tool_call_mark, Target};
use rmcp::model::{ContentBlock, Role};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;
pub use terminal::EchoOff;
/// Raw key input for a view the session drives itself (the dock's selection).
pub use terminal::KeyListener as KeyReader;
/// The alternate screen, for other full-screen views (the tool-call list).
pub use terminal::Screen;
pub use view::ChildRow;
use view::{ChildStatus, LineKind, Navigator, Outcome, Snapshot, TranscriptLine};

pub const REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const TOOL_RESULT_CHARS: usize = 200;

struct OpenView {
    /// The list of children; `None` while a child session is on screen instead.
    screen: Option<terminal::Screen>,
    navigator: Navigator,
    snapshot: Snapshot,
    frame: view::Frame,
    child: Option<ChildScreen>,
    /// Opened on a child from the dock: leaving that child closes the view.
    from_dock: bool,
}

impl OpenView {
    fn list(navigator: Navigator) -> Self {
        let mut view = Self::empty(navigator);
        view.screen = Some(terminal::Screen::enter());
        view
    }

    /// Straight into `child`, chosen from the dock.
    fn child(child: &ChildRow) -> Self {
        let mut navigator = Navigator::default();
        navigator.enter(child);
        let mut view = Self::empty(navigator);
        view.from_dock = true;
        view.show_child(Some(child.clone()));
        if view.child.is_none() {
            view.screen = Some(terminal::Screen::enter());
        }
        view
    }

    fn empty(navigator: Navigator) -> Self {
        Self {
            screen: None,
            navigator,
            snapshot: Snapshot::default(),
            frame: view::Frame::default(),
            child: None,
            from_dock: false,
        }
    }

    /// The child at the top of the path, shown the way the session shows itself, when the relay
    /// hosts the terminal (otherwise the navigator's own transcript view stays).
    fn show_child(&mut self, row: Option<ChildRow>) {
        let Some(id) = self.navigator.current() else {
            return;
        };
        if self.child.is_some() || !relay::is_active() {
            return;
        }
        let row = row.filter(|row| row.id == id).or_else(|| {
            self.snapshot
                .children
                .iter()
                .find(|row| row.id == id)
                .cloned()
        });
        self.child = Some(ChildScreen::new(id.to_string(), row));
        // The list's screen goes; the child is drawn over the transcript from now on.
        self.screen = None;
    }
}

/// A child session on screen, drawn through the relay exactly as the session draws itself: its
/// transcript rendered by the session's own renderer, and the status rule and input box under
/// it, with nothing below them.
struct ChildScreen {
    id: String,
    row: Option<ChildRow>,
    pane: Arc<dock::ChildPane>,
    /// What the transcript was rendered from (and at what width), and the rendering.
    rendered: Option<(u64, String)>,
    /// When the child was first seen running, for a child that does not say when it started.
    seen_running: Option<std::time::Instant>,
    /// The child's tool calls as last loaded, for ↑ and clicks on its tool-call lines.
    calls: Vec<ToolCall>,
    /// The child's tool calls, full screen over it, while open.
    tools: Option<ToolCallsView>,
}

impl ChildScreen {
    fn new(id: String, row: Option<ChildRow>) -> Self {
        Self {
            calls: Vec::new(),
            tools: None,
            // router-acp delegates take messages; goose's own subagents do not.
            pane: dock::ChildPane::new(is_router_delegate(&id)),
            id,
            row,
            rendered: None,
            seen_running: None,
        }
    }

    fn update(&mut self, loaded: ChildTranscript, width: usize) {
        if let Some(row) = loaded.row {
            self.row = Some(row);
        }
        if !loaded.calls.is_empty() {
            if let Some(tools) = self.tools.as_mut() {
                tools.refresh(loaded.calls.clone());
            }
            self.calls = loaded.calls;
        }
        let running = self
            .row
            .as_ref()
            .is_some_and(|row| row.status == ChildStatus::Running);
        let started = running.then(|| {
            let seen = *self
                .seen_running
                .get_or_insert_with(std::time::Instant::now);
            self.row
                .as_ref()
                .and_then(|row| row.started_at)
                .and_then(|started| (chrono::Utc::now() - started).to_std().ok())
                .and_then(|elapsed| std::time::Instant::now().checked_sub(elapsed))
                .unwrap_or(seen)
        });
        if !running {
            self.seen_running = None;
        }
        self.pane.set_turn(started);
        self.pane.set_context(loaded.context);

        let status = self
            .row
            .as_ref()
            .map_or("unknown", |row| row.status.label());
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&(loaded.fingerprint, width, status), &mut hasher);
        let key = std::hash::Hasher::finish(&hasher);
        if self.rendered.as_ref().is_some_and(|(seen, _)| *seen == key) {
            return;
        }
        let (model, label) = self
            .row
            .as_ref()
            .map_or(("unknown model", self.id.as_str()), |row| {
                (row.model.as_str(), row.label.as_str())
            });
        let mut text = output::child_session_header(status, model, label, &self.id);
        text.push_str(&output::render_transcript(&loaded.entries, false));
        if let Some(notice) = loaded.notice {
            text.push_str(&output::render_transcript(
                &[output::TranscriptEntry::Notice(notice)],
                false,
            ));
        }
        self.rendered = Some((key, text));
    }

    fn draw(&self) {
        // The tool-call list has the screen while it is open.
        if self.tools.is_some() {
            return;
        }
        if let Some((_, text)) = &self.rendered {
            relay::show_child(text, self.pane.painter());
        }
    }

    /// The child's tool calls, full screen: on call `id`'s details (a click on its line), or
    /// on the newest call (↑). False when the child has made none.
    fn open_tools(&mut self, id: Option<&str>) -> bool {
        let calls = self.calls.clone();
        self.tools = match id {
            Some(id) => ToolCallsView::open_at(calls, id),
            None => ToolCallsView::open(calls),
        };
        if self.tools.is_none() {
            return false;
        }
        // Over the session's alternate screen, which now holds the list.
        relay::hide_child();
        true
    }

    /// A key for the open tool-call list; once it closes the child is shown again.
    fn tools_key(&mut self, key: Key) {
        let Some(tools) = self.tools.as_mut() else {
            return;
        };
        if !tools.key(key) {
            self.tools = None;
            self.draw();
        }
    }
}

impl Drop for ChildScreen {
    fn drop(&mut self) {
        relay::hide_child();
    }
}

/// A child session's transcript as loaded, with what the pane shows for it.
#[derive(Default)]
struct ChildTranscript {
    row: Option<ChildRow>,
    entries: Vec<output::TranscriptEntry>,
    /// Its tool calls, with the ids its tool-call lines carry.
    calls: Vec<ToolCall>,
    context: Option<(usize, usize)>,
    notice: Option<String>,
    /// Changes whenever the transcript does.
    fingerprint: u64,
}

pub struct ChildSessionNavigator {
    session_manager: Arc<SessionManager>,
    root_id: String,
    available: bool,
    has_children: bool,
    hinted: bool,
    listener: Option<terminal::KeyListener>,
    open: Option<OpenView>,
    router: Option<router::RouterChildren>,
    messenger: Option<Messenger>,
}

/// Sends a message to a router-acp delegate (by its state session id) and answers how
/// router-acp delivered it (`steered`, `input`, `turn`).
pub type Messenger = Arc<
    dyn Fn(String, String) -> futures::future::BoxFuture<'static, Result<String, String>>
        + Send
        + Sync,
>;

/// The status line after a message went out.
fn delivery_status(delivery: &str) -> String {
    match delivery {
        "steered" => "sent into the running turn".into(),
        "input" => "sent · the worker's parent picks it up".into(),
        "turn" => "sent · the child takes a turn on it".into(),
        other => format!("sent ({other})"),
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default()
}

impl ChildSessionNavigator {
    /// `enabled` should only be true for interactive, human-readable output; the terminal
    /// itself must also be capable before any control sequence is written.
    pub fn new(session_manager: Arc<SessionManager>, root_id: String, enabled: bool) -> Self {
        Self {
            session_manager,
            root_id,
            available: enabled && terminal::is_capable(),
            has_children: false,
            hinted: false,
            listener: None,
            open: None,
            router: None,
            messenger: None,
        }
    }

    /// Also lists the delegates of router session `router_session_id`, which live in
    /// router-acp's state rather than goose's.
    pub fn with_router(
        mut self,
        source: anyhow::Result<RouterStateSource>,
        router_session_id: String,
    ) -> Self {
        if self.available {
            self.router = Some(router::RouterChildren::start(source, router_session_id));
        }
        self
    }

    /// How a message typed into a router-acp delegate's input box reaches it.
    pub fn with_messenger(mut self, messenger: Messenger) -> Self {
        self.messenger = Some(messenger);
        self
    }

    /// Sends `text` to child `id` without holding up the keys; the status line says how it
    /// was delivered, or why it was not.
    fn send(&self, id: &str, text: String, pane: Arc<dock::ChildPane>) {
        let Some(messenger) = self.messenger.clone() else {
            pane.set_status(Some("this child takes no messages here".into()));
            return;
        };
        pane.set_status(Some("sending…".into()));
        let id = id.to_string();
        tokio::spawn(async move {
            let status = match messenger(id, text).await {
                Ok(delivery) => delivery_status(&delivery),
                Err(error) => format!("not sent: {}", first_line(&error)),
            };
            pane.set_status(Some(status));
        });
    }

    /// Reads keys for the whole turn, so text can be typed while the agent works.
    pub fn listen_for_input(&mut self) {
        self.has_children = true;
        self.hinted = true;
        self.listen();
    }

    pub fn watches_router(&self) -> bool {
        self.router.is_some()
    }

    /// Offers the navigator once router-acp reports a delegate at work, since router
    /// delegates never show up in goose's own subagent state.
    pub fn check_router(&mut self) {
        if !self.has_children
            && self
                .router
                .as_ref()
                .is_some_and(|router| router.any_running(chrono::Utc::now()))
        {
            self.has_children = true;
            self.listen();
        }
    }

    pub fn observe(&mut self, message: &Message) {
        self.has_children |= delegates_child(message);
        self.listen();
    }

    fn listen(&mut self) {
        if !self.available || !self.has_children || self.listener.is_some() {
            return;
        }
        match terminal::KeyListener::start() {
            Ok(listener) => self.listener = Some(listener),
            Err(_) => {
                self.available = false;
                return;
            }
        }
        if !self.hinted {
            self.hinted = true;
            println!("{}", style("  Press Tab to inspect child sessions.").dim());
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub async fn next_key(&mut self) -> Key {
        match self.listener.as_mut() {
            Some(listener) => match listener.next().await {
                Some(key) => key,
                None => std::future::pending().await,
            },
            None => std::future::pending().await,
        }
    }

    pub async fn handle_key(&mut self, key: Key, turn_active: bool) {
        let Some(view) = self.open.as_mut() else {
            if key == Key::Open {
                self.open = Some(OpenView::list(Navigator::default()));
                self.refresh(turn_active).await;
            }
            return;
        };
        if let Some(child) = view.child.as_mut() {
            // The child's tool-call list takes every key until it closes.
            if child.tools.is_some() {
                child.tools_key(key);
                return;
            }
            match key {
                Key::Open => return self.close(),
                // ↑ on an empty input box, as in the session: the child's tool calls.
                Key::Up if child.pane.input_is_empty() => {
                    child.open_tools(None);
                    return;
                }
                // A click on one of the child's tool-call lines opens that call.
                Key::Activate => {
                    if let Some(relay::Target::ToolCall(id)) = relay::take_activation() {
                        child.open_tools(Some(&id));
                    }
                    return;
                }
                // Esc on an empty input box leaves the child: back to the dock it was chosen
                // from, or to the list.
                Key::Escape if child.pane.input_is_empty() => {
                    if view.from_dock {
                        return self.close();
                    }
                    view.screen = Some(terminal::Screen::enter());
                    view.navigator.ascend();
                    view.child = None;
                }
                key => {
                    let typed = child.pane.type_key(key);
                    let (id, pane) = (child.id.clone(), child.pane.clone());
                    if let dock::Typed::Submitted(text) = typed {
                        self.send(&id, text, pane);
                    }
                    relay::repaint();
                    return;
                }
            }
        } else if view.navigator.handle(key, &view.snapshot, &view.frame) == Outcome::Close {
            return self.close();
        } else {
            view.show_child(None);
        }
        self.refresh(turn_active).await;
    }

    pub async fn refresh(&mut self, turn_active: bool) {
        let Some(view) = self.open.as_mut() else {
            return;
        };
        if let Some(router) = &self.router {
            let router_path: Vec<&str> = view
                .navigator
                .path_ids()
                .into_iter()
                .filter(|id| is_router_delegate(id))
                .collect();
            router.follow(&router_path);
        }
        if let Some(child) = view.child.as_mut() {
            let loaded = load_child(
                &self.session_manager,
                &self.root_id,
                self.router.as_ref(),
                &child.id,
                turn_active,
            )
            .await;
            let (width, _) = terminal_size();
            child.update(loaded, width);
            child.draw();
            return;
        }
        if let Ok(snapshot) = load_snapshot(
            &self.session_manager,
            &self.root_id,
            self.router.as_ref(),
            view.navigator.current(),
            turn_active,
        )
        .await
        {
            view.snapshot = snapshot;
        }
        if let Some(screen) = &view.screen {
            let (width, height) = screen.size();
            view.frame = view.navigator.render(&view.snapshot, width, height);
            screen.draw(&view.frame.lines);
        }
    }

    pub fn close(&mut self) {
        self.open = None;
    }

    /// Hands stdin back to prompts that read it themselves; listening resumes on the next
    /// observed message.
    pub fn suspend(&mut self) {
        self.open = None;
        self.listener = None;
    }

    /// Opens the navigator inside `child` (chosen from the dock) while a turn runs.
    pub async fn open_at(&mut self, child: &ChildRow, turn_active: bool) {
        if !self.available {
            return;
        }
        self.open = Some(OpenView::child(child));
        self.refresh(turn_active).await;
    }

    /// `browse`, starting inside `child`.
    pub async fn browse_at(&mut self, child: &ChildRow) {
        if !self.available {
            return;
        }
        self.has_children = true;
        self.hinted = true;
        self.listen();
        self.open_at(child, false).await;
        let mut tick = tokio::time::interval(REFRESH_INTERVAL);
        while self.is_open() {
            tokio::select! {
                key = self.next_key() => self.handle_key(key, false).await,
                _ = tick.tick() => self.refresh(false).await,
            }
        }
        self.suspend();
    }

    /// Opens the navigator between turns, when completed and failed children stay inspectable.
    pub async fn browse(&mut self) {
        if !self.available {
            println!(
                "{}",
                style("Child sessions can only be browsed in an interactive terminal.").dim()
            );
            return;
        }
        self.has_children = true;
        self.hinted = true;
        self.listen();
        self.handle_key(Key::Open, false).await;
        let mut tick = tokio::time::interval(REFRESH_INTERVAL);
        while self.is_open() {
            tokio::select! {
                key = self.next_key() => self.handle_key(key, false).await,
                _ = tick.tick() => self.refresh(false).await,
            }
        }
        self.suspend();
    }
}

fn delegates_child(message: &Message) -> bool {
    message.content.iter().any(|content| {
        let MessageContent::ToolResponse(response) = content else {
            return false;
        };
        response.tool_result.as_ref().is_ok_and(|result| {
            result
                .meta
                .as_ref()
                .is_some_and(|meta| meta.0.contains_key("subagent_session_id"))
        })
    })
}

fn is_router_delegate(id: &str) -> bool {
    id.contains(DELEGATE_ID_SEPARATOR)
}

async fn load_snapshot(
    session_manager: &SessionManager,
    root_id: &str,
    router: Option<&router::RouterChildren>,
    current: Option<&str>,
    turn_active: bool,
) -> anyhow::Result<Snapshot> {
    let mut snapshot = match current {
        Some(id) if is_router_delegate(id) => Snapshot {
            transcript: Some(Vec::new()),
            ..Default::default()
        },
        _ => load_goose_snapshot(session_manager, root_id, current, turn_active).await?,
    };
    let Some(router) = router else {
        return Ok(snapshot);
    };
    let delegates = match router.latest() {
        router::Fetched::Ready(delegates) => delegates,
        router::Fetched::Pending => {
            snapshot.notice = Some("Loading router-acp delegates…".to_string());
            return Ok(snapshot);
        }
        router::Fetched::Failed(error) => {
            snapshot.notice = Some(format!("router-acp delegates unavailable: {error}"));
            return Ok(snapshot);
        }
    };
    let now = chrono::Utc::now();
    match current {
        None => snapshot
            .children
            .extend(router::rows_for(&delegates, router.root(), now)),
        Some(id) if is_router_delegate(id) => {
            snapshot.children = router::rows_for(&delegates, id, now);
            match delegates.iter().find(|delegate| delegate.id == id) {
                Some(delegate) => {
                    snapshot.transcript = Some(router::transcript_lines(&delegate.log))
                }
                None => snapshot.notice = Some("router-acp no longer lists this delegate".into()),
            }
        }
        Some(_) => {}
    }
    Ok(snapshot)
}

/// A child session's transcript and its row in the parent's list, for showing it the way the
/// parent shows itself.
async fn load_child(
    session_manager: &SessionManager,
    root_id: &str,
    router: Option<&router::RouterChildren>,
    id: &str,
    turn_active: bool,
) -> ChildTranscript {
    let mut loaded = ChildTranscript::default();
    if let Ok(snapshot) = load_snapshot(session_manager, root_id, router, None, turn_active).await {
        loaded.row = snapshot.children.into_iter().find(|row| row.id == id);
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if is_router_delegate(id) {
        match router.map(router::RouterChildren::latest) {
            Some(router::Fetched::Ready(delegates)) => {
                match delegates.iter().find(|delegate| delegate.id == id) {
                    Some(delegate) => {
                        let (lines, calls) = router::transcript(&delegate.log);
                        loaded.entries = router_entries(lines, &calls);
                        loaded.calls = calls;
                        delegate.log.len().hash(&mut hasher);
                        if let Some(last) = delegate.log.last() {
                            (&last.kind, &last.summary, last.ts).hash(&mut hasher);
                        }
                    }
                    None => loaded.notice = Some("router-acp no longer lists this delegate".into()),
                }
            }
            Some(router::Fetched::Pending) => {
                loaded.notice = Some("Loading router-acp delegates…".into())
            }
            Some(router::Fetched::Failed(error)) => {
                loaded.notice = Some(format!("router-acp delegates unavailable: {error}"))
            }
            None => loaded.notice = Some("router-acp delegates unavailable".into()),
        }
    } else {
        match session_manager.get_session(id, true).await {
            Ok(session) => {
                let messages = session
                    .conversation
                    .map(|conversation| conversation.messages().clone())
                    .unwrap_or_default();
                messages.len().hash(&mut hasher);
                if let Some(last) = messages.last() {
                    serde_json::to_string(last)
                        .unwrap_or_default()
                        .hash(&mut hasher);
                }
                loaded.context = session
                    .usage
                    .total_tokens
                    .zip(session.model_config.as_ref())
                    .map(|(used, config)| (used.max(0) as usize, config.context_limit()));
                loaded.calls = tool_calls::collect(&messages);
                loaded.entries = messages
                    .into_iter()
                    .map(output::TranscriptEntry::Message)
                    .collect();
            }
            Err(error) => loaded.notice = Some(format!("child session unavailable: {error}")),
        }
    }
    loaded.notice.hash(&mut hasher);
    loaded.fingerprint = hasher.finish();
    loaded
}

/// A router-acp delegate's log, read as a transcript, in the entries a session renders: its
/// tasks as submitted prompts, its streamed text and response as replies, its tool calls
/// (the n-th tool-call line carrying the id of `calls[n]`).
fn router_entries(lines: Vec<TranscriptLine>, calls: &[ToolCall]) -> Vec<output::TranscriptEntry> {
    let mut entries = Vec::new();
    let mut call_ids = calls.iter().map(|call| call.id.clone());
    let mut speaker = String::new();
    let mut text = String::new();
    let flush = |entries: &mut Vec<output::TranscriptEntry>, speaker: &str, text: &mut String| {
        let body = std::mem::take(text);
        let body = body.trim_matches('\n');
        if body.trim().is_empty() {
            return;
        }
        let message = if speaker == "task" {
            Message::user()
        } else {
            Message::assistant()
        };
        entries.push(output::TranscriptEntry::Message(message.with_text(body)));
    };
    for line in lines {
        match line.kind {
            LineKind::Heading => {
                flush(&mut entries, &speaker, &mut text);
                speaker = line.text;
            }
            LineKind::Body => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&line.text);
            }
            LineKind::Tool => {
                flush(&mut entries, &speaker, &mut text);
                entries.push(match line.text.strip_prefix("→ ") {
                    Some(call) => output::TranscriptEntry::ToolCall {
                        summary: call.to_string(),
                        id: call_ids.next(),
                    },
                    None => output::TranscriptEntry::Notice(
                        line.text.trim_start_matches("· ").to_string(),
                    ),
                });
            }
        }
    }
    flush(&mut entries, &speaker, &mut text);
    entries
}

fn terminal_size() -> (usize, usize) {
    let (rows, cols) = console::Term::stdout().size();
    (cols as usize, rows as usize)
}

/// A session's own goose subagents as top-level rows, for the dock.
pub(super) async fn goose_children(
    session_manager: &SessionManager,
    session_id: &str,
    turn_active: bool,
) -> anyhow::Result<Vec<ChildRow>> {
    Ok(
        load_goose_snapshot(session_manager, session_id, None, turn_active)
            .await?
            .children,
    )
}

async fn load_goose_snapshot(
    session_manager: &SessionManager,
    root_id: &str,
    current: Option<&str>,
    turn_active: bool,
) -> anyhow::Result<Snapshot> {
    let id = current.unwrap_or(root_id);
    let session = session_manager.get_session(id, true).await?;
    let messages = session
        .conversation
        .map(|conversation| conversation.messages().clone())
        .unwrap_or_default();
    let states = subagent_states(&messages);
    let children = session_manager
        .list_child_sessions(id)
        .await?
        .into_iter()
        .map(|child| ChildRow {
            status: child_status(states.get(&child.id).copied(), turn_active),
            model: child
                .model_config
                .map(|config| config.model_name)
                .unwrap_or_else(|| "unknown model".to_string()),
            started_at: Some(child.created_at),
            label: child.name,
            activity: child.last_message_snippet.unwrap_or_default(),
            id: child.id,
        })
        .collect();
    Ok(Snapshot {
        children,
        transcript: current.map(|_| transcript_lines(&messages)),
        notice: None,
    })
}

/// A child the parent still counts as running is only actually running while the parent's
/// turn drives it; between turns it is stopped.
fn child_status(state: Option<SubagentState>, turn_active: bool) -> ChildStatus {
    match state {
        Some(SubagentState::Completed) => ChildStatus::Completed,
        Some(SubagentState::Failed) => ChildStatus::Failed,
        Some(SubagentState::Cancelled) => ChildStatus::Cancelled,
        Some(SubagentState::Running) | None if turn_active => ChildStatus::Running,
        Some(SubagentState::Running) | None => ChildStatus::Stopped,
    }
}

fn transcript_lines(messages: &[Message]) -> Vec<TranscriptLine> {
    let line = |kind, text: String| TranscriptLine { kind, text };
    let mut lines = Vec::new();
    for message in messages.iter().filter(|message| message.is_user_visible()) {
        let mut body = Vec::new();
        for content in &message.content {
            match content {
                MessageContent::Text(text) if !text.text.trim().is_empty() => {
                    body.push(line(LineKind::Body, text.text.clone()))
                }
                MessageContent::ToolRequest(request) => {
                    if let Ok(call) = &request.tool_call {
                        body.push(line(LineKind::Tool, format!("→ {}", call.name)));
                    }
                }
                MessageContent::ToolResponse(response) => {
                    let summary = match &response.tool_result {
                        Ok(result) => result
                            .content
                            .iter()
                            .find_map(|block| match block {
                                ContentBlock::Text(text) => text.text.lines().next(),
                                _ => None,
                            })
                            .unwrap_or("done")
                            .to_string(),
                        Err(error) => format!("error: {}", error.message),
                    };
                    body.push(line(
                        LineKind::Tool,
                        format!("← {}", safe_truncate(&summary, TOOL_RESULT_CHARS)),
                    ));
                }
                MessageContent::Error(error) => {
                    body.push(line(LineKind::Body, format!("error: {}", error.message)))
                }
                _ => {}
            }
        }
        if body.is_empty() {
            continue;
        }
        let speaker = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        lines.push(line(LineKind::Heading, speaker.to_string()));
        lines.extend(body);
        lines.push(line(LineKind::Body, String::new()));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_children_are_stopped_between_turns() {
        assert_eq!(
            child_status(Some(SubagentState::Running), true),
            ChildStatus::Running
        );
        assert_eq!(
            child_status(Some(SubagentState::Running), false),
            ChildStatus::Stopped
        );
        assert_eq!(child_status(None, false), ChildStatus::Stopped);
        assert_eq!(
            child_status(Some(SubagentState::Failed), true),
            ChildStatus::Failed
        );
    }

    #[test]
    fn transcript_shows_visible_text_and_tool_activity() {
        let hidden = Message::user()
            .with_text("internal")
            .with_visibility(false, true);
        let messages = vec![
            Message::user().with_text("Subagent task"),
            hidden,
            Message::assistant().with_text("Working on it"),
        ];

        let lines: Vec<(LineKind, String)> = transcript_lines(&messages)
            .into_iter()
            .map(|line| (line.kind, line.text))
            .collect();

        assert_eq!(
            lines,
            [
                (LineKind::Heading, "user".to_string()),
                (LineKind::Body, "Subagent task".to_string()),
                (LineKind::Body, String::new()),
                (LineKind::Heading, "assistant".to_string()),
                (LineKind::Body, "Working on it".to_string()),
                (LineKind::Body, String::new()),
            ]
        );
    }

    #[test]
    fn a_child_screen_opens_like_a_session_and_its_pane_has_nothing_under_the_input() {
        let row = ChildRow {
            id: "child-1".into(),
            label: "jam-packet-map".into(),
            status: ChildStatus::Running,
            model: "opus 5.5".into(),
            started_at: Some(chrono::Utc::now() - chrono::Duration::seconds(75)),
            activity: String::new(),
        };
        let mut child = ChildScreen::new("child-1".into(), Some(row));
        let loaded = || ChildTranscript {
            entries: vec![
                output::TranscriptEntry::Message(Message::user().with_text("Map the packets")),
                output::TranscriptEntry::Message(Message::assistant().with_text("Mapped.\n")),
            ],
            context: Some((1_000, 10_000)),
            fingerprint: 7,
            ..Default::default()
        };
        child.update(loaded(), 100);
        let text = console::strip_ansi_codes(&child.rendered.as_ref().unwrap().1).into_owned();
        assert!(text.contains("child session · running"), "{text}");
        assert!(text.contains("child-1 · jam-packet-map"), "{text}");
        assert!(text.contains("\n› Map the packets\n"), "{text}");
        assert!(text.contains("Mapped."), "{text}");

        let pane: Vec<String> = (child.pane.painter())(100, 30)
            .iter()
            .map(|line| console::strip_ansi_codes(line).trim_end().to_string())
            .collect();
        assert_eq!(pane.len(), 3, "{pane:?}");
        assert!(pane[0].contains(" working 1m 1"), "{pane:?}");
        assert!(pane[0].ends_with("10% 1k/10k"), "{pane:?}");
        assert_eq!(pane[1], "›");

        // The same transcript is not rendered again.
        let before = child.rendered.as_ref().unwrap().0;
        child.update(loaded(), 100);
        assert_eq!(child.rendered.as_ref().unwrap().0, before);
    }

    #[test]
    fn a_router_delegates_log_becomes_the_entries_a_session_renders() {
        let line = |kind, text: &str| TranscriptLine {
            kind,
            text: text.to_string(),
        };
        let entries = router_entries(
            vec![
                line(LineKind::Heading, "task"),
                line(LineKind::Body, "Map the packets"),
                line(LineKind::Tool, "… 3 more lines"),
                line(LineKind::Body, String::new().as_str()),
                line(LineKind::Heading, "assistant"),
                line(LineKind::Body, "Reading."),
                line(LineKind::Tool, "→ Read file · completed"),
                line(LineKind::Body, "Done."),
            ],
            &[],
        );
        let shown: Vec<String> = entries.iter().map(shown).collect();
        assert_eq!(
            shown,
            [
                "User: Map the packets",
                "notice: … 3 more lines",
                "Assistant: Reading.",
                "tool: Read file · completed (None)",
                "Assistant: Done.",
            ]
        );
    }

    fn shown(entry: &output::TranscriptEntry) -> String {
        match entry {
            output::TranscriptEntry::Message(message) => {
                format!("{:?}: {}", message.role, message.as_concat_text())
            }
            output::TranscriptEntry::ToolCall { summary, id } => {
                format!("tool: {summary} ({id:?})")
            }
            output::TranscriptEntry::Notice(notice) => format!("notice: {notice}"),
        }
    }

    #[test]
    fn a_delegates_tool_call_lines_name_the_calls_its_list_opens() {
        let log: Vec<goose::providers::router_acp::RouterDelegateLog> =
            serde_json::from_value(serde_json::json!([
                {"kind": "delegate_task", "summary": "Map the packets"},
                {"kind": "tool_call", "summary": "Read file [pending]", "detail": {
                    "toolCallId": "toolu_01A2b3C4d5", "title": "Read file", "kind": "read",
                    "rawInput": {"path": "src/jam.rs"}, "status": "pending"}},
                {"kind": "tool_call", "summary": "toolu_01A2b3C4d5 [completed]", "detail": {
                    "toolCallId": "toolu_01A2b3C4d5", "status": "completed",
                    "content": [{"type": "content", "content": {"type": "text", "text": "fn jam() {}"}}]}},
                {"kind": "tool_call", "summary": "Run tests [failed]", "detail": {
                    "toolCallId": "toolu_2", "title": "Run tests", "status": "failed",
                    "content": [{"type": "content", "content": {"type": "text", "text": "1 failed"}}]}},
            ]))
            .unwrap();
        let (lines, calls) = router::transcript(&log);
        let entries = router_entries(lines, &calls);
        let tools: Vec<String> = entries
            .iter()
            .filter(|entry| matches!(entry, output::TranscriptEntry::ToolCall { .. }))
            .map(shown)
            .collect();
        assert_eq!(
            tools,
            [
                format!("tool: Read file · completed ({:?})", Some(&calls[0].id)),
                format!("tool: Run tests · failed ({:?})", Some(&calls[1].id)),
            ]
        );
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].status, tool_calls::CallStatus::Done);
        assert_eq!(calls[0].kind.as_deref(), Some("read"));
        assert_eq!(calls[0].output, ["fn jam() {}"]);
        assert!(calls[0].arguments.as_ref().unwrap().contains_key("path"));
        assert_eq!(calls[1].status, tool_calls::CallStatus::Failed);
        assert_eq!(calls[1].error.as_deref(), Some("1 failed"));
    }

    #[tokio::test]
    async fn navigator_stays_inert_without_a_capable_terminal() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let session_manager = Arc::new(SessionManager::new(temp_dir.path().to_path_buf()));
        let mut navigator = ChildSessionNavigator::new(session_manager, "root".into(), false);
        let mut delegate = Message::user();
        let mut meta = rmcp::model::MetaObject::new();
        meta.0
            .insert("subagent_session_id".into(), serde_json::json!("child"));
        delegate.content.push(MessageContent::tool_response(
            "call",
            Ok(rmcp::model::CallToolResult::success(vec![]).with_meta(Some(meta))),
        ));
        assert!(delegates_child(&delegate));

        navigator.observe(&delegate);

        assert!(navigator.listener.is_none());
        assert!(!navigator.is_open());
    }
}
