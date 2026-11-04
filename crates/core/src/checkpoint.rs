//! Checkpoints: a session's working tree, snapshotted at each turn, so that one
//! bad turn can be rolled back — whichever agent took it.
//!
//! Claude Code has `/rewind` of its own, and it knows things this cannot: the
//! conversation, and exactly which edits its own tools made. This does not try
//! to replace it. It is the same safety for every harness cctop watches —
//! Codex, Gemini CLI, Cursor, OpenCode — and it works from the repository's
//! side, so it also covers what an agent did through a shell command, which a
//! harness that tracks only its own file edits cannot undo.
//!
//! Off unless `[settings] checkpoints = true`, because it writes into the
//! user's repositories (refs and objects, never their files) — see below.
//!
//! # Without touching the user's state
//!
//! A snapshot is taken while the agent and the person are both working in the
//! checkout, so it must not change anything either of them can see: not the
//! index (what is staged is theirs), not the working tree, not `HEAD`, not the
//! stash list. `git stash create` would be the obvious tool and is the wrong
//! one: it refreshes the real index on the way, which takes `index.lock` and
//! can fail a commit the agent is making that instant.
//!
//! So it is plumbing over an index of its own, in a temporary directory:
//!
//! ```text
//! GIT_INDEX_FILE=<tmp> git read-tree HEAD
//! GIT_INDEX_FILE=<tmp> git add -A          # respects .gitignore
//! GIT_INDEX_FILE=<tmp> git write-tree
//! git commit-tree <tree> -p HEAD -m "turn ended"
//! git update-ref refs/cctop/checkpoints/<session>/<n> <commit> ""
//! ```
//!
//! What lands in the repository is one ref per checkpoint and the objects it
//! names. Nothing under `refs/heads` or `refs/tags`, so no branch list,
//! `git log --all` aside, ever shows them; no reflog, because refs outside the
//! usual namespaces get none by default; and nothing a push sends, because a
//! push sends branches and tags. Every git command here runs with hooks and
//! `core.fsmonitor` off — a `reference-transaction` hook firing on a monitor's
//! snapshot is the same surprise as an fsmonitor run on a read — and with
//! cctop's own identity, so a checkout with no `user.name` can still be
//! snapshotted and nothing is signed in the user's name.
//!
//! A snapshot whose tree is the same as the session's newest checkpoint's is
//! not made: a turn that changed nothing has nothing to roll back.
//!
//! ponytail: `read-tree HEAD` gives the temporary index no stat data, so `add
//! -A` hashes every file in the checkout, every time. Copying the real index
//! instead would let git skip unchanged files, and would also copy a split
//! index's pointer to a shared file this index cannot follow. On a checkout of
//! ordinary size it is milliseconds; on a monorepo it is a background process
//! working for a while, at low priority, which is why it never runs in the hook.
//!
//! # When
//!
//! At the two turn boundaries every harness reports: a prompt submitted
//! (`UserPromptSubmit`, Gemini's `BeforeAgent`, Cursor's `beforeSubmitPrompt`)
//! and a turn ended (`Stop`, `AfterAgent`, `stop`, OpenCode's `session.idle`,
//! Codex's `agent-turn-complete`). The first is the state a turn starts from,
//! which is what a rewind of that turn wants; the second is there because
//! OpenCode and a Codex whose hooks are not yet trusted report nothing else,
//! and the end of one turn is the start of the next unless somebody edits in
//! between. Where both fire, the second usually finds nothing new and is
//! skipped.
//!
//! The hook does not take the snapshot. `cctop hook` runs inside the agent's
//! session with a deadline of a quarter of a second and must exit 0 whatever
//! happens (see [`crate::hook`]); a `git add -A` of a large checkout is
//! neither quick nor certain. It spawns `cctop checkpoint take` detached — its
//! own session, no terminal, every stream on `/dev/null` — and returns without
//! waiting. Handing the request to a running TUI instead would make
//! checkpoints exist only while a dashboard is open, and once per dashboard
//! when two are.
//!
//! # Retention
//!
//! The newest [`KEEP`] per session, and none older than [`MAX_AGE_SECS`] for
//! any session in the repository, pruned each time a checkpoint is made. Only
//! the refs: the objects become unreachable and `git gc` reclaims them on its
//! own schedule, which is the user's.
//!
//! # Rewind
//!
//! [`rewind`] puts the working tree back to a checkpoint's tree, and first
//! takes a checkpoint of how it is now, so a rewind can itself be rewound. It
//! restores files and nothing else: `HEAD` stays where it is, no commit is
//! undone, and files the ignore rules exclude are never touched. A file that is
//! untracked now and was not in the checkpoint is the agent's new file, and is
//! removed. The index is set to the checkpoint's tree too — a working tree
//! put back under an index still holding the agent's staged work would commit
//! the agent's work on the next `git commit`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Checkpoints kept per session. Fifty turns is a long session's worth, and
/// each is a ref and whatever objects changed — not a copy of the tree.
pub const KEEP: usize = 50;

