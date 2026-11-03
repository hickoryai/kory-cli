//! The terminal's one owner while the pane is up, the way Claude Code runs in a tab.
//!
//! The session's stdin, stdout and stderr go to a pseudo-terminal. Everything written to it is
//! fed to a terminal emulator (`vt100`) that keeps the transcript and its scrollback, and this
//! relay draws the real terminal (on its alternate screen) from that: the transcript's last
//! rows on top and the pane (`dock.rs`: context line, input box, children) pinned under them.
//! Nothing the pane draws ever enters the transcript, and a resize simply draws again at the
//! new size. The wheel and PageUp/PageDown scroll the transcript while the pane stays put; a
//! "Jump to bottom" pill on its last row returns to the live end (so does typing). When the
//! process exits the transcript is printed to the normal screen, so it stays in the
//! terminal's own history.
//!
//! A program that takes the whole screen (the child navigator, the tool-call list, an editor)
//! switches the emulator to its alternate screen, and the relay then shows that screen as is,
//! passing it the mouse when it asked for one. Keys reach the session through the
//! pseudo-terminal unchanged, except that Ctrl-C raises SIGINT when the session's terminal
//! settings would have.
//!
//! `suspend`/`resume` mark a question drawn into the transcript (a tool confirmation): while
//! one is up the real cursor shows where the transcript's is, and the view stays at the
//! bottom. They travel in the output as private OSC sequences, so they take effect exactly
//! between the bytes written before and after them.

use std::io::Write;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Paints the pane: its lines for a terminal of (cols, lines), top first.
pub type Painter = Arc<dyn Fn(usize, usize) -> Vec<String> + Send + Sync>;

const MARKER: &[u8] = b"7799";
const SUSPEND: &str = "\x1b]7799;kory-pane;suspend\x07";
const RESUME: &str = "\x1b]7799;kory-pane;resume\x07";
/// Transcript lines kept above the screen.
const SCROLLBACK: usize = 20_000;
/// Output kept for replaying at a new size; past it the older half goes.
const LOG_LIMIT: usize = 8 * 1024 * 1024;
/// Rows one wheel notch scrolls.
const WHEEL_ROWS: usize = 3;
const IDLE_POLL_MS: i32 = 200;
/// The fastest the screen is drawn again while output streams.
const FRAME: Duration = Duration::from_millis(16);
/// An escape sequence typed in pieces waits this long for its rest.
const HELD_INPUT: Duration = Duration::from_millis(30);
const ENTER: &str = "\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b[H\x1b[2J";
const LEAVE: &str =
    "\x1b[?2026l\x1b[0m\x1b[?1006l\x1b[?1000l\x1b[?2004l\x1b[<u\x1b[?25h\x1b[?1049l";
const PILL: &str = " Jump to bottom (click) ↓ ";

