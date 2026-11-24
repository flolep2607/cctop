//! Where OpenCode keeps its sessions — one SQLite database per release
//! channel — and the top-level sessions in each. Reading a session's messages
//! stays in cctop-core's `session::opencode`.

use super::{Discovered, opencode_line, readonly};
use crate::{ActivityState, Provider, paths};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Every OpenCode session on the machine, most recently active first. A
/// session migrated between channels is listed once, from the copy updated
/// last.
pub fn list() -> Vec<Discovered> {
    let mut sessions = Vec::new();
    for path in database_paths() {
        if let Ok(db) = readonly(&path) {
            sessions.extend(sessions_in(&db, &path));
        }
    }
    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    let mut seen = HashSet::new();
    sessions.retain(|s| seen.insert(s.session_id.clone()));
    sessions
}

/// Which storage layout a database holds.
///
/// OpenCode 2 replaced the `session`/`message`/`part` tables with
/// `session_v2`/`session_message` and migrated every old row forward, so a 2.x
/// database holds all its sessions in `session_v2` and leaves the legacy tables
/// frozen. Reading the old tables there would show only the sessions that
/// predate the migration and none written since — the bug this detection fixes.
/// The two layouts also differ in shape: a 2.x message carries its parts inline
/// under `data.content[]` rather than in a `part` table, and nests the model
/// under `data.model`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    V1,
    V2,
}

/// `V2` when the database has a `session_v2` table, else `V1`. One indexed
/// probe, dwarfed by the per-session reads it routes.
pub fn schema(db: &Connection) -> Schema {
    let has_v2 = db
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'session_v2'",
            [],
            |_| Ok(()),
        )
        .optional()
        .ok()
        .flatten()
        .is_some();
    if has_v2 { Schema::V2 } else { Schema::V1 }
}

/// The session table for this schema.
pub fn session_table(schema: Schema) -> &'static str {
    match schema {
        Schema::V1 => "session",
        Schema::V2 => "session_v2",
    }
}

/// Stable releases use `opencode.db`; nonstandard channels suffix the filename.
pub fn database_paths() -> Vec<PathBuf> {
    paths::opencode_data_roots()
        .into_iter()
        .flat_map(|root| {
            paths::list_dir(&root)
                .into_iter()
                .filter(|name| name.starts_with("opencode") && name.ends_with(".db"))
                .map(move |name| root.join(name))
        })
        .collect()
}

/// When a session last did something: the newest message's own update stamp,
/// or one of its parts' if later — a tool call finishing, a reply streaming in.
///
/// Not the session row's `time_updated`, which keeps a different clock.
/// OpenCode moves it for a rename, a share or an archive, and not for every
/// message: on a real database it trailed the newest message by anything from
/// a tenth of a second to fifteen hours. It is also the key a cached extraction
/// is checked against (see cctop-core's `session::effective_mtime_ms`), so a stamp that
/// missed a message kept that message's spend off the row until something else
/// moved it.
///
/// Read through the two indexes OpenCode keeps — a session's messages by time,
/// a message's parts — so it is two index probes per session, not a scan.
///
/// Not the newest message alone: a prompt queued while a reply is still
/// streaming is created at once, so it is the newest row while the reply
/// before it keeps writing. The newest few are read, and the latest stamp
/// among them wins.
///
/// ponytail: a subagent is a session of its own, so its work moves the
/// parent's stamp only when the `task` call that started it returns. And a
/// reply still streaming behind three or more queued prompts is not seen
/// until one of them starts.
const LAST_ENTRY: &str = "(SELECT max(x) FROM (SELECT max(m.time_updated, coalesce(\
     (SELECT max(p.time_updated) FROM part p WHERE p.message_id = m.id), 0)) AS x \
     FROM message m WHERE m.session_id = session.id \
     ORDER BY m.time_created DESC, m.id DESC LIMIT 3))";

/// [`LAST_ENTRY`] for OpenCode 2, where a message's parts are inline and the
/// `session_message` row's own `time_updated` is the newest stamp there is.
const LAST_ENTRY_V2: &str = "(SELECT max(time_updated) FROM (SELECT time_updated \
     FROM session_message WHERE session_id = session_v2.id ORDER BY seq DESC LIMIT 3))";

