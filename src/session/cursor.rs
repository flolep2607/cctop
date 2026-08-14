//! Cursor native-agent discovery and transcript extraction.
//!
//! Cursor's exported JSONL contains conversation content and tool calls, but
//! no model, token, context, cost, or per-event timestamp data. Keep those
//! fields unavailable rather than estimating them from incomplete evidence.

use super::extract::{for_each_jsonl, push_tool_detail, tool_detail};
use super::{Session, SessionData, Surface};
use crate::config;
use crate::pricing::Provider;
use crate::util;
use rayon::prelude::*;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn created_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.created())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or_else(|| config::file_mtime_ms(path))
}

fn project_slug(path: &Path, roots: &[PathBuf]) -> String {
    path.ancestors()
        .find(|p| {
            p.parent()
                .is_some_and(|parent| roots.iter().any(|root| parent == root))
        })
        .and_then(Path::file_name)
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn summarize(path: PathBuf, roots: &[PathBuf]) -> Option<Session> {
    let session_id = path.file_stem()?.to_string_lossy().into_owned();
    if !config::is_full_uuid(&session_id) {
        return None;
    }
    // Only files in Cursor's documented agent-transcripts subtree are sessions.
    if !path
        .components()
        .any(|c| c.as_os_str() == "agent-transcripts")
    {
        return None;
    }

    let mut session = Session::new(Provider::Cursor, session_id);
    session.surface = Surface::Editor;
    session.harness = "Cursor".into();
    session.started_at = util::ms_to_rfc3339(created_ms(&path) as i64);
    session.last_active = util::ms_to_rfc3339(config::file_mtime_ms(&path) as i64);
    session.label_source = project_slug(&path, roots);
    session.data_file = Some(path);
    session.cost_available = false;
    session.total_cost = None;
    Some(session)
}

pub fn list_sessions() -> Vec<Session> {
    let roots = config::cursor_projects_roots();
    let files: Vec<_> = roots
        .iter()
        .filter(|root| config::dir_exists(root))
        .flat_map(|root| config::rglob(root, ".jsonl"))
        .collect();
    let mut sessions: Vec<_> = files
        .into_par_iter()
        .filter_map(|path| summarize(path, &roots))
        .collect();
    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));

    // Cursor can retain a copied transcript under two project slugs. Its UUID
    // remains the stable identity, so keep the newest copy.
    let mut seen = HashSet::new();
    sessions.retain(|s| seen.insert(s.session_id.clone()));
    sessions
}

pub fn extract(path: &Path) -> SessionData {
    let mut data = SessionData::default();
    let result = for_each_jsonl(path, |item| {
        let Some(content) = item
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        else {
            return;
        };
        for block in content {
            if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                continue;
            }
            let name = block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let input = block.get("input").unwrap_or(&Value::Null);
            let (short, full) = tool_detail(name, input);
            data.metrics.tool_count += 1;
            *data.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
            push_tool_detail(
                &mut data.metrics.tool_details,
                name,
                short,
                full,
                String::new(),
                None,
                None,
            );
        }
    });
    if let Err(err) = result {
        data.error = Some(format!(
            "Could not read Cursor transcript {}: {err}",
            path.display()
        ));
    }
    data
}

pub fn delete(session: &Session) -> std::io::Result<()> {
    if let Some(path) = &session.data_file {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_cursor_tool_calls_without_inventing_usage() {
        // The test thread name is the `::`-separated test path, which is not a
        // legal Windows filename. The pid alone is unique enough here.
        let path = std::env::temp_dir().join(format!("cctop-cursor-{}.jsonl", std::process::id()));
        std::fs::write(
            &path,
            concat!(
                "{\"role\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"},{\"type\":\"tool_use\",\"name\":\"Read\",\"input\":{\"path\":\"src/main.rs\"}}]}}\n",
                "{\"role\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"Read\",\"input\":{}}]}}\n"
            ),
        )
        .unwrap();
        let data = extract(&path);
        let _ = std::fs::remove_file(path);

        assert_eq!(data.metrics.tool_count, 2);
        assert_eq!(data.metrics.tools.get("Read"), Some(&2));
        assert_eq!(data.tokens.total, 0);
        assert_eq!(data.costs.total, 0.0);
        assert!(data.last_model.is_empty());
    }
}
