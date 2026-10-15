//! Gemini CLI session discovery and chat extraction.
//!
//! Gemini files its chats under `~/.gemini/tmp/<project>/chats/`, in one of two
//! shapes depending on the release that wrote them: an older single JSON object
//! holding the whole conversation, and a newer JSONL log of a header line, one
//! line per message, and `{"$set": …}` patches revising header fields in place.
//! Both carry the same records, so everything below reads them as one format.
//!
//! Gemini records per-turn token counts but never a cost, so costs here are
//! estimated from published rates the same way Codex's are — see [`extract`].
//!
//! The table's figures — tokens, cost, calls and their failures, the context
//! meter, the last tool — are read through the typed model in
//! [`super::gemini_line`]; [`for_each_record`] stays a `Value` walk for the
//! conversation view, which renders every field a message has.

use super::extract::{self, for_each_jsonl};
use super::gemini_line::{self, Record, TokenCounts};
use super::{ContextUsage, Costs, ModelBreakdown, Session, SessionData, Surface, Tokens};
use crate::config;
use crate::pricing::{self, Provider};
use crate::util;
use rayon::prelude::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Walk a chat file's records in order, whichever shape it is written in.
///
/// The JSONL form's `$set` patches are unwrapped and handed over as plain
/// objects: every field they revise is one the header already declares, so a
/// caller that simply takes the last value it sees ends up with the same
/// header the JSON form states outright.
pub fn for_each_record(path: &Path, mut f: impl FnMut(&Value)) -> std::io::Result<()> {
    if path.extension().is_some_and(|ext| ext == "jsonl") {
        return for_each_jsonl(path, |item| match item.get("$set") {
            Some(patch) => f(patch),
            None => f(item),
        });
    }
    let root: Value = serde_json::from_str(&std::fs::read_to_string(path)?)?;
    f(&root);
    if let Some(messages) = root.get("messages").and_then(Value::as_array) {
        messages.iter().for_each(f);
    }
    Ok(())
}

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
fn chat_files() -> Vec<PathBuf> {
    config::gemini_chats_roots()
        .into_iter()
        .flat_map(|root| {
            config::list_dir(&root)
                .into_iter()
                .map(move |project| root.join(project).join("chats"))
        })
        .filter(|dir| config::dir_exists(dir))
        .flat_map(|dir| {
            config::list_dir(&dir)
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
fn last_turn_ms(path: &Path) -> Option<i64> {
    if path.extension().is_none_or(|ext| ext != "jsonl") {
        return None;
    }
    extract::last_entry_ms(path, |bytes| {
        let record = Record::parse(bytes)?;
        if !record.kind.is("user") && !record.kind.is("gemini") {
            return None;
        }
        util::parse_ts(record.timestamp.get()?).map(|d| d.timestamp_millis())
    })
}

fn summarize(path: PathBuf) -> Option<Session> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let started = started_from_stem(&stem)?;

    // Resuming a chat opens a new file under the same `sessionId`, and the
    // segments are disjoint rather than cumulative. Keying rows on that id
    // would collapse them onto each other and lose every earlier segment's
    // usage, so the filename — unique, and what Gemini itself indexes by — is
    // the identity here.
    let mut session = Session::new(Provider::Gemini, stem);
    session.surface = Surface::Cli;
    session.started_at = started;
    let touched = config::file_mtime_ms(&path) as i64;
    session.touched_at = util::ms_to_rfc3339(touched);
    session.last_active = util::ms_to_rfc3339(last_turn_ms(&path).unwrap_or(touched));
    session.label_source = project_root(path.parent()?);
    session.data_file = Some(path);
    Some(session)
}

pub fn list_sessions() -> Vec<Session> {
    let mut sessions: Vec<_> = chat_files().into_par_iter().filter_map(summarize).collect();
    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    sessions
}

/// One assistant turn's token counts, in cctop's shape.
///
/// Verified against 605 recorded turns: `total` equals `input + output +
/// thoughts + tool` exactly, and `cached` is never among the addends — it is
/// the part of `input` that was served from context cache. So the full-rate
/// input is the uncached remainder, which is the same split Codex reports.
fn turn_tokens(usage: &TokenCounts) -> Tokens {
    let count = TokenCounts::count;
    let input_total = count(&usage.input);
    let cached_input = count(&usage.cached).min(input_total);
    Tokens {
        input: input_total - cached_input,
        input_total,
        cached_input,
        output: count(&usage.output),
        reasoning_output: count(&usage.thoughts),
        // Gemini's `tool` bucket is counted into `total` but has no rate of its
        // own and reads zero on every turn seen so far; leave it to `total`
        // rather than folding it into a bucket that would be billed.
        total: count(&usage.total),
        ..Default::default()
    }
}

fn add(target: &mut Tokens, turn: &Tokens) {
    target.input += turn.input;
    target.input_total += turn.input_total;
    target.cached_input += turn.cached_input;
    target.output += turn.output;
    target.reasoning_output += turn.reasoning_output;
    target.total += turn.total;
}

/// What one turn cost at the model's published rates.
///
/// Thinking tokens bill as output, so they join `output` here instead of being
/// dropped — they are the larger half of a reasoning turn's output on Gemini 3.
fn turn_costs(tokens: &Tokens, rates: &pricing::CodexPricing) -> Costs {
    let input = util::token_cost(tokens.input, rates.input);
    let cached_input = util::token_cost(tokens.cached_input, rates.cached_input);
    let output = util::token_cost(tokens.output + tokens.reasoning_output, rates.output);
    Costs {
        input,
        cached_input,
        output,
        total: input + cached_input + output,
        ..Default::default()
    }
}

fn add_costs(target: &mut Costs, turn: &Costs) {
    target.input += turn.input;
    target.cached_input += turn.cached_input;
    target.output += turn.output;
    target.total += turn.total;
}

/// What the table needs from one `gemini` message, owned so it can outlive the
/// line it was read from.
struct Turn {
    model: String,
    ts: String,
    tokens: Option<Tokens>,
    calls: Vec<Call>,
}

struct Call {
    name: String,
    args: Value,
    id: Option<String>,
    ts: Option<String>,
    failed: bool,
}

impl Turn {
    fn read(record: &Record) -> Turn {
        Turn {
            model: record.model.get().unwrap_or("unknown").to_string(),
            ts: record.timestamp.or_empty().to_string(),
            tokens: record.tokens.get().map(turn_tokens),
            calls: record
                .calls()
                .map(|call| Call {
                    name: call.name.get().unwrap_or("unknown").to_string(),
                    args: call.args.value(),
                    id: call.id.get().map(str::to_string),
                    ts: call.timestamp.get().map(str::to_string),
                    failed: call.status.is("error"),
                })
                .collect(),
        }
    }
}

/// Parse one Gemini chat file.
///
/// Gemini records what a turn consumed but never what it cost, so cost is
/// estimated from the model's published per-token rates, exactly as Codex's is.
/// Rates are looked up through [`pricing::resolve_codex`]: the name is Codex's
/// but the shape — flat input, cached input, output, with no cache-write tier —
/// is Gemini's billing model too, and duplicating the resolver to rename it
/// would buy nothing.
///
/// A message written again under an id already seen replaces the earlier copy
/// rather than adding to it. An append-only log revises a message — a call's
/// result arriving, its status settling — by writing it out again, and summing
/// both copies would count the turn's tokens and calls twice. A message with
/// no id is always its own.
pub fn extract(path: &Path) -> SessionData {
    let mut data = SessionData::default();
    let mut turns: Vec<Turn> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();

    let result = gemini_line::for_each(path, |record| {
        // Header records and `$set` patches both carry the topic Gemini's
        // `update_topic` tool writes, which is the closest thing to a title.
        if let Some(summary) = record.summary.get() {
            data.title = Some(summary.to_string());
        }
        if !record.kind.is("gemini") {
            return;
        }
        let turn = Turn::read(record);
        match record.id.get() {
            Some(id) => match by_id.get(id) {
                Some(&at) => turns[at] = turn,
                None => {
                    by_id.insert(id.to_string(), turns.len());
                    turns.push(turn);
                }
            },
            None => turns.push(turn),
        }
    });

    if let Err(err) = result {
        data.error = Some(format!(
            "Could not read Gemini chat {}: {err}",
            path.display()
        ));
        return data;
    }

    let mut breakdown: HashMap<String, (Tokens, Costs)> = HashMap::new();
    for turn in turns {
        if let Some(tokens) = &turn.tokens {
            data.last_model = turn.model.clone();
            let costs = turn_costs(tokens, &pricing::resolve_codex(&turn.model));
            add(&mut data.tokens, tokens);
            add_costs(&mut data.costs, &costs);
            let (model_tokens, model_costs) = breakdown.entry(turn.model.clone()).or_default();
            add(model_tokens, tokens);
            add_costs(model_costs, &costs);

            if let Some(dt) = util::parse_ts(&turn.ts) {
                data.record_cost(&dt, &turn.model, costs.total);
                // All input kinds plus output — and thinking, which Gemini
                // bills as output. `total` is not reused because it folds in
                // the unbilled `tool` bucket.
                let billed =
                    tokens.input + tokens.cached_input + tokens.output + tokens.reasoning_output;
                *data
                    .tokens_by_day
                    .entry(util::local_date_key(&dt))
                    .or_default()
                    .entry(turn.model.clone())
                    .or_insert(0) += billed;
                *data
                    .tokens_by_hour
                    .entry(util::local_hour_key(&dt))
                    .or_default()
                    .entry(turn.model.clone())
                    .or_insert(0) += billed;
            }
        }

        for call in turn.calls {
            let name = call.name.as_str();
            let (short, full) = extract::tool_detail(name, &call.args);
            extract::push_tool_detail(
                &mut data.metrics.tool_details,
                name,
                short,
                full,
                call.ts.unwrap_or_else(|| turn.ts.clone()),
                call.id,
                None,
            );
            *data.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
            data.metrics.tool_count += 1;
            data.metrics.tool_errors += u64::from(call.failed);
            // The edit's own arguments are its diff: Gemini keeps no patch, so
            // the lines it changed are read off what it was asked to write.
            let delta = matches!(name, "replace" | "write_file")
                .then(|| extract::edit_delta(&call.args))
                .flatten();
            if let Some(detail) = data
                .metrics
                .tool_details
                .get_mut(name)
                .and_then(|details| details.last_mut())
            {
                detail.failed = call.failed;
                if let Some(delta) = delta.filter(|_| !call.failed) {
                    data.metrics.lines_added += delta.added as u64;
                    data.metrics.lines_removed += delta.removed as u64;
                    detail.delta = Some(delta);
                }
            }
        }
    }

    let mut models: Vec<_> = breakdown.into_iter().collect();
    models.sort_by(|a, b| a.0.cmp(&b.0));
    data.models = models.iter().map(|(model, _)| model.clone()).collect();
    data.model_breakdown = models
        .into_iter()
        .map(|(model, (tokens, costs))| ModelBreakdown {
            model,
            total: costs.total,
            tokens,
            costs,
        })
        .collect();
    data
}

/// Every Gemini model the CLI runs — 2.5 Pro and Flash, 3 Pro and Flash — takes
/// a million-token prompt, so a model LiteLLM has not listed yet is still
/// measured against the window it almost certainly has rather than none.
const GEMINI_DEFAULT_CTX: u64 = 1_048_576;

/// The newest `gemini` message in the chat's tail, read whole.
///
/// ponytail: the JSONL shape only, for the reason [`last_turn_ms`] gives.
fn last_reply<T>(path: &Path, mut f: impl FnMut(&Record) -> Option<T>) -> Option<T> {
    if path.extension().is_none_or(|ext| ext != "jsonl") {
        return None;
    }
    let text = util::read_tail(path, 65_536)?;
    text.lines().rev().find_map(|line| {
        let record = Record::parse(line.trim().as_bytes())?;
        if !record.kind.is("gemini") {
            return None;
        }
        f(&record)
    })
}

/// Context-window use: the prompt the newest reply was sent, which is what
/// Gemini counts as `input` — cached part included, as Codex's is.
pub fn extract_context(session: &Session) -> Option<ContextUsage> {
    let path = session.data_file.as_deref()?;
    let (used, model) = last_reply(path, |record| {
        let used = TokenCounts::count(&record.tokens.get()?.input);
        (used > 0).then(|| (used, record.model.or_empty().to_string()))
    })?;
    Some(ContextUsage {
        used,
        max: pricing::litellm_max_input_tokens(&model).unwrap_or(GEMINI_DEFAULT_CTX),
        compacted: false,
    })
}

/// Name of the newest call in the chat's tail.
pub fn extract_last_tool(session: &Session) -> String {
    session
        .data_file
        .as_deref()
        .and_then(|path| {
            last_reply(path, |record| {
                record
                    .calls()
                    .filter_map(|call| call.name.get())
                    .last()
                    .map(str::to_string)
            })
        })
        .unwrap_or_default()
}

/// A Gemini session is its one JSONL file.
pub use super::delete_transcript as delete;

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, body: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("cctop-gemini-{}-{name}", std::process::id()));
        std::fs::write(&path, body).expect("write fixture");
        path
    }

    /// A JSONL chat is dated by its newest message, not by the header patch
    /// written after it.
    #[test]
    fn a_jsonl_chat_is_dated_by_its_last_message() {
        let path = fixture(
            "last.jsonl",
            concat!(
                "{\"sessionId\":\"s\",\"startTime\":\"2026-05-14T17:34:00.000Z\"}\n",
                "{\"type\":\"user\",\"timestamp\":\"2026-05-14T17:34:06.028Z\",\"content\":[]}\n",
                "{\"type\":\"gemini\",\"timestamp\":\"2026-05-14T17:34:13.475Z\",\"content\":\"hi\"}\n",
                "{\"$set\":{\"lastUpdated\":\"2026-05-15T09:00:00.000Z\"}}\n",
            ),
        );
        let expected = util::parse_ts("2026-05-14T17:34:13.475Z").map(|d| d.timestamp_millis());
        assert_eq!(last_turn_ms(&path), expected);
        let _ = std::fs::remove_file(&path);
    }

    /// The recorded counts are the only usage evidence Gemini leaves, so the
    /// split between them has to survive intact: cached input is part of the
    /// prompt, not an addition to it, and thinking tokens are output.
    #[test]
    fn splits_cached_input_out_of_the_prompt_and_bills_thoughts_as_output() {
        let path = fixture(
            "usage.jsonl",
            concat!(
                "{\"sessionId\":\"79709c93\",\"startTime\":\"2026-05-14T17:34:01.387Z\",\"kind\":\"main\"}\n",
                "{\"$set\":{\"summary\":\"Improving script latency\"}}\n",
                "{\"type\":\"user\",\"timestamp\":\"2026-05-14T17:34:06.028Z\",\"content\":[{\"text\":\"go\"}]}\n",
                "{\"type\":\"gemini\",\"timestamp\":\"2026-05-14T17:34:13.475Z\",\"model\":\"gemini-test\",",
                "\"tokens\":{\"input\":8678,\"output\":133,\"cached\":8000,\"thoughts\":364,\"tool\":0,\"total\":9175},",
                "\"toolCalls\":[{\"id\":\"read_file_1\",\"name\":\"read_file\",\"args\":{\"file_path\":\"README.md\"},\"status\":\"success\"},",
                "{\"id\":\"replace_1\",\"name\":\"replace\",\"args\":{\"file_path\":\"src/main.rs\"},\"status\":\"error\"}]}\n",
            ),
        );

        let data = extract(&path);
        std::fs::remove_file(&path).expect("remove fixture");

        assert_eq!(data.title.as_deref(), Some("Improving script latency"));
        assert_eq!(data.last_model, "gemini-test");
        assert_eq!(data.tokens.input_total, 8678);
        assert_eq!(data.tokens.cached_input, 8000);
        assert_eq!(
            data.tokens.input, 678,
            "cached input is not billed at full rate"
        );
        assert_eq!(data.tokens.reasoning_output, 364);
        assert_eq!(data.tokens.total, 9175);
        // `all_input` must rebuild the prompt Gemini reported, not double it.
        assert_eq!(data.tokens.all_input(), 8678);
        // The day's token bucket counts thinking as billed output but not the
        // unbilled `tool` bucket inside `total`: 678 + 8000 + 133 + 364.
        assert_eq!(
            data.tokens_by_day
                .values()
                .flat_map(|m| m.values())
                .sum::<u64>(),
            9175
        );

        assert_eq!(data.metrics.tool_count, 2);
        assert_eq!(data.metrics.tools.get("read_file"), Some(&1));
        assert!(!data.metrics.tool_details["read_file"][0].failed);
        assert!(
            data.metrics.tool_details["replace"][0].failed,
            "status=error must be flagged"
        );
    }

    /// The older whole-file shape has to yield the same figures as the newer
    /// append-only one; only the packaging differs.
    #[test]
    fn reads_the_single_object_shape_identically() {
        let path = fixture(
            "whole.json",
            concat!(
                "{\"sessionId\":\"62ae72f3\",\"startTime\":\"2026-03-23T07:17:09.209Z\",",
                "\"summary\":\"Clarify bundling\",\"messages\":[",
                "{\"type\":\"gemini\",\"timestamp\":\"2026-03-23T07:17:13.148Z\",\"model\":\"gemini-test\",",
                "\"tokens\":{\"input\":9679,\"output\":46,\"cached\":0,\"thoughts\":120,\"tool\":0,\"total\":9845},",
                "\"toolCalls\":[{\"id\":\"read_file_2\",\"name\":\"read_file\",\"args\":{\"file_path\":\"docs/x.md\"},\"status\":\"success\"}]}]}",
            ),
        );

        let data = extract(&path);
        std::fs::remove_file(&path).expect("remove fixture");

        assert_eq!(data.title.as_deref(), Some("Clarify bundling"));
        assert_eq!(data.tokens.total, 9845);
        assert_eq!(data.tokens.input, 9679);
        assert_eq!(data.tokens.cached_input, 0);
        assert_eq!(data.metrics.tool_count, 1);
        assert_eq!(data.model_breakdown.len(), 1);
    }

    /// A hand-built chat in the JSONL shape: a header, a prompt, a reply that
    /// is written once while its call runs and again once it has finished,
    /// an `info` line, a second reply with an edit, and a header patch.
    const CHAT: &str = concat!(
        "{\"sessionId\":\"s\",\"startTime\":\"2026-05-14T17:34:00.000Z\",\"kind\":\"main\"}\n",
        "{\"id\":\"u1\",\"type\":\"user\",\"timestamp\":\"2026-05-14T17:34:06.000Z\",\"content\":[{\"text\":\"go\"}]}\n",
        "{\"id\":\"g1\",\"type\":\"gemini\",\"timestamp\":\"2026-05-14T17:34:10.000Z\",\"model\":\"gemini-test\",",
        "\"tokens\":{\"input\":1000,\"output\":10,\"cached\":0,\"thoughts\":5,\"tool\":0,\"total\":1015},",
        "\"toolCalls\":[{\"id\":\"c1\",\"name\":\"run_shell_command\",\"args\":{\"command\":\"ls\"}}]}\n",
        "{\"id\":\"g1\",\"type\":\"gemini\",\"timestamp\":\"2026-05-14T17:34:10.000Z\",\"model\":\"gemini-test\",",
        "\"tokens\":{\"input\":1000,\"output\":10,\"cached\":0,\"thoughts\":5,\"tool\":0,\"total\":1015},",
        "\"toolCalls\":[{\"id\":\"c1\",\"name\":\"run_shell_command\",\"args\":{\"command\":\"ls\"},\"status\":\"error\",\"result\":[{\"x\":1}]}]}\n",
        "{\"id\":\"i1\",\"type\":\"info\",\"timestamp\":\"2026-05-14T17:34:11.000Z\",\"content\":\"Switched\"}\n",
        "{\"id\":\"g2\",\"type\":\"gemini\",\"timestamp\":\"2026-05-14T17:34:20.000Z\",\"model\":\"gemini-test\",",
        "\"tokens\":{\"input\":4000,\"output\":20,\"cached\":3000,\"thoughts\":0,\"tool\":0,\"total\":4020},",
        "\"toolCalls\":[{\"id\":\"c2\",\"name\":\"replace\",\"args\":{\"file_path\":\"src/a.rs\",\"old_string\":\"a\",\"new_string\":\"b\\nc\"},\"status\":\"success\"},",
        "{\"id\":\"c3\",\"name\":\"read_file\",\"args\":{\"file_path\":\"src/b.rs\"},\"status\":\"success\"}],",
        "\"futureField\":{\"nested\":[1,2]}}\n",
        "{\"$set\":{\"lastUpdated\":\"2026-05-14T17:35:00.000Z\"}}\n",
    );

    /// A reply written twice under one id is one turn: its tokens and its
    /// call are counted once, with the outcome the later copy settled on.
    #[test]
    fn a_rewritten_message_counts_once_with_its_final_outcome() {
        let path = fixture("rewrite.jsonl", CHAT);
        let data = extract(&path);
        std::fs::remove_file(&path).expect("remove fixture");

        assert_eq!(data.tokens.input_total, 5000);
        assert_eq!(data.tokens.cached_input, 3000);
        assert_eq!(data.tokens.output, 30);
        assert_eq!(data.metrics.tool_count, 3);
        assert_eq!(data.metrics.tools["run_shell_command"], 1);
        assert_eq!(data.metrics.tool_errors, 1);
        assert!(data.metrics.tool_details["run_shell_command"][0].failed);
        // The edit's arguments are its diff, and the path is the detail.
        assert_eq!(data.metrics.lines_added, 2);
        assert_eq!(data.metrics.lines_removed, 1);
        assert_eq!(data.metrics.tool_details["replace"][0].d, "src/a.rs");
        assert_eq!(data.metrics.tool_details["read_file"][0].d, "src/b.rs");
    }

    /// The context meter reads the newest reply's prompt, and the last-tool
    /// cell the newest call — both from the tail, past the header patch.
    #[test]
    fn context_and_last_tool_come_from_the_newest_reply() {
        let path = fixture("tail.jsonl", CHAT);
        let mut session = Session::new(Provider::Gemini, "s".into());
        session.data_file = Some(path.clone());
        let context = extract_context(&session);
        let last_tool = extract_last_tool(&session);
        std::fs::remove_file(&path).expect("remove fixture");

        let context = context.expect("a context figure");
        assert_eq!(context.used, 4000);
        // `gemini-test` is in no table: the Gemini window stands in.
        assert_eq!(context.max, GEMINI_DEFAULT_CTX);
        assert_eq!(last_tool, "read_file");
    }

    /// A reply too long to build — a tool output past the line cap — still
    /// counts its tokens and calls; only its arguments are dropped.
    #[test]
    fn an_oversized_reply_keeps_its_tokens() {
        let filler = "x".repeat(crate::config::MAX_JSONL_LINE_BYTES + 1);
        let body = format!(
            "{{\"id\":\"g\",\"type\":\"gemini\",\"timestamp\":\"2026-05-14T17:34:10.000Z\",\"model\":\"gemini-test\",\
             \"tokens\":{{\"input\":7,\"output\":3,\"total\":10}},\
             \"toolCalls\":[{{\"name\":\"write_file\",\"args\":{{\"file_path\":\"f\",\"content\":\"{filler}\"}},\"status\":\"success\"}}]}}\n"
        );
        let path = fixture("big.jsonl", &body);
        let data = extract(&path);
        std::fs::remove_file(&path).expect("remove fixture");

        assert_eq!(data.tokens.input_total, 7);
        assert_eq!(data.metrics.tool_count, 1);
        assert_eq!(data.metrics.lines_added, 0);
    }

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
