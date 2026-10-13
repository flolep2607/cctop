//! The client half: `cctop hook` and `cctop yolo-hook`, which read what the
//! agent sent, wrap it in cctop's envelope and deliver it to every running
//! cctop, under the deadline and exit-0 guarantee the module docs describe.

use super::*;

/// Longest `cctop hook` may take, start to finish, whatever it is doing.
///
/// The agent is blocked for this long on every hook fire, so it buys nothing to
/// be generous: a local socket with a reader attached answers in microseconds
/// (measured at 1–2ms including the process spawn), and anything slower than
/// this is a cctop that cannot keep up, whose events are better dropped.
pub(crate) const DEADLINE: std::time::Duration = std::time::Duration::from_millis(250);

/// Cap on a single event, so a pathological payload cannot be read forever.
/// A hook's input is a small object; a transcript never comes through here,
/// only its path.
pub(super) const MAX_EVENT: u64 = 256 * 1024;

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
pub(crate) fn socket_dir() -> Option<PathBuf> {
    Some(crate::config::runtime_base().join("cctop").join("hooks.d"))
}

/// Where the process trees the agents reported are kept between runs.
///
/// Beside the sockets, and for the same reason they are there: the runtime
/// directory is cleared when the machine reboots, and a pid outlives a cctop but
/// never outlives a boot. Keeping the map anywhere more durable would mean
/// deciding when a recorded pid stopped meaning what it said; keeping it here
/// means the question cannot arise.
fn claims_path() -> Option<PathBuf> {
    socket_dir().map(|d| d.join("agents.json"))
}

/// Remember which processes each session reported running under.
///
/// Written whenever the map changes, which is rare: a session states its tree
/// once and then repeats it. Written through a temporary file so a second cctop
/// reading at the wrong moment sees the old map rather than half of the new one.
///
/// The temporary is unique to the write ([`crate::util::write_atomic`]):
/// this runs on every change to the map, and every cctop on the machine both
/// writes and reads it, so two writers at once is the ordinary case.
pub fn save_claims(claims: &std::collections::HashMap<String, Vec<u32>>) {
    let Some(path) = claims_path() else { return };
    let Ok(json) = serde_json::to_vec(claims) else {
        return;
    };
    // Nothing here is worth reporting: the map is an optimisation over waiting
    // for the next hook event, and a cctop that cannot write it simply waits.
    let _ = crate::util::write_atomic(&path, &json, false);
}

/// What the agents had reported when some cctop last heard from them.
///
/// The reason this is read at all: a session blocked on a question sends
/// nothing until it is answered, so a cctop that started after the question was
/// asked would have no claim for the one row most likely to want a tab
/// blinking — until the person went and answered it, which is the moment the
/// notice stops being useful. A pid whose process is gone, or is no longer that
/// provider's agent, claims nothing when it is resolved, so a stale file costs
/// nothing to carry.
pub fn load_claims() -> std::collections::HashMap<String, Vec<u32>> {
    claims_path()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The addresses to deliver to, oldest name first.
///
/// Sorted so delivery order is stable rather than whatever the directory
/// happens to yield, which makes a truncated fan-out reproducible.
pub(super) fn peers(dir: &Path) -> Vec<PathBuf> {
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
    let fired = event.clone();
    let args = args.to_vec();
    let (tx, rx) = std::sync::mpsc::channel();
    // Its own channel, because the advice is ready before delivery starts and
    // must not be lost to a cctop that is slow to accept.
    let (advice_tx, advice_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // The log line is written here rather than above, and not because it is
        // tidier. `elog` opens its file on the first event, and that is a
        // `create_dir_all`, a `stat` and an `open` — calls a cache directory on
        // a hung NFS or FUSE mount can sit inside for as long as the kernel
        // keeps retrying, on the thread whose return is the agent's tool call.
        // Nothing about the deadline covers work done before the deadline
        // starts, so this is work that has to happen on the abandonable thread
        // to be covered at all. It is opt-in behind `CCTOP_LOG`, which is
        // exactly the thing somebody turns on while a hook misbehaves.
        crate::elog::event("hook", "fire", serde_json::json!({ "name": fired }));
        let _ = std::panic::catch_unwind(|| forward(&args, &advice_tx));
        let _ = tx.send(());
    });
    // Returning exits the process, which takes the thread with it wherever it
    // got to. A dropped event is a cheaper failure than a stalled agent.
    answer(&event, settle(&rx, &advice_rx, DEADLINE));
    0
}

