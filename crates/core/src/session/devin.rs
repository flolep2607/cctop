//! Devin CLI session discovery and transcript extraction.
//!
//! Devin keeps a session's metadata in a SQLite database
//! (`~/.local/share/devin/cli/sessions.db`, or `$CHISEL_SESSION_DB`) and the
//! conversation it can share in an ATIF-v1.7 transcript beside it
//! (`transcripts/<id>.json`). The two answer different questions: the
//! transcript is the artefact other tools are meant to read — per-step token
//! usage, tool calls with arguments, the model each step ran — while the
//! database is the live store the CLI is actually working from, where
//! `message_nodes` is the newest state of the conversation and
//! `tool_call_state` carries each call's outcome.
//!
//! Cost is the honest gap: neither side records one, so a Devin row reports
//! tokens and tools but never a dollar figure — the same position Cursor and
//! Windsurf are in. ponytail: the session row's `metadata` does carry
//! `total_credit_cost` and `total_acu_cost`, but those are Devin's own plan
//! units, not dollars, and read zero on every session seen; the cost column
//! stays empty rather than show a credit count under a `$`.
//!
//! The transcript is read through the typed model in [`super::devin_atif`].

use super::devin_atif::Transcript;
use super::extract::{push_tool_detail, tool_detail};
use super::readonly;
use super::{ActivityState, Costs, ModelBreakdown, Session, SessionData, Tokens};
use crate::config;
use crate::pricing::Provider;
use crate::util;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

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
        .unwrap_or_else(|| config::DEVIN_SESSIONS_DB.clone())
}

/// Discover every persisted Devin session, newest activity first within each
/// home.
pub fn list_sessions() -> Vec<Session> {
    let mut sessions = Vec::new();
    for (i, cli) in config::devin_cli_dirs().iter().enumerate() {
        // This user's own database honours `$CHISEL_SESSION_DB`; another
        // home's can only be the conventional one.
        let (db, transcripts) = match i {
            0 => (
                config::DEVIN_SESSIONS_DB.clone(),
                config::DEVIN_TRANSCRIPTS_DIR.clone(),
            ),
            _ => (cli.join("sessions.db"), cli.join("transcripts")),
        };
        list_in(&db, &transcripts, &mut sessions);
    }
    sessions
}

fn list_in(db_path: &Path, transcripts: &Path, sessions: &mut Vec<Session>) {
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

        let mut session = Session::new(Provider::Devin, session_id);
        session.model = model;
        session.label_source = working_dir;
        // The database stamps seconds, not milliseconds.
        session.started_at = util::ms_to_rfc3339(created_at * 1000);
        session.last_active = util::ms_to_rfc3339(last_activity_at * 1000);
        session.title = title.filter(|t| !t.is_empty());
        session.data_file = Some(transcript);
        // Devin records no cost data locally — tokens yes, dollars no.
        session.cost_available = false;
        session.total_cost = None;
        sessions.push(session);
    }
}

