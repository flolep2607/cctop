//! Watching an agent's terminal from inside cctop.
//!
//! The [`shim`](crate::shim) already owns the pty of anything started with
//! `cctop run`, so it can hand out a copy of everything the agent draws. This is
//! the other end: connect, rebuild the screen from the replay, and keep feeding
//! the parser as the agent paints. Keystrokes travel back up the same socket,
//! which is the path [`inject`](crate::inject) has always used.
//!
//! A watcher also tells the shim how much room it has, so the agent can be sized
//! to fit rather than shown cropped — see [`frame`] for why that needs a wire
//! format, and [`shim`](crate::shim) for how the sizes of several watchers are
//! reconciled into the one size a pty can have.
//!
//! Nothing here is platform-specific except the socket, so the parser and the
//! key encoding compile everywhere; [`attach`] simply never succeeds off unix.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Write;

/// The wire format an attached connection speaks: `[kind][len: u32 BE][payload]`.
///
/// Both directions carry two kinds of message, and one of them is raw keystrokes
/// — which can be any byte at all, including newlines and escapes. That leaves
/// nothing a control message could use as a delimiter, so the stream is framed
/// rather than the line protocol the handshake started out as.
pub mod frame {
    /// Shim → watcher: bytes the pty produced.
    pub const OUTPUT: u8 = b'o';
    /// Shim → watcher: the size the pty now has, as `"<cols> <rows>"`.
    pub const SIZE: u8 = b's';
    /// Watcher → shim: bytes to type into the pty.
    pub const KEYS: u8 = b'k';
    /// Watcher → shim: the size this watcher can display, as `"<cols> <rows>"`.
    pub const RESIZE: u8 = b'r';

    /// Frames are pty chunks and keystrokes, both far below this. The cap exists
    /// so a desynchronised stream fails fast instead of trying to allocate
    /// whatever four bytes of misread payload happen to say.
    const MAX_FRAME: usize = 1 << 20;

    const HEADER: usize = 1 + 4;

    pub fn encode(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER + payload.len());
        out.push(kind);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    /// A `SIZE` or `RESIZE` frame.
    pub fn size(kind: u8, cols: u16, rows: u16) -> Vec<u8> {
        encode(kind, format!("{cols} {rows}").as_bytes())
    }

    /// The `(cols, rows)` in a size frame's payload, if it is one.
    pub fn parse_size(payload: &[u8]) -> Option<(u16, u16)> {
        let text = std::str::from_utf8(payload).ok()?;
        let (cols, rows) = text.trim().split_once(' ')?;
        let (cols, rows) = (cols.parse().ok()?, rows.parse().ok()?);
        (cols > 0 && rows > 0).then_some((cols, rows))
    }

    /// Reassembles frames from a byte stream that splits them anywhere.
    #[derive(Default)]
    pub struct Decoder {
        buf: Vec<u8>,
        /// A length past [`MAX_FRAME`] means the stream is no longer frame
        /// aligned, and no later byte can put it back — so stop rather than
        /// hand up payloads cut from the middle of other frames.
        lost: bool,
    }

    impl Decoder {
        pub fn push(&mut self, bytes: &[u8]) {
            self.buf.extend_from_slice(bytes);
        }

        /// The next complete frame as `(kind, payload)`.
        pub fn next(&mut self) -> Option<(u8, Vec<u8>)> {
            if self.lost || self.buf.len() < HEADER {
                return None;
            }
            let len = u32::from_be_bytes(self.buf[1..HEADER].try_into().ok()?) as usize;
            if len > MAX_FRAME {
                self.lost = true;
                return None;
            }
            if self.buf.len() < HEADER + len {
                return None;
            }
            let kind = self.buf[0];
            let payload = self.buf[HEADER..HEADER + len].to_vec();
            self.buf.drain(..HEADER + len);
            Some((kind, payload))
        }
    }
}

/// One thing the shim said, kept in order so a resize is applied to the parser
/// before the redraw the agent sent in response to it.
enum Event {
    Output(Vec<u8>),
    Size(u16, u16),
}

