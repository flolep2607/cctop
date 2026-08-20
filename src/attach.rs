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

/// Lines of the agent's output cctop keeps behind the top of a pane.
///
/// Only ever scrolled by a pane with no multiplexer behind it: under tmux the
/// wheel goes to tmux, whose history is both deeper and cheaper. This one is not
/// cheap — every row is a full-width array of 32-byte cells, so a pane that has
/// scrolled this far has ~20MB behind it at a normal width. Enough to find what
/// just went past, which is what the wheel is reached for, and short of the cost
/// of keeping a session's whole life in memory when the transcript already has
/// it.
const SCROLLBACK: usize = 5_000;

/// Lines one notch of the wheel moves, matching what terminals themselves send
/// for a notch when an application is not reading the mouse.
const WHEEL_LINES: usize = 3;

/// A paste may be much larger than an ordinary key frame. Keep each write
/// comfortably below the wire format's one-megabyte safety limit, without
/// degrading it into the one-frame-per-character path used for keystrokes.
const PASTE_CHUNK: usize = 64 * 1024;

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
///
/// [`Attach::pump`] reads these on every platform, but only [`read_event`]
/// builds one and that is unix-only — there is no shim to hear from otherwise.
/// The type still has to exist, because `pump` and the `Attach` it belongs to do.
#[cfg_attr(not(unix), allow(dead_code))]
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
    ///
    /// Typing returns to the live screen first, the way every terminal does it:
    /// answering a prompt you scrolled away from must show you the answer, not
    /// leave you reading history while the agent moves on beneath it.
    pub fn send_key(&mut self, key: KeyEvent) -> bool {
        if self.parser.screen().scrollback() > 0 {
            self.parser.screen_mut().set_scrollback(0);
        }
        match encode(key) {
            Some(bytes) => self.send(&frame::encode(frame::KEYS, &bytes)),
            None => true,
        }
    }

    /// Paste `text` into the agent in one go. Returns false once the connection
    /// is gone.
    ///
    /// A paste arrives from crossterm as one string, and it has to leave here as
    /// one write too: sending it a keystroke at a time makes it crawl in, and
    /// every newline in it lands as the Enter that submits whatever has been
    /// typed so far — which is how pasting a five-line message into Claude Code
    /// used to ask it five questions.
    ///
    /// Whether the brackets go around it is the agent's decision, read the same
    /// way [`wheel`](Self::wheel) reads mouse tracking: from what the agent asked
    /// for in its own output. Nothing filters a pty, so an application that never
    /// enabled bracketed paste would read `\x1b[200~` as the literal text it
    /// spells and the paste would arrive with junk on both ends. Claude Code and
    /// every other agent's editor turn it on; a bare shell behind a pane does
    /// not, and that shell gets the plain text a terminal without the mode would
    /// have sent it.
    ///
    /// Two things are rewritten on the way through, both of which a real terminal
    /// also does. Newlines become carriage returns, because that is what Enter is
    /// on a pty. And an end marker occurring inside the pasted text is dropped:
    /// left in, it would close the bracket early and hand the remainder to the
    /// agent as keystrokes, which is the one way a paste can still submit itself.
    ///
    /// It leaves in `PASTE_CHUNK`-sized writes rather than one, because a
    /// clipboard has no size limit and the wire format does: a single frame past
    /// `MAX_FRAME` is how the decoder decides the stream is no longer frame
    /// aligned, and it never recovers from that. A paste smaller than a chunk —
    /// which is all of them, in practice — is still exactly one write.
    pub fn send_paste(&mut self, text: &str) -> bool {
        if self.parser.screen().scrollback() > 0 {
            self.parser.screen_mut().set_scrollback(0);
        }
        let body = text
            .replace("\x1b[201~", "")
            .replace("\r\n", "\r")
            .replace('\n', "\r");
        if body.is_empty() {
            return true;
        }
        let bytes = match self.parser.screen().bracketed_paste() {
            true => format!("\x1b[200~{body}\x1b[201~").into_bytes(),
            false => body.into_bytes(),
        };
        for chunk in bytes.chunks(PASTE_CHUNK) {
            if !self.send(&frame::encode(frame::KEYS, chunk)) {
                return false;
            }
        }
        true
    }

    /// Scroll the wheel at `(col, row)`, given pane-relative and zero-based.
    ///
    /// Goes to the agent only if the agent asked for mouse reporting — under
    /// tmux it always has, and scrolling is then tmux's copy-mode, which is the
    /// history the agent actually has. Nothing else may be sent one: a pty
    /// filters nothing, so an application that never enabled tracking would read
    /// the report as the keystrokes its bytes spell, and a wheel over a plain
    /// shell would type `[<64;3;9M` into it. The screen knows which it is
    /// because enabling tracking is something the agent did in its own output.
    ///
    /// Everyone else scrolls what cctop kept, which is what makes the wheel work
    /// over an agent that has no multiplexer behind it at all.
    ///
    /// The encoding is the agent's too. SGR is what everything modern selects —
    /// and the only one that survives past column 223 — but the default is still
    /// what an agent gets if it never asked for better.
    /// Report a button press, release or drag to the agent.
    ///
    /// Only when the agent asked for mouse reporting. Claude Code, opencode and
    /// pi all turn it on and use it — for the file picker, for placing the
    /// cursor in the composer, for the agents list — and until this existed a
    /// click inside a pane went nowhere, because cctop holds the terminal's
    /// mouse capture and had nothing to do with what it caught.
    ///
    /// What each mode wants is the whole reason this is not one sequence: an
    /// agent in `Press` mode asked for presses and gets confused by releases it
    /// never requested, and drags belong only to the motion modes. Reporting
    /// more than was asked for is how a click turns into stray text.
    ///
    /// `false` means the agent is gone.
    pub fn mouse(&mut self, kind: MouseKind, button: MouseButton, col: u16, row: u16) -> bool {
        let screen = self.parser.screen();
        let Some(bytes) = encode_mouse(
            screen.mouse_protocol_mode(),
            screen.mouse_protocol_encoding(),
            kind,
            button,
            col,
            row,
        ) else {
            // Either the agent never asked for this event, or the encoding
            // cannot name where it happened. Both are silence, not failure.
            return true;
        };
        self.send(&frame::encode(frame::KEYS, &bytes))
    }

    pub fn wheel(&mut self, up: bool, col: u16, row: u16) -> bool {
        use vt100::{MouseProtocolEncoding as Enc, MouseProtocolMode as Mode};
        if self.parser.screen().mouse_protocol_mode() == Mode::None {
            let at = self.parser.screen().scrollback();
            let to = match up {
                true => at + WHEEL_LINES,
                false => at.saturating_sub(WHEEL_LINES),
            };
            // Clamped by the screen to what there is, so scrolling past the top
            // of a short history stops there rather than emptying the pane.
            self.parser.screen_mut().set_scrollback(to);
            return true;
        }
        // 64 and 65 are the wheel's two buttons in every xterm encoding: bit 6
        // marks the event as a wheel rather than a button, and the low bit is
        // the direction.
        let button = 64 + u16::from(!up);
        let bytes = match self.parser.screen().mouse_protocol_encoding() {
            Enc::Sgr => format!("\x1b[<{button};{};{}M", col + 1, row + 1).into_bytes(),
            // One printable byte each, biased by 32, so anything past column 223
            // cannot be said at all. Reporting it at the edge would name the
            // wrong cell; dropping it costs a scroll the agent could not have
            // placed correctly anyway.
            _ => {
                if col > 222 || row > 222 {
                    return true;
                }
                let cell = |v: u16| u8::try_from(v + 33).unwrap_or(u8::MAX);
                vec![
                    0x1b,
                    b'[',
                    b'M',
                    u8::try_from(button + 32).unwrap_or(u8::MAX),
                    cell(col),
                    cell(row),
                ]
            }
        };
        self.send(&frame::encode(frame::KEYS, &bytes))
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

/// The bytes an agent expects for one mouse event, or `None` when it should not
/// be told at all.
///
/// What each mode wants is the whole reason this is not one sequence: an agent
/// in `Press` mode asked for presses and would read a release it never
/// requested as stray input, and drags belong only to the motion modes.
fn encode_mouse(
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
    kind: MouseKind,
    button: MouseButton,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    use vt100::{MouseProtocolEncoding as Enc, MouseProtocolMode as Mode};
    let wanted = match kind {
        MouseKind::Press => mode != Mode::None,
        MouseKind::Release => matches!(
            mode,
            Mode::PressRelease | Mode::ButtonMotion | Mode::AnyMotion
        ),
        MouseKind::Drag => matches!(mode, Mode::ButtonMotion | Mode::AnyMotion),
    };
    if !wanted {
        return None;
    }
    let code = match button {
        MouseButton::Left => 0u16,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    // Bit 5 marks motion, which is what separates a drag from the press that
    // began it.
    let code = code
        + if matches!(kind, MouseKind::Drag) {
            32
        } else {
            0
        };
    match encoding {
        // The only encoding that can say *which* button came up; the final
        // letter carries press against release.
        Enc::Sgr => {
            let end = match kind {
                MouseKind::Release => 'm',
                _ => 'M',
            };
            Some(format!("\x1b[<{code};{};{}{end}", col + 1, row + 1).into_bytes())
        }
        // One printable byte each, biased by 32, so nothing past column 223 can
        // be said at all — and naming the wrong cell is worse than staying
        // quiet, because a click acts where it lands.
        _ => {
            if col > 222 || row > 222 {
                return None;
            }
            // Legacy has no per-button release: 3 means "whatever was held is
            // now up", which is all the encoding can express.
            let code = match kind {
                MouseKind::Release => 3,
                _ => code,
            };
            let cell = |v: u16| u8::try_from(v + 33).unwrap_or(u8::MAX);
            Some(vec![
                0x1b,
                b'[',
                b'M',
                u8::try_from(code + 32).unwrap_or(u8::MAX),
                cell(col),
                cell(row),
            ])
        }
    }
}

/// Which of a button's events this is.
///
/// crossterm's `MouseEventKind` also carries the wheel and bare movement, which
/// are reported by [`Attach::wheel`] and deliberately not at all respectively —
/// capture delivers motion continuously and an agent in a press-only mode has
/// no use for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseKind {
    Press,
    Release,
    Drag,
}

/// The three buttons xterm can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    /// Encoded but never sent: `on_mouse` drops the right button before it gets
    /// here, because under tmux it opens tmux's own pane menu over the agent.
    /// The arm stays so the encoding is complete if that ever changes — hence
    /// `allow` rather than removing the variant.
    #[allow(dead_code)]
    Right,
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
        parser: vt100::Parser::new(size.1, size.0, SCROLLBACK),
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
///
/// Every caller is unix-only, because every one of them is reading a shim's
/// socket.
#[cfg(unix)]
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
            let control = c.to_ascii_lowercase() as u8 & 0x1f;
            match alt {
                true => vec![0x1b, control],
                false => vec![control],
            }
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
        // Ctrl-Backspace is the other delete-word, and terminals send it as
        // 0x17 — the same byte Ctrl-W would, which is what readline and every
        // agent's editor listen for.
        KeyCode::Backspace if ctrl => vec![0x17],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Esc => vec![0x1b],
        // Arrows and the editing keys carry their modifiers in the sequence.
        // Sending the bare form threw them away, so Ctrl-Left arrived as a plain
        // Left and moved one character where the user asked for a word.
        KeyCode::Up => csi_letter(b'A', key.modifiers),
        KeyCode::Down => csi_letter(b'B', key.modifiers),
        KeyCode::Right => csi_letter(b'C', key.modifiers),
        KeyCode::Left => csi_letter(b'D', key.modifiers),
        KeyCode::Home => csi_letter(b'H', key.modifiers),
        KeyCode::End => csi_letter(b'F', key.modifiers),
        KeyCode::Insert => csi_tilde(2, key.modifiers),
        KeyCode::Delete => csi_tilde(3, key.modifiers),
        KeyCode::PageUp => csi_tilde(5, key.modifiers),
        KeyCode::PageDown => csi_tilde(6, key.modifiers),
        _ => return None,
    };
    Some(bytes)
}

