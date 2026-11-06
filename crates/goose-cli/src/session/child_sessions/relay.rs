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
/// Ctrl-C this many times within the window ends the process even when the session does not
/// answer it (a hung turn or shutdown).
const FORCE_QUIT_PRESSES: usize = 3;
const FORCE_QUIT_WINDOW: Duration = Duration::from_secs(2);
/// Output kept for replaying at a new size; past it the older half goes.
const LOG_LIMIT: usize = 8 * 1024 * 1024;
/// Rows one wheel notch scrolls.
const WHEEL_ROWS: usize = 3;
const IDLE_POLL_MS: i32 = 200;
/// The fastest the screen is drawn again while output streams.
const FRAME: Duration = Duration::from_millis(16);
/// An escape sequence typed in pieces waits this long for its rest.
const HELD_INPUT: Duration = Duration::from_millis(30);
// Any-motion mouse tracking (1003), so what can be clicked lights up under the pointer.
const ENTER: &str = "\x1b[?1049h\x1b[?1003h\x1b[?1006h\x1b[H\x1b[2J";
const LEAVE: &str =
    "\x1b[?2026l\x1b[0m\x1b[?1006l\x1b[?1003l\x1b[?1002l\x1b[?2004l\x1b[<u\x1b[?25h\x1b[?1049l";
const PILL: &str = " Jump to bottom (click) ↓ ";
/// What can be clicked is drawn white under the pointer.
const HOVER: &str = "\x1b[0m\x1b[97m";
/// SGR mouse button code for motion with no button held.
const MOTION: u16 = 35;

use base64::prelude::{Engine as _, BASE64_STANDARD};

/// Something on screen a click opens, as the arrow keys and Enter would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A tool-call line in the transcript, by its tool call id.
    ToolCall(String),
    /// A child row in the pane, by its index in the pane's list of children.
    Child(usize),
}

/// The last thing clicked; the session takes it on `Key::Activate`.
static ACTIVATED: Mutex<Option<Target>> = Mutex::new(None);

/// What the click behind the latest `Key::Activate` landed on.
pub fn take_activation() -> Option<Target> {
    ACTIVATED.lock().unwrap().take()
}

/// Printed just before a tool-call line, so a click on the line opens that call. Nothing
/// when no relay hosts the terminal.
pub fn tool_call_mark(id: &str) -> String {
    if active().is_none() {
        return String::new();
    }
    let id: String = id
        .chars()
        .filter(|c| !c.is_control() && *c != ';')
        .collect();
    format!("\x1b]7799;kory-pane;tool;{id}\x07")
}

/// Leads a pane line that is child `index`'s row, so a click on it opens that child. The
/// relay takes it off before the line is drawn.
pub fn child_row_mark(index: usize) -> String {
    format!("{CHILD_MARK}{index}\x07")
}

const CHILD_MARK: &str = "\x1b]7799;kory-pane;child;";

/// A pane line's child mark, taken off: the line and the child it is the row of.
fn take_child_mark(line: &str) -> (&str, Option<usize>) {
    let Some(rest) = line.strip_prefix(CHILD_MARK) else {
        return (line, None);
    };
    match rest.split_once('\x07') {
        Some((index, line)) => (line, index.parse().ok()),
        None => (line, None),
    }
}

struct Relay {
    real_out: RawFd,
    real_in: RawFd,
    master: RawFd,
    slave: RawFd,
    saved: [RawFd; 3],
    real_termios: libc::termios,
    wake: [RawFd; 2],
    painter: Mutex<Option<Painter>>,
    /// A child session shown in place of the session's own transcript and pane.
    child: Mutex<Option<ChildFeed>>,
    stop: AtomicBool,
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// A child session's rendered transcript and its pane; `generation` changes with either.
struct ChildFeed {
    generation: u64,
    transcript: Arc<Vec<u8>>,
    painter: Painter,
}

/// The child session on screen: its own emulator, so it scrolls, selects and rewraps exactly
/// as the session's transcript does.
struct ChildView {
    view: View,
    generation: u64,
    fed: Arc<Vec<u8>>,
}

impl ChildView {
    fn new(size: (usize, usize)) -> Self {
        Self {
            view: View::new(size.0, size.1),
            generation: u64::MAX,
            fed: Arc::default(),
        }
    }