struct Relay {
    real_out: RawFd,
    real_in: RawFd,
    master: RawFd,
    slave: RawFd,
    saved: [RawFd; 3],
    real_termios: libc::termios,
    wake: [RawFd; 2],
    painter: Mutex<Option<Painter>>,
    stop: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

static RELAY: OnceLock<Option<Arc<Relay>>> = OnceLock::new();

/// Routes the session's terminal through the relay (once per process) and makes `painter` the
/// pane. False when the terminal cannot host one; the caller then keeps its plain output.
pub fn install(painter: Painter) -> bool {
    let Some(relay) = RELAY.get_or_init(start).clone() else {
        return false;
    };
    *relay.painter.lock().unwrap() = Some(painter);
    relay.wake();
    true
}

/// Routes the session's terminal through the relay before anything is printed, so the
/// session's header and everything after it are in the transcript; the pane comes later.
pub fn begin() -> bool {
    RELAY.get_or_init(start).is_some()
}

/// The pane goes away; the transcript stays on screen.
pub fn remove_painter() {
    if let Some(relay) = active() {
        *relay.painter.lock().unwrap() = None;
        relay.wake();
    }
}

/// Draw again: what the pane shows changed.
pub fn repaint() {
    if let Some(relay) = active() {
        relay.wake();
    }
}

/// A question drawn into the transcript takes the cursor, after everything written so far.
pub fn suspend() {
    if active().is_some() {
        write_stdout(SUSPEND);
    }
}

pub fn resume() {
    if active().is_some() {
        write_stdout(RESUME);
    }
}

fn active() -> Option<Arc<Relay>> {
    RELAY.get().and_then(|relay| relay.clone())
}

fn write_stdout(text: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

fn start() -> Option<Arc<Relay>> {
    // SAFETY: plain POSIX calls on descriptors this function owns; every failure closes what
    // it opened before returning.
    unsafe {
        let mut real_termios = std::mem::zeroed::<libc::termios>();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut real_termios) != 0 {
            return None;
        }
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if master < 0 || libc::grantpt(master) != 0 || libc::unlockpt(master) != 0 {
            close_all(&[master]);
            return None;
        }
        let name = libc::ptsname(master);
        let slave = if name.is_null() {
            -1
        } else {
            libc::open(name, libc::O_RDWR | libc::O_NOCTTY)
        };
        let mut wake = [-1; 2];
        if slave < 0 || libc::pipe(wake.as_mut_ptr()) != 0 {
            close_all(&[master, slave, wake[0], wake[1]]);
            return None;
        }
        // The session keeps the terminal settings it had; the relay reads the real terminal raw.
        libc::tcsetattr(slave, libc::TCSANOW, &real_termios);
        let real_out = libc::dup(libc::STDOUT_FILENO);
        let real_in = libc::dup(libc::STDIN_FILENO);
        let saved = [
            libc::dup(libc::STDIN_FILENO),
            libc::dup(libc::STDOUT_FILENO),
            libc::dup(libc::STDERR_FILENO),
        ];
        for fd in [master, wake[0], wake[1], real_in] {
            libc::fcntl(
                fd,
                libc::F_SETFL,
                libc::fcntl(fd, libc::F_GETFL) | libc::O_NONBLOCK,
            );
        }
        for fd in [
            master, wake[0], wake[1], real_in, real_out, saved[0], saved[1], saved[2],
        ] {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        let mut raw = real_termios;
        libc::cfmakeraw(&mut raw);
        libc::tcsetattr(real_in, libc::TCSANOW, &raw);
        let size = copy_size(real_out, master);
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        write_fd(real_out, ENTER.as_bytes());
        libc::dup2(slave, libc::STDIN_FILENO);
        libc::dup2(slave, libc::STDOUT_FILENO);
        libc::dup2(slave, libc::STDERR_FILENO);

        let relay = Arc::new(Relay {
            real_out,
            real_in,
            master,
            slave,
            saved,
            real_termios,
            wake,
            painter: Mutex::new(None),
            stop: AtomicBool::new(false),
            thread: Mutex::new(None),
        });
        let pump = relay.clone();
        *relay.thread.lock().unwrap() = Some(std::thread::spawn(move || pump.run(size)));
        libc::atexit(uninstall_at_exit);
        Some(relay)
    }
}

fn close_all(fds: &[RawFd]) {
    for &fd in fds {
        if fd >= 0 {
            // SAFETY: closing descriptors opened by `start`.
            unsafe { libc::close(fd) };
        }
    }
}

/// The process is exiting: give the session its terminal back, pass on what is still in
/// flight, leave the alternate screen and print the transcript where the terminal keeps it.
extern "C" fn uninstall_at_exit() {
    let Some(relay) = active() else {
        return;
    };
    // SAFETY: restores the descriptors `start` saved.
    unsafe {
        for (fd, saved) in relay.saved.iter().enumerate() {
            libc::dup2(*saved, fd as RawFd);
        }
    }
    relay.stop.store(true, Ordering::SeqCst);
    relay.wake();
    let thread = relay.thread.lock().unwrap().take();
    if let Some(thread) = thread {
        let _ = thread.join();
    }
}

/// The real terminal's size onto the pseudo-terminal, so everything that asks for the size
/// gets the real one. Returns (cols, lines).
fn copy_size(real: RawFd, master: RawFd) -> (usize, usize) {
    // SAFETY: TIOCGWINSZ/TIOCSWINSZ with a properly sized winsize.
    unsafe {
        let mut size = std::mem::zeroed::<libc::winsize>();
        if libc::ioctl(real, libc::TIOCGWINSZ, &mut size) != 0
            || size.ws_col == 0
            || size.ws_row == 0
        {
            return (80, 24);
        }
        libc::ioctl(master, libc::TIOCSWINSZ, &size);
        (size.ws_col as usize, size.ws_row as usize)
    }
}

fn write_fd(fd: RawFd, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        // SAFETY: writing from a live slice.
        let n = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if n < 0 {
            match std::io::Error::last_os_error().kind() {
                std::io::ErrorKind::Interrupted => continue,
                std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                _ => return,
            }
        }
        bytes = &bytes[n as usize..];
    }
}

fn read_all(fd: RawFd, buf: &mut [u8], into: &mut Vec<u8>) {
    loop {
        // SAFETY: reading into a buffer of the given length.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            return;
        }
        into.extend_from_slice(&buf[..n as usize]);
    }
}

