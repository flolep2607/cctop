//! `cctop run <command…>`: launch an agent on a pty cctop owns.
//!
//! Nothing can type into a terminal it doesn't hold the master side of, and the
//! master belongs to whichever process created the pty — normally the terminal
//! emulator, which offers no way in. Launching the agent from here moves that
//! master into a process that does: this one, which proxies the real terminal
//! byte-for-byte and listens on a unix socket for lines to type on the agent's
//! behalf.
//!
//! It is tmux's trick minus the multiplexing, so the session behaves exactly
//! like one started directly and needs no root and no kernel opt-in.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};

/// How often the outer terminal's size is compared against the pty's.
///
/// A SIGWINCH handler would be exact, but a signal handler can only touch
/// async-signal-safe state; polling twice a second costs one `ioctl` and keeps
/// the resize path ordinary code. Resizes are rare and human-paced.
const RESIZE_POLL: std::time::Duration = std::time::Duration::from_millis(500);

/// Run `argv` on a pty this process owns, proxying the real terminal.
///
/// Returns the child's exit code so `cctop run claude` is a transparent stand-in
/// for `claude` in a shell alias.
pub fn run(argv: &[String]) -> anyhow::Result<i32> {
    if argv.is_empty() {
        anyhow::bail!("usage: cctop run <command> [args…]  (e.g. cctop run claude)");
    }
    let (mut child, master) = spawn_on_pty(argv, None)?;
    let pid = child.id();

    // Bind before entering raw mode: a failure here should print normally.
    let listener = listen(pid)?;
    let socket = socket_path(pid);

    let raw = crossterm::terminal::enable_raw_mode().is_ok();
    // Backgrounded (`setsid cctop claude </dev/null >/dev/null &`), there is no
    // window here and nobody reading it, so this end gets no say in the size —
    // a watcher would otherwise be held to a terminal that does not exist.
    let has_window = std::io::IsTerminal::is_terminal(&std::io::stdout());
    let local = match has_window {
        true => crossterm::terminal::size().unwrap_or((80, 24)),
        false => (0, 0),
    };
    let fan = Arc::new(Mutex::new(Fanout::new(master.try_clone()?, local)));
    {
        let (master, fan) = (master.try_clone()?, Arc::clone(&fan));
        std::thread::spawn(move || pump_output(master, fan, Echo::Yes));
    }
    {
        let master = master.try_clone()?;
        std::thread::spawn(move || pump_input(master));
    }
    if has_window {
        let fan = Arc::clone(&fan);
        std::thread::spawn(move || watch_resize(fan));
    }
    let control = master.try_clone()?;
    std::thread::spawn(move || serve(listener, control, fan));

    let status = child.wait();
    if raw {
        let _ = crossterm::terminal::disable_raw_mode();
    }
    if let Some(socket) = socket {
        let _ = std::fs::remove_file(socket);
    }
    Ok(status?.code().unwrap_or(1))
}

/// An agent on a pty this process owns, drawing for whoever attaches rather
/// than for a terminal.
///
/// Its life is this process's: the pty master closes when cctop exits, and the
/// agent is hung up on. That is the same bargain `run` makes with the window it
/// was started in.
pub struct Hosted {
    /// The agent's pid, which is also what its socket is named after.
    pub pid: u32,
    /// What to call it on screen until its own session row shows up.
    pub label: String,
    child: std::process::Child,
    socket: Option<PathBuf>,
}

impl Hosted {
    /// The agent's exit code, once it has one.
    pub fn finished(&mut self) -> Option<i32> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code().unwrap_or(1)),
            // A child that cannot be waited on is one we cannot report on
            // either, and leaving cctop running for it would hang the session.
            Err(_) => Some(1),
            Ok(None) => None,
        }
    }
}

impl Drop for Hosted {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(socket) = &self.socket {
            let _ = std::fs::remove_file(socket);
        }
    }
}