/// xterm's modifier parameter: 1 plus a bit per held modifier. 1 means none,
/// and a sequence carrying it is written without the parameter at all — which
/// is the form every terminal has always sent and some readers only accept.
fn modifier_param(mods: KeyModifiers) -> Option<u8> {
    let bits = (mods.contains(KeyModifiers::SHIFT) as u8)
        | ((mods.contains(KeyModifiers::ALT) as u8) << 1)
        | ((mods.contains(KeyModifiers::CONTROL) as u8) << 2);
    (bits != 0).then_some(bits + 1)
}

/// A cursor-key sequence: `\x1b[D`, or `\x1b[1;5D` when modifiers are held.
fn csi_letter(final_byte: u8, mods: KeyModifiers) -> Vec<u8> {
    match modifier_param(mods) {
        Some(param) => format!("\x1b[1;{param}{}", final_byte as char).into_bytes(),
        None => vec![0x1b, b'[', final_byte],
    }
}

/// A numbered editing key: `\x1b[3~`, or `\x1b[3;5~` with modifiers.
fn csi_tilde(number: u8, mods: KeyModifiers) -> Vec<u8> {
    match modifier_param(mods) {
        Some(param) => format!("\x1b[{number};{param}~").into_bytes(),
        None => format!("\x1b[{number}~").into_bytes(),
    }
}

