//! Where Codex keeps its rollouts, and what a rollout's head says about the
//! session. Reading the turns stays in cctop-core's `session::codex`.

use super::codex_line::Line;
use super::{Discovered, last_entry_ms, scan_first_lines};
use crate::time::parse_ts;
use crate::{Provider, paths};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone, Default)]
pub struct StaticParts {
    pub session_id: String,
    pub started_at: String,
    pub model: String,
    pub cwd: String,
}

static STATIC_CACHE: LazyLock<Mutex<HashMap<PathBuf, StaticParts>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn collect_static(path: &Path) -> StaticParts {
    if let Ok(cache) = STATIC_CACHE.lock()
        && let Some(hit) = cache.get(path)
    {
        return hit.clone();
    }

    let mut parts = StaticParts::default();
    if let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string())
        && let Some(uuid) = paths::trailing_uuid(&stem)
    {
        parts.session_id = uuid.to_string();
    }

    let mut done = false;
    scan_first_lines(path, 50, |bytes| {
        if done {
            return true;
        }
        let Some(item) = Line::parse(bytes) else {
            return false;
        };
        let payload = item.payload.get();
        match item.kind.get() {
            Some("session_meta") => {
                if let Some(p) = payload {
                    // A forked rollout carries a second session_meta naming the
                    // original session it was forked from. The filename UUID (or
                    // the first session_meta) is this file's own identity; later
                    // session_meta entries must not overwrite it.
                    if parts.session_id.is_empty()
                        && let Some(id) = p.id.get()
                    {
                        parts.session_id = id.to_string();
                    }
                    if parts.started_at.is_empty()
                        && let Some(ts) = p.timestamp.get().or_else(|| item.timestamp.get())
                    {
                        parts.started_at = ts.to_string();
                    }
                    if parts.cwd.is_empty()
                        && let Some(cwd) = p.cwd.get()
                    {
                        parts.cwd = cwd.to_string();
                    }
                }
            }
            Some("turn_context") => {
                if let Some(m) = payload.and_then(|p| p.model.get()) {
                    parts.model = m.to_string();
                }
            }
            _ => {}
        }
        done = !parts.session_id.is_empty()
            && !parts.started_at.is_empty()
            && !parts.model.is_empty()
            && !parts.cwd.is_empty();
        true
    });

    if !parts.session_id.is_empty()
        && let Ok(mut cache) = STATIC_CACHE.lock()
    {
        cache.insert(path.to_path_buf(), parts.clone());
    }
    parts
}

/// When a rollout line is a turn — a message, a tool call or its output, the
/// model's reasoning — its time in Unix milliseconds.
///
/// The event stream beside it (`token_count`, `turn_context`, the
/// `session_meta` a resume repeats) is bookkeeping, and so is a `developer`
/// or `system` message: Codex writes its own instructions in that shape.
pub fn turn_ms(bytes: &[u8]) -> Option<i64> {
    let item = Line::parse(bytes)?;
    if !item.kind.is("response_item") {
        return None;
    }
    let payload = item.payload.get()?;
    if payload.kind.is("message") && !(payload.role.is("user") || payload.role.is("assistant")) {
        return None;
    }
    parse_ts(item.timestamp.get()?).map(|d| d.timestamp_millis())
}

/// Every rollout file, newest first.
pub fn rollouts() -> Vec<PathBuf> {
    let mut files: Vec<_> = paths::codex_sessions_roots()
        .iter()
        .filter(|root| paths::dir_exists(root))
        .flat_map(|root| paths::rglob(root, ".jsonl"))
        .collect();
    // By filename, not full path: the rollout name carries the timestamp, and
    // sorting by path would order by whose home the file sits in first.
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()).then_with(|| a.cmp(b)));
    files.reverse();
    files
}

/// The session one rollout holds.
pub fn discover(path: PathBuf) -> Discovered {
    let statics = collect_static(&path);
    let mtime = paths::file_mtime_ms(&path) as i64;
    let touched_at = chrono::DateTime::from_timestamp_millis(mtime)
        .map(|d| d.to_rfc3339())
        .unwrap_or_else(|| statics.started_at.clone());
    let last_active = last_entry_ms(&path, turn_ms)
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|d| d.to_rfc3339())
        .unwrap_or_else(|| touched_at.clone());

    let mut s = Discovered::new(Provider::Codex, statics.session_id);
    s.started_at = statics.started_at;
    s.last_active = last_active;
    s.touched_at = touched_at;
    s.model = statics.model;
    s.label_source = statics.cwd;
    s.data_file = Some(path);
    s
}

/// All Codex rollout sessions, newest file first.
pub fn list() -> Vec<Discovered> {
    rollouts().into_iter().map(discover).collect()
}
