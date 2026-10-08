//! The `!` column, said to the agent rather than to you.
//!
//! [`crate::collide`] tells the *human* that two agents hold one file. By the
//! time a person has looked at the table, the second agent has usually made
//! its edit — and it is the one party that could have done something cheap
//! about it: re-read the file, pick another, or say so. This module tells that
//! agent directly, at the moment it reaches for the file, through the one
//! channel its harness offers for context that is not a decision.
//!
//! # Where the facts come from
//!
//! Not from cctop. The hook's sockets carry events one way, and cctop being
//! closed is the ordinary case (see [`crate::hook`]); a warning that needed a
//! running dashboard would be missing exactly when nobody is watching. Nor
//! from transcripts: parsing every live session's history on each tool call is
//! the cost the hook's deadline exists to rule out, and a transcript's writes
//! carry no liveness of their own.
//!
//! So the hooks keep the record themselves. Every write a hook sees lands in a
//! small ledger beside the hook sockets — path, session, when, and the agent
//! process that made it — and a later fire in another session reads it back.
//! One small file read per write-tool call, and nothing at all otherwise.
//!
//! # What each harness can be told
//!
//! | harness | checked on | how it is told |
//! |---|---|---|
//! | Claude Code | `PreToolUse` of `Write`, `Edit`, `MultiEdit`, `NotebookEdit` | `hookSpecificOutput.additionalContext`, which reaches the model alongside the tool result |
//! | Gemini CLI | `AfterTool` of `write_file`, `replace` | `hookSpecificOutput.additionalContext`, appended to the tool result — `BeforeTool` has no context channel |
//! | Codex | never | its writes are recorded from `PostToolUse` of `apply_patch`, so the others hear about them |
//!
//! No `permissionDecision`, no `decision`, no `systemMessage`: the answer
//! carries context and nothing else, so it cannot block, prompt or deny.
//!
//! ponytail: Codex is told nothing. Its hook contract for `PreToolUse` output
//! is not in the mirrored docs, and guessing at the shape of a JSON answer is
//! exactly how a monitor turns into an error notice on every edit. Cursor is
//! the same — cctop does not install its `postToolUse`, and a Cursor running
//! Claude Code's hooks by compatibility (its payload carries
//! `cursor_version`) is recorded but never answered, for the same reason.
//!
//! # Opt-in
//!
//! Off unless `[settings] warn_agents = true`. With it off, the hook reads the
//! settings file on a write-tool call and does nothing else — no ledger, and
//! stdout as silent as it has always been.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a write counts as "recently".
///
/// The same judgement [`crate::session::MAX_RECENT_WRITES`] makes by count: a
/// file another agent wrote an hour ago is one it has almost certainly
/// finished with, and a warning about it is one that teaches the agent to
/// ignore the next.
const WINDOW_MS: i64 = 30 * 60 * 1000;

/// How many writes the ledger holds, newest kept.
///
/// Far above what the live sessions on one machine write inside
/// [`WINDOW_MS`], and small enough that reading it whole on every write-tool
/// call stays in the tens of microseconds.
const MAX_ENTRIES: usize = 256;

/// How long a hook waits for another hook to finish rewriting the ledger.
///
/// A rewrite is a read, a sort and a rename — microseconds — so a lock held
/// longer than this belongs to a hook that was stopped or killed mid-write.
/// Giving up drops one record; waiting would spend the agent's deadline and
/// the event delivery queued behind it.
const LOCK_PATIENCE: Duration = Duration::from_millis(25);

/// How many peers the advice names. One is the usual case; more than a few is
/// a checkout that needs a person, not a longer message.
const MAX_NAMED: usize = 3;

/// Claude Code's file-writing tools, as its hooks name them.
const CLAUDE_WRITES: [&str; 4] = ["Write", "Edit", "MultiEdit", "NotebookEdit"];

/// Gemini CLI's, as its hooks name them.
const GEMINI_WRITES: [&str; 2] = ["write_file", "replace"];

