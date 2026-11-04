//! Land a session's work as a pull request: push its branch to `origin` and
//! open a GitHub pull request for it with `gh`, title and body prefilled.
//!
//! The last step after `F` forks a session into a worktree of its own. Two
//! calls, because a person stands between them: [`prepare`] answers whether
//! there is anything to land and what it would say, and [`open`] pushes and
//! opens once the title has been read.
//!
//! # What it refuses, and why it refuses rather than fixes
//!
//! A checkout outside git, a detached `HEAD`, a branch that is the default one,
//! a branch with no commits the default lacks, no `origin`, and a `gh` that is
//! missing or logged out. Each is a sentence on the status line rather than a
//! repair: the fixes — branching off `main`, committing, logging in — are each
//! a decision about someone's repository or account, and none of them is one a
//! keystroke on a dashboard should make on their behalf.
//!
//! Never a force-push and never a merge. A push that is refused because the
//! remote branch has moved on is reported as git said it; settling that is a
//! job for someone with the checkout open.
//!
//! # Where it pushes and opens
//!
//! `origin`, and a pull request into `origin`'s default branch, named to `gh`
//! with `--repo` so a clone with several remotes cannot make it ask which.
//!
//! ponytail: one remote. A fork workflow — push to your fork, open against
//! upstream — is not modelled; `gh pr create` by hand does that.
//!
//! # Nothing it runs can ask
//!
//! Every command runs without a controlling terminal, with stdin closed and
//! git's and gh's prompts turned off, under a deadline. The TUI owns the
//! terminal: an ssh passphrase prompt or a `gh` question would draw over the
//! table and wait for keys the dashboard is reading.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The `gh` a real run uses. Tests hand [`prepare`] and [`open`] a stub
/// script's path instead, and under test this name is refused outright, so a
/// test that forgot to never reaches GitHub as whoever ran `cargo test`.
pub const GH: &str = "gh";

/// Commits listed in the body before the rest are counted.
const MAX_COMMITS: usize = 30;
/// Files listed in the body before the rest are counted.
const MAX_FILES: usize = 50;
/// Long enough for a push over a slow link, short enough that a hung one ends.
const SLOW: Duration = Duration::from_secs(120);
/// A question git or gh answers from local state, or one API call.
const QUICK: Duration = Duration::from_secs(30);

/// One file the pull request changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStat {
    pub path: String,
    /// `None` for a binary file.
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

/// What [`prepare`] found: everything [`open`] needs, and what the person is
/// shown before it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub root: PathBuf,
    pub branch: String,
    /// `origin`'s default branch, which the pull request goes into.
    pub base: String,
    /// `host/owner/repo` for `gh --repo`; `None` when `origin` is a path on
    /// disk, which only a test's repository has.
    pub repo: Option<String>,
    /// Subjects of the commits the base lacks, oldest first.
    pub commits: Vec<String>,
    /// Commits past [`MAX_COMMITS`], counted.
    pub more_commits: usize,
    /// The files those commits change against the merge-base.
    pub files: Vec<FileStat>,
    /// Paths `git status` reports: edits and new files that are not in any
    /// commit, and so not in the pull request.
    pub uncommitted: usize,
    /// An open pull request for this branch already, by URL.
    pub existing: Option<String>,
}

