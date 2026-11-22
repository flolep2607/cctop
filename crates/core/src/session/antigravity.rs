//! Antigravity CLI (`agy`) conversation discovery and extraction.
//!
//! `agy` keeps everything under `~/.gemini/antigravity-cli/`. Each conversation
//! is a directory, `brain/<uuid>/`, and its log is
//! `brain/<uuid>/.system_generated/logs/transcript.jsonl`: one line per step,
//! with `type` (`USER_INPUT`, `PLANNER_RESPONSE`, `GENERIC`, `SYSTEM_MESSAGE`,
//! …), `status` and `created_at`. A `PLANNER_RESPONSE` is a model turn and
//! carries `model`, `input_tokens`, `cache_read_tokens`, `output_tokens` and the
//! `tool_calls` it made; each call is answered, in order, by a `GENERIC` step
//! whose `status` is `ERROR` or `INVALID` when the call failed.
//!
//! The transcript holds no working directory and no title. Both come from
//! `conversation_summaries.db` beside `brain/`, which also says which
//! conversations are subagents (`parent_conversation_id`). A swarm run makes
//! one per worker, so a parent can have a dozen children. They are folded into
//! the parent the way Claude Code's sidechains are, rather than listed as rows
//! that would bury the conversation that started them.
//!
//! The log is also written in pieces under `logs/chunks/transcript/`. Those
//! concatenate byte for byte to `transcript.jsonl`, so only the whole file is
//! read. `transcript_full.jsonl` is the same steps with `content` untruncated,
//! which the figures here do not need.
//!
//! Cost is estimated at API rates, as every harness's is: `agy` names a model
//! as an API model plus an effort level, and
//! [`pricing::antigravity_api_model`] takes it back to the name it is billed
//! under.
//!
//! ponytail: written against `agy` 1.x as found on disk; Google publishes no
//! schema. Unknown fields are ignored and a missing one reads as zero, so a
//! format change costs figures, never a wrong row.

use super::extract::{self, for_each_jsonl};
use super::gemini::{self, Breakdown};
use super::{ContextUsage, Session, SessionData, Subagent, SubagentStatus, Surface, Tokens};
use crate::config;
use crate::pricing::{self, Provider};
use crate::util;
use serde_json::{Map, Value};
use std::collections::{HashMap, VecDeque};
use std::io::BufRead;
use std::path::{Path, PathBuf};

const SUMMARIES_DB: &str = "conversation_summaries.db";

fn transcript_in(app_dir: &Path, id: &str) -> PathBuf {
    app_dir
        .join("brain")
        .join(id)
        .join(".system_generated")
        .join("logs")
        .join("transcript.jsonl")
}

/// `<app>/brain/<id>/.system_generated/logs/transcript.jsonl` → (`<app>`, `<id>`).
fn locate(transcript: &Path) -> Option<(&Path, String)> {
    let conversation = transcript.ancestors().nth(3)?;
    let id = conversation.file_name()?.to_string_lossy().into_owned();
    Some((conversation.parent()?.parent()?, id))
}

/// One row of `conversation_summaries`.
#[derive(Default, Clone)]
struct Summary {
    title: String,
    workspace: String,
    parent: String,
    agent: String,
    idle: bool,
}

