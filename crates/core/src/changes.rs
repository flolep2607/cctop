//! What a session did to its repository: the files changed in its working
//! directory's checkout since the session started, and their diffs.
//!
//! # Which base
//!
//! "Since the session started" has to be a commit to diff against, and there
//! were two plausible ones:
//!
//! - **The merge-base with the default branch.** Right for an agent on a branch
//!   of its own, and wrong for everything else: an agent working on `main`
//!   has no merge-base to speak of, and one handed a branch somebody else
//!   started is charged with the whole branch.
//! - **Where `HEAD` was when the session started.** Right whichever branch the
//!   agent is on, because it asks the checkout's own history rather than a
//!   naming convention. Commits the agent made since are in the diff, and so is
//!   whatever it left uncommitted.
//!
//! This is the second, read from the checkout's `HEAD` reflog: every move of
//! `HEAD` with the time it happened and the commit it moved from and to. The
//! newest move at or before the start is where `HEAD` stood. A checkout whose
//! first move came later was either made during the session — a worktree the
//! agent created for itself — or has had its older moves expired; the first
//! move's *old* side answers both, and a creation (whose old side is the zero
//! id) falls back to its new side, the commit the checkout was made at. With
//! no reflog at all, the newest commit on `HEAD`'s history from before the
//! start stands in.
//!
//! # Moving the base past what was not the session's
//!
//! `HEAD` at the start is only the right base until something else moves
//! `HEAD` under the session: a `git pull` in the middle of it, by the person or
//! by the agent, brought in every commit anyone else pushed, and against the
//! start's base they all read as the session's — one pull and a session that
//! touched three files reported a hundred and sixty. So the reflog is read on
//! past the start, and every move that is not this checkout's own work moves
//! the base with it:
//!
//! - a fast-forward (`pull: Fast-forward`, `merge X: Fast-forward`), a
//!   `reset`, a `checkout` of another branch, the start of a rebase: the base
//!   becomes where `HEAD` landed, so what the session does next is measured
//!   from there, and a rebase's replayed commits — the session's own — are in.
//! - a merge that made a commit (`pull: Merge made by…`, `commit (merge)`):
//!   the base becomes the merged-in side, the commit's second parent. Against
//!   it, the merge's result differs by exactly the local side's work.
//! - anything else — `commit`, `commit (amend)`, `cherry-pick`, `revert`, a
//!   rebase's `pick`s and its `finish` — is the session's own and stays in.
//!
//! The start is the latest run of the session in this checkout, not its first:
//! a session resumed today after a week away is charged with today, and is
//! given that start by its caller ([`since`]).
//!
//! ponytail: the diff is the checkout as it is now against how it was, so a
//! person's edits — or another agent's in the same checkout — during the
//! session are in it too. Telling them apart would need the transcript's
//! edits, which Tool Activity already shows; this is the repository's side.
//!
//! Untracked files the ignore rules let through are listed as wholly new: an
//! agent's new file is usually its most important change, and `git diff`
//! never shows one that was not added. Only those written since the start,
//! though: an untracked scratch file from last month is not the session's.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Files listed before the rest are counted rather than shown.
const MAX_FILES: usize = 300;
/// Bytes of patch read from git, across every file. A regenerated lockfile or a
/// committed build can be megabytes, and none of it is worth holding in memory
/// to show the first screen of.
const MAX_PATCH_BYTES: usize = 1 << 20;
/// Lines of one file's diff kept for the panel to expand.
pub const MAX_FILE_LINES: usize = 400;
/// Bytes of an untracked file read to count and show its lines.
const MAX_UNTRACKED_BYTES: usize = 256 << 10;
/// Bytes of the untracked-file list read: an unignored `node_modules` is
/// hundreds of thousands of names.
const MAX_LIST_BYTES: usize = 256 << 10;

/// How the base commit was chosen, which the panel names so the diff is never
/// read as relative to something it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseKind {
    /// Where `HEAD` was when the session started, from the reflog.
    AtStart,
    /// The checkout was made after the session started; the commit it was made
    /// at.
    Created,
    /// No reflog to read: the newest commit before the session started.
    CommitBefore,
    /// Nothing was committed before the session started (or the repository
    /// has no commits): every file in the checkout counts as new.
    Empty,
    /// The session's start is unknown, so only uncommitted changes are shown.
    Head,
    /// Where a pull, merge, rebase, reset or checkout during the session left
    /// `HEAD` — see the module docs.
    Synced,
}

