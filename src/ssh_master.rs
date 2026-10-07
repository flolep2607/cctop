//! One ssh connection per host, shared by the launcher and the sandbox.
//!
//! The launcher's directory field completes paths on a host by listing them
//! over ssh, one round trip per directory. Each of those authenticating on its
//! own would be a second or two per keystroke on a good day and a password
//! prompt inside the TUI on a bad one. So as soon as a host is picked, one
//! OpenSSH ControlMaster is started for it in the background, and every listing
//! rides it — and so does the `cctop sandbox` that the launch then starts,
//! which finds the master at the same socket and borrows it instead of
//! authenticating again.
//!
//! **Started non-interactively, always.** `BatchMode=yes`, no controlling
//! terminal (`setsid`), stdin from `/dev/null`: a host that wants a password,
//! a passphrase or a host-key answer fails here with ssh's own reason instead
//! of asking on a terminal the TUI is drawing on. The sandbox in the launch tab
//! has a terminal of its own and starts a master interactively when there is
//! none to borrow, so such a host still works — it just completes no paths.
//!
//! **Shared, so counted.** A master outlives whoever started it — the launch
//! tab may be in rmux and outlast cctop — so a master is taken down by whoever
//! lets go of it last rather than by whoever started it. Each user writes a
//! lease file named by its pid next to the socket; letting go removes it, and
//! the one that finds no live lease left asks the master to exit. A cctop that
//! is killed outright never lets go, which is what `ControlPersist`'s idle
//! timeout is for: a master nobody is using exits on its own.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The longest a Unix socket path may be, less a margin.
///
/// `sun_path` is 108 bytes on Linux, and ssh appends a random suffix to the
/// ControlPath while it binds — a path that fits on paper fails at bind time.
pub const SOCKET_PATH_MAX: usize = 90;

/// How long a master with no session on it lingers before exiting.
///
/// The backstop for a cctop that did not get to let go — killed, or its
/// terminal closed. Long enough that picking a host, wandering off and coming
/// back to launch still finds it up; short enough that nothing is left
/// connected overnight. A running sandbox always has its sshfs session on the
/// master, so this never cuts one off.
const PERSIST_IDLE: &str = "10m";

/// How long ssh may take to reach the host, and how long the whole connect may
/// take before it is given up on. The second is the one that bounds a host
/// that accepts the TCP connection and then says nothing.
const CONNECT_TIMEOUT_SECS: u64 = 8;
const CONNECT_DEADLINE: Duration = Duration::from_secs(15);