    /// The child's transcript as rendered now. Output that only grew is appended, so a view
    /// scrolled up stays on its rows; anything else is drawn afresh.
    fn update(&mut self, feed: &ChildFeed) -> bool {
        if self.generation == feed.generation {
            return false;
        }
        self.generation = feed.generation;
        if !Arc::ptr_eq(&self.fed, &feed.transcript) {
            match feed.transcript.strip_prefix(self.fed.as_slice()) {
                Some(more) => self.view.output(more),
                None => {
                    let scroll_up = self.view.scroll_up;
                    self.view = View::new(self.view.size().0, self.view.size().1);
                    self.view.output(&feed.transcript);
                    self.view.scroll_up = scroll_up;
                }
            }
            self.fed = feed.transcript.clone();
        }
        true
    }
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

/// Shows child session output (`transcript`, as the session itself would have printed it) and
/// `painter`'s pane in place of the session's own, until `hide_child`. Called again as the
/// child goes on. False when no relay hosts the terminal.
pub fn show_child(transcript: &str, painter: Painter) -> bool {
    let Some(relay) = active() else {
        return false;
    };
    {
        let mut child = relay.child.lock().unwrap();
        let bytes = crlf(transcript);
        let generation = child
            .as_ref()
            .map_or(0, |feed| feed.generation.wrapping_add(1));
        let transcript = match child.as_ref() {
            Some(feed) if *feed.transcript == bytes => feed.transcript.clone(),
            _ => Arc::new(bytes),
        };
        *child = Some(ChildFeed {
            generation,
            transcript,
            painter,
        });
    }
    relay.wake();
    true
}

/// Back to the session's own transcript and pane.
pub fn hide_child() {
    if let Some(relay) = active() {
        if relay.child.lock().unwrap().take().is_some() {
            relay.wake();
        }
    }
}

/// Whether a relay hosts the terminal, so a child session can be shown in it.
pub fn is_active() -> bool {
    active().is_some()
}

/// Captured output ends lines with `\n` alone; the pseudo-terminal would have added the `\r`.
fn crlf(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len() + text.len() / 32);
    let mut previous = 0u8;
    for &byte in text.as_bytes() {
        if byte == b'\n' && previous != b'\r' {
            bytes.push(b'\r');
        }
        bytes.push(byte);
        previous = byte;
    }
    bytes
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
        // The session keeps the terminal settings it had, but without echo: keys typed while
        // nothing reads them (an extension still loading, a turn starting) would otherwise be
        // printed into the transcript as `^[` and stray letters. The pane shows what is typed;
        // a program that wants echo turns it on. The relay reads the real terminal raw.
        let mut session_termios = real_termios;
        session_termios.c_lflag &= !(libc::ECHO | libc::ECHONL);
        libc::tcsetattr(slave, libc::TCSANOW, &session_termios);
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
            child: Mutex::new(None),
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

    /// Ctrl-C pressed again and again: the session is not answering it, so the terminal comes
    /// back now (transcript printed, settings restored) and the process ends, whatever it was
    /// waiting on.
    fn force_quit(&self, view: &mut View) -> ! {
        let mut out = view.forwarded();
        out.extend_from_slice(LEAVE.as_bytes());
        // SAFETY: the real terminal's own settings back, then an immediate exit; nothing else
        // in the process can be trusted to finish.
        unsafe { libc::tcsetattr(self.real_in, libc::TCSANOW, &self.real_termios) };
        write_fd(self.real_out, &out);
        write_fd(self.real_out, &view.transcript());
        write_fd(
            self.real_out,
            b"\r\n  kory: force quit (Ctrl-C pressed 3 times)\r\n",
        );
        unsafe { libc::_exit(130) }
    }

