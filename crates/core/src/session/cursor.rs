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
use super::{ContextUsage, Session, SessionData, Subagent, SubagentStatus};
use crate::config;
use crate::util;
// Finding the chats is cctop-discovery's; reading them is here.
use cctop_discovery::session::cursor::{self as found, created_ms, open};
use rayon::prelude::*;
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn list_sessions() -> Vec<Session> {
    list_sessions_in(
        &config::cursor_projects_roots(),
        &config::cursor_global_dbs(),
    )
}

/// The rows under `roots`, titled from `dbs`. Finding them is
/// cctop-discovery's; core only lists in parallel, under a trace span, and
/// says that Cursor records no cost.
fn list_sessions_in(roots: &[PathBuf], dbs: &[PathBuf]) -> Vec<Session> {
    let (files, subagents) = found::transcripts_in(roots);
    let mut sessions: Vec<_> = files
        .into_par_iter()
        .filter_map(|path| found::summarize(path, roots))
        .filter(|s| !subagents.contains(&s.session_id))
        .map(|found| {
            let mut session = Session::from_found(found);
            session.cost_available = false;
            session.total_cost = None;
            session
        })
        .collect();
    found::newest_copy_only(&mut sessions, |s| &s.found);
    if !sessions.is_empty() {
        let headers = {
            let _span = crate::trace::span("walk.cursor.headers");
            found::headers_in(dbs)
        };
        for s in &mut sessions {
            if let Some(h) = headers.get(&s.session_id) {
                h.apply(&mut s.found);
            }
        }
    }
    sessions
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
fn composer(conn: &Connection, id: &str) -> rusqlite::Result<Option<Composer>> {
    let Some((model, title, params)) = conn
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
        .optional()?
    else {
        return Ok(None);
    };
    Ok(Some(Composer {
        // "default" is Cursor's Auto router, not a model. Shown as what it is,
        // and never handed to pricing, where it would substring-match some
        // provider's `…@default` model.
        model: model
            .filter(|m| !m.is_empty())
            .map(|m| if m == "default" { "auto".into() } else { m }),
        title: title.filter(|t| !t.is_empty()),
        effort: params.as_deref().and_then(effort),
    }))
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
fn subagent_type(conn: &Connection, id: &str) -> rusqlite::Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT json_extract(value, '$.subagentInfo.subagentTypeName') \
             FROM composerHeaders WHERE composerId = ?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten()
        .filter(|t| !t.is_empty()))
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
    let quiet = super::claude::SUBAGENT_QUIET_MS;
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
            // quiet is all there is, as it is for a Claude one without hooks
            // — and since quiet is a matter of the clock, a running one has
            // this extraction looked at again once it would have gone quiet.
            status: if now - touched >= quiet {
                SubagentStatus::Done
            } else {
                recheck(&mut data.recheck_at_ms, touched + quiet);
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
    // A database that is there but does not answer — it will not open, a read
    // tears on a checkpoint, or the chat is not in it yet because an immutable
    // read sees only what Cursor has checkpointed — is asked again later.
    //
    // ponytail: a chat that is never in any database (Cursor's agent CLI, beside
    // a desktop Cursor) is re-read every few minutes and never cached on disk.
    let mut answered = dbs.is_empty();
    for db in dbs {
        let Ok(conn) = open(db) else { continue };
        let Ok(Some(c)) = composer(&conn, &id) else {
            continue;
        };
        data.last_model = c.model.unwrap_or_default();
        data.title = c.title;
        data.reasoning_effort = c.effort;
        answered = true;
        for sa in &mut data.subagents {
            match subagent_type(&conn, &sa.agent_id) {
                Ok(Some(kind)) => sa.agent_type = kind,
                Ok(None) => {}
                Err(_) => answered = false,
            }
        }
        break;
    }
    if !answered {
        recheck(&mut data.recheck_at_ms, now + DB_RECHECK_MS);
    }
    data
}

/// How long an extraction the database did not answer is served before the
/// database is asked again.
const DB_RECHECK_MS: i64 = 5 * 60 * 1000;

fn recheck(at: &mut Option<i64>, when: i64) {
    *at = Some(at.map_or(when, |t| t.min(when)));
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
///
/// That includes the top-level copy Cursor sometimes keeps of a subagent,
/// `<sid>/<sid>.jsonl`, which only the parent's `subagents/` marks as one: left
/// behind, it would come back as a row of its own on the next walk.
pub fn delete(session: &Session) -> std::io::Result<()> {
    let Some(path) = &session.data_file else {
        return Ok(());
    };
    std::fs::remove_file(path)?;
    let subagents = super::subagents_dir(path);
    if let Some(chats) = subagents.parent().and_then(Path::parent) {
        for name in config::list_dir(&subagents) {
            let Some(sid) = name.strip_suffix(".jsonl") else {
                continue;
            };
            if !config::is_full_uuid(sid) {
                continue;
            }
            let copy = chats.join(sid);
            gone(std::fs::remove_file(copy.join(&name)))?;
            gone(std::fs::remove_dir_all(copy.join("subagents")))?;
            // Only once empty: whatever else is in it is not known to be ours.
            let _ = std::fs::remove_dir(&copy);
        }
    }
    gone(std::fs::remove_dir_all(subagents))
}

/// Already gone is what a removal wanted.
fn gone(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;
    use crate::session::temp_path;
    use cctop_discovery::mounts::Mount;
    use cctop_discovery::session::cursor::{
        immutable_uri, on_windows_drive, on_windows_drive_really, open_immutable, workspace_dir,
    };

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

    /// Set a file's mtime to `ms` (Unix ms).
    fn touch(path: &Path, ms: i64) {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms as u64);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(t)
            .unwrap();
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
        // The subagent wrote last, and that is what keeps its parent live.
        touch(&chats.join(P).join(format!("{P}.jsonl")), 1_767_225_600_000);
        let sub_written = 1_767_225_700_000;
        touch(
            &chats.join(P).join("subagents").join(format!("{S}.jsonl")),
            sub_written,
        );
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
        let p = bare.iter().find(|s| s.session_id == P).unwrap();
        assert_eq!(p.touched_at, util::ms_to_rfc3339(sub_written));
        assert_eq!(super::super::effective_mtime_ms(p), sub_written as u64);
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
        for authority in ["wsl$", "wsl.localhost"] {
            assert_eq!(
                workspace_dir(
                    Some(format!("/Ubuntu{}", work.display())),
                    Some(authority.into())
                ),
                Some(work.clone()),
                "{authority}"
            );
        }

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

        // Deleting the parent takes the subagent's top-level copy with it,
        // which would otherwise come back as a row of its own.
        let p = rows.into_iter().find(|s| s.session_id == P).unwrap();
        delete(&p).unwrap();
        assert!(!chats.join(S).exists());
        assert_eq!(
            ids(&list_sessions_in(std::slice::from_ref(&root), &[])),
            [Q]
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

        // A subagent written just now is running, and only the clock says
        // when it stops: such an extraction is looked at again then, and
        // never cached on disk meanwhile.
        let fresh = extract_with(&main, std::slice::from_ref(&db));
        let written =
            config::file_mtime_ms(&tmp.join(P).join("subagents").join(format!("{S}.jsonl")));
        assert!(
            fresh
                .subagents
                .iter()
                .all(|s| s.status == SubagentStatus::Running)
        );
        assert!(fresh.recheck_at_ms.is_some_and(|at| at
            <= written as i64 + super::super::claude::SUBAGENT_QUIET_MS
            && at > util::now_ms()));
        let old = util::now_ms() - 60_000;
        for f in super::super::transcript_files(&main) {
            touch(&f, old);
        }

        let data = extract_with(&main, std::slice::from_ref(&db));
        assert_eq!(data.error, None);
        assert_eq!(
            data.recheck_at_ms, None,
            "settled, and the database answered"
        );
        assert!(
            data.subagents
                .iter()
                .all(|s| s.status == SubagentStatus::Done)
        );
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

        // No database, or no usable row, is today's transcript-only answer —
        // final when there is no database, asked again when one did not
        // answer.
        for (dbs, recheck) in [(vec![], false), (vec![tmp.join("missing.vscdb")], true)] {
            let bare = extract_with(&main, &dbs);
            assert_eq!((bare.last_model.as_str(), bare.error), ("", None));
            assert_eq!(bare.subagents.len(), 2);
            assert_eq!(bare.recheck_at_ms.is_some(), recheck, "{dbs:?}");
        }
        let unknown = tmp.join(T).join(format!("{T}.jsonl"));
        write(&unknown, &calls(1));
        let unknown = extract_with(&unknown, std::slice::from_ref(&db));
        assert!(
            unknown.recheck_at_ms.is_some(),
            "not in the database yet may be not checkpointed yet"
        );
        let q = tmp.join(Q).join(format!("{Q}.jsonl"));
        write(&q, &calls(1));
        let null = extract_with(&q, std::slice::from_ref(&db));
        assert_eq!(
            (
                null.last_model.as_str(),
                null.title,
                null.error,
                null.recheck_at_ms
            ),
            ("", None, None, None)
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

    /// The immutable open reads a WAL database through a path that needs
    /// escaping, and leaves nothing beside it — the promise made to the
    /// Cursor on the other side of 9P. A link into the drive is judged by
    /// where it leads.
    #[test]
    fn an_immutable_open_reads_and_writes_nothing() {
        let tmp = temp_path("cursor-immutable");
        let dir = tmp.join("John Doe#1?");
        let db = fixture_db(&dir, &[(P, r#"{"name":"Plan it"}"#)], &[]);
        Connection::open(&db)
            .unwrap()
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
        let beside = |ext: &str| dir.join(format!("state.vscdb-{ext}"));
        assert!(!beside("wal").exists() && !beside("shm").exists());

        let headers = open_immutable(&db)
            .unwrap()
            .query_row("SELECT count(*) FROM composerHeaders", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap();
        assert_eq!(headers, 1);
        assert!(!beside("wal").exists() && !beside("shm").exists());

        let link = tmp.join("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        let real = std::fs::canonicalize(&db).unwrap();
        let mounts = [
            Mount {
                point: "/".into(),
                fstype: "ext4".into(),
            },
            Mount {
                point: real.parent().unwrap().to_path_buf(),
                fstype: "9p".into(),
            },
        ];
        assert_eq!(
            on_windows_drive_really(&link.join("state.vscdb"), &mounts),
            Some(real)
        );
        let _ = std::fs::remove_dir_all(tmp);
    }
}
