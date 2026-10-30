//! OpenCode session discovery and extraction from its current SQLite store.

use super::extract;
use super::opencode_line::{self, Message, Part};
use super::readonly;
use super::{
    ActivityState, ContextUsage, Costs, FallbackRates, ModelBreakdown, Session, SessionData,
    Subagent, SubagentStatus, Tokens,
};
use crate::config;
use crate::pricing::Provider;
use crate::util;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The line delta behind OpenCode's built-in editing tools, and the patch to
/// show for it.
///
/// `edit` is handed the text it replaces and the text that takes its place, so
/// its diff is the literal before and after. `write` can replace a file
/// wholesale, so its removed lines cannot be known from the recorded input and
/// are deliberately not guessed. `apply_patch` carries a real patch.
///
/// Read by the table's extraction and by the conversation reader alike: what
/// counts as an edit is one fact about this harness, not one per caller.
pub fn tool_delta(name: &str, state: &Value) -> Option<super::Delta> {
    if state.get("status").and_then(Value::as_str) != Some("completed") {
        return None;
    }
    input_delta(name, state.get("input")?)
}

/// [`tool_delta`] for a call already known to have completed.
fn input_delta(name: &str, input: &Value) -> Option<super::Delta> {
    match name {
        "edit" => extract::edit_delta(input),
        "apply_patch" => input
            .get("patch")
            .and_then(Value::as_str)
            .map(|patch| extract::parse_apply_patch(patch).1),
        _ => None,
    }
}

thread_local! {
    /// Read-only connections, kept open and reused.
    ///
    /// Every read of an OpenCode database goes through here — discovery,
    /// extraction, the activity dot, the context window, the last tool, and the
    /// transcript scan behind search. Opening per read came to several thousand
    /// opens of a 555 MB database per walk, and the cost was not the reading:
    /// 240,000 voluntary context switches and nine seconds of kernel time, spent
    /// almost entirely on setting up and tearing down file locks. Extraction
    /// alone — the one call a walk makes per session, and the heaviest of them by
    /// a factor of three — had been paying that price per session all along.
    ///
    /// Thread-local rather than shared, which is what makes it safe *and*
    /// fast. `SQLITE_OPEN_NO_MUTEX` means a connection may not be used from two
    /// threads at once, so one shared connection would have to be behind a
    /// mutex — and extraction fans out across every core, so that mutex would
    /// serialise the one path that most needs not to be. A connection per
    /// thread has neither problem: the serial tail pass opens one, and a cold
    /// extraction opens one per worker instead of one per session.
    ///
    /// ponytail: held for the life of the thread, never revalidated. SQLite
    /// writes in place and a read-only connection picks up another process's
    /// commits, so this stays correct while OpenCode is running; a database
    /// swapped out wholesale underneath it would be read from the old file
    /// until cctop restarts.
    static CONNECTIONS: RefCell<HashMap<PathBuf, Connection>> = RefCell::new(HashMap::new());
}

/// Release this thread's cached connections.
///
/// A cached connection keeps a handle on the database, so a test that writes a
/// scratch database, reads it, and then removes it has to say when it is done.
/// Nothing in the program needs this: the only deletion cctop performs is
/// [`delete`], which removes a *row* through its own connection.
#[cfg(test)]
fn close_databases() {
    CONNECTIONS.with(|cache| cache.borrow_mut().clear());
}

/// Run `query` against `path`'s database, opening it only the first time this
/// thread asks. `None` if it cannot be opened.
fn with_db<T>(path: &Path, query: impl FnOnce(&Connection) -> T) -> Option<T> {
    CONNECTIONS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(path) {
            cache.insert(path.to_path_buf(), readonly(path).ok()?);
        }
        cache.get(path).map(query)
    })
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
enum Schema {
    V1,
    V2,
}

/// `V2` when the database has a `session_v2` table, else `V1`. One indexed
/// probe, dwarfed by the per-session reads it routes.
fn schema(db: &Connection) -> Schema {
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
fn session_table(schema: Schema) -> &'static str {
    match schema {
        Schema::V1 => "session",
        Schema::V2 => "session_v2",
    }
}

/// Reshape one OpenCode 2 `content[]` tool item into the flat `part` shape the
/// typed [`Part`] reader and [`record_tool`] already understand: `name` becomes
/// `tool`, `id` becomes `callID`, and the item-level `time: {created, completed}`
/// becomes the state's `time: {start, end}`.
fn v2_tool_part(item: &Value) -> Value {
    let state = item.get("state");
    let time = item.get("time");
    serde_json::json!({
        "type": "tool",
        "tool": item.get("name").and_then(Value::as_str).unwrap_or("unknown"),
        "callID": item.get("id").and_then(Value::as_str).unwrap_or(""),
        "state": {
            "status": state.and_then(|s| s.get("status")).and_then(Value::as_str).unwrap_or(""),
            "input": state.and_then(|s| s.get("input")).cloned().unwrap_or(Value::Null),
            "time": {
                "start": time.and_then(|t| t.get("created")).cloned().unwrap_or(Value::Null),
                "end": time.and_then(|t| t.get("completed")).cloned().unwrap_or(Value::Null),
            },
        },
    })
}

/// A session's assistant turns, oldest first, each paired with whether it is a
/// compaction summary turn whose model and effort must not be taken as the
/// conversation's own. In 2.x the compaction is its own `compaction` row,
/// carrying the same spend an assistant turn does; in 1.x it was an assistant
/// turn flagged in its own `data` (seen by [`Message::is_summary`]).
fn usage_rows(db: &Connection, schema: Schema, session_id: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let sql = match schema {
        Schema::V1 => {
            "SELECT data, '' FROM message WHERE session_id = ?1 \
             AND json_extract(data, '$.role') = 'assistant' ORDER BY time_created, id"
        }
        Schema::V2 => {
            "SELECT data, type FROM session_message WHERE session_id = ?1 \
             AND type IN ('assistant', 'compaction') ORDER BY seq"
        }
    };
    let Ok(mut stmt) = db.prepare(sql) else {
        return out;
    };
    let Ok(rows) = stmt.query_map(params![session_id], |row| {
        let data = row.get::<_, String>(0)?;
        // 1.x has one column; 2.x's second column is the message `type`, which
        // is `compaction` for the summary turn.
        let summary = match schema {
            Schema::V1 => false,
            Schema::V2 => row.get::<_, String>(1)? == "compaction",
        };
        Ok((data, summary))
    }) else {
        return out;
    };
    out.extend(rows.flatten());
    out
}

/// A session's parts as flat `part` JSON, oldest first, for the extract loop to
/// parse and keep the `tool` ones. In 1.x these are the `part` rows as written
/// — read whole and filtered in Rust, so a row of malformed JSON is skipped
/// rather than failing a `json_extract` over the table. In 2.x they are the
/// `tool` items inside each assistant message's `content[]`, reshaped by
/// [`v2_tool_part`].
fn tool_part_jsons(db: &Connection, schema: Schema, session_id: &str) -> Vec<String> {
    match schema {
        Schema::V1 => {
            let mut out = Vec::new();
            if let Ok(mut stmt) =
                db.prepare("SELECT data FROM part WHERE session_id = ?1 ORDER BY time_created, id")
                && let Ok(rows) = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))
            {
                out.extend(rows.flatten());
            }
            out
        }
        Schema::V2 => v2_tool_items(db, session_id)
            .iter()
            .map(|item| v2_tool_part(item).to_string())
            .collect(),
    }
}

/// Every `tool` item across a 2.x session's assistant messages, oldest first.
fn v2_tool_items(db: &Connection, session_id: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let Ok(mut stmt) = db.prepare(
        "SELECT data FROM session_message WHERE session_id = ?1 AND type = 'assistant' ORDER BY seq",
    ) else {
        return out;
    };
    let Ok(rows) = stmt.query_map(params![session_id], |row| row.get::<_, String>(0)) else {
        return out;
    };
    for raw in rows.flatten() {
        for item in content_items(&raw) {
            if item.get("type").and_then(Value::as_str) == Some("tool") {
                out.push(item);
            }
        }
    }
    out
}

/// The `content[]` array of a 2.x message's `data`, or empty.
fn content_items(raw: &str) -> Vec<Value> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.get("content").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

/// How many times this session was compacted.
fn compaction_count(db: &Connection, schema: Schema, session_id: &str) -> u32 {
    let sql = match schema {
        Schema::V1 => {
            // `json_valid` guards the extract: one malformed `part` row otherwise
            // fails the whole count.
            "SELECT count(*) FROM part WHERE session_id = ?1 \
             AND json_valid(data) AND json_extract(data, '$.type') = 'compaction'"
        }
        Schema::V2 => {
            "SELECT count(*) FROM session_message WHERE session_id = ?1 AND type = 'compaction'"
        }
    };
    db.query_row(sql, params![session_id], |row| row.get::<_, u32>(0))
        .unwrap_or(0)
}