/// Codex's one writing tool, under both spellings a transcript has used.
const CODEX_WRITES: [&str; 2] = ["apply_patch", "ApplyPatch"];

/// The agent process a session runs in, pinned by its start time.
///
/// A pid alone would outlive the agent: once it exits, the number is free to
/// be handed to anything, and a ledger entry would go on vouching for a
/// session that ended. The start time (in clock ticks since boot, from
/// `/proc/<pid>/stat`) is what makes "this pid is still that process" a
/// question with an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pid: u32,
    start: u64,
}

/// One write, as the ledger keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    /// Absolute and lexically normalised, by [`crate::collide::normalise`], so
    /// two harnesses' spellings of one file meet.
    path: String,
    session: String,
    harness: String,
    cwd: String,
    /// Unix milliseconds.
    at: i64,
    agent: Agent,
}

/// Which JSON a harness wants its context in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reply {
    /// `hookSpecificOutput` naming the event, with `additionalContext`. Claude
    /// Code and Gemini CLI spell it identically; only the event differs.
    Context(&'static str),
}

/// What one hook fire asks of the ledger.
#[derive(Debug, PartialEq)]
struct Plan {
    /// Look for peers, and answer in this shape if there are any.
    check: Option<Reply>,
    /// Add these files to the ledger once the check is done.
    record: bool,
    files: Vec<String>,
    harness: &'static str,
}

/// Decide from the payload alone whether this fire has anything to do here.
///
/// Cheap and pure on purpose: it runs first, so every event that is not a
/// file write — which is nearly all of them — leaves before the settings file
/// is even opened.
fn plan(event: &str, body: &Value) -> Option<Plan> {
    let tool = body.get("tool_name").and_then(Value::as_str)?;
    // Cursor runs Claude Code's hooks by compatibility and says so in the
    // payload. Its own reading of a Claude-shaped answer is undocumented, so
    // it is recorded and never answered.
    let cursor = body.get("cursor_version").is_some();
    let (check, record, harness) = match event {
        "PreToolUse" if CLAUDE_WRITES.contains(&tool) && !cursor => {
            (Some(Reply::Context("PreToolUse")), false, "Claude Code")
        }
        "PostToolUse" if CLAUDE_WRITES.contains(&tool) => (
            None,
            true,
            match cursor {
                true => "Cursor",
                false => "Claude Code",
            },
        ),
        "PostToolUse" if CODEX_WRITES.contains(&tool) => (None, true, "Codex"),
        // Gemini's `BeforeTool` can deny or rewrite but cannot add context, so
        // the warning waits for the result. It is checked before its own write
        // is recorded, so it never finds itself.
        "AfterTool" if GEMINI_WRITES.contains(&tool) => {
            (Some(Reply::Context("AfterTool")), true, "Gemini CLI")
        }
        _ => return None,
    };

    let cwd = body.get("cwd").and_then(Value::as_str).unwrap_or_default();
    let input = body.get("tool_input");
    let mut named: Vec<String> = ["file_path", "notebook_path", "path"]
        .iter()
        .filter_map(|k| input?.get(*k)?.as_str())
        .map(str::to_string)
        .collect();
    if CODEX_WRITES.contains(&tool) {
        // A patch arrives as a raw string, bare or under one of the keys the
        // Codex tool schemas have used for it.
        let patch = input.and_then(|i| {
            i.as_str().or_else(|| {
                ["command", "input", "patch"]
                    .iter()
                    .find_map(|k| i.get(*k)?.as_str())
            })
        });
        named.extend(
            patch
                .map(crate::session::extract::patch_files)
                .unwrap_or_default(),
        );
    }

    let mut files: Vec<String> = named
        .iter()
        .filter(|p| !p.trim().is_empty())
        .map(|p| crate::collide::normalise(p, cwd))
        // The `!` column's exemption, for the `!` column's reason: a warning
        // that fires on every `MEMORY.md` append is one that gets read past.
        .filter(|p| crate::collide::contested(p))
        .collect();
    files.sort();
    files.dedup();
    (!files.is_empty()).then_some(Plan {
        check,
        record,
        files,
        harness,
    })
}

