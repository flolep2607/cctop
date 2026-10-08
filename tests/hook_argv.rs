//! The hook is spawned by somebody else's program, so the only thing that can be
//! relied on about its command line is that it is the one the harness wrote.
//! These drive the real binary rather than anything callable from a unit test,
//! because the guarantee they check — an exit code — is decided before any of
//! cctop's code is in a position to influence it.

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::process::Command;

fn cctop(args: &[OsString]) -> std::process::Output {
    // The spawned binary is cctop proper, not a test, so the redirects core
    // applies under test do not reach it: its sockets, cache and settings are
    // wherever the environment says, which has to be somewhere of this test's.
    let dir = std::env::temp_dir().join(format!("cctop-hook-argv-{}", std::process::id()));
    for sub in ["run", "home", "cache"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    Command::new(env!("CARGO_BIN_EXE_cctop"))
        .args(args)
        // Nothing of the developer's own configuration in the answer: the hook
        // reads the socket directory and, with `warn_agents`, a ledger.
        .env("CCTOP_LOG", "off")
        .env("XDG_RUNTIME_DIR", dir.join("run"))
        .env("HOME", dir.join("home"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .output()
        .expect("the binary is built")
}

/// A non-UTF-8 argument is legal in `execve` and `std::env::args` panics on one.
/// It reached the guard before the guard that turns an unwind into a silent
/// success, which exited 101 — the code Claude Code reads as a decision to
/// block the tool call.
#[test]
fn a_hook_with_a_non_utf8_argument_still_exits_zero() {
    let out = cctop(&[
        OsString::from("hook"),
        OsString::from("Stop"),
        OsString::from_vec(b"\xff\xfe".to_vec()),
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The dispatch compares the OS string, so a word that is nearly `hook` is not
/// `hook`: it has to reach whatever answers a command cctop does not know,
/// whatever that turns out to be, and not the path whose whole promise is to
/// exit 0 silently.
#[test]
fn a_near_miss_first_argument_is_not_answered_as_the_hook() {
    let out = cctop(&[OsString::from_vec(b"hoo\xffk".to_vec())]);
    assert_ne!(out.status.code(), Some(0), "it fell into the hook path");
}
