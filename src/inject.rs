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
//! 2. tmux — the server does, and `send-keys` asks it politely.
//! 3. `TIOCSTI` — nobody has to: the kernel pushes a byte into the slave's own
//!    input queue. Needs root for a foreign tty and `dev.tty.legacy_tiocsti=1`,
//!    both off by default, and it's the one path that reaches sessions started
//!    before cctop was involved.
//!
//! ponytail: no screen or zellij backend. `screen -X stuff` reaches a session's
//! *current* window and zellij's `write-chars` its *focused* pane, neither
//! targetable from a PID — add when someone asks.

use std::process::Command;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

/// What a real Enter key sends on a pty in raw mode. A `\n` is read as Enter by
/// some agents and ignored by others; `\r` is what the terminal would have sent.
const SUBMIT: char = '\r';

/// Type `text` into the terminal running the agent at `pid`, then submit it.
///
/// Backends are tried strongest first; each reports `None` when it doesn't apply
/// to this session, so a session under `cctop run` never falls through to the
/// root-only path and the error the user sees names every option they have.
pub fn send_line(pid: u32, text: &str) -> Result<(), String> {
    for backend in [shim_send, tmux_send, tiocsti_send] {
        if let Some(result) = backend(pid, text) {
            return result;
        }
    }
    Err(format!(
        "no way to type into session {pid}: start the agent with `cctop run <agent>` \
         or inside tmux, or run cctop as root with dev.tty.legacy_tiocsti=1"
    ))
}

/// Hand the line to the `cctop run` shim that owns this agent's pty.
#[cfg(unix)]
fn shim_send(pid: u32, text: &str) -> Option<Result<(), String>> {
    use std::io::Write;

    let path = crate::shim::socket_path(pid)?;
    // A stale socket file from a crashed shim refuses connections, which is
    // indistinguishable from "no shim" and correctly falls through.
    let mut stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    Some(
        stream
            .write_all(format!("{text}{SUBMIT}").as_bytes())
            .map_err(|e| format!("cctop run socket: {e}")),
    )
}

#[cfg(not(unix))]
fn shim_send(_pid: u32, _text: &str) -> Option<Result<(), String>> {
    None
}

/// Ask tmux to type into the pane holding this agent.
fn tmux_send(pid: u32, text: &str) -> Option<Result<(), String>> {
    let pane = pane_for(pid)?;
    Some(send(&pane, text))
}

/// Push the line into the tty's input queue with `TIOCSTI`.
///
/// The last resort, and the only one that reaches a session already running in a
/// plain terminal. Both of its preconditions are off by default, so an applicable
/// session with an unmet precondition returns the reason rather than falling
/// through to a less specific error.
#[cfg(target_os = "linux")]
fn tiocsti_send(pid: u32, text: &str) -> Option<Result<(), String>> {
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
    for byte in text.bytes().chain(std::iter::once(SUBMIT as u8)) {
        // SAFETY: writing one byte through a live tty fd; TIOCSTI takes a
        // pointer to that single byte.
        if unsafe { libc::ioctl(file.as_raw_fd(), libc::TIOCSTI, &byte) } == -1 {
            let err = std::io::Error::last_os_error();
            // The kernel distinguishes the two preconditions: EIO is the disabled
            // sysctl, EPERM is a tty that isn't ours and so needs CAP_SYS_ADMIN.
            return Some(Err(match err.raw_os_error() {
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
            }));
        }
    }
    Some(Ok(()))
}

#[cfg(not(target_os = "linux"))]
fn tiocsti_send(_pid: u32, _text: &str) -> Option<Result<(), String>> {
    None
}

/// How far up the process tree to look for a pane before giving up. Deep enough
/// for a shell under a wrapper under a pane, short enough to never spin.
const MAX_DEPTH: usize = 32;

/// The tmux pane hosting `pid`, if it is in one.
///
/// tmux reports each pane's own child — usually the shell the agent was launched
/// from, or the agent itself when it *is* the pane command — so the two meet by
/// walking up from the agent.
fn pane_for(pid: u32) -> Option<String> {
    let panes = list_panes()?;
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, false, ProcessRefreshKind::nothing());

    let mut current = pid;
    for _ in 0..MAX_DEPTH {
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
    tmux(&["send-keys", "-t", pane, "-l", "--", text])?;
    tmux(&["send-keys", "-t", pane, "Enter"])
}

/// Every pane on the server as `(pane_pid, pane_id)`.
///
/// `None` when tmux isn't installed or no server is running — both mean "this
/// session isn't in a pane", which is the caller's only question.
fn list_panes() -> Option<Vec<(u32, String)>> {
    let out = Command::new("tmux")
        .args(["list-panes", "-a", "-F", "#{pane_pid} #{pane_id}"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                let (pid, id) = line.split_once(' ')?;
                Some((pid.parse().ok()?, id.to_string()))
            })
            .collect(),
    )
}

fn tmux(args: &[&str]) -> Result<(), String> {
    let out = Command::new("tmux")
        .args(args)
        .output()
        .map_err(|e| format!("tmux: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if err.is_empty() {
        "tmux rejected the keys".into()
    } else {
        err
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole feature is the round trip: find the pane holding a process we
    /// only know by PID, then have what we send arrive as that process's input.
    /// Nothing smaller than a real tmux server tests either half.
    #[test]
    fn types_into_the_pane_holding_a_pid() {
        if Command::new("tmux").arg("-V").output().is_err() {
            eprintln!("skipping: tmux not installed");
            return;
        }
        let out = std::env::temp_dir().join("cctop-mux-test.txt");
        let _ = std::fs::remove_file(&out);
        // `tee` takes the path as an argument, so the reader is findable by
        // cmdline; the trailing `:` stops the shell from exec'ing it, keeping a
        // shell between the pane and the reader so the ancestor walk has to
        // climb at least one level.
        let session = "cctop-mux-test";
        let script = format!("tee {} >/dev/null; :", out.display());
        assert!(
            Command::new("tmux")
                .args(["new-session", "-d", "-s", session, "sh", "-c", &script])
                .status()
                .unwrap()
                .success()
        );

        let reader = wait_for(|| {
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
                            .any(|a| a.to_string_lossy().contains("cctop-mux-test.txt"))
                })
                .map(|p| p.pid().as_u32())
        });

        let pane = reader.and_then(pane_for);
        if let Some(pane) = &pane {
            send(pane, "continue").unwrap();
        }
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", session])
            .status();

        assert!(pane.is_some(), "no pane found for the reader process");
        let text = wait_for(|| std::fs::read_to_string(&out).ok().filter(|t| !t.is_empty()));
        let _ = std::fs::remove_file(&out);
        assert_eq!(text.unwrap().trim(), "continue");
    }

    /// tmux starts panes and flushes writes asynchronously; poll rather than
    /// guess a sleep long enough for a loaded CI box.
    fn wait_for<T>(mut f: impl FnMut() -> Option<T>) -> Option<T> {
        for _ in 0..50 {
            if let Some(v) = f() {
                return Some(v);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        None
    }
}