/// Who is asking: the session, and the process it runs in when that could be
/// found.
struct Me<'a> {
    session: &'a str,
    cwd: &'a str,
    agent: Option<Agent>,
}

/// `cctop hook`'s entry point: what, if anything, to write to stdout for this
/// fire.
///
/// `None` is the answer for every event but a file write, for every write
/// with the setting off, and for every failure along the way — a ledger that
/// will not parse, a directory that cannot be made, a lock that stays held.
/// The caller runs this on its abandonable thread, so a stall here costs at
/// most the deadline and an answer that never arrives.
pub fn consider(name: &str, payload: &[u8], chain: &[u32]) -> Option<String> {
    let body: Value = serde_json::from_slice(payload).ok()?;
    let event = match name.is_empty() {
        true => body.get("hook_event_name").and_then(Value::as_str)?,
        false => name,
    };
    let plan = plan(event, &body)?;
    if crate::settings::Settings::load().warn_agents != Some(true) {
        return None;
    }
    let me = Me {
        session: body.get("session_id").and_then(Value::as_str)?,
        cwd: body.get("cwd").and_then(Value::as_str).unwrap_or_default(),
        agent: agent_of(chain),
    };
    let ledger = Ledger::at(crate::hook::socket_dir()?);
    let advice = run(&ledger, &plan, &me, crate::util::now_ms(), alive);
    if advice.is_some() {
        crate::elog::event(
            "hook",
            "advise",
            serde_json::json!({ "event": event, "files": plan.files.len() }),
        );
    }
    advice
}

/// Check, then record — the half of [`consider`] that does not touch the real
/// machine's settings, clock or process table, so it can be tested.
fn run(
    ledger: &Ledger,
    plan: &Plan,
    me: &Me,
    now: i64,
    alive: impl Fn(&Agent) -> bool,
) -> Option<String> {
    if me.session.is_empty() {
        return None;
    }
    let advice = plan.check.and_then(|reply| {
        let held = holders(&ledger.load(), &plan.files, me, now, &alive);
        (!held.is_empty()).then(|| render(reply, &held, now))
    });
    // A write with no agent process to vouch for it would never pass the
    // liveness check, so recording it would only take a slot.
    if plan.record
        && let Some(agent) = me.agent
    {
        ledger.record(&plan.files, me.session, plan.harness, me.cwd, agent, now);
    }
    advice
}

/// The other live sessions' writes to any of `files` inside the window,
/// newest first, one per session and file.
fn holders(
    entries: &[Entry],
    files: &[String],
    me: &Me,
    now: i64,
    alive: &impl Fn(&Agent) -> bool,
) -> Vec<Entry> {
    let mut found: Vec<Entry> = entries
        .iter()
        .filter(|e| files.contains(&e.path))
        .filter(|e| e.session != me.session)
        // One process under a new session id — after a `/clear`, say — is the
        // same agent, and warning it about itself would be the whole warning
        // crying wolf.
        .filter(|e| Some(e.agent) != me.agent)
        .filter(|e| (0..=WINDOW_MS).contains(&(now - e.at)))
        .filter(|e| alive(&e.agent))
        .cloned()
        .collect();
    found.sort_by_key(|e| std::cmp::Reverse(e.at));
    let mut seen = std::collections::HashSet::new();
    found.retain(|e| seen.insert((e.session.clone(), e.path.clone())));
    found.truncate(MAX_NAMED);
    found
}