impl BaseKind {
    /// How the panel's first line describes this base.
    pub fn describe(self) -> &'static str {
        match self {
            BaseKind::AtStart => "HEAD when the session started",
            BaseKind::Created => "where this checkout was made, after the session started",
            BaseKind::CommitBefore => "the last commit before the session started (no reflog)",
            BaseKind::Empty => "nothing: no commit predates the session",
            BaseKind::Head => "HEAD: the session's start is unknown, so uncommitted only",
            BaseKind::Synced => "where the last pull, merge or rebase in the session left HEAD",
        }
    }
}

/// The commit the diff is against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    /// Full object id; the empty tree's for [`BaseKind::Empty`].
    pub commit: String,
    pub kind: BaseKind,
}

/// One changed file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileChange {
    /// Relative to the checkout root.
    pub path: String,
    /// Lines added and removed; `None` for a binary file.
    pub added: Option<u64>,
    pub removed: Option<u64>,
    /// Not known to git at all: new, and never added.
    pub untracked: bool,
    /// The diff's lines from the first hunk on, without git's file header.
    pub diff: Vec<String>,
    /// The diff was longer than what is kept.
    pub cut: bool,
}

/// The changes in a checkout since a session started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes {
    pub root: PathBuf,
    pub base: Base,
    pub files: Vec<FileChange>,
    /// Files past [`MAX_FILES`], counted and not listed.
    pub more_files: usize,
    /// The patch ran past [`MAX_PATCH_BYTES`]: files after the cut have counts
    /// but no diff.
    pub patch_cut: bool,
}

impl Changes {
    pub fn totals(&self) -> (u64, u64) {
        self.files.iter().fold((0, 0), |(a, r), f| {
            (a + f.added.unwrap_or(0), r + f.removed.unwrap_or(0))
        })
    }
}

/// The changes in `dir`'s checkout since `started` (seconds since the epoch).
///
/// `Ok(None)` is a directory outside any repository — an answer, not a
/// failure. Runs several git commands and reads files, so it belongs on a
/// worker thread.
pub fn read(dir: &Path, started: Option<i64>) -> Result<Option<Changes>, String> {
    let Some((_, root)) = crate::worktree::locate(dir) else {
        return Ok(None);
    };
    let base = choose_base(&root, started)?;
    let (numstat, _) = run_capped(
        diff_cmd(&root).args(["--numstat", "-z", &base.commit, "--"]),
        MAX_PATCH_BYTES,
    )?;
    let mut files = parse_numstat(&numstat);
    let (patch, patch_cut) = run_capped(
        diff_cmd(&root).args(["-p", "-U3", &base.commit, "--"]),
        MAX_PATCH_BYTES,
    )?;
    attach_diffs(&mut files, &patch, patch_cut);

    let (others, _) = run_capped(
        git(&root).args(["ls-files", "--others", "--exclude-standard", "-z"]),
        MAX_LIST_BYTES,
    )?;
    let others: Vec<&str> = others
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter(|p| written_since(&root.join(p), started))
        .collect();
    let room = MAX_FILES.saturating_sub(files.len());
    let more_untracked = others.len().saturating_sub(room);
    files.extend(others.iter().take(room).map(|p| untracked(&root, p)));
    let more_tracked = files.len().saturating_sub(MAX_FILES);
    files.truncate(MAX_FILES);
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Some(Changes {
        root,
        base,
        files,
        more_files: more_tracked + more_untracked,
        patch_cut,
    }))
}

/// Whether the file at `path` was written at or after `started`; anything is,
/// when the start is unknown.
fn written_since(path: &Path, started: Option<i64>) -> bool {
    let Some(started) = started else {
        return true;
    };
    std::fs::symlink_metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
        .is_none_or(|at| at.as_secs() as i64 >= started)
}

/// When a session's latest run started, for [`read`]: the later of its first
/// message and its running agent's process start.
///
/// The transcript's start is the session's first-ever moment, which a resume
/// keeps; the process is the run going on now. Measured from the first, a
/// session resumed after a week is charged with the week.
/// Where a session's changes are read from: its directory, while it is still
/// one on this machine.
pub fn checkout_of(session: &crate::session::Session) -> Result<PathBuf, String> {
    // A stopped sandbox's directory was the host's, mounted for as long as the
    // agent ran, and the same path here now is something else.
    if session.sandbox.is_some() && !session.is_running() {
        return Err(
            "this session ran in a sandbox that has stopped, so its checkout is no \
             longer mounted here"
                .into(),
        );
    }
    match session.work_dir() {
        Some(dir) => Ok(dir),
        None if session.label_source.is_empty() => {
            Err("this session did not record a directory".into())
        }
        None => Err(format!(
            "{} no longer exists on this machine",
            crate::util::tildify(&session.label_source)
        )),
    }
}

