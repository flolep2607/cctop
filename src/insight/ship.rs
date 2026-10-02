//! `cctop yield` — did the spend ship?
//!
//! `optimize` asks what a session wasted and `compare` how a model behaved.
//! Neither can say whether the work was *kept*, and a session that edited
//! forty files, passed its tests and was then thrown away cost exactly as much
//! as one whose work is in production. The repository is the only record of
//! which was which, so this asks it.
//!
//! The obvious way to tie a commit to a session is the clock: a commit made
//! while a session was open, or shortly after, is probably that session's.
//! That breaks the moment two sessions overlap, which on a machine running
//! several agents at once is most of the time. cctop knows something a clock
//! does not — which files each session edited — so a commit is matched by
//! **path overlap inside the session's window**, and the clock only bounds the
//! candidates. Two sessions open at once in the same repository are told apart
//! by what they touched.
//!
//! It is still a heuristic, and the report says so under every table. What it
//! reads is the repository as it is now: no fetch, no network, nothing
//! written. Git runs once per repository, not once per session, and a
//! repository git cannot read is reported as unreadable rather than guessed
//! at.

use super::{Analysis, plural};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// How long after a session's last activity a commit can still be its own.
///
/// Four hours, because the commit is often not the agent's. A session ends,
/// somebody reads the diff over lunch and commits it; the work is the
/// session's however long the review took. Much longer and the window starts
/// catching the *next* session's commits to the same central files — and path
/// overlap can only choose between candidates, not notice that the right one
/// is missing. A session still open has no end yet, so its window runs to its
/// latest activity like any other and is simply reported as in progress until
/// the grace has passed.
pub const GRACE_SECS: i64 = 4 * 60 * 60;

/// What became of one session's spend.
///
/// Declared best first: a session that worked in two repositories is the best
/// of its two outcomes, because one shipped repository means the money bought
/// something, and the per-repository table still shows the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Class {
    /// A commit attributed to it is on the default branch and was not undone.
    Shipped,
    /// It committed, and a later commit reverted that work.
    ///
    /// Ranked above unmerged on purpose. A revert is a decision somebody made
    /// about work that landed; unmerged is the absence of one, and when both
    /// are true the decision is the more informative thing to say.
    Reverted,
    /// It committed, but only on branches that never reached the default one.
    Unmerged,
    /// It edited files in a repository git could not read.
    ///
    /// Above uncommitted, because "could not look" is not "looked and found
    /// nothing", and the second is a claim about somebody's work.
    Unreadable,
    /// Nothing committed yet, but its window has not closed: it was active
    /// less than [`GRACE_SECS`] ago. Calling it uncommitted would file every
    /// session running right now as abandoned.
    Open,
    /// It edited files in a repository, and no commit matched it.
    Uncommitted,
    /// Every file it edited is outside any repository.
    NotInGit,
}

impl Class {
    pub const ALL: [Class; 7] = [
        Class::Shipped,
        Class::Reverted,
        Class::Unmerged,
        Class::Unreadable,
        Class::Open,
        Class::Uncommitted,
        Class::NotInGit,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Class::Shipped => "shipped",
            Class::Reverted => "reverted",
            Class::Unmerged => "unmerged",
            Class::Unreadable => "unreadable",
            Class::Open => "in progress",
            Class::Uncommitted => "uncommitted",
            Class::NotInGit => "not in git",
        }
    }

    /// The JSON spelling, which a script will match on and so cannot carry a
    /// space.
    pub fn key(&self) -> &'static str {
        match self {
            Class::Open => "in_progress",
            Class::NotInGit => "not_in_git",
            other => other.as_str(),
        }
    }
}

/// One commit, as git reported it, with its paths made absolute against the
/// repository it was read from so they compare directly with what a session
/// edited.
#[derive(Debug, Clone, Default)]
pub struct Commit {
    pub sha: String,
    /// Author time, Unix seconds. Author rather than committer, because a
    /// rebase or cherry-pick moves the committer date to whenever that
    /// happened and would carry a session's commit out of its own window.
    pub at: i64,
    pub subject: String,
    pub files: HashSet<String>,
    /// The commits this one says it reverts — `This reverts commit <sha>.`,
    /// which is what `git revert` writes and what nearly everyone leaves in.
    pub reverts: Vec<String>,
    /// Reachable from the repository's default branch — see [`default_refs`].
    pub on_default: bool,
}

/// One session's work in one repository: what [`settle`] matches commits to.
#[derive(Debug, Clone)]
pub struct Work {
    /// Index of the session this belongs to, in whatever list the caller keeps.
    pub session: usize,
    pub files: HashSet<String>,
    /// First edit in this repository, Unix seconds.
    pub start: i64,
    /// The session's last activity — or its last edit, if that is later.
    pub end: i64,
}

impl Work {
    fn contains(&self, at: i64) -> bool {
        self.start <= at && at <= self.end.saturating_add(GRACE_SECS)
    }
}

/// What [`settle`] decided.
#[derive(Debug, Clone, Default)]
pub struct Settled {
    /// Per commit, the [`Work`] it was attributed to.
    pub owner: Vec<Option<usize>>,
    /// Per commit, whether a revert undid it — see [`reverted`].
    pub reverted: Vec<bool>,
    /// Per work, its outcome.
    pub class: Vec<Class>,
}

/// The session a commit belongs to, if any.
///
/// Candidates are the windows the commit falls inside that edited at least one
/// file it touches. Among them the most shared files wins — that is the whole
/// point of reading paths, and a session that wrote three of a commit's files
/// is a better claim than one that wrote one of them, however close the second
/// was in time. A tie goes to the tightest window: a session open for twenty
/// minutes around a commit says more about it than one left open all week.
/// The last tie goes to the earlier index, so the answer never depends on hash
/// order.
fn owner(commit: &Commit, works: &[Work]) -> Option<usize> {
    works
        .iter()
        .enumerate()
        .filter(|(_, w)| w.contains(commit.at))
        .map(|(k, w)| {
            let (small, large) = match w.files.len() <= commit.files.len() {
                true => (&w.files, &commit.files),
                false => (&commit.files, &w.files),
            };
            let shared = small.iter().filter(|f| large.contains(*f)).count();
            (k, shared, w.end.saturating_sub(w.start))
        })
        .filter(|&(_, shared, _)| shared > 0)
        .max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)).then(b.0.cmp(&a.0)))
        .map(|(k, _, _)| k)
}

