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

use super::extract::{self, for_each_jsonl};
use super::{Costs, ModelBreakdown, Session, SessionData, Surface, Tokens};
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
fn for_each_record(path: &Path, mut f: impl FnMut(&Value)) -> std::io::Result<()> {
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
    config::list_dir(&config::GEMINI_CHATS_ROOT)
        .into_iter()
        .map(|project| config::GEMINI_CHATS_ROOT.join(project).join("chats"))
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
    session.harness = "Gemini".into();
    session.started_at = started;
    session.last_active = util::ms_to_rfc3339(config::file_mtime_ms(&path) as i64);
    session.label_source = project_root(path.parent()?);
    session.data_file = Some(path);
    Some(session)
}

pub fn list_sessions() -> Vec<Session> {
    if !config::dir_exists(&config::GEMINI_CHATS_ROOT) {
        return Vec::new();
    }
    let mut sessions: Vec<_> = chat_files().into_par_iter().filter_map(summarize).collect();
    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    sessions
}

fn token(tokens: &Value, key: &str) -> u64 {
    tokens.get(key).and_then(Value::as_u64).unwrap_or(0)
}

/// One assistant turn's token counts, in cctop's shape.
///
/// Verified against 605 recorded turns: `total` equals `input + output +
/// thoughts + tool` exactly, and `cached` is never among the addends — it is
/// the part of `input` that was served from context cache. So the full-rate
/// input is the uncached remainder, which is the same split Codex reports.
fn turn_tokens(usage: &Value) -> Tokens {
    let input_total = token(usage, "input");
    let cached_input = token(usage, "cached").min(input_total);
    Tokens {
        input: input_total - cached_input,
        input_total,
        cached_input,
        output: token(usage, "output"),
        reasoning_output: token(usage, "thoughts"),
        // Gemini's `tool` bucket is counted into `total` but has no rate of its
        // own and reads zero on every turn seen so far; leave it to `total`
        // rather than folding it into a bucket that would be billed.
        total: token(usage, "total"),
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

/// Parse one Gemini chat file.
///
/// Gemini records what a turn consumed but never what it cost, so cost is
/// estimated from the model's published per-token rates, exactly as Codex's is.
/// Rates are looked up through [`pricing::resolve_codex`]: the name is Codex's
/// but the shape — flat input, cached input, output, with no cache-write tier —
/// is Gemini's billing model too, and duplicating the resolver to rename it
/// would buy nothing.
pub fn extract(path: &Path) -> SessionData {
    let mut data = SessionData::default();
    let mut breakdown: HashMap<String, (Tokens, Costs)> = HashMap::new();

    let result = for_each_record(path, |item| {
        // Header records and `$set` patches both carry the topic Gemini's
        // `update_topic` tool writes, which is the closest thing to a title.
        if let Some(summary) = item.get("summary").and_then(Value::as_str) {
            data.title = Some(summary.to_string());
        }
        if item.get("type").and_then(Value::as_str) != Some("gemini") {
            return;
        }

        let model = item
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let ts = item
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        if let Some(usage) = item.get("tokens") {
            data.last_model = model.clone();
            let tokens = turn_tokens(usage);
            let costs = turn_costs(&tokens, &pricing::resolve_codex(&model));
            add(&mut data.tokens, &tokens);
            add_costs(&mut data.costs, &costs);
            let (model_tokens, model_costs) = breakdown.entry(model.clone()).or_default();
            add(model_tokens, &tokens);
            add_costs(model_costs, &costs);

            if let Some(dt) = util::parse_ts(&ts) {
                *data
                    .costs_by_day
                    .entry(util::local_date_key(&dt))
                    .or_default()
                    .entry(model.clone())
                    .or_insert(0.0) += costs.total;
                *data
                    .costs_by_hour
                    .entry(util::local_hour_key(&dt))
                    .or_default()
                    .entry(model.clone())
                    .or_insert(0.0) += costs.total;
            }
        }

        for call in item
            .get("toolCalls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = call
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let args = call.get("args").unwrap_or(&Value::Null);
            let (short, full) = extract::tool_detail(name, args);
            extract::push_tool_detail(
                &mut data.metrics.tool_details,
                name,
                short,
                full,
                call.get("timestamp")
                    .and_then(Value::as_str)
                    .unwrap_or(&ts)
                    .to_string(),
                call.get("id").and_then(Value::as_str).map(str::to_string),
                None,
            );
            *data.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
            data.metrics.tool_count += 1;
            let failed = call.get("status").and_then(Value::as_str) == Some("error");
            data.metrics.tool_errors += u64::from(failed);
            if let Some(detail) = data
                .metrics
                .tool_details
                .get_mut(name)
                .and_then(|details| details.last_mut())
            {
                detail.failed = failed;
            }
        }
    });

    if let Err(err) = result {
        data.error = Some(format!(
            "Could not read Gemini chat {}: {err}",
            path.display()
        ));
        return data;
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

pub fn delete(session: &Session) -> std::io::Result<()> {
    let Some(path) = &session.data_file else {
        return Ok(());
    };
    std::fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, body: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("cctop-gemini-{}-{name}", std::process::id()));
        std::fs::write(&path, body).expect("write fixture");
        path
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
