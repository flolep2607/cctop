//! The receiving side: `cctop move hello | plan | receive | finish | abort`.
//!
//! Every subcommand reads its parameters as JSON on stdin and answers one JSON
//! line on stdout ([`Answer`]); a refusal is an answer too, so the exit status
//! is 0 whenever this cctop understood the question. An older cctop that does
//! not know `move` exits non-zero with clap's complaint, which is how the
//! sender tells the two apart.
//!
//! `receive` does the whole of a move's work here in one process — verify,
//! place, install, resume — writing an [`Undo`] into the ticket's journal
//! before each change takes effect. A failure undoes the journal at once. A
//! success keeps it until the sender says `finish` (keep everything) or
//! `abort` (undo everything); a ticket the sender never comes back for is
//! kept after [`STALE`], since by then the agent here is the only copy
//! running.
//!
//! A ticket is the sender's own move id, which it knows before a byte leaves:
//! a sender that lost the answer — a dropped ssh, a tunnel that timed out
//! while this side was still resuming — can still name the move to `abort`,
//! and must, because the agent here may be running. [`abort`] answers `Ok`
//! only when nothing of the move is left running here, so the sender thaws its
//! own agent only on that word.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{
    Answer, Ask, Harness, Hello, MAX_TOTAL, Manifest, PROTOCOL, Placement, Receipt, SPARE_DISK,
    Ticket, git, run, staging,
};

/// How long a received move waits for `finish` or `abort` before it is
/// simply kept.
pub const STALE: Duration = Duration::from_secs(30 * 60);

/// Where a move that was kept leaves its id, so a late `abort` for it is
/// told the truth — it is running here, and staying — instead of hearing
/// that nothing is here to undo.
fn kept_dir() -> PathBuf {
    staging().join("kept")
}

fn mark_kept(move_id: &str) {
    let dir = kept_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(move_id), b"");
}

fn was_kept(move_id: &str) -> bool {
    kept_dir().join(move_id).exists()
}

/// How long the resumed agent has to be up, and to stay up, before the move
/// counts as having arrived.
const START_WAIT: Duration = Duration::from_secs(8);
const SETTLE: Duration = Duration::from_secs(2);

/// One change a receive made, and how to take it back.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Undo {
    /// A directory this receive created.
    RemoveDir(PathBuf),
    /// A directory that was empty before, emptied again.
    EmptyDir(PathBuf),
    /// The snapshot's ref, fetched into a repository here.
    DeleteRef { repo: PathBuf, refname: String },
    /// A worktree this receive added.
    RemoveWorktree { repo: PathBuf, dir: PathBuf },
    /// A clean checkout switched to the session's work: back to the commit or
    /// branch it was on, its branch put back where it was (or removed, if this
    /// made it), and the files the work added removed.
    SwitchBack {
        root: PathBuf,
        prev: String,
        prev_branch: Option<String>,
        branch_was: Option<(String, Option<String>)>,
        added: Vec<String>,
    },
    /// A transcript installed into the harness's store.
    RemoveFile(PathBuf),
    /// A file git ignores here, copied aside before the snapshot replaced it:
    /// copied back.
    RestoreFile { from: PathBuf, to: PathBuf },
    /// The tab the agent was resumed in.
    KillTab(String),
}

/// The undo steps of one receive, written to disk as each is taken, so an
/// `abort` from a later process can still undo them.
pub struct Journal {
    path: PathBuf,
    steps: Vec<Undo>,
    notes: Vec<String>,
}

