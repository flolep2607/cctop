//! `cctop sandbox [--agent <agent>] <host>:<path> [agent args…]`: an agent
//! here, its work there.
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
//! - **The path, mounted here.** sshfs puts the host's `<path>` on this
//!   machine, so Read, Edit, Write, Glob and Grep — which run in this process
//!   tree and know nothing of ssh — see the host's files. Where it goes is
//!   [`remote_fs::mount_point`](crate::remote_fs::mount_point)'s choice, below.
//! - **A shell that is not here.** Every command the agent runs is handed to
//!   cctop instead of to a shell, which sends it to the host over the
//!   connection, in the same directory, and brings back its output and its
//!   exit code. How the agent is made to do that is the agent's own business,
//!   and [`reach`] is the table of which ones have a way.
//!
//! The file tools can only see what is mounted, so each agent also gets a
//! guard that refuses them anywhere else — without one, a read of
//! `/etc/hosts` would quietly read *this* machine's.
//!
//! ## One path when it can be, two names when it cannot
//!
//! The mount goes at the *same* absolute path as on the host when that path
//! can be made here: an empty directory, or a missing one this user can
//! create. Then a path means the same file to the file tools and to the
//! host's shell, and nothing translates anything.
//!
//! Often it cannot be. The host's home is `/home/alice.smith`, this
//! machine's user is `alice`, and `/home` is root's; or the path is here and
//! full of this machine's own files. Then the mount goes under
//! `~/.cache/cctop/remote/<host>/<path>` instead — stable, so Claude Code's
//! per-directory history and `--resume` find it again next launch — and the
//! directory has two names: `L`, where it is mounted here and where the agent
//! works, and `R`, the host's own name for it. [`PathMap`] translates at the
//! two places a name crosses from one machine to the other:
//!
//! - **Into a command** (`--sandbox-exec`, `--sandbox-shell`): the directory
//!   the command starts in, and every `L` in the command's text, become `R`,
//!   since the model writes the paths it has seen and it has seen `L`. Back
//!   out of the call, the directory the command ended in becomes `L` again
//!   before Claude Code reads it.
//! - **Into a file tool** (`--sandbox-guard`): the host's commands print `R`
//!   — `pwd`, `git rev-parse --show-toplevel`, a compiler's error — and the
//!   model then asks Read or Edit for it. The guard hands Claude Code the same
//!   call with `R` rewritten to `L`.
//!
//! Output is never rewritten: it is the host's, byte for byte. No environment
//! crosses to the host either — a call carries a command line and a
//! directory, nothing else — so there is none to translate.

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

/// Where the host's directory is mounted here, set only when that is not the
/// host's own path for it: the `L` of [`PathMap`], whose `R` is the path half
/// of [`ENV_SANDBOX`].
const ENV_MOUNT: &str = "CCTOP_SANDBOX_MOUNT";

/// `statfs`'s answer for any FUSE filesystem, `fuse` and `fuseblk` alike.
const FUSE_SUPER_MAGIC: i64 = 0x6573_5546;

use crate::ssh_master::{SOCKET_PATH_MAX, mux_args};

/// How an agent's commands are made to run on the host, or that they cannot.
///
/// The bar for being on this table is that *nothing* the agent runs can land
/// on this machine by accident. A launch that quietly ran an agent's commands
/// here while the person believed they were running on a server is the one
/// failure worse than refusing, so an agent with no sound way in is refused
/// for a remote location rather than started half-remote.
///
/// Each answer was read off the agent itself — its source, its shipped docs,
/// or strings in its binary — not assumed:
///
/// - **claude**: every Bash call goes through `$CLAUDE_CODE_SHELL_PREFIX`,
///   split at its last ` -`, so the prefix `cctop --sandbox-exec` receives the
///   whole command line as one argument. The file tools are kept to the mount
///   by a PreToolUse hook ([`guard`]) installed with `--settings`.
/// - **opencode**: runs its shell tool as `<shell> -c <command>`, where the
///   shell is the config's `shell` key (then `$SHELL`). The key takes one
///   executable and no arguments, so it points at a two-line script that
///   execs `cctop --sandbox-shell`, which also answers the interactive
///   terminal opencode opens with the same shell. The config arrives in
///   `OPENCODE_CONFIG_CONTENT`, the highest-precedence layer, with
///   `permission.external_directory = "deny"` — opencode's own rule for any
///   tool touching a path outside the directory it was started in — as the
///   guard. `--standalone`, because the shared background server would
///   resolve the shell once for every session and ignore this one's.
/// - **codex**: builds `[<shell>, -lc, <command>]` with the shell taken from
///   the passwd entry, not from `$SHELL` or any setting, so there is nothing
///   to point at cctop. Its own remote mode (an `exec-server` over ssh) needs
///   a codex binary on the host, which this feature promises not to need.
/// - **devin**: no shell setting at all; a PreToolUse hook can rewrite a
///   command, but a hook that fails with anything but exit 2 lets the
///   original run here, and its long-lived terminal sessions bypass it.
/// - **pi**: documents `shellPath` and a remote-execution extension point, but
///   is not installed anywhere this was checked, so it is not claimed.
/// - **gemini** (not in the launcher): runs a bare `bash` off `PATH`, so only
///   a fake `bash` would redirect it — and that would catch its hooks too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Claude Code's shell prefix, and the guard hook.
    ShellPrefix,
    /// opencode's `shell` setting, and its `external_directory` rule.
    ShellSetting,
    /// No sound way: launched only on this machine.
    Local,
}

/// Look `agent` (a command name, `claude`) up in the table above.
pub fn reach(agent: &str) -> Reach {
    match agent {
        "claude" => Reach::ShellPrefix,
        "opencode" => Reach::ShellSetting,
        _ => Reach::Local,
    }
}

/// What a launcher says about an agent with no way to the host.
pub fn local_only(agent: &str) -> String {
    format!("{agent} can't run commands on a remote host yet")
}

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

