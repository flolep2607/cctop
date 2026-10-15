//! Git checkouts a launch can start in: where a directory's repository is,
//! and a new worktree of it for an agent to work in apart.
//!
//! Here rather than in the terminal UI because both launchers make one: the
//! TUI's `F` and harness options, and the web launcher's branch field, which
//! the server answers. One `git worktree add`, so the two put a branch's
//! checkout in the same place.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A directory's repository: its common git directory and the checkout root.
pub type Located = Option<(PathBuf, PathBuf)>;

/// Resolve `dir` to `(common git dir, checkout root)`.
///
/// The common directory is what every worktree of a repository shares, which
/// is why it and not the checkout root is the repository's identity: two
/// worktrees have two roots and one common directory. A linked worktree's `.git`
/// is a file naming its private git dir, and that dir's `commondir` names the
/// shared one. A submodule's git dir has no `commondir` — it is a repository of
/// its own, and is grouped as one.
///
/// Relative paths are refused: they would resolve against cctop's own working
/// directory, and group a session under whatever repository cctop was started
/// in.
pub fn locate(dir: &Path) -> Located {
    if !dir.is_absolute() {
        return None;
    }
    let root = dir.ancestors().find(|d| d.join(".git").exists())?;
    let git = root.join(".git");
    if git.is_dir() {
        return Some((git, root.to_path_buf()));
    }
    let pointer = std::fs::read_to_string(&git).ok()?;
    let target = PathBuf::from(pointer.trim().strip_prefix("gitdir:")?.trim());
    let target = if target.is_absolute() {
        target
    } else {
        root.join(target)
    };
    let common = match std::fs::read_to_string(target.join("commondir")) {
        Ok(rel) => {
            let joined = target.join(rel.trim());
            PathBuf::from(crate::collide::normalise(&joined.to_string_lossy(), "/"))
        }
        Err(_) => target,
    };
    Some((common, root.to_path_buf()))
}

/// The directory a repository is known by: the one holding its `.git`, or the
/// common directory itself for a bare repository, which has no other.
pub fn repo_dir(common: &Path) -> PathBuf {
    match common.file_name() {
        Some(name) if name == ".git" => common.parent().unwrap_or(common).to_path_buf(),
        _ => common.to_path_buf(),
    }
}

/// A directory's repository, as every surface names it: where it is on disk,
/// and what makes two copies of it the same repository.
#[derive(Debug, Clone, PartialEq)]
pub struct Repo {
    /// The common git directory, shared by every worktree of one clone.
    pub common: PathBuf,
    /// The checkout the directory is in.
    pub root: PathBuf,
    /// The repository's identity: its `origin` URL normalised
    /// (`github.com/owner/repo`), or the repository's directory when it has no
    /// origin. Equal for every worktree of one clone, because they share one
    /// config; equal for separate clones of one origin, which share nothing
    /// else on disk.
    pub key: String,
}

impl Repo {
    /// The key without its host — `o/r` for `github.com/o/r` — which is what
    /// a search for the repository means. Matching the host would make
    /// `github` find every repository hosted there. A local path is all path.
    pub fn path(&self) -> &str {
        match self.key.starts_with('/') {
            true => &self.key,
            false => self.key.split_once('/').map_or(&*self.key, |(_, p)| p),
        }
    }
}

/// How long a directory's repository is trusted. A checkout moves about as
/// often as a branch, and this is asked per row per refresh.
const REPO_TTL: Duration = Duration::from_secs(60);

/// Directory -> its repository and when that was read. `None` is an answer
/// too — not in a repository — and is cached like one.
type Reading = (Option<Repo>, Instant);