/// The control socket for `host`: the same path for every cctop of this user,
/// which is what lets the sandbox find what the launcher started.
///
/// Named by a hash of the host and the uid rather than the host itself: a
/// `user@long.host.name` would not fit in a socket path, and the runtime
/// directory may be `/tmp`-backed and shared.
pub fn socket_for(host: &str) -> Option<PathBuf> {
    // FNV-1a, not `DefaultHasher`: two cctop binaries of different Rust
    // versions — an update while a TUI is open — must agree on the name.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    // SAFETY: getuid cannot fail.
    let uid = unsafe { libc::getuid() };
    for byte in host.bytes().chain(uid.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let name = format!("m-{:012x}.sock", hash & 0xffff_ffff_ffff);
    socket_dirs()
        .into_iter()
        .map(|dir| dir.join(&name))
        .find(|path| path.as_os_str().len() <= SOCKET_PATH_MAX)
}

/// Where control sockets may go, preferred first. Created private on demand by
/// [`private_dir`].
pub fn socket_dirs() -> Vec<PathBuf> {
    let runtime = crate::config::runtime_base().join("cctop");
    std::iter::once(runtime)
        .chain(dirs::home_dir().map(|h| h.join(".ssh")))
        .collect()
}

/// Make `dir` exist and be this user's alone, as ssh insists a socket's
/// directory is.
pub fn private_dir(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let _ = std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
    true
}

/// Whether a master is answering on `socket`.
pub fn is_up(socket: &Path, host: &str) -> bool {
    Command::new("ssh")
        .arg("-S")
        .arg(socket)
        .args(["-O", "check"])
        .arg(host)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The ssh options every call over a master uses.
///
/// `ControlMaster=no` so a call never tries to become a master of its own,
/// and `BatchMode` so one made after the master has gone fails rather than
/// asking for a password on a terminal that is not its to use.
pub fn mux_args(socket: &Path) -> Vec<std::ffi::OsString> {
    vec![
        "-S".into(),
        socket.as_os_str().to_owned(),
        "-o".into(),
        "ControlMaster=no".into(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-T".into(),
    ]
}

/// What a failed non-interactive connect said, in the few words the field has
/// room for.
///
/// The cases that matter are the ones the person can do something about: a
/// host that needs a password or a key prompt still works from the launch tab,
/// which has a terminal to ask on, and saying so is the difference between
/// "broken" and "fine, just not completable".
pub fn failure_reason(stderr: &str, timed_out: bool) -> String {
    if timed_out {
        return "timed out".to_string();
    }
    let text = stderr.to_lowercase();
    let said = |needle: &str| text.contains(needle);
    if said("permission denied") || said("passphrase") || said("keyboard-interactive") {
        return "needs a password or key prompt".to_string();
    }
    if said("host key verification failed") || said("remote host identification has changed") {
        return "host key not trusted yet".to_string();
    }
    if said("could not resolve hostname") {
        return "unknown host".to_string();
    }
    if said("connection refused") {
        return "connection refused".to_string();
    }
    if said("connection timed out") || said("operation timed out") {
        return "timed out".to_string();
    }
    if said("no route to host") || said("network is unreachable") {
        return "unreachable".to_string();
    }
    stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(|l| crate::util::truncate(l, 60))
        .unwrap_or_else(|| "ssh failed".to_string())
}

/// The hosts this process holds a lease on, to let go of at exit.
static HELD: Mutex<Vec<(String, PathBuf)>> = Mutex::new(Vec::new());

/// Start a master for `host` without ever prompting, or join the one already
/// there. Blocks for up to [`CONNECT_DEADLINE`]; call it off the UI thread.
///
/// On success this process holds a lease on the master, let go of by
/// [`release_all`].
pub fn connect(host: &str) -> Result<PathBuf, String> {
    let socket = socket_for(host).ok_or("no directory short enough for an ssh socket")?;
    if let Some(dir) = socket.parent()
        && !private_dir(dir)
    {
        return Err(format!("cannot create {}", dir.display()));
    }
    if !is_up(&socket, host) {
        start(host, &socket)?;
    }
    take_lease(&socket, std::process::id());
    if let Ok(mut held) = HELD.lock()
        && !held.iter().any(|(h, _)| h == host)
    {
        held.push((host.to_string(), socket.clone()));
    }
    Ok(socket)
}

/// `ssh -f -N -M`: authenticate, then fork into the background as the master.
///
/// `-f` is what makes "connected" a plain exit status — ssh forks only once
/// authentication is done and the control socket is listening — and lets the
/// master outlive this process, which a launch tab borrowing it needs.
fn start(host: &str, socket: &Path) -> Result<(), String> {
    let log = socket.with_extension("log");
    let stderr = std::fs::File::create(&log).map_err(|e| e.to_string())?;
    let mut cmd = Command::new("ssh");
    cmd.args(["-f", "-N", "-M", "-S"])
        .arg(socket)
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            &format!("ControlPersist={PERSIST_IDLE}"),
            "-o",
            &format!("ConnectTimeout={CONNECT_TIMEOUT_SECS}"),
            "-o",
            "ClearAllForwardings=yes",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
        ])
        .arg(host)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // A file, not a pipe: the backgrounded master keeps whatever stderr it
        // was given, and a pipe would then never reach end-of-file.
        .stderr(stderr);
    // No controlling terminal, so nothing ssh does can reach the one the TUI
    // is drawing on — BatchMode says not to ask, and this makes sure.
    // SAFETY: setsid is async-signal-safe and allocates nothing.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("could not run ssh: {e}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() > CONNECT_DEADLINE => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    match status {
        Some(s) if s.success() && is_up(socket, host) => Ok(()),
        Some(_) => Err(failure_reason(&said, false)),
        None => Err(failure_reason(&said, true)),
    }
}

/// The directory of lease files beside `socket`.
fn lease_dir(socket: &Path) -> PathBuf {
    let mut name = socket.as_os_str().to_owned();
    name.push(".leases");
    PathBuf::from(name)
}

/// Record that `pid` is using the master on `socket`.
pub fn take_lease(socket: &Path, pid: u32) {
    let dir = lease_dir(socket);
    if private_dir(&dir) {
        let _ = std::fs::write(dir.join(pid.to_string()), "");
    }
}

/// The pids holding a lease that are still running.
fn live_leases(socket: &Path) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir(lease_dir(socket)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| Path::new(&format!("/proc/{pid}")).exists())
        .collect()
}

/// Let go of `pid`'s lease, and take the master down if nobody else holds one.
///
/// Returns whether the master was asked to exit.
pub fn release(host: &str, socket: &Path, pid: u32) -> bool {
    release_with(socket, pid, || {
        let _ = Command::new("ssh")
            .arg("-S")
            .arg(socket)
            .args(["-O", "exit"])
            .arg(host)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    })
}