/// Parse an ATIF transcript, enriched with tool outcomes from the database.
///
/// Tokens are summed from the steps rather than taken from `final_metrics`:
/// the two agree on every transcript seen, but only the steps say which model
/// and which hour a token belongs to, and a session total that is the sum of
/// its own breakdown cannot disagree with it. `final_metrics` answers only for
/// a transcript whose steps carry no counts at all.
pub fn extract(path: &Path) -> SessionData {
    let mut data = SessionData::default();

    let json_content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) => {
            data.error = Some(format!(
                "Could not read Devin transcript {}: {err}",
                path.display()
            ));
            return data;
        }
    };

    let transcript = match Transcript::parse(&json_content) {
        Ok(t) => t,
        Err(err) => {
            data.error = Some(format!(
                "Could not parse Devin transcript {}: {err}",
                path.display()
            ));
            return data;
        }
    };

    let agent_model = transcript.agent.get().and_then(|a| a.model_name.get());

    // Outcomes live in the database, not the transcript: one query maps each
    // call id to the status its update last reported, so a call still running
    // (no update row) is not mistaken for a failure.
    let statuses = tool_statuses(transcript.session_id.get(), &db_for(Some(path)));

    // Tokens attributed to each model across the session's steps. One model is
    // spelled two ways in the same transcript — most steps record the slug the
    // API was called with ("swe-2-high") while a few carry the display name the
    // database knows ("SWE-2 High") — so the bucket is a case-and-punctuation
    // fold, not the raw string, or the report splits one model into two rows.
    // The slug is the spelling shown: it is what the pricing tables key on.
    let mut per_model: HashMap<String, Tokens> = HashMap::new();
    let mut spellings: HashMap<String, String> = HashMap::new();
    let mut canon_order: Vec<String> = Vec::new();
    let mut counted = false;

    for step in transcript.steps().filter(|s| s.source.is("agent")) {
        let ts = step.timestamp.or_empty();

        let step_tokens = step.metrics.get().map(|m| m.tokens());
        if let Some(tokens) = &step_tokens {
            counted = true;
            add(&mut data.tokens, tokens);
        }

        if let Some(model) = step.model_name.get() {
            // Buckets key on the fold and the spelling is chosen once all
            // steps are read: bucketing under the spelling seen so far
            // splits the model the moment a later step spells it better.
            let canon = canonical_model(model);
            match spellings.get_mut(&canon) {
                Some(seen)
                    if *seen != seen.to_ascii_lowercase()
                        && model == model.to_ascii_lowercase() =>
                {
                    *seen = model.to_string();
                }
                Some(_) => {}
                None => {
                    spellings.insert(canon.clone(), model.to_string());
                    canon_order.push(canon.clone());
                }
            }
            let entry = per_model.entry(canon.clone()).or_default();
            if let Some(tokens) = &step_tokens {
                add(entry, tokens);
                // A step with a timestamp and its own token counts is all a
                // bucket needs; a step missing either contributes nothing
                // rather than an invented figure.
                if let Some(dt) = util::parse_ts(ts) {
                    *data
                        .tokens_by_day
                        .entry(util::local_date_key(&dt))
                        .or_default()
                        .entry(canon.clone())
                        .or_insert(0) += tokens.total;
                    *data
                        .tokens_by_hour
                        .entry(util::local_hour_key(&dt))
                        .or_default()
                        .entry(canon.clone())
                        .or_insert(0) += tokens.total;
                }
            }
        }

        for call in step.calls() {
            let name = call.function_name.get().unwrap_or("unknown");
            let input = call.arguments.value();
            let call_id = call.tool_call_id.get().map(str::to_string);

            let (short, full) = tool_detail(name, &input);
            push_tool_detail(
                &mut data.metrics.tool_details,
                name,
                short,
                full,
                ts.to_string(),
                call_id.clone(),
                None,
            );
            *data.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
            data.metrics.tool_count += 1;

            let failed = call_id
                .as_deref()
                .and_then(|id| statuses.get(id))
                .is_some_and(|status| status != "completed");
            data.metrics.tool_errors += u64::from(failed);
            // The diff an edit made lives in its arguments — ATIF keeps no
            // structuredPatch — and the report's Changes view is built from
            // deltas, so it is attached here rather than on display.
            let delta = matches!(name, "edit" | "write")
                .then(|| super::extract::edit_delta(&input))
                .flatten();
            if let Some(detail) = data
                .metrics
                .tool_details
                .get_mut(name)
                .and_then(|details| details.last_mut())
            {
                detail.failed = failed;
                if let Some(delta) = delta {
                    // A failed edit changed nothing, so it adds no lines; its
                    // diff is still what it tried, which the Tools panel shows.
                    if !failed {
                        data.metrics.lines_added += delta.added as u64;
                        data.metrics.lines_removed += delta.removed as u64;
                    }
                    detail.delta = Some(delta);
                }
            }
        }
    }

    if !counted && let Some(totals) = transcript.final_metrics.get() {
        data.tokens = totals.tokens();
    }

    let display = |canon: &str| {
        spellings
            .get(canon)
            .cloned()
            .unwrap_or_else(|| canon.to_string())
    };
    data.models = canon_order.iter().map(|c| display(c)).collect();
    for by_model in data
        .tokens_by_day
        .values_mut()
        .chain(data.tokens_by_hour.values_mut())
    {
        *by_model = std::mem::take(by_model)
            .into_iter()
            .map(|(canon, n)| (display(&canon), n))
            .collect();
    }
    // The header model comes from `agent.model_name`, which is the display
    // spelling; show it the way the steps do when they are the same model.
    data.last_model = match agent_model {
        Some(m) => display(&canonical_model(m)),
        None => String::new(),
    };
    for (canon, tokens) in per_model {
        data.model_breakdown.push(ModelBreakdown {
            model: display(&canon),
            total: 0.0,
            tokens,
            costs: Costs::default(),
        });
    }
    data.model_breakdown
        .sort_by_key(|b| std::cmp::Reverse(b.tokens.total));

    data
}