/// Whether each commit was undone by a revert that itself still stands.
///
/// A revert of a revert puts the work back — `git revert` of the revert is
/// how people reapply — so a commit is reverted only by a revert that is not
/// itself reverted. And a revert only counts as far as it reached: a commit on
/// the default branch is not undone by a revert sitting on somebody's side
/// branch, while a side-branch commit is undone by one anywhere.
fn reverted(commits: &[Commit]) -> Vec<bool> {
    let targets: Vec<(usize, &str)> = commits
        .iter()
        .enumerate()
        .flat_map(|(r, c)| c.reverts.iter().map(move |t| (r, t.as_str())))
        .collect();
    let by: Vec<Vec<usize>> = commits
        .iter()
        .enumerate()
        .map(|(i, c)| {
            targets
                .iter()
                .filter(|&&(r, t)| r != i && c.sha.starts_with(t))
                .map(|&(r, _)| r)
                .collect()
        })
        .collect();

    fn go(i: usize, commits: &[Commit], by: &[Vec<usize>], memo: &mut [Option<bool>]) -> bool {
        if let Some(v) = memo[i] {
            return v;
        }
        // Provisionally standing, so a cycle — two commits each claiming to
        // revert the other, which only a hand-written message can produce —
        // ends rather than recursing forever.
        memo[i] = Some(false);
        let undone = by[i].iter().any(|&r| {
            (!commits[i].on_default || commits[r].on_default) && !go(r, commits, by, memo)
        });
        memo[i] = Some(undone);
        undone
    }
    let mut memo = vec![None; commits.len()];
    (0..commits.len())
        .map(|i| go(i, commits, &by, &mut memo))
        .collect()
}

/// Attribute every commit to at most one session's work, and classify the
/// work by what its commits became.
///
/// Pure: no git, no clock — `now` is passed in — so every rule here is tested
/// on plain data.
pub fn settle(commits: &[Commit], works: &[Work], now: i64) -> Settled {
    let owner: Vec<Option<usize>> = commits.iter().map(|c| owner(c, works)).collect();
    let undone = reverted(commits);
    let mut mine: Vec<Vec<usize>> = vec![Vec::new(); works.len()];
    for (c, o) in owner.iter().enumerate() {
        if let Some(k) = o {
            mine[*k].push(c);
        }
    }
    let class = works
        .iter()
        .zip(&mine)
        .map(|(w, cs)| {
            if cs.iter().any(|&c| commits[c].on_default && !undone[c]) {
                Class::Shipped
            } else if cs.iter().any(|&c| undone[c]) {
                Class::Reverted
            } else if !cs.is_empty() {
                Class::Unmerged
            } else if now <= w.end.saturating_add(GRACE_SECS) {
                Class::Open
            } else {
                Class::Uncommitted
            }
        })
        .collect();
    Settled {
        owner,
        reverted: undone,
        class,
    }
}

/// The repository a file is in: the nearest directory above it holding a
/// `.git`, which is a directory in a main checkout and a file in a worktree or
/// submodule.
///
/// A walk rather than `git rev-parse --show-toplevel`, because the answer is
/// needed for every file every session edited — thousands — and a process per
/// directory is most of a second where a `stat` per level is nothing. Every
/// directory the walk passes through is cached, so a second file in the same
/// tree costs one lookup. A file since deleted still resolves: the walk only
/// asks whether `.git` exists at each level, not whether the file does.
fn repo_of(path: &str, cache: &mut HashMap<PathBuf, Option<PathBuf>>) -> Option<PathBuf> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return None;
    }
    let mut dir = path.parent()?;
    let mut walked: Vec<PathBuf> = Vec::new();
    let found = loop {
        if let Some(hit) = cache.get(dir) {
            break hit.clone();
        }
        walked.push(dir.to_path_buf());
        if dir.join(".git").exists() {
            break Some(dir.to_path_buf());
        }
        match dir.parent() {
            Some(up) => dir = up,
            None => break None,
        }
    };
    for d in walked {
        cache.insert(d, found.clone());
    }
    found
}

/// Run git in `root`, read-only, and return its stdout.
///
/// The environment is scrubbed of the variables that would point git
/// somewhere other than `-C` — cctop run from inside a git hook inherits
/// `GIT_DIR` — and `GIT_OPTIONAL_LOCKS=0` keeps git from refreshing the index
/// behind our back, which is the one write a read can otherwise cause.
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "log.showSignature=false", "-c", "core.quotePath=off"])
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let line = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        return Err(match line {
            "" => format!("git exited with {}", out.status),
            l => l.trim().to_string(),
        });
    }
    Ok(out.stdout)
}

/// The record separator and unit separator git is asked to put between
/// commits and fields. Neither appears in a commit message anyone typed, and
/// file names come NUL-separated under `-z`, so nothing needs escaping.
const RS: char = '\x1e';
const US: char = '\x1f';

/// Parse `git log -z --name-only --format=<RS>%H %at<US>%s<US>%B<US>`.
///
/// Each commit is `RS`, the header, then a NUL and a newline git inserts before
/// the NUL-separated names. Split on `RS` first, then on `US`; whatever
/// follows the last `US` is the names.
fn parse_log(raw: &[u8], root: &Path) -> Vec<Commit> {
    let text = String::from_utf8_lossy(raw);
    text.split(RS)
        .filter(|c| !c.trim().is_empty())
        .filter_map(|chunk| {
            let mut parts = chunk.splitn(4, US);
            let head = parts.next()?;
            let subject = parts.next()?;
            let body = parts.next()?;
            let names = parts.next().unwrap_or("");
            let (sha, at) = head.trim().split_once(' ')?;
            let files = names
                .split('\0')
                .map(|n| n.trim_start_matches('\n'))
                .filter(|n| !n.is_empty())
                .map(|n| root.join(n).to_string_lossy().into_owned())
                .collect();
            Some(Commit {
                sha: sha.to_string(),
                at: at.trim().parse().ok()?,
                subject: subject.trim().to_string(),
                files,
                reverts: reverts_named(body),
                on_default: false,
            })
        })
        .collect()
}

/// The shas a commit message says it reverts.
///
/// Seven hex digits at least, which is git's own shortest abbreviation; any
/// fewer and "This reverts commit abc" in a sentence about something else
/// would match half the history.
fn reverts_named(body: &str) -> Vec<String> {
    const MARK: &str = "This reverts commit ";
    body.match_indices(MARK)
        .filter_map(|(at, _)| {
            let sha: String = body[at + MARK.len()..]
                .chars()
                .take_while(|c| c.is_ascii_hexdigit())
                .collect();
            (sha.len() >= 7).then(|| sha.to_ascii_lowercase())
        })
        .collect()
}

