//! Where Claude Code keeps its transcripts, and what each one's head and tail
//! say about the session — the CLI's under each projects root, and Claude for
//! Mac's under its own. Reading the turns stays in cctop-core's
//! `session::claude`.

use super::claude_line::Line;
use super::{
    Discovered, MacMeta, last_entry_ms, scan_first_lines, scan_last_lines, transcript_files,
};
use crate::time::{ms_to_rfc3339, parse_ts};
use crate::{Provider, Surface, paths};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// Fields that are written in the first few lines and never change.
#[derive(Debug, Clone, Default)]
struct StaticParts {
    model: String,
    cwd: String,
    started_at: String,
    ai_title: Option<String>,
    /// Whether an `/effort` switch is among the first lines, before any
    /// model has answered. See `summarize`.
    switched_effort: bool,
    /// The id this session was *launched* with, which is not its own once it
    /// has been resumed. See [`Discovered::launch_id`].
    launch_id: String,
}

static STATIC_CACHE: LazyLock<Mutex<HashMap<PathBuf, StaticParts>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn collect_static(transcript: &Path) -> StaticParts {
    if let Ok(cache) = STATIC_CACHE.lock()
        && let Some(hit) = cache.get(transcript)
    {
        return hit.clone();
    }

    let mut parts = StaticParts::default();
    let mut earliest: Option<chrono::DateTime<chrono::Utc>> = None;

    scan_first_lines(transcript, 50, |bytes| {
        let Some(item) = Line::parse(bytes) else {
            return false;
        };
        if let Some(ts) = item.timestamp.get()
            && let Some(parsed) = parse_ts(ts)
            && earliest.is_none_or(|e| parsed < e)
        {
            earliest = Some(parsed);
        }
        if parts.cwd.is_empty()
            && let Some(cwd) = item.cwd.get()
        {
            parts.cwd = cwd.to_string();
        }
        if parts.model.is_empty()
            && item.kind.is("assistant")
            && let Some(m) = item.message.get().and_then(|m| m.model.get())
            && m != "<synthetic>"
        {
            parts.model = m.to_string();
        }
        // `sessionId` is this transcript's own id and `session_id` is the one
        // on the command line that started it — the same thing until a resume,
        // and the only record of the fork afterwards.
        if parts.launch_id.is_empty()
            && let Some(id) = item.session_id.get()
        {
            parts.launch_id = id.to_string();
        }
        if item.kind.is("user")
            && item
                .content()
                .text()
                .and_then(super::effort_switch)
                .is_some()
        {
            parts.switched_effort = true;
        }
        if item.kind.is("ai-title")
            && let Some(t) = item.ai_title.get()
        {
            parts.ai_title = Some(t.to_string());
        }
        true
    });

    parts.started_at = earliest.map(|d| d.to_rfc3339()).unwrap_or_default();

    // Only cache once a model is known; an empty session may still be filling in.
    if !parts.model.is_empty()
        && let Ok(mut cache) = STATIC_CACHE.lock()
    {
        cache.insert(transcript.to_path_buf(), parts.clone());
    }
    parts
}

/// A `/rename` title, which is usually written late in the session.
fn scan_custom_title(transcript: &Path) -> Option<String> {
    let mut title = None;
    scan_last_lines(transcript, 200, |bytes| {
        let Some(item) = Line::parse(bytes) else {
            return false;
        };
        if title.is_none() && item.kind.is("custom-title") {
            title = item.custom_title.get().map(str::to_string);
        }
        true
    });
    title
}

/// When a transcript line is a turn — something said, or a tool called or
/// answered — its time in Unix milliseconds.
///
/// Every other kind of line is metadata a session writes about itself: a
/// title, a summary, a file-history snapshot, a permission-mode switch, hook
/// progress. Claude Code writes several of those on a resume before anything
/// is asked, and a row whose "last" moved for one of them claimed activity
/// that never happened. `isMeta` user lines are the harness talking to
/// itself (a local command's caveat, an injected skill) and are left out on
/// the same grounds.
fn turn_ms(bytes: &[u8]) -> Option<i64> {
    let item = Line::parse(bytes)?;
    if !(item.kind.is("user") || item.kind.is("assistant")) || item.meta() {
        return None;
    }
    parse_ts(item.timestamp.get()?).map(|d| d.timestamp_millis())
}