pub fn since(session: &crate::session::Session) -> Option<i64> {
    let first = chrono::DateTime::parse_from_rfc3339(&session.started_at)
        .ok()
        .map(|t| t.timestamp());
    let run = session.root_pid().and_then(crate::proc::started_epoch);
    match (first, run) {
        (Some(first), Some(run)) => Some(first.max(run)),
        (first, run) => first.or(run),
    }
}

/// `git -C root`, scrubbed of the environment that would aim it elsewhere, and
/// without the optional locks: cctop only ever looks at this checkout, and an
/// index refresh holding `index.lock` would fail a commit the agent is making.
///
/// And without `core.fsmonitor`, which names a program git runs on a read of
/// the working tree. A person typing `git diff` chose to run whatever the
/// checkout's config says; selecting a row is not that choice.
pub(crate) fn git(root: &Path) -> Command {
    let mut cmd = crate::worktree::git_in(root);
    cmd.env("GIT_OPTIONAL_LOCKS", "0").args([
        "-c",
        "core.quotepath=off",
        "-c",
        "core.fsmonitor=false",
    ]);
    cmd
}

/// `git diff` as the panel needs it whatever the user's configuration: no
/// colour (the panel colours by theme), no external or text-converted diff,
/// and no rename pairing, so each file is one entry with one path in both the
/// counts and the patch — which is what lets the two be matched by position.
pub(crate) fn diff_cmd(root: &Path) -> Command {
    let mut cmd = git(root);
    cmd.args([
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--no-renames",
        "--ignore-submodules=dirty",
    ]);
    cmd
}

/// Run `cmd` and read at most `cap` bytes of what it prints, killing it past
/// that: a `git diff` of a vendored tree is not something to wait for in full.
/// The second value is whether the output was cut.
pub(crate) fn run_capped(cmd: &mut Command, cap: usize) -> Result<(String, bool), String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    let mut out = Vec::new();
    let stdout = child.stdout.take().ok_or("git: no output")?;
    stdout
        .take(cap as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("git: {e}"))?;
    let cut = out.len() > cap;
    if cut {
        out.truncate(cap);
        let _ = child.kill();
        let _ = child.wait();
    } else {
        let status = child.wait().map_err(|e| format!("git: {e}"))?;
        if !status.success() {
            return Err(format!("git exited with {status}"));
        }
    }
    Ok((String::from_utf8_lossy(&out).into_owned(), cut))
}

/// Run a short git command for its first line of output.
fn ask(root: &Path, args: &[&str]) -> Option<String> {
    let out = git(root)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    (out.status.success() && !line.is_empty()).then_some(line)
}

/// The commit to diff against; see the module docs for why this one.
fn choose_base(root: &Path, started: Option<i64>) -> Result<Base, String> {
    let empty = || -> Result<Base, String> {
        // Asked rather than spelled out: a SHA-256 repository's empty tree has
        // a different id.
        let commit = ask(root, &["hash-object", "-t", "tree", "/dev/null"])
            .ok_or("git could not name the empty tree")?;
        Ok(Base {
            commit,
            kind: BaseKind::Empty,
        })
    };
    let Some(head) = ask(root, &["rev-parse", "--verify", "-q", "HEAD"]) else {
        return empty();
    };
    let Some(started) = started else {
        return Ok(Base {
            commit: head,
            kind: BaseKind::Head,
        });
    };
    let log = ask(root, &["rev-parse", "--git-path", "logs/HEAD"])
        .map(|p| root.join(p))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    // A reflog can name a commit that has since been pruned; it is only a base
    // if git can still read it.
    let readable = |commit: &str| {
        let spec = format!("{commit}^{{commit}}");
        ask(root, &["rev-parse", "--verify", "-q", &spec]).is_some()
    };
    let at_start = head_at(&log, started)
        .filter(|(commit, _)| readable(commit))
        .map(|(commit, kind)| Base { commit, kind })
        .or_else(|| {
            ask(
                root,
                &["rev-list", "-1", &format!("--before={started}"), "HEAD"],
            )
            .map(|commit| Base {
                commit,
                kind: BaseKind::CommitBefore,
            })
        });
    let mut base = match at_start {
        Some(base) => base,
        None => empty()?,
    };
    for step in moves_after(&log, started) {
        let commit = match step {
            Move::To(new) => Some(new),
            Move::MergedIn(new) => ask(root, &["rev-parse", "--verify", "-q", &format!("{new}^2")]),
        };
        if let Some(commit) = commit.filter(|c| readable(c)) {
            base = Base {
                commit,
                kind: BaseKind::Synced,
            };
        }
    }
    Ok(base)
}