/// A live connection to the shim driving one agent.
pub struct Attach {
    input: Box<dyn Write + Send>,
    /// Closes the connection from both ends when the watcher goes away.
    ///
    /// Dropping the writer is not enough: the reader thread holds its own handle
    /// on the same socket, so nothing reaches end-of-file and the shim goes on
    /// sizing the agent to fit a screen nobody is looking at.
    close: Box<dyn Fn() + Send>,
    /// Filled by the reader thread, drained by [`Attach::pump`]. A buffer rather
    /// than a channel because the UI loop already wakes on a timer; a channel
    /// would add plumbing and arrive no sooner.
    pending: std::sync::Arc<std::sync::Mutex<Vec<Event>>>,
    /// Set by the reader thread when the shim hangs up. A pane showing an agent
    /// it does not own has nothing else to go on: there is no child to wait for,
    /// so the closed socket is what says the agent is gone.
    closed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Screen the agent believes it is drawing on, `(cols, rows)`. The shim
    /// decides this — a watcher asks, and may be given less because another
    /// watcher, or the window the agent was launched from, is narrower.
    pub size: (u16, u16),
    /// The last size asked for, so an unchanged panel doesn't re-ask every frame.
    /// Each request costs the agent a SIGWINCH and a full repaint.
    requested: (u16, u16),
    /// The agent's screen, rebuilt from its output.
    pub parser: vt100::Parser,
}

impl Attach {
    /// Fold whatever has arrived into the screen. True when it changed.
    pub fn pump(&mut self) -> bool {
        let events = {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            std::mem::take(&mut *pending)
        };
        if events.is_empty() {
            return false;
        }
        for event in events {
            match event {
                Event::Output(bytes) => self.parser.process(&bytes),
                Event::Size(cols, rows) => {
                    self.size = (cols, rows);
                    self.parser.screen_mut().set_size(rows, cols);
                }
            }
        }
        true
    }

    /// Whether the shim has hung up, so this screen will never change again.
    pub fn closed(&self) -> bool {
        self.closed.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Ask for the agent to be drawn at `cols`×`rows`.
    ///
    /// Only a request: the size that comes back is whatever the shim can give
    /// every watcher at once, and it arrives as a size frame rather than here.
    pub fn resize(&mut self, cols: u16, rows: u16) -> bool {
        if (cols, rows) == self.requested || cols == 0 || rows == 0 {
            return true;
        }
        self.requested = (cols, rows);
        self.send(&frame::size(frame::RESIZE, cols, rows))
    }

    /// Type a key into the agent. Returns false once the connection is gone.
    pub fn send_key(&mut self, key: KeyEvent) -> bool {
        match encode(key) {
            Some(bytes) => self.send(&frame::encode(frame::KEYS, &bytes)),
            None => true,
        }
    }

    fn send(&mut self, bytes: &[u8]) -> bool {
        self.input
            .write_all(bytes)
            .and_then(|()| self.input.flush())
            .is_ok()
    }
}

impl Drop for Attach {
    fn drop(&mut self) {
        (self.close)();
    }
}

/// Connect to the shim owning `pid` and start collecting its output.
///
/// `None` when this agent wasn't started by `cctop run` — the socket is the only
/// way in, and there isn't one.
#[cfg(unix)]
pub fn attach(pid: u32) -> Option<Attach> {
    use std::io::Read;

    let mut stream = connect(pid)?;
    // The shim announces the pty's size before anything else, so the parser can
    // be built to match. Bounded: a peer that never answers must not wedge the
    // UI thread that called this.
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));

    let mut decoder = frame::Decoder::default();
    let mut buf = [0u8; 8192];
    let mut early = Vec::new();
    let size = loop {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        decoder.push(&buf[..n]);
        let mut size = None;
        while let Some(event) = read_event(&mut decoder) {
            if let Event::Size(cols, rows) = event {
                size = Some((cols, rows));
                break;
            }
            early.push(event);
        }
        if let Some(size) = size {
            break size;
        }
    };
    let _ = stream.set_read_timeout(None);
    // The replay arrived in the same breath as the size and is already decoded
    // as far as the buffer goes. Nothing else would pull it out until the agent
    // next draws, which for one sitting at its prompt is never.
    while let Some(event) = read_event(&mut decoder) {
        early.push(event);
    }

    let pending = std::sync::Arc::new(std::sync::Mutex::new(early));
    let closed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let collector = std::sync::Arc::clone(&pending);
    let hangup = std::sync::Arc::clone(&closed);
    let mut reader = stream.try_clone().ok()?;
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            decoder.push(&buf[..n]);
            let mut pending = collector
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            while let Some(event) = read_event(&mut decoder) {
                pending.push(event);
            }
        }
        hangup.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let closer = stream.try_clone().ok()?;
    Some(Attach {
        input: Box::new(stream),
        close: Box::new(move || {
            let _ = closer.shutdown(std::net::Shutdown::Both);
        }),
        pending,
        closed,
        size,
        requested: (0, 0),
        // No scrollback: this shows what the agent is showing. Its own history
        // is in the transcript, which the panels already read.
        parser: vt100::Parser::new(size.1, size.0, 0),
    })
}