/// The newest turn across the main transcript and any subagent transcripts,
/// and the newest mtime across the same files, as RFC 3339.
///
/// The turn is what the row shows as "last"; the mtime is any write at all,
/// which is what liveness wants. A transcript with no turn in its tail
/// answers with its mtime, so a session is never dated earlier than it was
/// when this read the files alone.
fn latest_activity(transcript: &Path) -> (String, String) {
    let files = transcript_files(transcript);
    let touched = files
        .iter()
        .map(|p| paths::file_mtime_ms(p))
        .max()
        .unwrap_or(0);
    let turn = files.iter().filter_map(|p| last_entry_ms(p, turn_ms)).max();
    (
        ms_to_rfc3339(turn.unwrap_or(touched as i64)),
        ms_to_rfc3339(touched as i64),
    )
}

/// The row one CLI transcript makes, or `None` for one with nothing to show.
pub fn summarize(transcript: &Path) -> Option<Discovered> {
    let session_id = transcript.file_stem()?.to_string_lossy().to_string();
    let statics = collect_static(transcript);
    // An abandoned session that never reached the model has nothing to show.
    // Except one switched to or from ultracode, whose switch starts or ends
    // the dashboard's party whether or not anything has been asked yet: see
    // `ui::rave`. It is listed without a model, and the process running it
    // can be matched to it rather than shown as a row of its own.
    //
    // ponytail: only a switch among the first lines is seen before the first
    // answer. A session with nothing in it but slash commands is short, and
    // once the model answers every line is read anyway.
    if statics.model.is_empty() && !statics.switched_effort {
        return None;
    }
    let custom_title = scan_custom_title(transcript);
    let (last_active, touched_at) = latest_activity(transcript);

    let mut s = Discovered::new(Provider::Claude, session_id);
    s.started_at = statics.started_at.clone();
    s.last_active = if last_active.is_empty() {
        statics.started_at
    } else {
        last_active
    };
    s.touched_at = touched_at;
    s.model = statics.model;
    s.label_source = statics.cwd;
    s.launch_id = statics.launch_id;
    s.data_file = Some(transcript.to_path_buf());
    s.title = custom_title.or(statics.ai_title);
    Some(s)
}

/// Every CLI transcript under each Claude projects root.
pub fn transcripts() -> Vec<PathBuf> {
    let mut transcripts = Vec::new();
    for root in paths::claude_projects_roots() {
        if !paths::dir_exists(&root) {
            continue;
        }
        for project in paths::list_dir(&root) {
            let project_dir = root.join(&project);
            if !project_dir.is_dir() {
                continue;
            }
            for entry in paths::list_dir(&project_dir) {
                let Some(stem) = entry.strip_suffix(".jsonl") else {
                    continue;
                };
                if !paths::is_full_uuid(stem) {
                    continue;
                }
                transcripts.push(project_dir.join(entry));
            }
        }
    }

    transcripts
}

/// All Claude sessions: CLI transcripts plus any Claude for Mac sessions.
pub fn list() -> Vec<Discovered> {
    let mut sessions: Vec<_> = transcripts().iter().filter_map(|p| summarize(p)).collect();
    sessions.extend(list_mac());
    sessions
}

