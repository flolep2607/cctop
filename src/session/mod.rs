//! Session discovery and the extracted-data model.

pub mod claude;
pub mod codex;
pub mod cursor;
pub mod extract;
pub mod gemini;
pub mod opencode;
pub mod pi;
pub mod search;
pub mod windsurf;

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
    /// An agent hosted by an editor rather than a dedicated CLI process.
    Editor,
    /// Claude for Mac, running Claude Code locally.
    DesktopCode,
    /// Claude for Mac, running in a cloud VM.
    DesktopCowork,
}

/// What a live agent is doing, inferred from the newest transcript event.
///
/// This deliberately captures only states that have a clear user-facing
/// meaning.  A missing or unrecognised event remains normal work rather than
/// guessing that the agent is stalled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActivityState {
    #[default]
    Working,
    WaitingForInput,
    ApiError,
}

impl Surface {
    pub fn is_desktop(&self) -> bool {
        matches!(self, Surface::DesktopCode | Surface::DesktopCowork)
    }

    pub fn label(&self, provider: Provider) -> &'static str {
        match (self, provider) {
            (Surface::Editor, Provider::Cursor) => "Cursor",
            (Surface::DesktopCowork, _) => "Claude Cowork",
            (Surface::DesktopCode, _) => "Claude Code",
            (_, Provider::Claude) => "Claude",
            (_, Provider::Codex) => "Codex",
            (_, Provider::Cursor) => "Cursor",
            (_, Provider::Gemini) => "Gemini",
            (_, Provider::OpenCode) => "OpenCode",
            (_, Provider::Pi) => "Pi",
            (_, Provider::Windsurf) => "Windsurf",
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
    /// A compaction is the newest thing in the transcript: it replaced the
    /// window and no request has measured the new one yet, so `used` describes
    /// the window as it stood *before* that compaction.
    ///
    /// Deliberately not called "compacting". The transcript cannot tell a
    /// compaction that is still running from one the session stopped right
    /// after, and a session that compacted and ended is not busy — only
    /// liveness settles that, so [`Session::is_compacting`] is where the two
    /// facts meet.
    #[serde(default, alias = "compacting")]
    pub compacted: bool,
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

/// What is occupying a session's context window, split by category.
///
/// Two of these numbers are measured and the rest are estimated, which is the
/// whole reason the type keeps them apart. `total` and `startup` come from the
/// usage figures the API itself reported; every other field is inferred from how
/// many characters the transcript holds. Nothing is scaled to make the parts add
/// up to the window — whatever is left over is [`unaccounted`], and that gap is
/// the honest answer to what the transcript cannot see.
///
/// Every field describes one and the same segment — the stretch of conversation
/// between a start (or a compaction) and the last request that reported its
/// size. Mixing segments is the one way this type can lie without any single
/// number being wrong: a `total` from one side of a compaction and parts from
/// the other differ by the whole conversation, and all of it lands in
/// [`unaccounted`], which is meant to hold only what cannot be seen.
///
/// [`unaccounted`]: ContextBreakdown::unaccounted
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ContextBreakdown {
    /// Window size at the last request, from its own usage figures. Exact.
    pub total: u64,
    /// The first request of the live segment: system prompt, tool schemas,
    /// CLAUDE.md, the skills index — everything sent before the conversation
    /// starts, plus the summary when the segment follows a compaction. Exact,
    /// but not decomposable: the transcript never records what the harness sent.
    pub startup: u64,
    /// Estimated from transcript characters.
    pub tool_output: u64,
    pub tool_input: u64,
    pub attachments: u64,
    pub user_text: u64,
    pub assistant_text: u64,
    /// The segment begins at a compaction summary rather than at the start of
    /// the session, so `startup` carries that summary too.
    pub after_compaction: bool,
    /// A compaction has since replaced this segment and no request has measured
    /// its replacement yet, so these numbers describe the window as it stood
    /// before that compaction — the last one anything measured.
    pub superseded: bool,
}

/// One request's measurement of the context window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtxPoint {
    pub ts: String,
    pub window: u64,
    /// The first request of a segment that follows a compaction — where the
    /// window dropped because the harness reclaimed it, not because the
    /// conversation shrank.
    #[serde(default)]
    pub after_compaction: bool,
}

/// How many points a series keeps.
///
/// A long session issues thousands of requests and the chart is at most a few
/// hundred columns wide, so the tail beyond this is detail no reader can see.
/// It is enforced by dropping every other point once the cap is passed, which
/// keeps the shape and the endpoints while halving the resolution — the same
/// trade a chart makes when it draws.
pub const MAX_CTX_POINTS: usize = 2000;

/// Halve `series` in place, keeping the first and last points.
pub fn decimate(series: &mut Vec<CtxPoint>) {
    if series.len() <= MAX_CTX_POINTS {
        return;
    }
    let last = series.pop();
    let mut kept: Vec<CtxPoint> = series.iter().step_by(2).cloned().collect();
    if let Some(last) = last {
        kept.push(last);
    }
    *series = kept;
}

impl ContextBreakdown {
    /// Everything the transcript could be read for, `startup` excluded.
    pub fn estimated(&self) -> u64 {
        self.tool_output + self.tool_input + self.attachments + self.user_text + self.assistant_text
    }