/// Open an attach connection to the shim owning `pid`.
#[cfg(unix)]
fn connect(pid: u32) -> Option<std::os::unix::net::UnixStream> {
    let path = crate::shim::socket_path(pid)?;
    let mut stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    stream.write_all(crate::shim::ATTACH_MAGIC).ok()?;
    Some(stream)
}

#[cfg(not(unix))]
pub fn attach(_pid: u32) -> Option<Attach> {
    None
}

/// `cctop attach [pid]` — put an agent on *this* terminal, with no cctop in the
/// way.
///
/// The TUI's attach view is a screen inside another screen; this is the same
/// connection wired straight to the real terminal, so the agent gets every key
/// and every pixel. With no pid and exactly one session running, that one is
/// taken; otherwise what is running is listed.
#[cfg(unix)]
pub fn run_terminal(args: &[String]) -> anyhow::Result<i32> {
    let sessions = crate::shim::sessions();
    let pid = match args {
        [] if sessions.len() == 1 => sessions[0],
        [] => return list_sessions(&sessions),
        [arg] => arg
            .parse::<u32>()
            .map_err(|_| anyhow::anyhow!("expected a pid; `cctop attach` lists what is running"))?,
        _ => anyhow::bail!("usage: cctop attach [pid]"),
    };
    if !sessions.contains(&pid) {
        anyhow::bail!(
            "no cctop-launched agent is running as {pid}; `cctop attach` lists what is running"
        );
    }
    proxy(pid)
}

