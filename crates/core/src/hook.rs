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
//!
//! # The one answer that is not silence
//!
//! Two events answer on stdout at all. `Stop` and `SubagentStop` get the
//! documented no-op `{"continue": true}` (see [`answer`]). And, only with
//! `[settings] warn_agents = true`, a file write another live session has
//! recently made to the same file gets the harness's *context* channel —
//! `hookSpecificOutput.additionalContext`, and nothing beside it. That is the
//! one place stdout carries content, and it is shaped so it cannot be a
//! decision: no `permissionDecision`, no `decision`, no `continue`, so the
//! harness proceeds exactly as it would have with no output. It is computed on
//! the same abandonable thread under the same deadline, and any failure on the
//! way — a ledger that will not parse, a lock another hook holds, a deadline
//! that runs out — is the ordinary silence. [`crate::advise`] has the rest.
//!
//! # The one hook that decides, and why it is not this one
//!
//! YOLO answers Claude Code's permission dialog with a decision, and that
//! decision does not come from `cctop hook`. It comes from a second command,
//! `cctop yolo-hook` ([`yolo_hook`]), installed as its own entry for
//! `PermissionRequest` only, so that nothing about this command — its
//! arguments, its settings, the state of any file — can make it decide. That
//! one prints the allow for a session YOLO is on for, in the process it was
//! switched on in, and is otherwise exactly as silent as this one, under the
//! same deadline and the same exit-0 guarantee. [`crate::yolo`] has the
//! matching rule.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

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
pub fn save_claims(claims: &std::collections::HashMap<String, Vec<u32>>) {
    let Some(path) = claims_path() else { return };
    let Some(dir) = path.parent() else { return };
    let _ = std::fs::create_dir_all(dir);
    let Ok(json) = serde_json::to_vec(claims) else {
        return;
    };
    // Nothing here is worth reporting: the map is an optimisation over waiting
    // for the next hook event, and a cctop that cannot write it simply waits.
    let tmp = temp_beside(&path, "json");
    if std::fs::write(&tmp, &json).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// A temporary name beside `path` that no other writer can already be holding.
///
/// `save_claims` runs on every change to the claims map — many times a minute —
/// and every cctop on the machine reads the file it produces, so two writers
/// arriving at one fixed temporary name is not a rare interleaving but the
/// ordinary case. They `File::create` the same inode, and whichever renames
/// first publishes a file the other is still writing into: what lands is
/// neither map, `load_claims` finds JSON it cannot read, and sessions go back
/// to pairing their processes against transcripts by recency.
///
/// The pid separates processes, which is the direction that matters here since
/// each cctop is its own writer; the counter separates threads inside one,
/// which a pid alone would call the same writer.
fn temp_beside(path: &Path, extension: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let seq = WRITES.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!("{extension}.{}.{seq}", std::process::id()))
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
const ANSWERED_ALLOW: &str = "cctop.answered.allow";
const ANSWERED_DENY: &str = "cctop.answered.deny";

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

fn answer_line(session_id: &str, allowed: bool) -> Vec<u8> {
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
const YOLO_ALLOWED: &str = "cctop.yolo.allowed";

fn yolo_allowed_line(session_id: &str, agent: Option<&str>, ask: Option<&str>) -> Vec<u8> {
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

/// `cctop yolo-hook PermissionRequest` — the one hook that may decide.
///
/// It answers [`YOLO_ALLOW`] for a permission dialog Claude Code is about to
/// draw in a session YOLO is on for, and prints nothing for anything else.
/// Separate from [`emit`] so that `cctop hook` stays what the module docs
/// promise, byte for byte: an observer that never decides. This keeps every
/// other guarantee `emit` has — exit 0 whatever happens, a panic hook that
/// turns an unwind into the same, all the work on a thread it can abandon
/// under [`DEADLINE`], stdin read to [`MAX_EVENT`] — and adds one more: the
/// only thing it can say is yes.
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
        let _ = std::panic::catch_unwind(|| yolo_decide(&event, &verdict_tx));
        let _ = done_tx.send(());
    });
    if settle(&done_rx, &verdict_rx, DEADLINE).is_some() {
        use std::io::Write;
        // `println!` panics on a closed stdout; see [`answer`].
        let _ = writeln!(std::io::stdout(), "{YOLO_ALLOW}");
    }
    0
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
fn envelope(name: &str, payload: &[u8], pids: &[u32]) -> Option<Vec<u8>> {
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
    Some(match line.chars().count() > MAX_ASK {
        true => line.chars().take(MAX_ASK - 1).chain(['…']).collect(),
        false => line,
    })
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
    /// See [`Tab::attention`](cctop_ui::tabs::Tab::attention).
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
    /// [`App::mark_answered`](cctop_ui::App::mark_answered).
    pub fn awaits_you(self) -> bool {
        matches!(self, Signal::NeedsInput | Signal::Idle | Signal::Acting)
    }

    /// Whether this changes *which sessions exist*, and so is worth a rescan
    /// rather than waiting for the next poll to notice.
    pub fn is_lifecycle(self) -> bool {
        matches!(self, Signal::Started | Signal::Ended)
    }

    /// What this says about the row's state, where the transcript is blind.
    ///
    /// Only the two waiting states are answered. A working signal returns
    /// `None` and leaves the transcript's reading alone, which is not
    /// deference so much as division of labour: a transcript reads *working*
    /// perfectly well — it is being written to — and it is the only one of the
    /// two that can see an API error. What it cannot see is either way of
    /// having stopped, because a held permission prompt and a finished turn are
    /// both, on disk, just an agent that stopped writing.
    pub fn activity(self) -> Option<crate::session::ActivityState> {
        match self {
            Signal::NeedsInput => Some(crate::session::ActivityState::Asking),
            Signal::Idle => Some(crate::session::ActivityState::WaitingForInput),
            Signal::Busy
            | Signal::Acting
            | Signal::Compacting
            | Signal::Started
            | Signal::Ended => None,
        }
    }

    /// A word for the STATE column, the hooks panel, and the option a cctop
    /// records on an rmux session. See [`Signal::from_label`].
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

    /// The inverse of [`Signal::label`], for a state read back off an rmux
    /// session — see [`rmux::State`](crate::rmux::State).
    ///
    /// Words rather than numbers, so the option stays legible to anyone running
    /// `rmux show-options` and so the two cctops on either side of it can be
    /// different versions. A word this one does not know reads as nothing
    /// reported, which falls back to the guess that was there before: a newer
    /// cctop teaching an older one a signal it cannot draw should cost the
    /// older one nothing.
    pub fn from_label(word: &str) -> Option<Signal> {
        match word {
            "asking" => Some(Signal::NeedsInput),
            "idle" => Some(Signal::Idle),
            "working" => Some(Signal::Busy),
            "acting" => Some(Signal::Acting),
            "compacting" => Some(Signal::Compacting),
            "started" => Some(Signal::Started),
            "ended" => Some(Signal::Ended),
            _ => None,
        }
    }

    /// Whether a claim this old can still be true, nothing having been said
    /// since.
    ///
    /// The asymmetry is the whole rule, and it is why this is not a plain
    /// timeout: a finished turn and a held question stay true until the agent
    /// says otherwise, so somebody away from their desk for an afternoon comes
    /// back to the tab that is still asking. Only a *working* claim expires,
    /// because the events that would have closed it — the tool returning, the
    /// turn ending — are exactly the ones a killed session never sends.
    ///
    /// Taken as an elapsed duration rather than read off `self` so that a
    /// report this process heard ([`Reported::is_current`], measuring an
    /// `Instant`) and one recovered from an rmux session ([`rmux::State`],
    /// measuring wall-clock seconds written by another process) are judged by
    /// one rule rather than two that have to be kept in step.
    ///
    /// [`rmux::State`]: crate::rmux::State
    pub fn is_current_after(self, elapsed: std::time::Duration) -> bool {
        !self.is_working() || elapsed < WORKING_TTL
    }
}

/// One event, resolved to the session it concerns.
#[derive(Debug, Clone)]
pub struct Event {
    pub session_id: String,
    /// The processes the hook that reported this ran under, nearest first.
    ///
    /// Empty from a harness whose hook is not `cctop hook`.
    /// The agent's own pid is in here somewhere; which one it is, is a question
    /// only the process table can answer — see
    /// [`Collector::collect`](crate::proc::Collector::collect).
    pub pids: Vec<u32>,
    /// `host:path` when the session was launched by `cctop sandbox` — see
    /// [`Session::sandbox`](crate::session::Session::sandbox).
    pub sandbox: Option<String>,
    /// What the agent last said about itself, and where it is working.
    pub reported: Reported,
    /// The subagent this event is about, when it is about one.
    ///
    /// `SubagentStop` is the only end-of-subagent signal that exists. A
    /// background subagent's tool_result arrives at *launch* — it says the agent
    /// started, not that it finished — so a transcript alone cannot tell a
    /// working subagent from a finished one.
    pub finished_agent: Option<String>,
    /// The subagent this event came from, on any event fired inside one.
    ///
    /// What lets a question one subagent is waiting on outlive the next thing
    /// a sibling does: see `App::apply_hooks`.
    pub agent: Option<String>,
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
    /// Whether this is a permission prompt that may yet answer itself.
    ///
    /// Claude Code's auto mode puts a permission request to a model before it
    /// puts it to the person, and that takes a few seconds. The hook fires when
    /// the request is raised, not when it is decided, so every auto-answered
    /// tool call rang the bell, blinked the tab and lit the attention card for
    /// a question nobody was ever going to be asked. Held back for
    /// [`PERMISSION_GRACE`]; if the decision lands first — `PermissionDenied`,
    /// or the tool simply running — the newer event replaces this one and it is
    /// never shown at all. See [`Reported::is_settled`].
    pub provisional: bool,
    /// What the prompt is asking to do, when this is one: see [`ask_of`].
    pub ask: Option<String>,
    /// Whether what is held is a question with choices — Claude Code's
    /// AskUserQuestion — rather than a permission prompt. Allow and Deny would
    /// press keys that *pick an option* there, so nothing may offer them.
    pub question: bool,
    /// The tool the prompt is for and what it was asked to touch, in full
    /// and redacted, when the payload named a tool: see
    /// [`crate::yolo_log::call_of`]. What a key press records in the YOLO
    /// log, which the one-line `ask` is too short and too unredacted for.
    pub call: Option<crate::yolo_log::Call>,
}

/// How long a permission prompt is given to answer itself before it is somebody
/// else's problem.
///
/// Long enough for auto mode's round trip, short enough that a prompt which
/// really is waiting for a person is not sat on. Wrong in one direction it
/// costs a few seconds' notice; wrong in the other it cries wolf on every tool
/// call, which is what teaches people to ignore the bell.
pub const PERMISSION_GRACE: std::time::Duration = std::time::Duration::from_secs(4);

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
        self.signal.is_current_after(self.at.elapsed())
    }

    /// Whether this can be shown yet.
    ///
    /// Everything but a fresh permission prompt can: see [`provisional`], and
    /// [`PERMISSION_GRACE`] for how long the exception lasts.
    ///
    /// [`provisional`]: Reported::provisional
    pub fn is_settled(&self) -> bool {
        !self.provisional || self.at.elapsed() >= PERMISSION_GRACE
    }
}

/// What the agents have reported, folded together the way both readers need it.
///
/// The dashboard's `App` and a standalone `cctop serve` each keep one, because
/// the folding is the subtle part and is not written twice: a subagent's open
/// question outlives whatever a sibling says next ([`still_asking`]), a prompt
/// inherits the description only its first event carried, a working claim
/// expires where a waiting one stands, and an `Ended` erases the lot.
#[derive(Default)]
pub struct Reports {
    /// The last report each session made, however old — callers gate on
    /// [`Reported::is_current`] and [`Reported::is_settled`].
    pub hooked: HashMap<String, Reported>,
    /// The questions a session's subagents are waiting on, by subagent id.
    ///
    /// `hooked` holds one report per session and every event replaces it, so a
    /// subagent's permission prompt was overwritten by whatever a sibling
    /// running beside it did next — and the session stopped asking while the
    /// question was still on screen. A question is kept here until the
    /// subagent that asked it says something else. See [`still_asking`].
    ///
    /// Visible to the crate for the tests that assert a question was closed,
    /// not for other writes — only [`Reports::observe`] folds these.
    pub asking_agents: HashMap<String, HashMap<String, Reported>>,
    /// The process tree each session's hooks reported running under, keyed by
    /// session id.
    ///
    /// Kept apart from `hooked` because it answers a different question and
    /// changes on a different clock: `hooked` is what the agent is *doing* and
    /// turns over constantly, while this is *where it is* and is written once
    /// and then repeated. Readers republish only the changes — see
    /// [`save_claims`].
    pub claims: HashMap<String, Vec<u32>>,
    /// Which sessions are working on another machine through `cctop
    /// sandbox`, as `host:path`, keyed by session id.
    ///
    /// Apart from `hooked` for the reason `claims` is: it is where the session
    /// is, said on every event and never changing, and an event that did not
    /// carry it — one from a cctop too old to send it — must not wipe it.
    pub sandboxes: HashMap<String, String>,
    /// Prompts `cctop yolo-hook` allowed before the observer's report of them
    /// arrived, by session: what was asked, and when the allow was heard.
    ///
    /// The two hooks fire together and race to every cctop, so the report
    /// that the prompt went up can land after the news that it was answered.
    /// Left to stand, it would be held for the grace, then shown — a row
    /// asking about a prompt that never reached the screen, for as long as
    /// the tool it allowed runs. See [`HOOK_RACE`].
    hook_allowed: HashMap<String, (Option<String>, std::time::Instant)>,
}

