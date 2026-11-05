//! The receiving side's git: which checkout a moved session lands in, and
//! putting its files there — every change written into the journal first, so
//! a failure anywhere after can put the checkout back as it was.
//!
//! Three ways to land, chosen by [`propose`] and re-checked by [`apply`]:
//!
//! - **Existing**: a checkout of the same repository that is clean and that
//!   nothing is running in. It is switched to the session's branch — only
//!   forward; a branch that went its own way here is left alone and the
//!   checkout detached — and the work in progress written over it, unstaged.
//! - **Worktree**: the same, when the checkout has work of its own or
//!   something running in it. That work is not cctop's to move aside, so the
//!   session gets a worktree of its own beside it.
//! - **Clone**: no checkout of it here. A new repository is made from the
//!   bundle, with the original's `origin` so it can fetch and push.

use std::path::{Path, PathBuf};

use super::receive::{Journal, Undo};
use super::{Ask, Kind, MOVE_REF, Manifest, Placement, git, plain_relative, run};

/// Where `ask` would land under `home`.
pub fn propose(ask: &Ask, home: &Path) -> Result<Placement, String> {
    if let Some(typed) = ask.dir.as_deref().filter(|d| !d.trim().is_empty()) {
        let dir = PathBuf::from(crate::util::untildify(typed.trim()));
        if !dir.is_absolute() {
            return Err(format!("{typed} is not an absolute path"));
        }
        return propose_at(ask, &dir);
    }
    let same = crate::repos::repos_under(home)
        .into_iter()
        .find(|root| key_of(root).as_deref() == Some(ask.repo.key.as_str()));
    if let Some(root) = same {
        return evaluate(ask, &root);
    }
    let name = safe_name(&ask.repo.name);
    let dir = free_dir(&home.join(&name));
    Ok(Placement {
        kind: Kind::Clone,
        dir: dir.to_string_lossy().into_owned(),
        root: None,
        has_head: false,
        note: Some(format!(
            "no checkout of {} here — a new clone",
            ask.repo.key
        )),
    })
}

/// A directory the person chose: a checkout of the same repository, or
/// somewhere empty to clone into. Anything else is somebody's directory.
fn propose_at(ask: &Ask, dir: &Path) -> Result<Placement, String> {
    if empty_or_missing(dir) {
        return Ok(Placement {
            kind: Kind::Clone,
            dir: dir.to_string_lossy().into_owned(),
            root: None,
            has_head: false,
            note: None,
        });
    }
    match key_of(dir) {
        Some(key) if key == ask.repo.key && is_root(dir) => evaluate(ask, dir),
        _ => Err(format!(
            "{} is neither empty nor a checkout of {}",
            dir.display(),
            ask.repo.key
        )),
    }
}

/// An existing checkout of the repository: into it, or beside it.
fn evaluate(ask: &Ask, root: &Path) -> Result<Placement, String> {
    let has_head = has_commit(root, &ask.repo.head);
    let busy = in_use(root);
    let dirty = !clean(root)?;
    if dirty || busy {
        let dir = free_dir(&PathBuf::from(format!("{}-moved", root.display())));
        let why = match busy {
            true => "something is running in it",
            false => "it has work of its own in it",
        };
        return Ok(Placement {
            kind: Kind::Worktree,
            dir: dir.to_string_lossy().into_owned(),
            root: Some(root.to_string_lossy().into_owned()),
            has_head,
            note: Some(format!(
                "{} is the checkout, but {why} — a worktree beside it",
                root.display()
            )),
        });
    }
    let note = ask.repo.branch.as_deref().and_then(|b| {
        let local = branch_tip(root, b)?;
        (!forward(root, &local, &ask.repo.head)).then(|| {
            format!("branch {b} went its own way here — the checkout will be left detached")
        })
    });
    Ok(Placement {
        kind: Kind::Existing,
        dir: root.to_string_lossy().into_owned(),
        root: None,
        has_head,
        note,
    })
}