impl Journal {
    pub fn new(path: PathBuf) -> Journal {
        Journal {
            path,
            steps: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn load(path: PathBuf) -> Result<Journal, String> {
        let text = std::fs::read_to_string(&path).map_err(|_| "no such move here".to_string())?;
        let steps = serde_json::from_str(&text).map_err(|e| format!("the journal: {e}"))?;
        Ok(Journal {
            path,
            steps,
            notes: Vec::new(),
        })
    }

    /// Record a step. Written before the caller goes on, so a crash after
    /// the change still leaves the way back on disk.
    pub fn push(&mut self, step: Undo) -> Result<(), String> {
        self.steps.push(step);
        let text = serde_json::to_string(&self.steps).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, text).map_err(|e| format!("the journal: {e}"))
    }

    /// Something the person should hear about how it landed.
    pub fn note(&mut self, note: String) {
        self.notes.push(note);
    }

    /// Take back every step, newest first. Each is attempted whatever the one
    /// before did; what could not be undone is said.
    pub fn undo_all(&mut self) -> Vec<String> {
        let mut failed = Vec::new();
        while let Some(step) = self.steps.pop() {
            if let Err(why) = undo(&step) {
                failed.push(why);
            }
        }
        let _ = std::fs::remove_file(&self.path);
        failed
    }

    /// The move is kept: only the travelling ref goes.
    fn finish(&mut self) {
        for step in self.steps.drain(..) {
            if let Undo::DeleteRef { .. } = step {
                let _ = undo(&step);
            }
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn undo(step: &Undo) -> Result<(), String> {
    match step {
        Undo::KillTab(name) => crate::rmux::kill(name),
        Undo::RestoreFile { from, to } => std::fs::copy(from, to)
            .map(|_| ())
            .map_err(|e| format!("{}: {e}", to.display())),
        Undo::RemoveFile(path) => match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", path.display()))
            }
            _ => Ok(()),
        },
        Undo::DeleteRef { repo, refname } => {
            run(git(repo).args(["update-ref", "-d", refname])).map(|_| ())
        }
        Undo::RemoveWorktree { repo, dir } => {
            let removed = run(git(repo).args(["worktree", "remove", "--force"]).arg(dir));
            let _ = run(git(repo).args(["worktree", "prune"]));
            removed.map(|_| ())
        }
        Undo::SwitchBack {
            root,
            prev,
            prev_branch,
            branch_was,
            added,
        } => {
            // Detached first: the branch may be the one checked out, and git
            // will not move a branch out from under its own checkout.
            run(git(root).args(["switch", "-q", "-f", "--detach", prev]))?;
            if let Some((branch, was)) = branch_was {
                match was {
                    Some(sha) => run(git(root).args(["branch", "-f", branch, sha]))?,
                    None => run(git(root).args(["branch", "-D", branch]))?,
                };
            }
            if let Some(b) = prev_branch {
                run(git(root).args(["switch", "-q", "-f", b]))?;
            }
            for rel in added {
                if super::plain_relative(rel) {
                    let _ = std::fs::remove_file(root.join(rel));
                }
            }
            Ok(())
        }
        Undo::RemoveDir(dir) => {
            std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))
        }
        Undo::EmptyDir(dir) => {
            for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
                let path = entry.path();
                let _ = match entry.file_type().is_ok_and(|t| t.is_dir()) {
                    true => std::fs::remove_dir_all(&path),
                    false => std::fs::remove_file(&path),
                };
            }
            Ok(())
        }
    }
}

/// `hello`.
pub fn hello(version: &str) -> Hello {
    Hello {
        protocol: PROTOCOL,
        version: version.to_string(),
        harnesses: [Harness::Claude, Harness::Codex]
            .into_iter()
            .filter(|h| crate::shim::is_command(h.command()))
            .collect(),
    }
}

/// `plan`.
pub fn plan(ask: &Ask) -> Result<Placement, String> {
    protocol(ask.protocol)?;
    installed(ask.harness)?;
    super::place::propose(ask, &crate::config::HOME)
}

fn protocol(theirs: u32) -> Result<(), String> {
    match theirs.cmp(&PROTOCOL) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Less => Err(format!(
            "the sending cctop speaks move protocol {theirs} and this one {PROTOCOL} — update the sending one"
        )),
        std::cmp::Ordering::Greater => Err(format!(
            "the sending cctop speaks move protocol {theirs} and this one {PROTOCOL} — update this one (cctop --update)"
        )),
    }
}

fn installed(harness: Harness) -> Result<(), String> {
    match crate::shim::is_command(harness.command()) {
        true => Ok(()),
        false => Err(format!("{} is not installed here", harness.command())),
    }
}

/// `receive`: the manifest line, then the bundle's bytes, then the
/// transcript's.
pub fn receive(input: &mut impl BufRead) -> Result<Receipt, String> {
    let mut line = String::new();
    input
        .by_ref()
        .take(64 * 1024)
        .read_line(&mut line)
        .map_err(|e| format!("the manifest: {e}"))?;
    let m: Manifest =
        serde_json::from_str(line.trim()).map_err(|e| format!("the manifest: {e}"))?;
    check(&m, 1)?;
    let store = match m.harness {
        Harness::Claude => crate::config::CLAUDE_CONFIG_DIR.clone(),
        Harness::Codex => crate::config::CODEX_HOME.clone(),
    };
    receive_checked(&m, input, &crate::config::HOME, &store, &resume)
}

/// [`receive`] once the manifest is read and has passed [`check`], with the
/// home placement is checked against, the harness's store and the resume
/// handed in, for the tests.
fn receive_checked(
    m: &Manifest,
    input: &mut impl Read,
    home: &Path,
    store: &Path,
    resume: &dyn Fn(&crate::session::Session) -> Result<String, String>,
) -> Result<Receipt, String> {
    let m = m.clone();
    let base = staging();
    // One at a time: two moves into one checkout would each undo into the
    // other's state.
    let _lock = lock(&base).map_err(|_| {
        "another move is arriving here right now — try again in a moment".to_string()
    })?;
    keep_stale(&base);

    if was_kept(&m.move_id) {
        return Err("that move already arrived here and was kept".into());
    }
    let ticket = m.move_id.clone();
    let dir = base.join(&ticket);
    // `create_dir`, not `_all`: a second receive of one move id is refused
    // rather than written into the first one's journal.
    std::fs::create_dir(&dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => "that move already arrived here".to_string(),
        _ => format!("{}: {e}", dir.display()),
    })?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let result = (|| {
        let bundle = dir.join("snapshot.bundle");
        let transcript = dir.join("transcript.jsonl");
        copy_exact(input, &bundle, &m.bundle)?;
        copy_exact(input, &transcript, &m.transcript)?;
        let mut journal = Journal::new(dir.join("journal.json"));
        match land(&m, &bundle, &transcript, store, home, resume, &mut journal) {
            Ok((session_id, rmux, cwd)) => {
                let note = (!journal.notes.is_empty()).then(|| journal.notes.join("; "));
                Ok(Receipt {
                    ticket: ticket.clone(),
                    session_id,
                    rmux,
                    dir: cwd.to_string_lossy().into_owned(),
                    note,
                })
            }
            Err(why) => {
                let failed = journal.undo_all();
                Err(match failed.is_empty() {
                    true => format!("{why} (nothing was kept here)"),
                    false => format!("{why}; and undoing it failed: {}", failed.join("; ")),
                })
            }
        }
    })();
    // The bytes are not needed once landed, and a failed receive keeps nothing.
    let _ = std::fs::remove_file(dir.join("snapshot.bundle"));
    let _ = std::fs::remove_file(dir.join("transcript.jsonl"));
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    result
}

