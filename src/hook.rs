//! Events pushed from a live agent into a running cctop.
//!
//! Everything else cctop knows is forensic: it walks transcripts written after
//! the fact and re-derives state from them. That works, but it cannot see the
//! moments that matter most — an agent finishing its turn, an agent blocking on
//! a question, a session beginning or ending — because a transcript records
//! "answered you" and "still thinking" identically, and a session that has
//! ended looks exactly like one that is merely quiet. The agents can say all of
//! it outright: Claude Code and Codex through their hooks, Codex also through
//! its `notify` program.
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
//! 0, writes nothing to stdout that could be read as a decision, and returns
//! promptly. Not by being careful —
//! by construction. Every fallible step discards its error, the whole exchange
//! runs under a deadline on a thread the process is willing to abandon, and a
//! panic hook turns even an unexpected unwind into a silent success. cctop being
//! absent, stopped, or mid-crash is the *ordinary* case, not an error worth
//! reporting.

use std::io::Read;
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
    let event = args.first().cloned().unwrap_or_default();
    let args = args.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(|| forward(&args));
        let _ = tx.send(());
    });
    // Returning exits the process, which takes the thread with it wherever it
    // got to. A dropped event is a cheaper failure than a stalled agent.
    let _ = rx.recv_timeout(DEADLINE);
    answer(&event);
    0
}

/// The one thing `cctop hook` ever writes to stdout, and only where staying
/// quiet is the riskier move.
///
/// Codex documents its `Stop` and `SubagentStop` hooks as expecting JSON on
/// stdout when they exit 0, and plain text as invalid for those two events.
/// Silence is *probably* fine — "exit 0 with no output is treated as success" is
/// the general rule — but a hook that fires at the end of every turn is the last
/// place to be relying on probably: getting it wrong means a failure notice in
/// somebody's session, once a turn, from a monitor.
///
/// So those two get the one answer that cannot mean anything: `continue: true`
/// is the documented default in both Codex and Claude Code, so writing it says
/// exactly what saying nothing was meant to. Every other event still gets
/// silence, because for them stdout is content the model may act on.
fn answer(event: &str) {
    let Some(line) = answer_for(event) else {
        return;
    };
    // `println!` panics on a closed stdout, which would exit 101 — the one exit
    // code that has a meaning to the agent.
    use std::io::Write;
    let _ = writeln!(std::io::stdout(), "{line}");
}

/// What to write for this event, or `None` for the silence every other event
/// gets. Separate from the writing so it can be asserted on.
fn answer_for(event: &str) -> Option<&'static str> {
    match event {
        "Stop" | "SubagentStop" => Some(r#"{"continue": true}"#),
        _ => None,
    }
}

