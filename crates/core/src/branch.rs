//! The git branch a session's working directory has checked out.
//!
//! Read here rather than in the table that shows it, because the branch is
//! part of what a session *is* to every surface that describes one — the
//! `--json` document, the web pages, a handoff brief, the MCP tools — and
//! none of those should have to depend on the terminal UI to ask.

use crate::session::Session;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a branch reading is trusted.
///
/// A checkout is a rare event and a name that is a few seconds out of date is
/// harmless; walking the filesystem once per row per frame is not.
const BRANCH_TTL: Duration = Duration::from_secs(15);

/// A branch reading, and when it was taken. `None` is a real answer — the
/// directory is not in a repository — and is cached just as a name is.
type Reading = (Option<String>, Instant);

/// Working directory -> its last reading.
static BRANCHES: LazyLock<Mutex<HashMap<String, Reading>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Branch checked out in a session's working directory, or `None` when it is
/// not in a repository. Cached, so the filter can ask per row per keystroke.
pub fn branch_of(s: &crate::session::Session) -> Option<String> {
    // A remote row's working directory is a path on another machine. Reading it
    // here would report the branch of whatever happens to sit at the same path
    // locally, which is worse than reporting nothing.
    match &s.remote {
        Some(r) => r.branch.clone(),
        // A sandboxed row's directory is the host's, mounted here for as long
        // as the agent runs. Once it has stopped, the mount is gone and the
        // same path is whatever this machine has there — an empty directory
        // at best, a checkout of something else at worst — so no answer.
        None if s.sandbox.is_some() && !s.is_running() => None,
        None => branch(&s.label_source),
    }
}

/// Branch of a working directory, cached.
///
/// `git rev-parse` would be a subprocess per row per frame, and `git2` is a C
/// library and a build step for what is one short file in a documented format.
///
/// The read happens with the lock *not* held: this is asked per visible row per
/// frame and once per non-matching session per keystroke, and [`read_head`]
/// walks the ancestors stat-ing for a `.git` before it reads a file — so paying
/// that under a process-global mutex turned every expiry into a stall for every
/// other thread asking any directory at all. Compute, then take the lock only to
/// record the answer.
fn branch(dir: &str) -> Option<String> {
    let (cached, fresh) = {
        let cache = BRANCHES.lock().unwrap_or_else(PoisonError::into_inner);
        let (name, fresh) = reading(&cache, dir);
        (name.map(str::to_string), fresh)
    };
    if fresh {
        return cached;
    }
    let read = read_uncached(dir);
    let mut cache = BRANCHES.lock().unwrap_or_else(PoisonError::into_inner);
    // Another thread may have read it while the lock was off. Its answer is at
    // least as recent as this one, so it is kept — the point of the TTL is that
    // nobody re-reads for a while, not that this thread must be the one.
    if !is_fresh(cache.get(dir)) {
        cache.insert(dir.to_string(), (read.clone(), Instant::now()));
    }
    read
}

/// The branch of `dir` from disk, and nothing else — no cache, no lock.
fn read_uncached(dir: &str) -> Option<String> {
    // A row with no directory has no repository to ask, and reading one anyway
    // would ask about whichever checkout cctop itself was started in.
    if dir.is_empty() {
        return None;
    }
    read_head(Path::new(dir))
}

/// Whether a reading is still inside the TTL.
fn is_fresh(reading: Option<&Reading>) -> bool {
    reading.is_some_and(|(_, at)| at.elapsed() < BRANCH_TTL)
}

/// The cached name for `dir`, and whether that name can still be trusted.
///
/// A directory with no reading at all counts as stale: nothing has said what is
/// checked out there yet. A row with no directory is answered here rather than
/// looked up — [`refresh`] reads no `HEAD` for it, so there is nothing to wait
/// for either.
fn reading<'c>(cache: &'c HashMap<String, Reading>, dir: &str) -> (Option<&'c str>, bool) {
    if dir.is_empty() {
        return (None, true);
    }
    match cache.get(dir) {
        Some((branch, at)) => (branch.as_deref(), at.elapsed() < BRANCH_TTL),
        None => (None, false),
    }
}

/// Order two rows by branch without copying either name.
///
/// The branch column is a sort key, and a sort is O(n log n) comparisons of a
/// list `refilter` rebuilds on every refresh *and* on every keystroke in the
/// search box. Asking [`branch_of`] for each side meant a mutex round trip, a
/// working directory hashed and a `String` copied, twice per comparison, to
/// compare two names that were sitting in the cache already.
pub fn cmp_branch(a: &Session, b: &Session) -> Ordering {
    // A remote row's name arrived with the row, from the machine that read it;
    // this machine's cache has nothing to say about it. A sandboxed row's
    // answer depends on whether its mount is still there, which only
    // `branch_of` asks.
    if a.remote.is_some() || b.remote.is_some() || a.sandbox.is_some() || b.sandbox.is_some() {
        return branch_of(a).cmp(&branch_of(b));
    }
    let (ad, bd) = (a.label_source.as_str(), b.label_source.as_str());
    // One lookup per side rather than two: asking whether a name is fresh and
    // then asking for it hashes the same key twice, and the answer cannot come
    // out differently — a name inside the TTL is the answer for as long as it
    // has been inside it. One lock for both sides for the same reason: the two
    // names are compared against one snapshot of the cache, not two.
    {
        let cache = BRANCHES.lock().unwrap_or_else(PoisonError::into_inner);
        let (left, left_fresh) = reading(&cache, ad);
        let (right, right_fresh) = reading(&cache, bd);
        if left_fresh && right_fresh {
            return left.cmp(&right);
        }
    }
    // Off the lock, because a read is an ancestor walk and a file read and this
    // is a comparison inside a sort: see [`branch`]. Both sides are read before
    // either is written back, so the two answers still describe one moment.
    let reads = (read_uncached(ad), read_uncached(bd));
    let mut cache = BRANCHES.lock().unwrap_or_else(PoisonError::into_inner);
    for (dir, read) in [(ad, reads.0), (bd, reads.1)] {
        if !dir.is_empty() {
            cache.insert(dir.to_string(), (read, Instant::now()));
        }
    }
    let (left, _) = reading(&cache, ad);
    let (right, _) = reading(&cache, bd);
    left.cmp(&right)
}