/// A move of `HEAD` during the session that was not the session's own work.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Move {
    /// `HEAD` landed here, and the session is measured from it on.
    To(String),
    /// This merge commit brought somebody else's work in; its second parent is
    /// what the local side's work is measured against.
    MergedIn(String),
}

/// The moves in a `logs/HEAD` reflog after `started` that move the base — see
/// the module docs for which those are.
fn moves_after(log: &str, started: i64) -> Vec<Move> {
    log.lines()
        .filter_map(|line| {
            let (head, message) = line.split_once('\t').unwrap_or((line, ""));
            let new = head.split(' ').nth(1)?;
            let mut tail = head.rsplit(' ');
            let _zone = tail.next()?;
            let at: i64 = tail.next()?.parse().ok()?;
            if at <= started {
                return None;
            }
            classify(message, new)
        })
        .collect()
}

/// Whose move one reflog entry was: `None` for the session's own.
fn classify(message: &str, new: &str) -> Option<Move> {
    // The command is what comes before the first `:` — `pull --rebase
    // (start)`, `merge origin/main`, `reset`.
    let command = message.split(':').next().unwrap_or("");
    if command.starts_with("commit (merge)") || message.contains(": Merge made by") {
        return Some(Move::MergedIn(new.to_string()));
    }
    let moved = message.contains(": Fast-forward")
        || (command.ends_with("(start)")
            && (command.starts_with("rebase") || command.starts_with("pull")))
        || command.starts_with("reset")
        || command.starts_with("checkout");
    moved.then(|| Move::To(new.to_string()))
}

/// Where `HEAD` stood at `started`, from the text of a `logs/HEAD` reflog.
///
/// Each line is `<old> <new> <name> <<email>> <seconds> <zone>\t<message>`,
/// oldest first. The newest entry at or before the start gives its new side.
/// With none, the first entry's old side is what `HEAD` was before anything
/// in the log happened — unless that is the zero id, which means the checkout
/// was created then, and the commit it was created at is the base. A
/// repository's very first commit has the zero id behind it too, and there the
/// base is nothing at all, which `None` leaves to the caller to find.
fn head_at(log: &str, started: i64) -> Option<(String, BaseKind)> {
    let entries: Vec<(&str, &str, i64, &str)> = log
        .lines()
        .filter_map(|line| {
            let (head, message) = line.split_once('\t').unwrap_or((line, ""));
            let mut words = head.split(' ');
            let old = words.next()?;
            let new = words.next()?;
            // The identity can hold spaces; the time is second from the end.
            let mut tail = head.rsplit(' ');
            let _zone = tail.next()?;
            let at = tail.next()?.parse().ok()?;
            Some((old, new, at, message))
        })
        .collect();
    let zero = |id: &str| id.bytes().all(|b| b == b'0');
    if let Some((_, new, _, _)) = entries.iter().rev().find(|(_, _, at, _)| *at <= started) {
        return (!zero(new)).then(|| (new.to_string(), BaseKind::AtStart));
    }
    let (old, new, _, message) = entries.first()?;
    if message.starts_with("commit (initial)") {
        None
    } else if zero(old) {
        (!zero(new)).then(|| (new.to_string(), BaseKind::Created))
    } else {
        Some((old.to_string(), BaseKind::AtStart))
    }
}

/// `git diff --numstat -z` without renames: `<added>\t<removed>\t<path>\0`,
/// with `-` for both counts of a binary file.
pub(crate) fn parse_numstat(out: &str) -> Vec<FileChange> {
    out.split('\0')
        .filter_map(|record| {
            let mut parts = record.splitn(3, '\t');
            let added = parts.next()?;
            let removed = parts.next()?;
            let path = parts.next().filter(|p| !p.is_empty())?;
            Some(FileChange {
                path: path.to_string(),
                added: added.parse().ok(),
                removed: removed.parse().ok(),
                ..FileChange::default()
            })
        })
        .collect()
}

