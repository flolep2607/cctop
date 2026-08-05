//! OpenCode session discovery and extraction from its current SQLite store.

use super::extract;
use super::{ActivityState, Costs, ModelBreakdown, Session, SessionData, Tokens};
use crate::config;
use crate::pricing::Provider;
use crate::util;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Exact line deltas from OpenCode's built-in editing tools.  `write` can
/// replace an existing file wholesale, so its removed lines cannot be known
/// from the recorded input and are deliberately not guessed.
fn tool_delta(name: &str, state: &Value) -> Option<super::Delta> {
    if state.get("status").and_then(Value::as_str) != Some("completed") {
        return None;
    }
    let input = state.get("input")?;
    match name {
        "edit" => {
            let removed = input
                .get("oldString")
                .or_else(|| input.get("old_string"))
                .and_then(Value::as_str)?
                .lines()
                .count() as u32;
            let added = input
                .get("newString")
                .or_else(|| input.get("new_string"))
                .and_then(Value::as_str)?
                .lines()
                .count() as u32;
            Some(super::Delta {
                added,
                removed,
                ..Default::default()
            })
        }
        "apply_patch" => input
            .get("patch")
            .and_then(Value::as_str)
            .map(|patch| extract::parse_apply_patch(patch).1),
        _ => None,
    }
}

fn tool_duration_ms(state: &Value) -> Option<i64> {
    let time = state.get("time")?;
    let start = time.get("start")?.as_i64()?;
    let end = time.get("end")?.as_i64()?;
    Some((end - start).max(0))
}

fn readonly(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

/// State of the newest OpenCode message. OpenCode keeps events in SQLite rather
/// than a transcript file, but its assistant messages record both completion
/// and provider errors.
pub fn extract_activity_state(path: &Path, session_id: &str) -> ActivityState {
    let Ok(db) = readonly(path) else {
        return ActivityState::Working;
    };
    let message = db
        .query_row(
            "SELECT id, data FROM message WHERE session_id = ?1 ORDER BY time_created DESC, id DESC LIMIT 1",
            params![session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .ok()
        .flatten();
    let Some((message_id, raw)) = message else {
        return ActivityState::Working;
    };
    let Ok(message) = serde_json::from_str::<Value>(&raw) else {
        return ActivityState::Working;
    };
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return ActivityState::Working;
    }
    if is_api_failure(&message) {
        ActivityState::ApiError
    } else if has_input_request(&db, &message_id) {
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
        let Ok(part) = serde_json::from_str::<Value>(&raw) else {
            return false;
        };
        matches!(
            part.get("type").and_then(Value::as_str),
            Some("tool" | "toolCall")
        ) && part
            .get("tool")
            .or_else(|| part.get("name"))
            .and_then(Value::as_str)
            .is_some_and(super::is_input_request_tool)
    })
}

fn is_api_failure(message: &Value) -> bool {
    let Some(error) = message.get("error") else {
        return false;
    };
    let text = [
        error.get("name").and_then(Value::as_str),
        error
            .get("data")
            .and_then(|data| data.get("message"))
            .and_then(Value::as_str),
        error.get("message").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();
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
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

fn millis_rfc3339(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339())
        .unwrap_or_default()
}

/// Stable releases use `opencode.db`; nonstandard channels suffix the filename.
fn database_paths() -> Vec<PathBuf> {
    config::list_dir(&config::OPENCODE_DATA_DIR)
        .into_iter()
        .filter(|name| name.starts_with("opencode") && name.ends_with(".db"))
        .map(|name| config::OPENCODE_DATA_DIR.join(name))
        .collect()
}

/// Discover sessions from every OpenCode channel database, preferring the most
/// recently updated copy if the same session was migrated between channels.
pub fn list_sessions() -> Vec<Session> {
    let mut sessions = Vec::new();
    for path in database_paths() {
        let Ok(db) = readonly(&path) else { continue };
        let Ok(mut stmt) = db
            .prepare("SELECT id, directory, title, model, time_created, time_updated FROM session")
        else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        }) else {
            continue;
        };
        for row in rows.flatten() {
            let (id, directory, title, model_json, created, updated) = row;
            let model = model_json
                .as_deref()
                .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_default();
            let mut session = Session::new(Provider::OpenCode, id);
            session.started_at = millis_rfc3339(created);
            session.last_active = millis_rfc3339(updated);
            session.label_source = directory;
            session.title = (!title.is_empty()).then_some(title);
            session.model = model;
            session.data_file = Some(path.clone());
            sessions.push(session);
        }
    }

    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    let mut seen = HashSet::new();
    sessions.retain(|s| seen.insert(s.session_id.clone()));
    sessions
}

fn u64_at(value: &Value, path: &[&str]) -> u64 {
    let mut current = value;
    for key in path {
        current = current.get(*key).unwrap_or(&Value::Null);
    }
    current.as_u64().unwrap_or(0)
}

