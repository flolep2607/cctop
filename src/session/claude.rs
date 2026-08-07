//! Claude session discovery and transcript extraction.

use super::extract::{self, for_each_jsonl, read_first_lines, read_last_lines};
use super::{
    ContextUsage, Costs, MacMeta, Metrics, ModelBreakdown, Session, SessionData, Subagent,
    SubagentStatus, Surface, Tokens, transcript_files,
};
use crate::config::{self, CLAUDE_1M_CTX, CLAUDE_DEFAULT_CTX};
use crate::pricing::{self, Provider};
use crate::util;
use rayon::prelude::*;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

/// `<command-name>/foo</command-name>` markers left by skill/slash invocations.
static CMD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<command-name>/?([^<]+)</command-name>").expect("static regex"));

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// Fields that are written in the first few lines and never change.
#[derive(Debug, Clone, Default)]
struct StaticParts {
    model: String,
    cwd: String,
    started_at: String,
    ai_title: Option<String>,
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

    for item in read_first_lines(transcript, 50) {
        if let Some(ts) = item.get("timestamp").and_then(Value::as_str)
            && let Some(parsed) = util::parse_ts(ts)
            && earliest.is_none_or(|e| parsed < e)
        {
            earliest = Some(parsed);
        }
        if parts.cwd.is_empty()
            && let Some(cwd) = item.get("cwd").and_then(Value::as_str)
        {
            parts.cwd = cwd.to_string();
        }
        if parts.model.is_empty()
            && item.get("type").and_then(Value::as_str) == Some("assistant")
            && let Some(m) = item
                .get("message")
                .and_then(|m| m.get("model"))
                .and_then(Value::as_str)
            && m != "<synthetic>"
        {
            parts.model = m.to_string();
        }
        if item.get("type").and_then(Value::as_str) == Some("ai-title")
            && let Some(t) = item.get("aiTitle").and_then(Value::as_str)
        {
            parts.ai_title = Some(t.to_string());
        }
    }

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
    read_last_lines(transcript, 200)
        .into_iter()
        .find_map(|item| {
            (item.get("type").and_then(Value::as_str) == Some("custom-title"))
                .then(|| {
                    item.get("customTitle")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .flatten()
        })
}

/// Newest mtime across the main transcript and any subagent transcripts.
fn latest_activity(transcript: &Path) -> String {
    let newest = transcript_files(transcript)
        .iter()
        .map(|p| config::file_mtime_ms(p))
        .max()
        .unwrap_or(0);
    util::ms_to_rfc3339(newest as i64)
}

fn summarize(transcript: &Path) -> Option<Session> {
    let session_id = transcript.file_stem()?.to_string_lossy().to_string();
    let statics = collect_static(transcript);
    // An abandoned session that never reached the model has nothing to show.
    if statics.model.is_empty() {
        return None;
    }
    let custom_title = scan_custom_title(transcript);
    let last_active = latest_activity(transcript);

    let mut s = Session::new(Provider::Claude, session_id);
    s.started_at = statics.started_at.clone();
    s.last_active = if last_active.is_empty() {
        statics.started_at
    } else {
        last_active
    };
    s.model = statics.model;
    s.label_source = statics.cwd;
    s.data_file = Some(transcript.to_path_buf());
    s.title = custom_title.or(statics.ai_title);
    Some(s)
}

/// All Claude sessions: CLI transcripts plus any Claude for Mac sessions.
pub fn list_sessions() -> Vec<Session> {
    let mut transcripts = Vec::new();

    if config::dir_exists(&config::CLAUDE_PROJECTS_ROOT) {
        for project in config::list_dir(&config::CLAUDE_PROJECTS_ROOT) {
            let project_dir = config::CLAUDE_PROJECTS_ROOT.join(&project);
            if !project_dir.is_dir() {
                continue;
            }
            for entry in config::list_dir(&project_dir) {
                let Some(stem) = entry.strip_suffix(".jsonl") else {
                    continue;
                };
                if !config::is_full_uuid(stem) {
                    continue;
                }
                transcripts.push(project_dir.join(entry));
            }
        }
    }

    let mut sessions: Vec<_> = transcripts
        .par_iter()
        .filter_map(|path| summarize(path))
        .collect();
    sessions.extend(list_mac_sessions());
    sessions
}

/// Locate the Claude Code transcript backing a desktop session.
fn find_desktop_jsonl(session_dir: &Path, cli_session_id: &str) -> Option<PathBuf> {
    let projects_dir = session_dir.join(".claude").join("projects");
    if !projects_dir.is_dir() {
        return None;
    }
    for project in config::list_dir(&projects_dir) {
        let project_dir = projects_dir.join(&project);
        if !project_dir.is_dir() {
            continue;
        }
        let exact = project_dir.join(format!("{cli_session_id}.jsonl"));
        if exact.is_file() {
            return Some(exact);
        }
        // The CLI session id can diverge from the filename in edge cases.
        for entry in config::list_dir(&project_dir) {
            if entry.ends_with(".jsonl") && entry.starts_with(cli_session_id) {
                return Some(project_dir.join(entry));
            }
        }
    }
    None
}

/// Sessions from Claude for Mac (both Code and Cowork surfaces).
pub fn list_mac_sessions() -> Vec<Session> {
    let mut sessions = Vec::new();
    let roots: [(&Option<PathBuf>, Option<Surface>); 2] = [
        (&config::CLAUDE_MAC_COWORK_ROOT, None),
        (&config::CLAUDE_MAC_CODE_ROOT, Some(Surface::DesktopCode)),
    ];
    for (root, forced) in roots {
        let Some(root) = root.as_ref() else { continue };
        if config::dir_exists(root) {
            scan_mac_root(root, forced, &mut sessions);
        }
    }
    sessions
}

fn scan_mac_root(root: &Path, forced_surface: Option<Surface>, out: &mut Vec<Session>) {
    for account in config::list_dir(root) {
        if account == "skills-plugin" {
            continue;
        }
        let account_dir = root.join(&account);
        if !account_dir.is_dir() {
            continue;
        }
        for device in config::list_dir(&account_dir) {
            let device_dir = account_dir.join(&device);
            if !device_dir.is_dir() {
                continue;
            }
            for entry in config::list_dir(&device_dir) {
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

                let mut s = Session::new(Provider::Claude, cli_session_id.to_string());
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

// ---------------------------------------------------------------------------
// Transcript extraction
// ---------------------------------------------------------------------------

/// One assistant message's token counts, held until the request is finalised.
#[derive(Debug, Clone, Default)]
struct Snapshot {
    input: u64,
    cache_read: u64,
    output: u64,
    cw5m: u64,
    cw1h: u64,
    model: String,
    ts: String,
}

/// Per-subagent-file accumulators.
#[derive(Debug, Default)]
struct SubStats {
    cost: f64,
    tool_count: u64,
    first_ts: Option<String>,
    last_ts: Option<String>,
    last_model: String,
    latest_used: u64,
    latest_used_ts: String,
}

/// A parent-side `Agent` tool_use block.
#[derive(Debug, Clone, Default)]
struct AgentUse {
    description: String,
    subagent_type: String,
    model: String,
    ts: String,
}

#[derive(Default)]
struct Extractor {
    token_totals: Tokens,
    cost_totals: Costs,
    tokens_by_model: HashMap<String, Tokens>,
    costs_by_model: HashMap<String, Costs>,
    costs_by_day: HashMap<String, HashMap<String, f64>>,
    costs_by_hour: HashMap<String, HashMap<String, f64>>,
    models: HashMap<String, u64>,
    last_model: String,
    last_main_model: String,
    custom_title: Option<String>,
    ai_title: Option<String>,
    metrics: Metrics,
    seen_tool_ids: HashSet<String>,
    seen_urls: HashSet<String>,
    seen_queries: HashSet<String>,
    sub_stats: HashMap<PathBuf, SubStats>,
    agent_uses: Vec<(String, AgentUse)>,
    agent_results: HashSet<String>,
    /// tool_use id -> the request key of the turn that issued it, and how many
    /// calls that turn issued. Tokens are billed per request, not per call.
    tool_turn: HashMap<String, (String, u8)>,
    /// tool_use id -> timestamp its result arrived.
    tool_result_ts: HashMap<String, String>,
    /// tool_use ids whose result was flagged `is_error`.
    tool_failed: HashSet<String>,
    /// tool_use id -> line changes reported by the result's patch.
    tool_delta: HashMap<String, super::Delta>,
    /// Request key -> final (input, output) tokens, filled when the file flushes.
    req_usage: HashMap<String, (u64, u64)>,
    error: Option<String>,
}

impl Extractor {
    /// Fold one message's tokens into every accumulator that tracks them.
    #[allow(clippy::too_many_arguments)]
    fn accumulate(
        &mut self,
        file: &Path,
        model: &str,
        inp: u64,
        cache_r: u64,
        out: u64,
        cw5m: u64,
        cw1h: u64,
        ts: &str,
        is_main: bool,
    ) {
        self.last_model = model.to_string();
        if is_main {
            self.last_main_model = model.to_string();
        }
        *self.models.entry(model.to_string()).or_insert(0) += 1;

        let p = pricing::resolve_claude(model);
        let c = Costs {
            input: util::token_cost(inp, p.input),
            cache_read: util::token_cost(cache_r, p.cache_read),
            output: util::token_cost(out, p.output),
            cache_write_5m: util::token_cost(cw5m, p.cache_write_5m),
            cache_write_1h: util::token_cost(cw1h, p.cache_write_1h),
            cached_input: 0.0,
            total: 0.0,
        };
        let call_cost = c.input + c.cache_read + c.output + c.cache_write_5m + c.cache_write_1h;

        self.token_totals.input += inp;
        self.token_totals.cache_read += cache_r;
        self.token_totals.output += out;
        self.token_totals.cache_write_5m += cw5m;
        self.token_totals.cache_write_1h += cw1h;

        self.cost_totals.input += c.input;
        self.cost_totals.cache_read += c.cache_read;
        self.cost_totals.output += c.output;
        self.cost_totals.cache_write_5m += c.cache_write_5m;
        self.cost_totals.cache_write_1h += c.cache_write_1h;

        let tm = self.tokens_by_model.entry(model.to_string()).or_default();
        tm.input += inp;
        tm.cache_read += cache_r;
        tm.output += out;
        tm.cache_write_5m += cw5m;
        tm.cache_write_1h += cw1h;

        let cm = self.costs_by_model.entry(model.to_string()).or_default();
        cm.input += c.input;
        cm.cache_read += c.cache_read;
        cm.output += c.output;
        cm.cache_write_5m += c.cache_write_5m;
        cm.cache_write_1h += c.cache_write_1h;
        cm.total += call_cost;

        if let Some(dt) = util::parse_ts(ts) {
            *self
                .costs_by_day
                .entry(util::local_date_key(&dt))
                .or_default()
                .entry(model.to_string())
                .or_insert(0.0) += call_cost;
            *self
                .costs_by_hour
                .entry(util::local_hour_key(&dt))
                .or_default()
                .entry(model.to_string())
                .or_insert(0.0) += call_cost;
        }

        if !is_main && let Some(stats) = self.sub_stats.get_mut(file) {
            stats.cost += call_cost;
            stats.last_model = model.to_string();
        }
    }

    fn record_tool(
        &mut self,
        name: &str,
        input: &Value,
        ts: &str,
        file: &Path,
        is_main: bool,
        id: Option<&str>,
    ) {
        if name.starts_with("mcp__") {
            self.metrics.mcp_tool_count += 1;
            if !self.metrics.mcp_tools.iter().any(|t| t == name) {
                self.metrics.mcp_tools.push(name.to_string());
            }
        }
        *self.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
        self.metrics.tool_count += 1;

        if !is_main && let Some(stats) = self.sub_stats.get_mut(file) {
            stats.tool_count += 1;
        }

        let (short, full) = extract::tool_detail(name, input);
        // Subagent transcripts live at `<stem>/subagents/<agent-id>.jsonl`, so
        // the file stem names the agent that made the call.
        let origin = (!is_main)
            .then(|| file.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .flatten();
        extract::push_tool_detail(
            &mut self.metrics.tool_details,
            name,
            short,
            full,
            ts.to_string(),
            id.map(str::to_string),
            origin,
        );

        match name {
            "WebFetch" => {
                if let Some(url) = input.get("url").and_then(Value::as_str)
                    && self.seen_urls.len() < 50
                    && self.seen_urls.insert(url.to_string())
                {
                    self.metrics.web_fetches.push(url.to_string());
                    self.metrics.web_fetch_count += 1;
                }
            }
            "WebSearch" => {
                if let Some(q) = input.get("query").and_then(Value::as_str)
                    && self.seen_queries.len() < 50
                    && self.seen_queries.insert(q.to_string())
                {
                    self.metrics.web_searches.push(q.to_string());
                    self.metrics.web_search_count += 1;
                }
            }
            _ => {}
        }
    }

    fn visit_user(&mut self, item: &Value, is_main: bool) {
        let ts = item.get("timestamp").and_then(Value::as_str).unwrap_or("");

        // Line counts, and the diff itself, from edit tool results.
        let mut delta: Option<super::Delta> = None;
        if let Some(patch) = item
            .get("toolUseResult")
            .and_then(|r| r.get("structuredPatch"))
            .and_then(Value::as_array)
        {
            let mut d = super::Delta::default();
            for hunk in patch {
                let Some(lines) = hunk.get("lines").and_then(Value::as_array) else {
                    continue;
                };
                for line in lines.iter().filter_map(Value::as_str) {
                    if line.starts_with('+') {
                        self.metrics.lines_added += 1;
                        d.added += 1;
                    } else if line.starts_with('-') {
                        self.metrics.lines_removed += 1;
                        d.removed += 1;
                    }
                    if d.hunks.len() < crate::config::MAX_DIFF_LINES {
                        d.hunks.push(line.to_string());
                    }
                }
            }
            if d.added > 0 || d.removed > 0 {
                delta = Some(d);
            }
        }

        let content = item.get("message").and_then(|m| m.get("content"));
        let mut texts: Vec<&str> = Vec::new();
        match content {
            Some(Value::String(s)) => texts.push(s),
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    match block {
                        Value::String(s) => texts.push(s),
                        Value::Object(_) => {
                            if let Some(t) = block.get("text").and_then(Value::as_str) {
                                texts.push(t);
                            }
                            if block.get("type").and_then(Value::as_str) == Some("tool_result")
                                && let Some(id) = block.get("tool_use_id").and_then(Value::as_str)
                            {
                                if is_main {
                                    self.agent_results.insert(id.to_string());
                                }
                                if !ts.is_empty() {
                                    self.tool_result_ts.insert(id.to_string(), ts.to_string());
                                }
                                if block.get("is_error").and_then(Value::as_bool) == Some(true) {
                                    self.tool_failed.insert(id.to_string());
                                }
                                if let Some(d) = delta.take() {
                                    self.tool_delta.insert(id.to_string(), d);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }

        for text in texts {
            for cap in CMD_RE.captures_iter(text) {
                let name = cap[1].trim();
                if !name.is_empty() {
                    *self.metrics.skills.entry(name.to_string()).or_insert(0) += 1;
                    self.metrics.skill_count += 1;
                }
            }
        }
    }

    fn visit_assistant(
        &mut self,
        item: &Value,
        file: &Path,
        is_main: bool,
        last_by_key: &mut HashMap<String, Snapshot>,
    ) {
        let Some(message) = item.get("message") else {
            return;
        };
        let ts = item.get("timestamp").and_then(Value::as_str).unwrap_or("");

        // The request key identifies the turn; tokens are attributed to it and
        // resolved once the file's streaming partials have been deduped.
        let turn_key = item
            .get("requestId")
            .and_then(Value::as_str)
            .or_else(|| message.get("id").and_then(Value::as_str))
            .map(str::to_string);
        let tool_calls_in_turn = message
            .get("content")
            .and_then(Value::as_array)
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
                    .count()
                    .min(u8::MAX as usize) as u8
            })
            .unwrap_or(0);

        // Tool scanning runs independently of the token dedup below: a streaming
        // partial and its final entry share a requestId, but each tool_use block
        // carries its own id, so global id dedup is what prevents double-counting.
        if let Some(blocks) = message.get("content").and_then(Value::as_array) {
            for block in blocks {
                if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                    continue;
                }
                let Some(name) = block.get("name").and_then(Value::as_str) else {
                    continue;
                };
                if let Some(id) = block.get("id").and_then(Value::as_str) {
                    if !self.seen_tool_ids.insert(id.to_string()) {
                        continue;
                    }
                    if is_main && name == "Agent" {
                        let inp = block.get("input").cloned().unwrap_or(Value::Null);
                        self.agent_uses.push((
                            id.to_string(),
                            AgentUse {
                                description: inp
                                    .get("description")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                subagent_type: inp
                                    .get("subagent_type")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                model: inp
                                    .get("model")
                                    .and_then(Value::as_str)
                                    .unwrap_or("")
                                    .to_string(),
                                ts: ts.to_string(),
                            },
                        ));
                    }
                }
                let input = block.get("input").cloned().unwrap_or(Value::Null);
                let block_id = block.get("id").and_then(Value::as_str);
                if let Some(id) = block_id
                    && let Some(key) = &turn_key
                {
                    self.tool_turn
                        .insert(id.to_string(), (key.clone(), tool_calls_in_turn));
                }
                self.record_tool(name, &input, ts, file, is_main, block_id);
            }
        }

        let Some(usage) = message.get("usage") else {
            return;
        };
        let u = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
        let input = u("input_tokens");
        let cache_read = u("cache_read_input_tokens");
        let output = u("output_tokens");
        let total_cw = u("cache_creation_input_tokens");
        let creation = usage.get("cache_creation");
        let cw5m_raw = creation
            .and_then(|c| c.get("ephemeral_5m_input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let cw1h = creation
            .and_then(|c| c.get("ephemeral_1h_input_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        // Older transcripts report only the aggregate; attribute the remainder
        // to the 5m tier, which is the default TTL.
        let cw5m = cw5m_raw + total_cw.saturating_sub(cw5m_raw + cw1h);

        if input == 0 && cache_read == 0 && output == 0 && cw5m == 0 && cw1h == 0 {
            return;
        }

        let model = message.get("model").and_then(Value::as_str).unwrap_or("");
        if model.is_empty() || model == "<synthetic>" {
            self.error = Some(format!(
                "Encountered billable Claude usage with unknown model in {}",
                file.display()
            ));
            return;
        }

        // Track the subagent's own cumulative context from its latest message.
        if !is_main
            && let Some(stats) = self.sub_stats.get_mut(file)
            && (stats.latest_used_ts.is_empty() || ts >= stats.latest_used_ts.as_str())
        {
            stats.latest_used = input + cache_read + total_cw;
            stats.latest_used_ts = ts.to_string();
        }

        let snapshot = Snapshot {
            input,
            cache_read,
            output,
            cw5m,
            cw1h,
            model: model.to_string(),
            ts: ts.to_string(),
        };

        // Streaming writes the same requestId repeatedly with growing counts;
        // keeping only the last occurrence per key avoids inflating totals.
        match turn_key.as_deref() {
            Some(k) => {
                last_by_key.insert(k.to_string(), snapshot);
            }
            None => self.accumulate(
                file, model, input, cache_read, output, cw5m, cw1h, ts, is_main,
            ),
        }
    }

    fn visit(
        &mut self,
        item: &Value,
        file: &Path,
        is_main: bool,
        last_by_key: &mut HashMap<String, Snapshot>,
    ) {
        // Any timestamped entry counts as subagent activity.
        if !is_main
            && let Some(ts) = item.get("timestamp").and_then(Value::as_str)
            && let Some(stats) = self.sub_stats.get_mut(file)
        {
            if stats.first_ts.as_deref().is_none_or(|f| ts < f) {
                stats.first_ts = Some(ts.to_string());
            }
            if stats.last_ts.as_deref().is_none_or(|l| ts > l) {
                stats.last_ts = Some(ts.to_string());
            }
        }

        match item.get("type").and_then(Value::as_str) {
            Some("system") => {
                if item.get("subtype").and_then(Value::as_str) == Some("turn_duration")
                    && let Some(ms) = item.get("durationMs").and_then(Value::as_u64)
                {
                    self.metrics.api_duration_ms += ms;
                }
            }
            Some("custom-title") if is_main => {
                if let Some(t) = item.get("customTitle").and_then(Value::as_str) {
                    self.custom_title = Some(t.to_string());
                }
            }
            Some("ai-title") if is_main => {
                if let Some(t) = item.get("aiTitle").and_then(Value::as_str) {
                    self.ai_title = Some(t.to_string());
                }
            }
            Some("user") => self.visit_user(item, is_main),
            Some("assistant") => self.visit_assistant(item, file, is_main, last_by_key),
            _ => {}
        }
    }
}

/// Parse a Claude session's transcripts into cost, token, and activity data.
pub fn extract(transcript: &Path) -> SessionData {
    let files = transcript_files(transcript);
    let mut ext = Extractor {
        sub_stats: files[1..]
            .iter()
            .map(|f| (f.clone(), SubStats::default()))
            .collect(),
        ..Default::default()
    };

    for (idx, file) in files.iter().enumerate() {
        let is_main = idx == 0;
        let mut last_by_key: HashMap<String, Snapshot> = HashMap::new();
        let _ = for_each_jsonl(file, |item| {
            ext.visit(item, file, is_main, &mut last_by_key);
        });
        // Flush the deduped snapshots for this file. The surviving entry per
        // request carries that turn's final token counts.
        let snapshots: Vec<(String, Snapshot)> = last_by_key.into_iter().collect();
        for (key, s) in snapshots {
            ext.req_usage
                .insert(key, (s.input + s.cache_read + s.cw5m + s.cw1h, s.output));
            ext.accumulate(
                file,
                &s.model,
                s.input,
                s.cache_read,
                s.output,
                s.cw5m,
                s.cw1h,
                &s.ts,
                is_main,
            );
        }
    }

    if let Some(err) = ext.error {
        return SessionData {
            error: Some(err),
            ..Default::default()
        };
    }
    if ext.models.is_empty() {
        return SessionData {
            error: Some(format!(
                "No assistant usage records found in {}",
                transcript.display()
            )),
            ..Default::default()
        };
    }

    attach_call_details(&mut ext);
    let subagents = build_subagents(&files, &ext);

    let mut tokens = ext.token_totals;
    tokens.total = tokens.input
        + tokens.cache_read
        + tokens.output
        + tokens.cache_write_5m
        + tokens.cache_write_1h;

    let mut costs = ext.cost_totals;
    costs.total =
        costs.input + costs.cache_read + costs.output + costs.cache_write_5m + costs.cache_write_1h;

    let mut model_breakdown: Vec<ModelBreakdown> = ext
        .tokens_by_model
        .iter()
        .map(|(model, t)| {
            let c = ext.costs_by_model.get(model).cloned().unwrap_or_default();
            ModelBreakdown {
                model: model.clone(),
                tokens: t.clone(),
                total: c.total,
                costs: c,
            }
        })
        .collect();
    model_breakdown.sort_by(|a, b| a.model.cmp(&b.model));

    let mut models: Vec<String> = ext.models.into_keys().collect();
    models.sort();

    let title = ext.custom_title.clone().or_else(|| ext.ai_title.clone());

    SessionData {
        title,
        custom_title: ext.custom_title,
        ai_title: ext.ai_title,
        last_model: if ext.last_main_model.is_empty() {
            ext.last_model
        } else {
            ext.last_main_model
        },
        reasoning_effort: None,
        models,
        model_breakdown,
        tokens,
        costs,
        costs_by_day: ext.costs_by_day,
        costs_by_hour: ext.costs_by_hour,
        metrics: ext.metrics,
        subagents,
        rates: None,
        error: None,
    }
}

/// Fill in each tool invocation's duration, edit delta, and turn token counts.
///
/// These can only be resolved after the whole file is parsed: the result arrives
/// in a later entry than the call, and a turn's final token counts are only known
/// once its streaming partials have been deduped.
fn attach_call_details(ext: &mut Extractor) {
    for details in ext.metrics.tool_details.values_mut() {
        for d in details.iter_mut() {
            let Some(id) = d.id.clone() else { continue };

            if let Some(result_ts) = ext.tool_result_ts.get(&id)
                && let (Some(start), Some(end)) = (util::parse_ts(&d.ts), util::parse_ts(result_ts))
            {
                d.dur_ms = Some((end.timestamp_millis() - start.timestamp_millis()).max(0));
            }
            if let Some(delta) = ext.tool_delta.get(&id) {
                d.delta = Some(delta.clone());
            }
            d.failed = ext.tool_failed.contains(&id);
            if let Some((key, shared)) = ext.tool_turn.get(&id) {
                d.shared = *shared;
                if let Some((tin, tout)) = ext.req_usage.get(key) {
                    d.tokens_in = *tin;
                    d.tokens_out = *tout;
                }
            }
        }
    }
}

/// Assemble the subagent list from on-disk transcripts plus parent-side records.
fn build_subagents(files: &[PathBuf], ext: &Extractor) -> Vec<Subagent> {
    let now_ms = util::now_ms();
    let mut subagents: Vec<Subagent> = Vec::new();

    for file in &files[1..] {
        let stats = ext.sub_stats.get(file);
        let agent_id = file
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        // Sidecar metadata written alongside the transcript.
        let meta: Value = std::fs::read_to_string(file.with_extension("meta.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        let tool_use_id = meta
            .get("toolUseId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let parent = tool_use_id
            .as_ref()
            .and_then(|id| ext.agent_uses.iter().find(|(k, _)| k == id).map(|(_, u)| u));

        let agent_type = meta
            .get("agentType")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| parent.map(|p| p.subagent_type.clone()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "?".into());
        let description = meta
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| parent.map(|p| p.description.clone()))
            .unwrap_or_default();
        let model = stats
            .map(|s| s.last_model.clone())
            .filter(|m| !m.is_empty())
            .or_else(|| parent.map(|p| p.model.clone()))
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "?".into());

        let first_ts = stats.and_then(|s| s.first_ts.clone());
        let last_ts = stats.and_then(|s| s.last_ts.clone());
        let first_ms = first_ts
            .as_deref()
            .and_then(util::parse_ts)
            .map(|d| d.timestamp_millis());
        let last_ms = last_ts
            .as_deref()
            .and_then(util::parse_ts)
            .map(|d| d.timestamp_millis());
        let duration_ms = match (first_ms, last_ms) {
            (Some(a), Some(b)) => (b - a).max(0),
            _ => 0,
        };
        let last_active_ms = config::file_mtime_ms(file) as i64;

        // Prefer the parent's tool_result as the completion signal; fall back to
        // recency only when no tool_use_id links this transcript to the parent.
        let status = match &tool_use_id {
            Some(id) => {
                if ext.agent_results.contains(id) {
                    SubagentStatus::Done
                } else {
                    SubagentStatus::Running
                }
            }
            None => {
                let newest = last_active_ms.max(last_ms.unwrap_or(0));
                if now_ms - newest < 30_000 {
                    SubagentStatus::Running
                } else {
                    SubagentStatus::Done
                }
            }
        };

        // Subagents hold isolated contexts, so infer the window from their own
        // usage. LiteLLM reports 1M for any model that *can* do 1M with the beta
        // header, which would understate CTX% for the common 200k default.
        let context = stats.filter(|s| s.latest_used > 0).map(|s| ContextUsage {
            used: s.latest_used,
            max: if s.latest_used > CLAUDE_DEFAULT_CTX {
                CLAUDE_1M_CTX
            } else {
                CLAUDE_DEFAULT_CTX
            },
            compacting: false,
        });

        subagents.push(Subagent {
            agent_id,
            agent_type,
            description,
            model,
            started_at: first_ts,
            last_active: last_ts,
            duration_ms,
            status,
            cost: stats.map(|s| s.cost).unwrap_or(0.0),
            tool_count: stats.map(|s| s.tool_count).unwrap_or(0),
            tool_use_id,
            context,
            ghost: false,
        });
    }

    let mut claimed: HashSet<String> = subagents
        .iter()
        .filter_map(|s| s.tool_use_id.clone())
        .collect();

    // Older agents wrote no toolUseId. Pair them back to a parent tool_use by
    // description, nearest start time winning, so they don't also appear as ghosts.
    for sa in subagents.iter_mut() {
        if sa.tool_use_id.is_some() {
            continue;
        }
        let sa_ts = sa
            .started_at
            .as_deref()
            .and_then(util::parse_ts)
            .map(|d| d.timestamp_millis());
        let mut best: Option<(String, i64)> = None;
        for (id, use_) in &ext.agent_uses {
            if claimed.contains(id) || use_.description != sa.description {
                continue;
            }
            let use_ts = util::parse_ts(&use_.ts).map(|d| d.timestamp_millis());
            let delta = match (sa_ts, use_ts) {
                (Some(a), Some(b)) => (a - b).abs(),
                _ => 0,
            };
            if best.as_ref().is_none_or(|(_, d)| delta < *d) {
                best = Some((id.clone(), delta));
            }
        }
        if let Some((id, _)) = best {
            if ext.agent_results.contains(&id) {
                sa.status = SubagentStatus::Done;
            }
            if let Some((_, use_)) = ext.agent_uses.iter().find(|(k, _)| *k == id)
                && sa.agent_type == "?"
                && !use_.subagent_type.is_empty()
            {
                sa.agent_type = use_.subagent_type.clone();
            }
            sa.tool_use_id = Some(id.clone());
            claimed.insert(id);
        }
    }

    // Claude Code purges old subagent transcripts but keeps the tool_use /
    // tool_result pair in the parent. Synthesise a row from what survives.
    for (id, use_) in &ext.agent_uses {
        if claimed.contains(id) {
            continue;
        }
        subagents.push(Subagent {
            agent_id: id.clone(),
            agent_type: if use_.subagent_type.is_empty() {
                "?".into()
            } else {
                use_.subagent_type.clone()
            },
            description: use_.description.clone(),
            model: if use_.model.is_empty() {
                "?".into()
            } else {
                use_.model.clone()
            },
            started_at: (!use_.ts.is_empty()).then(|| use_.ts.clone()),
            last_active: None,
            duration_ms: 0,
            status: if ext.agent_results.contains(id) {
                SubagentStatus::Done
            } else {
                SubagentStatus::Running
            },
            cost: 0.0,
            tool_count: 0,
            tool_use_id: Some(id.clone()),
            context: None,
            ghost: true,
        });
    }

    subagents
}

// ---------------------------------------------------------------------------
// Tail scans for live sessions
// ---------------------------------------------------------------------------

/// Context-window setting pinned in Claude Code settings, most specific first.
fn settings_context(session: &Session) -> Option<u64> {
    let config_root = match (session.surface.is_desktop(), &session.mac_meta) {
        (true, Some(meta)) => meta.session_dir.join(".claude"),
        _ => config::CLAUDE_CONFIG_DIR.clone(),
    };
    let mut candidates = Vec::new();
    if !session.label_source.is_empty() {
        let cwd = Path::new(&session.label_source);
        candidates.push(cwd.join(".claude").join("settings.local.json"));
        candidates.push(cwd.join(".claude").join("settings.json"));
    }
    candidates.push(config_root.join("settings.json"));

    for path in candidates {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(model) = v.get("model").and_then(Value::as_str) {
            return Some(if model.contains("[1m]") {
                CLAUDE_1M_CTX
            } else {
                CLAUDE_DEFAULT_CTX
            });
        }
    }
    None
}

/// Context usage from the tail of a session's own transcript.
///
/// Reads the parent transcript specifically: a running subagent streams into its
/// own file with an isolated context, and letting it win by mtime would make the
/// parent's CTX% jump to an unrelated number.
pub fn extract_context(session: &Session) -> Option<ContextUsage> {
    let file = session.data_file.as_ref()?;

    let settings_ctx = settings_context(session);
    let litellm_ctx = pricing::litellm_max_input_tokens(&session.model);
    let mut saw_compact_boundary = false;

    // Widening rescans lines it has already rejected, but only ever from behind
    // the newest usage entry, so the boundary flag keeps meaning "a compaction
    // started after the last request" rather than "this session compacted once".
    let used = util::scan_tail_escalating(file, |text| {
        for line in text.lines().rev() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(item) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if item.get("type").and_then(Value::as_str) == Some("system")
                && item.get("subtype").and_then(Value::as_str) == Some("compact_boundary")
            {
                saw_compact_boundary = true;
            }
            if item.get("type").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let Some(u) = item.get("message").and_then(|m| m.get("usage")) else {
                continue;
            };
            let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
            let used =
                g("input_tokens") + g("cache_creation_input_tokens") + g("cache_read_input_tokens");
            if used == 0 {
                continue;
            }
            return Some(used);
        }
        None
    });

    if let Some(used) = used {
        // Resolution order: a pinned [1m] setting, then inference from observed
        // usage, then LiteLLM, then the 200k default.
        let max = settings_ctx
            .or(if used > CLAUDE_DEFAULT_CTX {
                Some(CLAUDE_1M_CTX)
            } else {
                None
            })
            .or(litellm_ctx)
            .unwrap_or(CLAUDE_DEFAULT_CTX);
        return Some(ContextUsage {
            used,
            max,
            compacting: saw_compact_boundary,
        });
    }

    saw_compact_boundary.then(|| ContextUsage {
        used: 0,
        max: settings_ctx.or(litellm_ctx).unwrap_or(CLAUDE_DEFAULT_CTX),
        compacting: true,
    })
}

/// Name of the most recently invoked tool, across main and subagent transcripts.
pub fn extract_last_tool(session: &Session) -> String {
    let Some(main) = session.data_file.as_ref() else {
        return String::new();
    };
    // The freshest activity may be in a subagent, so pick by mtime here.
    let target = transcript_files(main)
        .into_iter()
        .max_by_key(|p| config::file_mtime_ms(p))
        .unwrap_or_else(|| main.clone());

    let Some(text) = util::read_tail(&target, 32_768) else {
        return String::new();
    };
    for line in text.lines().rev() {
        let Ok(item) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if item.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(blocks) = item
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for block in blocks.iter().rev() {
            if block.get("type").and_then(Value::as_str) == Some("tool_use")
                && let Some(name) = block.get("name").and_then(Value::as_str)
            {
                return name.to_string();
            }
        }
    }
    String::new()
}

/// Remove a session's transcript, its subagent directory, and any desktop metadata.
pub fn delete(session: &Session) {
    let Some(file) = session.data_file.as_ref() else {
        return;
    };
    let _ = std::fs::remove_file(file);
    let _ = std::fs::remove_dir_all(file.with_extension(""));
    if let Some(meta) = &session.mac_meta {
        let _ = std::fs::remove_file(&meta.meta_path);
        let _ = std::fs::remove_dir_all(&meta.session_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed call must be distinguishable from a successful one. Claude
    /// records the outcome on the `tool_result`, which is a separate entry from
    /// the call, linked only by `tool_use_id`.
    #[test]
    fn tool_results_marked_is_error_flag_their_call() {
        let path = std::env::temp_dir().join(format!(
            "cctop-claude-failed-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"assistant","timestamp":"2026-08-05T10:00:00.000Z","requestId":"req_1","message":{"id":"m1","role":"assistant","model":"claude-opus-5","content":[{"type":"tool_use","id":"toolu_ok","name":"Bash","input":{"command":"true"}}],"usage":{"input_tokens":10,"output_tokens":2}}}"#,
                "\n",
                r#"{"type":"user","timestamp":"2026-08-05T10:00:01.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_ok","content":"ok"}]}}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2026-08-05T10:00:02.000Z","requestId":"req_2","message":{"id":"m2","role":"assistant","model":"claude-opus-5","content":[{"type":"tool_use","id":"toolu_bad","name":"Bash","input":{"command":"exit 1"}}],"usage":{"input_tokens":10,"output_tokens":2}}}"#,
                "\n",
                r#"{"type":"user","timestamp":"2026-08-05T10:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_bad","content":"Exit code 1","is_error":true}]}}"#,
                "\n",
            ),
        )
        .expect("write transcript");

        let data = extract(&path);
        let _ = std::fs::remove_file(&path);

        let calls = &data.metrics.tool_details["Bash"];
        assert_eq!(calls.len(), 2);
        let ok = calls
            .iter()
            .find(|d| d.d.contains("true"))
            .expect("ok call");
        let bad = calls
            .iter()
            .find(|d| d.d.contains("exit 1"))
            .expect("failed call");
        assert!(!ok.failed, "a successful result must not be flagged");
        assert!(bad.failed, "is_error must be flagged");
    }

    /// A single transcript entry can be hundreds of kilobytes — a big file read,
    /// a pasted log — so the newest request's usage is regularly further from EOF
    /// than any one fixed tail window reaches. When the scan misses it the row
    /// falls through to the compaction fallback and reports COMPCT with a full
    /// context window, which reads as "this session is stalled" for a session
    /// that is merely mid-answer.
    #[test]
    fn a_usage_entry_buried_behind_a_huge_line_is_still_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("buried.jsonl");
        let filler = "x".repeat(200_000);
        std::fs::write(
            &path,
            format!(
                concat!(
                    r#"{{"type":"assistant","message":{{"role":"assistant","usage":{{"input_tokens":14,"cache_creation_input_tokens":400,"cache_read_input_tokens":647000}}}}}}"#,
                    "\n",
                    r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","content":"{}"}}]}}}}"#,
                    "\n",
                ),
                filler
            ),
        )
        .expect("write transcript");

        let mut session = Session::new(Provider::Claude, "buried".into());
        session.data_file = Some(path);

        let ctx = extract_context(&session).expect("usage past the first tail window");
        assert_eq!(ctx.used, 647_414);
        assert!(!ctx.compacting);
    }
}
