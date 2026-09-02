//! Whether the corpus changed at all, answered with `stat` calls alone.
//!
//! A walk is expensive in proportion to how many sessions have ever existed,
//! and almost none of them can have changed since the last one: on a machine
//! with a hundred transcripts, discovery is two thirds of a warm run and every
//! millisecond of it is spent re-learning what the previous run already knew.
//! What is missing is not a better per-session cache — [`crate::cache`] already
//! keys those on the transcript's length and mtime — but a way to skip the
//! whole run before it starts.
//!
//! So the corpus gets one hash. A stat-only pass over every provider root
//! collects `(path, device, inode, mtime, size)` per file and folds them into a
//! single number. Equal hash means no transcript was written, created, renamed
//! or deleted since the last walk, and the rows from that walk are still exactly
//! right.
//!
//! The pass can never cost more than the walk it replaces: discovery already
//! reads these same directories and then *opens* what it finds. The one place
//! that is not automatically true is Gemini, whose chat root sits beside a
//! `tool-outputs/` directory holding a file per tool call — which its own
//! discovery deliberately does not descend into, and neither does this.

use std::path::{Path, PathBuf};

/// How long the newest write in the corpus stays a reason to wait.
///
/// A streaming assistant turn rewrites its transcript continuously, so during a
/// live turn the fingerprint differs on *every* poll and a cache keyed on it
/// would never once be hit — it would cost a stat pass per refresh and return
/// nothing, which is worse than not having it. Deferring while the newest write
/// is younger than this coalesces a burst into one recompute, and bounds the
/// staleness it buys to the window rather than leaving it open-ended.
const DEFAULT_SETTLE_MS: i64 = 2_000;

/// The longest a deferral may hold the previous rows.
///
/// A settle window that only ever asks "was there a write just now" can be held
/// open forever by a corpus that is never quiet: one agent writing every few
/// hundred milliseconds for an hour defers every poll for an hour, and a session
/// started in the meantime never appears. Two clocks are involved as well —
/// mtimes come from the filesystem's — so a file stamped in the future defers
/// unconditionally until real time catches up with it.
///
/// Neither is hypothetical enough to leave to chance in a live monitor, so a
/// deferral expires on its own: past this age the next call recomputes whatever
/// the corpus is doing. It is a multiple of the window rather than a constant
/// so that raising the window raises the ceiling with it.
const DEFER_CEILING: u32 = 5;

/// `$CCTOP_SETTLE_MS` overrides [`DEFAULT_SETTLE_MS`]. `0` disables deferring
/// altogether, which is what a caller wanting every change reflected on the
/// very next refresh — at the cost of recomputing throughout a live turn —
/// would set.
///
/// Read once, like every other environment knob cctop has: the window is a
/// property of how the machine is being watched, not something to re-read per
/// poll.
static SETTLE_MS: std::sync::LazyLock<i64> = std::sync::LazyLock::new(|| {
    std::env::var("CCTOP_SETTLE_MS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(DEFAULT_SETTLE_MS)
});

/// One stat-only reading of the corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corpus {
    /// Every discovered file's identity, folded into one number.
    pub hash: u64,
    /// The newest mtime seen, in epoch milliseconds, which is what the settle
    /// window is measured against.
    pub newest_mtime_ms: u64,
    /// How many files were stat-ed, for `cctop --trace`.
    pub files: usize,
}

/// What a caller should do with the reading it just took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing on disk moved: the previous rows are still the right rows.
    Reuse,
    /// Something moved, but too recently to be finished moving. Serve the
    /// previous rows and record nothing — see [`Snapshot::decide`].
    Defer,
    /// Walk, and hand the result back to [`Snapshot::recorded`].
    Recompute,
}

/// The last accepted reading, and when it was accepted.
///
/// The defer logic lives here, at the layer that owns the whole-corpus reading,
/// and deliberately not one level down in the per-file reconciliation that
/// [`crate::cache`] does. The design this is adapted from tried it the other way
/// and recorded the outcome: a per-file defer signal is incomplete — a file can
/// only say whether *it* is settling, never whether the run as a whole is — so
/// some files were deferred while others in the same run were not, and the
/// snapshot that came out was a mixture of two moments that had never both been
/// true. Here there is one decision per run and it is taken before anything is
/// read.
#[derive(Debug, Default)]
pub struct Snapshot {
    accepted: Option<u64>,
    accepted_at_ms: i64,
}

