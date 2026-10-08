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
//! The repository root is the unit for the *neighbourhood* warning. A shared
//! file is stronger evidence and is not limited by it: two sessions holding
//! one path on disk are racing for it wherever they were launched from — an
//! agent working a parent checkout can write into a nested one another agent
//! owns, and an edit landing outside the repository has no ground to compare
//! at all.
//!
//! Only running sessions are compared. A session that has stopped may well have
//! left uncommitted work behind, but nothing it does from here on can race
//! anyone.

use crate::session::Session;
use std::collections::{HashMap, HashSet};
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
    for s in sessions.iter_mut().filter(|s| s.remote.is_none()) {
        // ponytail: one formatted key per row, which is what the map is keyed
        // by. Reusing one buffer across the loop would mean writing
        // `Session::key`'s format out a second time, and a stamp that quietly
        // stops finding its row is worse than an allocation per refresh.
        s.conflict = map.get(&s.key()).map(|c| c.level);
    }
    map
}

/// Group the live sessions by repository and report the overlaps.
///
/// Pure over the sessions handed in, apart from the repository-root cache, so
/// it can be recomputed every refresh without touching a transcript.
pub fn detect(sessions: &[Session]) -> Map {
    // Remote rows are excluded on both counts: their paths are on another
    // filesystem, so `ground` would stat whatever happens to sit at the same
    // path here, and each machine already detects and reports its own overlaps
    // through `--json`. [`apply`] leaves their carried verdict alone.
    let live: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.is_running() && s.remote.is_none() && !s.label_source.is_empty())
        .collect();

    let mut out = Map::new();

    // One key per live session, derived at most once and only where something
    // needs it. The map is keyed by owned Strings, but nothing that only
    // compares two sessions needs one: `Session::key` formats, and both passes
    // below pair every session with every other, so a key derived per pair is a
    // key derived per pair of times — three over in the second pass, where the
    // filter, the duplicate check and the push each wanted the same name. A
    // session with nothing to report never derives one at all, which on a
    // machine running agents across many checkouts is most of them.
    let mut keys: Vec<Option<String>> = vec![None; live.len()];
    // Whether two live rows can carry one key, which is the only way a pair can
    // name a peer its row has already been told about. Asked on the first pair
    // that shares a file, since answering costs a pass over `live` and most
    // machines have nothing to report.
    let mut keys_repeat: Option<bool> = None;

    // A shared file is global evidence, not scoped to a ground: the two
    // sessions hold one path on disk wherever they were launched from. An
    // agent working a parent checkout can write into a nested one another
    // agent owns, and an edit landing outside the repository has no ground to
    // group by at all — so this pass pairs every live session with every
    // other, and the ground grouping below is only for the lesser warning.
    for (i, s) in live.iter().enumerate() {
        let mine: HashSet<&str> = s
            .recent_writes
            .iter()
            .map(String::as_str)
            .filter(|p| contested(p))
            .collect();
        if mine.is_empty() {
            continue;
        }
        for (j, other) in live.iter().enumerate().skip(i + 1) {
            // One path on two machines is two files.
            if machine(s) != machine(other) {
                continue;
            }
            let common: Vec<&str> = other
                .recent_writes
                .iter()
                .map(String::as_str)
                .filter(|p| mine.contains(p))
                .collect();
            if common.is_empty() {
                continue;
            }
            let repeat = *keys_repeat.get_or_insert_with(|| keys_repeat_in(&live));
            // Reported both ways round: a collision is worth nothing to the one
            // session that was not told about it.
            for (a, b) in [(i, j), (j, i)] {
                derive_key(&mut keys, a, live[a]);
                derive_key(&mut keys, b, live[b]);
                let (key, peer) = (key_of(&keys, a), key_of(&keys, b));
                let entry = slot(&mut out, key, Overlap::File);
                // Each pair of rows is visited once, so a row can only already
                // have been told about this peer if some other row is the same
                // session — and then the list really can hold it twice.
                if !repeat || !entry.peers.iter().any(|p| p == peer) {
                    entry.peers.push(peer.to_string());
                }
                entry.files.extend(common.iter().map(|p| p.to_string()));
            }
        }
    }

    // Same ground without a shared file — the lesser warning. A session
    // already at [`Overlap::File`] keeps only the peers its warning is about
    // rather than gaining the whole neighbourhood.
    let mut by_repo: HashMap<(Option<&str>, PathBuf), Vec<usize>> = HashMap::new();
    for (i, s) in live.iter().enumerate() {
        by_repo
            .entry((machine(s), ground(&s.label_source)))
            .or_default()
            .push(i);
    }
    for group in by_repo.into_values().filter(|g| g.len() > 1) {
        // The whole group's keys before the pairs: each name is needed
        // alongside another, and two mutable borrows cannot overlap.
        for &i in &group {
            derive_key(&mut keys, i, live[i]);
        }
        for &i in &group {
            let key = key_of(&keys, i);
            for &j in &group {
                let other = key_of(&keys, j);
                // The same session listed twice is one row, not a pair of them:
                // the key is what says so, not the index.
                if other == key {
                    continue;
                }
                let entry = slot(&mut out, key, Overlap::Directory);
                // ponytail: this scan is the one that stays. The file pass above
                // can only find a peer already named when two rows are one
                // session, and asks whether that is so rather than looking; here
                // a row really can arrive with a peer from the file pass, so the
                // list has to be searched — once per peer, which is a ground of
                // n sessions costing n^2 again. It is the same warning one
                // session at a time, and one checkout with a hundred agents in
                // it is not the case this is worth losing the other work over.
                if entry.level == Overlap::Directory && !entry.peers.iter().any(|p| p == other) {
                    entry.peers.push(other.to_string());
                }
            }
        }
    }

    for c in out.values_mut() {
        c.files.sort();
        c.files.dedup();
    }
    out
}

