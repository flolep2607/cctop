//! Handing a tab's agent to tmux, so it outlives the cctop that started it.
//!
//! A pane normally runs its agent on a pty [`shim::host`](crate::shim::host)
//! owns, which means the agent's life is cctop's life: quit, and the agent goes
//! with it. That is the wrong bargain for a coding session. Nothing about
//! watching sessions should require staying in the monitor.
//!
//! So the pane runs `tmux new-session` instead of the agent, and the agent runs
//! inside the tmux server. What cctop hosts is then only the tmux *client* —
//! closing the pane detaches, quitting cctop detaches, and the agent notices
//! neither. Everything else is unchanged: it is still a process on a pty cctop
//! can draw and type into, so [`attach`](crate::attach) and the pane machinery
//! need to know nothing about any of this.
//!
//! Reattaching falls out of the same command. `new-session -A` attaches to the
//! session if it exists and creates it otherwise, so reopening a tab is the same
//! call as opening it, and the agent is found exactly where it was left —
//! scrollback and all.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Prefix on every tmux session cctop creates.
///
/// A namespace, so `cctop-*` is unambiguously ours and a user's own sessions are
/// never adopted, reattached to, or killed by anything here.
const PREFIX: &str = "cctop";

/// Which multiplexer binary cctop drives, `tmux` unless `CCTOP_MUX` says
/// otherwise.
///
/// rmux reimplements tmux's command surface, so everything in this module —
/// `new-session -A`, `set-option`, `list-panes -F`, `kill-session` — is spelled
/// the same for either one and the binary name is the whole difference. Naming
/// it here rather than at each call site is also what keeps that true: a second
/// hard-coded `"tmux"` is a call that silently keeps talking to the other
/// daemon.
///
/// Opt-in rather than "prefer rmux if installed", because the two keep separate
/// daemons and separate sessions. Switching backends does not migrate anything:
/// the agents running under the old one are still running, and cctop simply
/// stops being able to see them. That is a fine thing to ask for and a terrible
/// thing to do to someone on upgrade.
///
/// Read once. The env cannot change under a running process in any way worth
/// answering, and this is called on paths that spawn a terminal.
pub fn bin() -> &'static str {
    static MUX: OnceLock<&'static str> = OnceLock::new();
    MUX.get_or_init(|| pick(std::env::var("CCTOP_MUX").ok().as_deref()))
}

/// The name half of [`bin`], split out so both answers can be tested without an
/// env var — which `bin` caches for the life of the process and so could only be
/// exercised once.
fn pick(value: Option<&str>) -> &'static str {
    match value {
        Some("rmux") => "rmux",
        // Anything unrecognised is tmux rather than an error: this is read deep
        // inside a launch, where the only way to report a typo would be to fail
        // the launch over it.
        _ => "tmux",
    }
}

/// Whether the multiplexer can be used at all.
///
/// Only that the binary exists: a server does not have to be running, since
/// `new-session` starts one. This is checked per launch rather than cached —
/// installing tmux while cctop runs should not require restarting it, and the
/// cost is one `tmux -V` against a launch that spawns a terminal anyway.
pub fn available() -> bool {
    Command::new(bin())
        .arg("-V")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// A way to put tmux on this machine, when one can be found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    /// What to call the package manager in the offer, so the user is agreeing
    /// to something they recognise rather than to "install tmux" in the
    /// abstract.
    pub manager: &'static str,
    /// The command to run, ready for a pane.
    pub argv: Vec<String>,
}

impl Install {
    /// The command as it would be typed, for the modal to show.
    ///
    /// The argv is `sh -c <script>`, so the interesting part is the last word
    /// and printing the whole vector would only bury it.
    pub fn shown(&self) -> &str {
        self.argv.last().map(String::as_str).unwrap_or_default()
    }
}

/// The package managers worth trying, best first, as the commands each one
/// installs tmux by and whether those commands have to run as root.
///
/// Homebrew leads because a machine with both it and a system manager is a mac
/// or a linuxbrew setup where brew is the one the user administers, and it is
/// the only entry that needs no password. Everything else is ordered by how
/// unambiguously its presence identifies the distribution.
///
/// The commands are a list and not one string because apt needs `update` first
/// — a container image ships with an empty package list, and `install` alone
/// fails on it with a message about a package that plainly exists — and each
/// command in the list has to be elevated separately. A single `sudo a && b`
/// would run `b` as the user, which is the half-privileged install that fails
/// at the step that matters.
const MANAGERS: &[(&str, &str, &[&str], bool)] = &[
    ("Homebrew", "brew", &["brew install tmux"], false),
    (
        "apt",
        "apt-get",
        &["apt-get update", "apt-get install -y tmux"],
        true,
    ),
    ("dnf", "dnf", &["dnf install -y tmux"], true),
    ("yum", "yum", &["yum install -y tmux"], true),
    ("pacman", "pacman", &["pacman -S --noconfirm tmux"], true),
    (
        "zypper",
        "zypper",
        &["zypper --non-interactive install tmux"],
        true,
    ),
    ("apk", "apk", &["apk add tmux"], true),
];

