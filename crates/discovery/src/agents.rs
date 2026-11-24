//! The one call this crate offers: every agent session on the machine, with the
//! process running it and what it is doing.

use crate::process::Collector;
use crate::session::{self, Discovered};
use crate::{ActivityState, Provider, paths};
use std::path::PathBuf;

/// One coding-agent session, as cctop shows it in a row.
///
/// `#[non_exhaustive]`, so a field added later is not a breaking change; build
/// one only through [`agents`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Agent {
    /// Which agent wrote it.
    pub provider: Provider,
    /// The agent's own id for the session: what its resume flag takes.
    pub session_id: String,
    /// The process running it, when one is. `None` is a session that is over,
    /// or one whose agent has not been matched to a process — see the crate
    /// docs for which agents can be.
    pub pid: Option<u32>,
    /// The directory the session works in, when the agent recorded one.
    pub cwd: Option<PathBuf>,
    /// The session's title: the agent's own, or one the user gave it.
    pub title: Option<String>,
    /// What the session is doing, read from the newest records of its
    /// transcript — for a running session only, since a finished one is
    /// doing nothing. A transcript cannot tell a permission prompt from work
    /// in progress, so `Asking` is rare here: see [`ActivityState::Asking`].
    pub state: Option<ActivityState>,
    /// When it last did something, RFC 3339.
    pub last_active: String,
}

/// Every session of every agent on this machine, newest first, with the ones
/// running matched to their process.
///
/// Reads files and the process table and nothing else: no network, no write,
/// no lock on another program's database. It is one pass, as long as a cctop
/// refresh, so call it on a timer of seconds, not in a loop.
pub fn agents() -> Vec<Agent> {
    let mut found = discovered();
    found.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    let processes = Collector::new().collect(&found);
    found
        .into_iter()
        .map(|s| {
            let pid = processes.get(&s.key()).and_then(|p| {
                p.process_list
                    .iter()
                    .find_map(|e| (e.is_root && !e.ghost).then_some(e.pid))
            });
            let state = pid.map(|_| session::state::live_state(&s).0);
            Agent {
                provider: s.provider,
                pid,
                // Cursor's is a project slug when its database says nothing
                // better, which is not a path.
                cwd: Some(PathBuf::from(&s.label_source)).filter(|p| p.is_absolute()),
                title: s.title.filter(|t| !t.is_empty()),
                state,
                last_active: s.last_active,
                session_id: s.session_id,
            }
        })
        .collect()
}

/// Every agent's sessions, each stamped with whose home it was read from.
///
/// ponytail: cctop-core's walk also folds the copies a session leaves in two
/// homes into one row and adds rows for agents with no transcript yet; this
/// does neither. Move that fold down when a caller sees the duplicate.
fn discovered() -> Vec<Discovered> {
    let mut all: Vec<Discovered> = [
        session::claude::list,
        session::codex::list,
        session::cursor::list,
        session::devin::list,
        session::gemini::list,
        session::opencode::list,
        session::pi::list,
        session::vibe::list,
        session::windsurf::list,
        session::antigravity::list,
    ]
    .iter()
    .flat_map(|list| list())
    .collect();
    for s in &mut all {
        if let Some(file) = &s.data_file {
            s.owner = paths::owner_of(file).map(str::to_string);
        }
    }
    all
}
