//! Which live sessions are about to overwrite each other.
//!
//! Two agents in one checkout is not a merge conflict — git would at least
//! announce that. It is one of them writing a file the other is still holding
//! in context, and the loser finds out when the work is already gone. cctop is
//! the only thing on the machine that can see both of them, so it is the only
//! thing that can say so while it still helps.
//!
//! The unit of comparison is the **repository root**, not the working
//! directory. A linked worktree carries its own `.git`, so two agents in two
//! worktrees of one repository are editing two sets of files on disk and do not
//! collide, while two agents started from different subdirectories of one
//! checkout do. Comparing directories — equal, or one an ancestor of the other
//! — gets both of those backwards, and the second is exactly the arrangement
//! this repository's own contributors are told to use.
//!
//! Only running sessions are compared. A session that has stopped may well have
//! left uncommitted work behind, but nothing it does from here on can race
//! anyone.

use crate::session::Session;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How close two sessions are to each other's work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Overlap {
    /// Same repository, no file written by both — yet.
    Directory,
    /// Both have written the same file.
    File,
}

/// What one session shares with the other live sessions around it.
#[derive(Debug, Clone)]
pub struct Collision {
    pub level: Overlap,
    /// Keys of the sessions it overlaps with. At [`Overlap::File`] this is only
    /// the ones sharing a file, not everyone in the repository: the peers worth
    /// naming are the ones the warning is about.
    pub peers: Vec<String>,
    /// Paths written by this session and at least one peer, sorted.
    pub files: Vec<String>,
}

/// Session key -> what it collides with. Absent means nothing to report.
pub type Map = HashMap<String, Collision>;

/// Detect the overlaps and stamp each row with the level it is at.
///
/// The level lives on the row so the table can colour, sort and filter on it
/// like any other cell, while the map carries the part only one panel and the
/// footer need — who, and which files.
pub fn apply(sessions: &mut [Session]) -> Map {
    let map = detect(sessions);
    for s in sessions.iter_mut() {
        s.conflict = map.get(&s.key()).map(|c| c.level);
    }
    map
}

/// Group the live sessions by repository and report the overlaps.
///
/// Pure over the sessions handed in, apart from the repository-root cache, so
/// it can be recomputed every refresh without touching a transcript.
pub fn detect(sessions: &[Session]) -> Map {
    let mut by_repo: HashMap<PathBuf, Vec<&Session>> = HashMap::new();
    for s in sessions.iter().filter(|s| s.is_running()) {
        if s.label_source.is_empty() {
            continue;
        }
        by_repo.entry(ground(&s.label_source)).or_default().push(s);
    }

    let mut out = Map::new();
    for group in by_repo.into_values().filter(|g| g.len() > 1) {
        for s in &group {
            let mine: HashSet<&str> = s.recent_writes.iter().map(String::as_str).collect();
            let mut sharing = Vec::new();
            let mut neighbours = Vec::new();
            let mut files: BTreeSet<&str> = BTreeSet::new();

            let key = s.key();
            for other in group.iter().filter(|o| o.key() != key) {
                let common: Vec<&str> = other
                    .recent_writes
                    .iter()
                    .map(String::as_str)
                    .filter(|p| mine.contains(p))
                    .collect();
                if common.is_empty() {
                    neighbours.push(other.key());
                } else {
                    files.extend(common);
                    sharing.push(other.key());
                }
            }

            let (level, peers) = match sharing.is_empty() {
                false => (Overlap::File, sharing),
                true => (Overlap::Directory, neighbours),
            };
            if peers.is_empty() {
                continue;
            }
            out.insert(
                key,
                Collision {
                    level,
                    peers,
                    files: files.into_iter().map(str::to_string).collect(),
                },
            );
        }
    }
    out
}

