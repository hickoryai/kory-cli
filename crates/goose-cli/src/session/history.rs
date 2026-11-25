//! The conversation as the CLI keeps it: what its views show and what a turn just said. A
//! router session's conversation belongs to the router and is stored whole in goose's
//! database; here only its newest part is held, so nothing the prompt does costs more as the
//! session grows. Any other session is held whole.

use super::shutdown;
use goose::conversation::message::Message;
use goose::conversation::Conversation;
use goose::session::session_manager::MessageCursor;
use goose::session::SessionManager;
use std::ops::Deref;
use std::sync::Arc;

/// Messages a router session keeps at least, when it has that many: enough to fill the
/// transcript's scrollback.
const WINDOW: usize = 20_000;
/// Held beyond this, the oldest go until `WINDOW` are left.
const WINDOW_LIMIT: usize = 30_000;
/// Held messages looked up in the database to find where the older ones start.
const ANCHORS: usize = 50;

#[derive(Default)]
pub struct History {
    conversation: Conversation,
    /// Only the newest messages are held.
    windowed: bool,
    /// Older messages are stored and not held.
    older: bool,
    /// A message that opts in to router-acp's planner is held / was before the held ones.
    planner: bool,
    planner_before: bool,
}

impl Deref for History {
    type Target = Conversation;

    fn deref(&self) -> &Conversation {
        &self.conversation
    }
}

impl History {
    pub async fn load(
        session_manager: &SessionManager,
        session_id: &str,
        windowed: bool,
    ) -> anyhow::Result<Self> {
        if !windowed {
            let session = session_manager.get_session(session_id, true).await?;
            return Ok(session.conversation.unwrap_or_default().into());
        }
        session_manager.get_session_settings(session_id).await?;
        let page = session_manager
            .messages_before(session_id, None, WINDOW)
            .await?;
        let mut history = Self::from(Conversation::new_unvalidated(page.messages));
        history.windowed = true;
        history.older = page.older.is_some();
        if history.older && !history.planner {
            history.planner_before = session_manager
                .any_user_text(
                    session_id,
                    page.older,
                    &shutdown::PLANNER_COMMANDS,
                    shutdown::starts_planner,
                )
                .await?;
        }
        Ok(history)
    }

    pub fn push(&mut self, message: Message) {
        self.planner |= shutdown::starts_planner(&message);
        self.conversation.push(message);
        if self.windowed && self.conversation.len() > WINDOW_LIMIT {
            let excess = self.conversation.len() - WINDOW;
            let dropped = self.conversation.messages_mut().drain(..excess);
            self.planner_before |= dropped.as_slice().iter().any(shutdown::starts_planner);
            drop(dropped);
            self.older = true;
            self.count_planner();
        }
    }

    pub fn pop(&mut self) {
        self.conversation.pop();
        self.count_planner();
    }

    pub fn truncate(&mut self, len: usize) {
        self.conversation.truncate(len);
        self.count_planner();
    }

    pub fn clear(&mut self) {
        self.replace(Conversation::default());
    }

    /// The whole conversation is now this one.
    pub fn replace(&mut self, conversation: Conversation) {
        let windowed = self.windowed;
        *self = conversation.into();
        self.windowed = windowed;
    }

    /// Whether the user opted the session in to router-acp's planner.
    pub fn is_planner(&self) -> bool {
        self.planner || self.planner_before
    }

    pub fn has_older(&self) -> bool {
        self.older
    }

    /// Where the stored messages before the held ones are.
    pub fn older(&self, session_manager: Arc<SessionManager>, session_id: &str) -> Option<Older> {
        self.older.then(|| Older {
            session_manager,
            session_id: session_id.to_string(),
            anchors: self
                .conversation
                .iter()
                .filter_map(|message| message.id.clone())
                .take(ANCHORS)
                .collect(),
            cursor: None,
        })
    }

    fn count_planner(&mut self) {
        self.planner = self.conversation.iter().any(shutdown::starts_planner);
    }
}

