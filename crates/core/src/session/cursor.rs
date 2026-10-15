//! Cursor native-agent discovery and transcript extraction.
//!
//! A row is a transcript, `<slug>/agent-transcripts/<id>/<id>.jsonl`, whose
//! lines carry the conversation and its tool calls and nothing else: no model,
//! token, context, cost, tool outcome or per-event timestamp. What the calls
//! themselves say — the tools, the lines an edit changed, the pages and
//! searches — is read through [`super::cursor_line`].
//!
//! Cursor's editor keeps more in its global `state.vscdb`, under the same id:
//! a chat's title, workspace and start time in the `composerHeaders` table,
//! its model, effort and context occupancy in `cursorDiskKV` under
//! `composerData:<id>`. That database only ever enriches a transcript's row.
//! The row, its cache key, its deletion and its watch stay the transcript's,
//! so a database cctop cannot find or read leaves every row exactly as the
//! transcript alone makes it. Tokens and cost are not in the database either —
//! every message's `tokenCount` is zero and every `usageData` empty — so they
//! stay unavailable rather than estimated.

use super::cursor_line;
use super::extract::{edit_delta, note_web, push_tool_detail, tool_detail};
use super::{ContextUsage, Session, SessionData, Subagent, SubagentStatus, Surface, readonly};
use crate::config;
use crate::pricing::Provider;
use crate::remote_fs::{self, Mount};
use crate::util;
use rayon::prelude::*;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

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

/// A subagent's transcript, `<parent>/subagents/<id>.jsonl`: part of its
/// parent's session, never one of its own. Only the immediate directory is
/// asked, so a root or home with `subagents` somewhere in its path hides
/// nothing.
fn is_subagent(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .is_some_and(|n| n == "subagents")
}

