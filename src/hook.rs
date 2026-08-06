//! Hook events pushed from a live agent into a running cctop.
//!
//! Everything else cctop knows is forensic: it walks transcripts written after
//! the fact and re-derives state from them. That works, but it cannot see the
//! two moments that matter most — an agent finishing its turn and an agent
//! blocking on a question — because a transcript records "answered you" and
//! "still thinking" identically. Claude Code's hooks say both outright.
//!
//! Two halves live here. [`emit`] is `cctop hook`, the command the agent spawns;
//! [`Listener`] is the socket the running UI holds open. The wire between them
//! is one JSON line per event, which is what the agent already hands the hook on
//! stdin.
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
//! panic hook turns even an unexpected unwind into a silent success. cctop being absent,
//! stopped, or mid-crash is the *ordinary* case, not an error worth reporting.

use std::io::{Read, Write};
use std::path::PathBuf;

/// Longest `cctop hook` may take, start to finish, whatever it is doing.
///
/// The agent is blocked for this long on every hook fire, so it buys nothing to
/// be generous: a local socket with a reader attached answers in microseconds
/// (measured at 1–2ms including the process spawn), and anything slower than
/// this is a cctop that cannot keep up, whose events are better dropped.
const DEADLINE: std::time::Duration = std::time::Duration::from_millis(250);

/// Cap on a single event, so a pathological payload cannot be read forever.
/// Claude Code's hook input is a small object; a transcript never comes through
/// here, only its path.
const MAX_EVENT: u64 = 256 * 1024;

/// The socket a running cctop listens on for hook events.
///
/// One per user rather than one per session: a hook knows which *agent session*
/// fired it, but nothing about which cctop should hear about it, so there is a
/// single well-known address and whichever cctop holds it does the listening.
pub fn socket_path() -> Option<PathBuf> {
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .map(|d| d.join("cctop").join("hooks.sock"))
}

/// `cctop hook [name]` — forward one hook event to a running cctop.
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

fn forward(args: &[String]) {
    let mut payload = Vec::new();
    // The event arrives on stdin as JSON. Bounded, and a read error just means
    // there is nothing to forward.
    if std::io::stdin()
        .take(MAX_EVENT)
        .read_to_end(&mut payload)
        .is_err()
    {
        return;
    }
    // The event name is in the payload as `hook_event_name`, but an installer
    // can also pass it as an argument — taking it from the command line means a
    // harness whose payload spells it differently still lands in the right bin.
    let name = args.first().cloned().unwrap_or_default();
    let Some(line) = envelope(&name, &payload) else {
        return;
    };

    #[cfg(unix)]
    {
        let Some(path) = socket_path() else { return };
        let Ok(mut stream) = std::os::unix::net::UnixStream::connect(path) else {
            // No cctop running, or a stale socket file. Both are ordinary.
            return;
        };
        let _ = stream.set_write_timeout(Some(DEADLINE));
        let _ = stream.write_all(&line);
        let _ = stream.flush();
    }
}

/// Wrap the agent's payload in one newline-delimited line, tagged with the event
/// name. Newlines inside the JSON are what makes the framing need doing at all.
fn envelope(name: &str, payload: &[u8]) -> Option<Vec<u8>> {
    let body: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let event = serde_json::json!({
        "event": match name.is_empty() {
            true => body.get("hook_event_name").and_then(|v| v.as_str()).unwrap_or_default(),
            false => name,
        },
        "session_id": body.get("session_id").and_then(|v| v.as_str()).unwrap_or_default(),
        "cwd": body.get("cwd").and_then(|v| v.as_str()).unwrap_or_default(),
    });
    let mut line = serde_json::to_vec(&event).ok()?;
    line.push(b'\n');
    Some(line)
}

/// What a hook event says about a session, as far as the UI is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// The agent asked something and is blocked on the answer.
    NeedsInput,
    /// The agent finished its turn; the prompt is the user's.
    Idle,
    /// The agent started working, which cancels either of the above.
    Busy,
}

