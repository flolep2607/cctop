//! Where Mistral Vibe keeps its sessions, in both its layouts (the Python
//! one's `session_*` directories and the unified store's), and the header
//! facts of each. Reading the turns stays in cctop-core's `session::vibe`.

use super::Discovered;
use crate::time::{ms_to_rfc3339, parse_ts};
use crate::{ActivityState, Permission};
use crate::{Provider, paths};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Every Vibe session on the machine, most recently active first.
pub fn list() -> Vec<Discovered> {
    let mut sessions: Vec<Discovered> = session_dirs()
        .iter()
        .filter_map(|dir| session_in(dir))
        .collect();
    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    sessions
}

/// Every session directory in every home, both layouts.
fn session_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for root in paths::vibe_sessions_roots() {
        for name in paths::list_dir(&root) {
            if name.starts_with("session_") {
                dirs.push(root.join(name));
            }
        }
        let unified = root.join("unified");
        for name in paths::list_dir(&unified) {
            dirs.push(unified.join(name));
        }
    }
    dirs
}

pub fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn session_in(dir: &Path) -> Option<Discovered> {
    let meta_path = dir.join("meta.json");
    let meta = read_json(&meta_path)?;
    let id = meta.get("session_id").and_then(Value::as_str)?;
    let mut session = Discovered::new(Provider::Vibe, id.to_string());
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
        .map(ms_to_rfc3339)
        .unwrap_or_default();
    session.last_active = ms_to_rfc3339(last_active_ms(&meta, &meta_path));
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
        .and_then(parse_ts)
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
pub fn last_active_ms(meta: &Value, meta_path: &Path) -> i64 {
    let dir = meta_path.parent().unwrap_or(meta_path);
    let messages = paths::file_mtime_ms(&dir.join("messages.jsonl"));
    let in_flight = newest_segment(dir)
        .filter(|s| std::fs::metadata(s).is_ok_and(|m| m.len() > 0))
        .map_or(0, |s| paths::file_mtime_ms(&s));
    ["end_time", "unseen_at"]
        .into_iter()
        .filter_map(|key| stamp(meta, key))
        .fold(messages.max(in_flight) as i64, i64::max)
}

/// The journal segment being appended to: segments are named by their first
/// sequence number, zero-padded, so the last name is the newest.
pub fn newest_segment(dir: &Path) -> Option<PathBuf> {
    let journal = dir.join("journal");
    paths::list_dir(&journal)
        .into_iter()
        .rev()
        .find(|n| n.ends_with(".jsonl"))
        .map(|n| journal.join(n))
}

/// How much of the newest journal segment is read for live state. A segment
/// holds one turn, and each model call in it embeds the whole prompt, so a
/// segment runs to megabytes; the records this module reads are small and come
/// last.
pub const JOURNAL_TAIL: u64 = 262_144;

pub fn is_unified(dir: &Path) -> bool {
    dir.join("CURRENT").is_file()
}

/// The current generation's directory and manifest.
pub fn generation(dir: &Path) -> Option<(PathBuf, Value)> {
    let current = read_json(&dir.join("CURRENT"))?;
    let generation = current.get("generation").and_then(Value::as_str)?;
    let gen_dir = dir.join("generations").join(generation);
    let manifest = read_json(&gen_dir.join("manifest.json"))?;
    Some((gen_dir, manifest))
}

/// What the newest journal records say about a unified session.
#[derive(Default)]
pub struct JournalTail {
    /// The newest projection envelope: the session as Vibe's UI last drew it.
    pub envelope: Option<Value>,
    /// The newest tool the history shows being run.
    pub last_tool: Option<String>,
    /// An approval or question registered and not yet answered.
    pub asking: bool,
    /// A model call started after the newest envelope.
    pub calling: bool,
}

/// A journal record's type, read off the end of the line.
///
/// Keys are written sorted, so `type` is always last, and that makes it
/// readable from a line cut off at its start: a model call's record embeds the
/// whole prompt, and can be larger than the tail that is read.
pub fn record_type(line: &str) -> Option<&str> {
    line.trim_end()
        .strip_suffix("\"}")?
        .rsplit_once("\"type\":\"")
        .map(|(_, t)| t)
}

pub fn read_journal(dir: &Path) -> JournalTail {
    let mut tail = JournalTail::default();
    let Some(text) = newest_segment(dir).and_then(|s| super::read_tail(&s, JOURNAL_TAIL)) else {
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
pub fn envelope(dir: &Path, tail: &JournalTail) -> Option<Value> {
    tail.envelope.clone().or_else(|| {
        let (gen_dir, _) = generation(dir)?;
        read_json(&gen_dir.join("projection-state.json"))?
            .get("snapshot")
            .cloned()
    })
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
pub fn live_state(session: &Discovered) -> (ActivityState, Option<Permission>) {
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
pub fn legacy_state(dir: &Path) -> ActivityState {
    let last = super::read_tail(&dir.join("messages.jsonl"), 65_536).and_then(|text| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_type_reads_the_last_key() {
        assert_eq!(
            record_type(r#"{"a":{"type":"x"},"type":"core_input"}"#),
            Some("core_input")
        );
        assert_eq!(record_type("truncated"), None);
    }
}
