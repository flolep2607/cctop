//! Events pushed from a live agent into a running cctop.
//!
//! Everything else cctop knows is forensic: it walks transcripts written after
//! the fact and re-derives state from them. That works, but it cannot see the
//! moments that matter most — an agent finishing its turn, an agent blocking on
//! a question, a session beginning or ending — because a transcript records
//! "answered you" and "still thinking" identically, and a session that has
//! ended looks exactly like one that is merely quiet. The agents can say all of
//! it outright: Claude Code through its hooks, Codex through its `notify`
//! program.
//!
//! Two halves live here. [`emit`] is `cctop hook`, the command the agent spawns;
//! [`Listener`] is the socket a running UI holds open. The wire between them is
//! one JSON line per event.
//!
//! # The hook must never break the session
//!
//! This is the whole risk of the feature and it shapes every line of [`emit`].
//! An agent does not treat its hooks as observers: Claude Code reads the exit
//! code as a *decision*, where 2 blocks the tool call outright and feeds stderr
//! back to the model, and it reads stdout as content to act on. A monitoring
//! hook that fails loudly would break the coding session it was meant to watch,
//! and it would do it at the worst moment — mid tool call, on someone else's
//! machine, for a feature they only turned on to get a nicer notification.
//!
//! So `cctop hook` has one guarantee it keeps ahead of doing its job: it exits
//! 0, writes nothing to stdout, and returns promptly. Not by being careful —
//! by construction. Every fallible step discards its error, the whole exchange
//! runs under a deadline on a thread the process is willing to abandon, and a
//! panic hook turns even an unexpected unwind into a silent success. cctop being
//! absent, stopped, or mid-crash is the *ordinary* case, not an error worth
//! reporting.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Longest `cctop hook` may take, start to finish, whatever it is doing.
///
/// The agent is blocked for this long on every hook fire, so it buys nothing to
/// be generous: a local socket with a reader attached answers in microseconds
/// (measured at 1–2ms including the process spawn), and anything slower than
/// this is a cctop that cannot keep up, whose events are better dropped.
const DEADLINE: std::time::Duration = std::time::Duration::from_millis(250);

/// Cap on a single event, so a pathological payload cannot be read forever.
/// A hook's input is a small object; a transcript never comes through here,
/// only its path.
const MAX_EVENT: u64 = 256 * 1024;

/// Cap on how many cctops one event is delivered to.
///
/// Nobody runs sixteen, so reaching this means the directory has filled with
/// addresses that are not being cleaned up — at which point delivering to the
/// first few and returning beats spending the agent's deadline on the rest.
const MAX_PEERS: usize = 16;

/// Where running cctops advertise themselves: one socket per instance.
///
/// A single well-known address would be simpler, but only one process can bind
/// it — so a second cctop was deaf, and the events it missed were exactly the
/// ones it existed to show. A directory lets the hook fan out instead.
fn socket_dir() -> Option<PathBuf> {
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .map(|d| d.join("cctop").join("hooks.d"))
}

/// The addresses to deliver to, oldest name first.
///
/// Sorted so delivery order is stable rather than whatever the directory
/// happens to yield, which makes a truncated fan-out reproducible.
fn peers(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "sock"))
        .collect();
    found.sort();
    found.truncate(MAX_PEERS);
    found
}

/// `cctop hook <event>`, or `cctop hook codex <json>` — forward one event to
/// every running cctop.
///
/// Always returns 0. See the module docs for why that is a guarantee and not an
/// aspiration.
pub fn emit(args: &[String]) -> i32 {
    // An unwind anywhere below would exit 101, which the agent reads as a hook
    // failure. Turn it into the same silent success as every other failure.
    std::panic::set_hook(Box::new(|_| std::process::exit(0)));

    // The work happens on a thread this one is willing to abandon, and the
    // deadline covers all of it rather than the one call that looked risky.
    //
    // Timing out the write is not enough: `UnixStream::connect` has no timeout,
    // and connecting to a socket whose owner is listening but has stopped
    // accepting blocks until it does. A cctop wedged that way would hang the
    // agent on every hook — which was measured, not imagined. Bounding the whole
    // operation also covers whatever else turns out to block that this comment
    // does not predict.
    let args = args.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(|| forward(&args));
        let _ = tx.send(());
    });
    // Returning exits the process, which takes the thread with it wherever it
    // got to. A dropped event is a cheaper failure than a stalled agent.
    let _ = rx.recv_timeout(DEADLINE);
    0
}

/// Read the event however this agent hands it over, and deliver it.
fn forward(args: &[String]) {
    // Claude Code writes the event to stdin and cctop names it on the command
    // line. Codex runs its `notify` program with the JSON as the last argument
    // and nothing on stdin at all, so the two are read differently and then
    // treated the same.
    let (name, payload) = match args.first().map(String::as_str) {
        Some(CODEX_SELECTOR) => match args.get(1) {
            Some(json) => (String::new(), json.as_bytes().to_vec()),
            None => return,
        },
        other => {
            let mut buf = Vec::new();
            // Bounded, and a read error just means there is nothing to forward.
            if std::io::stdin()
                .take(MAX_EVENT)
                .read_to_end(&mut buf)
                .is_err()
            {
                return;
            }
            (other.unwrap_or_default().to_string(), buf)
        }
    };

    let Some(line) = envelope(&name, &payload) else {
        return;
    };
    deliver(&line);
}