/// Everything a manifest can be refused for before a byte of it arrives,
/// answering how many bytes follow it.
///
/// `copies` is how many times those bytes are kept here at once: one over
/// ssh, where they stream straight into the ticket, and two over the tunnel,
/// where the upload is held until [`commit`] streams it into the ticket.
fn check(m: &Manifest, copies: u64) -> Result<u64, String> {
    protocol(m.protocol)?;
    installed(m.harness)?;
    if !crate::remote_open::id_ok(&m.session_id) {
        return Err("the manifest names a session id that is not one".into());
    }
    // The move id names the ticket, the upload and the directories they are
    // kept in: a uuid, as the sender makes them, and nothing that could be a
    // path.
    if !crate::config::is_full_uuid(&m.move_id) {
        return Err("the manifest names a move id that is not one".into());
    }
    let total = m.bundle.len.saturating_add(m.transcript.len);
    if total > MAX_TOTAL {
        return Err(format!(
            "{} is more than a move carries ({})",
            human(total),
            human(MAX_TOTAL)
        ));
    }
    let base = staging();
    std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    if let Some(free) = free_bytes(&base)
        && free < total.saturating_mul(copies).saturating_add(SPARE_DISK)
    {
        return Err(format!(
            "only {} free here, and the move is {}",
            human(free),
            human(total)
        ));
    }
    Ok(total)
}

/// Place, install, resume — each step journalled. `store` is the harness's
/// own directory, and `resume` starts the agent and answers its tab; both are
/// handed in so the rollback can be tested without a real store or agent.
fn land(
    m: &Manifest,
    bundle: &Path,
    transcript: &Path,
    store: &Path,
    home: &Path,
    resume: &dyn Fn(&crate::session::Session) -> Result<String, String>,
    journal: &mut Journal,
) -> Result<(String, String, PathBuf), String> {
    let cwd = super::place::apply(m, bundle, home, journal)?;
    let cwd_text = cwd.to_string_lossy().into_owned();
    let (id, file) = super::transcript::install(
        m.harness,
        &m.transcript_meta,
        &m.session_id,
        transcript,
        store,
        &cwd_text,
    )?;
    journal.push(Undo::RemoveFile(file))?;
    // Back on a machine it once left: the marker there would refuse the
    // resume below, and is no longer true.
    super::clear_moved(m.harness.provider(), &id);
    let mut session = crate::session::Session::new(m.harness.provider(), id.clone());
    session.label_source = cwd_text;
    let rmux = resume(&session)?;
    // The agent is running: if its undo step cannot be written, the undo
    // that follows would leave it running with everything under it taken
    // away — so it is stopped here, before the error goes up.
    if let Err(why) = journal.push(Undo::KillTab(rmux.clone())) {
        let _ = crate::rmux::kill(&rmux);
        return Err(why);
    }
    Ok((id, rmux, cwd))
}

/// Start the agent in a tab, and wait for it to be running.
fn resume(session: &crate::session::Session) -> Result<String, String> {
    let done = crate::actions::resume(session).map_err(|(_, why)| why)?;
    let rmux = done.rmux.ok_or("the agent was not started in a tab")?;
    // Up and settled, or the tab goes with the rest of the undo.
    if let Err(why) = wait_running(&rmux) {
        let _ = crate::rmux::kill(&rmux);
        return Err(why);
    }
    Ok(rmux)
}

