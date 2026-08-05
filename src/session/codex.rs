//! Codex session discovery and rollout extraction.

use super::extract::{self, for_each_jsonl, read_first_lines};
use super::{
    CodexRates, ContextUsage, Costs, Metrics, ModelBreakdown, Session, SessionData, Tokens,
};
use crate::config::{self, CODEX_DEFAULT_CTX};
use crate::pricing::{self, Provider};
use crate::util;
use rayon::prelude::*;
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

/// A Codex `exec` wrapper delegates to a concrete nested tool.  The rollout
/// records the wrapper as the call name, but showing that in the activity pane
/// obscures whether the agent read, edited, or ran a command.
fn unwrap_exec(source: &str) -> Option<(String, Value)> {
    // The source can contain a patch or shell command mentioning `tools.`.
    // Only a real invocation is the tool delegated by the wrapper.
    let start = tool_call_start(source)?;
    let name_end =
        source[start..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))? + start;
    let name = &source[start..name_end];
    let open = source[name_end..].find('(')? + name_end;
    let body = parenthesized(source, open)?;
    let body = body.trim();
    let args = serde_json::from_str(body)
        .or_else(|_| serde_json::from_str(&quote_bare_keys(body)))
        .unwrap_or_else(|_| bound_json_string(source, open, body).unwrap_or(Value::Null));
    Some((name.to_string(), args))
}

/// Byte offset immediately after `tools.` in executable JavaScript, not inside
/// a quoted patch/string argument.
///
/// A directly awaited call is the most reliable signal, so it wins. But the
/// agent also batches tools inside a wrapper — `await Promise.all([tools.a(…),
/// tools.b(…)])` — where nothing is awaited directly and requiring `await
/// tools.` left the raw JavaScript on screen. Fall back to the first call in
/// that case; only its arguments are shown, as before, since the activity pane
/// describes one tool per rollout entry.
fn tool_call_start(source: &str) -> Option<usize> {
    find_outside_strings(source, b"await tools.")
        .or_else(|| find_outside_strings(source, b"tools."))
}

/// Byte offset immediately after `needle`, skipping string literals.
fn find_outside_strings(source: &str, needle: &[u8]) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut quote = None;
    let mut escaped = false;
    for (index, &byte) in bytes.iter().enumerate() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == q {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'\"' | b'`' => quote = Some(byte),
            _ if bytes[index..].starts_with(needle) => return Some(index + needle.len()),
            _ => {}
        }
    }
    None
}

/// Quote bare object keys so a JavaScript object literal parses as JSON.
///
/// `exec` is handed JavaScript, so payloads are literals like `{cmd:"ls"}`
/// rather than JSON. Left unparsed, roughly two thirds of Codex tool calls
/// reached the activity pane with no arguments, losing the command or file
/// that makes the entry worth reading.
///
/// Only keys are rewritten. Values that are JavaScript expressions rather than
/// literals still will not parse, and fall through to `bound_json_string`.
fn quote_bare_keys(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + 16);
    let mut quote = None;
    let mut escaped = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == q {
                quote = None;
            }
            out.push(byte);
            index += 1;
            continue;
        }
        if matches!(byte, b'\'' | b'\"' | b'`') {
            quote = Some(byte);
            out.push(byte);
            index += 1;
            continue;
        }
        // A key can only start right after the object opens or a comma.
        if matches!(byte, b'{' | b',') {
            let key_start = index + 1 + count_spaces(&bytes[index + 1..]);
            let key_end = key_start
                + bytes[key_start..]
                    .iter()
                    .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                    .count();
            let colon = key_end + count_spaces(&bytes[key_end..]);
            if key_end > key_start && bytes.get(colon) == Some(&b':') {
                out.push(byte);
                out.extend_from_slice(&bytes[index + 1..key_start]);
                out.push(b'"');
                out.extend_from_slice(&bytes[key_start..key_end]);
                out.push(b'"');
                // Leave the spacing and the colon to the normal path.
                index = key_end;
                continue;
            }
        }
        out.push(byte);
        index += 1;
    }
    // Only ASCII quotes were inserted, at ASCII boundaries.
    String::from_utf8(out).unwrap_or_else(|_| source.to_string())
}

