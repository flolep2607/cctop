//! The directories the agents write their sessions to, for a caller that would
//! rather be told about a change than list on a timer.

use crate::{Provider, paths};
use std::path::PathBuf;

/// A directory one agent writes its sessions under.
///
/// inotify watches one directory, not a tree, so this says how far down the
/// sessions are: watch [`path`](Self::path) and each directory below it down
/// to [`depth`](Self::depth) levels, and every write to a session lands as an
/// event on one of those watches.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WatchPath {
    /// Whose sessions are under it.
    pub provider: Provider,
    /// The directory. It may not exist yet: an agent creates it on its first
    /// session, so watch the nearest parent that does until it appears.
    pub path: PathBuf,
    /// How many levels of directories below `path` to watch too: `0` is
    /// `path` alone, `2` is its children and grandchildren. `None` is the
    /// whole tree.
    ///
    /// Not always `None` because some trees are mostly something else — a
    /// Gemini project directory holds a file per tool call beside its few
    /// transcripts, and a Cursor project can hold a `node_modules` — and a
    /// watch per directory there would spend the inotify limit on nothing.
    pub depth: Option<u8>,
}

/// Every directory an agent writes its sessions under, this user's and, when
/// cctop reads them, other users' too.
///
/// A change in any of them is a reason to call [`agents`](crate::agents) again.
/// Two things a session does are not file writes, so no watch sees them: a
/// process starting with no transcript yet, and a process exiting. Pair the
/// watches with a slow timer — or with a pidfd on each
/// [`pid`](crate::Agent::pid) — to see an agent stop.
///
/// The list is the same until an agent's configuration changes (a new
/// `CLAUDE_CONFIG_DIR` profile, a home appearing), with one exception: Cursor
/// is listed per project, so call this again when a directory appears at the
/// top of a Cursor path with `depth: Some(1)`.
pub fn watch_paths() -> Vec<WatchPath> {
    let mut out = Vec::new();
    let mut add = |provider, dirs: Vec<PathBuf>, depth| {
        for path in dirs {
            if !out
                .iter()
                .any(|w: &WatchPath| w.provider == provider && w.path == path)
            {
                out.push(WatchPath {
                    provider,
                    path,
                    depth,
                });
            }
        }
    };
    // `<root>/<project>/<id>.jsonl`.
    add(Provider::Claude, paths::claude_projects_roots(), Some(1));
    // `<root>/<yyyy>/<mm>/<dd>/rollout-*.jsonl`, read as a tree.
    add(Provider::Codex, paths::codex_sessions_roots(), None);

    // `<root>/<slug>/agent-transcripts/…`, beside a `canvases/` that is not
    // worth a watch: the root to see a new project, each project's transcripts
    // as a tree. The chat names are in the global database.
    let cursor_roots = paths::cursor_projects_roots();
    let transcripts = cursor_roots
        .iter()
        .flat_map(|root| {
            paths::list_dir(root)
                .into_iter()
                .map(move |slug| root.join(slug).join("agent-transcripts"))
        })
        .filter(|dir| paths::dir_exists(dir))
        .collect();
    add(Provider::Cursor, cursor_roots, Some(1));
    add(Provider::Cursor, transcripts, None);
    add(
        Provider::Cursor,
        parents(paths::cursor_global_dbs()),
        Some(0),
    );

    // `sessions.db` beside `transcripts/<id>.json`.
    let devin = paths::devin_cli_dirs();
    let mut devin_transcripts: Vec<PathBuf> = devin.iter().map(|d| d.join("transcripts")).collect();
    devin_transcripts.push(paths::DEVIN_TRANSCRIPTS_DIR.clone());
    add(Provider::Devin, devin, Some(0));
    add(
        Provider::Devin,
        parents(vec![paths::DEVIN_SESSIONS_DB.clone()]),
        Some(0),
    );
    add(Provider::Devin, devin_transcripts, Some(0));

    // `<root>/<project>/chats/session-*.json`.
    add(Provider::Gemini, paths::gemini_chats_roots(), Some(2));
    // `opencode*.db`, and its `-wal`, in the root.
    add(Provider::OpenCode, paths::opencode_data_roots(), Some(0));
    add(Provider::Pi, paths::pi_sessions_roots(), None);
    // `<root>/session_*/` and `<root>/unified/<id>/`.
    add(Provider::Vibe, paths::vibe_sessions_roots(), Some(2));
    // `<root>/<workspace>/state.vscdb`.
    add(
        Provider::Windsurf,
        paths::windsurf_workspace_roots(),
        Some(1),
    );
    // The summaries database in the directory, and
    // `brain/<id>/.system_generated/logs/transcript.jsonl` under it.
    add(Provider::Antigravity, paths::antigravity_dirs(), Some(0));
    add(Provider::Antigravity, paths::antigravity_brains(), Some(3));
    out
}

/// The directory a database file is in: SQLite writes its `-wal` and
/// `-journal` beside it, and a writer may replace the file, so the directory
/// is what sees every change.
fn parents(files: Vec<PathBuf>) -> Vec<PathBuf> {
    files
        .into_iter()
        .filter_map(|f| f.parent().map(PathBuf::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every agent has somewhere to watch, even with none of them installed:
    /// the directory a first session would create is listed before it exists.
    #[test]
    fn every_agent_has_a_path_to_watch() {
        let watched = watch_paths();
        for p in Provider::ALL {
            assert!(watched.iter().any(|w| w.provider == p), "{p:?}");
        }
        for w in &watched {
            assert!(w.path.is_absolute(), "{w:?}");
        }
    }

    #[test]
    fn no_directory_is_listed_twice_for_one_agent() {
        let watched = watch_paths();
        for (i, a) in watched.iter().enumerate() {
            assert!(
                !watched[i + 1..]
                    .iter()
                    .any(|b| a.provider == b.provider && a.path == b.path),
                "{a:?}"
            );
        }
    }
}
