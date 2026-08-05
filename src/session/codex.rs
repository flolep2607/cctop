//! Codex session discovery and rollout extraction.

use super::extract::{self, for_each_jsonl, read_first_lines};
use super::{
    CodexRates, ContextUsage, Costs, Metrics, ModelBreakdown, Session, SessionData, Tokens,
};
use crate::config::{self, CODEX_DEFAULT_CTX};
use crate::pricing::{self, Provider};
use crate::util;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone, Default)]
struct StaticParts {
    session_id: String,
    started_at: String,
    model: String,
    cwd: String,
}

static STATIC_CACHE: LazyLock<Mutex<HashMap<PathBuf, StaticParts>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn collect_static(path: &Path) -> StaticParts {
    if let Ok(cache) = STATIC_CACHE.lock()
        && let Some(hit) = cache.get(path)
    {
        return hit.clone();
    }

    let mut parts = StaticParts::default();
    if let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().to_string())
        && let Some(uuid) = config::trailing_uuid(&stem)
    {
        parts.session_id = uuid.to_string();
    }

    for item in read_first_lines(path, 50) {
        let payload = item.get("payload");
        match item.get("type").and_then(Value::as_str) {
            Some("session_meta") => {
                if let Some(p) = payload {
                    if let Some(id) = p.get("id").and_then(Value::as_str) {
                        parts.session_id = id.to_string();
                    }
                    if let Some(ts) = p
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("timestamp").and_then(Value::as_str))
                    {
                        parts.started_at = ts.to_string();
                    }
                    if let Some(cwd) = p.get("cwd").and_then(Value::as_str) {
                        parts.cwd = cwd.to_string();
                    }
                }
            }
            Some("turn_context") => {
                if let Some(m) = payload.and_then(|p| p.get("model")).and_then(Value::as_str) {
                    parts.model = m.to_string();
                }
            }
            _ => {}
        }
        if !parts.session_id.is_empty()
            && !parts.started_at.is_empty()
            && !parts.model.is_empty()
            && !parts.cwd.is_empty()
        {
            break;
        }
    }

    if !parts.session_id.is_empty()
        && let Ok(mut cache) = STATIC_CACHE.lock()
    {
        cache.insert(path.to_path_buf(), parts.clone());
    }
    parts
}