impl Snapshot {
    /// Decide what to do about `corpus`, given what was last accepted.
    ///
    /// A mismatch inside the settle window returns [`Verdict::Defer`] and
    /// changes nothing, which is the whole point: the deferred hash is *not*
    /// recorded, so the next call re-reads the corpus and the recompute happens
    /// as soon as the writes stop rather than being lost.
    pub fn decide(&mut self, corpus: &Corpus, now_ms: i64) -> Verdict {
        // With nothing to fall back on there is nothing to serve, so a first
        // run always walks however busy the corpus is.
        let Some(accepted) = self.accepted else {
            return Verdict::Recompute;
        };
        if accepted == corpus.hash {
            return Verdict::Reuse;
        }
        let settle = *SETTLE_MS;
        let since_write = now_ms - corpus.newest_mtime_ms as i64;
        let held_for = now_ms - self.accepted_at_ms;
        if since_write < settle && held_for < settle * i64::from(DEFER_CEILING) {
            Verdict::Defer
        } else {
            Verdict::Recompute
        }
    }

    /// Accept `corpus` as what the rows now in hand were derived from.
    ///
    /// Called with the reading taken *before* the walk, not one taken after it:
    /// a transcript written while the walk was running is then a mismatch on the
    /// next call and earns a recompute, where recording the later reading would
    /// claim rows that were built without it.
    pub fn recorded(&mut self, corpus: &Corpus, now_ms: i64) {
        self.accepted = Some(corpus.hash);
        self.accepted_at_ms = now_ms;
    }
}

/// Stat every file under every provider root and fold the result into one hash.
///
/// Nothing is opened and nothing is parsed, so this is a syscall per file and no
/// allocation beyond the paths.
///
/// ponytail: cctop's own `cost-cache.json` is deliberately not part of this,
/// and cannot become part of it while the cache lives outside the provider
/// roots. That file is rewritten *after* a walk finishes, so including it would
/// make every walk invalidate the fingerprint it had just established — the key
/// would chase its own tail and never once match.
pub fn compute_corpus_fingerprint() -> Corpus {
    let _span = crate::trace::span("fingerprint");
    let mut files: Vec<(String, u64, u64, u64, u64)> = Vec::new();
    for root in roots() {
        collect(&root, &mut files);
    }
    // Readdir order is the filesystem's business and changes under renames, so
    // the fold is over a sorted list rather than over the walk.
    files.sort_unstable();

    let mut hash = FNV_OFFSET;
    let mut newest = 0u64;
    for (path, dev, ino, mtime, size) in &files {
        write(&mut hash, path.as_bytes());
        for n in [dev, ino, mtime, size] {
            write(&mut hash, &n.to_le_bytes());
        }
        newest = newest.max(*mtime);
    }
    crate::trace::fact("corpus files", files.len().to_string());
    Corpus {
        hash,
        newest_mtime_ms: newest,
        files: files.len(),
    }
}

/// Every provider's session root that is present on this machine.
///
/// This is the same list [`crate::watch`] watches, plus the two providers it
/// does not — Gemini and Windsurf. A root missing here is not a missed
/// notification, as it is there, but a session whose changes the fingerprint
/// cannot see: the table would keep serving rows from before it was written.
/// So the list errs wide, and so does the walk below.
fn roots() -> Vec<PathBuf> {
    let mut roots = crate::config::claude_projects_roots();
    roots.extend(crate::config::codex_sessions_roots());
    roots.extend(crate::config::cursor_projects_roots());
    roots.extend(crate::config::pi_sessions_roots());
    roots.extend(crate::config::opencode_data_roots());
    roots.extend(crate::config::gemini_chats_roots());
    roots.extend(crate::config::windsurf_workspace_roots());
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

/// Directory names never descended into.
///
/// Gemini writes one file per tool call into `tool-outputs/`, beside the chats
/// directory and far larger than it, which its own discovery skips for the same
/// reason. Every other name is walked: including a file no parser reads costs at
/// worst a recompute nobody needed, while omitting one costs a stale table, and
/// only one of those is a bug.
const SKIP_DIRS: &[&str] = &["tool-outputs"];

/// Recursively stat everything under `dir`.
///
/// Symlinked directories are followed as files rather than descended into,
/// because `read_dir` would loop on a cycle and a provider has no reason to
/// link one session tree into another.
fn collect(dir: &Path, out: &mut Vec<(String, u64, u64, u64, u64)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            let skipped = path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| SKIP_DIRS.contains(&n));
            if !skipped {
                collect(&path, out);
            }
            continue;
        }
        // `metadata` on the entry rather than the path: on the platforms that
        // carry it in the directory read, this costs nothing.
        let Ok(meta) = entry.metadata() else {
            // A file that vanished between the read and the stat is a change in
            // itself, and hashing nothing for it says so on the next pass.
            continue;
        };
        let (dev, ino) = identity(&meta);
        out.push((
            path.to_string_lossy().replace('\\', "/"),
            dev,
            ino,
            crate::config::file_mtime_ms(&path),
            meta.len(),
        ));
    }
}