/// The refs that count as "shipped": the remote's default branch, and the
/// local default branch, whichever exist.
///
/// Both, not the first found. The remote's is the honest answer for work that
/// goes through review, but cctop never fetches, so a remote-tracking ref is
/// only as fresh as the last `git fetch` somebody ran — and a merged pull
/// request read through a stale `origin/main` would report as unmerged. The
/// local branch covers that, and covers the solo repository that is committed
/// to on `main` and pushed later. Committing to the local default branch is a
/// decision to keep the work, which is what this is asking.
///
/// `HEAD` only when neither exists, for a repository whose one branch has
/// some other name.
fn default_refs(root: &Path) -> Result<Vec<String>, String> {
    let listed = git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            "refs/remotes/origin/master",
            "refs/heads/main",
            "refs/heads/master",
        ],
    )?;
    let have: Vec<String> = String::from_utf8_lossy(&listed)
        .lines()
        .map(str::to_string)
        .collect();
    let first = |names: &[&str]| {
        names
            .iter()
            .find(|n| have.iter().any(|h| h == *n))
            .map(|n| n.to_string())
    };
    let mut refs: Vec<String> = [
        first(&[
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            "refs/remotes/origin/master",
        ]),
        first(&["refs/heads/main", "refs/heads/master"]),
    ]
    .into_iter()
    .flatten()
    .collect();
    if refs.is_empty() {
        refs.push("HEAD".into());
    }
    Ok(refs)
}

/// How far before the oldest session the default branch is walked, so a
/// commit whose committer date is a little behind its author date — or a
/// machine whose clock was — is not dropped by `--since` and misread as
/// unmerged.
const REACH_SLACK_SECS: i64 = 30 * 24 * 60 * 60;

/// What one repository said.
#[derive(Debug, Clone, Default)]
pub struct Read {
    pub commits: Vec<Commit>,
    /// The refs the default branch was read from, for the report to name.
    pub default: Vec<String>,
}

/// Every commit since `since` on any branch, and which are on the default one.
///
/// Three processes: the refs, the log, and the default branch's reachable set.
/// `--all` with the stash and notes excluded, because both are commits that
/// were never anybody's work on a branch — a stash's index commit has one
/// parent and would otherwise read as an unmerged commit. Merges are left out
/// too: a merge's own diff is the conflict resolution, and attributing it would
/// let one `git merge` of a long branch claim every file on it for whichever
/// session happened to be open.
fn read_repo(root: &Path, since: i64) -> Result<Read, String> {
    let default = default_refs(root)?;
    let since_arg = format!("--since=@{since}");
    let log = git(
        root,
        &[
            "log",
            "--exclude=refs/stash",
            "--exclude=refs/notes/*",
            "--all",
            "--no-merges",
            "--no-renames",
            &since_arg,
            "-z",
            "--name-only",
            "--format=%x1e%H %at%x1f%s%x1f%B%x1f",
        ],
    )?;
    let mut commits = parse_log(&log, root);
    if commits.is_empty() {
        return Ok(Read { commits, default });
    }
    let reach_arg = format!("--since=@{}", since.saturating_sub(REACH_SLACK_SECS));
    let mut args: Vec<&str> = vec!["rev-list", &reach_arg];
    args.extend(default.iter().map(String::as_str));
    let reach = git(root, &args)?;
    let on: HashSet<&str> = std::str::from_utf8(&reach)
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .collect();
    for c in &mut commits {
        c.on_default = on.contains(c.sha.as_str());
    }
    Ok(Read { commits, default })
}

/// One repository in the report.
#[derive(Debug, Clone, Default)]
pub struct Repo {
    pub path: PathBuf,
    /// Why git could not be read here, if it could not.
    pub error: Option<String>,
    pub default: Vec<String>,
    /// Commits in the window that was read, and how many went to a session
    /// the report is about.
    pub commits: usize,
    pub attributed: usize,
}

/// One session's place in one repository.
#[derive(Debug, Clone)]
pub struct Part {
    pub repo: usize,
    pub class: Class,
    /// Files it edited here, which is how its cost is divided between
    /// repositories when it worked in more than one.
    pub files: usize,
}

/// One session and what became of it.
#[derive(Debug, Clone)]
pub struct Outcome<'a> {
    pub analysis: &'a Analysis,
    pub class: Class,
    pub parts: Vec<Part>,
    /// The commits attributed to it, oldest first.
    pub commits: Vec<Credit>,
}

/// A commit credited to a session, with what decided its class.
#[derive(Debug, Clone)]
pub struct Credit {
    pub sha: String,
    pub subject: String,
    pub on_default: bool,
    pub reverted: bool,
}

impl Outcome<'_> {
    /// The session's cost, or `None` where it has no price — zero included,
    /// because a model missing from the price table bills `$0.00` and is not
    /// free for it.
    pub fn usd(&self) -> Option<f64> {
        let a = self.analysis;
        (a.cost_available && a.cost > 0.0).then_some(a.cost)
    }
}

/// Everything `cctop yield` prints.
#[derive(Debug, Clone, Default)]
pub struct Report<'a> {
    pub outcomes: Vec<Outcome<'a>>,
    pub repos: Vec<Repo>,
    /// Sessions in scope that edited no file by path, and how many of those
    /// wrote through the shell — which names no file, so cannot be matched.
    pub without_edits: usize,
    pub shell_only: usize,
}

/// Seconds since the epoch of an ISO timestamp.
fn secs(ts: &str) -> Option<i64> {
    crate::util::parse_ts(ts).map(|t| t.timestamp())
}