#[cfg(unix)]
fn list_sessions(sessions: &[u32]) -> anyhow::Result<i32> {
    if sessions.is_empty() {
        eprintln!(
            "No agents are running under cctop. Start one with `cctop claude` \
             (or codex, opencode, pi) and it becomes attachable."
        );
        return Ok(1);
    }
    eprintln!("Agents running under cctop:\n");
    let mut sys = sysinfo::System::new();
    let pids: Vec<sysinfo::Pid> = sessions
        .iter()
        .map(|&p| sysinfo::Pid::from_u32(p))
        .collect();
    // The command and the directory have to be asked for by name; the default
    // refresh returns neither, which leaves a list of bare pids to choose from.
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&pids),
        true,
        sysinfo::ProcessRefreshKind::nothing()
            .with_cmd(sysinfo::UpdateKind::Always)
            .with_cwd(sysinfo::UpdateKind::Always),
    );
    for &pid in sessions {
        let process = sys.process(sysinfo::Pid::from_u32(pid));
        let command = process
            .map(|p| {
                p.cmd()
                    .iter()
                    .map(|a| a.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        // Two agents of the same name are told apart by where they are working,
        // which is the whole reason to print anything but the pid.
        let cwd = process
            .and_then(|p| p.cwd())
            .map(|d| format!("  in {}", d.display()))
            .unwrap_or_default();
        eprintln!("  {pid:>7}  {}{cwd}", crate::util::truncate(&command, 60));
    }
    eprintln!("\nAttach with `cctop attach <pid>`.");
    Ok(0)
}

/// The detach key, matching the one the TUI uses. A function key because those
/// are the ones an agent never wants: any Ctrl- combination worth pressing is
/// one it might.
#[cfg(unix)]
const DETACH: &[u8] = b"\x1b[24~";

/// Undo what the agent did to this terminal on its way out: leave the alternate
/// screen, stop the mouse reporting a TUI turns on, show the cursor, drop any
/// colour still in effect.
#[cfg(unix)]
const RESTORE: &[u8] = b"\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?25h\x1b[0m";

#[cfg(unix)]
fn proxy(pid: u32) -> anyhow::Result<i32> {
    use std::io::{IsTerminal, Read};

    // Proxying a terminal into a pipe helps nobody: the agent's escape sequences
    // land as garbage, and the resize this sends would shrink a live session to
    // fit a window that isn't there.
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        anyhow::bail!("cctop attach needs a terminal; run it from an interactive shell");
    }

    let stream = connect(pid)
        .ok_or_else(|| anyhow::anyhow!("could not open the control socket for {pid}"))?;

    let raw = crossterm::terminal::enable_raw_mode().is_ok();
    // The alternate screen keeps the agent's painting out of this shell's
    // scrollback, so detaching leaves the terminal as it was found.
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(b"\x1b[?1049h");
    let _ = stdout.flush();

    // Say how big we are before anything is drawn, so the replay arrives already
    // sized for this window.
    let mut input = stream.try_clone()?;
    if let Ok((cols, rows)) = crossterm::terminal::size() {
        let _ = input.write_all(&frame::size(frame::RESIZE, cols, rows));
    }

    // Keys, until the detach sequence. Shutting the socket down is what stops
    // the reader below, which is otherwise parked on a blocking read.
    {
        let mut input = input.try_clone()?;
        let socket = stream.try_clone()?;
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin();
            let mut buf = [0u8; 1024];
            while let Ok(n) = stdin.read(&mut buf) {
                // A terminal writes an escape sequence in one go, so looking for
                // the detach key within a single read is enough.
                if n == 0 || buf[..n].windows(DETACH.len()).any(|w| w == DETACH) {
                    break;
                }
                if input
                    .write_all(&frame::encode(frame::KEYS, &buf[..n]))
                    .and_then(|()| input.flush())
                    .is_err()
                {
                    break;
                }
            }
            let _ = socket.shutdown(std::net::Shutdown::Both);
        });
    }

    // The pty follows this window, within whatever the other viewers allow.
    {
        let mut input = input.try_clone()?;
        std::thread::spawn(move || {
            let mut last = crossterm::terminal::size().unwrap_or((80, 24));
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let Ok(size) = crossterm::terminal::size() else {
                    continue;
                };
                if size != last {
                    last = size;
                    if input
                        .write_all(&frame::size(frame::RESIZE, size.0, size.1))
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });
    }

    let mut reader = stream;
    let mut decoder = frame::Decoder::default();
    let mut buf = [0u8; 8192];
    'session: while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        decoder.push(&buf[..n]);
        // Size frames say nothing this terminal has to act on: the agent draws
        // within what it was given and the rest of the window stays blank.
        while let Some((kind, payload)) = decoder.next() {
            if kind == frame::OUTPUT
                && (stdout.write_all(&payload).is_err() || stdout.flush().is_err())
            {
                break 'session;
            }
        }
    }

    let _ = stdout.write_all(RESTORE);
    let _ = stdout.flush();
    if raw {
        let _ = crossterm::terminal::disable_raw_mode();
    }
    eprintln!("Detached from {pid}. It is still running — `cctop attach {pid}` comes back.");
    Ok(0)
}

/// The next frame that means something to a watcher. Unknown kinds are skipped
/// so a newer shim can add one without this end having to understand it.
fn read_event(decoder: &mut frame::Decoder) -> Option<Event> {
    loop {
        let (kind, payload) = decoder.next()?;
        match kind {
            frame::OUTPUT => return Some(Event::Output(payload)),
            frame::SIZE => {
                if let Some((cols, rows)) = frame::parse_size(&payload) {
                    return Some(Event::Size(cols, rows));
                }
            }
            _ => {}
        }
    }
}

