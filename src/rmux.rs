//! Handing a tab's agent to rmux, so it outlives the cctop that started it.
//!
//! A pane normally runs its agent on a pty [`shim::host`](crate::shim::host)
//! owns, which means the agent's life is cctop's life: quit, and the agent goes
//! with it. That is the wrong bargain for a coding session. Nothing about
//! watching sessions should require staying in the monitor.
//!
//! So the pane runs `rmux new-session` instead of the agent, and the agent runs
//! inside the rmux server. What cctop hosts is then only the rmux *client* —
//! closing the pane detaches, quitting cctop detaches, and the agent notices
//! neither. Everything else is unchanged: it is still a process on a pty cctop
//! can draw and type into, so [`attach`](crate::attach) and the pane machinery
//! need to know nothing about any of this.
//!
//! Reattaching falls out of the same command. `new-session -A` attaches to the
//! session if it exists and creates it otherwise, so reopening a tab is the same
//! call as opening it, and the agent is found exactly where it was left —
//! scrollback and all.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Prefix on every rmux session cctop creates.
///
/// A namespace, so `cctop-*` is unambiguously ours and a user's own sessions are
/// never adopted, reattached to, or killed by anything here.
const PREFIX: &str = "cctop";

/// The multiplexer cctop drives.
///
/// One name in one place, so a second hard-coded `"rmux"` cannot drift from it.
/// cctop used to speak tmux's command surface and choose between the two with
/// `CCTOP_MUX`; rmux reimplements that surface, so the commands here —
/// `new-session -A`, `set-option`, `list-panes -F`, `kill-session` — are the
/// ones tmux always took. What is no longer optional is the daemon they reach.
///
/// The cost of dropping the choice, said plainly because it lands on upgrade:
/// the two keep separate daemons and separate sessions, so agents still running
/// under a tmux server are still running and cctop simply stops being able to
/// see them. `tmux attach -t cctop-…` reaches them, and nothing here kills one.
pub const BIN: &str = "rmux";

/// Run one piece of SDK work against the local daemon and wait for it.
///
/// The SDK is async and cctop is not. Everything here is one short exchange
/// with a daemon on a unix socket, so a current-thread runtime built for the
/// call is the whole of what "async" needs to mean at this boundary — no
/// runtime is kept, and nothing above this function learns that one existed.
fn on_daemon<T, F, Fut>(work: F) -> Result<T, String>
where
    F: FnOnce(rmux_sdk::Rmux) -> Fut,
    Fut: std::future::Future<Output = rmux_sdk::Result<T>>,
{
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start a runtime for rmux: {error}"))?;
    runtime.block_on(async move {
        // `connect_or_start` rather than `connect`: the daemon is started by
        // whichever of these runs first, and a cctop that only ever asks
        // questions should still get answers.
        let rmux = rmux_sdk::Rmux::builder()
            .connect_or_start()
            .await
            .map_err(|error| format!("{error}"))?;
        work(rmux).await.map_err(|error| format!("{error}"))
    })
}

/// Whether the multiplexer can be used at all.
///
/// Only that the binary exists: a server does not have to be running, since
/// `new-session` starts one. This is checked per launch rather than cached —
/// installing rmux while cctop runs should not require restarting it, and the
/// cost is one `rmux -V` against a launch that spawns a terminal anyway.
pub fn available() -> bool {
    Command::new(BIN)
        .arg("-V")
        .output()
        .is_ok_and(|out| out.status.success())
}

/// A way to put rmux on this machine, when one can be found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    /// What to call the package manager in the offer, so the user is agreeing
    /// to something they recognise rather than to "install rmux" in the
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

/// The ways rmux can be put on this machine, best first: what to call each one
/// in the offer, the command that has to exist for it to be possible, and the
/// command that does it.
///
/// All three are root-free, which is why there is no `sudo` anywhere below.
/// rmux also ships through apt and dnf, and both want a signed repository added
/// first — three commands, a key, and a distribution name to get right. cctop
/// offering to run that on someone's machine is offering to misconfigure a
/// package source; the routes here install one binary and are undone by
/// deleting it.
///
/// Homebrew leads because a machine that has it is a machine whose owner
/// administers it that way. cargo is next because a cctop installed by cargo —
/// which is most of them — already has it, and it builds the same version this
/// was written against. The install script is last because it is the one that
/// needs nothing, which also makes it the one with least to recommend it.
const MANAGERS: &[(&str, &str, &str)] = &[
    ("Homebrew", "brew", "brew install rmux"),
    ("cargo", "cargo", "cargo install rmux --locked"),
    (
        "rmux.io",
        "curl",
        "curl -fsSL https://rmux.io/install.sh | sh",
    ),
];