/// Whether two of `live` carry the same key.
///
/// The session list is not deduplicated before it gets here, and a row named
/// twice is one row, not two — so the peers a pair appends can repeat. Matched
/// on the key's two fields rather than on [`Session::key`], which formats a
/// String per comparison: this is a pass over every live session, and the whole
/// point is to keep the per-pair work off the heap.
fn keys_repeat_in(live: &[&Session]) -> bool {
    let mut seen: HashSet<(&crate::pricing::Provider, &str)> = HashSet::new();
    live.iter()
        .any(|s| !seen.insert((&s.provider, s.session_id.as_str())))
}

/// The machine whose files a live session's paths name: `None` for this one,
/// the host for a session working elsewhere under `cctop sandbox`.
///
/// Its paths are spelled exactly like local ones — that is the point of
/// mounting the host's directory at the same path — so without this a
/// sandboxed session and a local one would be told they share a checkout
/// that is in fact on two different machines.
fn machine(s: &Session) -> Option<&str> {
    s.sandbox.as_deref().map(crate::sandbox::host_of)
}

/// Remember `live[i]`'s key, unless something already needed it.
fn derive_key(keys: &mut [Option<String>], i: usize, s: &Session) {
    if keys[i].is_none() {
        keys[i] = Some(s.key());
    }
}

/// The key [`derive_key`] stored for `live[i]`.
fn key_of(keys: &[Option<String>], i: usize) -> &str {
    keys[i].as_deref().expect("key derived before it was read")
}

/// The row for `key`, empty at `level` if there is not one yet.
///
/// `entry` takes the key by value, so asking for a row that is already there
/// formats a String to drop on the floor — and the hit is the common case, once
/// per pair of sessions per direction. A miss pays for a String and a second
/// hash; a hit pays for one.
fn slot<'a>(out: &'a mut Map, key: &str, level: Overlap) -> &'a mut Collision {
    if out.contains_key(key) {
        return out.get_mut(key).expect("row present a moment ago");
    }
    out.entry(key.to_string()).or_insert(Collision {
        level,
        peers: Vec::new(),
        files: Vec::new(),
    })
}

