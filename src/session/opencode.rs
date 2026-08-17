//! OpenCode session discovery and extraction from its current SQLite store.

use super::extract;
use super::{
    ActivityState, ContextUsage, Costs, FallbackRates, ModelBreakdown, Session, SessionData, Tokens,
};
use crate::config;
use crate::pricing::Provider;
use crate::util;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::Value;
use std::cell::RefCell;
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

thread_local! {
    /// Read-only connections, kept open and reused.
    ///
    /// Every session in a database is asked about separately — its activity
    /// dot, its context window, its last tool, its costs — and each of those
    /// used to open a connection of its own. On a machine with 1478 OpenCode
    /// sessions that came to several thousand opens of a 555 MB database per
    /// walk, and the cost was not the reading: 240,000 voluntary context
    /// switches and nine seconds of kernel time, spent almost entirely on
    /// setting up and tearing down file locks.
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

/// State of the newest OpenCode message. OpenCode keeps events in SQLite rather
/// than a transcript file, but its assistant messages record both completion
/// and provider errors.
pub fn extract_activity_state(path: &Path, session_id: &str) -> ActivityState {
    with_db(path, |db| activity_state(db, session_id)).unwrap_or(ActivityState::Working)
}

fn activity_state(db: &Connection, session_id: &str) -> ActivityState {
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
    } else if has_input_request(db, &message_id) {
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
            session.started_at = util::ms_to_rfc3339(created);
            session.last_active = util::ms_to_rfc3339(updated);
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

fn assistant_usage(value: &Value, rates: &mut FallbackRates) -> (String, String, Tokens, Costs) {
    let model = value
        .get("modelID")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let ts = value
        .get("time")
        .and_then(|v| v.get("created"))
        .and_then(Value::as_i64)
        .map(util::ms_to_rfc3339)
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
    // OpenCode writes `cost: 0` for providers it has no rates for, which is
    // indistinguishable in the record from a genuinely free model. A cost it did
    // compute is authoritative; a zero falls back to pricing the tokens.
    let reported = value.get("cost").and_then(Value::as_f64).unwrap_or(0.0);
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
    let mut rates = FallbackRates::default();

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
            let (model, ts, tokens, costs) = assistant_usage(&message, &mut rates);
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
        data.costs.total = if cost > 0.0 || data.tokens.total == 0 {
            cost
        } else {
            rates
                .costs(&data.last_model, &data.tokens)
                .map_or(0.0, |c| c.total)
        };
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
                .map(util::ms_to_rfc3339)
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
            let failed = state.get("status").and_then(Value::as_str) == Some("error");
            data.metrics.tool_errors += u64::from(failed);
            if let Some(detail) = data
                .metrics
                .tool_details
                .get_mut(name)
                .and_then(|details| details.last_mut())
            {
                detail.delta = delta;
                detail.dur_ms = duration_ms;
                detail.failed = failed;
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

/// Context usage from the newest assistant message that carries token counts.
///
/// OpenCode records no window size of its own, so the ceiling comes from
/// LiteLLM's listing for the model. Without one there is no denominator, and a
/// guessed window would misreport CTX% for every model that does not match it.
pub fn extract_context(session: &Session) -> Option<ContextUsage> {
    with_db(session.data_file.as_ref()?, |db| context_of(db, session)).flatten()
}

fn context_of(db: &Connection, session: &Session) -> Option<ContextUsage> {
    // A turn that was aborted or failed before the provider answered is still
    // recorded, with every count at zero. Those sit at the end of many sessions,
    // so walk back past them rather than reading the newest row and giving up.
    let mut stmt = db
        .prepare(
            "SELECT data FROM message WHERE session_id = ?1 \
             AND json_extract(data, '$.role') = 'assistant' \
             ORDER BY time_created DESC, id DESC LIMIT 40",
        )
        .ok()?;
    let rows = stmt
        .query_map(params![session.session_id], |row| row.get::<_, String>(0))
        .ok()?;

    for raw in rows.flatten() {
        let Ok(message) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        // What the next turn has to resend: everything the model was given,
        // cached or not. Output is excluded — the next input count includes it.
        let used = u64_at(&message, &["tokens", "input"])
            + u64_at(&message, &["tokens", "cache", "read"])
            + u64_at(&message, &["tokens", "cache", "write"]);
        if used == 0 {
            continue;
        }
        let model = message
            .get("modelID")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
            .unwrap_or(&session.model);
        return Some(ContextUsage {
            used,
            max: crate::pricing::litellm_max_input_tokens(model)?,
            compacted: false,
        });
    }
    None
}

pub fn extract_last_tool(session: &Session) -> String {
    let Some(path) = &session.data_file else {
        return String::new();
    };
    with_db(path, |db| last_tool(db, &session.session_id)).unwrap_or_default()
}

fn last_tool(db: &Connection, session_id: &str) -> String {
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
        let message = serde_json::json!({
            "role": "assistant",
            "modelID": "canopywave/zai/glm-5.1",
            "cost": 0.5,
            "tokens": {"input": 1_000_000, "output": 0, "cache": {"read": 0, "write": 0}},
        });
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