    /// The window minus everything attributed to it.
    ///
    /// Negative when the estimate overshoots, which happens when the harness has
    /// dropped old tool results from the window that the transcript still holds.
    /// Reported signed rather than clamped, because "the categories below add up
    /// to more than the window" is information, not an error.
    pub fn unaccounted(&self) -> i64 {
        self.total as i64 - self.startup as i64 - self.estimated() as i64
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
    /// Where the agent is hosted, when it can be inferred (for example Cursor
    /// versus a terminal CLI). This is intentionally distinct from `model`.
    pub harness: String,
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
    /// Tool calls the transcript reported as failed. Zero for a provider that
    /// records no outcome — see [`Session::error_rate`], which is the only
    /// thing that should read this.
    pub tool_errors: u64,
    /// Compactions this session has been through, when the harness records
    /// them. Claude Code only, so `0` elsewhere means "not said" rather than
    /// "never happened".
    pub compactions: u32,
    /// `None` when the active plan bundles this provider's usage.
    pub total_cost: Option<f64>,
    /// False when the provider's transcript contains no billable usage data.
    pub cost_available: bool,
    /// Recorded usage has a zero total cost, as with a free model. This stays
    /// distinct from a session that simply has not recorded any usage yet.
    pub cost_is_free: bool,
    pub cost_hour: f64,
    pub cost_today: f64,
    pub costs_by_day: HashMap<String, HashMap<String, f64>>,
    pub costs_by_hour: HashMap<String, HashMap<String, f64>>,
    pub subagents: Vec<Subagent>,
    pub subagents_cost: f64,
    pub context: Option<ContextUsage>,
    pub last_tool: String,
    pub process: Option<crate::proc::ProcInfo>,
    /// Liveness inferred from a growing transcript when no per-session process
    /// exists (currently Cursor native agents).
    pub inferred_running: bool,
    /// A transcript-derived state that refines the liveness dot.
    pub activity_state: ActivityState,
    /// How much this session asks before it acts, when its own hooks have said.
    ///
    /// Read from the transcript, which Claude Code stamps with the mode on
    /// every `user` record, and overwritten by the session's own hooks when it
    /// has them — those are fresher, since they arrive as the turn happens
    /// rather than after it is written down.
    ///
    /// `None` when neither could say: a harness that does not record it, or a
    /// transcript whose tail holds no user record. That is deliberately not the
    /// same as "it asks about everything", which would be a guess about the one
    /// column whose whole job is not to guess.
    pub permission: Option<crate::hook::Permission>,

    /// Absolute paths this session has written recently, newest first.
    ///
    /// Kept on the row rather than looked up per frame because the only
    /// question asked of it — does anyone else hold this file — is asked about
    /// every live session at once, and [`SessionData`] is loaded for one.
    /// Bounded by [`MAX_RECENT_WRITES`]; spelled by [`crate::collide::normalise`]
    /// so two harnesses' spellings of one path compare equal.
    pub recent_writes: Vec<String>,

    /// How close another live session is to this one's work, as
    /// [`crate::collide`] last measured it. `None` is the ordinary case: no
    /// other running agent shares this repository.
    pub conflict: Option<crate::collide::Overlap>,

    /// Set when this row came from another machine over ssh, rather than from
    /// this one's disk.
    ///
    /// The single test for "cctop cannot act on this": every path that signals
    /// a process, deletes a transcript, opens a pty or reads a git directory is
    /// about *this* filesystem, and would quietly do the wrong thing to a
    /// same-named path if it ran for a remote row.
    pub remote: Option<Remote>,

    // --- Rate tracking ---
    pub tokens_per_min: f64,
    pub cost_per_min: f64,
}

/// Where a row came from, when it did not come from this machine.
#[derive(Debug, Clone)]
pub struct Remote {
    /// The ssh target exactly as the user spelled it, which is what the HOST
    /// column shows and what any message about the row names.
    pub host: String,
    /// Branch as that machine read it. Carried rather than looked up, because
    /// the working directory is a path on *that* filesystem and reading it here
    /// would report whatever happens to live at the same path locally.
    pub branch: Option<String>,
}

/// Tool names that mean "this file was modified", across harnesses.
///
/// Shared because two features now turn on it — the handoff brief's file list
/// and collision detection — and a name known to one but not the other would
/// show a session editing a file that cctop swore nobody was editing.
pub const EDIT_TOOLS: &[&str] = &[
    "Edit",
    "edit",
    "Write",
    "write",
    "MultiEdit",
    "NotebookEdit",
    "str_replace_editor",
    "ApplyPatch",
    "apply_patch",
];

/// How many written paths a row carries.
///
/// Enough to cover what a session has open at once, and far short of every file
/// it has ever touched: a path it wrote an hour and forty edits ago is one it
/// has almost certainly finished with, and treating it as contested would make
/// the warning cry wolf on any long session.
pub const MAX_RECENT_WRITES: usize = 32;

impl Session {
    pub fn new(provider: Provider, session_id: String) -> Self {
        Session {
            provider,
            surface: Surface::Cli,
            session_id,
            started_at: String::new(),
            last_active: String::new(),
            model: String::new(),
            harness: String::new(),
            label_source: String::new(),
            data_file: None,
            title: None,
            mac_meta: None,
            abbrev_label: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            tool_count: 0,
            tool_errors: 0,
            compactions: 0,
            total_cost: Some(0.0),
            cost_available: true,
            cost_is_free: false,
            cost_hour: 0.0,
            cost_today: 0.0,
            costs_by_day: HashMap::new(),
            costs_by_hour: HashMap::new(),
            subagents: Vec::new(),
            subagents_cost: 0.0,
            context: None,
            last_tool: String::new(),
            process: None,
            inferred_running: false,
            activity_state: ActivityState::Working,
            permission: None,
            recent_writes: Vec::new(),
            conflict: None,
            remote: None,
            tokens_per_min: 0.0,
            cost_per_min: 0.0,
        }
    }