/// Wait for the worker until it finishes or the deadline passes, then take
/// whatever advice it had made by then.
///
/// Nothing it had not: an answer still being worked out at the deadline is no
/// answer, and one made before a delivery that wedged is still good.
///
/// The deadline is a parameter rather than [`DEADLINE`] itself so the test can
/// make the same choice against a shorter wait, and costs milliseconds rather
/// than a quarter of a second per case.
fn settle(
    done: &std::sync::mpsc::Receiver<()>,
    advice: &std::sync::mpsc::Receiver<Option<String>>,
    deadline: std::time::Duration,
) -> Option<String> {
    let _ = done.recv_timeout(deadline);
    advice.try_recv().ok().flatten()
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
///
/// `advice` is the other exception, and is `None` unless the user turned
/// `warn_agents` on — see the module docs. The two never meet: advice is only
/// ever made for a file write, and the no-op only for the end of a turn.
fn answer(event: &str, advice: Option<String>) {
    let Some(line) = advice.or_else(|| answer_for(event).map(str::to_string)) else {
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
///
/// The advice for this fire, if any, is sent on `advice` before delivery
/// begins, so a wedged cctop can cost the agent its events but not its
/// warning.
fn forward(args: &[String], advice: &std::sync::mpsc::Sender<Option<String>>) {
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

    let chain = ancestry();
    let _ = advice.send(crate::advise::consider(&name, &payload, &chain));
    let Some(line) = envelope(&name, &payload, &chain) else {
        return;
    };
    deliver(&line);
}

/// The events [`announce_answer`] sends. Spelled so no harness could send them.
pub(super) const ANSWERED_ALLOW: &str = "cctop.answered.allow";
pub(super) const ANSWERED_DENY: &str = "cctop.answered.deny";

/// Tell every running cctop that `session_id`'s permission prompt was answered
/// from outside its terminal.
///
/// Through the same sockets the agents' hooks use, because the page that
/// answered is served by one process and the rows that show the prompt live in
/// every cctop on the machine — and the answer is exactly the kind of fact a
/// hook event is: said once, by whoever saw it happen.
pub fn announce_answer(session_id: &str, allowed: bool) {
    deliver(&answer_line(session_id, allowed));
}

pub(super) fn answer_line(session_id: &str, allowed: bool) -> Vec<u8> {
    let event = serde_json::json!({
        "event": if allowed { ANSWERED_ALLOW } else { ANSWERED_DENY },
        "session_id": session_id,
    });
    let mut line = event.to_string().into_bytes();
    line.push(b'\n');
    line
}

/// The event `cctop yolo-hook` sends once it has allowed a prompt.
///
/// Its own name rather than [`ANSWERED_ALLOW`], because it carries what was
/// allowed and races the observer's report of the same prompt: see
/// [`Reports::observe`].
pub(super) const YOLO_ALLOWED: &str = "cctop.yolo.allowed";

pub(super) fn yolo_allowed_line(
    session_id: &str,
    agent: Option<&str>,
    ask: Option<&str>,
) -> Vec<u8> {
    let event = serde_json::json!({
        "event": YOLO_ALLOWED,
        "session_id": session_id,
        "agent_id": agent.unwrap_or_default(),
        // Never absent, so the reader can tell this from every other
        // working event: none of those carries an ask. Empty when the
        // prompt named nothing, which is what the observer's report of it
        // carries too.
        "ask": ask.unwrap_or_default(),
    });
    let mut line = event.to_string().into_bytes();
    line.push(b'\n');
    line
}

/// The one thing `cctop yolo-hook` ever prints: Claude Code's
/// `PermissionRequest` answer meaning "allow", and nothing beside it.
///
/// Checked against the schema in Claude Code 2.1.293 itself: the
/// `PermissionRequest` hook-specific output is `{hookEventName, decision}`,
/// where `decision` is `{"behavior": "allow"}` with an optional
/// `updatedInput` and `updatedPermissions`, or a deny. Neither option is
/// used — the tool runs as asked, and nothing is written into the person's
/// permanent rules. No `continue`, no top-level `decision`, no
/// `permissionDecision`: those are other events' fields, and the validator
/// rejects or reinterprets them.
pub const YOLO_ALLOW: &str = r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#;

/// `cctop yolo-hook <Event>` — the one hook that may decide.
///
/// It says two things, each in one place only. For `PermissionRequest`, it
/// answers [`YOLO_ALLOW`] for a permission dialog Claude Code is about to
/// draw in a session YOLO is on for. For `UserPromptExpansion`, it answers
/// `/yolo` typed by the person with a block whose reason is the result, so
/// the switch takes no model turn ([`yolo_typed`]). It prints nothing for
/// anything else. Separate from [`emit`] so that `cctop hook` stays what the
/// module docs promise, byte for byte: an observer that never decides. This
/// keeps every other guarantee `emit` has — exit 0 whatever happens, a panic
/// hook that turns an unwind into the same, all the work on a thread it can
/// abandon under [`DEADLINE`], stdin read to [`MAX_EVENT`] — and adds one
/// more: the only things it can say are yes to a prompt and the answer to
/// `/yolo`.
///
/// # Why it is safe to say yes
///
/// Only for a session someone switched YOLO on for, and only in the process
/// that was running it then: the payload's `session_id` must have an entry in
/// the switch, and the process recorded with it — pid and start time — must
/// be one of this hook's ancestors. Claude Code spawns its hooks, so that is
/// true exactly when the process asking is the one the person meant. See
/// [`crate::yolo`] for what that rules out.
///
/// Silence is the answer everywhere else, because silence is what Claude Code
/// reads as "no opinion": the dialog goes up as it would with no hook at all.
/// No entry, a file that will not parse, a payload that is empty, malformed
/// or about another event, a question with choices (allowing that would skip
/// the question, not answer it), a deadline that passed — all the same
/// nothing. It never denies: a no is the person's to give.
///
/// # Why `PermissionRequest`, not `PreToolUse`
///
/// `PreToolUse` fires on every tool call and its allow skips the person's own
/// `ask` and `deny` rules; `PermissionRequest` fires only when a dialog was
/// about to go up, which is exactly the moment YOLO stands in for a person
/// clicking Allow.
pub fn yolo_hook(args: &[String]) -> i32 {
    std::panic::set_hook(Box::new(|_| std::process::exit(0)));
    let event = args.first().cloned().unwrap_or_default();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    // The verdict on a channel of its own, sent before the bookkeeping
    // starts, so a slow write cannot cost the answer — and an answer not
    // made by the deadline is no answer.
    let (verdict_tx, verdict_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // By the command line's event, and each path checks the payload
        // agrees: an event neither knows is the silence.
        let _ = std::panic::catch_unwind(|| match event.as_str() {
            "PermissionRequest" => yolo_decide(&event, &verdict_tx),
            "UserPromptExpansion" => yolo_typed(&event, &verdict_tx),
            _ => {}
        });
        let _ = done_tx.send(());
    });
    if let Some(answer) = settle(&done_rx, &verdict_rx, DEADLINE) {
        use std::io::Write;
        // `println!` panics on a closed stdout; see [`answer`].
        let _ = writeln!(std::io::stdout(), "{answer}");
    }
    0
}

/// `cctop yolo-hook UserPromptExpansion`: answer `/yolo` typed in Claude Code
/// without letting it expand, so no model turn runs for a switch.
///
/// The second and last thing this command may say, and as narrow as the
/// first: a block, only for Claude Code's own `/yolo` slash command, whose
/// reason is the answer. Claude Code fires `UserPromptExpansion` only for a
/// command the person typed — see [`crate::yolo::typed`] for why that makes it
/// safe to switch YOLO on from here. Every other command, a payload that
/// will not parse, an event that disagrees with the command line, a deadline
/// that passed: nothing printed, and the command expands as it would with no
/// hook at all.
fn yolo_typed(event: &str, verdict: &std::sync::mpsc::Sender<Option<String>>) {
    let mut payload = Vec::new();
    if std::io::stdin()
        .take(MAX_EVENT)
        .read_to_end(&mut payload)
        .is_err()
    {
        return;
    }
    let Some(typed) = typed_yolo(event, &payload) else {
        return;
    };
    crate::elog::event(
        "yolo",
        "typed",
        serde_json::json!({ "session": typed.session, "args": typed.args }),
    );
    let reason = crate::yolo::typed(&typed.session, &typed.args, typed.cwd.as_deref());
    let _ = verdict.send(Some(yolo_block(&reason)));
}

/// A `/yolo` the person typed, read out of a `UserPromptExpansion` payload.
#[derive(Debug, PartialEq, Eq)]
struct TypedYolo {
    session: String,
    args: String,
    cwd: Option<String>,
}

/// The payload, when it is `/yolo` typed in Claude Code; `None` for the
/// silence every other case gets. Apart from the reading and the printing so
/// every refusal can be asserted on without a process.
///
/// Fields checked against Claude Code 2.1.295's own schema for the event:
/// `expansion_type` is `slash_command` or `mcp_prompt`, and `command_name` is
/// the bare name, `yolo`, for a skill in `~/.claude/skills`. An MCP prompt
/// that happens to be called `yolo` is somebody else's.
fn typed_yolo(event: &str, payload: &[u8]) -> Option<TypedYolo> {
    if event != "UserPromptExpansion" {
        return None;
    }
    let body: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let field = |key: &str| body.get(key).and_then(|v| v.as_str());
    if field("hook_event_name") != Some("UserPromptExpansion")
        || field("expansion_type") != Some("slash_command")
        || field("command_name") != Some(YOLO_COMMAND)
    {
        return None;
    }
    let session = field("session_id").filter(|s| !s.trim().is_empty())?;
    Some(TypedYolo {
        session: session.to_string(),
        args: field("command_args").unwrap_or_default().to_string(),
        cwd: field("cwd").map(str::to_string),
    })
}

/// The command `cctop yolo-hook UserPromptExpansion` answers, and the matcher
/// its settings entry carries so Claude Code spawns it for nothing else.
pub(super) const YOLO_COMMAND: &str = "yolo";

/// Claude Code's `UserPromptExpansion` answer: don't expand, show `reason`.
///
/// Checked against 2.1.295's schema: top-level `decision: "block"` and
/// `reason`, which the person sees in place of the command, and the event's
/// own `suppressOriginalPrompt`, which drops the "Original prompt: /yolo …"
/// line under it — the person just typed that. Nothing reaches the model.
fn yolo_block(reason: &str) -> String {
    serde_json::json!({
        "decision": "block",
        "reason": reason,
        "hookSpecificOutput": {
            "hookEventName": "UserPromptExpansion",
            "suppressOriginalPrompt": true,
        },
    })
    .to_string()
}

/// Read the prompt, decide, and — having allowed — tell the cctops.
fn yolo_decide(event: &str, verdict: &std::sync::mpsc::Sender<Option<String>>) {
    crate::elog::event("hook", "yolo-fire", serde_json::json!({ "name": event }));
    let mut payload = Vec::new();
    if std::io::stdin()
        .take(MAX_EVENT)
        .read_to_end(&mut payload)
        .is_err()
    {
        return;
    }
    let chain = ancestry();
    let Some(body) = yolo_verdict(event, &payload, |id| crate::yolo::allows(id, &chain)) else {
        return;
    };
    let _ = verdict.send(Some(YOLO_ALLOW.to_string()));
    let field = |key: &str| {
        body.get(key)
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
    };
    let Some(session) = field("session_id") else {
        return;
    };
    let ask = ask_of(&body);
    // The lasting record before anything that can wait: one append with no
    // lock, so a busy `yolo.lock` below cannot cost it, and after the
    // verdict, so nothing here can cost the answer.
    let mut line = crate::yolo_log::Line::allow(
        session,
        crate::yolo_log::Via::Hook,
        crate::yolo_log::call_of(&body),
        ask.as_deref(),
    );
    line.cwd = field("cwd").map(str::to_string);
    line.harness = Some(crate::pricing::Provider::Claude.as_str().to_string());
    crate::yolo_log::record(&line);
    crate::elog::event(
        "yolo",
        "hook-allowed",
        serde_json::json!({ "session": session, "ask": ask }),
    );
    // The list first: it is the page's record, and the fan-out below may
    // spend what is left of the deadline on a wedged cctop.
    crate::yolo::record_allowed(session, ask.clone());
    deliver(&yolo_allowed_line(
        session,
        field("agent_id"),
        ask.as_deref(),
    ));
}

/// The payload, when it is a permission prompt YOLO answers; `None` for the
/// silence every other case gets. `allows` is the switch: see
/// [`crate::yolo::allows`].
///
/// Apart from the reading and the printing so every refusal can be asserted
/// on without a process.
fn yolo_verdict(
    event: &str,
    payload: &[u8],
    allows: impl FnOnce(&str) -> bool,
) -> Option<serde_json::Value> {
    if event != "PermissionRequest" {
        return None;
    }
    let body: serde_json::Value = serde_json::from_slice(payload).ok()?;
    // The payload must agree with the command line about what this is: a
    // settings file edited by hand to fire this on another event is still
    // not a permission dialog.
    if body.get("hook_event_name").and_then(|v| v.as_str()) != Some("PermissionRequest") {
        return None;
    }
    if is_question(&body) {
        return None;
    }
    let session = body.get("session_id")?.as_str()?;
    allows(session).then_some(body)
}

/// Write one framed event to every cctop that will take it.
fn deliver(line: &[u8]) {
    let Some(dir) = socket_dir() else { return };
    deliver_to(&dir, line);
}

/// [`deliver`] against a named directory.
///
/// Split in two so the connect below can be asserted against a fixture built to
/// be a wedge. The real thing fans out over `$XDG_RUNTIME_DIR`, which on the
/// machine running the tests belongs to whoever is running them, so pointing a
/// test at one of its own peers is the only way to have a peer at all.
fn deliver_to(dir: &Path, line: &[u8]) {
    use std::io::Write;

    let started = std::time::Instant::now();
    let mut reached = 0usize;
    let peers = peers(dir);
    for path in &peers {
        // Whatever is left of the agent's patience. Stopping here rather than
        // starting another connect is what keeps the fan-out from turning one
        // wedged cctop into a slow hook for everyone.
        let left = DEADLINE.saturating_sub(started.elapsed());
        if left.is_zero() {
            break;
        }
        match connect(path, left) {
            Ok(mut stream) => {
                let _ = stream.set_write_timeout(Some(DEADLINE));
                let _ = stream.write_all(line);
                let _ = stream.flush();
                reached += 1;
            }
            Err(Unreachable::Dead) => {
                let _ = std::fs::remove_file(path);
            }
            // A peer that is listening and not answering keeps its address.
            // Something behind it is alive and holding the socket it is still
            // answering on, so unlinking would deafen a live cctop over one
            // slow moment — the difference between dropping an event and
            // dropping every event after it.
            Err(Unreachable::Wedged) => {}
        }
    }
    crate::elog::event(
        "hook",
        "deliver",
        serde_json::json!({
            "peers": peers.len(),
            "reached": reached,
            "ms": started.elapsed().as_millis() as u64,
        }),
    );
}

/// What a peer that did not take the event turned out to be.
enum Unreachable {
    /// Nothing is listening on that address.
    ///
    /// The socket file outlived the process that bound it. Nobody else can be
    /// about to bind this name — an address is stamped with the instant it was
    /// created and never reused — so the process that finds it dead is the one
    /// that can clean it up.
    Dead,
    /// A peer that is there and did not answer in the time left.
    Wedged,
}

/// Reach one peer inside `limit`, or say what it was instead of answering.
///
/// [`std::os::unix::net::UnixStream::connect`] has no timeout, and on a unix
/// socket the wait it can take is not a timeout-shaped thing: a listener that
/// is alive with a full queue and nobody draining it holds the connection in the
/// kernel until it does, for as long as it likes. From outside, that is
/// indistinguishable from a cctop whose own thread has stalled — and `emit` can
/// walk away from it, because returning exits the process and takes the thread
/// with it. [`announce_answer`] cannot: it runs on one of `serve`'s bounded
/// pool of connection threads, and `--tunnel` puts those behind a URL anybody
/// can reach.
///
/// So the connect is issued on a non-blocking socket, which on AF_UNIX turns the
/// wait into an answer: the kernel does not queue a connection it has no room
/// for, it reports `EAGAIN` at once. `poll` is what keeps the one case that
/// could still be in flight honest — waited for, never for longer than the
/// fan-out has left.
fn connect(
    path: &Path,
    limit: std::time::Duration,
) -> Result<std::os::unix::net::UnixStream, Unreachable> {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;

    // A name the kernel could not have taken is not a cctop that died, so it is
    // neither waited for nor cleaned up.
    let bytes = path.as_os_str().as_bytes();
    let room = std::mem::size_of::<libc::sockaddr_un>()
        - std::mem::offset_of!(libc::sockaddr_un, sun_path)
        - 1;
    if bytes.is_empty() || bytes.len() > room || bytes.contains(&0) {
        return Err(Unreachable::Wedged);
    }
    // SAFETY: zeroed is a valid `sockaddr_un` once the family and the path are
    // written, and the length below covers exactly those bytes — plus the NUL
    // `zeroed` left at the end, which is what makes it a C string.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (slot, byte) in addr.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    let len =
        (std::mem::offset_of!(libc::sockaddr_un, sun_path) + bytes.len() + 1) as libc::socklen_t;

    // SAFETY: a fresh descriptor, non-blocking so the connect cannot wait and
    // close-on-exec so nothing this process spawns inherits it. It is owned
    // from here on: every arm below either closes it or hands it to the stream
    // that takes it over.
    let raw = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if raw < 0 {
        return Err(Unreachable::Wedged);
    }

    // SAFETY: `addr` is fully initialised and `len` its true length; the
    // descriptor is this function's and blocking is never requested.
    let asked =
        unsafe { libc::connect(raw, std::ptr::from_ref(&addr).cast::<libc::sockaddr>(), len) };
    if asked != 0 {
        match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EINPROGRESS) => {}
            // The two refusals that mean the file outlived its owner, and only
            // those two: everything else is one peer being strange, which is
            // not evidence that it has gone.
            Some(libc::ENOENT | libc::ECONNREFUSED) => {
                // SAFETY: an open descriptor this function owns, closed once.
                unsafe { libc::close(raw) };
                return Err(Unreachable::Dead);
            }
            _ => {
                // EAGAIN is the queue being full — the wedge itself, reported
                // rather than entered, which is the whole reason for the
                // non-blocking socket.
                // SAFETY: as above.
                unsafe { libc::close(raw) };
                return Err(Unreachable::Wedged);
            }
        }
        let mut waiting = libc::pollfd {
            fd: raw,
            events: libc::POLLOUT,
            revents: 0,
        };
        let millis = i32::try_from(limit.as_millis()).unwrap_or(i32::MAX);
        // SAFETY: one descriptor, owned, with a timeout — `poll` cannot outlive
        // the limit it was handed.
        if unsafe { libc::poll(&mut waiting, 1, millis) } <= 0 {
            // SAFETY: as above.
            unsafe { libc::close(raw) };
            return Err(Unreachable::Wedged);
        }
        // Writable only means the connect finished; SO_ERROR is what says how.
        let mut refused: libc::c_int = 0;
        let mut errlen = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
        // SAFETY: `SO_ERROR` writes one `c_int`, whose length is passed in.
        let err = unsafe {
            libc::getsockopt(
                raw,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                std::ptr::from_mut(&mut refused).cast(),
                &mut errlen,
            )
        };
        if err != 0 || refused != 0 {
            // SAFETY: an open descriptor this function owns, closed once.
            unsafe { libc::close(raw) };
            return Err(
                if refused == libc::ENOENT || refused == libc::ECONNREFUSED {
                    Unreachable::Dead
                } else {
                    Unreachable::Wedged
                },
            );
        }
    }

    // SAFETY: the descriptor is this function's; clearing one flag on a socket
    // it has just connected cannot fail. Without it the write below would take
    // `EAGAIN` rather than the timeout, and a dropped write would look like a
    // delivered event.
    unsafe {
        let flags = libc::fcntl(raw, libc::F_GETFL);
        if flags < 0 || libc::fcntl(raw, libc::F_SETFL, flags & !libc::O_NONBLOCK) < 0 {
            libc::close(raw);
            return Err(Unreachable::Wedged);
        }
    }
    // SAFETY: a connected socket this function owns and nobody else holds, so
    // the stream takes it over exactly once.
    Ok(unsafe { std::os::unix::net::UnixStream::from_raw_fd(raw) })
}