/// The resumed agent is up, and still up a moment later: a harness that
/// cannot find the session, or is not logged in, exits at once.
fn wait_running(rmux: &str) -> Result<(), String> {
    let started = std::time::Instant::now();
    while crate::rmux::agent_pid(rmux).is_none() {
        if started.elapsed() > START_WAIT {
            return Err("the agent did not start there".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    std::thread::sleep(SETTLE);
    match crate::rmux::agent_pid(rmux) {
        // SAFETY: signal 0 only asks whether the pid exists.
        Some(pid) if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 => Ok(()),
        _ => Err("the agent started there and exited at once".into()),
    }
}

/// Exactly `blob.len` bytes from `input` into `path`, matching its sha256.
fn copy_exact(input: &mut impl Read, path: &Path, blob: &super::Blob) -> Result<(), String> {
    use sha2::Digest;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hasher = sha2::Sha256::new();
    let mut left = blob.len;
    let mut buf = vec![0u8; 1 << 16];
    while left > 0 {
        let want = buf.len().min(left as usize);
        let n = input
            .read(&mut buf[..want])
            .map_err(|e| format!("receiving: {e}"))?;
        if n == 0 {
            return Err("the stream ended before everything arrived".into());
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])
            .map_err(|e| format!("{}: {e}", path.display()))?;
        left -= n as u64;
    }
    match super::hex(&hasher.finalize()) == blob.sha256 {
        true => Ok(()),
        false => Err("what arrived is not what was sent (checksum)".into()),
    }
}

// ---------------------------------------------------------------------------
// The same receive, over a sibling's tunnel
// ---------------------------------------------------------------------------
//
// Over ssh the manifest and the bytes are one stream into one process. Over
// the tunnel they are many requests — no single one may outlast the asking
// side's timeout — so the bytes are held in an upload until the last one is
// in, and then poured into [`receive`] as the same stream ssh would have
// carried. Everything [`receive`] checks and undoes is then checked and undone
// the same way, by the same code.

/// The most one tunnel request carries of a move's bytes.
pub const CHUNK: u64 = 2 * 1024 * 1024;

/// Where uploads are held until they are committed.
fn uploads() -> PathBuf {
    staging().join("uploads")
}

/// The upload `id` names, if it is one.
fn upload_dir(id: &str) -> Result<PathBuf, String> {
    match crate::config::is_full_uuid(id) {
        true => Ok(uploads().join(id)),
        false => Err("that is not a move upload".into()),
    }
}

/// How an upload ended, once [`commit`] has run it.
const RESULT: &str = "result.json";

/// `begin`: check the manifest as `receive` would, and open an upload for
/// its bytes. Only one upload at a time — as only one receive — and one left
/// behind by a sender that went away stops counting after [`STALE`].
pub fn begin(m: &Manifest) -> Result<String, String> {
    check(m, 2)?;
    open_upload(m)
}

/// [`begin`] once the manifest has passed.
fn open_upload(m: &Manifest) -> Result<String, String> {
    let base = uploads();
    std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    for entry in std::fs::read_dir(&base).into_iter().flatten().flatten() {
        // An upload that has its answer is not arriving any more: it waits
        // only for the sender's `finish` or `abort`, and is dropped once that
        // has had [`STALE`] to come.
        let answered = entry.path().join(RESULT).exists();
        // By its newest file, not the directory: a chunk appends to `data`,
        // which leaves the directory's own time where it was, and a slow
        // uplink can take longer than [`STALE`] to send a large one.
        let fresh = std::fs::read_dir(entry.path())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|file| file.metadata().and_then(|m| m.modified()).ok())
            .any(|t| t.elapsed().is_ok_and(|e| e < STALE));
        match (fresh, answered) {
            (true, false) => {
                return Err(
                    "another move is arriving here right now — try again in a moment".into(),
                );
            }
            (true, true) => {}
            (false, _) => {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    // Named by the move, so the sender can abort it without having heard
    // back from this `begin`.
    let id = m.move_id.clone();
    let dir = base.join(&id);
    std::fs::create_dir(&dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => "that move is already arriving here".to_string(),
        _ => format!("{}: {e}", dir.display()),
    })?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let manifest = serde_json::to_string(m).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("manifest.json"), manifest)
        .and_then(|()| std::fs::write(dir.join("data"), b""))
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(id)
}

/// `chunk`: the `seq`th [`CHUNK`] of an upload's bytes, in order. A chunk out
/// of order, too long, or past what the manifest declared is refused; the
/// sender then aborts.
pub fn chunk(id: &str, seq: u64, data: &[u8]) -> Result<(), String> {
    let dir = upload_dir(id)?;
    let m = upload_manifest(&dir)?;
    let total = m.bundle.len.saturating_add(m.transcript.len);
    let path = dir.join("data");
    let had = std::fs::metadata(&path)
        .map(|meta| meta.len())
        .map_err(|_| "no such upload — it may have been given up on".to_string())?;
    if data.len() as u64 > CHUNK || seq.checked_mul(CHUNK) != Some(had) {
        return Err(format!("chunk {seq} is not the one expected next"));
    }
    if had + data.len() as u64 > total {
        return Err("more arrived than the manifest declared".into());
    }
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(data)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn upload_manifest(dir: &Path) -> Result<Manifest, String> {
    let text = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|_| "no such upload — it may have been given up on".to_string())?;
    serde_json::from_str(&text).map_err(|e| format!("the manifest: {e}"))
}

/// `commit`: every byte is in, so run [`receive`] on it — on a thread of its
/// own, since placing a large repository and waiting out the agent's start
/// can take longer than one request may. [`status`] answers it.
pub fn commit(id: &str) -> Result<(), String> {
    let dir = upload_dir(id)?;
    let m = upload_manifest(&dir)?;
    let total = m.bundle.len.saturating_add(m.transcript.len);
    let had = std::fs::metadata(dir.join("data")).map_or(0, |meta| meta.len());
    if had != total {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!(
            "{} of {} arrived — nothing was kept here",
            human(had),
            human(total)
        ));
    }
    if dir.join(RESULT).exists() {
        return Err("that upload is already being received".into());
    }
    // Made with `create_new`, so of two commits at once only one runs: the
    // second would fail on the lock and write its failure over the first
    // one's success, and the sender would thaw an agent that also runs here.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join("committing"))
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => "that upload is already being received".into(),
            _ => e.to_string(),
        })?;
    std::thread::spawn(move || {
        let answer = Answer::from(pour(&dir, &m));
        let _ = std::fs::remove_file(dir.join("data"));
        if let Ok(text) = serde_json::to_string(&answer) {
            let _ = std::fs::write(dir.join(RESULT), text);
        }
    });
    Ok(())
}