/// How long after a `yolo-hook` allow a report of the same prompt is taken to
/// be the one it answered.
///
/// Both hooks are bounded by [`DEADLINE`], so their events land within it of
/// each other; four times that covers a busy machine. Not longer, because a
/// genuine second prompt in the same words would be answered by the same hook
/// anyway, and one that was not — YOLO switched off in between — should show.
const HOOK_RACE: std::time::Duration = std::time::Duration::from_millis(1000);

impl Reports {
    /// A fresh memory, seeded with the pid claims some cctop last wrote.
    ///
    /// The file is the only way to know where a session runs when it has said
    /// nothing since this process started — which is exactly what a session
    /// blocked on a question does. See [`load_claims`].
    pub fn new() -> Reports {
        Reports {
            claims: load_claims(),
            ..Default::default()
        }
    }

    /// Fold one event into what each session is taken to be doing.
    ///
    /// Returns `(lifecycle, claims_moved)`: lifecycle when the set of sessions
    /// itself changed — a reason to rescan now rather than at the next poll —
    /// and claims_moved when the session→pid map did, which is the caller's
    /// cue to republish it (`save_claims`, and the worker/loader that resolves
    /// pids by it).
    pub fn observe(&mut self, event: &Event) -> (bool, bool) {
        let lifecycle = event.reported.signal.is_lifecycle();
        let mut moved = false;
        match event.reported.signal {
            // Nothing more will be said about it, and leaving the last signal
            // behind would have the row claim a state forever.
            Signal::Ended => {
                self.hooked.remove(&event.session_id);
                self.asking_agents.remove(&event.session_id);
                self.sandboxes.remove(&event.session_id);
                moved = self.claims.remove(&event.session_id).is_some();
            }
            _ => {
                if let Some(sandbox) = &event.sandbox {
                    self.sandboxes
                        .insert(event.session_id.clone(), sandbox.clone());
                }
                if !event.pids.is_empty() && self.claims.get(&event.session_id) != Some(&event.pids)
                {
                    self.claims
                        .insert(event.session_id.clone(), event.pids.clone());
                    moved = true;
                }
                let reported = self.after_yolo_hook(event);
                let mut reported = still_asking(
                    &mut self.asking_agents,
                    &event.session_id,
                    event.agent.clone(),
                    reported,
                );
                // Claude Code follows a `PermissionRequest` with a
                // `permission_prompt` notification for the same question, and
                // only the first says what the question is. Without this the
                // second wiped it a few seconds in — exactly when the prompt
                // stopped being provisional and was shown.
                if reported.signal == Signal::NeedsInput
                    && reported.ask.is_none()
                    && let Some(before) = self.hooked.get(&event.session_id)
                    && before.signal == Signal::NeedsInput
                {
                    reported.ask = before.ask.clone();
                    reported.call = reported.call.take().or_else(|| before.call.clone());
                    // And what kind of prompt it is: the notification knows
                    // even less about that than about the question's words.
                    reported.question = reported.question || before.question;
                }
                self.hooked.insert(event.session_id.clone(), reported);
            }
        }
        // A working claim that nothing has confirmed for a quarter of an hour
        // is dropped rather than believed: see [`Reported::is_current`]. Swept
        // here because this is the only place the map grows, and a session that
        // was killed mid-turn will never send the event that would clear it.
        self.hooked.retain(|_, reported| reported.is_current());
        // A chain outlives its usefulness exactly when the report it came with
        // does, and a session that has been swept must stop claiming a pid —
        // otherwise a reused pid would be handed to a session that is gone. A
        // waiting session is never swept, which is the case this exists for.
        let before = self.claims.len();
        self.claims.retain(|id, _| self.hooked.contains_key(id));
        self.sandboxes.retain(|id, _| self.hooked.contains_key(id));
        (lifecycle, moved || self.claims.len() != before)
    }

    /// What `event` says once `cctop yolo-hook`'s allows are taken into
    /// account: see [`Reports::hook_allowed`].
    ///
    /// The allow is recognised by its ask. [`envelope`] gives an event one only
    /// when it is a permission prompt, so a working event carrying one is the
    /// allow and nothing else; the ask is kept to match the prompt it answered.
    fn after_yolo_hook(&mut self, event: &Event) -> Reported {
        let mut reported = event.reported.clone();
        self.hook_allowed
            .retain(|_, (_, at)| at.elapsed() < HOOK_RACE);
        let ask = || reported.ask.clone().filter(|a| !a.is_empty());
        if reported.signal == Signal::Busy && reported.ask.is_some() {
            // Remembered only when it came first. Arriving second, it simply
            // replaces the report it answers, and a memory of it would swallow
            // the next prompt in the same words.
            let answers = |held: &Reported| {
                held.signal == Signal::NeedsInput
                    && held.ask.clone().filter(|a| !a.is_empty()) == ask()
            };
            let held = match &event.agent {
                Some(agent) => self
                    .asking_agents
                    .get(&event.session_id)
                    .and_then(|open| open.get(agent)),
                None => self.hooked.get(&event.session_id),
            };
            if !held.is_some_and(answers) {
                self.hook_allowed
                    .insert(event.session_id.clone(), (ask(), reported.at));
            }
            reported.ask = None;
        } else if reported.signal == Signal::NeedsInput
            && reported.provisional
            && self
                .hook_allowed
                .get(&event.session_id)
                .is_some_and(|(allowed, _)| *allowed == ask())
        {
            // The report of a prompt the hook has already answered: the tool
            // is running, which is what the allow said.
            self.hook_allowed.remove(&event.session_id);
            reported.signal = Signal::Busy;
            reported.provisional = false;
            reported.ask = None;
            reported.question = false;
        }
        reported
    }

    /// Promote every held permission prompt whose grace has expired, and say
    /// whether any had.
    ///
    /// Auto mode's answer arrives as its own event and replaces the report, so
    /// a prompt still sitting here when the grace runs out is one a person has
    /// to answer. Cleared rather than re-tested every tick: once a prompt is
    /// real it stays real.
    pub fn promote_matured(&mut self) -> bool {
        let mut matured = false;
        for reported in self.hooked.values_mut() {
            if reported.provisional && reported.at.elapsed() >= PERMISSION_GRACE {
                reported.provisional = false;
                matured = true;
            }
        }
        matured
    }

    /// Stamp `session` with the machine it works on, when its hooks said.
    ///
    /// Beside [`Session::apply_reports`](crate::session::Session::apply_reports)
    /// at both of its callers, and for the same reason: rows are rebuilt from
    /// transcripts, which know nothing of how the agent was launched.
    pub fn stamp_sandbox(&self, session: &mut crate::session::Session) {
        if let Some(sandbox) = self.sandboxes.get(&session.session_id) {
            session.sandbox = Some(sandbox.clone());
        }
    }

    /// The report `session_id`'s hooks last made, however old, if they made one.
    pub fn report(&self, session_id: &str) -> Option<&Reported> {
        if let Some(reported) = self.hooked.get(session_id) {
            return Some(reported);
        }
        // Gemini CLI reports a full session id, but names the chat file it
        // writes — which is the only identity cctop's rows have, because
        // resuming reuses the id across disjoint files — after the *first
        // eight characters* of it. Without this last step every Gemini event
        // lands on no row at all.
        let tail = gemini_id_tail(session_id)?;
        self.hooked
            .iter()
            .find(|(id, _)| id.starts_with(tail))
            .map(|(_, reported)| reported)
    }
}

/// The report a session's hooks last made, however old, if they made one.
///
/// Gemini CLI reports a full session id, but names the chat file it writes —
/// which is the only identity cctop's rows have, because resuming reuses the
/// id across disjoint files — after the *first eight characters* of it.
/// Without this fallback every Gemini event lands on no row at all.
///
/// `None` for every other harness's ids, which is what keeps the tail from
/// matching on the end of a uuid that happens to line up: only a stem shaped
/// like Gemini's is looked up loosely, and only ever against a full id's
/// prefix.
pub fn gemini_id_tail(session_id: &str) -> Option<&str> {
    let tail = session_id.strip_prefix("session-")?.rsplit_once('-')?.1;
    (tail.len() == 8 && tail.chars().all(|c| c.is_ascii_alphanumeric())).then_some(tail)
}

/// What a session should be taken to be doing after `reported`, given the
/// questions its subagents are still waiting on.
///
/// A subagent's question stands until *that* subagent says something else —
/// its tool running, or being denied, or it stopping — however busy the
/// agents beside it are. While any stands, the session is asking: the most
/// recent open question is what it reports, so the tab stays lit and the
/// bell's grace period is the question's own.
fn still_asking(
    asking_agents: &mut HashMap<String, HashMap<String, Reported>>,
    session: &str,
    agent: Option<String>,
    reported: Reported,
) -> Reported {
    let open = asking_agents.entry(session.to_string()).or_default();
    match agent {
        Some(agent) => match reported.signal {
            Signal::NeedsInput => {
                open.insert(agent, reported.clone());
            }
            _ => {
                open.remove(&agent);
            }
        },
        // The session's own turn ending. A subagent it was waiting on cannot
        // still be asking once it has — a prompt dismissed with Esc ends the
        // turn and sends nothing from the subagent — and a question kept past
        // that would light the tab for good, since a question is never aged
        // out.
        //
        // ponytail: a *background* subagent asking across its parent's `Stop`
        // is taken to have been answered.
        None if reported.signal == Signal::Idle => open.clear(),
        None => {}
    }
    if reported.signal == Signal::NeedsInput {
        return reported;
    }
    match open.values().max_by_key(|asked| asked.at) {
        Some(asked) => asked.clone(),
        None => {
            asking_agents.remove(session);
            reported
        }
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
        // `UserPromptExpansion` is the path `UserPromptSubmit` does not cover:
        // typing `/skillname` expands into a prompt without submitting one, so
        // a session driven entirely by slash commands never reported starting
        // work at all.
        //
        // `PostToolBatch` fires once after every call in a batch has resolved,
        // where `PostToolUse` fires once per call and concurrently. It is the
        // moment the model is about to be asked again — the same "still going"
        // fact, arrived at once instead of five times.
        //
        // `SubagentStart` pairs with the `SubagentStop` already here. Without
        // it the Subagents panel learned of a subagent only when it ended,
        // which is the half of its life nobody needs to watch.
        //
        // `ElicitationResult` closes out the `Elicitation` below: the answer
        // has been given and the MCP call is moving again.
        //
        // `TaskCreated` and `TaskCompleted` are the agent's own plan changing
        // under it. Neither is a state cctop draws differently, but both happen
        // only while an agent is working, and both are moments a turn that
        // looked stalled is demonstrably not.
        "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" | "SubagentStop"
        | "UserPromptExpansion" | "PostToolBatch" | "SubagentStart"
        | "ElicitationResult" | "TaskCreated" | "TaskCompleted"
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
        // An MCP server asking the user something mid-task, which is a held
        // prompt of a kind cctop had no way to see: it is not a permission
        // dialog, so `PermissionRequest` never fires, and the screen goes still
        // the same way it does for a tool that runs long and quietly. A session
        // blocked on one of these looked idle.
        "permission.asked" | "PermissionRequest" | "Elicitation" => Some(Signal::NeedsInput),
        // The answer to one of those, from a harness that has no event for it.
        // OpenCode 2 raises nothing when a prompt is answered — the request
        // simply stops being pending — so its plugin says so from the API
        // instead, which is the same closing-out a `PostToolUse` does for Claude
        // Code: without it a tab keeps asking about a question that was
        // answered minutes ago, and the only thing that clears it is the tool
        // call that follows, or the end of the turn.
        "permission.replied" | "permission.rejected" => Some(Signal::Busy),
        // Auto mode refused a tool call. The turn continues — the model is told
        // it may retry — so this is not the end of anything, but it is the one
        // event that says a refusal happened at all. Without it the only trace
        // is a tool that never ran.
        "PermissionDenied" => Some(Signal::Busy),
        // cctop's own, sent by [`announce_answer`] when the page answered a
        // prompt. Allowed, the tool runs and its own events follow. Denied is
        // Esc, which ends the turn and sends nothing at all — so without this
        // the row asked a question that was no longer on screen, and the next
        // Allow typed a `1` into the composer.
        ANSWERED_ALLOW | YOLO_ALLOWED => Some(Signal::Busy),
        ANSWERED_DENY => Some(Signal::Idle),
        // Compaction is the one kind of work worth naming separately: the
        // context panel is about to lurch, and it is not the agent stalling.
        "PreCompact" | "PreCompress" | "preCompact" | "session.compacted" => {
            Some(Signal::Compacting)
        }
        // And the other end of it: compaction is over and the agent is moving
        // again. Claude Code also raises a `SessionStart` after compacting,
        // which says the same thing — but `SessionStart` is `Started`, and a
        // session that has merely compacted is not one that has just begun.
        "PostCompact" => Some(Signal::Busy),
        // A teammate in an agent team finishing its turn. The team's own
        // `Stop`, one member at a time — and the moment a team stops being
        // something to watch and starts being something to answer.
        "TeammateIdle" => Some(Signal::Idle),
        // Everything below happens only while a session is alive and doing
        // something, and none of it is a state cctop draws differently. They
        // are here because a turn can otherwise go minutes without an event:
        // a long answer with no tool calls in it raises nothing at all between
        // `UserPromptSubmit` and `Stop`, and a session with nothing to report
        // is one whose liveness falls back to guessing at a still screen.
        //
        // `MessageDisplay` is the frequent one — once per batch of streamed
        // lines, several times per message — and the only event that fires
        // *during* an answer. It is display-only and cctop returns no
        // `displayContent`, so what is on screen is untouched.
        //
        // `CwdChanged` and `DirectoryAdded` also move the ground under a row:
        // the project a session is drawn against comes from its working
        // directory, and an agent that `cd`s has been drawn in the wrong place
        // until its next transcript write said otherwise.
        //
        // `FileChanged` fires only once something has named a file to watch,
        // which cctop does not — so it is registered against the day something
        // does, and costs nothing until then.
        "MessageDisplay" | "InstructionsLoaded" | "CwdChanged" | "DirectoryAdded"
        | "ConfigChange" | "FileChanged" | "WorktreeRemove" => Some(Signal::Busy),
        // Claude Code fires this only under `--init-only`, `--init` or
        // `--maintenance`, never on a normal start. A session that begins this
        // way is still a session beginning.
        "Setup" => Some(Signal::Started),
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
        pids: value
            .get("pids")
            .and_then(|v| v.as_array())
            .map(|pids| {
                pids.iter()
                    .filter_map(|p| p.as_u64())
                    .map(|p| p as u32)
                    .collect()
            })
            .unwrap_or_default(),
        sandbox: value
            .get("sandbox")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
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
        agent: value
            .get("agent_id")
            .and_then(|v| v.as_str())
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        reported: Reported {
            // The prompt that auto mode may answer on your behalf, and the only
            // one worth holding: an MCP elicitation and a plain notification
            // are questions for a person however long you wait.
            provisional: matches!(
                value.get("event").and_then(|v| v.as_str()),
                Some("PermissionRequest" | "permission.asked")
            ),
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
            ask: value
                .get("ask")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            question: value.get("question").and_then(|v| v.as_bool()) == Some(true),
            call: value
                .get("call")
                .and_then(|v| serde_json::from_value(v.clone()).ok()),
            at: std::time::Instant::now(),
        },
    })
}