/// The filesystem's own name for a file, where it has one.
///
/// `(device, inode)` distinguishes two files that trade paths in the same
/// millisecond at the same size, which a path-and-stat pair cannot. Windows has
/// no such pair through `std`, and the path is already hashed alongside this, so
/// there it contributes nothing and the size and mtime carry the reading — a
/// weaker key on that platform, deliberately, rather than a `cfg` that changes
/// what the caller has to do.
fn identity(meta: &std::fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        (0, 0)
    }
}

// FNV-1a, matching `build.rs`: a hash of a few thousand short byte strings
// wants nothing more, and a dependency for it would be paid for by every build.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn write(hash: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *hash ^= u64::from(*b);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
    // Length-delimited, so ("ab", "c") and ("a", "bc") do not collide.
    *hash ^= bytes.len() as u64;
    *hash = hash.wrapping_mul(FNV_PRIME);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fold that [`compute_corpus_fingerprint`] applies, over a directory
    /// named outright. The roots come from the environment, which a test cannot
    /// have to itself, so the tests exercise the walk and the fold here and the
    /// root list is left to the one place that lists it.
    fn fingerprint_of(dir: &Path) -> Corpus {
        let mut files = Vec::new();
        collect(dir, &mut files);
        files.sort_unstable();
        let mut hash = FNV_OFFSET;
        let mut newest = 0;
        for (path, dev, ino, mtime, size) in &files {
            write(&mut hash, path.as_bytes());
            for n in [dev, ino, mtime, size] {
                write(&mut hash, &n.to_le_bytes());
            }
            newest = newest.max(*mtime);
        }
        Corpus {
            hash,
            newest_mtime_ms: newest,
            files: files.len(),
        }
    }

    /// The whole premise: a corpus nobody wrote to hashes the same twice. If it
    /// did not, every run would recompute and the pass would be pure cost.
    #[test]
    fn an_untouched_corpus_hashes_the_same() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("project")).unwrap();
        std::fs::write(dir.path().join("project").join("a.jsonl"), b"{}\n").unwrap();
        std::fs::write(dir.path().join("b.jsonl"), b"{}\n").unwrap();

        let first = fingerprint_of(dir.path());
        assert_eq!(first.files, 2);
        assert_eq!(first, fingerprint_of(dir.path()));
    }

    /// And the other half: a transcript that grew has to be visible as a
    /// different hash, or the table would keep serving rows from before it.
    #[test]
    fn a_changed_file_changes_the_hash() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.jsonl");
        std::fs::write(&file, b"{}\n").unwrap();
        let before = fingerprint_of(dir.path());

        std::fs::write(&file, b"{}\n{}\n").unwrap();
        assert_ne!(
            before.hash,
            fingerprint_of(dir.path()).hash,
            "an appended transcript must invalidate the fingerprint"
        );

        // A new file counts as much as a changed one — a session that did not
        // exist is exactly what a walk is for.
        let grown = fingerprint_of(dir.path());
        std::fs::write(dir.path().join("b.jsonl"), b"{}\n").unwrap();
        assert_ne!(grown.hash, fingerprint_of(dir.path()).hash);

        // As does one that went away.
        let two = fingerprint_of(dir.path());
        std::fs::remove_file(&file).unwrap();
        assert_ne!(two.hash, fingerprint_of(dir.path()).hash);
    }

    /// The directory a provider fills with tool output is not a transcript, and
    /// descending into it would make a busy turn cost more to notice than to
    /// re-read.
    #[test]
    fn the_skipped_directories_are_not_walked() {
        let dir = tempfile::tempdir().unwrap();
        let noise = dir.path().join("tool-outputs");
        std::fs::create_dir(&noise).unwrap();
        std::fs::write(noise.join("call-1.json"), b"{}\n").unwrap();
        assert_eq!(fingerprint_of(dir.path()).files, 0);
    }

    /// A reading with nothing behind it but the two fields the settle window
    /// consults, so the decisions can be tested without a filesystem.
    fn corpus(hash: u64, newest_mtime_ms: i64) -> Corpus {
        Corpus {
            hash,
            newest_mtime_ms: newest_mtime_ms.max(0) as u64,
            files: 1,
        }
    }

    /// A first run has nothing to reuse, an unchanged corpus reuses, and a
    /// changed one that has been quiet for longer than the window recomputes.
    #[test]
    fn a_quiet_corpus_reuses_and_a_settled_change_recomputes() {
        let mut snap = Snapshot::default();
        let now = 1_000_000;

        assert_eq!(
            snap.decide(&corpus(1, now - 10_000), now),
            Verdict::Recompute
        );
        snap.recorded(&corpus(1, now - 10_000), now);

        assert_eq!(snap.decide(&corpus(1, now - 10_000), now), Verdict::Reuse);
        assert_eq!(
            snap.decide(&corpus(2, now - 10_000), now),
            Verdict::Recompute,
            "a change nobody has touched for ten seconds is finished changing"
        );
    }

    /// The reason the window exists: a transcript written a moment ago is being
    /// written *now*, so the run waits rather than recomputing per keystroke —
    /// and, crucially, records nothing while it waits, so the recompute still
    /// happens once the writes stop.
    #[test]
    fn a_streaming_write_defers_without_being_recorded() {
        let mut snap = Snapshot::default();
        let now = 1_000_000;
        snap.recorded(&corpus(1, now - 10_000), now);

        assert_eq!(
            snap.decide(&corpus(2, now - 50), now),
            Verdict::Defer,
            "a write from 50ms ago is inside the default window"
        );
        // The deferred hash must not have been accepted, or the corpus would
        // have been declared settled at a value nothing was ever built from.
        assert_eq!(
            snap.decide(&corpus(2, now - 5_000), now + 5_000),
            Verdict::Recompute,
            "once the window passes, the same change must recompute"
        );
    }

    /// A corpus that is never quiet must not defer forever. Without the ceiling
    /// one agent writing steadily would hide every session started beside it.
    #[test]
    fn a_never_quiet_corpus_stops_deferring() {
        let mut snap = Snapshot::default();
        let now = 1_000_000;
        snap.recorded(&corpus(1, now), now);

        // Still writing, every time it is asked.
        assert_eq!(
            snap.decide(&corpus(2, now + 999), now + 1_000),
            Verdict::Defer
        );
        let past_ceiling = now + i64::from(DEFER_CEILING) * DEFAULT_SETTLE_MS;
        assert_eq!(
            snap.decide(&corpus(2, past_ceiling), past_ceiling),
            Verdict::Recompute,
            "a deferral older than the ceiling must expire on its own"
        );
    }

    /// An mtime from a clock ahead of this one is in the future, which would
    /// otherwise read as "written a moment ago" for as long as the skew lasts.
    #[test]
    fn a_future_mtime_cannot_defer_past_the_ceiling() {
        let mut snap = Snapshot::default();
        let now = 1_000_000;
        snap.recorded(&corpus(1, now), now);
        let far_future = now + 86_400_000;

        assert_eq!(snap.decide(&corpus(2, far_future), now), Verdict::Defer);
        let past_ceiling = now + i64::from(DEFER_CEILING) * DEFAULT_SETTLE_MS;
        assert_eq!(
            snap.decide(&corpus(2, far_future), past_ceiling),
            Verdict::Recompute
        );
    }
}