impl Relay {
    fn wake(&self) {
        // SAFETY: a one-byte write to the relay's own non-blocking pipe.
        unsafe { libc::write(self.wake[1], b"w".as_ptr().cast(), 1) };
    }

    /// Whether the session's terminal settings turn Ctrl-C into SIGINT right now.
    fn signals_on(&self) -> bool {
        // SAFETY: reading the pseudo-terminal's settings.
        unsafe {
            let mut termios = std::mem::zeroed::<libc::termios>();
            libc::tcgetattr(self.slave, &mut termios) == 0 && termios.c_lflag & libc::ISIG != 0
        }
    }

    fn run(&self, (cols, lines): (usize, usize)) {
        let mut view = View::new(cols, lines);
        let mut buf = vec![0u8; 64 * 1024];
        let mut last_frame = Instant::now() - FRAME;
        let mut dirty = true;
        loop {
            let stopping = self.stop.load(Ordering::SeqCst);
            let timeout = if stopping {
                30
            } else if dirty || view.holding_input() {
                FRAME.as_millis() as i32
            } else {
                IDLE_POLL_MS
            };
            let mut fds = [self.master, self.real_in, self.wake[0]].map(|fd| libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            });
            // SAFETY: polling three descriptors this relay owns.
            unsafe { libc::poll(fds.as_mut_ptr(), 3, timeout) };

            let mut output = Vec::new();
            read_all(self.master, &mut buf, &mut output);
            if !output.is_empty() {
                view.output(&output);
                dirty = true;
            }
            let replies = view.take_replies();
            if !replies.is_empty() {
                write_fd(self.master, &replies);
            }
            let mut typed = Vec::new();
            if !stopping {
                read_all(self.real_in, &mut buf, &mut typed);
            }
            let input = view.input(&typed, Instant::now());
            if input.scrolled {
                dirty = true;
            }
            if input.interrupt && self.signals_on() {
                // SAFETY: the signal the terminal would have raised for Ctrl-C.
                unsafe { libc::kill(libc::getpid(), libc::SIGINT) };
            } else if input.interrupt {
                write_fd(self.master, b"\x03");
            }
            if !input.forward.is_empty() {
                write_fd(self.master, &input.forward);
            }
            // SAFETY: draining the wake pipe.
            while unsafe { libc::read(self.wake[0], buf.as_mut_ptr().cast(), buf.len()) } > 0 {
                dirty = true;
            }
            let size = copy_size(self.real_out, self.master);
            if size != view.size() {
                view.resize(size);
                dirty = true;
            }
            if stopping && output.is_empty() {
                let mut out = view.forwarded();
                out.extend_from_slice(LEAVE.as_bytes());
                // SAFETY: the real terminal's own settings back.
                unsafe { libc::tcsetattr(self.real_in, libc::TCSANOW, &self.real_termios) };
                write_fd(self.real_out, &out);
                write_fd(self.real_out, &view.transcript());
                close_all(&[self.slave, self.master]);
                return;
            }
            if dirty && last_frame.elapsed() >= FRAME {
                let painter = self.painter.lock().unwrap().clone();
                let pane = painter
                    .map(|painter| painter(size.0, size.1))
                    .unwrap_or_default();
                let mut out = view.forwarded();
                out.extend_from_slice(&view.frame(&pane));
                write_fd(self.real_out, &out);
                last_frame = Instant::now();
                dirty = false;
            }
        }
    }
}

/// What the emulator hands the relay besides the screen: sequences meant for the real
/// terminal (titles, progress, keyboard modes), answers to the session's queries, and the
/// pane's markers.
#[derive(Default)]
struct Sink {
    forward: Vec<u8>,
    replies: Vec<u8>,
    /// Some(true) at a suspend marker, Some(false) at a resume, the last one seen.
    question: Option<bool>,
}