/// One event, resolved to the session it concerns.
#[derive(Debug, Clone)]
pub struct Event {
    pub session_id: String,
    pub signal: Signal,
}

/// Map an event name onto what it means. Unknown names are dropped rather than
/// guessed at, so a newer Claude Code can add events without confusing this one.
fn signal_of(event: &str) -> Option<Signal> {
    match event {
        // The turn is over. This is the event the whole feature exists for:
        // nothing in a transcript distinguishes it from the agent still working.
        "Stop" => Some(Signal::Idle),
        // Claude Code raises this when it wants the user — a permission prompt,
        // or an idle nudge.
        "Notification" => Some(Signal::NeedsInput),
        // Both mean work has started, which answers whatever came before.
        "UserPromptSubmit" | "PreToolUse" => Some(Signal::Busy),
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
    /// Removed on the way out. A leftover file is survivable — the next cctop
    /// finds it refuses connections and replaces it — but only the instance
    /// that bound it can know it is finished with it.
    path: PathBuf,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Listener {
    /// Start listening, unless another cctop already is.
    ///
    /// The address is shared, so the first instance to claim it serves everyone
    /// and later ones simply go without. Liveness is tested by connecting rather
    /// than by the file existing: a socket whose owner died refuses connections
    /// but stays on disk, and only then is it ours to replace.
    #[cfg(unix)]
    pub fn start() -> Option<Listener> {
        use std::os::unix::net::{UnixListener, UnixStream};

        let path = socket_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok()?;
            // Hook events name the user's projects; keep the directory private
            // even when it lands in a shared cache root.
            let _ = std::fs::set_permissions(
                parent,
                std::os::unix::fs::PermissionsExt::from_mode(0o700),
            );
        }
        if UnixStream::connect(&path).is_ok() {
            // A live cctop already holds it.
            return None;
        }
        let _ = std::fs::remove_file(&path);
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
        signal: signal_of(value.get("event")?.as_str()?)?,
    })
}

/// Events cctop asks Claude Code to tell it about.
///
/// Deliberately the smallest set that answers "does this session want me":
/// every hook fire costs the agent a process spawn, so an event cctop would
/// only use to redraw something it can already see is not worth the fork.
const WANTED: [&str; 4] = ["Stop", "Notification", "UserPromptSubmit", "PreToolUse"];

/// How cctop's entries are recognised in a settings file it does not own.
///
/// Matching on the command text rather than a marker key: the file belongs to
/// the user and their other tools, the schema has no room for a comment, and
/// this is the one string that is unmistakably ours.
const MARKER: &str = "cctop hook";

/// Claude Code's settings file, honouring the same override the rest of cctop
/// reads.
fn settings_file() -> PathBuf {
    crate::config::CLAUDE_CONFIG_DIR.join("settings.json")
}

/// Add cctop's hooks to Claude Code's settings, leaving everything else alone.
///
/// The file is the user's, and by the time cctop sees it their other tools have
/// usually put hooks in it — so this merges into the arrays rather than writing
/// them, and never reorders or reformats what it did not add.
pub fn install() -> anyhow::Result<String> {
    let path = settings_file();
    let mut root = read_settings(&path)?;
    let Some(exe) = std::env::current_exe().ok().and_then(|p| {
        // A bare `cctop` would depend on the agent's PATH, which is not this
        // shell's. The absolute path is what makes the hook fire at all.
        p.to_str().map(str::to_string)
    }) else {
        anyhow::bail!("could not find cctop's own path");
    };

    let hooks = root
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("`hooks` in {} is not an object", path.display()))?;

    let mut added = 0;
    for event in WANTED {
        let list = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| anyhow::anyhow!("`hooks.{event}` is not an array"))?;
        // Idempotent: running the installer twice must not fire twice.
        list.retain(|entry| !is_ours(entry));
        list.push(serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": format!("{exe} hook {event}"),
            }]
        }));
        added += 1;
    }
    write_settings(&path, &root)?;
    Ok(format!("Added {added} hooks to {}", path.display()))
}

