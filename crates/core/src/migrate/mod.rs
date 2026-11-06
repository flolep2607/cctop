//! Moving a session to another machine: its conversation, and the files it
//! was working on.
//!
//! `X` on a row asks another machine to take a session over. The files travel
//! as a git snapshot — the working tree as it is, uncommitted and untracked
//! work included, ignored files not — and the transcript travels beside them,
//! its paths rewritten for where the checkout lands. The other machine puts
//! both in place and resumes the agent there; only once that agent is running
//! is the one here stopped. It is a move, not a copy: two agents writing one
//! conversation is the one outcome every step is ordered to rule out.
//!
//! # The order, and why
//!
//! 1. Here, [`send`]: the guards, then `hello` (does that cctop know how), the
//!    agent frozen, the snapshot taken, and `plan` (where would it land).
//! 2. The person confirms, and may edit the directory.
//! 3. The bundle and the transcript go across in one `receive`. There,
//!    [`receive`] verifies them, places the files ([`place`]), installs the
//!    transcript ([`transcript`]) and resumes the agent, writing an undo step
//!    for everything it changes into a journal. Any failure undoes all of it.
//! 4. Here again, a fence: the checkout and the transcript must be exactly as
//!    they were snapshotted. If either moved, the far side is told to undo and
//!    the agent here is thawed — nothing moved. Otherwise the agent here is
//!    stopped, a marker says where it went, and the far side is told to keep
//!    what it did (`finish`).
//!
//! At no point are there two live writers: before the far agent starts, this
//! one is frozen; after it is confirmed, this one is gone. A move that fails
//! once its bytes have left thaws this one only after the far side has said,
//! by the move's id, that nothing of it runs there — see [`send`].
//!
//! # What can move
//!
//! Claude Code's CLI sessions and Codex's: a transcript is a JSONL file whose
//! working-directory fields can be rewritten. Everything else is refused with
//! the reason, before anything is frozen — see [`movable`].
//!
//! # Transport
//!
//! Over ssh to a `--host` ([`wire`]), every parameter as JSON on stdin: the far
//! side's shell joins argv into one line, and a path with a space in it would
//! become two words. Over a sibling's tunnel, in signed chunks — see [`wire`].

pub mod place;
pub mod receive;
pub mod send;
pub mod snapshot;
pub mod transcript;
pub mod wire;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::pricing::Provider;
use crate::session::Session;

/// Bumped whenever a message below changes shape. Both ends must agree; the
/// side that is behind is named in the refusal.
pub const PROTOCOL: u32 = 1;

/// The most a move may carry: a bundle and a transcript together. A checkout
/// past this is not one to push down an ssh pipe on a keystroke.
pub const MAX_TOTAL: u64 = 2 * 1024 * 1024 * 1024;

/// What the receiving disk must still have free after a move.
pub const SPARE_DISK: u64 = 1024 * 1024 * 1024;

/// The harnesses whose sessions can move. Their transcripts are JSONL files
/// with the working directory in known fields, which is what lets one be
/// re-homed under a different path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Harness {
    Claude,
    Codex,
}

impl Harness {
    pub fn provider(self) -> Provider {
        match self {
            Harness::Claude => Provider::Claude,
            Harness::Codex => Provider::Codex,
        }
    }

    /// The command that resumes it, which the far side has to have.
    pub fn command(self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Codex => "codex",
        }
    }
}

/// `hello`: who answers, and what it can take.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub version: String,
    /// The movable harnesses installed there.
    pub harnesses: Vec<Harness>,
}

/// The repository a session is in, as the far side needs it to find or make
/// a checkout of it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Repo {
    /// `remote.origin.url`, when there is one — set on a fresh clone so it
    /// can fetch and push like the original.
    pub origin: Option<String>,
    /// The repository's identity, as [`crate::worktree::Repo::key`] spells
    /// it: what makes a checkout there the same repository as this one.
    pub key: String,
    /// The checkout directory's own name, for a new clone's default place.
    pub name: String,
    /// The commit the checkout is on.
    pub head: String,
    /// The branch it is on, if it is on one.
    pub branch: Option<String>,
    /// The session's directory below the checkout root, `/`-separated;
    /// empty for the root itself.
    pub rel_cwd: String,
}