/// The answer, as the one JSON object the harness reads from stdout.
///
/// Worded for the model reading it: what happened, why it matters, and what
/// to do — because it arrives with no other explanation and the agent has
/// never heard of cctop.
fn render(reply: Reply, held: &[Entry], now: i64) -> String {
    let mut text = String::from(
        "cctop: another agent that is still running on this machine wrote this file recently.\n",
    );
    for e in held {
        let short: String = e.session.chars().take(8).collect();
        text.push_str(&format!(
            "- {} — {} ago, by {} session {short} working in {}\n",
            e.path,
            crate::util::compact_duration(now - e.at),
            e.harness,
            e.cwd,
        ));
    }
    text.push_str(
        "Two agents editing one file do not merge: whichever writes last silently replaces \
         the other's work. Re-read the file before changing it again, and settle who finishes \
         first — or move one of you into a separate git worktree.",
    );
    let Reply::Context(event) = reply;
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": event,
            "additionalContext": text,
        }
    })
    .to_string()
}

/// The ledger file, beside the hook sockets.
///
/// In the runtime directory for the same reason the process-tree map is (see
/// `hook::claims_path`): a pid never outlives a boot, and the directory is
/// cleared when the machine restarts, so no entry can outlast the process it
/// names by more than a boot.
struct Ledger {
    dir: PathBuf,
    /// [`LOCK_PATIENCE`] for a hook. The tests wait longer, because the test
    /// binary forks children in parallel and a child holds an inherited
    /// `flock` until it execs — a window the hook, which forks nothing, never
    /// sees.
    patience: Duration,
}

impl Ledger {
    fn at(dir: PathBuf) -> Ledger {
        Ledger {
            dir,
            patience: LOCK_PATIENCE,
        }
    }

    fn file(&self) -> PathBuf {
        self.dir.join("writes.json")
    }

    /// Everything recorded, or nothing: a missing or mangled ledger is an
    /// empty one, never an error.
    ///
    /// Read without the lock. Writers replace the file by rename, so a reader
    /// sees the old ledger or the new one, never half of either.
    fn load(&self) -> Vec<Entry> {
        std::fs::read(self.file())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Stamp `files` as written by this session now, and prune what has aged
    /// out.
    ///
    /// Under an exclusive lock, because two agents writing one file at once is
    /// the very case this exists for — and a read-modify-write without one
    /// would drop whichever of their two records lost the rename.
    fn record(
        &self,
        files: &[String],
        session: &str,
        harness: &str,
        cwd: &str,
        agent: Agent,
        now: i64,
    ) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        // The ledger names the user's projects; the directory stays private
        // even when it has to fall back to a shared cache root.
        let _ = std::fs::set_permissions(
            &self.dir,
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        );
        let Some(_lock) = Lock::take(&self.dir.join("writes.lock"), self.patience) else {
            return;
        };
        let mut entries = self.load();
        entries.retain(|e| {
            (0..=WINDOW_MS).contains(&(now - e.at))
                && !(e.session == session && files.contains(&e.path))
        });
        entries.extend(files.iter().map(|path| Entry {
            path: path.clone(),
            session: session.to_string(),
            harness: harness.to_string(),
            cwd: cwd.to_string(),
            at: now,
            agent,
        }));
        entries.sort_by_key(|e| std::cmp::Reverse(e.at));
        entries.truncate(MAX_ENTRIES);
        let Ok(json) = serde_json::to_vec(&entries) else {
            return;
        };
        // One temporary name is enough: only the lock holder writes it.
        let tmp = self.dir.join("writes.json.tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, self.file());
        }
    }
}

/// An exclusive `flock` on a file, released when this is dropped — or when
/// the process exits, which is what makes it safe for a hook that may be
/// abandoned mid-write.
struct Lock {
    _held: std::fs::File,
}

