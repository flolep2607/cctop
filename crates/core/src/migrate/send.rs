//! The sending side: from `X` on a row to the agent running on the other
//! machine and stopped here.
//!
//! Two calls, with the person's confirmation between them:
//!
//! - [`prepare`] asks everything that can be asked without changing anything:
//!   whether the row can move, whether the far cctop can take it, and where it
//!   would land. Nothing is frozen and nothing is written, so the confirmation
//!   can sit open as long as it likes.
//! - [`run`] does the move: freeze the agent, snapshot, send, fence, stop.
//!   The agent is frozen only for those seconds, and thawed when the move
//!   fails — but only once the far side has said nothing of it runs there.
//!   A far side that cannot say so leaves the agent here stopped, and the
//!   failure says how to settle it by hand: a stopped agent can be continued,
//!   two agents writing one conversation cannot be unwritten. A cctop that
//!   quits mid-move leaves it stopped for the same reason, and says so on the
//!   way out ([`in_flight`]).

use std::path::{Path, PathBuf};

use super::wire::Wire;
use super::{Ask, Blob, Harness, Manifest, Moved, PROTOCOL, Placement, Receipt, Repo};
use crate::session::Session;

/// What [`prepare`] found, held through the confirmation.
#[derive(Debug, Clone)]
pub struct Plan {
    pub harness: Harness,
    pub session_id: String,
    pub title: String,
    pub cwd: PathBuf,
    pub transcript: PathBuf,
    pub pid: Option<u32>,
    pub repo: Repo,
    pub placement: Placement,
    /// The far machine's name.
    pub machine: String,
    /// What it would carry, roughly: the transcript, and the work in progress.
    pub size_hint: u64,
    /// When the agent started, as `/proc` counts it: a pid the agent no longer
    /// holds — it exited, and was resumed, or the number was reused — is never
    /// stopped or signalled.
    pub pid_start: Option<u64>,
    /// The transcript as it was when `X` was pressed. An agent that has
    /// written to it since has taken a turn the plan never saw.
    pub transcript_stamp: Option<(u64, std::time::SystemTime)>,
    /// The directory the person typed, if they typed one.
    pub typed: Option<String>,
}

/// How a move ended.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub receipt: Receipt,
    pub machine: String,
    /// Said but not fatal: the far side did not hear `finish`, say.
    pub note: Option<String>,
}

/// Everything that can be asked before anything changes.
pub fn prepare(session: &Session, wire: &dyn Wire) -> Result<Plan, String> {
    let harness = super::movable(session)?;
    if let Some(moved) = super::moved_to(session.provider, &session.session_id) {
        return Err(super::moved_refusal(&moved));
    }
    let cwd = session
        .work_dir()
        .ok_or("its directory is gone from here")?;
    let transcript = session
        .data_file
        .clone()
        .ok_or("its transcript is not on this machine")?;
    let (_, repo) = super::snapshot::describe(&cwd)?;
    let hello = wire.hello()?;
    if hello.protocol != PROTOCOL {
        return Err(format!(
            "{} speaks move protocol {} and this cctop {PROTOCOL} — update the one that is behind",
            wire.machine(),
            hello.protocol
        ));
    }
    if !hello.harnesses.contains(&harness) {
        return Err(format!(
            "{} is not installed on {}",
            harness.command(),
            wire.machine()
        ));
    }
    let placement = wire.plan(&Ask {
        protocol: PROTOCOL,
        harness,
        repo: repo.clone(),
        dir: None,
    })?;
    let size_hint = std::fs::metadata(&transcript).map(|m| m.len()).unwrap_or(0);
    let pid = session.is_running().then(|| session.root_pid()).flatten();
    let transcript_stamp = stamp(&transcript);
    Ok(Plan {
        harness,
        session_id: session.session_id.clone(),
        title: session.display_label().to_string(),
        cwd,
        transcript,
        pid,
        pid_start: pid.and_then(crate::yolo::start_time),
        transcript_stamp,
        typed: None,
        repo,
        placement,
        machine: wire.machine().to_string(),
        size_hint,
    })
}

/// The person typed a directory of their own: ask again where that would be.
pub fn replan(plan: &mut Plan, dir: &str, wire: &dyn Wire) -> Result<(), String> {
    plan.placement = wire.plan(&Ask {
        protocol: PROTOCOL,
        harness: plan.harness,
        repo: plan.repo.clone(),
        dir: Some(dir.to_string()),
    })?;
    plan.typed = Some(dir.to_string());
    Ok(())
}

/// The agent a move in progress holds stopped, and where it is going: what
/// a cctop that quits mid-move says on its way out.
static FROZEN: std::sync::Mutex<Option<(u32, String)>> = std::sync::Mutex::new(None);

