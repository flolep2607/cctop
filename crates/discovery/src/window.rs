//! Where a running agent can be looked at: its terminal, and its tmux pane.

use std::path::PathBuf;

/// One process running a session, and where it can be found on screen.
///
/// `#[non_exhaustive]`, so a field added later is not a breaking change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AgentProcess {
    /// The agent's own process.
    pub pid: u32,
    /// The terminal it runs in, as `/dev/pts/N`, when it runs in one.
    ///
    /// Read from the process's standard streams, so it is `None` for an agent
    /// with none of them on a terminal — one an editor runs, or one under
    /// another user, whose descriptors only that user and root can read.
    pub tty: Option<PathBuf>,
    /// The tmux pane it was started in, when it was started in one.
    pub tmux: Option<TmuxPane>,
}

/// A tmux pane, as the process inside it was told about it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TmuxPane {
    /// The pane's id, `%N`: what `tmux select-pane -t` takes. Stable for the
    /// pane's life, even when it moves to another window.
    pub pane: String,
    /// The server's socket, for `tmux -S`. A server other than the default
    /// one — `tmux -L name` — is reached only through this.
    pub socket: Option<PathBuf>,
}

impl AgentProcess {
    pub(crate) fn of(pid: u32) -> Self {
        AgentProcess {
            pid,
            tty: tty_of(pid),
            tmux: std::fs::read(format!("/proc/{pid}/environ"))
                .ok()
                .and_then(|env| tmux_in(&env)),
        }
    }
}

/// The terminal behind the first of stdin, stdout and stderr that is on one.
///
/// All three, because an agent can be started with any one of them redirected
/// — a prompt piped in, output teed to a log — and still be drawn in a
/// terminal by the others.
fn tty_of(pid: u32) -> Option<PathBuf> {
    (0..3)
        .filter_map(|fd| std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).ok())
        .find(|target| is_terminal(target))
}

fn is_terminal(path: &std::path::Path) -> bool {
    path.starts_with("/dev/pts") || path.to_str().is_some_and(|p| p.starts_with("/dev/tty"))
}

/// The pane in a process's environment, from what tmux sets in every pane:
/// `TMUX_PANE=%N`, and `TMUX=<socket>,<server pid>,<session>`.
///
/// The environment is the one the process started with. That is the right one
/// here: tmux sets these when it creates the pane, an agent inherits them from
/// the shell in it, and nothing later changes which pane that is.
fn tmux_in(environ: &[u8]) -> Option<TmuxPane> {
    let var = |name: &[u8]| {
        environ.split(|&b| b == 0).find_map(|entry| {
            let value = entry.strip_prefix(name)?.strip_prefix(b"=")?;
            std::str::from_utf8(value).ok()
        })
    };
    let pane = var(b"TMUX_PANE").filter(|p| p.starts_with('%'))?;
    let socket = var(b"TMUX")
        .and_then(|t| t.split(',').next())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    Some(TmuxPane {
        pane: pane.to_string(),
        socket,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_and_its_socket_are_read_from_the_environment() {
        let env = b"HOME=/home/a\0TMUX=/tmp/tmux-1000/default,4242,0\0TMUX_PANE=%17\0";
        assert_eq!(
            tmux_in(env),
            Some(TmuxPane {
                pane: "%17".into(),
                socket: Some("/tmp/tmux-1000/default".into()),
            })
        );
    }

    /// `TMUX_PANE` is the pane; `TMUX` alone is not enough to say which one, and
    /// a variable that merely starts with the same name is a different one.
    #[test]
    fn no_pane_without_tmux_pane() {
        assert_eq!(tmux_in(b"TMUX=/tmp/tmux-1000/default,1,0\0"), None);
        assert_eq!(tmux_in(b"TMUX_PANES=%1\0"), None);
        assert_eq!(tmux_in(b""), None);
        assert_eq!(
            tmux_in(b"TMUX_PANE=%3\0"),
            Some(TmuxPane {
                pane: "%3".into(),
                socket: None,
            })
        );
    }

    #[test]
    fn only_a_terminal_device_is_a_tty() {
        assert!(is_terminal("/dev/pts/4".as_ref()));
        assert!(is_terminal("/dev/tty2".as_ref()));
        assert!(!is_terminal("/dev/null".as_ref()));
        assert!(!is_terminal("pipe:[12345]".as_ref()));
        assert!(!is_terminal("/home/a/log.txt".as_ref()));
    }

    /// The real thing, on this process: whatever its streams are, the answer
    /// is consistent with them, and a process that is gone has neither.
    #[test]
    fn a_process_is_read_from_proc() {
        let me = AgentProcess::of(std::process::id());
        assert_eq!(me.pid, std::process::id());
        if let Some(tty) = &me.tty {
            assert!(is_terminal(tty), "{tty:?}");
        }
        let gone = AgentProcess::of(u32::MAX);
        assert_eq!((gone.tty, gone.tmux), (None, None));
    }
}
