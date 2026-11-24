//! Where Devin keeps its sessions: `sessions.db` for the index, with a
//! transcript per session beside it. Reading the transcripts stays in
//! cctop-core's `session::devin`.

use super::{Discovered, readonly};
use crate::{ActivityState, Permission, Provider, paths};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use std::path::Path;

/// Discover every persisted Devin session, newest activity first within each
/// home.
pub fn list() -> Vec<Discovered> {
    let mut sessions = Vec::new();
    for (i, cli) in paths::devin_cli_dirs().iter().enumerate() {
        // This user's own database honours `$CHISEL_SESSION_DB`; another
        // home's can only be the conventional one.
        let (db, transcripts) = match i {
            0 => (
                paths::DEVIN_SESSIONS_DB.clone(),
                paths::DEVIN_TRANSCRIPTS_DIR.clone(),
            ),
            _ => (cli.join("sessions.db"), cli.join("transcripts")),
        };
        list_in(&db, &transcripts, &mut sessions);
    }
    sessions
}

fn list_in(db_path: &Path, transcripts: &Path, sessions: &mut Vec<Discovered>) {
    let Ok(db) = readonly(db_path) else {
        return;
    };

    let mut stmt = match db.prepare(
        "SELECT id, working_directory, model, created_at, last_activity_at, title \
         FROM sessions WHERE hidden = 0 ORDER BY last_activity_at DESC",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return,
    };

    let rows = match stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    }) {
        Ok(rows) => rows,
        Err(_) => return,
    };

    for row in rows {
        let Ok((session_id, working_dir, model, created_at, last_activity_at, title)) = row else {
            continue;
        };
        let transcript = transcripts.join(format!("{session_id}.json"));
        // Without the transcript there is nothing to extract; the row would be
        // a name with no metrics, which is worse than not listing it.
        if !transcript.exists() {
            continue;
        }

        let mut session = Discovered::new(Provider::Devin, session_id);
        session.model = model;
        session.label_source = working_dir;
        // The database stamps seconds, not milliseconds.
        session.started_at = crate::time::ms_to_rfc3339(created_at * 1000);
        session.last_active = crate::time::ms_to_rfc3339(last_activity_at * 1000);
        session.title = title.filter(|t| !t.is_empty());
        session.data_file = Some(transcript);
        sessions.push(session);
    }
}

/// The database that goes with `transcript`: `transcripts/<id>.json` sits
/// beside `sessions.db`, in this user's home or, for root, in whoever's home
/// the session was read from. Asking the session rather than the static is
/// what keeps another user's row from being answered out of root's own
/// database, where its id is simply absent.
pub fn db_for(transcript: Option<&Path>) -> std::path::PathBuf {
    transcript
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|cli| cli.join("sessions.db"))
        .unwrap_or_else(|| paths::DEVIN_SESSIONS_DB.clone())
}

/// The session's live state and permission mode, read off the database.
///
/// Devin's transcript is one JSON document rather than a line-delimited log,
/// so the tail-walk in [`super::state::live_state`] cannot see inside it — but the
/// newest `message_nodes` row says the same thing faster: a user or tool
/// message means the agent owes a reply, and an assistant message is the end
/// of the turn unless its tool calls are still in flight. `agent_mode` on the
/// session row answers the second question `live_state` asks.
pub fn live_state(session: &Discovered) -> (ActivityState, Option<Permission>) {
    let Ok(db) = readonly(&db_for(session.data_file.as_deref())) else {
        return (ActivityState::Working, None);
    };
    (
        activity_state(&db, &session.session_id),
        permission_mode(&db, &session.session_id),
    )
}

pub fn activity_state(db: &Connection, session_id: &str) -> ActivityState {
    let raw = db
        .query_row(
            "SELECT chat_message FROM message_nodes \
             WHERE session_id = ?1 ORDER BY row_id DESC LIMIT 1",
            [session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten();
    let Some(raw) = raw else {
        return ActivityState::Working;
    };
    let Ok(message) = serde_json::from_str::<Value>(&raw) else {
        return ActivityState::Working;
    };

    match message.get("role").and_then(Value::as_str) {
        Some("assistant") => {
            match message.get("tool_calls").and_then(Value::as_array) {
                // No calls issued: the answer is finished and the prompt is
                // the user's — the same state a question tool leaves, since
                // the transcript cannot tell a held prompt from a held turn.
                Some(calls) if calls.is_empty() => ActivityState::WaitingForInput,
                None => ActivityState::WaitingForInput,
                Some(_) => ActivityState::Working,
            }
        }
        _ => ActivityState::Working,
    }
}

/// Devin's `--permission-mode`, translated into the column's four buckets.
///
/// `smart` — edits auto-approve plus a fast model auto-running clearly-safe
/// actions — has no honest bucket: `edits` understates it and `BYPASS`
/// overstates it, so it reports nothing rather than guess, which is the same
/// rule [`Permission::parse`] applies to modes it has not seen.
pub fn permission_mode(db: &Connection, session_id: &str) -> Option<Permission> {
    let mode: String = db
        .query_row(
            "SELECT agent_mode FROM sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()?;
    match mode.as_str() {
        "normal" | "auto" => Some(Permission::Ask),
        "accept-edits" => Some(Permission::AcceptEdits),
        "plan" => Some(Permission::Plan),
        "dangerous" | "yolo" | "bypass" | "autonomous" => Some(Permission::Bypass),
        _ => None,
    }
}