// ---------------------------------------------------------------------------
// Installation
// ---------------------------------------------------------------------------

/// Events cctop asks Claude Code to tell it about: all of them but one.
///
/// This used to be a deliberately small set, on the grounds that every hook
/// fire costs the agent a process spawn and an event cctop would only use to
/// redraw something it can already see is not worth the fork. That reasoning
/// was right about the fork and wrong about the cost of being conservative: a
/// settings file that names only the events this version knows about is one
/// that has to be rewritten every time cctop learns another, on every machine,
/// by everybody. [`repair`] softens that — it tops an install up on the way up
/// — but it can only add what *this* binary knows to ask for, so an agent
/// already running still says nothing about the rest until it restarts.
///
/// So the settings file now says "tell cctop everything", and the binary
/// decides what any of it means. Adding an event cctop acts on is a change to
/// [`signal_of`] alone, and takes effect on the next fire rather than the next
/// install.
///
/// What that costs is real and worth naming: `MessageDisplay` fires once per
/// batch of streamed lines, several times per assistant message, where the rest
/// fire a handful of times in a session. It buys the one thing nothing else
/// says — a long answer with no tool calls in it raises nothing at all between
/// `UserPromptSubmit` and `Stop`, and a turn with no events is one whose
/// liveness falls back to guessing at a still screen.
///
/// `WorktreeCreate` is the single exception, and it is not about cost.
/// It does not observe worktree creation — it **replaces** it. Claude Code
/// stops calling `git worktree` and takes the path from the hook's stdout, and
/// "if the hook fails or produces no path, worktree creation fails with an
/// error". This hook writes nothing to stdout by construction (see the module
/// docs), so installing it would break `claude --worktree`, every subagent with
/// `isolation: "worktree"`, and every backgrounded session Claude Code isolates
/// in one. It is the one event on offer that could take a session down, which
/// is the thing this module exists not to do.
const CLAUDE_EVENTS: &[&str] = &[
    "Setup",
    "SessionStart",
    "InstructionsLoaded",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "MessageDisplay",
    "PreToolUse",
    "PermissionRequest",
    "PermissionDenied",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "Notification",
    "SubagentStart",
    "SubagentStop",
    "TaskCreated",
    "TaskCompleted",
    "Stop",
    "StopFailure",
    "TeammateIdle",
    "ConfigChange",
    "CwdChanged",
    "DirectoryAdded",
    "FileChanged",
    "WorktreeRemove",
    "PreCompact",
    "PostCompact",
    "SessionEnd",
    "Elicitation",
    "ElicitationResult",
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

/// The same for the one entry that may decide: `<cctop> yolo-hook <Event>`.
/// See [`yolo_hook`].
const YOLO_MARKER: &str = " yolo-hook ";

/// The events `cctop yolo-hook` is installed for, beside the observer, in
/// Claude Code's settings. One, because a permission dialog about to go up is
/// the only moment YOLO answers — see [`yolo_hook`] for why not `PreToolUse`.
const CLAUDE_DECIDING: &[&str] = &["PermissionRequest"];

/// How a missing `yolo-hook` entry is named in a health report.
const YOLO_HOOK_LABEL: &str = "PermissionRequest (yolo-hook)";

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
        /// The events that also get `cctop yolo-hook`, beside the observer.
        deciding: &'static [&'static str],
    },
    /// Codex's single `notify` program, in TOML.
    Notify(PathBuf),
    /// A plugin file cctop owns outright and can therefore write and delete
    /// whole, rather than merging into somebody else's document.
    Plugin(PathBuf),
    /// Claude Code's `/yolo`, a skill file cctop owns outright like the
    /// plugin. `commands` is where a command file of the same name would
    /// be, which the skill would hide; `hooks` is the settings file of the
    /// same scope, whose state decides whether a missing skill is an install
    /// that is short or no install at all.
    Skill {
        path: PathBuf,
        commands: PathBuf,
        hooks: PathBuf,
    },
}

impl Config {
    /// The file this writes.
    fn path(&self) -> &Path {
        match self {
            Config::Json { path, .. }
            | Config::Notify(path)
            | Config::Plugin(path)
            | Config::Skill { path, .. } => path,
        }
    }

