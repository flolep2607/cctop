//! Session discovery and the extracted-data model.

pub mod claude;
pub mod codex;
pub mod extract;
pub mod opencode;
pub mod pi;

use crate::pricing::Provider;
use crate::util;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Where a session is being driven from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Surface {
    /// A coding agent in a terminal.
    Cli,
    /// Claude for Mac, running Claude Code locally.
    DesktopCode,
    /// Claude for Mac, running in a cloud VM.
    DesktopCowork,
}

impl Surface {
    pub fn is_desktop(&self) -> bool {
        matches!(self, Surface::DesktopCode | Surface::DesktopCowork)
    }

    pub fn label(&self, provider: Provider) -> &'static str {
        match (self, provider) {
            (Surface::DesktopCowork, _) => "Claude Cowork",
            (Surface::DesktopCode, _) => "Claude Code",
            (_, Provider::Claude) => "Claude",
            (_, Provider::Codex) => "Codex",
            (_, Provider::OpenCode) => "OpenCode",
            (_, Provider::Pi) => "Pi",
        }
    }
}

/// Paths backing a Claude for Mac session, needed to delete it cleanly.
#[derive(Debug, Clone)]
pub struct MacMeta {
    pub meta_path: PathBuf,
    pub session_dir: PathBuf,
}

/// Context-window consumption for a session or subagent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ContextUsage {
    pub used: u64,
    pub max: u64,
    #[serde(default)]
    pub compacting: bool,
}

impl ContextUsage {
    /// Percentage of the way to auto-compaction (not of the raw window).
    pub fn percent_to_compact(&self) -> f64 {
        let compact_at = self.max as f64 * *crate::config::COMPACT_THRESHOLD;
        if compact_at <= 0.0 {
            return 0.0;
        }
        self.used as f64 / compact_at * 100.0
    }
}

/// A discovered session plus everything annotated onto it for display.
#[derive(Debug, Clone)]
pub struct Session {
    pub provider: Provider,
    pub surface: Surface,
    pub session_id: String,
    pub started_at: String,
    pub last_active: String,
    pub model: String,
    /// Working directory the session was launched from.
    pub label_source: String,
    pub data_file: Option<PathBuf>,
    pub title: Option<String>,
    pub mac_meta: Option<MacMeta>,

    // --- Annotated after extraction ---
    pub abbrev_label: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tool_count: u64,
    /// `None` when the active plan bundles this provider's usage.
    pub total_cost: Option<f64>,
    pub cost_hour: f64,
    pub cost_today: f64,
    pub costs_by_day: HashMap<String, HashMap<String, f64>>,
    pub costs_by_hour: HashMap<String, HashMap<String, f64>>,
    pub subagents: Vec<Subagent>,
    pub subagents_cost: f64,
    pub context: Option<ContextUsage>,
    pub last_tool: String,
    pub process: Option<crate::proc::ProcInfo>,

    // --- Rate tracking ---
    pub tokens_per_min: f64,
    pub cost_per_min: f64,
}

impl Session {
    pub fn new(provider: Provider, session_id: String) -> Self {
        Session {
            provider,
            surface: Surface::Cli,
            session_id,
            started_at: String::new(),
            last_active: String::new(),
            model: String::new(),
            label_source: String::new(),
            data_file: None,
            title: None,
            mac_meta: None,
            abbrev_label: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            tool_count: 0,
            total_cost: Some(0.0),
            cost_hour: 0.0,
            cost_today: 0.0,
            costs_by_day: HashMap::new(),
            costs_by_hour: HashMap::new(),
            subagents: Vec::new(),
            subagents_cost: 0.0,
            context: None,
            last_tool: String::new(),
            process: None,
            tokens_per_min: 0.0,
            cost_per_min: 0.0,
        }
    }

    /// Stable identity used as a map key across refreshes.
    pub fn key(&self) -> String {
        format!("{}:{}", self.provider.as_str(), self.session_id)
    }

    pub fn is_running(&self) -> bool {
        self.process.is_some()
    }

    /// Title if renamed, else the abbreviated working directory.
    pub fn display_label(&self) -> &str {
        self.title
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| {
                if self.abbrev_label.is_empty() {
                    if self.label_source.is_empty() {
                        "unknown"
                    } else {
                        &self.label_source
                    }
                } else {
                    &self.abbrev_label
                }
            })
    }
}

// ---------------------------------------------------------------------------
// Extracted transcript data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tokens {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write_5m: u64,
    #[serde(default)]
    pub cache_write_1h: u64,
    // Codex-only
    #[serde(default)]
    pub input_total: u64,
    #[serde(default)]
    pub cached_input: u64,
    #[serde(default)]
    pub reasoning_output: u64,
    #[serde(default)]
    pub total: u64,
}