/// How cctop would install rmux here, if it can work out how.
///
/// `None` is the honest answer for a machine with none of the three. Offering
/// an install that cannot run is worse than the silent fallback it would
/// replace, since the user has then been told rmux is one keypress away and
/// watched the keypress fail.
///
/// Nothing here runs anything: it only decides what *would* be run, so the
/// decision can be shown to the user before any of it happens.
pub fn installer() -> Option<Install> {
    let (manager, _, command) = MANAGERS
        .iter()
        .find(|(_, needs, _)| crate::shim::is_command(needs))?;
    Some(Install {
        manager,
        // A script rather than an argv because the last entry is a pipeline,
        // and because a pane runs this on a pty where a shell is what reads it.
        argv: vec!["sh".to_string(), "-c".to_string(), command.to_string()],
    })
}

/// The command that puts `argv` in the rmux session called `name`, attaching to
/// it instead if it is already there.
///
/// `-c` sets the working directory for a session being created and is ignored
/// for one being attached to, which is the behaviour wanted in both cases: a new
/// agent starts in its project, and an existing one is not moved.
pub fn attach_or_create(argv: &[String], name: &str, cwd: Option<&Path>) -> Vec<String> {
    let mut out = vec![
        BIN.to_string(),
        "new-session".to_string(),
        "-A".to_string(),
        "-s".to_string(),
        name.to_string(),
    ];
    if let Some(dir) = cwd.filter(|d| d.is_dir()) {
        out.push("-c".into());
        out.push(dir.to_string_lossy().into_owned());
    }
    // `--` so an agent's own flags are never read as rmux's.
    out.push("--".into());
    out.extend(argv.iter().cloned());
    out
}

/// How many lines of an agent's output a cctop-owned rmux session keeps.
///
/// rmux reports no `history-limit` until one is set, so what a pane keeps
/// unasked is rmux's business and not something to rely on — tmux's answer was
/// 2000, which is a few minutes of a working agent: a pane you can scroll but
/// not scroll *back* to anything. Naming a number is what makes the scrollback
/// a property of cctop's sessions rather than of whatever the daemon defaults
/// to this release. These lines cost nothing until they exist and are gone with
/// the session.
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
/// before, with rmux's own defaults. Nothing here can cost a launch, and a
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
    let Ok(out) = Command::new(BIN).args(&create).output() else {
        return;
    };
    if !out.status.success() {
        return;
    }
    let placeholder = String::from_utf8_lossy(&out.stdout).trim().to_string();

    // One invocation for the three: `;` is rmux's own separator between
    // commands, which is why these need no shell to be read as three.
    let _ = Command::new(BIN)
        .args(["set-option", "-t", name, "history-limit", HISTORY_LINES])
        .args([";", "set-option", "-t", name, "mouse", "on"])
        .args([";", "set-option", "-t", name, "status", "off"])
        // Several cctops may hold a client on this session at once — that is
        // what sharing tabs means. Left at rmux's default the window is sized to
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
    let made = Command::new(BIN)
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
    let _ = Command::new(BIN)
        .args(["kill-window", "-t", &placeholder])
        .output();
}

/// The command that attaches to an existing rmux session and nothing else.
///
/// Distinct from [`attach_or_create`] so that picking an agent from the
/// launcher which has since ended reports that, rather than silently creating
/// an empty session wearing its name.
pub fn attach(name: &str) -> Vec<String> {
    vec![
        BIN.to_string(),
        "attach-session".to_string(),
        "-t".to_string(),
        format!("={name}"),
    ]
}

/// One browser share of a multiplexer session.
#[derive(Clone)]
pub struct Share {
    /// The link that can type into the agent. A shell credential, and named
    /// that way so nothing here hands it out by accident.
    ///
    /// An `Option` because rmux mints one per role and a `--spectator-only`
    /// share has none — [`web_share`] refuses that rather than reaching for the
    /// read-only link, which is a different thing wearing the same shape.
    pub operator: Option<String>,
    /// The pairing code the browser asks for after the link, absent under
    /// `--no-pin`. Useless without the link and useless on its own, which is why
    /// it is safe to put on the status line while the link goes to the clipboard.
    pub pin: Option<String>,
}

/// The tunnel rmux raises for a share cctop hands out.
///
/// An account-less SSH tunnel, chosen over cctop's own quick tunnel after the
/// quick tunnel was measured and found unable to carry the thing a share is
/// made of. See [`web_share`] for the measurement.
pub const SHARE_TUNNEL: &str = "localhost-run";