/// The top-level sessions in one OpenCode database.
pub fn sessions_in(db: &Connection, path: &Path) -> Vec<Discovered> {
    let mut sessions = Vec::new();
    let schema = schema(db);
    let table = session_table(schema);
    let last_entry = match schema {
        Schema::V1 => LAST_ENTRY,
        Schema::V2 => LAST_ENTRY_V2,
    };
    // A subagent is a session of its own, with `parent_id` naming the one that
    // spawned it; listed, each stands alone as a conversation. They are hidden
    // here and folded into their parent by `extract`, the way Claude's subagents
    // are. A database from before the column existed has no subagents to hide,
    // so it falls back to the unfiltered query.
    //
    // A database without the columns the last entry is read from falls back
    // the same way, to the row's own stamp.
    const COLUMNS: &str = "SELECT id, directory, title, model, time_created, time_updated";
    let candidates = [
        format!("{COLUMNS}, {last_entry} FROM {table} WHERE parent_id IS NULL"),
        format!("{COLUMNS}, {last_entry} FROM {table}"),
        format!("{COLUMNS}, NULL FROM {table} WHERE parent_id IS NULL"),
        format!("{COLUMNS}, NULL FROM {table}"),
    ];
    let Some(mut stmt) = candidates.iter().find_map(|sql| db.prepare(sql).ok()) else {
        return sessions;
    };
    // 2.x lets `title` be NULL; read it as optional so a row without one is
    // kept rather than dropped by a failed `String` conversion.
    let Ok(rows) = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, Option<i64>>(6)?,
        ))
    }) else {
        return sessions;
    };
    for row in rows.flatten() {
        let (id, directory, title, model_json, created, updated, last_entry) = row;
        let title = title.unwrap_or_default();
        let model = model_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
        let mut session = Discovered::new(Provider::OpenCode, id);
        session.started_at = crate::time::ms_to_rfc3339(created);
        session.last_active = crate::time::ms_to_rfc3339(last_entry.unwrap_or(updated));
        session.label_source = directory;
        session.title = (!title.is_empty()).then_some(title);
        session.model = model;
        session.data_file = Some(path.to_path_buf());
        sessions.push(session);
    }
    sessions
}

/// The `content[]` array of a 2.x message's `data`, or empty.
pub fn content_items(raw: &str) -> Vec<Value> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.get("content").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

pub fn activity_state(db: &Connection, session_id: &str) -> ActivityState {
    let schema = schema(db);
    // 1.x needs the message id to find the message's `part` rows; 2.x carries
    // the parts inline, so the row's own `data` is enough.
    let newest = match schema {
        Schema::V1 => db.query_row(
            "SELECT json_extract(data, '$.role'), id, data FROM message \
             WHERE session_id = ?1 ORDER BY time_created DESC, id DESC LIMIT 1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        ),
        Schema::V2 => db.query_row(
            "SELECT type, '', data FROM session_message \
             WHERE session_id = ?1 ORDER BY seq DESC LIMIT 1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        ),
    }
    .optional()
    .ok()
    .flatten();
    let Some((role, message_id, raw)) = newest else {
        return ActivityState::Working;
    };
    if role != "assistant" {
        return ActivityState::Working;
    }
    let Some(message) = opencode_line::parse::<opencode_line::Message>(&raw) else {
        return ActivityState::Working;
    };
    let waiting = match schema {
        Schema::V1 => has_input_request(db, &message_id),
        Schema::V2 => content_items(&raw).iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("tool")
                && item
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(super::state::is_input_request_tool)
        }),
    };
    if is_api_failure(&message) {
        ActivityState::ApiError
    } else if waiting {
        ActivityState::WaitingForInput
    } else {
        ActivityState::Working
    }
}

fn has_input_request(db: &Connection, message_id: &str) -> bool {
    let Ok(mut stmt) = db.prepare("SELECT data FROM part WHERE message_id = ?1") else {
        return false;
    };
    let Ok(rows) = stmt.query_map(params![message_id], |row| row.get::<_, String>(0)) else {
        return false;
    };
    rows.flatten().any(|raw| {
        opencode_line::parse::<opencode_line::Part>(&raw).is_some_and(|part| {
            part.kind.is("tool")
                && part
                    .tool
                    .get()
                    .is_some_and(super::state::is_input_request_tool)
        })
    })
}

fn is_api_failure(message: &opencode_line::Message) -> bool {
    let Some(text) = message.error_text() else {
        return false;
    };
    [
        "timeout",
        "timed out",
        "rate limit",
        "overloaded",
        "connection",
        "network",
        "api error",
        "apierror",
        "internal server",
        "queue is full",
        // OpenCode 2 names failures `provider.rate-limit`, `provider.quota`,
        // `provider.transport` and so on — every provider fault, as against the
        // `aborted` and `unknown` it leaves out, which are not the model hitting
        // a wall.
        "provider.",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

/// [`activity_state`] for the session `session_id` in the database at `path`,
/// opened read-only for this one read. cctop-core asks through its own
/// connection pool instead.
pub fn activity_state_at(path: &Path, session_id: &str) -> ActivityState {
    readonly(path)
        .map(|db| activity_state(&db, session_id))
        .unwrap_or(ActivityState::Working)
}