/// Locate the Claude Code transcript backing a desktop session.
fn find_desktop_jsonl(session_dir: &Path, cli_session_id: &str) -> Option<PathBuf> {
    let projects_dir = session_dir.join(".claude").join("projects");
    if !projects_dir.is_dir() {
        return None;
    }
    for project in paths::list_dir(&projects_dir) {
        let project_dir = projects_dir.join(&project);
        if !project_dir.is_dir() {
            continue;
        }
        let exact = project_dir.join(format!("{cli_session_id}.jsonl"));
        if exact.is_file() {
            return Some(exact);
        }
        // The CLI session id can diverge from the filename in edge cases.
        for entry in paths::list_dir(&project_dir) {
            if entry.ends_with(".jsonl") && entry.starts_with(cli_session_id) {
                return Some(project_dir.join(entry));
            }
        }
    }
    None
}

/// Sessions from Claude for Mac (both Code and Cowork surfaces).
pub fn list_mac() -> Vec<Discovered> {
    let mut sessions = Vec::new();
    let surfaces: [(&Option<PathBuf>, &str, Option<Surface>); 2] = [
        (
            &paths::CLAUDE_MAC_COWORK_ROOT,
            "local-agent-mode-sessions",
            None,
        ),
        (
            &paths::CLAUDE_MAC_CODE_ROOT,
            "claude-code-sessions",
            Some(Surface::DesktopCode),
        ),
    ];
    for (primary, leaf, forced) in surfaces {
        for root in paths::claude_mac_roots(primary, leaf) {
            if paths::dir_exists(&root) {
                scan_mac_root(&root, forced, &mut sessions);
            }
        }
    }
    sessions
}

fn scan_mac_root(root: &Path, forced_surface: Option<Surface>, out: &mut Vec<Discovered>) {
    for account in paths::list_dir(root) {
        if account == "skills-plugin" {
            continue;
        }
        let account_dir = root.join(&account);
        if !account_dir.is_dir() {
            continue;
        }
        for device in paths::list_dir(&account_dir) {
            let device_dir = account_dir.join(&device);
            if !device_dir.is_dir() {
                continue;
            }
            for entry in paths::list_dir(&device_dir) {
                if !entry.starts_with("local_") || !entry.ends_with(".json") {
                    continue;
                }
                let meta_path = device_dir.join(&entry);
                let Ok(text) = std::fs::read_to_string(&meta_path) else {
                    continue;
                };
                let Ok(meta) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if meta.get("isArchived").and_then(Value::as_bool) == Some(true) {
                    continue;
                }
                let Some(cli_session_id) = meta.get("cliSessionId").and_then(Value::as_str) else {
                    continue;
                };
                let session_dir_name = meta
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| entry.trim_end_matches(".json").to_string());
                let session_dir = device_dir.join(&session_dir_name);
                let Some(jsonl) = find_desktop_jsonl(&session_dir, cli_session_id) else {
                    continue; // no transcript written yet
                };

                let statics = collect_static(&jsonl);
                let meta_model = meta.get("model").and_then(Value::as_str).unwrap_or("");
                if statics.model.is_empty() && meta_model.is_empty() {
                    continue;
                }

                let surface = forced_surface.unwrap_or({
                    if meta.get("vmProcessName").is_some() {
                        Surface::DesktopCowork
                    } else {
                        Surface::DesktopCode
                    }
                });

                let mut s = Discovered::new(Provider::Claude, cli_session_id.to_string());
                s.surface = surface;
                s.started_at = meta
                    .get("createdAt")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                s.last_active = meta
                    .get("lastActivityAt")
                    .and_then(Value::as_str)
                    .unwrap_or(&s.started_at)
                    .to_string();
                s.model = if statics.model.is_empty() {
                    meta_model.to_string()
                } else {
                    statics.model.clone()
                };
                s.label_source = meta
                    .get("userSelectedFolders")
                    .and_then(|v| v.get(0))
                    .and_then(Value::as_str)
                    .or_else(|| meta.get("cwd").and_then(Value::as_str))
                    .unwrap_or_default()
                    .to_string();
                s.title = meta
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| scan_custom_title(&jsonl))
                    .or(statics.ai_title);
                s.data_file = Some(jsonl);
                s.mac_meta = Some(MacMeta {
                    meta_path,
                    session_dir,
                });
                out.push(s);
            }
        }
    }
}