/// Checkpoints older than this are pruned, a session's or not: a week-old
/// turn is not one anybody rewinds to, and a finished session's refs would
/// otherwise stay forever.
pub const MAX_AGE_SECS: i64 = 7 * 24 * 60 * 60;

/// Where the refs live: under `refs/` so git keeps their objects, and outside
/// `heads` and `tags` so nothing shows or pushes them.
pub const NAMESPACE: &str = "refs/cctop/checkpoints";

/// Paths a checkpoint's difference from now lists before counting the rest.
const MAX_DIFFERS: usize = 200;

/// One checkpoint, as its ref and commit say it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// Its number within the session, counting up from 1.
    pub n: u64,
    pub commit: String,
    pub tree: String,
    /// Where `HEAD` was when it was taken; `None` in a repository with no
    /// commits yet.
    pub head: Option<String>,
    /// When, in seconds since the epoch.
    pub at: i64,
    /// Why it was taken: "prompt submitted", "turn ended", "before rewind to #3".
    pub why: String,
}

/// What a rewind to a checkpoint would do to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// It differs, and goes back to the checkpoint's content.
    Revert,
    /// It is not in the checkpoint, and is removed.
    Remove,
    /// It is in the checkpoint and gone now, and comes back.
    Restore,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Differ {
    pub path: String,
    pub effect: Effect,
}

/// A checkpoint, and how the checkout now differs from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    pub checkpoint: Checkpoint,
    /// The first [`MAX_DIFFERS`] files a rewind would change.
    pub differs: Vec<Differ>,
    /// Files past those, counted.
    pub more: usize,
}

impl Point {
    pub fn files(&self) -> usize {
        self.differs.len() + self.more
    }
}

/// A session's checkpoints, newest first, against the checkout as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Survey {
    /// `HEAD` now, to tell a checkpoint taken before the agent committed.
    pub head: Option<String>,
    pub points: Vec<Point>,
}

/// What [`rewind`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewound {
    /// The checkpoint the tree now matches.
    pub to: u64,
    /// The checkpoint holding what was there before: the way back.
    pub undo: u64,
    /// Untracked files removed because the checkpoint did not have them.
    pub removed: usize,
    /// `HEAD` had moved since the checkpoint, and was left where it is.
    pub head_moved: bool,
}

/// What a fire of `cctop hook` should snapshot, if it is a turn boundary: the
/// reason, the session and the directory.
///
/// A subagent's events are its parent's session's, and are left out: the
/// turn is the parent's, and a subagent finishing is the middle of it.
pub fn hook_request<'a>(
    event: &str,
    session: &'a str,
    cwd: &'a str,
    agent_id: &str,
) -> Option<(&'static str, &'a str, &'a Path)> {
    let why = match event {
        "UserPromptSubmit" | "BeforeAgent" | "beforeSubmitPrompt" => "prompt submitted",
        "Stop" | "AfterAgent" | "stop" | "session.idle" | "agent-turn-complete" => "turn ended",
        _ => return None,
    };
    let cwd = Path::new(cwd);
    (agent_id.is_empty() && ref_safe(session) && cwd.is_absolute()).then_some((why, session, cwd))
}

/// Start `exe checkpoint take <session> <dir> <why>` and return without
/// waiting for it.
///
/// Its own session (`setsid`), so it has no controlling terminal and the
/// agent's Ctrl-C does not reach it; every stream on `/dev/null`, so nothing
/// it prints can be read by the agent as a hook's answer and no pipe of the
/// agent's is held open by it. The child is reaped on a thread that the hook
/// abandons when it exits, which leaves the child to init — the point.
pub fn spawn_take(exe: &Path, session: &str, dir: &Path, why: &str) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut cmd = Command::new(exe);
    cmd.args(["checkpoint", "take", session])
        .arg(dir)
        .arg(why)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and allocates nothing.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Whether a session id can be a ref's path component as it is.