/// Put the bundle's snapshot in place as `m.placement` says, and answer the
/// directory the session resumes in.
pub fn apply(m: &Manifest, bundle: &Path, journal: &mut Journal) -> Result<PathBuf, String> {
    if !plain_relative(&m.repo.rel_cwd) {
        return Err("the session's directory walks out of its checkout".into());
    }
    let dir = PathBuf::from(&m.placement.dir);
    if !dir.is_absolute() {
        return Err("the directory to land in is not an absolute path".into());
    }
    let move_ref = format!("{MOVE_REF}/{}", m.move_id);
    let fetch_into = |repo: &Path, journal: &mut Journal| -> Result<(), String> {
        run(git(repo).args(["bundle", "verify", "-q"]).arg(bundle))?;
        run(git(repo)
            .args(["fetch", "-q", "--no-tags"])
            .arg(bundle)
            .arg(format!("{move_ref}:{move_ref}")))?;
        journal.push(Undo::DeleteRef {
            repo: repo.to_path_buf(),
            refname: move_ref.clone(),
        })
    };
    let snap = &m.snapshot;
    let head = &m.repo.head;
    let mut note = m.placement.note.clone();
    let work = match m.placement.kind {
        Kind::Clone => {
            if !empty_or_missing(&dir) {
                return Err(format!("{} is no longer empty", dir.display()));
            }
            match dir.exists() {
                true => journal.push(Undo::EmptyDir(dir.clone()))?,
                false => {
                    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                    journal.push(Undo::RemoveDir(dir.clone()))?;
                }
            }
            run(git(&dir).args(["init", "-q"]))?;
            fetch_into(&dir, journal)?;
            if let Some(origin) = m.repo.origin.as_deref() {
                match origin_ok(origin) {
                    true => {
                        run(git(&dir).args(["remote", "add", "origin", origin]))?;
                    }
                    false => note = Some("its origin URL was not one cctop sets".into()),
                }
            }
            match m.repo.branch.as_deref().filter(|b| branch_name_ok(b)) {
                Some(b) => run(git(&dir).args(["switch", "-q", "-c", b, head]))?,
                None => run(git(&dir).args(["switch", "-q", "--detach", head]))?,
            };
            dir.clone()
        }
        Kind::Worktree => {
            let root = PathBuf::from(m.placement.root.as_deref().ok_or("no checkout named")?);
            check_checkout(&root, &m.repo.key)?;
            if dir.exists() {
                return Err(format!("{} is already there", dir.display()));
            }
            fetch_into(&root, journal)?;
            run(git(&root)
                .args(["worktree", "add", "-q", "--detach"])
                .arg(&dir)
                .arg(head))?;
            journal.push(Undo::RemoveWorktree {
                repo: root.clone(),
                dir: dir.clone(),
            })?;
            dir.clone()
        }
        Kind::Existing => {
            check_checkout(&dir, &m.repo.key)?;
            // Asked again now rather than trusted from the plan: the person
            // may have spent a minute on the confirmation.
            if !clean(&dir)? || in_use(&dir) {
                return Err(format!(
                    "{} has changed since it was offered — move it again",
                    dir.display()
                ));
            }
            fetch_into(&dir, journal)?;
            let prev = run(git(&dir).args(["rev-parse", "HEAD"]))?;
            let prev_branch = run(git(&dir).args(["symbolic-ref", "-q", "--short", "HEAD"]))
                .ok()
                .filter(|b| !b.is_empty());
            let added = added_paths(&dir, &prev, snap)?;
            let mut branch_was = None;
            match m.repo.branch.as_deref().filter(|b| branch_name_ok(b)) {
                Some(b) => match branch_tip(&dir, b) {
                    Some(local) if !forward(&dir, &local, head) => {
                        run(git(&dir).args(["switch", "-q", "--detach", head]))?;
                        note = Some(format!(
                            "branch {b} went its own way here — left detached at the session's commit"
                        ));
                    }
                    local => {
                        branch_was = Some((b.to_string(), local));
                        run(git(&dir).args(["switch", "-q", "-C", b, head]))?;
                    }
                },
                None => {
                    run(git(&dir).args(["switch", "-q", "--detach", head]))?;
                }
            }
            journal.push(Undo::SwitchBack {
                root: dir.clone(),
                prev,
                prev_branch,
                branch_was,
                added,
            })?;
            dir.clone()
        }
    };
    run(git(&work).args([
        "restore",
        "-q",
        &format!("--source={snap}"),
        "--worktree",
        "--",
        ".",
    ]))?;
    let cwd = match m.repo.rel_cwd.is_empty() {
        true => work,
        false => work.join(&m.repo.rel_cwd),
    };
    // An empty directory is not in a tree, and the session may have been in one.
    let _ = std::fs::create_dir_all(&cwd);
    if let Some(note) = note {
        journal.note(note);
    }
    Ok(cwd)
}