    /// Stable identity used as a map key across refreshes.
    pub fn key(&self) -> String {
        format!("{}:{}", self.provider.as_str(), self.session_id)
    }

    /// Share of this session's tool calls that failed, or `None` when the
    /// harness does not record outcomes and a rate would be an invention.
    ///
    /// A session with no calls yet also reports `None`: nought out of nought is
    /// not a clean run, and drawing it as `0%` puts a reassuring figure on a
    /// session that has done nothing.
    pub fn error_rate(&self) -> Option<f64> {
        if !self.provider.records_tool_outcomes() || self.tool_count == 0 {
            return None;
        }
        Some(self.tool_errors as f64 / self.tool_count as f64)
    }

    pub fn is_running(&self) -> bool {
        self.process.is_some() || self.inferred_running
    }

    /// The command that reopens this session in a terminal, if its provider
    /// has one.
    ///
    /// `None` is a real answer, not a gap to be filled in later: Cursor, Gemini
    /// and Windsurf keep their conversations inside an editor or a UI of their
    /// own, and there is no CLI invocation that picks one back up. Callers show
    /// the transcript's path for those instead of guessing at a flag.
    pub fn resume_argv(&self) -> Option<Vec<String>> {
        let argv = match self.provider {
            Provider::Claude => vec!["claude", "--resume", &self.session_id],
            Provider::Codex => vec!["codex", "resume", &self.session_id],
            Provider::OpenCode => vec!["opencode", "--session", &self.session_id],
            Provider::Pi => vec!["pi", "--session", &self.session_id],
            Provider::Cursor | Provider::Gemini | Provider::Windsurf => return None,
        };
        Some(argv.into_iter().map(str::to_string).collect())
    }

    /// The working directory a resumed session should start in.
    ///
    /// The agent is being put back where it was, and half of what a transcript
    /// refers to is relative to that directory. A path that no longer exists —
    /// a deleted checkout, a session from another machine — yields `None`, so
    /// the agent starts wherever cctop was launched rather than failing to
    /// start at all.
    pub fn work_dir(&self) -> Option<PathBuf> {
        Some(PathBuf::from(&self.label_source)).filter(|dir| dir.is_dir())
    }