impl Lock {
    /// Try for the lock until `patience` runs out, never blocking on it.
    fn take(path: &Path, patience: Duration) -> Option<Lock> {
        use std::os::fd::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)
            .ok()?;
        let started = Instant::now();
        loop {
            // SAFETY: flock on a descriptor this function owns for the call.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Some(Lock { _held: file });
            }
            if started.elapsed() >= patience {
                return None;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// Shells and wrappers a harness may put between itself and the hook.
///
/// Claude Code hands the command to `sh -c`, and people wrap agents in `env`
/// or `timeout`. None of them is the agent, and all of them are gone the
/// moment the hook returns, so pinning a write to one would make it look dead
/// by the next fire.
const WRAPPERS: [&str; 10] = [
    "sh", "bash", "dash", "zsh", "fish", "ksh", "mksh", "env", "timeout", "nohup",
];

/// The nearest ancestor that is not a shell or wrapper: the agent itself.
///
/// ponytail: a harness that runs its hooks under a sandbox helper of its own
/// would be pinned to that helper, whose exit makes its writes look finished.
/// That drops a warning rather than inventing one, which is the right way for
/// this to be wrong.
fn agent_of(chain: &[u32]) -> Option<Agent> {
    chain.iter().find_map(|&pid| {
        let (comm, start) = stat(pid)?;
        (!WRAPPERS.contains(&comm.as_str())).then_some(Agent { pid, start })
    })
}

/// Whether the process that made a write is still the same process.
fn alive(agent: &Agent) -> bool {
    stat(agent.pid).is_some_and(|(_, start)| start == agent.start)
}

/// A process's name and start time, from one read of `/proc/<pid>/stat`.
fn stat(pid: u32) -> Option<(String, u64)> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_stat(&text)
}

/// `comm` is parenthesised and may itself hold spaces and parens, so it runs
/// to the *last* `)`; `starttime` is the 22nd field, the 20th after it.
fn parse_stat(text: &str) -> Option<(String, u64)> {
    let open = text.find('(')?;
    let (head, rest) = text.rsplit_once(')')?;
    let comm = head.get(open + 1..)?.to_string();
    let start = rest.split_whitespace().nth(19)?.parse().ok()?;
    Some((comm, start))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_800_000_000_000;

    fn scratch(name: &str) -> Ledger {
        let dir = std::env::temp_dir().join(format!(
            "cctop-advise-{}-{name}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Ledger {
            dir,
            patience: Duration::from_secs(2),
        }
    }

    fn agent(pid: u32) -> Option<Agent> {
        Some(Agent { pid, start: 7 })
    }

    fn me<'a>(session: &'a str, pid: u32) -> Me<'a> {
        Me {
            session,
            cwd: "/repo",
            agent: agent(pid),
        }
    }

    fn claude(event: &str, tool: &str, file: &str) -> Value {
        json!({
            "session_id": "ignored-here",
            "cwd": "/repo",
            "hook_event_name": event,
            "tool_name": tool,
            "tool_input": { "file_path": file, "old_string": "a", "new_string": "b" },
        })
    }

    fn everyone_alive(_: &Agent) -> bool {
        true
    }

    /// Session `a` writes a file, then session `b` reaches for it.
    fn a_then_b(ledger: &Ledger, gap_ms: i64, alive: impl Fn(&Agent) -> bool) -> Option<String> {
        let wrote = plan("PostToolUse", &claude("PostToolUse", "Edit", "src/ui.rs")).unwrap();
        assert_eq!(
            run(ledger, &wrote, &me("aaaaaaaa-1111", 100), NOW, &alive),
            None
        );
        let about = plan(
            "PreToolUse",
            &claude("PreToolUse", "Edit", "/repo/src/ui.rs"),
        )
        .unwrap();
        run(
            ledger,
            &about,
            &me("bbbbbbbb-2222", 200),
            NOW + gap_ms,
            &alive,
        )
    }