/// The upload as the one stream [`receive`] reads: the manifest line, then
/// the bytes.
fn pour(dir: &Path, m: &Manifest) -> Result<Receipt, String> {
    let line = serde_json::to_string(m).map_err(|e| e.to_string())? + "\n";
    let data = std::fs::File::open(dir.join("data")).map_err(|e| e.to_string())?;
    let mut stream = std::io::BufReader::new(std::io::Cursor::new(line).chain(data));
    receive(&mut stream)
}

/// `status`: `None` while the commit is still running, else how it ended.
///
/// The answer stays readable until the sender says `finish` or `abort`: a
/// reply lost on the way back is asked for again, and the ticket it names is
/// still there to undo.
pub fn status(id: &str) -> Result<Option<Receipt>, String> {
    let dir = upload_dir(id)?;
    let Ok(text) = std::fs::read_to_string(dir.join(RESULT)) else {
        return match dir.join("committing").exists() {
            true => Ok(None),
            false => Err("no such upload — it may have been given up on".into()),
        };
    };
    serde_json::from_str::<Answer<Receipt>>(&text)
        .map_err(|e| format!("its answer: {e}"))?
        .into_result()
        .map(Some)
}

/// `finish`: keep it.
pub fn finish(t: &Ticket) -> Result<(), String> {
    let dir = ticket_dir(t)?;
    let _lock = lock(&staging()).map_err(|_| "a move is arriving here — ask again".to_string())?;
    let mut journal = Journal::load(dir.join("journal.json"))?;
    journal.finish();
    mark_kept(&t.ticket);
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(uploads().join(&t.ticket));
    Ok(())
}

/// `abort`: take it all back, by the move's id.
///
/// `Ok` means nothing of that move is running here — undone now, or it never
/// landed — and is the only answer on which the sender lets its own agent go
/// on. Anything else is an `Err` the sender asks again about: a receive still
/// in progress, a move already kept, an undo that could not stop the agent.
pub fn abort(t: &Ticket) -> Result<(), String> {
    let dir = ticket_dir(t)?;
    let upload = uploads().join(&t.ticket);
    // An upload committed but not yet received: the receive may be about to
    // start the agent, and holds no lock until its thread takes one.
    if upload.join("committing").exists() && !upload.join(RESULT).exists() {
        return Err("it is being received right now — ask again in a moment".into());
    }
    // A receive in progress holds this; once it is free, what is on disk is
    // the whole story.
    let _lock =
        lock(&staging()).map_err(|_| "it is being received right now — ask again".to_string())?;
    if upload.join("committing").exists() && !upload.join(RESULT).exists() {
        return Err("it is being received right now — ask again in a moment".into());
    }
    if was_kept(&t.ticket) {
        return Err("it was already kept here and is running".into());
    }
    let undone = match Journal::load(dir.join("journal.json")) {
        Ok(mut journal) => journal.undo_all(),
        // No journal: it never landed, or failed and was undone as it failed.
        Err(_) => Vec::new(),
    };
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&upload);
    match undone.is_empty() {
        true => Ok(()),
        false => Err(format!("undoing it failed: {}", undone.join("; "))),
    }
}

/// The staging lock every receive holds, taken without waiting.
fn lock(base: &Path) -> Result<std::fs::File, ()> {
    let _ = std::fs::create_dir_all(base);
    crate::util::lock_within(&base.join("lock"), Duration::ZERO).map_err(|_| ())
}

fn ticket_dir(t: &Ticket) -> Result<PathBuf, String> {
    protocol(t.protocol)?;
    match crate::config::is_full_uuid(&t.ticket) {
        true => Ok(staging().join(&t.ticket)),
        false => Err("that is not a move ticket".into()),
    }
}