    /// A compaction is the newest thing in the transcript *and* something is
    /// still there to send the request that follows it.
    ///
    /// Liveness is checked here rather than baked into [`ContextUsage`] because
    /// the transcript of a session that compacted and then stopped never
    /// changes again: a flag stored at read time would keep calling that
    /// session busy for as long as it is listed.
    pub fn is_compacting(&self) -> bool {
        self.context.is_some_and(|c| c.compacted) && self.is_running()
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

/// What the newest events say about a session: what it is doing, and how much
/// it asks before it acts.
///
/// Both come off the same tail, so they are read together — a second pass over
/// the last 64KB of every transcript, on every refresh, to answer one more
/// question would be the kind of per-tick waste `proc` was just relieved of.
///
/// The permission mode is only carried by Claude Code, on each `user` record,
/// and the newest one wins: it is a setting the user can change mid-session.
pub fn live_state(session: &Session) -> (ActivityState, Option<crate::hook::Permission>) {
    let Some(file) = session.data_file.as_ref() else {
        return (ActivityState::Working, None);
    };
    if session.provider == crate::pricing::Provider::OpenCode {
        return (
            opencode::extract_activity_state(file, &session.session_id),
            None,
        );
    }
    let Some(text) = crate::util::read_tail(file, 65_536) else {
        return (ActivityState::Working, None);
    };

    // Found on the way to the state, which is why the walk is not cut short the
    // moment the state is known: the newest record that names a mode may be
    // older than the newest record that names a state.
    let mut permission = None;
    let mut state = None;
    for line in text.lines().rev() {
        let Ok(item) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        if permission.is_none() {
            permission = item
                .get("permissionMode")
                .and_then(|v| v.as_str())
                .and_then(crate::hook::Permission::parse);
        }
        if state.is_some() {
            if permission.is_some() {
                break;
            }
            continue;
        }
        state = activity_of(session.provider, &item);
    }
    (state.unwrap_or(ActivityState::Working), permission)
}

/// What one record says about the session's state, or `None` if it says nothing
/// and the walk should carry on past it.
fn activity_of(
    provider: crate::pricing::Provider,
    item: &serde_json::Value,
) -> Option<ActivityState> {
    if is_api_error_event(item) {
        return Some(ActivityState::ApiError);
    }
    if is_waiting_for_input_event(provider, item) {
        return Some(ActivityState::WaitingForInput);
    }
    if is_passive_event(item) {
        return None;
    }
    // The newest meaningful event was ordinary progress (a tool call, result,
    // or stream event), so do not let an older completed answer make the row
    // look like it is awaiting input.
    Some(ActivityState::Working)
}

/// Token counters and turn metadata are often appended after the event that
/// actually describes the agent's state.  Ignore them while walking backwards
/// so a finished response is still shown as waiting for the user.
fn is_passive_event(item: &serde_json::Value) -> bool {
    let kind = item
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if matches!(kind, "session_meta" | "turn_context") {
        return true;
    }
    kind == "event_msg"
        && item
            .get("payload")
            .and_then(|p| p.get("type"))
            .and_then(serde_json::Value::as_str)
            == Some("token_count")
}

fn is_api_error_event(item: &serde_json::Value) -> bool {
    let kind = item
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let subtype = item
        .get("subtype")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if matches!(kind, "error" | "api_error") || matches!(subtype, "api_error" | "error") {
        return true;
    }
    let payload = item.get("payload").unwrap_or(item);
    matches!(
        payload.get("type").and_then(serde_json::Value::as_str),
        Some("error" | "api_error" | "stream_error" | "turn_aborted")
    )
}

fn is_waiting_for_input_event(
    provider: crate::pricing::Provider,
    item: &serde_json::Value,
) -> bool {
    match provider {
        crate::pricing::Provider::Claude | crate::pricing::Provider::Cursor => {
            if item.get("type").and_then(serde_json::Value::as_str) != Some("assistant") {
                return false;
            }
            let Some(blocks) = item
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(serde_json::Value::as_array)
            else {
                return false;
            };
            blocks.iter().any(|b| {
                matches!(
                    b.get("type").and_then(serde_json::Value::as_str),
                    Some("tool_use" | "toolCall")
                ) && b
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(is_input_request_tool)
            })
        }
        crate::pricing::Provider::Codex => {
            let payload = item.get("payload").unwrap_or(item);
            matches!(
                item.get("type").and_then(serde_json::Value::as_str),
                Some("function_call" | "custom_tool_call" | "response_item")
            ) && payload
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(is_input_request_tool)
        }
        crate::pricing::Provider::Pi => {
            item.get("type").and_then(serde_json::Value::as_str) == Some("message")
                && item
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|blocks| {
                        blocks.iter().any(|b| {
                            b.get("type").and_then(serde_json::Value::as_str) == Some("toolCall")
                                && b.get("name")
                                    .and_then(serde_json::Value::as_str)
                                    .is_some_and(is_input_request_tool)
                        })
                    })
        }
        crate::pricing::Provider::Gemini => {
            item.get("type").and_then(serde_json::Value::as_str) == Some("gemini")
                && item
                    .get("toolCalls")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|calls| {
                        calls.iter().any(|call| {
                            call.get("name")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(is_input_request_tool)
                        })
                    })
        }
        // OpenCode keeps no transcript to tail, and Windsurf's conversation blob
        // is one SQLite value rewritten wholesale rather than a growing log, so
        // neither has a "newest event" this walk could read.
        crate::pricing::Provider::OpenCode | crate::pricing::Provider::Windsurf => false,
    }
}

fn is_input_request_tool(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "askuserquestion"
            | "ask_user_question"
            | "ask_user"
            | "askuser"
            | "question"
            | "request_user_input"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Each harness spells resuming differently, and the three that cannot be
    /// resumed from a shell must say so rather than producing a command that
    /// looks plausible and does nothing.
    #[test]
    fn each_provider_resumes_the_way_its_harness_does() {
        let argv = |provider| Session::new(provider, "sid".into()).resume_argv();
        assert_eq!(
            argv(Provider::Claude),
            Some(vec!["claude".into(), "--resume".into(), "sid".into()])
        );
        assert_eq!(
            argv(Provider::Codex),
            Some(vec!["codex".into(), "resume".into(), "sid".into()])
        );
        assert_eq!(
            argv(Provider::OpenCode),
            Some(vec!["opencode".into(), "--session".into(), "sid".into()])
        );
        assert_eq!(
            argv(Provider::Pi),
            Some(vec!["pi".into(), "--session".into(), "sid".into()])
        );
        for provider in [Provider::Cursor, Provider::Gemini, Provider::Windsurf] {
            assert_eq!(argv(provider), None, "{provider:?} has no resume command");
        }
    }

    /// A resumed agent belongs in the directory the session ran in — but a path
    /// that has since gone must not stop it from starting at all.
    #[test]
    fn a_resumed_session_only_claims_a_directory_that_exists() {
        let mut s = Session::new(Provider::Claude, "sid".into());
        s.label_source = "/nonexistent/gone".into();
        assert_eq!(s.work_dir(), None);

        s.label_source = std::env::temp_dir().to_string_lossy().into_owned();
        assert_eq!(s.work_dir(), Some(std::env::temp_dir()));
    }

    #[test]
    fn classifies_completed_assistant_responses_as_waiting() {
        let claude = json!({
            "type": "assistant",
            "message": {"content": [{"type": "tool_use", "name": "AskUserQuestion"}]}
        });
        assert!(is_waiting_for_input_event(Provider::Claude, &claude));

        let codex = json!({
            "type": "function_call",
            "payload": {"name": "request_user_input"}
        });
        assert!(is_waiting_for_input_event(Provider::Codex, &codex));
    }

    #[test]
    fn keeps_tool_turns_and_api_errors_distinct() {
        let tool_turn = json!({
            "type": "assistant",
            "message": {"content": [{"type": "tool_use", "name": "Read"}]}
        });
        assert!(!is_waiting_for_input_event(Provider::Claude, &tool_turn));

        let error = json!({"type": "system", "subtype": "api_error"});
        assert!(is_api_error_event(&error));
    }

    #[test]
    fn ignores_codex_token_bookkeeping_when_finding_last_state() {
        let item = json!({
            "type": "event_msg",
            "payload": {"type": "token_count"}
        });
        assert!(is_passive_event(&item));
    }

    /// The permission mode is on Claude Code's `user` records, so it can be read
    /// without hooks — and it is the *newest* one, because the user can change
    /// it mid-session.
    #[test]
    fn the_permission_mode_comes_off_the_transcript_tail() {
        let path = std::env::temp_dir().join(format!(
            "cctop-permission-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        std::fs::write(
            &path,
            concat!(
                r#"{"type":"user","permissionMode":"default"}"#,
                "\n",
                r#"{"type":"user","permissionMode":"bypassPermissions"}"#,
                "\n",
                // Newer, but says nothing about the mode: the answer above stands.
                r#"{"type":"assistant","message":{"content":[]}}"#,
                "\n",
            ),
        )
        .expect("write transcript");
        let mut session = Session::new(Provider::Claude, "test".into());
        session.data_file = Some(path.clone());
        assert_eq!(
            live_state(&session).1,
            Some(crate::hook::Permission::Bypass),
            "the newest record that names a mode wins"
        );
        let _ = std::fs::remove_file(&path);

        // A harness that never records it says nothing, rather than guessing at
        // the safe end.
        std::fs::write(
            &path,
            "{\"type\":\"assistant\",\"message\":{\"content\":[]}}\n",
        )
        .expect("write transcript");
        assert_eq!(live_state(&session).1, None);
        let _ = std::fs::remove_file(path);
    }

    /// The rate has three answers, and two of them are `None` for reasons that
    /// must not be confused: a harness that cannot report outcomes, and a
    /// session that has not made a call yet. Neither is a clean run, and
    /// drawing either as `0%` would put a reassuring figure on a row that has
    /// earned nothing of the sort.
    #[test]
    fn an_error_rate_is_only_reported_where_one_can_be_measured() {
        let mut s = Session::new(Provider::Claude, "x".into());
        assert_eq!(s.error_rate(), None, "no calls yet is not a clean run");

        s.tool_count = 40;
        s.tool_errors = 10;
        assert_eq!(s.error_rate(), Some(0.25));

        s.tool_errors = 0;
        assert_eq!(s.error_rate(), Some(0.0), "a clean run is a real answer");

        // Pi counts its calls but records no outcome, so the same figures mean
        // nothing there.
        let mut quiet = Session::new(Provider::Pi, "x".into());
        quiet.tool_count = 40;
        assert_eq!(quiet.error_rate(), None);
    }

    #[test]
    fn tail_state_uses_the_last_meaningful_codex_event() {
        let path = std::env::temp_dir().join(format!(
            "cctop-activity-state-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"function_call\",\"payload\":{\"name\":\"request_user_input\"}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\"}}\n"
            ),
        )
        .expect("write transcript");
        let mut session = Session::new(Provider::Codex, "test".into());
        session.data_file = Some(path.clone());
        assert_eq!(live_state(&session).0, ActivityState::WaitingForInput);
        let _ = std::fs::remove_file(path);
    }

    fn detail(ts: &str, full: &str) -> ToolDetail {
        ToolDetail {
            d: "x".into(),
            ts: ts.into(),
            full: Some(full.into()),
            ..Default::default()
        }
    }

    #[test]
    fn finalize_caps_details_per_session_keeping_the_newest() {
        let mut data = SessionData::default();
        // Two tools, each within the per-tool cap, together over the session cap.
        for tool in ["Read", "Bash"] {
            let list = data.metrics.tool_details.entry(tool.into()).or_default();
            for i in 0..crate::config::MAX_TOOL_DETAILS {
                list.push(detail(&format!("2026-01-01T00:{i:04}"), "arg"));
            }
        }
        data.metrics
            .tool_details
            .entry("Edit".into())
            .or_default()
            .push(detail("2027-01-01T00:00", "arg"));

        data.finalize();
        let total: usize = data.metrics.tool_details.values().map(Vec::len).sum();
        assert_eq!(total, crate::config::MAX_SESSION_TOOL_DETAILS);
        // The single newest call survives even though its tool is the smallest.
        assert_eq!(data.metrics.tool_details["Edit"].len(), 1);
        // …and what remains of a trimmed tool is its tail, not its head.
        let read = &data.metrics.tool_details["Read"];
        assert_eq!(
            read.last().expect("kept details").ts,
            format!("2026-01-01T00:{:04}", crate::config::MAX_TOOL_DETAILS - 1)
        );
    }

    #[test]
    fn finalize_bounds_the_large_string_fields() {
        let mut data = SessionData::default();
        let mut d = detail("2026-01-01T00:00", &"é".repeat(5_000));
        d.delta = Some(Delta {
            added: 1,
            removed: 0,
            hunks: vec!["+".repeat(5_000)],
        });
        data.metrics.tool_details.insert("Bash".into(), vec![d]);

        data.finalize();
        let d = &data.metrics.tool_details["Bash"][0];
        assert_eq!(
            d.full.as_ref().expect("full kept").chars().count(),
            crate::config::MAX_TOOL_DETAIL_CHARS + 1 // the ellipsis
        );
        assert_eq!(
            d.delta.as_ref().expect("delta kept").hunks[0]
                .chars()
                .count(),
            crate::config::MAX_DIFF_LINE_CHARS + 1
        );
    }

    #[test]
    fn finalize_leaves_a_small_session_untouched() {
        let mut data = SessionData::default();
        data.metrics
            .tool_details
            .insert("Bash".into(), vec![detail("2026-01-01T00:00", "ls")]);
        data.finalize();
        assert_eq!(
            data.metrics.tool_details["Bash"][0].full.as_deref(),
            Some("ls")
        );
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

/// LiteLLM rates for harnesses that record a cost of their own but may not
/// always know one.
///
/// A harness that supports arbitrary providers — a local proxy, a gateway, any
/// OpenAI-compatible endpoint — has no rates for them and writes `0` into the
/// same field it uses for a real charge. Taken at face value that reads as "this
/// was free", so the tokens are priced here instead. Rates are memoised because
/// a LiteLLM lookup scans the whole table while a session has one model and
/// hundreds of messages.
#[derive(Default)]
pub struct FallbackRates(HashMap<String, Option<crate::pricing::GenericPricing>>);

impl FallbackRates {
    /// What `tokens` cost at LiteLLM's rates for `model`, or `None` when LiteLLM
    /// lists no such model — which the caller has to keep distinct from free.
    pub fn costs(&mut self, model: &str, tokens: &Tokens) -> Option<Costs> {
        let p = (*self
            .0
            .entry(model.to_string())
            .or_insert_with(|| crate::pricing::resolve_generic(model)))?;
        let per_m = |count: u64, rate: f64| count as f64 * rate / 1e6;
        let costs = Costs {
            input: per_m(tokens.input, p.input),
            // Reasoning is reported alongside output, not on top of it, so it is
            // deliberately not billed again.
            output: per_m(tokens.output, p.output),
            cache_read: per_m(tokens.cache_read, p.cache_read),
            cache_write_5m: per_m(tokens.cache_write_5m, p.cache_write),
            cache_write_1h: per_m(tokens.cache_write_1h, p.cache_write),
            cached_input: per_m(tokens.cached_input, p.cache_read),
            total: 0.0,
        };
        Some(Costs {
            total: costs.input
                + costs.output
                + costs.cache_read
                + costs.cache_write_5m
                + costs.cache_write_1h
                + costs.cached_input,
            ..costs
        })
    }
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
    /// The call reported an error. Providers that do not record a per-call
    /// outcome leave this false rather than guessing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
    /// Subagent that issued the call, or `None` for the main session. Tool
    /// activity from subagents is interleaved into the same log, so without this
    /// there is no way to tell who did what.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub tool_count: u64,
    /// Calls the transcript reported as failed, over the same population as
    /// `tool_count` — main session and subagents together, so the two divide.
    ///
    /// Always zero for a provider whose transcript records no per-call outcome.
    /// [`crate::pricing::Provider::records_tool_outcomes`] is what tells those
    /// apart from a session that simply had no failures; without it a harness
    /// that cannot say would read as a harness with nothing to report.
    #[serde(default)]
    pub tool_errors: u64,
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
    /// Provider-reported reasoning effort, when the transcript exposes it.
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    pub models: Vec<String>,
    pub model_breakdown: Vec<ModelBreakdown>,
    pub tokens: Tokens,
    pub costs: Costs,
    /// `YYYY-MM-DD` -> model -> USD.
    pub costs_by_day: HashMap<String, HashMap<String, f64>>,
    /// `YYYY-MM-DDTHH` -> model -> USD.
    pub costs_by_hour: HashMap<String, HashMap<String, f64>>,
    pub metrics: Metrics,
    /// Claude only: what the live context window is filled with. Absent for
    /// providers whose transcripts don't report per-request usage.
    #[serde(default)]
    pub context_breakdown: Option<ContextBreakdown>,
    /// The window measured at each request, oldest first.
    ///
    /// The breakdown says what the window holds *now*; this says how it got
    /// there, which is the part that explains a session's cost. A window that
    /// climbed steadily is a conversation that grew; one that jumped is a single
    /// tool result that will do it again.
    #[serde(default)]
    pub context_series: Vec<CtxPoint>,
    /// Compactions the session has been through.
    ///
    /// Counted as they are seen rather than read back off `context_series`,
    /// which is decimated past [`MAX_CTX_POINTS`] and can lose a marker, and
    /// which records nothing at all for a compaction no request followed.
    ///
    /// Claude Code only, for the same reason as the breakdown above: it is the
    /// only transcript that says a compaction happened.
    #[serde(default)]
    pub compactions: u32,
    pub subagents: Vec<Subagent>,
    /// Codex reports per-million rates directly; surfaced in the Cost panel.
    pub rates: Option<CodexRates>,
    /// Set when extraction failed; the row still renders with zeroed figures.
    pub error: Option<String>,
}

/// Keep the newest `MAX_SESSION_TOOL_DETAILS` details across all tools, and
/// bound the string fields each one carries.
///
/// The per-tool cap alone lets a busy session hold thousands of details; the
/// panel only ever shows a recent slice of them. Details are ranked by
/// timestamp so "the newest" means newest in the session, not newest per tool —
/// a tool used once at the very end should survive while an early flood of
/// reads does not.
fn trim_tool_details(details: &mut HashMap<String, Vec<ToolDetail>>) {
    for list in details.values_mut() {
        for d in list.iter_mut() {
            truncate_chars(&mut d.d, crate::config::MAX_TOOL_DETAIL_CHARS);
            if let Some(full) = d.full.as_mut() {
                truncate_chars(full, crate::config::MAX_TOOL_DETAIL_CHARS);
            }
            if let Some(delta) = d.delta.as_mut() {
                for line in delta.hunks.iter_mut() {
                    truncate_chars(line, crate::config::MAX_DIFF_LINE_CHARS);
                }
            }
        }
    }

    let total: usize = details.values().map(Vec::len).sum();
    if total <= crate::config::MAX_SESSION_TOOL_DETAILS {
        return;
    }

    // Rank newest-first. Within one tool the vectors are already chronological,
    // so the index breaks ties for details sharing (or missing) a timestamp.
    let mut ranked: Vec<(&str, usize)> = details
        .iter()
        .flat_map(|(name, list)| (0..list.len()).map(move |i| (name.as_str(), i)))
        .collect();
    ranked.sort_by(|a, b| {
        details[b.0][b.1]
            .ts
            .cmp(&details[a.0][a.1].ts)
            .then(b.1.cmp(&a.1))
    });
    ranked.truncate(crate::config::MAX_SESSION_TOOL_DETAILS);

    let mut keep: HashMap<&str, Vec<bool>> = details
        .iter()
        .map(|(name, list)| (name.as_str(), vec![false; list.len()]))
        .collect();
    for (name, i) in ranked {
        keep.get_mut(name).expect("name came from details")[i] = true;
    }
    let keep: HashMap<String, Vec<bool>> =
        keep.into_iter().map(|(k, v)| (k.to_string(), v)).collect();

    for (name, list) in details.iter_mut() {
        let flags = &keep[name];
        let mut i = 0;
        list.retain(|_| {
            i += 1;
            flags[i - 1]
        });
    }
    // A tool whose every detail was dropped would otherwise leave an empty
    // entry that the panel renders as a tool with no calls.
    details.retain(|_, list| !list.is_empty());
}

/// Truncate to at most `max` characters, never mid-character.
fn truncate_chars(s: &mut String, max: usize) {
    if s.chars().count() <= max {
        return;
    }
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    s.truncate(end);
    s.push('…');
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

    /// Bring freshly extracted data down to what is worth keeping.
    ///
    /// This runs on every extraction, before the result is either displayed or
    /// cached, so a cached session and a re-parsed one show exactly the same
    /// thing. Trimming only on the way to disk would be cheaper and wrong: the
    /// Tools panel would quietly change contents the first time a session was
    /// served from cache.
    pub fn finalize(&mut self) {
        trim_tool_details(&mut self.metrics.tool_details);
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
    let ((mut codex, claude), ((opencode, pi), (cursor, (gemini, windsurf)))) = rayon::join(
        || rayon::join(codex::list_sessions, claude::list_sessions),
        || {
            rayon::join(
                || rayon::join(opencode::list_sessions, pi::list_sessions),
                || {
                    rayon::join(cursor::list_sessions, || {
                        rayon::join(gemini::list_sessions, windsurf::list_sessions)
                    })
                },
            )
        },
    );
    codex.extend(claude);
    codex.extend(opencode);
    codex.extend(pi);
    codex.extend(cursor);
    codex.extend(gemini);
    codex.extend(windsurf);
    let mut sessions = codex;
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
        Provider::Codex | Provider::Cursor | Provider::Gemini | Provider::Pi => {
            crate::config::file_mtime_ms(f)
        }
        // Every OpenCode session shares one WAL-backed database, as every
        // Windsurf conversation in a workspace shares one `state.vscdb`. Using
        // the file mtime would invalidate hundreds of unchanged sessions
        // whenever one message lands; the per-session timestamp is the right
        // key. Windsurf has no such timestamp of its own, so its rows fall back
        // to the file and re-extract together — cheap, since the blob is small.
        Provider::OpenCode | Provider::Windsurf => util::parse_ts(&session.last_active)
            .map(|d| d.timestamp_millis().max(0) as u64)
            .unwrap_or_else(|| crate::config::file_mtime_ms(f)),
    }
}