/// Hand each file its section of the patch.
///
/// Matched by position: the counts and the patch come from the same diff
/// machinery with the same options, so the n-th `diff --git` header is the
/// n-th counted file. Parsing the path back out of the header instead would
/// mean undoing git's quoting, and a path with ` b/` in it is ambiguous even
/// then. When the two disagree in number — a patch cut short, or a git that
/// does something this does not expect — files past the agreement get no diff
/// rather than someone else's.
fn attach_diffs(files: &mut [FileChange], patch: &str, cut: bool) {
    let mut sections: Vec<Vec<&str>> = Vec::new();
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            sections.push(Vec::new());
        } else if let Some(section) = sections.last_mut() {
            section.push(line);
        }
    }
    // The last section of a cut patch is partial; it is kept, and marked.
    let complete = if cut {
        sections.len().saturating_sub(1)
    } else {
        sections.len()
    };
    for (i, (file, section)) in files.iter_mut().zip(&sections).enumerate() {
        // Git's own header — index, mode and the ---/+++ pair — says nothing
        // the row above it does not; the first hunk is where the change is.
        // Mode changes and "Binary files differ" have no hunk, and are kept.
        let body: Vec<&str> = match section.iter().position(|l| l.starts_with("@@")) {
            Some(at) => section[at..].to_vec(),
            None => section
                .iter()
                .copied()
                .filter(|l| {
                    !l.starts_with("index ") && !l.starts_with("--- ") && !l.starts_with("+++ ")
                })
                .collect(),
        };
        file.cut = body.len() > MAX_FILE_LINES || i >= complete;
        file.diff = body
            .into_iter()
            .take(MAX_FILE_LINES)
            .map(str::to_string)
            .collect();
    }
}

