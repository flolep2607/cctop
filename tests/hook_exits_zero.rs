//! The guarantee, run the way an agent runs it.
//!
//! Everything in `crates/core/src/hook.rs`'s own tests exercises the pieces — the envelope,
//! the connect, the answer — and none of them runs `emit`, because `emit`
//! installs a panic hook that turns every unwind in the test binary into a
//! silent `exit(0)`. So the module's headline promise was asserted nowhere: that
//! the command an agent spawns many times a minute exits 0, writes nothing that
//! could be read as a decision, and comes back.
//!
//! These run the real binary, which is also the only way to see the parts that
//! exist only in the process as a whole: `fork`, the exit status Claude Code
//! reads as a *decision*, and a stdout that has to stay empty.

use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The ceiling on one fire, past which the child is killed and the fire fails.
///
/// Generous on purpose. The hook's own bound is a quarter of a second — the
/// deadline in `crates/core/src/hook.rs` — so a fire that runs into seconds is a stall, not
/// a slow path; what this also measures is the `fork` and `exec` around it,
/// which on a machine with several builds on it can take a while. The ceiling
/// exists so that a stall fails rather than hangs: an unbounded wait inside the
/// hook does not trip an assertion, it stops the suite until somebody notices a
/// build that never finishes.
const PATIENCE: Duration = Duration::from_secs(5);

/// The one answer that is not silence, spelled exactly as `answer_for` spells
/// it. Compared rather than parsed because this line is short enough that
/// equality says more than a parser would: a parse would also pass a line
/// that carried a second field.
const NO_OP: &str = r#"{"continue": true}"#;

/// One `cctop hook` fire, finished.
struct Fired {
    code: Option<i32>,
    stdout: String,
    took: Duration,
}

/// A directory of the hook's own, so no test touches the machine it runs on.
fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cctop-hook-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["run", "home", "cache", "config", "data"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

/// The command, pointed at `dir` and nowhere else.
///
/// `$XDG_RUNTIME_DIR` is what `socket_dir()` follows, so this is what stops one
/// of these firing into the sockets of whatever cctop is running on the machine,
/// and what stops a real cctop's listeners from being counted as peers of the
/// wedged one below. `HOME` goes with it: `warn_agents` is a setting in the
/// developer's own Claude configuration, and turned on it would put a warning on
/// the very stdout these tests are asserting is empty.
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

/// The socket directory the hook will look in, which does not exist yet.
fn hooks(dir: &Path) -> PathBuf {
    dir.join("run").join("cctop").join("hooks.d")
}

/// Run one hook with `stdin` closed behind it, or fail.
///
/// The give-up is not a convenience. An unbounded wait inside the hook — which
/// is what this whole file exists to rule out — does not make an assertion fail,
/// it makes the suite hang until somebody notices a build that never finishes,
/// so the wait is bounded and the child killed.
fn fire(dir: &Path, event: &str, stdin: Option<&str>) -> Fired {
    let started = Instant::now();
    let mut child = command(dir)
        .arg("hook")
        .arg(event)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Nothing here asserts on stderr, and a hook that complained on every
        // fire is a failure these tests would notice anyway.
        .stderr(Stdio::null())
        .spawn()
        .expect("the hook binary runs");

    let mut pipe = child.stdin.take().expect("stdin was piped");
    if let Some(text) = stdin {
        let _ = pipe.write_all(text.as_bytes());
    }
    // Closing it is what "the agent closed the pipe" looks like from the other
    // end, and it has to happen before the child reaches its read.
    drop(pipe);

    let mut out = child.stdout.take().expect("stdout was piped");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out.read_to_string(&mut text);
        let _ = tx.send(text);
    });
    // Read to end rather than waited on: the pipe closes with the process, so
    // EOF arriving is the process leaving — and one that never closes it is the
    // stall these tests are for.
    let stdout = rx.recv_timeout(PATIENCE).unwrap_or_else(|_| {
        let _ = child.kill();
        panic!("`cctop hook {event}` had not returned after {PATIENCE:?}");
    });
    let status = child.wait().expect("the hook exits");
    Fired {
        code: status.code(),
        stdout,
        took: started.elapsed(),
    }
}

