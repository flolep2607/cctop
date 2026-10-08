//! Type a line into the terminal driving a live agent session.
//!
//! An interactive agent reads its keyboard from the pty its terminal owns, and
//! only whoever holds that pty's master side can push bytes in. Writing to
//! `/proc/<pid>/fd/0` or `/dev/pts/N` reaches the *output* side and merely paints
//! the screen, and the master can't be reopened through `/proc` — that symlink
//! points at `/dev/ptmx`, so opening it mints a fresh pty instead. So each
//! backend here is a different answer to "who holds the master":
//!
//! 1. [`shim`](crate::shim) — cctop does, because the agent was started by
//!    `cctop run`. Works anywhere, needs no privileges.
//! 2. rmux — cctop's own daemon ([`crate::mux`]) does, because the agent is in
//!    one of cctop's tabs, and `send-keys` asks it politely.
//! 3. `TIOCSTI` — nobody has to: the kernel pushes a byte into the slave's own
//!    input queue. Needs root for a foreign tty and `dev.tty.legacy_tiocsti=1`,
//!    both off by default, and it's the one path that reaches sessions started
//!    before cctop was involved.
//!
//! ponytail: no screen or zellij backend. `screen -X stuff` reaches a session's
//! *current* window and zellij's `write-chars` its *focused* pane, neither
//! targetable from a PID — add when someone asks.

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// What a real Enter key sends on a pty in raw mode. A `\n` is read as Enter by
/// some agents and ignored by others; `\r` is what the terminal would have sent.
const SUBMIT: char = '\r';

/// Gap between the text and the Enter that submits it.
///
/// An agent's TUI reads its keyboard in chunks, and a `\r` arriving in the same
/// chunk as the text is not a keypress — it is the newline in the middle of a
/// paste, which Claude Code keeps in the prompt instead of sending. Whether the
/// two land in one read is a race, so `s` submitted sometimes and left the line
/// sitting there other times. rmux never sees this because its Enter is a
/// second `send-keys` round trip; the paths that write bytes straight to the pty
/// have to leave the gap themselves.
///
// ponytail: a fixed delay, tuned against Claude Code. If some agent still eats
// the Enter, this is the number to raise.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(60);

/// One key, pressed on its own with no Enter after it.
///
/// What answers a permission prompt: the agents draw it as a menu that acts on
/// the keypress, so a digit or a letter *is* the answer and an Enter after it
/// would land on whatever the agent shows next. Only the keys an answer needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Escape,
}

impl Key {
    /// What the key sends down a pty.
    fn bytes(self) -> Vec<u8> {
        match self {
            Key::Char(c) => c.to_string().into_bytes(),
            Key::Escape => vec![0x1b],
        }
    }
}

/// What reaches the agent's keyboard: a line to submit, or one key.
#[derive(Debug, Clone, Copy)]
enum Input<'a> {
    Line(&'a str),
    Key(Key),
}

impl Input<'_> {
    /// The writes that make it up, each its own read on the agent's side — see
    /// [`SETTLE`] for why a line and its Enter cannot share one.
    fn writes(self) -> Vec<Vec<u8>> {
        match self {
            Input::Line(text) => vec![text.as_bytes().to_vec(), vec![SUBMIT as u8]],
            Input::Key(key) => vec![key.bytes()],
        }
    }
}

/// Type `text` into the terminal running the agent at `pid`, then submit it.
///
/// Backends are tried strongest first; each reports `None` when it doesn't apply
/// to this session, so a session under `cctop run` never falls through to the
/// root-only path and the error the user sees names every option they have.
pub fn send_line(pid: u32, text: &str) -> Result<(), String> {
    deliver(pid, Input::Line(text))
}

/// Press `key` in the terminal running the agent at `pid`, and nothing else.
pub fn press(pid: u32, key: Key) -> Result<(), String> {
    deliver(pid, Input::Key(key))
}

fn deliver(pid: u32, input: Input) -> Result<(), String> {
    let what = match input {
        Input::Line(_) => "send",
        Input::Key(_) => "press",
    };
    for (backend, send) in [
        (
            "shim",
            shim_send as fn(u32, Input) -> Option<Result<(), String>>,
        ),
        ("rmux", rmux_send),
        ("tiocsti", tiocsti_send),
    ] {
        if let Some(result) = send(pid, input) {
            crate::elog::bytes(
                "inject",
                what,
                "out",
                &input.writes().concat(),
                serde_json::json!({"pid": pid, "via": backend, "ok": result.is_ok()}),
            );
            return result;
        }
    }
    crate::elog::event(
        "inject",
        what,
        serde_json::json!({"pid": pid, "via": "none", "ok": false}),
    );
    Err(format!(
        "no way to type into session {pid}: start the agent with `cctop run <agent>` \
         or in a cctop tab, or run cctop as root with dev.tty.legacy_tiocsti=1"
    ))
}

