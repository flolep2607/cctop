//! `cctop yolo-hook`, run the way Claude Code runs it.
//!
//! The one hook allowed to answer a permission prompt, so the one whose
//! silence matters most: for every session but the one YOLO was switched on
//! for, in the process it was switched on in, it must print nothing and exit
//! 0, whatever is on its stdin and whatever state the switch's file is in.
//! The unit tests in `crates/core/src/hook.rs` and `crates/core/src/yolo.rs`
//! cover the decision; these cover the process — the exit status, the bytes
//! on stdout, the deadline, and the ancestry the match is made against, which
//! only a real child of a real parent has.
//!
//! The parent here is this test process: it spawns the hook, so it is the
//! hook's agent, and recording its pid and start time is switching YOLO on
//! for it.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The ceiling on one fire; see `hook_exits_zero.rs`.
const PATIENCE: Duration = Duration::from_secs(5);

/// The answer, spelled as `cctop_core::hook::YOLO_ALLOW` spells it.
const ALLOW: &str = r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#;

struct Fired {
    code: Option<i32>,
    stdout: Vec<u8>,
    took: Duration,
}

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cctop-yolo-hook-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["run/cctop", "home", "cache", "config", "data"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

/// The command, pointed at `dir` and nowhere else — see `hook_exits_zero.rs`.
fn command(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_cctop"));
    c.env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("HOME", dir.join("home"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CCTOP_LOG");
    c
}

fn state(dir: &Path) -> PathBuf {
    dir.join("run").join("cctop").join("yolo.json")
}

/// The lasting record, under the sandbox's `XDG_DATA_HOME`.
fn yolo_log(dir: &Path) -> PathBuf {
    dir.join("data").join("cctop").join("yolo-log.jsonl")
}

/// Every line the hook has added to the lasting record.
fn logged(dir: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(yolo_log(dir))
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// When `pid` started, in clock ticks since boot, as the kernel says.
fn start_of(pid: u32) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    stat.rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .nth(19)
        .unwrap()
        .parse()
        .unwrap()
}

/// Write the switch: `session` on, for the process `pid` started at `start`.
fn switch(dir: &Path, session: &str, agent: Option<(u32, u64)>) {
    let entry = match agent {
        Some((pid, start)) => serde_json::json!({
            "since": "2026-01-01T00:00:00Z",
            "allowed": [],
            "agent": {"pid": pid, "start": start},
        }),
        None => serde_json::json!({ "since": "2026-01-01T00:00:00Z", "allowed": [] }),
    };
    let doc = serde_json::json!({ "sessions": { session: entry } });
    std::fs::write(state(dir), doc.to_string()).unwrap();
}

/// This test process, which is the parent of every hook it fires.
fn me() -> (u32, u64) {
    let pid = std::process::id();
    (pid, start_of(pid))
}

fn request(session: &str) -> Vec<u8> {
    request_for(session, "rm -rf build")
}

fn request_for(session: &str, command: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "session_id": session,
        "transcript_path": "/t.jsonl",
        "cwd": "/w",
        "permission_mode": "default",
        "hook_event_name": "PermissionRequest",
        "tool_name": "Bash",
        "tool_input": {"command": command},
        "permission_suggestions": [],
    }))
    .unwrap()
}

fn fire_as(dir: &Path, word: &str, event: &str, stdin: &[u8]) -> Fired {
    fire_with(dir, word, event, stdin, &[])
}

/// [`fire_as`] with these variables set, as Claude Code sets them for a hook.
fn fire_with(dir: &Path, word: &str, event: &str, stdin: &[u8], vars: &[(&str, &str)]) -> Fired {
    let started = Instant::now();
    let mut cmd = command(dir);
    cmd.env_remove("CLAUDE_PID")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CLAUDECODE");
    for (k, v) in vars {
        cmd.env(k, v);
    }
    let mut child = cmd
        .arg(word)
        .arg(event)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the hook binary runs");
    let mut pipe = child.stdin.take().unwrap();
    let stdin = stdin.to_vec();
    // On a thread: an oversized payload fills the pipe, and the hook stops
    // reading at its cap.
    std::thread::spawn(move || {
        let _ = pipe.write_all(&stdin);
    });
    let mut out = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = out.read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    let stdout = rx.recv_timeout(PATIENCE).unwrap_or_else(|_| {
        let _ = child.kill();
        panic!("`cctop {word} {event}` had not returned after {PATIENCE:?}");
    });
    let status = child.wait().unwrap();
    Fired {
        code: status.code(),
        stdout,
        took: started.elapsed(),
    }
}