    /// Write cctop into this one file, and say what happened to it.
    fn install(&self, exe: &str) -> anyhow::Result<String> {
        match self {
            Config::Json {
                path,
                shape,
                events,
                deciding,
            } => {
                json_install(path, *shape, events, deciding, exe)?;
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
            Config::Skill { path, commands, .. } => {
                skill_install(path, commands, exe)?;
                Ok(format!("added {SKILL_LABEL} at {}", path.display()))
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
            Config::Skill { path, .. } => Ok(match skill_remove(path)? {
                true => format!("removed {SKILL_LABEL} from {}", path.display()),
                false => format!("no {SKILL_LABEL} of cctop's installed"),
            }),
        }
    }

    /// What is in this one file right now.
    fn health(&self) -> Health {
        match self {
            Config::Json {
                path,
                shape,
                events,
                deciding,
            } => json_health(path, *shape, events, deciding),
            Config::Notify(path) => notify_health(path),
            Config::Plugin(path) => plugin_health(path),
            Config::Skill {
                path,
                commands,
                hooks,
            } => skill_health(path, commands, hooks),
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
            Harness::Claude => {
                // Honours the same `$CLAUDE_CONFIG_DIR` override as the rest
                // of cctop.
                let hooks = match project {
                    None => crate::config::CLAUDE_CONFIG_DIR.join("settings.json"),
                    Some(dir) => dir.join(".claude").join("settings.json"),
                };
                let settings = Config::Json {
                    path: hooks.clone(),
                    shape: Shape::Nested,
                    events: CLAUDE_EVENTS,
                    deciding: CLAUDE_DECIDING,
                };
                match project {
                    // `/yolo` is one person's switch for their own sessions,
                    // so it is theirs alone: a project's `.claude/` is checked
                    // in, and a command that hands out permission is not one
                    // to give everyone who clones it.
                    Some(_) => vec![settings],
                    None => {
                        let dir = &*crate::config::CLAUDE_CONFIG_DIR;
                        vec![
                            settings,
                            Config::Skill {
                                path: dir.join("skills").join("yolo").join("SKILL.md"),
                                commands: dir.join("commands").join("yolo.md"),
                                hooks,
                            },
                        ]
                    }
                }
            }
            Harness::Gemini => vec![Config::Json {
                path: match project {
                    None => crate::config::GEMINI_HOME.join("settings.json"),
                    Some(dir) => dir.join(".gemini").join("settings.json"),
                },
                shape: Shape::Nested,
                events: GEMINI_EVENTS,
                deciding: &[],
            }],
            Harness::Cursor => vec![Config::Json {
                path: match project {
                    None => crate::config::CURSOR_HOME.join("hooks.json"),
                    Some(dir) => dir.join(".cursor").join("hooks.json"),
                },
                shape: Shape::Flat,
                events: CURSOR_EVENTS,
                deciding: &[],
            }],
            Harness::Codex => {
                let hooks = Config::Json {
                    path: match project {
                        None => crate::config::CODEX_HOME.join("hooks.json"),
                        Some(dir) => dir.join(".codex").join("hooks.json"),
                    },
                    shape: Shape::Nested,
                    events: CODEX_EVENTS,
                    // ponytail: Codex keeps the key press. Its hooks take
                    // Claude Code's shape, but whether it honours a decision
                    // from one has not been checked against it.
                    deciding: &[],
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
///
/// A file that already says exactly this is not written at all. That is what
/// makes [`repair`] safe to run at every start, from every cctop at once: the
/// file is read fresh here, after whatever health check sent the caller, so a
/// second cctop that got there first leaves this one nothing to do, and an
/// install that is already right keeps its bytes, its formatting and its mtime.
fn json_install(
    path: &Path,
    shape: Shape,
    events: &[&str],
    deciding: &[&str],
    exe: &str,
) -> anyhow::Result<()> {
    let before = read_settings(path)?;
    let mut root = before.clone();
    json_merge(&mut root, path, shape, events, deciding, exe)?;
    if root == before {
        return Ok(());
    }
    write_settings(path, &root)
}

/// What [`json_install`] does to a document, without the file around it — so
/// that [`json_health`] can ask whether an install would change anything by
/// running the very code that would make the change.
fn json_merge(
    root: &mut serde_json::Map<String, serde_json::Value>,
    path: &Path,
    shape: Shape,
    events: &[&str],
    deciding: &[&str],
    exe: &str,
) -> anyhow::Result<()> {
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

    // An event an older cctop registered and this one no longer wants is
    // swept as [`json_remove`] sweeps it: left behind, it keeps firing in a
    // form nothing reads, and it is one of the differences that make an
    // install outdated.
    // An array emptied here goes with its key; one the user left empty stays.
    let mut emptied = Vec::new();
    for (event, value) in hooks.iter_mut() {
        if !events.contains(&event.as_str())
            && let Some(list) = value.as_array_mut()
            && drop_ours(list) > 0
            && list.is_empty()
        {
            emptied.push(event.clone());
        }
    }
    for event in emptied {
        // `shift_remove`, as in `json_remove`: the user's key order stays.
        hooks.shift_remove(&event);
    }

    for event in events {
        let list = hooks
            .entry(*event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("`hooks.{event}` is not an array"))?;
        // Idempotent: running the installer twice must not fire twice, and an
        // entry left by an older cctop at a path that has since moved is
        // replaced rather than added to.
        drop_ours(list);
        list.extend(our_entries_for(shape, event, deciding, exe));
    }
    Ok(())
}

/// The entries an install writes for one event, in order: the observer, and
/// beside it the deciding `yolo-hook` where this event has one.
fn our_entries_for(
    shape: Shape,
    event: &str,
    deciding: &[&str],
    exe: &str,
) -> Vec<serde_json::Value> {
    let mut commands = vec![hook_command(exe, event)];
    // Its own entry, beside the observer's rather than inside it: the
    // harness runs an event's hooks in parallel, so the observer reports
    // exactly as it does without it.
    if deciding.contains(&event) {
        commands.push(yolo_hook_command(exe, event));
    }
    commands
        .into_iter()
        .map(|command| {
            let command = serde_json::json!({
                "type": "command",
                "command": command,
            });
            match shape {
                Shape::Nested => serde_json::json!({ "hooks": [command] }),
                Shape::Flat => command,
            }
        })
        .collect()
}

/// cctop's entries in one settings document, event by event, and nothing else.
///
/// Where they sit among the user's entries is left out on purpose. An install
/// appends, so another tool that appends after it leaves cctop's entry
/// somewhere other than last, and comparing whole lists would call that
/// outdated and move it back on every start — a rewrite of the user's file
/// for a difference that changes nothing about what fires.
fn our_entries(
    root: &serde_json::Map<String, serde_json::Value>,
) -> std::collections::BTreeMap<&str, Vec<&serde_json::Value>> {
    root.get("hooks")
        .and_then(|h| h.as_object())
        .into_iter()
        .flatten()
        .filter_map(|(event, value)| {
            let ours: Vec<_> = value.as_array()?.iter().filter(|e| is_ours(e)).collect();
            (!ours.is_empty()).then_some((event.as_str(), ours))
        })
        .collect()
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
                removed += drop_ours(list);
            }
        }
        // An event whose only entry was ours goes too, rather than leaving an
        // empty array behind in someone else's file.
        hooks.retain(|_, value| !value.as_array().is_some_and(|l| l.is_empty()));
        // And the `hooks` object itself, once cctop's were all it held.
        // `shift_remove`, not `remove`: under `preserve_order` the latter is a
        // swap, which would move the file's last key into the hole.
        if hooks.is_empty() {
            root.shift_remove("hooks");
        }
    }
    if removed > 0 {
        write_settings(path, &root)?;
    }
    Ok(removed)
}

/// Take cctop's commands out of one event's list, and say how many went.
///
/// By command, not by entry: cctop writes a wrapper holding only its own
/// command, but a user may add theirs into the same one, and dropping the
/// wrapper whole would delete a hook that was never cctop's. A wrapper left
/// with nothing in it goes, like an entry that was cctop's alone.
fn drop_ours(list: &mut Vec<serde_json::Value>) -> usize {
    let is_our_hook = |h: &serde_json::Value| {
        h.get("command")
            .and_then(|c| c.as_str())
            .is_some_and(is_our_command)
    };
    let mut removed = 0;
    for entry in list.iter_mut() {
        if let Some(inner) = entry.get_mut("hooks").and_then(|h| h.as_array_mut())
            && inner.iter().any(|h| !is_our_hook(h))
        {
            let before = inner.len();
            inner.retain(|h| !is_our_hook(h));
            removed += before - inner.len();
        }
    }
    let before = list.len();
    list.retain(|entry| !is_ours(entry));
    removed + before - list.len()
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
    let Some((exe, event)) = split_ours(command) else {
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

/// The command line the installer writes for one event.
///
/// The harnesses hand this to a shell, so a path with a space in it — an
/// `/Applications/My Tools/cctop`, a home directory with a space — has to be
/// quoted, or every fire runs `/Applications/My` and exits 127. A path with
/// nothing the shell would read is written bare, so an install that already
/// works is byte-for-byte what it was.
fn hook_command(exe: &str, event: &str) -> String {
    command_line(exe, MARKER, event)
}

/// The command line for the deciding entry: see [`yolo_hook`].
fn yolo_hook_command(exe: &str, event: &str) -> String {
    command_line(exe, YOLO_MARKER, event)
}

fn command_line(exe: &str, marker: &str, event: &str) -> String {
    let plain = !exe.is_empty()
        && exe
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:=@%".contains(c));
    if plain {
        format!("{exe}{marker}{event}")
    } else {
        format!("'{}'{marker}{event}", exe.replace('\'', r"'\''"))
    }
}

/// A command line cut at whichever of cctop's two words it carries, into the
/// binary and the event. The two cannot be confused: `yolo-hook` has no space
/// before its `hook`.
fn split_ours(command: &str) -> Option<(&str, &str)> {
    command
        .rsplit_once(YOLO_MARKER)
        .or_else(|| command.rsplit_once(MARKER))
}

/// The cctop an installed command names, taken back out of the command text.
///
/// Everything before the last `hook` is the path, which is why this splits on
/// the marker rather than on whitespace: an install from before the path was
/// quoted wrote a spaced path bare, and it still has to be recognised to be
/// repaired. A quoted path is unquoted, the inverse of [`hook_command`].
fn recorded_exe(command: &str) -> Option<String> {
    if !is_our_command(command) {
        return None;
    }
    let exe = split_ours(command)?.0.trim();
    Some(
        match exe.strip_prefix('\'').and_then(|e| e.strip_suffix('\'')) {
            Some(quoted) => quoted.replace(r"'\''", "'"),
            None => exe.to_string(),
        },
    )
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

/// The file a write to `path` should land in: the link's target when `path` is
/// a symlink, `path` itself otherwise.
///
/// A config symlinked in from a dotfiles repository is common, and the
/// write-and-rename below would replace the link with a plain file — after
/// which the user's next change to the repository silently stops reaching the
/// agent. A dangling link is left to be replaced, since there is nothing behind
/// it to keep in step.
fn through_link(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(target) if path.is_symlink() => target,
        _ => path.to_path_buf(),
    }
}

fn write_settings(
    path: &Path,
    root: &serde_json::Map<String, serde_json::Value>,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let path = &through_link(path);
    // Written beside the target and renamed: a crash mid-write would otherwise
    // leave the user with no settings at all, which breaks their agent far more
    // thoroughly than a missing hook.
    let tmp = temp_beside(path, "json");
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

    // Already this, exactly: nothing to write. See [`json_install`].
    if codex_notify_argv(&doc).is_some_and(|argv| argv == codex_notify(exe)) {
        return Ok(());
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
    let path = &through_link(path);
    let tmp = temp_beside(path, "toml");
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

/// The names a plugin file has to carry for the OpenCode of `api` that cctop
/// wants.
///
/// OpenCode 2 registers the two tool moments as hooks and reads the asking from
/// the API, so its half is looked for under the names that API and those hooks
/// use rather than the ones 1 announced them with — and it has to say when a
/// prompt was answered as well as when one went up, which 1's half never had to.
fn wanted(api: crate::opencode::Api) -> Vec<&'static str> {
    match api {
        crate::opencode::Api::V1 => OPENCODE_EVENTS.to_vec(),
        crate::opencode::Api::V2 => vec![
            "session.idle",
            "session.created",
            "session.deleted",
            "session.compacted",
            "permission.asked",
            "permission.replied",
            "execute.before",
            "execute.after",
        ],
    }
}

/// Which dialects a plugin file on disk carries, read back out of its exports.
///
/// They cannot be mistaken for one another, which is the point: OpenCode 2
/// refuses anything without a default export, so a version 1 file on a version 2
/// server is not loaded at all — it says so in a log line and carries on, and
/// the session looks to cctop like one that is merely quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PluginShape {
    /// A version 1 plugin: a named export and nothing else.
    V1,
    /// A version 2 plugin: a default export and nothing else.
    V2,
    /// What cctop writes now: both halves in one file, which is right on every
    /// version and is what makes an upgrade a non-event.
    Both,
}

fn plugin_shape(text: &str) -> PluginShape {
    match (
        text.contains("export default"),
        text.contains("export const cctop"),
    ) {
        (true, true) => PluginShape::Both,
        (true, false) => PluginShape::V2,
        (false, _) => PluginShape::V1,
    }
}

/// The events `text` does not forward to the OpenCode of `api` that cctop wants.
fn plugin_shortfall(text: &str, api: crate::opencode::Api) -> Vec<&'static str> {
    let for_this_one = wanted(api);
    // A file carrying both halves forwards everything on every version, so it is
    // checked against the union and nothing else. One carrying a single half is
    // short of *everything* on the other version, which is not a shortfall to be
    // filled in by editing: the file is replaced whole, which is what an install
    // does anyway. It is reported so that an OpenCode upgraded under a cctop
    // installed before this one is rewritten rather than going quiet.
    let carried = match plugin_shape(text) {
        PluginShape::Both => {
            let mut both = wanted(crate::opencode::Api::V1);
            for name in wanted(crate::opencode::Api::V2) {
                if !both.contains(&name) {
                    both.push(name);
                }
            }
            both
        }
        PluginShape::V1 if api == crate::opencode::Api::V1 => for_this_one,
        PluginShape::V2 if api == crate::opencode::Api::V2 => for_this_one,
        PluginShape::V1 | PluginShape::V2 => return for_this_one,
    };
    carried
        .iter()
        .filter(|event| !text.contains(**event))
        .copied()
        .collect()
}

/// The line the plugin records this binary's path on, and how its own state is
/// read back out.
const PLUGIN_MARKER: &str = "const CCTOP = ";

/// The plugin cctop writes, as a file rather than as a string in here.
///
/// A real `.ts` file because it is a real program: an editor highlights it, a
/// formatter can be run over it, and a diff of it is a diff of JavaScript rather
/// than of Rust lines with every brace doubled to survive `format!`. Embedded
/// with `include_str!` for the reason [`cctop_serve`] embeds its pages the same
/// way — an installed cctop is one binary, and a plugin cctop cannot find is a
/// plugin that reports nothing.
///
/// The one line that is per-install is a placeholder: `PLUGIN_PATH`, replaced
/// with this binary's path at install time, and read back out of the installed
/// file by [`plugin_exe`].
const PLUGIN: &str = include_str!("assets/opencode-plugin.ts");

/// What the placeholder is written as, quoted, so the substitution is one
/// replacement of a whole JSON string rather than a splice inside one.
const PLUGIN_PATH: &str = "\"$CCTOP\"";

/// The two exports whole, for the tests that take one of them out. Which
/// dialect a file is comes from the shorter markers in [`plugin_shape`], because
/// a file written for one version alone has a shorter line here.
#[cfg(test)]
const PLUGIN_DEFAULT_EXPORT: &str = "export default { id: \"cctop\", setup, server: cctopV1 }";
#[cfg(test)]
const PLUGIN_NAMED_EXPORT: &str = "export const cctop = cctopV1";

/// The plugin cctop writes, for every OpenCode there is.
///
/// OpenCode has no hook commands to register: extensions are code it loads at
/// startup, so the only way in is a file, and cctop writes the whole of it. That
/// makes this the one integration that runs *inside* the agent's process rather
/// than beside it, which is why every line of the plugin is wrapped: a plugin
/// that throws is a plugin that can spoil the session it is watching, and there
/// is no exit code to hide behind here.
///
/// # One file for both OpenCodes
///
/// Version 2 replaced the plugin API outright and does not load a version 1
/// plugin at all — it logs a warning nobody opens and carries on, so a session
/// on version 2 reports nothing and looks merely quiet. The obvious answer is to
/// write whichever file the installed version wants, which is what this did
/// first, and it has one bad property: it is only right until somebody upgrades.
///
/// So the file carries both dialects instead, and neither can be run on the
/// wrong OpenCode by accident:
///
/// - A named export is the version 1 plugin, and is what version 1 has always
///   loaded — including 1.14, which knows nothing about object entrypoints.
/// - The default export is `{ id, setup, server }`: version 2 calls `setup`,
///   and version 1.18.29 and newer call `server`, which is the same version 1
///   plugin again. Verified against 1.14.25, 1.18.33 and 2.0.20 rather than
///   assumed: 1.18.33 calls *both*, handing `setup` a context with no `event`,
///   `tool` or `permission` on it, which is why every use of them is optional
///   and the whole body is guarded. Version 2 calls only `setup`, so a version 2
///   session is never reported twice.
///
/// Both halves hand each moment to `cctop hook` the same way and under the same
/// names, so everything downstream of the plugin reads one vocabulary.
fn plugin_source(exe: &str) -> String {
    PLUGIN.replace(
        PLUGIN_PATH,
        &serde_json::Value::String(exe.to_string()).to_string(),
    )
}

/// The same plugin with one entry point taken out.
///
/// Not what cctop installs — this is for the tests, which need a file that is
/// *only* version 1 or *only* version 2 to prove that one written for the other
/// is noticed rather than read as installed. Taken out of the real file rather
/// than written twice, so the two fixtures cannot drift from what ships: each
/// keeps every name cctop wants, which is exactly the state being tested — a
/// plugin that says the right things and is loaded by nothing.
#[cfg(test)]
fn plugin_source_for(exe: &str, api: crate::opencode::Api) -> String {
    let text = plugin_source(exe);
    match api {
        crate::opencode::Api::V1 => text.replace(PLUGIN_DEFAULT_EXPORT, ""),
        crate::opencode::Api::V2 => text.replace(PLUGIN_NAMED_EXPORT, ""),
    }
}

/// Write the plugin, replacing whatever cctop left there before.
fn plugin_install(path: &Path, exe: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let source = plugin_source(exe);
    // Already this, exactly: nothing to write. See [`json_install`].
    if std::fs::read_to_string(path).is_ok_and(|text| text == source) {
        return Ok(());
    }
    let tmp = temp_beside(path, "ts");
    std::fs::write(&tmp, source)?;
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
// Claude Code's /yolo
// ---------------------------------------------------------------------------

/// What the panel and the install lines call the skill.
const SKILL_LABEL: &str = "/yolo";

/// The line of the skill's frontmatter that marks it cctop's and records the
/// binary it runs, as a JSON string, the way the plugin's marker does.
/// `metadata` is the map Claude Code documents as free-form and left to the
/// tooling that wrote it.
const SKILL_MARKER: &str = "  cctop: ";

/// The line of the same map that keeps the skill's key between rewrites. See
/// [`skill_source`] for what the key is for.
const SKILL_KEY: &str = "  cctop-key: ";

/// `/yolo`, `/yolo off` and `/yolo status` in Claude Code.
///
/// A skill rather than a file in `commands/`, because the docs mirrored in
/// `docs/harnesses/claude/skills.md` say command files are the older form and
/// to prefer a skill for new work; the two behave the same.
///
/// The work is done by the `` !`…` `` line, which Claude Code runs while it
/// expands the command — when the person types it, before the model sees
/// anything — and replaces with what the command printed. The model receives
/// only that line, and is told to repeat it.
///
/// Three things in the frontmatter carry the weight:
///
/// - `disable-model-invocation`, so the model cannot run `/yolo` through its
///   Skill tool: Claude Code refuses that call and keeps the description out
///   of the model's context altogether. Without it, an expansion triggered by
///   the model would run the shell line exactly as the person's does.
/// - `allowed-tools`, because Claude Code checks an expansion's shell line
///   against the permission rules and, outside auto mode, aborts the command
///   on anything short of allow; in auto mode it would hand the line to the
///   model to run instead, which is the one thing this must not do. The grant
///   does not end with the expansion, though: it lasts the whole turn the
///   person typed `/yolo` in, and covers the model's Bash calls too. A real
///   session showed what that costs with a plain `cctop yolo --slash *`
///   grant: the person types `/yolo off`, and the model, in the same turn,
///   runs `cctop yolo --slash on` with no prompt. So the line carries a key —
///   random, written once and kept across rewrites — and the grant names it.
///   The model never sees the line, only what it printed, so the command it
///   could type is not the one the grant covers, and it is prompted for like
///   any other. The key is not checked by `cctop yolo`: its whole job is to
///   make the granted command one nobody else can spell.
/// - the marker, so a `/yolo` that is not cctop's is never overwritten.
///
/// What it cannot carry is proof that a person typed it. Claude Code runs an
/// expansion's shell line through the Bash tool's own code — the same
/// environment, the same process tree, `AI_AGENT` and all — so `cctop yolo`
/// sees exactly what it would see if the model had run it. Checked against
/// 2.1.294's binary and a real session: two runs, one each way, differ in
/// their pids and nothing else. The model's own Bash call is held at a
/// permission prompt like any other command, and that is the guard; see
/// [`crate::yolo::command`].
fn skill_source(exe: &str, key: &str) -> String {
    let run = format!("{} yolo --slash={key}", command_word(exe));
    // YAML's double-quoted strings are a superset of JSON's, so serde's
    // quoting is a correct YAML quoting of any path at all.
    let quote = |text: &str| serde_json::Value::String(text.to_string()).to_string();
    format!(
        "---\n\
         description: {description}\n\
         argument-hint: \"[on|off|status]\"\n\
         disable-model-invocation: true\n\
         allowed-tools: {allow}\n\
         metadata:\n\
         {SKILL_MARKER}{exe_json}\n\
         {SKILL_KEY}{key_json}\n\
         ---\n\
         cctop's answer to /yolo, already carried out:\n\
         \n\
         !`{run} $ARGUMENTS`\n\
         \n\
         Repeat that answer to the user exactly as it is, and run nothing because of it.\n",
        description = quote(
            "Switch cctop's YOLO on for this session, so every permission prompt \
             is allowed until it ends. /yolo off stops it; /yolo status says which."
        ),
        allow = quote(&format!("Bash({run} *)")),
        exe_json = quote(exe),
        key_json = quote(key),
    )
}

/// A new key for [`skill_source`]: 128 bits from the kernel, as hex.
fn new_skill_key() -> std::io::Result<String> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// The key a skill file was written with, read back out of its metadata.
fn skill_key(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.starts_with(SKILL_KEY))?;
    serde_json::from_str::<String>(line[SKILL_KEY.len()..].trim())
        .ok()
        .filter(|key| !key.is_empty())
}

/// The binary as the first word of a shell command, quoted only when it has
/// to be — the same rule the hooks' command lines follow.
fn command_word(exe: &str) -> String {
    command_line(exe, "", "")
}

/// The cctop a skill file names, read back out of its marker.
fn skill_exe(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.starts_with(SKILL_MARKER))?;
    serde_json::from_str::<String>(line[SKILL_MARKER.len()..].trim()).ok()
}

/// The file in the way of cctop's `/yolo`, when there is one: a skill at its
/// path that cctop did not write, or a command file of the same name, which
/// the skill would hide. Either is the person's own `/yolo`.
fn foreign_yolo(path: &Path, commands: &Path) -> Option<PathBuf> {
    match std::fs::read_to_string(path) {
        Ok(text) if skill_exe(&text).is_none() => return Some(path.to_path_buf()),
        Ok(_) => return None,
        Err(_) => {}
    }
    commands.exists().then(|| commands.to_path_buf())
}

/// Write `/yolo`, unless the person has a `/yolo` of their own.
fn skill_install(path: &Path, commands: &Path, exe: &str) -> anyhow::Result<()> {
    if let Some(theirs) = foreign_yolo(path, commands) {
        anyhow::bail!(
            "{} is a {SKILL_LABEL} of your own, so cctop's is not installed",
            theirs.display()
        );
    }
    let before = std::fs::read_to_string(path).ok();
    // The key a rewrite keeps, so that refreshing the file does not change
    // the command a turn in progress was granted.
    let key = match before.as_deref().and_then(skill_key) {
        Some(key) => key,
        None => new_skill_key()?,
    };
    let source = skill_source(exe, &key);
    // Already this, exactly: nothing to write. See [`json_install`].
    if before.is_some_and(|text| text == source) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = temp_beside(path, "md");
    std::fs::write(&tmp, source)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Delete cctop's `/yolo`, and its directory once that is empty, and say
/// whether there was one. Anything else at that path is left alone.
fn skill_remove(path: &Path) -> anyhow::Result<bool> {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
        Ok(text) if skill_exe(&text).is_none() => Ok(false),
        Ok(_) => {
            std::fs::remove_file(path)?;
            // Only when empty: `remove_dir` refuses anything else, which is
            // the point — a file the person put beside it stays.
            if let Some(dir) = path.parent() {
                let _ = std::fs::remove_dir(dir);
            }
            Ok(true)
        }
    }
}

/// What `/yolo` has to say.
///
/// Missing is only "not installed" when the hooks beside it are not cctop's
/// either. With them installed, it is an install from before `/yolo` existed
/// — short of something this version writes, which [`repair`] fills in the
/// way it fills in a hook event added since.
fn skill_health(path: &Path, commands: &Path, hooks: &Path) -> Health {
    if let Some(theirs) = foreign_yolo(path, commands) {
        return Health::Foreign(theirs);
    }
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            match json_health(hooks, Shape::Nested, CLAUDE_EVENTS, CLAUDE_DECIDING) {
                Health::Absent | Health::Unreadable(_) => Health::Absent,
                _ => Health::Partial(vec![SKILL_LABEL]),
            }
        }
        Err(e) => Health::Unreadable(e.to_string()),
        Ok(text) => {
            let exe = skill_exe(&text);
            // No key is an older form too: the rewrite gives it one.
            let outdated = exe.as_deref().is_some_and(|exe| {
                skill_key(&text).is_none_or(|key| text != skill_source(exe, &key))
            });
            verdict(exe, Vec::new(), outdated)
        }
    }
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// What one JSON settings file has to say about cctop.
fn json_health(
    path: &Path,
    shape: Shape,
    events: &[&'static str],
    deciding: &[&'static str],
) -> Health {
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
        let ours: Vec<&str> = hooks
            .and_then(|h| h.get(*event))
            .and_then(|v| v.as_array())
            .map(|list| list.as_slice())
            .unwrap_or_default()
            .iter()
            .flat_map(entry_commands)
            .filter(|c| is_our_command(c))
            .collect();
        if ours.is_empty() {
            missing.push(*event);
            continue;
        }
        let exe = ours.iter().find_map(|c| recorded_exe(c));
        // A command the installer would not write today — a spaced path left
        // bare by an older cctop — is counted as missing, so that `repair`
        // rewrites it instead of calling it installed while every fire fails.
        let has = |wanted: &dyn Fn(&str) -> String| {
            ours.iter()
                .any(|c| exe.as_deref().is_some_and(|e| *c == wanted(e)))
        };
        if !has(&|e| hook_command(e, event)) {
            missing.push(*event);
        }
        if deciding.contains(event) && !has(&|e| yolo_hook_command(e, event)) {
            missing.push(YOLO_HOOK_LABEL);
        }
        recorded = recorded.or(exe);
    }
    // Every event there and well formed, and still not necessarily what this
    // version writes: a timeout, a matcher, an event since dropped, a field an
    // older cctop added. Asked of the installer itself, at the binary the file
    // already names, so "outdated" can never drift from what an install does.
    let outdated = missing.is_empty()
        && recorded.as_deref().is_some_and(|exe| {
            let mut want = root.clone();
            json_merge(&mut want, path, shape, events, deciding, exe).is_ok()
                && our_entries(&want) != our_entries(&root)
        });
    verdict(recorded, missing, outdated)
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
            Some(argv @ [exe, ..]) => verdict(
                Some(exe.clone()),
                Vec::new(),
                *argv != codex_notify(exe)[..],
            ),
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
            //
            // A plugin written for the *other* OpenCode is short all of them,
            // and that is not a shortfall to be filled in by editing: the file
            // is replaced whole, which is what an install does anyway. The
            // difference is that it has to be reported at all. A V1 plugin on a
            // V2 server names this binary and every event cctop wants, and
            // forwards nothing, because OpenCode 2 does not load it — it says so
            // in a log line and carries on, and the session looks to cctop like
            // one that is merely quiet.
            //
            // A plugin carrying everything is still outdated when it is not
            // the text this version writes: the file is cctop's outright, so
            // any difference at all is an older cctop's.
            Some(exe) => {
                let outdated = text != plugin_source(&exe);
                verdict(
                    Some(exe),
                    plugin_shortfall(&text, crate::opencode::api()),
                    outdated,
                )
            }
        },
    }
}

/// Turn "cctop is recorded here, at this path, missing these events, in an
/// older form or not" into the one verdict every harness is reported with.
///
/// A shortfall outranks the form: filling it in is the same whole rewrite, so
/// the report names the more specific of the two.
fn verdict(recorded: Option<String>, missing: Vec<&'static str>, outdated: bool) -> Health {
    let own = own_exe().ok();
    match recorded {
        None => Health::Absent,
        Some(exe) if !Path::new(&exe).exists() => Health::Broken(exe),
        Some(exe) if Some(exe.as_str()) != own.as_deref() => match missing.is_empty() && outdated {
            true => Health::Outdated { exe: Some(exe) },
            false => Health::Other { exe, missing },
        },
        Some(_) if !missing.is_empty() => Health::Partial(missing),
        Some(_) if outdated => Health::Outdated { exe: None },
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
    /// Every event registered, but not in the form this version writes: a
    /// command line, an argument, a timeout, a matcher or any other field an
    /// older cctop wrote differently. `exe` is the other cctop the entries
    /// name, and `None` when they name this one.
    ///
    /// Its own state rather than a kind of [`Health::Partial`], because nothing
    /// is missing: every event fires. What it fires may be the old way of
    /// invoking cctop, which after an update is exactly what nobody would
    /// notice until something read wrong.
    Outdated { exe: Option<String> },
    /// Registered at a path that is gone. The hooks are firing nothing at all,
    /// and this is the one case worth fixing without being asked.
    Broken(String),
    /// The file could not be read, so nothing can be said about it.
    Unreadable(String),
    /// Something of the person's own is where cctop would write — their own
    /// `/yolo` — so cctop's is not installed, by choice rather than by fault.
    Foreign(PathBuf),
}

impl Health {
    /// Whether this is something the user would want to see rather than a
    /// working install or an absence they chose.
    pub fn is_problem(&self) -> bool {
        match self {
            Health::Partial(_)
            | Health::Outdated { .. }
            | Health::Broken(_)
            | Health::Unreadable(_) => true,
            Health::Other { missing, .. } => !missing.is_empty(),
            Health::Absent | Health::Installed | Health::Foreign(_) => false,
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
        // The skill on its own: the file is the whole of what is missing.
        Health::Partial(missing) if missing[..] == [SKILL_LABEL] => (
            format!("{SKILL_LABEL} missing; cctop writes it at its next start"),
            true,
        ),
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
        Health::Outdated { exe: None } => ("installed, in an older cctop's form".into(), true),
        Health::Outdated { exe: Some(exe) } => (
            format!("{exe} is installed here, in an older cctop's form"),
            true,
        ),
        Health::Broken(exe) => (format!("points at {exe}, which is gone"), true),
        Health::Unreadable(why) => (why.clone(), true),
        Health::Foreign(path) => (
            format!(
                "{} is your own {SKILL_LABEL}, so cctop's is not installed",
                path.display()
            ),
            false,
        ),
    }
}

/// What one pass of [`repair`] did, and what it could not.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Repair {
    /// One line per file rewritten, ready for a status line.
    pub fixed: Vec<String>,
    /// Whether something is still wrong that repair would not or could not
    /// fix: a file that will not parse, or a write that failed. Worth one line
    /// pointing at the panel; everything repair did fix is not.
    pub needs_attention: bool,
}

impl Repair {
    /// The pass as lines for a terminal, for the entry points that have no
    /// status line of their own to put it on. Empty when there was nothing to
    /// do, which is the ordinary case and not worth a line.
    pub fn terminal_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .fixed
            .iter()
            .map(|fixed| format!("cctop: hooks: {fixed}"))
            .collect();
        if self.needs_attention {
            lines.push("cctop: agent hooks need attention — run `cctop --hooks-status`".into());
        }
        lines
    }
}

/// Fix the three ways an install can look present and deliver less than it
/// should: a recorded cctop that is gone, a set of events that is short, and
/// entries in a form an older cctop wrote.
///
/// All three are cases where there is no behaviour to preserve and nothing the
/// user could have meant. A hook naming a binary that no longer exists fires
/// nothing. An install written by an older cctop registers the events *that*
/// version knew about, so every event added since is one the agent never
/// mentions — and the states those events close out are exactly the ones that
/// otherwise stick: a tool call that failed goes on reading as a held question,
/// a turn that died on an API error goes on reading as work in progress. And an
/// entry in an older form invokes cctop the way an older cctop wanted, which
/// after an update that changed it is a hook nobody would think to reinstall.
///
/// The one thing repair will not do is move an install between binaries. Two
/// cctops on one machine is a choice, and events are delivered to every cctop
/// listening whichever binary fires them — so a shortfall or an old form in an
/// install another cctop owns is fixed *at that binary*, leaving its choice
/// alone. Only an install naming a cctop that is gone is repointed, because
/// there is nothing left there to respect.
///
/// Meant to run at every start of anything long-lived, off the thread that
/// draws: when there is nothing to do it is one read per config file and no
/// write, and each write it does make is re-read, compared and renamed into
/// place (see [`json_install`]), so two cctops starting at once cannot tear a
/// file between them.
pub fn repair(cwd: Option<&Path>) -> Repair {
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
fn repair_in(scopes: &[Scope]) -> Repair {
    let Ok(own) = own_exe() else {
        return Repair::default();
    };
    let mut repair = Repair::default();
    for harness in HARNESSES {
        for scope in scopes {
            for config in harness.configs(scope) {
                let health = config.health();
                // Written whole either way: `install` replaces cctop's entries
                // for every event it wants, so topping up a short install,
                // refreshing an old one and repointing a dead one are the same
                // write with a different path.
                let (exe, what) = match &health {
                    Health::Broken(_) => (own.clone(), "repointed at this cctop".to_string()),
                    Health::Outdated { exe } => (
                        exe.clone().unwrap_or_else(|| own.clone()),
                        "brought up to date".to_string(),
                    ),
                    _ => match health.shortfall() {
                        None => {
                            repair.needs_attention |= health.is_problem();
                            continue;
                        }
                        Some((count, at)) => (
                            at.unwrap_or(&own).to_string(),
                            match config {
                                Config::Skill { .. } => format!("added {SKILL_LABEL}"),
                                _ => format!("filled in {count} missing hooks"),
                            },
                        ),
                    },
                };
                match config.install(&exe) {
                    Ok(_) => repair.fixed.push(format!(
                        "{} ({}): {what}",
                        harness.label(),
                        scope.label()
                    )),
                    Err(_) => repair.needs_attention = true,
                }
            }
        }
    }
    repair
}

#[cfg(test)]
mod tests {
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

    /// The whole point of the field: the pids the hook walked survive the wire
    /// and come back out of `parse` as the chain that was sent.
    #[test]
    fn the_process_tree_survives_the_wire() {
        let raw = br#"{"session_id":"s","hook_event_name":"Stop","cwd":"/w"}"#;
        let line = envelope("", raw, &[41, 42, 43]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap()).expect("parse");
        assert_eq!(event.pids, vec![41, 42, 43]);
    }

    /// A harness whose hook is not `cctop hook` reports no tree at all, and an
    /// empty claim must read as "no claim" rather than as an empty one.
    #[test]
    fn an_event_without_a_process_tree_claims_nothing() {
        let raw = br#"{"session_id":"s","hook_event_name":"Stop"}"#;
        let line = envelope("", raw, &[]).expect("envelope");
        assert!(
            parse(std::str::from_utf8(&line).unwrap())
                .unwrap()
                .pids
                .is_empty()
        );
        // And one that never carried the field at all.
        let event = parse(r#"{"session_id":"s","event":"Stop"}"#).expect("parse");
        assert!(event.pids.is_empty());
    }

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
        let line = envelope("", raw, &[]).expect("envelope");
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
            let line = envelope("", raw.as_bytes(), &[]).expect("envelope");
            let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
            assert!(event.finished_agent.is_none(), "{name} named a subagent");
        }
    }

    /// The envelope must survive the payload, whatever is in it: the fields are
    /// pulled out of somebody else's JSON and a missing one is normal.
    #[test]
    fn an_event_is_reduced_to_the_session_and_what_happened() {
        let raw = br#"{"session_id":"abc","cwd":"/x","hook_event_name":"Stop","extra":{"a":1}}"#;
        let line = envelope("", raw, &[]).expect("envelope");
        assert!(line.ends_with(b"\n"), "the wire is newline delimited");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.session_id, "abc");
        assert_eq!(event.reported.cwd, "/x");
        assert_eq!(event.reported.signal, Signal::Idle);

        // The argument wins, for a harness whose payload names events its way.
        let line = envelope("Notification", raw, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.signal, Signal::NeedsInput);

        // Junk in, nothing out — never a panic, and never a bogus event.
        assert!(envelope("Stop", b"not json", &[]).is_none());
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
        let line = envelope("", raw, &[]).expect("envelope");
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
            std::str::from_utf8(&envelope("", raw, &[]).expect("envelope"))
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
            std::str::from_utf8(&envelope("", raw, &[]).expect("envelope"))
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
            std::str::from_utf8(&envelope("", raw, &[]).expect("envelope"))
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
            std::str::from_utf8(&envelope("", raw, &[]).expect("envelope"))
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

    #[test]
    fn an_mcp_elicitation_is_a_session_waiting_on_you() {
        assert_eq!(signal_of("Elicitation", ""), Some(Signal::NeedsInput));
        assert_eq!(signal_of("ElicitationResult", ""), Some(Signal::Busy));
        assert!(CLAUDE_EVENTS.contains(&"Elicitation"));
    }

    /// One event Claude Code offers is never asked for, because installing it
    /// would break the session.
    ///
    /// `WorktreeCreate` replaces `git worktree` rather than observing it, and
    /// takes the new worktree's path from the hook's stdout. This hook writes
    /// nothing to stdout by construction, and a `WorktreeCreate` hook that
    /// produces no path fails worktree creation outright — so installing it
    /// would break `claude --worktree`, every `isolation: "worktree"` subagent,
    /// and every backgrounded session Claude Code isolates in one.
    ///
    /// Everything else Claude Code raises is registered, so that adding an
    /// event cctop cares about is a change to [`signal_of`] and not to
    /// everybody's settings file. This is the one that cannot join them.
    #[test]
    fn the_event_that_would_replace_git_is_never_installed() {
        assert!(
            !CLAUDE_EVENTS.contains(&"WorktreeCreate"),
            "installing WorktreeCreate makes worktree creation fail — see the \
             note above CLAUDE_EVENTS"
        );
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

    /// A permission prompt is held back; every other question is not.
    ///
    /// Claude Code's auto mode puts the request to a model first, which takes a
    /// few seconds, so the hook fires for prompts nobody is ever asked. An
    /// elicitation and a notification have no such second opinion coming and
    /// are shown at once.
    #[test]
    fn a_permission_prompt_waits_and_the_other_questions_do_not() {
        let of = |line: &str| parse(line).expect("a parseable event").reported;
        let event = |name: &str| format!(r#"{{"session_id":"abc","event":"{name}","cwd":"/tmp"}}"#);

        let asked = of(&event("PermissionRequest"));
        assert_eq!(asked.signal, Signal::NeedsInput);
        assert!(asked.provisional, "the prompt was not held");
        assert!(
            !asked.is_settled(),
            "a prompt shown the instant it was raised"
        );

        // Held, not lost: it becomes real when the grace runs out with no
        // answer having arrived.
        let matured = Reported {
            call: None,
            at: std::time::Instant::now() - (PERMISSION_GRACE + std::time::Duration::from_secs(1)),
            ..asked
        };
        assert!(matured.is_settled(), "a prompt held past its grace");

        for name in ["Elicitation", "Notification"] {
            let other = of(&event(name));
            assert!(
                other.is_settled(),
                "{name} was held back, and nothing is going to answer it for you"
            );
        }
        // And the answer itself, which is what replaces a held prompt before
        // anyone is told about it.
        let denied = of(&event("PermissionDenied"));
        assert_eq!(denied.signal, Signal::Busy);
        assert!(denied.is_settled());
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
            call: None,
            provisional: false,
            ask: None,
            question: false,
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

    /// An answer from the page reads back as the state the agent is left in:
    /// working on the tool it was allowed, or waiting on you after a denial
    /// ended its turn.
    #[test]
    fn an_answer_from_the_page_reads_back_as_what_it_left() {
        let read = |allowed| {
            let line = String::from_utf8(answer_line("s-1", allowed)).unwrap();
            let event = parse(line.trim()).expect("a parseable event");
            assert_eq!(event.session_id, "s-1");
            event.reported.signal
        };
        assert_eq!(read(true), Signal::Busy);
        assert_eq!(read(false), Signal::Idle);
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

    /// A permission request also carries its tool and whole command,
    /// redacted, through the socket to the reader — and keeps it through the
    /// nameless notification Claude Code sends after it. Nothing else carries
    /// one.
    #[test]
    fn a_permission_request_carries_its_call() {
        let raw = br#"{"session_id":"s-1","cwd":"/w","hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"TOKEN=dummy make\nmake install"}}"#;
        let line = envelope("PermissionRequest", raw, &[]).expect("envelope");
        let text = std::str::from_utf8(&line).unwrap();
        let sent: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert!(!sent["call"].to_string().contains("dummy"), "{text}");
        let event = parse(text.trim()).expect("parse");
        let call = event.reported.call.clone().expect("a call");
        assert_eq!(call.tool, "Bash");
        assert_eq!(
            call.detail.as_deref(),
            Some("TOKEN=[redacted] make\nmake install")
        );

        let mut reports = Reports::default();
        reports.observe(&event);
        let note = br#"{"session_id":"s-1","cwd":"/w","hook_event_name":"Notification","notification_type":"permission_prompt"}"#;
        let line = envelope("Notification", note, &[]).expect("envelope");
        let note = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(note.reported.call, None);
        reports.observe(&note);
        assert_eq!(reports.report("s-1").unwrap().call, Some(call));

        let tool = br#"{"session_id":"s-1","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let line = envelope("PreToolUse", tool, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.call, None);
    }

    /// End to end, from the bytes Claude Code actually writes to the hook.
    #[test]
    fn an_idle_nudge_arrives_as_a_finished_turn() {
        let raw = br#"{"session_id":"s-1","transcript_path":"/t.jsonl","cwd":"/w","hook_event_name":"Notification","message":"Claude is waiting for your input","title":"Claude Code","notification_type":"idle_prompt"}"#;
        let line = envelope("Notification", raw, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.signal, Signal::Idle);
    }

    /// A `cctop sandbox` session says where it works on every event; the row
    /// keeps it through an event that did not say (an older hook binary), and
    /// loses it when the session ends.
    #[test]
    fn a_sandboxed_session_is_stamped_with_its_host() {
        let line = |event: &str, sandbox: &str| {
            parse(&format!(
                r#"{{"event":"{event}","session_id":"s-1","cwd":"/srv","sandbox":"{sandbox}"}}"#
            ))
            .expect("parse")
        };
        let mut reports = Reports::default();
        reports.observe(&line("PreToolUse", "box:/srv"));
        reports.observe(&line("PostToolUse", ""));
        let mut row = crate::session::Session::new(crate::pricing::Provider::Claude, "s-1".into());
        reports.stamp_sandbox(&mut row);
        assert_eq!(row.sandbox.as_deref(), Some("box:/srv"));

        reports.observe(&line("SessionEnd", ""));
        assert!(reports.sandboxes.is_empty());
        let mut other =
            crate::session::Session::new(crate::pricing::Provider::Claude, "s-2".into());
        reports.stamp_sandbox(&mut other);
        assert_eq!(other.sandbox, None);
    }

    /// The permission mode rides in on every Claude Code event, and it is the
    /// one fact here that no transcript records.
    #[test]
    fn a_session_reports_how_much_it_asks_before_it_acts() {
        let raw = br#"{"session_id":"s-1","cwd":"/w","hook_event_name":"PreToolUse","permission_mode":"bypassPermissions","tool_name":"Bash"}"#;
        let line = envelope("PreToolUse", raw, &[]).expect("envelope");
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
        let line = envelope("Stop", raw, &[]).expect("envelope");
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
    /// Regression: an entry was dropped whole if any command in it was cctop's,
    /// so a user's own command added into the same wrapper went with it on the
    /// next install, repair or remove.
    #[test]
    fn a_users_command_sharing_a_wrapper_with_ours_survives() {
        let dir = scratch("hooks-shared");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = serde_json::json!({"type": "command", "command": "notify-send done"});
        let settings = serde_json::json!({
            "hooks": {"Stop": [{"hooks": [
                {"type": "command", "command": "/old/cctop hook Stop"},
                theirs,
            ]}]}
        });
        std::fs::write(&path, settings.to_string()).unwrap();

        let commands = |path: &Path| -> Vec<String> {
            let root = read_settings(path).unwrap();
            let list = root["hooks"]["Stop"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            list.iter()
                .flat_map(|e| entry_commands(e).map(str::to_string).collect::<Vec<_>>())
                .collect()
        };
        install(&scope);
        assert!(commands(&path).contains(&"notify-send done".to_string()));
        remove(&scope);
        assert_eq!(commands(&path), ["notify-send done"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_with_a_space_is_quoted_for_the_shell_and_read_back() {
        let spaced = "/home/My User/.cargo/bin/cctop";
        let command = hook_command(spaced, "Stop");
        assert_eq!(command, "'/home/My User/.cargo/bin/cctop' hook Stop");
        // What the harness does with it: hand it to a shell, which must see one
        // program, not `/home/My` with arguments.
        let words = std::process::Command::new("sh")
            .args(["-c", &format!("set -- {command}; echo \"$#|$1\"")])
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&words.stdout).trim(),
            format!("3|{spaced}")
        );
        assert_eq!(recorded_exe(&command).as_deref(), Some(spaced));

        let quote = "/opt/it's/cctop";
        assert_eq!(
            recorded_exe(&hook_command(quote, "Stop")).as_deref(),
            Some(quote)
        );
        // A path the shell reads as-is stays bare, so a working install is
        // not rewritten.
        assert_eq!(
            hook_command("/usr/local/bin/cctop", "Stop"),
            "/usr/local/bin/cctop hook Stop"
        );
    }

    #[test]
    fn a_spaced_path_an_older_cctop_left_bare_counts_as_missing() {
        let dir = scratch("hooks-spaced");
        let bin = dir.join("My Tools");
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("cctop");
        std::fs::write(&exe, "").unwrap();
        let path = dir.join("settings.json");
        let bare = format!("{} hook Stop", exe.display());
        let settings = serde_json::json!({"hooks": {"Stop": [{"hooks": [
            {"type": "command", "command": bare},
        ]}]}});
        std::fs::write(&path, settings.to_string()).unwrap();

        match json_health(&path, Shape::Nested, &["Stop"], &[]) {
            Health::Other {
                exe: found,
                missing,
            } => {
                assert_eq!(found, exe.display().to_string());
                assert_eq!(missing, ["Stop"]);
            }
            other => panic!("expected the bare entry to be flagged, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_symlinked_settings_file_stays_a_symlink() {
        let dir = scratch("hooks-link");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let dotfiles = dir.join("dotfiles");
        std::fs::create_dir_all(&dotfiles).unwrap();
        let real = dotfiles.join("settings.json");
        std::fs::write(&real, r#"{"model": "opus"}"#).unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();

        install(&scope);
        assert!(path.is_symlink(), "the link was replaced by a plain file");
        let written = read_settings(&real).unwrap();
        assert_eq!(written["model"], "opus");
        assert!(
            written.contains_key("hooks"),
            "the hooks never reached the target"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_round_trip_keeps_the_users_key_order() {
        let dir = scratch("hooks-order");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Deliberately not alphabetical, and `hooks` is not last, so a sorted
        // map and a swap-remove would each move something.
        let theirs = r#"{"zeta": 1, "hooks": {}, "model": "opus", "alpha": {"y": 1, "b": 2}}"#;
        std::fs::write(&path, theirs).unwrap();
        let keys =
            |path: &Path| -> Vec<String> { read_settings(path).unwrap().keys().cloned().collect() };

        install(&scope);
        assert_eq!(keys(&path), ["zeta", "hooks", "model", "alpha"]);
        let alpha: Vec<String> = read_settings(&path).unwrap()["alpha"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(alpha, ["y", "b"]);
        remove(&scope);
        assert_eq!(keys(&path), ["zeta", "model", "alpha"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        for event in wanted(crate::opencode::api()) {
            assert!(text.contains(event), "the plugin forwards no {event}");
        }
        // A plugin from an older cctop forwards fewer events, and cctop owns the
        // whole file — so it is a shortfall to be filled in rather than
        // something to leave alone. Nothing else notices: the exe line, which is
        // all health used to read, is identical. `session.compacted` rather than
        // one of the tool moments, because which of those a file is expected to
        // register is up to the OpenCode that is installed.
        std::fs::write(&path, text.replace("session.compacted", "session.gone")).unwrap();
        assert!(
            Harness::OpenCode.health(&scope).unwrap().is_problem(),
            "a plugin missing an event read as fine"
        );
        assert!(!repair_in(std::slice::from_ref(&scope)).fixed.is_empty());
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

    /// One file, both OpenCodes.
    ///
    /// This is what makes an upgrade a non-event: the file cctop installed when
    /// the machine was on one version is still the right file on the other, so
    /// an OpenCode upgraded under a cctop that never runs again keeps reporting
    /// instead of going quiet until somebody reinstalls the hooks.
    ///
    /// The shape is the one OpenCode's own migration guide documents — a named
    /// export for version 1, a default export carrying `setup` for version 2 and
    /// `server` for version 1.18.29 and newer — and it was checked against real
    /// binaries rather than assumed: 1.14.25 loads the named export, 1.18.33 calls
    /// `server` *and* `setup` (with a context carrying none of version 2's
    /// domains, which is why the body is guarded), and 2.0.20 calls `setup` alone,
    /// so a version 2 session is never reported twice.
    #[test]
    fn one_plugin_file_carries_both_open_codes() {
        let text = plugin_source("/usr/bin/cctop");
        // Both entrypoints, and the V1 function reachable from both.
        assert!(
            text.contains("export const cctop = cctopV1"),
            "1.x loads a named export"
        );
        assert!(
            text.contains("export default { id: \"cctop\", setup, server: cctopV1 }"),
            "2 loads setup, 1.18.29+ loads server"
        );
        assert!(
            text.contains("export default"),
            "a V2 plugin default-exports"
        );
        assert!(text.contains("id: \"cctop\""), "and names itself");
        // Each half reads its own version's payload key, and neither reads the
        // other's: the wrong key is how a port reports nothing at all.
        assert!(
            text.contains("event.properties"),
            "the V1 half reads `properties`"
        );
        assert!(text.contains("event?.data"), "the V2 half reads `data`");
        // One spawn, shared, so there is one thing to be wrong about.
        assert_eq!(
            text.matches("node:child_process").count(),
            1,
            "the two halves share one way of handing an event over"
        );
        assert!(text.contains("const CCTOP = \"/usr/bin/cctop\""));
        assert!(text.contains(&format!("\"hook\", \"{OPENCODE_SELECTOR}\"")));
    }

    /// The file OpenCode 2 loads is a different dialect from the one OpenCode 1
    /// loads, and only one of the two names every event cctop wants: the tool
    /// moments are hooks there, so they are registered as `execute.before` and
    /// reported as the names the rest of cctop reads.
    #[test]
    fn the_plugin_for_opencode_two_is_written_in_the_api_it_loads() {
        let text = plugin_source("/usr/bin/cctop");
        assert!(
            text.contains(PLUGIN_DEFAULT_EXPORT),
            "a V2 plugin default-exports"
        );
        assert!(text.contains("id: \"cctop\""), "and names itself");
        assert!(text.contains("ctx.event?.subscribe"), "it subscribes");
        assert!(
            text.contains("event?.data"),
            "V2 payloads are under `data`, where 1 had `properties`"
        );
        assert!(text.contains("ctx.tool.hook(\"execute.before\""));
        assert!(text.contains("ctx.tool.hook(\"execute.after\""));
        for event in wanted(crate::opencode::Api::V2) {
            assert!(text.contains(event), "the plugin registers no {event}");
        }
        // The stream carries four of them; the asking and the tool moments come
        // from the API and the hooks, which is why they are not in this set.
        let streamed = text
            .split("const STREAMED = new Set([")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .unwrap_or_default();
        assert_eq!(
            streamed,
            r#""session.idle","session.created","session.deleted","session.compacted""#
        );
    }

    /// The one line that is per-install, and the one way the file can come out
    /// wrong in a way nothing else would notice: a plugin that loaded, said
    /// nothing, and looked installed — because it still had the placeholder
    /// where the binary's path should be.
    #[test]
    fn the_binarys_path_is_substituted_and_leaves_nothing_behind() {
        assert_eq!(
            PLUGIN.matches(PLUGIN_PATH).count(),
            1,
            "one placeholder, and only one, or the substitution is a guess"
        );
        for exe in ["/usr/local/bin/cctop", "/home/some body/bin/cctop"] {
            let text = plugin_source(exe);
            assert!(!text.contains(PLUGIN_PATH), "{exe} left the placeholder in");
            assert!(
                !text.contains("$CCTOP"),
                "a path that is not quoted JSON cannot be read back"
            );
            // A path with a quote or a backslash in it has to survive, because
            // health reads this line back and a splice inside a quoted string
            // would not.
            assert_eq!(plugin_exe(&text).as_deref(), Some(exe));
        }
    }

    /// The version 1 half, on its own, is the plugin OpenCode 1 has always
    /// loaded: a named export of a function returning its hooks, reading the
    /// payload key 1 puts it under.
    #[test]
    fn the_version_one_half_is_the_plugin_opencode_one_has_always_loaded() {
        let text = plugin_source_for("/usr/bin/cctop", crate::opencode::Api::V1);
        assert!(text.contains("export const cctop = cctopV1"));
        assert!(text.contains("event.properties"));
        assert!(!text.contains("export default"));
        for event in OPENCODE_EVENTS {
            assert!(text.contains(event), "the plugin forwards no {event}");
        }
    }

    /// The failure a single-dialect file has when it meets the other OpenCode:
    /// it names the right binary and every event cctop wants, and forwards
    /// nothing, because the server does not load it. Nothing else can see that,
    /// so health has to — and the file cctop writes now cannot be in that state.
    #[test]
    fn a_single_dialect_file_is_short_of_everything_on_the_other_version() {
        let v1 = plugin_source_for("/usr/bin/cctop", crate::opencode::Api::V1);
        let v2 = plugin_source_for("/usr/bin/cctop", crate::opencode::Api::V2);
        let both = plugin_source("/usr/bin/cctop");
        assert_eq!(plugin_shape(&v1), PluginShape::V1);
        assert_eq!(plugin_shape(&v2), PluginShape::V2);
        assert_eq!(plugin_shape(&both), PluginShape::Both);
        // Each single-dialect file is right for its own OpenCode...
        assert!(plugin_shortfall(&v1, crate::opencode::Api::V1).is_empty());
        assert!(plugin_shortfall(&v2, crate::opencode::Api::V2).is_empty());
        // ...and forwards nothing on the other, which is what gets it rewritten.
        assert_eq!(
            plugin_shortfall(&v1, crate::opencode::Api::V2),
            wanted(crate::opencode::Api::V2)
        );
        assert_eq!(
            plugin_shortfall(&v2, crate::opencode::Api::V1),
            wanted(crate::opencode::Api::V1)
        );
        // The file cctop writes is right on both, and carries every name.
        for api in [crate::opencode::Api::V1, crate::opencode::Api::V2] {
            assert!(
                plugin_shortfall(&both, api).is_empty(),
                "the file cctop writes is short on {api:?}"
            );
        }
        for api in [crate::opencode::Api::V1, crate::opencode::Api::V2] {
            for event in wanted(api) {
                assert!(both.contains(event), "the plugin carries no {event}");
            }
        }
    }

    /// The plugin for OpenCode 2 is written in the API that OpenCode 2 has.
    ///
    /// This is the part a translation cannot do. OpenCode 2 raises no event
    /// when it stops to ask — not for a permission, not for a question, not
    /// when either is answered — so a file that only listened to the stream
    /// would report a blocked session as a working one, which is the state a
    /// held prompt has always been hardest to see.
    #[test]
    fn the_plugin_for_opencode_two_asks_the_api_what_is_waiting() {
        let text = plugin_source("/usr/bin/cctop");
        assert!(
            text.contains("ctx.permission.list"),
            "the asking is a row in an API, not an event"
        );
        assert!(text.contains("report(\"permission.asked\""));
        assert!(
            text.contains("report(\"permission.replied\""),
            "nothing in OpenCode 2 says a prompt was answered"
        );
        // And the question is the same fact as the permission: 2 counts an
        // agent's question as a request whose action is `question`.
        assert!(text.contains("askOf"));
    }

    /// A prompt is said once, an answer once, and a session that has gone quiet
    /// stops being asked about — unless something is still pending, which is
    /// the whole reason to keep asking.
    #[test]
    fn the_poll_says_each_moment_once_and_forgets() {
        let text = plugin_source_for("/usr/bin/cctop", crate::opencode::Api::V2);
        assert!(text.contains("const POLL_MS = 1000"));
        assert!(text.contains("const FORGET_MS = 120000"));
        assert!(
            text.contains("if (now - at > FORGET_MS && !pending0?.size)"),
            "a prompt left up is still up, and cctop's claim has to keep being true"
        );
        // Told what it has already said, so a request that stays pending is not
        // re-announced once a second.
        assert!(text.contains("const told = waiting.get(sessionID)"));
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

    /// The moment a prompt is answered, which for OpenCode 2 cctop has to be
    /// told: a tab that keeps asking about a question that was answered two
    /// minutes ago is worse than one that never noticed.
    #[test]
    fn an_answered_prompt_says_the_agent_is_working_again() {
        assert_eq!(
            signal_of("permission.replied", ""),
            Some(Signal::Busy),
            "the answer closes the prompt out, the way a tool call does"
        );
        assert_eq!(signal_of("permission.rejected", ""), Some(Signal::Busy));
        assert_eq!(signal_of("permission.asked", ""), Some(Signal::NeedsInput));
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
        // A whole install, the deciding entry included, so the one thing
        // repair could find wrong with it is where it points.
        let write_pointing_at = |exe: &Path| {
            let hooks: serde_json::Map<String, serde_json::Value> = CLAUDE_EVENTS
                .iter()
                .map(|e| {
                    let mut entries = vec![serde_json::json!({"hooks": [{"type": "command",
                        "command": format!("{} hook {e}", exe.display())}]})];
                    if CLAUDE_DECIDING.contains(e) {
                        entries.push(serde_json::json!({"hooks": [{"type": "command",
                            "command": format!("{} yolo-hook {e}", exe.display())}]}));
                    }
                    ((*e).to_string(), serde_json::Value::Array(entries))
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
            !repair_in(std::slice::from_ref(&scope)).fixed.is_empty(),
            "a dead path was not repaired"
        );
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);

        // A path that exists is another install, whoever made it.
        let other = dir.join("other-cctop");
        std::fs::write(&other, "").unwrap();
        write_pointing_at(&other);
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(
            repair_in(std::slice::from_ref(&scope)).fixed.is_empty(),
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
            !repair_in(std::slice::from_ref(&scope)).fixed.is_empty(),
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
        assert!(repair_in(std::slice::from_ref(&scope)).fixed.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A whole Claude Code install at `exe`, in a form an older cctop might
    /// have written: every event and the deciding entry there, each wrapper
    /// carrying a matcher and every command a timeout this version does not
    /// write — the shape of a hook whose invocation changed under an update.
    fn old_form_install(exe: &str) -> serde_json::Value {
        let hooks: serde_json::Map<String, serde_json::Value> = CLAUDE_EVENTS
            .iter()
            .map(|e| {
                let mut commands = vec![hook_command(exe, e)];
                if CLAUDE_DECIDING.contains(e) {
                    commands.push(yolo_hook_command(exe, e));
                }
                let entries = commands
                    .into_iter()
                    .map(|c| {
                        serde_json::json!({"matcher": "*", "hooks": [{"type": "command",
                            "command": c, "timeout": 5}]})
                    })
                    .collect();
                ((*e).to_string(), serde_json::Value::Array(entries))
            })
            .collect();
        serde_json::json!({ "hooks": hooks })
    }

    /// The issue's case: an update changed how a hook is written, and the
    /// entries an older cctop wrote — every event present, nothing missing —
    /// are rewritten into this version's form without anyone reinstalling.
    /// The user's own hooks, in the same event and in the same wrapper as
    /// cctop's, come through untouched.
    #[test]
    fn an_install_in_an_older_form_is_refreshed_and_the_users_hooks_survive() {
        let dir = scratch("outdated");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let own = own_exe().unwrap();

        let mut doc = old_form_install(&own);
        let stop = doc["hooks"]["Stop"].as_array_mut().unwrap();
        // The user's own entry, beside cctop's.
        stop.push(
            serde_json::json!({"hooks": [{"type": "command", "command": "notify-send done"}]}),
        );
        // And one of theirs in the wrapper cctop's sits in.
        stop[0]["hooks"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type": "command", "command": "say stopped"}));
        // An event this cctop does not register, holding one of theirs.
        doc["hooks"]["NotACctopEvent"] =
            serde_json::json!([{"hooks": [{"type": "command", "command": "beep"}]}]);
        // And an event an older cctop registered and this one no longer does.
        doc["hooks"]["Retired"] = serde_json::json!([{"hooks": [{"type": "command",
            "command": hook_command(&own, "Retired")}]}]);
        std::fs::write(&path, doc.to_string()).unwrap();

        let health = Harness::Claude.health(&scope).unwrap();
        assert_eq!(health, Health::Outdated { exe: None });
        assert!(health.is_problem(), "an old form read as fine");

        let repair = repair_in(std::slice::from_ref(&scope));
        assert_eq!(
            repair.fixed,
            vec!["Claude Code (project): brought up to date"]
        );
        assert!(!repair.needs_attention);
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);

        let root = read_settings(&path).unwrap();
        // cctop's entries are exactly what an install writes now.
        let mut fresh = serde_json::Map::new();
        json_merge(
            &mut fresh,
            &path,
            Shape::Nested,
            CLAUDE_EVENTS,
            CLAUDE_DECIDING,
            &own,
        )
        .unwrap();
        assert_eq!(our_entries(&root), our_entries(&fresh));
        // Every one of the user's survived, the wrapper's matcher included.
        let all: Vec<&str> = root["hooks"]
            .as_object()
            .unwrap()
            .values()
            .flat_map(|list| list.as_array().unwrap())
            .flat_map(entry_commands)
            .collect();
        for theirs in ["notify-send done", "say stopped", "beep"] {
            assert!(all.contains(&theirs), "{theirs} was lost: {all:?}");
        }
        let shared = root["hooks"]["Stop"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| entry_commands(e).any(|c| c == "say stopped"))
            .unwrap();
        assert_eq!(shared["matcher"], "*", "the user's wrapper was rewritten");
        assert!(
            root["hooks"].get("Retired").is_none(),
            "a retired event was left"
        );

        // And the second pass has nothing to do.
        assert!(repair_in(std::slice::from_ref(&scope)).fixed.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Repair runs at every start, so the ordinary case — everything already
    /// right — must cost a read and nothing else: no rewrite, so not even the
    /// mtime moves. The user's formatting is part of what that keeps.
    #[test]
    fn an_install_already_up_to_date_is_not_rewritten() {
        let dir = scratch("up-to-date");
        let scope = Scope::Project(dir.clone());
        for harness in HARNESSES {
            harness.install(&scope, &own_exe().unwrap());
        }
        let claude = Harness::Claude.config_file(&scope).unwrap();
        // Reformatted by hand, and moved after somebody else's entry: neither
        // is a difference in what fires.
        let mut root = read_settings(&claude).unwrap();
        root["hooks"]["Stop"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"hooks": [{"type": "command", "command": "theirs"}]}));
        root["hooks"]["Stop"].as_array_mut().unwrap().reverse();
        std::fs::write(&claude, serde_json::to_string(&root).unwrap()).unwrap();

        let files: Vec<PathBuf> = HARNESSES
            .iter()
            .flat_map(|h| h.configs(&scope))
            .map(|c| c.path().to_path_buf())
            .collect();
        let long_ago =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
        for file in &files {
            std::fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_modified(long_ago)
                .unwrap();
        }
        let before: Vec<Vec<u8>> = files.iter().map(|f| std::fs::read(f).unwrap()).collect();

        let repair = repair_in(std::slice::from_ref(&scope));
        assert_eq!(
            repair,
            Repair::default(),
            "an up-to-date install was touched"
        );
        for (file, bytes) in files.iter().zip(&before) {
            assert_eq!(
                &std::fs::read(file).unwrap(),
                bytes,
                "{} changed",
                file.display()
            );
            assert_eq!(
                std::fs::metadata(file).unwrap().modified().unwrap(),
                long_ago,
                "{} was rewritten",
                file.display()
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Another cctop's install in an older form is refreshed *at that cctop*:
    /// repair keeps its rule of never moving an install between live binaries.
    #[test]
    fn another_cctops_outdated_install_is_refreshed_at_its_own_binary() {
        let dir = scratch("outdated-other");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = dir.join("their-cctop");
        std::fs::write(&theirs, "").unwrap();
        let theirs = theirs.display().to_string();
        std::fs::write(&path, old_form_install(&theirs).to_string()).unwrap();

        assert_eq!(
            Harness::Claude.health(&scope).unwrap(),
            Health::Outdated {
                exe: Some(theirs.clone())
            }
        );
        assert!(!repair_in(std::slice::from_ref(&scope)).fixed.is_empty());
        assert_eq!(
            Harness::Claude.health(&scope).unwrap(),
            Health::Other {
                exe: theirs.clone(),
                missing: Vec::new()
            },
            "the form was refreshed, but the install changed hands"
        );
        let mut fresh = serde_json::Map::new();
        json_merge(
            &mut fresh,
            &path,
            Shape::Nested,
            CLAUDE_EVENTS,
            CLAUDE_DECIDING,
            &theirs,
        )
        .unwrap();
        assert_eq!(
            our_entries(&read_settings(&path).unwrap()),
            our_entries(&fresh)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The plugin is cctop's file outright, so any difference from what this
    /// version writes is an older cctop's, even with every event in it.
    #[test]
    fn an_opencode_plugin_from_an_older_cctop_is_rewritten() {
        let dir = scratch("outdated-plugin");
        let scope = Scope::Project(dir.clone());
        let path = Harness::OpenCode.config_file(&scope).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let own = own_exe().unwrap();
        std::fs::write(
            &path,
            format!("{}\n// an older cctop\n", plugin_source(&own)),
        )
        .unwrap();

        assert_eq!(
            Harness::OpenCode.health(&scope).unwrap(),
            Health::Outdated { exe: None }
        );
        assert!(!repair_in(std::slice::from_ref(&scope)).fixed.is_empty());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), plugin_source(&own));
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
            recorded_exe("/Applications/My Tools/cctop hook Stop").as_deref(),
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
        let line = envelope("PermissionRequest", raw, &[]).expect("envelope");
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

    /// The hook's allow, landing on either side of the observer's report of
    /// the same prompt, leaves the session working — and a prompt it did not
    /// answer still asks.
    #[test]
    fn a_prompt_the_yolo_hook_allowed_never_reads_as_asking() {
        let raised = |ask: &str| {
            parse(&format!(
                r#"{{"session_id":"s","event":"PermissionRequest","cwd":"/w","ask":"{ask}"}}"#
            ))
            .unwrap()
        };
        let allowed = |ask: Option<&str>| {
            let line = yolo_allowed_line("s", None, ask);
            parse(std::str::from_utf8(&line).unwrap()).unwrap()
        };

        // The allow first, then the report it raced.
        let mut reports = Reports::default();
        reports.observe(&allowed(Some("Bash: ls")));
        reports.observe(&raised("Bash: ls"));
        assert_eq!(reports.report("s").unwrap().signal, Signal::Busy);
        // Used up: the next prompt in the same words asks.
        reports.observe(&raised("Bash: ls"));
        assert_eq!(reports.report("s").unwrap().signal, Signal::NeedsInput);

        // The report first, then the allow.
        let mut reports = Reports::default();
        reports.observe(&raised("Bash: ls"));
        reports.observe(&allowed(Some("Bash: ls")));
        let now = reports.report("s").unwrap();
        assert_eq!((now.signal, now.ask.as_deref()), (Signal::Busy, None));
        // Nothing remembered from an allow that came second.
        reports.observe(&raised("Bash: ls"));
        assert_eq!(reports.report("s").unwrap().signal, Signal::NeedsInput);

        // A prompt in other words is not the one allowed.
        let mut reports = Reports::default();
        reports.observe(&allowed(Some("Bash: ls")));
        reports.observe(&raised("Bash: rm -rf /"));
        assert_eq!(reports.report("s").unwrap().signal, Signal::NeedsInput);

        // A prompt that named nothing, allowed as nothing.
        let mut reports = Reports::default();
        reports.observe(&allowed(None));
        reports.observe(
            &parse(r#"{"session_id":"s","event":"PermissionRequest","cwd":"/w"}"#).unwrap(),
        );
        assert_eq!(reports.report("s").unwrap().signal, Signal::Busy);
    }

    /// A subagent's prompt the hook allowed is closed out by the allow, so the
    /// session does not go on asking on its behalf.
    #[test]
    fn a_subagents_prompt_the_yolo_hook_allowed_is_closed() {
        let mut reports = Reports::default();
        reports.observe(
            &parse(
                r#"{"session_id":"s","event":"PermissionRequest","cwd":"/w","agent_id":"sub","ask":"Bash: ls"}"#,
            )
            .unwrap(),
        );
        assert_eq!(reports.report("s").unwrap().signal, Signal::NeedsInput);
        let line = yolo_allowed_line("s", Some("sub"), Some("Bash: ls"));
        reports.observe(&parse(std::str::from_utf8(&line).unwrap()).unwrap());
        assert_eq!(reports.report("s").unwrap().signal, Signal::Busy);
        assert!(
            reports
                .asking_agents
                .get("s")
                .is_none_or(|open| open.is_empty())
        );
    }

    /// Claude Code gets the deciding entry beside the observer, for
    /// `PermissionRequest` alone; health notices it missing, repair puts it
    /// back, and uninstall takes both.
    #[test]
    fn the_yolo_hook_is_installed_beside_the_observer_and_removed_with_it() {
        let dir = scratch("yolo-install");
        let scope = Scope::Project(dir.clone());
        let path = Harness::Claude.config_file(&scope).unwrap();
        install(&scope);
        let exe = own_exe().unwrap();
        let commands = |event: &str| -> Vec<String> {
            read_settings(&path).unwrap()["hooks"][event]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|e| entry_commands(e).map(str::to_string).collect::<Vec<_>>())
                .collect()
        };
        assert_eq!(
            commands("PermissionRequest"),
            [
                hook_command(&exe, "PermissionRequest"),
                yolo_hook_command(&exe, "PermissionRequest")
            ]
        );
        for event in CLAUDE_EVENTS.iter().filter(|e| **e != "PermissionRequest") {
            assert_eq!(commands(event), [hook_command(&exe, event)], "{event}");
        }
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);
        // Nobody else decides: no other harness was given one.
        for harness in [Harness::Gemini, Harness::Cursor, Harness::Codex] {
            if let Some(file) = harness.config_file(&scope) {
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                assert!(!text.contains(YOLO_MARKER.trim()), "{}", harness.label());
            }
        }

        // An install from before the hook: health says what is short, and
        // repair fills it in.
        let mut root = read_settings(&path).unwrap();
        root["hooks"]["PermissionRequest"]
            .as_array_mut()
            .unwrap()
            .retain(|e| !entry_commands(e).any(|c| c.contains(YOLO_MARKER)));
        write_settings(&path, &root).unwrap();
        assert_eq!(
            Harness::Claude.health(&scope).unwrap(),
            Health::Partial(vec![YOLO_HOOK_LABEL])
        );
        repair_in(std::slice::from_ref(&scope));
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Installed);
        assert_eq!(commands("PermissionRequest").len(), 2, "repair doubled up");

        remove(&scope);
        assert_eq!(Harness::Claude.health(&scope).unwrap(), Health::Absent);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("hook"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The deciding command is recognised as cctop's, under any binary name,
    /// and the path read back out of it.
    #[test]
    fn a_yolo_hook_command_is_recognised_as_ours() {
        for exe in ["/usr/local/bin/cctop", "/Applications/My Tools/cctop-0.29"] {
            let command = yolo_hook_command(exe, "PermissionRequest");
            assert!(is_our_command(&command), "{command}");
            assert_eq!(recorded_exe(&command).as_deref(), Some(exe));
        }
        assert!(!is_our_command(
            "/usr/bin/other yolo-hook PermissionRequest"
        ));
    }
}