/// An untracked file as a diff from nothing: every line added.
fn untracked(root: &Path, path: &str) -> FileChange {
    let mut file = FileChange {
        path: path.to_string(),
        untracked: true,
        ..FileChange::default()
    };
    let mut bytes = Vec::new();
    let read = std::fs::File::open(root.join(path)).and_then(|f| {
        f.take(MAX_UNTRACKED_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
    });
    if read.is_err() {
        // A directory (a nested repository) or something unreadable: listed,
        // with nothing to count.
        return file;
    }
    // Git's own test for binary: a NUL in the first 8000 bytes.
    if bytes.iter().take(8000).any(|&b| b == 0) {
        file.diff = vec!["Binary file, untracked".into()];
        return file;
    }
    let long = bytes.len() > MAX_UNTRACKED_BYTES;
    bytes.truncate(MAX_UNTRACKED_BYTES);
    let text = String::from_utf8_lossy(&bytes);
    let count = text.lines().count();
    file.added = Some(count as u64);
    file.removed = Some(0);
    file.diff = text
        .lines()
        .take(MAX_FILE_LINES)
        .map(|l| format!("+{l}"))
        .collect();
    file.cut = long || count > MAX_FILE_LINES;
    file
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";
    const C: &str = "3333333333333333333333333333333333333333";
    const Z: &str = "0000000000000000000000000000000000000000";

    fn entry(old: &str, new: &str, at: i64) -> String {
        format!("{old} {new} Some One <one@x> {at} +0100\tcommit: did a thing\n")
    }

    #[test]
    fn head_at_start_is_the_newest_move_before_it() {
        let log = [entry(Z, A, 100), entry(A, B, 200), entry(B, C, 300)].concat();
        assert_eq!(head_at(&log, 250), Some((B.into(), BaseKind::AtStart)));
        assert_eq!(head_at(&log, 200), Some((B.into(), BaseKind::AtStart)));
        assert_eq!(head_at(&log, 1000), Some((C.into(), BaseKind::AtStart)));
    }

    #[test]
    fn a_checkout_made_during_the_session_starts_where_it_was_made() {
        let log = [entry(Z, A, 100), entry(A, B, 200)].concat();
        assert_eq!(head_at(&log, 50), Some((A.into(), BaseKind::Created)));
    }

    #[test]
    fn a_repository_born_during_the_session_has_no_base_in_its_reflog() {
        let log = format!("{Z} {A} T <t@t> 100 +0000\tcommit (initial): first\n");
        assert_eq!(head_at(&log, 50), None);
    }

    #[test]
    fn an_expired_reflog_still_knows_what_head_was_before_its_first_move() {
        let log = [entry(A, B, 200), entry(B, C, 300)].concat();
        assert_eq!(head_at(&log, 50), Some((A.into(), BaseKind::AtStart)));
    }

    #[test]
    fn no_reflog_is_no_answer() {
        assert_eq!(head_at("", 50), None);
        assert_eq!(head_at("garbage\n", 50), None);
    }

    #[test]
    fn numstat_reads_counts_binaries_and_odd_names() {
        let files = parse_numstat("3\t1\tsrc/a.rs\0-\t-\tlogo.png\x000\t0\twith\ttab and space\0");
        assert_eq!(files.len(), 3);
        assert_eq!((files[0].added, files[0].removed), (Some(3), Some(1)));
        assert_eq!(files[1].path, "logo.png");
        assert_eq!((files[1].added, files[1].removed), (None, None));
        assert_eq!(files[2].path, "with\ttab and space");
    }

    #[test]
    fn each_file_gets_its_own_section_without_the_header() {
        let mut files = parse_numstat("1\t1\ta\0-\t-\tb\0");
        let patch = "diff --git a/a b/a\nindex 1..2 100644\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n\
                     diff --git a/b b/b\nindex 3..4 100644\nBinary files a/b and b/b differ\n";
        attach_diffs(&mut files, patch, false);
        assert_eq!(files[0].diff, ["@@ -1 +1 @@", "-old", "+new"]);
        assert_eq!(files[1].diff, ["Binary files a/b and b/b differ"]);
        assert!(!files[0].cut && !files[1].cut);

        // Cut short: the partial last section is marked, and a file the patch
        // never reached has no diff at all.
        let mut files = parse_numstat("1\t1\ta\x001\t1\tb\0");
        attach_diffs(&mut files, "diff --git a/a b/a\n@@ -1 +1 @@\n-o", true);
        assert!(files[0].cut);
        assert!(files[1].diff.is_empty());
    }

    /// A realistic epoch for the fixtures: git reads a short number given to
    /// `--before` as a date phrase, not as seconds, and a session's start is
    /// never one.
    const T: i64 = 1_700_000_000;

    /// git in `dir` with a fixed identity and date, so reflog times are the
    /// test's to choose.
    fn git_at(dir: &Path, at: i64, args: &[&str]) {
        let out = crate::worktree::git_in(dir)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.logAllRefUpdates=true",
            ])
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .env("GIT_AUTHOR_DATE", format!("@{at} +0000"))
            .env("GIT_COMMITTER_DATE", format!("@{at} +0000"))
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// The whole read against a real repository: a commit before the start, one
    /// during it, an uncommitted edit and a new file — and the commit from
    /// before is not among the changes.
    #[test]
    fn the_diff_runs_from_head_at_the_start_to_the_working_tree() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path();
        git_at(repo, T + 1000, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("before.txt"), "one\n").unwrap();
        std::fs::write(repo.join("kept.txt"), "a\nb\n").unwrap();
        git_at(repo, T + 1000, &["add", "."]);
        git_at(repo, T + 1000, &["commit", "-q", "-m", "before"]);
        std::fs::write(repo.join("before.txt"), "one\ntwo\n").unwrap();
        git_at(repo, T + 1500, &["commit", "-q", "-am", "also before"]);

        // The session starts at 2000.
        std::fs::write(repo.join("during.txt"), "x\ny\nz\n").unwrap();
        git_at(repo, T + 2500, &["add", "during.txt"]);
        git_at(repo, T + 2500, &["commit", "-q", "-m", "during"]);
        std::fs::write(repo.join("kept.txt"), "a\nB\n").unwrap();
        std::fs::write(repo.join("new.txt"), "fresh\n").unwrap();

        let changes = read(repo, Some(T + 2000))
            .expect("read")
            .expect("a repository");
        assert_eq!(changes.base.kind, BaseKind::AtStart);
        let paths: Vec<&str> = changes.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["during.txt", "kept.txt", "new.txt"]);
        let kept = &changes.files[1];
        assert_eq!((kept.added, kept.removed), (Some(1), Some(1)));
        assert!(kept.diff.iter().any(|l| l == "+B"), "{:?}", kept.diff);
        let new = &changes.files[2];
        assert!(new.untracked);
        assert_eq!(new.diff, ["+fresh"]);
        assert_eq!(changes.totals(), (5, 1));

        // Before any commit existed, everything is new.
        let all = read(repo, Some(T + 10)).unwrap().unwrap();
        assert_eq!(all.base.kind, BaseKind::Empty);
        assert_eq!(all.files.len(), 4);
        // Without a start, only what is uncommitted.
        let loose = read(repo, None).unwrap().unwrap();
        assert_eq!(loose.base.kind, BaseKind::Head);
        let paths: Vec<&str> = loose.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["kept.txt", "new.txt"]);
    }

    /// Without a reflog the commit times stand in.
    #[test]
    fn without_a_reflog_the_last_commit_before_the_start_is_the_base() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path();
        git_at(repo, T + 1000, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("f"), "1\n").unwrap();
        git_at(repo, T + 1000, &["add", "."]);
        git_at(repo, T + 1000, &["commit", "-q", "-m", "one"]);
        std::fs::write(repo.join("f"), "1\n2\n").unwrap();
        git_at(repo, T + 3000, &["commit", "-q", "-am", "two"]);
        std::fs::remove_file(repo.join(".git/logs/HEAD")).unwrap();

        let changes = read(repo, Some(T + 2000)).unwrap().unwrap();
        assert_eq!(changes.base.kind, BaseKind::CommitBefore);
        assert_eq!(changes.files.len(), 1);
        assert_eq!(changes.files[0].added, Some(1));

        // And before the first commit, the empty tree.
        let all = read(repo, Some(T + 10)).unwrap().unwrap();
        assert_eq!(all.base.kind, BaseKind::Empty);
        assert_eq!(all.files[0].added, Some(2));
    }

    /// The moves that are not the session's own, read off their messages.
    #[test]
    fn a_pull_merge_rebase_reset_or_checkout_moves_the_base() {
        let to = |m: &str| classify(m, A);
        assert_eq!(to("pull: Fast-forward"), Some(Move::To(A.into())));
        assert_eq!(
            to("merge origin/main: Fast-forward"),
            Some(Move::To(A.into()))
        );
        assert_eq!(
            to("pull: Merge made by the 'ort' strategy."),
            Some(Move::MergedIn(A.into()))
        );
        assert_eq!(
            to("commit (merge): Merge branch 'x'"),
            Some(Move::MergedIn(A.into()))
        );
        assert_eq!(
            to("pull --rebase (start): checkout origin/main"),
            Some(Move::To(A.into()))
        );
        assert_eq!(
            to("rebase -i (start): checkout HEAD~3"),
            Some(Move::To(A.into()))
        );
        assert_eq!(to("reset: moving to origin/main"), Some(Move::To(A.into())));
        assert_eq!(
            to("checkout: moving from main to fix"),
            Some(Move::To(A.into()))
        );
        // The session's own work stays in.
        for own in [
            "commit: add the thing",
            "commit (amend): add the thing",
            "cherry-pick: x",
            "revert: Revert x",
            "rebase (pick): replay",
            "rebase (finish): returning to refs/heads/fix",
            "pull --rebase (finish): returning to refs/heads/main",
        ] {
            assert_eq!(to(own), None, "{own}");
        }
        // Only moves after the start count.
        let log = [
            format!("{A} {B} t <t@t> 100 +0000\treset: moving to B\n"),
            format!("{B} {C} t <t@t> 300 +0000\tpull: Fast-forward\n"),
        ]
        .concat();
        assert_eq!(moves_after(&log, 200), [Move::To(C.into())]);
    }

    /// A clone whose upstream moves during the session.
    fn clone_with_upstream(tmp: &Path) -> (PathBuf, PathBuf) {
        let up = tmp.join("up");
        let repo = tmp.join("repo");
        std::fs::create_dir_all(&up).unwrap();
        git_at(&up, T + 1000, &["init", "-q", "-b", "main"]);
        std::fs::write(up.join("base.txt"), "1\n").unwrap();
        git_at(&up, T + 1000, &["add", "."]);
        git_at(&up, T + 1000, &["commit", "-q", "-m", "base"]);
        git_at(
            tmp,
            T + 1100,
            &["clone", "-q", up.to_str().unwrap(), repo.to_str().unwrap()],
        );
        // Somebody else's work, pushed while the session runs.
        for (at, name) in [(T + 2100, "theirs1.txt"), (T + 2200, "theirs2.txt")] {
            std::fs::write(up.join(name), "theirs\n").unwrap();
            git_at(&up, at, &["add", name]);
            git_at(&up, at, &["commit", "-q", "-m", name]);
        }
        (up, repo)
    }

    fn paths(changes: &Changes) -> Vec<&str> {
        changes.files.iter().map(|f| f.path.as_str()).collect()
    }

    /// A pull in the middle of a session — fast-forward or merge — brings
    /// other people's commits, and they are not the session's changes; the
    /// agent's own commit, before or after the pull, is.
    #[test]
    fn a_pull_during_the_session_is_not_counted_as_its_changes() {
        // Pulled first, then worked: a fast-forward.
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, repo) = clone_with_upstream(tmp.path());
        git_at(&repo, T + 2500, &["-c", "pull.rebase=false", "pull", "-q"]);
        std::fs::write(repo.join("mine.txt"), "mine\n").unwrap();
        git_at(&repo, T + 2600, &["add", "mine.txt"]);
        git_at(&repo, T + 2600, &["commit", "-q", "-m", "mine"]);
        let changes = read(&repo, Some(T + 2000)).unwrap().unwrap();
        assert_eq!(changes.base.kind, BaseKind::Synced);
        assert_eq!(paths(&changes), ["mine.txt"]);

        // Worked first, then pulled: a merge.
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, repo) = clone_with_upstream(tmp.path());
        std::fs::write(repo.join("mine.txt"), "mine\n").unwrap();
        git_at(&repo, T + 2300, &["add", "mine.txt"]);
        git_at(&repo, T + 2300, &["commit", "-q", "-m", "mine"]);
        git_at(
            &repo,
            T + 2500,
            &["-c", "pull.rebase=false", "pull", "-q", "--no-edit"],
        );
        let changes = read(&repo, Some(T + 2000)).unwrap().unwrap();
        assert_eq!(changes.base.kind, BaseKind::Synced);
        assert_eq!(paths(&changes), ["mine.txt"]);

        // Worked first, then pulled with a rebase: the replayed commit is in.
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, repo) = clone_with_upstream(tmp.path());
        std::fs::write(repo.join("mine.txt"), "mine\n").unwrap();
        git_at(&repo, T + 2300, &["add", "mine.txt"]);
        git_at(&repo, T + 2300, &["commit", "-q", "-m", "mine"]);
        git_at(&repo, T + 2500, &["pull", "-q", "--rebase"]);
        let changes = read(&repo, Some(T + 2000)).unwrap().unwrap();
        assert_eq!(paths(&changes), ["mine.txt"]);
    }

    /// An untracked file last written before the session is not its new file.
    #[test]
    fn an_untracked_file_older_than_the_session_is_not_its_change() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path();
        git_at(repo, T + 1000, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a"), "1\n").unwrap();
        git_at(repo, T + 1000, &["add", "."]);
        git_at(repo, T + 1000, &["commit", "-q", "-m", "a"]);
        let old = repo.join("scratch-from-last-month.txt");
        std::fs::write(&old, "old\n").unwrap();
        let then = std::time::UNIX_EPOCH + std::time::Duration::from_secs((T + 500) as u64);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(then)
            .unwrap();
        std::fs::write(repo.join("fresh.txt"), "new\n").unwrap();
        let changes = read(repo, Some(T + 2000)).unwrap().unwrap();
        assert_eq!(paths(&changes), ["fresh.txt"]);
        // With no start known, every untracked file is listed, as before.
        assert_eq!(read(repo, None).unwrap().unwrap().files.len(), 2);
    }

    /// A resumed session is measured from the run going on now, not from its
    /// first message.
    #[test]
    fn a_resumed_session_starts_at_its_running_agent() {
        let mut session =
            crate::session::Session::new(crate::pricing::Provider::Claude, "s".into());
        session.started_at = "2020-01-01T00:00:00Z".into();
        let first = chrono::DateTime::parse_from_rfc3339(&session.started_at)
            .unwrap()
            .timestamp();
        assert_eq!(since(&session), Some(first));
        session.process = Some(crate::proc::ProcInfo {
            process_list: vec![crate::proc::ProcEntry {
                pid: std::process::id(),
                is_root: true,
                ghost: false,
                cpu: 0.0,
                memory: 0,
                args: String::new(),
            }],
            ..Default::default()
        });
        let run = since(&session).expect("a start");
        let now = chrono::Utc::now().timestamp();
        assert!(run > first && run <= now && run > now - 86_400, "{run}");
    }

    #[test]
    fn a_directory_outside_any_repository_is_none() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert_eq!(read(tmp.path(), Some(T)), Ok(None));
    }
}