/// The bytes a terminal would send for this key.
///
/// Only what an agent reads: text, the editing keys, and the arrows. Anything
/// unmapped is dropped rather than guessed at, since a wrong escape sequence is
/// worse in a live session than a key that did nothing.
fn encode(key: KeyEvent) -> Option<Vec<u8>> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let bytes = match key.code {
        // Ctrl-A..Ctrl-Z are the letter with the top three bits cleared, which
        // is also how Ctrl-C reaches the agent as a signal.
        KeyCode::Char(c) if ctrl && c.is_ascii_alphabetic() => {
            vec![c.to_ascii_lowercase() as u8 & 0x1f]
        }
        KeyCode::Char(c) => {
            let mut b = c.to_string().into_bytes();
            if alt {
                b.insert(0, 0x1b);
            }
            b
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        _ => return None,
    };
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_payload_is_read_or_refused() {
        assert_eq!(frame::parse_size(b"120 40"), Some((120, 40)));
        // A zero dimension is a screen nothing can be drawn on, and would have
        // the shim shrink the pty to it for every other viewer too.
        assert_eq!(frame::parse_size(b"0 40"), None);
        assert_eq!(frame::parse_size(b"something else"), None);
        assert_eq!(frame::parse_size(b""), None);
    }

    /// The stream carries keystrokes, which can be any byte at all, so frames
    /// are found by length alone — nothing may be inferred from the payload.
    #[test]
    fn frames_survive_a_stream_that_splits_them_anywhere() {
        let mut wire = Vec::new();
        wire.extend(frame::size(frame::SIZE, 100, 30));
        // A payload containing what a header looks like, and a naked NUL.
        wire.extend(frame::encode(
            frame::OUTPUT,
            b"o\x00\x00\x00\x09hello\x1b[A",
        ));
        wire.extend(frame::encode(frame::KEYS, b""));

        // Fed one byte at a time, which is the worst a socket can do to us.
        let mut decoder = frame::Decoder::default();
        let mut got = Vec::new();
        for byte in &wire {
            decoder.push(&[*byte]);
            while let Some(f) = decoder.next() {
                got.push(f);
            }
        }
        assert_eq!(
            got,
            vec![
                (frame::SIZE, b"100 30".to_vec()),
                (frame::OUTPUT, b"o\x00\x00\x00\x09hello\x1b[A".to_vec()),
                (frame::KEYS, Vec::new()),
            ]
        );

        // And arriving all at once.
        let mut decoder = frame::Decoder::default();
        decoder.push(&wire);
        assert_eq!(std::iter::from_fn(|| decoder.next()).count(), 3);
    }

    /// A watcher must not act on frames cut out of the middle of other frames,
    /// so a stream that stops making sense has to stop producing them.
    #[test]
    fn a_desynchronised_stream_yields_nothing_rather_than_garbage() {
        let mut decoder = frame::Decoder::default();
        decoder.push(&[frame::OUTPUT, 0xff, 0xff, 0xff, 0xff]);
        decoder.push(b"whatever follows");
        assert_eq!(decoder.next(), None);
        decoder.push(&frame::size(frame::SIZE, 80, 24));
        assert_eq!(decoder.next(), None);
    }

    /// Every one of these reaches a live agent, so a wrong byte is a wrong
    /// keystroke in someone's session — Ctrl-C especially.
    #[test]
    fn keys_encode_as_a_terminal_would_send_them() {
        let plain = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);

        assert_eq!(encode(plain('a')), Some(b"a".to_vec()));
        assert_eq!(encode(plain('é')), Some("é".as_bytes().to_vec()));
        assert_eq!(encode(ctrl('c')), Some(vec![3]));
        assert_eq!(encode(ctrl('C')), Some(vec![3]));
        // Enter is a carriage return on a pty, not a newline.
        assert_eq!(encode(key(KeyCode::Enter)), Some(b"\r".to_vec()));
        assert_eq!(encode(key(KeyCode::Backspace)), Some(vec![0x7f]));
        assert_eq!(encode(key(KeyCode::Up)), Some(b"\x1b[A".to_vec()));
        assert_eq!(encode(key(KeyCode::F(5))), None);
    }
}
