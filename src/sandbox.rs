//! `cctop sandbox <host>:<path> [claude args…]`: Claude Code here, its work there.
//!
//! The agent runs on this machine — its terminal, its login, its hooks, the
//! dashboard watching it — while what it *does* lands on another one. Nothing
//! is installed on the far side: it needs `bash`, `setsid` and an ssh login,
//! which every Linux box anyone would point an agent at already has.
//!
//! Three pieces of ordinary plumbing make that work, and this module is the
//! glue between them:
//!
//! - **One ssh connection.** A ControlMaster started as a child of this
//!   process, so every later ssh — the mount and each Bash call — rides it
//!   instead of authenticating again. It is the user's own OpenSSH reading the
//!   user's own config, so a host alias, a jump host or an agent-held key work
//!   exactly as they do at a prompt.
//! - **The path, mounted at the same path.** sshfs puts the host's `<path>` at
//!   the identical absolute path here, so Read, Edit, Write, Glob and Grep —
//!   which run in this process tree and know nothing of ssh — see the host's
//!   files under the names the host's shell uses for them. One path means
//!   nothing has to translate between the two.
//! - **A shell that is not here.** Claude Code runs every Bash call through
//!   `$CLAUDE_CODE_SHELL_PREFIX`, which is set to `cctop --sandbox-exec`.
//!   That helper ([`exec`]) sends the call to the host over the connection,
//!   in the same directory, and brings back its output, its exit code and the
//!   directory it ended in.
//!
//! The file tools can only see what is mounted, so a separate PreToolUse hook
//! ([`guard`]) refuses them anywhere else and tells the model to use Bash —
//! without it, a Read of `/etc/hosts` would quietly read *this* machine's.
//!
//! ponytail: the mount needs `<path>` to exist here, or to be creatable by this
//! user. A path under a home that is root's here (`/home/someone-else/…`)
//! needs a `sudo mkdir` first, and the error says so. A private user and mount
//! namespace would lift that — mount anywhere, as nobody, visible to the agent
//! alone — at the cost of the agent no longer sharing this machine's view of
//! the filesystem, which the dashboard and `cctop hook` both rely on.

use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Set on the agent, and so on everything it spawns: `host:path`.
///
/// `--sandbox-exec` reads it to know where to send a call, and `cctop hook`
/// carries it to the dashboard, which marks the row as working elsewhere.
pub const ENV_SANDBOX: &str = "CCTOP_SANDBOX";

/// The ControlMaster's socket, for `--sandbox-exec` to ride.
const ENV_SOCKET: &str = "CCTOP_SANDBOX_SOCKET";

/// `statfs`'s answer for any FUSE filesystem, `fuse` and `fuseblk` alike.
const FUSE_SUPER_MAGIC: i64 = 0x6573_5546;

/// The longest a Unix socket path may be, less a margin.
///
/// `sun_path` is 108 bytes on Linux, and ssh appends a random suffix to the
/// ControlPath while it binds — a path that fits on paper fails at bind time,
/// which is how the first prototype found out.
const SOCKET_PATH_MAX: usize = 90;

/// How long the mount is given to appear once sshfs has been started.
const MOUNT_WAIT: Duration = Duration::from_secs(20);

/// The tools whose paths are on this machine, and so are only the host's
/// inside the mount.
const FILE_TOOLS: &str = "Read|Edit|Write|MultiEdit|NotebookEdit|Glob|Grep";

/// What the user asked for: which host, and which directory on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// The ssh target as typed, so an alias from `~/.ssh/config` stays one.
    pub host: String,
    /// The directory as typed. Resolved on the host by [`probe`] before use.
    pub path: String,
}

impl Spec {
    /// Parse `host:path`, or `[v6::addr]:path`.
    ///
    /// The *first* colon separates, the reverse of `--host`'s rule: there the
    /// part after the colon is a binary's path and the target may be anything,
    /// here a directory path is the more likely thing to contain a colon than
    /// a host name is.
    pub fn parse(spec: &str) -> Option<Spec> {
        let (host, path) = match spec.strip_prefix('[') {
            Some(rest) => {
                let (addr, path) = rest.split_once("]:")?;
                (format!("[{addr}]"), path)
            }
            None => {
                let (host, path) = spec.split_once(':')?;
                (host.to_string(), path)
            }
        };
        (!host.is_empty() && host != "[]" && !path.is_empty()).then(|| Spec {
            host,
            path: path.to_string(),
        })
    }
}

/// The host half of a `CCTOP_SANDBOX` value, which is what a row shows.
pub fn host_of(sandbox: &str) -> &str {
    if sandbox.starts_with('[')
        && let Some(end) = sandbox.find("]:")
    {
        return &sandbox[..=end];
    }
    sandbox.split_once(':').map_or(sandbox, |(host, _)| host)
}

/// `text` as one word to a POSIX shell.
///
/// Bare when it cannot be misread, so the command lines this builds stay
/// readable in an error message; single-quoted otherwise.
pub fn sh_quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-+=:,@%".contains(&b));
    match plain {
        true => text.to_string(),
        false => format!("'{}'", text.replace('\'', r"'\''")),
    }
}

// ---------------------------------------------------------------------------
// `cctop sandbox`: setting up, running the agent, taking it all down
// ---------------------------------------------------------------------------

const USAGE: &str = "\
usage: cctop sandbox <host>:<path> [claude args…]

Run Claude Code here with its Bash commands running on <host>, in <path>, over
one ssh connection, and <path> mounted here at the same path with sshfs so the
file tools see the host's files. Nothing is installed on the host.

  cctop sandbox devbox:/srv/api
  cctop sandbox devbox:~/src/api --model opus

Needs sshfs and fusermount3 here (sudo apt install sshfs fuse3), and bash and
setsid on the host. <path> must exist here or be creatable by you: it is the
mount point.";

