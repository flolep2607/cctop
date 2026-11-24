//! Where Devin keeps its sessions: `sessions.db` for the index, with a
//! transcript per session beside it. Reading the transcripts stays in
//! cctop-core's `session::devin`.

use super::{Discovered, readonly};
use crate::{Provider, paths};
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