/// State of the newest OpenCode message. OpenCode keeps events in SQLite rather
/// than a transcript file, but its assistant messages record both completion
/// and provider errors.
pub fn extract_activity_state(path: &Path, session_id: &str) -> ActivityState {
    with_db(path, |db| activity_state(db, session_id)).unwrap_or(ActivityState::Working)
}

fn activity_state(db: &Connection, session_id: &str) -> ActivityState {
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
                    .is_some_and(super::is_input_request_tool)
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
            part.kind.is("tool") && part.tool.get().is_some_and(super::is_input_request_tool)
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

/// Stable releases use `opencode.db`; nonstandard channels suffix the filename.
fn database_paths() -> Vec<PathBuf> {
    config::opencode_data_roots()
        .into_iter()
        .flat_map(|root| {
            config::list_dir(&root)
                .into_iter()
                .filter(|name| name.starts_with("opencode") && name.ends_with(".db"))
                .map(move |name| root.join(name))
        })
        .collect()
}

/// Every message one session recorded, oldest first, with its own parts.
///
/// `message` holds the envelope and `part` the words and the calls, so the two
/// are read as one ordered pass: the join is the only place that knows which
/// part belongs to which message, and a reader needs them together.
///
/// Ponytail: the whole session is read, not the newest of it. A subagent's
/// messages are not in it — a subagent is its own session, so a parent's
/// conversation cannot pick one up by accident — but a parent that delegated
/// reads as though it did not.
pub fn for_each_message(path: &Path, session_id: &str, mut f: impl FnMut(&Value, i64, &[Value])) {
    with_db(path, |db| {
        if schema(db) == Schema::V2 {
            for_each_message_v2(db, session_id, &mut f);
            return;
        }
        let Ok(mut stmt) = db.prepare(
            "SELECT m.id, m.time_created, m.data, p.data FROM message m \
             LEFT JOIN part p ON p.message_id = m.id \
             WHERE m.session_id = ?1 ORDER BY m.time_created, m.id, p.id",
        ) else {
            return;
        };
        let Ok(mut rows) = stmt.query(params![session_id]) else {
            return;
        };
        let mut current = String::new();
        let mut created = 0i64;
        let mut message = Value::Null;
        let mut parts: Vec<Value> = Vec::new();
        while let Ok(Some(row)) = rows.next() {
            let Ok(id) = row.get::<_, String>(0) else {
                continue;
            };
            if id != current {
                if !current.is_empty() && !message.is_null() {
                    f(&message, created, &parts);
                }
                current = id.clone();
                created = row
                    .get::<_, Option<i64>>(1)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                message = row
                    .get::<_, Option<String>>(2)
                    .ok()
                    .flatten()
                    .and_then(|raw| serde_json::from_str(&raw).ok())
                    .unwrap_or(Value::Null);
                parts.clear();
            }
            // A message with no parts at all is a `LEFT JOIN` that found nothing,
            // and one whose part will not parse is the same as not being there.
            if let Ok(Some(raw)) = row.get::<_, Option<String>>(3)
                && let Ok(part) = serde_json::from_str::<Value>(&raw)
            {
                parts.push(part);
            }
        }
        if !current.is_empty() && !message.is_null() {
            f(&message, created, &parts);
        }
    });
}

/// [`for_each_message`] for an OpenCode 2 database, where a message carries its
/// parts inline under `data.content[]` rather than in a `part` table.
///
/// Each row is reshaped into the `(message, parts)` the sink already reads: the
/// `type` column becomes the `role` the sink keys on, a user turn's inline
/// `text` becomes a text part, a `compaction` row becomes the one marker part,
/// and each assistant `content` item is passed through — tool items reshaped by
/// [`v2_sink_part`] so the sink's `opencode_call` finds `tool`, `input`,
/// `output` and `error` where it expects them.
fn for_each_message_v2(
    db: &Connection,
    session_id: &str,
    f: &mut impl FnMut(&Value, i64, &[Value]),
) {
    let Ok(mut stmt) = db.prepare(
        "SELECT type, time_created, data FROM session_message \
         WHERE session_id = ?1 ORDER BY seq",
    ) else {
        return;
    };
    let Ok(mut rows) = stmt.query(params![session_id]) else {
        return;
    };
    while let Ok(Some(row)) = rows.next() {
        let (Ok(kind), Ok(created), Ok(raw)) = (
            row.get::<_, String>(0),
            row.get::<_, i64>(1),
            row.get::<_, String>(2),
        ) else {
            continue;
        };
        let mut message = serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null);
        let role = match kind.as_str() {
            "assistant" => "assistant",
            "user" => "user",
            _ => "system",
        };
        if let Some(obj) = message.as_object_mut() {
            obj.insert("role".into(), Value::from(role));
        }
        let parts: Vec<Value> = match kind.as_str() {
            "assistant" => content_items(&raw).iter().map(v2_sink_part).collect(),
            "user" => message
                .get("text")
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
                .map(|t| vec![serde_json::json!({"type": "text", "text": t})])
                .unwrap_or_default(),
            "compaction" => vec![serde_json::json!({"type": "compaction"})],
            _ => Vec::new(),
        };
        if !message.is_null() {
            f(&message, created, &parts);
        }
    }
}

/// Reshape one OpenCode 2 `content[]` item into the part shape the conversation
/// sink's `opencode_call` reads. Text and reasoning items already match; a tool
/// item's `name`/`state.content`/`state.error.message` are moved to the
/// `tool`/`state.output`/`state.error` the sink looks for.
fn v2_sink_part(item: &Value) -> Value {
    if item.get("type").and_then(Value::as_str) != Some("tool") {
        return item.clone();
    }
    let state = item.get("state");
    serde_json::json!({
        "type": "tool",
        "tool": item.get("name").and_then(Value::as_str).unwrap_or("tool"),
        "state": {
            "status": state.and_then(|s| s.get("status")).cloned().unwrap_or(Value::Null),
            "input": state.and_then(|s| s.get("input")).cloned().unwrap_or(Value::Null),
            "output": state.and_then(|s| s.get("content")).cloned().unwrap_or(Value::Null),
            // The error is an object `{type, message}`; the sink wants the words.
            "error": state
                .and_then(|s| s.get("error"))
                .and_then(|e| e.get("message").or(Some(e)))
                .cloned()
                .unwrap_or(Value::Null),
        },
    })
}

/// Feed every stored message and part of one session to `feed`, stopping early
/// once it says the session is found.
///
/// This exists so that searching shares the connection pool rather than opening
/// the database per candidate: search runs as the user types, and the pool is
/// what keeps that from costing an open of a database that may be half a
/// gigabyte, once per session, per keystroke.
///
/// `part` carries the text of a message; `message` carries the envelope, and the
/// tool calls that are worth finding a session by. Both are per-session in this
/// schema, so neither can leak another session's text into this hit. The `part`
/// table is absent in older databases, which is a failed prepare rather than an
/// empty result — hence trying each in turn.
pub fn scan_session(path: &Path, session_id: &str, mut feed: impl FnMut(&str) -> bool) -> bool {
    with_db(path, |db| {
        // 2.x keeps a message's text and tool calls inline in its own `data`,
        // so the one `session_message` table carries everything to search.
        let queries: &[&str] = match schema(db) {
            Schema::V1 => &[
                "SELECT data FROM part WHERE session_id = ?1 ORDER BY id",
                "SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created, id",
            ],
            Schema::V2 => &["SELECT data FROM session_message WHERE session_id = ?1 ORDER BY seq"],
        };
        for sql in queries {
            let Ok(mut stmt) = db.prepare(sql) else {
                continue;
            };
            let Ok(rows) = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))
            else {
                continue;
            };
            for raw in rows.flatten() {
                if feed(&raw) {
                    return true;
                }
            }
        }
        false
    })
    .unwrap_or(false)
}

/// When a message was written, in RFC-3339.
///
/// The column is the fallback because `data.time.created` is the envelope's own
/// idea of when it was spoken, which is not always when the row landed.
pub fn message_time(message: &Value, column: i64) -> String {
    let ms = message
        .pointer("/time/created")
        .and_then(Value::as_i64)
        .unwrap_or(column);
    util::ms_to_rfc3339(ms)
}

