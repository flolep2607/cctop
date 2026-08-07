//! Windsurf Cascade conversation discovery and extraction.
//!
//! Windsurf is a VS Code fork, so it inherits the editor's per-workspace
//! `state.vscdb` — a SQLite file whose `ItemTable` maps a setting key to a JSON
//! blob. Cascade parks its conversations in one of those blobs as a list of
//! tabs, each holding the bubbles of one conversation.
//!
//! What that blob does *not* hold is any accounting: no model, no token counts,
//! no cost, no per-message timestamp. Windsurf bills in credits server-side and
//! keeps none of it locally, so these sessions report `cost_available = false`
//! and leave the money columns blank rather than estimating a figure the editor
//! never wrote down.
//!
//! ponytail: written against the documented `ItemTable` layout rather than a
//! Windsurf install, so the settings-key list and the bubble field names below
//! are the part that could be wrong. Both fail closed — an unrecognised key
//! yields no rows, an unrecognised bubble yields no tool calls — so a wrong
//! guess costs visibility, never a wrong number. Add the real key to
//! `CHAT_DATA_KEYS` once someone can read it off a live install.

use super::{Session, SessionData, Surface};
use crate::config;
use crate::pricing::Provider;
use crate::util;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
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

fn readonly(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}

/// The Cascade blob from one workspace database, already parsed.
///
/// The stored type is taken as it comes: VS Code writes some `ItemTable` entries
/// as BLOB and others as TEXT, and asking rusqlite for either concrete type
/// fails outright on the other.
pub(super) fn chat_data(db: &Connection) -> Option<Value> {
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

pub(super) fn tabs(data: &Value) -> &[Value] {
    data.get("tabs")
        .and_then(Value::as_array)
        .map_or(&[], |t| t)
}

pub(super) fn tab_id(tab: &Value) -> Option<String> {
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

/// Undo the URI escaping VS Code applies to workspace paths.
///
/// Only `%XX` needs undoing and only a handful of bytes are ever escaped, so a
/// scan is the whole job — a URI crate would be a dependency for six lines.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| u8::from_str_radix(&s[i + 1..i + 3], 16).ok())
            .flatten();
        match decoded {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn databases() -> Vec<PathBuf> {
    config::list_dir(&config::WINDSURF_WORKSPACE_STORAGE)
        .into_iter()
        .map(|entry| config::WINDSURF_WORKSPACE_STORAGE.join(entry))
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
pub fn list_sessions() -> Vec<Session> {
    if !config::dir_exists(&config::WINDSURF_WORKSPACE_STORAGE) {
        return Vec::new();
    }
    let mut sessions = Vec::new();
    for path in databases() {
        let Ok(db) = readonly(&path) else { continue };
        let Some(data) = chat_data(&db) else { continue };
        let touched = util::ms_to_rfc3339(config::file_mtime_ms(&path) as i64);
        let dir = path.parent().map(workspace_dir).unwrap_or_default();

        for tab in tabs(&data) {
            let Some(id) = tab_id(tab) else { continue };
            let mut session = Session::new(Provider::Windsurf, id);
            session.surface = Surface::Editor;
            session.harness = "Windsurf".into();
            session.started_at = touched.clone();
            session.last_active = touched.clone();
            session.label_source = dir.clone();
            session.title = tab
                .get("chatTitle")
                .and_then(Value::as_str)
                .filter(|title| !title.is_empty())
                .map(str::to_string);
            session.data_file = Some(path.clone());
            // Nothing in the blob is denominated in tokens or dollars.
            session.cost_available = false;
            session.total_cost = None;
            sessions.push(session);
        }
    }
    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    sessions
}

/// Tool invocations recorded on one conversation bubble.
///
/// Cascade has used more than one name for the field across releases, and a
/// call's name has appeared as both `name` and `toolName`, so accept either
/// rather than silently reporting zero tools on the shape we did not pick.
fn bubble_tool_calls(bubble: &Value) -> impl Iterator<Item = &str> {
    ["toolCalls", "tool_calls"]
        .into_iter()
        .filter_map(|key| bubble.get(key))
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|call| {
            call.get("name")
                .or_else(|| call.get("toolName"))
                .and_then(Value::as_str)
        })
}

/// Count one conversation's tool activity.
///
/// Tool arguments are not part of the documented bubble shape, so details carry
/// the tool name alone; a fabricated argument line would read as though the
/// transcript recorded one.
pub fn extract(path: &Path, session_id: &str) -> SessionData {
    let mut data = SessionData::default();
    let Ok(db) = readonly(path) else {
        data.error = Some(format!(
            "Could not open Windsurf workspace state {}",
            path.display()
        ));
        return data;
    };
    let Some(chat) = chat_data(&db) else {
        return data;
    };
    let Some(tab) = tabs(&chat)
        .iter()
        .find(|tab| tab_id(tab).as_deref() == Some(session_id))
    else {
        return data;
    };

    data.title = tab
        .get("chatTitle")
        .and_then(Value::as_str)
        .filter(|title| !title.is_empty())
        .map(str::to_string);

    for bubble in tab
        .get("bubbles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for name in bubble_tool_calls(bubble) {
            super::extract::push_tool_detail(
                &mut data.metrics.tool_details,
                name,
                String::new(),
                None,
                String::new(),
                None,
                None,
            );
            *data.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
            data.metrics.tool_count += 1;
        }
    }
    data
}

/// Refuse, and say so.
///
/// Deleting one conversation means rewriting a settings blob inside the editor's
/// own live database, which is not a thing to do behind a running Windsurf.
/// Reporting the refusal beats returning success and leaving the row in place.
pub fn delete(_session: &Session) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Windsurf conversations live inside the editor's workspace database; delete them from Windsurf",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, chat: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "cctop-windsurf-{}-{name}.vscdb",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let db = Connection::open(&path).expect("create fixture db");
        db.execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB)")
            .expect("create ItemTable");
        db.execute(
            "INSERT INTO ItemTable VALUES ('cascade.chatdata', ?1)",
            [chat],
        )
        .expect("insert chat data");
        path
    }

    #[test]
    fn counts_tool_calls_without_inventing_usage() {
        let path = fixture(
            "tools",
            concat!(
                r#"{"tabs":[{"tabId":"tab-1","chatTitle":"Refactor the parser","bubbles":["#,
                r#"{"type":"user","text":"go"},"#,
                r#"{"type":"ai","text":"on it","toolCalls":[{"name":"read_file"},{"toolName":"edit_file"}]},"#,
                r#"{"type":"ai","text":"done","tool_calls":[{"name":"read_file"}]}"#,
                r#"]},{"tabId":"tab-2","bubbles":[{"type":"ai","toolCalls":[{"name":"run_command"}]}]}]}"#,
            ),
        );

        let data = extract(&path, "tab-1");
        std::fs::remove_file(&path).expect("remove fixture");

        assert_eq!(data.title.as_deref(), Some("Refactor the parser"));
        assert_eq!(data.metrics.tool_count, 3, "tab-2's call belongs to tab-2");
        assert_eq!(data.metrics.tools.get("read_file"), Some(&2));
        assert_eq!(data.metrics.tools.get("edit_file"), Some(&1));
        // Windsurf records no accounting, and none may be conjured from tools.
        assert_eq!(data.tokens.total, 0);
        assert_eq!(data.costs.total, 0.0);
        assert!(data.last_model.is_empty());
    }

    #[test]
    fn an_unknown_tab_yields_nothing_rather_than_another_tabs_data() {
        let path = fixture("missing", r#"{"tabs":[{"tabId":"tab-1","bubbles":[]}]}"#);
        let data = extract(&path, "tab-999");
        std::fs::remove_file(&path).expect("remove fixture");
        assert_eq!(data.metrics.tool_count, 0);
        assert!(data.title.is_none());
        assert!(data.error.is_none());
    }

    #[test]
    fn decodes_escaped_workspace_paths() {
        assert_eq!(percent_decode("/home/flo/my%20work"), "/home/flo/my work");
        assert_eq!(percent_decode("/plain/path"), "/plain/path");
        // A stray `%` is data, not the start of an escape.
        assert_eq!(percent_decode("/100%"), "/100%");
    }
}