/// Live sessions standing on the same ground as `dir`, each with whichever of
/// `files` it has already written.
///
/// The question [`detect`] answers for sessions cctop can see, asked instead by
/// something cctop cannot — an agent about to start a batch of edits, which has
/// no row in the table yet and so cannot be compared against one.
pub fn peers_of<'a>(
    sessions: &'a [Session],
    dir: &str,
    files: &[String],
) -> Vec<(&'a Session, Vec<String>)> {
    let here = ground(dir);
    let wanted: HashSet<String> = files.iter().map(|f| normalise(f, dir)).collect();
    sessions
        .iter()
        .filter(|s| s.is_running() && !s.label_source.is_empty())
        .filter(|s| ground(&s.label_source) == here)
        .map(|s| {
            let shared = s
                .recent_writes
                .iter()
                .filter(|w| wanted.contains(*w))
                .cloned()
                .collect();
            (s, shared)
        })
        .collect()
}

/// How long a repository root is trusted.
///
/// The same bargain [`crate::ui::columns`] makes for the branch name: a
/// checkout moves rarely, and walking the filesystem once per session per frame
/// to prove it hasn't is the cost that actually shows up.
const ROOT_TTL: Duration = Duration::from_secs(60);

static ROOTS: LazyLock<Mutex<HashMap<String, (PathBuf, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The ground a session stands on: its repository root, or the working
/// directory itself when it is not in a repository at all.
///
/// Falling back to the directory rather than to a shared "no repo" bucket
/// matters — otherwise every agent running outside a checkout would be reported
/// as colliding with every other one.
fn ground(dir: &str) -> PathBuf {
    let mut cache = ROOTS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((root, at)) = cache.get(dir)
        && at.elapsed() < ROOT_TTL
    {
        return root.clone();
    }
    let root = repo_root(Path::new(dir)).unwrap_or_else(|| PathBuf::from(dir));
    cache.insert(dir.to_string(), (root.clone(), Instant::now()));
    root
}

/// The nearest ancestor holding a `.git`, which for a linked worktree is the
/// worktree itself rather than the repository it was taken from.
fn repo_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Absolute, lexically normalised form of a path a transcript reported.
///
/// Harnesses disagree: Claude Code records absolute paths, OpenCode records
/// them relative to the session's directory. Two sessions can only be compared
/// once both are spelled the same way, and this is the only place that happens.
///
/// Lexical rather than [`std::fs::canonicalize`]: the file may have been
/// deleted since, and a comparison that silently stops working for deletions
/// would miss the collision that hurts most.
pub fn normalise(path: &str, cwd: &str) -> String {
    use std::path::Component;
    let path = path.trim();
    // `is_absolute` alone is false on Windows for a rooted path with no drive,
    // which is how every transcript written on Unix spells one.
    let rooted = Path::new(path);
    let joined = match rooted.is_absolute() || rooted.has_root() {
        true => rooted.to_path_buf(),
        false => Path::new(cwd).join(path),
    };

    let mut out = PathBuf::new();
    // Normal components pushed so far, and so how far a `..` may walk back.
    let mut depth = 0usize;
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => {
                out.pop();
                depth -= 1;
            }
            // Nothing left to pop: keeping the `..` is more honest than
            // silently anchoring the path somewhere it does not point.
            Component::ParentDir => out.push(".."),
            other => {
                out.push(other.as_os_str());
                if matches!(other, Component::Normal(_)) {
                    depth += 1;
                }
            }
        }
    }
    out.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    /// A scratch directory unique to one test, cleaned up by [`Fixture`].
    struct Fixture(PathBuf);

    impl Fixture {
        fn new(name: &str) -> Fixture {
            let root = std::env::temp_dir().join(format!(
                "cctop-collide-{}-{name}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("scratch directory");
            Fixture(root)
        }

        /// A directory with a `.git` of its own, so [`ground`] stops there.
        fn checkout(&self, rel: &str) -> String {
            let dir = self.0.join(rel);
            std::fs::create_dir_all(dir.join(".git")).expect("checkout");
            dir.to_string_lossy().into_owned()
        }

        fn dir(&self, rel: &str) -> String {
            let dir = self.0.join(rel);
            std::fs::create_dir_all(&dir).expect("directory");
            dir.to_string_lossy().into_owned()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn live(id: &str, dir: &str, writes: &[&str]) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.label_source = dir.into();
        s.process = Some(crate::proc::ProcInfo::default());
        s.recent_writes = writes
            .iter()
            .map(|w| normalise(w, "/anywhere"))
            .collect::<Vec<_>>();
        s
    }

    /// The whole point: two agents in one checkout, both having written the
    /// same file, are reported to each other by name — including when one of
    /// them was started from a subdirectory, which is the usual case.
    #[test]
    fn two_sessions_writing_one_file_collide() {
        let fx = Fixture::new("same-file");
        let repo = fx.checkout("repo");
        let sub = fx.dir("repo/src");
        let shared = format!("{repo}/src/ui.rs");

        let a = live("a", &repo, &[&shared, &format!("{repo}/README.md")]);
        let b = live("b", &sub, &[&shared]);
        let map = detect(&[a.clone(), b.clone()]);

        let hit = map.get(&a.key()).expect("a collides");
        assert_eq!(hit.level, Overlap::File);
        assert_eq!(hit.peers, vec![b.key()]);
        assert_eq!(hit.files, vec![normalise(&shared, "/anywhere")]);
        // Symmetric: the warning is useless if only one of them gets it.
        assert_eq!(map.get(&b.key()).expect("b collides").level, Overlap::File);
    }

    /// Sharing a repository without having touched the same file yet is worth
    /// knowing but is not the same warning.
    #[test]
    fn sharing_a_repository_alone_is_the_lesser_overlap() {
        let fx = Fixture::new("same-repo");
        let repo = fx.checkout("repo");
        let a = live("a", &repo, &[&format!("{repo}/a.rs")]);
        let b = live("b", &repo, &[&format!("{repo}/b.rs")]);

        let map = detect(&[a.clone(), b]);
        assert_eq!(map[&a.key()].level, Overlap::Directory);
        assert!(map[&a.key()].files.is_empty());
    }

    #[test]
    fn a_stopped_session_races_nobody() {
        let fx = Fixture::new("stopped");
        let repo = fx.checkout("repo");
        let file = format!("{repo}/x.rs");
        let a = live("a", &repo, &[&file]);
        let mut b = live("b", &repo, &[&file]);
        b.process = None;
        assert!(detect(&[a, b]).is_empty());
    }

    /// Two directories that merely share a prefix are two projects. `repo` and
    /// `repo2` must never be folded together by a `starts_with`.
    #[test]
    fn a_shared_prefix_is_not_a_shared_repository() {
        let fx = Fixture::new("prefix");
        let one = fx.checkout("repo");
        let two = fx.checkout("repo2");
        let a = live("a", &one, &["x.rs"]);
        let b = live("b", &two, &["x.rs"]);
        assert!(detect(&[a, b]).is_empty());
    }

    /// A worktree is how this project's contributors are told to keep out of
    /// each other's way, so reporting them as colliding would flag the fix as
    /// the fault. Its own `.git` is what makes it a separate root.
    #[test]
    fn agents_in_separate_worktrees_do_not_collide() {
        let fx = Fixture::new("worktree");
        let main = fx.checkout("repo");
        let wt = fx.dir("repo/.claude/worktrees/agent-1");
        std::fs::write(Path::new(&wt).join(".git"), "gitdir: /elsewhere\n").unwrap();

        let a = live("a", &main, &["src/ui.rs"]);
        let b = live("b", &wt, &["src/ui.rs"]);
        assert!(detect(&[a, b]).is_empty(), "worktrees are separate ground");

        // But two agents in the *same* worktree still are.
        let c = live("c", &wt, &["src/ui.rs"]);
        let d = live("d", &wt, &["src/ui.rs"]);
        assert_eq!(detect(&[c, d]).len(), 2);
    }

    #[test]
    fn paths_are_compared_in_one_spelling() {
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            normalise("src/ui.rs", "/repo"),
            format!("{sep}repo{sep}src{sep}ui.rs")
        );
        assert_eq!(
            normalise("/repo/src/ui.rs", "/other"),
            format!("{sep}repo{sep}src{sep}ui.rs")
        );
        assert_eq!(
            normalise("./src/../src/ui.rs", "/repo"),
            format!("{sep}repo{sep}src{sep}ui.rs")
        );
        // Nothing to pop: kept rather than quietly anchored at the root.
        assert_eq!(normalise("../out", ""), format!("..{sep}out"));
    }
}