/// Write one framed event to every cctop that will take it.
#[cfg(unix)]
fn deliver(line: &[u8]) {
    use std::os::unix::net::UnixStream;

    let Some(dir) = socket_dir() else { return };
    let started = std::time::Instant::now();
    for path in peers(&dir) {
        // Whatever is left of the agent's patience. Stopping here rather than
        // starting another connect is what keeps the fan-out from turning one
        // wedged cctop into a slow hook for everyone.
        if started.elapsed() >= DEADLINE {
            return;
        }
        match UnixStream::connect(&path) {
            Ok(mut stream) => {
                let _ = stream.set_write_timeout(Some(DEADLINE));
                let _ = stream.write_all(line);
                let _ = stream.flush();
            }
            // A socket file whose owner is gone refuses connections but stays on
            // disk. Nobody else can be about to bind this name — an address is
            // stamped with the instant it was created and never reused — so the
            // process that finds it dead is the one that can clean it up.
            Err(_) => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

#[cfg(not(unix))]
fn deliver(_line: &[u8]) {}

/// Reduce whatever the agent sent to the three things cctop needs, on one line.
///
/// Newlines inside the agent's JSON are what makes the framing need doing at
/// all. The field names differ per agent, so both spellings are tried: Claude
/// Code says `session_id` and `hook_event_name`, Codex says `thread-id` and
/// `type`.
fn envelope(name: &str, payload: &[u8]) -> Option<Vec<u8>> {
    let body: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let field = |key: &str| body.get(key).and_then(|v| v.as_str()).unwrap_or_default();
    let event = serde_json::json!({
        // The event name is in the payload, but an installer can also pass it as
        // an argument — taking it from the command line means a harness whose
        // payload spells it differently still lands in the right bin.
        "event": match name.is_empty() {
            true => match field("hook_event_name") {
                "" => field("type"),
                claude => claude,
            },
            false => name,
        },
        "session_id": match field("session_id") {
            "" => field("thread-id"),
            claude => claude,
        },
        "cwd": field("cwd"),
        // Only `SubagentStop` carries one, and it is the id cctop's own
        // subagent transcripts are named after.
        "agent_id": field("agent_id"),
    });
    let mut line = serde_json::to_vec(&event).ok()?;
    line.push(b'\n');
    Some(line)
}

/// What an event says about a session, as far as the UI is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// The agent asked something and is blocked on the answer.
    NeedsInput,
    /// The agent finished its turn; the prompt is the user's.
    Idle,
    /// The agent started working, which cancels either of the above.
    Busy,
    /// The agent is compacting its context: working, and about to lose history.
    Compacting,
    /// The session has begun. Like [`Signal::Busy`], but there is a row to find.
    Started,
    /// The session is over. There is nothing left to report about it.
    Ended,
}

impl Signal {
    /// Whether the agent is working rather than waiting on you.
    pub fn is_working(self) -> bool {
        matches!(self, Signal::Busy | Signal::Compacting | Signal::Started)
    }

    /// Whether this changes *which sessions exist*, and so is worth a rescan
    /// rather than waiting for the next poll to notice.
    pub fn is_lifecycle(self) -> bool {
        matches!(self, Signal::Started | Signal::Ended)
    }

    /// A word for the STATE column and the hooks panel.
    pub fn label(self) -> &'static str {
        match self {
            Signal::NeedsInput => "asking",
            Signal::Idle => "idle",
            Signal::Busy => "working",
            Signal::Compacting => "compacting",
            Signal::Started => "started",
            Signal::Ended => "ended",
        }
    }
}

/// One event, resolved to the session it concerns.
#[derive(Debug, Clone)]
pub struct Event {
    pub session_id: String,
    /// What the agent last said about itself, and where it is working.
    pub reported: Reported,
    /// The subagent this event is about, when it is about one.
    ///
    /// `SubagentStop` is the only end-of-subagent signal that exists. A
    /// background subagent's tool_result arrives at *launch* — it says the agent
    /// started, not that it finished — so a transcript alone cannot tell a
    /// working subagent from a finished one.
    pub finished_agent: Option<String>,
}

/// The last thing a session reported, kept per session.
///
/// The directory comes along because it is the only human-readable name an
/// event carries: a session id says nothing, and the row it belongs to may not
/// have been discovered yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    pub signal: Signal,
    /// Empty when the agent did not say.
    pub cwd: String,
}

/// Map an event name onto what it means. Unknown names are dropped rather than
/// guessed at, so a newer agent can add events without confusing this one.
fn signal_of(event: &str) -> Option<Signal> {
    match event {
        // The turn is over. This is the event the whole feature exists for:
        // nothing in a transcript distinguishes it from the agent still working.
        // Codex's `notify` fires once per turn and says only this.
        "Stop" | "agent-turn-complete" => Some(Signal::Idle),
        // Claude Code raises this when it wants the user — a permission prompt,
        // or an idle nudge.
        "Notification" => Some(Signal::NeedsInput),
        // All of these mean work has started, which answers whatever came
        // before. `SubagentStop` included: a subagent finishing tells you the
        // agent that spawned it is still going.
        "UserPromptSubmit" | "PreToolUse" | "SubagentStop" => Some(Signal::Busy),
        // Compaction is the one kind of work worth naming separately: the
        // context panel is about to lurch, and it is not the agent stalling.
        "PreCompact" => Some(Signal::Compacting),
        "SessionStart" => Some(Signal::Started),
        "SessionEnd" => Some(Signal::Ended),
        _ => None,
    }
}

/// The socket a running cctop listens on, and the events it has collected.
///
/// Events land on a reader thread and are drained by the UI loop, which is the
/// same shape [`Attach`](crate::attach::Attach) uses — the loop already wakes on
/// a timer, so a channel would add plumbing and arrive no sooner.
pub struct Listener {
    pending: std::sync::Arc<std::sync::Mutex<Vec<Event>>>,
    /// Removed on the way out. A leftover file is survivable — the next hook to
    /// find it refuses connections unlinks it — but only the instance that bound
    /// it can know it is finished with it.
    path: PathBuf,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Listener {
    /// Start listening on an address of this instance's own.
    ///
    /// Unlike the shared address this replaced, every cctop gets one: two
    /// windows open on the same machine both see the agents report in.
    #[cfg(unix)]
    pub fn start() -> Option<Listener> {
        use std::os::unix::net::UnixListener;

        let dir = socket_dir()?;
        std::fs::create_dir_all(&dir).ok()?;
        // Hook events name the user's projects; keep the directory private even
        // when it lands in a shared cache root.
        let _ = std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));

