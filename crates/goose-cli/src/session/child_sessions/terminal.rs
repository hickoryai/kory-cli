use super::keys::{Key, KeyParser};
use std::io::{IsTerminal, Write};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::mpsc;

const POLL_MS: i32 = 50;
const ENTER_SCREEN: &str = "\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1006h";
const LEAVE_SCREEN: &str = "\x1b[?1006l\x1b[?1000l\x1b[?25h\x1b[?1049l";

pub fn is_capable() -> bool {
    std::io::stdin().is_terminal()
        && std::io::stdout().is_terminal()
        && std::env::var("TERM").map_or(true, |term| term != "dumb")
}

/// Acts on a key as soon as it is read, on the reader's thread, or hands it back for whoever
/// awaits keys.
pub type DirectKeys = Arc<dyn Fn(Key) -> Option<Key> + Send + Sync>;

/// Reads keys from stdin without line buffering or echo for as long as it lives.
/// ISIG stays on so Ctrl-C still reaches the session's cancel handler, and OPOST stays on so
/// parent output printed meanwhile keeps its newline translation.
///
/// With `DirectKeys` what is typed shows at once however busy the task awaiting keys is. A key
/// handed to that task holds the ones after it back until it was handled, so keys never take
/// effect out of order.
pub struct KeyListener {
    keys: mpsc::UnboundedReceiver<Key>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    original: libc::termios,
    fd: i32,
    direct: bool,
    /// Keys handed over and not handled yet.
    unhandled: Arc<AtomicUsize>,
    /// The key `next` gave out last counts in `unhandled`.
    handling: bool,
}

impl KeyListener {
    pub fn start() -> std::io::Result<Self> {
        Self::start_on(std::io::stdin().as_raw_fd(), None)
    }

    /// For the session's own input box: what `direct` acts on never waits for the session.
    pub fn start_direct(direct: DirectKeys) -> std::io::Result<Self> {
        Self::start_on(std::io::stdin().as_raw_fd(), Some(direct))
    }

    pub(super) fn start_on(fd: i32, direct: Option<DirectKeys>) -> std::io::Result<Self> {
        let original = get_termios(fd)?;
        let cbreak = input_mode(original, false);
        set_termios(fd, &cbreak)?;
        let typed_ahead = enters_restored(read_pending(fd));

        let (tx, keys) = mpsc::unbounded_channel();
        let stop = Arc::new(AtomicBool::new(false));
        let unhandled = Arc::new(AtomicUsize::new(0));
        let is_direct = direct.is_some();
        let delivery = Delivery {
            direct,
            unhandled: unhandled.clone(),
            keys: tx,
        };
        let reader = std::thread::spawn({
            let stop = stop.clone();
            move || read_keys(fd, &typed_ahead, &stop, &delivery)
        });
        Ok(Self {
            keys,
            stop,
            reader: Some(reader),
            original,
            fd,
            direct: is_direct,
            unhandled,
            handling: false,
        })
    }

    /// Whether Ctrl-C and Ctrl-D are keys, as at the prompt, rather than signals and end of
    /// input.
    pub fn set_ctrl_keys(&self, ctrl_keys: bool) -> std::io::Result<()> {
        set_termios(self.fd, &input_mode(self.original, ctrl_keys))
    }

    pub fn is_direct(&self) -> bool {
        self.direct
    }

    /// The next key; the one before it counts as handled.
    pub async fn next(&mut self) -> Option<Key> {
        self.handled();
        let key = self.keys.recv().await?;
        self.handling = key != Key::MessageQueued;
        Some(key)
    }

    /// The key `next` gave out last is dealt with.
    pub fn handled(&mut self) {
        if std::mem::take(&mut self.handling) {
            self.unhandled.fetch_sub(1, Ordering::AcqRel);
        }
    }
}

fn input_mode(mut mode: libc::termios, ctrl_keys: bool) -> libc::termios {
    mode.c_lflag &= !(libc::ICANON | libc::ECHO);
    if ctrl_keys {
        mode.c_lflag &= !libc::ISIG;
    }
    // A previous raw-mode reader may have disabled output processing. The session
    // still writes ordinary newlines while this listener owns input.
    mode.c_oflag |= libc::OPOST | libc::ONLCR;
    // Enter arrives as CR and Ctrl+J as LF, so they stay two keys.
    mode.c_iflag &= !(libc::ICRNL | libc::INLCR | libc::IXON);
    mode.c_cc[libc::VMIN] = 1;
    mode.c_cc[libc::VTIME] = 0;
    mode
}

impl Drop for KeyListener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let _ = set_termios(self.fd, &self.original);
    }
}

/// Input already typed when this listener took over.
fn read_pending(fd: i32) -> Vec<u8> {
    let mut count: libc::c_int = 0;
    if unsafe { libc::ioctl(fd, libc::FIONREAD, &mut count) } != 0 || count <= 0 {
        return Vec::new();
    }
    let mut pending = vec![0u8; count as usize];
    let read = unsafe { libc::read(fd, pending.as_mut_ptr().cast(), pending.len()) };
    pending.truncate(read.max(0) as usize);
    pending
}

