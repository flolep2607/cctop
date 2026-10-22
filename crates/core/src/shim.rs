//! `cctop run <command…>`: launch an agent on a pty cctop owns.
//!
//! Nothing can type into a terminal it doesn't hold the master side of, and the
//! master belongs to whichever process created the pty — normally the terminal
//! emulator, which offers no way in. Launching the agent from here moves that
//! master into a process that does: this one, which proxies the real terminal
//! byte-for-byte and listens on a unix socket for lines to type on the agent's
//! behalf.
//!
//! It is rmux's trick minus the multiplexing, so the session behaves exactly
//! like one started directly and needs no root and no kernel opt-in.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
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
    run_in(argv, None, &[], |_| {})
}

/// [`run`], started in `cwd` with `env` added to what the agent inherits.
///
/// `spawned` is handed the agent's pid as soon as it has one, for a caller
/// that has to be able to end the agent from elsewhere — `cctop sandbox` hangs
/// it up when cctop itself is told to stop, so its teardown still runs.
pub fn run_in(
    argv: &[String],
    cwd: Option<&std::path::Path>,
    env: &[(String, String)],
    spawned: impl FnOnce(u32),
) -> anyhow::Result<i32> {
    if argv.is_empty() {
        anyhow::bail!("usage: cctop run <command> [args…]  (e.g. cctop run claude)");
    }
    let size = crossterm::terminal::size().unwrap_or((80, 24));
    let (mut child, master) = spawn_on_pty_at_env(argv, cwd, size, env)?;
    let pid = child.id();
    spawned(pid);
    crate::elog::event(
        "shim",
        "spawn",
        serde_json::json!({ "mode": "run", "pid": pid, "cmd": argv.join(" ") }),
    );

    // Bind before entering raw mode: a failure here should print normally.
    let (listener, path) = listen(pid)?;
    // Held to the end of the function, so every `?` below unlinks it too.
    let _socket = Bound(Some(path.clone()));

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
    let stop = Arc::new(AtomicBool::new(false));
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
    let listener = Arc::new(listener);
    {
        let (listener, fan, stop) = (Arc::clone(&listener), Arc::clone(&fan), Arc::clone(&stop));
        std::thread::spawn(move || serve(listener, fan, stop));
    }
    // The shim's teardown, so the thread waiting for connections and the
    // control socket go with the agent rather than outliving it.
    let shim = Shim { stop, fan, path };

    let status = child.wait();
    crate::elog::event(
        "shim",
        "exit",
        serde_json::json!({
            "pid": pid,
            "code": status.as_ref().ok().and_then(|s| s.code()),
        }),
    );
    if raw {
        let _ = crossterm::terminal::disable_raw_mode();
    }
    // The shim's teardown, so the thread waiting for connections and the
    // control socket go with the agent rather than outliving it.
    shim.end();
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
    /// The shim's own teardown: the thread waiting for connections, the
    /// control socket and the pty master all go with the agent.
    shim: Shim,
    /// Dropped after [`Hosted::drop`] has reaped the agent, so the socket goes
    /// once nothing can be typed through it.
    _socket: Bound,
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

    /// How the agent ended, once it has: the whole status rather than
    /// [`Hosted::finished`]'s code, so a kill by signal can be told from an
    /// exit and said as one.
    pub fn exit_status(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }
}

impl Drop for Hosted {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // The shim's teardown: the thread waiting for connections, the
        // control socket and the pty master all go with the agent, rather
        // than being left for the next one to trip over.
        self.shim.end();
    }
}

/// The threads and the descriptors one shim holds, and the teardown that ends
/// them.
///
/// A hosted agent's shim belongs to the agent: when the agent ends, the
/// thread that waits for connections ends with it, and so do the control
/// socket it was listening on and the pty master it was holding. Without
/// this, every agent that ended left a thread blocked in `accept` for ever,
/// with the control socket open and the pty master in its hand — a thread
/// and three descriptors per agent, for as long as cctop ran.
struct Shim {
    /// Set when the shim is over, so the thread waiting for a connection can
    /// notice and end.
    stop: Arc<AtomicBool>,
    /// The fanout, which holds the pty master and the live connections: both
    /// are hung up when the agent ends.
    fan: Arc<Mutex<Fanout>>,
    /// Where the control socket is, to wake the accept loop.
    path: PathBuf,
}

impl Shim {
    /// End the shim: wake the accept loop, hang up the clients, and let the
    /// pty go.
    fn end(&self) {
        self.stop.store(true, Ordering::Relaxed);
        // The accept loop is parked in `accept`, and a closed listener is
        // what a parked `accept` needs to come back: a connection to the
        // socket is that, and the loop sees the flag and ends without
        // serving it.
        let _ = UnixStream::connect(&self.path);
        // The clients are hung up rather than left waiting: a watcher that is
        // not told the agent is gone waits for output that will never come.
        locked(&self.fan).hangup();
    }
}

/// A shim's control socket, unlinked when this goes out of scope.
///
/// A guard rather than a `remove_file` at the end of the happy path, because the
/// happy path is not the only way out: `run` returns early through a dozen `?`s
/// after binding, and each one left a `<pid>.sock` behind for good.
///
/// ponytail: a process killed by a signal it does not handle (SIGHUP from a
/// closed terminal, SIGKILL) runs no destructor at all. Those leftovers are
/// [`sweep`]'s, at the next cctop start.
struct Bound(Option<PathBuf>);