/// How far up the process tree the hook looks for the agent that spawned it.
///
/// The answer is always near the bottom — a shell, sometimes a wrapper or a
/// sandbox — and everything above it is the terminal, the multiplexer and init,
/// which name no agent. Eight covers every harness measured and bounds what the
/// walk can cost when the answer is not there at all.
const MAX_ANCESTRY: usize = 8;

/// The pids between this hook and init, nearest first.
///
/// This is the one fact only `cctop hook` can report, and it is worth the
/// agent's microseconds because the alternative is a guess. An event names the
/// session it is about; cctop's rows name the *process* they are about; nothing
/// else on the machine knows both. An agent launched without `--resume` carries
/// no session id on its command line, so cctop otherwise pairs a directory's
/// processes against its transcripts by recency — which hands a pid to the
/// wrong transcript as soon as two agents share a checkout, and puts the alert
/// on the wrong tab. The hook is a child of the agent, so its own ancestry
/// settles it.
///
/// The whole chain is sent rather than a guess at which link is the agent: the
/// hook cannot tell (each harness stacks its own shells, wrappers and sandboxes
/// in between), while cctop already knows which pids are agent processes and
/// need only intersect the two.
///
/// `cctop yolo` walks the same chain for the same reason: the process it
/// records is the one the YOLO hook will look for among its own ancestors.
pub(crate) fn ancestry() -> Vec<u32> {
    // Read directly rather than through `sysinfo`, which was measured at 4.2ms
    // against 152us for the identical chain. The hook spends the agent's
    // deadline, so a 28x saving on a fact this small is worth reading `/proc`
    // by hand.
    fn parent(pid: u32) -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // `comm` is parenthesised and may itself contain spaces and parens, so
        // the fields after it are only countable from the last `)`.
        stat.rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }
    walk(std::process::id(), parent)
}