/// Check that `dir`'s checkout has work to land, and gather what the pull
/// request would say. `say` is told each step as it starts.
///
/// Runs git and gh, some of it over the network; it belongs on a worker thread.
pub fn prepare(dir: &Path, gh: &str, say: &mut dyn FnMut(String)) -> Result<Plan, String> {
    say("Checking the branch…".into());
    let (_, root) = crate::worktree::locate(dir).ok_or("not in a git repository")?;
    let branch = ask(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok_or("HEAD is detached: check out a branch first")?;
    let origin = ask(&root, &["remote", "get-url", "origin"])
        .ok_or("this repository has no origin remote to push to")?;
    let base = default_branch(&root)?;
    if branch == base || branch == "main" || branch == "master" {
        return Err(format!(
            "{branch} is the default branch — a pull request needs a branch of its own (F forks one)"
        ));
    }
    // origin's copy when there is one: a local `main` that has fallen behind
    // would count commits already merged upstream as this branch's.
    let base_ref = [
        format!("refs/remotes/origin/{base}"),
        format!("refs/heads/{base}"),
    ]
    .into_iter()
    .find(|r| ask(&root, &["rev-parse", "--verify", "--quiet", r]).is_some())
    .ok_or_else(|| format!("there is no {base} to compare {branch} with — git fetch origin"))?;
    let range = format!("{base_ref}..HEAD");
    let log = run(
        git(&root).args(["log", "--reverse", "--format=%s", &range, "--"]),
        QUICK,
    )?;
    let all: Vec<String> = log.lines().map(str::to_string).collect();
    if all.is_empty() {
        return Err(format!("{branch} has no commits that {base} does not have"));
    }
    let more_commits = all.len().saturating_sub(MAX_COMMITS);
    let commits = all.into_iter().take(MAX_COMMITS).collect();
    // Three dots: against where the branch left the base, which is the diff a
    // reviewer is shown, rather than against wherever the base has got to since.
    let numstat = run(
        git(&root).args([
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            "--numstat",
            "-z",
            &format!("{base_ref}...HEAD"),
            "--",
        ]),
        QUICK,
    )?;
    let files = crate::changes::parse_numstat(&numstat)
        .into_iter()
        .map(|f| FileStat {
            path: f.path,
            added: f.added,
            removed: f.removed,
        })
        .collect();
    let status = run(
        git(&root).args(["status", "--porcelain", "-z", "--no-renames"]),
        QUICK,
    )?;
    let uncommitted = status.split('\0').filter(|r| !r.is_empty()).count();
    let key = crate::worktree::normalise_url(&origin, &root);
    let repo = (!key.starts_with('/')).then_some(key);

    say("Checking gh…".into());
    let host = repo.as_deref().and_then(|r| r.split('/').next());
    gh_ready(gh, &root, host)?;

    say(format!("Looking for an open pull request for {branch}…"));
    let existing = open_pr(gh, &root, repo.as_deref(), &branch);
    Ok(Plan {
        root,
        branch,
        base,
        repo,
        commits,
        more_commits,
        files,
        uncommitted,
        existing,
    })
}

/// Push the branch, then open its pull request — or, when one is open already,
/// only push, and answer with that one's URL. `say` is told each step.
pub fn open(
    plan: &Plan,
    title: &str,
    body: &str,
    gh: &str,
    say: &mut dyn FnMut(String),
) -> Result<String, String> {
    say(format!("Pushing {} to origin…", plan.branch));
    // A refspec rather than the bare name, so nothing in the repository's
    // config — `push.default`, a `remote.origin.push` with a `+` — decides
    // what goes where, or whether it is forced.
    let refspec = format!("refs/heads/{0}:refs/heads/{0}", plan.branch);
    run(
        git(&plan.root).args(["push", "--set-upstream", "origin", &refspec]),
        SLOW,
    )
    .map_err(|why| format!("could not push {}: {why}", plan.branch))?;
    if let Some(url) = &plan.existing {
        return Ok(url.clone());
    }
    say("Opening the pull request…".into());
    let mut cmd = gh_cmd(gh, &plan.root)?;
    cmd.args(["pr", "create", "--head", &plan.branch, "--base", &plan.base]);
    if let Some(repo) = &plan.repo {
        cmd.args(["--repo", repo]);
    }
    cmd.args(["--title", title, "--body", body]);
    let out = run(&mut cmd, SLOW).map_err(|why| format!("gh could not open it: {why}"))?;
    // gh prints the new pull request's URL as its last line.
    out.lines()
        .rev()
        .map(str::trim)
        .find(|l| l.starts_with("https://") || l.starts_with("http://"))
        .map(str::to_string)
        .ok_or_else(|| "gh opened it but did not say where".to_string())
}

/// The title the box opens with: the session's own title when it has one, the
/// commit's subject when there is only one, and the branch name made readable
/// otherwise.
///
/// Not [`Session::display_label`] as it stands: without a title that falls
/// back to the directory, and a pull request called `~/code/web` says nothing.
///
/// [`Session::display_label`]: crate::session::Session::display_label
pub fn title(session_title: Option<&str>, plan: &Plan) -> String {
    let named = session_title
        .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(str::to_string);
    let one_commit = (plan.commits.len() == 1 && plan.more_commits == 0)
        .then(|| plan.commits[0].trim().to_string())
        .filter(|s| !s.is_empty());
    let text = named.or(one_commit).unwrap_or_else(|| {
        let words = plan.branch.rsplit('/').next().unwrap_or(&plan.branch);
        let words = words.replace(['-', '_'], " ");
        let mut chars = words.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => plan.branch.clone(),
        }
    });
    crate::util::clip(&text, 120)
}