        // Stamped with the moment it was created as well as the pid, so a name
        // is never reused. That is what lets a hook that finds an address dead
        // delete it without racing an instance that is just starting up.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = dir.join(format!("{}-{stamp}.sock", std::process::id()));
        let listener = UnixListener::bind(&path).ok()?;

        let pending = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let collector = std::sync::Arc::clone(&pending);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                // One connection per event, so it is read to EOF and dropped.
                // A hook that connects and stalls must not hold up the next one.
                let collector = std::sync::Arc::clone(&collector);
                std::thread::spawn(move || {
                    let mut text = String::new();
                    if stream.take(MAX_EVENT).read_to_string(&mut text).is_err() {
                        return;
                    }
                    let events: Vec<Event> = text.lines().filter_map(parse).collect();
                    if events.is_empty() {
                        return;
                    }
                    collector
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .extend(events);
                });
            }
        });
        Some(Listener { pending, path })
    }

    #[cfg(not(unix))]
    pub fn start() -> Option<Listener> {
        None
    }

    /// Everything that has arrived since the last call.
    pub fn drain(&self) -> Vec<Event> {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *pending)
    }

    /// How many *other* cctops are also listening.
    pub fn peer_count(&self) -> usize {
        socket_dir()
            .map(|dir| peers(&dir).len().saturating_sub(1))
            .unwrap_or(0)
    }
}

fn parse(line: &str) -> Option<Event> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let session_id = value.get("session_id")?.as_str()?.to_string();
    // A session cctop cannot name is an event it cannot apply to a row.
    if session_id.is_empty() {
        return None;
    }
    Some(Event {
        session_id,
        // Claude Code names it `agent_id`, and cctop stores that subagent's
        // transcript as `agent-<agent_id>.jsonl`, so the two line up directly.
        finished_agent: (value.get("event").and_then(|v| v.as_str()) == Some("SubagentStop"))
            .then(|| value.get("agent_id").and_then(|v| v.as_str()))
            .flatten()
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        reported: Reported {
            signal: signal_of(value.get("event")?.as_str()?)?,
            cwd: value
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        },
    })
}

// ---------------------------------------------------------------------------
// Installation
// ---------------------------------------------------------------------------

/// Events cctop asks Claude Code to tell it about.
///
/// Deliberately still a small set: every hook fire costs the agent a process
/// spawn, so an event cctop would only use to redraw something it can already
/// see is not worth the fork. `PreToolUse` is the only frequent one; the rest
/// fire a handful of times in a session and each answers a question the
/// transcript cannot.
const WANTED: [&str; 8] = [
    "Stop",
    "Notification",
    "UserPromptSubmit",
    "PreToolUse",
    "SessionStart",
    "SessionEnd",
    "PreCompact",
    "SubagentStop",
];

/// The word that separates the binary from the event in a command cctop wrote.
///
/// Entries are recognised by their command text rather than by a marker key:
/// the file belongs to the user and their other tools, and its schema has no
/// room for a comment.
const MARKER: &str = " hook ";

/// The argument that tells `cctop hook` its payload is a Codex one, arriving in
/// argv rather than on stdin.
const CODEX_SELECTOR: &str = "codex";

/// Which settings file an install writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// `~/.claude/settings.json` — every session on this machine.
    User,
    /// `<dir>/.claude/settings.json` — sessions started in one project. Shared
    /// with whoever else checks that file out, which is why it is never the
    /// default.
    Project(PathBuf),
}

impl Scope {
    pub fn settings_file(&self) -> PathBuf {
        match self {
            // Honours the same `$CLAUDE_CONFIG_DIR` override as the rest of cctop.
            Scope::User => crate::config::CLAUDE_CONFIG_DIR.join("settings.json"),
            Scope::Project(dir) => dir.join(".claude").join("settings.json"),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Scope::User => "user",
            Scope::Project(_) => "project",
        }
    }

    /// Parse the word a `--install-hooks` argument carries.
    pub fn parse(word: &str, cwd: &Path) -> Option<Scope> {
        match word {
            "user" | "global" => Some(Scope::User),
            "project" | "local" => Some(Scope::Project(cwd.to_path_buf())),
            _ => None,
        }
    }
}

/// This binary's absolute path, which is what an installed hook has to name.
///
/// A bare `cctop` would depend on the agent's `PATH`, which is not this shell's.
/// The absolute path is what makes the hook fire at all.
fn own_exe() -> anyhow::Result<String> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .ok_or_else(|| anyhow::anyhow!("could not find cctop's own path"))
}

/// Add cctop's hooks to a Claude Code settings file, leaving everything else
/// alone.
///
/// The file is the user's, and by the time cctop sees it their other tools have
/// usually put hooks in it — so this merges into the arrays rather than writing
/// them, and never reorders or reformats what it did not add.
pub fn install(scope: &Scope) -> anyhow::Result<String> {
    let path = scope.settings_file();
    let mut root = read_settings(&path)?;
    let exe = own_exe()?;

    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("`hooks` in {} is not an object", path.display()))?;

    for event in WANTED {
        let list = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("`hooks.{event}` is not an array"))?;
        // Idempotent: running the installer twice must not fire twice, and an
        // entry left by an older cctop at a path that has since moved is
        // replaced rather than added to.
        list.retain(|entry| !is_ours(entry));
        list.push(serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": format!("{exe} hook {event}"),
            }]
        }));
    }
    write_settings(&path, &root)?;
    Ok(format!(
        "Added {} hooks to {}",
        WANTED.len(),
        path.display()
    ))
}