/// Discover sessions from every OpenCode channel database, preferring the most
/// recently updated copy if the same session was migrated between channels.
pub fn list_sessions() -> Vec<Session> {
    let mut sessions = Vec::new();
    for path in database_paths() {
        // Pooled per database rather than opened per walk: this is the walk's
        // first and broadest read, and it opens one connection per channel
        // either way — the table below is then reused by every extract that
        // follows on the same thread.
        let Some(found) = with_db(&path, |db| sessions_in(db, &path)) else {
            continue;
        };
        sessions.extend(found);
    }

    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    let mut seen = HashSet::new();
    sessions.retain(|s| seen.insert(s.session_id.clone()));
    sessions
}

/// When a session last did something: the newest message's own update stamp,
/// or one of its parts' if later — a tool call finishing, a reply streaming in.
///
/// Not the session row's `time_updated`, which keeps a different clock.
/// OpenCode moves it for a rename, a share or an archive, and not for every
/// message: on a real database it trailed the newest message by anything from
/// a tenth of a second to fifteen hours. It is also the key a cached extraction
/// is checked against (see [`super::effective_mtime_ms`]), so a stamp that
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

fn sessions_in(db: &Connection, path: &Path) -> Vec<Session> {
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
        let mut session = Session::new(Provider::OpenCode, id);
        session.started_at = util::ms_to_rfc3339(created);
        session.last_active = util::ms_to_rfc3339(last_entry.unwrap_or(updated));
        session.label_source = directory;
        session.title = (!title.is_empty()).then_some(title);
        session.model = model;
        session.data_file = Some(path.to_path_buf());
        sessions.push(session);
    }
    sessions
}

fn assistant_usage(
    message: &Message,
    rates: &mut FallbackRates,
) -> (String, String, Tokens, Costs) {
    let model = message.model_name().unwrap_or("unknown").to_string();
    let ts = message
        .created_ms()
        .map(util::ms_to_rfc3339)
        .unwrap_or_default();
    let tokens = message.tokens();
    // OpenCode writes `cost: 0` for providers it has no rates for, which is
    // indistinguishable in the record from a genuinely free model. A cost it did
    // compute is authoritative; a zero falls back to pricing the tokens.
    let reported = message.cost.value().unwrap_or(0.0);
    let total = if reported > 0.0 || tokens.total == 0 {
        reported
    } else {
        rates.costs(&model, &tokens).map_or(0.0, |c| c.total)
    };
    let costs = Costs {
        total,
        ..Default::default()
    };
    (model, ts, tokens, costs)
}

fn add_usage(target: &mut SessionData, tokens: &Tokens, costs: &Costs) {
    target.tokens.input += tokens.input;
    target.tokens.output += tokens.output;
    target.tokens.cache_read += tokens.cache_read;
    target.tokens.cache_write_5m += tokens.cache_write_5m;
    target.tokens.reasoning_output += tokens.reasoning_output;
    target.tokens.total += tokens.total;
    target.costs.total += costs.total;
}

/// Extract accounting and tools for one OpenCode session.
///
/// The database records a cost on every assistant message, and that figure wins
/// wherever OpenCode knows the provider's rates. It reports zero for providers
/// it has no pricing for, so a zero falls back to pricing the tokens against
/// LiteLLM rather than being taken at face value.
pub fn extract(path: &Path, session_id: &str) -> SessionData {
    // Pooled like every other read of this database, and for the same reason it
    // matters most here: this is the call a walk makes once per session, so the
    // one-per-session open this used to pay was the shape of a whole walk's cost.
    with_db(path, |db| extract_of(db, session_id)).unwrap_or_else(|| SessionData {
        error: Some(format!(
            "Could not open OpenCode database {}",
            path.display()
        )),
        ..Default::default()
    })
}

/// Count one tool call: its detail, its outcome, the lines it changed, and the
/// fetch, search or skill it was.
fn record_tool(
    data: &mut SessionData,
    part: &Part,
    seen_urls: &mut HashSet<String>,
    seen_queries: &mut HashSet<String>,
) {
    let name = part.tool.get().unwrap_or("unknown");
    let no_state = opencode_line::ToolState::default();
    let state = part.state.get().unwrap_or(&no_state);
    let args = state.input.value();
    let (short, full) = extract::tool_detail(name, &args);
    let ts = state
        .time
        .get()
        .and_then(|t| t.start.value())
        .map(util::ms_to_rfc3339)
        .unwrap_or_default();
    extract::push_tool_detail(
        &mut data.metrics.tool_details,
        name,
        short,
        full,
        ts,
        part.call_id.get().map(str::to_string),
        None,
    );
    let m = &mut data.metrics;
    *m.tools.entry(name.to_string()).or_insert(0) += 1;
    m.tool_count += 1;
    // `invalid` is what OpenCode turns a call into when the model named a tool
    // that does not exist or handed one arguments it refused: it "completes",
    // but the call the model meant never ran.
    let failed = state.status.is("error") || name == "invalid";
    m.tool_errors += u64::from(failed);
    let delta = state
        .status
        .is("completed")
        .then(|| input_delta(name, &args))
        .flatten();
    if let Some(d) = &delta {
        m.lines_added += u64::from(d.added);
        m.lines_removed += u64::from(d.removed);
    }

    let arg = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
    };
    match name {
        "webfetch" => {
            if let Some(url) = arg("url") {
                extract::note_web(&mut m.web_fetches, &mut m.web_fetch_count, seen_urls, url);
            }
        }
        "websearch" | "codesearch" => {
            if let Some(q) = arg("query") {
                extract::note_web(
                    &mut m.web_searches,
                    &mut m.web_search_count,
                    seen_queries,
                    q,
                );
            }
        }
        "skill" => {
            if let Some(skill) = arg("name") {
                *m.skills.entry(skill.to_string()).or_insert(0) += 1;
                m.skill_count += 1;
            }
        }
        _ => {}
    }

    if let Some(detail) = m
        .tool_details
        .get_mut(name)
        .and_then(|details| details.last_mut())
    {
        detail.delta = delta;
        detail.dur_ms = state.duration_ms();
        detail.failed = failed;
    }
}

fn extract_of(db: &Connection, session_id: &str) -> SessionData {
    let mut data = SessionData::default();
    let mut breakdown: HashMap<String, (Tokens, Costs)> = HashMap::new();
    let mut saw_usage = false;
    let mut rates = FallbackRates::default();
    let schema = schema(db);

    let aggregate = db
        .query_row(
            &format!(
                "SELECT title, model, cost, tokens_input, tokens_output, tokens_reasoning, \
                 tokens_cache_read, tokens_cache_write FROM {} WHERE id = ?1",
                session_table(schema)
            ),
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, u64>(5)?,
                    row.get::<_, u64>(6)?,
                    row.get::<_, u64>(7)?,
                ))
            },
        )
        .optional()
        .ok()
        .flatten();
    if let Some((title, model_json, ..)) = &aggregate {
        data.title = title
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        data.last_model = model_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
    }

    for (raw, is_summary) in usage_rows(db, schema, session_id) {
        let Some(message) = opencode_line::parse::<Message>(&raw) else {
            continue;
        };
        let is_summary = is_summary || message.is_summary();
        let (model, ts, tokens, costs) = assistant_usage(&message, &mut rates);
        saw_usage = true;
        // The newest turn's, as Codex's is: a session that was switched
        // from `high` to the default shows the default. A compaction's
        // summary turn is the summariser's, not a switch, so it leaves
        // both as the conversation set them.
        if !is_summary {
            data.last_model = model.clone();
            data.reasoning_effort = message.variant_name().map(str::to_string);
        }
        data.metrics.api_duration_ms += message.duration_ms().unwrap_or(0);
        add_usage(&mut data, &tokens, &costs);
        let (mt, mc) = breakdown.entry(model.clone()).or_default();
        mt.input += tokens.input;
        mt.output += tokens.output;
        mt.cache_read += tokens.cache_read;
        mt.cache_write_5m += tokens.cache_write_5m;
        mt.reasoning_output += tokens.reasoning_output;
        mt.total += tokens.total;
        mc.total += costs.total;
        if let Some(dt) = util::parse_ts(&ts) {
            data.record_cost(&dt, &model, costs.total);
            // Reasoning is already inside `output` here — billing it again
            // would double-count, the same rule `FallbackRates` applies.
            let billed = tokens.all_input() + tokens.output;
            *data
                .tokens_by_day
                .entry(util::local_date_key(&dt))
                .or_default()
                .entry(model.clone())
                .or_insert(0) += billed;
            *data
                .tokens_by_hour
                .entry(util::local_hour_key(&dt))
                .or_default()
                .entry(model)
                .or_insert(0) += billed;
        }
    }

    if !saw_usage
        && let Some((_, _, cost, input, output, reasoning, cache_read, cache_write)) = aggregate
    {
        data.tokens = Tokens {
            input,
            output,
            cache_read,
            cache_write_5m: cache_write,
            reasoning_output: reasoning,
            total: input + output + cache_read + cache_write,
            ..Default::default()
        };
        data.costs.total = if cost > 0.0 || data.tokens.total == 0 {
            cost
        } else {
            rates
                .costs(&data.last_model, &data.tokens)
                .map_or(0.0, |c| c.total)
        };
    }

    let mut seen_urls = HashSet::new();
    let mut seen_queries = HashSet::new();
    for raw in tool_part_jsons(db, schema, session_id) {
        if let Some(part) = opencode_line::parse::<Part>(&raw)
            && part.kind.is("tool")
        {
            record_tool(&mut data, &part, &mut seen_urls, &mut seen_queries);
        }
    }
    // Written where OpenCode cut the conversation down to a summary,
    // automatically or because the person asked. A 1.x compaction is a `part`
    // row; a 2.x one is a `compaction` message.
    data.compactions = compaction_count(db, schema, session_id);

    fold_subagents(
        db,
        schema,
        session_id,
        &mut data,
        &mut breakdown,
        &mut rates,
    );

    let mut models: Vec<_> = breakdown.into_iter().collect();
    models.sort_by(|a, b| a.0.cmp(&b.0));
    data.models = models.iter().map(|(m, _)| m.clone()).collect();
    data.model_breakdown = models
        .into_iter()
        .map(|(model, (tokens, costs))| ModelBreakdown {
            model,
            total: costs.total,
            tokens,
            costs,
        })
        .collect();
    data
}