impl vt100::Callbacks for Sink {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.forward.push(0x07);
    }

    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.forward.extend_from_slice(b"\x1b]2;");
        self.forward.extend_from_slice(title);
        self.forward.push(0x07);
    }

    fn set_window_icon_name(&mut self, _: &mut vt100::Screen, name: &[u8]) {
        self.forward.extend_from_slice(b"\x1b]1;");
        self.forward.extend_from_slice(name);
        self.forward.push(0x07);
    }

    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, ty: &[u8], data: &[u8]) {
        self.forward.extend_from_slice(b"\x1b]52;");
        self.forward.extend_from_slice(ty);
        self.forward.push(b';');
        self.forward.extend_from_slice(data);
        self.forward.push(0x07);
    }

    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        let first = params.first().and_then(|p| p.first()).copied();
        match (i1, c, first) {
            // The kitty keyboard protocol's push and pop reach the real terminal.
            (Some(b'>'), 'u', _) | (Some(b'<'), 'u', _) | (Some(b'='), 'u', _) => {
                self.forward.extend_from_slice(b"\x1b[");
                self.forward.push(i1.unwrap_or(b'>'));
                let numbers: Vec<String> = params
                    .iter()
                    .map(|p| p.iter().map(u16::to_string).collect::<Vec<_>>().join(":"))
                    .collect();
                self.forward.extend_from_slice(numbers.join(";").as_bytes());
                self.forward.push(b'u');
            }
            // The cursor position report and the device attributes, as a terminal answers.
            (None, 'n', Some(6)) => {
                let (row, col) = screen.cursor_position();
                self.replies
                    .extend_from_slice(format!("\x1b[{};{}R", row + 1, col + 1).as_bytes());
            }
            (None, 'n', Some(5)) => self.replies.extend_from_slice(b"\x1b[0n"),
            (None, 'c', None | Some(0)) => self.replies.extend_from_slice(b"\x1b[?62;22c"),
            _ => {}
        }
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        match params {
            [MARKER, b"kory-pane", b"suspend"] => self.question = Some(true),
            [MARKER, b"kory-pane", b"resume"] => self.question = Some(false),
            [MARKER, ..] => {}
            // Progress (OSC 9;4), notifications and the like are the real terminal's.
            params => {
                self.forward.extend_from_slice(b"\x1b]");
                self.forward.extend_from_slice(&params.join(&b';'));
                self.forward.push(0x07);
            }
        }
    }
}

/// What typed input meant: bytes for the session, whether Ctrl-C was pressed, and whether the
/// view scrolled.
#[derive(Default, Debug)]
struct Input {
    forward: Vec<u8>,
    interrupt: bool,
    scrolled: bool,
}

/// The transcript, how far it is scrolled up, and the rows on the real screen.
struct View {
    parser: vt100::Parser<Sink>,
    /// Rows the view sits above the transcript's live end; 0 follows the output.
    scroll_up: usize,
    /// The transcript's length at the last frame, to keep a scrolled view on its rows.
    last_end: usize,
    /// A question in the transcript has the cursor.
    question: bool,
    /// Input bytes of an escape sequence still arriving.
    held: Vec<u8>,
    held_since: Option<Instant>,
    /// The transcript rows on screen at the last frame, for clicks on the pill.
    transcript_rows: usize,
    /// Whether the real terminal has bracketed paste on.
    paste_mode: bool,
    /// The session's output, replayed into a new emulator when the size changes.
    log: Vec<u8>,
}

