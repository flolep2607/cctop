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
    let (mut child, master) = spawn_on_pty(argv)?;
    let pid = child.id();

    // Bind before entering raw mode: a failure here should print normally.
    let listener = listen(pid)?;
    let socket = socket_path(pid);

    let raw = crossterm::terminal::enable_raw_mode().is_ok();
    let fan = Arc::new(Mutex::new(Fanout::default()));
    {
        let (master, fan) = (master.try_clone()?, Arc::clone(&fan));
        std::thread::spawn(move || pump_output(master, fan));
    }
    for task in [pump_input, watch_resize] {
        let master = master.try_clone()?;
        std::thread::spawn(move || task(master));
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

/// Everything the pty has said lately, and everyone listening.
#[derive(Default)]
struct Fanout {
    recent: Vec<u8>,
    subs: Vec<SyncSender<Vec<u8>>>,
}

impl Fanout {
    fn push(&mut self, chunk: &[u8]) {
        self.recent.extend_from_slice(chunk);
        if self.recent.len() > REPLAY_BYTES {
            self.recent.drain(..self.recent.len() - REPLAY_BYTES);
        }
        // ponytail: a subscriber that falls a channel behind is dropped rather
        // than slowing the agent's own terminal down. It reconnects and gets the
        // replay. Buffer per subscriber if that ever proves too twitchy.
        self.subs.retain(|tx| tx.try_send(chunk.to_vec()).is_ok());
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
    let mut buf = [0u8; 4096];
    let mut first = true;
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        let mut bytes = &buf[..n];
        // The magic is written on its own, and a write that small arrives whole,
        // so testing the first read for it is enough.
        if std::mem::take(&mut first)
            && let Some(rest) = bytes.strip_prefix(ATTACH_MAGIC)
        {
            subscribe(&stream, &master, &fan);
            bytes = rest;
        }
        if !bytes.is_empty() && (master.write_all(bytes).is_err() || master.flush().is_err()) {
            break;
        }
    }
}

/// Send this client the recent output and everything that follows.
fn subscribe(stream: &UnixStream, master: &File, fan: &Arc<Mutex<Fanout>>) {
    let Ok(mut out) = stream.try_clone() else {
        return;
    };
    let (cols, rows) = pty_size(master);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(256);
    {
        // Held across the replay write so no chunk slips in between the replay
        // and the subscription, which would reorder the client's screen.
        let mut fan = locked(fan);
        if writeln!(out, "cctop-size {cols} {rows}").is_err() || out.write_all(&fan.recent).is_err()
        {
            return;
        }
        fan.subs.push(tx);
    }
    std::thread::spawn(move || {
        while let Ok(chunk) = rx.recv() {
            if out.write_all(&chunk).is_err() {
                break;
            }
        }
    });
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
fn spawn_on_pty(argv: &[String]) -> anyhow::Result<(std::process::Child, File)> {
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

fn pump_output(mut master: File, fan: Arc<Mutex<Fanout>>) {
    let mut out = std::io::stdout();
    let mut buf = [0u8; 8192];
    // Flush every chunk: a TUI's escape sequences must not sit in a line buffer
    // waiting for a newline that never comes.
    while let Ok(n) = master.read(&mut buf) {
        if n == 0 || out.write_all(&buf[..n]).is_err() || out.flush().is_err() {
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

fn watch_resize(master: File) {
    let mut last = crossterm::terminal::size().unwrap_or((80, 24));
    loop {
        std::thread::sleep(RESIZE_POLL);
        let Ok(size) = crossterm::terminal::size() else {
            continue;
        };
        if size != last {
            last = size;
            let ws = winsize(size.0, size.1);
            // SAFETY: `master` is a live pty master and `ws` outlives the call.
            unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ as _, &ws) };
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
        let (mut child, master) = spawn_on_pty(&[
            "sh".into(),
            "-c".into(),
            format!("tee {} >/dev/null; :", out.display()),
        ])
        .unwrap();
        let pid = child.id();
        let listener = listen(pid).unwrap();
        let fan = Arc::new(Mutex::new(Fanout::default()));
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

        let (mut child, master) = spawn_on_pty(&[
            "sh".into(),
            "-c".into(),
            "printf 'ALREADY-DRAWN\\r\\n'; cat".into(),
        ])
        .unwrap();
        let pid = child.id();
        let listener = listen(pid).unwrap();
        let fan = Arc::new(Mutex::new(Fanout::default()));
        for spawn in [0, 1] {
            let (master, fan) = (master.try_clone().unwrap(), Arc::clone(&fan));
            match spawn {
                0 => std::thread::spawn(move || pump_output(master, fan)),
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
            spawn_on_pty(&["sh".into(), "-c".into(), "sleep 30".into()]).expect("pty child");
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
