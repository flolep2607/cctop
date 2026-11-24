//! Where Gemini CLI keeps its chats, and the header facts of each. Reading the
//! turns stays in cctop-core's `session::gemini`.

use super::gemini_line::Record;
use super::{Discovered, last_entry_ms};
use crate::time::{ms_to_rfc3339, parse_ts};
use crate::{Provider, Surface, paths};
use std::path::{Path, PathBuf};

/// Start time taken from the filename.
///
/// Gemini names a chat for the UTC minute it began
/// (`session-2026-05-14T17-34-79709c93`). Minute precision is all the table
/// sorts on, and reading it here keeps discovery from parsing every transcript
/// on disk — the JSON shape has no header to stop at, so the alternative is
/// deserialising multi-megabyte conversations just to learn when they started.
fn started_from_stem(stem: &str) -> Option<String> {
    let (date, clock) = stem.strip_prefix("session-")?.split_once('T')?;
    let mut parts = clock.split('-');
    let (hour, minute) = (parts.next()?, parts.next()?);
    if date.len() != 10 || hour.len() != 2 || minute.len() != 2 {
        return None;
    }
    Some(format!("{date}T{hour}:{minute}:00Z"))
}

/// Every `chats/session-*.json{,l}` under the chat root.
///
/// Descending only into the two directory levels Gemini actually uses avoids
/// walking `tool-outputs/`, which holds a file per tool call and dwarfs the
/// transcripts it sits beside.
pub fn chat_files() -> Vec<PathBuf> {
    paths::gemini_chats_roots()
        .into_iter()
        .flat_map(|root| {
            paths::list_dir(&root)
                .into_iter()
                .map(move |project| root.join(project).join("chats"))
        })
        .filter(|dir| paths::dir_exists(dir))
        .flat_map(|dir| {
            paths::list_dir(&dir)
                .into_iter()
                .filter(|name| {
                    name.starts_with("session-")
                        && (name.ends_with(".json") || name.ends_with(".jsonl"))
                })
                .map(move |name| dir.join(name))
        })
        .collect()
}

/// Working directory the project subtree was recorded for.
///
/// Gemini writes the launch directory to `.project_root` beside `chats/`. Older
/// subtrees are named for a hash of that path with no `.project_root` to undo
/// it, and a hash is not a directory, so those keep an empty label rather than
/// showing something that looks like a path but isn't.
fn project_root(chats_dir: &Path) -> String {
    chats_dir
        .parent()
        .map(|project| project.join(".project_root"))
        .and_then(|marker| std::fs::read_to_string(marker).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// The newest message's time in a chat file, in Unix milliseconds.
///
/// Only the JSONL shape answers: its messages are lines, and the newest is
/// near the end, beside the `$set` patches that revise the header and are not
/// a message. ponytail: the older single-object shape would have to be
/// deserialised whole to reach its last message — the cost
/// [`started_from_stem`] exists to avoid — so it keeps its mtime. Nothing
/// writes that shape any more, so the file is not touched after the fact.
pub fn last_turn_ms(path: &Path) -> Option<i64> {
    if path.extension().is_none_or(|ext| ext != "jsonl") {
        return None;
    }
    last_entry_ms(path, |bytes| {
        let record = Record::parse(bytes)?;
        if !record.kind.is("user") && !record.kind.is("gemini") {
            return None;
        }
        parse_ts(record.timestamp.get()?).map(|d| d.timestamp_millis())
    })
}

pub fn summarize(path: PathBuf) -> Option<Discovered> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let started = started_from_stem(&stem)?;

    // Resuming a chat opens a new file under the same `sessionId`, and the
    // segments are disjoint rather than cumulative. Keying rows on that id
    // would collapse them onto each other and lose every earlier segment's
    // usage, so the filename — unique, and what Gemini itself indexes by — is
    // the identity here.
    let mut session = Discovered::new(Provider::Gemini, stem);
    session.surface = Surface::Cli;
    session.started_at = started;
    let touched = paths::file_mtime_ms(&path) as i64;
    session.touched_at = ms_to_rfc3339(touched);
    session.last_active = ms_to_rfc3339(last_turn_ms(&path).unwrap_or(touched));
    session.label_source = project_root(path.parent()?);
    session.data_file = Some(path);
    Some(session)
}

/// Every Gemini chat on the machine, newest first.
pub fn list() -> Vec<Discovered> {
    let mut sessions: Vec<_> = chat_files().into_iter().filter_map(summarize).collect();
    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    sessions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_start_time_from_the_chat_filename() {
        assert_eq!(
            started_from_stem("session-2026-05-14T17-34-79709c93").as_deref(),
            Some("2026-05-14T17:34:00Z")
        );
        assert_eq!(started_from_stem("logs"), None);
        assert_eq!(started_from_stem("session-nonsense"), None);
    }
}