/// Take cctop's hooks back out, leaving every other entry untouched.
pub fn remove() -> anyhow::Result<String> {
    let path = settings_file();
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
    entry
        .get("hooks")
        .and_then(|h| h.as_array())
        .is_some_and(|inner| {
            inner.iter().any(|h| {
                h.get("command")
                    .and_then(|c| c.as_str())
                    .is_some_and(|c| c.contains(MARKER))
            })
        })
}

fn read_settings(
    path: &std::path::Path,
) -> anyhow::Result<serde_json::Map<String, serde_json::Value>> {
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
    path: &std::path::Path,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The envelope must survive the payload, whatever is in it: the fields are
    /// pulled out of somebody else's JSON and a missing one is normal.
    #[test]
    fn an_event_is_reduced_to_the_session_and_what_happened() {
        let raw = br#"{"session_id":"abc","cwd":"/x","hook_event_name":"Stop","extra":{"a":1}}"#;
        let line = envelope("", raw).expect("envelope");
        assert!(line.ends_with(b"\n"), "the wire is newline delimited");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.session_id, "abc");
        assert_eq!(event.signal, Signal::Idle);

        // The argument wins, for a harness whose payload names events its way.
        let line = envelope("Notification", raw).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.signal, Signal::NeedsInput);

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

    /// The settings file belongs to the user and their other tools. Installing
    /// must not disturb a hook cctop did not write, and removing must put the
    /// file back exactly as it was found.
    #[test]
    fn installing_leaves_another_tools_hooks_exactly_as_they_were() {
        let dir = std::env::temp_dir().join(format!("cctop-hooks-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        let theirs = serde_json::json!({
            "model": "opus",
            "hooks": {
                "SessionStart": [{"hooks": [{"type": "command", "command": "/theirs.mjs"}]}]
            }
        });
        std::fs::write(&path, serde_json::to_string_pretty(&theirs).unwrap()).unwrap();

        let mut root = read_settings(&path).unwrap();
        let hooks = root["hooks"].as_object_mut().unwrap();
        for event in WANTED {
            let list = hooks
                .entry(event)
                .or_insert_with(|| serde_json::json!([]))
                .as_array_mut()
                .unwrap();
            list.retain(|e| !is_ours(e));
            list.push(serde_json::json!({
                "hooks": [{"type": "command", "command": "/bin/cctop hook Stop"}]
            }));
        }
        write_settings(&path, &root).unwrap();

        let after = read_settings(&path).unwrap();
        assert_eq!(after["model"], "opus", "an unrelated setting was disturbed");
        assert_eq!(
            after["hooks"]["SessionStart"], theirs["hooks"]["SessionStart"],
            "another tool's hook was disturbed"
        );
        assert!(after["hooks"]["Stop"].as_array().unwrap().len() == 1);

        // And removal puts it back, including dropping the arrays we created.
        let mut root = read_settings(&path).unwrap();
        let hooks = root["hooks"].as_object_mut().unwrap();
        for (_, v) in hooks.iter_mut() {
            if let Some(l) = v.as_array_mut() {
                l.retain(|e| !is_ours(e));
            }
        }
        hooks.retain(|_, v| !v.as_array().is_some_and(|l| l.is_empty()));
        write_settings(&path, &root).unwrap();
        assert_eq!(read_settings(&path).unwrap(), *theirs.as_object().unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A settings file cctop cannot parse must be refused, not rewritten — the
    /// alternative is destroying whatever the user actually had in it.
    #[test]
    fn an_unparsable_settings_file_is_refused_rather_than_replaced() {
        let dir = std::env::temp_dir().join(format!("cctop-badjson-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::write(&path, "{ this is not json").unwrap();
        assert!(read_settings(&path).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ this is not json"
        );
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

        let dir = std::env::temp_dir().join(format!("cctop-wedge-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("hooks.sock");
        let _ = std::fs::remove_file(&path);
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
            // The same shape `emit` uses, against the wedged address.
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
        forward(&["Stop".into()]);
        assert!(
            started.elapsed() < DEADLINE * 2,
            "the agent was held up for {:?}",
            started.elapsed()
        );
    }
}