/// The pull request's description: its commits and the files they change.
///
/// What the commits *are*, not what was said to get them. A handoff brief
/// quotes the session's prompts, which is right for an agent picking the work
/// up and wrong for a page that may be public: a prompt is a private
/// conversation with an agent, and can hold anything that was pasted into it.
/// So nothing from the transcript goes in, and the brief — which needs no
/// model call, and could have been used — is left for handoffs.
///
/// The file list is the pull request's own diff against the merge-base, not
/// the Changes panel's since-the-session-started one: the panel counts
/// uncommitted work and work from before a fork, and neither is what a
/// reviewer will be shown.
pub fn body(plan: &Plan) -> String {
    let mut out = String::from("## Commits\n\n");
    for subject in &plan.commits {
        out.push_str(&format!("- {}\n", subject.trim()));
    }
    if plan.more_commits > 0 {
        out.push_str(&format!(
            "- … and {}\n",
            crate::util::plural(plan.more_commits, "more commit")
        ));
    }
    if !plan.files.is_empty() {
        out.push_str("\n## Files\n\n");
        for f in plan.files.iter().take(MAX_FILES) {
            let counts = match (f.added, f.removed) {
                (Some(a), Some(r)) => format!("+{a} −{r}"),
                _ => "binary".to_string(),
            };
            out.push_str(&format!("- `{}` {counts}\n", f.path.replace('`', "'")));
        }
        let more = plan.files.len().saturating_sub(MAX_FILES);
        if more > 0 {
            out.push_str(&format!(
                "- … and {}\n",
                crate::util::plural(more, "more file")
            ));
        }
    }
    out.push_str("\nOpened from cctop.\n");
    out
}

/// `origin`'s default branch: what `origin/HEAD` points at, or failing that
/// whichever of `main` and `master` origin has.
///
/// `origin/HEAD` is set by a clone and by `git remote set-head`, and is missing
/// from a repository that was `init`ed and had a remote added later — hence the
/// fallback, which is the guess every other tool makes too.
fn default_branch(root: &Path) -> Result<String, String> {
    if let Some(head) = ask(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) && let Some(name) = head.strip_prefix("origin/")
    {
        return Ok(name.to_string());
    }
    ["main", "master"]
        .into_iter()
        .find(|b| {
            let r = format!("refs/remotes/origin/{b}");
            ask(root, &["rev-parse", "--verify", "--quiet", &r]).is_some()
        })
        .map(str::to_string)
        .ok_or_else(|| {
            "cannot tell origin's default branch — git remote set-head origin --auto".to_string()
        })
}

/// Whether `gh` is installed and logged in, for `host` when the remote names
/// one.
fn gh_ready(gh: &str, root: &Path, host: Option<&str>) -> Result<(), String> {
    if !crate::shim::is_command(gh) {
        return Err("gh is not installed — https://cli.github.com, then gh auth login".into());
    }
    let mut cmd = gh_cmd(gh, root)?;
    cmd.args(["auth", "status"]);
    if let Some(host) = host {
        cmd.args(["--hostname", host]);
    }
    run(&mut cmd, QUICK)
        .map(|_| ())
        .map_err(|_| "gh is not logged in — run gh auth login".into())
}

/// The URL of an open pull request whose head is `branch`, if there is one.
///
/// A failure reads as none: `gh pr view` exits non-zero for "no pull request"
/// and for everything else alike, and the cost of a wrong "none" is that
/// `gh pr create` runs and says, itself, that one exists.
fn open_pr(gh: &str, root: &Path, repo: Option<&str>, branch: &str) -> Option<String> {
    let mut cmd = gh_cmd(gh, root).ok()?;
    cmd.args(["pr", "view", branch, "--json", "url,state"]);
    if let Some(repo) = repo {
        cmd.args(["--repo", repo]);
    }
    let out = run(&mut cmd, QUICK).ok()?;
    let v: serde_json::Value = serde_json::from_str(&out).ok()?;
    // `gh pr view` finds closed and merged ones too; a new branch tip after a
    // merge deserves a pull request of its own.
    (v["state"].as_str() == Some("OPEN"))
        .then(|| v["url"].as_str().map(str::to_string))
        .flatten()
}