/// The path half of a `CCTOP_SANDBOX` value: the host's own name for the
/// directory.
fn path_of(sandbox: &str) -> Option<&str> {
    sandbox
        .strip_prefix(host_of(sandbox))?
        .strip_prefix(':')
        .filter(|p| !p.is_empty())
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
usage: cctop sandbox [--agent claude|opencode] <host>:<path> [agent args…]

Run an agent here (Claude Code unless --agent says otherwise) with its shell
commands running on <host>, in <path>, over one ssh connection, and <path>
mounted here with sshfs so the file tools see the host's files: at the same
path when that can be made here, else under ~/.cache/cctop/remote/<host>/.
Nothing is installed on the host.

  cctop sandbox devbox:/srv/api
  cctop sandbox devbox:~/src/api --model opus
  cctop sandbox --agent opencode devbox:~/src/api

Needs sshfs and fusermount3 here, and offers to install sshfs when it is
missing and the terminal can answer; bash and setsid on the host.";

/// Set by the TUI on the sandbox it starts in a tab.
///
/// A failure there — no sshfs and the install declined, a host that will not
/// answer — would otherwise print its reason and end the tab in the same
/// instant, which reads as a tab that flashed and vanished. With this set the
/// reason stays on screen until Enter.
pub const ENV_HOLD: &str = "CCTOP_SANDBOX_HOLD";

/// [`run`], holding a failure on screen when [`ENV_HOLD`] asks for it.
pub fn run_held(args: &[String]) -> anyhow::Result<i32> {
    let result = run(args);
    match result {
        Err(e) if std::env::var_os(ENV_HOLD).is_some() => {
            eprintln!("\n{e:#}\n\nPress Enter to close this tab.");
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            Ok(1)
        }
        other => other,
    }
}

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
    let (agent, args) = split_agent(args)?;
    let Some(first) = args.first() else {
        anyhow::bail!("expected <host>:<path>\n\n{USAGE}");
    };
    let Some(spec) = Spec::parse(first) else {
        anyhow::bail!("expected <host>:<path>, got `{first}`\n\n{USAGE}");
    };
    if reach(&agent) == Reach::Local {
        anyhow::bail!("{}", local_only(&agent));
    }
    // Before anything else: an install that is declined or fails should not
    // leave an ssh connection behind it, and this is the step most likely to
    // stop a first run.
    crate::sshfs::ensure()?;
    let fusermount = require_tools()?;
    // Absolute, because it is put in an environment variable that a shell in
    // some other directory will run.
    let exe = std::env::current_exe()?;

    let mut sandbox = Sandbox {
        host: spec.host.clone(),
        socket: socket_path(&spec)?,
        fusermount,
        master: None,
        borrowed: false,
        sshfs: None,
        root: None,
        created: Vec::new(),
        shim: None,
        log: crate::config::runtime_base().join("cctop"),
    };
    // The launcher's connection, when it made one for this host: already
    // authenticated, so this launch asks nothing.
    match crate::ssh_master::socket_for(&spec.host)
        .filter(|shared| crate::ssh_master::is_up(shared, &spec.host))
    {
        Some(shared) => {
            crate::ssh_master::take_lease(&shared, std::process::id());
            sandbox.socket = shared;
            sandbox.borrowed = true;
        }
        None => {
            eprintln!("cctop sandbox: connecting to {}…", spec.host);
            sandbox.connect()?;
        }
    }
    let probe = sandbox.probe(&spec.path)?;
    let root = PathBuf::from(&probe.root);
    // The same rules the launcher marks its suggestions with, so a directory
    // that cannot be used is refused here in the words the field used, before
    // anything is created or mounted.
    if let Err(why) = crate::remote_fs::verdict(&probe.facts()) {
        anyhow::bail!("{}:{} can't be used: {why}", spec.host, probe.root);
    }
    let point = crate::remote_fs::mount_point(
        &spec.host,
        &root,
        &crate::remote_fs::mounts_here(),
        &crate::remote_fs::here,
        &crate::config::CACHE_DIR.join("remote"),
    )
    .map_err(|why| anyhow::anyhow!("{}:{} can't be mounted: {why}", spec.host, probe.root))?;
    sandbox.created = prepare_mountpoint(&point, sandbox.fusermount)?;
    match point == root {
        true => eprintln!(
            "cctop sandbox: mounting {}:{} at the same path here…",
            spec.host, probe.root
        ),
        false => eprintln!(
            "cctop sandbox: mounting {}:{} at {}…",
            spec.host,
            probe.root,
            point.display()
        ),
    }
    sandbox.mount(&probe.root, &point)?;
    let map = PathMap::new(&point, &root);

    let target = format!("{}:{}", spec.host, probe.root);
    let mut env = vec![
        (ENV_SANDBOX.to_string(), target.clone()),
        (
            ENV_SOCKET.to_string(),
            sandbox.socket.to_string_lossy().into_owned(),
        ),
    ];
    if map.is_some() {
        env.push((ENV_MOUNT.to_string(), point.to_string_lossy().into_owned()));
    }
    let mut argv = vec![agent.clone()];
    match reach(&agent) {
        Reach::ShellPrefix => {
            env.extend([
                // Claude Code wants a bash-family shell for its own command
                // strings; the host's shell is reached through the prefix, not
                // through this.
                ("SHELL".to_string(), "/bin/bash".to_string()),
                ("CLAUDE_CODE_SHELL".to_string(), "/bin/bash".to_string()),
                // Claude Code splits the prefix at its last " -", quoting the
                // part before as one executable and passing the rest as words.
                // A prefix with no " -" in it is taken whole as a program name
                // — which is why the helper is a flag rather than a subcommand.
                (
                    "CLAUDE_CODE_SHELL_PREFIX".to_string(),
                    format!("{} --sandbox-exec", exe.display()),
                ),
            ]);
            argv.extend([
                // Scoped to this launch: the user's settings files are not
                // touched, and the guard does not outlive the sandbox it guards.
                "--settings".to_string(),
                guard_settings(&exe, &point, map.as_ref()),
                "--append-system-prompt".to_string(),
                system_prompt(&spec.host, &probe, map.as_ref()),
            ]);
        }
        Reach::ShellSetting => {
            let shim = write_shell_shim(&exe)?;
            let content = opencode_config(
                std::env::var("OPENCODE_CONFIG_CONTENT").ok().as_deref(),
                &shim,
            );
            sandbox.shim = shim.parent().map(Path::to_path_buf);
            env.extend([
                ("SHELL".to_string(), shim.to_string_lossy().into_owned()),
                ("OPENCODE_CONFIG_CONTENT".to_string(), content),
            ]);
            argv.push("--standalone".to_string());
        }
        // Refused above.
        Reach::Local => unreachable!("checked before connecting"),
    }
    argv.extend(args[1..].iter().cloned());

    let stop = Stop::install();
    let code = crate::shim::run_in(&argv, Some(&point), &env, |pid| stop.watch(pid))?;
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
        (false, _) => anyhow::bail!("cctop sandbox needs sshfs on this machine"),
        // sshfs is here and fuse3 is not: a package that does not depend on
        // it, or a fusermount3 removed since.
        (true, None) => anyhow::bail!(
            "cctop sandbox needs fusermount3 on this machine.\n  \
             Debian/Ubuntu: sudo apt-get install fuse3"
        ),
    }
}

