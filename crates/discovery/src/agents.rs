//! The one call this crate offers: every agent session on the machine, with the
//! processes running it and what it is doing.

use crate::process::{Attribution, Collector, ProcInfo};
use crate::session::{self, Discovered};
use crate::window::AgentProcess;
use crate::{ActivityState, Provider, paths, watch};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

/// One coding-agent session, as cctop shows it in a row.
///
/// `#[non_exhaustive]`, so a field added later is not a breaking change; build
/// one only through [`agents`] or [`scan`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Agent {
    /// Which agent wrote it.
    pub provider: Provider,
    /// The agent's own id for the session: what its resume flag takes.
    ///
    /// Unique in one listing: a session run by two processes at once is one
    /// `Agent` with two entries in [`processes`](Self::processes), never two
    /// `Agent`s with one id.
    pub session_id: String,
    /// The process running it, when one is: the first of
    /// [`processes`](Self::processes). `None` is a session that is over, or one
    /// whose agent has not been matched to a process — see the crate docs for
    /// which agents can be.
    pub pid: Option<u32>,
    /// Every process running this session, the one in [`pid`](Self::pid)
    /// first and the rest by pid.
    ///
    /// Usually one. More than one is the same session open twice — `claude
    /// --resume <id>` in two terminals — and each has its own terminal, so
    /// each is listed with its own [`tty`](AgentProcess::tty) and
    /// [`tmux`](AgentProcess::tmux) pane. The agent's own children (tools,
    /// shells, MCP servers) are not listed: these are the separate process
    /// trees, one per place the session can be looked at.
    pub processes: Vec<AgentProcess>,
    /// The directory the session works in, when the agent recorded one.
    pub cwd: Option<PathBuf>,
    /// The session's title: the agent's own, or one the user gave it.
    pub title: Option<String>,
    /// What the session is doing, read from the newest records of its
    /// transcript — for a running session only, since a finished one is
    /// doing nothing. A transcript cannot tell a permission prompt from work
    /// in progress, so `Asking` is rare here: see [`ActivityState::Asking`].
    ///
    /// One per session, not per process: two processes on one session write
    /// one transcript, and this is read from it.
    pub state: Option<ActivityState>,
    /// When it last did something, RFC 3339.
    pub last_active: String,
}

/// Something that kept one agent's sessions out of a [`Scan`].
///
/// The other agents' sessions are listed regardless: one agent whose files
/// cannot be read is no reason to see none at all.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScanError {
    /// Whose sessions may be missing.
    pub provider: Provider,
    /// The file or directory it was about, when it was about one.
    pub path: Option<PathBuf>,
    /// What went wrong, for a person to read.
    pub message: String,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: ", self.provider.display_name())?;
        if let Some(path) = &self.path {
            write!(f, "{}: ", path.display())?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for ScanError {}

/// What [`scan`] found, and what stopped it finding more.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Scan {
    /// As [`agents`] returns them.
    pub agents: Vec<Agent>,
    /// One for each agent, and each of its directories, that could not be
    /// read. Empty is the usual answer.
    pub errors: Vec<ScanError>,
}

/// Every session of every agent on this machine, newest first, with the ones
/// running matched to their processes.
///
/// Reads files and the process table and nothing else: no network, no write,
/// no lock on another program's database, no thread or runtime of its own. It
/// is one pass, as long as a cctop refresh, so call it from a worker thread
/// when something changes — [`watch_paths`](crate::watch_paths) says where —
/// or on a timer of seconds, not in a loop.
///
/// The same as [`scan`] with its errors dropped.
pub fn agents() -> Vec<Agent> {
    scan().agents
}