/// Read every repository the selected sessions edited in, and settle them.
///
/// `all` is every session, `selected` the ones the report is about. The two
/// differ under `--provider` and `--since`, and attribution has to run over
/// all of them: a commit a Claude session made is not Codex's for want of the
/// Claude session being filtered out of the report, and with only Codex in
/// the running it would have been handed to whichever Codex session shared a
/// file with it.
pub fn build<'a>(all: &'a [Analysis], selected: &[&'a Analysis], now: i64) -> Report<'a> {
    let chosen: HashSet<*const Analysis> = selected.iter().map(|a| *a as *const _).collect();
    let mut cache: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();

    // Per session, its edited files grouped by repository, with the first
    // edit in each.
    let mut repo_ix: HashMap<PathBuf, usize> = HashMap::new();
    let mut repos: Vec<Repo> = Vec::new();
    let mut works: Vec<(usize, Work)> = Vec::new(); // (repo, work)
    let mut in_git: Vec<bool> = vec![false; all.len()];
    for (i, a) in all.iter().enumerate() {
        let mut here: HashMap<usize, Work> = HashMap::new();
        let mut end = secs(&a.last_active).unwrap_or(i64::MIN);
        for e in a.edited.values() {
            if let Some(t) = secs(&e.last) {
                end = end.max(t);
            }
        }
        for (path, e) in &a.edited {
            // A file inside `.git` — a commit message, a hook — is never
            // itself committed, and would leave every session that wrote one
            // looking uncommitted.
            if path.contains("/.git/") {
                continue;
            }
            let Some(root) = repo_of(path, &mut cache) else {
                continue;
            };
            let r = *repo_ix.entry(root.clone()).or_insert_with(|| {
                repos.push(Repo {
                    path: root,
                    ..Default::default()
                });
                repos.len() - 1
            });
            let w = here.entry(r).or_insert_with(|| Work {
                session: i,
                files: HashSet::new(),
                start: i64::MAX,
                end,
            });
            w.files.insert(path.clone());
            if let Some(t) = secs(&e.first) {
                w.start = w.start.min(t);
            }
        }
        in_git[i] = !here.is_empty();
        let mut here: Vec<(usize, Work)> = here.into_iter().collect();
        here.sort_by_key(|(r, _)| *r);
        works.extend(here);
    }

    // Only the repositories a selected session worked in are read: the rest
    // could only ever compete for commits no reported session can win.
    let wanted: HashSet<usize> = works
        .iter()
        .filter(|(_, w)| chosen.contains(&(&all[w.session] as *const _)))
        .map(|(r, _)| *r)
        .collect();
    let reads: Vec<(usize, Result<Read, String>)> = repos
        .par_iter()
        .enumerate()
        .filter(|(r, _)| wanted.contains(r))
        .map(|(r, repo)| {
            let since = works
                .iter()
                .filter(|(wr, w)| *wr == r && w.start != i64::MAX)
                .map(|(_, w)| w.start)
                .min()
                .unwrap_or(now);
            (r, read_repo(&repo.path, since))
        })
        .collect();

    // One commit list across repositories, deduplicated by sha: a worktree
    // outside `.claude/worktrees/` resolves to a root of its own, but its
    // commits are the main repository's, and one commit must not be won once
    // per spelling of the same checkout.
    let mut by_sha: HashMap<String, (Commit, HashSet<usize>)> = HashMap::new();
    let mut unreadable: HashSet<usize> = HashSet::new();
    for (r, read) in reads {
        match read {
            Ok(read) => {
                repos[r].default = read.default;
                for c in read.commits {
                    let slot = by_sha.entry(c.sha.clone()).or_insert_with(|| {
                        (
                            Commit {
                                files: HashSet::new(),
                                ..c.clone()
                            },
                            HashSet::new(),
                        )
                    });
                    slot.0.files.extend(c.files);
                    slot.0.on_default |= c.on_default;
                    slot.1.insert(r);
                }
            }
            Err(e) => {
                repos[r].error = Some(e);
                unreadable.insert(r);
            }
        }
    }
    let mut merged: Vec<(Commit, HashSet<usize>)> = by_sha.into_values().collect();
    merged.sort_by(|a, b| a.0.at.cmp(&b.0.at).then(a.0.sha.cmp(&b.0.sha)));
    let (commits, seen_in): (Vec<Commit>, Vec<HashSet<usize>>) = merged.into_iter().unzip();

    // Work in an unreadable repository takes part in nothing: it has no
    // commits to win, and must not be called uncommitted for that.
    let live: Vec<usize> = (0..works.len())
        .filter(|&k| !unreadable.contains(&works[k].0))
        .collect();
    let live_works: Vec<Work> = live.iter().map(|&k| works[k].1.clone()).collect();
    let settled = settle(&commits, &live_works, now);
    let mut class: Vec<Class> = vec![Class::Unreadable; works.len()];
    for (j, &k) in live.iter().enumerate() {
        class[k] = settled.class[j];
    }

    for (c, rs) in seen_in.iter().enumerate() {
        for &r in rs {
            repos[r].commits += 1;
            // Only to a session in the report: under `--since` most of what
            // was read went to older sessions, and counting those would claim
            // a match rate the table below does not show.
            let to_chosen = settled.owner[c]
                .is_some_and(|j| chosen.contains(&(&all[live_works[j].session] as *const _)));
            if to_chosen {
                repos[r].attributed += 1;
            }
        }
    }
    let mut won: HashMap<usize, Vec<usize>> = HashMap::new();
    for (c, o) in settled.owner.iter().enumerate() {
        if let Some(j) = o {
            won.entry(live_works[*j].session).or_default().push(c);
        }
    }

    let mut parts: HashMap<usize, Vec<Part>> = HashMap::new();
    for (k, (r, w)) in works.iter().enumerate() {
        parts.entry(w.session).or_default().push(Part {
            repo: *r,
            class: class[k],
            files: w.files.len(),
        });
    }

    let mut report = Report {
        repos,
        ..Default::default()
    };
    for (i, a) in all.iter().enumerate() {
        if !chosen.contains(&(a as *const _)) {
            continue;
        }
        if a.edited.is_empty() {
            report.without_edits += 1;
            if a.bash_writes > 0 {
                report.shell_only += 1;
            }
            continue;
        }
        let parts = parts.remove(&i).unwrap_or_default();
        let class = match in_git[i] {
            true => parts
                .iter()
                .map(|p| p.class)
                .min()
                .unwrap_or(Class::NotInGit),
            false => Class::NotInGit,
        };
        let commits = won
            .get(&i)
            .map(|cs| {
                cs.iter()
                    .map(|&c| Credit {
                        sha: commits[c].sha.clone(),
                        subject: commits[c].subject.clone(),
                        on_default: commits[c].on_default,
                        reverted: settled.reverted[c],
                    })
                    .collect()
            })
            .unwrap_or_default();
        report.outcomes.push(Outcome {
            analysis: a,
            class,
            parts,
            commits,
        });
    }
    report
}

/// Spend and sessions per class, for one row of any table.
#[derive(Debug, Clone, Default)]
pub struct Tally {
    pub sessions: usize,
    /// Sessions with no price, counted in `sessions` and not in any dollars.
    pub unpriced: usize,
    pub usd: f64,
    pub by_class: HashMap<Class, (usize, f64)>,
}

impl Tally {
    fn add(&mut self, class: Class, usd: Option<f64>) {
        self.sessions += 1;
        let slot = self.by_class.entry(class).or_default();
        slot.0 += 1;
        match usd {
            Some(v) => {
                self.usd += v;
                slot.1 += v;
            }
            None => self.unpriced += 1,
        }
    }

    /// Only dollars, for a share of something already counted elsewhere.
    fn spend(&mut self, class: Class, usd: f64) {
        self.usd += usd;
        self.by_class.entry(class).or_default().1 += usd;
    }

    /// The class's share of this row's priced spend.
    pub fn share(&self, class: Class) -> Option<f64> {
        let usd = self.by_class.get(&class).map_or(0.0, |s| s.1);
        (self.usd > 0.0).then(|| usd * 100.0 / self.usd)
    }
}

impl Report<'_> {
    pub fn totals(&self) -> Tally {
        let mut t = Tally::default();
        for o in &self.outcomes {
            t.add(o.class, o.usd());
        }
        t
    }

    /// Per model, by the agents in each session.
    ///
    /// Each agent's cost goes to its own model, the way `compare` credits it,
    /// so a Haiku subagent's search inside an Opus session is Haiku's spend.
    /// The outcome does not split: whether the session's work shipped is one
    /// fact about the session, and every agent in it shares that fate. A
    /// commit cannot be traced to the subagent that wrote a given line, and
    /// pretending otherwise would invent precision. Crediting the whole
    /// session to its dominant model was the simpler design, and would put
    /// every subagent's dollars under a model that did not spend them.
    pub fn by_model(&self) -> Vec<(String, Tally)> {
        let mut rows: HashMap<String, Tally> = HashMap::new();
        for o in &self.outcomes {
            let priced = o.usd().is_some();
            let mut seen: HashSet<String> = HashSet::new();
            for s in o.analysis.agent_slices() {
                if s.model.is_empty() {
                    continue;
                }
                let row = rows.entry(s.model.clone()).or_default();
                if seen.insert(s.model.clone()) {
                    row.sessions += 1;
                    row.by_class.entry(o.class).or_default().0 += 1;
                    if !priced {
                        row.unpriced += 1;
                    }
                }
                if priced && s.cost > 0.0 {
                    row.spend(o.class, s.cost);
                }
            }
        }
        sorted(rows)
    }

    /// Per repository, with a session that worked in several split between
    /// them by the share of its files each held, and classed by what happened
    /// *there* — which can differ from its overall class.
    pub fn by_repo(&self) -> Vec<(usize, Tally)> {
        let mut rows: HashMap<usize, Tally> = HashMap::new();
        for o in &self.outcomes {
            let files: usize = o.parts.iter().map(|p| p.files).sum();
            for p in &o.parts {
                let row = rows.entry(p.repo).or_default();
                row.sessions += 1;
                row.by_class.entry(p.class).or_default().0 += 1;
                match o.usd() {
                    Some(v) if files > 0 => {
                        row.spend(p.class, v * p.files as f64 / files as f64);
                    }
                    _ => row.unpriced += 1,
                }
            }
        }
        let mut out: Vec<(usize, Tally)> = rows.into_iter().collect();
        out.sort_by(|a, b| {
            b.1.usd
                .total_cmp(&a.1.usd)
                .then(b.1.sessions.cmp(&a.1.sessions))
                .then(self.repos[a.0].path.cmp(&self.repos[b.0].path))
        });
        out
    }

    /// The classes any session landed in, in display order, so a table does
    /// not carry a column of dashes for something that never happened.
    fn present(&self) -> Vec<Class> {
        let seen: HashSet<Class> = self.outcomes.iter().map(|o| o.class).collect();
        Class::ALL
            .into_iter()
            .filter(|c| {
                seen.contains(c)
                    || matches!(c, Class::Shipped | Class::Unmerged | Class::Uncommitted)
            })
            .collect()
    }
}