/// Open `name`'s terminal to a browser, reachable from off this machine.
///
/// `tunnelled` asks rmux to raise a tunnel of its own — [`SHARE_TUNNEL`] — and
/// put its origin in the link's fragment as the endpoint the browser opens a
/// socket to. Without it the endpoint is this machine's loopback: correct, and
/// reachable only from a browser already on it.
///
/// # Why rmux's tunnel and not cctop's
///
/// cctop holds a quick tunnel of its own for the served page, and pointing the
/// share at it with `--tunnel-url` looks like the tidier answer: one way out of
/// this machine, one origin to trust, one thing to close. It was the answer
/// here, and it does not work.
///
/// A share is a WebSocket. Through the quick tunnel the upgrade does not
/// survive — rmux answers the request as a plain `GET /share`, which is a 404,
/// and the browser sits on `Disconnected. Reconnecting…` for ever. Measured
/// rather than reasoned about:
///
/// ```text
/// loopback,          Upgrade: websocket → 101 Switching Protocols
/// cctop quick tunnel, Upgrade: websocket → 404
/// rmux localhost-run, Upgrade: websocket → 101
/// ```
///
/// So the share gets an ingress that carries what it is made of, and cctop's
/// tunnel keeps carrying the page, which is plain HTTP and an event stream.
/// Two ways out of the machine, because one of them cannot do this job.
///
/// The share itself is untouched by the route. rmux encrypts operator traffic
/// end to end and pairs it with a PIN, so the tunnel carries ciphertext it
/// cannot read — which is the difference between this and the served page,
/// where Cloudflare terminates the TLS and reads what passes through.
///
/// Asked over the daemon's own IPC rather than by running `rmux web-share` and
/// reading its output. That is not a tidiness: the CLI prints the operator link
/// on stderr and the spectator link on stdout, the two are the same shape, and
/// the *stream* is the only thing telling them apart. Handing out input to a
/// live coding agent should not rest on which file descriptor a line arrived
/// on, and here it does not — the two links are separate fields.
fn web_share(name: &str, tunnelled: bool) -> Result<Share, String> {
    share_with(name, tunnelled, false)
}

/// The share for `name`, minted on the first ask and reused after it.
///
/// Reused because a share is not free on either side. rmux keeps one per mint —
/// `rmux web-share list` grows a row each time — and each tunnelled one is an
/// SSH connection to a public relay that rate-limits per address, which is how
/// a page reopened five times started coming back untunnelled. One share per
/// session per cctop is also the honest number: it is one terminal, and every
/// link minted for it opens the same one.
///
/// Keyed by whether it is the embedded flavour, because that is a different
/// link and not a different terminal — see [`web_share_embedded`].
///
/// Tunnelled first, loopback second, and which one came back is the `bool`. A
/// machine with no way out still has a terminal worth opening from the browser
/// sitting on it, and a caller that cannot say which it got would be handing
/// out a link that silently only works from one desk.
///
/// Held for the life of the process. If the session it belongs to ends, the
/// share ends with it and the cached link stops answering — which is correct,
/// since there is no terminal left for it to reach either.
pub fn share_link(name: &str, embedded: bool) -> Result<(Share, bool), String> {
    /// A share and whether its endpoint is reachable off this machine.
    type Reachable = (Share, bool);
    static CACHE: std::sync::Mutex<Option<HashMap<(String, bool), Reachable>>> =
        std::sync::Mutex::new(None);
    let key = (name.to_string(), embedded);
    if let Ok(cache) = CACHE.lock()
        && let Some(held) = cache.as_ref().and_then(|c| c.get(&key))
    {
        return Ok(held.clone());
    }
    let mint = |tunnelled| match embedded {
        true => web_share_embedded(name, tunnelled),
        false => web_share(name, tunnelled),
    };
    // The tunnel is the half that needs a network and a relay that will have
    // it; the loopback share needs neither, so a failure to reach the world is
    // not a failure to open a terminal.
    let made = match mint(true) {
        Ok(share) => (share, true),
        Err(why) => (mint(false).map_err(|_| why)?, false),
    };
    if let Ok(mut cache) = CACHE.lock() {
        cache
            .get_or_insert_with(HashMap::new)
            .insert(key, made.clone());
    }
    Ok(made)
}