static REPOS: LazyLock<Mutex<HashMap<String, Reading>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// [`locate`] and the origin URL for `dir`, cached.
///
/// In core rather than beside the tree view that groups by it, so the
/// `--json` document, the filter and the launchers resolve a `.git` one way
/// and from one cache.
pub fn repo_cached(dir: &str) -> Option<Repo> {
    if let Some((found, at)) = REPOS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(dir)
        && at.elapsed() < REPO_TTL
    {
        return found.clone();
    }
    // Read with the lock off, for the reason `branch::branch` gives.
    let found = locate(Path::new(dir)).map(|(common, root)| {
        let key = origin_url(&common)
            .map(|u| normalise_url(&u, &repo_dir(&common)))
            .unwrap_or_else(|| repo_dir(&common).to_string_lossy().into_owned());
        Repo { common, root, key }
    });
    REPOS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(dir.to_string(), (found.clone(), Instant::now()));
    found
}

/// [`repo_cached`], as the `(common dir, checkout root)` pair [`locate`] gives.
pub fn locate_cached(dir: &str) -> Located {
    repo_cached(dir).map(|r| (r.common, r.root))
}

/// The repository a session is working in, or `None` when it is in none —
/// or when this machine cannot say.
pub fn repo_of(s: &crate::session::Session) -> Option<Repo> {
    // A remote row's directory is a path on another machine, and a stopped
    // sandbox's mount is gone: the same two refusals `branch_of` makes, for
    // the same reason — reading whatever sits at that path here is worse than
    // no answer.
    if s.remote.is_some() || (s.sandbox.is_some() && !s.is_running()) {
        return None;
    }
    repo_cached(&s.label_source)
}