/// Follow `parent` up from `pid`, stopping at init, at a cycle, or at
/// [`MAX_ANCESTRY`].
fn walk(pid: u32, mut parent: impl FnMut(u32) -> Option<u32>) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut current = pid;
    while chain.len() < MAX_ANCESTRY {
        // A pid of 0 is the kernel's placeholder for "no parent recorded", and
        // 1 is init, which is nobody's agent.
        let Some(next) = parent(current).filter(|p| *p > 1) else {
            break;
        };
        // A parent chain cannot really loop, but it is read from a table that
        // changes underneath the walk, so an unbounded loop is not worth the
        // risk of assuming it.
        if chain.contains(&next) {
            break;
        }
        chain.push(next);
        current = next;
    }
    chain
}

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
pub(super) fn envelope(name: &str, payload: &[u8], pids: &[u32]) -> Option<Vec<u8>> {
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
        // Every event fired inside a subagent carries it, and it is the id
        // cctop's own subagent transcripts are named after. Cursor spells it
        // `subagent_id`.
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
        // What a permission prompt is asking for, so whoever answers it from
        // somewhere other than the agent's own terminal is not approving blind.
        "ask": ask_of(&body),
        // The same prompt's tool and whole input field, redacted, for the
        // YOLO log when cctop answers it with a key press. Only on a
        // permission prompt, as `ask` is, and absent on everything else.
        "call": is_permission(&body).then(|| crate::yolo_log::call_of(&body)).flatten(),
        // A question with choices, which Allow and Deny must never answer: see
        // [`Reported::question`].
        "question": is_question(&body),
        // The one fact the agent does not state and only this process can: see
        // [`ancestry`]. Absent from any harness whose hook cctop does not
        // spawn, which the reader treats as "no claim" rather than as an empty
        // one.
        "pids": pids,
        // Where the session's work actually is, when `cctop sandbox` launched
        // it: inherited from the launch, and nothing the payload could say.
        // An environment read, so it costs the deadline nothing.
        "sandbox": std::env::var(crate::sandbox::ENV_SANDBOX).unwrap_or_default(),
    });
    let mut line = serde_json::to_vec(&event).ok()?;
    line.push(b'\n');
    Some(line)
}