fn add(target: &mut Tokens, step: &Tokens) {
    target.input += step.input;
    target.input_total += step.input_total;
    target.cached_input += step.cached_input;
    target.output += step.output;
    target.total += step.total;
}

/// The name a model's spellings fold into: lowercase, letters and digits only.
///
/// "swe-2-high" and "SWE-2 High" are one model here — the fold is how a slug
/// and its display name are recognised as such without a table of aliases.
fn canonical_model(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Map each tool call in `session_id` to its last reported status.
///
/// The transcript records that a call was made; the database records how it
/// ended. Returns an empty map when the database cannot be read — a transcript
/// without its database is still worth extracting.
pub fn tool_statuses(session_id: Option<&str>, db: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(session_id) = session_id else {
        return out;
    };
    let Ok(db) = readonly(db) else {
        return out;
    };
    let Ok(mut stmt) = db.prepare(
        "SELECT tool_call_id, tool_call_update_json FROM tool_call_state WHERE session_id = ?1",
    ) else {
        return out;
    };
    let Ok(rows) = stmt.query_map([session_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    }) else {
        return out;
    };
    for row in rows.flatten() {
        let (id, update) = row;
        if let Some(status) = update
            .as_deref()
            .and_then(|j| serde_json::from_str::<Value>(j).ok())
            .and_then(|v| v.get("status").and_then(Value::as_str).map(str::to_string))
        {
            out.insert(id, status);
        }
    }
    out
}

/// Delete a Devin session: hide the database row and remove the transcript.
///
/// `hidden` is Devin's own flag for taking a session out of view, and setting
/// it is what the CLI itself does — deleting rows outright while the live CLI
/// holds the database open risks corrupting the in-flight state it is writing.
/// The transcript is the disposable artefact and goes entirely.
pub fn delete(session: &Session) -> std::io::Result<()> {
    if let Some(path) = &session.data_file
        && path.exists()
    {
        std::fs::remove_file(path)?;
    }
    if let Ok(db) = Connection::open(db_for(session.data_file.as_deref())) {
        let _ = db.execute(
            "UPDATE sessions SET hidden = 1 WHERE id = ?1",
            [&session.session_id],
        );
    }
    Ok(())
}

/// Name of the most recently invoked tool, from the database's call ledger.
pub fn extract_last_tool(session: &Session) -> String {
    let Ok(db) = readonly(&db_for(session.data_file.as_deref())) else {
        return String::new();
    };
    db.query_row(
        "SELECT tool_call_update_json, tool_call_json FROM tool_call_state \
         WHERE session_id = ?1 ORDER BY ROWID DESC LIMIT 1",
        [&session.session_id],
        |row| {
            let update: Option<String> = row.get(0)?;
            let call: Option<String> = row.get(1)?;
            Ok((update, call))
        },
    )
    .ok()
    .and_then(|(update, call)| {
        // The update's inferenceToolName is the real tool name; the call's ACP
        // `kind` ("search", "execute") is the coarser fallback.
        update
            .as_deref()
            .and_then(|j| serde_json::from_str::<Value>(j).ok())
            .and_then(|v| {
                v.pointer("/_meta/cognition.ai~1inferenceToolName")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| {
                call.as_deref()
                    .and_then(|j| serde_json::from_str::<Value>(j).ok())
                    .and_then(|v| v.get("kind").and_then(Value::as_str).map(str::to_string))
            })
    })
    .unwrap_or_default()
}

/// The session's live state and permission mode, read off the database.
///
/// Devin's transcript is one JSON document rather than a line-delimited log,
/// so the tail-walk in [`super::live_state`] cannot see inside it — but the
/// newest `message_nodes` row says the same thing faster: a user or tool
/// message means the agent owes a reply, and an assistant message is the end
/// of the turn unless its tool calls are still in flight. `agent_mode` on the
/// session row answers the second question `live_state` asks.
pub fn live_state(session: &Session) -> (ActivityState, Option<crate::hook::Permission>) {
    let Ok(db) = readonly(&db_for(session.data_file.as_deref())) else {
        return (ActivityState::Working, None);
    };
    (
        activity_state(&db, &session.session_id),
        permission_mode(&db, &session.session_id),
    )
}

fn activity_state(db: &Connection, session_id: &str) -> ActivityState {
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
/// rule [`crate::hook::Permission::parse`] applies to modes it has not seen.
fn permission_mode(db: &Connection, session_id: &str) -> Option<crate::hook::Permission> {
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
        "normal" | "auto" => Some(crate::hook::Permission::Ask),
        "accept-edits" => Some(crate::hook::Permission::AcceptEdits),
        "plan" => Some(crate::hook::Permission::Plan),
        "dangerous" | "yolo" | "bypass" | "autonomous" => Some(crate::hook::Permission::Bypass),
        _ => None,
    }
}

/// Context-window usage from the newest node's token bookkeeping.
///
/// `num_tokens_preceding` is what the next request would carry in; the window
/// size comes from the model table, and a model it does not know simply has
/// no context figure to show.
///
/// ponytail: Cognition's own SWE models are in no published table and Devin
/// records no window for them, so a session on one shows no context figure.
/// Guessing a window would put a percentage on the row that means nothing.
pub fn extract_context(session: &Session) -> Option<crate::session::ContextUsage> {
    let db = readonly(&db_for(session.data_file.as_deref())).ok()?;
    let used = db
        .query_row(
            "SELECT metadata FROM message_nodes \
             WHERE session_id = ?1 AND metadata IS NOT NULL ORDER BY row_id DESC LIMIT 1",
            [&session.session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
        .and_then(|raw| {
            serde_json::from_str::<Value>(&raw)
                .ok()
                .and_then(|v| v.get("num_tokens_preceding").and_then(Value::as_u64))
        })?;
    if used == 0 {
        return None;
    }
    Some(crate::session::ContextUsage {
        used,
        max: crate::pricing::litellm_max_input_tokens(&session.model)?,
        compacted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &TempDir, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        fs::write(&path, body).unwrap();
        path
    }

    /// An ATIF transcript in the shape Devin actually writes: metadata on
    /// `agent`, invocations on agent `steps`, totals under `final_metrics`.
    const TRANSCRIPT: &str = r#"{
        "schema_version": "ATIF-v1.7",
        "session_id": "test-session",
        "agent": {
            "name": "devin",
            "version": "3000.10.27",
            "model_name": "swe-1-6-slow",
            "tool_definitions": [
                {"type": "function", "function": {"name": "read", "description": "Read a file"}}
            ]
        },
        "steps": [
            {"step_id": 1, "timestamp": "2026-09-15T06:32:12Z", "source": "system", "message": "prompt"},
            {"step_id": 2, "timestamp": "2026-09-15T06:33:00Z", "source": "user", "message": "hi"},
            {"step_id": 3, "timestamp": "2026-09-15T06:33:05Z", "source": "agent",
             "message": "working", "model_name": "swe-1-6-slow",
             "tool_calls": [
                {"tool_call_id": "call_1", "function_name": "read", "arguments": {"file_path": "/x"}},
                {"tool_call_id": "call_2", "function_name": "exec", "arguments": {"command": "ls"}}
             ],
             "metrics": {"prompt_tokens": 100, "completion_tokens": 10, "cached_tokens": 50}},
            {"step_id": 4, "timestamp": "2026-09-15T06:33:10Z", "source": "agent",
             "message": "done", "model_name": "swe-1-6-slow", "tool_calls": [],
             "metrics": {"prompt_tokens": 120, "completion_tokens": 20, "cached_tokens": 60}}
        ],
        "final_metrics": {
            "total_prompt_tokens": 220,
            "total_completion_tokens": 30,
            "total_cached_tokens": 110,
            "total_steps": 4
        }
    }"#;

    #[test]
    fn extracts_model_tokens_and_invocations_from_atif() {
        let dir = TempDir::new().unwrap();
        let data = extract(&write(&dir, "test-session.json", TRANSCRIPT));

        assert_eq!(data.last_model, "swe-1-6-slow");
        // Cached tokens are the part of the prompt read from cache, not an
        // addition to it: 220 prompt tokens in all, 110 of them cached.
        assert_eq!(data.tokens.all_input(), 220);
        assert_eq!(data.tokens.input, 110);
        assert_eq!(data.tokens.cached_input, 110);
        assert_eq!(data.tokens.cache_read, 0);
        assert_eq!(data.tokens.output, 30);
        assert_eq!(data.tokens.total, 250);
        // Invocations, not tool_definitions — the toolset is not the usage.
        assert_eq!(data.metrics.tool_count, 2);
        assert_eq!(data.metrics.tools["read"], 1);
        assert_eq!(data.metrics.tools["exec"], 1);
        assert_eq!(data.models, vec!["swe-1-6-slow".to_string()]);
        let breakdown = &data.model_breakdown[0];
        assert_eq!(breakdown.tokens.all_input(), 220);
        assert_eq!(breakdown.tokens.output, 30);
        // Every agent step carries both a timestamp and its own metrics, so
        // the token buckets fill: 110 + 140 across the two steps, wherever
        // the local day boundary happens to fall.
        assert_eq!(
            data.tokens_by_day
                .values()
                .flat_map(|m| m.values())
                .sum::<u64>(),
            250
        );
        assert_eq!(
            data.tokens_by_hour
                .values()
                .flat_map(|m| m.values())
                .sum::<u64>(),
            250
        );
    }

    #[test]
    fn tool_errors_come_from_recorded_statuses_only() {
        let dir = TempDir::new().unwrap();
        let data = extract(&write(&dir, "test-session.json", TRANSCRIPT));
        // "test-session" has no rows in any reachable database, and a call with
        // no recorded status is not a failure — zero, not a guess.
        assert_eq!(data.metrics.tool_errors, 0);
    }

    /// The ERR column for Devin is the database's word on each call: a
    /// `failed` update is an error, a `completed` one is not, and the
    /// transcript is read from `transcripts/` beside the `sessions.db` that
    /// says so.
    #[test]
    fn a_failed_status_in_the_database_is_a_tool_error() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("transcripts")).unwrap();
        let db = Connection::open(dir.path().join("sessions.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE tool_call_state (session_id TEXT, tool_call_id TEXT, \
             tool_call_json TEXT, tool_call_update_json TEXT);
             INSERT INTO tool_call_state VALUES
               ('test-session', 'call_1', '{}', '{\"status\":\"completed\"}'),
               ('test-session', 'call_2', '{}', '{\"status\":\"failed\"}');",
        )
        .unwrap();
        drop(db);
        let data = extract(&write(&dir, "transcripts/test-session.json", TRANSCRIPT));
        assert_eq!(data.metrics.tool_count, 2);
        assert_eq!(data.metrics.tool_errors, 1);
        assert!(data.metrics.tool_details["exec"][0].failed);
        assert!(!data.metrics.tool_details["read"][0].failed);
    }

    /// An edit's arguments are its diff, so the lines column fills from them;
    /// steps with no counts of their own leave the session's total to
    /// `final_metrics`.
    #[test]
    fn edits_count_lines_and_final_metrics_stand_in_for_missing_steps() {
        let dir = TempDir::new().unwrap();
        let transcript = r#"{
            "session_id": "edits",
            "agent": {"model_name": "swe-2-high"},
            "steps": [
                {"source": "agent", "timestamp": "2026-09-15T06:33:05Z", "model_name": "swe-2-high",
                 "tool_calls": [{"tool_call_id": "e1", "function_name": "edit",
                    "arguments": {"file_path": "/src/a.rs", "old_string": "a\nb", "new_string": "c\nd\ne"}}]},
                {"source": "agent", "unknown_future_field": {"x": [1]}, "metrics": "n/a"}
            ],
            "final_metrics": {"total_prompt_tokens": 500, "total_completion_tokens": 20, "total_cached_tokens": 400}
        }"#;
        let data = extract(&write(&dir, "edits.json", transcript));
        assert_eq!(data.metrics.lines_added, 3);
        assert_eq!(data.metrics.lines_removed, 2);
        assert_eq!(data.metrics.tool_details["edit"][0].d, "/src/a.rs");
        assert_eq!(data.tokens.all_input(), 500);
        assert_eq!(data.tokens.cached_input, 400);
        assert_eq!(data.tokens.output, 20);
    }

    #[test]
    fn missing_and_malformed_transcripts_report_an_error() {
        let dir = TempDir::new().unwrap();
        let data = extract(&dir.path().join("gone.json"));
        assert!(
            data.error
                .as_deref()
                .unwrap_or("")
                .contains("Could not read")
        );

        let data = extract(&write(&dir, "bad.json", "not json"));
        assert!(
            data.error
                .as_deref()
                .unwrap_or("")
                .contains("Could not parse")
        );
    }

    /// Steps and `agent.model_name` spell the same model two ways — the slug
    /// the API was called with and the display name the database knows. They
    /// are one row in the breakdown, shown the way the steps spell it, or the
    /// report lists one model twice.
    #[test]
    fn one_models_spellings_fold_into_one_row() {
        let dir = TempDir::new().unwrap();
        let transcript = TRANSCRIPT.replace(
            r#""model_name": "swe-1-6-slow", "tool_calls": []"#,
            r#""model_name": "SWE-1-6 Slow", "tool_calls": []"#,
        );
        let data = extract(&write(&dir, "test-session.json", transcript.as_str()));

        assert_eq!(data.models, vec!["swe-1-6-slow".to_string()]);
        assert_eq!(data.last_model, "swe-1-6-slow");
        assert_eq!(data.model_breakdown.len(), 1);
        assert_eq!(data.model_breakdown[0].model, "swe-1-6-slow");
        assert_eq!(data.model_breakdown[0].tokens.all_input(), 220);
        let spellings: std::collections::HashSet<&String> =
            data.tokens_by_day.values().flat_map(|m| m.keys()).collect();
        assert_eq!(spellings.len(), 1);
    }

    #[test]
    fn devin_provider_has_correct_string_representation() {
        assert_eq!(Provider::Devin.as_str(), "devin");
    }

    #[test]
    fn devin_records_tool_outcomes() {
        assert!(Provider::Devin.records_tool_outcomes());
    }
}