    /// The whole feature: the second agent is told who holds its file, how
    /// long ago they wrote it, and nothing that could read as a decision.
    #[test]
    fn the_second_agent_is_told_who_wrote_its_file() {
        let ledger = scratch("told");
        let answer = a_then_b(&ledger, 3 * 60 * 1000, everyone_alive).expect("advice");
        let value: Value = serde_json::from_str(&answer).expect("one JSON object");

        let out = value["hookSpecificOutput"]
            .as_object()
            .expect("hook output");
        assert_eq!(out["hookEventName"], "PreToolUse");
        let text = out["additionalContext"].as_str().unwrap();
        assert!(text.contains("/repo/src/ui.rs"), "{text}");
        assert!(text.contains("aaaaaaaa"), "names the peer: {text}");
        assert!(text.contains("3m00s ago"), "says how long ago: {text}");
        assert!(text.contains("Claude Code"), "{text}");
        // Context and only context: nothing here can block, prompt or deny.
        assert_eq!(value.as_object().unwrap().len(), 1);
        assert_eq!(out.len(), 2, "a decision snuck into the advice: {out:?}");
        let _ = std::fs::remove_dir_all(&ledger.dir);
    }

    /// Its own writes are not a peer's, and neither is the same agent process
    /// under a fresh session id.
    #[test]
    fn nobody_is_warned_about_themselves() {
        let ledger = scratch("self");
        let wrote = plan("PostToolUse", &claude("PostToolUse", "Write", "src/x.rs")).unwrap();
        let about = plan("PreToolUse", &claude("PreToolUse", "Edit", "src/x.rs")).unwrap();
        run(&ledger, &wrote, &me("one", 100), NOW, everyone_alive);

        assert_eq!(
            run(&ledger, &about, &me("one", 100), NOW, everyone_alive),
            None
        );
        assert_eq!(
            run(
                &ledger,
                &about,
                &me("after-clear", 100),
                NOW,
                everyone_alive
            ),
            None,
            "the same process is the same agent"
        );
        let _ = std::fs::remove_dir_all(&ledger.dir);
    }

    /// A peer that has exited, or wrote long enough ago to have finished,
    /// races nobody.
    #[test]
    fn a_finished_or_stale_peer_is_not_mentioned() {
        let dead = scratch("dead");
        assert_eq!(a_then_b(&dead, 1000, |_| false), None);
        let stale = scratch("stale");
        assert_eq!(a_then_b(&stale, WINDOW_MS + 1, everyone_alive), None);
        for l in [dead, stale] {
            let _ = std::fs::remove_dir_all(&l.dir);
        }
    }

    /// A pid is a number the kernel hands out again. Only the same pid with the
    /// same start time is the same process.
    #[test]
    fn a_reused_pid_is_not_the_agent_that_wrote() {
        let me = std::process::id();
        let (_, start) = stat(me).expect("own stat");
        assert!(alive(&Agent { pid: me, start }));
        assert!(!alive(&Agent {
            pid: me,
            start: start + 1
        }));
        assert!(!alive(&Agent {
            pid: u32::MAX,
            start
        }));
    }

    #[test]
    fn a_process_name_with_parens_still_parses() {
        let line = "42 (we(i)rd name) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 99 20";
        assert_eq!(parse_stat(line), Some(("we(i)rd name".into(), 99)));
        assert_eq!(parse_stat("garbage"), None);
    }

    /// Only a file write has anything to do here; every other event leaves
    /// before the settings file is opened.
    #[test]
    fn only_a_file_write_is_considered() {
        for (event, tool) in [
            ("PreToolUse", "Bash"),
            ("PreToolUse", "Read"),
            ("PostToolUse", "Grep"),
            ("Stop", "Edit"),
            ("BeforeTool", "write_file"),
        ] {
            assert_eq!(
                plan(event, &claude(event, tool, "src/x.rs")),
                None,
                "{event} {tool}"
            );
        }
        assert_eq!(plan("PreToolUse", &json!({ "session_id": "s" })), None);
    }

    /// The `!` column's exemption holds here too, for the same reason.
    #[test]
    fn prose_and_lock_files_are_not_worth_a_warning() {
        for file in ["MEMORY.md", "Cargo.lock", "package-lock.json"] {
            assert_eq!(
                plan("PreToolUse", &claude("PreToolUse", "Edit", file)),
                None
            );
        }
    }