/// Run `argv` on a pty this process owns without taking the terminal.
///
/// The difference from [`run`] is what is *not* here: no echo to stdout, no
/// keyboard, no raw mode, and no size of its own. `cctop <agent>` draws the
/// agent inside its own interface, so a shim also painting it would be two
/// programs writing to one screen.
///
/// `cwd` is where the agent starts, which for a tab opened from a session's row
/// is that session's project rather than wherever cctop was launched.
pub fn host(argv: &[String], cwd: Option<&std::path::Path>) -> anyhow::Result<Hosted> {
    if argv.is_empty() {
        anyhow::bail!("usage: cctop <command> [args…]  (e.g. cctop claude)");
    }
    let (child, master) = spawn_on_pty(argv, cwd)?;
    let pid = child.id();
    let listener = listen(pid)?;
    let socket = socket_path(pid);

    // No window here at all, so the panel that attaches sets the size alone.
    let fan = Arc::new(Mutex::new(Fanout::new(master.try_clone()?, (0, 0))));
    {
        let (master, fan) = (master.try_clone()?, Arc::clone(&fan));
        std::thread::spawn(move || pump_output(master, fan, Echo::No));
    }
    std::thread::spawn(move || serve(listener, master, fan));
    // The command as typed, minus any path, which is what the user called it.
    let label = argv
        .iter()
        .map(|arg| arg.rsplit('/').next().unwrap_or(arg))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(Hosted {
        pid,
        label,
        child,
        socket,
    })
}

/// Whether `word` names something a shell could run, which is how `cctop claude`
/// is told apart from a mistyped flag or subcommand.
///
/// Resolved the way `execvp` does: a word containing a separator is a path taken
/// as given, a bare word is looked up in `PATH`. The executable bit matters —
/// without it `cctop notes.txt` would exec and fail instead of reaching clap's
/// usage error.
pub fn is_command(word: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let executable = |p: PathBuf| {
        std::fs::metadata(&p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if word.contains('/') {
        return executable(PathBuf::from(word));
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .any(|dir| executable(dir.join(word)))
}

/// The control socket for the agent running as `pid`, if a base directory exists.
///
/// Named after the agent rather than the shim so the UI, which knows a session
/// only by its agent PID, can find it with one lookup.
pub fn socket_path(pid: u32) -> Option<PathBuf> {
    Some(base_dir()?.join(format!("{pid}.sock")))
}

/// Runtime dir when there is one (cleaned on logout, already user-private),
/// otherwise the cache dir.
fn base_dir() -> Option<PathBuf> {
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .map(|d| d.join("cctop"))
}

/// PIDs of the agents currently reachable through a shim, oldest socket first.
///
/// A socket whose shim has exited refuses connections rather than disappearing —
/// the file only goes away if `cctop run` got to clean up — so liveness is tested
/// by connecting, and the leftovers are swept up on the way past. Connecting is
/// also the honest test: a pid can be recycled, but a socket cannot be.
pub fn sessions() -> Vec<u32> {
    let Some(dir) = base_dir() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, u32)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(pid) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".sock"))
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        if UnixStream::connect(&path).is_err() {
            let _ = std::fs::remove_file(&path);
            continue;
        }
        let created = entry
            .metadata()
            .and_then(|m| m.created().or_else(|_| m.modified()))
            .unwrap_or(std::time::UNIX_EPOCH);
        found.push((created, pid));
    }
    found.sort_unstable();
    found.into_iter().map(|(_, pid)| pid).collect()
}

