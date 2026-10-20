//! Mistral Vibe session discovery and extraction.
//!
//! Vibe has written two layouts under `$VIBE_HOME/logs/session`, and a home can
//! hold both:
//!
//! - **`session_<date>_<time>_<id>/`**, from the Python CLI: `meta.json` with
//!   the session's running totals under `stats`, beside `messages.jsonl`, the
//!   conversation in OpenAI chat shape.
//! - **`unified/<id>/`**, from the store the Rust frontend's harness writes:
//!   `meta.json` again, then a content-addressed snapshot (`CURRENT` names a
//!   generation, whose `manifest.json` lists the `chunks/` that make up its
//!   checkpoint and its projection) and a `journal/` of everything since. The
//!   projection's envelope is the session as Vibe's own UI sees it — status,
//!   token totals, context use — and the journal carries a fresh copy of it on
//!   every step, so the newest one there is live where the snapshot is not.
//!
//! Every session's `data_file` is its `meta.json`: both layouts have one, and
//! the directory around it is the session.
//!
//! Cost is cctop's estimate in both layouts: the tokens at Mistral's published
//! rates ([`crate::pricing::resolve_mistral`]). The Python layout's own
//! `stats.session_cost` is not used — it is priced from whatever per-model
//! prices the user's Vibe config carries, and the unified store records none,
//! so taking it would price the same model two ways depending on the layout.

use super::extract::{push_tool_detail, tool_detail};
use super::{ActivityState, ContextUsage, ModelBreakdown, Session, SessionData, Tokens};
use crate::config;
use crate::pricing::Provider;
use crate::util;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// How much of the newest journal segment is read for live state. A segment
/// holds one turn, and each model call in it embeds the whole prompt, so a
/// segment runs to megabytes; the records this module reads are small and come
/// last.
const JOURNAL_TAIL: u64 = 262_144;

pub fn list_sessions() -> Vec<Session> {
    let mut sessions: Vec<Session> = session_dirs()
        .iter()
        .filter_map(|dir| session_in(dir))
        .collect();
    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    sessions
}