/// Take cctop's hooks back out, leaving every other entry untouched.
pub fn remove(scope: &Scope) -> anyhow::Result<String> {
    let path = scope.settings_file();
    let mut root = read_settings(&path)?;
    let mut removed = 0;
    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for (_, value) in hooks.iter_mut() {
            if let Some(list) = value.as_array_mut() {
                let before = list.len();
                list.retain(|entry| !is_ours(entry));
                removed += before - list.len();
            }
        }
        // An event whose only entry was ours goes too, rather than leaving an
        // empty array behind in someone else's file.
        hooks.retain(|_, value| !value.as_array().is_some_and(|l| l.is_empty()));
    }
    if removed > 0 {
        write_settings(&path, &root)?;
    }
    Ok(format!(
        "Removed {removed} cctop hooks from {}",
        path.display()
    ))
}

/// Whether a settings entry is one cctop wrote.
fn is_ours(entry: &serde_json::Value) -> bool {
    entry_commands(entry).any(is_our_command)
}

/// Whether a command line is one the installer wrote: `<a cctop> hook <Event>`.
///
/// Matching the literal string `cctop hook` was the obvious thing and was
/// wrong: it assumes the binary is named exactly `cctop`, so anyone running
/// `cctop-0.1.12`, a renamed build, or cargo's own test binary had their
/// entries go unrecognised — which made installing twice register the hooks
/// twice, and removing leave them all behind. What is actually distinctive is
/// the shape: a program whose *file name* mentions cctop, the word `hook`, and
/// one bare event name.
fn is_our_command(command: &str) -> bool {
    let Some((exe, event)) = command.rsplit_once(MARKER) else {
        return false;
    };
    let event = event.trim();
    !event.is_empty()
        && !event.contains(char::is_whitespace)
        && Path::new(exe.trim())
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("cctop"))
}

fn entry_commands(entry: &serde_json::Value) -> impl Iterator<Item = &str> {
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|inner| inner.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|h| h.get("command").and_then(|c| c.as_str()))
}

/// The cctop an installed command names, taken back out of the command text.
///
/// The installer writes `<exe> hook <Event>` and never quotes, so everything
/// before the last `hook` is the path — including a path with spaces in it,
/// which is why this splits on the marker rather than on whitespace.
fn recorded_exe(command: &str) -> Option<&str> {
    if !is_our_command(command) {
        return None;
    }
    Some(command.rsplit_once(MARKER)?.0.trim())
}

fn read_settings(path: &Path) -> anyhow::Result<serde_json::Map<String, serde_json::Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}".to_string(),
        Err(e) => return Err(e.into()),
    };
    let value: serde_json::Value = serde_json::from_str(text.trim()).map_err(|e| {
        // Refusing is the only safe move: rewriting a file cctop could not parse
        // would throw away whatever the user has in it.
        anyhow::anyhow!("{} is not valid JSON ({e}); fix it first", path.display())
    })?;
    match value {
        serde_json::Value::Object(map) => Ok(map),
        _ => anyhow::bail!("{} is not a JSON object", path.display()),
    }
}

fn write_settings(
    path: &Path,
    root: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Written beside the target and renamed: a crash mid-write would otherwise
    // leave the user with no settings at all, which breaks their agent far more
    // thoroughly than a missing hook.
    let tmp = path.with_extension("json.cctop-tmp");
    std::fs::write(&tmp, format!("{}\n", serde_json::to_string_pretty(root)?))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Codex
// ---------------------------------------------------------------------------

/// Codex's config file, honouring the same `$CODEX_HOME` override as the rest
/// of cctop.
fn codex_config_file() -> PathBuf {
    crate::config::CODEX_HOME.join("config.toml")
}

/// The `notify` value cctop installs.
///
/// Codex runs this program once a turn and appends the event as one more
/// argument, so the JSON arrives in argv rather than on stdin — which is why
/// `hook` needs to be told the payload is a Codex one before it looks for it.
fn codex_notify(exe: &str) -> Vec<String> {
    vec![exe.to_string(), "hook".into(), CODEX_SELECTOR.into()]
}

/// Point Codex's `notify` at cctop.
///
/// Codex allows exactly one `notify` program, so unlike Claude Code's hook
/// arrays this cannot merge: an existing entry that is not ours is left alone
/// and reported, because replacing someone's desktop-notification script with a
/// monitor is not a trade cctop gets to make for them.
pub fn codex_install() -> anyhow::Result<String> {
    let path = codex_config_file();
    let mut doc = read_codex(&path)?;
    let exe = own_exe()?;

    if let Some(existing) = codex_notify_argv(&doc)
        && !existing.iter().any(|a| a.contains("cctop"))
    {
        anyhow::bail!(
            "{} already sets notify = {existing:?}; remove it first if you want cctop to have it",
            path.display()
        );
    }

    let mut array = toml_edit::Array::new();
    for arg in codex_notify(&exe) {
        array.push(arg);
    }
    doc["notify"] = toml_edit::value(array);
    write_codex(&path, &doc)?;
    Ok(format!(
        "Pointed Codex's notify at cctop in {}",
        path.display()
    ))
}

/// Take cctop back out of Codex's `notify`, leaving another tool's alone.
pub fn codex_remove() -> anyhow::Result<String> {
    let path = codex_config_file();
    let mut doc = read_codex(&path)?;
    match codex_notify_argv(&doc) {
        Some(argv) if argv.iter().any(|a| a.contains("cctop")) => {
            doc.remove("notify");
            write_codex(&path, &doc)?;
            Ok(format!("Removed cctop from notify in {}", path.display()))
        }
        Some(_) => Ok(format!(
            "notify in {} belongs to something else; left alone",
            path.display()
        )),
        None => Ok(format!(
            "Codex was not notifying cctop ({})",
            path.display()
        )),
    }
}

/// The `notify` program Codex is configured to run, as its argument vector.
fn codex_notify_argv(doc: &toml_edit::DocumentMut) -> Option<Vec<String>> {
    let array = doc.get("notify")?.as_array()?;
    Some(
        array
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
    )
}

fn read_codex(path: &Path) -> anyhow::Result<toml_edit::DocumentMut> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    // Same refusal as the JSON side, and for the same reason: a config cctop
    // cannot parse is one it must not rewrite. `toml_edit` is used rather than a
    // plain deserializer so the user's comments and layout survive the edit.
    text.parse::<toml_edit::DocumentMut>()
        .map_err(|e| anyhow::anyhow!("{} is not valid TOML ({e}); fix it first", path.display()))
}