/// How cctop would install tmux here, if it can work out how.
///
/// `None` is the honest answer for a machine with no recognised package manager
/// — and for one where the install needs root that cctop has no way to reach.
/// Offering an install that cannot run is worse than the silent fallback it
/// would replace, since the user has then been told tmux is one keypress away
/// and watched the keypress fail.
///
/// Nothing here runs anything: it only decides what *would* be run, so the
/// decision can be shown to the user before any of it happens.
pub fn installer() -> Option<Install> {
    // Every entry below installs tmux by name, so there is nothing honest to
    // offer someone who asked for a different multiplexer. rmux ships through
    // cargo, brew, winget, scoop, apt and dnf under its own instructions; cctop
    // pointing a package manager at the wrong package is worse than saying
    // nothing.
    if bin() != "tmux" {
        return None;
    }
    let root = is_root();
    let (manager, _, commands, needs_root) = MANAGERS
        .iter()
        .find(|(_, bin, _, _)| crate::shim::is_command(bin))?;

    let elevate = match (needs_root, root) {
        // Already root, or a manager that never wanted to be.
        (false, _) | (true, true) => "",
        // sudo prompts for a password, which is fine — the install runs on a
        // pty in a pane, so the prompt is somewhere the user can answer it.
        (true, false) if crate::shim::is_command("sudo") => "sudo ",
        // Root is needed, and there is no way to become it.
        (true, false) => return None,
    };
    // `&&` and not `;`: a failed `update` leaves a package list too stale to
    // install from, and running the install anyway only buries the real error
    // under a second one.
    let script = commands
        .iter()
        .map(|command| format!("{elevate}{command}"))
        .collect::<Vec<_>>()
        .join(" && ");
    Some(Install {
        manager,
        // Kept as a script rather than an argv because apt needs two commands
        // and `&&` is the only thing that sequences them correctly.
        argv: vec!["sh".to_string(), "-c".to_string(), script],
    })
}

/// Whether cctop is already running as root.
#[cfg(unix)]
fn is_root() -> bool {
    // Safe: geteuid reads a process property and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn is_root() -> bool {
    false
}

/// The command that puts `argv` in the tmux session called `name`, attaching to
/// it instead if it is already there.
///
/// `-c` sets the working directory for a session being created and is ignored
/// for one being attached to, which is the behaviour wanted in both cases: a new
/// agent starts in its project, and an existing one is not moved.
pub fn attach_or_create(argv: &[String], name: &str, cwd: Option<&Path>) -> Vec<String> {
    let mut out = vec![
        bin().to_string(),
        "new-session".to_string(),
        "-A".to_string(),
        "-s".to_string(),
        name.to_string(),
    ];
    if let Some(dir) = cwd.filter(|d| d.is_dir()) {
        out.push("-c".into());
        out.push(dir.to_string_lossy().into_owned());
    }
    // `--` so an agent's own flags are never read as tmux's.
    out.push("--".into());
    out.extend(argv.iter().cloned());
    out
}

/// How many lines of an agent's output a cctop-owned tmux session keeps.
///
/// tmux's own default is 2000, which is a few minutes of a working agent — a
/// pane you can scroll but not scroll *back* to anything. These lines cost
/// nothing until they exist and are gone with the session.
const HISTORY_LINES: &str = "50000";

/// Create the session before the client that attaches to it, so the agent's
/// pane is made with cctop's options already set.
///
/// This exists for `history-limit` alone. A pane's scrollback is allocated when
/// the pane is *made* and never resized, so unlike `mouse` or `status` it cannot
/// be set on a session already running the agent — [`quiet`] and [`mouse`] fix a
/// session after the fact, and this is the one thing that has to happen before
/// it. Hence the detour: the session is created holding a placeholder, the
/// options are set on it, and the agent goes into a second window made after
/// they were, which is what makes its pane new enough to have read them. Then
/// the placeholder window goes, leaving the one-window session everything else
/// here expects. `respawn-pane` looks like the shorter way and is not — it
/// replaces the process and keeps the pane, scrollback and all.
///
/// Best effort in the strongest sense — every failure leaves the session absent
/// or removed, and [`attach_or_create`] then creates it exactly as it did
/// before, with tmux's own defaults. Nothing here can cost a launch, and a
/// session that already exists is left alone: it is someone's running agent, and
/// this would take its window out from under it.
pub fn prepare(argv: &[String], name: &str, cwd: Option<&Path>) {
    if exists(name) {
        return;
    }
    let dir = cwd.filter(|d| d.is_dir()).map(|d| d.to_string_lossy());
    let mut create = vec!["new-session", "-d", "-P", "-F", "#{window_id}", "-s", name];
    if let Some(dir) = &dir {
        create.extend(["-c", dir]);
    }
    // A window running nothing is a session that ends before it can be
    // configured, so the placeholder has to outlive the two commands after it.
    // The day is only how long an unreachable failure would sit around.
    create.extend(["--", "sleep", "86400"]);
    let Ok(out) = Command::new(bin()).args(&create).output() else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let placeholder = String::from_utf8_lossy(&out.stdout).trim().to_string();

    // One invocation for the three: `;` is tmux's own separator between
    // commands, which is why these need no shell to be read as three.
    let _ = Command::new(bin())
        .args(["set-option", "-t", name, "history-limit", HISTORY_LINES])
        .args([";", "set-option", "-t", name, "mouse", "on"])
        .args([";", "set-option", "-t", name, "status", "off"])
        // Several cctops may hold a client on this session at once — that is
        // what sharing tabs means. Left at tmux's default the window is sized to
        // the *smallest* of them, so one cctop in a narrow terminal cramps the
        // agent for everybody. `latest` sizes it to whichever client is being
        // used, which is the one whose window is worth fitting.
        .args([";", "set-option", "-t", name, "window-size", "latest"])
        .output();

    // Current, not background: this is the window the client is about to attach
    // to, and the placeholder is on its way out.
    let mut window = vec!["new-window", "-t", name];
    if let Some(dir) = &dir {
        window.extend(["-c", dir]);
    }
    window.push("--");
    window.extend(argv.iter().map(String::as_str));
    let made = Command::new(bin())
        .args(&window)
        .output()
        .is_ok_and(|out| out.status.success());
    if !made {
        // The session exists but holds a placeholder where the agent should be,
        // and `new-session -A` would happily attach to that. Removing it puts
        // the launch back on the path it would have taken had none of this run.
        let _ = kill(name);
        return;
    }
    // Last, so the session is never briefly windowless — killing its only window
    // is killing the session, and with it the agent just put in the other one.
    let _ = Command::new(bin())
        .args(["kill-window", "-t", &placeholder])
        .output();
}

