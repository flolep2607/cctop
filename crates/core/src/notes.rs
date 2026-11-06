//! What you wrote down about a session: lines of your own, kept beside it.
//!
//! A session's transcript is the agent's record; this is the person's — "ask
//! Sam about expiry", "the flaky test is not ours" — the things worth knowing
//! the next time you look at that tab and nowhere else to put them. One list of
//! lines per session key, in one file under cctop's data directory, so they
//! outlive cctop, the tab, and the agent.
//!
//! Private to the user (written `0600`): a note is whatever someone typed, and
//! nothing about it says it is safe for others on the machine to read.
//!
//! ponytail: notes are this machine's. A session read from another machine is
//! noted here, about that session, and the other machine's cctop never sees
//! it — the shared state between machines is the session list, not a notebook.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The notes of every session, as loaded.
///
/// Several cctops on one machine share the file, so each change is made to
/// the file as it is *then* — locked, read again, changed, written — never by
/// writing out this copy, which would drop whatever another cctop added since
/// it was read. [`Notes::refresh`] picks those up for the screen.
#[derive(Debug, Default)]
pub struct Notes {
    by_session: BTreeMap<String, Vec<String>>,
    /// Where they are saved; `None` keeps them in memory only, as under test.
    path: Option<PathBuf>,
    /// The file's modification time when last read, to know when another
    /// cctop has written it.
    stamp: u64,
}

/// The longest note kept. A note is a line to glance at beside a terminal,
/// and a paste of a log into one is a mistake best stopped at the door.
pub const NOTE_MAX: usize = 500;

impl Notes {
    /// This user's notes, from disk. A missing or unreadable file is no notes
    /// yet, not a failure: the first one written creates it.
    pub fn load() -> Notes {
        if crate::under_test() {
            return Notes::default();
        }
        let path = crate::config::data_base().join("cctop").join("notes.json");
        Notes::at(path)
    }

    /// The notes saved at `path`.
    fn at(path: PathBuf) -> Notes {
        let mut notes = Notes {
            path: Some(path),
            ..Notes::default()
        };
        notes.reread();
        notes
    }

    /// Read the file again if another cctop has written it since.
    pub fn refresh(&mut self) {
        let Some(path) = &self.path else { return };
        if crate::config::file_mtime_ms(path) != self.stamp {
            self.reread();
        }
    }

    fn reread(&mut self) {
        let Some(path) = &self.path else { return };
        self.stamp = crate::config::file_mtime_ms(path);
        self.by_session = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
    }

    /// The notes on `key`, oldest first.
    pub fn of(&self, key: &str) -> &[String] {
        self.by_session.get(key).map_or(&[], Vec::as_slice)
    }

    /// Replace note `at` on `key`, or add one when `at` is past the end; an
    /// empty `text` removes the note instead. Saved at once — a note is typed
    /// rarely, and one lost to a crash is the one you meant to remember.
    pub fn set(&mut self, key: &str, at: usize, text: &str) -> std::io::Result<()> {
        // Held across the read and the write, so two cctops saving at once
        // take turns rather than each writing out what it read before the
        // other's note.
        let _lock = match &self.path {
            Some(path) => {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let lock = path.with_extension("json.lock");
                Some(crate::util::lock_within(
                    &lock,
                    std::time::Duration::from_secs(2),
                )?)
            }
            None => None,
        };
        self.reread();
        let text: String = text.trim().chars().take(NOTE_MAX).collect();
        let list = self.by_session.entry(key.to_string()).or_default();
        match (at < list.len(), text.is_empty()) {
            (true, true) => {
                list.remove(at);
            }
            (true, false) => list[at] = text,
            (false, false) => list.push(text),
            (false, true) => {}
        }
        if list.is_empty() {
            self.by_session.remove(key);
        }
        self.save()
    }

    fn save(&mut self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let bytes = serde_json::to_vec_pretty(&self.by_session).map_err(std::io::Error::other)?;
        crate::util::write_atomic(path, &bytes, true)?;
        // Our own write is not news to read back.
        self.stamp = crate::config::file_mtime_ms(path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_added_edited_and_removed() {
        let mut notes = Notes::default();
        notes.set("claude:a", 0, "  first  ").unwrap();
        notes.set("claude:a", 9, "second").unwrap();
        assert_eq!(notes.of("claude:a"), ["first", "second"]);
        notes.set("claude:a", 0, "changed").unwrap();
        notes.set("claude:a", 1, "").unwrap();
        assert_eq!(notes.of("claude:a"), ["changed"]);
        // An empty line past the end adds nothing.
        notes.set("claude:a", 5, " ").unwrap();
        notes.set("claude:a", 0, "").unwrap();
        assert!(notes.of("claude:a").is_empty());
        assert!(notes.by_session.is_empty());
    }

    /// Two cctops with the file open each add a note; neither loses the
    /// other's, though each loaded the file before the other wrote.
    #[test]
    fn two_cctops_never_write_over_each_others_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        let mut a = Notes::at(path.clone());
        let mut b = Notes::at(path.clone());
        a.set("claude:a", 0, "from a").unwrap();
        b.set("codex:b", 0, "from b").unwrap();
        let back = Notes::at(path);
        assert_eq!(back.of("claude:a"), ["from a"]);
        assert_eq!(back.of("codex:b"), ["from b"]);
        // And the first sees the second's once it looks again.
        std::thread::sleep(std::time::Duration::from_millis(5));
        a.refresh();
        assert_eq!(a.of("codex:b"), ["from b"]);
    }

    #[test]
    fn a_note_is_saved_privately_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cctop").join("notes.json");
        let mut notes = Notes {
            path: Some(path.clone()),
            ..Notes::default()
        };
        notes.set("codex:b", 0, "ask Sam").unwrap();
        let back: BTreeMap<String, Vec<String>> =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back["codex:b"], ["ask Sam"]);
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
