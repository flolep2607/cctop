//! Cursor native-agent discovery and transcript extraction.
//!
//! Cursor's exported JSONL contains conversation content and tool calls, but
//! no model, token, context, cost, tool outcome or per-event timestamp data.
//! Keep those fields unavailable rather than estimating them from incomplete
//! evidence; what the calls themselves say — the tools, the lines an edit
//! changed, the pages and searches — is read through [`super::cursor_line`].

use super::cursor_line;
use super::extract::{edit_delta, push_tool_detail, tool_detail};
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

/// Whether a tool, by its name, writes files — the calls whose arguments are
/// read for the lines they changed.
fn edits(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    ["edit", "write", "replace"]
        .iter()
        .any(|verb| name.contains(verb))
}

/// The tools, their arguments, and what those say: the lines an edit changed,
/// the pages fetched, the searches run.
///
/// ponytail: the export records no tool result, so no call is ever known to
/// have failed and the error rate is `─` rather than `0%`; see
/// [`Provider::records_tool_outcomes`].
pub fn extract(path: &Path) -> SessionData {
    let mut data = SessionData::default();
    let mut seen_urls = HashSet::new();
    let mut seen_queries = HashSet::new();
    let result = cursor_line::for_each(path, |line| {
        for call in line.calls() {
            let name = call.name.get().unwrap_or("unknown");
            let input = call.input.value();
            let (short, full) = tool_detail(name, &input);
            let m = &mut data.metrics;
            m.tool_count += 1;
            *m.tools.entry(name.to_string()).or_insert(0) += 1;
            push_tool_detail(
                &mut m.tool_details,
                name,
                short,
                full,
                String::new(),
                None,
                None,
            );
            // The snake and camel spellings `edit_delta` reads are the ones
            // every harness that names its edits this way has used; arguments
            // in any other shape add nothing rather than a guess.
            if edits(name)
                && let Some(delta) = edit_delta(&input)
            {
                m.lines_added += u64::from(delta.added);
                m.lines_removed += u64::from(delta.removed);
                if let Some(detail) = m.tool_details.get_mut(name).and_then(|l| l.last_mut()) {
                    detail.delta = Some(delta);
                }
            }
            let arg = |key: &str| {
                input
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
            };
            match name.to_ascii_lowercase().replace('_', "").as_str() {
                "webfetch" => {
                    if let Some(url) = arg("url")
                        && seen_urls.len() < 50
                        && seen_urls.insert(url.to_string())
                    {
                        m.web_fetches.push(url.to_string());
                        m.web_fetch_count += 1;
                    }
                }
                "websearch" => {
                    if let Some(q) = arg("query").or_else(|| arg("search_term"))
                        && seen_queries.len() < 50
                        && seen_queries.insert(q.to_string())
                    {
                        m.web_searches.push(q.to_string());
                        m.web_search_count += 1;
                    }
                }
                _ => {}
            }
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
        // The pid alone is unique enough here.
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

    /// Hand-built in the export's shape, with made-up content: what the calls'
    /// own arguments say is counted, and a line of an unexpected shape is
    /// passed over without losing the rest.
    #[test]
    fn edits_fetches_and_searches_come_from_the_arguments() {
        let path = std::env::temp_dir().join(format!(
            "cctop-cursor-args-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            concat!(
                r#"{"role":"user","message":{"content":[{"type":"text","text":"go"}]}}"#,
                "\n",
                r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"StrReplace","input":{"path":"a.rs","old_string":"x","new_string":"y\nz"}},{"type":"tool_use","name":"WebFetch","input":{"url":"https://example.invalid"}},{"type":"tool_use","name":"WebSearch","input":{"search_term":"rust"}}]}}"#,
                "\n",
                r#"{"role":"assistant","message":"not blocks","unknown":{"a":1}}"#,
                "\n",
                r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"path":"a.rs","content":"not an edit"}}]}}"#,
                "\n",
            ),
        )
        .unwrap();
        let data = extract(&path);
        let _ = std::fs::remove_file(path);

        let m = &data.metrics;
        assert_eq!(m.tool_count, 4);
        assert_eq!(
            (m.lines_added, m.lines_removed),
            (2, 1),
            "a read is not an edit"
        );
        assert_eq!(m.web_fetches, ["https://example.invalid"]);
        assert_eq!(m.web_searches, ["rust"]);
        assert!(m.tool_details["StrReplace"][0].delta.is_some());
        assert_eq!(m.tool_errors, 0);
        assert!(!Provider::Cursor.records_tool_outcomes());
    }
}
