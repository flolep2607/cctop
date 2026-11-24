//! Windsurf Cascade conversations, found in the editor's per-workspace
//! `state.vscdb`: a SQLite file whose `ItemTable` maps a setting key to a JSON
//! blob, one of which holds Cascade's tabs. The extraction of tool calls from
//! those tabs stays in cctop-core.
//!
//! ponytail: written against the documented `ItemTable` layout rather than a
//! Windsurf install, so `CHAT_DATA_KEYS` is the part that could be wrong. It
//! fails closed — an unrecognised key yields no rows.

use super::{Discovered, readonly};
use crate::{Provider, Surface, paths};
use rusqlite::Connection;
use rusqlite::types::ValueRef;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Settings keys Cascade has been observed to store its conversations under.
/// Tried in order; the first that parses into tabs wins.
const CHAT_DATA_KEYS: [&str; 4] = [
    "cascade.chatdata",
    "workbench.panel.aichat.view.aichat.chatdata",
    "aiChat.chatdata",
    "chat.data",
];

/// The Cascade blob from one workspace database, already parsed.
///
/// The stored type is taken as it comes: VS Code writes some `ItemTable` entries
/// as BLOB and others as TEXT, and asking rusqlite for either concrete type
/// fails outright on the other.
pub fn chat_data(db: &Connection) -> Option<Value> {
    let mut stmt = db
        .prepare("SELECT value FROM ItemTable WHERE key = ?1")
        .ok()?;
    CHAT_DATA_KEYS.iter().find_map(|key| {
        let raw = stmt
            .query_row([key], |row| match row.get_ref(0)? {
                ValueRef::Text(bytes) | ValueRef::Blob(bytes) => Ok(bytes.to_vec()),
                _ => Ok(Vec::new()),
            })
            .ok()?;
        let value: Value = serde_json::from_slice(&raw).ok()?;
        value
            .get("tabs")
            .and_then(Value::as_array)
            .is_some()
            .then_some(value)
    })
}

pub fn tabs(data: &Value) -> &[Value] {
    data.get("tabs")
        .and_then(Value::as_array)
        .map_or(&[], |t| t)
}

pub fn tab_id(tab: &Value) -> Option<String> {
    tab.get("tabId")
        .and_then(|id| {
            id.as_str()
                .map(str::to_string)
                .or_else(|| id.as_u64().map(|n| n.to_string()))
        })
        .filter(|id| !id.is_empty())
}

/// Working directory of a workspace storage entry.
///
/// VS Code records it beside the database as a `file://` URI. Only local paths
/// are resolvable, so a remote or virtual workspace keeps an empty label.
fn workspace_dir(storage_dir: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(storage_dir.join("workspace.json")) else {
        return String::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return String::new();
    };
    value
        .get("folder")
        .and_then(Value::as_str)
        .and_then(|uri| uri.strip_prefix("file://"))
        .map(percent_decode)
        .unwrap_or_default()
}

/// Undo the URI escaping VS Code applies to workspace paths. A `%` that
/// starts no escape stays as it is, and `+` is a plus: this is a path.
fn percent_decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

fn databases() -> Vec<PathBuf> {
    paths::windsurf_workspace_roots()
        .into_iter()
        .flat_map(|root| {
            paths::list_dir(&root)
                .into_iter()
                .map(move |entry| root.join(entry))
        })
        .filter(|dir| dir.is_dir())
        .map(|dir| dir.join("state.vscdb"))
        .filter(|db| db.is_file())
        .collect()
}

/// One row per Cascade conversation.
///
/// Windsurf stamps neither a start nor an end on a conversation, so both times
/// come from the workspace database's mtime. That is honest about ordering —
/// the most recently used workspace really did change last — and deliberately
/// wrong about nothing else: every tab in a workspace shares the timestamp
/// because the file is all the evidence there is.
pub fn list() -> Vec<Discovered> {
    let mut sessions = Vec::new();
    for path in databases() {
        let Ok(db) = readonly(&path) else { continue };
        let Some(data) = chat_data(&db) else { continue };
        let touched = crate::time::ms_to_rfc3339(paths::file_mtime_ms(&path) as i64);
        let dir = path.parent().map(workspace_dir).unwrap_or_default();

        for tab in tabs(&data) {
            let Some(id) = tab_id(tab) else { continue };
            let mut session = Discovered::new(Provider::Windsurf, id);
            session.surface = Surface::Editor;
            session.started_at = touched.clone();
            session.last_active = touched.clone();
            session.label_source = dir.clone();
            session.title = tab
                .get("chatTitle")
                .and_then(Value::as_str)
                .filter(|title| !title.is_empty())
                .map(str::to_string);
            session.data_file = Some(path.clone());
            sessions.push(session);
        }
    }
    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    sessions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_escaped_workspace_paths() {
        assert_eq!(percent_decode("/home/flo/my%20work"), "/home/flo/my work");
        assert_eq!(percent_decode("/plain/path"), "/plain/path");
        // A stray `%` is data, not the start of an escape.
        assert_eq!(percent_decode("/100%"), "/100%");
    }

    /// Regression: the two bytes after a `%` were sliced out of the `str`, so a
    /// `%` followed by a multi-byte character cut it in half and panicked the
    /// discovery walk. A URI VS Code wrote never has one; a hand-edited
    /// `workspace.json` can.
    #[test]
    fn a_percent_before_a_multibyte_character_is_data() {
        assert_eq!(percent_decode("/x/%€"), "/x/%€");
        assert_eq!(percent_decode("/x/%a€"), "/x/%a€");
    }
}