/// [`release`] with the exit itself passed in, so the counting can be tested
/// without an ssh.
fn release_with(socket: &Path, pid: u32, exit: impl FnOnce()) -> bool {
    let dir = lease_dir(socket);
    let _ = std::fs::remove_file(dir.join(pid.to_string()));
    if !live_leases(socket).is_empty() {
        return false;
    }
    exit();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(socket);
    true
}

/// Let go of every master this process connected to, at exit.
pub fn release_all() {
    let held = match HELD.lock() {
        Ok(mut held) => std::mem::take(&mut *held),
        Err(_) => return,
    };
    for (host, socket) in held {
        release(&host, &socket, std::process::id());
    }
}

/// Something that runs one shell command line on a host and returns its
/// stdout: ssh over the master in cctop, a local `sh` in the tests.
pub trait Runner: Send + Sync {
    fn run(&self, host: &str, command: &str, timeout: Duration) -> Result<String, String>;
}

/// The real [`Runner`]: `ssh` over the host's shared master, never prompting.
pub struct Ssh;

impl Runner for Ssh {
    fn run(&self, host: &str, command: &str, timeout: Duration) -> Result<String, String> {
        let socket = socket_for(host).ok_or("no ssh socket")?;
        let mut cmd = Command::new("ssh");
        cmd.args(mux_args(&socket)).arg(host).arg("--").arg(command);
        run_bounded(cmd, timeout)
    }
}

/// Run `cmd`, give up on it after `timeout`, and return its stdout.
///
/// Read on a thread of its own, so a far side that stops answering is a
/// timeout here rather than a read that never returns.
pub fn run_bounded(mut cmd: Command, timeout: Duration) -> Result<String, String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run ssh: {e}"))?;
    let mut out = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(out) = out.as_mut() {
            let _ = out.read_to_end(&mut buf);
        }
        buf
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("timed out".to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let stdout = reader.join().unwrap_or_default();
    if !status.success() {
        let mut err = String::new();
        if let Some(mut e) = child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        return Err(failure_reason(&err, false));
    }
    String::from_utf8(stdout).map_err(|_| "output was not UTF-8".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_is_stable_short_and_per_host() {
        let a = socket_for("procdb").expect("a socket path");
        assert_eq!(Some(a.clone()), socket_for("procdb"), "the same every time");
        assert_ne!(Some(a.clone()), socket_for("devbox"));
        assert!(a.as_os_str().len() <= SOCKET_PATH_MAX, "{}", a.display());
        let long = socket_for(&format!("someone@{}.example.com", "x".repeat(200)))
            .expect("a long host still fits");
        assert!(long.as_os_str().len() <= SOCKET_PATH_MAX);
    }

    #[test]
    fn a_failed_connect_says_what_the_person_can_do_about_it() {
        assert_eq!(
            failure_reason("flo@box: Permission denied (publickey,password).\n", false),
            "needs a password or key prompt"
        );
        assert_eq!(
            failure_reason("Host key verification failed.\n", false),
            "host key not trusted yet"
        );
        assert_eq!(
            failure_reason(
                "ssh: Could not resolve hostname nope: Name or service not known",
                false
            ),
            "unknown host"
        );
        assert_eq!(failure_reason("", true), "timed out");
        assert_eq!(
            failure_reason("\nsomething odd\n\n", false),
            "something odd"
        );
        assert_eq!(failure_reason("", false), "ssh failed");
    }

    /// The last one out takes the master down; anyone still running keeps it.
    #[test]
    fn a_master_is_kept_while_any_lease_is_live() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("m-test.sock");
        // This process is alive; pid 0x7fff_fff0 is not.
        take_lease(&socket, std::process::id());
        take_lease(&socket, 0x7fff_fff0);
        assert_eq!(live_leases(&socket), vec![std::process::id()]);
        // A dead holder letting go leaves the live one's lease, so no exit.
        let mut exited = false;
        assert!(!release_with(&socket, 0x7fff_fff0, || exited = true));
        assert!(!exited);
        // The last live one out asks the master to go, and the bookkeeping
        // goes with it.
        assert!(release_with(&socket, std::process::id(), || exited = true));
        assert!(exited);
        assert!(!lease_dir(&socket).exists());
    }

    #[test]
    fn a_command_that_overruns_is_cut_off() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 5"]);
        let started = Instant::now();
        assert_eq!(
            run_bounded(cmd, Duration::from_millis(200)),
            Err("timed out".to_string())
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo hi"]);
        assert_eq!(run_bounded(cmd, Duration::from_secs(5)), Ok("hi\n".into()));
    }
}
