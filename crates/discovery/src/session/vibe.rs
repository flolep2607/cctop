//! Where Mistral Vibe keeps its sessions, in both its layouts (the Python
//! one's `session_*` directories and the unified store's), and the header
//! facts of each. Reading the turns stays in cctop-core's `session::vibe`.

use super::Discovered;
use crate::time::{ms_to_rfc3339, parse_ts};
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