/// Fold a session's subagents into it.
///
/// OpenCode records each subagent as a session of its own, linked by
/// `parent_id`, where Claude keeps its subagents inside one transcript. So the
/// spend that Claude's parser sums for free has to be summed here by hand: each
/// child's tokens and cost are added to the parent's totals — otherwise hiding
/// the child rows would hide their spend — and each child is listed under the
/// parent rather than standing alone.
///
/// The subagent's own metrics come straight off its session row, which already
/// carries the aggregate OpenCode keeps; only the tool count is a query. The
/// per-message context window and tool history are left out, as its numbers on
/// the parent's Subagents pane do not need them.
fn fold_subagents(
    db: &Connection,
    schema: Schema,
    parent_id: &str,
    data: &mut SessionData,
    breakdown: &mut HashMap<String, (Tokens, Costs)>,
    rates: &mut FallbackRates,
) {
    let Ok(mut stmt) = db.prepare(&format!(
        "SELECT id, agent, title, model, cost, tokens_input, tokens_output, tokens_reasoning, \
         tokens_cache_read, tokens_cache_write, time_created, time_updated \
         FROM {} WHERE parent_id = ?1 ORDER BY time_created, id",
        session_table(schema)
    )) else {
        return;
    };
    let Ok(rows) = stmt.query_map(params![parent_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, f64>(4)?,
            row.get::<_, u64>(5)?,
            row.get::<_, u64>(6)?,
            row.get::<_, u64>(7)?,
            row.get::<_, u64>(8)?,
            row.get::<_, u64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
        ))
    }) else {
        return;
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    for row in rows.flatten() {
        let (
            id,
            agent,
            title,
            model_json,
            cost,
            input,
            output,
            reasoning,
            read,
            write,
            created,
            updated,
        ) = row;
        // The row's model column can be NULL — OpenCode leaves it unset on
        // some subagents — while every message the subagent wrote still names
        // its model, so the last one speaks for it. An empty name would be
        // priced as whatever model it matched and charted as a model of its own.
        let model = model_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
            .filter(|m| !m.is_empty())
            .or_else(|| {
                let sql = match schema {
                    Schema::V1 => {
                        "SELECT json_extract(data, '$.modelID') FROM message WHERE session_id = ?1 \
                         AND json_extract(data, '$.role') = 'assistant' \
                         AND json_extract(data, '$.modelID') <> '' \
                         ORDER BY time_created DESC, id DESC LIMIT 1"
                    }
                    Schema::V2 => {
                        "SELECT json_extract(data, '$.model.id') FROM session_message \
                         WHERE session_id = ?1 AND type = 'assistant' \
                         AND json_extract(data, '$.model.id') <> '' ORDER BY seq DESC LIMIT 1"
                    }
                };
                db.query_row(sql, params![id], |row| row.get::<_, String>(0))
                    .ok()
            })
            .unwrap_or_else(|| "unknown".to_string());
        let tokens = Tokens {
            input,
            output,
            cache_read: read,
            cache_write_5m: write,
            reasoning_output: reasoning,
            total: input + output + read + write,
            ..Default::default()
        };
        // The same zero-cost fallback the parent uses: a provider OpenCode has
        // no rates for reports nothing, and its tokens are priced from LiteLLM.
        let cost = if cost > 0.0 || tokens.total == 0 {
            cost
        } else {
            rates.costs(&model, &tokens).map_or(0.0, |c| c.total)
        };

        add_usage(
            data,
            &tokens,
            &Costs {
                total: cost,
                ..Default::default()
            },
        );
        let (mt, mc) = breakdown.entry(model.clone()).or_default();
        mt.input += tokens.input;
        mt.output += tokens.output;
        mt.cache_read += tokens.cache_read;
        mt.cache_write_5m += tokens.cache_write_5m;
        mt.reasoning_output += tokens.reasoning_output;
        mt.total += tokens.total;
        mc.total += cost;
        // Attribute the subagent's spend to when it last ran, so the burn
        // charts do not lose it. ponytail: one point per subagent, not per
        // message — a per-minute curve only if a subagent's timeline matters.
        if let Some(dt) = util::parse_ts(&util::ms_to_rfc3339(updated)) {
            data.record_cost(&dt, &model, cost);
        }

        let tool_count_sql = match schema {
            Schema::V1 => {
                "SELECT count(*) FROM part WHERE session_id = ?1 \
                 AND json_extract(data, '$.type') = 'tool'"
            }
            Schema::V2 => {
                // `json_valid` guards `json_each`, which errors on a malformed row.
                "SELECT count(*) FROM session_message m, json_each(m.data, '$.content') c \
                 WHERE m.session_id = ?1 AND json_valid(m.data) \
                 AND json_extract(c.value, '$.type') = 'tool'"
            }
        };
        let tool_count = db
            .query_row(tool_count_sql, params![id], |row| row.get::<_, u64>(0))
            .unwrap_or(0);

        data.subagents.push(Subagent {
            agent_id: id,
            agent_type: agent.unwrap_or_default(),
            description: title.unwrap_or_default(),
            model,
            started_at: Some(util::ms_to_rfc3339(created)),
            last_active: Some(util::ms_to_rfc3339(updated)),
            duration_ms: (updated - created).max(0),
            // A subagent still writing is one whose parent has not moved on; the
            // same quiet window the Claude parser uses tells the two apart.
            status: if now_ms - updated >= 30_000 {
                SubagentStatus::Done
            } else {
                SubagentStatus::Running
            },
            cost,
            tool_count,
            turns: 0,
            tokens: 0,
            last_text: None,
            tool_use_id: None,
            context: None,
            ghost: false,
        });
    }
}

pub fn delete(session: &Session) -> rusqlite::Result<()> {
    let Some(path) = &session.data_file else {
        return Ok(());
    };
    let db = Connection::open(path)?;
    db.execute_batch("PRAGMA foreign_keys = ON")?;
    db.execute(
        &format!("DELETE FROM {} WHERE id = ?1", session_table(schema(&db))),
        params![session.session_id],
    )?;
    Ok(())
}

/// Context usage from the newest assistant message that carries token counts.
///
/// OpenCode records no window size of its own, so the ceiling comes from
/// LiteLLM's listing for the model. Without one there is no denominator, and a
/// guessed window would misreport CTX% for every model that does not match it.
pub fn extract_context(session: &Session) -> Option<ContextUsage> {
    with_db(session.data_file.as_ref()?, |db| context_of(db, session)).flatten()
}