/// Read the event however this agent hands it over, and deliver it.
fn forward(args: &[String]) {
    // Claude Code, Gemini CLI and Cursor all write the event to stdin, and cctop
    // names it on the command line. Codex runs its `notify` program with the
    // JSON as the last argument and nothing on stdin at all, and cctop's
    // OpenCode plugin copies that shape, so those are read differently and then
    // treated the same.
    let (name, payload) = match args.first().map(String::as_str) {
        Some(word) if is_argv_payload(word) => match args.get(1) {
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
    // Both scoped here rather than at the top of the module: writing is what
    // only the unix half does, and an import the Windows build cannot use is a
    // warning, which CI treats as an error.
    use std::io::Write;
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

/// Reduce whatever the agent sent to the few things cctop needs, on one line.
///
/// Newlines inside the agent's JSON are what makes the framing need doing at
/// all. Every harness names the same handful of facts differently, so each is
/// looked for under all the spellings anyone uses rather than branching on which
/// agent this is — the payload does not always say, and a harness that renames a
/// field in a release should degrade to a missing value rather than a wrong one.
///
/// | fact | Claude Code, Gemini CLI, Codex hooks | Codex `notify` | Cursor | OpenCode |
/// |---|---|---|---|---|
/// | event | `hook_event_name` | `type` | `hook_event_name` | `type` |
/// | session | `session_id` | `thread-id` | `session_id`, `conversation_id` | `sessionID` |
/// | directory | `cwd` | `cwd` | `workspace_roots[0]` | `directory` |
///
/// Codex appears twice because it reports twice: its hook framework borrowed
/// Claude Code's spelling wholesale, while the older `notify` program keeps its
/// own.
fn envelope(name: &str, payload: &[u8]) -> Option<Vec<u8>> {
    let body: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let field = |key: &str| body.get(key).and_then(|v| v.as_str()).unwrap_or_default();
    // The first of these keys the payload actually carries.
    let first = |keys: &[&str]| keys.iter().map(|k| field(k)).find(|v| !v.is_empty());
    let event = serde_json::json!({
        // The event name is in the payload, but an installer can also pass it as
        // an argument — taking it from the command line means a harness whose
        // payload spells it differently still lands in the right bin.
        "event": match name.is_empty() {
            true => first(&["hook_event_name", "type"]).unwrap_or_default(),
            false => name,
        },
        "session_id": first(&["session_id", "thread-id", "conversation_id", "sessionID"])
            .unwrap_or_default(),
        // Cursor sends no `cwd` at all: the directory it is working in is the
        // first of its workspace roots, which is an array rather than a string.
        "cwd": first(&["cwd", "directory"]).unwrap_or_else(|| {
            body.get("workspace_roots")
                .and_then(|v| v.as_array())
                .and_then(|roots| roots.first())
                .and_then(|v| v.as_str())
                .unwrap_or_default()
        }),
        // Only a subagent finishing carries one, and it is the id cctop's own
        // subagent transcripts are named after. Cursor spells it `subagent_id`.
        "agent_id": first(&["agent_id", "subagent_id"]).unwrap_or_default(),
        // Which of the many things `Notification` means — see
        // [`notification_signal`]. Absent on every other event, and on a Claude
        // Code old enough not to send it.
        "notification_type": field("notification_type"),
        // How much this session is asking before it acts. On every Claude Code
        // event, and the one fact here that is about the agent's *settings*
        // rather than what it is doing — which is exactly why it is worth
        // carrying: nothing in a transcript says it, so a session running with
        // permissions turned off is otherwise indistinguishable from any other.
        "permission_mode": first(&["permission_mode", "permissionMode"]).unwrap_or_default(),
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
    /// A tool call has begun and has not come back yet.
    ///
    /// Working, like [`Signal::Busy`] — but told apart from it because it is the
    /// only state a *held permission prompt* can be in, and for most harnesses
    /// nothing reports one in time to be useful. Claude Code is the exception:
    /// it raises `PermissionRequest` the instant the prompt goes up, which is
    /// [`Signal::NeedsInput`] outright and needs no inference. Everything else
    /// either raises a `Notification` on a six-second timer (`NOTIFY_HOOKS` in
    /// Claude Code 2.1.x) — by which point you have answered or gone looking
    /// for the tab yourself — or says nothing at all.
    /// See [`Tab::attention`](crate::ui::tabs::Tab::attention).
    Acting,
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
        matches!(
            self,
            Signal::Busy | Signal::Acting | Signal::Compacting | Signal::Started
        )
    }

    /// Whether typing at the agent is an answer to this state, and so cancels
    /// it. A tool call in flight counts: the answer to a permission prompt is
    /// keystrokes, and no event reports that it arrived — see
    /// [`App::mark_answered`](crate::ui::App::mark_answered).
    pub fn awaits_you(self) -> bool {
        matches!(self, Signal::NeedsInput | Signal::Idle | Signal::Acting)
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
            Signal::Acting => "acting",
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
    /// How much the session asks before it acts, when it has said.
    pub permission: Option<Permission>,
    /// When it arrived, which is what bounds how long it is believed. See
    /// [`Reported::is_current`].
    pub at: std::time::Instant,
}

/// How long "the agent is working" is believed with nothing said since.
///
/// A working claim has a shelf life and a waiting one does not, which is the
/// whole asymmetry. An agent that is thinking, or mid tool call, says something
/// again within a turn: the next tool starts, the tool comes back, the turn
/// ends. Silence this long means the event that would have closed it never
/// arrived — the session was killed, the terminal closed, the turn was
/// interrupted — and none of those produce an event of their own. Believing the
/// stale claim forever is how a session nobody is running goes on being drawn as
/// busy, and how a tool call that failed goes on being drawn as a held question.
///
/// Generous on purpose: a long turn of quiet thinking is exactly the case hooks
/// exist to report, and falling back to guessing at it early would undo that.
const WORKING_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

impl Reported {
    /// Whether this can still be true, nothing having been said since.
    ///
    /// A finished turn and a held question are stable facts: they stay true
    /// until the agent says otherwise, however long that takes — somebody away
    /// from their desk for an afternoon should still come back to the tab that
    /// is asking. Only [`Signal::is_working`] expires. See [`WORKING_TTL`].
    pub fn is_current(&self) -> bool {
        !self.signal.is_working() || self.at.elapsed() < WORKING_TTL
    }
}

/// How much a session asks before it acts.
///
/// Only a live agent's own hooks can answer this — it is a setting, not an
/// event, and no transcript records it. That is what makes it worth a column:
/// an agent running with its permission prompts turned off looks exactly like
/// every other agent right up until it does something you would have refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Asks before anything it is not already allowed to do.
    Ask,
    /// Files are written without asking; everything else still asks.
    AcceptEdits,
    /// Reading and planning only — it cannot act at all yet.
    Plan,
    /// Asks about nothing. `--dangerously-skip-permissions`.
    Bypass,
}

impl Permission {
    /// Parse the mode a harness reported, or `None` for one cctop has not seen.
    ///
    /// An unknown mode is deliberately not folded into [`Permission::Ask`]: a
    /// future mode is at least as likely to be a *looser* one, and quietly
    /// drawing it as the safe end is the wrong way to be wrong about this.
    /// The spellings cctop's own [`label`](Permission::label) uses are accepted
    /// too, so a mode that has been through `--json` and back — which is how a
    /// remote machine's rows arrive — reads as the mode it left as.
    pub fn parse(word: &str) -> Option<Permission> {
        match word {
            "default" | "ask" => Some(Permission::Ask),
            "acceptEdits" | "auto" | "edits" => Some(Permission::AcceptEdits),
            "plan" => Some(Permission::Plan),
            "bypassPermissions" | "dontAsk" | "BYPASS" => Some(Permission::Bypass),
            _ => None,
        }
    }

    /// A word for the column, short enough for it.
    pub fn label(self) -> &'static str {
        match self {
            Permission::Ask => "ask",
            Permission::AcceptEdits => "edits",
            Permission::Plan => "plan",
            Permission::Bypass => "BYPASS",
        }
    }

    /// Whether this mode lets the agent act without asking at all — the case
    /// the column exists to make visible.
    pub fn is_unrestricted(self) -> bool {
        matches!(self, Permission::Bypass)
    }
}

/// What a `Notification` is actually about.
///
/// The event is not one thing. Claude Code raises it for a permission prompt,
/// for a login succeeding, for an MCP server answering an elicitation, and for
/// the nudge it sends when a finished turn has sat there for a minute — all
/// down the same hook, told apart only by `notification_type`.
///
/// Reading every one of them as a held question is what makes a session that
/// merely finished go amber and stay amber: `idle_prompt` arrives *after*
/// `Stop`, so the nudge overwrites the truth with something more alarming than
/// it and nothing corrects it until the next turn.
///
/// The default is still [`Signal::NeedsInput`]. An unrecognised type — a newer
/// Claude Code's, or none at all from one too old to send the field — is more
/// likely to be a prompt worth showing than not, and that is the behaviour
/// every version had before this field was read.
fn notification_signal(kind: &str) -> Option<Signal> {
    match kind {
        // The turn ended a minute ago and nobody is blocked: this is the same
        // fact `Stop` already reported, said again more loudly.
        "idle_prompt" => Some(Signal::Idle),
        // An MCP elicitation was answered, which is the answer arriving rather
        // than the question — the agent is moving again.
        "elicitation_response" => Some(Signal::Busy),
        // None of these say anything about whether *this* agent is waiting on
        // you: a login, a computer-use session changing hands, an MCP server
        // acknowledging, or another session finishing. Dropped rather than
        // mapped, so a true state already on the row survives them.
        //
        // `agent_needs_input` and `agent_completed` are the pair that made this
        // list necessary. Both are raised by whichever session has agent view
        // open, *about a background session that is not it* — and the payload
        // carries the watcher's `session_id`, not the waiting one's. Reading
        // either as news about the sender turns the session that is merely
        // watching amber. The session actually blocked reports for itself, over
        // its own hooks, on the row it belongs to.
        "auth_success"
        | "computer_use_enter"
        | "computer_use_exit"
        | "elicitation_complete"
        | "agent_needs_input"
        | "agent_completed"
        | "push_notification" => None,
        // `permission_prompt`, `worker_permission_prompt`, the elicitation
        // dialogs, and whatever comes next: the case the amber exists for.
        _ => Some(Signal::NeedsInput),
    }
}

/// Map an event name onto what it means. Unknown names are dropped rather than
/// guessed at, so a newer agent can add events without confusing this one.
///
/// Every harness's vocabulary lands in the same match. They do not collide —
/// Claude Code and Gemini CLI capitalise their events, Cursor lower-cases its,
/// Codex and OpenCode use hyphens and dots — so one table can hold the lot, and
/// a fact means the same thing to the UI whichever agent reported it.
///
/// `notification` is the `notification_type` of a `Notification`, and empty for
/// every other event.
fn signal_of(event: &str, notification: &str) -> Option<Signal> {
    match event {
        // The turn is over. This is the event the whole feature exists for:
        // nothing in a transcript distinguishes it from the agent still working.
        // Codex's `notify` fires once per turn and says only this; Gemini CLI
        // calls the end of its agent loop `AfterAgent`; Cursor and OpenCode say
        // it in their own spelling.
        // `StopFailure` is the same fact arrived at badly: the turn is over
        // because the API refused it — rate limited, overloaded, out of
        // credit — and the prompt is the user's again either way. Told apart
        // from `Stop` only in that it is the one Claude Code fires instead,
        // never as well, so a turn that dies this way has no other ending.
        "Stop" | "StopFailure" | "agent-turn-complete" | "AfterAgent" | "stop"
        | "session.idle" => Some(Signal::Idle),
        // Several different facts share this event; which one is in the payload.
        "Notification" => notification_signal(notification),
        // All of these mean work has started, which answers whatever came
        // before. `SubagentStop` included: a subagent finishing tells you the
        // agent that spawned it is still going.
        //
        // `PostToolUse` is here for the permission prompt specifically. The
        // sequence is `PreToolUse`, then `Notification` because the tool needs
        // an answer, then — once you give it — the tool runs and this fires.
        // Nothing between the answer and this event says the answer happened,
        // so without it a prompt you have already dealt with keeps its tab
        // blinking until the *next* tool call or the end of the turn.
        // `PostToolUseFailure` sits here for the same reason `PostToolUse`
        // does: a tool that errored is a tool that came back. Claude Code
        // fires one or the other and never both, and erroring is an ordinary
        // way for a call to end — a grep that matched nothing, a test that
        // failed, a command that exited 1 — so leaving this out left a tool
        // call in flight for as long as the turn ran.
        "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" | "SubagentStop"
        // Gemini CLI: a prompt submitted, and a tool call coming back.
        | "BeforeAgent" | "AfterTool"
        // OpenCode: the same, from the plugin.
        | "tool.execute.after"
        // Cursor: the same two moments, lower-cased.
        | "beforeSubmitPrompt" | "subagentStop" => Some(Signal::Busy),
        // A tool call has started, which is the same "working" fact with one
        // more thing known about it: it is the state a permission prompt is
        // held in. See [`Signal::Acting`].
        //
        // Cursor's shell command is the one tool call it reports, which is
        // enough for the cue — and the one harness with nothing to close it out,
        // so there its turn ending is what clears it.
        "PreToolUse" | "BeforeTool" | "beforeShellExecution" | "tool.execute.before" => {
            Some(Signal::Acting)
        }
        // A held permission prompt, in the harnesses that have an event for it.
        // Cursor and Gemini both raise `Notification` the way Claude Code does,
        // and it is matched above.
        //
        // `PermissionRequest` is Claude Code's, and it is the exact fact rather
        // than an inference: it fires the moment the prompt goes up. The
        // `Notification` for the same prompt is on a six-second timer, by which
        // point cctop has usually already guessed it from a tool call held over
        // a still screen — a guess that is wrong for a tool that runs long and
        // silently. See [`Signal::Acting`].
        "permission.asked" | "PermissionRequest" => Some(Signal::NeedsInput),
        // Compaction is the one kind of work worth naming separately: the
        // context panel is about to lurch, and it is not the agent stalling.
        "PreCompact" | "PreCompress" | "preCompact" | "session.compacted" => {
            Some(Signal::Compacting)
        }
        // And the other end of it, where the harness has one: compaction is
        // over and the agent is moving again. Claude Code raises a
        // `SessionStart` after compacting, which says the same thing, so only
        // Codex needs this asked for.
        "PostCompact" => Some(Signal::Busy),
        "SessionStart" | "sessionStart" | "session.created" => Some(Signal::Started),
        "SessionEnd" | "sessionEnd" | "session.deleted" => Some(Signal::Ended),
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
        finished_agent: matches!(
            value.get("event").and_then(|v| v.as_str()),
            Some("SubagentStop" | "subagentStop")
        )
        .then(|| value.get("agent_id").and_then(|v| v.as_str()))
        .flatten()
        .filter(|id| !id.is_empty())
        .map(str::to_string),
        reported: Reported {
            signal: signal_of(
                value.get("event")?.as_str()?,
                value
                    .get("notification_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            )?,
            cwd: value
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            permission: value
                .get("permission_mode")
                .and_then(|v| v.as_str())
                .and_then(Permission::parse),
            at: std::time::Instant::now(),
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
/// see is not worth the fork. The `*ToolUse` pair is the frequent one; the rest
/// fire a handful of times in a session and each answers a question the
/// transcript cannot.
///
/// `PostToolUse` earns the second fork per tool call because it is the only
/// event that follows an answered permission prompt — see [`signal_of`].
///
/// The three that close a state out cost almost nothing, because each is the
/// case its partner does not cover. `PostToolUseFailure` fires *instead of*
/// `PostToolUse` when a tool errors, which is the ordinary end of a failing
/// grep or test run — without it a tool call that failed stays in flight
/// forever, and a tool call in flight over a still screen is how cctop
/// recognises a held permission prompt. `StopFailure` fires instead of `Stop`
/// when the turn ends on an API error, so without it a rate-limited session is
/// drawn as working until somebody types into it. `PermissionRequest` fires at
/// most once per tool call, and only when Claude Code is actually about to ask.
const CLAUDE_EVENTS: &[&str] = &[
    "Stop",
    "StopFailure",
    "Notification",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "SessionStart",
    "SessionEnd",
    "PreCompact",
    "SubagentStop",
];

/// The same list in Gemini CLI's vocabulary.
///
/// It has no `Stop`: the end of a turn is the end of its agent loop, which is
/// `AfterAgent`.
///
/// `AfterTool` used to be left out, on the grounds that `BeforeTool` already
/// says the agent is working. It does not: it says a tool call is *in flight*,
/// which cctop reads as a held permission prompt once the screen goes still,
/// and Gemini has nothing else to say the call came back. Without the pair, a
/// Gemini session spent every tool call looking like it was asking.
const GEMINI_EVENTS: &[&str] = &[
    "BeforeAgent",
    "AfterAgent",
    "BeforeTool",
    "AfterTool",
    "Notification",
    "SessionStart",
    "SessionEnd",
    "PreCompress",
];

/// The same list in Cursor's vocabulary.
///
/// `beforeShellExecution` stands in for a tool call: it is the one Cursor
/// reports that a monitor can do anything with, and skipping `beforeReadFile`
/// and `afterFileEdit` keeps the fan-out down to roughly what the other
/// harnesses cost.
///
/// Cursor also reads Claude Code's `settings.json` for hooks of its own accord,
/// so with both installed each moment arrives twice. That is a wasted process
/// spawn and nothing worse — the second event carries the same fact as the
/// first, and applying it again changes nothing.
const CURSOR_EVENTS: &[&str] = &[
    "stop",
    "beforeSubmitPrompt",
    "beforeShellExecution",
    "sessionStart",
    "sessionEnd",
    "preCompact",
    "subagentStop",
];

/// The same list in Codex's vocabulary.
///
/// Codex grew a hook framework of its own, and it borrowed Claude Code's
/// spelling wholesale — same event names, same nested `hooks` object, same JSON
/// on stdin — so everything that reads a Claude Code event already reads a
/// Codex one. What it does not have is the pair of failure events: Codex's
/// `PostToolUse` fires for a command that exited non-zero as well as one that
/// succeeded, and a turn that dies has no second ending, so nothing here needs
/// to close those out.
///
/// `notify` stays installed alongside. It says only that a turn finished, but it
/// says it without being trusted first, and until somebody has run `/hooks`
/// inside Codex these hooks deliver nothing at all.
const CODEX_EVENTS: &[&str] = &[
    "Stop",
    "UserPromptSubmit",
    "PreToolUse",
    "PermissionRequest",
    "PostToolUse",
    "SessionStart",
    "SessionEnd",
    "PreCompact",
    "PostCompact",
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

/// The same, for the plugin cctop installs into OpenCode.
const OPENCODE_SELECTOR: &str = "opencode";

/// Whether this first argument means "the event is the next argument" rather
/// than naming an event to read from stdin.
fn is_argv_payload(word: &str) -> bool {
    matches!(word, CODEX_SELECTOR | OPENCODE_SELECTOR)
}

/// An agent cctop can ask to report on itself.
///
/// Each one is asked in its own way — three different config files, a `notify`
/// program, and a plugin — but every one of them ends up spawning
/// `cctop hook`, and what comes back is the same [`Event`] whichever it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Gemini,
    Cursor,
    Codex,
    OpenCode,
}

/// Every harness an install touches, in the order they are reported.
pub const HARNESSES: [Harness; 5] = [
    Harness::Claude,
    Harness::Gemini,
    Harness::Cursor,
    Harness::Codex,
    Harness::OpenCode,
];

/// How a harness spells one entry in its `hooks` object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// Claude Code and Gemini CLI: a wrapper holding a list of commands, so a
    /// `matcher` can select which tools an entry applies to.
    Nested,
    /// Cursor: the command entry itself, directly under the event name, in a
    /// file whose root also carries a `version`.
    Flat,
}

/// What an install has to write for one harness in one scope.
enum Config {
    /// A JSON settings file with a `hooks` object, one array per event.
    Json {
        path: PathBuf,
        shape: Shape,
        events: &'static [&'static str],
    },
    /// Codex's single `notify` program, in TOML.
    Notify(PathBuf),
    /// A plugin file cctop owns outright and can therefore write and delete
    /// whole, rather than merging into somebody else's document.
    Plugin(PathBuf),
}

impl Config {
    /// The file this writes.
    fn path(&self) -> &Path {
        match self {
            Config::Json { path, .. } | Config::Notify(path) | Config::Plugin(path) => path,
        }
    }

    /// Write cctop into this one file, and say what happened to it.
    fn install(&self, exe: &str) -> anyhow::Result<String> {
        match self {
            Config::Json {
                path,
                shape,
                events,
            } => {
                json_install(path, *shape, events, exe)?;
                let note = match self.note() {
                    Some(note) => format!(" — {note}"),
                    None => String::new(),
                };
                Ok(format!(
                    "added {} hooks to {}{note}",
                    events.len(),
                    path.display()
                ))
            }
            Config::Notify(path) => {
                notify_install(path, exe)?;
                Ok("notify points at cctop".to_string())
            }
            Config::Plugin(path) => {
                plugin_install(path, exe)?;
                Ok(format!("wrote plugin {}", path.display()))
            }
        }
    }

    /// Take cctop back out of this one file.
    fn remove(&self) -> anyhow::Result<String> {
        match self {
            Config::Json { path, .. } => {
                let removed = json_remove(path)?;
                Ok(format!("removed {removed} hooks from {}", path.display()))
            }
            Config::Notify(path) => notify_remove(path),
            Config::Plugin(path) => {
                let removed = plugin_remove(path)?;
                Ok(match removed {
                    true => format!("removed plugin {}", path.display()),
                    false => "nothing of cctop's installed".to_string(),
                })
            }
        }
    }

    /// What is in this one file right now.
    fn health(&self) -> Health {
        match self {
            Config::Json {
                path,
                shape,
                events,
            } => json_health(path, *shape, events),
            Config::Notify(path) => notify_health(path),
            Config::Plugin(path) => plugin_health(path),
        }
    }

    /// What a reader needs told about this file beyond its state.
    ///
    /// Only Codex has one: its hooks are inert until a person has looked at
    /// them, and an install that reads as done while delivering nothing is
    /// worse than one that says what is left to do.
    fn note(&self) -> Option<&'static str> {
        match self {
            Config::Json { events, .. } if std::ptr::eq(*events, CODEX_EVENTS) => {
                Some("trust them with /hooks inside Codex")
            }
            _ => None,
        }
    }
}

impl Harness {
    /// The name to print. Long enough to be unambiguous in a list of five.
    pub fn label(self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Gemini => "Gemini CLI",
            Harness::Cursor => "Cursor",
            Harness::Codex => "Codex",
            Harness::OpenCode => "OpenCode",
        }
    }

    /// Every file an install writes for this harness at this scope, in the order
    /// they are reported. Empty where the harness has no such scope.
    ///
    /// A list because Codex has two: hooks, which say everything, and `notify`,
    /// which says only that a turn ended but works the moment it is written.
    /// Every other harness has exactly one.
    fn configs(self, scope: &Scope) -> Vec<Config> {
        let project = match scope {
            Scope::User => None,
            Scope::Project(dir) => Some(dir.as_path()),
        };
        match self {
            Harness::Claude => vec![Config::Json {
                // Honours the same `$CLAUDE_CONFIG_DIR` override as the rest of
                // cctop.
                path: match project {
                    None => crate::config::CLAUDE_CONFIG_DIR.join("settings.json"),
                    Some(dir) => dir.join(".claude").join("settings.json"),
                },
                shape: Shape::Nested,
                events: CLAUDE_EVENTS,
            }],
            Harness::Gemini => vec![Config::Json {
                path: match project {
                    None => crate::config::GEMINI_HOME.join("settings.json"),
                    Some(dir) => dir.join(".gemini").join("settings.json"),
                },
                shape: Shape::Nested,
                events: GEMINI_EVENTS,
            }],
            Harness::Cursor => vec![Config::Json {
                path: match project {
                    None => crate::config::CURSOR_HOME.join("hooks.json"),
                    Some(dir) => dir.join(".cursor").join("hooks.json"),
                },
                shape: Shape::Flat,
                events: CURSOR_EVENTS,
            }],
            Harness::Codex => {
                let hooks = Config::Json {
                    path: match project {
                        None => crate::config::CODEX_HOME.join("hooks.json"),
                        Some(dir) => dir.join(".codex").join("hooks.json"),
                    },
                    shape: Shape::Nested,
                    events: CODEX_EVENTS,
                };
                match project {
                    // `notify` is a single machine-wide program, so only the
                    // user scope has one; a project install is hooks alone.
                    Some(_) => vec![hooks],
                    None => vec![
                        hooks,
                        Config::Notify(crate::config::CODEX_HOME.join("config.toml")),
                    ],
                }
            }
            Harness::OpenCode => vec![Config::Plugin(
                match project {
                    None => crate::config::OPENCODE_CONFIG_DIR.clone(),
                    Some(dir) => dir.join(".opencode"),
                }
                .join("plugins")
                .join(PLUGIN_FILE),
            )],
        }
    }

    /// The first file an install for this scope would write.
    ///
    /// For the tests: the report names each file from its own entry.
    #[cfg(test)]
    fn config_file(self, scope: &Scope) -> Option<PathBuf> {
        self.configs(scope)
            .first()
            .map(|config| config.path().to_path_buf())
    }

    /// Ask this harness to report, leaving everything else in its config alone.
    fn install(self, scope: &Scope, exe: &str) -> Vec<String> {
        let configs = self.configs(scope);
        if configs.is_empty() {
            return vec![format!("{}: has no {} scope", self.label(), scope.label())];
        }
        configs
            .iter()
            .map(|config| match config.install(exe) {
                Ok(what) => format!("{}: {what}", self.label()),
                Err(e) => format!("{}: {e}", self.label()),
            })
            .collect()
    }

    /// The state of the first file this harness writes, which is the whole of
    /// it for every harness but Codex.
    ///
    /// For the tests only: everything else wants one entry per file, which is
    /// what [`harness_status`] gives it.
    #[cfg(test)]
    fn health(self, scope: &Scope) -> Option<Health> {
        self.configs(scope).first().map(Config::health)
    }

    /// Take cctop back out, leaving every other entry untouched.
    fn remove(self, scope: &Scope) -> Vec<String> {
        self.configs(scope)
            .iter()
            .map(|config| match config.remove() {
                Ok(what) => format!("{}: {what}", self.label()),
                Err(e) => format!("{}: {e}", self.label()),
            })
            .collect()
    }
}

/// Which settings file an install writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// The per-user config of every harness — every session on this machine.
    User,
    /// One project's checked-in config: sessions started in that directory,
    /// shared with whoever else checks the file out, which is why it is never
    /// the default.
    Project(PathBuf),
}

impl Scope {
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

/// Ask every harness that has this scope to report, and say what happened to
/// each.
///
/// One harness cannot fail the others. A `notify` slot already holding somebody
/// else's program, a settings file that is not valid JSON, an agent that is not
/// installed at all — each of those is one line of the answer, not the end of
/// the run, because they are independent files and half an install is better
/// than none.
pub fn install(scope: &Scope) -> Vec<String> {
    let exe = match own_exe() {
        Ok(exe) => exe,
        Err(e) => return vec![e.to_string()],
    };
    HARNESSES
        .iter()
        .flat_map(|h| h.install(scope, &exe))
        .collect()
}

/// Take cctop back out of every harness, leaving every other entry untouched.
pub fn remove(scope: &Scope) -> Vec<String> {
    HARNESSES
        .iter()
        .flat_map(|h| h.remove(scope))
        .filter(|line| !line.is_empty())
        .collect()
}

/// Add cctop's hooks to one JSON settings file, leaving everything else alone.
///
/// The file is the user's, and by the time cctop sees it their other tools have
/// usually put hooks in it — so this merges into the arrays rather than writing
/// them, and never reorders or reformats what it did not add.
fn json_install(path: &Path, shape: Shape, events: &[&str], exe: &str) -> anyhow::Result<()> {
    let mut root = read_settings(path)?;
    // Cursor versions its hooks file and ignores one without the field. Only
    // written when absent, so a file that already declares a newer version is
    // not quietly downgraded.
    if shape == Shape::Flat {
        root.entry("version")
            .or_insert_with(|| serde_json::json!(1));
    }

    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("`hooks` in {} is not an object", path.display()))?;

    for event in events {
        let list = hooks
            .entry(*event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("`hooks.{event}` is not an array"))?;
        // Idempotent: running the installer twice must not fire twice, and an
        // entry left by an older cctop at a path that has since moved is
        // replaced rather than added to.
        list.retain(|entry| !is_ours(entry));
        let command = serde_json::json!({
            "type": "command",
            "command": format!("{exe} hook {event}"),
        });
        list.push(match shape {
            Shape::Nested => serde_json::json!({ "hooks": [command] }),
            Shape::Flat => command,
        });
    }
    write_settings(path, &root)
}

/// Take cctop's entries out of one JSON settings file, and say how many went.
///
/// Every event is swept, not just the ones this cctop would install: an entry
/// written by an older version that has since dropped an event is still cctop's
/// to clean up, and leaving it behind would keep firing at a monitor that no
/// longer reports it installed.
fn json_remove(path: &Path) -> anyhow::Result<usize> {
    let mut root = read_settings(path)?;
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
        // And the `hooks` object itself, once cctop's were all it held.
        if hooks.is_empty() {
            root.remove("hooks");
        }
    }
    if removed > 0 {
        write_settings(path, &root)?;
    }
    Ok(removed)
}

/// Whether a settings entry is one cctop wrote, in either shape.
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

/// The command lines one entry names, whichever shape it is written in.
///
/// Both are accepted everywhere rather than per harness: the two shapes are
/// told apart by what the entry holds, and an agent that grows the other one —
/// Cursor already reads Claude Code's nested files — needs no second reader.
fn entry_commands(entry: &serde_json::Value) -> impl Iterator<Item = &str> {
    let nested = entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .map(|inner| inner.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|h| h.get("command").and_then(|c| c.as_str()));
    let flat = entry.get("command").and_then(|c| c.as_str());
    nested.chain(flat)
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
/// Codex allows exactly one `notify` program, so unlike the hook arrays this
/// cannot merge: an existing entry that is not ours is left alone and reported,
/// because replacing someone's desktop-notification script with a monitor is not
/// a trade cctop gets to make for them.
fn notify_install(path: &Path, exe: &str) -> anyhow::Result<()> {
    let mut doc = read_codex(path)?;

    if let Some(existing) = codex_notify_argv(&doc)
        && !existing.iter().any(|a| a.contains("cctop"))
    {
        anyhow::bail!(
            "{} already sets notify = {existing:?}; remove it first if you want cctop to have it",
            path.display()
        );
    }

    let mut array = toml_edit::Array::new();
    for arg in codex_notify(exe) {
        array.push(arg);
    }
    doc["notify"] = toml_edit::value(array);
    write_codex(path, &doc)
}

/// Take cctop back out of Codex's `notify`, leaving another tool's alone.
fn notify_remove(path: &Path) -> anyhow::Result<String> {
    let mut doc = read_codex(path)?;
    match codex_notify_argv(&doc) {
        Some(argv) if argv.iter().any(|a| a.contains("cctop")) => {
            doc.remove("notify");
            write_codex(path, &doc)?;
            Ok(format!("removed from notify in {}", path.display()))
        }
        Some(_) => Ok(format!(
            "notify in {} belongs to something else; left alone",
            path.display()
        )),
        None => Ok(format!("was not notifying cctop ({})", path.display())),
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
// OpenCode
// ---------------------------------------------------------------------------

/// The plugin file cctop owns. Named for cctop so that a file cctop did not
/// write is never the one it deletes.
const PLUGIN_FILE: &str = "cctop.ts";

/// The events the OpenCode plugin forwards, in OpenCode's own vocabulary.
///
/// A list rather than a literal inside the plugin text so that the same names
/// can be looked for when reading a plugin file back: a plugin written by an
/// older cctop is short exactly the names added since, which is the same
/// shortfall a stale `settings.json` has and is reported the same way.
const OPENCODE_EVENTS: &[&str] = &[
    "session.idle",
    "session.created",
    "session.deleted",
    "session.compacted",
    "permission.asked",
    "tool.execute.before",
    "tool.execute.after",
];

/// The line the plugin records this binary's path on, and how its own state is
/// read back out.
const PLUGIN_MARKER: &str = "const CCTOP = ";

/// The plugin cctop writes into OpenCode.
///
/// OpenCode has no hook commands to register: extensions are code it loads at
/// startup, so the only way in is a file, and cctop writes the whole of it. That
/// makes this the one integration that runs *inside* the agent's process rather
/// than beside it, which is why every line of the handler is wrapped: a plugin
/// that throws is a plugin that can spoil the session it is watching, and there
/// is no exit code to hide behind here.
///
/// The event is handed to `cctop hook` as one argument, the same way Codex does
/// it, and the child is left to run on its own — nothing waits for it, so the
/// agent's own loop is never blocked on a monitor.
fn plugin_source(exe: &str) -> String {
    let exe = serde_json::Value::String(exe.to_string());
    let reported = serde_json::to_string(OPENCODE_EVENTS).unwrap_or_default();
    format!(
        r#"// Written by cctop, which watches coding agents. Safe to delete: removing
// this file is all it takes to stop reporting.
//
// Reports the moments a transcript cannot show — a turn finishing, a session
// starting or ending — to whatever cctop is running. Every failure is
// swallowed on purpose: this runs inside OpenCode, and a monitor must never be
// the reason a session breaks.
{PLUGIN_MARKER}{exe}

// Only the events cctop has something to say about. Anything else is ignored
// here rather than spawning a process to be dropped at the other end.
const REPORTED = new Set({reported})

export const cctop = async ({{ directory, worktree }}) => {{
  return {{
    event: async ({{ event }}) => {{
      try {{
        const type = event?.type
        if (!type || !REPORTED.has(type)) return
        const props = event.properties ?? {{}}
        const sessionID = props.sessionID ?? props.info?.id ?? props.sessionId
        if (!sessionID) return
        const payload = JSON.stringify({{
          type,
          sessionID,
          directory: directory ?? worktree ?? "",
        }})
        Bun.spawn([CCTOP, "hook", "{OPENCODE_SELECTOR}", payload], {{
          stdin: "ignore",
          stdout: "ignore",
          stderr: "ignore",
        }}).unref()
      }} catch {{
        // A monitor is never worth an exception in somebody else's agent.
      }}
    }},
  }}
}}
"#
    )
}

/// Write the plugin, replacing whatever cctop left there before.
fn plugin_install(path: &Path, exe: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("ts.cctop-tmp");
    std::fs::write(&tmp, plugin_source(exe))?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Delete the plugin, and say whether there was one. A file at that name that
/// cctop did not write is left alone, however unlikely that is.
fn plugin_remove(path: &Path) -> anyhow::Result<bool> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
        Ok(text) if !text.contains(PLUGIN_MARKER) => Ok(false),
        Ok(_) => {
            std::fs::remove_file(path)?;
            Ok(true)
        }
    }
}

/// The cctop a plugin file names, read back out of the line that records it.
fn plugin_exe(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.starts_with(PLUGIN_MARKER))?;
    // Written by `serde_json`, so it is read back the same way rather than by
    // trimming quotes: a path with a backslash or a quote in it survives.
    serde_json::from_str::<String>(line[PLUGIN_MARKER.len()..].trim()).ok()
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// What one JSON settings file has to say about cctop.
fn json_health(path: &Path, shape: Shape, events: &[&'static str]) -> Health {
    let root = match read_settings(path) {
        Err(e) => return Health::Unreadable(e.to_string()),
        Ok(root) => root,
    };
    // Cursor ignores a hooks file with no `version`, so one without it is not
    // installed however many entries it holds.
    if shape == Shape::Flat && root.get("version").is_none() {
        return Health::Absent;
    }
    let hooks = root.get("hooks").and_then(|h| h.as_object());
    let mut missing = Vec::new();
    let mut recorded: Option<String> = None;
    for event in events {
        let entry = hooks
            .and_then(|h| h.get(*event))
            .and_then(|v| v.as_array())
            .and_then(|list| list.iter().find(|e| is_ours(e)));
        match entry {
            None => missing.push(*event),
            Some(entry) => {
                recorded = recorded.or_else(|| {
                    entry_commands(entry)
                        .find_map(recorded_exe)
                        .map(str::to_string)
                })
            }
        }
    }
    verdict(recorded, missing)
}

/// What Codex's config has to say.
fn notify_health(path: &Path) -> Health {
    match read_codex(path) {
        Err(e) => Health::Unreadable(e.to_string()),
        Ok(doc) => match codex_notify_argv(&doc).as_deref() {
            None | Some([]) => Health::Absent,
            // Somebody else's notify program, which cctop reports and leaves.
            Some([exe, ..]) if !exe.contains("cctop") => Health::Other {
                exe: exe.clone(),
                missing: Vec::new(),
            },
            Some([exe, ..]) => verdict(Some(exe.clone()), Vec::new()),
        },
    }
}

/// What the OpenCode plugin file has to say.
fn plugin_health(path: &Path) -> Health {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Health::Absent,
        Err(e) => Health::Unreadable(e.to_string()),
        Ok(text) => match plugin_exe(&text) {
            None => Health::Unreadable(format!("{} is not a cctop plugin", path.display())),
            // A plugin from an older cctop forwards fewer events, and the file
            // says which: the names are in it verbatim. Reported as a shortfall
            // so it is filled in the same way a stale settings file is.
            Some(exe) => verdict(
                Some(exe),
                OPENCODE_EVENTS
                    .iter()
                    .filter(|event| !text.contains(**event))
                    .copied()
                    .collect(),
            ),
        },
    }
}

/// Turn "cctop is recorded here, at this path, missing these events" into the
/// one verdict every harness is reported with.
fn verdict(recorded: Option<String>, missing: Vec<&'static str>) -> Health {
    match recorded {
        None => Health::Absent,
        Some(exe) if !Path::new(&exe).exists() => Health::Broken(exe),
        Some(exe) if Some(exe.as_str()) != own_exe().ok().as_deref() => {
            Health::Other { exe, missing }
        }
        Some(_) if !missing.is_empty() => Health::Partial(missing),
        Some(_) => Health::Installed,
    }
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
    /// on one machine is a choice, not a fault, so the path is reported and
    /// left — but the events are still counted, because "some other cctop
    /// installed this" is no reason for the file to be missing half of them.
    /// That combination used to report only the path, which hid a stale install
    /// behind a line that read as fine.
    Other {
        exe: String,
        missing: Vec<&'static str>,
    },
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
        match self {
            Health::Partial(_) | Health::Broken(_) | Health::Unreadable(_) => true,
            Health::Other { missing, .. } => !missing.is_empty(),
            Health::Absent | Health::Installed => false,
        }
    }

    /// The events this file should register and does not, and the binary they
    /// belong at — this one, unless another cctop already owns the entries.
    fn shortfall(&self) -> Option<(usize, Option<&str>)> {
        match self {
            Health::Partial(missing) => Some((missing.len(), None)),
            Health::Other { exe, missing } if !missing.is_empty() => {
                Some((missing.len(), Some(exe.as_str())))
            }
            _ => None,
        }
    }
}

/// What one of a harness's config files, in one scope, has to say.
#[derive(Debug, Clone)]
pub struct ScopeStatus {
    pub harness: Harness,
    pub scope: Scope,
    pub path: PathBuf,
    pub health: Health,
    /// What a reader needs told beyond the state — Codex's hooks needing to be
    /// trusted before they fire, and nothing else so far.
    pub note: Option<&'static str>,
}

/// The whole integration, in one value the CLI and the UI both render.
#[derive(Debug, Clone)]
pub struct Report {
    /// Every harness in every scope that was looked at, in install order.
    pub entries: Vec<ScopeStatus>,
    /// Whether this cctop is receiving events, and how many others also are.
    pub listening: bool,
    pub peers: usize,
}

/// Inspect every file one harness writes in one scope.
///
/// One entry per file, which for Codex is two: hooks and `notify` are installed
/// and go wrong independently, and one line covering both would have to lie
/// about one of them.
pub fn harness_status(harness: Harness, scope: Scope) -> Vec<ScopeStatus> {
    harness
        .configs(&scope)
        .iter()
        .map(|config| ScopeStatus {
            harness,
            scope: scope.clone(),
            path: config.path().to_path_buf(),
            health: config.health(),
            note: config.note(),
        })
        .collect()
}

/// Whether Codex's hooks are written anywhere that would apply.
///
/// Written, not working: Codex will not run a hook until a person has reviewed
/// and trusted it, and it records that trust against a hash of the hook in a
/// place it does not document — so nothing on disk here can say whether it
/// happened. The caller pairs this with whether Codex has actually reported, and
/// the two together are what distinguish "not set up" from "set up and waiting
/// on you".
pub fn codex_hooks_installed(cwd: Option<&Path>) -> bool {
    let mut scopes = vec![Scope::User];
    if let Some(dir) = cwd {
        scopes.push(Scope::Project(dir.to_path_buf()));
    }
    scopes.iter().any(|scope| {
        Harness::Codex.configs(scope).iter().any(|config| {
            matches!(config, Config::Json { .. }) && config.health() != Health::Absent
        })
    })
}

/// Inspect the whole integration. `cwd` decides which project scope is looked
/// at; `listener` is this instance's, when it has one.
pub fn status(cwd: Option<&Path>, listener: Option<&Listener>) -> Report {
    let mut scopes = vec![Scope::User];
    if let Some(dir) = cwd {
        scopes.push(Scope::Project(dir.to_path_buf()));
    }
    // Harness first, then scope: the answer to "is Claude Code reporting" is
    // both of its lines together, and reading them apart is how a project
    // install gets mistaken for the user one.
    let entries = HARNESSES
        .iter()
        .flat_map(|harness| {
            scopes
                .iter()
                .flat_map(|scope| harness_status(*harness, scope.clone()))
        })
        .collect();

    Report {
        entries,
        listening: listener.is_some(),
        peers: listener.map(Listener::peer_count).unwrap_or(0),
    }
}

impl Report {
    /// The report as lines to print or draw, each tagged with whether it is
    /// something to worry about.
    ///
    /// One line per harness, and a second only where a scope has something to
    /// say. Five agents times two scopes is a wall of "not installed" that
    /// buries the line that matters, and the panel this is drawn in is exactly
    /// as tall as the lines it is handed.
    pub fn lines(&self) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        for harness in HARNESSES {
            let mine: Vec<&ScopeStatus> = self
                .entries
                .iter()
                .filter(|entry| entry.harness == harness)
                .collect();
            if mine.iter().all(|entry| entry.health == Health::Absent) {
                if let Some(entry) = mine.first() {
                    out.push((
                        format!(
                            "{}: not installed ({})",
                            harness.label(),
                            entry.path.display()
                        ),
                        false,
                    ));
                }
                continue;
            }
            // Only the scopes with something to say. A user install and no
            // project one is the ordinary shape, and printing the absence of the
            // second doubles the panel to say nothing.
            for entry in mine.iter().filter(|entry| entry.health != Health::Absent) {
                let (text, bad) = describe(&entry.health);
                // The note rides on the end rather than replacing the state:
                // Codex's hooks really are installed once they are written, and
                // still deliver nothing until a person has trusted them.
                let note = match entry.note {
                    Some(note) if entry.health == Health::Installed => format!(" — {note}"),
                    _ => String::new(),
                };
                out.push((
                    format!(
                        "{} ({}) {}: {text}{note}",
                        harness.label(),
                        entry.scope.label(),
                        entry.path.display()
                    ),
                    bad,
                ));
            }
        }
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
        Health::Other { exe, missing } if missing.is_empty() => {
            (format!("installed, pointing at {exe}"), false)
        }
        Health::Other { exe, missing } => (
            format!(
                "{exe} is installed here, and missing {}",
                missing.join(", ")
            ),
            true,
        ),
        Health::Broken(exe) => (format!("points at {exe}, which is gone"), true),
        Health::Unreadable(why) => (why.clone(), true),
    }
}

/// Fix the two ways an install can look present and deliver less than it
/// should: a recorded cctop that is gone, and a set of events that is short.
///
/// Both are cases where there is no behaviour to preserve and nothing the user
/// could have meant. A hook naming a binary that no longer exists fires
/// nothing. An install written by an older cctop registers the events *that*
/// version knew about, so every event added since is one the agent never
/// mentions — and the states those events close out are exactly the ones that
/// otherwise stick: a tool call that failed goes on reading as a held question,
/// a turn that died on an API error goes on reading as work in progress. That
/// was reported in the panel and nowhere else, so an install could sit half
/// wired for as long as nobody pressed `h`.
///
/// The one thing repair will not do is move an install between binaries. Two
/// cctops on one machine is a choice, and events are delivered to every cctop
/// listening whichever binary fires them — so a shortfall in an install another
/// cctop owns is filled in *at that binary*, leaving its choice alone. Only an
/// install naming a cctop that is gone is repointed, because there is nothing
/// left there to respect.
pub fn repair(cwd: Option<&Path>) -> Vec<String> {
    let mut scopes = vec![Scope::User];
    if let Some(dir) = cwd {
        scopes.push(Scope::Project(dir.to_path_buf()));
    }
    repair_in(&scopes)
}

/// [`repair`] over exactly the scopes named.
///
/// Split out for the tests, which must not be able to reach the machine's own
/// config: repair *writes*, and a test that swept [`Scope::User`] would edit the
/// settings of whoever ran `cargo test`.
fn repair_in(scopes: &[Scope]) -> Vec<String> {
    let Ok(own) = own_exe() else {
        return Vec::new();
    };
    let mut fixed = Vec::new();
    for harness in HARNESSES {
        for scope in scopes {
            for config in harness.configs(scope) {
                let health = config.health();
                // Written whole either way: `install` replaces cctop's entries
                // for every event it wants, so topping up a short install and
                // repointing a dead one are the same write with a different
                // path.
                let (exe, what) = match &health {
                    Health::Broken(_) => (own.clone(), "repointed at this cctop".to_string()),
                    _ => match health.shortfall() {
                        None => continue,
                        Some((count, at)) => (
                            at.unwrap_or(&own).to_string(),
                            format!("filled in {count} missing hooks"),
                        ),
                    },
                };
                if config.install(&exe).is_ok() {
                    fixed.push(format!("{} ({}): {what}", harness.label(), scope.label()));
                }
            }
        }
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

    /// Gemini CLI and Cursor report the same moments as Claude Code, under
    /// their own names and in their own fields. Both payloads are the ones the
    /// agents actually send, and each has to reduce to the same [`Event`].
    #[test]
    fn every_harness_reports_a_finished_turn_the_same_way() {
        // Gemini: identical field names to Claude Code, and the end of its agent
        // loop rather than a `Stop`.
        let raw = br#"{"session_id":"a1b2c3d4-0000-4000-8000-00000000ffff","transcript_path":"/t.jsonl","cwd":"/home/flo/cctop","hook_event_name":"AfterAgent","timestamp":"2026-08-07T00:00:00Z","prompt":"hi","prompt_response":"ok","stop_hook_active":false}"#;
        let event = parse(
            std::str::from_utf8(&envelope("", raw).expect("envelope"))
                .unwrap()
                .trim(),
        )
        .expect("parse");
        assert_eq!(event.session_id, "a1b2c3d4-0000-4000-8000-00000000ffff");
        assert_eq!(event.reported.cwd, "/home/flo/cctop");
        assert_eq!(event.reported.signal, Signal::Idle);

        // Cursor: lower-cased events, and no `cwd` at all — the directory is the
        // first of its workspace roots.
        let raw = br#"{"conversation_id":"c8f2e1a0-1111-4111-8111-111111111111","session_id":"c8f2e1a0-1111-4111-8111-111111111111","hook_event_name":"stop","cursor_version":"2026.06.04","workspace_roots":["/home/flo/cctop"],"status":"completed"}"#;
        let event = parse(
            std::str::from_utf8(&envelope("", raw).expect("envelope"))
                .unwrap()
                .trim(),
        )
        .expect("parse");
        assert_eq!(event.session_id, "c8f2e1a0-1111-4111-8111-111111111111");
        assert_eq!(
            event.reported.cwd, "/home/flo/cctop",
            "a Cursor event carries its directory as a workspace root"
        );
        assert_eq!(event.reported.signal, Signal::Idle);

        // Cursor with only the conversation id, which is what its older payloads
        // carry, and a subagent finishing — the id has to survive either way.
        let raw = br#"{"conversation_id":"c8f2e1a0-1111-4111-8111-111111111111","hook_event_name":"subagentStop","subagent_id":"sub-77","workspace_roots":["/w"]}"#;
        let event = parse(
            std::str::from_utf8(&envelope("", raw).expect("envelope"))
                .unwrap()
                .trim(),
        )
        .expect("parse");
        assert_eq!(event.session_id, "c8f2e1a0-1111-4111-8111-111111111111");
        assert_eq!(event.finished_agent.as_deref(), Some("sub-77"));
        assert_eq!(event.reported.signal, Signal::Busy);

        // OpenCode, whose plugin hands the event over in argv the way Codex does.
        let raw =
            br#"{"type":"session.idle","sessionID":"ses_8a7c","directory":"/home/flo/cctop"}"#;
        let event = parse(
            std::str::from_utf8(&envelope("", raw).expect("envelope"))
                .unwrap()
                .trim(),
        )
        .expect("parse");
        assert_eq!(event.session_id, "ses_8a7c");
        assert_eq!(event.reported.cwd, "/home/flo/cctop");
        assert_eq!(event.reported.signal, Signal::Idle);
    }

    /// Five vocabularies share one table, so the thing that can go wrong is two
    /// harnesses spelling different facts the same way.
    #[test]
    fn no_two_harnesses_disagree_about_a_shared_event_name() {
        let mut seen: Vec<(&str, Signal)> = Vec::new();
        for event in CLAUDE_EVENTS
            .iter()
            .chain(GEMINI_EVENTS)
            .chain(CURSOR_EVENTS)
        {
            let Some(signal) = signal_of(event, "") else {
                panic!("{event} is installed but means nothing to cctop");
            };
            if let Some((_, other)) = seen.iter().find(|(name, _)| name == event) {
                assert_eq!(*other, signal, "{event} means two things");
            }
            seen.push((event, signal));
        }
    }

    /// A permission prompt is the one exchange with no event of its own for the
    /// *answer*, so the tool running afterwards has to carry that news.
    ///
    /// The prompt itself is reported three times over: `PreToolUse` says a tool
    /// is in flight, which is enough for the tab to blink as soon as the screen
    /// goes still; `PermissionRequest` says outright that Claude Code is asking;
    /// and the `Notification` confirms it six seconds later.
    #[test]
    fn an_answered_permission_prompt_stops_asking() {
        assert_eq!(signal_of("PreToolUse", ""), Some(Signal::Acting));
        assert_eq!(signal_of("PermissionRequest", ""), Some(Signal::NeedsInput));
        assert_eq!(signal_of("Notification", ""), Some(Signal::NeedsInput));
        assert_eq!(
            signal_of("PostToolUse", ""),
            Some(Signal::Busy),
            "without this the prompt stays 'asking' until the next tool or the \
             end of the turn"
        );
        for event in ["PermissionRequest", "PostToolUse"] {
            assert!(
                CLAUDE_EVENTS.contains(&event),
                "{event} has to be installed to arrive at all"
            );
        }
    }

    /// The states that stick, and the events that are the only thing that ends
    /// them.
    ///
    /// Regression, and the reason a session could sit on a state it was not in.
    /// Claude Code fires `PostToolUse` only for a tool that *succeeded* and
    /// `Stop` only for a turn that ended *cleanly*, and cctop asked for neither
    /// partner: so a grep that matched nothing left a tool call in flight — read
    /// as a held permission prompt the moment the screen went still — and a
    /// rate-limited turn left the session working with nobody working.
    #[test]
    fn a_tool_that_failed_and_a_turn_that_died_both_end() {
        assert_eq!(
            signal_of("PostToolUseFailure", ""),
            Some(Signal::Busy),
            "a tool that errored is a tool that came back"
        );
        assert_eq!(
            signal_of("StopFailure", ""),
            Some(Signal::Idle),
            "the turn is over, badly, and the prompt is the user's again"
        );
        for event in ["PostToolUseFailure", "StopFailure"] {
            assert!(
                CLAUDE_EVENTS.contains(&event),
                "{event} has to be installed to arrive at all"
            );
        }
    }

    /// A working claim goes stale; a waiting one does not.
    ///
    /// The events that would close out "working" — the tool coming back, the
    /// turn ending — are exactly the ones that never arrive when a session is
    /// killed, interrupted, or has its terminal closed. A held question has no
    /// such partner and no shelf life: somebody back from lunch should still
    /// find the tab that is asking.
    #[test]
    fn a_stale_claim_to_be_working_stops_being_believed() {
        let aged = |signal: Signal, ago: std::time::Duration| Reported {
            signal,
            cwd: "/w".into(),
            permission: None,
            at: std::time::Instant::now() - ago,
        };
        let stale = WORKING_TTL + std::time::Duration::from_secs(1);
        for signal in [
            Signal::Busy,
            Signal::Acting,
            Signal::Compacting,
            Signal::Started,
        ] {
            assert!(aged(signal, std::time::Duration::ZERO).is_current());
            assert!(
                !aged(signal, stale).is_current(),
                "{} was believed after {stale:?} of silence",
                signal.label()
            );
        }
        for signal in [Signal::NeedsInput, Signal::Idle] {
            assert!(
                aged(signal, stale).is_current(),
                "{} expired, and nothing would have said it again",
                signal.label()
            );
        }
    }

    /// `Notification` is several unrelated facts sharing one event, and reading
    /// them all as a held question is what leaves a finished turn amber.
    #[test]
    fn only_a_notification_that_blocks_the_agent_asks_for_you() {
        let notification = |kind: &str| signal_of("Notification", kind);

        // The 60-second nudge, which arrives *after* `Stop` and would otherwise
        // overwrite a finished turn with something more alarming than it is.
        assert_eq!(notification("idle_prompt"), Some(Signal::Idle));
        // A real block, in this session.
        assert_eq!(
            notification("worker_permission_prompt"),
            Some(Signal::NeedsInput)
        );
        assert_eq!(notification("permission_prompt"), Some(Signal::NeedsInput));
        // An elicitation answered is the answer, not the question.
        assert_eq!(notification("elicitation_response"), Some(Signal::Busy));
        // These say nothing about whether the agent is waiting, so they must not
        // be allowed to overwrite what is already known about it.
        // `agent_needs_input` and `agent_completed` are the pair worth naming:
        // both are raised by the session watching a *background* one, and carry
        // the watcher's session id rather than the waiting session's. Read as
        // news about the sender, the first turns a session that is only
        // watching amber — while the session actually blocked reports for
        // itself, on its own row.
        for quiet in [
            "auth_success",
            "computer_use_enter",
            "computer_use_exit",
            "elicitation_complete",
            "agent_needs_input",
            "agent_completed",
            "push_notification",
        ] {
            assert_eq!(notification(quiet), None, "{quiet} claimed a state");
        }
        // A type cctop has never seen, and a Claude Code too old to send one at
        // all, both keep the behaviour every version had before this was read.
        assert_eq!(notification("something_new"), Some(Signal::NeedsInput));
        assert_eq!(notification(""), Some(Signal::NeedsInput));
    }

    /// End to end, from the bytes Claude Code actually writes to the hook.
    #[test]
    fn an_idle_nudge_arrives_as_a_finished_turn() {
        let raw = br#"{"session_id":"s-1","transcript_path":"/t.jsonl","cwd":"/w","hook_event_name":"Notification","message":"Claude is waiting for your input","title":"Claude Code","notification_type":"idle_prompt"}"#;
        let line = envelope("Notification", raw).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.signal, Signal::Idle);
    }

    /// The permission mode rides in on every Claude Code event, and it is the
    /// one fact here that no transcript records.
    #[test]
    fn a_session_reports_how_much_it_asks_before_it_acts() {
        let raw = br#"{"session_id":"s-1","cwd":"/w","hook_event_name":"PreToolUse","permission_mode":"bypassPermissions","tool_name":"Bash"}"#;
        let line = envelope("PreToolUse", raw).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.permission, Some(Permission::Bypass));
        assert!(Permission::Bypass.is_unrestricted());

        // Every spelling seen in the wild, across the payload and transcripts.
        for (word, mode) in [
            ("default", Permission::Ask),
            ("ask", Permission::Ask),
            ("acceptEdits", Permission::AcceptEdits),
            ("auto", Permission::AcceptEdits),
            ("plan", Permission::Plan),
            ("bypassPermissions", Permission::Bypass),
        ] {
            assert_eq!(Permission::parse(word), Some(mode), "{word}");
        }
        // The modes that do ask must never read as unrestricted — this is the
        // whole reason the column is worth drawing.
        for quiet in [Permission::Ask, Permission::AcceptEdits, Permission::Plan] {
            assert!(!quiet.is_unrestricted(), "{quiet:?}");
        }

        // An unknown mode stays unknown rather than being folded into the safe
        // end: a future mode is at least as likely to be a looser one.
        assert_eq!(Permission::parse("somethingNew"), None);
        let raw = br#"{"session_id":"s-2","cwd":"/w","hook_event_name":"Stop"}"#;
        let line = envelope("Stop", raw).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.permission, None, "silence is not a mode");
    }

    /// The lifecycle events are the ones that change which rows exist, and the
    /// table has to be told to go and look rather than wait for its poll.
    #[test]
    fn only_the_lifecycle_events_ask_for_a_rescan() {
        assert!(signal_of("SessionStart", "").unwrap().is_lifecycle());
        assert!(signal_of("SessionEnd", "").unwrap().is_lifecycle());
        assert!(!signal_of("PreToolUse", "").unwrap().is_lifecycle());
        assert!(!signal_of("Stop", "").unwrap().is_lifecycle());
        // Compaction is work, not a stalled agent, and a finished subagent means
        // the one that spawned it is still going.
        assert!(signal_of("PreCompact", "").unwrap().is_working());
        assert!(signal_of("SubagentStop", "").unwrap().is_working());
        assert!(!signal_of("Stop", "").unwrap().is_working());
    }

    /// The settings file belongs to the user and their other tools. Installing
    /// must not disturb a hook cctop did not write, and removing must put the
    /// file back exactly as it was found.
    #[test]
    fn installing_leaves_another_tools_hooks_exactly_as_they_were() {
        let dir = scratch("hooks");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = serde_json::json!({
            "model": "opus",
            "hooks": {
                "SessionStart": [{"hooks": [{"type": "command", "command": "/theirs.mjs"}]}]
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&theirs).unwrap()).unwrap();

        install(&scope);
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
        install(&scope);
        assert_eq!(
            read_settings(&path).unwrap()["hooks"]["Stop"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);

        remove(&scope);
        assert_eq!(read_settings(&path).unwrap(), *theirs.as_object().unwrap());
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Absent);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Each harness is asked in its own dialect, and one install does the lot.
    ///
    /// The shapes are not interchangeable: Cursor ignores a hooks file with no
    /// `version` and does not read Claude Code's nested wrapper in its own file,
    /// and OpenCode has no command to register at all — it loads a plugin. An
    /// install that wrote Claude's shape everywhere would look installed in the
    /// panel and deliver nothing.
    #[test]
    fn every_harness_is_asked_in_its_own_dialect() {
        let dir = scratch("dialects");
        let scope = Scope::Project(dir.clone());
        let done = install(&scope);
        assert_eq!(
            done.len(),
            5,
            "every harness with a project scope should have been written: {done:?}"
        );

        // Claude Code, Gemini CLI and Codex: the nested wrapper, under their own
        // names. Codex borrowed Claude Code's shape outright, which is why one
        // reader serves both — but it is written to its own file, and `notify`
        // is the machine-wide half it does not have here.
        for (harness, event) in [
            (Harness::Claude, "Stop"),
            (Harness::Gemini, "AfterAgent"),
            (Harness::Codex, "PermissionRequest"),
        ] {
            let path = harness.config_file(&scope).unwrap();
            let root = read_settings(&path).unwrap();
            let command = root["hooks"][event][0]["hooks"][0]["command"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            assert!(
                is_our_command(&command),
                "{} wrote {command:?} for {event}",
                harness.label()
            );
            assert!(command.ends_with(event), "the event has to be named");
            assert_eq!(harness.health(&scope), Some(Health::Installed));
        }

        // Cursor: the command entry directly, under a versioned root.
        let path = Harness::Cursor.config_file(&scope).unwrap();
        assert!(path.ends_with(".cursor/hooks.json"));
        let root = read_settings(&path).unwrap();
        assert_eq!(root["version"], 1, "Cursor ignores an unversioned file");
        let command = root["hooks"]["stop"][0]["command"].as_str().unwrap();
        assert!(is_our_command(command), "Cursor got {command:?}");

        // OpenCode: a plugin file, naming this binary.
        let path = Harness::OpenCode.config_file(&scope).unwrap();
        assert!(path.ends_with(".opencode/plugins/cctop.ts"));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(plugin_exe(&text).as_deref(), own_exe().ok().as_deref());
        assert!(
            text.contains(&format!("\"hook\", \"{OPENCODE_SELECTOR}\"")),
            "the plugin has to call the hook the way `hook` reads it"
        );
        for event in OPENCODE_EVENTS {
            assert!(text.contains(event), "the plugin forwards no {event}");
        }
        // A plugin from an older cctop forwards fewer events, and cctop owns the
        // whole file — so it is a shortfall to be filled in rather than
        // something to leave alone. Nothing else notices: the exe line, which is
        // all health used to read, is identical.
        std::fs::write(&path, text.replace(",\"tool.execute.after\"", "")).unwrap();
        assert!(
            Harness::OpenCode.health(&scope).unwrap().is_problem(),
            "a plugin missing an event read as fine"
        );
        assert!(!repair_in(std::slice::from_ref(&scope)).is_empty());
        assert_eq!(
            Harness::OpenCode.health(&scope).unwrap(),
            Health::Installed,
            "the plugin was not rewritten"
        );

        // And every one of them comes back out.
        remove(&scope);
        for harness in HARNESSES {
            assert!(
                matches!(harness.health(&scope), None | Some(Health::Absent)),
                "{} was left behind",
                harness.label()
            );
        }
        assert!(
            !Harness::OpenCode.config_file(&scope).unwrap().exists(),
            "the plugin file was left on disk"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An install from an older cctop registers fewer events than this one
    /// wants. That is not "installed" — the events it does not know about are
    /// silently never delivered — so it has to be visible.
    #[test]
    fn an_install_missing_newer_events_reads_as_partial() {
        let dir = scratch("partial");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
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

        match Harness::Claude.health(&scope).unwrap() {
            Health::Partial(missing) => {
                assert!(missing.contains(&"SessionEnd"));
                assert!(!missing.contains(&"Stop"));
            }
            other => panic!("expected Partial, got {other:?}"),
        }

        // And installing over it fills the gap without doubling `Stop`.
        install(&scope);
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);
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
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let gone = dir.join("moved-away-cctop");
        let write_pointing_at = |exe: &Path| {
            let hooks: serde_json::Map<String, serde_json::Value> = CLAUDE_EVENTS
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
            Harness::Claude.health(&scope).unwrap(),
            Health::Broken(gone.display().to_string())
        );
        assert!(
            !repair_in(std::slice::from_ref(&scope)).is_empty(),
            "a dead path was not repaired"
        );
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);

        // A path that exists is another install, whoever made it.
        let other = dir.join("other-cctop");
        std::fs::write(&other, "").unwrap();
        write_pointing_at(&other);
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(
            repair_in(std::slice::from_ref(&scope)).is_empty(),
            "somebody else's live install was taken over"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An install written by an older cctop is short a few events, and every
    /// event it is short is a state that then sticks. Repair fills it in on the
    /// way up, without moving the install between binaries.
    ///
    /// Regression, and the shape of a real report: hooks installed months ago
    /// registered `PreToolUse` and no `PostToolUse`, so every tool call in every
    /// session stayed in flight — which cctop draws as a held permission prompt.
    /// The panel called it partial and nothing else did, so it went unnoticed
    /// until somebody wondered why the tabs kept blinking. Worse, an install
    /// naming a *different* cctop reported only that path and swallowed the
    /// shortfall entirely.
    #[test]
    fn an_install_short_of_events_is_filled_in_where_it_stands() {
        let dir = scratch("shortfall");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        // Somebody else's live cctop, registering the two events an old version
        // knew and none of the ones that close a state out.
        let theirs = dir.join("their-cctop");
        std::fs::write(&theirs, "").unwrap();
        let hooks: serde_json::Map<String, serde_json::Value> = ["Stop", "PreToolUse"]
            .iter()
            .map(|e| {
                (
                    (*e).to_string(),
                    serde_json::json!([{"hooks": [{"type": "command",
                        "command": format!("{} hook {e}", theirs.display())}]}]),
                )
            })
            .collect();
        std::fs::write(&path, serde_json::json!({"hooks": hooks}).to_string()).unwrap();

        let health = Harness::Claude.health(&scope).unwrap();
        match &health {
            Health::Other { exe, missing } => {
                assert_eq!(exe, &theirs.display().to_string());
                assert!(missing.contains(&"PostToolUse"), "{missing:?}");
            }
            other => panic!("expected Other with a shortfall, got {other:?}"),
        }
        assert!(
            health.is_problem(),
            "a shortfall behind another cctop's path read as fine"
        );

        assert!(
            !repair_in(std::slice::from_ref(&scope)).is_empty(),
            "the shortfall was not filled"
        );
        let health = Harness::Claude.health(&scope).unwrap();
        assert_eq!(
            health,
            Health::Other {
                exe: theirs.display().to_string(),
                missing: Vec::new()
            },
            "the events were filled in, but the install changed hands"
        );
        // And nothing left to do the second time.
        assert!(repair_in(std::slice::from_ref(&scope)).is_empty());
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

    /// Codex reports twice, and the two halves are installed and go wrong
    /// independently: hooks say everything but are inert until a person has
    /// trusted them, and `notify` says only that a turn ended but works the
    /// moment it is written.
    ///
    /// Codex grew its hook framework after cctop had settled for `notify`, and
    /// it borrowed Claude Code's spelling wholesale — so what this mostly checks
    /// is that the reader needed nothing new, and that a report carrying one
    /// line per file tells the truth about both.
    #[test]
    fn codex_is_asked_through_its_hooks_and_its_notify_both() {
        // Only the project scope is written here. `$CODEX_HOME` resolves once
        // per process, so a test that pointed it somewhere else would be
        // gambling on running first — and losing that gamble means editing the
        // config of whoever ran `cargo test`.
        let dir = scratch("codex-hooks");
        let scope = Scope::Project(dir.clone());
        let done = install(&scope);
        let codex: Vec<&String> = done.iter().filter(|l| l.starts_with("Codex:")).collect();
        assert_eq!(codex.len(), 1, "the project scope is hooks alone: {done:?}");
        assert!(
            codex[0].contains("/hooks inside Codex"),
            "an install that reads as done while delivering nothing has to say \
             so: {:?}",
            codex[0]
        );

        // The hooks file is Claude Code's shape, in Codex's own file.
        let hooks = dir.join(".codex").join("hooks.json");
        let root = read_settings(&hooks).unwrap();
        let command = root["hooks"]["PermissionRequest"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap_or_default();
        assert!(is_our_command(command), "Codex got {command:?}");

        let entries = harness_status(Harness::Codex, scope.clone());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].health, Health::Installed);
        assert!(entries[0].note.is_some(), "the trust step went unsaid");

        // The user scope is the one that also has `notify`, which is a single
        // machine-wide program. Only its shape is checked: writing it would mean
        // writing the real `$CODEX_HOME`.
        let both = Harness::Codex.configs(&Scope::User);
        assert_eq!(both.len(), 2, "the user scope is hooks and notify");
        assert!(matches!(both[0], Config::Json { .. }));
        assert!(both[0].path().ends_with("hooks.json"));
        assert!(matches!(both[1], Config::Notify(_)));
        assert!(both[1].path().ends_with("config.toml"));
        assert!(both[1].note().is_none(), "notify needs no trusting");

        // And a Codex hook event needs nothing new to be understood: the payload
        // is Claude Code's, so the same reader takes it.
        let raw = br#"{"session_id":"thr_123","cwd":"/home/flo/cctop","hook_event_name":"PermissionRequest","model":"gpt-5.6","permission_mode":"default","tool_name":"Bash","tool_input":{"command":"rm -rf build"}}"#;
        let line = envelope("PermissionRequest", raw).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.session_id, "thr_123");
        assert_eq!(event.reported.signal, Signal::NeedsInput);
        assert_eq!(event.reported.permission, Some(Permission::Ask));

        remove(&scope);
        for entry in harness_status(Harness::Codex, scope) {
            assert_eq!(
                entry.health,
                Health::Absent,
                "{} was left",
                entry.path.display()
            );
        }
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
        use std::io::Write;

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

    /// The only stdout `cctop hook` ever writes, and the reason it writes it.
    ///
    /// Codex reads its `Stop` and `SubagentStop` hooks as expecting JSON when
    /// they exit 0 and treats plain text there as invalid, so those two answer
    /// with the one thing that cannot mean anything — `continue: true` is the
    /// documented default in both Codex and Claude Code. Every other event stays
    /// silent, because for them stdout is content the model may act on.
    #[test]
    fn only_the_two_events_that_demand_an_answer_get_one() {
        for event in ["Stop", "SubagentStop"] {
            let line = answer_for(event).unwrap_or_default();
            let value: serde_json::Value = serde_json::from_str(line).expect("must be JSON");
            assert_eq!(value["continue"], serde_json::json!(true));
            assert_eq!(
                value.as_object().map(serde_json::Map::len),
                Some(1),
                "a decision snuck into the no-op"
            );
        }
        for quiet in [
            "PreToolUse",
            "PostToolUse",
            "PermissionRequest",
            "UserPromptSubmit",
            "SessionStart",
            "SessionEnd",
            "Notification",
            CODEX_SELECTOR,
            "",
        ] {
            assert_eq!(answer_for(quiet), None, "{quiet} wrote to stdout");
        }
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
