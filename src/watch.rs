//! Watch the provider directories so a new session appears without waiting for
//! the next walk.
//!
//! Only *discovery* is event-driven. A transcript being appended to says nothing
//! this program doesn't already know — the fast tick re-reads the running rows
//! anyway, and their size and mtime tell it what changed for the cost of one
//! `stat`. What polling genuinely can't do cheaply is notice a session that did
//! not exist before, because that means walking every provider's directory tree.
//! So the walk becomes a safety net and this becomes the trigger.
//!
//! One walk per create would still be too few. A transcript is created before it
//! carries enough to build a session from — Claude writes the file at startup and
//! the model name only arrives with the first assistant message — so the walk the
//! create triggers finds nothing, and the appends that follow are the very events
//! this module ignores. Creates are therefore remembered until they turn into a
//! session, and the caller re-walks while any are outstanding.
//!
//! Watch failures are not errors: a provider directory may not exist, and on
//! Linux a recursive watch consumes one inotify watch per directory, which a
//! large enough tree can exhaust. Either way the periodic walk still runs, so the
//! only cost is that a new session takes until the next one to appear.

use notify::{EventKind, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a created file stays worth re-walking for.
///
/// A transcript exists before it says anything a session can be built from —
/// Claude writes the file at startup and the first assistant message, which is
/// where the model name comes from, lands whenever the model answers. Until then
/// a walk finds nothing, so the create alone cannot be the last word.
const PENDING_FOR: Duration = Duration::from_secs(120);

/// Cap on remembered creates, so a provider rewriting a directory in bulk cannot
/// grow this without bound.
const MAX_PENDING: usize = 256;

pub struct Watch {
    /// Dropping this stops the watch, so it is held for its lifetime alone.
    _watcher: notify::RecommendedWatcher,
    structural: Arc<AtomicBool>,
    /// Files seen created, with when — dropped once they turn into a session.
    pending: Arc<Mutex<HashMap<PathBuf, Instant>>>,
}

impl Watch {
    /// Begin watching every provider root that exists.
    ///
    /// `None` when no watcher could be created at all, which leaves the caller on
    /// its periodic walk.
    pub fn start() -> Option<Self> {
        let structural = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let flag = Arc::clone(&structural);
        let noted = Arc::clone(&pending);
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            // Appends are deliberately ignored; see the module note.
            if let Ok(event) = res
                && matches!(event.kind, EventKind::Create(_) | EventKind::Remove(_))
            {
                flag.store(true, Ordering::Relaxed);
                note(&noted, &event);
            }
        })
        .ok()?;

        let mut watching = 0usize;
        for root in roots() {
            if watcher.watch(&root, RecursiveMode::Recursive).is_ok() {
                watching += 1;
            }
        }
        // Nothing watched means nothing will ever fire, and claiming otherwise
        // would let the caller stretch its walk interval for no reason.
        (watching > 0).then_some(Watch {
            _watcher: watcher,
            structural,
            pending,
        })
    }

    /// Whether a session file appeared or disappeared since the last call.
    ///
    /// Reading clears the flag: each structural change earns exactly one walk.
    pub fn took_structural_change(&self) -> bool {
        self.structural.swap(false, Ordering::Relaxed)
    }

    /// Whether some created file has still to turn into a session.
    ///
    /// One walk per create is not enough, because a transcript is created empty
    /// and only becomes summarizable once the model has spoken. So creates are
    /// remembered and this stays true until the caller reports the path
    /// discovered, the file goes away, or [`PENDING_FOR`] passes — after which
    /// the file is presumed to be something that will never be a session.
    pub fn awaiting_discovery(&self, discovered: impl Fn(&Path) -> bool) -> bool {
        let Ok(mut pending) = self.pending.lock() else {
            return false;
        };
        pending.retain(|path, at| at.elapsed() < PENDING_FOR && path.exists() && !discovered(path));
        !pending.is_empty()
    }
}

/// Remember created files and forget removed ones.
fn note(pending: &Mutex<HashMap<PathBuf, Instant>>, event: &notify::Event) {
    let Ok(mut pending) = pending.lock() else {
        return;
    };
    for path in &event.paths {
        match event.kind {
            // Directories are containers; what gets summarized is the file that
            // lands inside one, which arrives as its own create.
            EventKind::Create(_) if path.is_file() => {
                if pending.len() < MAX_PENDING {
                    pending.insert(path.clone(), Instant::now());
                }
            }
            _ => {
                pending.remove(path);
            }
        }
    }
}