/// `remote.origin.url` from a repository's `config`.
///
// ponytail: reads the one file, not `include`/`includeIf` or the global
// config; an origin set only there reads as none, and the repository falls
// back to being known by its directory.
fn origin_url(common: &Path) -> Option<String> {
    let config = std::fs::read_to_string(common.join("config")).ok()?;
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            // Section names are case-insensitive, the subsection is not.
            let header = line.trim_start_matches('[').trim_end_matches(']');
            in_origin = header
                .split_once(char::is_whitespace)
                .is_some_and(|(s, sub)| {
                    s.eq_ignore_ascii_case("remote") && sub.trim() == "\"origin\""
                });
        } else if in_origin
            && let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case("url")
        {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// One spelling for the ways a URL names a repository: `git@host:o/r.git`,
/// `ssh://git@host:22/o/r` and `https://host/o/r/` are all `host/o/r`.
///
/// A local path — `/srv/up`, `file:///srv/up`, `../up` — is the directory it
/// names, resolved against `base` (the repository's own directory, which is
/// where git resolves a relative remote) and lexically normalised: so a clone
/// of `/srv/up` has the key `/srv/up` itself has, and two unrelated repos that
/// both spell their origin `../up` do not share one.
fn normalise_url(url: &str, base: &Path) -> String {
    let (rest, scheme) = match url.split_once("://") {
        Some((scheme, rest)) => (rest, Some(scheme)),
        None => (url, None),
    };
    // Git's rule: without a scheme, a colon before any slash is `host:path`,
    // and anything else is a path.
    let scp = scheme.is_none() && rest.find(':').is_some_and(|c| !rest[..c].contains('/'));
    if scheme.is_some_and(|s| s.eq_ignore_ascii_case("file")) || (scheme.is_none() && !scp) {
        let path = rest.trim_end_matches('/');
        let path = path.strip_suffix("/.git").unwrap_or(path);
        let base = base.to_string_lossy();
        return crate::collide::normalise(path, &base);
    }
    // Credentials and the ssh user say who is fetching, not what.
    let rest = match rest.split_once('@') {
        Some((user, after)) if !user.contains('/') => after,
        _ => rest,
    };
    let (host, path) = rest
        .split_once(if scp { ':' } else { '/' })
        .unwrap_or((rest, ""));
    // A port is how to reach the host, which `git@host:` never spells.
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    let path = path.trim_matches('/');
    let path = path
        .strip_suffix(".git")
        .unwrap_or(path)
        .trim_end_matches('/');
    format!("{host}/{path}")
}

/// `git -C dir`, and nothing else deciding which repository that is.
///
/// Git's own repository-selection variables outrank `-C`: with `GIT_DIR` set,
/// `git -C elsewhere init` reinitialises `$GIT_DIR`, not `elsewhere`. A git hook
/// exports them to everything it runs, so a cctop or a test run from one would
/// otherwise aim every command here at the repository being committed to —
/// which is how a test once flipped this repository to `core.bare = true`.
/// The list is `git rev-parse --local-env-vars`.
pub fn git_in(dir: &std::path::Path) -> std::process::Command {
    const LOCAL: [&str; 15] = [
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_OBJECT_DIRECTORY",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_GRAFT_FILE",
        "GIT_INDEX_FILE",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_REPLACE_REF_BASE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
        "GIT_COMMON_DIR",
    ];
    let mut git = std::process::Command::new("git");
    for var in LOCAL {
        git.env_remove(var);
    }
    git.arg("-C").arg(dir);
    git
}

/// `git worktree add` for `branch`, at `<repo>/.claude/worktrees/<branch>`.
///
/// `.claude/worktrees/` because it is where Claude Code's own `--worktree`
/// puts them, so the two never disagree about where a repository's parallel
/// checkouts live. A branch that already exists is checked out; any other is
/// created from `base`'s HEAD — the checkout the launch is into, not the main
/// one, so forking from a worktree continues that worktree's work.
///
/// The name is checked by git before it is joined onto a path: `../x` is a
/// directory outside the repository long before it is a bad branch name.
///
// ponytail: blocks the caller for the checkout — the TUI's draw loop, or one
// request's thread in the server; move the TUI's to worker::Request if a large
// repository makes it noticeable.
pub fn add_worktree(
    base: &std::path::Path,
    repo: &std::path::Path,
    branch: &str,
) -> Result<std::path::PathBuf, String> {
    let git = |args: &[&std::ffi::OsStr]| {
        git_in(base)
            .args(args)
            .output()
            .map_err(|e| format!("git: {e}"))
    };
    let fail = |out: std::process::Output| {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("git failed");
        line.trim_start_matches("fatal: ").to_string()
    };
    let check = git(&[
        "check-ref-format".as_ref(),
        "--branch".as_ref(),
        branch.as_ref(),
    ])?;
    // A dash in front would be an option to `worktree add`, and the name can
    // come from a request now, not only from someone at the keyboard.
    if !check.status.success() || branch.starts_with('-') {
        return Err(format!("{branch:?} is not a valid branch name"));
    }
    let dir = repo.join(".claude/worktrees");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // Self-ignoring, so the checkouts never show up as untracked files in
    // the repository they came from, whatever its own .gitignore says.
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, "*\n").map_err(|e| format!("{}: {e}", ignore.display()))?;
    }
    let path = dir.join(branch);
    let head = format!("refs/heads/{branch}");
    let exists = git(&[
        "show-ref".as_ref(),
        "--verify".as_ref(),
        "--quiet".as_ref(),
        head.as_ref(),
    ])?
    .status
    .success();
    let out = if exists {
        git(&[
            "worktree".as_ref(),
            "add".as_ref(),
            path.as_ref(),
            branch.as_ref(),
        ])?
    } else {
        git(&[
            "worktree".as_ref(),
            "add".as_ref(),
            "-b".as_ref(),
            branch.as_ref(),
            path.as_ref(),
        ])?
    };
    match out.status.success() {
        true => Ok(path),
        false => Err(fail(out)),
    }
}

