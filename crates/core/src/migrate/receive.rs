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
    let base = staging();
    // One at a time: two moves into one checkout would each undo into the
    // other's state.
    let _lock = crate::util::lock_within(&base.join("lock"), Duration::ZERO).map_err(|_| {
        "another move is arriving here right now — try again in a moment".to_string()
    })?;
    keep_stale(&base);

    let ticket = crate::util::new_uuid();
    let dir = base.join(&ticket);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
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
        let store = match m.harness {
            Harness::Claude => crate::config::CLAUDE_CONFIG_DIR.clone(),
            Harness::Codex => crate::config::CODEX_HOME.clone(),
        };
        match land(&m, &bundle, &transcript, &store, &resume, &mut journal) {
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
    if !crate::remote_open::id_ok(&m.session_id) || !crate::remote_open::id_ok(&m.move_id) {
        return Err("the manifest names a session id that is not one".into());
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
    resume: &dyn Fn(&crate::session::Session) -> Result<String, String>,
    journal: &mut Journal,
) -> Result<(String, String, PathBuf), String> {
    let cwd = super::place::apply(m, bundle, journal)?;
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
    journal.push(Undo::KillTab(rmux.clone()))?;
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
        // By its newest file, not the directory: a chunk appends to `data`,
        // which leaves the directory's own time where it was, and a slow
        // uplink can take longer than [`STALE`] to send a large one.
        let fresh = std::fs::read_dir(entry.path())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|file| file.metadata().and_then(|m| m.modified()).ok())
            .any(|t| t.elapsed().is_ok_and(|e| e < STALE));
        match fresh {
            true => {
                return Err(
                    "another move is arriving here right now — try again in a moment".into(),
                );
            }
            false => {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    let id = crate::util::new_uuid();
    let dir = base.join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
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
    if dir.join("committing").exists() || dir.join(RESULT).exists() {
        return Err("that upload is already being received".into());
    }
    std::fs::write(dir.join("committing"), b"").map_err(|e| e.to_string())?;
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

/// `status`: `None` while the commit is still running, else how it ended —
/// after which the upload is gone.
pub fn status(id: &str) -> Result<Option<Receipt>, String> {
    let dir = upload_dir(id)?;
    let Ok(text) = std::fs::read_to_string(dir.join(RESULT)) else {
        return match dir.join("committing").exists() {
            true => Ok(None),
            false => Err("no such upload — it may have been given up on".into()),
        };
    };
    let _ = std::fs::remove_dir_all(&dir);
    serde_json::from_str::<Answer<Receipt>>(&text)
        .map_err(|e| format!("its answer: {e}"))?
        .into_result()
        .map(Some)
}

/// `finish`: keep it.
pub fn finish(t: &Ticket) -> Result<(), String> {
    let mut journal = Journal::load(ticket_dir(t)?.join("journal.json"))?;
    journal.finish();
    let _ = std::fs::remove_dir_all(ticket_dir(t)?);
    Ok(())
}

/// `abort`: take it all back. An upload not yet committed is simply
/// dropped; a received move is undone from its journal.
pub fn abort(t: &Ticket) -> Result<(), String> {
    if let Ok(dir) = upload_dir(&t.ticket)
        && dir.is_dir()
    {
        protocol(t.protocol)?;
        if dir.join("committing").exists() && !dir.join(RESULT).exists() {
            return Err("it is being received right now — abort its ticket once it is".into());
        }
        let _ = std::fs::remove_dir_all(&dir);
        return Ok(());
    }
    let mut journal = Journal::load(ticket_dir(t)?.join("journal.json"))?;
    let failed = journal.undo_all();
    let _ = std::fs::remove_dir_all(ticket_dir(t)?);
    match failed.is_empty() {
        true => Ok(()),
        false => Err(format!("undoing it failed: {}", failed.join("; "))),
    }
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
            let _ = std::fs::remove_dir_all(entry.path());
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
        Some("receive") => serde_json::to_string(&Answer::from(receive(&mut input))),
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
        let err = land(&m, &bundle, &transcript, &store, &failing, &mut journal).unwrap_err();
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
        let (got, tab, cwd) = land(&m, &bundle, &transcript, &store, &ok, &mut journal).unwrap();
        assert_eq!((got.as_str(), tab.as_str(), cwd), (id, "tab", dest.clone()));
    }

    /// An upload takes its bytes in order and no more than declared, admits
    /// no second upload beside it, and is poured into the same `receive` an
    /// ssh stream feeds — which checks what arrived exactly as it would have.
    #[test]
    fn a_tunnel_upload_arrives_in_order_and_is_received_like_a_stream() {
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
        assert!(!uploads().join(&id).exists());

        // An upload the sender gives up on is dropped by its abort.
        let id = open_upload(&m).unwrap();
        abort(&Ticket {
            protocol: PROTOCOL,
            ticket: id.clone(),
        })
        .unwrap();
        assert!(!uploads().join(&id).exists());
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
            move_id: "mv1".into(),
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