impl From<Conversation> for History {
    fn from(conversation: Conversation) -> Self {
        let mut history = Self {
            conversation,
            ..Self::default()
        };
        history.count_planner();
        history
    }
}

/// Reads back through the messages before the held ones.
#[derive(Clone)]
pub struct Older {
    session_manager: Arc<SessionManager>,
    session_id: String,
    /// The oldest held messages' ids. One still being said is not stored yet.
    anchors: Vec<String>,
    cursor: Option<MessageCursor>,
}

impl Older {
    /// The `limit` messages before the ones read so far, and the reader for those before them.
    pub async fn page(mut self, limit: usize) -> anyhow::Result<(Vec<Message>, Option<Self>)> {
        if self.cursor.is_none() {
            for id in &self.anchors {
                self.cursor = self
                    .session_manager
                    .message_cursor(&self.session_id, id)
                    .await?;
                if self.cursor.is_some() {
                    break;
                }
            }
        }
        let page = self
            .session_manager
            .messages_before(&self.session_id, self.cursor, limit)
            .await?;
        self.cursor = page.older;
        Ok((page.messages, self.cursor.is_some().then_some(self)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(n: usize) -> Message {
        Message::assistant()
            .with_text(format!("reply {n}"))
            .with_id(format!("m{n}"))
    }

    #[test]
    fn a_router_session_holds_a_bounded_part_of_its_conversation() {
        let mut history = History {
            windowed: true,
            ..History::default()
        };
        history.push(Message::user().with_text("/implement Docs/Plans/X.md"));
        let mut most = 0;
        for n in 0..400_000 {
            history.push(said(n));
            most = most.max(history.len());
        }
        assert!(most <= WINDOW_LIMIT, "{most}");
        assert!(history.len() >= WINDOW);
        assert_eq!(history.last().unwrap().id.as_deref(), Some("m399999"));
        assert!(history.has_older());
        assert!(
            history.is_planner(),
            "the opt-in outlives the message that gave it"
        );
        // Streamed pieces of one message still merge into it.
        let held = history.len();
        history.push(Message::assistant().with_text(" more").with_id("m399999"));
        assert_eq!(history.len(), held);
        assert_eq!(
            history.last().unwrap().as_concat_text(),
            "reply 399999 more"
        );
    }

    #[test]
    #[ignore = "prints measurements"]
    fn bench_prompt_work_in_a_session_of_440_000_messages() {
        use super::super::{input, tool_calls};
        use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
        use std::time::Instant;
        let mut whole = History::default();
        let mut windowed = History {
            windowed: true,
            ..History::default()
        };
        for n in 0..220_000 {
            let id = format!("call-{n}");
            let request = Message::assistant()
                .with_tool_request(&id, Ok(CallToolRequestParams::new("Terminal")));
            let output = vec![ContentBlock::text("x".repeat(1_000))];
            let response =
                Message::user().with_tool_response(&id, Ok(CallToolResult::success(output)));
            for message in [request, response] {
                whole.push(message.clone());
                windowed.push(message);
            }
        }
        let timed = |what: &str, work: &dyn Fn() -> usize| {
            let started = Instant::now();
            let size = work();
            println!("{what}: {:?} ({size})", started.elapsed());
        };
        for (name, history) in [("whole", &whole), ("windowed", &windowed)] {
            println!("{name}: {} messages", history.len());
            timed("  editor text (was built at every prompt)", &|| {
                input::conversation_strings(history).len()
            });
            timed("  planner scan (was run at every prompt and turn)", &|| {
                usize::from(history.iter().any(shutdown::starts_planner))
            });
            timed("  planner flag", &|| usize::from(history.is_planner()));
            timed(
                "  tool calls (↑, click, each message while listed)",
                &|| tool_calls::collect(history.messages()).len(),
            );
            timed("  whether there are tool calls", &|| {
                usize::from(tool_calls::has_tool_calls(history.messages()))
            });
        }
        let started = Instant::now();
        let pushes = 100_000;
        for n in 0..pushes {
            windowed.push(said(n));
        }
        println!(
            "windowed push: {:?} each",
            started.elapsed() / pushes as u32
        );
    }

    #[tokio::test]
    #[ignore = "prints measurements"]
    async fn bench_loading_a_session_of_440_000_messages() {
        use rmcp::model::{CallToolResult, ContentBlock};
        use std::time::Instant;
        let temp_dir = tempfile::TempDir::new().unwrap();
        let sm = SessionManager::new(temp_dir.path().to_path_buf());
        let id = sm
            .create_session(
                temp_dir.path().to_path_buf(),
                "long".to_string(),
                goose::session::session_manager::SessionType::User,
                goose::config::GooseMode::default(),
            )
            .await
            .unwrap()
            .id;
        let messages = (0..440_000).map(|n| match n % 2 {
            0 => said(n),
            _ => Message::user().with_tool_response(
                format!("call-{n}"),
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    "x".repeat(1_500),
                )])),
            ),
        });
        sm.replace_conversation(&id, &Conversation::new_unvalidated(messages))
            .await
            .unwrap();
        let started = Instant::now();
        let windowed = History::load(&sm, &id, true).await.unwrap();
        println!(
            "windowed: {} messages in {:?}",
            windowed.len(),
            started.elapsed()
        );
        let started = Instant::now();
        let older = windowed.older(Arc::new(sm), &id).unwrap();
        let page = older.clone().page(2_000).await.unwrap().0;
        println!("older page of {}: {:?}", page.len(), started.elapsed());
        drop((windowed, page));
        let started = Instant::now();
        let whole = History::load(&older.session_manager, &id, false)
            .await
            .unwrap();
        println!("whole: {} messages in {:?}", whole.len(), started.elapsed());
    }

    #[test]
    fn any_other_session_keeps_everything() {
        let mut history = History::default();
        for n in 0..WINDOW_LIMIT + 10 {
            history.push(said(n));
        }
        assert_eq!(history.len(), WINDOW_LIMIT + 10);
        assert!(!history.has_older() && !history.is_planner());
    }

    #[test]
    fn taking_the_opt_in_back_takes_it_back() {
        let mut history = History::default();
        history.push(Message::user().with_text("/implement"));
        assert!(history.is_planner());
        history.pop();
        assert!(!history.is_planner());
    }

    #[tokio::test]
    async fn a_long_session_loads_its_newest_part_and_reads_the_rest_on_demand() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let sm = Arc::new(SessionManager::new(temp_dir.path().to_path_buf()));
        let id = sm
            .create_session(
                temp_dir.path().to_path_buf(),
                "long".to_string(),
                goose::session::session_manager::SessionType::User,
                goose::config::GooseMode::default(),
            )
            .await
            .unwrap()
            .id;
        let mut all = vec![Message::user()
            .with_text("/implement Docs/Plans/X.md")
            .with_id("start")];
        all.extend((0..WINDOW + 500).map(said));
        sm.replace_conversation(&id, &Conversation::new_unvalidated(all))
            .await
            .unwrap();

        let history = History::load(&sm, &id, true).await.unwrap();
        assert_eq!(history.len(), WINDOW);
        assert!(history.is_planner(), "found among the messages not held");
        let older = history.older(sm.clone(), &id).unwrap();
        let (page, older) = older.page(400).await.unwrap();
        assert_eq!(page.len(), 400);
        assert_eq!(page.last().unwrap().id.as_deref(), Some("m499"));
        let (page, older) = older.unwrap().page(400).await.unwrap();
        assert_eq!(page.first().unwrap().id.as_deref(), Some("start"));
        assert!(older.is_none());

        let whole = History::load(&sm, &id, false).await.unwrap();
        assert_eq!(whole.len(), WINDOW + 501);
        assert!(!whole.has_older() && whole.is_planner());
    }
}