fn count_spaces(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|b| b.is_ascii_whitespace()).count()
}

/// Resolve the common `const patch = "..."; tools.apply_patch(patch)` shape.
/// `exec` receives JavaScript source rather than structured arguments, so a
/// bare identifier otherwise loses the patch text needed for file and delta
/// extraction. Intentionally only JSON double-quoted strings are resolved.
fn bound_json_string(source: &str, before: usize, identifier: &str) -> Option<Value> {
    if identifier.is_empty()
        || !identifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    let prefix = format!("const {identifier} =");
    let start = source[..before].rfind(&prefix)? + prefix.len();
    let value = source[start..].trim_start();
    if !value.starts_with('"') {
        return None;
    }

    let mut escaped = false;
    for (offset, byte) in value.as_bytes().iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            return serde_json::from_str(&value[..=offset]).ok();
        }
    }
    None
}

/// Contents of a balanced call argument list, excluding the outer parens.
/// Tool payloads are JSON, so handling quoted strings and escapes is enough
/// to avoid mistaking parentheses inside shell commands for the call boundary.
fn parenthesized(source: &str, open: usize) -> Option<&str> {
    let bytes = source.as_bytes();
    if bytes.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (offset, &byte) in bytes[open..].iter().enumerate() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == q {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'\"' | b'`' => quote = Some(byte),
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return source.get(open + 1..open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Convert the functions used inside Codex's `exec` wrapper into the common
/// tool names and argument shapes understood by the activity renderer.
fn normalise_exec_tool(name: String, mut args: Value) -> (String, Value) {
    match name.as_str() {
        "exec_command" => {
            if let Some(object) = args.as_object_mut()
                && let Some(cmd) = object.remove("cmd")
            {
                object.insert("command".to_string(), cmd);
            }
            ("Bash".to_string(), args)
        }
        "view_image" => {
            if let Some(object) = args.as_object_mut()
                && let Some(path) = object.remove("path")
            {
                object.insert("file_path".to_string(), path);
            }
            ("Read".to_string(), args)
        }
        "read_mcp_resource" => {
            if let Some(object) = args.as_object_mut()
                && let Some(uri) = object.remove("uri")
            {
                object.insert("file_path".to_string(), uri);
            }
            ("Read".to_string(), args)
        }
        _ => (name, args),
    }
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
                    // A forked rollout carries a second session_meta naming the
                    // original session it was forked from. The filename UUID (or
                    // the first session_meta) is this file's own identity; later
                    // session_meta entries must not overwrite it.
                    if parts.session_id.is_empty()
                        && let Some(id) = p.get("id").and_then(Value::as_str)
                    {
                        parts.session_id = id.to_string();
                    }
                    if parts.started_at.is_empty()
                        && let Some(ts) = p
                            .get("timestamp")
                            .and_then(Value::as_str)
                            .or_else(|| item.get("timestamp").and_then(Value::as_str))
                    {
                        parts.started_at = ts.to_string();
                    }
                    if parts.cwd.is_empty()
                        && let Some(cwd) = p.get("cwd").and_then(Value::as_str)
                    {
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
        .into_par_iter()
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
    let mut reasoning_effort: Option<String> = None;
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
        if item_type == "turn_context"
            && let Some(effort) = payload
                .and_then(|p| p.get("effort").or_else(|| p.get("reasoning_effort")))
                .and_then(Value::as_str)
        {
            reasoning_effort = Some(effort.to_string());
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
                // `arguments` is usually a JSON-encoded string, but some tools
                // (notably `apply_patch`) pass a raw payload instead.
                // `function_call` carries `arguments`; `custom_tool_call` (which
                // is how `apply_patch` arrives) carries `input` instead.
                let raw_field = p.get("arguments").or_else(|| p.get("input"));
                let raw_args = raw_field.and_then(Value::as_str);
                let mut args: Value = match raw_field {
                    Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
                    Some(other) => other.clone(),
                    None => Value::Null,
                };
                let outer_name = p.get("name").and_then(Value::as_str).unwrap_or("unknown");
                let name = if outer_name == "exec" {
                    raw_args
                        .and_then(unwrap_exec)
                        .map(|(nested_name, nested_args)| {
                            let (name, nested_args) = normalise_exec_tool(nested_name, nested_args);
                            args = nested_args;
                            name
                        })
                        .unwrap_or_else(|| outer_name.to_string())
                } else {
                    outer_name.to_string()
                };
                if effective_type == "custom_tool_call" {
                    metrics.mcp_tool_count += 1;
                    if !metrics.mcp_tools.iter().any(|t| t == &name) {
                        metrics.mcp_tools.push(name.clone());
                    }
                }
                *metrics.tools.entry(name.clone()).or_insert(0) += 1;
                metrics.tool_count += 1;

                let mut delta = None;
                let (short, full) = if name == "apply_patch"
                    && let Some(raw) = args.as_str().or(raw_args.filter(|_| args.is_null()))
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
                    extract::tool_detail(&name, &args)
                };

                extract::push_tool_detail(
                    &mut metrics.tool_details,
                    &name,
                    short,
                    full,
                    ts.to_string(),
                    p.get("call_id").and_then(Value::as_str).map(str::to_string),
                    None,
                );
                if let Some(d) = delta
                    && let Some(list) = metrics.tool_details.get_mut(&name)
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
            reasoning_effort,
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
        reasoning_effort,
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
            if name == "exec"
                && let Some(source) = p
                    .get("arguments")
                    .or_else(|| p.get("input"))
                    .and_then(Value::as_str)
                && let Some((nested_name, nested_args)) = unwrap_exec(source)
            {
                return normalise_exec_tool(nested_name, nested_args).0;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unwraps_exec_command_into_bash_with_its_command() {
        let source = r#"const result = await tools.exec_command({"cmd":"rg -n \"exec\" src","workdir":"/repo"});
text(result.output);"#;
        let (name, args) = unwrap_exec(source).expect("nested tool call");
        let (name, args) = normalise_exec_tool(name, args);
        assert_eq!(name, "Bash");
        assert_eq!(
            args,
            json!({"command": "rg -n \"exec\" src", "workdir": "/repo"})
        );
    }

    /// Regression: payloads are JavaScript object literals, so bare keys are
    /// the norm. Requiring strict JSON dropped the arguments — and with them
    /// the command shown in the activity pane.
    #[test]
    fn unwraps_bare_object_keys_from_javascript_literals() {
        let source = r#"const r = await tools.write_stdin({session_id:50042,chars:"",yield_time_ms:1000});
text(r.output);"#;
        let (name, args) = unwrap_exec(source).expect("nested tool call");
        assert_eq!(name, "write_stdin");
        assert_eq!(
            args,
            json!({"session_id": 50042, "chars": "", "yield_time_ms": 1000})
        );
    }

    /// Regression: batched calls are wrapped, so nothing is awaited directly
    /// and the raw JavaScript was rendered instead of the delegated tool.
    #[test]
    fn unwraps_tools_batched_inside_a_wrapper() {
        let source = r#"const results = await Promise.all([
  tools.exec_command({cmd:"sed -n '1,20p' Cargo.toml","workdir":"/repo"}),
  tools.exec_command({cmd:"ls -la","workdir":"/repo"}),
]);"#;
        let (name, args) = unwrap_exec(source).expect("nested tool call");
        let (name, args) = normalise_exec_tool(name, args);
        assert_eq!(name, "Bash");
        assert_eq!(
            args,
            json!({"command": "sed -n '1,20p' Cargo.toml", "workdir": "/repo"})
        );
    }

    /// Mixed and already-quoted keys must survive untouched, and a colon inside
    /// a string value must not be mistaken for a key separator.
    #[test]
    fn key_quoting_leaves_strings_and_arrays_alone() {
        assert_eq!(quote_bare_keys(r#"{"a":1}"#), r#"{"a":1}"#);
        assert_eq!(quote_bare_keys(r#"{a:1,"b":2}"#), r#"{"a":1,"b":2}"#);
        // Array elements are not keys.
        assert_eq!(quote_bare_keys("[78591,8570]"), "[78591,8570]");
        // A `key:` shape inside a string value is data, not syntax.
        assert_eq!(
            quote_bare_keys(r#"{cmd:"git log --format=x:y"}"#),
            r#"{"cmd":"git log --format=x:y"}"#
        );
        // Nested objects in arrays still get their keys quoted.
        assert_eq!(
            quote_bare_keys(r#"{plan:[{step:"a",status:"done"}]}"#),
            r#"{"plan":[{"step":"a","status":"done"}]}"#
        );
    }

    #[test]
    fn unwraps_raw_apply_patch_payload() {
        let source = r#"await tools.apply_patch("*** Begin Patch\n*** Update File: src/lib.rs\n*** End Patch");"#;
        let (name, args) = unwrap_exec(source).expect("nested patch call");
        assert_eq!(name, "apply_patch");
        assert_eq!(
            args.as_str(),
            Some("*** Begin Patch\n*** Update File: src/lib.rs\n*** End Patch")
        );
    }

    #[test]
    fn resolves_patch_variable_for_file_and_delta_extraction() {
        let source = r#"const patch = "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-old\n+new\n*** End Patch";
const result = await tools.apply_patch(patch);"#;
        let (name, args) = unwrap_exec(source).expect("nested patch call");
        assert_eq!(name, "apply_patch");
        let (file, delta) = extract::parse_apply_patch(args.as_str().expect("patch text"));
        assert_eq!(file, "src/lib.rs");
        assert_eq!((delta.added, delta.removed), (1, 1));
    }

    #[test]
    fn ignores_tools_mentions_inside_patch_text() {
        let source = r#"const patch = "*** Begin Patch\n*** Update File: src/lib.rs\n+metrics.tools.entry(\"Bash\")\n+source.find(\"await tools.\")\n*** End Patch";
const result = await tools.apply_patch(patch);"#;
        let (name, args) = unwrap_exec(source).expect("nested patch call");
        assert_eq!(name, "apply_patch");
        let (file, delta) = extract::parse_apply_patch(args.as_str().expect("patch text"));
        assert_eq!(file, "src/lib.rs");
        assert_eq!((delta.added, delta.removed), (2, 0));
    }

    #[test]
    fn exec_image_view_is_presented_as_a_read() {
        let (name, args) = normalise_exec_tool(
            "view_image".to_string(),
            json!({"path": "/tmp/chart.png", "detail": "high"}),
        );
        assert_eq!(name, "Read");
        assert_eq!(args["file_path"], "/tmp/chart.png");
    }

    #[test]
    fn extraction_counts_nested_exec_tool_instead_of_exec() {
        let path = std::env::temp_dir().join(format!(
            "cctop-codex-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        let item = json!({
            "type": "function_call",
            "timestamp": "2026-08-06T00:00:00Z",
            "payload": {
                "call_id": "call-1",
                "name": "exec",
                "arguments": "const result = await tools.exec_command({\"cmd\":\"rg --files\"});\ntext(result.output);"
            }
        });
        std::fs::write(&path, format!("{item}\n")).expect("write rollout");

        let data = extract(&path);
        let mut session = Session::new(Provider::Codex, "test".to_string());
        session.data_file = Some(path.clone());

        assert_eq!(data.metrics.tool_count, 1);
        assert_eq!(data.metrics.tools.get("Bash"), Some(&1));
        assert!(!data.metrics.tools.contains_key("exec"));
        assert_eq!(data.metrics.tool_details["Bash"][0].d, "rg --files");
        assert_eq!(extract_last_tool(&session), "Bash");
        let _ = std::fs::remove_file(&path);
    }
}