fn sorted(rows: HashMap<String, Tally>) -> Vec<(String, Tally)> {
    let mut out: Vec<(String, Tally)> = rows.into_iter().collect();
    out.sort_by(|a, b| {
        b.1.usd
            .total_cmp(&a.1.usd)
            .then(b.1.sessions.cmp(&a.1.sessions))
            .then(a.0.cmp(&b.0))
    });
    out
}

/// A path with the home directory as `~`, because every repository on the
/// machine shares that prefix and it is the least informative part.
fn tilde(path: &Path) -> String {
    let shown = path.to_string_lossy().into_owned();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && shown.starts_with(&home) => {
            format!("~{}", &shown[home.len()..])
        }
        _ => shown,
    }
}

fn money(v: f64, unpriced: bool) -> String {
    match (v > 0.0, unpriced) {
        (true, _) => crate::util::adaptive_usd(v),
        // Nothing priced at all: not "$0.00", which reads as free.
        (false, true) => "—".into(),
        (false, false) => "$0".into(),
    }
}

fn pct(v: Option<f64>) -> String {
    v.map(|v| format!("{v:.0}%")).unwrap_or_else(|| "—".into())
}

/// The report as text.
pub fn report(r: &Report) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    if r.outcomes.is_empty() {
        let _ = writeln!(
            out,
            "\n  No session edited a file by path, so there is nothing to match \
             against commits.\n"
        );
        return out;
    }
    let total = r.totals();
    let classes = r.present();
    let in_git = r
        .outcomes
        .iter()
        .filter(|o| o.class != Class::NotInGit)
        .count();
    let touched = r
        .outcomes
        .iter()
        .flat_map(|o| o.parts.iter().map(|p| p.repo))
        .collect::<HashSet<usize>>()
        .len();
    let commits: usize = r.repos.iter().map(|p| p.commits).sum();
    let attributed: usize = r.repos.iter().map(|p| p.attributed).sum();

    out.push('\n');
    let _ = writeln!(
        out,
        "  {} edited files{}; {} of them in {}.",
        plural(r.outcomes.len(), "session"),
        match total.usd > 0.0 {
            true => format!(", {} between them", crate::util::adaptive_usd(total.usd)),
            false => String::new(),
        },
        in_git,
        match touched {
            1 => "1 repository".to_string(),
            n => format!("{n} repositories"),
        },
    );
    let _ = writeln!(
        out,
        "  {} read from them; {} credited to these sessions.",
        plural(commits, "commit"),
        attributed,
    );
    out.push('\n');

    let _ = writeln!(
        out,
        "  {:<12} {:>8} {:>10} {:>6}",
        "", "sessions", "spend", "share"
    );
    for c in &classes {
        let (n, usd) = total.by_class.get(c).copied().unwrap_or_default();
        let _ = writeln!(
            out,
            "  {:<12} {:>8} {:>10} {:>6}",
            c.as_str(),
            n,
            money(usd, total.unpriced > 0),
            pct(total.share(*c)),
        );
    }
    out.push('\n');

    // Per model, then per repository, as shares of each row's own spend: the
    // question is "how much of what this model cost shipped", and dollars per
    // class would make every row's answer a sum the reader has to do.
    let models = r.by_model();
    let width = models
        .iter()
        .map(|(m, _)| m.chars().count())
        .chain(r.repos.iter().map(|p| tilde(&p.path).chars().count()))
        .max()
        .unwrap_or(0)
        .clamp(12, 44);
    let header = |out: &mut String, first: &str| {
        let _ = write!(out, "  {:<width$} {:>8} {:>10}", first, "sessions", "spend");
        for c in &classes {
            let _ = write!(out, " {:>11}", c.as_str());
        }
        out.push('\n');
    };
    let line = |out: &mut String, name: &str, t: &Tally| {
        let name: String = name.chars().take(width).collect();
        let _ = write!(
            out,
            "  {:<width$} {:>8} {:>10}",
            name,
            t.sessions,
            money(t.usd, t.unpriced > 0)
        );
        for c in &classes {
            let _ = write!(out, " {:>11}", pct(t.share(*c)));
        }
        out.push('\n');
    };
    header(&mut out, "model");
    for (m, t) in &models {
        line(&mut out, m, t);
    }
    out.push('\n');
    header(&mut out, "repository");
    for (k, t) in r.by_repo() {
        let repo = &r.repos[k];
        line(&mut out, &tilde(&repo.path), &t);
        if let Some(e) = &repo.error {
            let _ = writeln!(out, "    git could not read it: {e}");
        }
    }
    out.push('\n');

    // The sessions a reader would go and look at: the most money that did not
    // end up anywhere.
    let mut lost: Vec<&Outcome> = r
        .outcomes
        .iter()
        .filter(|o| {
            matches!(
                o.class,
                Class::Unmerged | Class::Uncommitted | Class::Reverted
            )
        })
        .filter(|o| o.usd().is_some())
        .collect();
    lost.sort_by(|a, b| b.analysis.cost.total_cmp(&a.analysis.cost));
    if !lost.is_empty() {
        let _ = writeln!(out, "  The most spend that did not ship");
        let shown: Vec<&&Outcome> = lost.iter().take(5).collect();
        let label = shown
            .iter()
            .map(|o| o.analysis.label.chars().count())
            .max()
            .unwrap_or(0)
            .min(40);
        for o in shown {
            let a = o.analysis;
            let day = a.last_active.get(..10).unwrap_or("");
            let _ = writeln!(
                out,
                "  {:>10}  {:<11}  {}  {:<label$}  {}",
                crate::util::adaptive_usd(a.cost),
                o.class.as_str(),
                day,
                a.label.chars().take(label).collect::<String>(),
                a.model,
            );
        }
        out.push('\n');
    }

    let mut say = |lines: &[&str]| {
        for l in lines {
            let _ = writeln!(out, "  {l}");
        }
        out.push('\n');
    };
    say(&[
        "A commit is credited to the session that edited the most of its",
        "files while open, or up to four hours after its last activity;",
        "a tie goes to the tighter window, and no commit counts twice.",
        "shipped is on the default branch — origin's or the local one —",
        "unmerged is committed only elsewhere, and in progress is a",
        "session active less than four hours ago with nothing committed.",
    ]);
    say(&[
        "A heuristic, not a record. A squash or rebase merge leaves the",
        "original commits off the default branch, so that work reads as",
        "unmerged unless the merge commit itself fell inside the window.",
        "Nothing is fetched, so a stale origin can under-report too.",
    ]);
    if r.without_edits > 0 {
        let first = format!(
            "{} edited no file by path and are not counted.",
            plural(r.without_edits, "session")
        );
        let second = format!(
            "{} of them wrote through the shell, which names no file.",
            r.shell_only
        );
        match r.shell_only {
            0 => say(&[first.as_str()]),
            _ => say(&[first.as_str(), second.as_str()]),
        }
    }
    if total.unpriced > 0 {
        let note = format!(
            "{} have no price: counted in sessions, not in spend or share.",
            plural(total.unpriced, "session")
        );
        say(&[note.as_str()]);
    }
    out
}