/// `--agent <name>` (or `--agent=<name>`) off the front of the arguments,
/// defaulting to claude, which is what `cctop sandbox` meant before it took
/// any other.
fn split_agent(args: &[String]) -> anyhow::Result<(String, &[String])> {
    match args.first().map(String::as_str) {
        Some("--agent") => match args.get(1) {
            Some(agent) => Ok((agent.clone(), &args[2..])),
            None => anyhow::bail!("--agent needs a name\n\n{USAGE}"),
        },
        Some(flag) if flag.starts_with("--agent=") => {
            Ok((flag["--agent=".len()..].to_string(), &args[1..]))
        }
        _ => Ok(("claude".to_string(), args)),
    }
}

/// The executable an agent is given as its shell: a script that hands every
/// invocation to `cctop --sandbox-shell`.
///
/// A script because the agents that take a shell setting take one path and no
/// arguments, and cctop's helper is a flag. Named `bash` because the commands
/// a model writes are bash, and an agent that looks at its shell's name to
/// decide how to quote should decide for bash. In a directory of its own,
/// private, removed with the sandbox.
fn write_shell_shim(exe: &Path) -> anyhow::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = crate::config::runtime_base()
        .join("cctop")
        .join(format!("sbx-shell-{}", std::process::id()));
    if !crate::ssh_master::private_dir(&dir) {
        anyhow::bail!("could not create {}", dir.display());
    }
    let path = dir.join("bash");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nexec {} --sandbox-shell \"$@\"\n",
            sh_quote(&exe.to_string_lossy())
        ),
    )?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
    Ok(path)
}

