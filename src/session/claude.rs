//! Claude session discovery and transcript extraction.

use super::extract::{self, for_each_jsonl, read_first_lines, read_last_lines};
use super::{
    ContextBreakdown, ContextUsage, Costs, MacMeta, Metrics, ModelBreakdown, Session, SessionData,
    Subagent, SubagentStatus, Surface, Tokens, transcript_files,
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

/// How long a subagent transcript must sit still before it counts as finished.
///
/// Long enough to cover the gap between a subagent's own turns — a slow model
/// reply or a long tool call writes nothing meanwhile — and short enough that a
/// finished agent does not linger as "running" for the rest of the session. The
/// `SubagentStop` hook answers this exactly when it is installed; this is what
/// the transcript alone can say.
const SUBAGENT_QUIET_MS: i64 = 30_000;

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

    for root in config::claude_projects_roots() {
        if !config::dir_exists(&root) {
            continue;
        }
        for project in config::list_dir(&root) {
            let project_dir = root.join(&project);
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
    let surfaces: [(&Option<PathBuf>, &str, Option<Surface>); 2] = [
        (
            &config::CLAUDE_MAC_COWORK_ROOT,
            "local-agent-mode-sessions",
            None,
        ),
        (
            &config::CLAUDE_MAC_CODE_ROOT,
            "claude-code-sessions",
            Some(Surface::DesktopCode),
        ),
    ];
    for (primary, leaf, forced) in surfaces {
        for root in config::claude_mac_roots(primary, leaf) {
            if config::dir_exists(&root) {
                scan_mac_root(&root, forced, &mut sessions);
            }
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
    /// Whether this turn ran in the session's own context rather than a
    /// sidechain, which decides whether its model can name the session.
    main_ctx: bool,
}

/// Order two transcript timestamps as instants.
///
/// String comparison is wrong the moment a transcript carries a non-`Z` offset
/// or a different sub-second precision. Unparseable stamps fall back to the
/// lexical order so behaviour never gets worse than before.
fn ts_before(a: &str, b: &str) -> bool {
    match (util::parse_ts(a), util::parse_ts(b)) {
        (Some(x), Some(y)) => x < y,
        _ => a < b,
    }
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

/// Characters of live-context content, by where it came from.
///
/// Counted in characters rather than tokens because the transcript is text and
/// cctop has no tokenizer; [`CHARS_PER_TOKEN`] converts once, at the end.
#[derive(Debug, Default)]
struct CtxChars {
    tool_output: u64,
    tool_input: u64,
    attachments: u64,
    user_text: u64,
    assistant_text: u64,
}

/// One stretch of conversation the window was measured over: from a start, or
/// from a compaction, up to the last request that reported its own size.
///
/// Grouped rather than left as loose fields so a compaction can retire the
/// whole segment in one move. Retiring only part of it is what makes the
/// measured total and the counted content describe different conversations.
#[derive(Debug, Default)]
struct CtxSegment {
    /// Window size at the segment's first request, and at its last.
    startup: Option<u64>,
    total: u64,
    chars: CtxChars,
    after_compaction: bool,
}

/// Characters per token for the text these transcripts carry.
///
/// Fitted, not assumed: across 167 local sessions, dividing the characters a
/// transcript accumulated by the context growth the API reported over the same
/// span lands at 2.75 — well under the usual prose rule of thumb, because this
/// content is mostly code, JSON and file paths. The fit is stable within a
/// session; it is the mix that moves it, not the length.
// ponytail: a fitted constant, not a tokenizer. If a category ever needs to be
// defensible on its own rather than as a share of the panel, swap in tiktoken.
const CHARS_PER_TOKEN: f64 = 2.75;

/// Whether an entry's content sits in the main conversation's context window.
///
/// Subagents run against their own window. Modern transcripts give them their
/// own file, but older ones interleave their turns into the parent's, flagged
/// `isSidechain` — and counting those would both credit another agent's tool
/// output to this session and let its usage figures move the window size itself.
/// Costs still count them, which is why this is separate from `is_main`: they
/// were billed to this session even though they were never in its context.
fn in_context(item: &Value, is_main: bool) -> bool {
    is_main && item.get("isSidechain").and_then(Value::as_bool) != Some(true)
}

/// Serialized size of a content value, as a stand-in for its share of the window.
///
/// Strings are measured directly so the common case allocates nothing; anything
/// structured is measured as the JSON the API is handed.
fn content_chars(v: Option<&Value>) -> u64 {
    match v {
        Some(Value::String(s)) => s.len() as u64,
        None | Some(Value::Null) => 0,
        Some(other) => other.to_string().len() as u64,
    }
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
    /// The segment the window is being measured over, restarted at each
    /// compaction.
    ctx: CtxSegment,
    /// The segment the last compaction retired, kept because the one that
    /// replaced it may never receive a request to measure.
    ctx_sealed: Option<CtxSegment>,
    /// Every request's window measurement, for the Context panel's chart.
    ctx_series: Vec<super::CtxPoint>,
    /// A compaction has happened and the next measured request is the first of
    /// the segment that replaced it.
    ctx_compacted: bool,
    /// How many compactions the transcript has been through, in total.
    compactions: u32,
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
        main_ctx: bool,
    ) {
        self.last_model = model.to_string();
        // `SessionData::last_model` promises the main transcript *excluding*
        // sidechains: older transcripts interleave `isSidechain` subagent turns
        // into the parent file, and their model must not name the session.
        if main_ctx {
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
        let ctx = in_context(item, is_main);

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
            Some(Value::String(s)) => {
                texts.push(s);
                if ctx {
                    self.ctx.chars.user_text += s.len() as u64;
                }
            }
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    match block {
                        Value::String(s) => {
                            texts.push(s);
                            if ctx {
                                self.ctx.chars.user_text += s.len() as u64;
                            }
                        }
                        Value::Object(_) => {
                            if let Some(t) = block.get("text").and_then(Value::as_str) {
                                texts.push(t);
                                if ctx {
                                    self.ctx.chars.user_text += t.len() as u64;
                                }
                            }
                            if block.get("type").and_then(Value::as_str) == Some("tool_result")
                                && let Some(id) = block.get("tool_use_id").and_then(Value::as_str)
                            {
                                if ctx {
                                    self.ctx.chars.tool_output +=
                                        content_chars(block.get("content"));
                                }
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

    /// Track how full the window was when this request was sent.
    ///
    /// The first request of a segment is the only place the harness's own
    /// overhead is visible: its input is the system prompt, the tool schemas,
    /// CLAUDE.md and the skills index, with nothing of the conversation in front
    /// of it yet. Everything counted before it is discarded rather than added,
    /// since that content is already inside the number.
    fn note_window(&mut self, message: &Value, ts: &str) {
        let Some(usage) = message.get("usage") else {
            return;
        };
        let u = |k: &str| usage.get(k).and_then(Value::as_u64).unwrap_or(0);
        let window =
            u("input_tokens") + u("cache_read_input_tokens") + u("cache_creation_input_tokens");
        if window == 0 {
            return;
        }
        if self.ctx.startup.is_none() {
            self.ctx.startup = Some(window);
            self.ctx.chars = CtxChars::default();
        }
        self.ctx.total = window;

        // The series spans the whole session rather than the live segment: a
        // compaction is the most interesting thing that can happen to a context
        // window, and a chart that restarted at each one would be the only view
        // in cctop that cannot show one.
        self.ctx_series.push(super::CtxPoint {
            ts: ts.to_string(),
            window,
            after_compaction: std::mem::take(&mut self.ctx_compacted),
        });
        super::decimate(&mut self.ctx_series);
    }

    /// A compaction replaced the conversation with a summary, so everything
    /// counted so far has left the window. Start the segment over; the next
    /// request's input becomes the new baseline, summary included.
    ///
    /// The old segment is sealed rather than dropped: a session can compact and
    /// then stop, and the segment opening here would then hold no request at
    /// all. The window it describes is gone, but it is the last one that was
    /// ever measured, and reporting it whole beats reporting a total from one
    /// side of the boundary against parts from the other.
    fn note_compaction(&mut self) {
        let sealed = std::mem::take(&mut self.ctx);
        if sealed.total > 0 {
            self.ctx_sealed = Some(sealed);
        }
        self.ctx.after_compaction = true;
        self.ctx_compacted = true;
        self.compactions += 1;
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

        // Before anything is counted: the request's own view of how full the
        // window is. Done first so the segment's opening request resets the
        // character counters before this entry's blocks land in them.
        let ctx = in_context(item, is_main);
        if ctx {
            self.note_window(message, ts);
        }

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
                    // What the assistant said back. Its thinking is deliberately
                    // not counted: Claude Code writes those blocks with an empty
                    // `thinking` string and only the signature survives, so there
                    // is nothing to measure and it stays in the unaccounted gap.
                    if ctx && block.get("type").and_then(Value::as_str) == Some("text") {
                        self.ctx.chars.assistant_text += content_chars(block.get("text"));
                    }
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
                if ctx {
                    self.ctx.chars.tool_input +=
                        content_chars(block.get("input")) + name.len() as u64;
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
            && (stats.latest_used_ts.is_empty() || !ts_before(ts, &stats.latest_used_ts))
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
            main_ctx: ctx,
        };

        // Streaming writes the same requestId repeatedly with growing counts;
        // keeping only the last occurrence per key avoids inflating totals.
        match turn_key.as_deref() {
            Some(k) => {
                last_by_key.insert(k.to_string(), snapshot);
            }
            None => self.accumulate(
                file, model, input, cache_read, output, cw5m, cw1h, ts, is_main, ctx,
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
            if stats.first_ts.as_deref().is_none_or(|f| ts_before(ts, f)) {
                stats.first_ts = Some(ts.to_string());
            }
            if stats.last_ts.as_deref().is_none_or(|l| ts_before(l, ts)) {
                stats.last_ts = Some(ts.to_string());
            }
        }

        // Newer transcripts flag the summary entry itself; older ones wrote a
        // `compact_boundary` system entry instead. Both mean the same thing.
        if in_context(item, is_main)
            && (item.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
                || item.get("subtype").and_then(Value::as_str) == Some("compact_boundary"))
        {
            self.note_compaction();
        }

        match item.get("type").and_then(Value::as_str) {
            Some("system") => {
                if item.get("subtype").and_then(Value::as_str) == Some("turn_duration")
                    && let Some(ms) = item.get("durationMs").and_then(Value::as_u64)
                {
                    self.metrics.api_duration_ms += ms;
                }
            }
            // Hook output, task reminders, IDE state, @-mentioned files and the
            // skill listing: content the harness splices into the next user
            // message, recorded as its own entry rather than inside one.
            Some("attachment") if in_context(item, is_main) => {
                let attachment = item.get("attachment");
                // Some kinds carry no `content` and are the payload themselves,
                // such as the file reference a compaction leaves behind.
                self.ctx.chars.attachments += match attachment.and_then(|a| a.get("content")) {
                    Some(content) => content_chars(Some(content)),
                    None => content_chars(attachment),
                };
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
        if let Err(err) = for_each_jsonl(file, |item| {
            ext.visit(item, file, is_main, &mut last_by_key);
        }) {
            // Never fall through to a fabricated $0: the cache only refuses to
            // persist a session when it carries an error.
            return SessionData {
                error: Some(format!(
                    "Could not read Claude transcript {}: {err}",
                    file.display()
                )),
                ..Default::default()
            };
        }
        // Flush the deduped snapshots for this file. The surviving entry per
        // request carries that turn's final token counts.
        let mut snapshots: Vec<(String, Snapshot)> = last_by_key.into_iter().collect();
        // A HashMap's order is randomised per process, and the last model
        // flushed becomes the session's MODEL. Order by instant (request key as
        // a tiebreak) so multi-model sessions don't flip between runs.
        snapshots.sort_by(|(ka, a), (kb, b)| {
            util::parse_ts(&a.ts)
                .cmp(&util::parse_ts(&b.ts))
                .then_with(|| ka.cmp(kb))
        });
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
                s.main_ctx,
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

    // A session with no usage figures has no window to break down; the estimate
    // alone would be a share of nothing.
    let est = |chars: u64| (chars as f64 / CHARS_PER_TOKEN).round() as u64;
    // A compaction with no request behind it leaves the live segment unmeasured:
    // fall back to the one it retired, whole, and say so. Splitting the
    // difference would put a pre-compaction total next to post-compaction parts,
    // and the entire conversation would surface as unaccounted.
    let superseded = ext.ctx.startup.is_none() && ext.ctx_sealed.is_some();
    let seg = match &ext.ctx_sealed {
        Some(sealed) if superseded => sealed,
        _ => &ext.ctx,
    };
    let c = &seg.chars;
    let context_breakdown = (seg.total > 0).then(|| ContextBreakdown {
        total: seg.total,
        startup: seg.startup.unwrap_or(0),
        tool_output: est(c.tool_output),
        tool_input: est(c.tool_input),
        attachments: est(c.attachments),
        user_text: est(c.user_text),
        assistant_text: est(c.assistant_text),
        after_compaction: seg.after_compaction,
        superseded,
    });

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
        context_breakdown,
        context_series: ext.ctx_series,
        compactions: ext.compactions,
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
    // Counted from the set of failed ids rather than from the details below,
    // which are capped: a session busy enough to overflow that cap is exactly
    // the one whose error rate is worth reading.
    ext.metrics.tool_errors = ext.tool_failed.len() as u64;
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

        // A transcript being appended to outranks the parent's tool_result,
        // because that result does not always mean what it appears to: an agent
        // spawned in the background is acknowledged the moment it *starts*, so
        // taking the result at face value marked every background subagent
        // finished about three seconds into a run that had barely begun. Still
        // writing means still working, whatever the parent has recorded.
        //
        // A subagent that has genuinely finished goes quiet and stays quiet, so
        // the two rules agree everywhere except the window this exists to fix.
        let newest = last_active_ms.max(last_ms.unwrap_or(0));
        let quiet = now_ms - newest >= SUBAGENT_QUIET_MS;
        let status = match &tool_use_id {
            Some(id) => {
                if ext.agent_results.contains(id) && quiet {
                    SubagentStatus::Done
                } else {
                    SubagentStatus::Running
                }
            }
            None => {
                if quiet {
                    SubagentStatus::Done
                } else {
                    SubagentStatus::Running
                }
            }
        };

        // Subagents hold isolated contexts, so infer the window from their own
        // usage. LiteLLM reports 1M for any model that *can* do 1M with the beta
        // header, which would understate CTX% for the common 200k default.
        let context = stats.filter(|s| s.latest_used > 0).map(|s| ContextUsage {
            used: s.latest_used,
            max: resolve_ctx_max(None, s.latest_used, None),
            compacted: false,
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

/// Size of the context window, from the most trustworthy source that knows one:
/// a pinned setting, then usage that has already outgrown the default, then
/// LiteLLM, then the default itself.
///
/// Every caller resolves through here so the sources cannot name two different
/// sizes for the same window. They would otherwise: whichever branch fires first
/// becomes the denominator of every percentage in the UI and fixes where the
/// auto-compaction marker sits, so a session that happens to cross 200k would
/// stop agreeing with its neighbours on the same model.
fn resolve_ctx_max(settings_ctx: Option<u64>, used: u64, litellm_ctx: Option<u64>) -> u64 {
    // Observed usage outranks every table, because it is the only figure that
    // was measured rather than looked up: a project settings file naming a 200k
    // model must not report a 1M session as 239% full. `used` itself is the
    // floor, so a window nobody can name still never reads as over-full.
    let named = settings_ctx.or(litellm_ctx).unwrap_or(CLAUDE_DEFAULT_CTX);
    if used > named {
        return CLAUDE_1M_CTX.max(used);
    }
    named
}

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

    // The session's own model string beats any settings file: the settings may
    // have changed, or name a different model than this session is running.
    let settings_ctx = session
        .model
        .contains("[1m]")
        .then_some(CLAUDE_1M_CTX)
        .or_else(|| settings_context(session));
    let litellm_ctx = pricing::litellm_max_input_tokens(&session.model);
    let mut compacted = false;

    // Widening rescans lines it has already rejected, but only ever from behind
    // the newest usage entry, so the boundary flag keeps meaning "a compaction
    // has replaced the window and nothing has measured the new one" rather than
    // "this session compacted once".
    let used = util::scan_tail_escalating(file, |text| {
        for line in text.lines().rev() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(item) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            // Both spellings of a compaction, matching what the full extractor
            // looks for: newer transcripts flag the summary entry itself, older
            // ones wrote a `compact_boundary`. Reading only one of them here
            // would let the column and the context panel disagree about whether
            // the same transcript has been compacted.
            if item.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
                || (item.get("type").and_then(Value::as_str) == Some("system")
                    && item.get("subtype").and_then(Value::as_str) == Some("compact_boundary"))
            {
                compacted = true;
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
        return Some(ContextUsage {
            used,
            max: resolve_ctx_max(settings_ctx, used, litellm_ctx),
            compacted,
        });
    }

    compacted.then(|| ContextUsage {
        used: 0,
        max: resolve_ctx_max(settings_ctx, 0, litellm_ctx),
        compacted: true,
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
pub fn delete(session: &Session) -> std::io::Result<()> {
    let Some(file) = session.data_file.as_ref() else {
        return Ok(());
    };
    std::fs::remove_file(file)?;
    remove_dir_if_present(&file.with_extension(""))?;
    if let Some(meta) = &session.mac_meta {
        remove_file_if_present(&meta.meta_path)?;
        remove_dir_if_present(&meta.session_dir)?;
    }
    Ok(())
}

fn remove_file_if_present(path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn remove_dir_if_present(path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp path, extension-free so callers can make it a file or dir.
    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cctop-claude-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ))
    }

    /// Write a transcript to a unique temp path and parse it.
    fn extract_lines(tag: &str, lines: &[String]) -> SessionData {
        let path = temp_path(tag).with_extension("jsonl");
        std::fs::write(&path, format!("{}\n", lines.join("\n"))).expect("write transcript");
        let data = extract(&path);
        let _ = std::fs::remove_file(&path);
        data
    }

    fn assistant(request: &str, window: u64, content: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"2026-08-05T10:00:00.000Z","requestId":"{request}","message":{{"id":"m_{request}","role":"assistant","model":"claude-opus-5","content":[{content}],"usage":{{"input_tokens":{window},"output_tokens":5}}}}}}"#
        )
    }

    /// The chart spans the session, not the live segment: a compaction is the
    /// most interesting thing that can happen to a context window, and the
    /// series is the only view that can show one. The point that opens the new
    /// segment is the one marked, because that is where the drop lands.
    #[test]
    fn the_context_series_spans_compactions_and_marks_them() {
        let data = extract_lines(
            "ctx-series",
            &[
                assistant("req_1", 50_000, r#"{"type":"text","text":"a"}"#),
                assistant("req_2", 90_000, r#"{"type":"text","text":"b"}"#),
                r#"{"type":"user","isCompactSummary":true,"timestamp":"2026-08-05T10:00:02.000Z","message":{"role":"user","content":"a summary of everything above"}}"#.to_string(),
                assistant("req_3", 20_000, r#"{"type":"text","text":"c"}"#),
                assistant("req_4", 35_000, r#"{"type":"text","text":"d"}"#),
            ],
        );

        let windows: Vec<u64> = data.context_series.iter().map(|p| p.window).collect();
        assert_eq!(windows, vec![50_000, 90_000, 20_000, 35_000]);
        let marked: Vec<bool> = data
            .context_series
            .iter()
            .map(|p| p.after_compaction)
            .collect();
        assert_eq!(
            marked,
            vec![false, false, true, false],
            "only the first request of the segment after a compaction is marked"
        );
    }

    /// A long session must not carry an unbounded series into the cache. The
    /// endpoints survive decimation, because the first and last windows are the
    /// two the header quotes.
    #[test]
    fn a_long_series_is_decimated_but_keeps_its_ends() {
        let mut series: Vec<super::super::CtxPoint> = (0..super::super::MAX_CTX_POINTS + 1)
            .map(|i| super::super::CtxPoint {
                ts: format!("{i}"),
                window: i as u64,
                after_compaction: false,
            })
            .collect();
        let last = series.last().expect("a point").window;
        super::super::decimate(&mut series);

        assert!(series.len() <= super::super::MAX_CTX_POINTS);
        assert_eq!(series.first().expect("a point").window, 0);
        assert_eq!(series.last().expect("a point").window, last);
    }

    /// The panel's whole claim is that it separates what was measured from what
    /// was guessed. `startup` is the first request's own input count, so anything
    /// the transcript recorded before it — the opening prompt here — is already
    /// inside that number and must not be counted a second time.
    #[test]
    fn context_breakdown_measures_startup_and_estimates_the_rest() {
        let result = "x".repeat(2750);
        let data = extract_lines(
            "ctx-split",
            &[
                r#"{"type":"user","timestamp":"2026-08-05T10:00:00.000Z","message":{"role":"user","content":"an opening prompt long enough to notice"}}"#.to_string(),
                assistant("req_1", 1000, r#"{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}"#),
                format!(
                    r#"{{"type":"user","timestamp":"2026-08-05T10:00:01.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_1","content":"{result}"}}]}}}}"#
                ),
                assistant("req_2", 5000, r#"{"type":"text","text":"done"}"#),
            ],
        );

        let b = data.context_breakdown.expect("a breakdown");
        assert_eq!(b.total, 5000, "the window is the last request's own figure");
        assert_eq!(b.startup, 1000, "startup is the first request's own figure");
        assert_eq!(
            b.user_text, 0,
            "the opening prompt is already inside startup"
        );
        assert_eq!(b.tool_output, 1000, "2750 chars at 2.75 chars/token");
        assert!(
            b.tool_input > 0,
            "the call's arguments occupy the window too"
        );
        assert!(!b.after_compaction);

        // Nothing is scaled to fill the window; the shortfall is reported as-is.
        assert_eq!(
            b.unaccounted(),
            5000 - 1000 - b.estimated() as i64,
            "the gap must be the plain remainder"
        );
        assert!(b.unaccounted() > 0);
    }

    /// A compaction throws the conversation away and replaces it with a summary,
    /// so a breakdown that kept counting across it would describe a window that
    /// no longer exists.
    #[test]
    fn compaction_restarts_the_context_breakdown() {
        let result = "x".repeat(2750);
        let data = extract_lines(
            "ctx-compact",
            &[
                assistant("req_1", 1000, r#"{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}"#),
                format!(
                    r#"{{"type":"user","timestamp":"2026-08-05T10:00:01.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_1","content":"{result}"}}]}}}}"#
                ),
                assistant("req_2", 5000, r#"{"type":"text","text":"done"}"#),
                r#"{"type":"user","isCompactSummary":true,"timestamp":"2026-08-05T10:00:02.000Z","message":{"role":"user","content":"a summary of everything above"}}"#.to_string(),
                assistant("req_3", 1500, r#"{"type":"text","text":"carrying on"}"#),
            ],
        );

        let b = data.context_breakdown.expect("a breakdown");
        assert!(b.after_compaction);
        assert_eq!(b.total, 1500);
        assert_eq!(
            b.startup, 1500,
            "the post-compaction baseline absorbs the summary"
        );
        assert_eq!(
            b.tool_output, 0,
            "the pre-compaction result has left the window"
        );
        assert_eq!(b.tool_input, 0);
        assert!(
            !b.superseded,
            "a request measured the post-compaction window"
        );
    }

    /// A session can compact and stop in the same breath, leaving the new
    /// segment with no request in it at all. The last usage figure then predates
    /// the boundary while every counted character postdates it, and pairing the
    /// two would file the entire conversation under `unaccounted` — which is
    /// supposed to hold only what the transcript cannot see. Reporting the
    /// retired segment whole keeps the total and its parts talking about one
    /// window.
    #[test]
    fn a_compaction_with_no_request_behind_it_reports_the_window_it_retired() {
        let result = "x".repeat(2750);
        let conversation = [
            assistant(
                "req_1",
                1000,
                r#"{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}"#,
            ),
            format!(
                r#"{{"type":"user","timestamp":"2026-08-05T10:00:01.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_1","content":"{result}"}}]}}}}"#
            ),
            assistant("req_2", 5000, r#"{"type":"text","text":"done"}"#),
        ];
        let mut compacted = conversation.to_vec();
        compacted.push(
            r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-08-05T10:00:02.000Z"}"#
                .to_string(),
        );

        let before = extract_lines("ctx-stop", &conversation)
            .context_breakdown
            .expect("a breakdown");
        let b = extract_lines("ctx-compact-stop", &compacted)
            .context_breakdown
            .expect("a breakdown");

        assert!(b.superseded, "nothing measured the window that replaced it");
        assert_eq!(b.total, before.total, "the last window anyone measured");
        assert_eq!(b.startup, before.startup, "from that same segment");
        assert_eq!(b.tool_output, before.tool_output, "and so are its parts");
        assert_eq!(
            b.unaccounted(),
            before.unaccounted(),
            "a trailing boundary reveals nothing new about the window it closed, \
             so it must not open a gap in it"
        );
    }

    /// The ordinary post-compaction session: the summary lands, work resumes,
    /// and the segment is measured from its own first request. If the retired
    /// segment leaked into this one the window would be described twice over.
    #[test]
    fn a_session_that_kept_working_after_compacting_describes_only_the_new_segment() {
        let result = "x".repeat(2750);
        let data = extract_lines(
            "ctx-compact-resume",
            &[
                assistant("req_1", 90_000, r#"{"type":"text","text":"before"}"#),
                r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-08-05T10:00:02.000Z"}"#.to_string(),
                r#"{"type":"user","isCompactSummary":true,"timestamp":"2026-08-05T10:00:03.000Z","message":{"role":"user","content":"a summary of everything above"}}"#.to_string(),
                assistant("req_2", 20_000, r#"{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"ls"}}"#),
                format!(
                    r#"{{"type":"user","timestamp":"2026-08-05T10:00:04.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_2","content":"{result}"}}]}}}}"#
                ),
                assistant("req_3", 22_000, r#"{"type":"text","text":"carrying on"}"#),
            ],
        );

        let b = data.context_breakdown.expect("a breakdown");
        assert!(b.after_compaction, "this segment opens on a summary");
        assert!(!b.superseded, "and a request has measured it");
        assert_eq!(b.total, 22_000);
        assert_eq!(
            b.startup, 20_000,
            "the summary is inside the segment's first request"
        );
        assert_eq!(b.tool_output, 1000, "counted from the boundary onwards");
        assert!(
            b.unaccounted() < 1_000,
            "the pre-compaction window must not reappear as a gap"
        );
    }

    /// Older transcripts write a subagent's turns into the parent's file. Those
    /// run against their own window, so letting one in would both credit its
    /// tool output to this session and let its usage figures resize the window.
    #[test]
    fn interleaved_subagent_turns_stay_out_of_the_context_breakdown() {
        let result = "x".repeat(2750);
        let sidechain = format!(
            r#"{{"type":"user","isSidechain":true,"timestamp":"2026-08-05T10:00:01.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"toolu_sub","content":"{result}"}}]}}}}"#
        );
        let data = extract_lines(
            "ctx-sidechain",
            &[
                assistant("req_1", 1000, r#"{"type":"text","text":"starting"}"#),
                sidechain,
                r#"{"type":"assistant","isSidechain":true,"timestamp":"2026-08-05T10:00:02.000Z","requestId":"req_sub","message":{"id":"m_sub","role":"assistant","model":"claude-haiku-4-5","content":[{"type":"text","text":"agent reply"}],"usage":{"input_tokens":90000,"output_tokens":5}}}"#.to_string(),
                assistant("req_2", 2000, r#"{"type":"text","text":"done"}"#),
            ],
        );

        let b = data.context_breakdown.expect("a breakdown");
        assert_eq!(b.total, 2000, "a subagent's window is not this session's");
        assert_eq!(
            b.tool_output, 0,
            "its tool output never entered this window"
        );
        // Its cost still counts: it was billed to this session either way.
        assert!(data.costs.total > 0.0);
    }

    /// Regression: snapshots were flushed straight out of a `HashMap`, whose
    /// order is randomised per process, so a two-model session's MODEL column
    /// flipped between runs — and the wrong value got cached.
    #[test]
    fn last_model_follows_the_timestamps_not_the_hash_order() {
        // Many turns, so a randomised order would disagree with itself quickly.
        let mut body = String::new();
        for i in 0..40 {
            let model = if i == 39 {
                "claude-opus-5"
            } else {
                "claude-haiku-4-5"
            };
            body.push_str(&format!(
                r#"{{"type":"assistant","timestamp":"2026-08-05T10:{:02}:00.000Z","requestId":"req_{i}","message":{{"id":"m{i}","model":"{model}","usage":{{"input_tokens":10,"output_tokens":2}}}}}}"#,
                i
            ));
            body.push('\n');
        }
        let path = temp_path("order").with_extension("jsonl");
        std::fs::write(&path, &body).expect("write transcript");
        let data = extract(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(data.last_model, "claude-opus-5");
    }

    /// `SessionData::last_model` documents "excluding subagent sidechains", but
    /// older transcripts interleave `isSidechain` turns into the parent file.
    #[test]
    fn sidechain_turns_do_not_claim_the_main_model_column() {
        let path = temp_path("sidechain").with_extension("jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"assistant","timestamp":"2026-08-05T10:00:00.000Z","requestId":"req_1","message":{"id":"m1","model":"claude-opus-5","usage":{"input_tokens":10,"output_tokens":2}}}"#,
                "\n",
                r#"{"type":"assistant","isSidechain":true,"timestamp":"2026-08-05T10:00:05.000Z","requestId":"req_2","message":{"id":"m2","model":"claude-haiku-4-5","usage":{"input_tokens":10,"output_tokens":2}}}"#,
                "\n",
            ),
        )
        .expect("write transcript");
        let data = extract(&path);
        let _ = std::fs::remove_file(&path);

        assert_eq!(data.last_model, "claude-opus-5");
        // The sidechain's tokens still count towards the session's totals.
        assert_eq!(data.tokens.output, 4);
    }

    /// Regression: a project settings file naming a 200k model used to win over
    /// what the session had actually used, yielding a CTX% of ~239%.
    #[test]
    fn context_window_is_never_smaller_than_observed_usage() {
        let dir = temp_path("ctx");
        std::fs::create_dir_all(dir.join(".claude")).expect("create settings dir");
        std::fs::write(
            dir.join(".claude").join("settings.json"),
            r#"{"model":"opus"}"#,
        )
        .expect("write settings");
        let transcript = dir.join("session.jsonl");
        std::fs::write(
            &transcript,
            r#"{"type":"assistant","timestamp":"2026-08-05T10:00:00.000Z","message":{"model":"claude-opus-5","usage":{"input_tokens":400000,"output_tokens":2}}}"#,
        )
        .expect("write transcript");

        let mut session = Session::new(Provider::Claude, "s1".into());
        session.label_source = dir.to_string_lossy().into_owned();
        session.data_file = Some(transcript);
        let ctx = extract_context(&session).expect("context usage");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(ctx.used, 400_000);
        assert!(ctx.max >= ctx.used, "{} < {}", ctx.max, ctx.used);
    }

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
        // And the total behind the ERR% column, which is counted from the set
        // of failed ids rather than from the capped details above.
        assert_eq!(data.metrics.tool_errors, 1);
        assert_eq!(data.metrics.tool_count, 2);
    }

    /// Every route to the large window has to name the same size. Two sessions on
    /// one model differ only in how far they have filled it, and that must not
    /// change the denominator: if crossing the 200k default swapped LiteLLM's
    /// figure for a different constant, every percentage in the UI and the
    /// auto-compaction marker would jump the moment a session got busy.
    #[test]
    fn crossing_the_default_does_not_change_the_window_litellm_reported() {
        let litellm = pricing::litellm_max_input_tokens("claude-opus-5").or(Some(CLAUDE_1M_CTX));
        let below = resolve_ctx_max(None, CLAUDE_DEFAULT_CTX - 1, litellm);
        let above = resolve_ctx_max(None, CLAUDE_DEFAULT_CTX + 1, litellm);
        let pinned = resolve_ctx_max(Some(CLAUDE_1M_CTX), 1, litellm);
        assert_eq!(below, above, "inference must agree with LiteLLM");
        assert_eq!(above, pinned, "inference must agree with a pinned [1m]");
    }

    /// The ordering the sources are consulted in is itself behaviour. A pinned
    /// setting outranks the tables, because the user stated it outright; but
    /// measured usage outranks even the pin, because a settings file naming a
    /// 200k model is a claim about the model and the usage figure is a fact
    /// about this session — deferring to the claim reports a 1M session as 239%
    /// full. The 200k default catches whatever nothing else knows.
    #[test]
    fn what_was_measured_outranks_what_was_configured() {
        assert_eq!(
            resolve_ctx_max(Some(CLAUDE_DEFAULT_CTX), 1, Some(CLAUDE_1M_CTX)),
            CLAUDE_DEFAULT_CTX,
            "a pin outranks LiteLLM while usage fits inside it"
        );
        assert_eq!(
            resolve_ctx_max(Some(CLAUDE_DEFAULT_CTX), CLAUDE_1M_CTX, Some(CLAUDE_1M_CTX)),
            CLAUDE_1M_CTX,
            "usage past the pinned window means the pin is describing another model"
        );
        // Nothing names a window this large, and a bar cannot be more than full.
        assert_eq!(
            resolve_ctx_max(None, CLAUDE_1M_CTX + 1, None),
            CLAUDE_1M_CTX + 1
        );
        assert_eq!(resolve_ctx_max(None, 1, None), CLAUDE_DEFAULT_CTX);
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
        assert!(!ctx.compacted);
    }
}
