//! What [`shim`](crate::shim) is on a platform without ptys.
//!
//! The real module runs an agent on a pty cctop owns and serves it over a unix
//! socket, so a tab can draw it and type into it. Both halves of that are unix:
//! raw `openpty`/`TIOCSWINSZ` ioctls, `CommandExt`, `UnixListener`. There is a
//! Windows equivalent — ConPTY and named pipes — but it is a port of the whole
//! feature rather than a shim over it, and nothing in the dashboard needs it.
//!
//! So Windows gets the surface and not the behaviour, the same bargain
//! [`attach`](crate::attach) already makes: hosting fails with a message that
//! says why, and nothing is ever running to enumerate. The session table, which
//! is what cctop is for, reads transcripts and process lists and does not care.
//!
//! Kept in step with the real module by the compiler, in both directions. Every
//! item here exists because something outside calls it *unconditionally*, and
//! nothing else may be here: `run`, `sessions` and `socket_path` are part of the
//! real surface but every caller of theirs is unix- or linux-gated, so a
//! courtesy stub of them would be a function with no callers — which is dead
//! code, which CI's `-D warnings` makes a build failure.

use std::path::PathBuf;

/// Stands in for a hosted agent, which this platform never has.
///
/// Constructible only by [`host`], which always fails, so the fields are only
/// ever read through a value that cannot exist. They are here because the
/// callers name them.
pub struct Hosted {
    pub pid: u32,
    pub label: String,
}

impl Hosted {
    /// Always finished, since nothing was ever started.
    pub fn finished(&mut self) -> Option<i32> {
        Some(1)
    }
}

/// Start an agent in a tab. See the module docs for why this cannot.
pub fn host(_argv: &[String], _cwd: Option<&std::path::Path>) -> anyhow::Result<Hosted> {
    anyhow::bail!("tabs need a pty, which Windows has not got — the dashboard works as usual")
}

/// Whether `word` names something runnable.
///
/// Real, unlike the rest of this module: it decides which agents the launcher
/// offers, and a wrong answer there is a wrong list rather than a failed
/// launch. Windows has no executable bit, so the extension does that job —
/// `PATHEXT` is the list of suffixes the shell would try, and a word that
/// already carries one is taken as given.
pub fn is_command(word: &str) -> bool {
    let exists = |p: PathBuf| p.is_file();
    let extensions = || {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .collect::<Vec<_>>()
    };
    let runnable = |base: PathBuf| {
        if base.extension().is_some() && exists(base.clone()) {
            return true;
        }
        extensions()
            .iter()
            .any(|ext| exists(PathBuf::from(format!("{}{ext}", base.display()))))
    };
    if word.contains('/') || word.contains('\\') {
        return runnable(PathBuf::from(word));
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .any(|dir| runnable(dir.join(word)))
}