/// opencode's inline config for a sandbox: the shell, and the guard, laid over
/// whatever inline config was already set.
///
/// `external_directory` set to `deny` outright rather than per pattern: the
/// directory opencode starts in — the mount — is the one place its file tools
/// may go, and anywhere else is this machine. An inline config already in the
/// environment keeps its other keys; one that is not a JSON object is
/// replaced, since a sandbox without its guard is not one to start.
fn opencode_config(existing: Option<&str>, shell: &Path) -> String {
    let mut config = existing
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    config["shell"] = serde_json::Value::String(shell.to_string_lossy().into_owned());
    if !config["permission"].is_object() {
        config["permission"] = serde_json::json!({});
    }
    config["permission"]["external_directory"] = serde_json::Value::String("deny".into());
    config.to_string()
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
    for dir in crate::ssh_master::socket_dirs() {
        let path = dir.join(&name);
        if path.as_os_str().len() <= SOCKET_PATH_MAX && crate::ssh_master::private_dir(&dir) {
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
    /// The master is the launcher's, leased rather than owned: let go of at
    /// the end instead of being told to exit, since the launcher may still be
    /// completing paths over it.
    borrowed: bool,
    sshfs: Option<Child>,
    /// Where the mount is, once it is.
    root: Option<PathBuf>,
    /// Directories made to mount on, deepest first, removed again if empty.
    created: Vec<PathBuf>,
    /// The directory holding an agent's shell script, removed at the end.
    shim: Option<PathBuf>,
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
    readable: bool,
    writable: bool,
}

impl Probe {
    /// What the probe found, in the shape the usability rules read.
    fn facts(&self) -> crate::remote_fs::RemoteFacts {
        let there = !self.root.is_empty();
        crate::remote_fs::RemoteFacts {
            exists: there,
            is_dir: there,
            readable: self.readable,
            writable: self.writable,
            resolved: there.then(|| self.root.clone()),
        }
    }
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

    /// Mount the host's `remote` at `root` here and wait until it is up.
    fn mount(&mut self, remote: &str, root: &Path) -> anyhow::Result<()> {
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
            .arg(format!("{}:{remote}", self.host))
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
        if let Some(dir) = self.shim.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
        if self.borrowed {
            crate::ssh_master::release(&self.host, &self.socket, std::process::id());
            return;
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
if cd -- "$p" 2>/dev/null; then
  echo "root=$(pwd -P)"
  [ -r . ] && echo readable=1
  [ -w . ] && echo writable=1
fi"#;
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
            Some(("readable", "1")) => probe.readable = true,
            Some(("writable", "1")) => probe.writable = true,
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
                    "{} exists here and is not empty, and the mount would hide what is there",
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
                anyhow::bail!(
                    "could not create {} to mount the host's directory on ({e})",
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
    crate::remote_fs::parse_mountinfo(mountinfo)
        .into_iter()
        .rev()
        .find(|m| m.point == path)
        .map(|m| m.fstype)
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

/// The `--settings` that installs the guard for this launch alone: the mount
/// it keeps the file tools inside, and the host's name for it when that is
/// another, for the guard to translate.
fn guard_settings(exe: &Path, mount: &Path, map: Option<&PathMap>) -> String {
    let mut command = format!(
        "{} --sandbox-guard {}",
        sh_quote(&exe.to_string_lossy()),
        sh_quote(&mount.to_string_lossy())
    );
    if let Some(map) = map {
        command.push_str(" --remote ");
        command.push_str(&sh_quote(&map.remote));
    }
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
fn system_prompt(host: &str, probe: &Probe, map: Option<&PathMap>) -> String {
    let mount = match map {
        None => format!(
            "see the host's {root} through an sshfs mount at the same path",
            root = probe.root
        ),
        Some(map) => format!(
            "see the host's {root} through an sshfs mount at {local}, which is your working \
             directory. The two names are one directory: in a Bash command, {local} is \
             rewritten to {root} before it is sent, and a file tool given a path under {root} \
             is pointed at {local} — so either name works, though output from the host will \
             say {root}",
            root = map.remote,
            local = map.local,
        ),
    };
    format!(
        "You are working on the remote host `{host}` (hostname `{hostname}`, {uname}) through \
         cctop sandbox. Your Bash tool runs every command on that host, over ssh, in the \
         matching working directory — programs, services, processes, package managers and \
         environment are the host's. The file tools (Read, Edit, Write, Glob, Grep, \
         NotebookEdit) run on the local machine and {mount}; they are refused anywhere outside \
         it. To read or change any other path on the host, use Bash.",
        hostname = probe.hostname,
        uname = probe.uname,
    )
}

// ---------------------------------------------------------------------------
// The directory's two names
// ---------------------------------------------------------------------------

/// The mounted directory's name here (`local`, L) and on the host (`remote`,
/// R), when they differ. When they do not there is no map, and nothing is
/// translated anywhere.
///
/// Paths are mapped by components, so `/x/ab` is never taken for something
/// under `/x/a`. A command's *text* is mapped by [`swap`], which only takes `L`
/// where it stands as a whole path or a path's leading part. That is safe to
/// do blind — without parsing the shell — because of what `L` is: a long path
/// under `~/.cache/cctop/remote/<host>/`, which no command contains except
/// where it means this directory. Inside quotes, in an argument, after `=` or
/// in a `PATH`-style list, an `L` is still a reference to the mount, and `R`
/// is what the host calls it.
///
/// ponytail: the host's name is inserted as it is. An `R` with a space or a
/// quote in it, where the model wrote `L` bare or quoted the other way,
/// changes how the host's shell splits the command; and an `L` the model
/// spelled differently (`~/.cache/…`, a backslash-escaped space) is not seen.
/// Every home directory and project path anyone has pointed this at has
/// neither, and both would need shell parsing to do properly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMap {
    pub local: String,
    pub remote: String,
}

impl PathMap {
    /// The map from `local` to `remote`, or `None` when they are one path.
    pub fn new(local: &Path, remote: &Path) -> Option<PathMap> {
        (local != remote).then(|| PathMap {
            local: local.to_string_lossy().into_owned(),
            remote: remote.to_string_lossy().into_owned(),
        })
    }

    /// The map a sandbox set up for the agent, read back in a helper it ran.
    fn from_env() -> Option<PathMap> {
        let local = std::env::var(ENV_MOUNT)
            .ok()
            .filter(|l| l.starts_with('/'))?;
        let target = std::env::var(ENV_SANDBOX).ok()?;
        let remote = path_of(&target).filter(|r| r.starts_with('/'))?;
        PathMap::new(Path::new(&local), Path::new(remote))
    }

    /// A path here as the host names it; anything outside the mount as it is.
    pub fn to_remote(&self, path: &Path) -> PathBuf {
        rebase(path, Path::new(&self.local), Path::new(&self.remote))
    }

    /// A path on the host as it is named here; anything outside as it is.
    pub fn to_local(&self, path: &Path) -> PathBuf {
        rebase(path, Path::new(&self.remote), Path::new(&self.local))
    }

    /// A command line with every `L` in it made `R`.
    pub fn command_to_remote<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        swap(text, &self.local, &self.remote)
    }
}

/// `path` moved from under `from` to under `to`, if it is under `from`.
fn rebase(path: &Path, from: &Path, to: &Path) -> PathBuf {
    match path.strip_prefix(from) {
        Ok(rest) if rest.as_os_str().is_empty() => to.to_path_buf(),
        Ok(rest) => to.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

/// Bytes that continue a file name: `from` followed by one of these is the
/// start of some other name (`/x/ab` after `/x/a`), and one of these — or a
/// `/` — before it makes it the tail of a longer path.
fn names_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"._-+~@%".contains(&b) || b >= 0x80
}

/// Every `from` in `text` that is a path or a path's leading part, made `to`.
///
/// "A path's leading part" by the bytes around it: nothing that continues a
/// name, and no `/`, before it; the end, a `/`, or something that is not part
/// of a name after it. So `'L'`, `"L/src"`, `--dir=L`, `PATH=L/bin:$PATH` and
/// `cd L && make` are all taken; `L.bak`, `Lx/` and `/other/L` are not.
pub fn swap<'a>(text: &'a str, from: &str, to: &str) -> std::borrow::Cow<'a, str> {
    if from.is_empty() || from == "/" || !text.contains(from) {
        return std::borrow::Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut at = 0;
    while let Some(found) = text[at..].find(from) {
        let start = at + found;
        let end = start + from.len();
        let before_ok = start == 0 || {
            let b = bytes[start - 1];
            b != b'/' && !names_byte(b)
        };
        let after_ok = bytes.get(end).is_none_or(|&b| b == b'/' || !names_byte(b));
        if before_ok && after_ok {
            out.push_str(&text[copied..start]);
            // The host's root itself: `L/src` is `/src`, not `//src`.
            match (to, bytes.get(end)) {
                ("/", Some(b'/')) => {}
                _ => out.push_str(to),
            }
            copied = end;
            at = end;
        } else {
            // One byte on, not past it: `from` starts with `/`, so this is a
            // character boundary, and an occurrence overlapping this one is
            // still found.
            at = start + 1;
        }
    }
    if copied == 0 {
        return std::borrow::Cow::Borrowed(text);
    }
    out.push_str(&text[copied..]);
    std::borrow::Cow::Owned(out)
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
            let map = PathMap::from_env();
            match remote(
                head,
                Some(&cwd_file),
                &target,
                Path::new(&socket),
                map.as_ref(),
            ) {
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

/// What an agent asked of the shell it was given.
#[derive(Debug, PartialEq, Eq)]
pub enum ShellCall<'a> {
    /// `-c <command>`, with or without `-l`: one command, run on the host.
    Command(&'a str),
    /// No command and no script: a terminal for a person, opened on the host.
    Interactive,
    /// A script file, or flags this does not know. Refused: there is no file
    /// here to hand the host, and guessing would run something here.
    Unknown,
}

/// Read a shell's argv the way bash would, as far as an agent uses it.
pub fn shell_call(args: &[String]) -> ShellCall<'_> {
    let mut i = 0;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "--login" | "--norc" | "--noprofile" | "-l" | "-i" => {}
            "--" => break,
            flag if flag.starts_with('-') && !flag.starts_with("--") => {
                if flag.contains('c') {
                    return match args.get(i + 1) {
                        Some(command) => ShellCall::Command(command),
                        None => ShellCall::Unknown,
                    };
                }
                if !flag[1..].chars().all(|c| "lis".contains(c)) {
                    return ShellCall::Unknown;
                }
            }
            _ => return ShellCall::Unknown,
        }
        i += 1;
    }
    match i + 1 >= args.len() {
        true => ShellCall::Interactive,
        false => ShellCall::Unknown,
    }
}

/// `cctop --sandbox-shell <shell args>`: what an agent with a shell setting
/// runs instead of bash.
///
/// Unlike [`exec`], there is nothing here to tell apart: everything an agent
/// runs through its configured shell is the agent's work, so all of it goes
/// to the host. The one exception is the variables having gone missing, which
/// means this is not inside a sandbox at all — and then it refuses rather than
/// runs bash, because running here is exactly what the setting promised not to.
pub fn shell(args: &[String]) -> i32 {
    let target = std::env::var(ENV_SANDBOX).ok().filter(|t| !t.is_empty());
    let socket = std::env::var_os(ENV_SOCKET).filter(|s| !s.is_empty());
    let (Some(target), Some(socket)) = (target, socket) else {
        eprintln!("cctop sandbox: this shell only runs inside `cctop sandbox`");
        return 126;
    };
    let host = host_of(&target);
    // ponytail: opencode's file tools get no translation. It has no hook that
    // can rewrite a tool's input, so a host path it read in a command's output
    // and handed to Read stays the host's name — and its `external_directory`
    // rule refuses it as outside the directory it started in, which is at
    // least a refusal and not a read of this machine's file.
    let map = PathMap::from_env();
    match shell_call(args) {
        ShellCall::Command(command) => {
            match remote(command, None, &target, Path::new(&socket), map.as_ref()) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("cctop sandbox: could not run this on {host}: {e}");
                    255
                }
            }
        }
        ShellCall::Interactive => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            let cwd = match &map {
                Some(map) => map.to_remote(&cwd),
                None => cwd,
            };
            let line = format!(
                "cd -- {} 2>/dev/null; exec \"${{SHELL:-sh}}\" -l",
                sh_quote(&cwd.to_string_lossy())
            );
            let mut cmd = Command::new("ssh");
            // `-t` over `mux_args`' `-T`: the last one ssh reads wins, and this
            // one is a terminal.
            cmd.args(mux_args(Path::new(&socket)))
                .arg("-t")
                .arg(host)
                .arg("--")
                .arg(line);
            let e = cmd.exec();
            eprintln!("cctop sandbox: could not run ssh: {e}");
            255
        }
        ShellCall::Unknown => {
            eprintln!(
                "cctop sandbox: `{}` is not a shell call cctop can send to {host}, so it was not \
                 run — running it here would act on this machine instead.",
                args.join(" ")
            );
            126
        }
    }
}

/// Run one Bash call on the host and report back as bash would have.
///
/// With a `map`, the call is translated on its way out — its directory and
/// its text, `L` to `R` — and the directory it ended in on its way back, `R`
/// to `L`, since that is where Claude Code's next call starts here. Its output
/// is not: that is the host speaking.
fn remote(
    head: &str,
    cwd_file: Option<&str>,
    target: &str,
    socket: &Path,
    map: Option<&PathMap>,
) -> std::io::Result<i32> {
    let host = host_of(target);
    let (head, cwd) = outbound(head, &std::env::current_dir()?, map);
    let nonce = nonce();
    let mut cmd = Command::new("ssh");
    cmd.args(mux_args(socket))
        .arg(host)
        .arg("--")
        .arg(remote_line(&head, &nonce, &cwd.to_string_lossy()));
    let mut stderr = std::io::stderr();
    let (code, cwd_now) = run_remote(cmd, &nonce, &mut stderr)?;
    if let (Some(dir), Some(file)) = (cwd_now, cwd_file) {
        let _ = std::fs::write(file, format!("{}\n", inbound(dir, map)));
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

/// A call's command and directory as the host is to be given them.
fn outbound<'a>(
    head: &'a str,
    cwd: &Path,
    map: Option<&PathMap>,
) -> (std::borrow::Cow<'a, str>, PathBuf) {
    match map {
        Some(map) => (map.command_to_remote(head), map.to_remote(cwd)),
        None => (std::borrow::Cow::Borrowed(head), cwd.to_path_buf()),
    }
}

/// The directory a call ended in on the host, as Claude Code is to be told it.
fn inbound(dir: String, map: Option<&PathMap>) -> String {
    match map {
        Some(map) => map.to_local(Path::new(&dir)).to_string_lossy().into_owned(),
        None => dir,
    }
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

/// `cctop --sandbox-guard <mount>… [--remote <host path>]`, the PreToolUse
/// hook a sandbox installs.
///
/// Not `cctop hook`, which must never answer with a decision; this one exists
/// to answer with one. Like it, though, it always exits 0: a crash here would
/// be read as a block with whatever it printed as the reason.
///
/// With `--remote`, the mount is not at the host's path, and a file tool given
/// the host's name for a file is handed back its name here instead — see
/// [`answer`].
pub fn guard(args: &[String]) -> i32 {
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
    let (roots, remote) = match args.iter().position(|a| a == "--remote") {
        Some(at) => (&args[..at], args.get(at + 1)),
        None => (args, None),
    };
    let map = match (roots.first(), remote) {
        (Some(local), Some(remote)) => PathMap::new(Path::new(local), Path::new(remote)),
        _ => None,
    };
    let roots: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
    if let Some(answer) = answer(&event, &roots, &own_state_dirs(), &host, map.as_ref()) {
        println!("{answer}");
    }
    0
}

/// What the guard says about one tool call: a refusal, the call rewritten
/// onto the mount, or nothing — which lets it through untouched.
///
/// A rewrite is `updatedInput` with no `permissionDecision`. Claude Code
/// (2.1.x) replaces the call's input with it and then runs its own permission
/// check on the new input, as if the model had asked for that path — so an
/// Edit still asks when it would have asked. An `allow` beside it would have
/// skipped that check, which is not this hook's to skip. The input is sent
/// whole, since it replaces rather than merges.
fn answer(
    event: &serde_json::Value,
    roots: &[PathBuf],
    own: &[PathBuf],
    host: &str,
    map: Option<&PathMap>,
) -> Option<serde_json::Value> {
    let updated = map.and_then(|map| rewrite(event, map, own));
    let reason = match &updated {
        Some(input) => {
            let mut event = event.clone();
            event["tool_input"] = input.clone();
            refusal(&event, roots, own, host)
        }
        None => refusal(event, roots, own, host),
    };
    match (reason, updated) {
        (Some(reason), _) => Some(serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason,
            },
        })),
        (None, Some(input)) => Some(serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "updatedInput": input,
            },
        })),
        (None, None) => None,
    }
}