impl View {
    fn new(cols: usize, lines: usize) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(
                lines.max(2) as u16,
                cols.max(2) as u16,
                SCROLLBACK,
                Sink::default(),
            ),
            scroll_up: 0,
            last_end: 0,
            question: false,
            held: Vec::new(),
            held_since: None,
            transcript_rows: 0,
            paste_mode: false,
            log: Vec::new(),
        }
    }

    fn size(&self) -> (usize, usize) {
        let (rows, cols) = self.parser.screen().size();
        (cols as usize, rows as usize)
    }

    fn output(&mut self, bytes: &[u8]) {
        self.log.extend_from_slice(bytes);
        if self.log.len() > LOG_LIMIT {
            // Keep the newer half, from a line start.
            let cut = self.log.len() - LOG_LIMIT / 2;
            let cut = self.log[cut..]
                .iter()
                .position(|b| *b == b'\n')
                .map_or(cut, |offset| cut + offset + 1);
            self.log.drain(..cut);
        }
        self.parser.process(bytes);
        if let Some(question) = self.parser.callbacks_mut().question.take() {
            self.question = question;
            if question {
                self.scroll_up = 0;
            }
        }
    }

    fn take_replies(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.parser.callbacks_mut().replies)
    }

    /// Sequences meant for the real terminal, and the bracketed-paste mode the session wants.
    fn forwarded(&mut self) -> Vec<u8> {
        let mut out = std::mem::take(&mut self.parser.callbacks_mut().forward);
        let paste = self.parser.screen().bracketed_paste();
        if paste != self.paste_mode {
            out.extend_from_slice(if paste {
                b"\x1b[?2004h"
            } else {
                b"\x1b[?2004l"
            });
            self.paste_mode = paste;
        }
        out
    }

    /// A new terminal size. The emulator cannot reflow, so a fresh one at the new size replays
    /// the output: lines wrap again at the new width, the way the session wrote them, and none
    /// is cut off by a narrower window and lost when it widens again.
    fn resize(&mut self, (cols, lines): (usize, usize)) {
        let mut parser = vt100::Parser::new_with_callbacks(
            lines.max(2) as u16,
            cols.max(2) as u16,
            SCROLLBACK,
            Sink::default(),
        );
        parser.process(&self.log);
        // What replaying asked of the real terminal was done the first time.
        *parser.callbacks_mut() = Sink::default();
        self.parser = parser;
        self.last_end = self.end();
    }

    fn holding_input(&self) -> bool {
        !self.held.is_empty()
    }

    /// Typed bytes: the wheel, PageUp/PageDown and the pill are the view's; everything else
    /// goes to the session (which also brings the view back to the live end).
    fn input(&mut self, typed: &[u8], now: Instant) -> Input {
        let mut input = Input::default();
        let mut bytes = std::mem::take(&mut self.held);
        bytes.extend_from_slice(typed);
        let full_screen = self.parser.screen().alternate_screen();
        let mut i = 0;
        while i < bytes.len() {
            let rest = &bytes[i..];
            if rest[0] == 0x1b {
                match sequence_len(rest) {
                    None => {
                        let since = *self.held_since.get_or_insert(now);
                        if now.duration_since(since) < HELD_INPUT {
                            self.held = rest.to_vec();
                            return input;
                        }
                        // A lone ESC or a sequence that never completed.
                        input.forward.extend_from_slice(rest);
                        break;
                    }
                    Some(len) => {
                        let sequence = &rest[..len];
                        i += len;
                        if let Some(mouse) = sgr_mouse(sequence) {
                            if full_screen {
                                if self.parser.screen().mouse_protocol_mode()
                                    != vt100::MouseProtocolMode::None
                                {
                                    input.forward.extend_from_slice(sequence);
                                }
                            } else {
                                input.scrolled |= self.mouse(mouse);
                            }
                            continue;
                        }
                        match sequence {
                            b"\x1b[5~" if !full_screen => {
                                input.scrolled |= self.scroll(self.page() as isize)
                            }
                            b"\x1b[6~" if !full_screen => {
                                input.scrolled |= self.scroll(-(self.page() as isize))
                            }
                            _ => {
                                input.scrolled |= self.follow();
                                input.forward.extend_from_slice(sequence);
                            }
                        }
                        continue;
                    }
                }
            }
            if rest[0] == 0x03 {
                input.interrupt = true;
            } else {
                input.scrolled |= self.follow();
                input.forward.push(rest[0]);
            }
            i += 1;
        }
        self.held_since = None;
        input
    }

    fn page(&self) -> usize {
        self.transcript_rows.saturating_sub(2).max(1)
    }

    /// Back to the live end; true when the view moved.
    fn follow(&mut self) -> bool {
        std::mem::replace(&mut self.scroll_up, 0) != 0
    }

    fn scroll(&mut self, rows: isize) -> bool {
        let before = self.scroll_up;
        self.scroll_up = self
            .scroll_up
            .saturating_add_signed(rows)
            .min(self.end().saturating_sub(self.transcript_rows.max(1)));
        before != self.scroll_up
    }

    fn mouse(&mut self, (button, col, row, pressed): (u16, u16, u16, bool)) -> bool {
        match button {
            64 => self.scroll(WHEEL_ROWS as isize),
            65 => self.scroll(-(WHEEL_ROWS as isize)),
            // The pill sits on the last transcript row.
            0 if pressed && self.scroll_up > 0 && row as usize + 1 == self.transcript_rows => {
                let (cols, _) = self.size();
                let (start, end) = pill_columns(cols);
                if (start..=end).contains(&(col as usize)) {
                    self.follow()
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// The last screen row holding output (or the cursor, once it has moved along its row).
    fn content_bottom(&self) -> usize {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let (cursor_row, cursor_col) = screen.cursor_position();
        let last_text = screen
            .rows(0, cols)
            .take(rows as usize)
            .enumerate()
            .filter(|(_, text)| !text.trim_end().is_empty())
            .map(|(row, _)| row)
            .last();
        let cursor = (cursor_col > 0).then_some(cursor_row as usize);
        match (last_text, cursor) {
            (Some(a), Some(b)) => a.max(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => 0,
        }
    }

    fn scrollback_len(&mut self) -> usize {
        let screen = self.parser.screen_mut();
        screen.set_scrollback(usize::MAX);
        let len = screen.scrollback();
        screen.set_scrollback(0);
        len
    }

    /// The transcript's length: every scrollback row and the screen's rows down to its output.
    fn end(&mut self) -> usize {
        let bottom = self.content_bottom();
        let empty = bottom == 0 && {
            let screen = self.parser.screen();
            let (_, cols) = screen.size();
            screen.cursor_position() == (0, 0)
                && screen
                    .rows(0, cols)
                    .next()
                    .is_none_or(|row| row.trim_end().is_empty())
        };
        self.scrollback_len() + if empty { 0 } else { bottom + 1 }
    }

    /// Transcript rows `start..end`, formatted.
    fn rows(&mut self, start: usize, end: usize) -> Vec<Vec<u8>> {
        let scrollback = self.scrollback_len();
        let (rows, cols) = self.parser.screen().size();
        let mut out = Vec::new();
        let mut at = start;
        while at < end {
            let offset = scrollback.saturating_sub(at);
            let screen = self.parser.screen_mut();
            screen.set_scrollback(offset);
            // With the view `offset` rows up, screen row 0 is transcript row scrollback - offset.
            let first = at - (scrollback - offset);
            let take = (end - at).min(rows as usize - first);
            out.extend(screen.rows_formatted(0, cols).skip(first).take(take));
            at += take;
            screen.set_scrollback(0);
            if take == 0 {
                break;
            }
        }
        out
    }

    /// The whole transcript, for the normal screen when the session ends.
    fn transcript(&mut self) -> Vec<u8> {
        if self.parser.screen().alternate_screen() {
            self.parser.process(b"\x1b[?1049l");
        }
        let end = self.end();
        let mut out = Vec::new();
        for row in self.rows(0, end) {
            out.extend_from_slice(&row);
            out.extend_from_slice(b"\x1b[0m\r\n");
        }
        out
    }

    /// One frame: the transcript's rows down to the pane, the pill when scrolled up, the pane.
    fn frame(&mut self, pane: &[String]) -> Vec<u8> {
        let (cols, lines) = self.size();
        let mut out = b"\x1b[?2026h\x1b[?25l".to_vec();
        if self.parser.screen().alternate_screen() {
            // A full-screen program: its screen as is.
            let screen = self.parser.screen();
            for (row, text) in screen.rows_formatted(0, cols as u16).enumerate() {
                out.extend(format!("\x1b[{};1H\x1b[0m\x1b[2K", row + 1).into_bytes());
                out.extend_from_slice(&text);
            }
            let (row, col) = screen.cursor_position();
            out.extend(format!("\x1b[0m\x1b[{};{}H", row + 1, col + 1).into_bytes());
            if !screen.hide_cursor() {
                out.extend_from_slice(b"\x1b[?25h");
            }
            out.extend_from_slice(b"\x1b[?2026l");
            return out;
        }

        let pane: Vec<String> = pane
            .iter()
            .take(lines.saturating_sub(1))
            .map(|line| console::truncate_str(line, cols.saturating_sub(1), "").into_owned())
            .collect();
        let rows = lines - pane.len();
        let end = self.end();
        if self.scroll_up > 0 {
            // A scrolled view stays on the rows it shows while output arrives.
            self.scroll_up += end.saturating_sub(self.last_end);
        }
        self.last_end = end;
        self.transcript_rows = rows;
        self.scroll_up = self.scroll_up.min(end.saturating_sub(rows));
        let view_end = end - self.scroll_up;
        let view_start = view_end.saturating_sub(rows);
        let shown = self.rows(view_start, view_end);
        for row in 0..rows {
            out.extend(format!("\x1b[{};1H\x1b[0m\x1b[2K", row + 1).into_bytes());
            if let Some(text) = shown.get(row) {
                out.extend_from_slice(text);
                out.extend_from_slice(b"\x1b[0m");
            }
        }
        if self.scroll_up > 0 && rows > 0 {
            let (start, _) = pill_columns(cols);
            out.extend(
                format!("\x1b[{rows};{}H\x1b[0m\x1b[7m{PILL}\x1b[0m", start + 1).into_bytes(),
            );
        }
        for (index, line) in pane.iter().enumerate() {
            out.extend(format!("\x1b[{};1H\x1b[0m\x1b[2K", rows + index + 1).into_bytes());
            out.extend_from_slice(line.as_bytes());
        }
        out.extend_from_slice(b"\x1b[0m");
        // While a question in the transcript waits for an answer, the cursor is its.
        let screen = self.parser.screen();
        let (row, col) = screen.cursor_position();
        let transcript_row = self.scrollback_len() + row as usize;
        if (self.question || pane.is_empty())
            && !self.parser.screen().hide_cursor()
            && (view_start..view_end).contains(&transcript_row)
        {
            out.extend(
                format!(
                    "\x1b[{};{}H\x1b[?25h",
                    transcript_row - view_start + 1,
                    col + 1
                )
                .into_bytes(),
            );
        }
        out.extend_from_slice(b"\x1b[?2026l");
        out
    }
}

/// The pill's first and last column (0-based), centered.
fn pill_columns(cols: usize) -> (usize, usize) {
    let width = console::measure_text_width(PILL);
    let start = cols.saturating_sub(width) / 2;
    (start, start + width - 1)
}

/// The length of the escape sequence at the start of `bytes`, `None` while it is incomplete.
fn sequence_len(bytes: &[u8]) -> Option<usize> {
    match bytes.get(1)? {
        b'[' => bytes[2..]
            .iter()
            .position(|b| (0x40..=0x7e).contains(b))
            .map(|offset| offset + 3),
        b'O' => bytes.get(2).map(|_| 3),
        _ => Some(2),
    }
}

/// An SGR mouse report: (button, col, row, pressed), 0-based.
fn sgr_mouse(sequence: &[u8]) -> Option<(u16, u16, u16, bool)> {
    let body = sequence.strip_prefix(b"\x1b[<")?;
    let (last, body) = body.split_last()?;
    if *last != b'M' && *last != b'm' {
        return None;
    }
    let text = std::str::from_utf8(body).ok()?;
    let mut fields = text.split(';').map(|field| field.parse::<u16>().ok());
    let (button, col, row) = (fields.next()??, fields.next()??, fields.next()??);
    Some((
        button,
        col.saturating_sub(1),
        row.saturating_sub(1),
        *last == b'M',
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(frame: &[u8]) -> String {
        console::strip_ansi_codes(&String::from_utf8_lossy(frame)).into_owned()
    }

    /// The frame as a screen of `lines` rows, by replaying it into an emulator.
    fn screen(frame: &[u8], cols: u16, lines: u16) -> Vec<String> {
        let mut parser = vt100::Parser::new(lines, cols, 0);
        parser.process(frame);
        parser
            .screen()
            .rows(0, cols)
            .map(|row| row.trim_end().to_string())
            .collect()
    }

    fn pane() -> Vec<String> {
        vec!["── rule".into(), "› typed".into()]
    }

    #[test]
    fn the_pane_is_pinned_under_the_transcripts_last_rows() {
        let mut view = View::new(40, 6);
        view.output(b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\n");
        let rows = screen(&view.frame(&pane()), 40, 6);
        assert_eq!(rows, ["two", "three", "four", "five", "── rule", "› typed"]);
    }

    #[test]
    fn a_short_transcript_starts_at_the_top() {
        let mut view = View::new(40, 6);
        view.output(b"only\r\n");
        let rows = screen(&view.frame(&pane()), 40, 6);
        assert_eq!(rows, ["only", "", "", "", "── rule", "› typed"]);
    }

    #[test]
    fn the_pane_never_enters_the_transcript() {
        let mut view = View::new(40, 6);
        for i in 0..20 {
            view.output(format!("line {i}\r\n").as_bytes());
            view.frame(&pane());
        }
        let transcript = plain(&view.transcript());
        assert!(!transcript.contains("typed"), "{transcript}");
        assert!(transcript.starts_with("line 0\r\n") && transcript.ends_with("line 19\r\n"));
    }

    #[test]
    fn scrolling_up_keeps_the_view_on_its_rows_and_shows_the_pill() {
        let mut view = View::new(60, 8);
        for i in 0..30 {
            view.output(format!("line {i}\r\n").as_bytes());
        }
        view.frame(&pane());
        let input = view.input(b"\x1b[<64;1;1M", Instant::now());
        assert!(input.scrolled && input.forward.is_empty());
        let rows = screen(&view.frame(&pane()), 60, 8);
        assert_eq!(rows[0], "line 21");
        assert!(rows[5].contains("Jump to bottom (click) ↓"), "{rows:?}");

        view.output(b"line 30\r\nline 31\r\n");
        let rows = screen(&view.frame(&pane()), 60, 8);
        assert_eq!(
            rows[0], "line 21",
            "new output does not move a scrolled view"
        );

        let (start, _) = pill_columns(60);
        let click = format!("\x1b[<0;{};6M", start + 2);
        assert!(view.input(click.as_bytes(), Instant::now()).scrolled);
        let rows = screen(&view.frame(&pane()), 60, 8);
        assert_eq!(rows[5], "line 31", "the pill jumps to the live end");
    }

    #[test]
    fn typing_returns_to_the_live_end_and_reaches_the_session() {
        let mut view = View::new(60, 8);
        for i in 0..30 {
            view.output(format!("line {i}\r\n").as_bytes());
        }
        view.frame(&pane());
        view.input(b"\x1b[5~", Instant::now());
        assert!(view.scroll_up > 0);
        let input = view.input(b"x\x1b[A", Instant::now());
        assert_eq!(input.forward, b"x\x1b[A");
        assert_eq!(view.scroll_up, 0);
    }

    #[test]
    fn ctrl_c_and_split_sequences() {
        let mut view = View::new(40, 6);
        let now = Instant::now();
        let input = view.input(b"a\x03\x1b[", now);
        assert!(input.interrupt);
        assert_eq!(input.forward, b"a");
        assert!(view.holding_input());
        let input = view.input(b"B", now);
        assert_eq!(input.forward, b"\x1b[B");
        view.input(b"\x1b", now);
        let input = view.input(b"", now + HELD_INPUT);
        assert_eq!(
            input.forward, b"\x1b",
            "a lone Escape goes through once input pauses"
        );
    }

    #[test]
    fn a_full_screen_program_gets_the_screen_and_the_mouse() {
        let mut view = View::new(40, 6);
        view.output(b"transcript\r\n\x1b[?1049h\x1b[?1000h\x1b[?1006h\x1b[Hview");
        let rows = screen(&view.frame(&pane()), 40, 6);
        assert_eq!(rows[0], "view");
        assert!(!rows.iter().any(|row| row.contains("typed")));
        assert_eq!(
            view.input(b"\x1b[<0;3;2M", Instant::now()).forward,
            b"\x1b[<0;3;2M"
        );
        view.output(b"\x1b[?1049l");
        let rows = screen(&view.frame(&pane()), 40, 6);
        assert_eq!(rows[0], "transcript");
    }

    #[test]
    fn titles_progress_and_keyboard_modes_reach_the_real_terminal() {
        let mut view = View::new(40, 6);
        view.output(b"\x1b]2;kory\x07\x1b]9;4;1;50\x07\x1b[>1u\x1b[?2004h");
        let forwarded = String::from_utf8(view.forwarded()).unwrap();
        assert!(forwarded.contains("\x1b]2;kory\x07"), "{forwarded:?}");
        assert!(forwarded.contains("\x1b]9;4;1;50\x07"), "{forwarded:?}");
        assert!(forwarded.contains("\x1b[>1u"), "{forwarded:?}");
        assert!(forwarded.ends_with("\x1b[?2004h"), "{forwarded:?}");
    }

    #[test]
    fn a_question_takes_the_cursor() {
        let mut view = View::new(40, 6);
        view.output(format!("{SUSPEND}Allow? ").as_bytes());
        let frame = String::from_utf8_lossy(&view.frame(&pane())).into_owned();
        assert!(
            frame.ends_with("\x1b[1;8H\x1b[?25h\x1b[?2026l"),
            "{frame:?}"
        );
        view.output(RESUME.as_bytes());
        let frame = String::from_utf8_lossy(&view.frame(&pane())).into_owned();
        assert!(!frame.contains("\x1b[?25h"), "{frame:?}");
    }

    /// Switching to a narrower monitor and back must not cut lines for good, and the narrow
    /// screen wraps them instead of cutting them.
    #[test]
    fn a_narrower_terminal_rewraps_and_a_wider_one_restores() {
        let mut view = View::new(40, 8);
        view.output(b"0123456789abcdefghijklmnopqrstuvwxyzABCD\r\n");
        view.resize((20, 8));
        let rows = screen(&view.frame(&[]), 20, 8);
        assert_eq!(&rows[..2], ["0123456789abcdefghij", "klmnopqrstuvwxyzABCD"]);
        view.resize((40, 8));
        let rows = screen(&view.frame(&[]), 40, 8);
        assert_eq!(rows[0], "0123456789abcdefghijklmnopqrstuvwxyzABCD");
    }

    #[test]
    fn a_shorter_terminal_keeps_the_newest_output() {
        let mut view = View::new(40, 10);
        for i in 0..8 {
            view.output(format!("line {i}\r\n").as_bytes());
        }
        view.resize((40, 5));
        let rows = screen(&view.frame(&[]), 40, 5);
        assert_eq!(rows[rows.len() - 1], "line 7", "{rows:?}");
        let transcript = plain(&view.transcript());
        assert!(transcript.starts_with("line 0\r\n"), "{transcript}");
    }
}