/// The same share, minted to live inside cctop's own page.
///
/// Four differences, all of them because the frame is not a browser tab:
///
/// - **No navigation bar and no disclaimer toast.** rmux draws both for a link
///   somebody was sent cold. Here the surrounding page is cctop's, the reader
///   arrived through it, and a second chrome inside the frame is chrome twice.
/// - **Operator only.** A spectator link nobody will ever use is a credential
///   minted for nothing.
/// - **No pairing code.** The PIN exists to protect a link that travelled on
///   its own — over chat, over mail — from being opened by whoever ended up
///   holding it. This one never travels: it is minted per request, behind the
///   page's token, and delivered into a frame's `src`. Anyone who can reach
///   that fetch can already mint another, so a PIN in the way of the reader
///   would be friction guarding a door that is already open behind it.
/// - **A dark palette**, which is the page's.
///
/// The tunnel is the one thing it does not change: an embedded share and a
/// copied one both need an ingress that carries a WebSocket, for the reason
/// [`web_share`] measures.
///
/// The consequence, stated once because it is the whole of the security model:
/// **whoever holds the page link can type into this agent's terminal**, not
/// merely prompt it. That is a shell, and it is why this is behind the same
/// `--no-actions` switch as everything else that acts.
fn web_share_embedded(name: &str, tunnelled: bool) -> Result<Share, String> {
    share_with(name, tunnelled, true)
}

fn share_with(name: &str, tunnelled: bool, embedded: bool) -> Result<Share, String> {
    let name = name.to_string();
    on_daemon(move |rmux| async move {
        let session = rmux.session(rmux_sdk::SessionName::new(name)?).await?;
        let mut builder = session.share();
        if tunnelled {
            builder = builder.tunnel_provider(SHARE_TUNNEL);
        }
        if embedded {
            builder = builder
                .operator_only()
                .no_navbar()
                .no_disclaimer()
                .no_pin()
                .dark_theme();
        }
        let handle = builder.await?;
        Ok(Share {
            operator: handle.operator_url().map(str::to_string),
            pin: handle.operator_pairing_code().map(str::to_string),
        })
    })
    .and_then(|share| match share.operator {
        // `--spectator-only` is a share this cannot return: there is no
        // operator link in it, and the read-only one is not a substitute.
        None => Err("the share came back without an operator link".to_string()),
        Some(_) => Ok(share),
    })
}

/// A rmux session name for a session being resumed.
///
/// Derived from the session's identity rather than allocated, which is what
/// makes resuming idempotent: pressing `R` twice finds the same rmux session and
/// reattaches to it, instead of starting a second agent on one transcript.
///
/// The whole id goes in, never a prefix of it. Names built from a prefix were
/// not unique: a Codex id is `rollout-<timestamp>-<uuid>`, so the first 24
/// characters end mid-timestamp at the minute — two Codex sessions started in
/// the same minute mapped to one rmux name, and resuming the second reattached
/// to the first while cctop reported it had opened the one asked for. Nothing
/// here can know where a provider keeps the distinguishing part of its id, so
/// the only safe answer is to keep all of it. rmux imposes no length limit worth
/// respecting — a 76-character name works — and the launcher already shortens
/// what it shows.
pub fn name_for_session(provider: &str, session_id: &str) -> String {
    // rmux reads `.` and `:` as address syntax, so neither can survive in a name.
    let id: String = session_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    sanitize(&format!("{PREFIX}-{provider}-{id}"))
}

/// A free rmux session name for a freshly launched agent.
///
/// Unlike a resume there is nothing to be idempotent about — two `claude` tabs
/// are two agents — so this counts up until it finds a name nobody is using.
pub fn free_name(label: &str) -> String {
    let base = sanitize(&format!("{PREFIX}-{label}"));
    if !exists(&base) {
        return base;
    }
    // Bounded: past a hundred live rmux sessions of one name, reusing the last
    // is a better failure than looping. `new-session -A` then attaches to it,
    // which is odd but harmless — and nobody has a hundred of these.
    (2..100)
        .map(|n| format!("{base}-{n}"))
        .find(|name| !exists(name))
        .unwrap_or(base)
}

/// Reduce a label to what rmux accepts in a session name.
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