#[cfg(test)]
mod tests {

    /// An agent is told only what it asked for. Reporting more than that is how
    /// a click becomes stray text in somebody's prompt: a press-only agent reads
    /// the release it never requested as input.
    #[test]
    fn an_agent_hears_only_the_events_it_asked_for() {
        use vt100::{MouseProtocolEncoding as Enc, MouseProtocolMode as Mode};
        let at = |mode, kind| encode_mouse(mode, Enc::Sgr, kind, MouseButton::Left, 0, 0);

        // Mouse off: nothing at all, whatever happens.
        assert!(at(Mode::None, MouseKind::Press).is_none());
        assert!(at(Mode::None, MouseKind::Release).is_none());

        // Presses only.
        assert!(at(Mode::Press, MouseKind::Press).is_some());
        assert!(at(Mode::Press, MouseKind::Release).is_none());
        assert!(at(Mode::Press, MouseKind::Drag).is_none());

        // Presses and releases, but a drag is motion and was not asked for.
        assert!(at(Mode::PressRelease, MouseKind::Release).is_some());
        assert!(at(Mode::PressRelease, MouseKind::Drag).is_none());

        // Motion modes take everything.
        assert!(at(Mode::ButtonMotion, MouseKind::Drag).is_some());
        assert!(at(Mode::AnyMotion, MouseKind::Drag).is_some());
    }