    /// Codex and Cursor are recorded so the others hear of them, and are
    /// never answered: neither documents what it would do with the JSON.
    #[test]
    fn only_harnesses_with_a_documented_context_channel_are_answered() {
        let patch =
            "*** Begin Patch\n*** Update File: src/a.rs\n*** Add File: src/b.rs\n*** End Patch";
        let codex = json!({
            "session_id": "c", "cwd": "/repo", "tool_name": "apply_patch",
            "tool_input": { "command": patch },
        });
        assert_eq!(plan("PreToolUse", &codex), None);
        let wrote = plan("PostToolUse", &codex).expect("recorded");
        assert_eq!(wrote.check, None);
        assert_eq!(wrote.files, ["/repo/src/a.rs", "/repo/src/b.rs"]);

        let mut cursor = claude("PreToolUse", "Write", "src/a.rs");
        cursor["cursor_version"] = json!("1.7.2");
        assert_eq!(plan("PreToolUse", &cursor), None);
        assert!(plan("PostToolUse", &cursor).is_some_and(|p| p.record));
    }

    /// Gemini's `BeforeTool` has no context channel, so it is told on
    /// `AfterTool` — checked before its own write is recorded.
    #[test]
    fn gemini_is_told_after_the_tool_and_records_its_own_write() {
        let ledger = scratch("gemini");
        let wrote = plan("PostToolUse", &claude("PostToolUse", "Edit", "src/ui.rs")).unwrap();
        run(&ledger, &wrote, &me("claude-a", 100), NOW, everyone_alive);

        let gemini = json!({
            "session_id": "g", "cwd": "/repo", "tool_name": "replace",
            "tool_input": { "file_path": "/repo/src/ui.rs" },
        });
        let after = plan("AfterTool", &gemini).unwrap();
        let answer =
            run(&ledger, &after, &me("gemini-b", 200), NOW, everyone_alive).expect("advice");
        let value: Value = serde_json::from_str(&answer).unwrap();
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "AfterTool");