fn summarize(path: PathBuf, roots: &[PathBuf]) -> Option<Session> {
    let session_id = path.file_stem()?.to_string_lossy().into_owned();
    if !config::is_full_uuid(&session_id) || is_subagent(&path) {
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
    // A subagent writes its own file while the parent's sits still waiting on
    // it, and the row is live for as long as either is being written.
    let touched = super::transcript_files(&path)
        .iter()
        .map(|f| config::file_mtime_ms(f))
        .max()
        .unwrap_or(0);
    session.touched_at = util::ms_to_rfc3339(touched as i64);
    session.label_source = project_slug(&path, roots);
    session.data_file = Some(path);
    session.cost_available = false;
    session.total_cost = None;
    Some(session)
}

pub fn list_sessions() -> Vec<Session> {
    list_sessions_in(
        &config::cursor_projects_roots(),
        &config::cursor_global_dbs(),
    )
}

fn list_sessions_in(roots: &[PathBuf], dbs: &[PathBuf]) -> Vec<Session> {
    // Transcripts live only under each project's `agent-transcripts/`; the
    // rest of a project directory is `mcps/` (up to ~130 files a project) and
    // `canvases/` with whole `node_modules` trees, which a walk of the root
    // would read only to find nothing. Recursive inside, for the
    // `<id>/<id>.jsonl` layout and its `subagents/`.
    let files: Vec<PathBuf> = roots
        .iter()
        .flat_map(|root| {
            config::list_dir(root).into_iter().flat_map(move |slug| {
                config::rglob(&root.join(slug).join("agent-transcripts"), ".jsonl")
            })
        })
        .collect();
    // Cursor sometimes keeps a subagent's transcript at the top level as well,
    // as `<id>/<id>.jsonl`, which only its id gives away.
    let subagents: HashSet<String> = files
        .iter()
        .filter(|f| is_subagent(f))
        .filter_map(|f| f.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    let mut sessions: Vec<_> = files
        .into_par_iter()
        .filter_map(|path| summarize(path, roots))
        .filter(|s| !subagents.contains(&s.session_id))
        .collect();
    sessions.sort_by(|a, b| b.last_active.cmp(&a.last_active));

    // Cursor can retain a copied transcript under two project slugs. Its UUID
    // remains the stable identity, so keep the newest copy.
    let mut seen = HashSet::new();
    sessions.retain(|s| seen.insert(s.session_id.clone()));

    if !sessions.is_empty() {
        let headers = {
            let _span = crate::trace::span("walk.cursor.headers");
            headers_in(dbs)
        };
        for s in &mut sessions {
            if let Some(h) = headers.get(&s.session_id) {
                h.apply(s);
            }
        }
    }
    sessions
}

/// The mounts this process sees, read once: the table changes when someone
/// mounts a drive, which a running cctop can live without noticing.
static MOUNTS: LazyLock<Vec<Mount>> = LazyLock::new(remote_fs::mounts_here);

/// Whether `path` is on a Windows drive seen through WSL — the 9P mount
/// `/mnt/c` is, or WSL 1's drvfs. Asked of the mount table rather than of a
/// `/mnt/` prefix, because `automount.root` can put the drives anywhere.
fn on_windows_drive(path: &Path, mounts: &[Mount]) -> bool {
    mounts
        .iter()
        .filter(|m| path.starts_with(&m.point))
        .max_by_key(|m| m.point.as_os_str().len())
        .is_some_and(|m| matches!(m.fstype.as_str(), "9p" | "v9fs" | "drvfs"))
}

/// `file:` URI for SQLite's `immutable` open. A Windows profile name can hold
/// a space, and a `?` or `#` would end the path early, so those are escaped
/// along with `%` itself.
fn immutable_uri(path: &Path) -> String {
    use percent_encoding::{AsciiSet, CONTROLS, percent_encode};
    const ESCAPE: &AsciiSet = &CONTROLS.add(b' ').add(b'%').add(b'?').add(b'#');
    format!(
        "file:{}?immutable=1",
        percent_encode(path.as_os_str().as_bytes(), ESCAPE)
    )
}

/// Open a Cursor global database for reading.
///
/// Under WSL it usually sits on the Windows drive, written by a Cursor on the
/// other side of 9P. An ordinary read-only open there is not read-only in
/// effect: SQLite creates `-shm` and `-wal` files in Cursor's directory, and
/// its locks and shared memory mean nothing to a writer that cannot see them.
/// `immutable=1` takes no lock and writes nothing. Its price is that it reads
/// only what Cursor has checkpointed into the main file, and that a checkpoint
/// landing mid-read can surface as an error or a value that will not parse.
/// Both fail closed — the row keeps what its transcript says — and everything
/// read here is enrichment, so that is the price worth paying. On a local
/// filesystem locks work, and the ordinary open sees uncheckpointed commits
/// too.
///
/// Opened per call and never pooled: an immutable connection is promised the
/// file never changes, so one kept open would never see anything new.
fn open(db: &Path) -> rusqlite::Result<Connection> {
    if !on_windows_drive(db, &MOUNTS) {
        return readonly(db);
    }
    Connection::open_with_flags(
        immutable_uri(db),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
}

/// What `composerHeaders` says about one chat.
#[derive(Debug, Default, Clone, PartialEq)]
struct Header {
    title: Option<String>,
    created_ms: Option<i64>,
    /// The workspace folder, only when it exists on this machine.
    dir: Option<PathBuf>,
}

impl Header {
    fn apply(&self, s: &mut Session) {
        if let Some(title) = &self.title {
            s.title = Some(title.clone());
        }
        if let Some(ms) = self.created_ms {
            s.started_at = util::ms_to_rfc3339(ms);
        }
        // The real folder rather than the project slug, which turns `/` and
        // `_` alike into `-` and so is not a path; with it the row has a
        // working directory and its recent writes resolve.
        if let Some(dir) = &self.dir {
            s.label_source = dir.to_string_lossy().into_owned();
        }
    }
}

/// The workspace a chat was opened in, as a folder here.
///
/// A WSL folder opened through Remote-WSL is `vscode-remote://wsl+<distro>`
/// with the Linux path as it is; one opened as a plain `\\wsl$\<Distro>\…`
/// folder is a `file` URI on authority `wsl$` whose path starts with the
/// distro. A multi-root workspace has no single folder at all, and a Windows
/// folder is no folder here, so both keep the slug — which is what `is_dir`
/// settles without having to know every shape.
fn workspace_dir(path: Option<String>, authority: Option<String>) -> Option<PathBuf> {
    let path = path?;
    let path = match authority.as_deref() {
        Some("wsl$") => path
            .trim_start_matches('/')
            .split_once('/')
            .map(|(_distro, rest)| format!("/{rest}"))?,
        _ => path,
    };
    let dir = PathBuf::from(path);
    dir.is_dir().then_some(dir)
}

/// Every chat's header, by id, across `dbs`; the first database to name a
/// chat wins.
///
/// One query over `composerHeaders`, a table of about a kilobyte a chat — never
/// the `composerData` values, which are a hundred kilobytes each. An older
/// Cursor without the table, or a database that will not open, adds nothing.
fn headers_in(dbs: &[PathBuf]) -> HashMap<String, Header> {
    let mut all = HashMap::new();
    for db in dbs {
        let Ok(conn) = open(db) else { continue };
        let Ok(mut stmt) = conn.prepare(
            "SELECT composerId, createdAt, json_extract(value, '$.name'), \
             json_extract(value, '$.workspaceIdentifier.uri.path'), \
             json_extract(value, '$.workspaceIdentifier.uri.authority') \
             FROM composerHeaders",
        ) else {
            continue;
        };
        let Ok(rows) = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Header {
                    created_ms: row.get(1).ok(),
                    title: row
                        .get::<_, Option<String>>(2)
                        .ok()
                        .flatten()
                        .filter(|t| !t.is_empty()),
                    dir: workspace_dir(row.get(3).ok().flatten(), row.get(4).ok().flatten()),
                },
            ))
        }) else {
            continue;
        };
        for (id, header) in rows.flatten() {
            all.entry(id).or_insert(header);
        }
    }
    all
}

