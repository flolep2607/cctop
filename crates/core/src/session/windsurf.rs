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

use super::{Session, SessionData, readonly};
// `pub(crate)`: search.rs reads the same tabs.
use cctop_discovery::session::windsurf;
pub(crate) use cctop_discovery::session::windsurf::{chat_data, tab_id, tabs};
use serde_json::Value;
use std::path::Path;

/// The rows Windsurf's workspace databases hold; found in `cctop-discovery`,
/// annotated here. Nothing in the blob is denominated in tokens or dollars.
pub fn list_sessions() -> Vec<Session> {
    windsurf::list()
        .into_iter()
        .map(|found| {
            let mut session = Session::from_found(found);
            session.cost_available = false;
            session.total_cost = None;
            session
        })
        .collect()
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
    use rusqlite::Connection;
    use std::path::PathBuf;

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
}