/// `cctop sandbox …`. Returns the agent's exit code.
pub fn run(args: &[String]) -> anyhow::Result<i32> {
    let Some(first) = args.first() else {
        eprintln!("{USAGE}");
        return Ok(2);
    };
    if matches!(first.as_str(), "-h" | "--help") {
        println!("{USAGE}");
        return Ok(0);
    }
    let Some(spec) = Spec::parse(first) else {
        anyhow::bail!("expected <host>:<path>, got `{first}`\n\n{USAGE}");
    };
    let fusermount = require_tools()?;
    // Absolute, because it is put in an environment variable that a shell in
    // some other directory will run.
    let exe = std::env::current_exe()?;

    let mut sandbox = Sandbox {
        host: spec.host.clone(),
        socket: socket_path(&spec)?,
        fusermount,
        master: None,
        sshfs: None,
        root: None,
        created: Vec::new(),
        log: crate::config::runtime_base().join("cctop"),
    };
    eprintln!("cctop sandbox: connecting to {}…", spec.host);
    sandbox.connect()?;
    let probe = sandbox.probe(&spec.path)?;
    let root = PathBuf::from(&probe.root);
    sandbox.created = prepare_mountpoint(&root, sandbox.fusermount)?;
    eprintln!(
        "cctop sandbox: mounting {}:{} at the same path here…",
        spec.host, probe.root
    );
    sandbox.mount(&root)?;

    let target = format!("{}:{}", spec.host, probe.root);
    let env = vec![
        // Claude Code wants a bash-family shell for its own command strings;
        // the host's shell is reached through the prefix, not through this.
        ("SHELL".to_string(), "/bin/bash".to_string()),
        ("CLAUDE_CODE_SHELL".to_string(), "/bin/bash".to_string()),
        // Claude Code splits the prefix at its last " -", quoting the part
        // before as one executable and passing the rest as words. A prefix with
        // no " -" in it is taken whole as a program name — which is why the
        // helper is a flag rather than a subcommand.
        (
            "CLAUDE_CODE_SHELL_PREFIX".to_string(),
            format!("{} --sandbox-exec", exe.display()),
        ),
        (ENV_SANDBOX.to_string(), target.clone()),
        (
            ENV_SOCKET.to_string(),
            sandbox.socket.to_string_lossy().into_owned(),
        ),
    ];
    let mut argv = vec![
        "claude".to_string(),
        // Scoped to this launch: the user's settings files are not touched,
        // and the guard does not outlive the sandbox it guards.
        "--settings".to_string(),
        guard_settings(&exe, &probe.root),
        "--append-system-prompt".to_string(),
        system_prompt(&spec.host, &probe),
    ];
    argv.extend(args[1..].iter().cloned());

    let stop = Stop::install();
    let code = crate::shim::run_in(&argv, Some(&root), &env, |pid| stop.watch(pid))?;
    // Dropping `sandbox` unmounts and closes the connection.
    drop(sandbox);
    Ok(code)
}

/// The fusermount to unmount with, once sshfs is known to be here too.
fn require_tools() -> anyhow::Result<&'static str> {
    let on_path = |name: &str| {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .any(|dir| dir.join(name).is_file())
    };
    let fusermount = ["fusermount3", "fusermount"]
        .into_iter()
        .find(|name| on_path(name));
    match (on_path("sshfs"), fusermount) {
        (true, Some(fusermount)) => Ok(fusermount),
        _ => anyhow::bail!(
            "cctop sandbox needs sshfs and fusermount3 on this machine.\n  \
             Debian/Ubuntu: sudo apt install sshfs fuse3"
        ),
    }
}

/// Where the ControlMaster listens: short, private, and one per sandbox.
///
/// The pid is in the name so two sandboxes on one host do not share a master
/// — each tears its own down, and a shared one would leave the second
/// without a connection the moment the first exited.
fn socket_path(spec: &Spec) -> anyhow::Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (&spec.host, &spec.path, std::process::id()).hash(&mut hasher);
    let name = format!("sbx-{:08x}.sock", hasher.finish() as u32);
    let runtime = crate::config::runtime_base().join("cctop");
    let home_ssh = dirs::home_dir().map(|h| h.join(".ssh"));
    for dir in std::iter::once(runtime).chain(home_ssh) {
        let path = dir.join(&name);
        if path.as_os_str().len() <= SOCKET_PATH_MAX && std::fs::create_dir_all(&dir).is_ok() {
            let _ =
                std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));
            return Ok(path);
        }
    }
    anyhow::bail!("no directory short enough for an ssh control socket (the limit is 108 bytes)")
}

/// Everything set up so far, taken down in reverse when dropped.
///
/// Drop rather than a function at the end of [`run`], so a failure halfway —
/// a mount that never appears, an agent that will not start — undoes exactly
/// what had been done and no more.
struct Sandbox {
    host: String,
    socket: PathBuf,
    fusermount: &'static str,
    master: Option<Child>,
    sshfs: Option<Child>,
    /// Where the mount is, once it is.
    root: Option<PathBuf>,
    /// Directories made to mount on, deepest first, removed again if empty.
    created: Vec<PathBuf>,
    /// The directory the two daemons' stderr goes to.
    log: PathBuf,
}

/// What the host said about itself.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Probe {
    hostname: String,
    uname: String,
    /// The directory, absolute and with symlinks resolved — the name the
    /// host's `pwd -P` will report, so the one the mount must use.
    root: String,
}

impl Sandbox {
    /// A log file for one of the daemons, which must not write over the
    /// agent's screen once it is drawing.
    fn log_file(&self, name: &str) -> (PathBuf, Stdio) {
        let path = self
            .log
            .join(format!("sandbox-{}-{name}.log", std::process::id()));
        let _ = std::fs::create_dir_all(&self.log);
        let stdio = std::fs::File::create(&path)
            .map(Stdio::from)
            .unwrap_or_else(|_| Stdio::null());
        (path, stdio)
    }