    /// SGR is one-based and can name which button was released; the legacy
    /// encoding is byte-biased and cannot, which is the whole reason both exist
    /// here rather than one being emitted for everybody.
    #[test]
    fn each_encoding_says_it_its_own_way() {
        use vt100::{MouseProtocolEncoding as Enc, MouseProtocolMode as Mode};
        let sgr = |kind, button, col, row| {
            String::from_utf8(
                encode_mouse(Mode::AnyMotion, Enc::Sgr, kind, button, col, row).expect("wanted"),
            )
            .unwrap()
        };
        assert_eq!(
            sgr(MouseKind::Press, MouseButton::Left, 0, 0),
            "\x1b[<0;1;1M"
        );
        assert_eq!(
            sgr(MouseKind::Press, MouseButton::Right, 9, 4),
            "\x1b[<2;10;5M"
        );
        // A release ends with `m`, and still names its button.
        assert_eq!(
            sgr(MouseKind::Release, MouseButton::Middle, 0, 0),
            "\x1b[<1;1;1m"
        );
        // A drag is the button with the motion bit set.
        assert_eq!(
            sgr(MouseKind::Drag, MouseButton::Left, 0, 0),
            "\x1b[<32;1;1M"
        );

        let legacy = |kind, button, col, row| {
            encode_mouse(Mode::AnyMotion, Enc::Utf8, kind, button, col, row)
        };
        assert_eq!(
            legacy(MouseKind::Press, MouseButton::Left, 0, 0),
            Some(vec![0x1b, b'[', b'M', 32, 33, 33])
        );
        // Button 3 is all a release can be said as here.
        assert_eq!(
            legacy(MouseKind::Release, MouseButton::Right, 0, 0),
            Some(vec![0x1b, b'[', b'M', 35, 33, 33])
        );
        // Past column 223 it would name the wrong cell, and a click acts where
        // it lands — so it says nothing instead.
        assert!(legacy(MouseKind::Press, MouseButton::Left, 300, 0).is_none());
        assert!(legacy(MouseKind::Press, MouseButton::Left, 0, 300).is_none());
    }
    use super::*;