/// The files the snapshot adds that were not on disk before: what an undo
/// removes after switching back, and nothing else — a file of the person's
/// that git ignores is never one of them.
fn added_paths(root: &Path, prev: &str, snap: &str) -> Result<Vec<String>, String> {
    let list = |rev: &str| -> Result<std::collections::HashSet<String>, String> {
        Ok(
            run(git(root).args(["ls-tree", "-r", "-z", "--name-only", rev]))?
                .split('\0')
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect(),
        )
    };
    let before = list(prev)?;
    Ok(list(snap)?
        .into_iter()
        .filter(|p| !before.contains(p) && !root.join(p).exists())
        .collect())
}

/// The checkout is still the repository it was offered as.
fn check_checkout(root: &Path, key: &str) -> Result<(), String> {
    match key_of(root) {
        Some(k) if k == key && is_root(root) => Ok(()),
        _ => Err(format!(
            "{} is no longer a checkout of {key}",
            root.display()
        )),
    }
}

fn key_of(dir: &Path) -> Option<String> {
    let (common, _) = crate::worktree::locate(dir)?;
    let cfg = run(git(dir).args(["config", "--get", "remote.origin.url"])).ok();
    Some(match cfg.filter(|u| !u.is_empty()) {
        Some(url) => crate::worktree::normalise_url(&url, &crate::worktree::repo_dir(&common)),
        None => crate::worktree::repo_dir(&common)
            .to_string_lossy()
            .into_owned(),
    })
}

fn is_root(dir: &Path) -> bool {
    crate::worktree::locate(dir).is_some_and(|(_, root)| root == dir)
}

fn clean(root: &Path) -> Result<bool, String> {
    Ok(run(git(root).args(["status", "--porcelain", "--untracked-files=normal"]))?.is_empty())
}

fn has_commit(root: &Path, sha: &str) -> bool {
    run(git(root).args(["cat-file", "-e", &format!("{sha}^{{commit}}")])).is_ok()
}

fn branch_tip(root: &Path, branch: &str) -> Option<String> {
    run(git(root).args([
        "rev-parse",
        "--verify",
        "-q",
        &format!("refs/heads/{branch}"),
    ]))
    .ok()
    .filter(|s| !s.is_empty())
}

/// Moving `from` to `to` loses nothing: it is the same commit, or behind it.
/// A `to` this repository does not have yet counts as not forward — the plan
/// is asked before the bundle arrives, and is asked again after.
fn forward(root: &Path, from: &str, to: &str) -> bool {
    from == to || run(git(root).args(["merge-base", "--is-ancestor", from, to])).is_ok()
}

/// Whether any process of this user has its working directory in `root`: an
/// agent, an editor, a shell. Switching the branch under any of them is a
/// surprise nobody asked for.
fn in_use(root: &Path) -> bool {
    let me = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|e| {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            return false;
        };
        pid != me && std::fs::read_link(e.path().join("cwd")).is_ok_and(|cwd| cwd.starts_with(root))
    })
}

fn empty_or_missing(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
}

/// `dir`, or `dir-2`, `dir-3`… — the first that does not exist.
fn free_dir(dir: &Path) -> PathBuf {
    if !dir.exists() {
        return dir.to_path_buf();
    }
    (2..100)
        .map(|n| PathBuf::from(format!("{}-{n}", dir.display())))
        .find(|d| !d.exists())
        .unwrap_or_else(|| dir.to_path_buf())
}