/// Hand the line to the `cctop run` shim that owns this agent's pty.
fn shim_send(pid: u32, input: Input) -> Option<Result<(), String>> {
    let path = crate::shim::socket_path(pid)?;
    // A stale socket file from a crashed shim refuses connections, which is
    // indistinguishable from "no shim" and correctly falls through.
    let mut stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    Some(write_apart(&mut stream, input).map_err(|e| format!("cctop run socket: {e}")))
}

/// Write each part of `input`, letting the agent take one in before the next —
/// see [`SETTLE`] for why a line and its Enter cannot be one write.
fn write_apart(out: &mut impl std::io::Write, input: Input) -> std::io::Result<()> {
    for (i, write) in input.writes().iter().enumerate() {
        if i > 0 {
            std::thread::sleep(SETTLE);
        }
        out.write_all(write)?;
        out.flush()?;
    }
    Ok(())
}

/// Ask rmux to type into the pane holding this agent.
fn rmux_send(pid: u32, input: Input) -> Option<Result<(), String>> {
    let pane = pane_for(pid)?;
    Some(match input {
        Input::Line(text) => send(&pane, text),
        Input::Key(Key::Char(c)) => keys(&pane, &c.to_string(), true),
        Input::Key(Key::Escape) => keys(&pane, "Escape", false),
    })
}

/// Push the line into the tty's input queue with `TIOCSTI`.
///
/// The last resort, and the only one that reaches a session already running in a
/// plain terminal. Both of its preconditions are off by default, so an applicable
/// session with an unmet precondition returns the reason rather than falling
/// through to a less specific error.
fn tiocsti_send(pid: u32, input: Input) -> Option<Result<(), String>> {
    use std::os::fd::AsRawFd;

    let tty = std::fs::read_link(format!("/proc/{pid}/fd/0")).ok()?;
    if !tty.starts_with("/dev/pts/") {
        return None;
    }
    // No privilege pre-check: the kernel's two refusals say different things, and
    // it allows the unprivileged case where cctop shares the agent's controlling
    // terminal. Letting the ioctl decide is both shorter and more capable.
    let file = match std::fs::OpenOptions::new().write(true).open(&tty) {
        Ok(f) => f,
        Err(e) => return Some(Err(format!("{}: {e}", tty.display()))),
    };
    let push = |byte: u8| {
        // SAFETY: writing one byte through a live tty fd; TIOCSTI takes a
        // pointer to that single byte.
        if unsafe { libc::ioctl(file.as_raw_fd(), libc::TIOCSTI as _, &byte) } != -1 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        // The kernel distinguishes the two preconditions: EIO is the disabled
        // sysctl, EPERM is a tty that isn't ours and so needs CAP_SYS_ADMIN.
        Err(match err.raw_os_error() {
            // Checked before the tty-ownership gate, so CAP_SYS_ADMIN clears
            // both and root never reaches this branch.
            Some(libc::EIO) => "the kernel has TIOCSTI disabled: run cctop as root, or \
                 sysctl -w dev.tty.legacy_tiocsti=1"
                .into(),
            Some(libc::EPERM) => format!(
                "typing into {} needs cctop as root (or start the agent with `cctop run`)",
                tty.display()
            ),
            _ => format!("TIOCSTI: {err}"),
        })
    };
    // Same reason as the shim path: the line reaches the input queue as one
    // burst, so the Enter needs its own moment or it reads as a paste.
    for (i, write) in input.writes().iter().enumerate() {
        if i > 0 {
            std::thread::sleep(SETTLE);
        }
        for &byte in write {
            if let Err(e) = push(byte) {
                return Some(Err(e));
            }
        }
    }
    Some(Ok(()))
}

/// How far up the process tree to look for a pane before giving up. Deep enough
/// for a shell under a wrapper under a pane, short enough to never spin.
const MAX_DEPTH: usize = 32;

/// The rmux pane hosting `pid`, if it is in one.
///
/// rmux reports each pane's own child — usually the shell the agent was launched
/// from, or the agent itself when it *is* the pane command — so the two meet by
/// walking up from the agent.
fn pane_for(pid: u32) -> Option<String> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, false, ProcessRefreshKind::nothing());
    pane_in(&sys, &list_panes()?, pid)
}