/// The tool call's input with the host's name for the directory made this
/// machine's, or `None` when no path in it is the host's.
///
/// Only the fields that are paths, only when absolute — a relative one is
/// already relative to the working directory, which is the mount — and never
/// one already on the mount or one of Claude Code's own files, both of which
/// could otherwise look like the host's when the host's directory is `/` or
/// shares a name with this machine's home.
fn rewrite(event: &serde_json::Value, map: &PathMap, own: &[PathBuf]) -> Option<serde_json::Value> {
    let tool = event.get("tool_name")?.as_str()?;
    let input = event.get("tool_input")?.as_object()?;
    let keys: &[&str] = match tool {
        "Read" | "Edit" | "Write" | "MultiEdit" => &["file_path"],
        "NotebookEdit" => &["notebook_path"],
        // A Glob pattern may be a path with wildcards in it; Grep's `glob` is
        // a file-name filter, never a directory.
        "Glob" => &["path", "pattern"],
        "Grep" => &["path"],
        _ => return None,
    };
    let local = Path::new(&map.local);
    let remote = Path::new(&map.remote);
    let mut out = input.clone();
    let mut changed = false;
    for key in keys {
        let Some(path) = input.get(*key).and_then(|v| v.as_str()) else {
            continue;
        };
        if !path.starts_with('/') {
            continue;
        }
        let seen = normalise(Path::new(path));
        if seen.starts_with(local)
            || own.iter().any(|dir| is_claude_state(&seen, dir))
            || !seen.starts_with(remote)
        {
            continue;
        }
        // By text, not through `normalise`: a Glob pattern keeps its
        // wildcards and a path its `..`, which the refusal then judges.
        let moved = match map.remote.as_str() {
            "/" => format!("{}{path}", map.local),
            prefix => match path.strip_prefix(prefix) {
                Some(rest) if rest.is_empty() || rest.starts_with('/') => {
                    format!("{}{rest}", map.local)
                }
                // `/R/./x`, `/R//x`: the same place, spelled so the text
                // does not start with R. The normalised path says where.
                _ => map.to_local(&seen).to_string_lossy().into_owned(),
            },
        };
        out.insert(key.to_string(), serde_json::Value::String(moved));
        changed = true;
    }
    changed.then_some(serde_json::Value::Object(out))
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
    let path = dir.join("ssh");
    write_executable(
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
    );
    path
}

