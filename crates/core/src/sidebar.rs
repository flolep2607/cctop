//! What an agent tab's sidebar shows about its session — the plan, and the
//! files the session changed — read by the machine that has the session.
//!
//! In core rather than the terminal UI because it is read in two places: by
//! the TUI for an agent on this machine, and by the cctop on another machine
//! for a tab there whose agent runs here (`cctop --sidebar`, `/api/sidebar`).
//! The document is the same either way, so the far side's sidebar is this
//! one's, carried across.
//!
//! A file's diff is read the same way, one file at a time when it is opened —
//! [`diff`] — and only for a file this session's list names. A path from
//! another machine is a request, and the list is the whole of what that
//! request may name: not "anything inside the checkout", which would make this
//! a way to read any file in any repository a session ever ran in.

use std::path::PathBuf;

use crate::diff_file::FileDiff;
use crate::plan::Step;
use crate::session::{Session, SessionData};

/// One changed file, as the sidebar lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct File {
    /// Relative to the checkout, or to the session's directory.
    pub path: String,
    /// `None` for a binary file, which has no lines to count.
    pub added: Option<u64>,
    pub removed: Option<u64>,
    /// Never added to git: its diff is the whole file, read from disk.
    pub untracked: bool,
}

/// The sidebar's document for one session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Sidebar {
    pub plan: Vec<Step>,
    /// The files, or a sentence saying why there are none to show.
    pub files: Result<Vec<File>, String>,
    /// Files past [`MAX_FILES`] that were left off the list.
    #[serde(default)]
    pub more: usize,
    /// The checkout the files were read from and the commit they are against,
    /// which is what a click on one diffs; `None` when they came from the
    /// transcript, which has no checkout to diff. A path on *this* machine, so
    /// it never crosses to another: a diff from there is asked of [`diff`].
    #[serde(skip)]
    pub checkout: Option<(PathBuf, String)>,
}

/// The most files a sidebar document carries. A session that touched more is
/// a checkout the tree cannot show usefully anyway, and the cap keeps the
/// answer another machine waits for small.
pub const MAX_FILES: usize = 2000;

/// The sidebar for `session`, from its parsed transcript.
///
/// The harness's own to-do tool first, as the agent's working record; what it
/// published through `set_plan` when it kept none.
pub fn of(session: &Session, data: &SessionData) -> Sidebar {
    let mut sidebar = read(
        data,
        crate::changes::checkout_of(session),
        crate::changes::since(session),
        &session.label_source,
    );
    if sidebar.plan.is_empty() {
        sidebar.plan = crate::plan::published(session.provider.as_str(), &session.session_id);
    }
    sidebar
}

/// The changed files and the plan, from `data` and the checkout at `dir`.
///
/// The checkout's own diff when there is one, as the Changes panel shows it:
/// that is what is actually on disk. Outside a repository — or where the
/// checkout cannot be read — the transcript's edits stand in, which at least
/// name the files.
pub fn read(
    data: &SessionData,
    dir: Result<PathBuf, String>,
    started: Option<i64>,
    cwd: &str,
) -> Sidebar {
    let details = &data.metrics.tool_details;
    let plan = crate::plan::latest(details);
    let edited = || -> Vec<File> {
        let root = format!("{}/", cwd.trim_end_matches('/'));
        crate::handoff::touched_files(details)
            .into_iter()
            .map(|t| File {
                path: t.path.strip_prefix(&root).unwrap_or(&t.path).to_string(),
                added: Some(t.added),
                removed: Some(t.removed),
                untracked: false,
            })
            .collect()
    };
    let mut checkout = None;
    let files = match dir.map(|dir| crate::changes::read(&dir, started)) {
        Ok(Ok(Some(changes))) => {
            checkout = Some((changes.root.clone(), changes.base.commit.clone()));
            Ok(changes
                .files
                .into_iter()
                .map(|f| File {
                    path: f.path,
                    added: f.added,
                    removed: f.removed,
                    untracked: f.untracked,
                })
                .collect())
        }
        Ok(Ok(None)) => Ok(edited()),
        Ok(Err(why)) | Err(why) => match edited() {
            files if files.is_empty() => Err(why),
            files => Ok(files),
        },
    };
    let (files, more) = capped(files);
    Sidebar {
        plan,
        files,
        more,
        checkout,
    }
}