///
/// Every harness's ids are uuids or `ses_…`-style words, so anything else is
/// refused rather than escaped: an escaping that two ids could share would
/// mix two sessions' checkpoints.
pub fn ref_safe(session: &str) -> bool {
    !session.is_empty()
        && session.len() <= 128
        && !session.starts_with('.')
        && !session.ends_with('.')
        && !session.ends_with(".lock")
        && !session.contains("..")
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// `git -C root` as [`crate::changes`] runs it, and without hooks: a
/// `reference-transaction` hook would otherwise fire on every snapshot.
fn git(root: &Path) -> Command {
    let mut cmd = crate::changes::git(root);
    cmd.args(["-c", "core.hooksPath=/dev/null"]);
    // cctop's name on what it writes: a checkout with no identity configured
    // can still be snapshotted, and nothing appears under the user's.
    for (var, value) in [
        ("GIT_AUTHOR_NAME", "cctop"),
        ("GIT_AUTHOR_EMAIL", "cctop@localhost"),
        ("GIT_COMMITTER_NAME", "cctop"),
        ("GIT_COMMITTER_EMAIL", "cctop@localhost"),
    ] {
        cmd.env(var, value);
    }
    cmd
}

/// Run `cmd` for its whole output, or the first line of what it complained.
fn run(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("failed");
        return Err(format!("git: {}", line.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn head(root: &Path) -> Option<String> {
    run(git(root).args(["rev-parse", "--verify", "-q", "HEAD"]))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn prefix(session: &str) -> String {
    format!("{NAMESPACE}/{session}/")
}

fn root_of(dir: &Path) -> Option<PathBuf> {
    crate::worktree::locate(dir).map(|(_, root)| root)
}

/// The tree the checkout would be if everything in it were added — the
/// snapshot, without the commit. See the module docs for why a separate index.
fn state_tree(root: &Path) -> Result<String, String> {
    let tmp = tempfile::tempdir().map_err(|e| format!("a temporary index: {e}"))?;
    let index = tmp.path().join("index");
    let with_index = |args: &[&str]| {
        let mut cmd = git(root);
        cmd.env("GIT_INDEX_FILE", &index).args(args);
        run(&mut cmd)
    };
    if head(root).is_some() {
        with_index(&["read-tree", "HEAD"])?;
    }
    // `--ignore-errors`: one unreadable file is not a reason to have no
    // checkpoint at all.
    with_index(&["add", "-A", "--ignore-errors", "--", "."])?;
    Ok(with_index(&["write-tree"])?.trim().to_string())
}

/// Every checkpoint ref under `pattern`, oldest number first within a session.
fn refs(root: &Path, pattern: &str) -> Result<Vec<(String, Checkpoint)>, String> {
    let out = run(git(root).args([
        "for-each-ref",
        "--format=%(refname)%00%(objectname)%00%(tree)%00%(parent)%00%(committerdate:raw)%00%(contents:subject)",
        pattern,
    ]))?;
    let mut found: Vec<(String, Checkpoint)> = out
        .lines()
        .filter_map(|line| {
            let mut f = line.split('\0');
            let name = f.next()?.to_string();
            let n = name.rsplit('/').next()?.parse().ok()?;
            let commit = f.next()?.to_string();
            let tree = f.next()?.to_string();
            let head = f.next().filter(|p| !p.is_empty()).map(str::to_string);
            let at = f.next()?.split(' ').next()?.parse().ok()?;
            let why = f.next().unwrap_or_default().to_string();
            Some((
                name,
                Checkpoint {
                    n,
                    commit,
                    tree,
                    head,
                    at,
                    why,
                },
            ))
        })
        .collect();
    found.sort_by(|(a, x), (b, y)| {
        let session = |name: &str| name.rsplit_once('/').map(|(s, _)| s.to_string());
        session(a).cmp(&session(b)).then(x.n.cmp(&y.n))
    });
    Ok(found)
}

fn session_refs(root: &Path, session: &str) -> Result<Vec<Checkpoint>, String> {
    Ok(refs(root, &prefix(session))?
        .into_iter()
        .map(|(_, c)| c)
        .collect())
}

/// A lock on one checkout's checkpoints, held while one is made.
///
/// Codex reports the end of a turn twice — through its hooks and through
/// `notify` — and both arrive in the same millisecond. Unserialised, both would
/// find the newest checkpoint different from now and make the same one twice.
/// The lock is a file in cctop's runtime directory rather than in `.git`,
/// which is the user's.
fn lock(root: &Path) -> Option<std::fs::File> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hasher);
    let dir = crate::config::runtime_base()
        .join("cctop")
        .join("checkpoints");
    std::fs::create_dir_all(&dir).ok()?;
    let file = std::fs::File::create(dir.join(format!("{:016x}.lock", hasher.finish()))).ok()?;
    file.lock().ok()?;
    Some(file)
}

/// A checkpoint, and whether it was made just now or the newest one already
/// held the same tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    pub checkpoint: Checkpoint,
    pub fresh: bool,
}

/// Snapshot `dir`'s checkout for `session`. `Ok(None)` is a directory outside
/// any repository.
pub fn take(dir: &Path, session: &str, why: &str) -> Result<Option<Taken>, String> {
    if !ref_safe(session) {
        return Err(format!("{session:?} is not a session id a ref can carry"));
    }
    let Some(root) = root_of(dir) else {
        return Ok(None);
    };
    take_in(&root, session, why, &[]).map(Some)
}

/// [`take`] in a checkout already found, keeping the refs in `protect` from
/// the prune that follows.
fn take_in(root: &Path, session: &str, why: &str, protect: &[&str]) -> Result<Taken, String> {
    let _lock = lock(root);
    let tree = state_tree(root)?;
    let mine = session_refs(root, session)?;
    if let Some(last) = mine.last()
        && last.tree == tree
    {
        return Ok(Taken {
            checkpoint: last.clone(),
            fresh: false,
        });
    }
    let head = head(root);
    let mut cmd = git(root);
    cmd.args(["commit-tree", "--no-gpg-sign", &tree]);
    if let Some(head) = &head {
        cmd.args(["-p", head]);
    }
    cmd.args([
        "-m",
        why,
        "-m",
        &format!("cctop checkpoint of session {session}"),
    ]);
    let commit = run(&mut cmd)?.trim().to_string();

    // The next number, and a few after it should another cctop have taken it
    // between the read and the write: an empty old value is git's "only if it
    // does not exist yet".
    let mut n = mine.last().map_or(1, |c| c.n + 1);
    let mut tries = 0;
    loop {
        let name = format!("{}{n}", prefix(session));
        match run(git(root).args(["update-ref", &name, &commit, ""])) {
            Ok(_) => break,
            Err(_) if tries < 5 => {
                tries += 1;
                n += 1;
            }
            Err(e) => return Err(e),
        }
    }
    let now = chrono::Utc::now().timestamp();
    let made = format!("{}{n}", prefix(session));
    let mut keep: Vec<&str> = protect.to_vec();
    keep.push(&made);
    prune(root, session, now, KEEP, &keep);
    Ok(Taken {
        checkpoint: Checkpoint {
            n,
            commit,
            tree,
            head,
            at: now,
            why: why.to_string(),
        },
        fresh: true,
    })
}

/// Drop `session`'s checkpoints past the newest `keep`, and every session's
/// older than [`MAX_AGE_SECS`] at `now` — except the refs in `protect`.
///
/// A failure is left for the next prune: the transaction is all or nothing,
/// and a ref another cctop deleted first fails it harmlessly.
fn prune(root: &Path, session: &str, now: i64, keep: usize, protect: &[&str]) {
    let Ok(all) = refs(root, NAMESPACE) else {
        return;
    };
    let mine = prefix(session);
    let own: Vec<&String> = all
        .iter()
        .filter(|(name, _)| name.starts_with(&mine))
        .map(|(name, _)| name)
        .collect();
    let surplus: HashSet<&String> = own
        .iter()
        .take(own.len().saturating_sub(keep))
        .copied()
        .collect();
    let doomed: String = all
        .iter()
        .filter(|(name, c)| surplus.contains(name) || now - c.at > MAX_AGE_SECS)
        .filter(|(name, _)| !protect.contains(&name.as_str()))
        .map(|(name, _)| format!("delete {name}\n"))
        .collect();
    if doomed.is_empty() {
        return;
    }
    let Ok(mut child) = git(root)
        .args(["update-ref", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(doomed.as_bytes());
    }
    let _ = child.wait();
}

/// `session`'s checkpoints in `dir`'s checkout, oldest first. `Ok(None)` is a
/// directory outside any repository.
pub fn list(dir: &Path, session: &str) -> Result<Option<Vec<Checkpoint>>, String> {
    if !ref_safe(session) {
        return Ok(Some(Vec::new()));
    }
    let Some(root) = root_of(dir) else {
        return Ok(None);
    };
    session_refs(&root, session).map(Some)
}

/// `session`'s checkpoints newest first, each with what a rewind to it would
/// change. Snapshots the checkout's present state to compare against, so it
/// belongs on a worker thread.
pub fn survey(dir: &Path, session: &str) -> Result<Option<Survey>, String> {
    let Some(root) = root_of(dir) else {
        return Ok(None);
    };
    let checkpoints = match ref_safe(session) {
        true => session_refs(&root, session)?,
        false => Vec::new(),
    };
    let now = match checkpoints.is_empty() {
        true => String::new(),
        false => state_tree(&root)?,
    };
    let mut points = Vec::new();
    for checkpoint in checkpoints.into_iter().rev() {
        let (differs, more) = match checkpoint.tree == now {
            true => (Vec::new(), 0),
            false => differs(&root, &now, &checkpoint.tree)?,
        };
        points.push(Point {
            checkpoint,
            differs,
            more,
        });
    }
    Ok(Some(Survey {
        head: head(&root),
        points,
    }))
}

/// What going from tree `now` to tree `then` changes, file by file.
fn differs(root: &Path, now: &str, then: &str) -> Result<(Vec<Differ>, usize), String> {
    let out = run(git(root).args([
        "diff-tree",
        "-r",
        "-z",
        "--no-renames",
        "--name-status",
        now,
        then,
    ]))?;
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut all = Vec::new();
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let effect = match status.chars().next() {
            Some('A') => Effect::Restore,
            Some('D') => Effect::Remove,
            _ => Effect::Revert,
        };
        all.push(Differ {
            path: path.to_string(),
            effect,
        });
    }
    let more = all.len().saturating_sub(MAX_DIFFERS);
    all.truncate(MAX_DIFFERS);
    Ok((all, more))
}

/// Put `dir`'s checkout back to `session`'s checkpoint `n`. See the module
/// docs for what it touches and what it leaves.
pub fn rewind(dir: &Path, session: &str, n: u64) -> Result<Rewound, String> {
    if !ref_safe(session) {
        return Err("this session has no checkpoints".into());
    }
    let root = root_of(dir).ok_or("not in a git repository")?;
    let target = session_refs(&root, session)?
        .into_iter()
        .find(|c| c.n == n)
        .ok_or(format!("there is no checkpoint #{n}"))?;
    let target_ref = format!("{}{n}", prefix(session));

    // The way back, before anything moves. Kept even if this is the session's
    // oldest checkpoint and the prune would otherwise take it.
    let undo = take_in(
        &root,
        session,
        &format!("before rewind to #{n}"),
        &[&target_ref],
    )?;

    // Read before the restore, which stages the checkpoint's files and so
    // changes what counts as untracked.
    let untracked = run(git(&root).args(["ls-files", "--others", "--exclude-standard", "-z"]))?;
    let kept = run(git(&root).args([
        "ls-tree",
        "-r",
        "-z",
        "--full-tree",
        "--name-only",
        &target.commit,
    ]))?;
    let kept: HashSet<&str> = kept.split('\0').filter(|p| !p.is_empty()).collect();

    // `--staged` as well as `--worktree`: see the module docs. No-overlay, the
    // default for `restore`, so a tracked file the checkpoint lacks goes.
    run(git(&root).args([
        "restore",
        &format!("--source={}", target.commit),
        "--staged",
        "--worktree",
        "--",
        ".",
    ]))?;

    let mut removed = 0;
    for path in untracked.split('\0').filter(|p| !p.is_empty()) {
        // A directory here is a nested repository git would not look inside;
        // it is somebody's checkout, not a file the agent wrote.
        if path.ends_with('/') || kept.contains(path) {
            continue;
        }
        let full = root.join(path);
        let is_file = std::fs::symlink_metadata(&full).is_ok_and(|m| !m.is_dir());
        if is_file && std::fs::remove_file(&full).is_ok() {
            removed += 1;
            // Directories the removal emptied: `remove_dir` refuses one with
            // anything left in it, ignored files included.
            for parent in full.ancestors().skip(1) {
                if parent == root || std::fs::remove_dir(parent).is_err() {
                    break;
                }
            }
        }
    }
    Ok(Rewound {
        to: n,
        undo: undo.checkpoint.n,
        removed,
        head_moved: head(&root) != target.head,
    })
}

/// `cctop checkpoint take|list|rewind`.
///
/// `take` is what the hook spawns, detached: it reports nothing and exits 0,
/// since nobody is reading. It lowers its own priority first, so a snapshot of
/// a large checkout yields to the agent it is a snapshot of. `list` and
/// `rewind` are the same operations as the Changes panel's, for a script or a
/// terminal without the dashboard.
pub fn run_cli(argv: &[String]) -> i32 {
    let arg = |i: usize| argv.get(i).map(String::as_str);
    let here = || std::env::current_dir().unwrap_or_default();
    let dir_at = |i: usize| arg(i).map(PathBuf::from).unwrap_or_else(here);
    match (arg(0), arg(1)) {
        (Some("take"), Some(session)) => {
            // SAFETY: nice only changes this process's own priority.
            unsafe {
                libc::nice(10);
            }
            let _ = take(&dir_at(2), session, arg(3).unwrap_or("taken by hand"));
            0
        }
        (Some("list"), Some(session)) => match survey(&dir_at(2), session) {
            Ok(Some(survey)) if survey.points.is_empty() => {
                println!("No checkpoints for {session} here.");
                0
            }
            Ok(Some(survey)) => {
                for p in &survey.points {
                    let c = &p.checkpoint;
                    let at = chrono::DateTime::from_timestamp(c.at, 0)
                        .map(|t| {
                            t.with_timezone(&chrono::Local)
                                .format("%Y-%m-%d %H:%M")
                                .to_string()
                        })
                        .unwrap_or_default();
                    println!(
                        "#{:<4} {at}  {:<22} {}",
                        c.n,
                        c.why,
                        match p.files() {
                            0 => "same as now".to_string(),
                            n => crate::util::plural(n, "file") + " differ",
                        }
                    );
                }
                0
            }
            Ok(None) => {
                eprintln!("cctop: not in a git repository");
                1
            }
            Err(e) => {
                eprintln!("cctop: {e}");
                1
            }
        },
        (Some("rewind"), Some(session)) => {
            let Some(n) = arg(2).and_then(|n| n.trim_start_matches('#').parse().ok()) else {
                eprintln!("usage: cctop checkpoint rewind <session> <n> [dir]");
                return 2;
            };
            match rewind(&dir_at(3), session, n) {
                Ok(r) => {
                    println!(
                        "Rewound to #{}. What was there before is checkpoint #{}: \
                         `cctop checkpoint rewind {session} {}` undoes this.",
                        r.to, r.undo, r.undo
                    );
                    if r.head_moved {
                        println!("HEAD has moved since #{}; it was left where it is.", r.to);
                    }
                    0
                }
                Err(e) => {
                    eprintln!("cctop: {e}");
                    1
                }
            }
        }
        _ => {
            eprintln!(
                "usage: cctop checkpoint list <session> [dir]\n       \
                 cctop checkpoint rewind <session> <n> [dir]"
            );
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// git in `dir` with a fixed identity, for setting a fixture up.
    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = crate::worktree::git_in(dir)
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            // So `status` cannot refresh the index the test compares.
            .env("GIT_OPTIONAL_LOCKS", "0")
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A repository with one commit, an ignore rule and an ignored file.
    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        let r = tmp.path();
        sh(r, &["init", "-q", "-b", "main"]);
        std::fs::write(r.join(".gitignore"), "*.log\n").unwrap();
        std::fs::write(r.join("a.txt"), "one\n").unwrap();
        std::fs::write(r.join("b.txt"), "bee\n").unwrap();
        sh(r, &["add", "."]);
        sh(r, &["commit", "-q", "-m", "first"]);
        std::fs::write(r.join("debug.log"), "noise\n").unwrap();
        tmp
    }

    const S: &str = "0b5e3c1a-session";

    /// Everything a person can see of a checkout's state, to compare before
    /// and after.
    fn state(r: &Path) -> (String, String, String, String) {
        (
            sh(r, &["rev-parse", "HEAD"]),
            sh(r, &["status", "--porcelain=v1", "--ignored", "-uall"]),
            std::fs::read(r.join(".git/index"))
                .map(|b| format!("{b:?}"))
                .unwrap(),
            sh(r, &["stash", "list"]),
        )
    }

    #[test]
    fn a_snapshot_leaves_index_worktree_head_and_stash_alone() {
        let tmp = repo();
        let r = tmp.path();
        // A stash, something staged, something modified, something new.
        std::fs::write(r.join("a.txt"), "stashed\n").unwrap();
        sh(r, &["stash", "-q"]);
        std::fs::write(r.join("a.txt"), "staged\n").unwrap();
        sh(r, &["add", "a.txt"]);
        std::fs::write(r.join("b.txt"), "modified\n").unwrap();
        std::fs::write(r.join("new.txt"), "new\n").unwrap();

        let before = state(r);
        let taken = take(r, S, "prompt submitted").unwrap().unwrap();
        assert!(taken.fresh);
        assert_eq!(taken.checkpoint.n, 1);
        assert_eq!(state(r), before, "the snapshot changed what the user sees");

        // What it holds is the working tree, ignored files left out.
        let files = sh(
            r,
            &["ls-tree", "-r", "--name-only", &taken.checkpoint.commit],
        );
        let files: Vec<&str> = files.lines().collect();
        assert_eq!(files, [".gitignore", "a.txt", "b.txt", "new.txt"]);
        let b = sh(r, &["show", &format!("{}:b.txt", taken.checkpoint.commit)]);
        assert_eq!(b, "modified\n");
        let head = sh(r, &["rev-parse", "HEAD"]);
        assert_eq!(taken.checkpoint.head.as_deref(), Some(head.trim()));
        // And no branch, tag or reflog shows it.
        assert_eq!(sh(r, &["branch", "--list"]).lines().count(), 1);
        assert!(sh(r, &["tag"]).is_empty());
        assert!(!r.join(".git/logs/refs/cctop").exists());
    }

    #[test]
    fn an_unchanged_tree_is_not_a_new_checkpoint() {
        let tmp = repo();
        let r = tmp.path();
        let first = take(r, S, "prompt submitted").unwrap().unwrap();
        let again = take(r, S, "turn ended").unwrap().unwrap();
        assert!(!again.fresh);
        assert_eq!(again.checkpoint.n, first.checkpoint.n);
        // An ignored file changing is no change either.
        std::fs::write(r.join("debug.log"), "more noise\n").unwrap();
        assert!(!take(r, S, "turn ended").unwrap().unwrap().fresh);

        std::fs::write(r.join("a.txt"), "two\n").unwrap();
        let next = take(r, S, "turn ended").unwrap().unwrap();
        assert!(next.fresh);
        assert_eq!(next.checkpoint.n, 2);
        let listed = list(r, S).unwrap().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[1].why, "turn ended");
        // Another session's are its own.
        assert!(list(r, "other").unwrap().unwrap().is_empty());
    }

    #[test]
    fn retention_keeps_the_newest_and_drops_the_old() {
        let tmp = repo();
        let r = tmp.path();
        for i in 0..5 {
            std::fs::write(r.join("a.txt"), format!("{i}\n")).unwrap();
            take(r, S, "turn ended").unwrap().unwrap();
        }
        take(r, "other", "turn ended").unwrap().unwrap();
        let now = chrono::Utc::now().timestamp();
        prune(r, S, now, 3, &[]);
        let left: Vec<u64> = list(r, S).unwrap().unwrap().iter().map(|c| c.n).collect();
        assert_eq!(left, [3, 4, 5]);
        assert_eq!(
            list(r, "other").unwrap().unwrap().len(),
            1,
            "not its session"
        );

        // A week on, everything goes but what is protected — whoever's it is.
        let keep = format!("{}5", prefix(S));
        prune(r, S, now + MAX_AGE_SECS + 60, KEEP, &[&keep]);
        let left: Vec<u64> = list(r, S).unwrap().unwrap().iter().map(|c| c.n).collect();
        assert_eq!(left, [5]);
        assert!(list(r, "other").unwrap().unwrap().is_empty());
    }

    #[test]
    fn a_rewind_restores_modified_deleted_and_added_files_and_not_ignored_ones() {
        let tmp = repo();
        let r = tmp.path();
        std::fs::write(r.join("kept-new.txt"), "untracked before\n").unwrap();
        let cp = take(r, S, "prompt submitted").unwrap().unwrap().checkpoint;

        // The bad turn: an edit, a deletion, new files, an ignored file, and
        // a commit.
        std::fs::write(r.join("a.txt"), "ruined\n").unwrap();
        std::fs::remove_file(r.join("b.txt")).unwrap();
        std::fs::create_dir_all(r.join("junk/deep")).unwrap();
        std::fs::write(r.join("junk/deep/x.rs"), "x\n").unwrap();
        std::fs::write(r.join("stray.txt"), "stray\n").unwrap();
        std::fs::write(r.join("debug.log"), "changed noise\n").unwrap();
        std::fs::write(r.join("build.log"), "new noise\n").unwrap();
        std::fs::write(r.join("c.txt"), "committed\n").unwrap();
        sh(r, &["add", "c.txt"]);
        sh(r, &["commit", "-q", "-m", "agent"]);
        let head = sh(r, &["rev-parse", "HEAD"]);

        let survey = survey(r, S).unwrap().unwrap();
        let point = &survey.points[0];
        let effect = |p: &str| point.differs.iter().find(|d| d.path == p).map(|d| d.effect);
        assert_eq!(effect("a.txt"), Some(Effect::Revert));
        assert_eq!(effect("b.txt"), Some(Effect::Restore));
        assert_eq!(effect("c.txt"), Some(Effect::Remove));
        assert_eq!(effect("stray.txt"), Some(Effect::Remove));
        assert_eq!(effect("debug.log"), None, "ignored files are not in it");

        let done = rewind(r, S, cp.n).unwrap();
        assert_eq!(done.to, cp.n);
        assert!(done.head_moved);
        assert_eq!(done.removed, 2);
        let read = |p: &str| std::fs::read_to_string(r.join(p)).ok();
        assert_eq!(read("a.txt").as_deref(), Some("one\n"));
        assert_eq!(read("b.txt").as_deref(), Some("bee\n"));
        assert_eq!(read("kept-new.txt").as_deref(), Some("untracked before\n"));
        assert_eq!(read("stray.txt"), None);
        assert_eq!(read("c.txt"), None, "the committed file is not in the tree");
        assert!(!r.join("junk").exists(), "emptied directories go too");
        assert_eq!(read("debug.log").as_deref(), Some("changed noise\n"));
        assert_eq!(read("build.log").as_deref(), Some("new noise\n"));
        assert_eq!(sh(r, &["rev-parse", "HEAD"]), head, "HEAD never moves");

        // The way back exists, holds the bad turn, and works.
        let undo = list(r, S)
            .unwrap()
            .unwrap()
            .into_iter()
            .find(|c| c.n == done.undo)
            .expect("the pre-rewind checkpoint");
        assert_eq!(undo.why, format!("before rewind to #{}", cp.n));
        let stray = sh(r, &["show", &format!("{}:stray.txt", undo.commit)]);
        assert_eq!(stray, "stray\n");
        rewind(r, S, done.undo).unwrap();
        assert_eq!(read("a.txt").as_deref(), Some("ruined\n"));
        assert_eq!(read("junk/deep/x.rs").as_deref(), Some("x\n"));
        assert_eq!(read("b.txt"), None);
    }

    #[test]
    fn a_rewind_to_nothing_is_an_error_and_changes_nothing() {
        let tmp = repo();
        let r = tmp.path();
        std::fs::write(r.join("a.txt"), "edited\n").unwrap();
        assert!(rewind(r, S, 7).is_err());
        assert_eq!(
            std::fs::read_to_string(r.join("a.txt")).unwrap(),
            "edited\n"
        );
        assert!(list(r, S).unwrap().unwrap().is_empty());
    }

    #[test]
    fn a_repository_with_no_commits_can_be_snapshotted() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        sh(r, &["init", "-q"]);
        std::fs::write(r.join("f"), "x\n").unwrap();
        let taken = take(r, S, "prompt submitted").unwrap().unwrap();
        assert_eq!(taken.checkpoint.head, None);
        let outside = tempfile::tempdir().unwrap();
        assert_eq!(take(outside.path(), S, "x"), Ok(None));
    }

    #[test]
    fn only_turn_boundaries_of_a_session_in_a_directory_are_asked_for() {
        let req = |event, session, cwd, agent| hook_request(event, session, cwd, agent);
        assert_eq!(
            req("UserPromptSubmit", "abc-1", "/w", "").map(|r| r.0),
            Some("prompt submitted")
        );
        for event in [
            "Stop",
            "AfterAgent",
            "stop",
            "session.idle",
            "agent-turn-complete",
        ] {
            assert_eq!(req(event, "abc", "/w", "").map(|r| r.0), Some("turn ended"));
        }
        assert_eq!(req("PreToolUse", "abc", "/w", ""), None);
        assert_eq!(req("Stop", "abc", "/w", "sub-1"), None, "a subagent's");
        assert_eq!(req("Stop", "", "/w", ""), None);
        assert_eq!(req("Stop", "../../heads/main", "/w", ""), None);
        assert_eq!(req("Stop", "abc", "relative", ""), None);
    }

    #[test]
    fn ref_safe_refuses_what_git_or_a_path_would_read_twice() {
        assert!(ref_safe("0b5e3c1a-77aa-4f0b-9c2e-0123456789ab"));
        assert!(ref_safe("ses_4f2a"));
        for bad in ["", ".hidden", "a..b", "a/b", "x.lock", "a b", "trail."] {
            assert!(!ref_safe(bad), "{bad:?}");
        }
    }

    /// The hook's half returns at once, whatever the child does — a child that
    /// takes its time, or one that cannot start at all.
    #[test]
    fn the_hand_off_does_not_wait_for_the_snapshot() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let slow = tmp.path().join("slow");
        std::fs::write(&slow, "#!/bin/sh\nsleep 5\n").unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = std::time::Instant::now();
        spawn_take(&slow, S, tmp.path(), "turn ended").unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "waited {:?}",
            started.elapsed()
        );
        assert!(spawn_take(&tmp.path().join("missing"), S, tmp.path(), "x").is_err());
    }
}