/// The terminal's line settings turned Enter into a line feed while the input waited, which a
/// listener would read as Ctrl+J and add a line instead of sending the message.
fn enters_restored(mut typed: Vec<u8>) -> Vec<u8> {
    for byte in &mut typed {
        if *byte == b'\n' {
            *byte = b'\r';
        }
    }
    typed
}

/// Where the reader's keys go.
struct Delivery {
    direct: Option<DirectKeys>,
    unhandled: Arc<AtomicUsize>,
    keys: mpsc::UnboundedSender<Key>,
}

impl Delivery {
    /// False once nobody takes keys.
    fn deliver(&self, key: Key) -> bool {
        let key = match &self.direct {
            Some(direct) if self.unhandled.load(Ordering::Acquire) == 0 => match direct(key) {
                Some(key) => key,
                None => return true,
            },
            _ => key,
        };
        if key != Key::MessageQueued {
            self.unhandled.fetch_add(1, Ordering::AcqRel);
        }
        self.keys.send(key).is_ok()
    }
}

fn read_keys(fd: i32, typed_ahead: &[u8], stop: &AtomicBool, delivery: &Delivery) {
    let mut parser = KeyParser::default();
    for key in parser.feed(typed_ahead) {
        if !delivery.deliver(key) {
            return;
        }
    }
    let mut buf = [0u8; 256];
    while !stop.load(Ordering::Relaxed) {
        let mut pollfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut pollfd, 1, POLL_MS) };
        if ready <= 0 {
            if let Some(key) = parser.flush_idle() {
                if !delivery.deliver(key) {
                    return;
                }
            }
            continue;
        }
        let read = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if read <= 0 {
            return;
        }
        for key in parser.feed(&buf[..read as usize]) {
            if !delivery.deliver(key) {
                return;
            }
        }
    }
}

fn get_termios(fd: i32) -> std::io::Result<libc::termios> {
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { termios.assume_init() })
}

fn set_termios(fd: i32, termios: &libc::termios) -> std::io::Result<()> {
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, termios) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// The alternate screen with mouse reporting, restored on drop.
pub struct Screen;

/// Full-screen views open now.
static SCREENS: AtomicUsize = AtomicUsize::new(0);

/// Whether a full-screen view has the terminal, and so the keys.
pub fn screen_is_open() -> bool {
    SCREENS.load(Ordering::Acquire) > 0
}

impl Screen {
    pub fn enter() -> Self {
        SCREENS.fetch_add(1, Ordering::AcqRel);
        super::dock::suspend();
        write_raw(ENTER_SCREEN);
        Self
    }

    pub fn size(&self) -> (usize, usize) {
        let (rows, cols) = console::Term::stdout().size();
        (cols as usize, rows as usize)
    }

    pub fn draw(&self, lines: &[String]) {
        let mut frame = String::from("\x1b[H");
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                frame.push_str("\r\n");
            }
            frame.push_str(line);
            frame.push_str("\x1b[K");
        }
        frame.push_str("\x1b[J");
        write_raw(&frame);
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        write_raw(LEAVE_SCREEN);
        super::dock::resume();
        SCREENS.fetch_sub(1, Ordering::AcqRel);
    }
}

fn write_raw(text: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(text.as_bytes());
    let _ = stdout.flush();
}

/// An isolated terminal, so a test never changes the test runner's own.
#[cfg(test)]
pub(super) fn test_terminal() -> (std::fs::File, std::fs::File) {
    use std::os::fd::FromRawFd;
    // SAFETY: descriptors this opens and hands to their owners.
    unsafe {
        let fd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(fd >= 0);
        let master = std::fs::File::from_raw_fd(fd);
        assert_eq!(libc::grantpt(fd), 0);
        assert_eq!(libc::unlockpt(fd), 0);
        let name = libc::ptsname(fd);
        assert!(!name.is_null());
        let slave_fd = libc::open(name, libc::O_RDWR | libc::O_NOCTTY);
        assert!(slave_fd >= 0);
        (master, std::fs::File::from_raw_fd(slave_fd))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_after_a_raw_reader_starts_each_line_at_the_left_margin() {
        use std::io::Read;

        let (mut master, mut slave) = test_terminal();
        for ctrl_keys in [false, true] {
            let mut raw = get_termios(slave.as_raw_fd()).unwrap();
            unsafe { libc::cfmakeraw(&mut raw) };
            set_termios(slave.as_raw_fd(), &input_mode(raw, ctrl_keys)).unwrap();
            slave.write_all(b"first\nsecond\n").unwrap();
            let mut output = Vec::new();
            while output.iter().filter(|&&b| b == b'\n').count() < 2 {
                let mut chunk = [0; 128];
                let count = master.read(&mut chunk).unwrap();
                assert!(count > 0);
                output.extend_from_slice(&chunk[..count]);
            }
            let mut terminal = vt100::Parser::new(4, 40, 0);
            terminal.process(&output);
            assert_eq!(terminal.screen().contents(), "first\nsecond");
            assert_eq!(terminal.screen().cursor_position(), (2, 0));
        }
    }

    #[test]
    fn enter_typed_before_the_listener_started_still_sends() {
        assert_eq!(enters_restored(b"/login\n".to_vec()), b"/login\r");
        let keys = KeyParser::default().feed(&enters_restored(b"a\n".to_vec()));
        assert_eq!(keys.last(), Some(&Key::Enter));
    }
}