fn write_codex(path: &Path, doc: &toml_edit::DocumentMut) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.cctop-tmp");
    std::fs::write(&tmp, doc.to_string())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// How an installed hook is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    /// Nothing of cctop's is in this file.
    Absent,
    /// Every wanted event is registered, at this binary.
    Installed,
    /// Registered, but not for everything cctop now wants — an install from an
    /// older cctop that knew fewer events.
    Partial(Vec<&'static str>),
    /// Registered at a path that is not this binary but does exist. Two cctops
    /// on one machine is a choice, not a fault, so this is reported and left.
    Other(String),
    /// Registered at a path that is gone. The hooks are firing nothing at all,
    /// and this is the one case worth fixing without being asked.
    Broken(String),
    /// The file could not be read, so nothing can be said about it.
    Unreadable(String),
}

impl Health {
    /// Whether this is something the user would want to see rather than a
    /// working install or an absence they chose.
    pub fn is_problem(&self) -> bool {
        matches!(
            self,
            Health::Partial(_) | Health::Broken(_) | Health::Unreadable(_)
        )
    }
}

/// What one settings file has to say.
#[derive(Debug, Clone)]
pub struct ScopeStatus {
    pub scope: Scope,
    pub path: PathBuf,
    pub health: Health,
}

/// The whole integration, in one value the CLI and the UI both render.
#[derive(Debug, Clone)]
pub struct Report {
    pub claude: Vec<ScopeStatus>,
    /// The `notify` program Codex will run, if any.
    pub codex: (PathBuf, Health),
    /// Whether this cctop is receiving events, and how many others also are.
    pub listening: bool,
    pub peers: usize,
}

/// Inspect one settings file.
pub fn scope_status(scope: Scope) -> ScopeStatus {
    let path = scope.settings_file();
    let health = match read_settings(&path) {
        Err(e) => Health::Unreadable(e.to_string()),
        Ok(root) => {
            let hooks = root.get("hooks").and_then(|h| h.as_object());
            let mut missing = Vec::new();
            let mut recorded: Option<String> = None;
            for event in WANTED {
                let entry = hooks
                    .and_then(|h| h.get(event))
                    .and_then(|v| v.as_array())
                    .and_then(|list| list.iter().find(|e| is_ours(e)));
                match entry {
                    None => missing.push(event),
                    Some(entry) => {
                        recorded = recorded.or_else(|| {
                            entry_commands(entry)
                                .find_map(recorded_exe)
                                .map(str::to_string)
                        })
                    }
                }
            }
            match recorded {
                None => Health::Absent,
                Some(exe) if !Path::new(&exe).exists() => Health::Broken(exe),
                Some(exe) if Some(exe.as_str()) != own_exe().ok().as_deref() => Health::Other(exe),
                Some(_) if !missing.is_empty() => Health::Partial(missing),
                Some(_) => Health::Installed,
            }
        }
    };
    ScopeStatus {
        scope,
        path,
        health,
    }
}

/// Inspect the whole integration. `cwd` decides which project scope is looked
/// at; `listener` is this instance's, when it has one.
pub fn status(cwd: Option<&Path>, listener: Option<&Listener>) -> Report {
    let mut claude = vec![scope_status(Scope::User)];
    if let Some(dir) = cwd {
        claude.push(scope_status(Scope::Project(dir.to_path_buf())));
    }

    let codex_path = codex_config_file();
    let codex = match read_codex(&codex_path) {
        Err(e) => Health::Unreadable(e.to_string()),
        Ok(doc) => match codex_notify_argv(&doc) {
            None => Health::Absent,
            Some(argv) => match argv.first() {
                None => Health::Absent,
                Some(exe) if !exe.contains("cctop") => Health::Other(argv.join(" ")),
                Some(exe) if !Path::new(exe).exists() => Health::Broken(exe.clone()),
                Some(exe) if Some(exe.as_str()) != own_exe().ok().as_deref() => {
                    Health::Other(exe.clone())
                }
                Some(_) => Health::Installed,
            },
        },
    };

    Report {
        claude,
        codex: (codex_path, codex),
        listening: listener.is_some(),
        peers: listener.map(Listener::peer_count).unwrap_or(0),
    }
}

impl Report {
    /// The report as lines to print or draw, each tagged with whether it is
    /// something to worry about.
    pub fn lines(&self) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        for status in &self.claude {
            let (text, bad) = describe(&status.health);
            out.push((
                format!(
                    "Claude Code ({}) {}: {text}",
                    status.scope.label(),
                    status.path.display()
                ),
                bad,
            ));
        }
        let (text, bad) = describe(&self.codex.1);
        out.push((
            format!("Codex notify {}: {text}", self.codex.0.display()),
            bad,
        ));
        out.push(match (self.listening, self.peers) {
            (false, _) => ("Listener: not running".into(), true),
            (true, 0) => ("Listener: receiving".into(), false),
            (true, n) => (
                format!("Listener: receiving, alongside {n} other cctop(s)"),
                false,
            ),
        });
        out
    }
}