/// Where a worktree for a launch into `dir` forks from: the checkout `dir` is
/// in, and the repository's own directory — or why there is none.
pub fn fork_point(dir: &Path) -> Result<(PathBuf, PathBuf), &'static str> {
    let (common, root) = locate(dir).ok_or("not in a git repository")?;
    Ok((root, repo_dir(&common)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_urls_normalise_to_one_spelling() {
        for url in [
            "git@github.com:o/r.git",
            "git@GitHub.com:o/r",
            "https://github.com/o/r",
            "https://github.com/o/r.git/",
            "https://user:tok@github.com/o/r.git",
            "ssh://git@github.com:22/o/r.git",
        ] {
            assert_eq!(
                normalise_url(url, Path::new("/g/repo")),
                "github.com/o/r",
                "{url}"
            );
        }
        let base = Path::new("/g/repo");
        assert_eq!(normalise_url("/srv/git/r.git", base), "/srv/git/r.git");
        assert_eq!(normalise_url("/srv/git/r/.git", base), "/srv/git/r");
        assert_eq!(normalise_url("file:///srv/git/r/", base), "/srv/git/r");
        // `git clone ./up b` stores the path as typed, `.` and all.
        assert_eq!(normalise_url("/g/./up", base), "/g/up");
        // Relative to the repository, so the same spelling in two places is
        // two repositories.
        assert_eq!(normalise_url("../upstream", base), "/g/upstream");
        assert_eq!(normalise_url("up", base), "/g/repo/up");
        assert_ne!(
            normalise_url("../up", base),
            normalise_url("../up", Path::new("/elsewhere/repo"))
        );
        assert_ne!(
            normalise_url("git@github.com:o/r", base),
            normalise_url("git@gitlab.com:o/r", base)
        );
    }

    /// One key for every worktree of a repository and every clone of its
    /// origin; another for an unrelated repository; none outside git.
    #[test]
    fn worktrees_and_clones_share_a_repo_key() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let git = |dir: &Path, args: &[&str]| {
            let out = git_in(dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        let at = |rel: &str| {
            let dir = tmp.path().join(rel);
            std::fs::create_dir_all(&dir).expect("dir");
            dir
        };
        let a = at("a");
        git(&a, &["init", "-q", "-b", "main"]);
        git(&a, &["commit", "-q", "--allow-empty", "-m", "root"]);
        git(&a, &["remote", "add", "origin", "git@github.com:o/r.git"]);
        let wt = add_worktree(&a, &a, "agent-1").expect("worktree");
        let b = at("b");
        git(&b, &["init", "-q"]);
        git(&b, &["remote", "add", "origin", "https://github.com/o/r"]);
        let lone = at("lone");
        git(&lone, &["init", "-q"]);
        let outside = at("outside");
        // A clone of a local path: git stores the path as given, `./` and all.
        git(tmp.path(), &["clone", "-q", "./lone", "lone-clone"]);
        let lone_clone = tmp.path().join("lone-clone");

        let key = |d: &Path| repo_cached(&d.to_string_lossy()).map(|r| r.key);
        assert_eq!(key(&a).as_deref(), Some("github.com/o/r"));
        assert_eq!(key(&wt), key(&a), "a worktree of the repository");
        assert_eq!(key(&b), key(&a), "a clone of the same origin");
        assert_eq!(key(&lone).as_deref(), Some(&*lone.to_string_lossy()));
        assert_eq!(key(&outside), None);
        assert_eq!(key(&lone_clone), key(&lone), "a clone of a local path");
        let root = |d: &Path| repo_cached(&d.to_string_lossy()).map(|r| r.root);
        assert_eq!(root(&wt), Some(wt.clone()), "each its own checkout");
    }

    /// Against a real repository, because what is being tested is what git
    /// accepts: a new branch forks, a taken one is refused with git's reason,
    /// and a name that would climb out of the directory never reaches a path.
    #[test]
    fn add_worktree_creates_checks_out_and_refuses() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path();
        let git = |args: &[&str]| {
            let out = git_in(repo)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "root"]);

        let path = add_worktree(repo, repo, "try-fork").expect("new branch");
        assert_eq!(path, repo.join(".claude/worktrees/try-fork"));
        assert!(path.join(".git").is_file(), "a linked worktree");
        assert!(repo.join(".claude/worktrees/.gitignore").is_file());

        let again = add_worktree(repo, repo, "try-fork").expect_err("already checked out");
        assert!(again.contains("try-fork"), "{again}");

        git(&["branch", "existing"]);
        let path = add_worktree(repo, repo, "existing").expect("existing branch");
        assert!(path.join(".git").is_file());

        let bad = add_worktree(repo, repo, "../escape").expect_err("bad name");
        assert!(bad.contains("not a valid branch name"), "{bad}");
        assert!(!repo.join(".claude/escape").exists());
    }
}