impl Drop for Bound {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
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
/// `size` is what the pty starts at: the pane it will be drawn in, so the agent
/// is not resized the moment the first frame lands.
pub fn host(
    argv: &[String],
    cwd: Option<&std::path::Path>,
    size: (u16, u16),
) -> anyhow::Result<Hosted> {
    if argv.is_empty() {
        anyhow::bail!("usage: cctop <command> [args…]  (e.g. cctop claude)");
    }
    let (child, master) = spawn_on_pty_at_env(argv, cwd, size, &[])?;
    let pid = child.id();
    crate::elog::event(
        "shim",
        "spawn",
        serde_json::json!({ "mode": "host", "pid": pid, "cmd": argv.join(" ") }),
    );
    let (listener, path) = listen(pid)?;
    let socket = Bound(Some(path.clone()));

    // No window here at all, so the panel that attaches sets the size alone.
    let fan = Arc::new(Mutex::new(Fanout::new(master.try_clone()?, (0, 0))));
    let stop = Arc::new(AtomicBool::new(false));
    {
        let (master, fan) = (master.try_clone()?, Arc::clone(&fan));
        std::thread::spawn(move || pump_output(master, fan, Echo::No));
    }
    let listener = Arc::new(listener);
    {
        let (listener, fan, stop) = (Arc::clone(&listener), Arc::clone(&fan), Arc::clone(&stop));
        std::thread::spawn(move || serve(listener, fan, stop));
    }
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
        shim: Shim { stop, fan, path },
        _socket: socket,
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
    Some(crate::config::runtime_base().join("cctop"))
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
        let Some(pid) = socket_pid(&path) else {
            continue;
        };
        if let Err(e) = UnixStream::connect(&path) {
            // Refused is the one answer that means nobody is listening; any
            // other failure says nothing about the listener, so the file stays.
            if refused(&e) {
                let _ = std::fs::remove_file(&path);
            }
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

/// The agent pid a shim socket is named after: `<pid>.sock`, nothing else.
fn socket_pid(path: &std::path::Path) -> Option<u32> {
    path.file_name()?
        .to_str()?
        .strip_suffix(".sock")?
        .parse::<u32>()
        .ok()
}

/// Whether a connect failed because nothing is listening on the socket.
fn refused(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(libc::ECONNREFUSED)
}

/// Whether a process with this pid exists. EPERM is somebody else's process,
/// which is still a process.
fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: signal 0 checks for existence and delivers nothing.
    let sent = unsafe { libc::kill(pid, 0) } == 0;
    sent || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// The shim sockets in `dir` that nothing will ever answer on again.
///
/// Two tests, both required. The pid in the name is the agent's, and it being
/// gone says the session is over — but the shim unlinks its socket a moment
/// *after* the agent exits, and a socket in that moment is still being
/// listened on. So the connect has to be refused as well, which only a socket
/// with no listener does.
///
/// ponytail: a dead socket whose pid has since been reused by an unrelated
/// process is kept. Telling it apart would mean trusting the connect alone,
/// which is what [`sessions`] does for the runtime directory it lists; this
/// sweep also reaches the cache directory, so it takes the stricter answer.
///
/// Each path comes with the [`Stamp`] it had when it was judged, taken before
/// either test ran, so [`sweep`] can tell the file it judged from one a new
/// shim bound under the same name since.
pub fn stale_sockets(dir: &std::path::Path) -> Vec<(PathBuf, Stamp)> {
    use std::os::unix::fs::FileTypeExt;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_socket()))
        .filter_map(|entry| {
            let path = entry.path();
            let pid = socket_pid(&path)?;
            let stamp = Stamp::of(&path)?;
            let dead = !pid_alive(pid) && UnixStream::connect(&path).is_err_and(|e| refused(&e));
            dead.then_some((path, stamp))
        })
        .collect()
}

/// Remove [`stale_sockets`] from every directory a shim may have bound in,
/// returning how many went.
///
/// Both the runtime directory and the cache directory, because a shim binds in
/// whichever [`config::runtime_base`](crate::config::runtime_base) answered at
/// the time — and the cache one is where they pile up, since the runtime one is
/// emptied at logout and the cache one never is.
///
/// Between the judging and the unlink, the dead pid can be reused by a new
/// agent whose shim's `listen` replaces the file with a live socket of the same
/// name. Unlinking by path would then take the live one, and that shim would
/// vanish from [`sessions`]; so the [`Stamp`] is compared again just before.
///
/// ponytail: the re-check and the unlink are still two calls, since there is no
/// unlink-if-inode; the window left is a stat's width, not a directory scan's.
pub fn sweep() -> usize {
    let mut dirs = vec![
        crate::config::runtime_base().join("cctop"),
        crate::config::CACHE_DIR.clone(),
    ];
    dirs.dedup();
    dirs.iter()
        .flat_map(|dir| stale_sockets(dir))
        .filter(|(path, stamp)| unlink_unchanged(path, *stamp))
        .count()
}

/// Which file a path named at one moment: its inode and its change time.
///
/// The inode alone is not enough, because a filesystem may hand a freed inode
/// number straight to the next file created — which is exactly what `listen`'s
/// unlink-then-bind does. The change time, to the nanosecond, tells those two
/// apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    ino: u64,
    ctime: (i64, i64),
}

impl Stamp {
    fn of(path: &std::path::Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path).ok()?;
        Some(Self {
            ino: m.ino(),
            ctime: (m.ctime(), m.ctime_nsec()),
        })
    }
}

/// Unlink `path` if it is still the file `stamp` was taken of; whether it went.
fn unlink_unchanged(path: &std::path::Path, stamp: Stamp) -> bool {
    Stamp::of(path) == Some(stamp) && std::fs::remove_file(path).is_ok()
}