fn describe(health: &Health) -> (String, bool) {
    match health {
        Health::Absent => ("not installed".into(), false),
        Health::Installed => ("installed".into(), false),
        Health::Partial(missing) => (
            format!("installed, but missing {}", missing.join(", ")),
            true,
        ),
        Health::Other(exe) => (format!("installed, pointing at {exe}"), false),
        Health::Broken(exe) => (format!("points at {exe}, which is gone"), true),
        Health::Unreadable(why) => (why.clone(), true),
    }
}

/// Repoint any install whose recorded cctop no longer exists at this one.
///
/// The narrow case on purpose. A hook naming a binary that is gone fires
/// nothing — there is no behaviour to preserve and nothing the user could have
/// meant by it — so fixing it silently is strictly better than a monitor that
/// quietly stopped being told anything. An install pointing at a *different*
/// cctop that does exist is left alone: that is a second install, not a fault,
/// and stealing it would be a surprise.
pub fn repair(cwd: Option<&Path>) -> Vec<String> {
    let mut fixed = Vec::new();
    let mut scopes = vec![Scope::User];
    if let Some(dir) = cwd {
        scopes.push(Scope::Project(dir.to_path_buf()));
    }
    for scope in scopes {
        if matches!(scope_status(scope.clone()).health, Health::Broken(_))
            && install(&scope).is_ok()
        {
            fixed.push(format!("repointed {} hooks at this cctop", scope.label()));
        }
    }
    if matches!(status(None, None).codex.1, Health::Broken(_)) && codex_install().is_ok() {
        fixed.push("repointed Codex's notify at this cctop".into());
    }
    fixed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cctop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The only word cctop gets that a *background* subagent has finished. Its
    /// tool_result arrives when it launches, so without this every background
    /// subagent reads as done about three seconds into its run — and the id has
    /// to survive the envelope, which forwards a fixed set of fields.
    ///
    /// The payload is the one Claude Code 2.1 actually sends, captured from a
    /// live `SubagentStop`.
    #[test]
    fn a_subagent_stop_names_the_subagent_that_finished() {
        let raw = br#"{"session_id":"parent-1","cwd":"/x","hook_event_name":"SubagentStop","agent_id":"ab3e95cbb4558bd90","agent_type":"general-purpose","last_assistant_message":"pineapple"}"#;
        let line = envelope("", raw).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");

        assert_eq!(
            event.finished_agent.as_deref(),
            Some("ab3e95cbb4558bd90"),
            "the id must reach the UI, not stop at the envelope"
        );
        // The session named is the parent's: a subagent has no session of its
        // own, and applying the signal to the id would find no row.
        assert_eq!(event.session_id, "parent-1");
    }

    /// Every other event has no subagent to report, and must not claim one —
    /// an empty id would match nothing and a stray one would retire a subagent
    /// that is still working.
    #[test]
    fn only_a_subagent_stop_reports_a_finished_subagent() {
        for name in ["Stop", "PreToolUse", "Notification"] {
            let raw = format!(r#"{{"session_id":"s","hook_event_name":"{name}"}}"#);
            let line = envelope("", raw.as_bytes()).expect("envelope");
            let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
            assert!(event.finished_agent.is_none(), "{name} named a subagent");
        }
    }

    /// The envelope must survive the payload, whatever is in it: the fields are
    /// pulled out of somebody else's JSON and a missing one is normal.
    #[test]
    fn an_event_is_reduced_to_the_session_and_what_happened() {
        let raw = br#"{"session_id":"abc","cwd":"/x","hook_event_name":"Stop","extra":{"a":1}}"#;
        let line = envelope("", raw).expect("envelope");
        assert!(line.ends_with(b"\n"), "the wire is newline delimited");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.session_id, "abc");
        assert_eq!(event.reported.cwd, "/x");
        assert_eq!(event.reported.signal, Signal::Idle);

        // The argument wins, for a harness whose payload names events its way.
        let line = envelope("Notification", raw).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.signal, Signal::NeedsInput);

        // Junk in, nothing out — never a panic, and never a bogus event.
        assert!(envelope("Stop", b"not json").is_none());
        assert!(parse("not json").is_none());
        assert!(
            parse(r#"{"event":"Stop"}"#).is_none(),
            "no session to apply to"
        );
        assert!(
            parse(r#"{"event":"SomethingNew","session_id":"a"}"#).is_none(),
            "an unknown event must be dropped, not guessed at"
        );
    }

    /// Codex names the same three things differently and puts them in argv.
    /// Recorded from a real `codex exec` run against a probe program.
    #[test]
    fn a_codex_turn_lands_in_the_same_bin_as_a_claude_stop() {
        let raw = br#"{"type":"agent-turn-complete","thread-id":"019fda22-5315-7580-84de-033e4f6835b5","turn-id":"019fda22-6995-7c40-bf6b-aaf54b274444","cwd":"/home/flo/cctop","client":"codex_exec","input-messages":["hi"],"last-assistant-message":"ok"}"#;
        let line = envelope("", raw).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.session_id, "019fda22-5315-7580-84de-033e4f6835b5");
        assert_eq!(event.reported.cwd, "/home/flo/cctop");
        assert_eq!(
            event.reported.signal,
            Signal::Idle,
            "a finished Codex turn is the same fact as a Claude Stop"
        );
    }

    /// The lifecycle events are the ones that change which rows exist, and the
    /// table has to be told to go and look rather than wait for its poll.
    #[test]
    fn only_the_lifecycle_events_ask_for_a_rescan() {
        assert!(signal_of("SessionStart").unwrap().is_lifecycle());
        assert!(signal_of("SessionEnd").unwrap().is_lifecycle());
        assert!(!signal_of("PreToolUse").unwrap().is_lifecycle());
        assert!(!signal_of("Stop").unwrap().is_lifecycle());
        // Compaction is work, not a stalled agent, and a finished subagent means
        // the one that spawned it is still going.
        assert!(signal_of("PreCompact").unwrap().is_working());
        assert!(signal_of("SubagentStop").unwrap().is_working());
        assert!(!signal_of("Stop").unwrap().is_working());
    }

    /// The settings file belongs to the user and their other tools. Installing
    /// must not disturb a hook cctop did not write, and removing must put the
    /// file back exactly as it was found.
    #[test]
    fn installing_leaves_another_tools_hooks_exactly_as_they_were() {
        let dir = scratch("hooks");
        let scope = Scope::Project(dir.clone());
        let path = scope.settings_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = serde_json::json!({
            "model": "opus",
            "hooks": {
                "SessionStart": [{"hooks": [{"type": "command", "command": "/theirs.mjs"}]}]
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&theirs).unwrap()).unwrap();

        install(&scope).unwrap();
        let after = read_settings(&path).unwrap();
        assert_eq!(after["model"], "opus", "an unrelated setting was disturbed");
        assert_eq!(
            after["hooks"]["SessionStart"][0], theirs["hooks"]["SessionStart"][0],
            "another tool's hook was disturbed"
        );
        assert_eq!(
            after["hooks"]["SessionStart"].as_array().unwrap().len(),
            2,
            "cctop's own SessionStart hook was not added alongside it"
        );
        assert_eq!(after["hooks"]["Stop"].as_array().unwrap().len(), 1);

        // Twice is once: an installer that doubled up would fire twice a turn.
        install(&scope).unwrap();
        assert_eq!(
            read_settings(&path).unwrap()["hooks"]["Stop"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(scope_status(scope.clone()).health, Health::Installed);

        remove(&scope).unwrap();
        assert_eq!(read_settings(&path).unwrap(), *theirs.as_object().unwrap());
        assert_eq!(scope_status(scope).health, Health::Absent);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An install from an older cctop registers fewer events than this one
    /// wants. That is not "installed" — the events it does not know about are
    /// silently never delivered — so it has to be visible.
    #[test]
    fn an_install_missing_newer_events_reads_as_partial() {
        let dir = scratch("partial");
        let scope = Scope::Project(dir.clone());
        let path = scope.settings_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let exe = own_exe().unwrap();
        std::fs::write(
            &path,
            serde_json::json!({
                "hooks": {"Stop": [{"hooks": [{"type": "command", "command": format!("{exe} hook Stop")}]}]}
            })
            .to_string(),
        )
        .unwrap();

        match scope_status(scope.clone()).health {
            Health::Partial(missing) => {
                assert!(missing.contains(&"SessionEnd"));
                assert!(!missing.contains(&"Stop"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }

        // And installing over it fills the gap without doubling `Stop`.
        install(&scope).unwrap();
        assert_eq!(scope_status(scope).health, Health::Installed);
        assert_eq!(
            read_settings(&path).unwrap()["hooks"]["Stop"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A hook naming a cctop that no longer exists fires nothing at all, and
    /// nothing is lost by repointing it. One that names a different cctop that
    /// *does* exist is somebody's second install, and must be left alone.
    #[test]
    fn a_hook_pointing_at_a_deleted_binary_is_repaired_but_a_live_one_is_not() {
        let dir = scratch("repair");
        let scope = Scope::Project(dir.clone());
        let path = scope.settings_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let gone = dir.join("moved-away-cctop");
        let write_pointing_at = |exe: &Path| {
            let hooks: serde_json::Map<String, serde_json::Value> = WANTED
                .iter()
                .map(|e| {
                    (
                        (*e).to_string(),
                        serde_json::json!([{"hooks": [{"type": "command",
                            "command": format!("{} hook {e}", exe.display())}]}]),
                    )
                })
                .collect();
            std::fs::write(&path, serde_json::json!({"hooks": hooks}).to_string()).unwrap();
        };

        write_pointing_at(&gone);
        assert_eq!(
            scope_status(scope.clone()).health,
            Health::Broken(gone.display().to_string())
        );
        assert!(
            !repair(Some(&dir)).is_empty(),
            "a dead path was not repaired"
        );
        assert_eq!(scope_status(scope.clone()).health, Health::Installed);

        // A path that exists is another install, whoever made it.
        let other = dir.join("other-cctop");
        std::fs::write(&other, "").unwrap();
        write_pointing_at(&other);
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(
            repair(Some(&dir)).is_empty(),
            "somebody else's live install was taken over"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Recognising our own entries has to survive the binary being called
    /// something other than exactly `cctop`.
    ///
    /// Regression. Matching the literal `cctop hook` looked equivalent and was
    /// not: a versioned or renamed binary produced entries the installer could
    /// no longer see, so installing twice registered every hook twice and
    /// removing left the lot behind. Cargo's own test binary is named that way,
    /// which is how this surfaced.
    #[test]
    fn an_entry_is_recognised_however_the_binary_is_named() {
        for command in [
            "/usr/local/bin/cctop hook Stop",
            "/home/me/.cargo/bin/cctop-0.1.12 hook PreToolUse",
            "/target/debug/deps/cctop-9f2c1a hook SessionEnd",
            // A path with spaces in it: the path ends at the last `hook`, not
            // at the first space.
            "/Applications/My Tools/cctop hook Stop",
        ] {
            assert!(is_our_command(command), "{command} was not recognised");
        }
        assert_eq!(
            recorded_exe("/Applications/My Tools/cctop hook Stop"),
            Some("/Applications/My Tools/cctop")
        );

        for command in [
            // Somebody else's program, whatever it is doing.
            "/usr/bin/something-else Stop",
            "/opt/theirs/notify hook Stop",
            // A `hook` that is not the installer's: no event, or a whole
            // command line after it.
            "/usr/local/bin/cctop hook ",
            "/usr/local/bin/cctop hook Stop && rm -rf /",
            // The word in a directory name rather than the program's.
            "/home/me/cctop/scripts/theirs.sh hook Stop",
        ] {
            assert!(!is_our_command(command), "{command} was claimed as ours");
            assert_eq!(recorded_exe(command), None);
        }
    }

    /// A settings file cctop cannot parse must be refused, not rewritten — the
    /// alternative is destroying whatever the user actually had in it.
    #[test]
    fn an_unparsable_settings_file_is_refused_rather_than_replaced() {
        let dir = scratch("badjson");
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        assert!(read_settings(&path).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ this is not json"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Codex's config is hand-written and full of comments, so the edit has to
    /// leave everything it did not touch byte for byte.
    #[test]
    fn editing_codexs_config_keeps_the_comments_around_it() {
        let dir = scratch("codex");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "# my settings\nmodel = \"gpt-5.6-terra\"\n\n[tui]\n# keep this\nnotifications = true\n",
        )
        .unwrap();

        let mut doc = read_codex(&path).unwrap();
        let mut array = toml_edit::Array::new();
        for arg in codex_notify("/usr/local/bin/cctop") {
            array.push(arg);
        }
        doc["notify"] = toml_edit::value(array);
        write_codex(&path, &doc).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my settings"), "a comment was lost");
        assert!(text.contains("# keep this"), "a comment was lost");
        assert!(text.contains(r#"notify = ["/usr/local/bin/cctop", "hook", "codex"]"#));

        // And the argv Codex will run is the one `hook` knows how to read.
        let argv = codex_notify_argv(&read_codex(&path).unwrap()).unwrap();
        assert_eq!(argv[1], "hook");
        assert_eq!(argv[2], CODEX_SELECTOR);

        doc = read_codex(&path).unwrap();
        doc.remove("notify");
        write_codex(&path, &doc).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("notify ="));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The guarantee that outranks doing the job: a cctop that is listening but
    /// has stopped accepting must not hold the agent up.
    ///
    /// Regression. `UnixStream::connect` takes no timeout, and connecting to a
    /// socket whose backlog is full blocks until someone accepts — so bounding
    /// only the write left the agent hanging indefinitely on every hook fire.
    /// Measured at over ten seconds before the deadline moved to cover the whole
    /// operation.
    #[cfg(unix)]
    #[test]
    fn a_wedged_cctop_does_not_hold_the_agent_up() {
        use std::os::unix::net::{UnixListener, UnixStream};

        let dir = scratch("wedge");
        let path = dir.join("hooks.sock");
        // Listening, never accepting, with the backlog stuffed — a cctop whose
        // UI thread has stalled looks exactly like this from the outside.
        let listener = UnixListener::bind(&path).expect("bind");
        let _wedge: Vec<UnixStream> = (0..8)
            .filter_map(|_| UnixStream::connect(&path).ok())
            .collect();

        let started = std::time::Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        let target = path.clone();
        std::thread::spawn(move || {
            // The same shape `deliver` uses, against the wedged address.
            if let Ok(mut stream) = UnixStream::connect(&target) {
                let _ = stream.set_write_timeout(Some(DEADLINE));
                let _ = stream.write_all(b"{}\n");
            }
            let _ = tx.send(());
        });
        let finished = rx.recv_timeout(DEADLINE).is_ok();
        let waited = started.elapsed();

        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);

        // Whether the connect blocked or not, the caller is released on time.
        // That release is what `emit` turns into an exit-0, and it is the only
        // property the agent cares about.
        assert!(
            waited < DEADLINE * 2,
            "the agent was held up for {waited:?} (finished={finished})"
        );
    }

    /// With no cctop listening at all — the ordinary case, on every tool call of
    /// every session on a machine where cctop is closed — the hook still
    /// succeeds, silently and promptly.
    #[cfg(unix)]
    #[test]
    fn the_hook_succeeds_when_nothing_is_listening() {
        let started = std::time::Instant::now();
        deliver(b"{\"event\":\"Stop\",\"session_id\":\"a\"}\n");
        assert!(
            started.elapsed() < DEADLINE * 2,
            "the agent was held up for {:?}",
            started.elapsed()
        );
    }

    /// Two cctops on one machine both hear about the same event. Before this,
    /// one bound a shared address and the other was silently deaf for its whole
    /// run.
    #[cfg(unix)]
    #[test]
    fn every_running_cctop_hears_the_same_event() {
        let a = Listener::start().expect("first listener");
        let b = Listener::start().expect("second listener");
        assert!(a.peer_count() >= 1, "the second cctop was not advertised");

        deliver(b"{\"event\":\"Stop\",\"session_id\":\"shared\"}\n");

        // Delivery is a connect and a write on another thread; give it a moment.
        // Filtered by session, because the address directory is the real one and
        // the other tests in this file are delivering into it at the same time.
        let mine = |events: &[Event]| events.iter().any(|e| e.session_id == "shared");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let (mut got_a, mut got_b) = (Vec::new(), Vec::new());
        while std::time::Instant::now() < deadline && !(mine(&got_a) && mine(&got_b)) {
            got_a.extend(a.drain());
            got_b.extend(b.drain());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(mine(&got_a), "the first cctop missed the event");
        assert!(mine(&got_b), "the second cctop missed the event");
    }

    /// A socket left behind by a cctop that died is cleaned up by whichever
    /// hook next finds it dead, so the directory does not grow forever.
    #[cfg(unix)]
    #[test]
    fn a_dead_address_is_cleaned_up_by_the_next_event() {
        let dir = socket_dir().expect("socket dir");
        std::fs::create_dir_all(&dir).unwrap();
        // A plain file with the right extension refuses connections exactly the
        // way an orphaned socket does.
        let stale = dir.join(format!("0-{}-stale.sock", std::process::id()));
        std::fs::write(&stale, "").unwrap();
        assert!(peers(&dir).contains(&stale));

        deliver(b"{\"event\":\"Stop\",\"session_id\":\"a\"}\n");
        assert!(!stale.exists(), "a dead address was left on disk");
    }
}
