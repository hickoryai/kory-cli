use super::*;
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

const CHILD: &str = "GOOSE_DOCK_TERMINAL_TEST";

struct Terminal {
    child: Child,
    master: File,
    parser: vt100::Parser,
    bytes: Vec<u8>,
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Terminal {
    fn start(mode: &str) -> Self {
        let mut master = -1;
        let mut slave = -1;
        let mut size = libc::winsize {
            ws_row: 30,
            ws_col: 100,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // The descriptors are initialized by openpty and owned by File after success.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut size,
                )
            },
            0
        );
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        assert_eq!(
            unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "session::child_sessions::dock::terminal_tests::terminal_child",
                "--nocapture",
            ])
            .env(CHILD, mode)
            .env("TERM", "xterm-256color")
            .env("CLICOLOR_FORCE", "1")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Self {
            child: command.spawn().unwrap(),
            master,
            parser: vt100::Parser::new(30, 100, 100),
            bytes: vec![],
        }
    }

    fn send(&mut self, text: &[u8]) {
        self.master.write_all(text).unwrap();
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size) },
            0
        );
    }

    fn until(&mut self, predicate: impl Fn(&Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !predicate(self) {
            assert!(
                Instant::now() < deadline,
                "terminal stalled: {}\nbytes={:?}",
                self.parser.screen().contents(),
                String::from_utf8_lossy(&self.bytes[self.bytes.len().saturating_sub(1800)..])
            );
            let mut fd = libc::pollfd {
                fd: self.master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut fd, 1, 50) } <= 0 {
                continue;
            }
            let mut buf = [0; 4096];
            let count = self.master.read(&mut buf).unwrap();
            assert!(
                count > 0,
                "terminal closed: {}",
                self.parser.screen().contents()
            );
            for byte in &buf[..count] {
                self.bytes.push(*byte);
                self.parser.process(&[*byte]);
                if self.bytes.ends_with(b"\x1b[6n") {
                    let (row, col) = self.parser.screen().cursor_position();
                    self.send(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
                }
            }
        }
    }
}

#[test]
fn prompt_stays_above_one_dock_through_background_output() {
    let mut terminal = Terminal::start("background");
    terminal.until(|t| t.parser.screen().contents().contains("> "));
    terminal.send(b"draft");
    terminal.until(|t| t.parser.screen().contents().contains("> draft"));
    terminal.until(|t| {
        let screen = t.parser.screen();
        screen.contents().contains("background-19")
            && screen.contents().contains("> draft")
            && screen.cursor_position().1 == 7
    });
    let screen = terminal.parser.screen();
    assert_eq!(
        screen.contents().matches("children ·").count(),
        1,
        "{}",
        screen.contents()
    );
    let (row, col) = screen.cursor_position();
    assert!(
        row < 23,
        "prompt escaped the output region: {row}\n{}",
        screen.contents()
    );
    assert_eq!(col, 7);
    assert_eq!(
        screen.cell(row, 2).unwrap().fgcolor(),
        vt100::Color::Default
    );
    terminal.send(b"\x15\x1b[B");
    terminal.until(|t| t.parser.screen().contents().contains("DOCK_FOCUSED"));
}

#[test]
fn resizing_the_prompt_keeps_one_dock_and_preserves_the_draft() {
    let mut terminal = Terminal::start("resize");
    terminal.until(|t| t.parser.screen().contents().contains("> "));
    terminal.send(b"draft");
    terminal.until(|t| t.parser.screen().contents().contains("> draft"));
    for (rows, cols) in [(25, 100), (40, 70), (18, 60), (6, 50), (30, 100)] {
        let footer_rows = if rows < 8 {
            0
        } else {
            (usize::from(rows) / 3).clamp(2, 7)
        };
        let previous_bytes = terminal.bytes.len();
        terminal.resize(rows, cols);
        terminal.until(|t| {
            t.bytes.len() > previous_bytes
                && t.parser.screen().contents().contains("> draft")
                && t.parser.screen().contents().contains("children ·") == (footer_rows > 0)
                && t.parser.screen().cursor_position().1 == 7
        });
        assert_eq!(
            terminal
                .parser
                .screen()
                .contents()
                .matches("children ·")
                .count(),
            usize::from(footer_rows > 0),
            "{}",
            terminal.parser.screen().contents()
        );
        assert!(
            usize::from(terminal.parser.screen().cursor_position().0)
                < usize::from(rows) - footer_rows
        );
    }
    terminal.send(b"\x15\x1b[B");
    terminal.until(|t| t.parser.screen().contents().contains("DOCK_FOCUSED"));
}

#[test]
fn up_sees_tools_arriving_after_the_prompt_without_recalling_history() {
    let mut terminal = Terminal::start("tools");
    terminal.until(|t| t.parser.screen().contents().contains("background-19"));
    terminal.send(b"\x10");
    terminal.until(|t| t.parser.screen().contents().contains("> old history"));
    terminal.send(b"\x15\x1b[A");
    terminal.until(|t| t.parser.screen().contents().contains("TOOLS_FOCUSED"));
    assert!(!terminal.parser.screen().contents().contains("old history"));
}

#[test]
fn terminal_child() {
    let Ok(mode) = std::env::var(CHILD) else {
        return;
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _runtime = runtime.enter();
    console::set_colors_enabled(true);
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            always_list: true,
            rows: vec![ChildRow {
                id: "child".into(),
                label: "child".into(),
                status: ChildStatus::Running,
                model: "fixture".into(),
                started_at: None,
                activity: "working".into(),
            }],
            ..State::default()
        }),
    });
    *ACTIVE.get_or_init(Mutex::default).lock().unwrap() = Arc::downgrade(&shared);
    for n in 0..35 {
        println!("transcript-{n}");
    }
    shared.fit();
    let mut editor = rustyline::Editor::<
        crate::session::completion::GooseCompleter,
        rustyline::history::DefaultHistory,
    >::new()
    .unwrap();
    editor.set_helper(Some(crate::session::completion::GooseCompleter::new(
        Arc::new(std::sync::RwLock::new(
            crate::session::CompletionCache::new(),
        )),
    )));
    editor.add_history_entry("old history").unwrap();
    let available = Arc::new(std::sync::atomic::AtomicBool::new(false));
    crate::session::input::bind_navigation(&mut editor, Some(available.clone()));
    let mut printer = editor.create_external_printer().unwrap();
    let painter = shared.clone();
    let resizing = mode == "resize";
    let background = std::thread::spawn(move || {
        use rustyline::ExternalPrinter;
        std::thread::sleep(Duration::from_millis(200));
        for n in 0..20 {
            if resizing {
                break;
            }
            available.store(true, std::sync::atomic::Ordering::Relaxed);
            printer.print(format!("\x1b[30mbackground-{n}\n")).unwrap();
            painter.paint();
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let _prompt = PromptGuard::start();
    editor.readline("> ").unwrap();
    background.join().unwrap();
    if mode == "tools" {
        assert!(crate::session::tool_calls::take_focus_request());
        println!("TOOLS_FOCUSED");
    } else {
        assert!(take_focus_request());
        println!("DOCK_FOCUSED");
    }
}