/// What `composerData:<id>` says about one chat: the model it is set to, its
/// name, and the reasoning effort chosen for that model.
#[derive(Debug, Default, PartialEq)]
struct Composer {
    model: Option<String>,
    title: Option<String>,
    effort: Option<String>,
}

/// One point lookup on the key's index. Only named fields are extracted: the
/// value also holds the chat's encryption keys, which have no business
/// leaving the database.
fn composer(conn: &Connection, id: &str) -> Option<Composer> {
    let (model, title, params) = conn
        .query_row(
            "SELECT json_extract(value, '$.modelConfig.modelName'), \
             json_extract(value, '$.name'), \
             json_extract(value, '$.modelConfig.selectedModels[0].parameters') \
             FROM cursorDiskKV WHERE key = ?1",
            [format!("composerData:{id}")],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .ok()
        .flatten()?;
    Some(Composer {
        // "default" is Cursor's Auto router, not a model. Shown as what it is,
        // and never handed to pricing, where it would substring-match some
        // provider's `…@default` model.
        model: model
            .filter(|m| !m.is_empty())
            .map(|m| if m == "default" { "auto".into() } else { m }),
        title: title.filter(|t| !t.is_empty()),
        effort: params.as_deref().and_then(effort),
    })
}

/// The `effort` parameter of a model's settings, found by id: it is first for
/// Grok and third for Claude, after `thinking` and `context`.
fn effort(params: &str) -> Option<String> {
    serde_json::from_str::<Vec<Value>>(params)
        .ok()?
        .iter()
        .find(|p| p.get("id").and_then(Value::as_str) == Some("effort"))
        .and_then(|p| p.get("value").and_then(Value::as_str))
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// A subagent's type — `explore`, `generalPurpose`, `shell` — which only the
/// database records; its transcript and its parent's call carry nothing that
/// links the two.
fn subagent_type(conn: &Connection, id: &str) -> Option<String> {
    conn.query_row(
        "SELECT json_extract(value, '$.subagentInfo.subagentTypeName') \
         FROM composerHeaders WHERE composerId = ?1",
        [id],
        |row| row.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .filter(|t| !t.is_empty())
}

/// How full the chat's context window is, by Cursor's own count of the prompt
/// it last sent against the window it allows — occupancy, not billing, which
/// is exactly what [`ContextUsage`] means. The window comes from Cursor too,
/// never from a model table: Cursor's model names are its own.
///
/// ponytail: on a Windows drive this reads only what Cursor has checkpointed
/// (see [`open`]), so a live chat's figure can trail by a checkpoint.
pub fn extract_context(s: &Session) -> Option<ContextUsage> {
    context_in(&config::cursor_global_dbs(), &s.session_id)
}

fn context_in(dbs: &[PathBuf], id: &str) -> Option<ContextUsage> {
    dbs.iter().find_map(|db| {
        let (used, max) = open(db)
            .ok()?
            .query_row(
                "SELECT json_extract(value, '$.promptTokenBreakdown.totalUsedTokens'), \
                 json_extract(value, '$.promptTokenBreakdown.maxTokens') \
                 FROM cursorDiskKV WHERE key = ?1",
                [format!("composerData:{id}")],
                |row| Ok((row.get::<_, Option<u64>>(0)?, row.get::<_, Option<u64>>(1)?)),
            )
            .ok()?;
        Some(ContextUsage {
            used: used?,
            max: max.filter(|m| *m > 0)?,
            compacted: false,
        })
    })
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
/// the pages fetched, the searches run — the session's own and its subagents',
/// which are its work too — then what the database adds: the model, the title
/// and the effort.
///
/// ponytail: the export records no tool result, so no call is ever known to
/// have failed and the error rate is `─` rather than `0%`; see
/// [`Provider::records_tool_outcomes`].
pub fn extract(path: &Path) -> SessionData {
    extract_with(path, &config::cursor_global_dbs())
}

fn extract_with(path: &Path, dbs: &[PathBuf]) -> SessionData {
    let mut data = SessionData::default();
    let mut seen = Seen::default();
    if let Err(err) = count_calls(path, None, &mut data, &mut seen) {
        data.error = Some(format!(
            "Could not read Cursor transcript {}: {err}",
            path.display()
        ));
    }
    let now = util::now_ms();
    for file in &super::transcript_files(path)[1..] {
        let Some(id) = file.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
            continue;
        };
        let before = data.metrics.tool_count;
        // A subagent file that will not read costs that subagent's calls,
        // not the session's.
        let _ = count_calls(file, Some(&id), &mut data, &mut seen);
        let started = created_ms(file) as i64;
        let touched = config::file_mtime_ms(file) as i64;
        data.subagents.push(Subagent {
            tool_count: data.metrics.tool_count - before,
            agent_id: id,
            // Filled from the database below when it knows.
            agent_type: "?".into(),
            description: String::new(),
            // Each subagent's model sits in its own hundred-kilobyte database
            // value; one lookup per subagent per parse is not worth a name
            // that is "auto" most of the time.
            model: "?".into(),
            started_at: Some(util::ms_to_rfc3339(started)),
            last_active: Some(util::ms_to_rfc3339(touched)),
            duration_ms: (touched - started).max(0),
            // No tool result marks a Cursor subagent finished, so going
            // quiet is all there is, as it is for a Claude one without hooks.
            status: if now - touched >= super::claude::SUBAGENT_QUIET_MS {
                SubagentStatus::Done
            } else {
                SubagentStatus::Running
            },
            cost: 0.0,
            turns: 0,
            tokens: 0,
            last_text: None,
            tool_use_id: None,
            context: None,
            ghost: false,
        });
    }

    let Some(id) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
        return data;
    };
    for db in dbs {
        let Ok(conn) = open(db) else { continue };
        let Some(c) = composer(&conn, &id) else {
            continue;
        };
        data.last_model = c.model.unwrap_or_default();
        data.title = c.title;
        data.reasoning_effort = c.effort;
        for sa in &mut data.subagents {
            if let Some(kind) = subagent_type(&conn, &sa.agent_id) {
                sa.agent_type = kind;
            }
        }
        break;
    }
    data
}

/// URLs and queries already counted, across a session and its subagents.
#[derive(Default)]
struct Seen {
    urls: HashSet<String>,
    queries: HashSet<String>,
}

/// Fold one transcript's calls into `data`, credited to `origin` when it is a
/// subagent's.
fn count_calls(
    path: &Path,
    origin: Option<&str>,
    data: &mut SessionData,
    seen: &mut Seen,
) -> std::io::Result<()> {
    cursor_line::for_each(path, |line| {
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
                origin.map(str::to_string),
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
                    if let Some(url) = arg("url") {
                        note_web(
                            &mut m.web_fetches,
                            &mut m.web_fetch_count,
                            &mut seen.urls,
                            url,
                        );
                    }
                }
                "websearch" => {
                    if let Some(q) = arg("query").or_else(|| arg("search_term")) {
                        note_web(
                            &mut m.web_searches,
                            &mut m.web_search_count,
                            &mut seen.queries,
                            q,
                        );
                    }
                }
                _ => {}
            }
        }
    })
}

