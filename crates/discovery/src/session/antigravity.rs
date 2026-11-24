//! Where Antigravity CLI (`agy`) keeps its conversations, and which of them
//! are top-level rows. See cctop-core's `session::antigravity` for the format;
//! the reading of the transcripts stays there.

use super::{Discovered, readonly};
use crate::{Provider, Surface, paths};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::io::BufRead;
use std::path::{Path, PathBuf};

pub const SUMMARIES_DB: &str = "conversation_summaries.db";

pub fn transcript_in(app_dir: &Path, id: &str) -> PathBuf {
    app_dir
        .join("brain")
        .join(id)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl")
}

/// `<app>/brain/<id>/.system_generated/logs/transcript.jsonl` → (`<app>`, `<id>`).
pub fn locate(transcript: &Path) -> Option<(&Path, String)> {
    let conversation = transcript.ancestors().nth(3)?;
    let id = conversation.file_name()?.to_string_lossy().into_owned();
    Some((conversation.parent()?.parent()?, id))
}

/// One row of `conversation_summaries`.
#[derive(Default, Clone)]
pub struct Summary {
    pub title: String,
    pub workspace: String,
    pub parent: String,
    pub agent: String,
    pub idle: bool,
}

/// Every conversation the index knows, by id. An index that will not open
/// leaves the map empty: the transcripts are still listed, without a title,
/// a directory or a parent.
pub fn summaries(app_dir: &Path) -> HashMap<String, Summary> {
    let Ok(db) = readonly(&app_dir.join(SUMMARIES_DB)) else {
        return HashMap::new();
    };
    let Ok(mut stmt) = db.prepare(
        "SELECT conversation_id, title, workspace_uris, parent_conversation_id, \
         agent_name, status FROM conversation_summaries",
    ) else {
        return HashMap::new();
    };
    let rows = stmt.query_map([], |row| {
        let uris: String = row.get(2)?;
        let status: String = row.get(5)?;
        Ok((
            row.get::<_, String>(0)?,
            Summary {
                title: row.get(1)?,
                workspace: first_workspace(&uris),
                parent: row.get(3)?,
                agent: row.get(4)?,
                idle: status.is_empty() || status.ends_with("_IDLE"),
            },
        ))
    });
    rows.map(|rows| rows.flatten().collect())
        .unwrap_or_default()
}

/// The first local path in a JSON list of `file://` URIs.
pub fn first_workspace(uris: &str) -> String {
    serde_json::from_str::<Vec<String>>(uris)
        .unwrap_or_default()
        .iter()
        .find_map(|uri| uri.strip_prefix("file://"))
        .map(|path| {
            percent_encoding::percent_decode_str(path)
                .decode_utf8_lossy()
                .into_owned()
        })
        .unwrap_or_default()
}

/// Every conversation under `root` in the index, children before grandchildren.
pub fn descendants(index: &HashMap<String, Summary>, root: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut queue = VecDeque::from([root.to_string()]);
    while let Some(parent) = queue.pop_front() {
        let mut children: Vec<_> = index
            .iter()
            .filter(|(id, s)| s.parent == parent && !found.contains(*id) && *id != root)
            .map(|(id, _)| id.clone())
            .collect();
        children.sort();
        queue.extend(children.iter().cloned());
        found.extend(children);
    }
    found
}

/// When a transcript's first step was written.
pub fn first_step_at(path: &Path) -> Option<String> {
    let mut line = String::new();
    std::io::BufReader::new(std::fs::File::open(path).ok()?)
        .read_line(&mut line)
        .ok()?;
    let step: Value = serde_json::from_str(&line).ok()?;
    Some(step.get("created_at")?.as_str()?.to_string())
}

pub fn list() -> Vec<Discovered> {
    let mut sessions = Vec::new();
    for app_dir in paths::antigravity_dirs() {
        let index = summaries(&app_dir);
        for id in paths::list_dir(&app_dir.join("brain")) {
            let summary = index.get(&id).cloned().unwrap_or_default();
            if !summary.parent.is_empty() {
                continue;
            }
            let path = transcript_in(&app_dir, &id);
            if !path.is_file() {
                continue;
            }
            // The row's figures include its subagents', so it is as fresh as
            // the newest of their transcripts — that is what the cache keys on.
            let touched = descendants(&index, &id)
                .iter()
                .map(|child| paths::file_mtime_ms(&transcript_in(&app_dir, child)))
                .chain([paths::file_mtime_ms(&path)])
                .max()
                .unwrap_or(0) as i64;
            let mut session = Discovered::new(Provider::Antigravity, id);
            session.surface = Surface::Cli;
            session.last_active = crate::time::ms_to_rfc3339(touched);
            session.touched_at = session.last_active.clone();
            session.started_at =
                first_step_at(&path).unwrap_or_else(|| session.last_active.clone());
            session.label_source = summary.workspace;
            session.title = Some(summary.title).filter(|t| !t.is_empty());
            session.data_file = Some(path);
            sessions.push(session);
        }
    }
    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    sessions
}
