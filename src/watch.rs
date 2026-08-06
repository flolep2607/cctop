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
//! Watch failures are not errors: a provider directory may not exist, and on
//! Linux a recursive watch consumes one inotify watch per directory, which a
//! large enough tree can exhaust. Either way the periodic walk still runs, so the
//! only cost is that a new session takes until the next one to appear.

use notify::{EventKind, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct Watch {
    /// Dropping this stops the watch, so it is held for its lifetime alone.
    _watcher: notify::RecommendedWatcher,
    structural: Arc<AtomicBool>,
}

impl Watch {
    /// Begin watching every provider root that exists.
    ///
    /// `None` when no watcher could be created at all, which leaves the caller on
    /// its periodic walk.
    pub fn start() -> Option<Self> {
        let structural = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&structural);
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            // Appends are deliberately ignored; see the module note.
            if let Ok(event) = res
                && matches!(event.kind, EventKind::Create(_) | EventKind::Remove(_))
            {
                flag.store(true, Ordering::Relaxed);
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
        })
    }

    /// Whether a session file appeared or disappeared since the last call.
    ///
    /// Reading clears the flag: each structural change earns exactly one walk.
    pub fn took_structural_change(&self) -> bool {
        self.structural.swap(false, Ordering::Relaxed)
    }
}

/// Every provider's session root that is present on this machine.
fn roots() -> Vec<PathBuf> {
    let mut roots = vec![
        crate::config::CLAUDE_PROJECTS_ROOT.clone(),
        crate::config::CODEX_SESSIONS_ROOT.clone(),
        crate::config::CURSOR_PROJECTS_ROOT.clone(),
        crate::config::PI_SESSIONS_ROOT.clone(),
        crate::config::OPENCODE_DATA_DIR.clone(),
    ];
    roots.extend(crate::config::CLAUDE_MAC_COWORK_ROOT.clone());
    roots.extend(crate::config::CLAUDE_MAC_CODE_ROOT.clone());
    roots.retain(|r| r.is_dir());
    roots
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A create under a watched tree has to reach the flag, and reading it has to
    /// disarm it — otherwise every tick would trigger a full walk forever.
    #[test]
    fn a_created_file_arms_the_flag_once() {
        let dir = tempfile::tempdir().unwrap();
        let structural = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&structural);
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res
                && matches!(event.kind, EventKind::Create(_) | EventKind::Remove(_))
            {
                flag.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        watcher.watch(dir.path(), RecursiveMode::Recursive).unwrap();
        let watch = Watch {
            _watcher: watcher,
            structural,
        };

        std::fs::write(dir.path().join("new-session.jsonl"), b"{}\n").unwrap();
        let armed = (0..50).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(100));
            watch.took_structural_change()
        });

        assert!(armed, "a created file did not reach the flag");
        assert!(
            !watch.took_structural_change(),
            "reading the flag must disarm it"
        );
    }
}