/// The walk itself, over a pane list and process table the caller supplies.
///
/// Split out two ways: because the hijack below cannot be reproduced against
/// the real server without typing into whichever pane the test runner happens
/// to sit in — which is the bug, not a way to test it — and because
/// [`crate::peek`] asks the same question of every running session per tick,
/// which is one process scan and one pane list for the lot rather than one each.
pub(crate) fn pane_in(sys: &System, panes: &[(u32, String)], pid: u32) -> Option<String> {
    // Climbing past cctop itself leaves the agent's terminal and enters cctop's
    // own, so the pane it then finds belongs to the user's shell rather than to
    // the session being typed into. That is not a near miss: it sends the line
    // to whatever the user is doing right now. It bit for real — a pty child
    // spawned by the test suite has cctop as its parent, so `send_line` walked
    // out of the pty, up through cargo, and typed "continue" into the terminal
    // the developer was working in.
    //
    // A session cctop launched itself is reached by `shim_send` through its
    // socket, and one launched into rmux belongs to the rmux server rather than
    // to cctop, so neither legitimate case needs this walk to climb that far.
    let own = std::process::id();

    let mut current = pid;
    for _ in 0..MAX_DEPTH {
        if current == own {
            return None;
        }
        if let Some((_, pane)) = panes.iter().find(|(pane_pid, _)| *pane_pid == current) {
            return Some(pane.clone());
        }
        let parent = sys.process(Pid::from_u32(current))?.parent()?.as_u32();
        if parent == 0 || parent == current {
            return None;
        }
        current = parent;
    }
    None
}

/// Type `text` into `pane` and submit it.
fn send(pane: &str, text: &str) -> Result<(), String> {
    // `-l --` sends the text literally, so a message containing "Enter" or "C-c"
    // is typed rather than interpreted, and a leading dash isn't read as a flag.
    // The newline that submits it has to be a separate, non-literal key.
    keys(pane, text, true)?;
    keys(pane, "Enter", false)
}

/// `send-keys` to one pane: `key` typed as text when `literal`, else read as
/// the name of a key. The pane is named `session:window.pane`, as
/// [`list_panes`] names it.
fn keys(pane: &str, key: &str, literal: bool) -> Result<(), String> {
    let target = crate::mux::pane_target(pane).ok_or_else(|| format!("no pane {pane}"))?;
    crate::mux::send_keys(&target, key, literal)
}