fn listen(pid: u32) -> anyhow::Result<std::os::unix::net::UnixListener> {
    let path = socket_path(pid).ok_or_else(|| anyhow::anyhow!("no runtime or cache directory"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        // The socket is a typing channel into a live agent: keep the directory
        // owner-only even when it lands in a shared cache root.
        let _ =
            std::fs::set_permissions(parent, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    // A shim that died without cleaning up leaves a file that would block bind.
    let _ = std::fs::remove_file(&path);
    Ok(std::os::unix::net::UnixListener::bind(&path)?)
}

/// First bytes of a connection that wants to *watch* the agent, not just type
/// into it. A connection that doesn't send it stays what it always was — bytes
/// in, nothing back — so an older cctop still injects into a newer shim.
pub const ATTACH_MAGIC: &[u8] = b"\x00cctop-attach\n";

/// How much recent output is kept for a client that connects mid-session. An
/// agent's TUI redraws its whole screen often, so replaying this is enough to
/// rebuild what's on it without waiting for the next repaint.
const REPLAY_BYTES: usize = 256 * 1024;

/// One watcher: where to send the pty's output, and how much room it has.
struct Sub {
    id: u64,
    tx: SyncSender<Vec<u8>>,
    size: (u16, u16),
}

/// Everything the pty has said lately, everyone listening, and the size that
/// suits them all.
struct Fanout {
    /// Kept for setting the pty's size, which is the one thing here that must
    /// reach the agent rather than a watcher.
    master: File,
    recent: Vec<u8>,
    subs: Vec<Sub>,
    next_id: u64,
    /// The window `cctop run` was started in, which is a viewer like any other
    /// and the only one when nobody has attached.
    local: (u16, u16),
    /// What the pty was last set to, so an unchanged size costs no ioctl. Every
    /// change is a SIGWINCH and a full repaint for the agent.
    applied: (u16, u16),
}

impl Fanout {
    fn new(master: File, local: (u16, u16)) -> Self {
        Self {
            recent: Vec::new(),
            subs: Vec::new(),
            next_id: 0,
            local,
            applied: pty_size(&master),
            master,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.recent.extend_from_slice(chunk);
        if self.recent.len() > REPLAY_BYTES {
            self.recent.drain(..self.recent.len() - REPLAY_BYTES);
        }
        let frame = crate::attach::frame::encode(crate::attach::frame::OUTPUT, chunk);
        let before = self.subs.len();
        // ponytail: a subscriber that falls a channel behind is dropped rather
        // than slowing the agent's own terminal down. It reconnects and gets the
        // replay. Buffer per subscriber if that ever proves too twitchy.
        self.subs
            .retain(|sub| sub.tx.try_send(frame.clone()).is_ok());
        // A watcher that went away may have been the one holding the pty small.
        if self.subs.len() != before {
            self.fit();
        }
    }

    fn set_local(&mut self, size: (u16, u16)) {
        self.local = size;
        self.fit();
    }

    fn remove(&mut self, id: u64) {
        self.subs.retain(|sub| sub.id != id);
        self.fit();
    }

    fn set_sub_size(&mut self, id: u64, size: (u16, u16)) {
        if let Some(sub) = self.subs.iter_mut().find(|sub| sub.id == id) {
            sub.size = size;
        }
        self.fit();
    }

    /// Resize the pty to the largest screen every viewer can show in full.
    ///
    /// A pty has one size and the agent draws to its edges, so with viewers of
    /// different sizes something has to give: either the small ones crop what
    /// they cannot fit, or everyone gets the small one's dimensions. tmux takes
    /// the second, and so does this — a cropped agent hides exactly the thing
    /// you attached to read, the prompt at the bottom of its screen. The window
    /// `cctop run` was started in shrinks along with the rest and is restored
    /// when the watcher detaches.
    fn fit(&mut self) {
        let size = std::iter::once(self.local)
            .chain(self.subs.iter().map(|sub| sub.size))
            .filter(|&(cols, rows)| cols > 0 && rows > 0)
            .fold(None::<(u16, u16)>, |acc, (cols, rows)| match acc {
                Some((c, r)) => Some((c.min(cols), r.min(rows))),
                None => Some((cols, rows)),
            });
        let Some((cols, rows)) = size else { return };
        if (cols, rows) == self.applied {
            return;
        }
        self.applied = (cols, rows);
        let ws = winsize(cols, rows);
        // SAFETY: `master` is a live pty master and `ws` outlives the call.
        unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &ws) };
        // Watchers size their own screens from this, not from what they asked
        // for, since what they asked for is only ever an upper bound.
        let frame = crate::attach::frame::size(crate::attach::frame::SIZE, cols, rows);
        self.subs
            .retain(|sub| sub.tx.try_send(frame.clone()).is_ok());
    }
}

fn locked(fan: &Mutex<Fanout>) -> std::sync::MutexGuard<'_, Fanout> {
    // A panicking subscriber thread must not take the agent's output with it.
    fan.lock().unwrap_or_else(|e| e.into_inner())
}

/// Type whatever each connection sends into the pty, and stream the pty back to
/// the ones that asked for it.
///
/// Deliberately unauthenticated: the socket sits in an owner-only directory, so
/// anyone who can reach it can already type into the terminal by other means.
fn serve(listener: std::os::unix::net::UnixListener, master: File, fan: Arc<Mutex<Fanout>>) {
    for stream in listener.incoming().flatten() {
        let Ok(master) = master.try_clone() else {
            continue;
        };
        let fan = Arc::clone(&fan);
        // A watcher holds its connection open for the life of the session, so
        // connections can't be served one after another on this thread.
        std::thread::spawn(move || converse(stream, master, fan));
    }
}

fn converse(mut stream: UnixStream, mut master: File, fan: Arc<Mutex<Fanout>>) {
    use crate::attach::frame;

    let mut buf = [0u8; 4096];
    let mut first = true;
    // Some once the connection has identified itself as a watcher, at which
    // point what it sends stops being raw bytes and starts being frames.
    let mut watcher: Option<(u64, frame::Decoder)> = None;

    'connection: while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        let mut bytes = &buf[..n];
        // The magic is written on its own, and a write that small arrives whole,
        // so testing the first read for it is enough.
        if std::mem::take(&mut first)
            && let Some(rest) = bytes.strip_prefix(ATTACH_MAGIC)
        {
            let Some(id) = subscribe(&stream, &fan) else {
                break;
            };
            watcher = Some((id, frame::Decoder::default()));
            bytes = rest;
        }
        let Some((id, decoder)) = watcher.as_mut() else {
            // An injector: everything it sends is meant for the keyboard.
            if !bytes.is_empty() && (master.write_all(bytes).is_err() || master.flush().is_err()) {
                break;
            }
            continue;
        };
        decoder.push(bytes);
        while let Some((kind, payload)) = decoder.next() {
            let delivered = match kind {
                frame::KEYS => master
                    .write_all(&payload)
                    .and_then(|()| master.flush())
                    .is_ok(),
                frame::RESIZE => {
                    if let Some(size) = frame::parse_size(&payload) {
                        locked(&fan).set_sub_size(*id, size);
                    }
                    true
                }
                // Unknown kinds are skipped rather than fatal, so a newer cctop
                // can add one and still drive a shim from an older release.
                _ => true,
            };
            if !delivered {
                break 'connection;
            }
        }
    }

    // Drop the subscription now rather than when the next chunk finds it dead:
    // this watcher may be the one keeping the pty small, and the window the
    // agent runs in should come back the moment it detaches.
    if let Some((id, _)) = watcher {
        locked(&fan).remove(id);
    }
}