/// Puts an executable a test is about to run at `path`, written by a child
/// process rather than by this one.
///
/// The test binary runs tests on many threads, and many of them fork. A file
/// this process opens for writing is open in every child forked while that
/// descriptor exists — `O_CLOEXEC` closes it at the child's exec, not at its
/// fork — and while any process holds a write descriptor on the inode, the
/// kernel refuses to execute it with `ETXTBSY`. Closing our own copy first, or
/// writing under a temporary name and renaming, does not help: the stray
/// descriptor is on the same inode. A `sh` of its own opening the file is
/// race-free, because nothing ever forks from that `sh`'s descriptor table.
#[cfg(test)]
pub(crate) fn write_executable(path: &Path, contents: &str) {
    let mut child = Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(path)
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn the writer");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(contents.as_bytes())
        .expect("write the executable");
    let status = child.wait().expect("wait for the writer");
    assert!(status.success(), "writing {} failed", path.display());
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

    /// Which agents may be sent to a host, and what the others are told.
    #[test]
    fn the_remote_capability_table() {
        assert_eq!(reach("claude"), Reach::ShellPrefix);
        assert_eq!(reach("opencode"), Reach::ShellSetting);
        for local in ["codex", "devin", "pi", "gemini", "htop"] {
            assert_eq!(reach(local), Reach::Local, "{local}");
        }
        assert_eq!(
            local_only("codex"),
            "codex can't run commands on a remote host yet"
        );
        // Every agent the launcher offers has an answer, and claude — what
        // `cctop sandbox` always meant — is one that goes.
        assert!(
            crate::alias::AGENTS
                .split_whitespace()
                .any(|a| reach(a) != Reach::Local)
        );
    }

    #[test]
    fn the_agent_is_named_before_the_spec_and_defaults_to_claude() {
        let args = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        let a = args("box:/srv --model opus");
        let (agent, rest) = split_agent(&a).expect("ok");
        assert_eq!((agent.as_str(), rest), ("claude", &a[..]));
        let a = args("--agent opencode box:~");
        let (agent, rest) = split_agent(&a).expect("ok");
        assert_eq!((agent.as_str(), rest), ("opencode", &a[2..]));
        let a = args("--agent=opencode box:~");
        assert_eq!(split_agent(&a).expect("ok").0, "opencode");
        assert!(split_agent(&args("--agent")).is_err());
    }

    /// The shell an agent is given reads its argv as bash would, as far as
    /// agents use it — and refuses what it cannot send rather than running it.
    #[test]
    fn a_shell_call_is_read_like_bash_reads_one() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            shell_call(&args(&["-c", "ls -la"])),
            ShellCall::Command("ls -la")
        );
        assert_eq!(
            shell_call(&args(&["-lc", "make"])),
            ShellCall::Command("make")
        );
        assert_eq!(
            shell_call(&args(&["-l", "-c", "make", "argv0"])),
            ShellCall::Command("make")
        );
        assert_eq!(shell_call(&args(&[])), ShellCall::Interactive);
        assert_eq!(shell_call(&args(&["-l"])), ShellCall::Interactive);
        assert_eq!(
            shell_call(&args(&["--login", "-i"])),
            ShellCall::Interactive
        );
        assert_eq!(shell_call(&args(&["script.sh"])), ShellCall::Unknown);
        assert_eq!(shell_call(&args(&["-c"])), ShellCall::Unknown);
        assert_eq!(shell_call(&args(&["-x", "y"])), ShellCall::Unknown);
    }

    /// The shell and the guard are laid over an inline config the person
    /// already had, which keeps its other keys.
    #[test]
    fn opencodes_config_carries_the_shell_and_the_guard() {
        let shell = Path::new("/run/user/1/cctop/sbx-shell-1/bash");
        let fresh: serde_json::Value =
            serde_json::from_str(&opencode_config(None, shell)).expect("json");
        assert_eq!(fresh["shell"], "/run/user/1/cctop/sbx-shell-1/bash");
        assert_eq!(fresh["permission"]["external_directory"], "deny");
        let theirs = r#"{"model":"x","permission":{"bash":"ask","external_directory":"allow"}}"#;
        let merged: serde_json::Value =
            serde_json::from_str(&opencode_config(Some(theirs), shell)).expect("json");
        assert_eq!(merged["model"], "x");
        assert_eq!(merged["permission"]["bash"], "ask");
        assert_eq!(merged["permission"]["external_directory"], "deny");
        // Not an object: replaced, never dropped.
        let junk: serde_json::Value =
            serde_json::from_str(&opencode_config(Some("[1]"), shell)).expect("json");
        assert_eq!(junk["permission"]["external_directory"], "deny");
    }

    /// The shim is a real executable that hands its argv to cctop untouched.
    #[test]
    fn the_shell_shim_passes_its_arguments_through() {
        let shim = write_shell_shim(Path::new("/bin/echo")).expect("shim");
        // The shim is written by this process, so a test forking while it was
        // open can hold it busy until that child execs (see
        // `write_executable`). The sandbox execs it long after; here the
        // retry waits out that child, which is milliseconds at most.
        let mut tries = 0;
        let out = loop {
            match Command::new(&shim).args(["-c", "it's one arg"]).output() {
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && tries < 100 => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(10));
                }
                run => break run.expect("run"),
            }
        };
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "--sandbox-shell -c it's one arg\n"
        );
        let _ = std::fs::remove_dir_all(shim.parent().expect("dir"));
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

    /// Only a race gets here — the mount point was chosen because it could be
    /// made — so the error says what failed and asks nothing of the person.
    #[test]
    fn a_mountpoint_that_cannot_be_made_says_why() {
        let err = prepare_mountpoint(Path::new("/proc/cctop-sandbox-test/x"), "fusermount3")
            .expect_err("cannot be made");
        let text = err.to_string();
        assert!(
            text.contains("could not create /proc/cctop-sandbox-test/x"),
            "{text}"
        );
        assert!(!text.contains("sudo"), "{text}");
    }

    #[test]
    fn the_probe_reads_what_the_host_said() {
        let p = parse_probe(
            "hostname=db1\nuname=Linux 6.8.0\nbash=/usr/bin/bash\nsetsid=/usr/bin/setsid\nroot=/home/f/x\nreadable=1\n",
        );
        assert!(p.readable && !p.writable);
        let facts = p.facts();
        assert!(facts.exists && facts.is_dir && facts.readable && !facts.writable);
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
        let exe = Path::new("/usr/bin/cctop");
        let settings = guard_settings(exe, Path::new("/srv/my app"), None);
        let v: serde_json::Value = serde_json::from_str(&settings).expect("json");
        let entry = &v["hooks"]["PreToolUse"][0];
        assert_eq!(entry["matcher"], FILE_TOOLS);
        assert_eq!(
            entry["hooks"][0]["command"],
            "/usr/bin/cctop --sandbox-guard '/srv/my app'"
        );
        // Mounted elsewhere: the guard is told the host's name too.
        let map = PathMap::new(Path::new(L), Path::new(R));
        let settings = guard_settings(exe, Path::new(L), map.as_ref());
        let v: serde_json::Value = serde_json::from_str(&settings).expect("json");
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            format!("/usr/bin/cctop --sandbox-guard {L} --remote {R}")
        );
    }

    /// The everyday pair: a host's home with no such home here.
    const L: &str = "/home/flo/.cache/cctop/remote/procdb/home/florian.leprat";
    const R: &str = "/home/florian.leprat";

    fn map() -> PathMap {
        PathMap::new(Path::new(L), Path::new(R)).expect("two names")
    }

    #[test]
    fn one_path_needs_no_map() {
        assert_eq!(PathMap::new(Path::new(R), Path::new(R)), None);
        assert!(PathMap::new(Path::new(L), Path::new(R)).is_some());
    }

    #[test]
    fn a_command_is_given_the_hosts_name() {
        let m = map();
        let out = |text: &str| m.command_to_remote(text).into_owned();
        assert_eq!(
            out(&format!("cat {L}/src/x.rs")),
            format!("cat {R}/src/x.rs")
        );
        assert_eq!(out(&format!("cd {L}")), format!("cd {R}"));
        // Inside quotes of either kind, after `=`, in a list.
        assert_eq!(
            out(&format!("eval 'ls \"{L}/a b\"' && x='{L}'")),
            format!("eval 'ls \"{R}/a b\"' && x='{R}'")
        );
        assert_eq!(
            out(&format!("PATH={L}/bin:{L}/node/bin:$PATH make --dir={L}")),
            format!("PATH={R}/bin:{R}/node/bin:$PATH make --dir={R}")
        );
        // Every occurrence, side by side.
        assert_eq!(
            out(&format!("diff {L}/a {L}/b")),
            format!("diff {R}/a {R}/b")
        );
        // Untouched: no L at all, and things that only start like it.
        assert_eq!(out("git status"), "git status");
        assert!(matches!(
            m.command_to_remote("git status"),
            std::borrow::Cow::Borrowed(_)
        ));
        for other in [
            format!("{L}x/y"),
            format!("{L}.bak"),
            format!("{L}-old"),
            format!("/mnt{L}"),
            format!("x{L}"),
        ] {
            assert_eq!(out(&other), other, "{other}");
        }
    }

    /// The boundary is the point: `/x/a` is not a prefix of `/x/ab`.
    #[test]
    fn a_path_boundary_is_a_whole_name() {
        assert_eq!(swap("/x/ab /x/a /x/a/b", "/x/a", "/R"), "/x/ab /R /R/b");
        assert_eq!(swap("cd /x/a;ls", "/x/a", "/R"), "cd /R;ls");
        assert_eq!(swap("(/x/a)", "/x/a", "/R"), "(/R)");
        // Overlapping spellings: the second one is still found.
        assert_eq!(swap("/a/a/a /a/a", "/a/a", "/R"), "/R/a /R");
        // The host's root: `L/src` is `/src`.
        assert_eq!(swap("ls /m/src /m", "/m", "/"), "ls /src /");
        // Nothing to swap from.
        assert_eq!(swap("ls /", "/", "/R"), "ls /");
    }

    #[test]
    fn a_path_goes_across_by_components() {
        let m = map();
        assert_eq!(m.to_remote(Path::new(L)), Path::new(R));
        assert_eq!(
            m.to_remote(&Path::new(L).join("src")),
            Path::new(R).join("src")
        );
        assert_eq!(
            m.to_local(&Path::new(R).join("a/b")),
            Path::new(L).join("a/b")
        );
        // Outside the directory: as it is, whichever way.
        assert_eq!(m.to_remote(Path::new("/tmp/x")), Path::new("/tmp/x"));
        assert_eq!(
            m.to_local(Path::new("/home/florian.leprat2")),
            Path::new("/home/florian.leprat2")
        );
        assert_eq!(m.to_local(Path::new("/etc")), Path::new("/etc"));
    }

    /// What a call carries out and brings back: `L` out, `R` back in.
    #[test]
    fn a_call_is_translated_out_and_its_directory_back() {
        let m = map();
        let command = format!("eval 'cat {L}/README'");
        let (head, cwd) = outbound(&command, &Path::new(L).join("src"), Some(&m));
        assert_eq!(head, format!("eval 'cat {R}/README'"));
        assert_eq!(cwd, Path::new(R).join("src"));
        assert_eq!(
            inbound(format!("{R}/src/sub"), Some(&m)),
            format!("{L}/src/sub")
        );
        assert_eq!(inbound("/var/log".into(), Some(&m)), "/var/log");
        // No map: as it was.
        let (head, cwd) = outbound("eval ls", Path::new(R), None);
        assert_eq!((head.as_ref(), cwd.as_path()), ("eval ls", Path::new(R)));
        assert_eq!(inbound(R.into(), None), R);
    }

    #[test]
    fn the_sandbox_value_splits_into_host_and_path() {
        assert_eq!(path_of("procdb:/home/f"), Some("/home/f"));
        assert_eq!(path_of("[::1]:/srv"), Some("/srv"));
        assert_eq!(path_of("procdb:/a:b"), Some("/a:b"));
        assert_eq!(path_of("procdb"), None);
        assert_eq!(path_of("procdb:"), None);
    }

    fn guard_says(tool: &str, input: serde_json::Value) -> Option<serde_json::Value> {
        let event = serde_json::json!({"tool_name": tool, "tool_input": input, "cwd": L});
        let own = [PathBuf::from("/home/flo/.claude"), PathBuf::from("/tmp")];
        answer(&event, &[PathBuf::from(L)], &own, "procdb", Some(&map()))
    }

    /// A file tool given the host's name for a file is handed the same call
    /// with this machine's — every other field kept, no decision taken.
    #[test]
    fn the_guard_points_a_host_path_at_the_mount() {
        use serde_json::json;
        let rewritten = |tool: &str, input: serde_json::Value, want: serde_json::Value| {
            assert_eq!(
                guard_says(tool, input),
                Some(json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PreToolUse",
                        "updatedInput": want,
                    },
                })),
                "{tool}"
            );
        };
        rewritten(
            "Read",
            json!({"file_path": format!("{R}/src/x.rs"), "offset": 10, "limit": 5}),
            json!({"file_path": format!("{L}/src/x.rs"), "offset": 10, "limit": 5}),
        );
        rewritten(
            "Edit",
            json!({"file_path": format!("{R}/a"), "old_string": "x", "new_string": "y"}),
            json!({"file_path": format!("{L}/a"), "old_string": "x", "new_string": "y"}),
        );
        rewritten(
            "Write",
            json!({"file_path": format!("{R}/new.txt"), "content": "hi"}),
            json!({"file_path": format!("{L}/new.txt"), "content": "hi"}),
        );
        rewritten(
            "MultiEdit",
            json!({"file_path": format!("{R}/a"), "edits": [{"old_string": "x", "new_string": "y"}]}),
            json!({"file_path": format!("{L}/a"), "edits": [{"old_string": "x", "new_string": "y"}]}),
        );
        rewritten(
            "NotebookEdit",
            json!({"notebook_path": format!("{R}/n.ipynb"), "new_source": "1"}),
            json!({"notebook_path": format!("{L}/n.ipynb"), "new_source": "1"}),
        );
        rewritten(
            "Glob",
            json!({"pattern": format!("{R}/src/**/*.rs")}),
            json!({"pattern": format!("{L}/src/**/*.rs")}),
        );
        rewritten(
            "Glob",
            json!({"pattern": "*.rs", "path": R}),
            json!({"pattern": "*.rs", "path": L}),
        );
        rewritten(
            "Grep",
            json!({"pattern": "fn main", "path": format!("{R}/src"), "glob": "*.rs"}),
            json!({"pattern": "fn main", "path": format!("{L}/src"), "glob": "*.rs"}),
        );
        // Spelled oddly, it is still the host's directory.
        rewritten(
            "Read",
            json!({"file_path": "/home/./florian.leprat/x"}),
            json!({"file_path": format!("{L}/x")}),
        );
    }

    /// The rest is as it was: let through untouched, or refused.
    #[test]
    fn the_guard_still_refuses_what_is_not_on_the_mount() {
        use serde_json::json;
        // Already this machine's name for it, relative, or Claude Code's own.
        assert_eq!(
            guard_says("Read", json!({"file_path": format!("{L}/x")})),
            None
        );
        assert_eq!(guard_says("Read", json!({"file_path": "src/x"})), None);
        assert_eq!(guard_says("Glob", json!({"pattern": "**/*.rs"})), None);
        assert_eq!(
            guard_says(
                "Write",
                json!({"file_path": "/home/flo/.claude/plans/p.md"})
            ),
            None
        );
        assert_eq!(
            guard_says("Bash", json!({"command": format!("cat {R}/x")})),
            None
        );
        // Neither name: refused, as before.
        let denied = |input| {
            guard_says("Read", input)
                .and_then(|a| {
                    a["hookSpecificOutput"]["permissionDecision"]
                        .as_str()
                        .map(str::to_string)
                })
                .unwrap_or_default()
        };
        assert_eq!(denied(json!({"file_path": "/etc/hosts"})), "deny");
        assert_eq!(
            denied(json!({"file_path": "/home/florian.leprat2/x"})),
            "deny"
        );
        // Out of the host's directory by `..`: rewritten, and then refused.
        assert_eq!(
            denied(json!({"file_path": format!("{R}/../other/x")})),
            "deny"
        );
    }

    /// A host whose whole filesystem is mounted: every absolute path is the
    /// host's — except the mount itself and Claude Code's own files.
    #[test]
    fn the_hosts_root_is_translated_without_swallowing_this_machine() {
        use serde_json::json;
        let local = "/home/flo/.cache/cctop/remote/procdb";
        let map = PathMap::new(Path::new(local), Path::new("/")).expect("map");
        let own = [PathBuf::from("/home/flo/.claude"), PathBuf::from("/tmp")];
        let say = |path: &str| {
            let event =
                json!({"tool_name": "Read", "tool_input": {"file_path": path}, "cwd": local});
            answer(&event, &[PathBuf::from(local)], &own, "procdb", Some(&map))
                .map(|a| a["hookSpecificOutput"]["updatedInput"]["file_path"].clone())
        };
        assert_eq!(say("/etc/hosts"), Some(json!(format!("{local}/etc/hosts"))));
        assert_eq!(say(&format!("{local}/etc/hosts")), None);
        assert_eq!(say("/home/flo/.claude/x"), None);
        assert_eq!(
            map.command_to_remote(&format!("cat {local}/etc/hosts")),
            "cat /etc/hosts"
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
        // A link rather than a copy: a copy is written by this process, and a
        // test forking meanwhile would keep it busy (see `write_executable`).
        // argv[0] is the link's path either way.
        std::os::unix::fs::symlink("/bin/sleep", &sleeper).expect("link sleep");
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
        crate::test_wait::eventually_true("the command to start", || !sleeping().is_empty());
        // What dying looks like from the far side: the channel's stdin closes.
        drop(child.stdin.take());
        let gone = crate::test_wait::waits_for(|| {
            child.try_wait().expect("wait").is_some() && holding(&token).is_empty()
        });
        assert!(
            gone,
            "the command outlived its channel: {:?}",
            holding(&token)
        );
    }
}