fn context_of(db: &Connection, session: &Session) -> Option<ContextUsage> {
    let schema = schema(db);
    // A turn that was aborted or failed before the provider answered is still
    // recorded, with every count at zero. Those sit at the end of many sessions,
    // so walk back past them rather than reading the newest row and giving up.
    let sql = match schema {
        Schema::V1 => {
            "SELECT time_created, data FROM message WHERE session_id = ?1 \
             AND json_extract(data, '$.role') = 'assistant' \
             ORDER BY time_created DESC, id DESC LIMIT 40"
        }
        Schema::V2 => {
            "SELECT time_created, data FROM session_message WHERE session_id = ?1 \
             AND type = 'assistant' ORDER BY seq DESC LIMIT 40"
        }
    };
    let mut stmt = db.prepare(sql).ok()?;
    let rows = stmt
        .query_map(params![session.session_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .ok()?;

    for (created, raw) in rows.flatten() {
        let Some(message) = opencode_line::parse::<Message>(&raw) else {
            continue;
        };
        // A compaction's summary turn measured the window being thrown away;
        // past it, the compaction part is newer than the measured turn and
        // the row says the window is stale until a real turn measures it.
        let used = message.window();
        if used == 0 || message.is_summary() {
            continue;
        }
        let model = message
            .model_name()
            .filter(|m| !m.is_empty())
            .unwrap_or(&session.model);
        return Some(ContextUsage {
            used,
            max: crate::pricing::litellm_max_input_tokens(model)?,
            compacted: compacted_since(db, schema, &session.session_id, created),
        });
    }
    None
}

/// Whether a compaction was written after the turn that last measured the
/// window, which then describes the window as it stood before it.
fn compacted_since(db: &Connection, schema: Schema, session_id: &str, created: i64) -> bool {
    let sql = match schema {
        Schema::V1 => {
            "SELECT 1 FROM part WHERE session_id = ?1 AND time_created > ?2 \
             AND json_extract(data, '$.type') = 'compaction' LIMIT 1"
        }
        Schema::V2 => {
            "SELECT 1 FROM session_message WHERE session_id = ?1 AND time_created > ?2 \
             AND type = 'compaction' LIMIT 1"
        }
    };
    db.query_row(sql, params![session_id, created], |_| Ok(()))
        .is_ok()
}

pub fn extract_last_tool(session: &Session) -> String {
    let Some(path) = &session.data_file else {
        return String::new();
    };
    with_db(path, |db| last_tool(db, &session.session_id)).unwrap_or_default()
}

fn last_tool(db: &Connection, session_id: &str) -> String {
    if schema(db) == Schema::V2 {
        // The tools are inline in the assistant content; take the newest
        // message that holds one and the last tool item within it.
        let raw: Option<String> = db
            .query_row(
                "SELECT data FROM session_message WHERE session_id = ?1 AND type = 'assistant' \
                 AND data LIKE '%\"type\":\"tool\"%' ORDER BY seq DESC LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten();
        return raw
            .map(|raw| content_items(&raw))
            .and_then(|items| {
                items
                    .iter()
                    .rev()
                    .find(|i| i.get("type").and_then(Value::as_str) == Some("tool"))
                    .and_then(|i| i.get("name").and_then(Value::as_str))
                    .map(str::to_string)
            })
            .unwrap_or_default();
    }
    db.query_row(
        "SELECT json_extract(data, '$.tool') FROM part \
         WHERE session_id = ?1 AND json_extract(data, '$.type') = 'tool' \
         ORDER BY time_created DESC, id DESC LIMIT 1",
        params![session_id],
        |row| row.get(0),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cctop-opencode-{tag}-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn extracts_sqlite_accounting_and_tool_activity() {
        let path = std::env::temp_dir().join(format!("cctop-opencode-{}.db", std::process::id()));
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, model TEXT, cost REAL NOT NULL,
                tokens_input INTEGER NOT NULL, tokens_output INTEGER NOT NULL,
                tokens_reasoning INTEGER NOT NULL, tokens_cache_read INTEGER NOT NULL,
                tokens_cache_write INTEGER NOT NULL
             );
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                "ses_1",
                "OpenCode work",
                r#"{"id":"claude-test","providerID":"anthropic"}"#,
                0.75,
                120u64,
                30u64,
                5u64,
                80u64,
                10u64
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
            params![
                "part_2",
                "ses_1",
                1_785_888_062_000i64,
                r#"{"type":"tool","tool":"edit","callID":"call_2","state":{"input":{"filePath":"src/main.rs","oldString":"old\nline","newString":"new\nlines\nhere"},"time":{"start":1785888062000,"end":1785888062750},"status":"completed","output":"Updated file"}}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_1",
                "ses_1",
                1_785_888_060_000i64,
                r#"{"role":"assistant","time":{"created":1785888060000},"modelID":"claude-test","providerID":"anthropic","cost":0.75,"tokens":{"input":120,"output":30,"reasoning":5,"cache":{"read":80,"write":10}}}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
            params![
                "part_1",
                "ses_1",
                1_785_888_061_000i64,
                r#"{"type":"tool","tool":"bash","callID":"call_1","state":{"input":{"command":"pwd"},"time":{"start":1785888061000},"status":"completed"}}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
            params![
                "part_3",
                "ses_1",
                1_785_888_063_000i64,
                r#"{"type":"tool","tool":"bash","callID":"call_3","state":{"input":{"command":"exit 1"},"time":{"start":1785888063000},"status":"error","output":"exit status 1"}}"#
            ],
        )
        .unwrap();
        drop(db);

        let data = extract(&path, "ses_1");
        assert_eq!(data.title.as_deref(), Some("OpenCode work"));
        assert_eq!(data.last_model, "claude-test");
        assert_eq!(data.tokens.total, 240);
        assert!((data.costs.total - 0.75).abs() < 1e-9);
        // Two bash calls: one that completed and one that errored.
        assert_eq!(data.metrics.tools.get("bash"), Some(&2));
        let edit = &data.metrics.tool_details["edit"][0];
        assert_eq!(edit.d, "src/main.rs");
        assert_eq!(edit.dur_ms, Some(750));
        let delta = edit.delta.as_ref().unwrap();
        assert_eq!((delta.added, delta.removed), (3, 2));
        assert!(!edit.failed);

        // `status: "error"` is the outcome OpenCode records for a failed call.
        let bash = &data.metrics.tool_details["bash"];
        assert_eq!(bash.len(), 2);
        assert!(!bash[0].failed, "a completed call must not be flagged");
        assert!(bash[1].failed, "status=error must be flagged");
        assert_eq!(data.metrics.tool_errors, 1);

        // Release the handle this thread holds before removing the file.
        close_databases();
        std::fs::remove_file(path).unwrap();
    }

    /// Build an OpenCode 2 database: `session_v2` plus `session_message`, whose
    /// parts live inline in each message's `data.content[]`.
    fn open_v2(path: &Path) -> Connection {
        let db = Connection::open(path).unwrap();
        db.execute_batch(
            "CREATE TABLE session_v2 (
                id TEXT PRIMARY KEY, parent_id TEXT, directory TEXT NOT NULL, title TEXT,
                model TEXT, cost REAL NOT NULL DEFAULT 0, tokens_input INTEGER NOT NULL DEFAULT 0,
                tokens_output INTEGER NOT NULL DEFAULT 0, tokens_reasoning INTEGER NOT NULL DEFAULT 0,
                tokens_cache_read INTEGER NOT NULL DEFAULT 0, tokens_cache_write INTEGER NOT NULL DEFAULT 0,
                time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, agent TEXT
             );
             CREATE TABLE session_message (
                id TEXT PRIMARY KEY, session_id TEXT NOT NULL, type TEXT NOT NULL,
                seq INTEGER NOT NULL, time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL,
                data TEXT NOT NULL
             );",
        )
        .unwrap();
        db
    }

    /// An OpenCode 2 database reads as the one earlier versions wrote: the
    /// sessions list, the accounting, the inline tools, the compaction count and
    /// the conversation all come from `session_v2`/`session_message`.
    #[test]
    fn opencode_two_reads_sessions_tools_and_conversation() {
        let path = temp_db("v2");
        let db = open_v2(&path);
        db.execute(
            "INSERT INTO session_v2 (id, directory, title, model, time_created, time_updated) \
             VALUES ('ses_2', '/work', 'v2 work', '{\"id\":\"claude-test\"}', 1000, 2000)",
            [],
        )
        .unwrap();
        // One assistant turn carrying its text and three tool calls inline.
        let assistant = r#"{"time":{"created":1100,"completed":1900},"model":{"id":"claude-test","variant":"high"},"cost":0.75,"tokens":{"input":120,"output":30,"reasoning":5,"cache":{"read":80,"write":10}},"content":[
            {"type":"text","text":"on it"},
            {"type":"tool","id":"c1","name":"bash","state":{"status":"completed","input":{"command":"pwd"}},"time":{"created":1100,"completed":1200}},
            {"type":"tool","id":"c2","name":"bash","state":{"status":"error","input":{"command":"exit 1"},"error":{"type":"tool.execution","message":"boom"}},"time":{"created":1200,"completed":1300}},
            {"type":"tool","id":"c3","name":"edit","state":{"status":"completed","input":{"filePath":"src/main.rs","oldString":"old\nline","newString":"new\nlines\nhere"}},"time":{"created":1300,"completed":1450}}
        ]}"#;
        let rows = [
            ("m0", "user", 0i64, 1050i64, r#"{"text":"do the thing"}"#),
            ("m1", "assistant", 1, 1100, assistant),
            // A compaction turn: its spend counts, but it is not the last model.
            (
                "m2",
                "compaction",
                2,
                1950,
                r#"{"model":{"id":"summariser"},"cost":0.1,"tokens":{"input":100,"output":0}}"#,
            ),
        ];
        for (id, kind, seq, created, data) in rows {
            db.execute(
                "INSERT INTO session_message (id, session_id, type, seq, time_created, time_updated, data) \
                 VALUES (?1, 'ses_2', ?2, ?3, ?4, ?4, ?5)",
                params![id, kind, seq, created, data],
            )
            .unwrap();
        }
        drop(db);

        let data = extract(&path, "ses_2");
        assert_eq!(data.title.as_deref(), Some("v2 work"));
        // The compaction's model does not become the conversation's.
        assert_eq!(data.last_model, "claude-test");
        assert_eq!(data.reasoning_effort.as_deref(), Some("high"));
        // Assistant 240 + compaction 100 input.
        assert_eq!(data.tokens.total, 340);
        assert!((data.costs.total - 0.85).abs() < 1e-9);
        assert_eq!(data.compactions, 1);
        assert_eq!(data.metrics.tools.get("bash"), Some(&2));
        assert_eq!(data.metrics.tool_errors, 1);
        let edit = &data.metrics.tool_details["edit"][0];
        assert_eq!(edit.d, "src/main.rs");
        assert_eq!(edit.dur_ms, Some(150));
        let delta = edit.delta.as_ref().unwrap();
        assert_eq!((delta.added, delta.removed), (3, 2));

        // Listed from `session_v2`, with its last-activity stamp.
        let listed = with_db(&path, |db| sessions_in(db, &path)).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, "ses_2");
        assert_eq!(listed[0].last_active, util::ms_to_rfc3339(1950));

        // The newest tool, read out of the inline content.
        assert_eq!(extract_last_tool(&listed[0]), "edit");

        // The conversation reads the user's words and the assistant's calls.
        let mut roles = Vec::new();
        for_each_message(&path, "ses_2", |message, _, parts| {
            roles.push(
                message
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            );
            if message.get("role").and_then(Value::as_str) == Some("user") {
                assert_eq!(parts[0]["text"], "do the thing");
            }
            if message.get("role").and_then(Value::as_str) == Some("assistant") {
                let tools: Vec<_> = parts
                    .iter()
                    .filter(|p| p["type"] == "tool")
                    .map(|p| p["tool"].as_str().unwrap().to_string())
                    .collect();
                assert_eq!(tools, ["bash", "bash", "edit"]);
            }
        });
        assert_eq!(roles, ["user", "assistant", "system"]);

        close_databases();
        std::fs::remove_file(path).unwrap();
    }

    /// A subagent is a session of its own with `parent_id` set. Its spend must
    /// count towards the parent it ran under, and it must be listed under it
    /// rather than folded away or left standing alone as a conversation.
    #[test]
    fn subagents_fold_into_their_parent() {
        let path = temp_db("subagents");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY, parent_id TEXT, agent TEXT, directory TEXT NOT NULL,
                title TEXT NOT NULL, model TEXT, cost REAL NOT NULL,
                tokens_input INTEGER NOT NULL, tokens_output INTEGER NOT NULL,
                tokens_reasoning INTEGER NOT NULL, tokens_cache_read INTEGER NOT NULL,
                tokens_cache_write INTEGER NOT NULL,
                time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL
             );
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();
        let cols = "id,parent_id,agent,directory,title,model,cost,tokens_input,tokens_output,\
                    tokens_reasoning,tokens_cache_read,tokens_cache_write,time_created,time_updated";
        db.execute(
            &format!("INSERT INTO session ({cols}) VALUES ('par',NULL,NULL,'/w','parent',?1,0.10,100,20,0,0,0,1000,2000)"),
            params![r#"{"id":"claude-x"}"#],
        )
        .unwrap();
        db.execute(
            &format!("INSERT INTO session ({cols}) VALUES ('kid','par','general','/w','find the bug',?1,0.05,200,40,0,0,0,1500,1800)"),
            params![r#"{"id":"claude-x"}"#],
        )
        .unwrap();
        // A tool call the subagent made, so its tool count is not zero.
        db.execute(
            "INSERT INTO part VALUES ('prt','kid',1600,?1)",
            params![r#"{"type":"tool","tool":"bash"}"#],
        )
        .unwrap();
        drop(db);

        let data = extract(&path, "par");
        assert_eq!(
            data.subagents.len(),
            1,
            "the child is listed under the parent"
        );
        let sa = &data.subagents[0];
        assert_eq!(sa.agent_type, "general");
        assert_eq!(sa.description, "find the bug");
        assert_eq!(sa.tool_count, 1);
        assert!((sa.cost - 0.05).abs() < 1e-9);
        // The parent's totals now carry the subagent's tokens and spend.
        assert_eq!(data.tokens.input, 300);
        assert!((data.costs.total - 0.15).abs() < 1e-9);

        close_databases();
        std::fs::remove_file(path).unwrap();
    }

    /// A subagent row with no model is named by its own messages, not left
    /// blank: a blank name was charted as a model of its own and priced as
    /// whichever listing it happened to match.
    #[test]
    fn a_subagent_with_no_model_is_named_by_its_messages() {
        let path = temp_db("subagent-model");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY, parent_id TEXT, agent TEXT, directory TEXT NOT NULL,
                title TEXT NOT NULL, model TEXT, cost REAL NOT NULL,
                tokens_input INTEGER NOT NULL, tokens_output INTEGER NOT NULL,
                tokens_reasoning INTEGER NOT NULL, tokens_cache_read INTEGER NOT NULL,
                tokens_cache_write INTEGER NOT NULL,
                time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL
             );
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();
        let cols = "id,parent_id,agent,directory,title,model,cost,tokens_input,tokens_output,\
                    tokens_reasoning,tokens_cache_read,tokens_cache_write,time_created,time_updated";
        db.execute(
            &format!("INSERT INTO session ({cols}) VALUES ('par',NULL,NULL,'/w','parent',?1,0.10,100,20,0,0,0,1000,2000)"),
            params![r#"{"id":"claude-x"}"#],
        )
        .unwrap();
        db.execute(
            &format!("INSERT INTO session ({cols}) VALUES ('kid','par','general','/w','look',NULL,0.05,200,40,0,0,0,1500,1800)"),
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO message VALUES ('m1','kid',1600,?1)",
            params![r#"{"role":"assistant","modelID":"kimi-k2.6"}"#],
        )
        .unwrap();
        drop(db);

        let data = extract(&path, "par");
        assert_eq!(data.subagents[0].model, "kimi-k2.6");
        assert!(
            data.costs_by_hour
                .values()
                .all(|by_model| !by_model.contains_key("")),
            "no spend is charted under a blank model"
        );

        close_databases();
        std::fs::remove_file(path).unwrap();
    }

    /// Every other read of this database shares the pool's read-only handle,
    /// which is then open for the life of the thread. `delete` opens a read-write
    /// one of its own on top of it, and the two have to coexist: a rollback
    /// journal grants a writer nothing while a reader holds the database, so a
    /// handle that outlived its usefulness would turn a deletion into a failure
    /// and leave the row the user asked to remove on screen.
    ///
    /// Read again afterwards through the same pooled handle, which is the other
    /// half of it — a handle kept open across a write must not answer with the
    /// row that has just gone.
    #[test]
    fn a_delete_lands_under_the_handle_the_pool_is_holding() {
        let path = temp_db("delete");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            // Rollback rather than WAL on purpose: WAL lets a writer past a
            // reader, so only this mode can fail for the reason above.
            "PRAGMA journal_mode = DELETE;
             CREATE TABLE session (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, model TEXT, cost REAL NOT NULL,
                tokens_input INTEGER NOT NULL, tokens_output INTEGER NOT NULL,
                tokens_reasoning INTEGER NOT NULL, tokens_cache_read INTEGER NOT NULL,
                tokens_cache_write INTEGER NOT NULL
             );
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT,
                FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE);
             CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT,
                FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                "ses_doomed",
                "Doomed",
                r#"{"id":"claude-test","providerID":"anthropic"}"#,
                0.5,
                120u64,
                30u64,
                5u64,
                80u64,
                10u64
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_1",
                "ses_doomed",
                1_785_888_060_000i64,
                r#"{"role":"assistant","time":{"created":1785888060000},"modelID":"claude-test","cost":0.5,"tokens":{"input":120,"output":30,"reasoning":5,"cache":{"read":80,"write":10}}}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
            params![
                "part_1",
                "ses_doomed",
                1_785_888_061_000i64,
                r#"{"type":"tool","tool":"bash","callID":"call_1","state":{"input":{"command":"ls"},"time":{"start":1785888061000},"status":"completed"}}"#
            ],
        )
        .unwrap();
        drop(db);

        let mut session = Session::new(Provider::OpenCode, "ses_doomed".to_string());
        session.data_file = Some(path.clone());

        // Open the pooled handle first, the way a walk has before anyone deletes.
        let before = extract(&path, "ses_doomed");
        assert_eq!(before.tokens.total, 240);
        assert_eq!(before.metrics.tool_count, 1);

        delete(&session).expect("a pooled reader must not lock the writer out");

        // The cascade is what takes the spend with it, so this also pins that
        // `foreign_keys` is on for the deleting connection — without it the
        // session's own row would go and its messages would stay behind,
        // still adding up.
        let after = extract(&path, "ses_doomed");
        assert_eq!(after.title, None, "a deleted session must not come back");
        assert_eq!(after.tokens.total, 0, "and neither must its spend");
        assert_eq!(after.metrics.tool_count, 0);

        close_databases();
        std::fs::remove_file(path).unwrap();
    }

    /// A custom provider (any OpenAI-compatible endpoint OpenCode has no rates
    /// for) reports `cost: 0` on every message. Reporting those sessions as free
    /// hid real spend, so the tokens are priced against LiteLLM instead — and the
    /// route prefix in the model name must not stop that lookup from landing.
    #[test]
    fn a_custom_provider_reporting_no_cost_is_priced_from_litellm() {
        let _rates = crate::pricing::install_test_table(&[(
            "zai/glm-5.1",
            serde_json::json!({
                "input_cost_per_token": 1.4e-6,
                "output_cost_per_token": 4.4e-6,
                "cache_read_input_token_cost": 2.6e-7,
                "cache_creation_input_token_cost": 0.0,
                "max_input_tokens": 200_000u64,
            }),
        )]);
        let path = temp_db("priced");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, model TEXT, cost REAL NOT NULL,
                tokens_input INTEGER NOT NULL, tokens_output INTEGER NOT NULL,
                tokens_reasoning INTEGER NOT NULL, tokens_cache_read INTEGER NOT NULL,
                tokens_cache_write INTEGER NOT NULL
             );
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO session VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                "ses_1",
                "proxied",
                r#"{"id":"canopywave/zai/glm-5.1","providerID":"myproxy"}"#,
                0.0,
                0u64,
                0u64,
                0u64,
                0u64,
                0u64
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_1",
                "ses_1",
                1_785_888_060_000i64,
                r#"{"role":"assistant","time":{"created":1785888060000},"modelID":"canopywave/zai/glm-5.1","providerID":"myproxy","cost":0,"tokens":{"input":1000000,"output":1000000,"reasoning":500000,"cache":{"read":1000000,"write":1000000}}}"#
            ],
        )
        .unwrap();
        drop(db);

        let data = extract(&path, "ses_1");
        // 1.40 input + 4.40 output + 0.26 cache read + 0.00 cache write per Mtok.
        // Reasoning is part of output, not billed on top of it.
        assert!(
            (data.costs.total - 6.06).abs() < 1e-9,
            "priced at {}",
            data.costs.total
        );

        let mut session = Session::new(Provider::OpenCode, "ses_1".to_string());
        session.data_file = Some(path.clone());
        let ctx = extract_context(&session).expect("context from the LiteLLM window");
        assert_eq!(ctx.used, 3_000_000);
        assert_eq!(ctx.max, 200_000);

        // Release the handle this thread holds before removing the file.
        close_databases();
        std::fs::remove_file(path).unwrap();
    }

    /// A cost OpenCode did compute is authoritative — pricing over the top of it
    /// would replace a provider's real invoice with a list price.
    #[test]
    fn a_reported_cost_is_never_second_guessed() {
        let _rates = crate::pricing::install_test_table(&[(
            "zai/glm-5.1",
            serde_json::json!({"input_cost_per_token": 1.4e-6, "output_cost_per_token": 4.4e-6}),
        )]);
        let mut rates = FallbackRates::default();
        let message = opencode_line::parse::<Message>(
            r#"{"role":"assistant","modelID":"canopywave/zai/glm-5.1","cost":0.5,
                "tokens":{"input":1000000,"output":0,"cache":{"read":0,"write":0}}}"#,
        )
        .unwrap();
        let (_, _, _, costs) = assistant_usage(&message, &mut rates);
        assert!((costs.total - 0.5).abs() < 1e-9);
    }

    /// A model LiteLLM does not list has no window, and a guessed one would make
    /// every CTX% wrong in a way the user cannot see.
    #[test]
    fn an_unlisted_model_reports_no_context_rather_than_a_guess() {
        let _rates = crate::pricing::install_test_table(&[]);
        let path = temp_db("noctx");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_1",
                "ses_1",
                1i64,
                r#"{"role":"assistant","modelID":"local/some-finetune","tokens":{"input":500,"output":10,"cache":{"read":0,"write":0}}}"#
            ],
        )
        .unwrap();
        drop(db);

        let mut session = Session::new(Provider::OpenCode, "ses_1".to_string());
        session.data_file = Some(path.clone());
        assert!(extract_context(&session).is_none());

        // Release the handle this thread holds before removing the file.
        close_databases();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn input_request_waits_and_api_timeout_is_red() {
        let path = std::env::temp_dir().join(format!(
            "cctop-opencode-state-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, data TEXT);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3)",
            params![
                "part_question",
                "msg_done",
                r#"{"type":"tool","tool":"question"}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_done",
                "ses_done",
                1i64,
                r#"{"role":"assistant","time":{"completed":1},"error":{"name":"MessageAbortedError"}}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_timeout",
                "ses_timeout",
                1i64,
                r#"{"role":"assistant","time":{"completed":1},"error":{"name":"APIError","data":{"message":"Request timed out"}}}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_queue_full",
                "ses_queue_full",
                1i64,
                r#"{"role":"assistant","time":{"completed":1},"error":{"name":"APIError","data":{"message":"Streaming response failed: [503] The request queue is full."}}}"#
            ],
        )
        .unwrap();
        drop(db);

        assert_eq!(
            extract_activity_state(&path, "ses_done"),
            ActivityState::WaitingForInput
        );
        assert_eq!(
            extract_activity_state(&path, "ses_timeout"),
            ActivityState::ApiError
        );
        assert_eq!(
            extract_activity_state(&path, "ses_queue_full"),
            ActivityState::ApiError
        );
        // Release the handle this thread holds before removing the file.
        close_databases();
        std::fs::remove_file(path).unwrap();
    }

    /// Search reads both tables, because the text of a message is in `part` and
    /// its envelope and tool calls are in `message` — a term naming either has
    /// to find the session. One database holds every session, so the scan is
    /// also the only thing keeping another session's text out of a hit.
    #[test]
    fn a_scan_reads_both_tables_and_stops_where_it_is_told_to() {
        let path = temp_db("scan");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE part (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();
        db.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_1",
                "ses_mine",
                1i64,
                r#"{"role":"assistant","modelID":"claude-test"}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
            params![
                "part_1",
                "ses_mine",
                1i64,
                r#"{"type":"text","text":"the needle is in a part"}"#
            ],
        )
        .unwrap();
        // Another session's row, in both tables.
        db.execute(
            "INSERT INTO message VALUES (?1, ?2, ?3, ?4)",
            params![
                "msg_2",
                "ses_theirs",
                1i64,
                r#"{"role":"assistant","modelID":"claude-test"}"#
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO part VALUES (?1, ?2, ?3, ?4)",
            params![
                "part_2",
                "ses_theirs",
                1i64,
                r#"{"type":"text","text":"the needle is not ours"}"#
            ],
        )
        .unwrap();
        drop(db);

        let collect = |session_id: &str| {
            let mut seen = Vec::new();
            let found = scan_session(&path, session_id, |raw| {
                seen.push(raw.to_string());
                false
            });
            (found, seen)
        };

        let (found, seen) = collect("ses_mine");
        assert!(!found, "the feed said no, so the scan must read it all");
        assert_eq!(seen.len(), 2, "a message and a part, not one of them");
        assert!(seen.iter().any(|raw| raw.contains("in a part")));
        assert!(
            !seen.iter().any(|raw| raw.contains("not ours")),
            "another session's text must not reach this hit"
        );

        // The envelope carries no needle of its own here, so a term named in the
        // message alone is the other half of what makes this worth scanning both.
        assert!(scan_session(&path, "ses_mine", |raw| raw.contains("modelID")));
        assert!(
            !scan_session(&path, "ses_mine", |raw| raw.contains("absent")),
            "a term nothing holds must not"
        );

        close_databases();
        std::fs::remove_file(path).unwrap();
    }

    /// The three tables as a current OpenCode creates them, trimmed to the
    /// columns cctop reads. Hand-built from the schema, with made-up rows.
    fn current_schema(path: &Path) -> Connection {
        let db = Connection::open(path).unwrap();
        db.execute_batch(
            "CREATE TABLE session (
                id TEXT PRIMARY KEY, parent_id TEXT, agent TEXT, directory TEXT NOT NULL,
                title TEXT NOT NULL, model TEXT, cost REAL NOT NULL DEFAULT 0,
                tokens_input INTEGER NOT NULL DEFAULT 0, tokens_output INTEGER NOT NULL DEFAULT 0,
                tokens_reasoning INTEGER NOT NULL DEFAULT 0,
                tokens_cache_read INTEGER NOT NULL DEFAULT 0,
                tokens_cache_write INTEGER NOT NULL DEFAULT 0,
                time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL
             );
             CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);
             CREATE INDEX message_session_time_created_id_idx ON message (session_id, time_created, id);
             CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL,
                time_created INTEGER NOT NULL, time_updated INTEGER NOT NULL, data TEXT NOT NULL);
             CREATE INDEX part_message_id_id_idx ON part (message_id, id);",
        )
        .unwrap();
        db
    }

    fn part(db: &Connection, id: &str, message: &str, at: i64, data: &str) {
        db.execute(
            "INSERT INTO part VALUES (?1, ?2, 'ses_t', ?3, ?3, ?4)",
            params![id, message, at, data],
        )
        .unwrap();
    }

    #[test]
    fn typed_rows_fill_effort_lines_web_skills_and_compactions() {
        let path = temp_db("typed");
        let db = current_schema(&path);
        db.execute(
            "INSERT INTO session (id, directory, title, time_created, time_updated) \
             VALUES ('ses_t', '/w', 't', 1000, 1000)",
            [],
        )
        .unwrap();
        for (id, at, data) in [
            (
                "msg_1",
                2000,
                r#"{"role":"user","time":{"created":2000},"agent":"build"}"#,
            ),
            (
                "msg_2",
                3000,
                r#"{"role":"assistant","modelID":"m","providerID":"p","variant":"high","cost":0.1,"time":{"created":3000,"completed":7000},"tokens":{"input":10,"output":2,"reasoning":0,"cache":{"read":0,"write":0}},"finish":"tool-calls"}"#,
            ),
            (
                "msg_3",
                8000,
                r#"{"role":"assistant","modelID":"m","variant":"medium","cost":0.1,"time":{"created":8000,"completed":9000},"tokens":{"input":10,"output":2,"cache":{"read":5,"write":0}},"error":{"name":"MessageAbortedError","data":{}}}"#,
            ),
        ] {
            db.execute(
                "INSERT INTO message VALUES (?1, 'ses_t', ?2, ?2, ?3)",
                params![id, at, data],
            )
            .unwrap();
        }
        part(
            &db,
            "p1",
            "msg_2",
            3100,
            r#"{"type":"step-start","snapshot":"x"}"#,
        );
        part(
            &db,
            "p2",
            "msg_2",
            3200,
            r#"{"type":"tool","tool":"edit","callID":"c1","state":{"status":"completed","input":{"filePath":"a.rs","oldString":"a","newString":"b\nc"},"time":{"start":3200,"end":3300}}}"#,
        );
        part(
            &db,
            "p3",
            "msg_2",
            3400,
            r#"{"type":"tool","tool":"webfetch","callID":"c2","state":{"status":"completed","input":{"url":"https://example.invalid/doc","format":"markdown"},"time":{"start":3400,"end":3500}}}"#,
        );
        part(
            &db,
            "p4",
            "msg_2",
            3600,
            r#"{"type":"tool","tool":"skill","callID":"c3","state":{"status":"completed","input":{"name":"run"},"time":{"start":3600,"end":3700}}}"#,
        );
        part(
            &db,
            "p5",
            "msg_2",
            3800,
            r#"{"type":"tool","tool":"invalid","callID":"c4","state":{"status":"completed","input":{"tool":"nope","error":"unknown"},"time":{"start":3800,"end":3810}}}"#,
        );
        part(
            &db,
            "p6",
            "msg_3",
            8100,
            r#"{"type":"compaction","auto":true}"#,
        );
        part(
            &db,
            "p7",
            "msg_3",
            8200,
            r#"{"type":"some-future-part","x":[1]}"#,
        );
        part(&db, "p8", "msg_3", 8300, "not json");
        drop(db);

        let data = extract(&path, "ses_t");
        let m = &data.metrics;
        assert_eq!(
            data.reasoning_effort.as_deref(),
            Some("medium"),
            "newest turn's"
        );
        assert_eq!(data.compactions, 1);
        assert_eq!(m.api_duration_ms, 5000);
        assert_eq!((m.lines_added, m.lines_removed), (2, 1));
        assert_eq!(m.web_fetches, ["https://example.invalid/doc"]);
        assert_eq!(m.web_fetch_count, 1);
        assert_eq!(m.skills.get("run"), Some(&1));
        assert_eq!(m.tool_count, 4);
        assert_eq!(m.tool_errors, 1, "an invalid call is one that never ran");
        assert_eq!(data.tokens.total, 29);

        close_databases();
        std::fs::remove_file(path).unwrap();
    }

    /// The row's `time_updated` trails real activity; the listing reads the
    /// newest message and its parts instead.
    #[test]
    fn last_activity_is_the_newest_message_or_part() {
        let path = temp_db("last");
        let db = current_schema(&path);
        db.execute_batch(
            "INSERT INTO session (id, directory, title, time_created, time_updated) VALUES
                ('ses_t', '/w', 't', 1000, 1500),
                ('ses_empty', '/w', 'e', 1000, 1200);
             INSERT INTO message VALUES ('m1', 'ses_t', 2000, 2500, '{}');
             INSERT INTO message VALUES ('m2', 'ses_t', 3000, 3100, '{}');
             -- A prompt queued while m2's reply was still streaming.
             INSERT INTO message VALUES ('m3', 'ses_t', 4000, 4000, '{}');",
        )
        .unwrap();
        part(&db, "p1", "m2", 3000, "{}");
        db.execute("UPDATE part SET time_updated = 9000 WHERE id = 'p1'", [])
            .unwrap();
        let sessions = sessions_in(&db, &path);
        drop(db);

        let at = |id: &str| {
            sessions
                .iter()
                .find(|s| s.session_id == id)
                .map(|s| s.last_active.clone())
                .unwrap()
        };
        assert_eq!(
            at("ses_t"),
            util::ms_to_rfc3339(9000),
            "the part that finished last"
        );
        assert_eq!(
            at("ses_empty"),
            util::ms_to_rfc3339(1200),
            "no messages: the row's own"
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_compaction_after_the_measured_turn_marks_the_window_stale() {
        let _rates = crate::pricing::install_test_table(&[(
            "m",
            serde_json::json!({
                "input_cost_per_token": 1e-6,
                "output_cost_per_token": 1e-6,
                "max_input_tokens": 100,
            }),
        )]);
        let path = temp_db("compacted");
        let db = current_schema(&path);
        db.execute(
            "INSERT INTO message VALUES ('m1', 'ses_t', 2000, 2000, ?1)",
            params![r#"{"role":"assistant","modelID":"m","tokens":{"input":30,"cache":{"read":20,"write":5}}}"#],
        )
        .unwrap();
        let mut session = Session::new(Provider::OpenCode, "ses_t".into());
        session.data_file = Some(path.clone());
        let ctx = context_of(&db, &session).unwrap();
        assert_eq!((ctx.used, ctx.max, ctx.compacted), (55, 100, false));

        part(
            &db,
            "p1",
            "m1",
            2500,
            r#"{"type":"compaction","auto":false}"#,
        );
        assert!(context_of(&db, &session).unwrap().compacted);

        // The summariser's turn, written just after the compaction part with
        // the old window as its input, is not the window the session goes on
        // with: the measure stays the last real turn's, marked stale.
        db.execute(
            "INSERT INTO message VALUES ('m2', 'ses_t', 2567, 2567, ?1)",
            params![r#"{"role":"assistant","modelID":"m","summary":true,"mode":"compaction","tokens":{"input":90}}"#],
        )
        .unwrap();
        let ctx = context_of(&db, &session).unwrap();
        assert_eq!((ctx.used, ctx.compacted), (55, true));
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}