/// Every session directory in every home, both layouts.
fn session_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for root in config::vibe_sessions_roots() {
        for name in config::list_dir(&root) {
            if name.starts_with("session_") {
                dirs.push(root.join(name));
            }
        }
        let unified = root.join("unified");
        for name in config::list_dir(&unified) {
            dirs.push(unified.join(name));
        }
    }
    dirs
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn session_in(dir: &Path) -> Option<Session> {
    let meta_path = dir.join("meta.json");
    let meta = read_json(&meta_path)?;
    let id = meta.get("session_id").and_then(Value::as_str)?;
    let mut session = Session::new(Provider::Vibe, id.to_string());
    session.label_source = meta
        .pointer("/environment/working_directory")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    session.title = meta
        .get("title")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    // Normalised through a parse: Vibe writes `+00:00` and the rest of cctop
    // compares these as strings.
    session.started_at = stamp(&meta, "start_time")
        .map(util::ms_to_rfc3339)
        .unwrap_or_default();
    session.last_active = util::ms_to_rfc3339(last_active_ms(&meta, &meta_path));
    // Only the Python layout's, which is in the file already read: the unified
    // store's takes reading the snapshot, which is extraction's job, and the
    // loader copies the model it finds onto the row.
    session.model = meta
        .pointer("/config/active_model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    session.data_file = Some(meta_path);
    Some(session)
}

fn stamp(meta: &Value, key: &str) -> Option<i64> {
    meta.get(key)
        .and_then(Value::as_str)
        .and_then(util::parse_ts)
        .map(|t| t.timestamp_millis())
}

/// When the session last had a message, which is not when its files last
/// changed: a background process finishing hours later writes the journal, a
/// snapshot rewrites `meta.json`, and neither is the conversation moving.
///
/// No message carries a time in either layout, so it is the newest of what
/// does: the turn stamps in `meta.json` (`end_time` lags the turn that set
/// `unseen_at`), the Python layout's `messages.jsonl`, which is only ever
/// appended a message, and the journal while a turn is in flight — a segment
/// holds one turn and an empty one has been opened after it, so a segment with
/// anything in it is the turn being written.
fn last_active_ms(meta: &Value, meta_path: &Path) -> i64 {
    let dir = meta_path.parent().unwrap_or(meta_path);
    let messages = config::file_mtime_ms(&dir.join("messages.jsonl"));
    let in_flight = newest_segment(dir)
        .filter(|s| std::fs::metadata(s).is_ok_and(|m| m.len() > 0))
        .map_or(0, |s| config::file_mtime_ms(&s));
    ["end_time", "unseen_at"]
        .into_iter()
        .filter_map(|key| stamp(meta, key))
        .fold(messages.max(in_flight) as i64, i64::max)
}

fn is_unified(dir: &Path) -> bool {
    dir.join("CURRENT").is_file()
}

/// The newest write anywhere in the session, from its `meta.json`.
///
/// `meta.json` is rewritten at the end of a turn and the journal on every step
/// of one, so neither alone says whether the session moved: a cache keyed on
/// `meta.json` would hold a running turn's figures until it finished.
pub fn mtime_ms(meta_path: &Path) -> u64 {
    let Some(dir) = meta_path.parent() else {
        return 0;
    };
    let journal = newest_segment(dir);
    [
        Some(meta_path.to_path_buf()),
        Some(dir.join("CURRENT")),
        Some(dir.join("messages.jsonl")),
        journal,
    ]
    .into_iter()
    .flatten()
    .map(|p| config::file_mtime_ms(&p))
    .max()
    .unwrap_or(0)
}

/// The journal segment being appended to: segments are named by their first
/// sequence number, zero-padded, so the last name is the newest.
fn newest_segment(dir: &Path) -> Option<PathBuf> {
    let journal = dir.join("journal");
    config::list_dir(&journal)
        .into_iter()
        .rev()
        .find(|n| n.ends_with(".jsonl"))
        .map(|n| journal.join(n))
}

/// The current generation's directory and manifest.
fn generation(dir: &Path) -> Option<(PathBuf, Value)> {
    let current = read_json(&dir.join("CURRENT"))?;
    let generation = current.get("generation").and_then(Value::as_str)?;
    let gen_dir = dir.join("generations").join(generation);
    let manifest = read_json(&gen_dir.join("manifest.json"))?;
    Some((gen_dir, manifest))
}

/// The items of one snapshot part — `checkpoint` (the model's context) or
/// `projection_state` (the UI's history) — concatenated from its chunks.
fn chunk_items(dir: &Path, manifest: &Value, part: &str) -> Vec<Value> {
    chunk_paths(dir, manifest, part)
        .iter()
        .filter_map(|path| read_json(path))
        .filter_map(|v| match v {
            Value::Array(items) => Some(items),
            _ => None,
        })
        .flatten()
        .collect()
}

fn chunk_paths(dir: &Path, manifest: &Value, part: &str) -> Vec<PathBuf> {
    manifest
        .pointer(&format!("/{part}/chunks"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|c| dir.join("chunks").join(format!("{c}.json")))
        .collect()
}

/// The files the conversation's text is in, for search to scan:
/// `messages.jsonl`, or the unified checkpoint's chunks.
pub fn conversation_files(meta_path: &Path) -> Vec<PathBuf> {
    let Some(dir) = meta_path.parent() else {
        return Vec::new();
    };
    if !is_unified(dir) {
        return vec![dir.join("messages.jsonl")];
    }
    generation(dir)
        .map(|(_, manifest)| chunk_paths(dir, &manifest, "checkpoint"))
        .unwrap_or_default()
}

/// The model the session runs on.
///
/// The unified store records a model only when it changes: `active_model` in
/// the runtime state when one was chosen for the session, else a
/// `model_change` entry in the history. A session that never changed model
/// ran on the configured default, which is all a fresh session has to go on.
fn model(dir: &Path, meta: &Value) -> String {
    if let Some(m) = meta.pointer("/config/active_model").and_then(Value::as_str) {
        return m.to_string();
    }
    let chosen = generation(dir).and_then(|(gen_dir, manifest)| {
        read_json(&gen_dir.join("runtime-state.json"))
            .and_then(|rt| {
                rt.pointer("/session_metadata/active_model")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| {
                chunk_items(dir, &manifest, "projection_state")
                    .iter()
                    .rev()
                    .find(|e| e.get("kind").and_then(Value::as_str) == Some("model_change"))
                    .and_then(|e| e.pointer("/details/model").and_then(Value::as_str))
                    .map(str::to_string)
            })
    });
    chosen.unwrap_or_else(configured_model)
}

/// `active_model` from Vibe's `config.toml`.
fn configured_model() -> String {
    std::fs::read_to_string(config::VIBE_HOME.join("config.toml"))
        .ok()
        .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
        .and_then(|doc| doc.get("active_model")?.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// What the newest journal records say about a unified session.
#[derive(Default)]
struct JournalTail {
    /// The newest projection envelope: the session as Vibe's UI last drew it.
    envelope: Option<Value>,
    /// The newest tool the history shows being run.
    last_tool: Option<String>,
    /// An approval or question registered and not yet answered.
    asking: bool,
    /// A model call started after the newest envelope.
    calling: bool,
}

/// A journal record's type, read off the end of the line.
///
/// Keys are written sorted, so `type` is always last, and that makes it
/// readable from a line cut off at its start: a model call's record embeds the
/// whole prompt, and can be larger than the tail that is read.
fn record_type(line: &str) -> Option<&str> {
    line.trim_end()
        .strip_suffix("\"}")?
        .rsplit_once("\"type\":\"")
        .map(|(_, t)| t)
}

fn read_journal(dir: &Path) -> JournalTail {
    let mut tail = JournalTail::default();
    let Some(text) = newest_segment(dir).and_then(|s| util::read_tail(&s, JOURNAL_TAIL)) else {
        return tail;
    };
    let mut resolved = std::collections::HashSet::new();
    let mut decided = false;
    for line in text.lines().rev() {
        let parse = || serde_json::from_str::<Value>(line).ok();
        match record_type(line) {
            Some("callback_resolved") => {
                if let Some(id) = parse().and_then(|r| {
                    r.pointer("/payload/callback_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                }) {
                    resolved.insert(id);
                }
            }
            Some("callback_registered") if !decided => {
                let id = parse().and_then(|r| {
                    r.pointer("/payload/callback_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
                if id.is_some_and(|id| !resolved.contains(&id)) {
                    tail.asking = true;
                }
            }
            Some("action_intent") if !decided && tail.envelope.is_none() => {
                tail.calling = true;
            }
            Some("projection_delta") => {
                let Some(record) = parse() else { continue };
                for op in record
                    .pointer("/payload/delta")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .rev()
                {
                    if tail.last_tool.is_none()
                        && let Some(tool) =
                            op.pointer("/entry/detail/toolName").and_then(Value::as_str)
                    {
                        tail.last_tool = Some(tool.to_string());
                    }
                    if tail.envelope.is_none()
                        && op.get("op").and_then(Value::as_str) == Some("set_envelope")
                    {
                        tail.envelope = op.get("state").cloned();
                        // A callback older than the envelope was answered or
                        // abandoned before the session was last drawn.
                        decided = true;
                    }
                }
            }
            _ => {}
        }
        if tail.envelope.is_some() && tail.last_tool.is_some() {
            break;
        }
    }
    tail
}

/// The newest envelope: the journal's, else the snapshot's.
fn envelope(dir: &Path, tail: &JournalTail) -> Option<Value> {
    tail.envelope.clone().or_else(|| {
        let (gen_dir, _) = generation(dir)?;
        read_json(&gen_dir.join("projection-state.json"))?
            .get("snapshot")
            .cloned()
    })
}

/// Vibe's token counts: input with its cached part inside it, as Codex reports.
fn usage_tokens(usage: &Value) -> Tokens {
    let n = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
    Tokens {
        total: n("inputTokens") + n("outputTokens"),
        ..Tokens::from_prompt(n("inputTokens"), n("cachedInputTokens"), n("outputTokens"))
    }
}

pub fn extract(meta_path: &Path) -> SessionData {
    let mut data = SessionData::default();
    let Some(dir) = meta_path.parent() else {
        return data;
    };
    let Some(meta) = read_json(meta_path) else {
        data.error = Some(format!(
            "Could not read Vibe session {}",
            meta_path.display()
        ));
        return data;
    };
    data.title = meta
        .get("title")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    data.last_model = model(dir, &meta);

    if is_unified(dir) {
        extract_unified(dir, &mut data);
    } else {
        extract_legacy(dir, &meta, &mut data);
    }

    // A model with no published rate prices at nothing, as it does for every
    // harness; `cctop doctor` is what tells that zero from a free model.
    if let Some(rates) = crate::pricing::resolve_mistral(&data.last_model) {
        data.costs = super::costs_at(rates, &data.tokens);
    }
    if !data.last_model.is_empty() {
        data.models = vec![data.last_model.clone()];
        data.model_breakdown.push(ModelBreakdown {
            model: data.last_model.clone(),
            total: data.costs.total,
            tokens: data.tokens.clone(),
            costs: data.costs.clone(),
        });
        // Vibe keeps its usage as running session totals, with no time on any
        // message or turn, so the whole session is filed under the moment it
        // was last active rather than left out of "today" and the month.
        //
        // ponytail: exact for a session that starts and ends inside one day
        // (or hour), which is nearly all of them. One that spans midnight
        // files everything under its last day, and resuming an old session
        // moves its whole cost to now, "last 60 min" included. Per-turn
        // attribution would need cctop to remember the totals it saw and file
        // the differences, since the store keeps no history of them.
        if let Some(dt) = chrono::DateTime::from_timestamp_millis(last_active_ms(&meta, meta_path))
        {
            let model = data.last_model.clone();
            let billed = data.tokens.all_input() + data.tokens.output;
            for (map, key) in [
                (&mut data.tokens_by_day, util::local_date_key(&dt)),
                (&mut data.tokens_by_hour, util::local_hour_key(&dt)),
            ] {
                *map.entry(key)
                    .or_default()
                    .entry(model.clone())
                    .or_insert(0) += billed;
            }
            if data.costs.total > 0.0 {
                data.record_cost(&dt, &model, data.costs.total);
            }
        }
    }
    data
}

/// Tokens from the envelope, tools from the checkpoint's context.
///
/// ponytail: the checkpoint is the model's context, so a compaction drops the
/// calls it summarised and the tool count falls with it. The projection's
/// history is no better — it keeps a window, not the session — and the journal
/// only reaches back to the last snapshot.
fn extract_unified(dir: &Path, data: &mut SessionData) {
    let tail = read_journal(dir);
    if let Some(usage) = envelope(dir, &tail)
        .as_ref()
        .and_then(|e| e.pointer("/session/tokenUsage"))
    {
        data.tokens = usage_tokens(usage);
    }
    let Some((_, manifest)) = generation(dir) else {
        return;
    };
    for item in chunk_items(dir, &manifest, "checkpoint") {
        let Some(message) = item.get("message") else {
            continue;
        };
        match message.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                for part in message
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|p| p.get("type").and_then(Value::as_str) == Some("tool_call"))
                {
                    let name = part
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let input = part
                        .pointer("/arguments/value")
                        .cloned()
                        .unwrap_or(Value::Null);
                    let id = part.get("id").and_then(Value::as_str).map(str::to_string);
                    record_tool(data, name, &input, id);
                }
            }
            Some("tool") if message.get("outcome").and_then(Value::as_str) == Some("failure") => {
                data.metrics.tool_errors += 1;
                let id = message.get("tool_call_id").and_then(Value::as_str);
                if let Some(detail) = data
                    .metrics
                    .tool_details
                    .values_mut()
                    .flatten()
                    .find(|d| d.id.is_some() && d.id.as_deref() == id)
                {
                    detail.failed = true;
                }
            }
            _ => {}
        }
    }
}

/// The conversation's messages, in order, in whichever shape the layout keeps
/// them: the unified checkpoint's (`content` as typed parts) or the Python
/// CLI's OpenAI chat shape.
///
/// ponytail: the checkpoint is written when a turn ends, so the unified
/// layout's conversation stops at the last finished turn; the one in flight is
/// only in the journal, as projection entries of yet another shape.
pub fn messages(meta_path: &Path) -> Vec<Value> {
    let Some(dir) = meta_path.parent() else {
        return Vec::new();
    };
    if !is_unified(dir) {
        let text = std::fs::read_to_string(dir.join("messages.jsonl")).unwrap_or_default();
        return text
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
    }
    let Some((_, manifest)) = generation(dir) else {
        return Vec::new();
    };
    chunk_items(dir, &manifest, "checkpoint")
        .into_iter()
        .filter_map(|mut item| item.get_mut("message").map(Value::take))
        .collect()
}

/// Tokens and the failure count from `stats`, tools from `messages.jsonl`.
fn extract_legacy(dir: &Path, meta: &Value, data: &mut SessionData) {
    let stat = |k: &str| {
        meta.pointer(&format!("/stats/{k}"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let (prompt, completion) = (
        stat("session_prompt_tokens"),
        stat("session_completion_tokens"),
    );
    data.tokens = Tokens {
        total: prompt + completion,
        ..Tokens::from_prompt(prompt, 0, completion)
    };
    data.metrics.tool_errors = stat("tool_calls_failed");
    let Ok(text) = std::fs::read_to_string(dir.join("messages.jsonl")) else {
        return;
    };
    for message in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        for call in message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = call
                .pointer("/function/name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            // OpenAI shape: the arguments are a JSON document in a string.
            let input = call
                .pointer("/function/arguments")
                .and_then(Value::as_str)
                .and_then(|a| serde_json::from_str(a).ok())
                .unwrap_or(Value::Null);
            let id = call.get("id").and_then(Value::as_str).map(str::to_string);
            record_tool(data, name, &input, id);
        }
    }
}

/// Count one tool call. Vibe stamps no time on a message, so the detail has
/// none rather than an invented one.
fn record_tool(data: &mut SessionData, name: &str, input: &Value, id: Option<String>) {
    let (short, full) = tool_detail(name, input);
    push_tool_detail(
        &mut data.metrics.tool_details,
        name,
        short,
        full,
        String::new(),
        id,
        None,
    );
    *data.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
    data.metrics.tool_count += 1;
}

/// The session's live state.
///
/// For the unified store, the journal says it outright: an approval nobody has
/// answered is a question, and otherwise the newest envelope's status is
/// `running` or `idle`. A model call newer than every envelope in the tail is
/// a turn in flight whatever the last envelope said.
///
/// ponytail: no permission mode. Vibe's is the agent a session was started
/// with (`--agent auto-approve` and kin), which the store does not record.
pub fn live_state(session: &Session) -> (ActivityState, Option<crate::hook::Permission>) {
    let Some(dir) = session.data_file.as_deref().and_then(Path::parent) else {
        return (ActivityState::Working, None);
    };
    if !is_unified(dir) {
        return (legacy_state(dir), None);
    }
    let tail = read_journal(dir);
    if tail.asking {
        return (ActivityState::Asking, None);
    }
    if tail.calling {
        return (ActivityState::Working, None);
    }
    let status = envelope(dir, &tail).and_then(|e| {
        e.pointer("/session/status/type")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    let state = match status.as_deref() {
        Some("idle") => ActivityState::WaitingForInput,
        _ => ActivityState::Working,
    };
    (state, None)
}

/// The Python layout's state from its last message: an assistant reply with no
/// calls in it is the end of the turn.
fn legacy_state(dir: &Path) -> ActivityState {
    let last = util::read_tail(&dir.join("messages.jsonl"), 65_536).and_then(|text| {
        text.lines()
            .rev()
            .find_map(|l| serde_json::from_str::<Value>(l).ok())
    });
    let Some(last) = last else {
        return ActivityState::Working;
    };
    let calls = last
        .get("tool_calls")
        .and_then(Value::as_array)
        .is_some_and(|c| !c.is_empty());
    match last.get("role").and_then(Value::as_str) {
        Some("assistant") if !calls => ActivityState::WaitingForInput,
        _ => ActivityState::Working,
    }
}

/// The newest tool the unified history shows, without its namespace:
/// `file_system.bash` is the `bash` the checkpoint calls it.
pub fn extract_last_tool(session: &Session) -> String {
    let Some(dir) = session.data_file.as_deref().and_then(Path::parent) else {
        return String::new();
    };
    read_journal(dir)
        .last_tool
        .map(|t| t.rsplit('.').next().unwrap_or(&t).to_string())
        .unwrap_or_default()
}

/// Context use from the envelope (or the Python layout's `stats`), against
/// the window the model table gives the session's model.
pub fn extract_context(session: &Session) -> Option<ContextUsage> {
    let meta_path = session.data_file.as_deref()?;
    let dir = meta_path.parent()?;
    let used = if is_unified(dir) {
        envelope(dir, &read_journal(dir))?
            .pointer("/session/contextUsage/totalTokens")?
            .as_u64()?
    } else {
        read_json(meta_path)?
            .pointer("/stats/context_tokens")?
            .as_u64()?
    };
    if used == 0 {
        return None;
    }
    Some(ContextUsage {
        used,
        max: crate::pricing::litellm_max_input_tokens(&session.model)?,
        compacted: false,
    })
}

/// Delete a session: its directory is the whole of it, in either layout.
pub fn delete(session: &Session) -> std::io::Result<()> {
    match session.data_file.as_deref().and_then(Path::parent) {
        Some(dir) if dir.exists() => std::fs::remove_dir_all(dir),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(path: &Path, body: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn envelope_line(status: &str, total: u64) -> String {
        format!(
            r#"{{"payload":{{"delta":[{{"op":"set_envelope","state":{{"session":{{"status":{{"type":"{status}"}},"tokenUsage":{{"cachedInputTokens":80,"inputTokens":100,"outputTokens":{total},"totalTokens":0}},"contextUsage":{{"totalTokens":4242}}}}}}}}]}},"sequence":3,"type":"projection_delta"}}"#
        )
    }

    /// A unified session as Vibe lays it out: `CURRENT` naming a generation,
    /// its manifest pointing into `chunks/`, a journal segment after it.
    fn unified(journal: &[String]) -> (TempDir, PathBuf) {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("unified").join("sid");
        write(
            &dir.join("meta.json"),
            r#"{"session_id":"sid","title":"Fix it","start_time":"2026-10-17T22:23:40.527000+00:00",
               "end_time":"2026-10-17T22:33:34.738000+00:00",
               "environment":{"working_directory":"/work"}}"#,
        );
        write(&dir.join("CURRENT"), r#"{"generation":"0007"}"#);
        write(
            &dir.join("generations/0007/manifest.json"),
            r#"{"checkpoint":{"chunks":["c1"]},"projection_state":{"chunks":["p1"]}}"#,
        );
        write(
            &dir.join("generations/0007/runtime-state.json"),
            r#"{"session_metadata":{"active_model":null}}"#,
        );
        write(
            &dir.join("generations/0007/projection-state.json"),
            r#"{"snapshot":{"session":{"status":{"type":"idle"},"tokenUsage":{"inputTokens":1,"outputTokens":1}}}}"#,
        );
        write(
            &dir.join("chunks/c1.json"),
            r#"[{"message":{"role":"user","content":[{"type":"text","text":"go"}]}},
                {"message":{"role":"assistant","content":[
                   {"type":"tool_call","id":"a","name":"bash","arguments":{"value":{"command":"ls"}}},
                   {"type":"tool_call","id":"b","name":"grep","arguments":{"value":{}}}]}},
                {"message":{"role":"tool","name":"bash","tool_call_id":"a","outcome":"success"}},
                {"message":{"role":"tool","name":"grep","tool_call_id":"b","outcome":"failure"}}]"#,
        );
        write(
            &dir.join("chunks/p1.json"),
            r#"[{"kind":"model_change","details":{"model":"zai-glm-5-3"}}]"#,
        );
        write(&dir.join("journal/0008.jsonl"), &journal.join("\n"));
        let meta = dir.join("meta.json");
        (tmp, meta)
    }

    fn session(meta: &Path) -> Session {
        let mut s = session_in(meta.parent().unwrap()).unwrap();
        s.data_file = Some(meta.to_path_buf());
        s
    }

    /// LAST is the last turn, not the last write: every file here was written
    /// just now, the way a background process finishing hours later writes them.
    #[test]
    fn last_active_is_the_last_turn_not_the_last_write() {
        let last = |meta: &Path| session(meta).last_active;

        // The newest segment is empty: the turn is over.
        let (_tmp, meta) = unified(&[]);
        assert!(
            last(&meta).starts_with("2026-10-17T22:33:34"),
            "{}",
            last(&meta)
        );

        // `unseen_at` is set by a turn `end_time` has not caught up with.
        write(
            &meta,
            r#"{"session_id":"sid","end_time":"2026-10-17T22:33:34+00:00",
               "unseen_at":"2026-10-17T23:10:00+00:00"}"#,
        );
        assert!(
            last(&meta).starts_with("2026-10-17T23:10:00"),
            "{}",
            last(&meta)
        );

        // A segment with records in it is a turn being written now.
        let (_tmp, meta) = unified(&[envelope_line("running", 20)]);
        assert!(last(&meta).as_str() > "2026-10-18", "{}", last(&meta));
    }

    #[test]
    fn unified_session_lists_and_extracts() {
        let (_tmp, meta) = unified(&[envelope_line("running", 20)]);
        let s = session(&meta);
        assert_eq!(s.session_id, "sid");
        assert_eq!(s.label_source, "/work");
        assert_eq!(s.title.as_deref(), Some("Fix it"));
        assert!(s.started_at.starts_with("2026-10-17T22:23:40"));

        let data = extract(&meta);
        // The model change in the history, since the runtime chose none.
        assert_eq!(data.last_model, "zai-glm-5-3");
        // Mistral's rate for GLM 5.3, with the cached part of the input at the
        // cached rate: 20 uncached at $1.4/M, 80 cached at $0.14/M, 20 out at
        // $4.4/M.
        let want = (20.0 * 1.4 + 80.0 * 0.14 + 20.0 * 4.4) / 1e6;
        assert!(
            (data.costs.total - want).abs() < 1e-12,
            "{}",
            data.costs.total
        );
        // The journal's envelope wins over the stale snapshot, and the cached
        // part of the input is inside it, not beside it.
        assert_eq!(data.tokens.all_input(), 100);
        assert_eq!(data.tokens.cached_input, 80);
        assert_eq!(data.tokens.output, 20);
        assert_eq!(data.tokens.total, 120);
        assert_eq!(data.metrics.tool_count, 2);
        assert_eq!(data.metrics.tool_errors, 1);
        assert!(data.metrics.tool_details["grep"][0].failed);
        assert!(!data.metrics.tool_details["bash"][0].failed);
        assert_eq!(data.model_breakdown[0].model, "zai-glm-5-3");
        assert_eq!(data.model_breakdown[0].total, data.costs.total);
        // With no time on any message, the session lands whole on the day and
        // hour it was last active: the fixture's files, written just now.
        let today = util::local_date_key(&chrono::Utc::now());
        assert_eq!(data.costs_by_day[&today]["zai-glm-5-3"], data.costs.total);
        assert_eq!(data.tokens_by_day[&today]["zai-glm-5-3"], 120);
        assert_eq!(data.cost_last_hour(&chrono::Utc::now()), data.costs.total);
    }

    #[test]
    fn unified_live_state_reads_the_journal() {
        let registered = r#"{"payload":{"callback_id":"cb1","kind":"approval"},"sequence":4,"type":"callback_registered"}"#;
        let resolved =
            r#"{"payload":{"callback_id":"cb1"},"sequence":5,"type":"callback_resolved"}"#;
        let state = |journal: &[String]| {
            let (_tmp, meta) = unified(journal);
            live_state(&session(&meta)).0
        };

        assert_eq!(
            state(&[envelope_line("idle", 1)]),
            ActivityState::WaitingForInput
        );
        assert_eq!(
            state(&[envelope_line("running", 1)]),
            ActivityState::Working
        );
        assert_eq!(
            state(&[envelope_line("running", 1), registered.into()]),
            ActivityState::Asking
        );
        assert_eq!(
            state(&[
                envelope_line("running", 1),
                registered.into(),
                resolved.into()
            ]),
            ActivityState::Working
        );
        // A model call's record cut off at its start by the tail is still a
        // model call: its type is the last thing on the line.
        assert_eq!(
            state(&[
                envelope_line("idle", 1),
                r#"ated prompt…"}]},"sequence":6,"type":"action_intent"}"#.into()
            ]),
            ActivityState::Working
        );
        // No journal yet: the snapshot's word.
        assert_eq!(state(&[]), ActivityState::WaitingForInput);
    }

    #[test]
    fn unified_context_comes_from_the_envelope() {
        let (_tmp, meta) = unified(&[envelope_line("idle", 1)]);
        let mut s = session(&meta);
        s.model = "mistral-large-latest".into();
        if let Some(ctx) = extract_context(&s) {
            assert_eq!(ctx.used, 4242);
        }
    }

    #[test]
    fn legacy_session_extracts_stats_and_tool_calls() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("session_20260515_132636_8e2fe70e");
        write(
            &dir.join("meta.json"),
            r#"{"session_id":"8e2fe70e-a980","start_time":"2026-05-15T13:26:36.456697+00:00",
               "environment":{"working_directory":"/repo"},"title":"Overall",
               "config":{"active_model":"mistral-medium-3.5"},
               "stats":{"session_prompt_tokens":97761,"session_completion_tokens":485,
                        "tool_calls_failed":1,"context_tokens":25585,"session_cost":0.15}}"#,
        );
        write(
            &dir.join("messages.jsonl"),
            concat!(
                r#"{"role":"user","content":"go"}"#,
                "\n",
                r#"{"role":"assistant","tool_calls":[{"id":"U6","function":{"name":"bash","arguments":"{\"command\": \"ls\"}"}},{"id":"PC","function":{"name":"read_file","arguments":"{\"path\": \"/x\"}"}}]}"#,
                "\n",
                r#"{"role":"tool","content":"ok","tool_call_id":"U6"}"#,
                "\n",
                r#"{"role":"assistant","content":"done"}"#,
            ),
        );
        let meta = dir.join("meta.json");
        let s = session(&meta);
        assert_eq!(s.model, "mistral-medium-3.5");
        assert_eq!(s.label_source, "/repo");
        assert!(s.cost_available);

        let data = extract(&meta);
        // At Mistral's rate for Medium 3.5, not the $0.15 Vibe wrote down:
        // 97,761 in at $1.5/M and 485 out at $7.5/M.
        assert!((data.costs.total - (97761.0 * 1.5 + 485.0 * 7.5) / 1e6).abs() < 1e-9);
        assert_eq!(data.tokens.all_input(), 97761);
        assert_eq!(data.tokens.output, 485);
        assert_eq!(data.metrics.tool_count, 2);
        assert_eq!(data.metrics.tools["read_file"], 1);
        assert_eq!(data.metrics.tool_errors, 1);
        assert_eq!(live_state(&s).0, ActivityState::WaitingForInput);
    }

    #[test]
    fn delete_removes_the_session_directory() {
        let (tmp, meta) = unified(&[]);
        delete(&session(&meta)).unwrap();
        assert!(!meta.parent().unwrap().exists());
        assert!(tmp.path().join("unified").exists());
    }

    #[test]
    fn record_type_reads_the_last_key() {
        assert_eq!(
            record_type(r#"{"a":{"type":"x"},"type":"core_input"}"#),
            Some("core_input")
        );
        assert_eq!(record_type("truncated"), None);
    }
}