/// Extensions where a second writer is ordinary rather than a race.
///
/// Two groups, one rule: **nothing here is work that can be silently lost.**
/// Being plain text is not the test — source code is plain text, and losing a
/// line of it is the whole reason this warning exists.
///
/// *Prose* — the markups and `txt` — is what two agents are supposed to be
/// writing at once. A changelog, a notes file, a memory index: edited by
/// section rather than rewritten whole, and a clash is legible on sight.
///
/// *Machine output* — lock files, checksum lists, logs — is nobody's
/// handwriting. Two agents in one checkout both running `cargo add` or
/// `npm install` will write a lock file every single time, and the fix for a
/// bad one is to regenerate it. A real disagreement about a dependency shows up
/// in git, which announces it properly.
///
/// ponytail: it is an extension, not an analysis. Two agents rewriting one
/// README whole *can* lose an edit, and this will not say so. That is the trade
/// the warning is worth having at all — one that fires on every `MEMORY.md`
/// write is one that gets read past, taking the `src/ui.rs` case with it.
const UNCONTESTED_EXT: [&str; 9] = [
    "md", "markdown", "rst", "adoc", "org", "txt", // prose
    "lock", "sum", "log", // written by a tool, not a person
];

/// Lock files that spell themselves with an ordinary config extension.
///
/// `Cargo.lock`, `yarn.lock`, `poetry.lock` and `flake.lock` are caught by
/// `lock` above; npm and pnpm use `.json` and `.yaml`, which are exactly the
/// extensions that must *not* be exempt in general — two agents editing one
/// `package.json` is the case the warning is for. So these are named outright.
const UNCONTESTED_NAME: [&str; 3] = ["package-lock.json", "npm-shrinkwrap.json", "pnpm-lock.yaml"];

/// Whether two agents writing `path` is worth reporting.
pub fn contested(path: &str) -> bool {
    let path = Path::new(path);
    let matches = |part: Option<&std::ffi::OsStr>, list: &[&str]| {
        part.and_then(|p| p.to_str())
            .is_some_and(|p| list.contains(&p.to_ascii_lowercase().as_str()))
    };
    !matches(path.file_name(), &UNCONTESTED_NAME) && !matches(path.extension(), &UNCONTESTED_EXT)
}