/// `plan`: where would this land?
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ask {
    pub protocol: u32,
    pub harness: Harness,
    pub repo: Repo,
    /// A directory the person typed, instead of the far side's own choice.
    pub dir: Option<String>,
}

/// How the files would be placed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Into a clean checkout of the same repository, on the branch.
    Existing,
    /// Into a new worktree beside a checkout that has work of its own in it,
    /// or something running in it.
    Worktree,
    /// Into a new repository made from the bundle.
    Clone,
}

/// `plan`'s answer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Placement {
    pub kind: Kind,
    /// Where the checkout is, or will be.
    pub dir: String,
    /// The existing checkout a worktree is added to.
    pub root: Option<String>,
    /// Whether that repository already has the commit the session is on —
    /// a thin bundle then carries only the snapshot.
    pub has_head: bool,
    /// What the person should know first, shown in the confirmation.
    pub note: Option<String>,
}

/// What `receive` is told before the bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub protocol: u32,
    pub harness: Harness,
    pub session_id: String,
    pub repo: Repo,
    pub placement: Placement,
    /// The snapshot commit, which the bundle carries as [`MOVE_REF`]`/<id>`.
    pub snapshot: String,
    pub move_id: String,
    pub bundle: Blob,
    pub transcript: Blob,
    pub transcript_meta: transcript::Meta,
}

/// A file that follows the manifest on the stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blob {
    pub len: u64,
    pub sha256: String,
}

/// `receive`'s answer: the agent is running there.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Receipt {
    /// What `finish` or `abort` names.
    pub ticket: String,
    /// The session's id there: its own, unless that one was taken.
    pub session_id: String,
    /// The rmux session the agent runs in.
    pub rmux: String,
    /// The directory it runs in.
    pub dir: String,
    pub note: Option<String>,
}

/// `finish` and `abort` name a receive by its ticket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    pub protocol: u32,
    pub ticket: String,
}

/// Where the snapshot commit lives while it travels, on both sides.
pub const MOVE_REF: &str = "refs/cctop/move";

/// Every answer on the wire: one JSON line, `{"ok": …}` or `{"error": "…"}`.
/// An error is the far side's sentence, shown as it is.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer<T> {
    Ok(T),
    Error(String),
}

impl<T> From<Result<T, String>> for Answer<T> {
    fn from(r: Result<T, String>) -> Self {
        match r {
            Ok(v) => Answer::Ok(v),
            Err(e) => Answer::Error(e),
        }
    }
}

impl<T> Answer<T> {
    pub fn into_result(self) -> Result<T, String> {
        match self {
            Answer::Ok(v) => Ok(v),
            Answer::Error(e) => Err(e),
        }
    }
}

/// The harness a session could move as, or why it cannot move at all.
///
/// Asked before anything is frozen or sent, so a refusal costs nothing.
pub fn movable(session: &Session) -> Result<Harness, String> {
    if let Some(remote) = &session.remote {
        return Err(format!("it is on {} — move it from there", remote.host));
    }
    if let Some(owner) = &session.owner {
        return Err(crate::inject::not_theirs(owner));
    }
    if session.process_only() {
        return Err("no transcript claims this process — there is nothing to move".into());
    }
    if session.sandbox.is_some() {
        return Err("it runs in a sandbox, whose checkout is not this machine's to send".into());
    }
    if session.surface.is_desktop() {
        return Err("a desktop app's session cannot be resumed from a shell".into());
    }
    let harness = match session.provider {
        Provider::Claude => Harness::Claude,
        Provider::Codex => Harness::Codex,
        other => {
            return Err(format!(
                "{} sessions cannot move yet — only Claude Code's and Codex's transcripts \
                 can be re-homed under another path",
                other.as_str()
            ));
        }
    };
    if session.data_file.as_deref().is_none_or(|f| !f.is_file()) {
        return Err("its transcript is not on this machine".into());
    }
    // Mid-turn the tree is still changing under the snapshot, and a held
    // permission prompt is a tool call in flight that the far agent would not
    // know it had asked.
    if session.is_running()
        && matches!(
            session.activity_state,
            crate::session::ActivityState::Working | crate::session::ActivityState::Asking
        )
    {
        return Err("it is mid-turn — wait for its turn to end, then move it".into());
    }
    if session.work_dir().is_none() {
        return Err("its directory is gone from here".into());
    }
    Ok(harness)
}