/// The report as JSON, for scripting.
pub fn as_json(r: &Report) -> String {
    let classes = |t: &Tally| {
        let mut m = serde_json::Map::new();
        for c in Class::ALL {
            if let Some(&(n, usd)) = t.by_class.get(&c) {
                m.insert(
                    c.key().into(),
                    serde_json::json!({
                        "sessions": n,
                        "usd": (usd > 0.0).then_some(usd),
                        "share_pct": t.share(c),
                    }),
                );
            }
        }
        serde_json::Value::Object(m)
    };
    let tally = |t: &Tally| {
        serde_json::json!({
            "sessions": t.sessions,
            "unpriced_sessions": t.unpriced,
            "usd": (t.usd > 0.0).then_some(t.usd),
            "classes": classes(t),
        })
    };
    let total = r.totals();
    let models: Vec<serde_json::Value> = r
        .by_model()
        .iter()
        .map(|(m, t)| {
            let mut v = tally(t);
            v["model"] = m.clone().into();
            v
        })
        .collect();
    let repos: Vec<serde_json::Value> = r
        .by_repo()
        .iter()
        .map(|(k, t)| {
            let repo = &r.repos[*k];
            let mut v = tally(t);
            v["path"] = repo.path.to_string_lossy().into_owned().into();
            v["readable"] = repo.error.is_none().into();
            v["error"] = repo.error.clone().into();
            v["default_refs"] = repo.default.clone().into();
            v["commits_read"] = repo.commits.into();
            v["commits_attributed"] = repo.attributed.into();
            v
        })
        .collect();
    let sessions: Vec<serde_json::Value> = r
        .outcomes
        .iter()
        .map(|o| {
            let a = o.analysis;
            serde_json::json!({
                "label": a.label,
                "provider": a.provider.as_str(),
                "model": a.model,
                "last_active": a.last_active,
                "usd": o.usd(),
                "class": o.class.key(),
                "files_edited": a.edited.len(),
                "repositories": o.parts.iter().map(|p| serde_json::json!({
                    "path": r.repos[p.repo].path.to_string_lossy(),
                    "class": p.class.key(),
                    "files": p.files,
                })).collect::<Vec<_>>(),
                "commits": o.commits.iter().map(|c| serde_json::json!({
                    "sha": c.sha,
                    "subject": c.subject,
                    "on_default_branch": c.on_default,
                    "reverted": c.reverted,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let doc = serde_json::json!({
        "method": "path-overlap-in-window",
        "grace_hours": GRACE_SECS / 3600,
        "sessions_without_edits": r.without_edits,
        "sessions_shell_writes_only": r.shell_only,
        "total": tally(&total),
        "models": models,
        "repositories": repos,
        "sessions": sessions,
        "caveat": "Heuristic. A commit is credited to the session that edited the \
                   most of its files within that session's window (first edit to \
                   last activity plus grace_hours); ties go to the tighter window and \
                   no commit counts twice. Squash and rebase merges leave the original \
                   commits off the default branch, so unmerged over-reports for \
                   repositories merged that way. Nothing is fetched.",
    });
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".into())
}

pub const HELP: &str = "\
cctop yield — did the spend ship?

USAGE:
  cctop yield [--provider NAME] [--since SPAN] [--json]

Matches each session to the git commits that contain its work, and says what
became of the money:

  shipped      a commit of its work is on the default branch
  reverted     its work was committed and later reverted
  unmerged     committed, but only on a branch that never landed
  uncommitted  it edited files in a repository and nothing was committed
  in progress  nothing committed yet, but it was active in the last 4 hours
  not in git   everything it edited is outside any repository

A commit is matched by the files it shares with a session, inside a window
from the session's first edit to four hours after its last activity. The most
shared files wins; a tie goes to the tighter window; no commit is counted for
two sessions. The default branch is origin's and the local main or master,
whichever exist.

It is a heuristic. A squash or rebase merge leaves the original commits off
the default branch, so that work reads as unmerged.

OPTIONS:
  --provider NAME  Only this harness: claude, codex, cursor, gemini, opencode,
                   pi, windsurf. Every session still competes for commits; only
                   the report is narrowed.
  --since SPAN     Only sessions active in the last SPAN — 24h, 7d, 2w — or
                   since a date, 2026-09-01.
  --json           Machine-readable, for scripting: every session with its
                   class and the commits credited to it.
  -h, --help       This.

It reads transcripts and runs git log; it fetches nothing and writes nothing.
";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::insight::{Edited, Task};
    use crate::pricing::Provider;

    fn work(session: usize, files: &[&str], start: i64, end: i64) -> Work {
        Work {
            session,
            files: files.iter().map(|f| f.to_string()).collect(),
            start,
            end,
        }
    }

    fn commit(sha: &str, at: i64, files: &[&str], on_default: bool) -> Commit {
        Commit {
            sha: sha.into(),
            at,
            subject: format!("commit {sha}"),
            files: files.iter().map(|f| f.to_string()).collect(),
            reverts: Vec::new(),
            on_default,
        }
    }

    const HOUR: i64 = 3600;
    /// Long after every window here, so nothing reads as in progress unless a
    /// test means it to.
    const LATER: i64 = 1_000 * HOUR;

    /// The reason to read paths at all. A session that wrote three of a
    /// commit's files owns it over one that wrote one, even when the second
    /// was the one open right next to the commit.
    #[test]
    fn overlap_beats_proximity() {
        let works = [
            work(0, &["/r/a", "/r/b", "/r/c"], 0, 10 * HOUR),
            work(1, &["/r/a"], 9 * HOUR, 10 * HOUR),
        ];
        let c = commit("aaaaaaa1", 10 * HOUR, &["/r/a", "/r/b", "/r/c"], true);
        assert_eq!(owner(&c, &works), Some(0));
    }

    /// Equal overlap is the one place time decides, and then the tighter
    /// window wins: a twenty-minute session around a commit says more than one
    /// left open all week.
    #[test]
    fn a_tie_goes_to_the_tightest_window() {
        let works = [
            work(0, &["/r/a"], 0, 100 * HOUR),
            work(1, &["/r/a"], 50 * HOUR, 51 * HOUR),
        ];
        let c = commit("aaaaaaa1", 51 * HOUR, &["/r/a"], true);
        assert_eq!(owner(&c, &works), Some(1));
    }

    /// A commit that shares no file with any session is nobody's, however
    /// close in time — the clock alone is exactly the guess this avoids.
    #[test]
    fn a_commit_with_no_shared_file_is_nobodys() {
        let works = [work(0, &["/r/a"], 0, HOUR)];
        let c = commit("aaaaaaa1", HOUR / 2, &["/r/z"], true);
        assert_eq!(owner(&c, &works), None);
    }

    /// The window runs from the first edit to the grace after last activity,
    /// inclusive at both ends, and not a second past either.
    #[test]
    fn the_window_is_first_edit_to_grace_after_last_activity() {
        let works = [work(0, &["/r/a"], 10 * HOUR, 12 * HOUR)];
        let at = |t: i64| owner(&commit("aaaaaaa1", t, &["/r/a"], true), &works);
        assert_eq!(at(10 * HOUR - 1), None, "before the first edit");
        assert_eq!(at(10 * HOUR), Some(0));
        assert_eq!(
            at(12 * HOUR + GRACE_SECS),
            Some(0),
            "the grace's last second"
        );
        assert_eq!(at(12 * HOUR + GRACE_SECS + 1), None, "past the grace");
    }

    /// Each commit goes to one session, and a second session that also
    /// touched the file gets nothing from it — so it is uncommitted, not
    /// shipped on the back of somebody else's commit.
    #[test]
    fn one_commit_goes_to_one_session() {
        let works = [
            work(0, &["/r/a", "/r/b"], 0, HOUR),
            work(1, &["/r/a"], 0, HOUR),
        ];
        let commits = [commit("aaaaaaa1", HOUR, &["/r/a", "/r/b"], true)];
        let s = settle(&commits, &works, LATER);
        assert_eq!(s.owner, vec![Some(0)]);
        assert_eq!(s.class, vec![Class::Shipped, Class::Uncommitted]);
    }

    /// Committed to a branch that never reached the default one is unmerged;
    /// one reachable commit among several is enough to call it shipped.
    #[test]
    fn shipped_means_reachable_from_the_default_branch() {
        let works = [work(0, &["/r/a"], 0, HOUR), work(1, &["/r/b"], 0, HOUR)];
        let commits = [
            commit("aaaaaaa1", HOUR, &["/r/a"], false),
            commit("aaaaaaa2", HOUR, &["/r/a"], true),
            commit("bbbbbbb1", HOUR, &["/r/b"], false),
        ];
        let s = settle(&commits, &works, LATER);
        assert_eq!(s.class, vec![Class::Shipped, Class::Unmerged]);
    }

    /// A revert undoes the work; a revert of that revert puts it back; and a
    /// revert left on a side branch does not undo what is on the default one.
    #[test]
    fn a_revert_undoes_work_unless_it_was_itself_reverted() {
        let works = [work(0, &["/r/a"], 0, HOUR)];
        let mut revert = commit("bbbbbbb2", 50 * HOUR, &["/r/a"], true);
        revert.reverts = vec!["aaaaaaa1".into()];
        let landed = commit("aaaaaaa1ffff", HOUR, &["/r/a"], true);

        let s = settle(&[landed.clone(), revert.clone()], &works, LATER);
        assert_eq!(s.reverted, vec![true, false]);
        assert_eq!(s.class, vec![Class::Reverted]);

        let mut reapply = commit("ccccccc3", 60 * HOUR, &["/r/a"], true);
        reapply.reverts = vec!["bbbbbbb2".into()];
        let s = settle(&[landed.clone(), revert.clone(), reapply], &works, LATER);
        assert_eq!(s.reverted, vec![false, true, false]);
        assert_eq!(s.class, vec![Class::Shipped]);

        let mut stray = revert;
        stray.on_default = false;
        let s = settle(&[landed, stray], &works, LATER);
        assert_eq!(s.class, vec![Class::Shipped], "the revert never landed");
    }

    /// A session still inside its window has not failed to commit; it has not
    /// committed yet. Calling it uncommitted would file everything running now
    /// as abandoned.
    #[test]
    fn a_session_inside_its_window_is_in_progress() {
        let works = [work(0, &["/r/a"], 0, 10 * HOUR)];
        assert_eq!(settle(&[], &works, 11 * HOUR).class, vec![Class::Open]);
        assert_eq!(
            settle(&[], &works, 10 * HOUR + GRACE_SECS + 1).class,
            vec![Class::Uncommitted]
        );
    }

    /// The message `git revert` writes, abbreviated or not, and nothing shorter
    /// than git's own shortest abbreviation.
    #[test]
    fn revert_messages_name_their_commit() {
        assert_eq!(
            reverts_named("Revert \"x\"\n\nThis reverts commit 0123456789ABCDEF.\n"),
            vec!["0123456789abcdef".to_string()]
        );
        assert!(reverts_named("This reverts commit abc, sort of").is_empty());
    }

    /// The shape `git log -z --name-only` actually prints, NUL before the
    /// names included, with paths joined onto the repository root.
    #[test]
    fn the_log_parses_into_commits_with_absolute_paths() {
        let raw = "\x1eaaaa 100\x1ffirst\x1ffirst\n\nbody\x1f\0\nsrc/a.rs\0b.txt\0\
                   \x1ebbbb 200\x1fRevert\x1fThis reverts commit aaaa1234.\x1f\0\nsrc/a.rs\0";
        let commits = parse_log(raw.as_bytes(), Path::new("/repo"));
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].sha, "aaaa");
        assert_eq!(commits[0].at, 100);
        assert_eq!(commits[0].subject, "first");
        let mut files: Vec<&String> = commits[0].files.iter().collect();
        files.sort();
        assert_eq!(files, vec!["/repo/b.txt", "/repo/src/a.rs"]);
        assert_eq!(commits[1].reverts, vec!["aaaa1234".to_string()]);
    }

    fn analysis(edits: &[(&str, &str)], last_active: &str, cost: f64) -> Analysis {
        Analysis {
            provider: Provider::Claude,
            label: "repo".into(),
            last_active: last_active.into(),
            model: "m".into(),
            cost,
            cost_available: true,
            task: Task::Coding,
            calls: 10,
            errors: 0,
            records_outcomes: true,
            edits: edits.len() as u64,
            bash_writes: 0,
            reads: 0,
            files_edited: edits.len() as u64,
            files_one_shot: 0,
            read_paths: Default::default(),
            junk_reads: 0,
            junk_tokens: 0,
            reread_tokens: 0,
            rereads: 0,
            cache_read: 0,
            input_total: 0,
            active_ms: 0,
            edited: edits
                .iter()
                .map(|(p, ts)| {
                    (
                        p.to_string(),
                        Edited {
                            first: ts.to_string(),
                            last: ts.to_string(),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            slices: Vec::new(),
            files_reworked: 0,
            truncated: false,
        }
    }

    /// A real repository, end to end: one session's work merged to main, one
    /// left on a side branch, one never committed, and one outside any
    /// repository — each read back as what it is.
    #[test]
    fn a_real_repository_sorts_shipped_from_unmerged_from_uncommitted() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let run = |args: &[&str], date: &str| {
            let out = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        let write = |name: &str, body: &str| std::fs::write(repo.join(name), body).unwrap();
        let date = "2026-01-01T09:00:00+00:00";
        run(&["init", "-q"], date);
        run(&["symbolic-ref", "HEAD", "refs/heads/main"], date);
        write("base.txt", "base");
        run(&["add", "."], date);
        run(&["commit", "-qm", "base"], date);

        // Session A edits a.txt; its commit lands on main.
        write("a.txt", "a");
        run(&["add", "a.txt"], "2026-01-01T10:30:00+00:00");
        run(&["commit", "-qm", "a"], "2026-01-01T10:30:00+00:00");

        // Session B edits b.txt; committed on a side branch only.
        run(&["checkout", "-qb", "side"], date);
        write("b.txt", "b");
        run(&["add", "b.txt"], "2026-01-01T11:30:00+00:00");
        run(&["commit", "-qm", "b"], "2026-01-01T11:30:00+00:00");
        run(&["checkout", "-q", "main"], date);

        let path = |f: &str| repo.join(f).to_string_lossy().into_owned();
        let outside = dir.path().join("loose.txt").to_string_lossy().into_owned();
        let all = [
            analysis(
                &[(&path("a.txt"), "2026-01-01T10:00:00Z")],
                "2026-01-01T10:20:00Z",
                3.0,
            ),
            analysis(
                &[(&path("b.txt"), "2026-01-01T11:00:00Z")],
                "2026-01-01T11:20:00Z",
                2.0,
            ),
            analysis(
                &[(&path("c.txt"), "2026-01-01T12:00:00Z")],
                "2026-01-01T12:20:00Z",
                1.0,
            ),
            analysis(
                &[(&outside, "2026-01-01T12:00:00Z")],
                "2026-01-01T12:20:00Z",
                0.5,
            ),
            analysis(&[], "2026-01-01T12:20:00Z", 0.25),
        ];
        let selected: Vec<&Analysis> = all.iter().collect();
        let now = secs("2026-02-01T00:00:00Z").unwrap();
        let r = build(&all, &selected, now);

        let classes: Vec<Class> = r.outcomes.iter().map(|o| o.class).collect();
        assert_eq!(
            classes,
            vec![
                Class::Shipped,
                Class::Unmerged,
                Class::Uncommitted,
                Class::NotInGit
            ]
        );
        assert_eq!(r.without_edits, 1);
        assert_eq!(r.repos.len(), 1);
        assert_eq!(r.repos[0].error, None);
        assert_eq!(r.repos[0].default, vec!["refs/heads/main".to_string()]);
        assert_eq!(
            r.repos[0].attributed, 2,
            "a and b; base predates every session"
        );
        assert_eq!(r.outcomes[0].commits.len(), 1);
        assert_eq!(r.outcomes[0].commits[0].subject, "a");

        let total = r.totals();
        assert_eq!(total.share(Class::Shipped).map(|v| v.round()), Some(46.0));
        let text = report(&r);
        assert!(text.contains("shipped"), "{text}");
        let json: serde_json::Value = serde_json::from_str(&as_json(&r)).unwrap();
        assert_eq!(json["sessions"][1]["class"], "unmerged");
    }

    /// A directory git cannot read is reported as such, never as a session
    /// that committed nothing.
    #[test]
    fn an_unreadable_repository_is_not_uncommitted() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("broken");
        // A `.git` that is neither a repository nor a gitdir pointer.
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let f = repo.join("x.rs").to_string_lossy().into_owned();
        let all = [analysis(
            &[(&f, "2026-01-01T10:00:00Z")],
            "2026-01-01T10:00:00Z",
            1.0,
        )];
        let selected: Vec<&Analysis> = all.iter().collect();
        let r = build(&all, &selected, secs("2026-02-01T00:00:00Z").unwrap());
        assert_eq!(r.outcomes[0].class, Class::Unreadable);
        assert!(r.repos[0].error.is_some());
    }
}