/// [`agents`], and the agents whose sessions could not be read.
///
/// An agent is reported when one of its directories exists and cannot be
/// listed — usually a permission — or when reading its files panicked, which
/// is a format cctop does not understand yet and worth an issue. Either way the
/// other agents are listed as usual.
///
/// ponytail: a single file that does not parse is skipped, not reported. Agents
/// write transcripts while they are read, so a half-written last line is the
/// normal case and not an error; a format change shows as sessions missing
/// rather than as an error here.
pub fn scan() -> Scan {
    let (mut found, mut errors) = discovered();
    found.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    let mut collector = Collector::new();
    let processes = collector.collect(&found);
    let attributions = collector.attributions();
    let agents = found
        .into_iter()
        .map(|s| {
            let key = s.key();
            let pids = roots_of(&key, processes.get(&key), attributions);
            let state = (!pids.is_empty()).then(|| session::state::live_state(&s).0);
            Agent {
                provider: s.provider,
                pid: pids.first().copied(),
                processes: pids.into_iter().map(AgentProcess::of).collect(),
                // Cursor's is a project slug when its database says nothing
                // better, which is not a path.
                cwd: Some(PathBuf::from(&s.label_source)).filter(|p| p.is_absolute()),
                title: s.title.filter(|t| !t.is_empty()),
                state,
                last_active: s.last_active,
                session_id: s.session_id,
            }
        })
        .collect();
    errors.sort_by_key(|e| e.provider.as_str());
    Scan { agents, errors }
}

/// The separate process trees running the session `key`: the root cctop shows,
/// then every other process attributed to the same session, by pid.
///
/// The collector keeps one root per session — cctop draws one row, with one
/// process tree under it — and the lowest pid wins. A second `claude --resume`
/// of the same id is attributed to the same session and then dropped there,
/// which is right for a row and wrong for a caller that wants to know where the
/// session is open. The attributions still have it, so it is recovered from
/// them; a process inside the root's own tree is the agent's child, not a
/// second copy, and stays out.
fn roots_of(key: &str, info: Option<&ProcInfo>, attributions: &[Attribution]) -> Vec<u32> {
    let Some(info) = info else {
        return Vec::new();
    };
    let Some(root) = info
        .process_list
        .iter()
        .find_map(|e| (e.is_root && !e.ghost).then_some(e.pid))
    else {
        return Vec::new();
    };
    let mut others: Vec<u32> = attributions
        .iter()
        .filter(|a| a.key == key && !info.process_list.iter().any(|e| e.pid == a.pid))
        .map(|a| a.pid)
        .collect();
    others.sort_unstable();
    others.dedup();
    std::iter::once(root).chain(others).collect()
}

/// Every agent's sessions, each stamped with whose home it was read from, and
/// the agents that could not be read.
///
/// ponytail: cctop-core's walk also folds the copies a session leaves in two
/// homes into one row and adds rows for agents with no transcript yet; this
/// does neither. Move that fold down when a caller sees the duplicate.
fn discovered() -> (Vec<Discovered>, Vec<ScanError>) {
    type Lister = fn() -> Vec<Discovered>;
    let listers: [(Provider, Lister); 10] = [
        (Provider::Claude, session::claude::list),
        (Provider::Codex, session::codex::list),
        (Provider::Cursor, session::cursor::list),
        (Provider::Devin, session::devin::list),
        (Provider::Gemini, session::gemini::list),
        (Provider::OpenCode, session::opencode::list),
        (Provider::Pi, session::pi::list),
        (Provider::Vibe, session::vibe::list),
        (Provider::Windsurf, session::windsurf::list),
        (Provider::Antigravity, session::antigravity::list),
    ];
    let watched = watch::watch_paths();
    let mut all = Vec::new();
    let mut errors = Vec::new();
    for (provider, list) in listers {
        // The listers skip what they cannot read, which is right for a file
        // being written and silent for a directory that will never open. That
        // second kind is checked here, once per directory, from the same list
        // a caller watches.
        errors.extend(
            watched
                .iter()
                .filter(|w| w.provider == provider)
                .filter_map(|w| unreadable(provider, &w.path)),
        );
        // A panic is a lister meeting a format it was not written for. It is
        // one agent's problem, so it costs that agent's sessions and no more.
        match catch_unwind(AssertUnwindSafe(list)) {
            Ok(found) => all.extend(found),
            Err(panic) => errors.push(ScanError {
                provider,
                path: None,
                message: format!("reading its sessions panicked: {}", panic_text(&panic)),
            }),
        }
    }
    for s in &mut all {
        if let Some(file) = &s.data_file {
            s.owner = paths::owner_of(file).map(str::to_string);
        }
    }
    (all, errors)
}