/// Where cctop keeps what a receive is doing until it is finished or undone.
pub fn staging() -> PathBuf {
    crate::config::CACHE_DIR.join("move")
}

/// The record that a session here was moved away, so resuming it here — which
/// would fork the conversation — is refused with where it went.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Moved {
    pub machine: String,
    pub session_id: String,
    pub at: String,
}

fn moved_path(provider: Provider, id: &str) -> PathBuf {
    crate::config::CACHE_DIR
        .join("moved")
        .join(format!("{}-{id}.json", provider.as_str()))
}

/// Where this session was moved to, if it was.
pub fn moved_to(provider: Provider, id: &str) -> Option<Moved> {
    if !crate::remote_open::id_ok(id) {
        return None;
    }
    let text = std::fs::read_to_string(moved_path(provider, id)).ok()?;
    serde_json::from_str(&text).ok()
}

/// What a resume here says about a session that left.
pub fn moved_refusal(moved: &Moved) -> String {
    format!(
        "it was moved to {} on {} and runs there as {} — resuming it here would fork \
         the conversation",
        moved.machine,
        moved.at.get(..10).unwrap_or(&moved.at),
        moved.session_id
    )
}

pub(crate) fn record_moved(provider: Provider, id: &str, moved: &Moved) {
    let path = moved_path(provider, id);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(moved) {
        let _ = std::fs::write(path, text);
    }
}

pub(crate) fn clear_moved(provider: Provider, id: &str) {
    let _ = std::fs::remove_file(moved_path(provider, id));
}

/// sha256 of a file, hex.
pub(crate) fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// git as a move runs it, on either side: the checkout's own hooks and
/// fsmonitor never run, objects are checked as they arrive, submodules are
/// not followed, LFS does not fetch, and no transport but a local bundle file
/// is allowed.
///
/// The receiving side is the one this matters for. A bundle carries objects
/// and refs, never config or hooks — but the repository it lands in has its
/// own, and a `post-checkout` hook or an fsmonitor program there would run on
/// a person's say-so they never gave: they pressed `X` on another machine.
pub(crate) fn git(dir: &Path) -> Command {
    let mut cmd = crate::worktree::git_in(dir);
    cmd.env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.quotepath=off",
            "-c",
            "transfer.fsckObjects=true",
            "-c",
            "submodule.recurse=false",
            "-c",
            "protocol.allow=never",
            "-c",
            "protocol.file.allow=always",
            "-c",
            "advice.detachedHead=false",
        ]);
    // cctop's name on what it commits: a checkout with no identity can still
    // be snapshotted, and nothing appears under the user's name.
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

/// Run a git command for its stdout, or the first line it complained with.
pub(crate) fn run(cmd: &mut Command) -> Result<String, String> {
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
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A directory relative to a checkout, made only of plain components: no
/// `..`, no root, nothing that could walk out of where it is joined.
pub(crate) fn plain_relative(rel: &str) -> bool {
    rel.split('/')
        .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains('\0'))
        || rel.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_one_line_either_way() {
        let ok: Answer<u32> = Ok(3).into();
        assert_eq!(serde_json::to_string(&ok).unwrap(), r#"{"ok":3}"#);
        let err: Answer<u32> = Err("no".to_string()).into();
        assert_eq!(serde_json::to_string(&err).unwrap(), r#"{"error":"no"}"#);
        let back: Answer<u32> = serde_json::from_str(r#"{"error":"no"}"#).unwrap();
        assert_eq!(back.into_result(), Err("no".into()));
    }

    #[test]
    fn a_relative_directory_cannot_walk_out() {
        assert!(plain_relative(""));
        assert!(plain_relative("crates/core"));
        assert!(!plain_relative("../x"));
        assert!(!plain_relative("a/../../x"));
        assert!(!plain_relative("/etc"));
        assert!(!plain_relative("a//b"));
        assert!(!plain_relative("a/./b"));
    }
}