/// Nothing is listening, which is the ordinary case: cctop is not running.
///
/// It has to be silent and it has to be quick. Silent because for most events
/// stdout is content the model may act on, and a monitor that writes a status
/// line into somebody's transcript is a monitor that has broken the session it
/// was meant to watch.
#[test]
fn a_hook_with_nothing_listening_exits_zero_and_says_nothing() {
    let dir = sandbox("quiet");
    let fired = fire(
        &dir,
        "PreToolUse",
        Some(r#"{"session_id":"abc","cwd":"/tmp","hook_event_name":"PreToolUse"}"#),
    );

    assert_eq!(fired.code, Some(0), "the hook failed");
    assert_eq!(fired.stdout, "", "stdout carried content for the model");
    assert!(fired.took < PATIENCE, "took {:?}", fired.took);
    let _ = std::fs::remove_dir_all(&dir);
}

/// An event that will not parse is still an event the agent must not be held up
/// by. The envelope is built from whatever arrived; a payload cctop cannot read
/// is dropped rather than reported.
#[test]
fn a_hook_whose_event_will_not_parse_exits_zero_and_says_nothing() {
    let dir = sandbox("garbage");
    let fired = fire(&dir, "PostToolUse", Some("{ this is not json at all"));

    assert_eq!(fired.code, Some(0), "the hook failed on a payload");
    assert_eq!(fired.stdout, "", "stdout carried content for the model");
    assert!(fired.took < PATIENCE, "took {:?}", fired.took);
    let _ = std::fs::remove_dir_all(&dir);
}

/// stdin closed with nothing on it, which is what a harness that runs the hook
/// without a payload does. A read that ends is a read that found nothing, and an
/// agent that closes the pipe is not an agent waiting.
#[test]
fn a_hook_whose_stdin_is_closed_exits_zero() {
    let dir = sandbox("closed");
    let fired = fire(&dir, "SessionStart", None);

    assert_eq!(fired.code, Some(0), "the hook failed on an empty stdin");
    assert!(fired.took < PATIENCE, "took {:?}", fired.took);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The two events that are documented to answer on stdout answer with the one
/// thing that cannot mean anything, and with nothing beside it.
///
/// `continue: true` is the documented default in both Codex and Claude Code, so
/// writing it says exactly what silence was meant to say — and no harness
/// reading it can find a decision in it. Anything else on the line would be.
#[test]
fn the_events_that_demand_an_answer_get_the_no_op_and_nothing_else() {
    let dir = sandbox("answer");
    for event in ["Stop", "SubagentStop"] {
        let fired = fire(
            &dir,
            event,
            Some(r#"{"session_id":"abc","hook_event_name":"Stop"}"#),
        );
        assert_eq!(fired.code, Some(0), "{event} failed");
        assert_eq!(
            fired.stdout.trim(),
            NO_OP,
            "{event} answered something else"
        );
        assert!(fired.took < PATIENCE, "{event} took {:?}", fired.took);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A cctop that is bound and listening and has stopped draining its queue.
///
/// This is what a cctop whose own thread has stalled looks like from outside,
/// and it is the shape that made `UnixStream::connect` — which takes no timeout
/// — block on the queue rather than fail on it.
///
/// Note what this does and does not prove. The hook survives the wedge either
/// way, because the thread that would be stuck in that connect is one the
/// process exits on. What the fix changed is that the connect no longer waits at
/// all, so the same stall cannot hold a `serve` connection thread — there are
/// `MAX_CONNECTIONS` of those, and `--tunnel` puts them behind a URL anybody can
/// reach. `a_wedged_cctop_does_not_hold_the_agent_up` in `crates/core/src/hook.rs` is the
/// test that fails when the connect stops being bounded; this one is here
/// because the guarantee is about the process, and only the process is all of it.
#[test]
fn a_hook_whose_cctop_has_stopped_accepting_still_returns() {
    let dir = sandbox("wedged");
    let hooks = hooks(&dir);
    std::fs::create_dir_all(&hooks).unwrap();
    let path = hooks.join("1-0000000000000000001.sock");
    let listener = UnixListener::bind(&path).expect("bind");
    // `bind` listens on `SOMAXCONN`, 4096 here and 128 elsewhere, and the
    // fixture holds every connection it makes; listening again on the same
    // socket is how Linux takes a new backlog, and one is plenty.
    // SAFETY: the listener's own descriptor, which it keeps owning.
    assert_eq!(
        unsafe { libc::listen(listener.as_raw_fd(), 1) },
        0,
        "listen"
    );

    // Fill the queue and know it is full: a non-blocking connect that finds no
    // room is refused with EAGAIN rather than left waiting, so the kernel says
    // when the fixture is done instead of a quiet second on the clock. Held
    // until the end, because dropping them would drain the queue.
    let held = cctop_core::test_wait::fill_queue(&path);
    assert!(!held.is_empty(), "the fixture never reached the listener");

    let fired = fire(
        &dir,
        "Stop",
        Some(r#"{"session_id":"abc","hook_event_name":"Stop"}"#),
    );
    assert_eq!(
        fired.code,
        Some(0),
        "the hook failed against a wedged cctop"
    );
    assert!(fired.took < PATIENCE, "took {:?}", fired.took);
    // And it did not deafen a cctop that is still listening there by unlinking
    // the socket of a process it could not get through to.
    assert!(
        path.exists(),
        "the hook deleted a live cctop's socket because it would not answer"
    );

    drop(held);
    drop(listener);
    let _ = std::fs::remove_dir_all(&dir);
}