    /// An `Attach` writing to memory instead of a socket, and the buffer it
    /// writes to.
    fn probe() -> (Attach, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
        #[derive(Clone)]
        struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = Sink(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())));
        let written = sink.0.clone();
        let attach = Attach {
            input: Box::new(sink),
            close: Box::new(|| {}),
            pending: std::sync::Arc::default(),
            closed: std::sync::Arc::default(),
            size: (80, 24),
            requested: (0, 0),
            parser: vt100::Parser::new(24, 80, SCROLLBACK),
        };
        (attach, written)
    }

    /// Nothing filters a pty, so an agent that never asked for mouse reporting
    /// would read a wheel report as the keystrokes its bytes spell.
    #[test]
    fn the_wheel_reaches_only_an_agent_that_asked_for_it() {
        let (mut attach, written) = probe();
        assert!(attach.wheel(true, 2, 8));
        assert!(written.lock().unwrap().is_empty());

        // 1000 turns tracking on, 1006 selects SGR — what tmux sends the moment
        // `mouse` is on, and what any modern application asks for.
        attach.parser.process(b"\x1b[?1000h\x1b[?1006h");
        assert!(attach.wheel(true, 2, 8));
        assert!(attach.wheel(false, 2, 8));
        // Coordinates are one-based on the wire; 64 is the wheel up, 65 down.
        assert_eq!(
            written.lock().unwrap().as_slice(),
            [
                frame::encode(frame::KEYS, b"\x1b[<64;3;9M"),
                frame::encode(frame::KEYS, b"\x1b[<65;3;9M"),
            ]
            .concat()
        );
    }

    #[test]
    fn a_paste_too_big_for_one_frame_is_split_rather_than_desynchronising() {
        let (mut attach, written) = probe();
        attach.parser.process(b"\x1b[?2004h");
        let text = "x".repeat(PASTE_CHUNK + 1);

        assert!(attach.send_paste(&text));
        let bytes = written.lock().unwrap();
        let mut decoder = frame::Decoder::default();
        decoder.push(&bytes);
        let frames: Vec<_> = std::iter::from_fn(|| decoder.next()).collect();

        // More than one frame, none of them near the cap that would make the
        // decoder give up on the stream, and the agent still reads one paste.
        assert!(frames.len() > 1, "sent as {} frame(s)", frames.len());
        assert!(frames.iter().all(|(kind, _)| *kind == frame::KEYS));
        let rejoined: Vec<u8> = frames.iter().flat_map(|(_, body)| body.clone()).collect();
        assert_eq!(rejoined, format!("\x1b[200~{text}\x1b[201~").into_bytes());
    }

    /// The default encoding spends one printable byte on each coordinate, so a
    /// position it cannot say is dropped rather than reported as another cell.
    #[test]
    fn a_wheel_in_the_default_encoding_says_what_it_can() {
        let (mut attach, written) = probe();
        attach.parser.process(b"\x1b[?1000h");
        assert!(attach.wheel(true, 2, 8));
        assert_eq!(
            written.lock().unwrap().as_slice(),
            frame::encode(frame::KEYS, b"\x1b[M\x60\x23\x29")
        );

        written.lock().unwrap().clear();
        assert!(attach.wheel(true, 300, 8));
        assert!(written.lock().unwrap().is_empty());
    }

    /// With no multiplexer behind a pane, the history the wheel moves through is
    /// the one cctop kept — and typing has to put the agent's live screen back,
    /// or an answer is given to a prompt that is no longer on screen.
    #[test]
    fn a_pane_with_no_one_reading_the_mouse_scrolls_what_cctop_kept() {
        let (mut attach, written) = probe();
        for i in 0..100 {
            attach.parser.process(format!("line {i}\r\n").as_bytes());
        }
        assert!(attach.wheel(true, 0, 0));
        assert!(attach.wheel(true, 0, 0));
        assert_eq!(attach.parser.screen().scrollback(), WHEEL_LINES * 2);
        // Nothing was sent: the agent never asked to hear about the mouse.
        assert!(written.lock().unwrap().is_empty());

        assert!(attach.wheel(false, 0, 0));
        assert_eq!(attach.parser.screen().scrollback(), WHEEL_LINES);
        attach.send_key(KeyEvent::from(KeyCode::Char('y')));
        assert_eq!(attach.parser.screen().scrollback(), 0);

        // The bottom is as far down as it goes, however hard the wheel is spun.
        assert!(attach.wheel(false, 0, 0));
        assert_eq!(attach.parser.screen().scrollback(), 0);
    }

    /// A paste has to leave as one write with one line ending per line, or the
    /// agent reads every newline in it as the Enter that submits the message.
    #[test]
    fn a_paste_reaches_an_agent_that_asked_for_brackets_as_one_paste() {
        let (mut attach, written) = probe();
        // 2004 is bracketed paste, which every agent's editor turns on.
        attach.parser.process(b"\x1b[?2004h");
        assert!(attach.send_paste("one\ntwo\r\nthree"));
        assert_eq!(
            written.lock().unwrap().as_slice(),
            frame::encode(frame::KEYS, b"\x1b[200~one\rtwo\rthree\x1b[201~")
        );
    }

    /// Nothing filters a pty, so a shell that never enabled the mode would read
    /// the markers as the text they spell and the paste would arrive with junk
    /// on both ends. It gets what a terminal without the mode would have sent.
    #[test]
    fn a_paste_to_an_agent_that_did_not_ask_carries_no_markers() {
        let (mut attach, written) = probe();
        assert!(attach.send_paste("ls -l\n"));
        assert_eq!(
            written.lock().unwrap().as_slice(),
            frame::encode(frame::KEYS, b"ls -l\r")
        );

        // Nothing to say means nothing on the wire.
        written.lock().unwrap().clear();
        assert!(attach.send_paste(""));
        assert!(written.lock().unwrap().is_empty());
    }

    /// An end marker inside the pasted text would close the bracket early and
    /// hand the remainder to the agent as keystrokes — the one way a bracketed
    /// paste can still submit itself.
    #[test]
    fn a_paste_cannot_close_its_own_bracket() {
        let (mut attach, written) = probe();
        attach.parser.process(b"\x1b[?2004h");
        assert!(attach.send_paste("hello\x1b[201~\nrm -rf /\n"));
        assert_eq!(
            written.lock().unwrap().as_slice(),
            frame::encode(frame::KEYS, b"\x1b[200~hello\rrm -rf /\r\x1b[201~")
        );
    }

    /// Pasting an answer to a prompt you scrolled away from must show you the
    /// answer, the same as typing one does.
    #[test]
    fn pasting_returns_to_the_live_screen() {
        let (mut attach, _written) = probe();
        for i in 0..100 {
            attach.parser.process(format!("line {i}\r\n").as_bytes());
        }
        assert!(attach.wheel(true, 0, 0));
        assert_eq!(attach.parser.screen().scrollback(), WHEEL_LINES);
        assert!(attach.send_paste("continue"));
        assert_eq!(attach.parser.screen().scrollback(), 0);
    }

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

    /// Word-wise movement is Ctrl held on an arrow, and it only works if the
    /// modifier survives the trip: the bare sequence is a plain arrow, which
    /// moves one character and looks like the key was ignored.
    #[test]
    fn modified_arrows_keep_their_modifier() {
        let with = |code, mods| encode(KeyEvent::new(code, mods));

        assert_eq!(
            with(KeyCode::Right, KeyModifiers::CONTROL),
            Some(b"\x1b[1;5C".to_vec())
        );
        assert_eq!(
            with(KeyCode::Left, KeyModifiers::CONTROL),
            Some(b"\x1b[1;5D".to_vec())
        );
        assert_eq!(
            with(KeyCode::Home, KeyModifiers::SHIFT),
            Some(b"\x1b[1;2H".to_vec())
        );
        assert_eq!(
            with(
                KeyCode::Up,
                KeyModifiers::CONTROL | KeyModifiers::SHIFT | KeyModifiers::ALT
            ),
            Some(b"\x1b[1;8A".to_vec())
        );
        assert_eq!(
            with(KeyCode::Delete, KeyModifiers::CONTROL),
            Some(b"\x1b[3;5~".to_vec())
        );
        // Unmodified keys keep the plain form every terminal has always sent.
        assert_eq!(
            with(KeyCode::Delete, KeyModifiers::NONE),
            Some(b"\x1b[3~".to_vec())
        );
        // Ctrl-Backspace is delete-word, which readline reads as Ctrl-W.
        assert_eq!(
            with(KeyCode::Backspace, KeyModifiers::CONTROL),
            Some(vec![0x17])
        );
        // Alt with a control character is the escape prefix, as in Alt-Ctrl-A.
        assert_eq!(
            with(
                KeyCode::Char('a'),
                KeyModifiers::CONTROL | KeyModifiers::ALT
            ),
            Some(vec![0x1b, 1])
        );
    }
}