/// All Codex rollout sessions, newest file first.
pub fn list_sessions() -> Vec<Session> {
    if !config::dir_exists(&config::CODEX_SESSIONS_ROOT) {
        return Vec::new();
    }
    let mut files = config::rglob(&config::CODEX_SESSIONS_ROOT, ".jsonl");
    files.sort();
    files.reverse();

    files
        .into_iter()
        .map(|path| {
            let statics = collect_static(&path);
            let mtime = config::file_mtime_ms(&path);
            let last_active = chrono::DateTime::from_timestamp_millis(mtime as i64)
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| statics.started_at.clone());

            let mut s = Session::new(Provider::Codex, statics.session_id);
            s.started_at = statics.started_at;
            s.last_active = last_active;
            s.model = statics.model;
            s.label_source = statics.cwd;
            s.data_file = Some(path);
            s
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Extraction
// ---------------------------------------------------------------------------

/// Raw token counts per time bucket, priced once the model is known.
#[derive(Default, Clone, Copy)]
struct RawBucket {
    input: u64,
    cached_input: u64,
    output: u64,
}

/// Parse a Codex rollout into cost, token, and activity data.
pub fn extract(path: &Path) -> SessionData {
    let mut model = String::new();
    let mut totals = Tokens::default();
    let mut saw_usage = false;
    let mut by_day: HashMap<String, RawBucket> = HashMap::new();
    let mut by_hour: HashMap<String, RawBucket> = HashMap::new();
    let mut metrics = Metrics::default();
    let mut seen_call_ids: HashSet<String> = HashSet::new();
    let mut seen_queries: HashSet<String> = HashSet::new();
    // call_id -> when its output arrived, so calls can be timed.
    let mut result_ts: HashMap<String, String> = HashMap::new();

    let _ = for_each_jsonl(path, |item| {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        let payload = item.get("payload");
        let ts = item.get("timestamp").and_then(Value::as_str).unwrap_or("");

        if item_type == "turn_context"
            && let Some(m) = payload.and_then(|p| p.get("model")).and_then(Value::as_str)
        {
            model = m.to_string();
        }

        if item_type == "event_msg"
            && payload.and_then(|p| p.get("type")).and_then(Value::as_str) == Some("token_count")
            && let Some(last) = payload
                .and_then(|p| p.get("info"))
                .and_then(|i| i.get("last_token_usage"))
            && last.is_object()
        {
            saw_usage = true;
            let g = |k: &str| last.get(k).and_then(Value::as_u64).unwrap_or(0);
            let (inp, cached, out) = (
                g("input_tokens"),
                g("cached_input_tokens"),
                g("output_tokens"),
            );
            totals.input_total += inp;
            totals.cached_input += cached;
            totals.output += out;
            totals.reasoning_output += g("reasoning_output_tokens");
            totals.total += g("total_tokens");

            if let Some(dt) = util::parse_ts(ts) {
                for (map, key) in [
                    (&mut by_day, util::local_date_key(&dt)),
                    (&mut by_hour, util::local_hour_key(&dt)),
                ] {
                    let b = map.entry(key).or_default();
                    b.input += inp;
                    b.cached_input += cached;
                    b.output += out;
                }
            }
        }

        // Function calls appear either at the top level or wrapped in a
        // `response_item`; in both cases the details live under `payload`.
        let effective_type = if item_type == "response_item" {
            payload
                .and_then(|p| p.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("")
        } else {
            item_type
        };
        let Some(p) = payload else { return };

        match effective_type {
            "function_call" | "custom_tool_call" => {
                if let Some(call_id) = p.get("call_id").and_then(Value::as_str)
                    && !seen_call_ids.insert(call_id.to_string())
                {
                    return;
                }
                let name = p.get("name").and_then(Value::as_str).unwrap_or("unknown");
                if effective_type == "custom_tool_call" {
                    metrics.mcp_tool_count += 1;
                    if !metrics.mcp_tools.iter().any(|t| t == name) {
                        metrics.mcp_tools.push(name.to_string());
                    }
                }
                *metrics.tools.entry(name.to_string()).or_insert(0) += 1;
                metrics.tool_count += 1;

                // `arguments` is usually a JSON-encoded string, but some tools
                // (notably `apply_patch`) pass a raw payload instead.
                // `function_call` carries `arguments`; `custom_tool_call` (which
                // is how `apply_patch` arrives) carries `input` instead.
                let raw_field = p.get("arguments").or_else(|| p.get("input"));
                let raw_args = raw_field.and_then(Value::as_str);
                let args: Value = match raw_field {
                    Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
                    Some(other) => other.clone(),
                    None => Value::Null,
                };

                let mut delta = None;
                let (short, full) = if name == "apply_patch"
                    && let Some(raw) = raw_args
                    && args.is_null()
                {
                    let (summary, d) = extract::parse_apply_patch(raw);
                    delta = Some(d);
                    (summary, Some(raw.to_string()))
                } else if args.is_null()
                    && let Some(raw) = raw_args.filter(|r| !r.is_empty())
                {
                    // Non-JSON arguments are still worth showing verbatim.
                    (extract::flatten_public(raw, 300), Some(raw.to_string()))
                } else {
                    extract::tool_detail(name, &args)
                };

                extract::push_tool_detail(
                    &mut metrics.tool_details,
                    name,
                    short,
                    full,
                    ts.to_string(),
                    p.get("call_id").and_then(Value::as_str).map(str::to_string),
                    None,
                );
                if let Some(d) = delta
                    && let Some(list) = metrics.tool_details.get_mut(name)
                    && let Some(last) = list.last_mut()
                {
                    metrics.lines_added += d.added as u64;
                    metrics.lines_removed += d.removed as u64;
                    last.delta = Some(d);
                }
            }
            "function_call_output" | "custom_tool_call_output" => {
                if let Some(call_id) = p.get("call_id").and_then(Value::as_str)
                    && !ts.is_empty()
                {
                    result_ts.insert(call_id.to_string(), ts.to_string());
                }
            }
            "web_search_call" => {
                if let Some(q) = p
                    .get("action")
                    .and_then(|a| a.get("query"))
                    .and_then(Value::as_str)
                    && !q.is_empty()
                    && seen_queries.len() < 50
                    && seen_queries.insert(q.to_string())
                {
                    metrics.web_searches.push(q.to_string());
                    metrics.web_search_count += 1;
                }
            }
            _ => {}
        }
    });

    // Time each call from its own entry to its output's.
    for details in metrics.tool_details.values_mut() {
        for d in details.iter_mut() {
            let Some(id) = d.id.as_deref() else { continue };
            if let Some(end) = result_ts.get(id)
                && let (Some(a), Some(b)) = (util::parse_ts(&d.ts), util::parse_ts(end))
            {
                d.dur_ms = Some((b.timestamp_millis() - a.timestamp_millis()).max(0));
            }
        }
    }

    if !saw_usage {
        return SessionData {
            last_model: model.clone(),
            models: if model.is_empty() {
                vec![]
            } else {
                vec![model]
            },
            metrics,
            ..Default::default()
        };
    }

    let p = pricing::resolve_codex(&model);
    // Codex reports total input including the cached portion; only the
    // uncached remainder is billed at the full input rate.
    totals.input = totals.input_total.saturating_sub(totals.cached_input);

    let costs = Costs {
        input: util::token_cost(totals.input, p.input),
        cached_input: util::token_cost(totals.cached_input, p.cached_input),
        output: util::token_cost(totals.output, p.output),
        ..Default::default()
    };
    let total = costs.input + costs.cached_input + costs.output;

    let finalize = |raw: HashMap<String, RawBucket>| -> HashMap<String, HashMap<String, f64>> {
        raw.into_iter()
            .map(|(key, b)| {
                let uncached = b.input.saturating_sub(b.cached_input);
                let cost = util::token_cost(uncached, p.input)
                    + util::token_cost(b.cached_input, p.cached_input)
                    + util::token_cost(b.output, p.output);
                (key, HashMap::from([(model.clone(), cost)]))
            })
            .collect()
    };

    SessionData {
        last_model: model.clone(),
        models: vec![model.clone()],
        model_breakdown: vec![ModelBreakdown {
            model: model.clone(),
            tokens: totals.clone(),
            costs: Costs { total, ..costs },
            total,
        }],
        tokens: totals,
        costs: Costs { total, ..costs },
        costs_by_day: finalize(by_day),
        costs_by_hour: finalize(by_hour),
        metrics,
        rates: Some(CodexRates {
            input: p.input,
            cached_input: p.cached_input,
            output: p.output,
        }),
        ..Default::default()
    }
}

/// Context usage from the last `token_count` event in the rollout tail.
pub fn extract_context(session: &Session) -> Option<ContextUsage> {
    let file = session.data_file.as_ref()?;
    let text = util::read_tail(file, 65_536)?;

    for line in text.lines().rev() {
        let Ok(item) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if item.get("type").and_then(Value::as_str) != Some("event_msg") {
            continue;
        }
        let Some(p) = item.get("payload") else {
            continue;
        };
        if p.get("type").and_then(Value::as_str) != Some("token_count") {
            continue;
        }
        let Some(info) = p.get("info") else { continue };
        let last = info
            .get("last_token_usage")
            .or_else(|| info.get("total_token_usage"))?;
        let used = last
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        if used == 0 {
            continue;
        }
        return Some(ContextUsage {
            used,
            max: info
                .get("model_context_window")
                .and_then(Value::as_u64)
                .unwrap_or(CODEX_DEFAULT_CTX),
            compacting: false,
        });
    }
    None
}

/// Name of the most recently invoked tool in the rollout tail.
pub fn extract_last_tool(session: &Session) -> String {
    let Some(file) = session.data_file.as_ref() else {
        return String::new();
    };
    let Some(text) = util::read_tail(file, 32_768) else {
        return String::new();
    };
    for line in text.lines().rev() {
        let Ok(item) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        let t = item.get("type").and_then(Value::as_str).unwrap_or("");
        let Some(p) = item.get("payload") else {
            continue;
        };
        let is_call = matches!(t, "function_call" | "custom_tool_call")
            || (t == "response_item"
                && p.get("type").and_then(Value::as_str) == Some("function_call"));
        if is_call && let Some(name) = p.get("name").and_then(Value::as_str) {
            return name.to_string();
        }
    }
    String::new()
}

/// Remove a Codex rollout file.
pub fn delete(session: &Session) {
    if let Some(file) = session.data_file.as_ref() {
        let _ = std::fs::remove_file(file);
    }
}