/// A directory name from the sender, made safe to join under the home.
fn safe_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    match clean.trim_start_matches('.') {
        "" => "project".into(),
        ok => ok.into(),
    }
}

/// An origin cctop will write into a new clone's config: https, ssh or the
/// scp form. Never `ext::` (a command), `file://` or a path (somewhere on this
/// disk), a leading dash (an option), or control characters.
pub fn origin_ok(url: &str) -> bool {
    if url.starts_with('-') || url.chars().any(char::is_control) || url.contains("::") {
        return false;
    }
    if url.starts_with("https://") || url.starts_with("ssh://") {
        return true;
    }
    // `user@host:path`: a colon before any slash, and something on each side.
    match url.split_once(':') {
        Some((host, path)) => {
            !host.is_empty() && !host.contains('/') && !path.is_empty() && !path.starts_with("//")
        }
        None => false,
    }
}

/// A branch name git would accept, and that cannot be read as an option.
fn branch_name_ok(b: &str) -> bool {
    !b.is_empty()
        && !b.starts_with('-')
        && !b.contains("..")
        && b.chars()
            .all(|c| !c.is_control() && !" ~^:?*[\\".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrate::snapshot::tests::{git_out, repo};
    use crate::migrate::{Blob, Harness, snapshot};

    const ORIGIN: &str = "https://example.com/o/app.git";

    /// A source checkout with work in progress, and its bundle.
    struct Source {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        snap: snapshot::Snapshot,
        bundle: PathBuf,
    }

    fn source(full: bool) -> Source {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo(&tmp.path().join("app"));
        git_out(&root, &["remote", "add", "origin", ORIGIN]);
        std::fs::write(root.join("a.txt"), "changed here\n").unwrap();
        std::fs::write(root.join("new.txt"), "brand new\n").unwrap();
        let snap = snapshot::take(&root, "mv1").unwrap();
        let bundle = tmp.path().join("b.bundle");
        snapshot::bundle(&snap, !full, &bundle).unwrap();
        Source {
            _tmp: tmp,
            root,
            snap,
            bundle,
        }
    }

    fn ask(src: &Source, dir: Option<&Path>) -> Ask {
        Ask {
            protocol: crate::migrate::PROTOCOL,
            harness: Harness::Claude,
            repo: src.snap.repo.clone(),
            dir: dir.map(|d| d.to_string_lossy().into_owned()),
        }
    }

    fn manifest(src: &Source, placement: Placement) -> Manifest {
        let none = Blob {
            len: 0,
            sha256: String::new(),
        };
        Manifest {
            protocol: crate::migrate::PROTOCOL,
            harness: Harness::Claude,
            session_id: "s".into(),
            repo: src.snap.repo.clone(),
            placement,
            snapshot: src.snap.commit.clone(),
            move_id: "mv1".into(),
            bundle: none.clone(),
            transcript: none,
            transcript_meta: crate::migrate::transcript::Meta {
                old_cwd: String::new(),
                codex_dated: None,
                codex_prefix: None,
            },
        }
    }

    fn journal(dir: &Path) -> Journal {
        Journal::new(dir.join("journal.json"))
    }

    /// No checkout of it there: a new one, on the branch, with the work in
    /// progress unstaged and the origin set; undone, nothing is left.
    #[test]
    fn with_no_checkout_there_it_is_cloned_and_undone_cleanly() {
        let src = source(true);
        let home = tempfile::tempdir().unwrap();
        let placement = propose(&ask(&src, None), home.path()).unwrap();
        assert_eq!(placement.kind, Kind::Clone);
        assert_eq!(PathBuf::from(&placement.dir), home.path().join("app"));
        let mut j = journal(home.path());
        let cwd = apply(&manifest(&src, placement), &src.bundle, &mut j).unwrap();
        assert_eq!(
            std::fs::read_to_string(cwd.join("a.txt")).unwrap(),
            "changed here\n"
        );
        assert_eq!(
            std::fs::read_to_string(cwd.join("new.txt")).unwrap(),
            "brand new\n"
        );
        assert_eq!(git_out(&cwd, &["symbolic-ref", "--short", "HEAD"]), "main");
        assert_eq!(git_out(&cwd, &["rev-parse", "HEAD"]), src.snap.repo.head);
        assert_eq!(git_out(&cwd, &["config", "remote.origin.url"]), ORIGIN);
        assert!(git_out(&cwd, &["status", "--porcelain"]).contains("a.txt"));
        assert!(j.undo_all().is_empty());
        assert!(!home.path().join("app").exists());
    }

    /// A clean checkout of the same repository is used as it is, moved to the
    /// session's branch and work — and put back exactly by an undo.
    #[test]
    fn a_clean_checkout_there_takes_the_work_and_gives_it_back() {
        let src = source(false);
        let home = tempfile::tempdir().unwrap();
        let there = home.path().join("code/app");
        std::fs::create_dir_all(home.path().join("code")).unwrap();
        let out = crate::worktree::git_in(home.path())
            .args(["clone", "-q"])
            .arg(&src.root)
            .arg(&there)
            .output()
            .unwrap();
        assert!(out.status.success());
        git_out(&there, &["remote", "set-url", "origin", ORIGIN]);
        git_out(&there, &["switch", "-q", "-c", "other"]);
        let placement = propose(&ask(&src, None), home.path()).unwrap();
        assert_eq!(placement.kind, Kind::Existing, "{placement:?}");
        assert!(placement.has_head);
        let mut j = journal(home.path());
        let cwd = apply(&manifest(&src, placement), &src.bundle, &mut j).unwrap();
        assert_eq!(cwd, there);
        assert_eq!(
            git_out(&there, &["symbolic-ref", "--short", "HEAD"]),
            "main"
        );
        assert_eq!(
            std::fs::read_to_string(there.join("new.txt")).unwrap(),
            "brand new\n"
        );
        assert!(j.undo_all().is_empty());
        assert_eq!(
            git_out(&there, &["symbolic-ref", "--short", "HEAD"]),
            "other"
        );
        assert_eq!(git_out(&there, &["status", "--porcelain"]), "");
        assert!(!there.join("new.txt").exists());
    }

    /// A checkout with work of its own is not touched: a worktree beside it.
    #[test]
    fn a_dirty_checkout_gets_a_worktree_beside_it() {
        let src = source(false);
        let home = tempfile::tempdir().unwrap();
        let there = home.path().join("app");
        let out = crate::worktree::git_in(home.path())
            .args(["clone", "-q"])
            .arg(&src.root)
            .arg(&there)
            .output()
            .unwrap();
        assert!(out.status.success());
        git_out(&there, &["remote", "set-url", "origin", ORIGIN]);
        std::fs::write(there.join("mine.txt"), "theirs\n").unwrap();
        let placement = propose(&ask(&src, None), home.path()).unwrap();
        assert_eq!(placement.kind, Kind::Worktree, "{placement:?}");
        let mut j = journal(home.path());
        let cwd = apply(&manifest(&src, placement), &src.bundle, &mut j).unwrap();
        assert_eq!(cwd, home.path().join("app-moved"));
        assert_eq!(
            std::fs::read_to_string(cwd.join("new.txt")).unwrap(),
            "brand new\n"
        );
        assert_eq!(
            std::fs::read_to_string(there.join("mine.txt")).unwrap(),
            "theirs\n"
        );
        assert!(j.undo_all().is_empty());
        assert!(!cwd.exists());
    }

    /// The branch moved on there in its own direction: it is left where it is
    /// and the checkout detached, with a note saying so.
    #[test]
    fn a_branch_that_diverged_there_is_left_alone() {
        let src = source(false);
        let home = tempfile::tempdir().unwrap();
        let there = home.path().join("app");
        let out = crate::worktree::git_in(home.path())
            .args(["clone", "-q"])
            .arg(&src.root)
            .arg(&there)
            .output()
            .unwrap();
        assert!(out.status.success());
        git_out(&there, &["remote", "set-url", "origin", ORIGIN]);
        std::fs::write(there.join("c.txt"), "c\n").unwrap();
        git_out(&there, &["add", "-A"]);
        git_out(
            &there,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "-m",
                "theirs",
            ],
        );
        let tip = git_out(&there, &["rev-parse", "main"]);
        let placement = propose(&ask(&src, None), home.path()).unwrap();
        assert!(
            placement
                .note
                .as_deref()
                .is_some_and(|n| n.contains("detached"))
        );
        let mut j = journal(home.path());
        apply(&manifest(&src, placement), &src.bundle, &mut j).unwrap();
        assert_eq!(git_out(&there, &["rev-parse", "main"]), tip);
        assert_eq!(git_out(&there, &["rev-parse", "HEAD"]), src.snap.repo.head);
        assert!(j.undo_all().is_empty());
        assert_eq!(
            git_out(&there, &["symbolic-ref", "--short", "HEAD"]),
            "main"
        );
    }

    /// The receiving repository's own hooks and fsmonitor never run.
    #[test]
    fn the_checkouts_hooks_and_fsmonitor_never_run() {
        let src = source(false);
        let home = tempfile::tempdir().unwrap();
        let there = home.path().join("app");
        let out = crate::worktree::git_in(home.path())
            .args(["clone", "-q"])
            .arg(&src.root)
            .arg(&there)
            .output()
            .unwrap();
        assert!(out.status.success());
        git_out(&there, &["remote", "set-url", "origin", ORIGIN]);
        let ran = home.path().join("ran");
        let script = format!("#!/bin/sh\ntouch {}\n", ran.display());
        let hooks = there.join(".git/hooks");
        for hook in ["post-checkout", "reference-transaction", "post-merge"] {
            let path = hooks.join(hook);
            std::fs::write(&path, &script).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let monitor = home.path().join("monitor.sh");
        std::fs::write(&monitor, &script).unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&monitor, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        git_out(
            &there,
            &["config", "core.fsmonitor", &monitor.to_string_lossy()],
        );
        let placement = propose(&ask(&src, None), home.path()).unwrap();
        let mut j = journal(home.path());
        apply(&manifest(&src, placement), &src.bundle, &mut j).unwrap();
        j.undo_all();
        assert!(!ran.exists(), "a hook or fsmonitor ran");
    }

    /// A directory the person typed has to be empty or the same repository.
    #[test]
    fn a_typed_directory_that_is_someone_elses_is_refused() {
        let src = source(false);
        let home = tempfile::tempdir().unwrap();
        let other = home.path().join("notes");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("x"), "x").unwrap();
        assert!(propose(&ask(&src, Some(&other)), home.path()).is_err());
        let empty = home.path().join("fresh");
        let p = propose(&ask(&src, Some(&empty)), home.path()).unwrap();
        assert_eq!(p.kind, Kind::Clone);
        assert!(propose(&ask(&src, Some(Path::new("relative/x"))), home.path()).is_err());
    }

    #[test]
    fn only_a_fetchable_origin_is_set() {
        for good in [
            "https://github.com/o/r.git",
            "ssh://git@host:22/o/r",
            "git@github.com:o/r.git",
        ] {
            assert!(origin_ok(good), "{good}");
        }
        for bad in [
            "ext::sh -c touch% /tmp/pwned",
            "file:///etc",
            "/srv/repo",
            "../up",
            "-uhttps://x",
            "https://x\ny",
            "host:",
        ] {
            assert!(!origin_ok(bad), "{bad}");
        }
    }

    #[test]
    fn a_name_from_the_sender_is_a_plain_directory_name() {
        assert_eq!(safe_name("cctop"), "cctop");
        assert_eq!(safe_name("../../etc"), "etc");
        assert_eq!(safe_name("..."), "project");
        assert_eq!(safe_name("a b/c"), "abc");
    }
}