/// Every conversation the index knows, by id. An index that will not open
/// leaves the map empty: the transcripts are still listed, without a title,
/// a directory or a parent.
fn summaries(app_dir: &Path) -> HashMap<String, Summary> {
    let Ok(db) = super::readonly(&app_dir.join(SUMMARIES_DB)) else {
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
fn first_workspace(uris: &str) -> String {
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
fn descendants(index: &HashMap<String, Summary>, root: &str) -> Vec<String> {
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
fn first_step_at(path: &Path) -> Option<String> {
    let mut line = String::new();
    std::io::BufReader::new(std::fs::File::open(path).ok()?)
        .read_line(&mut line)
        .ok()?;
    let step: Value = serde_json::from_str(&line).ok()?;
    Some(step.get("created_at")?.as_str()?.to_string())
}

pub fn list_sessions() -> Vec<Session> {
    let mut sessions = Vec::new();
    for app_dir in config::antigravity_dirs() {
        let index = summaries(&app_dir);
        for id in config::list_dir(&app_dir.join("brain")) {
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
                .map(|child| config::file_mtime_ms(&transcript_in(&app_dir, child)))
                .chain([config::file_mtime_ms(&path)])
                .max()
                .unwrap_or(0) as i64;
            let mut session = Session::new(Provider::Antigravity, id);
            session.surface = Surface::Cli;
            session.last_active = util::ms_to_rfc3339(touched);
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

/// `agy` writes every argument value JSON-encoded inside a string —
/// `"CommandLine": "\"git status\""` — and names them in PascalCase. Decode
/// them and rename the ones the shared tool readers look for, so a command,
/// a path and an edit's diff read as they do for every other harness.
pub(crate) fn plain_args(args: Option<&Value>) -> Value {
    let mut plain = Map::new();
    for (key, value) in args.and_then(Value::as_object).into_iter().flatten() {
        let value = match value.as_str().map(serde_json::from_str::<Value>) {
            Some(Ok(decoded)) => decoded,
            _ => value.clone(),
        };
        let key = match key.as_str() {
            "CommandLine" => "command",
            "Path" => "file_path",
            "Target" => "old_string",
            "Replacement" => "new_string",
            "Content" => "content",
            "Prompt" => "prompt",
            other => other,
        };
        plain.insert(key.to_string(), value);
    }
    Value::Object(plain)
}

/// The name the shared detail reader knows an `agy` tool by.
pub(crate) fn reader_name(name: &str) -> &str {
    match name {
        "run_command" => "bash",
        "edit_file" => "edit",
        "invoke_subagent" => "agent",
        "search_web" => "websearch",
        other => other,
    }
}

fn count(step: &Value, field: &str) -> u64 {
    step.get(field).and_then(Value::as_u64).unwrap_or(0)
}

pub(crate) fn str_of<'a>(step: &'a Value, field: &str) -> &'a str {
    step.get(field).and_then(Value::as_str).unwrap_or("")
}

/// One model turn's tokens. `cache_read_tokens` sits beside `input_tokens`
/// rather than inside it — a turn reads 2,520 input and 11,777 cached after
/// one that read 13,834 input and none — so the prompt is their sum.
fn turn_tokens(step: &Value) -> Tokens {
    let cached = count(step, "cache_read_tokens");
    Tokens::from_prompt(
        count(step, "input_tokens") + cached,
        cached,
        count(step, "output_tokens"),
    )
}

/// Fold one transcript into `data`, its calls credited to `origin` when it is
/// a subagent's. Returns the model it last used.
fn read_into(
    path: &Path,
    origin: Option<&str>,
    data: &mut SessionData,
    breakdown: &mut Breakdown,
) -> std::io::Result<String> {
    let mut model = String::new();
    // Calls waiting for the `GENERIC` step that answers them, in order.
    let mut pending: VecDeque<String> = VecDeque::new();
    for_each_jsonl(path, |step| match str_of(step, "type") {
        "PLANNER_RESPONSE" => {
            let ts = str_of(step, "created_at");
            if let Some(name) = step.get("model").and_then(Value::as_str) {
                model = name.to_string();
                let api = pricing::antigravity_api_model(name);
                gemini::account_turn(data, breakdown, name, &api, ts, &turn_tokens(step));
            }
            for call in step
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = str_of(call, "name");
                if name.is_empty() {
                    continue;
                }
                let args = plain_args(call.get("args"));
                let (short, full) = extract::tool_detail(reader_name(name), &args);
                extract::push_tool_detail(
                    &mut data.metrics.tool_details,
                    name,
                    short,
                    full,
                    ts.to_string(),
                    None,
                    origin.map(str::to_string),
                );
                if let Some(delta) = matches!(name, "edit_file" | "write_file")
                    .then(|| extract::edit_delta(&args))
                    .flatten()
                    && let Some(detail) = data
                        .metrics
                        .tool_details
                        .get_mut(name)
                        .and_then(|d| d.last_mut())
                {
                    detail.delta = Some(delta);
                }
                *data.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
                data.metrics.tool_count += 1;
                pending.push_back(name.to_string());
            }
        }
        "GENERIC" => {
            let Some(name) = pending.pop_front() else {
                return;
            };
            let failed = matches!(str_of(step, "status"), "ERROR" | "INVALID");
            data.metrics.tool_errors += u64::from(failed);
            // The call this answers is the oldest of its name still unanswered,
            // which is the last of that name pushed no later than it: answers
            // arrive in call order, so the same name's earlier calls are
            // already settled.
            let Some(detail) = data.metrics.tool_details.get_mut(&name).and_then(|d| {
                d.iter_mut()
                    .rev()
                    .nth(pending.iter().filter(|n| **n == name).count())
            }) else {
                return;
            };
            detail.failed = failed;
            // A failed edit changed nothing; its diff stays as what it tried.
            if !failed && let Some(delta) = &detail.delta {
                data.metrics.lines_added += delta.added as u64;
                data.metrics.lines_removed += delta.removed as u64;
            }
        }
        _ => {}
    })?;
    Ok(model)
}

/// Parse a conversation and every subagent it started.
///
/// A subagent's tokens, cost and calls count toward the parent — they are its
/// work, and its bill — and each also gets a [`Subagent`] entry saying what
/// share was its own.
pub fn extract(path: &Path) -> SessionData {
    let mut data = SessionData::default();
    let mut breakdown = Breakdown::new();
    match read_into(path, None, &mut data, &mut breakdown) {
        Ok(model) => data.last_model = model,
        Err(err) => {
            data.error = Some(format!(
                "Could not read Antigravity transcript {}: {err}",
                path.display()
            ));
            return data;
        }
    }

    if let Some((app_dir, id)) = locate(path) {
        let index = summaries(app_dir);
        data.title = index
            .get(&id)
            .map(|s| s.title.clone())
            .filter(|t| !t.is_empty());
        for child in descendants(&index, &id) {
            let file = transcript_in(app_dir, &child);
            let (cost, tokens, tools) = (
                data.costs.total,
                data.tokens.all_input() + data.tokens.output,
                data.metrics.tool_count,
            );
            // A subagent that will not read costs its own figures, not the
            // session's.
            let Ok(model) = read_into(&file, Some(&child), &mut data, &mut breakdown) else {
                continue;
            };
            let summary = &index[&child];
            let started = first_step_at(&file);
            let touched = config::file_mtime_ms(&file) as i64;
            data.subagents.push(Subagent {
                agent_id: child.clone(),
                agent_type: summary.agent.clone(),
                description: String::new(),
                model,
                duration_ms: started
                    .as_deref()
                    .and_then(util::parse_ts)
                    .map_or(0, |t| (touched - t.timestamp_millis()).max(0)),
                started_at: started,
                last_active: Some(util::ms_to_rfc3339(touched)),
                status: if summary.idle {
                    SubagentStatus::Done
                } else {
                    SubagentStatus::Running
                },
                cost: data.costs.total - cost,
                tool_count: data.metrics.tool_count - tools,
                turns: 0,
                tokens: data.tokens.all_input() + data.tokens.output - tokens,
                last_text: None,
                tool_use_id: None,
                context: None,
                ghost: false,
            });
        }
    }
    gemini::finish_breakdown(&mut data, breakdown);
    data
}

/// The newest model turn in the transcript's tail that `f` answers for.
fn last_turn<T>(path: &Path, mut f: impl FnMut(&Value) -> Option<T>) -> Option<T> {
    util::scan_tail_escalating(path, |text| {
        text.lines().rev().find_map(|line| {
            let step: Value = serde_json::from_str(line.trim()).ok()?;
            (str_of(&step, "type") == "PLANNER_RESPONSE").then_some(())?;
            f(&step)
        })
    })
}

/// Context-window use: the whole prompt the newest turn was sent.
pub fn extract_context(session: &Session) -> Option<ContextUsage> {
    let path = session.data_file.as_deref()?;
    let (used, model) = last_turn(path, |step| {
        let used = turn_tokens(step).input_total;
        (used > 0).then(|| (used, str_of(step, "model").to_string()))
    })?;
    Some(ContextUsage {
        used,
        max: pricing::litellm_max_input_tokens(&pricing::antigravity_api_model(&model))
            .unwrap_or(gemini::GEMINI_DEFAULT_CTX),
        compacted: false,
    })
}

/// Name of the newest call in the transcript's tail.
pub fn extract_last_tool(session: &Session) -> String {
    session
        .data_file
        .as_deref()
        .and_then(|path| {
            last_turn(path, |step| {
                step.get("tool_calls")?
                    .as_array()?
                    .iter()
                    .rev()
                    .find_map(|call| call.get("name")?.as_str())
                    .map(str::to_string)
            })
        })
        .unwrap_or_default()
}

/// Refuse, and say so.
///
/// A conversation is its `brain/` directory, its `conversations/<id>.db`, its
/// row in the summaries index and its subagents' copies of all three, and
/// `agy` may hold any of them open. Removing the transcript alone would leave
/// `agy` listing a conversation with no log.
pub fn delete(_session: &Session) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Antigravity conversations span several files agy manages; delete them from agy",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app directory with the summaries index and the given transcripts.
    fn app(name: &str, convs: &[(&str, &str, &str)]) -> PathBuf {
        let dir = super::super::temp_path(&format!("agy-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let db = rusqlite::Connection::open(dir.join(SUMMARIES_DB)).unwrap();
        db.execute_batch(
            "CREATE TABLE conversation_summaries (conversation_id text, title text, \
             workspace_uris text, parent_conversation_id text, agent_name text, status text)",
        )
        .unwrap();
        for (id, parent, body) in convs {
            db.execute(
                "INSERT INTO conversation_summaries VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                [
                    *id,
                    if parent.is_empty() {
                        "Fix the build"
                    } else {
                        ""
                    },
                    r#"["file:///home/u/my%20proj"]"#,
                    parent,
                    if parent.is_empty() { "" } else { "verifier" },
                    "CASCADE_RUN_STATUS_IDLE",
                ],
            )
            .unwrap();
            let path = transcript_in(&dir, id);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        dir
    }

    const PARENT: &str = concat!(
        r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-21T17:38:58Z","content":"go"}"#,
        "\n",
        r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-21T17:38:59Z","model":"gemini-test","input_tokens":1000,"cache_read_tokens":0,"output_tokens":10,"tool_calls":[{"name":"run_command","args":{"CommandLine":"\"git status\"","WaitMsBeforeAsync":"5000"}},{"name":"edit_file","args":{"Path":"\"src/a.rs\"","Target":"\"a\"","Replacement":"\"b\\nc\""}}]}"#,
        "\n",
        r#"{"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","created_at":"2026-11-21T17:39:00Z","content":"ok"}"#,
        "\n",
        r#"{"step_index":3,"source":"MODEL","type":"GENERIC","status":"ERROR","created_at":"2026-11-21T17:39:01Z","error":"target content not found"}"#,
        "\n",
        r#"{"step_index":4,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-21T17:39:02Z","model":"gemini-test","input_tokens":200,"cache_read_tokens":1000,"output_tokens":5,"tool_calls":[{"name":"edit_file","args":{"Path":"\"src/a.rs\"","Target":"\"a\"","Replacement":"\"b\\nc\""}}]}"#,
        "\n",
        r#"{"step_index":5,"source":"MODEL","type":"GENERIC","status":"DONE","created_at":"2026-11-21T17:39:03Z","content":"ok"}"#,
        "\n",
    );

    const CHILD: &str = concat!(
        r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-11-21T17:40:00Z","content":"check"}"#,
        "\n",
        r#"{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-11-21T17:40:01Z","model":"gemini-worker","input_tokens":300,"cache_read_tokens":0,"output_tokens":7,"tool_calls":[{"name":"read_file","args":{"Path":"\"README.md\""}}]}"#,
        "\n",
        r#"{"step_index":2,"source":"MODEL","type":"GENERIC","status":"DONE","created_at":"2026-11-21T17:40:02Z","content":"text"}"#,
        "\n",
    );

    const P: &str = "11111111-1111-1111-1111-111111111111";
    const C: &str = "22222222-2222-2222-2222-222222222222";

    #[test]
    fn reads_tokens_calls_and_outcomes() {
        let dir = app("parent", &[(P, "", PARENT)]);
        let data = extract(&transcript_in(&dir, P));
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(data.title.as_deref(), Some("Fix the build"));
        assert_eq!(data.last_model, "gemini-test");
        // Cached input sits beside input, so the prompt is their sum.
        assert_eq!(data.tokens.input_total, 2200);
        assert_eq!(data.tokens.cached_input, 1000);
        assert_eq!(data.tokens.input, 1200);
        assert_eq!(data.tokens.output, 15);

        assert_eq!(data.metrics.tool_count, 3);
        assert_eq!(data.metrics.tools["edit_file"], 2);
        // The second answer is an ERROR, and it belongs to the first edit.
        assert_eq!(data.metrics.tool_errors, 1);
        assert!(!data.metrics.tool_details["run_command"][0].failed);
        assert!(data.metrics.tool_details["edit_file"][0].failed);
        assert!(!data.metrics.tool_details["edit_file"][1].failed);
        // JSON-encoded arguments are decoded for the detail column.
        assert_eq!(data.metrics.tool_details["run_command"][0].d, "git status");
        assert_eq!(data.metrics.tool_details["edit_file"][0].d, "src/a.rs");
        // Only the edit that landed counts its lines.
        assert_eq!(data.metrics.lines_added, 2);
        assert_eq!(data.metrics.lines_removed, 1);
    }

    #[test]
    fn subagents_fold_into_their_parent_and_are_not_rows() {
        let dir = app("swarm", &[(P, "", PARENT), (C, P, CHILD)]);
        let data = extract(&transcript_in(&dir, P));
        let index = summaries(&dir);
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(descendants(&index, P), vec![C.to_string()]);
        assert_eq!(
            data.tokens.input_total, 2500,
            "the child's prompt is billed too"
        );
        assert_eq!(data.metrics.tool_count, 4);
        // The parent's own model, not the child's last one.
        assert_eq!(data.last_model, "gemini-test");
        assert_eq!(data.models, vec!["gemini-test", "gemini-worker"]);
        assert_eq!(data.subagents.len(), 1);
        let sa = &data.subagents[0];
        assert_eq!(sa.agent_id, C);
        assert_eq!(sa.agent_type, "verifier");
        assert_eq!(sa.model, "gemini-worker");
        assert_eq!(sa.tool_count, 1);
        assert_eq!(sa.tokens, 307);
        assert!(matches!(sa.status, SubagentStatus::Done));
    }

    #[test]
    fn the_index_supplies_title_directory_and_parenthood() {
        let dir = app("index", &[(P, "", PARENT), (C, P, CHILD)]);
        let index = summaries(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(index[P].workspace, "/home/u/my proj");
        assert_eq!(index[C].parent, P);
        assert!(index[P].idle);
    }

    #[test]
    fn context_and_last_tool_come_from_the_newest_turn() {
        let dir = app("tail", &[(P, "", PARENT)]);
        let mut session = Session::new(Provider::Antigravity, P.into());
        session.data_file = Some(transcript_in(&dir, P));
        let context = extract_context(&session);
        let last_tool = extract_last_tool(&session);
        std::fs::remove_dir_all(&dir).unwrap();

        let context = context.expect("a context figure");
        assert_eq!(context.used, 1200);
        assert_eq!(context.max, gemini::GEMINI_DEFAULT_CTX);
        assert_eq!(last_tool, "edit_file");
    }

    #[test]
    fn locates_the_app_directory_and_id_from_a_transcript() {
        let path = transcript_in(Path::new("/x/agy"), P);
        let (app_dir, id) = locate(&path).unwrap();
        assert_eq!(app_dir, Path::new("/x/agy"));
        assert_eq!(id, P);
    }
}