/// The longest [`ask_of`] summary kept: enough for a command line or a path,
/// and bounded so a tool input the size of a file never rides every event.
pub(crate) const MAX_ASK: usize = 300;

/// One line saying what a `PermissionRequest` wants to do — `Bash: rm -rf
/// build`, `Edit: src/main.rs` — or `None` for every other event.
///
/// Only the permission events, because this is the question a person is being
/// asked to say yes to; every other event is answered by nobody. The one field
/// that says the most is picked per tool shape rather than per tool, since
/// Claude, Codex and the MCP servers behind them name their tools freely but
/// agree on `command`, `file_path`, `url` and `pattern`.
///
/// A harness that writes the question out in words beats anything assembled
/// from a tool name: OpenCode 2 puts the agent's own sentence in `message` for
/// the questions it asks, and `question: <the question>` is a worse line than
/// the question. So `message` is taken whole when it is there.
/// Whether a permission event is really Claude Code's AskUserQuestion: a
/// menu of answers, where the keys Allow and Deny press would pick an option
/// or cancel the question rather than approve anything.
fn is_question(body: &serde_json::Value) -> bool {
    body.get("tool_name").and_then(|v| v.as_str()) == Some("AskUserQuestion")
}

/// Whether an event is a permission prompt, in any harness's spelling.
fn is_permission(body: &serde_json::Value) -> bool {
    matches!(
        body.get("hook_event_name")
            .or_else(|| body.get("type"))
            .and_then(|v| v.as_str()),
        Some("PermissionRequest" | "permission.asked")
    )
}

