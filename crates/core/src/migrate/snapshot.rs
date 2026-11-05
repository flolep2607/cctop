//! The sending side's git: the working tree as a commit, and that commit as a
//! bundle — without touching the person's branch, index or stash.

use std::path::{Path, PathBuf};

use super::{MOVE_REF, Repo, git, run};

/// What was snapshotted, and where it can be found again for the fence.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// The checkout root.
    pub root: PathBuf,
    pub repo: Repo,
    /// The snapshot commit.
    pub commit: String,
    /// Its tree, which the fence compares the checkout against.
    pub tree: String,
    /// The ref it is kept under until the move is over.
    pub refname: String,
}

/// The repository `cwd` is in, as the far side needs it to find a checkout
/// of it: asked before the confirmation, with nothing frozen or written.
pub fn describe(cwd: &Path) -> Result<(PathBuf, Repo), String> {
    let (_, root) = crate::worktree::locate(cwd).ok_or_else(|| {
        format!(
            "{} is not in a git repository — only files in a git checkout can travel; \
             `git init` it and commit what matters, then move it",
            cwd.display()
        )
    })?;
    let head = run(git(&root).args(["rev-parse", "--verify", "-q", "HEAD^{commit}"]))
        .ok()
        .filter(|h| !h.is_empty())
        .ok_or("the repository has no commits yet — commit once, then move it")?;
    let branch = run(git(&root).args(["symbolic-ref", "-q", "--short", "HEAD"]))
        .ok()
        .filter(|b| !b.is_empty());
    let origin = run(git(&root).args(["config", "--get", "remote.origin.url"]))
        .ok()
        .filter(|o| !o.is_empty());
    let key = crate::worktree::repo_cached(&root.to_string_lossy())
        .map(|r| r.key)
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".into());
    let rel_cwd = cwd
        .strip_prefix(&root)
        .map(|r| {
            r.iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let repo = Repo {
        origin,
        key,
        name,
        head,
        branch,
        rel_cwd,
    };
    Ok((root, repo))
}

/// Snapshot the checkout `cwd` is in, under `refs/cctop/move/<move_id>`.
///
/// The working tree is added to a temporary index — the one in `.git` is the
/// person's, and what they staged stays staged — so tracked changes and
/// untracked files are in it and ignored files are not, exactly as
/// `git add -A` would have them. The commit's parent is HEAD, so the far side
/// can tell which of it is history and which is the work in progress.
pub fn take(cwd: &Path, move_id: &str) -> Result<Snapshot, String> {
    let (root, repo) = describe(cwd)?;
    let tree = tree_of(&root)?;
    let commit = run(git(&root).args([
        "commit-tree",
        &tree,
        "-p",
        &repo.head,
        "-m",
        "cctop: the working tree, moved to another machine",
    ]))?;
    let refname = format!("{MOVE_REF}/{move_id}");
    run(git(&root).args(["update-ref", &refname, &commit]))?;
    Ok(Snapshot {
        root,
        repo,
        commit,
        tree,
        refname,
    })
}

/// The tree the checkout would be if everything in it were added: what the
/// snapshot holds, and what the fence asks again before the agent here is
/// stopped.
pub fn tree_of(root: &Path) -> Result<String, String> {
    let tmp = tempfile::tempdir().map_err(|e| format!("a temporary index: {e}"))?;
    let index = tmp.path().join("index");
    let with_index = |args: &[&str]| {
        let mut cmd = git(root);
        cmd.env("GIT_INDEX_FILE", &index).args(args);
        run(&mut cmd)
    };
    with_index(&["read-tree", "HEAD"])?;
    with_index(&["add", "-A", "--", "."])?;
    with_index(&["write-tree"])
}

/// Write the bundle the far side needs to `out`.
///
/// Thin when the far repository already has HEAD: only the snapshot and what
/// it adds. Otherwise the branch's whole history goes too, so a fresh clone
/// there has it.
pub fn bundle(snap: &Snapshot, has_head: bool, out: &Path) -> Result<(), String> {
    let mut cmd = git(&snap.root);
    cmd.args(["bundle", "create", "-q"])
        .arg(out)
        .arg(&snap.refname);
    if has_head {
        cmd.arg(format!("^{}", snap.repo.head));
    } else if let Some(branch) = &snap.repo.branch {
        cmd.arg(format!("refs/heads/{branch}"));
    }
    run(&mut cmd).map(|_| ())
}

/// Drop the snapshot's ref; the objects go with the next gc.
pub fn forget(snap: &Snapshot) {
    let _ = run(git(&snap.root).args(["update-ref", "-d", &snap.refname]));
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A repository with one commit, as a person would have it.
    pub(crate) fn repo(dir: &Path) -> PathBuf {
        let root = dir.to_path_buf();
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let out = crate::worktree::git_in(&root)
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        std::fs::write(root.join(".gitignore"), "secret.env\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        root
    }

    pub(crate) fn git_out(root: &Path, args: &[&str]) -> String {
        let out = crate::worktree::git_in(root).args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// The snapshot has the work in progress — a changed file, a new one —
    /// and not the ignored one, and leaves what the person staged, their
    /// branch and their stash exactly as they were.
    #[test]
    fn a_snapshot_takes_the_work_and_touches_nothing_of_the_persons() {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo(&tmp.path().join("app"));
        std::fs::write(root.join("a.txt"), "two\n").unwrap();
        std::fs::write(root.join("new.txt"), "fresh\n").unwrap();
        std::fs::write(root.join("secret.env"), "TOKEN=x\n").unwrap();
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let status_before = git_out(&root, &["status", "--porcelain"]);
        let head_before = git_out(&root, &["rev-parse", "HEAD"]);

        let snap = take(&root.join("sub"), "m1").unwrap();

        assert_eq!(git_out(&root, &["status", "--porcelain"]), status_before);
        assert_eq!(git_out(&root, &["rev-parse", "HEAD"]), head_before);
        assert_eq!(git_out(&root, &["stash", "list"]), "");
        assert_eq!(snap.repo.head, head_before);
        assert_eq!(snap.repo.branch.as_deref(), Some("main"));
        assert_eq!(snap.repo.rel_cwd, "sub");
        let files = git_out(&root, &["ls-tree", "-r", "--name-only", &snap.commit]);
        assert!(files.contains("new.txt"), "{files}");
        assert!(!files.contains("secret.env"), "{files}");
        assert_eq!(
            git_out(&root, &["show", &format!("{}:a.txt", snap.commit)]),
            "two"
        );
        assert_eq!(tree_of(&root).unwrap(), snap.tree);
    }

    /// A thin bundle needs HEAD on the far side; a full one carries the branch.
    #[test]
    fn a_bundle_is_thin_only_when_the_far_side_has_the_history() {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo(&tmp.path().join("app"));
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        let snap = take(&root, "m2").unwrap();
        let thin = tmp.path().join("thin.bundle");
        let full = tmp.path().join("full.bundle");
        bundle(&snap, true, &thin).unwrap();
        bundle(&snap, false, &full).unwrap();
        let heads = |b: &Path| git_out(&root, &["bundle", "list-heads", &b.to_string_lossy()]);
        assert!(heads(&thin).contains("refs/cctop/move/m2"));
        assert!(!heads(&thin).contains("refs/heads/main"));
        assert!(heads(&full).contains("refs/heads/main"));
    }

    #[test]
    fn a_directory_outside_git_is_refused_with_what_to_do() {
        let tmp = tempfile::tempdir().unwrap();
        let why = take(tmp.path(), "m3").unwrap_err();
        assert!(why.contains("git init"), "{why}");
    }
}