/// `files` cut to [`MAX_FILES`], and how many were cut.
fn capped(files: Result<Vec<File>, String>) -> (Result<Vec<File>, String>, usize) {
    match files {
        Ok(mut files) if files.len() > MAX_FILES => {
            let more = files.len() - MAX_FILES;
            files.truncate(MAX_FILES);
            (Ok(files), more)
        }
        other => (other, 0),
    }
}

/// Both versions of a file another machine asked for, past this many bytes
/// together, are left out: the hunks still show, and only the unchanged
/// stretches between them cannot be opened. Read here, every byte of a 4 MB
/// file twice over would cross a tunnel for a click.
const MAX_REMOTE_VERSIONS: usize = 1 << 20;

/// One file's diff in `session`'s checkout, for another machine's diff view.
///
/// Only a file the session's sidebar lists, against the same base: the list
/// is read again here rather than trusted from the request, so a path the
/// session did not change is refused whatever it names.
pub fn diff(session: &Session, data: &SessionData, path: &str) -> Result<FileDiff, String> {
    let sidebar = of(session, data);
    let Some((root, base)) = sidebar.checkout else {
        return Err("this session's files are not in a checkout to diff".into());
    };
    let files = sidebar.files.unwrap_or_default();
    let Some(file) = files.iter().find(|f| f.path == path) else {
        return Err(format!("{path:?} is not a file this session changed"));
    };
    let mut diff = crate::diff_file::read(&root, &base, path, file.untracked)?;
    let size = |v: &Option<Vec<String>>| v.iter().flatten().map(|l| l.len() + 1).sum::<usize>();
    if size(&diff.old) + size(&diff.new) > MAX_REMOTE_VERSIONS {
        diff.old = None;
        diff.new = None;
    }
    Ok(diff)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &std::path::Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A checkout with one tracked file changed, and a session working in it.
    fn session_in_repo() -> (tempfile::TempDir, Session) {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\n").unwrap();
        git(dir.path(), &["add", "a.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "x"]);
        std::fs::write(dir.path().join("a.txt"), "one\nTWO\n").unwrap();
        let mut session = Session::new(crate::pricing::Provider::Claude, "s1".into());
        session.label_source = dir.path().to_string_lossy().into_owned();
        (dir, session)
    }

    /// A long list is cut, and says by how much.
    #[test]
    fn a_sidebar_carries_at_most_max_files() {
        let files: Vec<File> = (0..MAX_FILES + 7)
            .map(|i| File {
                path: format!("f{i}"),
                ..File::default()
            })
            .collect();
        let (kept, more) = capped(Ok(files));
        assert_eq!(kept.unwrap().len(), MAX_FILES);
        assert_eq!(more, 7);
        assert_eq!(capped(Err("x".into())).1, 0);
    }

    /// The document crosses as JSON and comes back the same, without the
    /// checkout's local path.
    #[test]
    fn a_sidebar_crosses_without_its_local_path() {
        let (_dir, session) = session_in_repo();
        let sidebar = of(&session, &SessionData::default());
        assert!(sidebar.checkout.is_some());
        let back: Sidebar =
            serde_json::from_str(&serde_json::to_string(&sidebar).unwrap()).unwrap();
        assert_eq!(back.files, sidebar.files);
        assert!(back.checkout.is_none());
    }

    /// Another machine gets a diff only for a file the session changed: not
    /// for another file in the checkout, not for one outside it, and not
    /// through a link that points out of it.
    #[test]
    fn only_a_listed_file_is_diffed_for_another_machine() {
        let (dir, session) = session_in_repo();
        let data = SessionData::default();
        let diff = diff(&session, &data, "a.txt").expect("a listed file");
        assert!(diff.diff.iter().any(|l| l == "+TWO"), "{:?}", diff.diff);

        std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
        git(dir.path(), &["add", "b.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "b"]);
        for path in ["b.txt", "../etc/passwd", "/etc/passwd", ""] {
            let err = super::diff(&session, &data, path).expect_err(path);
            assert!(
                err.contains("not a file this session changed"),
                "{path}: {err}"
            );
        }
    }

    /// A link in the checkout that points out of it reads as nothing, even
    /// when it is a file the session lists.
    #[test]
    fn a_link_out_of_the_checkout_is_not_read() {
        let (dir, session) = session_in_repo();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "key\n").unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("leak")).unwrap();
        let read = crate::diff_file::read(dir.path(), "HEAD", "leak", true);
        assert!(read.is_err(), "{read:?}");
        let _ = session;
    }
}