impl Tokens {
    /// Everything billed as input, across both providers' shapes.
    pub fn all_input(&self) -> u64 {
        self.input + self.cached_input + self.cache_read + self.cache_write_5m + self.cache_write_1h
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Costs {
    #[serde(default)]
    pub input: f64,
    #[serde(default)]
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write_5m: f64,
    #[serde(default)]
    pub cache_write_1h: f64,
    #[serde(default)]
    pub cached_input: f64,
    #[serde(default)]
    pub total: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelBreakdown {
    pub model: String,
    pub tokens: Tokens,
    pub costs: Costs,
    pub total: f64,
}

/// Line-level change produced by an edit, taken from the tool result's patch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Delta {
    pub added: u32,
    pub removed: u32,
    /// Unified-diff lines, capped by `MAX_DIFF_LINES`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hunks: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolDetail {
    /// Truncated single-line form for the panel.
    pub d: String,
    pub ts: String,
    /// Full text for the clipboard, when it differs from `d`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,
    /// `tool_use` id, used to match the call to its result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Wall time from the call being issued to its result arriving.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dur_ms: Option<i64>,
    /// Tokens billed for the assistant turn that issued this call.
    ///
    /// Billing is per API request, not per tool call, so when one turn issues
    /// several calls they all carry that turn's figures and `shared` records how
    /// many. Dividing would invent precision the transcript doesn't have.
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub shared: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
    /// Subagent that issued the call, or `None` for the main session. Tool
    /// activity from subagents is interleaved into the same log, so without this
    /// there is no way to tell who did what.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub tool_count: u64,
    pub tools: HashMap<String, u64>,
    pub tool_details: HashMap<String, Vec<ToolDetail>>,
    pub mcp_tool_count: u64,
    pub mcp_tools: Vec<String>,
    pub skill_count: u64,
    pub skills: HashMap<String, u64>,
    pub web_fetch_count: u64,
    pub web_fetches: Vec<String>,
    pub web_search_count: u64,
    pub web_searches: Vec<String>,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub api_duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubagentStatus {
    Running,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subagent {
    pub agent_id: String,
    #[serde(rename = "type")]
    pub agent_type: String,
    pub description: String,
    pub model: String,
    pub started_at: Option<String>,
    pub last_active: Option<String>,
    pub duration_ms: i64,
    pub status: SubagentStatus,
    pub cost: f64,
    pub tool_count: u64,
    pub tool_use_id: Option<String>,
    pub context: Option<ContextUsage>,
    /// The on-disk transcript was purged; only parent-side metadata survives.
    pub ghost: bool,
}

/// Everything parsed out of a session's transcript(s).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionData {
    pub title: Option<String>,
    pub custom_title: Option<String>,
    pub ai_title: Option<String>,
    /// Latest model seen in the *main* transcript, excluding subagent sidechains.
    pub last_model: String,
    pub models: Vec<String>,
    pub model_breakdown: Vec<ModelBreakdown>,
    pub tokens: Tokens,
    pub costs: Costs,
    /// `YYYY-MM-DD` -> model -> USD.
    pub costs_by_day: HashMap<String, HashMap<String, f64>>,
    /// `YYYY-MM-DDTHH` -> model -> USD.
    pub costs_by_hour: HashMap<String, HashMap<String, f64>>,
    pub metrics: Metrics,
    pub subagents: Vec<Subagent>,
    /// Codex reports per-million rates directly; surfaced in the Cost panel.
    pub rates: Option<CodexRates>,
    /// Set when extraction failed; the row still renders with zeroed figures.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CodexRates {
    pub input: f64,
    pub cached_input: f64,
    pub output: f64,
}

impl SessionData {
    /// Sum of a bucket map's per-model costs for one key.
    fn bucket_total(map: &HashMap<String, HashMap<String, f64>>, key: &str) -> f64 {
        map.get(key).map(|m| m.values().sum()).unwrap_or(0.0)
    }

    /// Spend in the current local hour.
    pub fn cost_this_hour(&self) -> f64 {
        let key = crate::util::local_hour_key(&chrono::Utc::now());
        Self::bucket_total(&self.costs_by_hour, &key)
    }

    /// Spend since local midnight.
    pub fn cost_today(&self) -> f64 {
        let today = crate::util::local_date_key(&chrono::Utc::now());
        self.costs_by_day
            .iter()
            .filter(|(day, _)| day.as_str() >= today.as_str())
            .map(|(_, m)| m.values().sum::<f64>())
            .sum()
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// All sessions from every known provider and surface, newest first.
pub fn list_all() -> Vec<Session> {
    let mut sessions = codex::list_sessions();
    sessions.extend(claude::list_sessions());
    sessions.extend(opencode::list_sessions());
    sessions.extend(pi::list_sessions());
    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    sessions
}

/// The main transcript plus any subagent sidechain transcripts.
pub fn transcript_files(main: &Path) -> Vec<PathBuf> {
    let mut files = vec![main.to_path_buf()];
    let stem = main.with_extension("");
    let subagents_dir = stem.join("subagents");
    if subagents_dir.is_dir() {
        for entry in crate::config::list_dir(&subagents_dir) {
            if entry.ends_with(".jsonl") {
                files.push(subagents_dir.join(entry));
            }
        }
    }
    files
}

/// Newest mtime across a session's transcripts.
///
/// A running subagent's file is appended without touching the parent, so the
/// parent's mtime alone would make an active session look idle.
pub fn effective_mtime_ms(session: &Session) -> u64 {
    let Some(f) = &session.data_file else {
        return 0;
    };
    match session.provider {
        Provider::Claude => transcript_files(f)
            .iter()
            .map(|p| crate::config::file_mtime_ms(p))
            .max()
            .unwrap_or(0),
        Provider::Codex | Provider::Pi => crate::config::file_mtime_ms(f),
        // Every OpenCode session shares one WAL-backed database. Using the DB
        // mtime would invalidate hundreds of unchanged sessions whenever one
        // message lands; its per-session updated timestamp is the right key.
        Provider::OpenCode => util::parse_ts(&session.last_active)
            .map(|d| d.timestamp_millis().max(0) as u64)
            .unwrap_or_else(|| crate::config::file_mtime_ms(f)),
    }
}