    fn run(&self, (cols, lines): (usize, usize)) {
        let mut view = View::new(cols, lines);
        let mut child: Option<ChildView> = None;
        let mut buf = vec![0u8; 64 * 1024];
        let mut last_frame = Instant::now() - FRAME;
        let mut dirty = true;
        let mut interrupts: Vec<Instant> = Vec::new();
        loop {
            let stopping = self.stop.load(Ordering::SeqCst);
            let timeout = if stopping {
                30
            } else if dirty
                || view.holding_input()
                || child
                    .as_ref()
                    .is_some_and(|shown| shown.view.holding_input())
            {
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
            let child_painter = {
                let feed = self.child.lock().unwrap();
                match (feed.as_ref(), child.as_mut()) {
                    (None, Some(_)) => {
                        child = None;
                        dirty = true;
                        None
                    }
                    (None, None) => None,
                    (Some(feed), shown) => {
                        let shown = match shown {
                            Some(shown) => shown,
                            None => child.insert(ChildView::new(view.size())),
                        };
                        dirty |= shown.update(feed);
                        Some(feed.painter.clone())
                    }
                }
            };
            let mut typed = Vec::new();
            if !stopping {
                read_all(self.real_in, &mut buf, &mut typed);
            }
            // A child on screen scrolls and selects in its own transcript.
            let input = match child.as_mut() {
                Some(shown) => shown.view.input(&typed, Instant::now()),
                None => view.input(&typed, Instant::now()),
            };
            if input.scrolled {
                dirty = true;
            }
            if input.interrupt > 0 {
                let now = Instant::now();
                interrupts.retain(|at| now.duration_since(*at) < FORCE_QUIT_WINDOW);
                interrupts.extend(std::iter::repeat_n(now, input.interrupt));
                if interrupts.len() >= FORCE_QUIT_PRESSES {
                    self.force_quit(&mut view);
                }
            }
            for sequence in &input.interrupts {
                if self.signals_on() {
                    // SAFETY: the signal the terminal would have raised for Ctrl-C.
                    unsafe { libc::kill(libc::getpid(), libc::SIGINT) };
                } else {
                    write_fd(self.master, sequence);
                }
            }
            if !input.forward.is_empty() {
                write_fd(self.master, &input.forward);
            }
            if let Some(target) = input.activate {
                *ACTIVATED.lock().unwrap() = Some(target);
                write_fd(self.master, super::keys::ACTIVATE_SEQUENCE);
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
            if let Some(shown) = child.as_mut().filter(|shown| shown.view.size() != size) {
                shown.view.resize(size);
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
                let painter = child_painter.or_else(|| self.painter.lock().unwrap().clone());
                let pane = painter
                    .map(|painter| painter(size.0, size.1))
                    .unwrap_or_default();
                let mut out = view.forwarded();
                match child.as_mut() {
                    Some(shown) => {
                        out.append(&mut shown.view.clipboard);
                        out.extend_from_slice(&shown.view.frame(&pane));
                    }
                    None => out.extend_from_slice(&view.frame(&pane)),
                }
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
    /// Tool-call lines marked since the view last took them: (transcript row, call id).
    tools: Vec<(usize, String)>,
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
                // The emulator reports an omitted parameter as 0, and `CSI < 0 u` pops nothing:
                // a pop sent that way left the protocol on after every turn, so Ctrl-C arrived
                // as `ESC[99;5u` text. An omitted parameter stays omitted.
                let omitted = params.iter().all(|p| p.iter().all(|n| *n == 0));
                if !omitted {
                    let numbers: Vec<String> = params
                        .iter()
                        .map(|p| p.iter().map(u16::to_string).collect::<Vec<_>>().join(":"))
                        .collect();
                    self.forward.extend_from_slice(numbers.join(";").as_bytes());
                }
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

    fn unhandled_osc(&mut self, screen: &mut vt100::Screen, params: &[&[u8]]) {
        match params {
            [MARKER, b"kory-pane", b"suspend"] => self.question = Some(true),
            [MARKER, b"kory-pane", b"resume"] => self.question = Some(false),
            // The line the cursor starts now is that tool call's.
            [MARKER, b"kory-pane", b"tool", id @ ..] if !screen.alternate_screen() => {
                screen.set_scrollback(usize::MAX);
                let scrollback = screen.scrollback();
                screen.set_scrollback(0);
                let row = scrollback + screen.cursor_position().0 as usize;
                let id = String::from_utf8_lossy(&id.join(&b';')).into_owned();
                self.tools.push((row, id));
            }
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
    /// Ctrl-C presses, plain or in the kitty keyboard protocol's encoding.
    interrupt: usize,
    /// Each press as it was typed, sent to the session as a key when its terminal settings do
    /// not make Ctrl-C a signal.
    interrupts: Vec<Vec<u8>>,
    scrolled: bool,
    /// A click on a tool call or child, for the session as `Key::Activate`.
    activate: Option<Target>,
}

/// Text dragged over with the mouse, as (transcript row, column) from where the drag began to
/// where it is now.
#[derive(Clone, Copy)]
struct Selection {
    anchor: (usize, usize),
    head: (usize, usize),
}

impl Selection {
    fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// The columns of transcript row `row` it covers, as (first column, width), when any.
    fn columns(&self, row: usize, cols: usize) -> Option<(usize, usize)> {
        let (start, end) = self.ordered();
        if row < start.0 || row > end.0 {
            return None;
        }
        let first = if row == start.0 { start.1 } else { 0 };
        let past = if row == end.0 { end.1 + 1 } else { cols };
        (past > first).then(|| (first, past.min(cols) - first))
    }
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
    /// The transcript row at the top of the screen at the last frame, for mouse positions.
    view_start: usize,
    selection: Option<Selection>,
    /// A copied selection waiting to reach the real terminal.
    clipboard: Vec<u8>,
    /// Whether the real terminal has bracketed paste on.
    paste_mode: bool,
    /// The session's output, replayed into a new emulator when the size changes.
    log: Vec<u8>,
    /// The transcript rows that are tool-call lines, and their call ids.
    tool_rows: std::collections::HashMap<usize, String>,
    /// The child rows of the pane at the last frame: (screen row, child index).
    pane_children: Vec<(usize, usize)>,
    /// Where the pointer is, as (col, row) on the screen.
    hover: Option<(u16, u16)>,
    /// A click on a target, waiting for `input` to report it.
    clicked: Option<Target>,
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
            view_start: 0,
            selection: None,
            clipboard: Vec::new(),
            paste_mode: false,
            log: Vec::new(),
            tool_rows: Default::default(),
            pane_children: Vec::new(),
            hover: None,
            clicked: None,
        }
    }

    /// Tool-call lines the emulator saw marked since the last call.
    fn take_marks(&mut self) {
        let marks = std::mem::take(&mut self.parser.callbacks_mut().tools);
        self.tool_rows.extend(marks);
    }

    /// What a click at (col, row) on the screen would open: a tool-call line of the
    /// transcript, or a child row of the pane.
    fn target_at(&mut self, row: u16) -> Option<Target> {
        let row = row as usize;
        if row < self.transcript_rows {
            let at = self.view_start + row;
            let id = self.tool_rows.get(&at)?.clone();
            // A mark from before the scrollback dropped its oldest rows names another row now.
            let (cols, _) = self.size();
            let (text, _) = self.row_text(at, 0, cols);
            return text
                .trim_start()
                .starts_with('▸')
                .then_some(Target::ToolCall(id));
        }
        self.pane_children
            .iter()
            .find(|(screen_row, _)| *screen_row == row)
            .map(|(_, index)| Target::Child(*index))
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
        self.take_marks();
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
        out.append(&mut self.clipboard);
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
        // The tool-call lines sit on other rows at the new width.
        let tools = std::mem::take(&mut parser.callbacks_mut().tools);
        // What replaying asked of the real terminal was done the first time.
        *parser.callbacks_mut() = Sink::default();
        self.parser = parser;
        self.tool_rows = tools.into_iter().collect();
        self.selection = None;
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
                            sequence if is_kitty_ctrl_c(sequence) => {
                                input.interrupt += 1;
                                input.interrupts.push(sequence.to_vec());
                            }
                            // A key release the protocol reports separately means nothing here.
                            sequence if is_kitty_release(sequence) => {}
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
                input.interrupt += 1;
                input.interrupts.push(vec![0x03]);
            } else {
                input.scrolled |= self.follow();
                input.forward.push(rest[0]);
            }
            i += 1;
        }
        self.held_since = None;
        input.activate = self.clicked.take();
        input
    }

    fn page(&self) -> usize {
        self.transcript_rows.saturating_sub(2).max(1)
    }

    /// Back to the live end, dropping any selection; true when the view changed.
    fn follow(&mut self) -> bool {
        let selected = self.selection.take().is_some();
        std::mem::replace(&mut self.scroll_up, 0) != 0 || selected
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
            0 if pressed
                && self.scroll_up > 0
                && row as usize + 1 == self.transcript_rows
                && (pill_columns(self.size().0).0..=pill_columns(self.size().0).1)
                    .contains(&(col as usize)) =>
            {
                self.follow()
            }
            // A tool call or a child opens, as Enter would.
            0 if pressed => match self.target_at(row) {
                Some(target) => {
                    self.selection = None;
                    self.clicked = Some(target);
                    false
                }
                None => self.begin_selection(col, row),
            },
            // The pointer moved: what it is over lights up.
            MOTION => {
                let before = self.hovered();
                self.hover = Some((col, row));
                self.hovered() != before
            }
            // Motion with the left button held.
            32 => self.drag_selection(col, row),
            0 => self.end_selection(col, row),
            _ => false,
        }
    }

    /// The target under the pointer, and the screen row it is on.
    fn hovered(&mut self) -> Option<(usize, Target)> {
        let (_, row) = self.hover?;
        self.target_at(row).map(|target| (row as usize, target))
    }

    /// The transcript position under the mouse, held inside the transcript's rows.
    fn position(&self, col: u16, row: u16) -> (usize, usize) {
        let row = (row as usize).min(self.transcript_rows.saturating_sub(1));
        (self.view_start + row, col as usize)
    }

    fn begin_selection(&mut self, col: u16, row: u16) -> bool {
        let had_one = self.selection.is_some();
        self.selection = ((row as usize) < self.transcript_rows).then(|| {
            let at = self.position(col, row);
            Selection {
                anchor: at,
                head: at,
            }
        });
        had_one
    }

    /// A drag to the top or bottom row keeps the view moving, so a selection can pass the screen.
    fn drag_selection(&mut self, col: u16, row: u16) -> bool {
        if self.selection.is_none() {
            return false;
        }
        if row == 0 {
            self.scroll(1);
        } else if row as usize + 1 >= self.transcript_rows {
            self.scroll(-1);
        }
        let at = self.position(col, row);
        if let Some(selection) = &mut self.selection {
            selection.head = at;
        }
        true
    }

    /// Letting go copies the text; a click that dragged nowhere selects nothing.
    fn end_selection(&mut self, col: u16, row: u16) -> bool {
        let at = self.position(col, row);
        let Some(selection) = &mut self.selection else {
            return false;
        };
        selection.head = at;
        let selection = *selection;
        if selection.anchor == selection.head {
            self.selection = None;
        } else {
            let text = self.selected_text(selection);
            self.clipboard
                .extend(format!("\x1b]52;c;{}\x07", BASE64_STANDARD.encode(text)).into_bytes());
        }
        true
    }

    /// Lines the terminal wrapped join back up, so a copied line is the line the session wrote.
    fn selected_text(&mut self, selection: Selection) -> String {
        let (cols, _) = self.size();
        let (start, end) = selection.ordered();
        let mut text = String::new();
        for row in start.0..=end.0 {
            let (first, width) = selection.columns(row, cols).unwrap_or((0, 0));
            let (line, wrapped) = self.row_text(row, first, width);
            text.push_str(line.trim_end());
            if row < end.0 && !wrapped {
                text.push('\n');
            }
        }
        text
    }

    /// Unstyled columns of a transcript row, and whether the row wraps onto the next.
    fn row_text(&mut self, at: usize, first_col: usize, width: usize) -> (String, bool) {
        let scrollback = self.scrollback_len();
        let offset = scrollback.saturating_sub(at);
        let row = at - (scrollback - offset);
        let screen = self.parser.screen_mut();
        screen.set_scrollback(offset);
        let found = (row as u16) < screen.size().0;
        let line = screen
            .rows(first_col as u16, width as u16)
            .nth(row)
            .unwrap_or_default();
        let wrapped = found && screen.row_wrapped(row as u16);
        screen.set_scrollback(0);
        (line, wrapped)
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

        let mut children = Vec::new();
        let pane: Vec<String> = pane
            .iter()
            .take(lines.saturating_sub(1))
            .enumerate()
            .map(|(index, line)| {
                let (line, child) = take_child_mark(line);
                if let Some(child) = child {
                    children.push((index, child));
                }
                console::truncate_str(line, cols.saturating_sub(1), "").into_owned()
            })
            .collect();
        let rows = lines - pane.len();
        self.pane_children = children
            .into_iter()
            .map(|(index, child)| (rows + index, child))
            .collect();
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
        self.view_start = view_start;
        let shown = self.rows(view_start, view_end);
        for row in 0..rows {
            out.extend(format!("\x1b[{};1H\x1b[0m\x1b[2K", row + 1).into_bytes());
            if let Some(text) = shown.get(row) {
                out.extend_from_slice(text);
                out.extend_from_slice(b"\x1b[0m");
            }
        }
        if let Some(selection) = self.selection {
            for at in view_start..view_end {
                let Some((first, width)) = selection.columns(at, cols) else {
                    continue;
                };
                let (text, _) = self.row_text(at, first, width);
                out.extend(
                    format!(
                        "\x1b[{};{}H\x1b[0m\x1b[7m{}\x1b[0m",
                        at - view_start + 1,
                        first + 1,
                        text.trim_end()
                    )
                    .into_bytes(),
                );
            }
        }
        // The tool call or child under the pointer, in white.
        let hovered = self.hovered().map(|(row, _)| row);
        if let Some(row) = hovered.filter(|row| *row < rows) {
            let (text, _) = self.row_text(view_start + row, 0, cols);
            out.extend(
                format!(
                    "\x1b[{};1H\x1b[0m\x1b[2K{HOVER}{}\x1b[0m",
                    row + 1,
                    text.trim_end()
                )
                .into_bytes(),
            );
        }
        if self.scroll_up > 0 && rows > 0 {
            let (start, _) = pill_columns(cols);
            out.extend(
                format!("\x1b[{rows};{}H\x1b[0m\x1b[7m{PILL}\x1b[0m", start + 1).into_bytes(),
            );
        }
        for (index, line) in pane.iter().enumerate() {
            out.extend(format!("\x1b[{};1H\x1b[0m\x1b[2K", rows + index + 1).into_bytes());
            if hovered == Some(rows + index) {
                out.extend(format!("{HOVER}{}", console::strip_ansi_codes(line)).into_bytes());
            } else {
                out.extend_from_slice(line.as_bytes());
            }
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

/// Ctrl+C as the kitty keyboard protocol reports a press or repeat: `CSI 99 ; 5 u`, with an
/// optional `:1`/`:2` event type and other modifier bits (Caps Lock, Num Lock) set.
fn is_kitty_ctrl_c(sequence: &[u8]) -> bool {
    let Some(body) = sequence
        .strip_prefix(b"\x1b[")
        .and_then(|rest| rest.strip_suffix(b"u"))
    else {
        return false;
    };
    let text = String::from_utf8_lossy(body);
    let mut fields = text.split(';');
    let code = fields
        .next()
        .and_then(|f| f.split(':').next()?.parse::<u32>().ok());
    let mut modifier = fields.next().unwrap_or("1").split(':');
    let mods = modifier
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .unwrap_or(1);
    let event = modifier.next().unwrap_or("1");
    let bits = mods.saturating_sub(1);
    code == Some(99) && bits & 4 != 0 && bits & !(4 | 64 | 128) == 0 && event != "3"
}

fn is_kitty_release(sequence: &[u8]) -> bool {
    sequence.starts_with(b"\x1b[") && sequence.ends_with(b":3u")
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

    fn feed(generation: u64, text: &str) -> ChildFeed {
        ChildFeed {
            generation,
            transcript: Arc::new(crlf(text)),
            painter: Arc::new(|_, _| Vec::new()),
        }
    }

    #[test]
    fn a_child_is_drawn_like_the_session_with_its_own_pane() {
        let mut child = ChildView::new((40, 6));
        assert!(child.update(&feed(0, "› task\nreply\n")));
        let rows = screen(&child.view.frame(&pane()), 40, 6);
        assert_eq!(rows, ["› task", "reply", "", "", "── rule", "› typed"]);
        assert!(!child.update(&feed(0, "› task\nreply\n")));
    }

    #[test]
    fn a_child_that_grew_keeps_a_scrolled_view_on_its_rows() {
        let text: String = (0..30).map(|i| format!("line {i}\n")).collect();
        let mut child = ChildView::new((60, 8));
        child.update(&feed(0, &text));
        child.view.frame(&pane());
        child.view.input(b"\x1b[<64;1;1M", Instant::now());
        let before = screen(&child.view.frame(&pane()), 60, 8);

        child.update(&feed(1, &format!("{text}line 30\nline 31\n")));
        assert_eq!(screen(&child.view.frame(&pane()), 60, 8)[0], before[0]);

        // A transcript that changed rather than grew is drawn afresh.
        child.view.follow();
        child.update(&feed(2, "other\n"));
        let rows = screen(&child.view.frame(&pane()), 60, 8);
        assert_eq!(rows[0], "other");
    }

    fn clickable_view() -> (View, Vec<String>) {
        let mut view = View::new(60, 8);
        view.output(
            b"intro\r\n\x1b]7799;kory-pane;tool;call-1\x07\x1b[2m  \xe2\x96\xb8 Read file\x1b[0m\r\nafter\r\n",
        );
        let pane = vec![
            "── rule".to_string(),
            format!("{}\x1b[2m  child row\x1b[0m", child_row_mark(3)),
        ];
        view.frame(&pane);
        (view, pane)
    }

    #[test]
    fn a_tool_call_line_lights_up_under_the_pointer_and_a_click_opens_it() {
        let (mut view, pane) = clickable_view();
        // Over plain output nothing lights up.
        assert!(!view.input(b"\x1b[<35;3;1M", Instant::now()).scrolled);
        assert!(!String::from_utf8_lossy(&view.frame(&pane)).contains(HOVER));

        let input = view.input(b"\x1b[<35;5;2M", Instant::now());
        assert!(input.scrolled && input.forward.is_empty());
        let frame = String::from_utf8_lossy(&view.frame(&pane)).into_owned();
        assert!(
            frame.contains(&format!("{HOVER}  ▸ Read file")),
            "{frame:?}"
        );

        let input = view.input(b"\x1b[<0;5;2M\x1b[<0;5;2m", Instant::now());
        assert_eq!(input.activate, Some(Target::ToolCall("call-1".into())));
        assert!(input.forward.is_empty() && view.selection.is_none());
    }

    #[test]
    fn a_child_row_lights_up_and_opens_and_its_mark_never_reaches_the_screen() {
        let (mut view, pane) = clickable_view();
        let frame = view.frame(&pane);
        assert!(!String::from_utf8_lossy(&frame).contains("kory-pane"));
        assert_eq!(screen(&frame, 60, 8)[7], "  child row");

        view.input(b"\x1b[<35;4;8M", Instant::now());
        let frame = String::from_utf8_lossy(&view.frame(&pane)).into_owned();
        assert!(frame.contains(&format!("{HOVER}  child row")), "{frame:?}");
        let input = view.input(b"\x1b[<0;4;8M", Instant::now());
        assert_eq!(input.activate, Some(Target::Child(3)));
    }

    #[test]
    fn tool_call_marks_follow_their_lines_through_a_resize() {
        let (mut view, pane) = clickable_view();
        view.resize((30, 8));
        view.frame(&pane);
        let input = view.input(b"\x1b[<0;5;2M", Instant::now());
        assert_eq!(input.activate, Some(Target::ToolCall("call-1".into())));
    }

    #[test]
    fn captured_line_ends_get_their_carriage_return_once() {
        assert_eq!(crlf("a\nb\r\nc"), b"a\r\nb\r\nc");
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

    /// The operator's report: after a turn left the kitty keyboard protocol on, Ctrl-C reached
    /// the session as `ESC[99;5u` and only echoed, so nothing could end the app.
    #[test]
    fn dragging_highlights_and_copies_the_text_through_the_terminal() {
        let mut view = View::new(40, 8);
        view.output(b"first line\r\nsecond line\r\nthird\r\n");
        view.frame(&pane());
        assert!(!view.input(b"\x1b[<0;3;1M", Instant::now()).scrolled);
        let drag = view.input(b"\x1b[<32;4;2M", Instant::now());
        assert!(drag.scrolled && drag.forward.is_empty());
        let frame = view.frame(&pane());
        assert!(
            String::from_utf8_lossy(&frame).contains("\x1b[7mrst line"),
            "the selection is highlighted"
        );
        assert!(view.input(b"\x1b[<0;4;2m", Instant::now()).scrolled);
        let copied = BASE64_STANDARD.encode("rst line\nseco");
        assert!(
            String::from_utf8_lossy(&view.forwarded()).contains(&format!("]52;c;{copied}\x07")),
            "the text reaches the clipboard"
        );
        assert!(view.selection.is_some(), "it stays highlighted");
        assert!(view.input(b"x", Instant::now()).scrolled);
        assert!(view.selection.is_none(), "typing drops it");
    }

    #[test]
    fn a_click_copies_nothing_and_a_wrapped_line_copies_as_one() {
        let mut view = View::new(10, 8);
        view.output(b"abcdefghijklmn\r\nnext\r\n");
        view.frame(&pane());
        view.input(b"\x1b[<0;1;1M\x1b[<0;1;1m", Instant::now());
        assert!(view.selection.is_none());
        assert!(!String::from_utf8_lossy(&view.forwarded()).contains("]52;"));
        view.input(b"\x1b[<0;1;1M\x1b[<32;2;3M\x1b[<0;2;3m", Instant::now());
        let copied = BASE64_STANDARD.encode("abcdefghijklmn\nne");
        assert!(String::from_utf8_lossy(&view.forwarded()).contains(&copied));
    }

    #[test]
    fn a_kitty_encoded_ctrl_c_is_an_interrupt() {
        let mut view = View::new(40, 6);
        let input = view.input(
            b"\x1b[99;5u\x1b[99;5:1u\x1b[99;69u\x1b[99;5:3u",
            Instant::now(),
        );
        assert_eq!(
            input.interrupt, 3,
            "press, explicit press and Caps Lock held"
        );
        assert_eq!(
            input.interrupts[0], b"\x1b[99;5u",
            "kept for a session reading keys"
        );
        assert!(
            input.forward.is_empty(),
            "the release means nothing: {:?}",
            input.forward
        );
        assert!(!is_kitty_ctrl_c(b"\x1b[99;7u"), "Ctrl+Alt+C is another key");
        assert!(!is_kitty_ctrl_c(b"\x1b[99u"), "a plain c");
    }

    #[test]
    fn kitty_keyboard_push_and_pop_reach_the_real_terminal() {
        let mut view = View::new(40, 6);
        view.output(b"\x1b[>1u");
        view.output(b"\x1b[<u");
        assert_eq!(
            String::from_utf8(view.forwarded()).unwrap(),
            "\x1b[>1u\x1b[<u"
        );
    }

    #[test]
    fn ctrl_c_and_split_sequences() {
        let mut view = View::new(40, 6);
        let now = Instant::now();
        let input = view.input(b"a\x03\x1b[", now);
        assert_eq!(input.interrupt, 1);
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
