//! Git checkouts a launch can start in: where a directory's repository is,
//! and a new worktree of it for an agent to work in apart.
//!
//! Here rather than in the terminal UI because both launchers make one: the
//! TUI's `F` and harness options, and the web launcher's branch field, which
//! the server answers. One `git worktree add`, so the two put a branch's
//! checkout in the same place.

use std::path::{Path, PathBuf};

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
/// created from `base`'s HEAD — the checkout `F` was pressed on, not the main
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