/// `gh`, in the checkout, with nothing that would make it ask or chatter.
fn gh_cmd(gh: &str, root: &Path) -> Result<Command, String> {
    if crate::under_test() && gh == GH {
        return Err("the real gh is never run under test".into());
    }
    let mut cmd = Command::new(gh);
    cmd.current_dir(root)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("NO_COLOR", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    Ok(cmd)
}

/// git in the checkout, as [`crate::changes`] runs it, with its password
/// prompt off for the push.
fn git(root: &Path) -> Command {
    let mut cmd = crate::changes::git(root);
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

/// A short git question's first line, or `None`.
fn ask(root: &Path, args: &[&str]) -> Option<String> {
    let out = run(git(root).args(args), QUICK).ok()?;
    let line = out.lines().next()?.trim().to_string();
    (!line.is_empty()).then_some(line)
}

/// Run `cmd` to completion or `deadline`, whichever is first, and answer its
/// stdout — or the last thing it said on stderr, which is where git and gh
/// both put their reason.
fn run(cmd: &mut Command, deadline: Duration) -> Result<String, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // No controlling terminal: an ssh asked for a passphrase opens /dev/tty
    // rather than reading stdin, and that terminal is the one the TUI draws on.
    // SAFETY: setsid is async-signal-safe and allocates nothing.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let name = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd.spawn().map_err(|e| format!("{name}: {e}"))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            String::from_utf8_lossy(&buf).into_owned()
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{name} took longer than {}s", deadline.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("{name}: {e}")),
        }
    };
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    if status.success() {
        return Ok(out);
    }
    let reason = err
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty() && !l.starts_with("hint:"))
        .map(|l| {
            l.trim_start_matches("fatal: ")
                .trim_start_matches("error: ")
        })
        .unwrap_or("it failed");
    Err(reason.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn git_ok(dir: &Path, args: &[&str]) {
        let out = crate::worktree::git_in(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A clone of a bare `origin` on `main`, with one commit there, checked
    /// out on `main`.
    fn clone(tmp: &Path) -> PathBuf {
        let origin = tmp.join("origin.git");
        git_ok(tmp, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        let seed = tmp.join("seed");
        git_ok(tmp, &["init", "-q", "-b", "main", "seed"]);
        std::fs::write(seed.join("a.txt"), "a\n").unwrap();
        git_ok(&seed, &["add", "."]);
        git_ok(&seed, &["commit", "-q", "-m", "root"]);
        git_ok(&seed, &["push", "-q", &origin.to_string_lossy(), "main"]);
        git_ok(tmp, &["clone", "-q", "origin.git", "work"]);
        tmp.join("work")
    }

    /// A `gh` that is logged in, has no pull request for anything until one
    /// is created, and prints the URL it was asked to make. Every call is
    /// logged beside it, one argv per line.
    fn stub_gh(tmp: &Path, existing: bool) -> String {
        let path = tmp.join("gh");
        let log = tmp.join("gh.log");
        let view = match existing {
            true => r#"echo '{"url":"https://github.com/o/r/pull/7","state":"OPEN"}'"#,
            false => "echo 'no pull requests found' >&2; exit 1",
        };
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\ncase \"$1 $2\" in\n\
                 'auth status') exit 0 ;;\n\
                 'pr view') {view} ;;\n\
                 'pr create') echo 'Creating pull request'; echo 'https://github.com/o/r/pull/8' ;;\n\
                 esac\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn quiet() -> impl FnMut(String) {
        |_| {}
    }

    #[test]
    fn refusals_name_what_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let gh = stub_gh(tmp.path(), false);
        let outside = tmp.path().join("plain");
        std::fs::create_dir(&outside).unwrap();
        let err = prepare(&outside, &gh, &mut quiet()).unwrap_err();
        assert!(err.contains("not in a git repository"), "{err}");

        let work = clone(tmp.path());
        let err = prepare(&work, &gh, &mut quiet()).unwrap_err();
        assert!(err.contains("main is the default branch"), "{err}");

        git_ok(&work, &["checkout", "-q", "-b", "agent-x"]);
        let err = prepare(&work, &gh, &mut quiet()).unwrap_err();
        assert!(err.contains("no commits that main does not have"), "{err}");

        let missing = tmp.path().join("no-such-gh").to_string_lossy().into_owned();
        git_ok(&work, &["commit", "-q", "--allow-empty", "-m", "x"]);
        let err = prepare(&work, &missing, &mut quiet()).unwrap_err();
        assert!(err.contains("gh is not installed"), "{err}");

        // The real one is refused under test, whatever is on PATH.
        let err = prepare(&work, GH, &mut quiet()).unwrap_err();
        assert!(err.contains("gh"), "{err}");

        git_ok(&work, &["checkout", "-q", "--detach"]);
        let err = prepare(&work, &gh, &mut quiet()).unwrap_err();
        assert!(err.contains("detached"), "{err}");
    }

    #[test]
    fn a_repository_without_origin_has_nowhere_to_push() {
        let tmp = tempfile::tempdir().unwrap();
        let gh = stub_gh(tmp.path(), false);
        git_ok(tmp.path(), &["init", "-q", "-b", "feature", "r"]);
        let err = prepare(&tmp.path().join("r"), &gh, &mut quiet()).unwrap_err();
        assert!(err.contains("no origin"), "{err}");
    }

    /// The whole path against a real bare origin and a stub gh: the plan
    /// counts the commits, files and uncommitted work, the push lands on
    /// origin, and the URL gh printed comes back.
    #[test]
    fn prepare_then_open_pushes_and_creates() {
        let tmp = tempfile::tempdir().unwrap();
        let gh = stub_gh(tmp.path(), false);
        let work = clone(tmp.path());
        git_ok(&work, &["checkout", "-q", "-b", "agent/fix-the-thing"]);
        std::fs::write(work.join("b.txt"), "one\ntwo\n").unwrap();
        git_ok(&work, &["add", "b.txt"]);
        git_ok(&work, &["commit", "-q", "-m", "Add b"]);
        std::fs::write(work.join("a.txt"), "a\nmore\n").unwrap();
        git_ok(&work, &["commit", "-q", "-am", "Grow a"]);
        std::fs::write(work.join("loose.txt"), "not committed\n").unwrap();

        let mut steps = Vec::new();
        let plan = prepare(&work, &gh, &mut |s| steps.push(s)).expect("plan");
        assert_eq!(plan.branch, "agent/fix-the-thing");
        assert_eq!(plan.base, "main");
        assert_eq!(plan.commits, ["Add b", "Grow a"]);
        assert_eq!(plan.files.len(), 2);
        assert_eq!(plan.uncommitted, 1);
        assert_eq!(plan.existing, None);
        assert_eq!(plan.repo, None, "a path origin is no --repo");
        assert!(steps[0].starts_with("Checking the branch"), "{steps:?}");

        let url = open(&plan, "Fix the thing", &body(&plan), &gh, &mut quiet()).expect("open");
        assert_eq!(url, "https://github.com/o/r/pull/8");
        let origin = tmp.path().join("origin.git");
        git_ok(
            &origin,
            &["rev-parse", "--verify", "refs/heads/agent/fix-the-thing"],
        );
        let log = std::fs::read_to_string(tmp.path().join("gh.log")).unwrap();
        assert!(
            log.contains("pr create --head agent/fix-the-thing --base main --title Fix the thing"),
            "{log}"
        );
    }

    /// An open pull request is reused: the branch is still pushed, and nothing
    /// is created.
    #[test]
    fn an_open_pull_request_is_not_opened_twice() {
        let tmp = tempfile::tempdir().unwrap();
        let gh = stub_gh(tmp.path(), true);
        let work = clone(tmp.path());
        git_ok(&work, &["checkout", "-q", "-b", "topic"]);
        git_ok(&work, &["commit", "-q", "--allow-empty", "-m", "x"]);
        let plan = prepare(&work, &gh, &mut quiet()).expect("plan");
        assert_eq!(
            plan.existing.as_deref(),
            Some("https://github.com/o/r/pull/7")
        );
        let url = open(&plan, "t", "b", &gh, &mut quiet()).expect("open");
        assert_eq!(url, "https://github.com/o/r/pull/7");
        let log = std::fs::read_to_string(tmp.path().join("gh.log")).unwrap();
        assert!(!log.contains("pr create"), "{log}");
    }

    fn plan(branch: &str, commits: &[&str]) -> Plan {
        Plan {
            root: "/r".into(),
            branch: branch.into(),
            base: "main".into(),
            repo: Some("github.com/o/r".into()),
            commits: commits.iter().map(|s| s.to_string()).collect(),
            more_commits: 0,
            files: vec![
                FileStat {
                    path: "src/a.rs".into(),
                    added: Some(3),
                    removed: Some(1),
                },
                FileStat {
                    path: "logo.png".into(),
                    added: None,
                    removed: None,
                },
            ],
            uncommitted: 0,
            existing: None,
        }
    }

    #[test]
    fn the_title_prefers_the_session_then_the_commit_then_the_branch() {
        let two = plan("agent/fix-login_hint", &["a", "b"]);
        assert_eq!(
            title(Some("Fix the login hint\nmore"), &two),
            "Fix the login hint"
        );
        assert_eq!(title(Some("  "), &two), "Fix login hint");
        assert_eq!(title(None, &plan("x", &["Only commit"])), "Only commit");
    }

    #[test]
    fn the_body_lists_commits_and_files_and_nothing_said() {
        let body = body(&plan("b", &["Add a", "Fix a"]));
        assert!(
            body.starts_with("## Commits\n\n- Add a\n- Fix a\n"),
            "{body}"
        );
        assert!(body.contains("- `src/a.rs` +3 −1\n"), "{body}");
        assert!(body.contains("- `logo.png` binary\n"), "{body}");
    }
}