        // And now Claude hears about Gemini in turn.
        let about = plan("PreToolUse", &claude("PreToolUse", "Edit", "src/ui.rs")).unwrap();
        let back = run(&ledger, &about, &me("claude-a", 100), NOW, everyone_alive)
            .expect("the record went both ways");
        assert!(back.contains("Gemini CLI"), "{back}");
        let _ = std::fs::remove_dir_all(&ledger.dir);
    }

    /// Every way the ledger can be broken is an empty ledger, and the hook
    /// says nothing.
    #[test]
    fn a_broken_ledger_is_silence() {
        let about = plan("PreToolUse", &claude("PreToolUse", "Edit", "src/x.rs")).unwrap();

        let mangled = scratch("mangled");
        std::fs::create_dir_all(&mangled.dir).unwrap();
        std::fs::write(mangled.file(), b"{ not json").unwrap();
        assert_eq!(
            run(&mangled, &about, &me("b", 2), NOW, everyone_alive),
            None
        );

        // A directory that cannot be made, because a file stands in its place.
        let blocked = scratch("blocked");
        std::fs::write(&blocked.dir, b"").unwrap();
        let wrote = plan("PostToolUse", &claude("PostToolUse", "Edit", "src/x.rs")).unwrap();
        assert_eq!(
            run(&blocked, &wrote, &me("a", 1), NOW, everyone_alive),
            None
        );
        assert_eq!(
            run(&blocked, &about, &me("b", 2), NOW, everyone_alive),
            None
        );

        let _ = std::fs::remove_dir_all(&mangled.dir);
        let _ = std::fs::remove_file(&blocked.dir);
    }

    /// A lock held by a hook that was stopped mid-write costs one record and a
    /// bounded wait — never the agent's deadline.
    #[test]
    fn a_held_lock_gives_up_well_inside_the_deadline() {
        // The hook's own patience, since the wait is what is under test.
        let ledger = Ledger::at(scratch("held").dir);
        std::fs::create_dir_all(&ledger.dir).unwrap();
        let held = Lock::take(&ledger.dir.join("writes.lock"), LOCK_PATIENCE).expect("first");

        let wrote = plan("PostToolUse", &claude("PostToolUse", "Edit", "src/x.rs")).unwrap();
        let started = Instant::now();
        assert_eq!(run(&ledger, &wrote, &me("a", 1), NOW, everyone_alive), None);
        let waited = started.elapsed();
        // The property is the deadline; the patience is far inside it, and a
        // loaded test machine can oversleep a millisecond by a lot.
        assert!(waited < crate::hook::DEADLINE, "waited {waited:?}");
        assert!(ledger.load().is_empty(), "wrote without the lock");

        drop(held);
        run(&ledger, &wrote, &me("a", 1), NOW, everyone_alive);
        assert_eq!(ledger.load().len(), 1, "the lock came back");
        let _ = std::fs::remove_dir_all(&ledger.dir);
    }

    /// Rewriting prunes: one entry per session and file, nothing past the
    /// window, never more than the cap.
    #[test]
    fn the_ledger_stays_small() {
        let ledger = scratch("small");
        let wrote = plan("PostToolUse", &claude("PostToolUse", "Edit", "src/x.rs")).unwrap();
        run(
            &ledger,
            &wrote,
            &me("a", 1),
            NOW - WINDOW_MS - 1,
            everyone_alive,
        );
        run(&ledger, &wrote, &me("b", 2), NOW - 10, everyone_alive);
        run(&ledger, &wrote, &me("b", 2), NOW, everyone_alive);
        let kept = ledger.load();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!((kept[0].session.as_str(), kept[0].at), ("b", NOW));

        for i in 0..(MAX_ENTRIES + 10) {
            let file = format!("src/f{i}.rs");
            let p = plan("PostToolUse", &claude("PostToolUse", "Edit", &file)).unwrap();
            run(&ledger, &p, &me("c", 3), NOW, everyone_alive);
        }
        assert_eq!(ledger.load().len(), MAX_ENTRIES);
        let _ = std::fs::remove_dir_all(&ledger.dir);
    }

    /// End to end with the setting at its default: a file write is answered
    /// with silence, and nothing is recorded.
    ///
    /// Asserted only when this machine's config does not turn it on — the
    /// test must not rewrite the user's own settings file to prove a point.
    #[test]
    fn off_by_default_means_silent_and_unrecorded() {
        assert_eq!(
            crate::settings::Settings::parse("").warn_agents,
            None,
            "the default must be off"
        );
        if crate::settings::Settings::load().warn_agents == Some(true) {
            return;
        }
        let payload = serde_json::to_vec(&claude("PreToolUse", "Edit", "src/x.rs")).unwrap();
        assert_eq!(consider("PreToolUse", &payload, &[]), None);
        assert_eq!(consider("", b"not json", &[]), None);
    }

    /// The walk from the hook finds a real, living process to pin writes to —
    /// not the shell it was started from.
    #[test]
    fn the_agent_is_the_first_ancestor_that_is_not_a_shell() {
        let chain: Vec<u32> = std::iter::successors(stat_parent(std::process::id()), |p| {
            stat_parent(*p).filter(|pp| *pp > 1)
        })
        .take(8)
        .collect();
        if let Some(found) = agent_of(&chain) {
            assert!(alive(&found));
            let (comm, _) = stat(found.pid).unwrap();
            assert!(!WRAPPERS.contains(&comm.as_str()), "{comm}");
        }
        assert_eq!(agent_of(&[]), None);
        assert_eq!(agent_of(&[u32::MAX]), None);
    }

    fn stat_parent(pid: u32) -> Option<u32> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        text.rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }
}