/// The command that attaches to an existing tmux session and nothing else.
///
/// Distinct from [`attach_or_create`] so that picking an agent from the
/// launcher which has since ended reports that, rather than silently creating
/// an empty session wearing its name.
pub fn attach(name: &str) -> Vec<String> {
    vec![
        bin().to_string(),
        "attach-session".to_string(),
        "-t".to_string(),
        format!("={name}"),
    ]
}

/// One browser share of a multiplexer session.
pub struct Share {
    /// The link that can type into the agent. A shell credential, and named
    /// that way so nothing here hands it out by accident.
    pub operator: String,
    /// The pairing code the browser asks for after the link, absent under
    /// `--no-pin`. Useless without the link and useless on its own, which is why
    /// it is safe to put on the status line while the link goes to the clipboard.
    pub pin: Option<String>,
}

/// Open `name`'s terminal to a browser, if the multiplexer can do that at all.
///
/// tmux cannot, so this fails under it rather than guessing: browser sharing is
/// rmux's, and the nearest tmux equivalent — tmate, ttyd, a reverse proxy — is a
/// different program with a different trust model that cctop has no business
/// substituting silently.
///
/// Run and parsed rather than handed to a pane, because `web-share` returns as
/// soon as the daemon has the share: the links outlive the command, so a pane
/// would draw them and exit. The cost is rmux's card UI, which only renders to a
/// terminal — the QR code in it is worth having, and someone who wants it can
/// run the same command in one.
pub fn web_share(name: &str) -> Result<Share, String> {
    web_share_with(bin(), name)
}