/// Live sessions standing on the same ground as `dir`, or holding one of
/// `files` wherever they stand, each with whichever of `files` it has already
/// written.
///
/// The question [`detect`] answers for sessions cctop can see, asked instead by
/// something cctop cannot — an agent about to start a batch of edits, which has
/// no row in the table yet and so cannot be compared against one. A session on
/// other ground earns its place by evidence alone: it already holds a file the
/// caller is about to touch.
pub fn peers_of<'a>(
    sessions: &'a [Session],
    dir: &str,
    files: &[String],
) -> Vec<(&'a Session, Vec<String>)> {
    let here = ground(dir);
    let wanted: HashSet<String> = files.iter().map(|f| normalise(f, dir)).collect();
    sessions
        .iter()
        .filter(|s| s.is_running() && s.remote.is_none() && !s.label_source.is_empty())
        .map(|s| {
            let shared: Vec<String> = s
                .recent_writes
                .iter()
                // The same exemption `detect` makes, because this answers the
                // same question for an agent that has no row yet.
                .filter(|w| wanted.contains(*w) && contested(w))
                .cloned()
                .collect();
            (s, shared)
        })
        .filter(|(s, shared)| !shared.is_empty() || ground(&s.label_source) == here)
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
    let fresh = {
        let cache = ROOTS.lock().unwrap_or_else(PoisonError::into_inner);
        match cache.get(dir) {
            Some((root, at)) if at.elapsed() < ROOT_TTL => Some(root.clone()),
            _ => None,
        }
    };
    if let Some(root) = fresh {
        return root;
    }
    // The ancestor walk is a dozen `stat`s on the worst day, and this is asked
    // per live session on every refresh — so it happens with the lock off, and
    // the lock is taken afterwards only to record the answer. Under it, every
    // other thread comparing roots against any directory waits for this one.
    let root = repo_root(Path::new(dir)).unwrap_or_else(|| PathBuf::from(dir));
    let mut cache = ROOTS.lock().unwrap_or_else(PoisonError::into_inner);
    // Another thread may have got here first; its reading is no older than this
    // one, and the point of the TTL is that nobody re-reads, not that this
    // thread must be the one to.
    let answered = cache.get(dir).filter(|(_, at)| at.elapsed() < ROOT_TTL);
    match answered {
        Some((cached, _)) => cached.clone(),
        None => {
            cache.insert(dir.to_string(), (root.clone(), Instant::now()));
            root
        }
    }
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
    let rooted = Path::new(path);
    let joined = match rooted.is_absolute() {
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

    /// A session working on another machine through `cctop sandbox` spells
    /// its paths exactly like a local one, and is still on another disk. Two
    /// sandboxed on the same host are on the same disk, and race as usual.
    #[test]
    fn a_sandboxed_session_collides_only_on_its_own_host() {
        let fx = Fixture::new("sandbox");
        let repo = fx.checkout("repo");
        let file = format!("{repo}/src/ui.rs");
        let local = live("local", &repo, &[&file]);
        let mut far = live("far", &repo, &[&file]);
        far.sandbox = Some(format!("box:{repo}"));
        assert!(detect(&[local.clone(), far.clone()]).is_empty());

        let mut also_far = live("also-far", &repo, &[&file]);
        also_far.sandbox = far.sandbox.clone();
        let map = detect(&[local, far, also_far]);
        assert_eq!(map.len(), 2, "{map:?}");
        assert!(map.values().all(|c| c.level == Overlap::File));
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

    /// The session list is not deduplicated before it gets here, so one session
    /// can arrive twice — and a row is worth one mention of each peer however
    /// many rows carry it. Two copies of a session have written the same files,
    /// so the row also names itself once, which is the quirk this deduplication
    /// is really for: without it the row would name itself twice.
    #[test]
    fn a_session_listed_twice_is_told_about_its_peers_once() {
        let fx = Fixture::new("twice");
        let repo = fx.checkout("repo");
        let shared = format!("{repo}/src/ui.rs");

        let a = live("a", &repo, &[&shared]);
        let b = live("b", &repo, &[&shared]);
        let hit = &detect(&[a.clone(), a.clone(), b.clone()])[&a.key()];

        assert_eq!(hit.level, Overlap::File);
        assert_eq!(
            hit.peers,
            vec![a.key(), b.key()],
            "one row, one mention each"
        );
    }

    /// The same for the lesser warning, where a pair is every other session
    /// standing on the same ground: one session is not its own neighbour, and
    /// two rows of one key are one row.
    #[test]
    fn a_session_listed_twice_is_not_its_own_neighbour() {
        let fx = Fixture::new("twice-quiet");
        let repo = fx.checkout("repo");
        // Prose, so this is the ground pass and nothing else.
        let a = live("a", &repo, &[&format!("{repo}/README.md")]);

        assert!(detect(&[a.clone(), a.clone()]).is_empty());
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

    /// Prose is where two agents at once is the normal state of affairs. A
    /// warning that fires every time both of them append to `MEMORY.md` is one
    /// that gets read past, which costs the `src/ui.rs` case it exists for.
    #[test]
    fn two_agents_in_one_notes_file_are_not_a_collision() {
        let fx = Fixture::new("prose");
        let repo = fx.checkout("repo");
        let notes = format!("{repo}/memory/MEMORY.md");

        let a = live("a", &repo, &[&notes, &format!("{repo}/TODO.txt")]);
        let b = live("b", &repo, &[&notes, &format!("{repo}/TODO.txt")]);
        let map = detect(&[a.clone(), b.clone()]);

        // Still in the same checkout, which is worth knowing — but the file
        // callout, and the footer line it drives, are gone.
        assert_eq!(map[&a.key()].level, Overlap::Directory);
        assert!(map[&a.key()].files.is_empty());
    }

    /// A lock file is the one two agents in a checkout collide on by accident:
    /// both run a package manager, neither wrote a line of it, and the fix for
    /// a bad one is to regenerate it.
    #[test]
    fn a_lock_file_is_not_somebody_s_work() {
        let fx = Fixture::new("locks");
        let repo = fx.checkout("repo");
        let locks: Vec<String> = [
            "Cargo.lock",
            "package-lock.json",
            "pnpm-lock.yaml",
            "go.sum",
            "build.log",
        ]
        .iter()
        .map(|f| format!("{repo}/{f}"))
        .collect();
        let borrowed: Vec<&str> = locks.iter().map(String::as_str).collect();

        let a = live("a", &repo, &borrowed);
        let b = live("b", &repo, &borrowed);
        assert_eq!(detect(&[a.clone(), b])[&a.key()].level, Overlap::Directory);
    }

    /// The config extensions the lock files borrow are not themselves exempt.
    /// Two agents editing one `package.json` is the case the warning is for.
    #[test]
    fn a_lock_file_s_extension_does_not_exempt_its_neighbours() {
        assert!(!contested("/repo/pnpm-lock.yaml"));
        assert!(contested("/repo/package.json"));
        assert!(contested("/repo/docker-compose.yaml"));
        assert!(contested("/repo/Cargo.toml"));
        assert!(contested("/repo/.env"));
        assert!(contested("/repo/data.csv"));
    }

    /// And the exemption is per file, not per session: sharing a notes file
    /// must not hide the source file shared alongside it.
    #[test]
    fn prose_alongside_code_still_reports_the_code() {
        let fx = Fixture::new("prose-and-code");
        let repo = fx.checkout("repo");
        let code = format!("{repo}/src/ui.rs");
        let notes = format!("{repo}/NOTES.md");

        let a = live("a", &repo, &[&code, &notes]);
        let b = live("b", &repo, &[&code, &notes]);
        let hit = &detect(&[a.clone(), b])[&a.key()];

        assert_eq!(hit.level, Overlap::File);
        assert_eq!(hit.files, vec![normalise(&code, "/anywhere")]);
    }

    /// `check_conflicts` answers the same question for an agent with no row
    /// yet, so it has to answer it the same way.
    #[test]
    fn the_agent_facing_answer_makes_the_same_exemption() {
        let fx = Fixture::new("prose-peers");
        let repo = fx.checkout("repo");
        let notes = format!("{repo}/MEMORY.md");
        let code = format!("{repo}/src/ui.rs");
        let sessions = [live("a", &repo, &[&notes, &code])];

        let asked = [notes.clone(), code.clone()];
        let peers = peers_of(&sessions, &repo, &asked);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].1, vec![normalise(&code, "/anywhere")]);
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
        // Each session's writes resolve against its own directory — the same
        // relative name is a different file on disk in each repository.
        let a = live("a", &one, &[&format!("{one}/x.rs")]);
        let b = live("b", &two, &[&format!("{two}/x.rs")]);
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

        // The same relative name resolves against each session's own ground,
        // so these are two different files on disk — which is the point of a
        // worktree.
        let a = live("a", &main, &[&format!("{main}/src/ui.rs")]);
        let b = live("b", &wt, &[&format!("{wt}/src/ui.rs")]);
        assert!(detect(&[a, b]).is_empty(), "worktrees are separate ground");

        // But two agents in the *same* worktree still are.
        let shared = format!("{wt}/src/ui.rs");
        let c = live("c", &wt, &[&shared]);
        let d = live("d", &wt, &[&shared]);
        assert_eq!(detect(&[c, d]).len(), 2);
    }

    /// Separate ground keeps the neighbourhood warning out, but a file is
    /// stronger evidence than where either session was launched: an agent in
    /// the parent checkout writing into a nested checkout is racing the agent
    /// that owns it, and the `!` has to say so.
    #[test]
    fn a_shared_file_collides_across_grounds() {
        let fx = Fixture::new("cross-ground");
        let outer = fx.checkout("outer");
        let inner = fx.checkout("outer/nested");
        let shared = format!("{inner}/src/x.rs");

        let a = live("a", &outer, &[&shared]);
        let b = live("b", &inner, &[&shared]);
        let map = detect(&[a.clone(), b.clone()]);

        let hit = map.get(&a.key()).expect("a collides");
        assert_eq!(hit.level, Overlap::File);
        assert_eq!(hit.peers, vec![b.key()]);
        assert_eq!(hit.files, vec![normalise(&shared, "/anywhere")]);
        assert_eq!(map[&b.key()].level, Overlap::File);
    }

    /// The agent-facing answer crosses grounds the same way — a session that
    /// already holds one of the caller's files is the peer that matters most,
    /// whichever checkout it was launched from.
    #[test]
    fn the_agent_facing_answer_reaches_across_grounds() {
        let fx = Fixture::new("peers-cross");
        let outer = fx.checkout("outer");
        let inner = fx.checkout("outer/nested");
        let shared = format!("{inner}/src/x.rs");
        let sessions = [live("a", &inner, &[&shared])];

        let peers = peers_of(&sessions, &outer, std::slice::from_ref(&shared));
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].1, vec![normalise(&shared, "/anywhere")]);
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