fn ask_of(body: &serde_json::Value) -> Option<String> {
    if !is_permission(body) {
        return None;
    }
    // A question says what it asks in its own words — the first one, when it
    // asks several — and that is the line worth showing.
    let asked = is_question(body)
        .then(|| {
            body.get("tool_input")?
                .get("questions")?
                .as_array()?
                .first()?
                .get("question")?
                .as_str()
                .map(str::to_string)
        })
        .flatten();
    let said = asked.or_else(|| {
        body.get("message")
            .and_then(|v| v.as_str())
            .map(str::to_string)
    });
    let tool = body
        .get("tool_name")
        .and_then(|v| v.as_str())
        .filter(|t| !t.is_empty());
    let line = match said {
        Some(said) => said,
        None => {
            let tool = tool?;
            let input = body.get("tool_input");
            let detail = ["command", "file_path", "path", "url", "pattern", "query"]
                .iter()
                .find_map(|key| input?.get(key)?.as_str().filter(|v| !v.is_empty()));
            match detail {
                Some(detail) => format!("{tool}: {detail}"),
                None => tool.to_string(),
            }
        }
    };
    // One line, because it is shown on one: a heredoc's body is not what the
    // person needs to see to recognise the command.
    let line: String = line
        .split(['\n', '\r'])
        .next()
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    Some(crate::util::truncate(&line, MAX_ASK))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chain stops where it stops being about an agent: init is nobody's
    /// parent worth reporting, and a table read while it changes must not be
    /// able to spin the walk forever.
    #[test]
    fn the_ancestry_walk_stops_at_init_a_cycle_and_the_cap() {
        let tree = |pid: u32| match pid {
            10 => Some(9),
            9 => Some(1),
            _ => None,
        };
        assert_eq!(walk(10, tree), vec![9]);

        let loops = |pid: u32| Some(if pid == 3 { 2 } else { 3 });
        assert_eq!(walk(2, loops), vec![3, 2]);

        // A chain with no end still costs a bounded number of reads.
        let endless = |pid: u32| Some(pid + 1);
        assert_eq!(walk(100, endless).len(), MAX_ANCESTRY);
    }

    /// An MCP server asking the user something is a held prompt cctop could not
    /// otherwise see.
    ///
    /// It is not a permission dialog, so `PermissionRequest` never fires for
    /// it, and the screen goes still exactly as it does for a tool running long
    /// and quietly — so a session blocked on one read as idle. `Elicitation`
    /// says it outright, and `ElicitationResult` is the answer arriving.
    #[test]
    fn an_ask_user_question_is_a_question_and_says_its_own_words() {
        let body = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "AskUserQuestion",
            "tool_input": { "questions": [
                { "question": "How should the deploy step happen?", "header": "Deploy",
                  "options": [{ "label": "You deploy" }, { "label": "Hold" }] }
            ] },
        });
        assert!(is_question(&body));
        assert_eq!(
            ask_of(&body).as_deref(),
            Some("How should the deploy step happen?")
        );
        let bash = serde_json::json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": { "command": "rm -rf build" },
        });
        assert!(!is_question(&bash));
        assert_eq!(ask_of(&bash).as_deref(), Some("Bash: rm -rf build"));
    }

    /// A permission request carries what it wants to do, on one bounded
    /// line, and nothing else does.
    #[test]
    fn a_permission_request_says_what_it_asks() {
        let ask = |raw: &str| ask_of(&serde_json::from_str(raw).unwrap());
        assert_eq!(
            ask(r#"{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"rm -rf build\necho done"}}"#).as_deref(),
            Some("Bash: rm -rf build")
        );
        assert_eq!(
            ask(r#"{"hook_event_name":"PermissionRequest","tool_name":"Edit","tool_input":{"file_path":"/w/src/main.rs","old_string":"a"}}"#).as_deref(),
            Some("Edit: /w/src/main.rs")
        );
        assert_eq!(
            ask(r#"{"hook_event_name":"PermissionRequest","tool_name":"mcp__x__y","tool_input":{"n":1}}"#).as_deref(),
            Some("mcp__x__y")
        );
        assert_eq!(
            ask(
                r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls"}}"#
            ),
            None
        );
        let long = format!(
            r#"{{"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{{"command":"{}"}}}}"#,
            "x".repeat(2000)
        );
        assert_eq!(ask(&long).unwrap().chars().count(), MAX_ASK);
    }

    /// The line shown beside an Allow button, which is the whole reason to read
    /// the request rather than the screen: it is what the agent asked, in the
    /// agent's words where there are any.
    #[test]
    fn a_question_is_shown_as_the_question_and_a_command_as_the_command() {
        let asked = ask_of(&serde_json::json!({
            "type": "permission.asked",
            "tool_name": "shell",
            "tool_input": { "command": "rm -rf build" },
        }));
        assert_eq!(asked.as_deref(), Some("shell: rm -rf build"));
        // What 2 puts on a request it raised as a question.
        let question = ask_of(&serde_json::json!({
            "type": "permission.asked",
            "tool_name": "question",
            "message": "Which database should this use?",
        }));
        assert_eq!(
            question.as_deref(),
            Some("Which database should this use?"),
            "the agent's own sentence beats an assembled one"
        );
        // A path, and a prompt long enough to need cutting down to one line.
        let path = ask_of(&serde_json::json!({
            "type": "permission.asked",
            "tool_name": "edit",
            "tool_input": { "file_path": "src/main.rs" },
        }));
        assert_eq!(path.as_deref(), Some("edit: src/main.rs"));
        let long = ask_of(&serde_json::json!({
            "type": "permission.asked",
            "tool_name": "shell",
            "tool_input": { "command": format!("echo {}\nsecond line", "x".repeat(MAX_ASK)) },
        }));
        assert_eq!(long.as_ref().map(|l| l.chars().count()), Some(MAX_ASK));
        assert!(!long.unwrap_or_default().contains('\n'));
        // Not a permission event, so no line at all.
        assert!(ask_of(&serde_json::json!({ "type": "session.idle" })).is_none());
    }

    /// The guarantee that outranks doing the job: a cctop that is listening but
    /// has stopped accepting must not hold the agent up.
    ///
    /// Regression. `UnixStream::connect` takes no timeout, and connecting to a
    /// socket whose queue is full blocks until someone accepts — so bounding
    /// only the write left the agent hanging indefinitely on every hook fire.
    /// Measured at over ten seconds before the deadline moved to cover the whole
    /// operation.
    ///
    /// It calls `deliver` itself rather than a copy of its body. The version
    /// before this one re-implemented the connect in the test and asserted the
    /// test's own thread came back, so a `deliver` that lost its bound would
    /// have gone on passing — and the wedge was eight connections against a
    /// backlog `UnixListener::bind` sets to `SOMAXCONN`, which is 4096 on the
    /// machine this was found on. Connection nine had room and the premise was
    /// quietly false.
    #[test]
    fn a_wedged_cctop_does_not_hold_the_agent_up() {
        let dir = scratch("wedge");
        let hooks = dir.join("hooks.d");
        std::fs::create_dir_all(&hooks).unwrap();
        let path = hooks.join("wedged.sock");
        // A cctop whose own thread has stalled looks exactly like this from the
        // outside: bound, listening, and not taking anything.
        let listener = listen_with_backlog(&path, 1);

        // Fill the queue, and know it is full rather than guess. A
        // non-blocking connect to a unix socket whose queue has no room fails
        // with EAGAIN at once, so the fixture asks the kernel instead of
        // reading a quiet second as the answer. Not the hook's own connect: a
        // fixture built with the function under test proves nothing about it.
        let held = crate::test_wait::fill_queue(&path);
        assert!(
            !held.is_empty(),
            "the fixture never reached its own listener"
        );

        // On a thread, because a `deliver` that has lost its bound does not come
        // back — it waits on the queue the filler above is still inside. A
        // test that hangs reports less than one that fails, so the wait here is
        // bounded and the regression arrives with a line number.
        let (done_tx, done) = std::sync::mpsc::channel();
        let target = hooks.clone();
        std::thread::spawn(move || {
            deliver_to(&target, b"{\"event\":\"cctop.answered.allow\"}\n");
            let _ = done_tx.send(());
        });
        let started = std::time::Instant::now();
        // Patience rather than a few deadlines: a `deliver` that lost its
        // bound never comes back while the queue is full, so this only says
        // how long to wait before calling that a hang.
        let returned = done.recv_timeout(crate::test_wait::PATIENCE).is_ok();
        let waited = started.elapsed();

        // The peer is alive and holding the socket it answers on, so the
        // address stays. Dropping it would deafen a live cctop over one slow
        // moment, which is a far worse failure than the one being tested.
        assert!(path.exists(), "the hook deleted a live cctop's socket");

        // SAFETY: the fixture's listener, owned by this test, closed once.
        unsafe { libc::close(listener) };
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);

        // Whether the connect blocked or not, the caller is released on time.
        // That release is what `emit` turns into an exit-0, and it is the only
        // property the agent cares about.
        //
        // Two numbers. The hang detector above waits as long as it likes,
        // because the point of it is to catch a `deliver` that never returns at
        // all. This allows two deadlines, because `deliver_to` may spend up to
        // one by design — it hands the connect whatever is left of `DEADLINE`
        // — so an assertion at exactly `DEADLINE` has no room for the scheduler
        // and reports on the machine rather than on the code. Against a full
        // queue the connect is refused at once, so what it measures is
        // ordinarily near nothing.
        assert!(returned, "`deliver` did not come back at all");
        assert!(
            waited < DEADLINE * 2,
            "the agent was held up for {waited:?}"
        );
    }

    /// A listener bound at `path` listening on `backlog`, as a raw descriptor.
    ///
    /// Hand-rolled because `UnixListener::bind` listens on `SOMAXCONN` and
    /// offers no other backlog. Two connections fill a backlog of one; the third
    /// is the one that finds no room.
    fn listen_with_backlog(path: &Path, backlog: i32) -> std::os::fd::RawFd {
        let (addr, len) = crate::test_wait::sockaddr(path);
        // SAFETY: each call is given a descriptor this test owns and a fully
        // initialised address; the descriptor is returned for the test to close.
        unsafe {
            let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
            assert!(fd >= 0, "socket");
            assert_eq!(
                libc::bind(fd, std::ptr::from_ref(&addr).cast(), len),
                0,
                "bind"
            );
            assert_eq!(libc::listen(fd, backlog), 0, "listen");
            fd
        }
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

    /// The advice is bounded by the same deadline as everything else: made
    /// in time, it survives a delivery that then wedges; still being worked
    /// out at the deadline, it is dropped and the agent hears nothing.
    ///
    /// What is asserted is order, not time: `settle` came back while the
    /// wedged worker was still wedged — its channels still open, its late
    /// advice not yet sent. Asserting `elapsed()` against a budget used to
    /// report on the scheduler rather than on the code, and went red whenever
    /// the rest of the suite ran alongside. A `settle` that ignored its
    /// deadline waits for the worker, finds the channel closed, and fails here.
    #[test]
    fn advice_is_kept_only_if_it_beat_the_deadline() {
        use std::sync::mpsc::{TryRecvError, channel};

        // Short, so the cases below cost milliseconds; the deadline's length
        // is not what is under test.
        let wait = DEADLINE / 10;
        // Far longer than any stall, so "still wedged" cannot come untrue
        // while `settle` is being slow, and short enough that a `settle` which
        // does wait it out fails in seconds rather than hanging the suite.
        let wedged = crate::test_wait::PATIENCE;

        // Advice made, then a delivery that never returns.
        //
        // "Made" is waited for, not assumed: the thread sending it is only
        // spawned here, and a loaded machine can leave it unscheduled past a
        // 25 ms `wait`, which read as advice that missed its deadline — once
        // in 117 runs of the suite with every core busy.
        let (done_tx, done) = channel::<()>();
        let (advice_tx, advice) = channel();
        let (made_tx, made) = channel::<()>();
        std::thread::spawn(move || {
            let _ = advice_tx.send(Some("{}".to_string()));
            let _ = made_tx.send(());
            std::thread::sleep(wedged);
            drop(done_tx);
        });
        made.recv_timeout(wedged)
            .expect("the advice was never made");
        assert_eq!(settle(&done, &advice, wait).as_deref(), Some("{}"));
        assert_eq!(
            done.try_recv(),
            Err(TryRecvError::Empty),
            "settle waited out the wedged delivery"
        );

        // A ledger stuck past the deadline: silence, before it speaks.
        let (done_tx, done) = channel::<()>();
        let (advice_tx, advice) = channel::<Option<String>>();
        std::thread::spawn(move || {
            std::thread::sleep(wedged);
            let _ = advice_tx.send(Some("late".to_string()));
            drop(done_tx);
        });
        assert_eq!(settle(&done, &advice, wait), None);
        assert_eq!(
            advice.try_recv(),
            Err(TryRecvError::Empty),
            "settle waited for the late advice"
        );

        // And a worker that panicked before advising is the ordinary silence.
        let (done_tx, done) = channel::<()>();
        let (advice_tx, advice) = channel::<Option<String>>();
        drop(advice_tx);
        let _ = done_tx.send(());
        assert_eq!(settle(&done, &advice, wait), None);
    }

    /// With no cctop listening at all — the ordinary case, on every tool call of
    /// every session on a machine where cctop is closed — the hook still
    /// succeeds, silently and promptly.
    #[test]
    fn the_hook_succeeds_when_nothing_is_listening() {
        let _dir = crate::config::claim_test_runtime_base("nothing-listening");
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
    #[test]
    fn every_running_cctop_hears_the_same_event() {
        let _dir = crate::config::claim_test_runtime_base("two-cctops");
        let a = Listener::start().expect("first listener");
        let b = Listener::start().expect("second listener");
        assert!(a.peer_count() >= 1, "the second cctop was not advertised");

        deliver(b"{\"event\":\"Stop\",\"session_id\":\"shared\"}\n");

        // Delivery is a connect and a write on another thread; give it a moment.
        // Filtered by session, because the other tests in this file are still
        // delivering into the address directory.
        let mine = |events: &[Event]| events.iter().any(|e| e.session_id == "shared");
        let (mut got_a, mut got_b) = (Vec::new(), Vec::new());
        crate::test_wait::waits_for(|| {
            got_a.extend(a.drain());
            got_b.extend(b.drain());
            mine(&got_a) && mine(&got_b)
        });
        assert!(mine(&got_a), "the first cctop missed the event");
        assert!(mine(&got_b), "the second cctop missed the event");
    }

    /// A restart is the case the file exists for: the map has to come back
    /// whole, because the session it most matters for is the one that has gone
    /// quiet waiting for an answer and will report nothing until it gets one.
    #[test]
    fn a_remembered_process_tree_survives_the_cctop_that_heard_it() {
        let _dir = crate::config::claim_test_runtime_base("claims");
        let mut claims = std::collections::HashMap::new();
        claims.insert("waiting".to_string(), vec![41, 42]);
        save_claims(&claims);
        assert_eq!(load_claims().get("waiting"), Some(&vec![41, 42]));

        // And emptying it is a real state, not a failure to write: every
        // session this cctop had heard from has since ended.
        save_claims(&std::collections::HashMap::new());
        assert!(load_claims().is_empty());
    }

    /// End to end on the real machine: the chain the walk reads is the chain a
    /// listening cctop gets. Asserted against a live process tree rather than a
    /// fixture, because the whole value of the field is that it describes one.
    #[test]
    fn the_tree_a_hook_walks_is_the_tree_cctop_receives() {
        let _dir = crate::config::claim_test_runtime_base("ancestry");
        let listener = Listener::start().expect("listener");
        let walked = ancestry();
        assert!(!walked.is_empty(), "this process has parents");

        let payload = br#"{"session_id":"tree","hook_event_name":"Stop"}"#;
        deliver(&envelope("", payload, &walked).expect("envelope"));

        let mut got = Vec::new();
        let event = crate::test_wait::eventually("the event to arrive", || {
            got.extend(listener.drain());
            got.iter()
                .find(|e: &&Event| e.session_id == "tree")
                .cloned()
        });
        assert_eq!(event.pids, walked);
    }

    /// A socket left behind by a cctop that died is cleaned up by whichever
    /// hook next finds it dead, so the directory does not grow forever.
    #[test]
    fn a_dead_address_is_cleaned_up_by_the_next_event() {
        let _dir = crate::config::claim_test_runtime_base("stale-address");
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

    /// A `PermissionRequest` payload as Claude Code 2.1.293 sends it.
    fn permission_request(session: &str, tool: &str, input: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "session_id": session,
            "transcript_path": "/t.jsonl",
            "cwd": "/w",
            "permission_mode": "default",
            "hook_event_name": "PermissionRequest",
            "tool_name": tool,
            "tool_input": input,
            "permission_suggestions": [],
        }))
        .unwrap()
    }

    /// The deciding hook says yes for the session the switch names and for
    /// nothing else, whatever arrives on its stdin.
    #[test]
    fn the_yolo_hook_allows_only_what_the_switch_allows() {
        let bash = permission_request("on", "Bash", serde_json::json!({"command": "ls"}));
        let yes = |id: &str| id == "on";
        assert!(yolo_verdict("PermissionRequest", &bash, yes).is_some());

        // The switch is asked about the payload's session, and its no is
        // final.
        let mut asked = Vec::new();
        assert!(
            yolo_verdict("PermissionRequest", &bash, |id| {
                asked.push(id.to_string());
                false
            })
            .is_none()
        );
        assert_eq!(asked, ["on"]);
        let other = permission_request("off", "Bash", serde_json::json!({"command": "ls"}));
        assert!(yolo_verdict("PermissionRequest", &other, yes).is_none());

        // Fired for another event, or a payload naming another event.
        for event in ["PreToolUse", "Notification", "", "Elicitation"] {
            assert!(yolo_verdict(event, &bash, yes).is_none(), "{event}");
        }
        let mut renamed: serde_json::Value = serde_json::from_slice(&bash).unwrap();
        renamed["hook_event_name"] = "PreToolUse".into();
        let renamed = serde_json::to_vec(&renamed).unwrap();
        assert!(yolo_verdict("PermissionRequest", &renamed, yes).is_none());

        // A question with choices: allowing it would skip the question.
        let question = permission_request(
            "on",
            "AskUserQuestion",
            serde_json::json!({"questions": [{"question": "Which?"}]}),
        );
        assert!(yolo_verdict("PermissionRequest", &question, yes).is_none());

        // Whatever else stdin can hold.
        let oversized = vec![b'{'; MAX_EVENT as usize];
        for payload in [
            &b""[..],
            b"not json",
            b"{\"session_id\":",
            b"\xff\xfe\x00garbage",
            b"[]",
            b"{\"hook_event_name\":\"PermissionRequest\"}",
            b"{\"hook_event_name\":\"PermissionRequest\",\"session_id\":7}",
            &oversized,
        ] {
            assert!(
                yolo_verdict("PermissionRequest", payload, |_| true).is_none(),
                "{:?}",
                String::from_utf8_lossy(&payload[..payload.len().min(40)])
            );
        }
    }

    /// The answer is exactly the `PermissionRequest` allow Claude Code's
    /// schema takes, with nothing beside it that another event would read as
    /// a decision of its own.
    #[test]
    fn the_yolo_answer_is_the_permission_request_allow_and_nothing_else() {
        let answer: serde_json::Value = serde_json::from_str(YOLO_ALLOW).unwrap();
        assert_eq!(
            answer,
            serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": {"behavior": "allow"},
                }
            })
        );
        assert!(!YOLO_ALLOW.contains('\n'));
        // And the observer is still the observer: no answer to a permission
        // prompt, YOLO or not.
        assert_eq!(answer_for("PermissionRequest"), None);
        for event in CLAUDE_EVENTS {
            assert!(
                answer_for(event).is_none_or(|a| !a.contains("decision")),
                "cctop hook {event} decides"
            );
        }
    }

    /// Only Claude Code's own `/yolo`, typed, is read as one: every other
    /// command, an MCP prompt of the same name, a payload that disagrees with
    /// the command line, and one with no session are the silence.
    #[test]
    fn only_a_typed_yolo_is_answered() {
        let body = |change: &dyn Fn(&mut serde_json::Value)| {
            let mut b = serde_json::json!({
                "session_id": "s1", "cwd": "/w", "hook_event_name": "UserPromptExpansion",
                "expansion_type": "slash_command", "command_name": "yolo",
                "command_args": "off", "command_source": "userSettings", "prompt": "/yolo off",
            });
            change(&mut b);
            serde_json::to_vec(&b).unwrap()
        };
        assert_eq!(
            typed_yolo("UserPromptExpansion", &body(&|_| {})),
            Some(TypedYolo {
                session: "s1".into(),
                args: "off".into(),
                cwd: Some("/w".into()),
            })
        );
        // No args at all is bare `/yolo`, which is on.
        let bare = typed_yolo(
            "UserPromptExpansion",
            &body(&|b| {
                b.as_object_mut().unwrap().shift_remove("command_args");
            }),
        );
        assert_eq!(bare.map(|t| t.args), Some(String::new()));
        assert_eq!(typed_yolo("PermissionRequest", &body(&|_| {})), None);
        assert_eq!(typed_yolo("", &body(&|_| {})), None);
        for change in [
            &(|b: &mut serde_json::Value| b["command_name"] = "deploy".into())
                as &dyn Fn(&mut serde_json::Value),
            &|b| b["command_name"] = "YOLO".into(),
            &|b| b["expansion_type"] = "mcp_prompt".into(),
            &|b| b["hook_event_name"] = "UserPromptSubmit".into(),
            &|b| b["session_id"] = " ".into(),
            &|b| b["session_id"] = 7.into(),
        ] {
            assert_eq!(typed_yolo("UserPromptExpansion", &body(change)), None);
        }
        assert_eq!(typed_yolo("UserPromptExpansion", b"{"), None);
        assert_eq!(typed_yolo("UserPromptExpansion", b""), None);
    }

    /// The answer is the block Claude Code 2.1.295's schema takes for this
    /// event, and nothing else: no `continue`, no `additionalContext` that
    /// would reach the model.
    #[test]
    fn the_typed_answer_is_a_block_and_nothing_else() {
        assert_eq!(
            yolo_block("YOLO on \"here\""),
            r#"{"decision":"block","reason":"YOLO on \"here\"","hookSpecificOutput":{"hookEventName":"UserPromptExpansion","suppressOriginalPrompt":true}}"#
        );
    }
}
