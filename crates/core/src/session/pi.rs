//! Pi coding-agent session discovery and JSONL extraction.
//!
//! Every read goes through the typed entries of [`super::pi_line`]. What the
//! table shows for a Pi session is what Pi wrote down: each turn's tokens and
//! dollars, each tool call and whether its result was an error, the thinking
//! level, compactions, and — from the newest turn's usage — how full the
//! window is.

use super::extract;
use super::lenient::Lenient;
use super::pi_line;
use super::{ContextUsage, Costs, FallbackRates, ModelBreakdown, Session, SessionData, Tokens};
use crate::config;
use crate::pricing::Provider;
use crate::util;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// When a Pi entry was written, from whichever of the two stamps carries it.
///
/// The message's own millisecond stamp is the one the harness wrote as the
/// message was sealed; the entry's ISO string is stamped as the line was
/// opened, and an entry still being streamed has no message stamp yet.
pub fn message_ts(item: &Value, message: &Value) -> String {
    message
        .get("timestamp")
        .and_then(Value::as_i64)
        .map(util::ms_to_rfc3339)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            item.get("timestamp")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// Discover every persisted Pi session, including custom session roots.
pub fn list_sessions() -> Vec<Session> {
    let mut sessions = Vec::new();
    let paths: Vec<_> = config::pi_sessions_roots()
        .iter()
        .flat_map(|root| config::rglob(root, ".jsonl"))
        .collect();
    for path in paths {
        let mut session_id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut started = String::new();
        let mut last = String::new();
        let mut cwd = String::new();
        let mut title = None;
        let mut model = String::new();

        let _ = pi_line::for_each(&path, |entry| match entry.kind.or_empty() {
            "session" => {
                if let Some(id) = entry.id.get() {
                    session_id = id.to_string();
                }
                started = entry.timestamp.or_empty().to_string();
                cwd = entry.cwd.or_empty().to_string();
                last = started.clone();
            }
            "session_info" => title = entry.name.get().map(str::to_string),
            "message" => {
                let Some(message) = entry.message() else {
                    return;
                };
                // Only a message is the session doing something: a rename or a
                // model switch written afterwards is not activity.
                let ts = entry.ts();
                if ts > last {
                    last = ts;
                }
                if message.role.is("assistant")
                    && let Some(m) = message.model.get()
                {
                    model = m.to_string();
                }
            }
            _ => {}
        });

        if session_id.is_empty() || started.is_empty() {
            continue;
        }
        let mut session = Session::new(Provider::Pi, session_id);
        session.started_at = started;
        session.last_active = last;
        session.label_source = cwd;
        session.title = title;
        session.model = model;
        session.data_file = Some(path);
        sessions.push(session);
    }
    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    sessions
}

/// The lines an edit or a write changed, from the arguments Pi's own tools
/// take.
///
/// `edit` replaces `oldText` with `newText`, or each such pair in a list under
/// `edits`; `write` is handed the whole file as `content`. Pi's spellings are
/// mapped onto the ones [`extract::edit_delta`] reads rather than taught to
/// every other harness's reader.
fn edit_delta(name: &str, args: &Value) -> Option<super::Delta> {
    let pair = |v: &Value| {
        serde_json::json!({
            "old_string": v.get("oldText"),
            "new_string": v.get("newText"),
        })
    };
    match name {
        "edit" => {
            let pairs: Vec<Value> = match args.get("edits").and_then(Value::as_array) {
                Some(edits) => edits.iter().map(pair).collect(),
                None => vec![pair(args)],
            };
            let mut total = super::Delta::default();
            for delta in pairs.iter().filter_map(extract::edit_delta) {
                total.added += delta.added;
                total.removed += delta.removed;
                let room = crate::config::MAX_DIFF_LINES.saturating_sub(total.hunks.len());
                total.hunks.extend(delta.hunks.into_iter().take(room));
            }
            (total.added > 0 || total.removed > 0).then_some(total)
        }
        // ponytail: a write's removed lines are whatever the file held before,
        // which the call does not record; only what it wrote is counted.
        "write" => extract::edit_delta(&serde_json::json!({ "content": args.get("content") })),
        _ => None,
    }
}

fn count(n: &Lenient<u64>) -> u64 {
    n.value().unwrap_or(0)
}

fn dollars(n: &Lenient<f64>) -> f64 {
    n.value().unwrap_or(0.0)
}

/// Parse Pi's documented session JSONL format.
///
/// Usage costs are recorded by Pi and preserved exactly rather than re-estimated
/// from a model ID. The exception is a turn Pi priced at nothing while reporting
/// tokens, which is what a provider it has no rates for produces: those are
/// priced from LiteLLM, since reporting them as free hides real spend.
///
/// Every tool call is answered by a `toolResult` message naming it, which says
/// whether it failed and, by its own stamp, when it finished.
pub fn extract(path: &Path) -> SessionData {
    let mut data = SessionData::default();
    let mut breakdown: HashMap<String, (Tokens, Costs)> = HashMap::new();
    let mut rates = FallbackRates::default();
    // Call id -> the tool it called, and when the turn that made it was sealed.
    let mut open_calls: HashMap<String, (String, Option<i64>)> = HashMap::new();

    let result = pi_line::for_each(path, |entry| {
        match entry.kind.or_empty() {
            "session_info" => {
                data.title = entry.name.get().map(str::to_string);
                return;
            }
            "thinking_level_change" => {
                data.reasoning_effort = entry.thinking_level.get().map(str::to_string);
                return;
            }
            "compaction" => {
                data.compactions += 1;
                return;
            }
            _ => {}
        }
        let Some(message) = entry.message() else {
            return;
        };
        if message.role.is("toolResult") {
            let Some(id) = message.tool_call_id.get() else {
                return;
            };
            let failed = message.is_error.value() == Some(true);
            data.metrics.tool_errors += u64::from(failed);
            let Some((name, issued)) = open_calls.remove(id) else {
                return;
            };
            let dur_ms = issued
                .zip(message.timestamp.value())
                .map(|(start, end)| (end - start).max(0));
            if let Some(detail) = data
                .metrics
                .tool_details
                .get_mut(&name)
                .and_then(|list| list.iter_mut().rev().find(|d| d.id.as_deref() == Some(id)))
            {
                detail.failed = failed;
                detail.dur_ms = dur_ms;
            }
            return;
        }
        if !message.role.is("assistant") {
            return;
        }

        let model = message.model.get().unwrap_or("unknown").to_string();
        data.last_model = model.clone();
        let no_usage = pi_line::Usage::default();
        let usage = message.usage.get().unwrap_or(&no_usage);
        let cache_write = count(&usage.cache_write);
        let cache_write_1h = count(&usage.cache_write_1h).min(cache_write);
        let tokens = Tokens {
            input: count(&usage.input),
            output: count(&usage.output),
            cache_read: count(&usage.cache_read),
            cache_write_5m: cache_write - cache_write_1h,
            cache_write_1h,
            reasoning_output: count(&usage.reasoning),
            total: count(&usage.total_tokens),
            ..Default::default()
        };
        let reported = usage.cost.get().map_or_else(Costs::default, |cost| Costs {
            input: dollars(&cost.input),
            output: dollars(&cost.output),
            cache_read: dollars(&cost.cache_read),
            cache_write_5m: dollars(&cost.cache_write),
            total: dollars(&cost.total),
            ..Default::default()
        });
        let billable = tokens.input + tokens.output + tokens.cache_read + tokens.cache_write_5m;
        let costs = if reported.total > 0.0 || billable == 0 {
            reported
        } else {
            rates.costs(&model, &tokens).unwrap_or(reported)
        };

        data.tokens.input += tokens.input;
        data.tokens.output += tokens.output;
        data.tokens.cache_read += tokens.cache_read;
        data.tokens.cache_write_5m += tokens.cache_write_5m;
        data.tokens.cache_write_1h += tokens.cache_write_1h;
        data.tokens.reasoning_output += tokens.reasoning_output;
        data.tokens.total += tokens.total;
        data.costs.input += costs.input;
        data.costs.output += costs.output;
        data.costs.cache_read += costs.cache_read;
        data.costs.cache_write_5m += costs.cache_write_5m;
        data.costs.total += costs.total;

        let (mt, mc) = breakdown.entry(model.clone()).or_default();
        mt.input += tokens.input;
        mt.output += tokens.output;
        mt.cache_read += tokens.cache_read;
        mt.cache_write_5m += tokens.cache_write_5m;
        mt.cache_write_1h += tokens.cache_write_1h;
        mt.reasoning_output += tokens.reasoning_output;
        mt.total += tokens.total;
        mc.input += costs.input;
        mc.output += costs.output;
        mc.cache_read += costs.cache_read;
        mc.cache_write_5m += costs.cache_write_5m;
        mc.total += costs.total;

        let ts = entry.ts();
        if let Some(dt) = util::parse_ts(&ts) {
            data.record_cost(&dt, &model, costs.total);
            // `billable` above is only a did-anything-happen check and skips
            // the 1h cache-write split; the bucket counts everything billed.
            let billed = tokens.all_input() + tokens.output;
            *data
                .tokens_by_day
                .entry(util::local_date_key(&dt))
                .or_default()
                .entry(model.clone())
                .or_insert(0) += billed;
            *data
                .tokens_by_hour
                .entry(util::local_hour_key(&dt))
                .or_default()
                .entry(model.clone())
                .or_insert(0) += billed;
        }

        for call in message.calls() {
            let name = call.name.get().unwrap_or("unknown");
            let args = call.arguments.value();
            let (short, full) = extract::tool_detail(name, &args);
            let id = call.id.get().map(str::to_string);
            extract::push_tool_detail(
                &mut data.metrics.tool_details,
                name,
                short,
                full,
                ts.clone(),
                id.clone(),
                None,
            );
            *data.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
            data.metrics.tool_count += 1;
            if let Some(id) = id {
                open_calls.insert(id, (name.to_string(), message.timestamp.value()));
            }
            if let Some(delta) = edit_delta(name, &args) {
                data.metrics.lines_added += u64::from(delta.added);
                data.metrics.lines_removed += u64::from(delta.removed);
                if let Some(detail) = data
                    .metrics
                    .tool_details
                    .get_mut(name)
                    .and_then(|list| list.last_mut())
                {
                    detail.delta = Some(delta);
                }
            }
        }
    });

    if let Err(err) = result {
        data.error = Some(format!(
            "Could not read Pi session {}: {err}",
            path.display()
        ));
        return data;
    }
    let mut models: Vec<_> = breakdown.into_iter().collect();
    models.sort_by(|a, b| a.0.cmp(&b.0));
    data.models = models.iter().map(|(m, _)| m.clone()).collect();
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

/// A Pi session is its one JSONL file.
pub use super::delete_transcript as delete;

/// How far back [`extract_context`] looks for a turn that measured the window.
/// A run of tool results between two turns is the usual distance, and a few
/// dozen of those is already a long one.
const CONTEXT_TAIL_LINES: usize = 200;

/// Context usage from the newest assistant turn that reported tokens.
///
/// Pi records each request's input, cache reads and cache writes, which
/// together are the window the request filled — the same reading OpenCode's
/// is. It records no window size, so the ceiling is LiteLLM's for the model;
/// without one there is no honest denominator and nothing is shown.
pub fn extract_context(session: &Session) -> Option<ContextUsage> {
    let path = session.data_file.as_ref()?;
    let mut found: Option<(u64, String)> = None;
    let mut compacted = false;
    extract::scan_last_lines(path, CONTEXT_TAIL_LINES, |bytes| {
        if found.is_some() {
            return true;
        }
        let Some(entry) = pi_line::Entry::parse(bytes) else {
            return false;
        };
        // Newest first, so a compaction seen before any measured turn is one
        // no request has measured the new window after.
        if entry.kind.is("compaction") {
            compacted = true;
        }
        if let Some(message) = entry.message()
            && message.role.is("assistant")
            && let Some(used) = message.usage.get().map(pi_line::Usage::window)
            && used > 0
        {
            let model = message
                .model
                .get()
                .filter(|m| !m.is_empty())
                .unwrap_or(&session.model);
            found = Some((used, model.to_string()));
        }
        true
    });
    let (used, model) = found?;
    Some(ContextUsage {
        used,
        max: crate::pricing::litellm_max_input_tokens(&model)?,
        compacted,
    })
}

pub fn extract_last_tool(session: &Session) -> String {
    let Some(path) = &session.data_file else {
        return String::new();
    };
    let mut last = String::new();
    let _ = pi_line::for_each(path, |entry| {
        if let Some(name) = entry
            .message()
            .and_then(|m| m.calls().filter_map(|c| c.name.get()).last())
        {
            last = name.to_string();
        }
    });
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_recorded_usage_cost_and_tools() {
        let path = std::env::temp_dir().join(format!("cctop-pi-{}.jsonl", std::process::id()));
        let fixture = concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"pi-1\",\"timestamp\":\"2026-08-05T00:00:00Z\",\"cwd\":\"/work\"}\n",
            "{\"type\":\"session_info\",\"name\":\"Pi work\"}\n",
            "{\"type\":\"message\",\"timestamp\":\"2026-08-05T00:01:00Z\",\"message\":{\"role\":\"assistant\",\"model\":\"gpt-test\",\"timestamp\":1785888060000,\"usage\":{\"input\":100,\"output\":20,\"cacheRead\":50,\"cacheWrite\":10,\"totalTokens\":180,\"cost\":{\"input\":0.1,\"output\":0.2,\"cacheRead\":0.01,\"cacheWrite\":0.02,\"total\":0.33}},\"content\":[{\"type\":\"toolCall\",\"id\":\"call-1\",\"name\":\"read\",\"arguments\":{\"path\":\"README.md\"}}]}}\n"
        );
        std::fs::write(&path, fixture).unwrap();

        let data = extract(&path);
        assert_eq!(data.title.as_deref(), Some("Pi work"));
        assert_eq!(data.last_model, "gpt-test");
        assert_eq!(data.tokens.total, 180);
        assert!((data.costs.total - 0.33).abs() < 1e-9);
        assert_eq!(data.metrics.tool_count, 1);
        assert_eq!(data.metrics.tools.get("read"), Some(&1));
        // The same message also buckets its billed tokens: 100 input + 20
        // output + 50 cache read + 10 cache write.
        assert_eq!(
            data.tokens_by_day
                .values()
                .flat_map(|m| m.values())
                .sum::<u64>(),
            180
        );

        std::fs::remove_file(path).unwrap();
    }

    /// Pi prices what it has rates for and writes zeros for the rest, so a turn
    /// with tokens and no cost is a provider Pi doesn't know — not a free one.
    #[test]
    fn a_turn_pi_could_not_price_is_costed_from_litellm() {
        let _rates = crate::pricing::install_test_table(&[(
            "zai/glm-5.1",
            serde_json::json!({
                "input_cost_per_token": 1.4e-6,
                "output_cost_per_token": 4.4e-6,
                "cache_read_input_token_cost": 2.6e-7,
                "cache_creation_input_token_cost": 0.0,
            }),
        )]);
        let path = std::env::temp_dir().join(format!(
            "cctop-pi-fallback-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"version\":3,\"id\":\"pi-2\",\"timestamp\":\"2026-08-05T00:00:00Z\",\"cwd\":\"/work\"}\n",
                "{\"type\":\"message\",\"timestamp\":\"2026-08-05T00:01:00Z\",\"message\":{\"role\":\"assistant\",\"model\":\"myproxy/zai/glm-5.1\",\"timestamp\":1785888060000,\"usage\":{\"input\":1000000,\"output\":1000000,\"cacheRead\":1000000,\"cacheWrite\":1000000,\"totalTokens\":4000000,\"cost\":{\"total\":0}}}}\n"
            ),
        )
        .unwrap();

        let data = extract(&path);
        // 1.40 input + 4.40 output + 0.26 cache read + 0.00 cache write per Mtok.
        assert!(
            (data.costs.total - 6.06).abs() < 1e-9,
            "priced at {}",
            data.costs.total
        );
        // The breakdown has to add up to the total, not just the total be right.
        assert!((data.costs.input - 1.4).abs() < 1e-9);
        assert!((data.costs.output - 4.4).abs() < 1e-9);
        assert!((data.costs.cache_read - 0.26).abs() < 1e-9);
        assert_eq!(data.costs.cache_write_5m, 0.0);

        std::fs::remove_file(path).unwrap();
    }

    fn scratch(tag: &str, body: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "cctop-pi-{tag}-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, body).unwrap();
        path
    }

    /// Hand-built from the entry kinds a Pi session writes — a header, a
    /// thinking level, a compaction, turns, tool results, and an entry kind
    /// nothing here reads — with made-up content.
    const SESSION: &str = concat!(
        r#"{"type":"session","version":3,"id":"pi-3","timestamp":"2026-08-05T00:00:00Z","cwd":"/work"}"#,
        "\n",
        r#"{"type":"thinking_level_change","id":"e1","parentId":null,"timestamp":"2026-08-05T00:00:01Z","thinkingLevel":"high"}"#,
        "\n",
        r#"{"type":"message","id":"e2","timestamp":"2026-08-05T00:00:02Z","message":{"role":"user","content":"fix it","timestamp":1785888002000}}"#,
        "\n",
        r#"{"type":"message","id":"e3","timestamp":"2026-08-05T00:00:03Z","message":{"role":"assistant","model":"m-1","provider":"p","timestamp":1785888003000,"stopReason":"toolUse","usage":{"input":100,"output":10,"cacheRead":900,"cacheWrite":0,"totalTokens":1010,"cost":{"total":0.01}},"content":[{"type":"thinking","thinking":"..."},{"type":"toolCall","id":"c1","name":"edit","arguments":{"path":"a.rs","oldText":"one\ntwo","newText":"one\n2\nthree"}},{"type":"toolCall","id":"c2","name":"bash","arguments":{"command":"false"}}]}}"#,
        "\n",
        r#"{"type":"message","id":"e4","timestamp":"2026-08-05T00:00:04Z","message":{"role":"toolResult","toolCallId":"c1","toolName":"edit","content":[{"type":"text","text":"ok"}],"isError":false,"timestamp":1785888003400}}"#,
        "\n",
        r#"{"type":"message","id":"e5","timestamp":"2026-08-05T00:00:05Z","message":{"role":"toolResult","toolCallId":"c2","toolName":"bash","content":[{"type":"text","text":"exit 1"}],"isError":true,"timestamp":1785888005000}}"#,
        "\n",
        r#"{"type":"custom","id":"e6","customType":"ext","data":{"k":[1,2]}}"#,
        "\n",
        r#"{"type":"compaction","id":"e7","timestamp":"2026-08-05T00:00:06Z","summary":"s","firstKeptEntryId":"e3","tokensBefore":1000}"#,
        "\n",
        r#"{"type":"message","id":"e8","timestamp":"2026-08-05T00:00:07Z","message":{"role":"assistant","model":"m-1","provider":"p","timestamp":1785888007000,"stopReason":"toolUse","usage":{"input":50,"output":5,"cacheRead":150,"cacheWrite":20,"totalTokens":225,"cost":{"total":0.02}},"content":[{"type":"toolCall","id":"c3","name":"write","arguments":{"path":"b.rs","content":"x\ny\n"}}]}}"#,
        "\n",
    );

    #[test]
    fn outcomes_durations_edits_effort_and_compactions_are_read() {
        let path = scratch("full", SESSION);
        let data = extract(&path);
        std::fs::remove_file(&path).unwrap();

        assert_eq!(data.metrics.tool_count, 3);
        assert_eq!(data.metrics.tool_errors, 1, "isError on the bash result");
        let bash = &data.metrics.tool_details["bash"][0];
        assert!(bash.failed);
        assert_eq!(bash.dur_ms, Some(2000), "result stamp minus the turn's");
        let edit = &data.metrics.tool_details["edit"][0];
        assert!(!edit.failed);
        assert_eq!(edit.dur_ms, Some(400));
        let delta = edit.delta.as_ref().unwrap();
        assert_eq!((delta.added, delta.removed), (3, 2));
        // 3 + 2 from the edit, and the write's two lines added.
        assert_eq!(data.metrics.lines_added, 5);
        assert_eq!(data.metrics.lines_removed, 2);
        assert_eq!(data.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(data.compactions, 1);
        assert_eq!(data.tokens.total, 1235);
        assert!((data.costs.total - 0.03).abs() < 1e-9);
        assert_eq!(data.last_model, "m-1");
    }

    /// A tool result over the line cap — a read of a big file — still says
    /// whether the call failed. The `Value` reader dropped the line whole.
    #[test]
    fn an_oversized_result_still_reports_its_outcome() {
        let big = "x".repeat(crate::config::MAX_JSONL_LINE_BYTES + 10);
        let body = format!(
            concat!(
                r#"{{"type":"session","id":"pi-4","timestamp":"2026-08-05T00:00:00Z","cwd":"/w"}}"#,
                "\n",
                r#"{{"type":"message","message":{{"role":"assistant","model":"m","timestamp":1785888003000,"content":[{{"type":"toolCall","id":"c1","name":"read","arguments":{{"path":"big"}}}}]}}}}"#,
                "\n",
                r#"{{"type":"message","message":{{"role":"toolResult","toolCallId":"c1","content":[{{"type":"text","text":"{}"}}],"isError":true,"timestamp":1785888004000}}}}"#,
                "\n",
            ),
            big
        );
        let path = scratch("big", &body);
        let data = extract(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(data.metrics.tool_errors, 1);
        assert!(data.metrics.tool_details["read"][0].failed);
    }

    #[test]
    fn edits_given_as_a_list_count_every_pair() {
        let delta = edit_delta(
            "edit",
            &serde_json::json!({"path": "a", "edits": [
                {"oldText": "a", "newText": "b\nc"},
                {"oldText": "d\ne", "newText": "f"},
            ]}),
        )
        .unwrap();
        assert_eq!((delta.added, delta.removed), (3, 3));
        assert!(edit_delta("read", &serde_json::json!({"path": "a"})).is_none());
    }

    /// The window is the newest turn's input and cache, against LiteLLM's
    /// ceiling for its model; a compaction after it marks it as stale.
    #[test]
    fn context_is_the_newest_turns_window() {
        let _rates = crate::pricing::install_test_table(&[(
            "m-1",
            serde_json::json!({
                "input_cost_per_token": 1e-6,
                "output_cost_per_token": 1e-6,
                "max_input_tokens": 1000,
            }),
        )]);
        let path = scratch("ctx", SESSION);
        let mut session = Session::new(Provider::Pi, "pi-3".into());
        session.data_file = Some(path.clone());
        let ctx = extract_context(&session).unwrap();
        assert_eq!((ctx.used, ctx.max), (220, 1000));
        assert!(!ctx.compacted, "the compaction is older than the turn");

        std::fs::write(
            &path,
            format!("{SESSION}{}\n", r#"{"type":"compaction","summary":"s"}"#),
        )
        .unwrap();
        assert!(extract_context(&session).unwrap().compacted);

        session.data_file = Some(scratch("ctx-none", "{\"type\":\"session\"}\n"));
        assert!(extract_context(&session).is_none());
        std::fs::remove_file(session.data_file.unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