/// The agent stopped by a move that has not ended — still going, or left
/// in doubt — and the machine it was going to.
pub fn in_flight() -> Option<(u32, String)> {
    FROZEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// What to tell the person about an agent left stopped: how to settle it
/// by hand, whichever way the far side turned out.
pub fn left_stopped(pid: u32, machine: &str) -> String {
    format!(
        "The agent here (pid {pid}) is left stopped, so it and a copy on {machine} never both \
         write the conversation. If it runs on {machine}, end this one: `kill {pid}`. \
         If it does not, let this one go on: `kill -CONT {pid}`."
    )
}

/// An agent held still with SIGSTOP. Let go by [`Frozen::thaw`] — only once
/// nothing of the move runs on the far side — or by its drop on a failure
/// before anything was sent; [`Frozen::keep`] leaves it stopped.
struct Frozen(Option<u32>);

impl Frozen {
    fn new(pid: Option<u32>, machine: &str) -> Frozen {
        if let Some(pid) = pid {
            *FROZEN
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some((pid, machine.to_string()));
            // SAFETY: a signal to a pid this user's session reported as its
            // own, whose start time was checked just before.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGSTOP) };
        }
        Frozen(pid)
    }

    fn thaw(mut self) {
        self.release();
    }

    /// Stopped it stays: the far side may be running the session.
    fn keep(mut self) {
        self.0 = None;
    }

    fn release(&mut self) {
        if let Some(pid) = self.0.take() {
            // SAFETY: as above. Sent after a SIGTERM too, which a stopped
            // process would otherwise not act on until it was continued.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGCONT) };
            *FROZEN
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        self.release();
    }
}

/// How long the far side is asked to abort before the move is left in doubt.
#[cfg(not(test))]
const ABORT_WAIT: std::time::Duration = std::time::Duration::from_secs(3 * 60);
#[cfg(test)]
const ABORT_WAIT: std::time::Duration = std::time::Duration::from_millis(300);

/// Ask the far side to undo the move until it says nothing of it runs there,
/// or [`ABORT_WAIT`] is up. A receive still in progress there answers "ask
/// again", and is asked again.
fn settle(wire: &dyn Wire, move_id: &str) -> Result<(), String> {
    let started = std::time::Instant::now();
    loop {
        let why = match wire.abort(move_id) {
            Ok(()) => return Ok(()),
            Err(why) => why,
        };
        if started.elapsed() > ABORT_WAIT {
            return Err(why);
        }
        std::thread::sleep(ABORT_WAIT.min(std::time::Duration::from_secs(2)) / 4);
    }
}

/// A move that failed after its bytes left: undone there and thawed here,
/// or — when the far side cannot say it is undone — left stopped here, with
/// what to do about it.
fn give_back(plan: &Plan, wire: &dyn Wire, move_id: &str, frozen: Frozen, why: String) -> String {
    match settle(wire, move_id) {
        Ok(()) => {
            frozen.thaw();
            format!("{why} — nothing moved")
        }
        Err(abort) => {
            frozen.keep();
            let settle_by_hand = match plan.pid {
                Some(pid) => left_stopped(pid, &plan.machine),
                None => format!("Look on {} for a tab running it.", plan.machine),
            };
            format!(
                "{why}, and {} could not say whether it kept a copy ({abort}). {settle_by_hand}",
                plan.machine
            )
        }
    }
}

/// The plan is still about the agent and the turn it was made for.
///
/// The confirmation can sit open a while. An agent that has written to its
/// transcript since has taken a turn — perhaps with a tool call still running
/// — and a pid whose start time changed is no longer that agent at all.
fn still_current(plan: &Plan) -> Result<(), String> {
    if stamp(&plan.transcript) != plan.transcript_stamp {
        return Err("it has done something since X was pressed — press X again".into());
    }
    if let Some(pid) = plan.pid
        && crate::yolo::start_time(pid) != plan.pid_start
    {
        return Err("its agent has ended or restarted since X was pressed — press X again".into());
    }
    Ok(())
}

/// Do the move. `say` hears each step; it runs on the caller's thread.
pub fn run(plan: &Plan, wire: &dyn Wire, say: &(dyn Fn(String) + Sync)) -> Result<Outcome, String> {
    still_current(plan)?;
    let move_id = crate::util::new_uuid();
    let frozen = Frozen::new(plan.pid, &plan.machine);
    // Asked again with it stopped: a line written between the check above and
    // the signal is a turn that has begun.
    still_current(plan)?;
    say("Snapshotting the checkout…".into());
    let snap = super::snapshot::take(&plan.cwd, &move_id)?;
    let result = send_snapshot(plan, wire, say, &snap, &move_id, frozen);
    super::snapshot::forget(&snap);
    result
}

fn send_snapshot(
    plan: &Plan,
    wire: &dyn Wire,
    say: &(dyn Fn(String) + Sync),
    snap: &super::snapshot::Snapshot,
    move_id: &str,
    frozen: Frozen,
) -> Result<Outcome, String> {
    // The head can have moved since the plan was made; where it would land
    // is asked again rather than trusted — as the person asked it, with their
    // own directory or none. An answer that lands it somewhere else than they
    // confirmed is theirs to look at again, not this code's to accept.
    let placement = plan.placement.clone();
    if snap.repo.head != plan.repo.head {
        let again = wire.plan(&Ask {
            protocol: PROTOCOL,
            harness: plan.harness,
            repo: snap.repo.clone(),
            dir: plan.typed.clone(),
        })?;
        if (again.kind, &again.dir, &again.root)
            != (placement.kind, &placement.dir, &placement.root)
        {
            return Err(format!(
                "where it would land on {} changed since X was pressed ({}) — press X again",
                plan.machine, again.dir
            ));
        }
    }
    super::snapshot::refuse_lfs(snap)?;
    let tmp = tempfile::tempdir().map_err(|e| format!("a temporary directory: {e}"))?;
    let bundle = tmp.path().join("snapshot.bundle");
    super::snapshot::bundle(snap, placement.has_head, &bundle)?;
    let transcript_before = stamp(&plan.transcript);
    let meta = super::transcript::export(
        plan.harness,
        &plan.transcript,
        &plan.cwd.to_string_lossy(),
        &plan.session_id,
    )?;
    let blob = |p: &Path| -> Result<Blob, String> {
        Ok(Blob {
            len: std::fs::metadata(p).map_err(|e| e.to_string())?.len(),
            sha256: super::sha256_file(p)?,
        })
    };
    let manifest = Manifest {
        protocol: PROTOCOL,
        harness: plan.harness,
        session_id: plan.session_id.clone(),
        repo: snap.repo.clone(),
        placement,
        snapshot: snap.commit.clone(),
        move_id: move_id.to_string(),
        bundle: blob(&bundle)?,
        transcript: blob(&plan.transcript)?,
        transcript_meta: meta,
    };
    let total = manifest.bundle.len + manifest.transcript.len;
    say(format!("Sending {} to {}…", mb(total), plan.machine));
    let progress = |sent: u64, total: u64| {
        // Every few hundred kilobytes, not every chunk: this is a status line.
        if sent == total || sent % (512 * 1024) < (1 << 16) {
            say(format!(
                "Sending {}/{} to {}…",
                mb(sent),
                mb(total),
                plan.machine
            ));
        }
    };
    // From here the far side may be running it, whatever this side heard.
    let receipt = match wire.receive(&manifest, &[&bundle, &plan.transcript], &progress) {
        Ok(receipt) => receipt,
        Err(why) => return Err(give_back(plan, wire, move_id, frozen, why)),
    };
    say(format!("{}: running — stopping it here…", plan.machine));

    // The fence: what arrived there has to be what is here, still. A file
    // written or a transcript line appended since the snapshot would be lost
    // by stopping this agent.
    let tree_now = super::snapshot::tree_of(&snap.root);
    let unchanged = tree_now.as_deref() == Ok(snap.tree.as_str())
        && stamp(&plan.transcript) == transcript_before;
    if !unchanged {
        return Err(give_back(
            plan,
            wire,
            &receipt.ticket,
            frozen,
            "the session changed while it was moving".into(),
        ));
    }

    // Still the agent the plan named: one that has ended while stopped left
    // its pid free, and the number can belong to anything by now.
    let ours = plan
        .pid
        .filter(|pid| crate::yolo::start_time(*pid) == plan.pid_start);
    match ours {
        Some(pid) => {
            // Only a tab whose own process is the agent goes with it. A
            // shell in a tab with `claude` typed into it is the person's
            // tab: it stays, and the shell with it.
            let tab =
                crate::rmux::holding(pid).filter(|tab| crate::rmux::agent_pid(tab) == Some(pid));
            if let Err(why) = crate::proc::terminate(pid) {
                return Err(give_back(
                    plan,
                    wire,
                    &receipt.ticket,
                    frozen,
                    format!("could not stop it here ({why})"),
                ));
            }
            frozen.thaw();
            if let Some(tab) = tab {
                let _ = crate::rmux::kill(&tab);
            }
        }
        None => frozen.thaw(),
    }
    super::record_moved(
        plan.harness.provider(),
        &plan.session_id,
        &Moved {
            machine: plan.machine.clone(),
            session_id: receipt.session_id.clone(),
            at: chrono::Utc::now().to_rfc3339(),
        },
    );
    let note = wire.finish(&receipt.ticket).err().map(|why| {
        format!(
            "{} did not confirm keeping it ({why}); it keeps it anyway",
            plan.machine
        )
    });
    Ok(Outcome {
        receipt,
        machine: plan.machine.clone(),
        note,
    })
}

/// A transcript's length and modification time: appended to, it changes.
fn stamp(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

fn mb(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b => format!("{} KB", b.div_ceil(1024)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A far side that answers what it is told.
    struct Far {
        aborts: std::sync::Mutex<Vec<Result<(), String>>>,
    }

    impl Wire for Far {
        fn machine(&self) -> &str {
            "box"
        }
        fn hello(&self) -> Result<super::super::Hello, String> {
            Err("unused".into())
        }
        fn plan(&self, _: &Ask) -> Result<Placement, String> {
            Err("unused".into())
        }
        fn receive(
            &self,
            _: &Manifest,
            _: &[&Path],
            _: &(dyn Fn(u64, u64) + Sync),
        ) -> Result<Receipt, String> {
            Err("unused".into())
        }
        fn finish(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn abort(&self, _: &str) -> Result<(), String> {
            let mut left = self.aborts.lock().unwrap();
            match left.is_empty() {
                true => Err("no answer".into()),
                false => left.remove(0),
            }
        }
    }

    fn plan(pid: Option<u32>, transcript: PathBuf) -> Plan {
        Plan {
            harness: super::super::Harness::Claude,
            session_id: "s".into(),
            title: "t".into(),
            cwd: "/".into(),
            transcript_stamp: stamp(&transcript),
            transcript,
            pid,
            pid_start: pid.and_then(crate::yolo::start_time),
            typed: None,
            repo: Repo {
                origin: None,
                key: "k".into(),
                name: "n".into(),
                head: "h".into(),
                branch: None,
                rel_cwd: String::new(),
            },
            placement: Placement {
                kind: super::super::Kind::Clone,
                dir: "/d".into(),
                root: None,
                has_head: false,
                note: None,
            },
            machine: "box".into(),
            size_hint: 0,
        }
    }

    fn stopped(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|s| {
                s.rsplit_once(')')
                    .map(|(_, rest)| rest.trim_start().starts_with('T'))
            })
            .unwrap_or(false)
    }

    /// The reply was lost after the bytes left. A far side that confirms the
    /// abort — even after asking to be asked again — gets the agent here
    /// thawed; one that never confirms leaves it stopped, with how to settle
    /// it by hand, and on record for the way out.
    #[test]
    fn an_agent_is_thawed_only_once_the_far_side_has_undone_it() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id();
        let transcript = tempfile::NamedTempFile::new().unwrap();
        let p = plan(Some(pid), transcript.path().to_path_buf());

        let far = Far {
            aborts: std::sync::Mutex::new(vec![Err("being received".into()), Ok(())]),
        };
        let frozen = Frozen::new(p.pid, &p.machine);
        crate::test_wait::eventually_true("the agent stopped", || stopped(pid));
        let said = give_back(&p, &far, "id", frozen, "the reply was lost".into());
        assert!(said.ends_with("nothing moved"), "{said}");
        crate::test_wait::eventually_true("the agent thawed", || !stopped(pid));
        assert_eq!(in_flight(), None);

        let silent = Far {
            aborts: std::sync::Mutex::new(Vec::new()),
        };
        let frozen = Frozen::new(p.pid, &p.machine);
        let said = give_back(&p, &silent, "id", frozen, "the reply was lost".into());
        assert!(said.contains(&format!("kill -CONT {pid}")), "{said}");
        assert!(stopped(pid), "thawed while the far side may be running it");
        assert_eq!(in_flight(), Some((pid, "box".into())));

        *FROZEN.lock().unwrap() = None;
        let _ = child.kill();
        let _ = child.wait();
    }

    /// A plan whose agent has written since `X`, or whose pid is no longer
    /// that agent, is not acted on.
    #[test]
    fn a_plan_the_agent_has_moved_past_is_refused() {
        let transcript = tempfile::NamedTempFile::new().unwrap();
        let p = plan(Some(std::process::id()), transcript.path().to_path_buf());
        still_current(&p).unwrap();
        let mut restarted = p.clone();
        restarted.pid_start = Some(1);
        assert!(still_current(&restarted).unwrap_err().contains("restarted"));
        std::fs::write(transcript.path(), "a turn\n").unwrap();
        assert!(
            still_current(&p)
                .unwrap_err()
                .contains("since X was pressed")
        );
    }
}