/// Send this client the recent output and everything that follows, and return
/// the id its size is tracked under.
fn subscribe(stream: &UnixStream, fan: &Arc<Mutex<Fanout>>) -> Option<u64> {
    use crate::attach::frame;

    let mut out = stream.try_clone().ok()?;
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(256);
    let id = {
        // Held across the replay write so no chunk slips in between the replay
        // and the subscription, which would reorder the client's screen.
        let mut fan = locked(fan);
        let (cols, rows) = fan.applied;
        // Size first: the replay is drawn for these dimensions, so a watcher
        // that folded it into a differently sized screen would wrap every line
        // in the wrong place.
        let header = frame::size(frame::SIZE, cols, rows);
        let replay = frame::encode(frame::OUTPUT, &fan.recent);
        if out.write_all(&header).is_err() || out.write_all(&replay).is_err() {
            return None;
        }
        let id = fan.next_id;
        fan.next_id += 1;
        // No size yet: a watcher has no say until it asks for one, so attaching
        // alone never resizes the agent.
        fan.subs.push(Sub {
            id,
            tx,
            size: (0, 0),
        });
        id
    };
    std::thread::spawn(move || {
        while let Ok(chunk) = rx.recv() {
            if out.write_all(&chunk).is_err() {
                break;
            }
        }
    });
    Some(id)
}

/// The pty's own geometry, which a watcher needs to wrap lines where the agent
/// wrapped them. Asked of the pty rather than of this process's terminal: they
/// agree, but only one of them is what the agent was told.
fn pty_size(master: &File) -> (u16, u16) {
    let mut ws = winsize(80, 24);
    // SAFETY: a live pty master fd and a winsize that outlives the call.
    if unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCGWINSZ as _, &raw mut ws) } == -1 {
        return (80, 24);
    }
    (ws.ws_col, ws.ws_row)
}