/// A directory that exists and cannot be listed. One that does not exist is
/// an agent that is not installed, which is not an error.
fn unreadable(provider: Provider, dir: &std::path::Path) -> Option<ScanError> {
    match std::fs::read_dir(dir) {
        Ok(_) => None,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        // The parent of a database file rather than a directory of sessions;
        // not something this check is about.
        Err(e) if e.kind() == std::io::ErrorKind::NotADirectory => None,
        Err(e) => Some(ScanError {
            provider,
            path: Some(dir.to_path_buf()),
            message: e.to_string(),
        }),
    }
}

fn panic_text(panic: &Box<dyn std::any::Any + Send>) -> &str {
    panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("no message")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcEntry;

    fn entry(pid: u32, is_root: bool) -> ProcEntry {
        ProcEntry {
            pid,
            cpu: 0.0,
            memory: 0,
            args: String::new(),
            is_root,
            ghost: false,
        }
    }

    fn attributed(pid: u32, key: &str) -> Attribution {
        Attribution {
            pid,
            key: key.to_string(),
            argv: String::new(),
            rule: "test",
            matched: String::new(),
        }
    }

    /// Regression for #430: `claude --resume <id>` in two terminals is one
    /// session in two processes. The collector keeps the lower pid as the
    /// row's root; the other one used to vanish from the listing, so a caller
    /// watching that session saw one terminal and never the second.
    #[test]
    fn a_session_open_twice_lists_both_processes_root_first() {
        let info = ProcInfo {
            process_list: vec![entry(200, true), entry(201, false)],
            ..Default::default()
        };
        let attributions = [
            attributed(300, "claude:s"),
            attributed(200, "claude:s"),
            attributed(250, "claude:s"),
            attributed(400, "claude:other"),
        ];
        assert_eq!(
            roots_of("claude:s", Some(&info), &attributions),
            [200, 250, 300]
        );
    }

    /// An agent's own child that matched as an agent too — a wrapper and the
    /// binary it starts — is inside the root's tree, so it is the same copy,
    /// not a second place the session is open.
    #[test]
    fn a_process_inside_the_roots_tree_is_not_a_second_copy() {
        let info = ProcInfo {
            process_list: vec![entry(200, true), entry(201, false)],
            ..Default::default()
        };
        let attributions = [attributed(200, "claude:s"), attributed(201, "claude:s")];
        assert_eq!(roots_of("claude:s", Some(&info), &attributions), [200]);
    }

    /// A session nothing runs has no processes, whatever was once attributed.
    #[test]
    fn a_session_with_no_live_root_has_no_processes() {
        assert!(roots_of("claude:s", None, &[attributed(1, "claude:s")]).is_empty());
        let mut ghost = entry(200, true);
        ghost.ghost = true;
        let info = ProcInfo {
            process_list: vec![ghost],
            ..Default::default()
        };
        assert!(roots_of("claude:s", Some(&info), &[]).is_empty());
    }

    /// A missing directory is an agent that is not installed; a directory that
    /// will not open is an error, and names the directory.
    #[test]
    fn only_a_directory_that_exists_and_will_not_open_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unreadable(Provider::Codex, dir.path()), None);
        assert_eq!(
            unreadable(Provider::Codex, &dir.path().join("not-installed")),
            None
        );
        let file = dir.path().join("state.vscdb");
        std::fs::write(&file, b"").unwrap();
        assert_eq!(unreadable(Provider::Codex, &file), None);

        // Root reads a mode-000 directory anyway, so there is nothing to
        // observe there.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let err = unreadable(Provider::Codex, &locked);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
        let err = err.expect("a mode-000 directory is an error");
        assert_eq!(err.path.as_deref(), Some(locked.as_path()));
    }

    #[test]
    fn a_panic_message_is_kept_either_way_it_was_raised() {
        let text = |p: Box<dyn std::any::Any + Send>| panic_text(&p).to_string();
        assert_eq!(text(Box::new("static")), "static");
        assert_eq!(text(Box::new(String::from("formatted"))), "formatted");
        assert_eq!(text(Box::new(7)), "no message");
    }
}
