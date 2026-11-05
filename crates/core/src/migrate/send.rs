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
//!   The agent is frozen only for those seconds, and thawed by [`Frozen`]'s
//!   drop on every way out — a cctop that dies mid-move leaves no agent
//!   stopped forever.

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
    Ok(Plan {
        harness,
        session_id: session.session_id.clone(),
        title: session.display_label().to_string(),
        cwd,
        transcript,
        pid: session.is_running().then(|| session.root_pid()).flatten(),
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
    Ok(())
}

/// An agent held still with SIGSTOP, and let go again however the move ends.
struct Frozen(Option<u32>);

impl Frozen {
    fn new(pid: Option<u32>) -> Frozen {
        if let Some(pid) = pid {
            // SAFETY: a signal to a pid this user's session reported as its own.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGSTOP) };
        }
        Frozen(pid)
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            // SAFETY: as above. Sent after a SIGTERM too, which a stopped
            // process would otherwise not act on until it was continued.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGCONT) };
        }
    }
}

/// Do the move. `say` hears each step; it runs on the caller's thread.
pub fn run(plan: &Plan, wire: &dyn Wire, say: &(dyn Fn(String) + Sync)) -> Result<Outcome, String> {
    let move_id = crate::util::new_uuid();
    let frozen = Frozen::new(plan.pid);
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
    // is asked again rather than trusted.
    let mut placement = plan.placement.clone();
    if snap.repo.head != plan.repo.head {
        let mut again = plan.clone();
        again.repo = snap.repo.clone();
        let dir = placement.dir.clone();
        replan(&mut again, &dir, wire)?;
        placement = again.placement;
    }
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
    let receipt = wire.receive(&manifest, &[&bundle, &plan.transcript], &progress)?;
    say(format!("{}: running — stopping it here…", plan.machine));

    // The fence: what arrived there has to be what is here, still. A file
    // written or a transcript line appended since the snapshot would be lost
    // by stopping this agent.
    let tree_now = super::snapshot::tree_of(&snap.root);
    let unchanged = tree_now.as_deref() == Ok(snap.tree.as_str())
        && stamp(&plan.transcript) == transcript_before;
    if !unchanged {
        let undone = wire.abort(&receipt.ticket);
        drop(frozen);
        return Err(match undone {
            Ok(()) => "the session changed while it was moving — nothing moved".into(),
            Err(why) => format!(
                "the session changed while it was moving, and {} could not undo its copy: {why}",
                plan.machine
            ),
        });
    }

    if let Some(pid) = plan.pid {
        let tab = crate::rmux::holding(pid);
        if let Err(why) = crate::proc::terminate(pid) {
            let _ = wire.abort(&receipt.ticket);
            drop(frozen);
            return Err(format!("could not stop it here ({why}) — nothing moved"));
        }
        drop(frozen);
        if let Some(tab) = tab {
            let _ = crate::rmux::kill(&tab);
        }
    } else {
        drop(frozen);
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