/// Bind the control socket for the agent running as `pid`.
///
/// The path comes back with the listener, because the shim needs it later to
/// wake its own accept loop when the agent ends.
fn listen(pid: u32) -> anyhow::Result<(std::os::unix::net::UnixListener, PathBuf)> {
    let path = socket_path(pid).ok_or_else(|| anyhow::anyhow!("no runtime or cache directory"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("could not create {}: {e}", parent.display()))?;
        // The socket is a typing channel into a live agent: keep the directory
        // owner-only even when it lands in a shared cache root.
        let _ =
            std::fs::set_permissions(parent, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    }
    // A shim that died without cleaning up leaves a file that would block bind.
    let _ = std::fs::remove_file(&path);
    let listener = std::os::unix::net::UnixListener::bind(&path)
        .map_err(|e| anyhow::anyhow!("could not bind {}: {e}", path.display()))?;
    Ok((listener, path))
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
    /// The clients this shim is serving, as the descriptors their connections
    /// are on.
    ///
    /// Held so they can be hung up when the agent ends: a thread blocked
    /// reading a client holds the pty master, and a watcher that is not told
    /// the agent is gone waits for output that will never come.
    conns: Vec<std::os::fd::RawFd>,
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
            conns: Vec::new(),
            next_id: 0,
            local,
            applied: pty_size(&master),
            master,
        }
    }

    /// Another handle on the pty master, for a thread that has to write to the
    /// agent.
    fn master(&self) -> Option<File> {
        self.master.try_clone().ok()
    }

    /// Hang up every client.
    ///
    /// The threads serving them are parked in a read, and a read on a hung-up
    /// socket ends — which is what lets them go, and with them the pty master
    /// they were holding.
    fn hangup(&mut self) {
        for fd in self.conns.drain(..) {
            // SAFETY: a descriptor of a connected socket. The thread serving
            // it holds the connection open and takes it off this list before
            // it closes, so the number is still that socket's.
            unsafe { libc::shutdown(fd, libc::SHUT_RDWR) };
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
        crate::elog::bytes(
            "pty",
            "out",
            "out",
            chunk,
            serde_json::json!({
                "subs": self.subs.len(),
                "dropped": before - self.subs.len(),
            }),
        );
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
    /// they cannot fit, or everyone gets the small one's dimensions. rmux takes
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
        crate::elog::event(
            "pty",
            "size",
            serde_json::json!({ "cols": cols, "rows": rows, "subs": self.subs.len() }),
        );
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
///
/// The loop ends when the shim does: `stop` is set by the shim's teardown,
/// and the connection that wakes the loop out of `accept` is how it is told.
fn serve(
    listener: Arc<std::os::unix::net::UnixListener>,
    fan: Arc<Mutex<Fanout>>,
    stop: Arc<AtomicBool>,
) {
    for stream in listener.incoming().flatten() {
        // The shim is over. This connection is the one that woke the loop, and
        // it is not served: the thread ends here, and the listener with it.
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let Some(master) = locked(&fan).master() else {
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

    // The connection is on the fanout's list while it is served, so the shim
    // can hang it up when the agent ends.
    let fd = stream.as_raw_fd();
    locked(&fan).conns.push(fd);

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
            crate::elog::bytes("pty", "inject", "in", bytes, serde_json::json!({}));
            if !bytes.is_empty() && (master.write_all(bytes).is_err() || master.flush().is_err()) {
                break;
            }
            continue;
        };
        decoder.push(bytes);
        while let Some((kind, payload)) = decoder.next() {
            let delivered = match kind {
                frame::KEYS => {
                    crate::elog::bytes(
                        "pty",
                        "keys",
                        "in",
                        &payload,
                        serde_json::json!({ "sub": *id }),
                    );
                    master
                        .write_all(&payload)
                        .and_then(|()| master.flush())
                        .is_ok()
                }
                frame::RESIZE => {
                    if let Some((cols, rows)) = frame::parse_size(&payload) {
                        crate::elog::event(
                            "pty",
                            "resize",
                            serde_json::json!({ "sub": *id, "cols": cols, "rows": rows }),
                        );
                        locked(&fan).set_sub_size(*id, (cols, rows));
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

    // Off the fanout's list before the connection is closed, so the number in
    // it is never one that has been given to something else.
    locked(&fan).conns.retain(|&open| open != fd);
    // Drop the subscription now rather than when the next chunk finds it dead:
    // this watcher may be the one keeping the pty small, and the window the
    // agent runs in should come back the moment it detaches.
    if let Some((id, _)) = watcher {
        crate::elog::event("pty", "unwatch", serde_json::json!({ "sub": id }));
        locked(&fan).remove(id);
    }
}

/// Send this client the recent output and everything that follows, and return
/// the id its size is tracked under.
///
/// The replay is written to the client *outside* the lock. Writing it under
/// the lock held the lock for as long as the client took to read it, and the
/// agent's own output waits on that lock — so a client that stopped reading
/// froze the agent's terminal, which is the one thing a watcher is not
/// allowed to do.
fn subscribe(stream: &UnixStream, fan: &Arc<Mutex<Fanout>>) -> Option<u64> {
    use crate::attach::frame;

    let mut out = stream.try_clone().ok()?;
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(256);
    // The replay is taken under the lock, and the subscription is made under
    // the same lock, so no chunk slips in between the two: what the replay
    // holds is everything before the subscription, and what the channel
    // carries is everything after it.
    let (id, header, replay) = {
        let mut fan = locked(fan);
        let (cols, rows) = fan.applied;
        // Size first: the replay is drawn for these dimensions, so a watcher
        // that folded it into a differently sized screen would wrap every line
        // in the wrong place.
        let header = frame::size(frame::SIZE, cols, rows);
        let replay = fan.recent.clone();
        let id = fan.next_id;
        fan.next_id += 1;
        // No size yet: a watcher has no say until it asks for one, so attaching
        // alone never resizes the agent.
        fan.subs.push(Sub {
            id,
            tx,
            size: (0, 0),
        });
        crate::elog::event(
            "pty",
            "watch",
            serde_json::json!({ "sub": id, "subs": fan.subs.len() }),
        );
        (id, header, replay)
    };
    // The writer sends the replay and then whatever comes through the
    // channel, in that order: the replay is what came before the
    // subscription, and the channel carries what came after it, so the
    // client's screen is put together in the order the agent produced it.
    std::thread::spawn(move || {
        let replay = frame::encode(frame::OUTPUT, &replay);
        if out.write_all(&header).is_err() || out.write_all(&replay).is_err() {
            return;
        }
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
///
/// Only the tests still want it: `run` goes through [`run_in`], which has an
/// environment to pass and so calls [`spawn_on_pty_at_env`] itself.
#[cfg(any(test, feature = "test-support"))]
fn spawn_on_pty(
    argv: &[String],
    cwd: Option<&std::path::Path>,
) -> anyhow::Result<(std::process::Child, File)> {
    spawn_on_pty_at(argv, cwd, crossterm::terminal::size().unwrap_or((80, 24)))
}

/// [`spawn_on_pty`] at `(cols, rows)` rather than the size of this terminal.
#[cfg(any(test, feature = "test-support"))]
fn spawn_on_pty_at(
    argv: &[String],
    cwd: Option<&std::path::Path>,
    size: (u16, u16),
) -> anyhow::Result<(std::process::Child, File)> {
    spawn_on_pty_at_env(argv, cwd, size, &[])
}

/// Spawn `argv` on a fresh pty of `size`, with `env` added, and hand back the
/// child and the master — for a caller that pumps the bytes itself: the
/// terminal a sibling opens over [`crate::remote_open`]'s socket.
pub fn spawn_pty(
    argv: &[String],
    size: (u16, u16),
    env: &[(String, String)],
) -> anyhow::Result<(std::process::Child, File)> {
    spawn_on_pty_at_env(argv, None, size, env)
}

/// Set a pty's size, which the program on it gets as a SIGWINCH.
pub fn set_pty_size(master: &File, cols: u16, rows: u16) {
    let ws = winsize(cols, rows);
    // SAFETY: `master` is a live pty master and `ws` outlives the call.
    unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &ws) };
}

/// [`spawn_on_pty_at`] with `inherited` applied to the child as though this
/// process's environment had it, and then scrubbed like everything else.
///
/// The scrubbing is a fixed list of names rather than a general filter, so what
/// a test can usefully do is put the names there and watch them go: writing them
/// into this process's own environment to get them into the child would be
/// `setenv` beside every other thread's `getenv`, which is undefined behaviour.
fn spawn_on_pty_at_env(
    argv: &[String],
    cwd: Option<&std::path::Path>,
    (cols, rows): (u16, u16),
    inherited: &[(String, String)],
) -> anyhow::Result<(std::process::Child, File)> {
    let mut size = winsize(cols, rows);
    let mut master_fd = -1;
    let mut slave_fd = -1;
    // SAFETY: both fds are written by openpty and only read after it succeeds.
    //
    // The trailing two arguments are `*const`; a `*mut` coerces to one, which
    // is what the null pointers below are written as.
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
    // The child is on a pty of ours, which is not an rmux pane whatever the
    // terminal cctop was started from happened to be. Inheriting these would
    // tell it otherwise: any multiplexer integration it attempted would address
    // the pane cctop is drawn in rather than its own, and `rmux new-session` —
    // how a tab hands its agent to rmux — refuses outright inside one.
    //
    // Five variables and not two, because an rmux pane sets `TMUX` and
    // `TMUX_PANE` as well as its own pair, plus `TMUX_PROGRAM` pointing at the
    // shim it answers tmux commands through. Leaving any of the tmux-named
    // three behind is how a cctop running inside rmux hands the whole set to
    // its agent — and `RMUX` alone is refused with "no current client", which
    // reads like a broken launch rather than a nested one.
    for (var, value) in inherited {
        cmd.env(var, value);
    }
    for var in ["RMUX", "RMUX_PANE", "TMUX", "TMUX_PANE", "TMUX_PROGRAM"] {
        cmd.env_remove(var);
    }
    // Anything the agent it is launching has to be told, which today is
    // OpenCode 2's own tab strip: cctop's tabs are the ones on the dashboard and
    // the tab bar inside the pane is a second, lesser account of the same fact.
    for (var, value) in crate::opencode::launch_env(argv) {
        cmd.env(var, value);
    }
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
    // Said in words, because the bare `No such file or directory (os error 2)`
    // never names the thing that was missing — and a missing agent, or one a
    // profile's PATH does not reach, is the usual reason a tab will not start.
    let child = cmd.spawn().map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            anyhow::anyhow!("`{}` was not found — is it installed and on PATH?", argv[0])
        }
        std::io::ErrorKind::PermissionDenied => {
            anyhow::anyhow!("`{}` could not be run: permission denied", argv[0])
        }
        _ => anyhow::anyhow!("`{}` could not be run: {error}", argv[0]),
    })?;
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
#[cfg(any(test, feature = "test-support"))]
pub fn test_session(argv: &[&str], local: (u16, u16)) -> (std::process::Child, u32) {
    let argv: Vec<String> = argv.iter().map(|s| (*s).to_string()).collect();
    let (child, master) = spawn_on_pty(&argv, None).expect("pty child");
    let pid = child.id();
    let (listener, _path) = listen(pid).expect("control socket");
    let fan = Arc::new(Mutex::new(Fanout::new(
        master.try_clone().expect("master"),
        local,
    )));
    locked(&fan).fit();
    {
        let (master, fan) = (master.try_clone().expect("master"), Arc::clone(&fan));
        std::thread::spawn(move || pump_output(master, fan, Echo::No));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let listener = Arc::new(listener);
    {
        let (listener, fan, stop) = (Arc::clone(&listener), Arc::clone(&fan), Arc::clone(&stop));
        std::thread::spawn(move || serve(listener, fan, stop));
    }
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
    let mut answers = Answers::default();
    let mut reply = master.try_clone().ok();
    // Flush every chunk: a TUI's escape sequences must not sit in a line buffer
    // waiting for a newline that never comes.
    while let Ok(n) = master.read(&mut buf) {
        if n == 0 {
            break;
        }
        if echo == Echo::Yes && (out.write_all(&buf[..n]).is_err() || out.flush().is_err()) {
            break;
        }
        // Only when nothing is echoing: with a real terminal behind this one it
        // answers for itself, and two answers to one question is worse than
        // none — the second is read as input the user typed.
        if echo == Echo::No
            && let Some(reply) = reply.as_mut()
        {
            answers.answer(&buf[..n], reply);
        }
        locked(&fan).push(&buf[..n]);
    }
}

/// What an agent asks its terminal at startup, and what to say back.
///
/// Every question here was captured from a real launch. Codex asks five before
/// it draws a cell — cursor position, foreground and background colour, the
/// kitty keyboard protocol, and the device attributes — and Claude Code asks for
/// device attributes and subscribes to theme changes.
///
/// A terminal answers those. A watcher is a picture of one: [`attach`] rebuilds
/// the screen from the agent's output and has nothing to say back, so under
/// `cctop run` the questions reach the real terminal and are answered, while a
/// hosted agent's questions used to fall into the dark. What an agent does then
/// is wait out a timeout and assume the least — no keyboard protocol, no
/// synchronized output, and whichever theme it guesses. A pane cctop hosts was
/// a worse terminal than a pane rmux hosts, for no reason other than that rmux
/// bothers to reply.
///
/// The device-attributes reply is also what unblocks the wait: an application
/// asking about the keyboard protocol sends `CSI c` behind it precisely so that
/// a terminal which knows nothing of the newer query still says *something*,
/// and the answer arriving is what ends the timeout.
#[derive(Default)]
struct Answers {
    /// The end of the last chunk, so a query split across two reads is still
    /// recognised. Long enough for the longest question below.
    tail: Vec<u8>,
}

/// Longest query, and so how much of a chunk has to be carried over.
const QUERY_MAX: usize = 16;

impl Answers {
    /// Reply to every question in `chunk`.
    fn answer(&mut self, chunk: &[u8], reply: &mut impl Write) {
        let mut seen = std::mem::take(&mut self.tail);
        seen.extend_from_slice(chunk);
        for (query, answer) in Self::table() {
            let found = strike(&mut seen, query);
            if found > 0 {
                crate::elog::event(
                    "pty",
                    "answer",
                    serde_json::json!({
                        "query": String::from_utf8_lossy(query).escape_default().to_string(),
                        "n": found,
                    }),
                );
            }
            for _ in 0..found {
                let _ = reply.write_all(answer.as_bytes());
            }
        }
        let _ = reply.flush();
        // The tail is carried so a query split across two reads is still seen
        // whole. It is carried *struck out*, which is the whole reason the
        // matches above are erased rather than counted: answered once and left
        // intact, every question in the last few bytes of a chunk would be
        // answered a second time on the next one.
        let keep = seen.len().saturating_sub(QUERY_MAX);
        self.tail = seen.split_off(keep);
    }

    /// The questions and their answers.
    ///
    /// ponytail: no reply to `CSI 6 n`, the cursor position. It is the one
    /// question whose answer changes as the agent draws, and the shim relays
    /// bytes rather than parsing them, so it does not know where the cursor is.
    /// A made-up position is worse than silence: an agent that believes the
    /// cursor is somewhere it is not draws its first frame in the wrong place.
    /// Codex asks and carries on regardless, which is why this has stayed a
    /// note rather than a parser.
    fn table() -> Vec<(&'static [u8], String)> {
        // Read per chunk rather than kept from when the agent started: it is
        // one relaxed load, and an agent that asks after a theme change hears
        // the new palette rather than the one in force at its launch.
        let light = LIGHT_PALETTE.load(Ordering::Relaxed);
        // A VT220 with ANSI colour: what the vt100 parser behind every watcher
        // actually implements, so this is a description rather than a boast.
        let da1 = "\x1b[?62;22c".to_string();
        vec![
            (b"\x1b[c".as_slice(), da1.clone()),
            (b"\x1b[0c".as_slice(), da1),
            // Secondary attributes: terminal type 0, "firmware" version 0.
            (b"\x1b[>c".as_slice(), "\x1b[>0;0;0c".to_string()),
            (b"\x1b[>0c".as_slice(), "\x1b[>0;0;0c".to_string()),
            // Device status: ready, no malfunction.
            (b"\x1b[5n".as_slice(), "\x1b[0n".to_string()),
            // Synchronized output: understood, currently reset. An agent told
            // this wraps its frames in it, which costs nothing here — the
            // watcher's parser ignores the mode and cctop's own draw is already
            // one flush — and saves the agent from assuming it must not.
            (b"\x1b[?2026$p".as_slice(), "\x1b[?2026;2$y".to_string()),
            (b"\x1b]10;?".as_slice(), color_reply(10, light)),
            (b"\x1b]11;?".as_slice(), color_reply(11, light)),
        ]
    }
}

/// Erase every occurrence of `needle` from `haystack`, and say how many there
/// were.
///
/// Zeroed rather than removed, so the positions of everything else are
/// untouched. A zero cannot begin any query here — all of them start with an
/// escape — so a struck-out match can never match again.
fn strike(haystack: &mut [u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    let mut found = 0;
    let mut i = 0;
    while i + needle.len() <= haystack.len() {
        if &haystack[i..i + needle.len()] == needle {
            haystack[i..i + needle.len()].fill(0);
            i += needle.len();
            found += 1;
            continue;
        }
        i += 1;
    }
    found
}

/// Whether cctop's palette is the light one, as the UI last said.
///
/// The palette is the terminal UI's, and the shim sits below the UI: it also
/// runs with no UI at all (`cctop run`). So the UI pushes its choice down
/// rather than the shim reaching up for it, and with nobody to say otherwise
/// the answer is dark.
///
/// A process-wide value rather than a question put to the UI, because of who
/// asks. The palette lives in a thread-local on the draw thread, and the
/// questions are answered on each agent's own `pump_output` thread, which never
/// chose a palette and so always saw dark: a light cctop told every agent it
/// hosted that the terminal was black, and Claude Code's `theme: auto` drew dark
/// colours onto a white pane.
static LIGHT_PALETTE: AtomicBool = AtomicBool::new(false);

/// Tell agents hosted from now on that cctop's palette is light (or not).
///
/// Called by the UI wherever its draw thread installs a palette — at startup
/// and on every theme change — so the answer follows the settings page. An
/// agent asks once, at startup, so one already running keeps the answer it got.
pub fn set_light_palette(light: bool) {
    LIGHT_PALETTE.store(light, Ordering::Relaxed);
}

/// The foreground (10) or background (11) colour to report, for a light or a
/// dark palette.
///
/// An agent that asks is deciding whether to draw for a light or a dark
/// terminal — Claude Code's `theme: auto` is exactly this question — and the
/// honest answer is cctop's own palette, because a hosted agent is drawn inside
/// it. Only the luminance is really being asked about, so the two ends of the
/// ramp say it unambiguously.
fn color_reply(which: u16, light: bool) -> String {
    let bright = "ffff/ffff/ffff";
    let dark = "0000/0000/0000";
    let value = match (which, light) {
        (11, true) | (10, false) => bright,
        _ => dark,
    };
    format!("\x1b]{which};rgb:{value}\x1b\\")
}

fn pump_input(mut master: File) {
    let mut stdin = std::io::stdin();
    let mut buf = [0u8; 1024];
    let mut tail = Vec::new();
    let window = crate::settings::Settings::load().paste_debounce();
    let mut pastes = crate::paste::PasteFilter::new(window, "run");
    while let Ok(n) = stdin.read(&mut buf) {
        if n == 0 {
            break;
        }
        // Everything goes through whole except bare-motion mouse reports —
        // see `attach::strip_hover_reports` — and a paste repeated straight
        // after itself, see `crate::paste`.
        let stripped = crate::attach::strip_hover_reports(&buf[..n], &mut tail);
        let bytes = pastes.feed(&stripped, std::time::Instant::now());
        crate::elog::bytes(
            "pty",
            "stdin",
            "in",
            &bytes,
            serde_json::json!({ "dropped": n - stripped.len() }),
        );
        if !bytes.is_empty() && master.write_all(&bytes).is_err() {
            break;
        }
        pastes.delivered(std::time::Instant::now());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_wait::{PATIENCE, wait_until, waits_for};

    /// The thread that waits for connections ends with the shim.
    ///
    /// It used to be parked in `accept` for ever: the control socket stayed
    /// open and the pty master stayed in its hand, so every agent that ended
    /// left a thread and three descriptors behind for as long as cctop ran.
    #[test]
    fn the_thread_waiting_for_connections_ends_with_the_shim() {
        let (mut child, master) =
            spawn_on_pty(&["sh".into(), "-c".into(), "sleep 30".into()], None).expect("a pty");
        let pid = child.id();
        let (listener, path) = listen(pid).expect("a control socket");
        let fan = Arc::new(Mutex::new(Fanout::new(
            master.try_clone().expect("a master"),
            (80, 24),
        )));
        let stop = Arc::new(AtomicBool::new(false));
        let served = {
            let (listener, fan, stop) = (Arc::new(listener), Arc::clone(&fan), Arc::clone(&stop));
            std::thread::spawn(move || serve(listener, fan, stop))
        };

        // The shim ends, the way a hosted agent's does.
        Shim {
            stop: Arc::clone(&stop),
            fan: Arc::clone(&fan),
            path: path.clone(),
        }
        .end();

        assert!(
            waits_for(|| served.is_finished()),
            "the accept loop is still parked in accept"
        );
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&path);
    }

    /// A watcher is told the agent is gone.
    ///
    /// It was not: the connection stayed open and the watcher waited for
    /// output that would never come, and the thread serving it held the pty
    /// master with it.
    #[test]
    fn a_watcher_is_told_the_agent_is_gone() {
        let hosted = host(
            &["sh".to_string(), "-c".to_string(), "sleep 30".to_string()],
            None,
            (80, 24),
        )
        .expect("a hosted agent");
        let pid = hosted.pid;
        let watching = crate::attach::attach(pid).expect("an attach connection");
        assert!(!watching.closed(), "it hung up on a live agent");

        drop(hosted);

        assert!(
            waits_for(|| watching.closed()),
            "the watcher was never told the agent is gone"
        );
    }

    /// A client that stops reading does not freeze the agent.
    ///
    /// The replay was written to the client with the fanout locked, so a
    /// client that would not read held the lock for as long as it took — and
    /// the agent's own output waits on that lock. A watcher was allowed to
    /// stop the agent's terminal by accident, and did.
    #[test]
    fn a_client_that_stops_reading_does_not_freeze_the_agent() {
        let (mut child, master) = spawn_on_pty(
            &[
                "sh".into(),
                "-c".into(),
                // More than the pty's own buffer holds, so the agent cannot
                // finish unless something keeps reading it.
                "head -c 2000000 /dev/zero | tr '\\0' 'x'".into(),
            ],
            None,
        )
        .expect("a pty");
        let pid = child.id();
        let (listener, path) = listen(pid).expect("a control socket");
        let fan = Arc::new(Mutex::new(Fanout::new(
            master.try_clone().expect("a master"),
            (80, 24),
        )));
        // A full replay, so the write that blocks is a big one.
        locked(&fan).recent = vec![b'x'; REPLAY_BYTES];
        {
            let (master, fan) = (master.try_clone().expect("a master"), Arc::clone(&fan));
            std::thread::spawn(move || pump_output(master, fan, Echo::No));
        }
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (listener, fan, stop) = (Arc::new(listener), Arc::clone(&fan), Arc::clone(&stop));
            std::thread::spawn(move || serve(listener, fan, stop));
        }

        // A client that will not read: a receive buffer of nothing, and a
        // socket nobody drains.
        let mut client = UnixStream::connect(&path).expect("a connection");
        let small: libc::c_int = 2048;
        // SAFETY: a live socket, and a buffer size the kernel takes as it is.
        unsafe {
            libc::setsockopt(
                client.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&small as *const libc::c_int).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            );
        }
        client.write_all(ATTACH_MAGIC).expect("the magic");
        // Waited for with a sleep rather than with the lock: with the old
        // code the lock is what the blocked write holds, and a test that
        // asked for it would wait for ever — which is the bug, not a
        // failure of the test.
        std::thread::sleep(std::time::Duration::from_millis(250));

        // The agent finishes: something kept reading the pty all along.
        let finished = wait_until(PATIENCE, || child.try_wait().ok().flatten());
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&path);
        assert!(
            finished.is_some(),
            "the agent stopped writing: the client froze it"
        );
    }

    /// A pid nothing runs as: a child that has been reaped.
    fn dead_pid() -> u32 {
        let mut child = Command::new("true").spawn().expect("spawn true");
        let pid = child.id();
        child.wait().expect("reap");
        assert!(
            !pid_alive(pid),
            "{pid} was reused before the test could use it"
        );
        pid
    }

    /// The one stale socket in `dir`, waited for. A process another test is
    /// spawning holds a copy of every fd this one has until it execs, a
    /// dropped listener included, so for that moment a dead socket still
    /// accepts connections.
    fn stale_once(dir: &std::path::Path) -> (PathBuf, Stamp) {
        crate::test_wait::eventually("one stale socket", || {
            <[_; 1]>::try_from(stale_sockets(dir)).ok().map(|[one]| one)
        })
    }

    /// The sweep's three cases side by side: only a socket whose agent is gone
    /// *and* that refuses connections goes. A live listener stays whatever its
    /// name says, and so does a dead one whose pid now belongs to someone else.
    #[test]
    fn the_sweep_takes_only_dead_sockets_of_dead_agents() {
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().expect("temp dir");

        // Listening, though the agent it is named after has exited: the shim
        // between its agent's exit and its own cleanup.
        let live = dir.path().join(format!("{}.sock", dead_pid()));
        let _listener = UnixListener::bind(&live).expect("bind live");

        // Nobody listening and nobody by that pid: what a SIGHUP leaves.
        let dead = dir.path().join(format!("{}.sock", dead_pid()));
        drop(UnixListener::bind(&dead).expect("bind dead"));

        // Nobody listening, but the pid is taken — here by this test.
        let reused = dir.path().join(format!("{}.sock", std::process::id()));
        drop(UnixListener::bind(&reused).expect("bind reused"));

        // Not a shim's: no pid in the name, or not a socket at all.
        let named = dir.path().join("default.sock");
        drop(UnixListener::bind(&named).expect("bind named"));
        let plain = dir.path().join(format!("{}.sock", dead_pid()));
        std::fs::write(&plain, "").expect("plain file");

        // Waited for, not asked once: see `stale_once`.
        crate::test_wait::eventually_true("the dead socket alone to be stale", || {
            let stale: Vec<PathBuf> = stale_sockets(dir.path())
                .into_iter()
                .map(|(path, _)| path)
                .collect();
            stale == [dead.clone()]
        });
        assert!(live.exists() && reused.exists() && named.exists() && plain.exists());
    }

    /// A socket judged dead, then rebound under its name by a new shim before
    /// the sweep reached it, is the new shim's: the sweep leaves it.
    #[test]
    fn the_sweep_spares_a_socket_rebound_since_it_was_judged() {
        use std::os::unix::net::UnixListener;
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(format!("{}.sock", dead_pid()));
        drop(UnixListener::bind(&path).expect("bind dead"));
        let (judged, stamp) = stale_once(dir.path());

        // A new shim's socket in its place. Bound beside it and renamed over,
        // rather than `listen`'s unlink-then-bind, so the two cannot share an
        // inode and a coarse clock tick and make this a coin toss.
        let fresh = dir.path().join("fresh");
        let _listener = UnixListener::bind(&fresh).expect("bind live");
        std::fs::rename(&fresh, &path).expect("rename over");

        assert!(!unlink_unchanged(&judged, stamp));
        assert!(path.exists());
    }

    /// Dropping a [`Bound`] unlinks its path. That `run_in` and [`host`] hold
    /// the guard for the shim's whole life is their bindings' business, not
    /// something this checks.
    #[test]
    fn a_bound_socket_goes_when_its_guard_does() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("1.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).expect("bind");
        drop(Bound(Some(path.clone())));
        assert!(!path.exists());
    }

    /// A cctop running inside a multiplexer must not hand its own pane to the
    /// agent it launches. rmux sets five variables — its own pair, tmux's pair
    /// for compatibility, and `TMUX_PROGRAM` — and `rmux new-session -A`, which
    /// is how a tab hands its agent over, refuses to nest under any of them:
    /// "sessions should be nested with care" for `$TMUX`, and the more
    /// confusing "no current client" for `$RMUX` alone. A tab that silently
    /// never opens is what leaking one of these looks like.
    #[test]
    fn a_hosted_agent_inherits_no_pane_of_its_own_parent() {
        let dir = tempfile::tempdir().expect("temp dir");
        let seen = dir.path().join("env.txt");
        let leaked: Vec<(String, String)> =
            ["RMUX", "RMUX_PANE", "TMUX", "TMUX_PANE", "TMUX_PROGRAM"]
                .iter()
                .map(|var| (var.to_string(), "leaked".to_string()))
                .collect();
        let argv: Vec<String> = ["sh", "-c", &format!("env > {}", seen.display())]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Same five names, handed in rather than written into this process's
        // environment where every other test thread would be reading them.
        let (mut child, _master) =
            spawn_on_pty_at_env(&argv, None, (80, 24), &leaked).expect("a pty for the child");
        // `sh` writes the file and exits at once; poll rather than sleep.
        waits_for(|| child.try_wait().ok().flatten().is_some());
        let _ = child.kill();
        let _ = child.wait();

        let env = std::fs::read_to_string(&seen).expect("the child wrote its environment");
        for var in ["RMUX", "RMUX_PANE", "TMUX", "TMUX_PANE", "TMUX_PROGRAM"] {
            assert!(
                !env.lines().any(|line| line.starts_with(&format!("{var}="))),
                "{var} reached the agent:\n{env}"
            );
        }
    }

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
    #[test]
    fn a_line_sent_to_the_socket_becomes_the_childs_input() {
        let out = std::env::temp_dir().join(format!("cctop-shim-test-{}.txt", std::process::id()));
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
        let (listener, _path) = listen(pid).unwrap();
        let fan = Arc::new(Mutex::new(Fanout::new(
            master.try_clone().unwrap(),
            (80, 24),
        )));
        let stop = Arc::new(AtomicBool::new(false));
        let listener = Arc::new(listener);
        {
            let (listener, fan, stop) =
                (Arc::clone(&listener), Arc::clone(&fan), Arc::clone(&stop));
            std::thread::spawn(move || serve(listener, fan, stop));
        }

        // Through the public entry point, so the socket lookup is covered too.
        let sent = crate::inject::send_line(pid, "continue");

        let text = wait_until(PATIENCE, || {
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
        let (listener, _path) = listen(pid).unwrap();
        let fan = Arc::new(Mutex::new(Fanout::new(
            master.try_clone().unwrap(),
            (80, 24),
        )));
        let stop = Arc::new(AtomicBool::new(false));
        let listener = Arc::new(listener);
        for spawn in [0, 1] {
            let (master, fan) = (master.try_clone().unwrap(), Arc::clone(&fan));
            match spawn {
                0 => std::thread::spawn(move || pump_output(master, fan, Echo::No)),
                _ => {
                    let (listener, fan, stop) =
                        (Arc::clone(&listener), Arc::clone(&fan), Arc::clone(&stop));
                    std::thread::spawn(move || serve(listener, fan, stop))
                }
            };
        }

        // Attach only once the replay buffer holds what the child said, so
        // what arrives can only have come from there.
        let spoke = waits_for(|| {
            locked(&fan)
                .recent
                .windows(b"ALREADY-DRAWN".len())
                .any(|w| w == b"ALREADY-DRAWN")
        });
        assert!(spoke, "the child never drew anything to replay");
        let mut attach = crate::attach::attach(pid).expect("no attach connection");
        let screen = |attach: &mut crate::attach::Attach, want: &str| {
            waits_for(|| {
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
        let (listener, _path) = listen(pid).unwrap();
        let local = (100u16, 40u16);
        let fan = Arc::new(Mutex::new(Fanout::new(master.try_clone().unwrap(), local)));
        locked(&fan).fit();
        {
            let (master, fan) = (master.try_clone().unwrap(), Arc::clone(&fan));
            std::thread::spawn(move || pump_output(master, fan, Echo::No));
        }
        let stop = Arc::new(AtomicBool::new(false));
        let listener = Arc::new(listener);
        {
            let (listener, fan, stop) =
                (Arc::clone(&listener), Arc::clone(&fan), Arc::clone(&stop));
            std::thread::spawn(move || serve(listener, fan, stop));
        }

        let settled = |want: (u16, u16)| waits_for(|| pty_size(&master) == want);
        let started_at_local = settled(local);

        let mut attach = crate::attach::attach(pid).expect("no attach connection");
        attach.resize(60, 20);
        let shrank = settled((60, 20));
        // The agent is told, not just the pty: without SIGWINCH reaching it, a
        // TUI would go on painting at the old width.
        let agent_told = waits_for(|| {
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

    /// A question split across two reads is still answered, and answered once.
    ///
    /// The pty hands over whatever has arrived, so a query can straddle a chunk
    /// boundary — and the carried-over tail is scanned again on the next chunk,
    /// which is how answering twice would happen. An agent that asked one
    /// question and got two answers reads the second as something the user
    /// typed.
    #[test]
    fn a_question_split_across_two_reads_is_answered_exactly_once() {
        let mut answers = Answers::default();
        let mut said = Vec::new();
        answers.answer(b"drawing\x1b", &mut said);
        assert!(said.is_empty(), "answered half a question");
        answers.answer(b"[c and on", &mut said);
        assert_eq!(said, b"\x1b[?62;22c");

        // The same question wholly inside the carried tail: answered on the
        // chunk it arrived in, and never again.
        let mut answers = Answers::default();
        let mut said = Vec::new();
        answers.answer(b"\x1b[5n", &mut said);
        answers.answer(b"more output", &mut said);
        answers.answer(b"and more", &mut said);
        assert_eq!(said, b"\x1b[0n", "the tail was answered twice");
    }

    /// Every question Codex and Claude Code were seen to ask, and the one this
    /// deliberately does not answer.
    #[test]
    fn the_questions_a_harness_asks_at_startup_get_answers() {
        let mut answers = Answers::default();
        let mut said = Vec::new();
        // Codex's opening, verbatim.
        answers.answer(b"\x1b[?2004h\x1b[>4;0m\x1b[>7u\x1b[?1004h\x1b[6n\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[?u\x1b[c", &mut said);
        let said = String::from_utf8_lossy(&said).to_string();
        assert!(said.contains("[?62;22c"), "no device attributes: {said:?}");
        assert!(said.contains("]10;rgb:"), "no foreground colour: {said:?}");
        assert!(said.contains("]11;rgb:"), "no background colour: {said:?}");
        // The cursor position is the one question with no honest fixed answer.
        assert!(
            !said.contains('R'),
            "answered the cursor position: {said:?}"
        );
    }

    /// The colour answers are the two ends of the ramp, and which end is which
    /// follows the palette: a light one has a white ground and dark ink.
    #[test]
    fn the_colour_answers_follow_the_palette() {
        let white = "\x1b]11;rgb:ffff/ffff/ffff\x1b\\";
        assert_eq!(color_reply(11, true), white);
        assert_eq!(color_reply(10, true), "\x1b]10;rgb:0000/0000/0000\x1b\\");
        assert_eq!(color_reply(11, false), "\x1b]11;rgb:0000/0000/0000\x1b\\");
        assert_eq!(color_reply(10, false), "\x1b]10;rgb:ffff/ffff/ffff\x1b\\");
    }

    /// A palette published on one thread is what an agent's questions are
    /// answered with on another — the exact bug: the UI chose light on its draw
    /// thread, and the `pump_output` thread answering the agent had never
    /// chosen anything, so it said dark.
    ///
    /// The only test in this crate that writes [`LIGHT_PALETTE`], and the only
    /// one that reads which end of the ramp it gets, so it races nobody. It
    /// puts dark back on the way out all the same.
    #[test]
    fn a_palette_published_on_one_thread_answers_an_agent_on_another() {
        let ask = || {
            std::thread::spawn(|| {
                let mut said = Vec::new();
                Answers::default().answer(b"\x1b]11;?\x1b\\\x1b]10;?\x1b\\", &mut said);
                String::from_utf8(said).unwrap()
            })
            .join()
            .unwrap()
        };
        std::thread::spawn(|| set_light_palette(true))
            .join()
            .unwrap();
        let light = ask();
        std::thread::spawn(|| set_light_palette(false))
            .join()
            .unwrap();
        let dark = ask();
        assert!(
            light.contains("]11;rgb:ffff/ffff/ffff") && light.contains("]10;rgb:0000/0000/0000"),
            "a light palette was answered as {light:?}"
        );
        assert!(
            dark.contains("]11;rgb:0000/0000/0000") && dark.contains("]10;rgb:ffff/ffff/ffff"),
            "a dark palette was answered as {dark:?}"
        );
    }

    /// The whole path on a real pty: an agent asks, and the answer arrives on
    /// its own stdin without any terminal being involved.
    #[test]
    fn a_hosted_agent_gets_an_answer_from_the_shim() {
        // Raw mode so the reply is not line-buffered or echoed back, then read
        // exactly the nine bytes of the reply and draw them where a watcher can
        // see them.
        let script = "stty raw -echo; printf '\\033[c'; dd bs=1 count=9 2>/dev/null | tr -d '\\033'; sleep 30";
        let argv: Vec<String> = ["sh", "-c", script].iter().map(|s| s.to_string()).collect();
        let Ok(hosted) = host(&argv, None, (80, 24)) else {
            eprintln!("skipping: no pty available");
            return;
        };
        let mut view = crate::attach::attach(hosted.pid).expect("attach to the shim");
        let seen = wait_until(PATIENCE, || {
            view.pump();
            let screen = view.parser.screen().contents();
            screen.contains("[?62;22c").then_some(screen)
        });
        drop(hosted);
        assert!(
            seen.is_some(),
            "the agent's question went unanswered on a real pty"
        );
    }
}