/// Every pane of cctop's daemon as `(pane_pid, pane)`.
///
/// `None` when no daemon is running, which means "this session isn't in a
/// pane" — the caller's only question.
///
/// Only cctop's own daemon: an agent in one of the user's own rmux panes is
/// theirs, and is reached through `cctop run`'s shim or TIOCSTI instead (#197).
pub(crate) fn list_panes() -> Option<Vec<(u32, String)>> {
    let listing =
        crate::mux::list_all_panes("#{pane_pid} #{session_name}:#{window_index}.#{pane_index}")?;
    Some(
        listing
            .lines()
            .filter_map(|line| {
                let (pid, id) = line.split_once(' ')?;
                Some((pid.parse().ok()?, id.to_string()))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_wait::{PATIENCE, wait_asking, wait_until};

    /// The Enter must reach the agent as its own write. Appended to the text it
    /// can land in the same read, where a TUI takes it for the newline inside a
    /// paste and the line is typed but never sent — which is what `s` did.
    #[test]
    fn the_submit_key_is_written_apart_from_the_line() {
        use std::io::Write;

        /// Records what each `write_all` was given, which is exactly the
        /// distinction the agent's read loop can see.
        struct Writes(Vec<Vec<u8>>);
        impl Write for Writes {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.push(buf.to_vec());
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let mut out = Writes(Vec::new());
        let start = std::time::Instant::now();
        write_apart(&mut out, Input::Line("continue")).unwrap();

        assert_eq!(out.0, vec![b"continue".to_vec(), vec![b'\r']]);
        assert!(
            start.elapsed() >= SETTLE,
            "the two writes went out back to back, which is the race this avoids"
        );
    }

    /// A key is one write and nothing after it: an Enter following the answer
    /// to a prompt would land on whatever the agent drew next.
    #[test]
    fn a_key_is_pressed_alone() {
        assert_eq!(Input::Key(Key::Char('1')).writes(), vec![b"1".to_vec()]);
        assert_eq!(Input::Key(Key::Escape).writes(), vec![vec![0x1b]]);
    }

    /// A child of cctop's own process must never resolve to a pane, because the
    /// only pane above it is the one cctop is running in — the user's terminal.
    ///
    /// The regression: `send_line` on a pty child spawned by the suite walked
    /// out of the pty, up through cargo and the shell, and typed "continue" into
    /// whatever the developer had on screen. Driven through a synthetic pane
    /// list, since reproducing it against the real server means doing it again.
    #[test]
    fn the_walk_stops_before_it_reaches_cctops_own_terminal() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn a child to walk up from");
        let kid = child.id();

        // Claim cctop's own parent is a pane, exactly as the real server would
        // report the shell that cargo was launched from.
        let sys = {
            let mut sys = System::new();
            sys.refresh_processes_specifics(
                ProcessesToUpdate::All,
                false,
                ProcessRefreshKind::nothing(),
            );
            sys
        };
        let ancestor = sys
            .process(Pid::from_u32(std::process::id()))
            .and_then(|p| p.parent())
            .map(|p| p.as_u32());
        let ancestor = ancestor.expect("cctop's own parent is what the walk must not reach");

        // Both probes run while the child is alive: once it is reaped the walk
        // stops at the missing process and returns `None` for a reason that has
        // nothing to do with the guard, which is how the first cut of this test
        // passed against the unfixed code.
        let hijacked = pane_in(&sys, &[(ancestor, "%99".to_string())], kid);
        // Positive control: a pane that really is the target still resolves, so
        // the guard is not simply refusing everything.
        let found = pane_in(&sys, &[(kid, "%7".to_string())], kid);

        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(
            found.as_deref(),
            Some("%7"),
            "a real pane must still resolve"
        );
        assert_eq!(
            hijacked, None,
            "the walk climbed through cctop into the user's own pane"
        );
    }

    /// The whole feature is the round trip: find the pane holding a process we
    /// only know by PID, then have what we send arrive as that process's input.
    /// Nothing smaller than a real rmux server tests either half: this one is
    /// cctop's own daemon on a socket of the test's, never the user's.
    #[test]
    fn types_into_the_pane_holding_a_pid() {
        let _daemon = crate::mux::TestDaemon::new("types_into_the_pane_holding_a_pid");
        let out = std::env::temp_dir().join(format!("cctop-mux-test-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&out);
        // `tee` takes the path as an argument, so the reader is findable by
        // cmdline; the trailing `:` stops the shell from exec'ing it, keeping a
        // shell between the pane and the reader so the ancestor walk has to
        // climb at least one level. The file carries the pid, so two `cargo
        // test` runs cannot read each other's.
        let session = "cctop-mux-test";
        let script = format!("tee {} >/dev/null; :", out.display());
        crate::mux::new_session(
            session,
            None,
            Vec::new(),
            &["sh".into(), "-c".into(), script],
        )
        .expect("a session on the test's daemon");

        // A scan of every process is the expensive kind of look.
        let reader = wait_asking(|| {
            let sys = {
                let mut s = System::new();
                s.refresh_processes_specifics(
                    ProcessesToUpdate::All,
                    true,
                    ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::Always),
                );
                s
            };
            sys.processes()
                .values()
                .find(|p| {
                    p.name().to_string_lossy().starts_with("tee")
                        && p.cmd()
                            .iter()
                            .any(|a| a.to_string_lossy() == out.to_string_lossy())
                })
                .map(|p| p.pid().as_u32())
        });

        // Asked until it answers: under a loaded suite the daemon can be slow
        // to list a pane whose process the scan above already saw.
        let pane = reader.and_then(|reader| wait_asking(|| pane_for(reader)));
        // Wait for the input to arrive *before* tearing the session down. Killing
        // it first destroys the child mid-read, discarding the very thing under
        // test — which is why this passed locally and failed on a loaded runner.
        let text = pane.as_ref().and_then(|pane| {
            send(pane, "continue").unwrap();
            wait_until(PATIENCE, || {
                std::fs::read_to_string(&out).ok().filter(|t| !t.is_empty())
            })
        });
        let _ = crate::mux::kill_session(session);
        let _ = std::fs::remove_file(&out);

        assert!(pane.is_some(), "no pane found for the reader process");
        assert_eq!(
            text.expect("nothing reached the child as input").trim(),
            "continue"
        );
    }
}