fn assistant_usage(value: &Value) -> (String, String, Tokens, Costs) {
    let model = value
        .get("modelID")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let ts = value
        .get("time")
        .and_then(|v| v.get("created"))
        .and_then(Value::as_i64)
        .map(millis_rfc3339)
        .unwrap_or_default();
    let input = u64_at(value, &["tokens", "input"]);
    let output = u64_at(value, &["tokens", "output"]);
    let cache_read = u64_at(value, &["tokens", "cache", "read"]);
    let cache_write = u64_at(value, &["tokens", "cache", "write"]);
    let reasoning = u64_at(value, &["tokens", "reasoning"]);
    let tokens = Tokens {
        input,
        output,
        cache_read,
        cache_write_5m: cache_write,
        reasoning_output: reasoning,
        total: input + output + cache_read + cache_write,
        ..Default::default()
    };
    let costs = Costs {
        total: value.get("cost").and_then(Value::as_f64).unwrap_or(0.0),
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

/// Extract accounting and tools for one OpenCode session. The database stores
/// the provider-reported cost on every assistant message, so no pricing lookup
/// or model-name guess is needed.
pub fn extract(path: &Path, session_id: &str) -> SessionData {
    let Ok(db) = readonly(path) else {
        return SessionData {
            error: Some(format!(
                "Could not open OpenCode database {}",
                path.display()
            )),
            ..Default::default()
        };
    };
    let mut data = SessionData::default();
    let mut breakdown: HashMap<String, (Tokens, Costs)> = HashMap::new();
    let mut saw_usage = false;

    let aggregate = db
        .query_row(
            "SELECT title, model, cost, tokens_input, tokens_output, tokens_reasoning, \
             tokens_cache_read, tokens_cache_write FROM session WHERE id = ?1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
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
        data.title = (!title.is_empty()).then(|| title.clone());
        data.last_model = model_json
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
            .and_then(|v| v.get("id").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
    }

    if let Ok(mut stmt) =
        db.prepare("SELECT data FROM message WHERE session_id = ?1 ORDER BY time_created, id")
        && let Ok(rows) = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))
    {
        for raw in rows.flatten() {
            let Ok(message) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            if message.get("role").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let (model, ts, tokens, costs) = assistant_usage(&message);
            saw_usage = true;
            data.last_model = model.clone();
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
                *data
                    .costs_by_day
                    .entry(util::local_date_key(&dt))
                    .or_default()
                    .entry(model.clone())
                    .or_insert(0.0) += costs.total;
                *data
                    .costs_by_hour
                    .entry(util::local_hour_key(&dt))
                    .or_default()
                    .entry(model)
                    .or_insert(0.0) += costs.total;
            }
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
        data.costs.total = cost;
    }

    if let Ok(mut stmt) =
        db.prepare("SELECT data FROM part WHERE session_id = ?1 ORDER BY time_created, id")
        && let Ok(rows) = stmt.query_map(params![session_id], |row| row.get::<_, String>(0))
    {
        for raw in rows.flatten() {
            let Ok(part) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            if part.get("type").and_then(Value::as_str) != Some("tool") {
                continue;
            }
            let name = part
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let state = part.get("state").unwrap_or(&Value::Null);
            let args = state.get("input").unwrap_or(&Value::Null);
            let (short, full) = extract::tool_detail(name, args);
            let delta = tool_delta(name, state);
            let duration_ms = tool_duration_ms(state);
            let ts = state
                .get("time")
                .and_then(|v| v.get("start"))
                .and_then(Value::as_i64)
                .map(millis_rfc3339)
                .unwrap_or_default();
            extract::push_tool_detail(
                &mut data.metrics.tool_details,
                name,
                short,
                full,
                ts,
                part.get("callID")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                None,
            );
            *data.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
            data.metrics.tool_count += 1;
            if let Some(detail) = data
                .metrics
                .tool_details
                .get_mut(name)
                .and_then(|details| details.last_mut())
            {
                detail.delta = delta;
                detail.dur_ms = duration_ms;
            }
        }
    }

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

pub fn delete(session: &Session) -> rusqlite::Result<()> {
    let Some(path) = &session.data_file else {
        return Ok(());
    };
    let db = Connection::open(path)?;
    db.execute_batch("PRAGMA foreign_keys = ON")?;
    db.execute(
        "DELETE FROM session WHERE id = ?1",
        params![session.session_id],
    )?;
    Ok(())
}

pub fn extract_last_tool(session: &Session) -> String {
    let Some(path) = &session.data_file else {
        return String::new();
    };
    let Ok(db) = readonly(path) else {
        return String::new();
    };
    db.query_row(
        "SELECT json_extract(data, '$.tool') FROM part \
         WHERE session_id = ?1 AND json_extract(data, '$.type') = 'tool' \
         ORDER BY time_created DESC, id DESC LIMIT 1",
        params![session.session_id],
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
        drop(db);

        let data = extract(&path, "ses_1");
        assert_eq!(data.title.as_deref(), Some("OpenCode work"));
        assert_eq!(data.last_model, "claude-test");
        assert_eq!(data.tokens.total, 240);
        assert!((data.costs.total - 0.75).abs() < 1e-9);
        assert_eq!(data.metrics.tools.get("bash"), Some(&1));
        let edit = &data.metrics.tool_details["edit"][0];
        assert_eq!(edit.d, "src/main.rs");
        assert_eq!(edit.dur_ms, Some(750));
        let delta = edit.delta.as_ref().unwrap();
        assert_eq!((delta.added, delta.removed), (3, 2));

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
        std::fs::remove_file(path).unwrap();
    }
}