fn fire(dir: &Path, stdin: &[u8]) -> Fired {
    fire_as(dir, "yolo-hook", "PermissionRequest", stdin)
}

/// A `UserPromptExpansion` payload for `/<name> <args>`, as Claude Code
/// 2.1.295 sends it (captured from a real session, ids replaced).
fn expansion(session: &str, name: &str, args: &str) -> serde_json::Value {
    serde_json::json!({
        "session_id": session,
        "transcript_path": "/t.jsonl",
        "cwd": "/w",
        "prompt_id": "p",
        "permission_mode": "default",
        "hook_event_name": "UserPromptExpansion",
        "expansion_type": "slash_command",
        "command_name": name,
        "command_args": args,
        "command_source": "userSettings",
        "prompt": format!("/{name} {args}"),
    })
}

/// Fire `cctop yolo-hook UserPromptExpansion` for `/yolo <args>` the way
/// Claude Code does, this test process standing as the agent.
fn type_yolo(dir: &Path, session: &str, args: &str) -> Fired {
    let pid = std::process::id().to_string();
    fire_with(
        dir,
        "yolo-hook",
        "UserPromptExpansion",
        &serde_json::to_vec(&expansion(session, "yolo", args)).unwrap(),
        &[("CLAUDE_PID", &pid), ("CLAUDE_CODE_SESSION_ID", session)],
    )
}

/// The block `/yolo` is answered with, and its reason.
#[track_caller]
fn blocked(fired: &Fired) -> String {
    assert_eq!(fired.code, Some(0));
    assert!(fired.took < PATIENCE, "took {:?}", fired.took);
    let out = String::from_utf8(fired.stdout.clone()).unwrap();
    assert_eq!(out.lines().count(), 1, "{out}");
    let answer: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(answer["decision"], "block", "{out}");
    assert_eq!(
        answer["hookSpecificOutput"],
        serde_json::json!({"hookEventName": "UserPromptExpansion", "suppressOriginalPrompt": true}),
    );
    assert_eq!(answer.as_object().unwrap().len(), 3, "{out}");
    answer["reason"].as_str().unwrap().to_string()
}

/// Exit 0, nothing on stdout, and back promptly.
#[track_caller]
fn assert_silent(fired: &Fired, case: &str) {
    assert_eq!(fired.code, Some(0), "{case}: exited non-zero");
    assert_eq!(
        String::from_utf8_lossy(&fired.stdout),
        "",
        "{case}: said something"
    );
    assert!(fired.took < PATIENCE, "{case}: took {:?}", fired.took);
}

/// The session YOLO is on for, asked from the process it is on for, is
/// allowed — with exactly the answer, and the allow told to the cctops and
/// written down for the page.
#[test]
fn the_recorded_session_in_the_recorded_process_is_allowed() {
    let dir = sandbox("allow");
    let hooks = dir.join("run").join("cctop").join("hooks.d");
    std::fs::create_dir_all(&hooks).unwrap();
    let listener = UnixListener::bind(hooks.join("1-0000000000000000001.sock")).unwrap();
    switch(&dir, "on", Some(me()));

    let fired = fire(&dir, &request("on"));
    assert_eq!(fired.code, Some(0));
    assert_eq!(String::from_utf8_lossy(&fired.stdout), format!("{ALLOW}\n"));
    assert!(fired.took < PATIENCE, "took {:?}", fired.took);

    let doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(state(&dir)).unwrap()).unwrap();
    assert_eq!(
        doc["sessions"]["on"]["allowed"][0]["ask"],
        "Bash: rm -rf build"
    );
    listener.set_nonblocking(true).unwrap();
    let (mut conn, _) = listener.accept().expect("the allow was not announced");
    conn.set_nonblocking(false).unwrap();
    conn.set_read_timeout(Some(PATIENCE)).unwrap();
    let mut line = String::new();
    let _ = conn.read_to_string(&mut line);
    let event: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(event["event"], "cctop.yolo.allowed");
    assert_eq!(event["session_id"], "on");
    assert_eq!(event["ask"], "Bash: rm -rf build");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The allow is also written to the lasting record — one line, redacted,