/// Read the branch straight out of the repository's `HEAD`.
///
/// Three shapes have to survive this: an ordinary `.git` directory; a `.git`
/// *file* holding a `gitdir:` pointer, which is what a linked worktree or a
/// submodule has and where the interesting branches usually live; and a HEAD
/// carrying a commit id instead of a ref, as during a rebase or a bisect —
/// reported as a short id, because "not on a branch" is itself worth seeing.
fn read_head(start: &Path) -> Option<String> {
    // A session is often launched from a subdirectory of its repository.
    let git = start
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|p| p.exists())?;
    let head = if git.is_dir() {
        git.join("HEAD")
    } else {
        let pointer = std::fs::read_to_string(&git).ok()?;
        let target = PathBuf::from(pointer.trim().strip_prefix("gitdir:")?.trim());
        // Absolute for a worktree, relative to the containing directory for a
        // submodule.
        let target = if target.is_absolute() {
            target
        } else {
            git.parent()?.join(target)
        };
        target.join("HEAD")
    };

    let head = std::fs::read_to_string(head).ok()?;
    let head = head.trim();
    if let Some(name) = head.strip_prefix("ref: refs/heads/") {
        Some(name.to_string())
    } else if let Some(other) = head.strip_prefix("ref: ") {
        // Some other ref namespace; show it whole rather than guess at a name.
        Some(other.to_string())
    } else if head.is_empty() {
        None
    } else {
        // Detached. Take chars, not bytes: a corrupt HEAD must not panic here.
        Some(std::iter::once('@').chain(head.chars().take(7)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn session(id: &str) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.started_at = "2026-01-01T00:00:00Z".into();
        s.last_active = "2026-01-01T00:00:00Z".into();
        s
    }

    /// The three HEAD shapes cctop actually meets: a checkout, a linked
    /// worktree pointing elsewhere, and a detached HEAD mid-rebase.
    #[test]
    fn head_is_read_for_a_checkout_a_worktree_and_a_detached_commit() {
        let root = std::env::temp_dir().join(format!("cctop-branch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/feat/idle\n").unwrap();
        assert_eq!(read_head(&repo).as_deref(), Some("feat/idle"));

        // From a subdirectory, since that is where agents are usually started.
        let deep = repo.join("src/ui");
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(read_head(&deep).as_deref(), Some("feat/idle"));

        // A linked worktree: `.git` is a file pointing at the real git dir.
        let gitdir = repo.join(".git/worktrees/wt");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/other\n").unwrap();
        let worktree = root.join("wt");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .unwrap();
        assert_eq!(read_head(&worktree).as_deref(), Some("other"));

        std::fs::write(repo.join(".git/HEAD"), "0123456789abcdef0123\n").unwrap();
        assert_eq!(read_head(&repo).as_deref(), Some("@0123456"));

        assert_eq!(read_head(&root.join("nowhere")), None);
        std::fs::remove_dir_all(&root).ok();
    }

    /// Sorting the branch column must order rows the way reading each row's
    /// branch does — a cheaper path through the cache is only allowed to be
    /// cheaper if it agrees. A row with no directory, one in no repository, one
    /// on `main` and one on `Alpha`, and a remote row whose name came from
    /// another machine, are all pairs the two paths could read differently.
    #[test]
    fn sorting_by_branch_agrees_with_reading_each_rows_branch() {
        let root = std::env::temp_dir().join(format!("cctop-branch-sort-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (name, head) in [
            ("main", "ref: refs/heads/main\n"),
            ("alpha", "ref: refs/heads/Alpha\n"),
        ] {
            let repo = root.join(name);
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(repo.join(".git/HEAD"), head).unwrap();
        }
        std::fs::create_dir_all(root.join("plain/src")).unwrap();

        let remote = |id: &str, branch: Option<&str>| {
            let mut s = session(id);
            s.label_source = root.join("main").to_string_lossy().into_owned();
            s.remote = Some(crate::session::Remote {
                host: "far".into(),
                branch: branch.map(str::to_string),
                skew: None,
            });
            s
        };
        let mut rows = vec![
            session("no-dir"),
            session("not-a-repo"),
            session("main"),
            session("alpha"),
        ];
        rows[1].label_source = root.join("plain/src").to_string_lossy().into_owned();
        rows[2].label_source = root.join("main").to_string_lossy().into_owned();
        rows[3].label_source = root.join("alpha/src").to_string_lossy().into_owned();
        rows.extend([remote("far-main", Some("main")), remote("far-none", None)]);

        for a in &rows {
            for b in &rows {
                assert_eq!(cmp_branch(a, b), branch_of(a).cmp(&branch_of(b)));
            }
        }
        std::fs::remove_dir_all(&root).ok();
    }
}