/// Spawn `argv` with a new pty as its controlling terminal, returning the master.
fn spawn_on_pty(
    argv: &[String],
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<(std::process::Child, File)> {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut size = winsize(cols, rows);
    let mut master_fd = -1;
    let mut slave_fd = -1;
    // SAFETY: both fds are written by openpty and only read after it succeeds.
    //
    // The trailing two arguments are `*mut` on Apple and `*const` on Linux, so
    // they are passed as `*mut` and left to coerce; writing `*const` compiles
    // only on Linux.
    let rc = unsafe {
        libc::openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null_mut::<libc::termios>(),
            &raw mut size,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: openpty just handed us both fds and nothing else owns them.
    let master = unsafe { File::from_raw_fd(master_fd) };
    let slave = unsafe { File::from_raw_fd(slave_fd) };

    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    // The child is on a pty of ours, which is not a tmux pane whatever the
    // terminal cctop was started from happened to be. Inheriting `$TMUX` would
    // tell it otherwise: any tmux integration it attempted would address the
    // pane cctop is drawn in rather than its own, and `tmux new-session` — how a
    // tab hands its agent to tmux — refuses outright inside one.
    cmd.env_remove("TMUX");
    cmd.env_remove("TMUX_PANE");
    // A directory that has since been removed would fail the spawn outright, so
    // an unusable one is simply not applied.
    if let Some(cwd) = cwd.filter(|dir| dir.is_dir()) {
        cmd.current_dir(cwd);
    }
    // SAFETY: runs between fork and exec, so only async-signal-safe calls are
    // allowed — these are all raw syscalls with no allocation.
    unsafe {
        cmd.pre_exec(move || {
            // A new session plus TIOCSCTTY makes the pty the child's controlling
            // terminal, without which it gets no SIGWINCH and no job control.
            if libc::setsid() == -1 || libc::ioctl(slave_fd, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            for target in 0..3 {
                if libc::dup2(slave_fd, target) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            if slave_fd > 2 {
                libc::close(slave_fd);
            }
            libc::close(master_fd);
            Ok(())
        })
    };
    let child = cmd.spawn()?;
    // The parent must drop its slave handle or reads on the master never see EOF
    // when the child exits.
    drop(slave);
    Ok((child, master))
}

/// An agent on a pty with a live control socket, as `run` would leave one, minus
/// the parts that need a real terminal. Returns the child and the pid its socket
/// is named after.
///
/// Here rather than in the tests that use it because everything it touches is
/// private to this module, and one of those tests lives in the UI.
#[cfg(test)]
pub(crate) fn test_session(argv: &[&str], local: (u16, u16)) -> (std::process::Child, u32) {
    let argv: Vec<String> = argv.iter().map(|s| (*s).to_string()).collect();
    let (child, master) = spawn_on_pty(&argv, None).expect("pty child");
    let pid = child.id();
    let listener = listen(pid).expect("control socket");
    let fan = Arc::new(Mutex::new(Fanout::new(
        master.try_clone().expect("master"),
        local,
    )));
    locked(&fan).fit();
    {
        let (master, fan) = (master.try_clone().expect("master"), Arc::clone(&fan));
        std::thread::spawn(move || pump_output(master, fan, Echo::No));
    }
    std::thread::spawn(move || serve(listener, master, fan));
    (child, pid)
}

/// Whether the agent's output is also this process's output. It is for `run`,
/// which stands in for the agent; it is not for `host`, whose stdout belongs to
/// the UI drawing the agent.
#[derive(PartialEq)]
enum Echo {
    Yes,
    No,
}

fn pump_output(mut master: File, fan: Arc<Mutex<Fanout>>, echo: Echo) {
    let mut out = std::io::stdout();
    let mut buf = [0u8; 8192];
    // Flush every chunk: a TUI's escape sequences must not sit in a line buffer
    // waiting for a newline that never comes.
    while let Ok(n) = master.read(&mut buf) {
        if n == 0 {
            break;
        }
        if echo == Echo::Yes && (out.write_all(&buf[..n]).is_err() || out.flush().is_err()) {
            break;
        }
        locked(&fan).push(&buf[..n]);
    }
}

fn pump_input(mut master: File) {
    let mut stdin = std::io::stdin();
    let mut buf = [0u8; 1024];
    while let Ok(n) = stdin.read(&mut buf) {
        if n == 0 || master.write_all(&buf[..n]).is_err() {
            break;
        }
    }
}

/// Keep the fanout told how big the window `cctop run` was started in is. It
/// decides what the pty does with that, since a watcher may be smaller.
fn watch_resize(fan: Arc<Mutex<Fanout>>) {
    let mut last = crossterm::terminal::size().unwrap_or((80, 24));
    loop {
        std::thread::sleep(RESIZE_POLL);
        let Ok(size) = crossterm::terminal::size() else {
            continue;
        };
        if size != last {
            last = size;
            locked(&fan).set_local(size);
        }
    }
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

// The whole module is Linux-only: see the note on the round-trip test below.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// `cctop <word> …` shims the word only when it is really a command, so this
    /// is what stands between a typo and an exec attempt.
    #[test]
    fn a_command_is_told_apart_from_a_stray_word() {
        assert!(is_command("sh"));
        assert!(is_command("/bin/sh"));
        assert!(!is_command("cctop-no-such-command"));
        // A readable file that isn't executable, reached by path.
        assert!(!is_command("/etc/hostname"));
    }

    /// The point of the shim is that a line handed to the socket arrives as the
    /// child's keyboard input. `run` itself can't be tested — it seizes the
    /// terminal — so this drives the same pty and the same listener.
    ///
    /// Linux-only, and not because the code is: this hung indefinitely on the
    /// macOS CI runner, with the pty spawn the only unbounded call in it. The
    /// likely cause is `pre_exec` deadlocking in the forked child, which that API
    /// explicitly warns about, but it is unreproducible from a Linux host — so the
    /// test is pinned to where its result means something rather than left to
    /// stall every release. `cctop run` on macOS is unverified.
    #[test]
    fn a_line_sent_to_the_socket_becomes_the_childs_input() {
        let out = std::env::temp_dir().join("cctop-shim-test.txt");
        let _ = std::fs::remove_file(&out);
        let (mut child, master) = spawn_on_pty(
            &[
                "sh".into(),
                "-c".into(),
                format!("tee {} >/dev/null; :", out.display()),
            ],
            None,
        )
        .unwrap();
        let pid = child.id();
        let listener = listen(pid).unwrap();
        let fan = Arc::new(Mutex::new(Fanout::new(
            master.try_clone().unwrap(),
            (80, 24),
        )));
        std::thread::spawn(move || serve(listener, master, fan));

        // Through the public entry point, so the socket lookup is covered too.
        let sent = crate::inject::send_line(pid, "continue");

        let text = (0..50).find_map(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            std::fs::read_to_string(&out).ok().filter(|t| !t.is_empty())
        });
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&out);
        let _ = socket_path(pid).map(std::fs::remove_file);

        sent.unwrap();
        // The pty's default line discipline turns the CR we send into the
        // newline that completes the child's line, exactly as a real Enter does.
        assert_eq!(text.unwrap().trim_end(), "continue");
    }

    /// Attaching is the protocol's other half: a watcher must be handed what the
    /// agent has already drawn, keep receiving what it draws next, and still be
    /// able to type on the same connection.
    #[test]
    fn a_watcher_gets_the_screen_and_can_still_type() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let (mut child, master) = spawn_on_pty(
            &[
                "sh".into(),
                "-c".into(),
                "printf 'ALREADY-DRAWN\\r\\n'; cat".into(),
            ],
            None,
        )
        .unwrap();
        let pid = child.id();
        let listener = listen(pid).unwrap();
        let fan = Arc::new(Mutex::new(Fanout::new(
            master.try_clone().unwrap(),
            (80, 24),
        )));
        for spawn in [0, 1] {
            let (master, fan) = (master.try_clone().unwrap(), Arc::clone(&fan));
            match spawn {
                0 => std::thread::spawn(move || pump_output(master, fan, Echo::No)),
                _ => {
                    let listener = listener.try_clone().unwrap();
                    std::thread::spawn(move || serve(listener, master, fan))
                }
            };
        }

        // Attach only after the child has spoken, so what arrives can only have
        // come from the replay buffer.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let mut attach = crate::attach::attach(pid).expect("no attach connection");
        let screen = |attach: &mut crate::attach::Attach, want: &str| {
            (0..50).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(100));
                attach.pump();
                attach.parser.screen().contents().contains(want)
            })
        };
        let replayed = screen(&mut attach, "ALREADY-DRAWN");

        // `cat` echoes through the pty, so what comes back proves the keystroke
        // reached the child rather than just the socket.
        attach.send_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        attach.send_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let typed = screen(&mut attach, "z");

        let _ = child.kill();
        let _ = child.wait();
        let _ = socket_path(pid).map(std::fs::remove_file);

        assert!(
            replayed,
            "the replay did not carry what the child had drawn"
        );
        assert!(typed, "the keystroke never reached the child");
    }

    /// The point of the size negotiation: a watcher smaller than the window the
    /// agent was launched in pulls the pty down to its size, the agent is told,
    /// and detaching gives the window back what it had.
    #[test]
    fn a_watchers_size_reaches_the_agent_and_is_returned_when_it_leaves() {
        // `stty size` reports the pty's dimensions as the child sees them, so
        // this child narrates every resize it is given.
        let (mut child, master) = spawn_on_pty(
            &[
                "sh".into(),
                "-c".into(),
                "while :; do stty size; sleep 0.2; done".into(),
            ],
            None,
        )
        .unwrap();
        let pid = child.id();
        let listener = listen(pid).unwrap();
        let local = (100u16, 40u16);
        let fan = Arc::new(Mutex::new(Fanout::new(master.try_clone().unwrap(), local)));
        locked(&fan).fit();
        {
            let (master, fan) = (master.try_clone().unwrap(), Arc::clone(&fan));
            std::thread::spawn(move || pump_output(master, fan, Echo::No));
        }
        {
            let (master, fan) = (master.try_clone().unwrap(), Arc::clone(&fan));
            std::thread::spawn(move || serve(listener, master, fan));
        }

        let settled = |want: (u16, u16)| {
            (0..50).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(100));
                pty_size(&master) == want
            })
        };
        let started_at_local = settled(local);

        let mut attach = crate::attach::attach(pid).expect("no attach connection");
        attach.resize(60, 20);
        let shrank = settled((60, 20));
        // The agent is told, not just the pty: without SIGWINCH reaching it, a
        // TUI would go on painting at the old width.
        let agent_told = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            attach.pump();
            // stty prints rows first.
            attach.parser.screen().contents().contains("20 60")
        });
        // And the shim's own answer, which is what sizes the watcher's screen.
        let watcher_told = attach.size == (60, 20);

        drop(attach);
        let restored = settled(local);

        let _ = child.kill();
        let _ = child.wait();
        let _ = socket_path(pid).map(std::fs::remove_file);

        assert!(started_at_local, "the pty did not start at the local size");
        assert!(shrank, "the watcher's size never reached the pty");
        assert!(agent_told, "the agent was not told its new size");
        assert!(watcher_told, "the watcher was not told the granted size");
        assert!(restored, "detaching did not give the window its size back");
    }

    /// Same pty child, no listener: the shim backend must fall through rather
    /// than fail, and land on the TIOCSTI path — which, unprivileged, can only
    /// name the precondition it lacks. Lives here because `spawn_on_pty` is the
    /// only way to get a process on a pty we can name by PID.
    #[test]
    fn a_pty_child_without_a_socket_reports_the_missing_precondition() {
        if unsafe { libc::geteuid() } == 0 {
            eprintln!("skipping: running as root, so no precondition is missing");
            return;
        }
        let (mut child, _master) =
            spawn_on_pty(&["sh".into(), "-c".into(), "sleep 30".into()], None).expect("pty child");
        let error = crate::inject::send_line(child.id(), "continue").unwrap_err();
        let _ = child.kill();
        let _ = child.wait();
        // Which one depends on the host: EIO when dev.tty.legacy_tiocsti is off,
        // EPERM when it is on but this isn't our controlling terminal.
        assert!(
            error.contains("legacy_tiocsti") || error.contains("root"),
            "expected a named precondition, got: {error}"
        );
    }
}