/// Moves the sender never came back for are kept: by then the agent here is
/// the one running, and undoing it would lose the only copy at work.
fn keep_stale(base: &Path) {
    for entry in std::fs::read_dir(base).into_iter().flatten().flatten() {
        let journal = entry.path().join("journal.json");
        let old = std::fs::metadata(&journal)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|e| e > STALE));
        if old && let Ok(mut j) = Journal::load(journal) {
            j.finish();
            if let Some(id) = entry.file_name().to_str() {
                mark_kept(id);
            }
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    // A record of a kept move only has to outlive the sender's last word
    // about it, which comes within minutes.
    for entry in std::fs::read_dir(kept_dir())
        .into_iter()
        .flatten()
        .flatten()
    {
        let old = entry.metadata().and_then(|m| m.modified()).is_ok_and(|t| {
            t.elapsed()
                .is_ok_and(|e| e > Duration::from_secs(24 * 3600))
        });
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is NUL-terminated and outlives the call; `stat` is written.
    let rc = unsafe { libc::statvfs(c.as_ptr(), &mut stat) };
    (rc == 0).then(|| stat.f_bavail as u64 * stat.f_frsize as u64)
}

fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b => format!("{} KB", b.div_ceil(1024)),
    }
}

/// `cctop move …`. Answers one JSON line; exit 0 when the question was
/// understood, whatever the answer.
pub fn run_cli(args: &[String], version: &str) -> i32 {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let read_json = |input: &mut dyn BufRead| -> Result<String, String> {
        let mut line = String::new();
        input
            .take(64 * 1024)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        Ok(line)
    };
    let answer = match args.first().map(String::as_str) {
        Some("hello") => serde_json::to_string(&Answer::Ok(hello(version))),
        Some("plan") => {
            let r = read_json(&mut input).and_then(|l| {
                serde_json::from_str::<Ask>(l.trim()).map_err(|e| format!("the question: {e}"))
            });
            serde_json::to_string(&Answer::from(r.and_then(|a| plan(&a))))
        }
        Some("receive") => {
            let received = receive(&mut input);
            let ticket = received.as_ref().ok().map(|r| r.ticket.clone());
            let line = serde_json::to_string(&Answer::from(received));
            // The sender may be gone — a dropped ssh — with the agent already
            // running here. An answer nobody can read is a move nobody will
            // `finish` or `abort`, and the sender has thawed its own agent by
            // now: so it is undone here, the moment the write fails.
            let said = line.as_ref().map_err(|e| e.to_string()).and_then(|line| {
                let mut out = std::io::stdout().lock();
                writeln!(out, "{line}")
                    .and_then(|()| out.flush())
                    .map_err(|e| e.to_string())
            });
            return match (said, ticket) {
                (Ok(()), _) => 0,
                (Err(why), ticket) => {
                    if let Some(ticket) = ticket {
                        let _ = abort(&Ticket {
                            protocol: PROTOCOL,
                            ticket,
                        });
                    }
                    eprintln!("cctop move: {why}");
                    1
                }
            };
        }
        Some(word @ ("finish" | "abort")) => {
            let r = read_json(&mut input).and_then(|l| {
                serde_json::from_str::<Ticket>(l.trim()).map_err(|e| format!("the ticket: {e}"))
            });
            let r = r.and_then(|t| match word {
                "finish" => finish(&t),
                _ => abort(&t),
            });
            serde_json::to_string(&Answer::from(r))
        }
        _ => {
            eprintln!("usage: cctop move hello | plan | receive | finish | abort");
            return 2;
        }
    };
    match answer {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(e) => {
            eprintln!("cctop move: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests that take the staging lock, or need it free, one at a time:
    /// it is a file lock, which threads of one process contend for too.
    static STAGING: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn staging_alone() -> std::sync::MutexGuard<'static, ()> {
        STAGING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn a_short_or_altered_stream_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let data = b"hello world";
        let blob = super::super::Blob {
            len: data.len() as u64,
            sha256: {
                use sha2::Digest;
                super::super::hex(&sha2::Sha256::digest(data))
            },
        };
        let ok = tmp.path().join("ok");
        copy_exact(&mut &data[..], &ok, &blob).unwrap();
        assert_eq!(std::fs::read(&ok).unwrap(), data);
        let short = tmp.path().join("short");
        assert!(copy_exact(&mut &data[..5], &short, &blob).is_err());
        let altered = tmp.path().join("altered");
        assert!(copy_exact(&mut &b"hello WORLD"[..], &altered, &blob).is_err());
    }

    /// Whatever fails after the files are placed, every step before it is
    /// taken back: the checkout, the transcript, the fetched ref.
    #[test]
    fn a_failure_after_landing_undoes_everything() {
        use crate::migrate::snapshot::tests::{git_out, repo};
        let tmp = tempfile::tempdir().unwrap();
        let src = repo(&tmp.path().join("src/app"));
        std::fs::write(src.join("new.txt"), "n\n").unwrap();
        let snap = crate::migrate::snapshot::take(&src, "mv9").unwrap();
        let bundle = tmp.path().join("b.bundle");
        crate::migrate::snapshot::bundle(&snap, false, &bundle).unwrap();
        let transcript = tmp.path().join("t.jsonl");
        let id = "0b4e8a54-8d39-4f4e-8a39-6b8a3c1c2d01";
        std::fs::write(
            &transcript,
            format!("{{\"sessionId\":\"{id}\",\"cwd\":\"/x\"}}\n"),
        )
        .unwrap();
        let dest = tmp.path().join("there/app");
        let m = Manifest {
            protocol: PROTOCOL,
            harness: Harness::Claude,
            session_id: id.into(),
            repo: snap.repo.clone(),
            placement: Placement {
                kind: crate::migrate::Kind::Clone,
                dir: dest.to_string_lossy().into_owned(),
                root: None,
                has_head: false,
                note: None,
            },
            snapshot: snap.commit.clone(),
            move_id: "mv9".into(),
            bundle: crate::migrate::Blob {
                len: 0,
                sha256: String::new(),
            },
            transcript: crate::migrate::Blob {
                len: 0,
                sha256: String::new(),
            },
            transcript_meta: crate::migrate::transcript::Meta {
                old_cwd: "/x".into(),
                codex_dated: None,
                codex_prefix: None,
            },
        };
        let store = tmp.path().join("claude");
        let mut journal = Journal::new(tmp.path().join("journal.json"));
        let failing = |_: &crate::session::Session| -> Result<String, String> {
            Err("the agent did not start there".into())
        };
        let err = land(
            &m,
            &bundle,
            &transcript,
            &store,
            tmp.path(),
            &failing,
            &mut journal,
        )
        .unwrap_err();
        assert!(err.contains("did not start"));
        // Placed and installed before the failure…
        assert!(dest.join("new.txt").exists());
        let installed = store
            .join("projects")
            .join(crate::convert::project_slug(&dest));
        assert!(installed.join(format!("{id}.jsonl")).exists());
        // …and gone after the undo.
        assert!(journal.undo_all().is_empty());
        assert!(!dest.exists());
        assert!(!installed.join(format!("{id}.jsonl")).exists());
        let _ = git_out(&src, &["status"]);

        // And the success path keeps both.
        let mut journal = Journal::new(tmp.path().join("journal2.json"));
        let ok = |_: &crate::session::Session| -> Result<String, String> { Ok("tab".into()) };
        let (got, tab, cwd) = land(
            &m,
            &bundle,
            &transcript,
            &store,
            tmp.path(),
            &ok,
            &mut journal,
        )
        .unwrap();
        assert_eq!((got.as_str(), tab.as_str(), cwd), (id, "tab", dest.clone()));
    }

    /// An upload takes its bytes in order and no more than declared, admits
    /// no second upload beside it, and is poured into the same `receive` an
    /// ssh stream feeds — which checks what arrived exactly as it would have.
    #[test]
    fn a_tunnel_upload_arrives_in_order_and_is_received_like_a_stream() {
        let _alone = staging_alone();
        let mut m = sample_manifest();
        m.bundle = crate::migrate::Blob {
            len: 5,
            sha256: "0".repeat(64),
        };
        m.transcript = crate::migrate::Blob {
            len: 3,
            sha256: "0".repeat(64),
        };
        let id = open_upload(&m).unwrap();
        let second = open_upload(&m).unwrap_err();
        assert!(second.contains("another move"), "{second}");

        assert!(chunk(&id, 1, b"x").is_err(), "out of order");
        assert!(chunk(&id, 0, b"123456789").is_err(), "more than declared");
        assert!(chunk("../../etc", 0, b"x").is_err());
        let early = commit(&id).unwrap_err();
        assert!(early.contains("0 KB of 1 KB"), "{early}");

        let id = open_upload(&m).unwrap();
        chunk(&id, 0, b"bundltra").unwrap();
        commit(&id).unwrap();
        assert!(commit(&id).is_err(), "committed once only");
        let started = std::time::Instant::now();
        let ended = loop {
            match status(&id) {
                Ok(None) if started.elapsed() < Duration::from_secs(20) => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                other => break other,
            }
        };
        // The made-up checksums are what refuses it — or, on a machine with
        // no `claude`, the harness before them: `receive`'s own checks either
        // way, and nothing kept.
        let why = ended.unwrap_err();
        assert!(
            why.contains("checksum") || why.contains("not installed"),
            "{why}"
        );
        // The answer stays until the sender has heard it — asked again, it
        // says the same — and the sender's abort clears it.
        assert_eq!(status(&id).unwrap_err(), why);
        let ticket = Ticket {
            protocol: PROTOCOL,
            ticket: id.clone(),
        };
        abort(&ticket).unwrap();
        assert!(!uploads().join(&id).exists());

        // An upload the sender gives up on is dropped by its abort.
        let id = open_upload(&m).unwrap();
        abort(&ticket).unwrap();
        assert!(!uploads().join(&id).exists());
    }

    /// A move that landed and started its agent, whose answer never reached
    /// the sender: the sender's abort, by the move id it already knew, stops
    /// that agent and takes the rest back. A move already kept is never
    /// reported as undone.
    #[test]
    fn a_move_whose_answer_was_lost_is_undone_by_its_id() {
        let _alone = staging_alone();
        use crate::migrate::snapshot::tests::repo;
        let _daemon = crate::mux::TestDaemon::new("move-lost-answer");
        let tmp = tempfile::tempdir().unwrap();
        let src = repo(&tmp.path().join("src/app"));
        std::fs::write(src.join("new.txt"), "n\n").unwrap();
        let move_id = "4d2a7c1e-9b3f-4e8d-a6c5-1f0e2d3c4b5a";
        let snap = crate::migrate::snapshot::take(&src, move_id).unwrap();
        let bundle = tmp.path().join("b.bundle");
        crate::migrate::snapshot::bundle(&snap, false, &bundle).unwrap();
        let id = "0b4e8a54-8d39-4f4e-8a39-6b8a3c1c2d02";
        let transcript = format!("{{\"sessionId\":\"{id}\",\"cwd\":\"/x\"}}\n");
        let blob = |bytes: &[u8]| crate::migrate::Blob {
            len: bytes.len() as u64,
            sha256: {
                use sha2::Digest;
                crate::migrate::hex(&sha2::Sha256::digest(bytes))
            },
        };
        let bundle_bytes = std::fs::read(&bundle).unwrap();
        let home = tmp.path().join("home");
        let dest = home.join("app");
        let mut m = sample_manifest();
        m.move_id = move_id.into();
        m.session_id = id.into();
        m.repo = snap.repo.clone();
        m.snapshot = snap.commit.clone();
        m.placement.dir = dest.to_string_lossy().into_owned();
        m.bundle = blob(&bundle_bytes);
        m.transcript = blob(transcript.as_bytes());

        // The "agent": a session in this test's daemon, as a resume makes one.
        let tab = "cctop-moved-lost";
        let resume = |_: &crate::session::Session| -> Result<String, String> {
            crate::mux::new_session(tab, None, Vec::new(), &["sleep".into(), "30".into()])?;
            Ok(tab.to_string())
        };
        let mut stream = std::io::Cursor::new([bundle_bytes, transcript.into_bytes()].concat());
        let store = tmp.path().join("claude");
        let receipt = receive_checked(&m, &mut stream, &home, &store, &resume).unwrap();
        assert_eq!(
            receipt.ticket, move_id,
            "the ticket is the sender's move id"
        );
        assert!(crate::mux::has_session(tab));
        assert!(dest.join("new.txt").exists());

        // The answer is lost; the sender aborts by the id it knew.
        let ticket = Ticket {
            protocol: PROTOCOL,
            ticket: move_id.into(),
        };
        abort(&ticket).unwrap();
        assert!(!crate::mux::has_session(tab), "the far agent still runs");
        assert!(!dest.exists());
        // Asked again, nothing is there and nothing runs: still a yes.
        abort(&ticket).unwrap();

        // Kept, it is never "undone": a late abort hears that it runs.
        mark_kept(move_id);
        assert!(abort(&ticket).unwrap_err().contains("kept"));
        let _ = std::fs::remove_file(kept_dir().join(move_id));
    }

    /// While a receive holds the lock, an abort cannot know what it will
    /// leave running, and says "ask again" rather than yes.
    #[test]
    fn an_abort_during_a_receive_is_asked_again() {
        let _alone = staging_alone();
        let base = staging();
        let _held = lock(&base).unwrap();
        let busy = abort(&Ticket {
            protocol: PROTOCOL,
            ticket: "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d".into(),
        })
        .unwrap_err();
        assert!(busy.contains("ask again"), "{busy}");
    }

    fn sample_manifest() -> Manifest {
        Manifest {
            protocol: PROTOCOL,
            harness: Harness::Claude,
            session_id: "0b4e8a54-8d39-4f4e-8a39-6b8a3c1c2d01".into(),
            repo: crate::migrate::Repo {
                origin: None,
                key: "example.com/o/app".into(),
                name: "app".into(),
                head: "0".repeat(40),
                branch: Some("main".into()),
                rel_cwd: String::new(),
            },
            placement: Placement {
                kind: crate::migrate::Kind::Clone,
                dir: "/nowhere/app".into(),
                root: None,
                has_head: false,
                note: None,
            },
            snapshot: "0".repeat(40),
            move_id: "6f1c2b3a-0d4e-4f5a-8b6c-7d8e9f0a1b2c".into(),
            bundle: crate::migrate::Blob {
                len: 0,
                sha256: String::new(),
            },
            transcript: crate::migrate::Blob {
                len: 0,
                sha256: String::new(),
            },
            transcript_meta: crate::migrate::transcript::Meta {
                old_cwd: "/x".into(),
                codex_dated: None,
                codex_prefix: None,
            },
        }
    }

    #[test]
    fn a_ticket_is_only_ever_a_uuid() {
        let bad = Ticket {
            protocol: PROTOCOL,
            ticket: "../../etc".into(),
        };
        assert!(ticket_dir(&bad).is_err());
    }

    #[test]
    fn the_side_that_is_behind_is_named() {
        assert!(protocol(PROTOCOL).is_ok());
        assert!(
            protocol(PROTOCOL + 1)
                .unwrap_err()
                .contains("update this one")
        );
        assert!(protocol(0).unwrap_err().contains("update the sending one"));
    }
}