/// Whether a rmux session by this name is alive.
pub fn exists(name: &str) -> bool {
    Command::new(BIN)
        .args(["has-session", "-t", &format!("={name}")])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// End a rmux session, taking the agent inside it with it.
///
/// The `=` prefix makes the target an exact name rather than a prefix match —
/// without it, killing `cctop-claude` would also kill `cctop-claude-2`.
pub fn kill(name: &str) -> Result<(), String> {
    let out = Command::new(BIN)
        .args(["kill-session", "-t", &format!("={name}")])
        .output()
        .map_err(|e| format!("rmux: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}

/// Turn off the status bar in one of cctop's own rmux sessions.
///
/// Two reasons, and the second is the one that matters. A cctop pane already has
/// a border with the agent's name on it and a footer under it, so rmux's bar is a
/// row of duplicate chrome inside someone else's frame.
///
/// More importantly it carries a clock, which rmux repaints every
/// `status-interval` — 15 seconds by default and 1 second in plenty of configs.
/// A pane's fallback idleness test is "has the screen stopped changing", so a
/// ticking clock is indistinguishable from an agent still working: with a
/// one-second interval no rmux-backed pane could ever go quiet, and the fallback
/// silently reported every abandoned agent as busy forever.
///
/// Scoped to the one session, so a user's own sessions and their global settings
/// are untouched. Best effort: a bar that stays on is cosmetic plus a weaker
/// fallback, and never worth failing a launch over.
pub fn quiet(name: &str) {
    // `=`, like every other target in this module. tmux rejected an exact-match
    // prefix on `set-option` outright ("no such session: =cctop-x") and this
    // was the one call that had to go without it; rmux takes it, so the
    // weaker form — a bare name, which rmux prefix-matches — is no longer the
    // only option here.
    let _ = Command::new(BIN)
        .args(["set-option", "-t", &format!("={name}"), "status", "off"])
        .output();
    // Let the agent's own notifications out. A harness that wants a desktop
    // notification wraps it in rmux's passthrough sequence, and rmux swallows
    // that unless told otherwise — so the OSC 9 an agent sends when it is
    // blocked never reached the pane's parser, where cctop now listens for it.
    // The bell always got through; this is the half that says what about.
    //
    // A pane option, not a session one, and set here rather than in [`prepare`]:
    // a pane inherits its options from the global set when it is made, so there
    // is nothing to configure until the agent's own pane exists. Best effort
    // like the rest — rmux before 3.3 has no such option and rejects it, which
    // costs a process and leaves the pane exactly as it was.
    let _ = Command::new("rmux")
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
    let _ = Command::new("rmux")
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
    let _ = Command::new("rmux")
        .args(["set-option", "-t", name, "@cctop_profile", profile])
        .output();
}

/// Record where this session's tab sits in the bar.
///
/// The third thing written onto the session rather than kept in this process,
/// and for the reason the other two are: every cctop on the machine draws the
/// same bar, and an arrangement one of them invented would otherwise last only
/// as long as it did. A tab dragged to the front stayed there until the next
/// F10 and then went back to being sorted by age, which is the arrangement
/// nobody chose.
///
/// Best effort, like its neighbours: an order that failed to save is a bar in
/// the old arrangement, not a broken one.
pub fn set_order(name: &str, order: usize) {
    let _ = Command::new(BIN)
        .args(["set-option", "-t", name, "@cctop_order", &order.to_string()])
        .output();
}

/// Every cctop-owned session, in the order their tabs were last left in.
///
/// [`running`] answers "which is newest", which is what the launcher wants.
/// The tab bar wants "which is first", and those are different questions the
/// moment anybody drags a tab.
pub fn running_in_tab_order() -> Vec<Running> {
    in_tab_order(running())
}

/// The ordering itself, given the sessions — the half worth testing.
///
/// Sessions nobody has arranged keep the order they had, which is oldest
/// first: a bar that renumbered itself when an unarranged agent appeared would
/// move tabs under someone typing into one. A stable sort is what leaves them
/// alone.
pub fn in_tab_order_of(newest_first: Vec<Running>) -> Vec<Running> {
    in_tab_order(newest_first)
}

fn in_tab_order(newest_first: Vec<Running>) -> Vec<Running> {
    let mut out: Vec<Running> = newest_first.into_iter().rev().collect();
    out.sort_by_key(|s| s.order.unwrap_or(u64::MAX));
    out
}

/// Let the wheel scroll one of cctop's own rmux sessions.
///
/// Without this a rmux-backed pane cannot be scrolled at all. rmux is on the
/// alternate screen, so the scrollback of whatever terminal cctop is running in
/// holds none of the agent's output, and the history that does hold it is
/// reachable only from copy-mode — which, with `mouse` off, only a prefix key
/// opens. Turning it on makes the wheel enter copy-mode and scroll, the way it
/// does in every terminal without rmux in the way.
///
/// Scoped to the one session and best effort, for the same reasons as
/// [`quiet`]: a user's own sessions keep their own setting, and a pane that
/// cannot be scrolled is not worth failing a launch over.
///
/// rmux keeps no history for a pane on the alternate screen, so this does
/// nothing for an agent that draws there — Claude writes to the normal buffer
/// and scrolls, but an agent that takes the alternate screen has nothing behind
/// it to scroll back to, under rmux or anywhere else.
pub fn mouse(name: &str) {
    // Same reason as `quiet` for the bare name: `set-option` rejects `=`.
    let _ = Command::new(BIN)
        .args(["set-option", "-t", name, "mouse", "on"])
        .output();
}

/// One cctop-owned rmux session, and the agent living in it.
///
/// The pid is the point. Everything cctop knows about a live agent — what its
/// hooks reported, what its transcript says, whether it is asking a question —
/// is keyed by the agent's own process, and a rmux-backed pane hosts the rmux
/// *client* instead. Without the pid on this side of the wall, an agent handed
/// to rmux is an agent cctop can only look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Running {
    /// The rmux session name, which is also cctop's handle on it.
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
    /// Not necessarily one of ours — a `rmux attach` in another terminal counts,
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
    /// suggests: rmux advances the session's clock on *client* activity, so a
    /// detached session's stands still however much its agent prints. Reading
    /// that one drew every unwatched agent as idle a few seconds after the last
    /// keystroke — and, with a tool call in flight, blinked its tab as if it
    /// were holding a permission prompt. The window's clock follows the output.
    pub activity: Option<u64>,
    /// What the cctop that started this agent called its tab, when it said.
    ///
    /// Written onto the session by [`set_label`] so that every other cctop names
    /// the tab the same thing. Nothing else can supply it: a resumed session is
    /// labelled after the conversation it is going back to, which the rmux name
    /// — a sanitised provider and uuid — cannot be read back out of.
    pub label: Option<String>,
    /// The account this agent was started under, when it was not the default
    /// one. Written onto the session by [`set_profile`], which says why.
    pub profile: Option<String>,
    /// Where this session's tab was last dragged to, if anywhere.
    ///
    /// Written onto the session by [`set_order`] for the same reason the label
    /// is: the session outlives every cctop, so it is the only place an
    /// arrangement of tabs can live. Without it the bar was rebuilt from
    /// creation times on every start, and a tab dragged to the front was back
    /// where it began the next time cctop opened.
    pub order: Option<u64>,
}

/// Every cctop-owned rmux session currently alive, newest first.
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
    // rmux looks up the session a pane belongs to — so asking per session would
    // be a subprocess each for the same answer.
    let Ok(out) = Command::new(BIN)
        .args([
            "list-panes",
            "-a",
            "-F",
            "#{session_name}\t#{pane_pid}\t#{pane_current_path}\t#{session_attached}\t#{session_created}\t#{window_activity}\t#{@cctop_label}\t#{@cctop_profile}\t#{@cctop_order}",
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
        let order = option().and_then(|v| v.parse::<u64>().ok());
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
                order,
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
    // The `=` that makes a target exact everywhere else in this module is a
    // parse error to `list-panes` under rmux ("can't find pane: =cctop-…"),
    // and the bare name it does accept prefix-matches — `cctop-claude-abc`
    // finds `cctop-claude-abc-2` when the first is gone. So the name comes back
    // in the format and is checked here: exact by answer rather than by syntax,
    // which is also the version that cannot be undone by either daemon
    // changing its mind about `=`.
    let out = Command::new(BIN)
        .args([
            "list-panes",
            "-t",
            name,
            "-F",
            "#{session_name}\t#{pane_pid}",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // First pane again: see [`running`].
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .find(|(session, _)| *session == name)
        .and_then(|(_, pid)| pid.trim().parse().ok())
}

/// The cctop session holding the agent running as `pid`, if one is.
///
/// The reverse of [`agent_pid`], and the question asked by anything that starts
/// from a process and needs to know whether cctop can reach its terminal: an
/// agent handed to rmux is on no pty of cctop's, so every other route to it
/// fails, but the session name is a way in.
pub fn holding(pid: u32) -> Option<String> {
    running()
        .into_iter()
        .find(|agent| agent.pid == Some(pid))
        .map(|agent| agent.name)
}

/// Serialises the tests that drive a real rmux server.
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

    /// The order written onto a session is the order read back off it.
    ///
    /// Against a real daemon, because that is the whole claim: the arrangement
    /// has to outlive the process that made it, and only rmux can say whether
    /// it did. The unit above tests the sorting; this tests that there is
    /// anything to sort by.
    #[test]
    fn an_order_written_onto_a_session_survives_in_it() {
        if !available() {
            eprintln!("skipping: rmux not installed");
            return;
        }
        let _turn = test_lock();
        let ours = format!("cctop-order-{}", std::process::id());
        let dir = std::env::temp_dir();
        assert!(
            Command::new(BIN)
                .args([
                    "new-session",
                    "-d",
                    "-s",
                    &ours,
                    "-c",
                    &dir.to_string_lossy(),
                    "--",
                    "sh",
                    "-c",
                    "sleep 30",
                ])
                .status()
                .is_ok_and(|s| s.success()),
            "could not start {ours}"
        );

        let before = wait_for(|| running().into_iter().find(|s| s.name == ours));
        set_order(&ours, 3);
        let after = wait_for(|| {
            running()
                .into_iter()
                .find(|s| s.name == ours && s.order.is_some())
        });
        let _ = kill(&ours);

        assert_eq!(before.map(|s| s.order), Some(None), "born with an order");
        assert_eq!(
            after.map(|s| s.order),
            Some(Some(3)),
            "the order did not come back off the session"
        );
    }

    /// The bar's order comes from what was written on the sessions, and the
    /// sessions nobody arranged keep the order they always had.
    ///
    /// The bug: the arrangement lived only in the running process, so a tab
    /// dragged to the front was back among the others — sorted by age — the
    /// next time cctop opened.
    #[test]
    fn arranged_tabs_lead_and_the_rest_stay_oldest_first() {
        let at = |name: &str, order: Option<u64>| Running {
            name: name.to_string(),
            pid: None,
            cwd: None,
            attached: false,
            activity: None,
            label: None,
            profile: None,
            order,
        };
        // As `running` gives them: newest first.
        let found = vec![
            at("newest", None),
            at("arranged-second", Some(1)),
            at("middle", None),
            at("arranged-first", Some(0)),
            at("oldest", None),
        ];
        let names: Vec<String> = in_tab_order(found).into_iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            [
                "arranged-first",
                "arranged-second",
                "oldest",
                "middle",
                "newest"
            ]
        );

        // With nothing arranged it is exactly what it was before: oldest first.
        let plain = vec![at("newest", None), at("middle", None), at("oldest", None)];
        let names: Vec<String> = in_tab_order(plain).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["oldest", "middle", "newest"]);
    }

    /// The share path, against a real daemon and through the same call the `W`
    /// key makes. There is nothing left to unit-test here: the operator link
    /// and its pairing code are fields on a typed handle rather than lines
    /// picked out of two streams, so what is worth checking is that the daemon
    /// answers and that what comes back is the operator's link and not the
    /// spectator's.
    #[test]
    fn a_real_rmux_session_comes_back_with_an_operator_link() {
        let _guard = test_lock();
        if !available() {
            eprintln!("skipping: rmux not installed");
            return;
        }
        let name = format!("cctop-share-{}", std::process::id());
        assert!(
            Command::new(BIN)
                .args(["new-session", "-d", "-s", &name, "--", "sleep", "30"])
                .output()
                .is_ok_and(|out| out.status.success()),
            "rmux would not start a session to share"
        );

        // Untunnelled, so the test needs no network: raising the tunnel is
        // rmux's half and dialling a provider from a unit test would be an SSH
        // connection to somebody else's host on every `cargo test`. What is
        // being checked here is the daemon's answer, which is the same either
        // way — the endpoint is loopback instead of a hostname.
        let share = web_share(&name, false);
        // Killing the session ends its share; `web-share -X` would also end any
        // the user has open, which is not this test's to touch.
        let _ = kill(&name);
        // And wait for it to be gone before the lock is. Killing the last
        // session stops the server, and a `new-session` that reaches the socket
        // while it is on its way down fails outright — so releasing the lock
        // the moment `kill` returns hands the next test a daemon mid-shutdown.
        // That is the race [`test_lock`] exists for, and serialising the tests
        // does not close it on its own.
        wait_for(|| (!exists(&name)).then_some(()));

        let share = share.expect("rmux shared the session");
        let operator = share.operator.expect("an operator link");
        assert!(operator.starts_with("https://"), "not a link: {operator}");
        // Untunnelled, so the link carries no endpoint at all: rmux writes one
        // into the fragment (`#e=wss://…`) only when there is somewhere off
        // this machine to name, and the browser page falls back to the
        // daemon's own loopback port when there is not.
        assert!(!operator.contains("e="), "an endpoint appeared: {operator}");
        assert!(
            operator.contains("#t="),
            "no share token in the link: {operator}"
        );
        // The spectator link is minted alongside it and must not be what came
        // back: it is the same shape and grants a different thing.
        assert!(
            !operator.contains("spectator"),
            "that is not the operator link: {operator}"
        );
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
        assert!(install.shown().contains("rmux"));
        if let Some(rest) = install.shown().strip_prefix("sudo ") {
            assert!(crate::shim::is_command("sudo"));
            // Only the leading command is elevated, so a second one after `&&`
            // needs its own sudo. apt is the entry where this matters.
            assert!(!rest.contains("&& apt-get install") || rest.contains("&& sudo"));
        }
    }

    /// The wrapped command has to attach-or-create by exact name, start in the
    /// project, and keep the agent's own flags away from rmux.
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
            &["rmux", "new-session", "-A", "-s", "cctop-claude-abc"]
        );
        // The agent's flags come after `--`, so rmux cannot claim them.
        let sep = out.iter().position(|a| a == "--").expect("separator");
        assert_eq!(&out[sep + 1..], &argv[..]);
        assert!(out.contains(&"-c".to_string()));

        // A directory that is not there is left off rather than failing the
        // spawn, matching what the pty path does with a stale cwd.
        let gone = attach_or_create(&argv, "n", Some(Path::new("/nonexistent/gone")));
        assert!(!gone.contains(&"-c".to_string()));
    }

    /// Resuming the same session twice must name the same rmux session, or `-A`
    /// has nothing to reattach to and the point is lost.
    #[test]
    fn a_session_always_maps_to_the_same_rmux_name() {
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
            "two Codex sessions from the same minute share a rmux session"
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

    /// rmux reads `.` and `:` as address syntax, so a name carrying either would
    /// target something other than the session meant — including, for a `:`,
    /// a *window* of it.
    #[test]
    fn names_carry_nothing_rmux_would_read_as_an_address() {
        for name in [
            name_for_session("claude", "/home/x/proj:1.2"),
            free_name("claude --resume /a/b"),
            sanitize("--leading and trailing--"),
        ] {
            assert!(
                !name.contains(['.', ':', ' ', '/']),
                "unsafe rmux name: {name}"
            );
            assert!(!name.starts_with('-') && !name.ends_with('-'), "{name}");
            assert!(!name.contains("--"), "collapsed runs: {name}");
        }
    }

    /// Every name is inside cctop's namespace, which is what keeps the kill and
    /// list paths off a user's own rmux sessions.
    #[test]
    fn names_stay_inside_the_cctop_namespace() {
        assert!(name_for_session("claude", "abc").starts_with("cctop-"));
        assert!(free_name("zsh").starts_with("cctop-"));
    }

    /// Finding the agent behind a session is the whole point of handing one to
    /// rmux and still knowing what it is doing: the pid reported here is what
    /// every hook, transcript, and table row is keyed by. Nothing smaller than a
    /// real server tests it, since the answer comes from rmux itself.
    #[test]
    fn a_session_reports_the_agent_inside_it() {
        if !available() {
            eprintln!("skipping: rmux not installed");
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
                Command::new(BIN)
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
        // agent that cctop put in rmux rather than telling the user cctop never
        // started it.
        let back = pid.and_then(holding);
        // Quieting the session is best effort, but it has to actually land: the
        // status bar it removes is what made an idle pane look busy forever.
        quiet(&ours);
        let status = Command::new(BIN)
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

        // Pid to session and back again agree, or `a` on a rmux-backed agent has
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
            eprintln!("skipping: rmux not installed");
            return;
        }
        let _turn = test_lock();
        let named = format!("cctop-probe-named-{}", std::process::id());
        let plain = format!("cctop-probe-plain-{}", std::process::id());
        for name in [&named, &plain] {
            assert!(
                Command::new("rmux")
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
    /// — the option can read back as set while the pane still holds rmux's 2000.
    #[test]
    fn a_prepared_session_holds_the_agent_with_room_to_scroll_back() {
        if !available() {
            eprintln!("skipping: rmux not installed");
            return;
        }
        let _turn = test_lock();
        let name = format!("cctop-prep-{}", std::process::id());
        let dir = std::env::temp_dir();
        let argv = vec!["sh".to_string(), "-c".to_string(), "sleep 30".to_string()];

        prepare(&argv, &name, Some(&dir));
        let ask = |fmt: &str| {
            Command::new(BIN)
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
            "the agent's pane kept rmux's default scrollback"
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
    /// printing. `session_activity` does not say so — rmux advances that on
    /// client activity, so a detached session's stands still — and reading it
    /// blinked every unwatched working tab as if it were asking a question.
    /// Only a real detached session producing output shows the difference.
    #[test]
    fn a_detached_session_that_is_printing_never_reads_as_quiet() {
        if !available() {
            eprintln!("skipping: rmux not installed");
            return;
        }
        let _turn = test_lock();
        let ours = format!("cctop-probe-busy-{}", std::process::id());
        assert!(
            Command::new("rmux")
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

    /// rmux creates sessions and spawns their commands asynchronously, so poll
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