/// [`web_share`] against a named multiplexer, so a test can reach the rmux path
/// without the process-wide cache in [`bin`].
fn web_share_with(bin: &str, name: &str) -> Result<Share, String> {
    let argv = web_share_argv(bin, name)
        .ok_or_else(|| format!("{bin} cannot share a terminal to a browser"))?;
    let out = Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .map_err(|error| format!("could not run {bin}: {error}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        // rmux's own message names the reason — no such session, web support
        // compiled out — and any summary here would only lose it.
        return Err(stderr.lines().last().unwrap_or("the share failed").into());
    }
    parse_share(&stdout, &stderr).ok_or_else(|| "the share reported no link".to_string())
}

/// The argv for [`web_share`], split out so both answers are reachable from a
/// test without the process-wide cache in [`bin`].
fn web_share_argv(bin: &str, name: &str) -> Option<Vec<String>> {
    (bin == "rmux").then(|| {
        vec![
            bin.to_string(),
            "web-share".to_string(),
            "-t".to_string(),
            name.to_string(),
        ]
    })
}

/// Pick the operator link and its pairing code out of what `web-share` printed.
///
/// The link is read from stderr, which is where rmux deliberately puts it: the
/// spectator link goes to stdout, and reading the wrong stream would hand out
/// input to a live shell believing it read-only. The two are the same shape, so
/// the stream is the only thing telling them apart — it is not a detail to
/// tidy into "first URL anywhere".
///
/// Within a stream the link is found by shape rather than by position, so a
/// reworded label does not silently stop returning one.
///
/// ponytail: only the operator link is kept. The spectator link is parsed away
/// because nothing asks for one yet; a read-only share wants its own key rather
/// than a second thing on the same one.
fn parse_share(stdout: &str, stderr: &str) -> Option<Share> {
    Some(Share {
        operator: stderr
            .split_whitespace()
            .find(|word| word.starts_with("https://"))
            .map(str::to_string)?,
        pin: stdout
            .lines()
            .find_map(|line| line.strip_prefix("operator pin "))
            .map(|pin| pin.trim().to_string()),
    })
}

/// A tmux session name for a session being resumed.
///
/// Derived from the session's identity rather than allocated, which is what
/// makes resuming idempotent: pressing `R` twice finds the same tmux session and
/// reattaches to it, instead of starting a second agent on one transcript.
///
/// The whole id goes in, never a prefix of it. Names built from a prefix were
/// not unique: a Codex id is `rollout-<timestamp>-<uuid>`, so the first 24
/// characters end mid-timestamp at the minute — two Codex sessions started in
/// the same minute mapped to one tmux name, and resuming the second reattached
/// to the first while cctop reported it had opened the one asked for. Nothing
/// here can know where a provider keeps the distinguishing part of its id, so
/// the only safe answer is to keep all of it. tmux imposes no length limit worth
/// respecting — a 76-character name works — and the launcher already shortens
/// what it shows.
pub fn name_for_session(provider: &str, session_id: &str) -> String {
    // tmux reads `.` and `:` as address syntax, so neither can survive in a name.
    let id: String = session_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    sanitize(&format!("{PREFIX}-{provider}-{id}"))
}

/// A free tmux session name for a freshly launched agent.
///
/// Unlike a resume there is nothing to be idempotent about — two `claude` tabs
/// are two agents — so this counts up until it finds a name nobody is using.
pub fn free_name(label: &str) -> String {
    let base = sanitize(&format!("{PREFIX}-{label}"));
    if !exists(&base) {
        return base;
    }
    // Bounded: past a hundred live tmux sessions of one name, reusing the last
    // is a better failure than looping. `new-session -A` then attaches to it,
    // which is odd but harmless — and nobody has a hundred of these.
    (2..100)
        .map(|n| format!("{base}-{n}"))
        .find(|name| !exists(name))
        .unwrap_or(base)
}

/// Reduce a label to what tmux accepts in a session name.
fn sanitize(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| match c {
            c if c.is_ascii_alphanumeric() || c == '-' || c == '_' => c,
            _ => '-',
        })
        .collect();
    // Collapse the runs the mapping above creates, so `claude --resume` does not
    // become `claude---resume`.
    let mut out = String::with_capacity(cleaned.len());
    for c in cleaned.chars() {
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    out.trim_matches('-').to_string()
}

/// Whether a tmux session by this name is alive.
pub fn exists(name: &str) -> bool {
    Command::new(bin())
        .args(["has-session", "-t", &format!("={name}")])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// End a tmux session, taking the agent inside it with it.
///
/// The `=` prefix makes the target an exact name rather than a prefix match —
/// without it, killing `cctop-claude` would also kill `cctop-claude-2`.
pub fn kill(name: &str) -> Result<(), String> {
    let out = Command::new(bin())
        .args(["kill-session", "-t", &format!("={name}")])
        .output()
        .map_err(|e| format!("tmux: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}

/// Turn off the status bar in one of cctop's own tmux sessions.
///
/// Two reasons, and the second is the one that matters. A cctop pane already has
/// a border with the agent's name on it and a footer under it, so tmux's bar is a
/// row of duplicate chrome inside someone else's frame.
///
/// More importantly it carries a clock, which tmux repaints every
/// `status-interval` — 15 seconds by default and 1 second in plenty of configs.
/// A pane's fallback idleness test is "has the screen stopped changing", so a
/// ticking clock is indistinguishable from an agent still working: with a
/// one-second interval no tmux-backed pane could ever go quiet, and the fallback
/// silently reported every abandoned agent as busy forever.
///
/// Scoped to the one session, so a user's own sessions and their global settings
/// are untouched. Best effort: a bar that stays on is cosmetic plus a weaker
/// fallback, and never worth failing a launch over.
pub fn quiet(name: &str) {
    // No `=` here, unlike every other target in this module: `set-option` is one
    // of the commands that rejects the exact-match prefix outright ("no such
    // session: =cctop-x"). Safe regardless, because tmux resolves an exact name
    // before it tries any prefix, and this is only ever called with the name of a
    // session just confirmed to exist.
    let _ = Command::new(bin())
        .args(["set-option", "-t", name, "status", "off"])
        .output();
    // Let the agent's own notifications out. A harness that wants a desktop
    // notification wraps it in tmux's passthrough sequence, and tmux swallows
    // that unless told otherwise — so the OSC 9 an agent sends when it is
    // blocked never reached the pane's parser, where cctop now listens for it.
    // The bell always got through; this is the half that says what about.
    //
    // A pane option, not a session one, and set here rather than in [`prepare`]:
    // a pane inherits its options from the global set when it is made, so there
    // is nothing to configure until the agent's own pane exists. Best effort
    // like the rest — tmux before 3.3 has no such option and rejects it, which
    // costs a process and leaves the pane exactly as it was.
    let _ = Command::new("tmux")
        .args(["set-option", "-p", "-t", name, "allow-passthrough", "on"])
        .output();
}

/// Record on the session itself what this tab is called.
///
/// The session is the only thing every cctop can see, so it is the only place a
/// tab's name can live if the tabs are to look the same in all of them. Without
/// it a cctop that did not start the agent has nothing to go on but the session
/// name — `cctop-claude-32cca860` for a launch, a sanitised uuid for a resume —
/// and the same agent would be called two different things on two screens.
///
/// Best effort, like every other option set here: a tab named after its session
/// is worse than one named properly, and better than a launch that failed.
pub fn set_label(name: &str, label: &str) {
    let _ = Command::new("tmux")
        .args(["set-option", "-t", name, "@cctop_label", label])
        .output();
}

/// Record which account the agent in `name` was started under.
///
/// The same trick as [`set_label`], for the same reason: the profile reached the
/// agent as an environment variable, and nothing outside the process can read
/// one back. Without it written down here, the answer lives only in the `Pane`
/// that chose it — and a pane is traded for a `Shared` on every tab switch, so
/// the account a border reports would fall back to whichever one cctop itself
/// would have used. The default profile writes nothing: unset is what it means.
pub fn set_profile(name: &str, profile: &str) {
    let _ = Command::new("tmux")
        .args(["set-option", "-t", name, "@cctop_profile", profile])
        .output();
}

/// Let the wheel scroll one of cctop's own tmux sessions.
///
/// Without this a tmux-backed pane cannot be scrolled at all. tmux is on the
/// alternate screen, so the scrollback of whatever terminal cctop is running in
/// holds none of the agent's output, and the history that does hold it is
/// reachable only from copy-mode — which, with `mouse` off, only a prefix key
/// opens. Turning it on makes the wheel enter copy-mode and scroll, the way it
/// does in every terminal without tmux in the way.
///
/// Scoped to the one session and best effort, for the same reasons as
/// [`quiet`]: a user's own sessions keep their own setting, and a pane that
/// cannot be scrolled is not worth failing a launch over.
///
/// tmux keeps no history for a pane on the alternate screen, so this does
/// nothing for an agent that draws there — Claude writes to the normal buffer
/// and scrolls, but an agent that takes the alternate screen has nothing behind
/// it to scroll back to, under tmux or anywhere else.
pub fn mouse(name: &str) {
    // Same reason as `quiet` for the bare name: `set-option` rejects `=`.
    let _ = Command::new(bin())
        .args(["set-option", "-t", name, "mouse", "on"])
        .output();
}

/// One cctop-owned tmux session, and the agent living in it.
///
/// The pid is the point. Everything cctop knows about a live agent — what its
/// hooks reported, what its transcript says, whether it is asking a question —
/// is keyed by the agent's own process, and a tmux-backed pane hosts the tmux
/// *client* instead. Without the pid on this side of the wall, an agent handed
/// to tmux is an agent cctop can only look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    /// The tmux session name, which is also cctop's handle on it.
    pub name: String,
    /// The agent's own pid: the pane's child, not any client's.
    ///
    /// `None` only in the moment between the session existing and its command
    /// being spawned, which resolves itself by the next look.
    pub pid: Option<u32>,
    /// Where the agent is working, which is the only human-readable thing a
    /// session name does not already carry.
    pub cwd: Option<PathBuf>,
    /// Whether a client is already looking at this session.
    ///
    /// Not necessarily one of ours — a `tmux attach` in another terminal counts,
    /// and reattaching to it from here would leave the two clients arguing over
    /// one window's size.
    pub attached: bool,
    /// When the session's window last produced output, in unix seconds.
    ///
    /// This is what lets a tab nobody is watching still say its agent has gone
    /// quiet — the same reading a pane takes off its own screen, from the one
    /// place that has it when there is no pane.
    ///
    /// `window_activity` and not `session_activity`, which is not what its name
    /// suggests: tmux advances the session's clock on *client* activity, so a
    /// detached session's stands still however much its agent prints. Reading
    /// that one drew every unwatched agent as idle a few seconds after the last
    /// keystroke — and, with a tool call in flight, blinked its tab as if it
    /// were holding a permission prompt. The window's clock follows the output.
    pub activity: Option<u64>,
    /// What the cctop that started this agent called its tab, when it said.
    ///
    /// Written onto the session by [`set_label`] so that every other cctop names
    /// the tab the same thing. Nothing else can supply it: a resumed session is
    /// labelled after the conversation it is going back to, which the tmux name
    /// — a sanitised provider and uuid — cannot be read back out of.
    pub label: Option<String>,
    /// The account this agent was started under, when it was not the default
    /// one. Written onto the session by [`set_profile`], which says why.
    pub profile: Option<String>,
}

/// Every cctop-owned tmux session currently alive, newest first.
///
/// What the UI needs to say "there are agents still running out there" after a
/// restart, and to offer them back — with enough about each to say which is
/// which, rather than a list of names.
///
/// Newest first because that is the order they are wanted in: the agent you
/// walked away from most recently is the one you are most likely coming back
/// for.
pub fn running() -> Vec<Running> {
    // One call for all of it. Every field below resolves in a pane's context —
    // tmux looks up the session a pane belongs to — so asking per session would
    // be a subprocess each for the same answer.
    let Ok(out) = Command::new(bin())
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{session_name}\t#{pane_pid}\t#{pane_current_path}\t#{session_attached}\t#{session_created}\t#{window_activity}\t#{@cctop_label}\t#{@cctop_profile}",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        // No server running: no sessions, not an error.
        return Vec::new();
    }

    let prefix = format!("{PREFIX}-");
    let mut found: Vec<(u64, Running)> = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.split('\t');
        let Some(name) = parts
            .next()
            .map(str::trim)
            .filter(|n| n.starts_with(&prefix))
        else {
            continue;
        };
        // A session with splits or extra windows lists several panes. The agent
        // is the one cctop started, which is the first — later panes are the
        // user's own, opened inside the session after the fact.
        if found.iter().any(|(_, s)| s.name == name) {
            continue;
        }
        let pid = parts.next().and_then(|p| p.trim().parse::<u32>().ok());
        let cwd = parts
            .next()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(PathBuf::from);
        let attached = parts.next().map(str::trim).is_some_and(|a| a != "0");
        let created = parts
            .next()
            .and_then(|c| c.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let activity = parts.next().and_then(|a| a.trim().parse::<u64>().ok());
        // Unset reads back as the empty string, not as a missing field.
        let mut option = || {
            parts
                .next()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        let label = option();
        let profile = option();
        found.push((
            created,
            Running {
                name: name.to_string(),
                pid,
                cwd,
                attached,
                activity,
                label,
                profile,
            },
        ));
    }
    // Descending, so the agent left most recently is the one offered first.
    found.sort_by_key(|(created, _)| std::cmp::Reverse(*created));
    found.into_iter().map(|(_, s)| s).collect()
}

/// Just the names, for the callers that only need to know which exist.
pub fn sessions() -> Vec<String> {
    running().into_iter().map(|s| s.name).collect()
}

/// The pid of the agent inside the session called `name`.
///
/// Targeted rather than a scan of [`running`], because this is asked per pane
/// and the answer for one session should not cost a listing of them all.
pub fn agent_pid(name: &str) -> Option<u32> {
    let out = Command::new(bin())
        .args(["list-panes", "-t", &format!("={name}"), "-F", "#{pane_pid}"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // First pane again: see [`running`].
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|line| line.trim().parse().ok())
}

/// The cctop session holding the agent running as `pid`, if one is.
///
/// The reverse of [`agent_pid`], and the question asked by anything that starts
/// from a process and needs to know whether cctop can reach its terminal: an
/// agent handed to tmux is on no pty of cctop's, so every other route to it
/// fails, but the session name is a way in.
pub fn holding(pid: u32) -> Option<String> {
    running()
        .into_iter()
        .find(|agent| agent.pid == Some(pid))
        .map(|agent| agent.name)
}

/// Serialises the tests that drive a real tmux server.
///
/// The server is one shared, machine-wide thing, and its *lifetime* is the part
/// that races: killing the last session stops the server, and a `new-session`
/// that reaches the socket while it is going down fails outright. Two tests
/// creating and killing sessions at once therefore fail each other at random,
/// which is what happens without this — nothing about the code under test is
/// racy, the fixture is.
#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A test that panicked while holding the lock has poisoned it. The next one
    // still wants its turn rather than a second failure caused by the first.
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {

    #[test]
    fn an_unrecognised_multiplexer_is_tmux() {
        assert_eq!(pick(Some("rmux")), "rmux");
        assert_eq!(pick(Some("zellij")), "tmux");
        assert_eq!(pick(Some("")), "tmux");
        assert_eq!(pick(None), "tmux");
    }

    #[test]
    fn only_rmux_is_offered_a_browser_share() {
        assert_eq!(web_share_argv("tmux", "cctop-claude-abc"), None);
        assert_eq!(
            web_share_argv("rmux", "cctop-claude-abc").unwrap(),
            &["rmux", "web-share", "-t", "cctop-claude-abc"]
        );
    }

    #[test]
    fn the_operator_link_is_read_from_the_stream_rmux_puts_it_on() {
        // Verbatim from `rmux 0.10.0 web-share -t …`, which splits the two
        // links across the streams on purpose.
        let stderr =
            "rmux: operator URL (keep private):\nrmux:   https://share.rmux.io/#t=OPERATOR\n";
        let stdout = concat!(
            "spectator https://share.rmux.io/#t=SPECTATOR\n",
            "operator URL emitted on stderr\n",
            "share does not expire\n",
            "operator pin 634138\n",
            "spectator pin 988762\n",
        );
        let share = parse_share(stdout, stderr).expect("a link was printed");
        assert_eq!(share.operator, "https://share.rmux.io/#t=OPERATOR");
        assert_eq!(share.pin.as_deref(), Some("634138"));
    }

    /// The parse above is against captured output; this is against rmux. A
    /// reworded label or a link moved between the streams would pass every unit
    /// test here and hand out the wrong link in the field.
    #[test]
    fn a_real_rmux_session_comes_back_with_an_operator_link() {
        if !Command::new("rmux")
            .arg("-V")
            .output()
            .is_ok_and(|out| out.status.success())
        {
            eprintln!("skipping: rmux not installed");
            return;
        }
        let name = format!("cctop-share-{}", std::process::id());
        let made = Command::new("rmux")
            .args(["new-session", "-d", "-s", &name, "--", "sleep", "30"])
            .output()
            .is_ok_and(|out| out.status.success());
        assert!(made, "rmux would not start a session to share");

        let share = web_share_with("rmux", &name);
        // Killing the session ends its share; `web-share -X` would also end any
        // the user has open, which is not this test's to touch.
        let _ = Command::new("rmux")
            .args(["kill-session", "-t", &format!("={name}")])
            .output();

        let share = share.expect("rmux shared the session");
        assert!(
            share.operator.starts_with("https://"),
            "not a link: {}",
            share.operator
        );
    }

    #[test]
    fn a_share_with_no_operator_link_is_not_a_share() {
        // `--spectator-only` prints a link, but not one this can return: reading
        // stdout here would hand out input to a live agent.
        let stdout = "spectator https://share.rmux.io/#t=SPECTATOR\n";
        assert!(parse_share(stdout, "").is_none());
    }
    use super::*;

    /// Whatever is offered has to be runnable as offered. An install the user
    /// accepts and then watches fail on a missing `sudo`, or on a script that
    /// was never a command, is worse than never having offered.
    #[test]
    fn an_offered_install_is_a_command_that_could_run() {
        let Some(install) = installer() else {
            return;
        };
        assert_eq!(install.argv[0], "sh");
        assert_eq!(install.argv[1], "-c");
        assert_eq!(install.shown(), install.argv[2]);
        assert!(install.shown().contains("tmux"));
        if let Some(rest) = install.shown().strip_prefix("sudo ") {
            assert!(crate::shim::is_command("sudo"));
            // Only the leading command is elevated, so a second one after `&&`
            // needs its own sudo. apt is the entry where this matters.
            assert!(!rest.contains("&& apt-get install") || rest.contains("&& sudo"));
        }
    }

    /// The wrapped command has to attach-or-create by exact name, start in the
    /// project, and keep the agent's own flags away from tmux.
    #[test]
    fn the_wrapper_attaches_or_creates_and_passes_the_agent_through() {
        let argv: Vec<String> = ["claude", "--resume", "abc"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let dir = std::env::temp_dir();
        let out = attach_or_create(&argv, "cctop-claude-abc", Some(&dir));

        assert_eq!(
            &out[..5],
            &["tmux", "new-session", "-A", "-s", "cctop-claude-abc"]
        );
        // The agent's flags come after `--`, so tmux cannot claim them.
        let sep = out.iter().position(|a| a == "--").expect("separator");
        assert_eq!(&out[sep + 1..], &argv[..]);
        assert!(out.contains(&"-c".to_string()));

        // A directory that is not there is left off rather than failing the
        // spawn, matching what the pty path does with a stale cwd.
        let gone = attach_or_create(&argv, "n", Some(Path::new("/nonexistent/gone")));
        assert!(!gone.contains(&"-c".to_string()));
    }

    /// Resuming the same session twice must name the same tmux session, or `-A`
    /// has nothing to reattach to and the point is lost.
    #[test]
    fn a_session_always_maps_to_the_same_tmux_name() {
        let a = name_for_session("claude", "32cca860-b503-43f8-8b95-7b75880abb8b");
        let b = name_for_session("claude", "32cca860-b503-43f8-8b95-7b75880abb8b");
        assert_eq!(a, b);
        assert_ne!(a, name_for_session("claude", "other-id"));
        // Different providers can share a session id without colliding.
        assert_ne!(
            a,
            name_for_session("codex", "32cca860-b503-43f8-8b95-7b75880abb8b")
        );
    }

    /// Two different sessions must never map to one name, or `R` on the second
    /// silently reattaches to the first — the worst failure this module has,
    /// because cctop reports having opened the session that was asked for.
    ///
    /// The shape that broke it: a Codex id is `rollout-<timestamp>-<uuid>`, and
    /// the name was built from a 24-character prefix, which ends inside the
    /// timestamp at the minute. Everything that distinguishes two sessions
    /// started in the same minute was cut off.
    #[test]
    fn two_sessions_never_share_a_name() {
        let first = name_for_session("codex", "rollout-2026-08-06T16-47-27-019fd565-d014-7b71");
        let second = name_for_session("codex", "rollout-2026-08-06T16-47-59-019fd999-aaaa-8c62");
        assert_ne!(
            first, second,
            "two Codex sessions from the same minute share a tmux session"
        );

        // The same for ids that differ only at the very end, which is where a
        // uuid keeps most of what makes it one.
        assert_ne!(
            name_for_session("claude", "32cca860-b503-43f8-8b95-7b75880abb8a"),
            name_for_session("claude", "32cca860-b503-43f8-8b95-7b75880abb8b")
        );

        // And a provider name that is a prefix of another's must not let one
        // provider's session wear the other's name.
        assert_ne!(
            name_for_session("code", "x-abc"),
            name_for_session("codex", "abc")
        );
    }

    /// tmux reads `.` and `:` as address syntax, so a name carrying either would
    /// target something other than the session meant — including, for a `:`,
    /// a *window* of it.
    #[test]
    fn names_carry_nothing_tmux_would_read_as_an_address() {
        for name in [
            name_for_session("claude", "/home/x/proj:1.2"),
            free_name("claude --resume /a/b"),
            sanitize("--leading and trailing--"),
        ] {
            assert!(
                !name.contains(['.', ':', ' ', '/']),
                "unsafe tmux name: {name}"
            );
            assert!(!name.starts_with('-') && !name.ends_with('-'), "{name}");
            assert!(!name.contains("--"), "collapsed runs: {name}");
        }
    }

    /// Every name is inside cctop's namespace, which is what keeps the kill and
    /// list paths off a user's own tmux sessions.
    #[test]
    fn names_stay_inside_the_cctop_namespace() {
        assert!(name_for_session("claude", "abc").starts_with("cctop-"));
        assert!(free_name("zsh").starts_with("cctop-"));
    }

    /// Finding the agent behind a session is the whole point of handing one to
    /// tmux and still knowing what it is doing: the pid reported here is what
    /// every hook, transcript, and table row is keyed by. Nothing smaller than a
    /// real server tests it, since the answer comes from tmux itself.
    #[test]
    fn a_session_reports_the_agent_inside_it() {
        if !available() {
            eprintln!("skipping: tmux not installed");
            return;
        }
        let _turn = test_lock();
        // Unique per test process, so a run never adopts or kills a session
        // belonging to a real cctop on the same machine.
        let ours = format!("cctop-probe-{}", std::process::id());
        let theirs = format!("outside-probe-{}", std::process::id());
        let dir = std::env::temp_dir();

        for name in [&ours, &theirs] {
            assert!(
                Command::new(bin())
                    .args([
                        "new-session",
                        "-d",
                        "-s",
                        name,
                        "-c",
                        &dir.to_string_lossy(),
                        "--",
                        "sh",
                        "-c",
                        "sleep 30",
                    ])
                    .status()
                    .is_ok_and(|s| s.success()),
                "could not start {name}"
            );
        }

        let found = wait_for(|| running().into_iter().find(|s| s.name == ours));
        let pid = agent_pid(&ours);
        let listed = sessions();
        let outsider = running().into_iter().any(|s| s.name == theirs);
        // The reverse lookup, which is what lets `a` on a session row reach an
        // agent that cctop put in tmux rather than telling the user cctop never
        // started it.
        let back = pid.and_then(holding);
        // Quieting the session is best effort, but it has to actually land: the
        // status bar it removes is what made an idle pane look busy forever.
        quiet(&ours);
        let status = Command::new(bin())
            .args(["show-options", "-t", &ours, "status"])
            .output()
            .ok()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string());
        for name in [&ours, &theirs] {
            let _ = kill(name);
        }

        let found = found.expect("the session cctop just created was not listed");
        // The pid is the pane's own child — the agent — and not a client's.
        assert_eq!(found.pid, pid, "the two ways of asking disagreed");
        assert!(found.pid.is_some_and(|p| p > 0), "no agent pid: {found:?}");
        assert_eq!(found.cwd.as_deref(), Some(dir.as_path()));
        // Nothing is looking at it: it was created detached.
        assert!(!found.attached);
        assert!(listed.contains(&ours));

        // Pid to session and back again agree, or `a` on a tmux-backed agent has
        // no way to find the session holding it.
        assert_eq!(back.as_deref(), Some(ours.as_str()));
        assert_eq!(status.as_deref(), Some("status off"), "status bar left on");

        // The namespace holds, which is what keeps every path here off a user's
        // own sessions.
        assert!(!outsider, "a session outside cctop's namespace was listed");
        assert!(!listed.contains(&theirs));

        // And once it is gone there is no agent to name.
        assert_eq!(agent_pid(&ours), None);
        assert!(!exists(&ours));
    }

    /// Two options on one session, read back off one listing.
    ///
    /// The profile is written beside the label because it has the same problem:
    /// it exists only in the process that chose it, and every other cctop — and
    /// this one after a tab switch — has nothing but the session. Worth a real
    /// server because the failure mode is positional: both arrive as fields of
    /// one `-F` line, an unset one comes back as the empty string rather than as
    /// a missing field, and reading them in the wrong order would put an account
    /// name in the tab bar.
    #[test]
    fn a_session_remembers_which_account_its_agent_runs_as() {
        if !available() {
            eprintln!("skipping: tmux not installed");
            return;
        }
        let _turn = test_lock();
        let named = format!("cctop-probe-named-{}", std::process::id());
        let plain = format!("cctop-probe-plain-{}", std::process::id());
        for name in [&named, &plain] {
            assert!(
                Command::new("tmux")
                    .args([
                        "new-session",
                        "-d",
                        "-s",
                        name,
                        "--",
                        "sh",
                        "-c",
                        "sleep 30"
                    ])
                    .status()
                    .is_ok_and(|s| s.success()),
                "could not start {name}"
            );
        }
        set_profile(&named, "work");
        // Deliberately no label on this one: the label field then reads back
        // empty, and the profile after it must still be the profile.
        let found = wait_for(|| running().into_iter().find(|s| s.name == named));
        let bare = running().into_iter().find(|s| s.name == plain);
        for name in [&named, &plain] {
            let _ = kill(name);
        }

        let found = found.expect("the session cctop just created was not listed");
        assert_eq!(found.profile.as_deref(), Some("work"));
        assert_eq!(found.label, None);
        // The default account writes nothing, and nothing is what it reads as —
        // not an empty string that would name a profile no one has.
        assert_eq!(bare.and_then(|s| s.profile), None);
    }

    /// A pane's scrollback is fixed when the pane is made, so the only proof
    /// [`prepare`] works is a real session reporting what its pane actually got
    /// — the option can read back as set while the pane still holds tmux's 2000.
    #[test]
    fn a_prepared_session_holds_the_agent_with_room_to_scroll_back() {
        if !available() {
            eprintln!("skipping: tmux not installed");
            return;
        }
        let _turn = test_lock();
        let name = format!("cctop-prep-{}", std::process::id());
        let dir = std::env::temp_dir();
        let argv = vec!["sh".to_string(), "-c".to_string(), "sleep 30".to_string()];

        prepare(&argv, &name, Some(&dir));
        let ask = |fmt: &str| {
            Command::new(bin())
                .args(["display-message", "-p", "-t", &name, fmt])
                .output()
                .ok()
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        let history = wait_for(|| ask("#{history_limit}"));
        let windows = ask("#{session_windows}");
        let mouse = ask("#{mouse}");
        let agent = agent_pid(&name);
        let _ = kill(&name);

        assert_eq!(
            history.as_deref(),
            Some(HISTORY_LINES),
            "the agent's pane kept tmux's default scrollback"
        );
        assert_eq!(mouse.as_deref(), Some("1"), "the wheel would do nothing");
        // The placeholder is gone: a second window would leave the agent sharing
        // a session with a `sleep`, and every lookup here assumes the one pane.
        assert_eq!(
            windows.as_deref(),
            Some("1"),
            "placeholder window left over"
        );
        assert!(agent.is_some_and(|p| p > 0), "no agent in the session");
    }

    /// An agent nobody is attached to still has to read as busy while it is
    /// printing. `session_activity` does not say so — tmux advances that on
    /// client activity, so a detached session's stands still — and reading it
    /// blinked every unwatched working tab as if it were asking a question.
    /// Only a real detached session producing output shows the difference.
    #[test]
    fn a_detached_session_that_is_printing_never_reads_as_quiet() {
        if !available() {
            eprintln!("skipping: tmux not installed");
            return;
        }
        let _turn = test_lock();
        let ours = format!("cctop-probe-busy-{}", std::process::id());
        assert!(
            Command::new("tmux")
                .args([
                    "new-session",
                    "-d",
                    "-s",
                    &ours,
                    "--",
                    "sh",
                    "-c",
                    "while true; do date; sleep 0.2; done",
                ])
                .status()
                .is_ok_and(|s| s.success()),
            "could not start {ours}"
        );

        let now = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        };
        let first = wait_for(|| running().into_iter().find(|s| s.name == ours))
            .and_then(|s| s.activity)
            .expect("no activity reported for a session that is printing");
        // Long enough that a clock which only moves on keystrokes has visibly
        // stopped: the stale reading is what made the tab go quiet.
        std::thread::sleep(std::time::Duration::from_secs(3));
        let later = running()
            .into_iter()
            .find(|s| s.name == ours)
            .and_then(|s| s.activity);
        let _ = kill(&ours);

        let later = later.expect("the session stopped being listed");
        assert!(
            later > first,
            "the clock stood still while the agent printed: {first} then {later}"
        );
        // And it is current, not merely moving — the judgement made of it is
        // "has this gone quiet in the last couple of seconds".
        assert!(
            now().saturating_sub(later) <= 2,
            "reported {later}, now {}",
            now()
        );
    }

    /// tmux creates sessions and spawns their commands asynchronously, so poll
    /// rather than guess a sleep long enough for a loaded runner.
    fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
        for _ in 0..50 {
            if let Some(v) = f() {
                return Some(v);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        None
    }
}