/// The transcript, and the subagents' beside it: they are this session's,
/// and no row of their own would ever offer to remove them.
pub fn delete(session: &Session) -> std::io::Result<()> {
    if let Some(path) = &session.data_file {
        std::fs::remove_file(path)?;
        match std::fs::remove_dir_all(super::subagents_dir(path)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            result => result?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::temp_path;

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

    const P: &str = "11111111-1111-4111-8111-111111111111";
    const S: &str = "22222222-2222-4222-8222-222222222222";
    const T: &str = "33333333-3333-4333-8333-333333333333";
    const Q: &str = "44444444-4444-4444-8444-444444444444";

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn calls(n: usize) -> String {
        let call = r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"path":"a"}}]}}"#;
        format!("{}\n", vec![call; n].join("\n"))
    }

    /// A global database in Cursor's shape. Values are bound as text, as the
    /// real rows are stored: bytes would be read by `json_extract` as JSONB.
    fn fixture_db(dir: &Path, headers: &[(&str, &str)], data: &[(&str, Option<&str>)]) -> PathBuf {
        let db = dir.join("state.vscdb");
        std::fs::create_dir_all(dir).unwrap();
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE composerHeaders(composerId TEXT PRIMARY KEY, workspaceId, \
             createdAt INTEGER, lastUpdatedAt INTEGER, isArchived, isSubagent, recency, \
             checkpointAt, value TEXT, subagentTypeName); \
             CREATE TABLE cursorDiskKV(key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);",
        )
        .unwrap();
        for (id, value) in headers {
            conn.execute(
                "INSERT INTO composerHeaders(composerId, createdAt, value) VALUES (?1, 1767225600000, ?2)",
                [id, value],
            )
            .unwrap();
        }
        for (id, value) in data {
            conn.execute(
                "INSERT INTO cursorDiskKV(key, value) VALUES (?1, ?2)",
                rusqlite::params![format!("composerData:{id}"), value],
            )
            .unwrap();
        }
        db
    }

    /// Only `<slug>/agent-transcripts` is walked, a subagent is never a row of
    /// its own — not even when Cursor keeps a top-level copy of it — and the
    /// database gives the rows their real folder, title and start.
    #[test]
    fn rows_are_top_level_transcripts_enriched_from_the_headers() {
        let tmp = temp_path("cursor-list");
        let root = tmp.join("projects");
        let work = tmp.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let chats = root.join("home-x-work").join("agent-transcripts");
        write(&chats.join(P).join(format!("{P}.jsonl")), &calls(1));
        write(
            &chats.join(P).join("subagents").join(format!("{S}.jsonl")),
            &calls(1),
        );
        write(&chats.join(S).join(format!("{S}.jsonl")), &calls(1));
        write(&chats.join(Q).join(format!("{Q}.jsonl")), &calls(1));
        // Would pass every check on the path, so only the narrowed walk keeps
        // it out.
        write(
            &root
                .join("home-x-work/mcps/agent-transcripts")
                .join(T)
                .join(format!("{T}.jsonl")),
            &calls(1),
        );

        let ids = |sessions: &[Session]| {
            let mut ids: Vec<_> = sessions.iter().map(|s| s.session_id.clone()).collect();
            ids.sort();
            ids
        };
        let bare = list_sessions_in(std::slice::from_ref(&root), &[]);
        assert_eq!(ids(&bare), [P, Q]);
        assert!(
            bare.iter()
                .all(|s| s.label_source == "home-x-work" && s.title.is_none())
        );

        let p_header = format!(
            r#"{{"name":"Plan it","workspaceIdentifier":{{"uri":{{"path":"{}","authority":"wsl+ubuntu"}}}}}}"#,
            work.display()
        );
        // A plain `\\wsl$\Distro\…` folder spells the distro first; a folder
        // that is not here keeps the slug.
        let q_header = format!(
            r#"{{"name":"","workspaceIdentifier":{{"uri":{{"path":"/Ubuntu{}","authority":"wsl$"}}}}}}"#,
            work.join("gone").display()
        );
        let db = fixture_db(&tmp.join("db"), &[(P, &p_header), (Q, &q_header)], &[]);
        let rows = list_sessions_in(std::slice::from_ref(&root), std::slice::from_ref(&db));
        let p = rows.iter().find(|s| s.session_id == P).unwrap();
        assert_eq!(p.title.as_deref(), Some("Plan it"));
        assert_eq!(p.label_source, work.to_string_lossy());
        assert_eq!(p.work_dir().as_deref(), Some(work.as_path()));
        assert_eq!(p.started_at, util::ms_to_rfc3339(1_767_225_600_000));
        assert!(!p.cost_available && p.total_cost.is_none());
        let q = rows.iter().find(|s| s.session_id == Q).unwrap();
        assert_eq!(
            (q.title.as_deref(), q.label_source.as_str()),
            (None, "home-x-work")
        );
        assert_eq!(
            workspace_dir(
                Some(format!("/Ubuntu{}", work.display())),
                Some("wsl$".into())
            ),
            Some(work.clone())
        );

        // An older Cursor without the table changes nothing.
        let old = tmp.join("old.vscdb");
        Connection::open(&old)
            .unwrap()
            .execute_batch("CREATE TABLE ItemTable(key, value)")
            .unwrap();
        let rows = list_sessions_in(std::slice::from_ref(&root), &[old]);
        assert!(
            rows.iter()
                .all(|s| s.label_source == "home-x-work" && s.title.is_none())
        );

        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn the_model_effort_and_subagents_come_without_inventing_usage() {
        let tmp = temp_path("cursor-extract");
        let main = tmp.join(P).join(format!("{P}.jsonl"));
        write(&main, &calls(1));
        write(
            &tmp.join(P).join("subagents").join(format!("{S}.jsonl")),
            &calls(2),
        );
        write(
            &tmp.join(P).join("subagents").join(format!("{T}.jsonl")),
            &calls(3),
        );
        let composer = r#"{"name":"Title","blobEncryptionKey":"secret","modelConfig":{"modelName":"default","selectedModels":[{"modelId":"default","parameters":[{"id":"thinking","value":"true"},{"id":"effort","value":"medium"}]}]}}"#;
        let db = fixture_db(
            &tmp.join("db"),
            &[(S, r#"{"subagentInfo":{"subagentTypeName":"explore"}}"#)],
            &[(P, Some(composer)), (Q, None)],
        );

        let data = extract_with(&main, std::slice::from_ref(&db));
        assert_eq!(data.error, None);
        assert_eq!(
            data.last_model, "auto",
            "Cursor's Auto router is not a model"
        );
        assert_eq!(data.reasoning_effort.as_deref(), Some("medium"));
        assert_eq!(data.title.as_deref(), Some("Title"));
        assert_eq!(
            data.metrics.tool_count, 6,
            "subagents' calls are the session's work"
        );
        let mut subs: Vec<_> = data
            .subagents
            .iter()
            .map(|s| {
                (
                    s.agent_id.as_str(),
                    s.agent_type.as_str(),
                    s.tool_count,
                    s.cost,
                )
            })
            .collect();
        subs.sort_by(|a, b| a.0.cmp(b.0));
        assert_eq!(subs, [(S, "explore", 2, 0.0), (T, "?", 3, 0.0)]);
        assert_eq!(data.tokens.total, 0);
        assert_eq!(data.costs.total, 0.0);

        // No database, or no usable row, is today's transcript-only answer.
        for dbs in [vec![], vec![tmp.join("missing.vscdb")]] {
            let bare = extract_with(&main, &dbs);
            assert_eq!((bare.last_model.as_str(), bare.error), ("", None));
            assert_eq!(bare.subagents.len(), 2);
        }
        let q = tmp.join(Q).join(format!("{Q}.jsonl"));
        write(&q, &calls(1));
        let null = extract_with(&q, std::slice::from_ref(&db));
        assert_eq!(
            (null.last_model.as_str(), null.title, null.error),
            ("", None, None)
        );

        // Deleting the session takes its subagents with it.
        let mut session = Session::new(Provider::Cursor, P.into());
        session.data_file = Some(main.clone());
        delete(&session).unwrap();
        assert!(!main.exists() && !tmp.join(P).join("subagents").exists());
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn context_is_cursors_own_used_over_max() {
        let tmp = temp_path("cursor-context");
        let db = fixture_db(
            &tmp,
            &[],
            &[
                (
                    P,
                    Some(
                        r#"{"promptTokenBreakdown":{"totalUsedTokens":47446,"maxTokens":200000}}"#,
                    ),
                ),
                (
                    S,
                    Some(r#"{"promptTokenBreakdown":{"totalUsedTokens":5,"maxTokens":0}}"#),
                ),
                (T, Some(r#"{"contextTokensUsed":5}"#)),
            ],
        );
        let dbs = std::slice::from_ref(&db);
        let ctx = context_in(dbs, P).unwrap();
        assert_eq!((ctx.used, ctx.max, ctx.compacted), (47446, 200_000, false));
        assert!(context_in(dbs, S).is_none());
        assert!(context_in(dbs, T).is_none());
        assert!(context_in(dbs, Q).is_none());
        let _ = std::fs::remove_dir_all(tmp);
    }

    #[test]
    fn a_windows_drive_is_opened_immutable_by_a_uri_that_survives_its_path() {
        assert_eq!(
            immutable_uri(Path::new("/mnt/c/Users/John Doe/a#b?c%d/state.vscdb")),
            "file:/mnt/c/Users/John%20Doe/a%23b%3Fc%25d/state.vscdb?immutable=1"
        );
        let mount = |point: &str, fstype: &str| Mount {
            point: point.into(),
            fstype: fstype.into(),
        };
        let mounts = [
            mount("/", "ext4"),
            mount("/mnt/c", "9p"),
            mount("/c", "drvfs"),
        ];
        assert!(on_windows_drive(
            Path::new("/mnt/c/Users/u/state.vscdb"),
            &mounts
        ));
        assert!(on_windows_drive(
            Path::new("/c/Users/u/state.vscdb"),
            &mounts
        ));
        assert!(!on_windows_drive(
            Path::new("/home/u/.config/Cursor/state.vscdb"),
            &mounts
        ));
        assert!(
            !on_windows_drive(Path::new("/mnt/cdrom/x"), &mounts),
            "a component, not a prefix"
        );
        // And the suite never reaches for the developer's own database.
        assert!(config::cursor_global_dbs().is_empty());
    }
}