/// with its tool and whole command — and the hook says exactly the allow.
#[test]
fn an_allow_is_logged_redacted() {
    let dir = sandbox("log");
    switch(&dir, "on", Some(me()));
    let fired = fire(&dir, &request_for("on", "TOKEN=dummy make\nmake install"));
    assert_eq!(fired.code, Some(0));
    assert_eq!(String::from_utf8_lossy(&fired.stdout), format!("{ALLOW}\n"));
    let lines = logged(&dir);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert_eq!(line["event"], "allow");
    assert_eq!(line["session"], "on");
    assert_eq!(line["via"], "hook");
    assert_eq!(line["harness"], "claude");
    assert_eq!(line["cwd"], "/w");
    assert_eq!(line["tool"], "Bash");
    assert_eq!(line["detail"], "TOKEN=[redacted] make\nmake install");
    let raw = std::fs::read_to_string(yolo_log(&dir)).unwrap();
    assert!(!raw.contains("dummy"), "{raw}");
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(yolo_log(&dir))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);

    // Silence is not logged.
    assert_silent(&fire(&dir, &request("other")), "another session");
    assert_eq!(logged(&dir).len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A data directory the log cannot be written in changes nothing about the
/// answer.
#[test]
fn an_unwritable_log_still_allows() {
    use std::os::unix::fs::PermissionsExt;
    let dir = sandbox("readonly-log");
    switch(&dir, "on", Some(me()));
    std::fs::set_permissions(dir.join("data"), std::fs::Permissions::from_mode(0o500)).unwrap();
    let fired = fire(&dir, &request("on"));
    std::fs::set_permissions(dir.join("data"), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(fired.code, Some(0));
    assert_eq!(String::from_utf8_lossy(&fired.stdout), format!("{ALLOW}\n"));
    assert!(!yolo_log(&dir).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every way the match can fail is the same silence: another session, another
/// process, a pid whose start time is not the recorded one, an entry with no
/// process, and a switch file that is missing or not JSON.
#[test]
fn anything_but_that_session_and_process_is_silence() {
    let dir = sandbox("silence");
    let (pid, start) = me();

    assert_silent(&fire(&dir, &request("on")), "no switch at all");
    std::fs::write(state(&dir), b"{\"sessions\": {").unwrap();
    assert_silent(&fire(&dir, &request("on")), "a corrupt switch");

    switch(&dir, "on", Some((pid, start)));
    assert_silent(&fire(&dir, &request("other")), "another session");

    // A live process that is not the hook's ancestor: what a resumed session
    // looks like, the same id in a new process.
    let mut stranger = Command::new("sleep").arg("30").spawn().unwrap();
    switch(&dir, "on", Some((stranger.id(), start_of(stranger.id()))));
    assert_silent(&fire(&dir, &request("on")), "another process");
    let _ = stranger.kill();
    let _ = stranger.wait();

    switch(&dir, "on", Some((pid, start + 1)));
    assert_silent(&fire(&dir, &request("on")), "a reused pid");
    switch(&dir, "on", Some((u32::MAX - 1, 1)));
    assert_silent(&fire(&dir, &request("on")), "a dead pid");
    switch(&dir, "on", None);
    assert_silent(&fire(&dir, &request("on")), "an entry from before the hook");
    let _ = std::fs::remove_dir_all(&dir);
}

/// With YOLO on and the process right, a payload that is not a permission
/// prompt — empty, malformed, not UTF-8, oversized, a question, another
/// event — gets nothing.
#[test]
fn a_payload_that_is_not_a_permission_prompt_is_silence() {
    let dir = sandbox("payloads");
    switch(&dir, "on", Some(me()));

    let mut oversized = request("on");
    oversized.truncate(oversized.len() - 1);
    oversized.extend(br#","pad":""#);
    oversized.extend(std::iter::repeat_n(b'x', 300 * 1024));
    oversized.extend(br#""}"#);
    let question = serde_json::to_vec(&serde_json::json!({
        "session_id": "on",
        "hook_event_name": "PermissionRequest",
        "tool_name": "AskUserQuestion",
        "tool_input": {"questions": [{"question": "Which?"}]},
    }))
    .unwrap();
    let mut elsewhere: serde_json::Value = serde_json::from_slice(&request("on")).unwrap();
    elsewhere["hook_event_name"] = "PreToolUse".into();
    let elsewhere = serde_json::to_vec(&elsewhere).unwrap();
    for (case, payload) in [
        ("empty", Vec::new()),
        ("malformed", b"{ this is not json".to_vec()),
        ("not utf-8", b"\xff\xfe\x00{\"session_id\":\"on\"}".to_vec()),
        ("oversized", oversized),
        ("a question", question),
        ("a payload about another event", elsewhere),
    ] {
        assert_silent(&fire(&dir, &payload), case);
    }
    for event in ["PreToolUse", "Elicitation", ""] {
        assert_silent(
            &fire_as(&dir, "yolo-hook", event, &request("on")),
            &format!("fired for {event:?}"),
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A writer holding the switch's lock does not hold the agent up: the answer
/// is read without the lock, and the list it would have been added to is
/// given up on inside the deadline.
#[test]
fn a_held_lock_does_not_hold_the_agent_up() {
    let dir = sandbox("locked");
    switch(&dir, "on", Some(me()));
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("run").join("cctop").join("yolo.lock"))
        .unwrap();
    // SAFETY: flock on a descriptor this test owns until the end.
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    let fired = fire(&dir, &request("on"));
    assert_eq!(fired.code, Some(0));
    assert_eq!(String::from_utf8_lossy(&fired.stdout), format!("{ALLOW}\n"));
    assert!(
        fired.took < Duration::from_secs(2),
        "a held lock held the hook: {:?}",
        fired.took
    );
    // The page's list gave up on the lock; the lasting record never asked
    // for it.
    assert_eq!(logged(&dir).len(), 1, "the busy lock cost the log line");
    drop(lock);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The observer is still the observer: with YOLO on for this very session and
/// process, `cctop hook PermissionRequest` says nothing.
#[test]
fn the_observer_never_decides_even_for_a_yolo_session() {
    let dir = sandbox("observer");
    switch(&dir, "on", Some(me()));
    assert_silent(
        &fire_as(&dir, "hook", "PermissionRequest", &request("on")),
        "cctop hook PermissionRequest",
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `cctop yolo log` reads both files from the data directory, and
/// `--session` keeps the sessions whose id starts with it.
#[test]
fn yolo_log_reads_both_files_and_filters() {
    let dir = sandbox("reader");
    let data = dir.join("data").join("cctop");
    std::fs::create_dir_all(&data).unwrap();
    let line = |session: &str, at: &str, detail: &str| {
        serde_json::json!({
            "at": at, "event": "allow", "session": session, "via": "key", "detail": detail,
        })
        .to_string()
            + "\n"
    };
    std::fs::write(
        data.join("yolo-log.old.jsonl"),
        line("abc-1", "2026-01-01T00:00:00Z", "cargo build")
            + &line("xyz-1", "2026-01-02T00:00:00Z", "ls"),
    )
    .unwrap();
    std::fs::write(
        data.join("yolo-log.jsonl"),
        line("abc-2", "2026-02-01T00:00:00Z", "cargo test"),
    )
    .unwrap();
    let out = command(&dir)
        .args(["yolo", "log", "--json", "--session", "abc"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let sessions: Vec<String> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["session"].to_string())
        .collect();
    assert_eq!(sessions, ["\"abc-1\"", "\"abc-2\""]);

    let help = command(&dir).arg("yolo").output().unwrap();
    assert_eq!(help.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&help.stdout).contains("cctop yolo log"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `/yolo`, `/yolo status` and `/yolo off` typed in Claude Code are answered
/// by the hook itself, with a block whose reason is the answer — so the
/// command never expands and no model turn runs — and the switch they throw
/// is the one the `PermissionRequest` hook honours, logged as typed.
#[test]
fn typed_yolo_is_answered_by_the_hook() {
    let dir = sandbox("typed");
    assert_eq!(
        blocked(&type_yolo(&dir, "s1", "status")),
        "YOLO is off for this session."
    );
    assert!(!state(&dir).exists(), "status wrote the switch");

    let on = blocked(&type_yolo(&dir, "s1", ""));
    assert!(on.starts_with("YOLO on for this session"), "{on}");
    let doc: serde_json::Value =
        serde_json::from_slice(&std::fs::read(state(&dir)).unwrap()).unwrap();
    assert_eq!(doc["sessions"]["s1"]["agent"]["pid"], std::process::id());
    // What the switch is for: the next permission prompt is allowed.
    let fired = fire(&dir, &request("s1"));
    assert_eq!(String::from_utf8_lossy(&fired.stdout), format!("{ALLOW}\n"));

    let status = blocked(&type_yolo(&dir, "s1", " status "));
    assert!(
        status.starts_with("YOLO is on for this session"),
        "{status}"
    );
    assert!(status.contains("1 prompt allowed"), "{status}");

    let off = blocked(&type_yolo(&dir, "s1", "off"));
    assert!(off.starts_with("YOLO off for this session"), "{off}");
    assert_silent(&fire(&dir, &request("s1")), "a prompt after /yolo off");
    assert_eq!(
        blocked(&type_yolo(&dir, "s1", "off")),
        "YOLO was not on for this session."
    );

    // The words it does not know are answered too, and change nothing.
    let bad = blocked(&type_yolo(&dir, "s1", "yes please"));
    assert!(bad.contains("on, off or status"), "{bad}");

    let switches: Vec<(String, String)> = logged(&dir)
        .iter()
        .filter(|l| l["event"] != "allow")
        .map(|l| {
            (
                l["event"].as_str().unwrap().to_string(),
                l["from"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        switches,
        [
            ("on".into(), "typed".into()),
            ("off".into(), "typed".into())
        ]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Without a `CLAUDE_PID` naming one of its ancestors there is no process to
/// give the permission to: `/yolo` is still answered — the person typed it —
/// but with why, and nothing is switched.
#[test]
fn typed_yolo_without_its_agent_switches_nothing() {
    let dir = sandbox("typed-no-agent");
    let payload = serde_json::to_vec(&expansion("s1", "yolo", "on")).unwrap();
    for vars in [vec![], vec![("CLAUDE_PID", "1")]] {
        let why = blocked(&fire_with(
            &dir,
            "yolo-hook",
            "UserPromptExpansion",
            &payload,
            &vars,
        ));
        assert!(why.starts_with("YOLO unchanged"), "{why}");
    }
    assert!(!state(&dir).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every expansion but Claude Code's own `/yolo` is silence, whichever way
/// it differs — and `cctop hook` says nothing even for `/yolo`.
#[test]
fn every_other_expansion_is_silence() {
    let dir = sandbox("typed-silence");
    let pid = std::process::id().to_string();
    let vars = [
        ("CLAUDE_PID", pid.as_str()),
        ("CLAUDE_CODE_SESSION_ID", "s1"),
    ];
    let with = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut body = expansion("s1", "yolo", "on");
        change(&mut body);
        serde_json::to_vec(&body).unwrap()
    };
    for (case, payload) in [
        (
            "another command",
            with(&|b| b["command_name"] = "deploy".into()),
        ),
        (
            "a longer name",
            with(&|b| b["command_name"] = "yolo2".into()),
        ),
        (
            "a plugin's yolo",
            with(&|b| b["command_name"] = "p:yolo".into()),
        ),
        (
            "an MCP prompt named yolo",
            with(&|b| b["expansion_type"] = "mcp_prompt".into()),
        ),
        (
            "a payload about another event",
            with(&|b| b["hook_event_name"] = "UserPromptSubmit".into()),
        ),
        (
            "no command name",
            with(&|b| b["command_name"] = serde_json::Value::Null),
        ),
        ("no session", with(&|b| b["session_id"] = "".into())),
        ("empty", Vec::new()),
        ("malformed", b"{ not json".to_vec()),
    ] {
        assert_silent(
            &fire_with(&dir, "yolo-hook", "UserPromptExpansion", &payload, &vars),
            case,
        );
    }
    let yolo = with(&|_| {});
    // `/yolo`'s payload on any other event, and the observer on this one.
    for event in ["PermissionRequest", "UserPromptSubmit", "PreToolUse", ""] {
        assert_silent(
            &fire_with(&dir, "yolo-hook", event, &yolo, &vars),
            &format!("yolo-hook fired for {event:?}"),
        );
    }
    assert_silent(
        &fire_with(&dir, "hook", "UserPromptExpansion", &yolo, &vars),
        "cctop hook UserPromptExpansion",
    );
    assert!(!state(&dir).exists(), "something switched");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `cctop yolo on` is refused, inside a session and out — a model's Bash
/// call is indistinguishable from the person there — while `status` and
/// `off`, which cannot widen anything, still answer.
#[test]
fn the_cli_cannot_switch_yolo_on() {
    let dir = sandbox("cli-on");
    let pid = std::process::id().to_string();
    let inside = [
        ("CLAUDECODE", "1"),
        ("CLAUDE_PID", pid.as_str()),
        ("CLAUDE_CODE_SESSION_ID", "s1"),
    ];
    for vars in [&inside[..], &[]] {
        let mut cmd = command(&dir);
        cmd.env_remove("CLAUDE_PID")
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CLAUDECODE");
        for (k, v) in vars {
            cmd.env(k, v);
        }
        let out = cmd.args(["yolo", "on"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("/yolo"));
    }
    // The flag the old `/yolo` skill ran is no way in either.
    let out = command(&dir)
        .envs(inside)
        .args(["yolo", "--slash=k", "on"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(!state(&dir).exists());

    let status = command(&dir)
        .envs(inside)
        .args(["yolo", "status"])
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&status.stdout),
        "YOLO is off for this session.\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