    /// Start the ControlMaster and wait until it answers.
    ///
    /// No timeout of its own beyond ssh's connect timeout: a passphrase or a
    /// host key question is asked on the terminal, and may take as long as the
    /// person takes. ssh asks on `/dev/tty`, so its stdin is not needed.
    fn connect(&mut self) -> anyhow::Result<()> {
        let (log, stderr) = self.log_file("ssh");
        let mut cmd = Command::new("ssh");
        cmd.args(["-N", "-M", "-S"])
            .arg(&self.socket)
            .args([
                "-o",
                "ControlPersist=no",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "ConnectTimeout=15",
            ])
            .arg(&self.host)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr);
        die_with_parent(&mut cmd);
        self.master = Some(cmd.spawn()?);
        loop {
            if let Some(Ok(Some(status))) = self.master.as_mut().map(Child::try_wait) {
                self.master = None;
                anyhow::bail!(
                    "could not connect to {} (ssh {status}){}",
                    self.host,
                    tail_of(&log)
                );
            }
            let up = Command::new("ssh")
                .arg("-S")
                .arg(&self.socket)
                .args(["-O", "check"])
                .arg(&self.host)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if up {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// One round trip that asks everything the setup needs to know.
    fn probe(&self, path: &str) -> anyhow::Result<Probe> {
        let out = Command::new("ssh")
            .args(mux_args(&self.socket))
            .arg(&self.host)
            .arg("--")
            .arg(probe_command(path))
            .stdin(Stdio::null())
            .output()?;
        let lines = String::from_utf8_lossy(&out.stdout).into_owned();
        let said = parse_probe(&lines);
        let has = |key: &str| {
            lines
                .lines()
                .any(|l| l.strip_prefix(key).is_some_and(|v| v.starts_with("=/")))
        };
        if !out.status.success() && said.uname.is_empty() {
            anyhow::bail!(
                "{} did not answer the probe: {}",
                self.host,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        if !has("bash") {
            anyhow::bail!(
                "{} has no bash on its PATH, which every Bash call runs in",
                self.host
            );
        }
        if !has("setsid") {
            anyhow::bail!(
                "{} has no setsid on its PATH, which is how an interrupted command is stopped there",
                self.host
            );
        }
        if said.root.is_empty() {
            anyhow::bail!("{path} is not a directory on {}", self.host);
        }
        Ok(said)
    }

    /// Mount the host's `root` at `root` here and wait until it is up.
    fn mount(&mut self, root: &Path) -> anyhow::Result<()> {
        let (log, stderr) = self.log_file("sshfs");
        let ssh_command = format!(
            "ssh -S {} -o ControlMaster=no -o BatchMode=yes",
            self.socket.display()
        );
        let mut cmd = Command::new("sshfs");
        // In the foreground and a child of this process, rather than the
        // daemon sshfs would make of itself: a daemon outlives a cctop that
        // was killed, and this one is told to go when cctop does — which
        // libfuse answers by unmounting.
        cmd.arg("-f")
            .arg("-o")
            .arg(format!("ssh_command={ssh_command}"))
            .args([
                "-o",
                "idmap=user,reconnect,auto_cache,attr_timeout=1,entry_timeout=1,\
                 negative_timeout=0,dcache_timeout=1",
            ])
            .arg(format!("{}:{}", self.host, root.display()))
            .arg(root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr);
        die_with_parent(&mut cmd);
        self.sshfs = Some(cmd.spawn()?);
        let started = Instant::now();
        loop {
            if let Some(Ok(Some(status))) = self.sshfs.as_mut().map(Child::try_wait) {
                self.sshfs = None;
                anyhow::bail!(
                    "sshfs could not mount {} ({status}){}",
                    root.display(),
                    tail_of(&log)
                );
            }
            if is_fuse_mount(root) {
                self.root = Some(root.to_path_buf());
                return Ok(());
            }
            if started.elapsed() > MOUNT_WAIT {
                anyhow::bail!(
                    "sshfs had not mounted {} after {}s{}",
                    root.display(),
                    MOUNT_WAIT.as_secs(),
                    tail_of(&log)
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Some(root) = self.root.take() {
            // A plain unmount first, so a mount nobody is in goes cleanly; a
            // lazy one when something still holds it — the agent's shell, an
            // editor — so it is detached now and freed when they let go.
            let quiet = |args: &[&str]| {
                Command::new(self.fusermount)
                    .args(args)
                    .arg(&root)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            };
            if !quiet(&["-u"]) && !quiet(&["-uz"]) {
                eprintln!(
                    "cctop sandbox: could not unmount {}; `{} -uz {}` will",
                    root.display(),
                    self.fusermount,
                    sh_quote(&root.to_string_lossy())
                );
            }
        }
        if let Some(sshfs) = self.sshfs.take() {
            reap(sshfs);
        }
        for dir in &self.created {
            // Only ever empty by now; a directory that is not is left alone.
            let _ = std::fs::remove_dir(dir);
        }
        if let Some(master) = self.master.take() {
            let _ = Command::new("ssh")
                .arg("-S")
                .arg(&self.socket)
                .args(["-O", "exit"])
                .arg(&self.host)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            reap(master);
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Wait a moment for a child that has been asked to go, then make it.
fn reap(mut child: Child) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        if let Ok(Some(_)) = child.try_wait() {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Have the kernel end `cmd`'s process when this one ends, however it ends.
///
/// The ssh master and sshfs are what a `kill -9` of cctop would otherwise
/// leave running, holding a connection and a mount nobody is using.
fn die_with_parent(cmd: &mut Command) {
    // SAFETY: between fork and exec; prctl is a raw syscall and allocates
    // nothing.
    unsafe {
        cmd.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// The last few lines of a daemon's log, for an error that wants its words.
fn tail_of(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    match lines.len() {
        0 => String::new(),
        n => format!(":\n  {}", lines[n.saturating_sub(5)..].join("\n  ")),
    }
}

/// The ssh options every call over the master uses.
///
/// `ControlMaster=no` so a call never tries to become a master of its own,
/// and `BatchMode` so one made after the master has gone fails rather than
/// asking for a password on a terminal that belongs to the agent.
fn mux_args(socket: &Path) -> Vec<std::ffi::OsString> {
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

/// The probe, as one command line for the host's login shell.
///
/// `sh` rather than the login shell's own syntax, whatever that is. A path
/// may start with `~`, which is expanded there, where the home it means is.
fn probe_command(path: &str) -> String {
    const SCRIPT: &str = r#"p=$1
case $p in "~") p=$HOME ;; "~/"*) p=$HOME/${p#"~/"} ;; esac
echo "hostname=$(hostname 2>/dev/null || uname -n)"
echo "uname=$(uname -sr)"
echo "bash=$(command -v bash)"
echo "setsid=$(command -v setsid)"
if cd -- "$p" 2>/dev/null; then echo "root=$(pwd -P)"; fi"#;
    format!(
        "exec sh -c {} cctop-sandbox {}",
        sh_quote(SCRIPT),
        sh_quote(path)
    )
}

fn parse_probe(out: &str) -> Probe {
    let mut probe = Probe::default();
    for line in out.lines() {
        match line.split_once('=') {
            Some(("hostname", v)) => probe.hostname = v.trim().to_string(),
            Some(("uname", v)) => probe.uname = v.trim().to_string(),
            Some(("root", v)) if v.starts_with('/') => probe.root = v.to_string(),
            _ => {}
        }
    }
    probe
}

/// Make `root` ready to be mounted on, returning the directories made for it.
///
/// A mount left behind by a cctop that did not get to clean up answers every
/// call with "Transport endpoint is not connected"; it is lazily unmounted
/// here rather than reported, since it is this feature's own debris. Anything
/// else already mounted there is someone else's and is left alone.
fn prepare_mountpoint(root: &Path, fusermount: &str) -> anyhow::Result<Vec<PathBuf>> {
    if let Some(fstype) = mount_type_at(root) {
        let stale = fstype.starts_with("fuse")
            && std::fs::metadata(root).err().and_then(|e| e.raw_os_error()) == Some(libc::ENOTCONN);
        if !stale {
            anyhow::bail!(
                "{} is already a mount point ({fstype}) — is another `cctop sandbox` using it?",
                root.display()
            );
        }
        eprintln!(
            "cctop sandbox: clearing a stale mount at {}",
            root.display()
        );
        let _ = Command::new(fusermount)
            .arg("-uz")
            .arg(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    match std::fs::metadata(root) {
        Ok(meta) if !meta.is_dir() => {
            anyhow::bail!("{} exists here and is not a directory", root.display())
        }
        Ok(_) => {
            // Mounting over files would hide them for as long as the agent
            // ran, and they would look like the host's.
            if std::fs::read_dir(root)?.next().is_some() {
                anyhow::bail!(
                    "{} exists here and is not empty; the host's directory is mounted at the \
                     same path, and would hide what is there",
                    root.display()
                );
            }
            Ok(Vec::new())
        }
        Err(_) => {
            let mut missing: Vec<PathBuf> = root
                .ancestors()
                .take_while(|dir| !dir.exists())
                .map(Path::to_path_buf)
                .collect();
            if let Err(e) = std::fs::create_dir_all(root) {
                let quoted = sh_quote(&root.to_string_lossy());
                anyhow::bail!(
                    "{} has to exist here to mount the host's directory on, and could not be \
                     created ({e}). Make it once with:\n  sudo mkdir -p {quoted} && sudo chown \
                     $USER {quoted}",
                    root.display()
                );
            }
            // Deepest first, which is the order they can be removed in.
            missing.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
            Ok(missing)
        }
    }
}

/// The filesystem type mounted at exactly `path`, if anything is.
fn mount_type_at(path: &Path) -> Option<String> {
    let table = std::fs::read_to_string("/proc/self/mountinfo").ok()?;
    mount_type_in(&table, path)
}

/// [`mount_type_at`] over a given mountinfo table. The last match wins, since
/// a later mount on the same point is the one on top.
fn mount_type_in(mountinfo: &str, path: &Path) -> Option<String> {
    let want = path.to_string_lossy();
    mountinfo
        .lines()
        .rev()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let point = left.split(' ').nth(4)?;
            let fstype = right.split(' ').next()?;
            (unescape_mountinfo(point) == want).then(|| fstype.to_string())
        })
        .next()
}

/// mountinfo spells a space, tab, newline and backslash as octal escapes.
fn unescape_mountinfo(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && i + 4 <= bytes.len()
            && let Ok(code) = u8::from_str_radix(&field[i + 1..i + 4], 8)
        {
            out.push(code);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether a live FUSE filesystem is mounted at exactly `path`.
///
/// Both halves: the table says the mount is *there* rather than on some
/// parent, and `statfs` answering with FUSE's magic says it is up rather
/// than still being set up or already dead.
fn is_fuse_mount(path: &Path) -> bool {
    if !mount_type_at(path).is_some_and(|t| t.starts_with("fuse")) {
        return false;
    }
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()) else {
        return false;
    };
    // SAFETY: a valid C string and a zeroed statfs the call fills in.
    let mut buf: libc::statfs = unsafe { std::mem::zeroed() };
    let rc = unsafe { libc::statfs(c_path.as_ptr(), &raw mut buf) };
    rc == 0 && buf.f_type as i64 == FUSE_SUPER_MAGIC
}

/// The `--settings` that installs the guard for this launch alone.
fn guard_settings(exe: &Path, root: &str) -> String {
    let command = format!(
        "{} --sandbox-guard {}",
        sh_quote(&exe.to_string_lossy()),
        sh_quote(root)
    );
    serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": FILE_TOOLS,
                "hooks": [{ "type": "command", "command": command }],
            }],
        },
    })
    .to_string()
}

/// What the model is told about where it is.
fn system_prompt(host: &str, probe: &Probe) -> String {
    format!(
        "You are working on the remote host `{host}` (hostname `{hostname}`, {uname}) through \
         cctop sandbox. Your Bash tool runs every command on that host, over ssh, in the same \
         working directory — programs, services, processes, package managers and environment \
         are the host's. The file tools (Read, Edit, Write, Glob, Grep, NotebookEdit) run on the \
         local machine and see the host's {root} through an sshfs mount at the same path; they \
         are refused anywhere outside it. To read or change any other path on the host, use \
         Bash.",
        hostname = probe.hostname,
        uname = probe.uname,
        root = probe.root,
    )
}

/// Ending the agent when cctop itself is told to stop.
///
/// `run` holds the terminal in raw mode, so Ctrl-C is a keystroke for the
/// agent, not a signal for cctop; what can still arrive is a SIGTERM or a
/// SIGHUP from outside. Left to their default they end cctop on the spot and
/// skip the unmount. Here they hang the agent up instead, `run_in` returns as
/// it would if the agent had quit, and the teardown runs on the ordinary path.
struct Stop {
    pid: std::sync::Arc<std::sync::atomic::AtomicU32>,
}

/// The write end of the pipe the signal handler pokes.
static STOP_PIPE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

extern "C" fn on_stop_signal(_: libc::c_int) {
    let fd = STOP_PIPE.load(std::sync::atomic::Ordering::Relaxed);
    if fd >= 0 {
        // SAFETY: write(2) is async-signal-safe, and the byte outlives it.
        unsafe { libc::write(fd, [1u8].as_ptr().cast(), 1) };
    }
}

impl Stop {
    fn install() -> Stop {
        let pid = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let mut fds = [-1; 2];
        // SAFETY: a two-int array for pipe2 to fill.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == 0 {
            STOP_PIPE.store(fds[1], std::sync::atomic::Ordering::Relaxed);
            for signal in [libc::SIGTERM, libc::SIGHUP] {
                // SAFETY: the handler only performs a write(2).
                unsafe {
                    libc::signal(
                        signal,
                        on_stop_signal as extern "C" fn(libc::c_int) as libc::sighandler_t,
                    )
                };
            }
            let read_fd = fds[0];
            let pid = std::sync::Arc::clone(&pid);
            std::thread::spawn(move || {
                let mut byte = [0u8; 1];
                // SAFETY: the read end is ours alone and outlives the thread.
                if unsafe { libc::read(read_fd, byte.as_mut_ptr().cast(), 1) } == 1 {
                    let agent = pid.load(std::sync::atomic::Ordering::Relaxed);
                    if agent > 0 {
                        // SAFETY: plain kill(2) of the agent we spawned.
                        unsafe { libc::kill(agent as libc::pid_t, libc::SIGHUP) };
                        std::thread::sleep(Duration::from_secs(5));
                        unsafe { libc::kill(agent as libc::pid_t, libc::SIGKILL) };
                    }
                }
            });
        }
        Stop { pid }
    }

    fn watch(&self, pid: u32) {
        self.pid.store(pid, std::sync::atomic::Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// `cctop --sandbox-exec`: one Bash call, run on the host
// ---------------------------------------------------------------------------

/// What one call through the shell prefix turned out to be.
#[derive(Debug, PartialEq, Eq)]
pub enum Call<'a> {
    /// A Bash tool call: `head` is the command with Claude Code's closing
    /// `pwd -P >| <file>` taken off, and `cwd_file` is that file.
    Remote { head: &'a str, cwd_file: String },
    /// A hook, a status line — anything that is not a Bash call. Run here.
    Local,
    /// It ends the way a Bash call ends but could not be taken apart.
    /// Refused: running it here would act on the wrong machine.
    Unreadable,
}

/// Tell a Bash tool call from everything else Claude Code runs through the
/// prefix.
///
/// A Bash call ends `… && pwd -P >| /tmp/claude-<hex>-cwd`, which is how
/// Claude Code learns where a `cd` left the shell. The file is unquoted in the
/// versions seen so far (it has nothing to quote), and accepted single-quoted
/// too, in case a later one quotes everything. Hooks and status lines are the
/// user's own commands and end however they end.
pub fn classify(command: &str) -> Call<'_> {
    const PWD: &str = "pwd -P >|";
    let trimmed = command.trim_end();
    let Some(at) = trimmed.rfind(PWD) else {
        return Call::Local;
    };
    let file = trimmed[at + PWD.len()..].trim();
    let file = match file.strip_prefix('\'').and_then(|f| f.strip_suffix('\'')) {
        Some(quoted) if !quoted.contains('\'') => quoted,
        Some(_) => return Call::Unreadable,
        None if !file.is_empty() && !file.contains(char::is_whitespace) => file,
        // `pwd -P >|` somewhere in a hook's own text, not at its end.
        None if file.contains(char::is_whitespace) => return Call::Local,
        None => return Call::Unreadable,
    };
    if !file.starts_with('/') {
        return Call::Unreadable;
    }
    match trimmed[..at].trim_end().strip_suffix("&&") {
        Some(head) => Call::Remote {
            head: head.trim_end(),
            cwd_file: file.to_string(),
        },
        None => Call::Unreadable,
    }
}

/// `cctop --sandbox-exec <command>`: what Claude Code calls instead of bash.
pub fn exec(args: &[String]) -> i32 {
    let command = args.first().map(String::as_str).unwrap_or_default();
    let target = std::env::var(ENV_SANDBOX).ok().filter(|t| !t.is_empty());
    let socket = std::env::var_os(ENV_SOCKET).filter(|s| !s.is_empty());
    match (classify(command), target, socket) {
        (Call::Remote { head, cwd_file }, Some(target), Some(socket)) => {
            match remote(head, &cwd_file, &target, Path::new(&socket)) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!(
                        "cctop sandbox: could not run this on {}: {e}",
                        host_of(&target)
                    );
                    255
                }
            }
        }
        (Call::Unreadable, target, _) => {
            eprintln!(
                "cctop sandbox: this Bash call was not in a shape cctop recognises, so it was \
                 not run — running it here would act on this machine rather than on {}.",
                target.as_deref().map(host_of).unwrap_or("the host")
            );
            1
        }
        // A hook or a status line — or a sandbox variable gone missing, in
        // which case there is no host to send to and this is plain bash. The
        // exec hands over stdin, stdout and the exit code untouched, which is
        // what keeps `cctop hook` returning exactly what it meant to.
        _ => {
            let e = Command::new("/bin/bash").arg("-c").arg(command).exec();
            eprintln!("cctop sandbox: could not run bash: {e}");
            127
        }
    }
}

/// Run one Bash call on the host and report back as bash would have.
fn remote(head: &str, cwd_file: &str, target: &str, socket: &Path) -> std::io::Result<i32> {
    let host = host_of(target);
    let cwd = std::env::current_dir()?;
    let nonce = nonce();
    let mut cmd = Command::new("ssh");
    cmd.args(mux_args(socket))
        .arg(host)
        .arg("--")
        .arg(remote_line(head, &nonce, &cwd.to_string_lossy()));
    let mut stderr = std::io::stderr();
    let (code, cwd_now) = run_remote(cmd, &nonce, &mut stderr)?;
    if let Some(dir) = cwd_now {
        let _ = std::fs::write(cwd_file, format!("{dir}\n"));
    }
    if code == 255 {
        let _ = writeln!(
            stderr,
            "cctop sandbox: ssh to {host} exited 255 — the connection failed, or the command \
             itself exited 255."
        );
    }
    Ok(code)
}

/// A tag no command's output will contain by accident.
fn nonce() -> String {
    let mut bytes = [0u8; 8];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if read.is_err() {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
            ^ u128::from(std::process::id());
        bytes.copy_from_slice(&seed.to_le_bytes()[..8]);
    }
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("@@cctop-cwd-{hex}@@")
}

/// The far side of one call, around Claude Code's own command.
///
/// `$1` is the command, `$2` the directory. It runs in a session of its own
/// so the whole of it can be stopped as a group; the watchdog reads the ssh
/// channel's stdin, which this side never writes and only ever closes — by
/// dying, when Claude Code stops the call — and stops the group when it does.
///
/// Every helper here is waited for before the script exits. The prototype's
/// watchdog was a subshell around `cat`, and killing `$!` killed the subshell
/// and left the `cat`: one stray process per call, each holding the channel,
/// and a mount that would not unmount at the end. A `read` builtin keeps the
/// watchdog one process, and its `sleep` is told to let go of the channel.
const WATCHDOG: &str = r#"cd -- "$2" 2>/dev/null || { printf 'cctop sandbox: %s is not a directory on %s\n' "$2" "$(hostname)" >&2; exit 1; }
exec 3<&0 0</dev/null
setsid bash -lc "$1" 3<&- &
pid=$!
{ while read -r -u 3 _; do :; done; kill -TERM -- "-$pid"; sleep 2 3<&-; kill -KILL -- "-$pid"; } >/dev/null 2>&1 &
watchdog=$!
exec 3<&-
wait "$pid"; rc=$?
kill "$watchdog" 2>/dev/null; wait "$watchdog" 2>/dev/null
kill -TERM -- "-$pid" 2>/dev/null
exit "$rc""#;

/// The command line the host's login shell is handed for one call.
///
/// Claude Code's `pwd -P >| <file>` would write the directory on the host,
/// where nothing reads it. It is replaced by a line on stderr tagged with
/// `nonce`, which [`Strip`] takes back out — no second round trip to fetch a
/// file, and nothing left behind on the host. Only after `&&`, as Claude
/// Code's own was: a failed command leaves the directory where it was.
fn remote_line(head: &str, nonce: &str, cwd: &str) -> String {
    let command = format!("{head} && printf '\\n{nonce}%s\\n' \"$(pwd -P)\" >&2");
    format!(
        "exec bash -c {} cctop-sandbox {} {}",
        sh_quote(WATCHDOG),
        sh_quote(&command),
        sh_quote(cwd)
    )
}

/// Run a prepared ssh call: stdout straight through, stderr through [`Strip`].
///
/// Returns the exit code and the directory the call ended in, if it said.
fn run_remote(
    mut cmd: Command,
    nonce: &str,
    err: &mut dyn Write,
) -> std::io::Result<(i32, Option<String>)> {
    let mut child = cmd.stdin(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    // Held, never written: the far side's watchdog is waiting for it to close.
    let hold = child.stdin.take();
    let mut strip = Strip::new(nonce);
    if let Some(mut from) = child.stderr.take() {
        let mut buf = [0u8; 8192];
        loop {
            match from.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = err.write_all(&strip.feed(&buf[..n]));
                    let _ = err.flush();
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    }
    let _ = err.write_all(&strip.finish());
    let status = child.wait()?;
    drop(hold);
    use std::os::unix::process::ExitStatusExt;
    let code = status
        .code()
        .or_else(|| status.signal().map(|s| 128 + s))
        .unwrap_or(1);
    Ok((code, strip.cwd))
}

/// Takes the directory line back out of a call's stderr, as it streams.
///
/// The line is `\n<nonce><dir>\n`, its leading newline included so it is
/// whole even after output that did not end in one. Everything else passes
/// through byte for byte; only bytes that could still turn out to be the start
/// of the tag are held back, and only until the next read says they are not.
pub struct Strip {
    tag: Vec<u8>,
    held: Vec<u8>,
    state: StripState,
    pub cwd: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum StripState {
    Looking,
    Reading,
    Done,
}

impl Strip {
    pub fn new(nonce: &str) -> Strip {
        let mut tag = b"\n".to_vec();
        tag.extend_from_slice(nonce.as_bytes());
        Strip {
            tag,
            held: Vec::new(),
            state: StripState::Looking,
            cwd: None,
        }
    }

    /// The bytes of `chunk` that are output, now that they are known to be.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<u8> {
        self.held.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            match self.state {
                StripState::Done => {
                    out.append(&mut self.held);
                    return out;
                }
                StripState::Reading => {
                    let Some(end) = self.held.iter().position(|&b| b == b'\n') else {
                        return out;
                    };
                    self.cwd = Some(String::from_utf8_lossy(&self.held[..end]).into_owned());
                    self.held.drain(..=end);
                    self.state = StripState::Done;
                }
                StripState::Looking => {
                    if let Some(at) = find(&self.held, &self.tag) {
                        out.extend_from_slice(&self.held[..at]);
                        self.held.drain(..at + self.tag.len());
                        self.state = StripState::Reading;
                        continue;
                    }
                    let keep = partial_suffix(&self.held, &self.tag);
                    let pass = self.held.len() - keep;
                    out.extend(self.held.drain(..pass));
                    return out;
                }
            }
        }
    }

    /// Whatever was still held when the stream ended.
    pub fn finish(&mut self) -> Vec<u8> {
        match self.state {
            // Cut off mid-line: the directory is what arrived.
            StripState::Reading => {
                self.cwd = Some(String::from_utf8_lossy(&self.held).into_owned());
                self.held.clear();
                Vec::new()
            }
            _ => std::mem::take(&mut self.held),
        }
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// How many trailing bytes of `buf` are a proper prefix of `tag`.
fn partial_suffix(buf: &[u8], tag: &[u8]) -> usize {
    (1..tag.len().min(buf.len() + 1))
        .rev()
        .find(|&n| buf.ends_with(&tag[..n]))
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// `cctop --sandbox-guard`: keeping the file tools inside the mount
// ---------------------------------------------------------------------------

/// `cctop --sandbox-guard <root>…`, the PreToolUse hook a sandbox installs.
///
/// Not `cctop hook`, which must never answer with a decision; this one exists
/// to answer with one. Like it, though, it always exits 0: a crash here would
/// be read as a block with whatever it printed as the reason.
pub fn guard(roots: &[String]) -> i32 {
    std::panic::set_hook(Box::new(|_| std::process::exit(0)));
    let mut input = Vec::new();
    if std::io::stdin()
        .take(1024 * 1024)
        .read_to_end(&mut input)
        .is_err()
    {
        return 0;
    }
    let Ok(event) = serde_json::from_slice::<serde_json::Value>(&input) else {
        return 0;
    };
    let host = std::env::var(ENV_SANDBOX)
        .ok()
        .map(|t| host_of(&t).to_string())
        .unwrap_or_else(|| "the host".to_string());
    let roots: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    let allowed = own_state_dirs();
    if let Some(reason) = refusal(&event, &roots, &allowed, &host) {
        let answer = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            },
        });
        println!("{answer}");
    }
    0
}

/// Where Claude Code keeps its own files on this machine, which it must go on
/// reaching: plans and memory under `~/.claude`, a background task's output
/// under the temporary directory. None of it is the host's business.
fn own_state_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = dirs::home_dir()
        .map(|h| h.join(".claude"))
        .into_iter()
        .collect();
    dirs.push(std::env::temp_dir());
    dirs
}

/// Why this tool call is refused, or `None` to let it through.
fn refusal(
    event: &serde_json::Value,
    roots: &[PathBuf],
    own: &[PathBuf],
    host: &str,
) -> Option<String> {
    let tool = event.get("tool_name")?.as_str()?;
    let input = event.get("tool_input")?;
    let field = |key: &str| {
        input
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
    };
    let path = match tool {
        "Read" | "Edit" | "Write" | "MultiEdit" => field("file_path")?.to_string(),
        "NotebookEdit" => field("notebook_path")?.to_string(),
        // No path means the working directory, which is inside the mount.
        // An absolute pattern names its own directory: everything before its
        // first wildcard.
        "Glob" | "Grep" => match (field("path"), field("pattern")) {
            (Some(path), _) => path.to_string(),
            (None, Some(pattern)) if tool == "Glob" && pattern.starts_with('/') => {
                let fixed = pattern.split(['*', '?', '[', '{']).next().unwrap_or("/");
                match fixed.rfind('/') {
                    Some(0) | None => "/".to_string(),
                    Some(end) => fixed[..end].to_string(),
                }
            }
            _ => return None,
        },
        _ => return None,
    };
    let cwd = event.get("cwd").and_then(|v| v.as_str()).unwrap_or("/");
    let full = normalise(&Path::new(cwd).join(&path));
    if roots.iter().any(|root| full.starts_with(root))
        || own.iter().any(|dir| is_claude_state(&full, dir))
    {
        return None;
    }
    let roots: Vec<String> = roots.iter().map(|r| r.display().to_string()).collect();
    Some(format!(
        "{} is not on {host}'s mount: the file tools run on the local machine, where only {} is \
         {host}'s. To reach {} on {host}, use Bash — it runs there.",
        full.display(),
        roots.join(", "),
        full.display(),
    ))
}

/// Whether `path` is Claude Code's own: under `~/.claude`, or a `claude-*`
/// entry of the temporary directory.
fn is_claude_state(path: &Path, dir: &Path) -> bool {
    let Ok(rest) = path.strip_prefix(dir) else {
        return false;
    };
    if dir.ends_with(".claude") {
        return true;
    }
    rest.components()
        .next()
        .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with("claude"))
}

/// `..` and `.` resolved without asking the filesystem, which for a path on
/// the mount would be a round trip to the host, and for one outside it is
/// the very lookup being refused.
fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => out.push(name),
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    out
}

/// For the tests in other modules that need an ssh stand-in: one that runs
/// the "remote" command right here, the way sshd would hand it to a shell.
#[cfg(test)]
pub(crate) fn fake_ssh(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("ssh");
    std::fs::write(
        &path,
        "#!/bin/sh\n\
         while [ $# -gt 0 ]; do\n\
           case $1 in\n\
             -S|-o|-O) shift 2 ;;\n\
             --) shift; break ;;\n\
             *) shift ;;\n\
           esac\n\
         done\n\
         exec sh -c \"$*\"\n",
    )
    .expect("write fake ssh");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real Bash call from Claude Code 2.1.289, as the prefix receives it.
    const BASH_CALL: &str = "source /home/flo/.claude/shell-snapshots/snapshot-bash-1.sh 2>/dev/null || true && { shopt -u extglob || setopt NO_EXTENDED_GLOB NO_BARE_GLOB_QUAL; } >/dev/null 2>&1 || true && { \\builtin unalias -- 'unsetenv'; \\builtin unset -f -- 'unsetenv'; } >/dev/null 2>&1 || true && eval 'ls -la' < /dev/null && pwd -P >| /tmp/claude-ab12-cwd";

    #[test]
    fn a_spec_splits_at_the_first_colon() {
        assert_eq!(
            Spec::parse("procdb:/home/f/x"),
            Some(Spec {
                host: "procdb".into(),
                path: "/home/f/x".into()
            })
        );
        assert_eq!(
            Spec::parse("flo@box:~/a:b").map(|s| s.path),
            Some("~/a:b".into())
        );
        assert_eq!(
            Spec::parse("[::1]:/srv").map(|s| s.host),
            Some("[::1]".into())
        );
        for bad in ["box", ":/srv", "box:", "[::1]/srv"] {
            assert_eq!(Spec::parse(bad), None, "{bad}");
        }
        assert_eq!(host_of("procdb:/srv/x"), "procdb");
        assert_eq!(host_of("[::1]:/srv"), "[::1]");
    }

    /// The row says where the work is, and stops claiming a branch for a
    /// mount that went away with the agent.
    #[test]
    fn a_sandboxed_row_names_its_host_and_no_stale_branch() {
        use crate::ui::columns::{self, ColumnId};
        let mut s = crate::session::Session::new(crate::pricing::Provider::Claude, "s".into());
        s.sandbox = Some("procdb:/home/f/x".into());
        // The checkout this test runs in has a branch; a stopped sandboxed
        // row at the same path must not report it.
        s.label_source = env!("CARGO_MANIFEST_DIR").into();
        let cell = columns::render_cell(ColumnId::Host, &s, &chrono::Utc::now());
        assert_eq!(cell, "procdb⇄");
        assert_eq!(columns::branch_of(&s), None);
    }

    #[test]
    fn a_bash_call_is_told_from_a_hook_by_its_closing_pwd() {
        match classify(BASH_CALL) {
            Call::Remote { head, cwd_file } => {
                assert!(head.ends_with("eval 'ls -la' < /dev/null"), "{head}");
                assert_eq!(cwd_file, "/tmp/claude-ab12-cwd");
            }
            other => panic!("{other:?}"),
        }
        // A later Claude Code may quote the file.
        let quoted = "eval 'x' && pwd -P >| '/tmp/claude dir-1-cwd'";
        assert_eq!(
            classify(quoted),
            Call::Remote {
                head: "eval 'x'",
                cwd_file: "/tmp/claude dir-1-cwd".into()
            }
        );
        // Hooks and status lines run here, untouched.
        assert_eq!(classify("cctop hook PreToolUse"), Call::Local);
        assert_eq!(
            classify("'/usr/bin/cctop' --sandbox-guard /srv"),
            Call::Local
        );
        assert_eq!(classify(""), Call::Local);
        assert_eq!(classify("echo pwd -P >| /tmp/x && echo done"), Call::Local);
        // Shaped like a Bash call and not readable as one: never run here.
        assert_eq!(classify("eval 'x' && pwd -P >| relative"), Call::Unreadable);
        assert_eq!(classify("eval 'x' ; pwd -P >| /tmp/c"), Call::Unreadable);
        assert_eq!(
            classify("eval 'x' && pwd -P >| '/tmp/a'\\''b'"),
            Call::Unreadable
        );
    }

    #[test]
    fn the_directory_line_is_taken_out_and_nothing_else_is() {
        let nonce = "@@N@@";
        let run = |chunks: &[&[u8]]| {
            let mut strip = Strip::new(nonce);
            let mut out = Vec::new();
            for chunk in chunks {
                out.extend(strip.feed(chunk));
            }
            out.extend(strip.finish());
            (String::from_utf8(out).expect("utf8"), strip.cwd)
        };
        assert_eq!(
            run(&[b"warn\n\n@@N@@/srv/x\n"]),
            ("warn\n".into(), Some("/srv/x".into()))
        );
        // Output that did not end in a newline keeps not ending in one.
        assert_eq!(
            run(&[b"no newline\n@@N@@/srv\n"]),
            ("no newline".into(), Some("/srv".into()))
        );
        // Split at every awkward place.
        assert_eq!(
            run(&[b"a\n", b"\n@@", b"N", b"@@/sr", b"v\n"]),
            ("a\n".into(), Some("/srv".into()))
        );
        // Something that only looks like the start of the tag is output.
        assert_eq!(run(&[b"x\n@@", b"Q"]), ("x\n@@Q".into(), None));
        assert_eq!(run(&[b"tail\n@"]), ("tail\n@".into(), None));
        // A failed command never printed it: everything is output.
        assert_eq!(run(&[b"boom\n"]), ("boom\n".into(), None));
    }

    fn event(tool: &str, input: serde_json::Value) -> serde_json::Value {
        serde_json::json!({"tool_name": tool, "tool_input": input, "cwd": "/srv/app"})
    }

    #[test]
    fn the_file_tools_are_kept_inside_the_mount() {
        let roots = [PathBuf::from("/srv/app")];
        let own = [PathBuf::from("/home/me/.claude"), PathBuf::from("/tmp")];
        let deny = |tool: &str, input| refusal(&event(tool, input), &roots, &own, "box");
        use serde_json::json;
        assert_eq!(
            deny("Read", json!({"file_path": "/srv/app/src/x.rs"})),
            None
        );
        assert_eq!(deny("Edit", json!({"file_path": "src/x.rs"})), None);
        assert_eq!(deny("Glob", json!({"pattern": "**/*.rs"})), None);
        assert_eq!(
            deny("Grep", json!({"pattern": "fn", "path": "/srv/app/src"})),
            None
        );
        // Claude Code's own files stay reachable.
        assert_eq!(
            deny("Write", json!({"file_path": "/home/me/.claude/plans/p.md"})),
            None
        );
        assert_eq!(
            deny("Read", json!({"file_path": "/tmp/claude-1000/t/x.output"})),
            None
        );
        // Other tools are not this hook's business.
        assert_eq!(deny("Bash", json!({"command": "cat /etc/hosts"})), None);

        let reason = deny("Read", json!({"file_path": "/etc/hosts"})).expect("refused");
        assert!(
            reason.contains("/etc/hosts") && reason.contains("Bash"),
            "{reason}"
        );
        assert!(deny("Read", json!({"file_path": "/srv/app/../other/x"})).is_some());
        assert!(
            deny("Read", json!({"file_path": "/srv/apple/x"})).is_some(),
            "a prefix is not a parent"
        );
        assert!(deny("NotebookEdit", json!({"notebook_path": "/home/me/n.ipynb"})).is_some());
        assert!(deny("Grep", json!({"pattern": "x", "path": "/var/log"})).is_some());
        assert!(deny("Glob", json!({"pattern": "/etc/**/*.conf"})).is_some());
        assert!(deny("Read", json!({"file_path": "/tmp/other"})).is_some());
    }

    #[test]
    fn quoting_survives_a_shell() {
        for text in ["plain", "with space", "it's", "", "$HOME `x` \\ \"q\""] {
            let out = Command::new("sh")
                .arg("-c")
                .arg(format!("printf %s {}", sh_quote(text)))
                .output()
                .expect("sh");
            assert_eq!(String::from_utf8_lossy(&out.stdout), text);
        }
    }

    #[test]
    fn the_mount_table_is_read_at_exactly_the_path() {
        let table = "\
36 35 98:0 / / rw - ext4 /dev/sda rw
90 36 0:50 / /home/a\\040b rw,nosuid - fuse.sshfs box:/x rw
91 36 0:51 / /srv/app rw - fuseblk box:/srv/app rw";
        assert_eq!(
            mount_type_in(table, Path::new("/home/a b")).as_deref(),
            Some("fuse.sshfs")
        );
        assert_eq!(
            mount_type_in(table, Path::new("/srv/app")).as_deref(),
            Some("fuseblk")
        );
        assert_eq!(mount_type_in(table, Path::new("/srv")), None);
    }

    #[test]
    fn a_missing_mountpoint_is_made_and_a_full_one_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("a/b");
        let made = prepare_mountpoint(&root, "fusermount3").expect("made");
        assert_eq!(made, vec![root.clone(), dir.path().join("a")]);
        std::fs::write(root.join("f"), "x").expect("write");
        let err = prepare_mountpoint(&root, "fusermount3").expect_err("not empty");
        assert!(err.to_string().contains("not empty"), "{err}");
    }

    #[test]
    fn a_mountpoint_that_cannot_be_made_says_how() {
        let err = prepare_mountpoint(Path::new("/proc/cctop-sandbox-test/x"), "fusermount3")
            .expect_err("cannot be made");
        let text = err.to_string();
        assert!(
            text.contains("sudo mkdir -p /proc/cctop-sandbox-test/x"),
            "{text}"
        );
        assert!(text.contains("sudo chown $USER"), "{text}");
    }

    #[test]
    fn the_probe_reads_what_the_host_said() {
        let p = parse_probe(
            "hostname=db1\nuname=Linux 6.8.0\nbash=/usr/bin/bash\nsetsid=/usr/bin/setsid\nroot=/home/f/x\n",
        );
        assert_eq!(p.hostname, "db1");
        assert_eq!(p.uname, "Linux 6.8.0");
        assert_eq!(p.root, "/home/f/x");
        // The probe runs in a real sh, ~ included.
        let out = Command::new("sh")
            .arg("-c")
            .arg(probe_command("~"))
            .env("HOME", "/")
            .output()
            .expect("sh");
        assert_eq!(parse_probe(&String::from_utf8_lossy(&out.stdout)).root, "/");
    }

    #[test]
    fn the_guard_is_installed_for_the_file_tools_alone() {
        let settings = guard_settings(Path::new("/usr/bin/cctop"), "/srv/my app");
        let v: serde_json::Value = serde_json::from_str(&settings).expect("json");
        let entry = &v["hooks"]["PreToolUse"][0];
        assert_eq!(entry["matcher"], FILE_TOOLS);
        assert_eq!(
            entry["hooks"][0]["command"],
            "/usr/bin/cctop --sandbox-guard '/srv/my app'"
        );
    }

    /// A call that runs a command "on the host" — here, through a stand-in
    /// ssh that runs it locally the way sshd would.
    fn call(dir: &Path, command: &str, cwd: &Path) -> (i32, Option<String>, String) {
        let ssh = fake_ssh(dir);
        let nonce = nonce();
        let mut cmd = Command::new(ssh);
        cmd.args(mux_args(Path::new("/nonexistent.sock")))
            .arg("box")
            .arg("--")
            .arg(remote_line(command, &nonce, &cwd.to_string_lossy()))
            .stdout(Stdio::null());
        let mut err = Vec::new();
        let (code, dir) = run_remote(cmd, &nonce, &mut err).expect("ran");
        (code, dir, String::from_utf8_lossy(&err).into_owned())
    }

    #[test]
    fn a_call_returns_its_code_its_stderr_and_where_it_ended() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let work = tmp.path().join("work");
        std::fs::create_dir_all(work.join("sub")).expect("mkdir");
        let work = work.canonicalize().expect("canonical");

        let (code, dir, err) = call(tmp.path(), "eval 'echo oops >&2; cd sub'", &work);
        assert_eq!(code, 0);
        assert_eq!(err, "oops\n", "only the command's own stderr");
        assert_eq!(
            dir.as_deref(),
            Some(work.join("sub").to_str().expect("utf8"))
        );

        // A failure keeps its code, and does not move the directory.
        let (code, dir, _) = call(tmp.path(), "eval 'cd sub; exit 7'", &work);
        assert_eq!(code, 7);
        assert_eq!(dir, None);

        // A directory the host does not have is said, not guessed at.
        let (code, _, err) = call(tmp.path(), "eval true", &tmp.path().join("gone"));
        assert_eq!(code, 1);
        assert!(err.contains("is not a directory"), "{err}");
    }

    /// Processes whose command line carries `token`.
    fn holding(token: &str) -> Vec<u32> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir("/proc").expect("proc").flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            if pid == std::process::id() {
                continue;
            }
            let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
            if String::from_utf8_lossy(&cmdline).contains(token) {
                found.push(pid);
            }
        }
        found
    }

    #[test]
    fn nothing_from_a_call_outlives_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let token = format!("cctop-sbx-leak-{}", std::process::id());
        for _ in 0..3 {
            let (code, _, _) = call(
                tmp.path(),
                &format!("eval ': {token}; echo hi'"),
                tmp.path(),
            );
            assert_eq!(code, 0);
        }
        assert_eq!(
            holding(&token),
            Vec::<u32>::new(),
            "a helper outlived its call"
        );
    }

    #[test]
    fn closing_the_channel_stops_the_command_on_the_host() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // A sleep under a name of its own, so the process that has to die can
        // be found by it — the command, and not only the shells around it.
        let sleeper = tmp
            .path()
            .join(format!("sleep-cctop-sbx-orphan-{}", std::process::id()));
        std::fs::copy("/bin/sleep", &sleeper).expect("copy sleep");
        let token = sleeper.to_string_lossy().into_owned();
        let ssh = fake_ssh(tmp.path());
        let mut child = Command::new(ssh)
            .args(mux_args(Path::new("/nonexistent.sock")))
            .arg("box")
            .arg("--")
            .arg(remote_line(
                &format!("eval '{token} 30'"),
                &nonce(),
                &tmp.path().to_string_lossy(),
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn");
        // The sleep itself, whose argv is its path and then `30`.
        let sleeping = || holding(&format!("{token}\0"));
        let started = Instant::now();
        while sleeping().is_empty() {
            assert!(started.elapsed() < Duration::from_secs(10), "never started");
            std::thread::sleep(Duration::from_millis(20));
        }
        // What dying looks like from the far side: the channel's stdin closes.
        drop(child.stdin.take());
        let started = Instant::now();
        loop {
            if child.try_wait().expect("wait").is_some() && holding(&token).is_empty() {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the command outlived its channel: {:?}",
                holding(&token)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