/// Every provider's session root that is present on this machine.
fn roots() -> Vec<PathBuf> {
    // Every home being scanned, so a session another user starts shows up as
    // promptly as one of this user's does.
    let mut roots = crate::config::claude_projects_roots();
    roots.extend(crate::config::codex_sessions_roots());
    roots.extend(crate::config::cursor_projects_roots());
    roots.extend(crate::config::pi_sessions_roots());
    roots.extend(crate::config::opencode_data_roots());
    roots.extend(crate::config::claude_mac_roots(
        &crate::config::CLAUDE_MAC_COWORK_ROOT,
        "local-agent-mode-sessions",
    ));
    roots.extend(crate::config::claude_mac_roots(
        &crate::config::CLAUDE_MAC_CODE_ROOT,
        "claude-code-sessions",
    ));
    roots.retain(|r| r.is_dir());
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A watch over `dir` alone, built the way [`Watch::start`] builds one.
    fn watch_dir(dir: &Path) -> Watch {
        let structural = Arc::new(AtomicBool::new(false));
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let flag = Arc::clone(&structural);
        let noted = Arc::clone(&pending);
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res
                && matches!(event.kind, EventKind::Create(_) | EventKind::Remove(_))
            {
                flag.store(true, Ordering::Relaxed);
                note(&noted, &event);
            }
        })
        .unwrap();
        watcher.watch(dir, RecursiveMode::Recursive).unwrap();
        Watch {
            _watcher: watcher,
            structural,
            pending,
        }
    }

    /// Poll until `f` holds, giving the watcher thread time to deliver.
    fn eventually(f: impl Fn() -> bool) -> bool {
        (0..50).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            f()
        })
    }

    /// A create under a watched tree has to reach the flag, and reading it has to
    /// disarm it — otherwise every tick would trigger a full walk forever.
    #[test]
    fn a_created_file_arms_the_flag_once() {
        let dir = tempfile::tempdir().unwrap();
        let watch = watch_dir(dir.path());

        std::fs::write(dir.path().join("new-session.jsonl"), b"{}\n").unwrap();
        let armed = eventually(|| watch.took_structural_change());

        assert!(armed, "a created file did not reach the flag");
        assert!(
            !watch.took_structural_change(),
            "reading the flag must disarm it"
        );
    }

    /// The walk a create earns runs while the transcript is still empty, so the
    /// create has to keep asking for walks until the file is a session — and stop
    /// the moment it is, or every tick would walk forever.
    #[test]
    fn a_create_keeps_asking_until_it_is_discovered() {
        let dir = tempfile::tempdir().unwrap();
        // Canonicalised because this is the one test that compares a path the
        // watcher reported against one it built itself. On macOS the temp dir is
        // `/var/folders/…`, a symlink to `/private/var/folders/…`, and FSEvents
        // reports the target — so the two spellings would never match.
        let root = dir.path().canonicalize().unwrap();
        let watch = watch_dir(&root);
        let transcript = root.join("new-session.jsonl");

        std::fs::write(&transcript, b"{}\n").unwrap();
        let waiting = eventually(|| watch.awaiting_discovery(|_| false));

        assert!(waiting, "a created file was not remembered");
        assert!(
            watch.awaiting_discovery(|_| false),
            "an undiscovered create must survive being read"
        );
        assert!(
            !watch.awaiting_discovery(|path| path == transcript),
            "a discovered create must stop asking for walks"
        );
        assert!(
            !watch.awaiting_discovery(|_| false),
            "a discovered create must be forgotten, not re-armed"
        );
    }

    /// A file that vanishes before it ever became a session must not hold the
    /// walk cadence open for the whole pending window.
    #[test]
    fn a_removed_create_stops_asking() {
        let dir = tempfile::tempdir().unwrap();
        let watch = watch_dir(dir.path());
        let transcript = dir.path().join("doomed.jsonl");

        std::fs::write(&transcript, b"{}\n").unwrap();
        assert!(
            eventually(|| watch.awaiting_discovery(|_| false)),
            "a created file was not remembered"
        );

        std::fs::remove_file(&transcript).unwrap();
        assert!(
            !watch.awaiting_discovery(|_| false),
            "a file that no longer exists must not keep asking for walks"
        );
    }
}
