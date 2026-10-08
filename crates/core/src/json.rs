//! The `--json` document: every session, as one array of plain records.
//!
//! The wire format for three readers — `cctop --json`, `--host`, which parses
//! it back off an ssh pipe, and the web dashboard, which streams it — so it is
//! built in one place, below all three of them.

use crate::pricing::{Plan, Provider};
use crate::session::Session;
use crate::util;
use serde::Serialize;

#[derive(Serialize)]
pub struct JsonAccount {
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    organization: Option<String>,
}

#[derive(Serialize)]
pub struct JsonCost {
    available: bool,
    /// `null` when the plan bundles this provider's usage.
    total: Option<String>,
    included: bool,
    /// Recorded usage that priced at nothing, as with a free model. Distinct
    /// from `available: false`, which is a provider that records no usage.
    free: bool,
    /// Spend in the current local clock hour, which is what this key has
    /// always meant; kept so older readers and peers go on getting it.
    this_hour: f64,
    /// Spend in the last 60 minutes, rolling — the `$/1H` column's figure.
    ///
    /// A new key rather than a new meaning for `this_hour`: a peer reading an
    /// older cctop finds this missing and falls back, where a changed meaning
    /// would be read wrong without anything saying so.
    last_hour: f64,
    today: f64,
    /// Smoothed live spend rate, USD per minute.
    per_min: f64,
    /// `YYYY-MM-DD` -> USD, trimmed to [`JSON_DAYS`] days.
    ///
    /// Present so a reader can compute the same spend windows the overview
    /// shows rather than only a lifetime total. Trimmed because a session
    /// running for months would otherwise carry a bucket per day of it, and no
    /// window here looks back further.
    by_day: std::collections::BTreeMap<String, f64>,
    /// `YYYY-MM-DDTHH` -> USD, trimmed to [`JSON_HOURS`] hours.
    by_hour: std::collections::BTreeMap<String, f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    breakdown: Option<crate::session::Costs>,
}

/// How far back the per-day and per-hour buckets in `--json` reach.
///
/// One more than the longest window anything downstream computes — the
/// overview's calendar month and its 30-day rolling total for days, today's
/// 24 hourly buckets for hours.
const JSON_DAYS: usize = 31;
const JSON_HOURS: usize = 48;

/// Flatten a `key -> model -> USD` bucket map, keeping the newest `keep` keys.
///
/// Keys sort lexicographically in time order in both spellings cctop uses
/// (`YYYY-MM-DD` and `YYYY-MM-DDTHH`), so "newest" is the tail of a sort.
fn trimmed_buckets(
    buckets: &std::collections::HashMap<String, std::collections::HashMap<String, f64>>,
    keep: usize,
) -> std::collections::BTreeMap<String, f64> {
    let mut keys: Vec<&String> = buckets.keys().collect();
    keys.sort();
    keys.iter()
        .rev()
        .take(keep)
        .map(|k| ((*k).clone(), buckets[*k].values().sum()))
        .collect()
}

#[derive(Serialize)]
pub struct JsonTokens {
    input: u64,
    output: u64,
    total: u64,
    detail: crate::session::Tokens,
}

#[derive(Serialize)]
pub struct JsonActivity {
    tool_count: u64,
    /// Calls the transcript reported as failed. Absent — rather than zero —
    /// where the harness records no per-call outcome, since the two mean very
    /// different things to anything totalling them up.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_errors: Option<u64>,
    /// Compactions the session has been through. Claude Code only.
    #[serde(skip_serializing_if = "Option::is_none")]
    compactions: Option<u32>,
    tools: std::collections::HashMap<String, u64>,
    skill_count: u64,
    skills: std::collections::HashMap<String, u64>,
    web_fetch_count: u64,
    web_fetches: Vec<String>,
    web_search_count: u64,
    web_searches: Vec<String>,
    mcp_tool_count: u64,
    mcp_tools: Vec<String>,
    lines_added: u64,
    lines_removed: u64,
}

#[derive(Serialize)]
pub struct JsonSession {
    provider: &'static str,
    surface: &'static str,
    /// What the status dot says: `working`, `waiting` for the user, or `error`
    /// on an API failure. Independent of `running`, which is about a process.
    state: &'static str,
    /// What an `asking` session wants to do, when its hook or its screen said.
    #[serde(skip_serializing_if = "Option::is_none")]
    asking_for: Option<String>,
    /// Present, and true, when what an `asking` session holds is a question
    /// with choices rather than a permission prompt: Allow and Deny do not
    /// apply to it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    asking_question: bool,
    /// Present when cctop allows every permission prompt this session raises:
    /// since when, and what it has allowed. See [`crate::yolo`].
    #[serde(skip_serializing_if = "Option::is_none")]
    yolo: Option<crate::yolo::Entry>,
    session_id: String,
    started_at: String,
    last_active: String,
    project: Option<String>,
    title: Option<String>,
    /// Login name of the user the session belongs to, when cctop is reading
    /// every user's homes and this one is not the reader's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    /// Which Claude profile — which `$CLAUDE_CONFIG_DIR` — the session was read
    /// out of. Absent for every other harness, none of which has the concept.
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
    /// Where the session is working when `cctop sandbox` launched it: the
    /// agent runs here, its commands and `path` are on `host`.
    #[serde(skip_serializing_if = "Option::is_none")]
    sandbox: Option<JsonSandbox>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<JsonAccount>,
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    harness: Option<String>,
    /// Branch checked out in the working directory, read on the machine the
    /// session is on — which is why it is carried rather than looked up by
    /// whoever reads this.
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    /// How much the session asks before it acts, when its own hooks said.
    /// Absent for a session with no cctop hooks installed — nothing in a
    /// transcript records this, so it cannot be inferred.
    #[serde(skip_serializing_if = "Option::is_none")]
    permission: Option<&'static str>,
    models: Vec<String>,
    plan: &'static str,
    running: bool,
    /// CPU and resident memory across the session's process tree. Absent where
    /// no per-session process exists to measure.
    #[serde(skip_serializing_if = "Option::is_none")]
    process: Option<JsonProcess>,
    cost: JsonCost,
    tokens: JsonTokens,
    /// Token rate per minute, smoothed the same way the `TOK/m` column is.
    tokens_per_min: f64,
    activity: JsonActivity,
    #[serde(skip_serializing_if = "Option::is_none")]
    rates: Option<crate::session::CodexRates>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subagents: Vec<crate::session::Subagent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<crate::session::ContextUsage>,
    /// Other running agents on this session's ground. Absent when there are
    /// none, which is the ordinary case.
    #[serde(skip_serializing_if = "Option::is_none")]
    conflict: Option<JsonConflict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
pub struct JsonSandbox {
    host: String,
    path: String,
}

#[derive(Serialize)]
pub struct JsonProcess {
    cpu: f32,
    memory: u64,
    command: String,
    /// Processes in the tree, which is what the `pids` figure in the UI counts.
    pids: usize,
}

#[derive(Serialize)]
pub struct JsonConflict {
    /// `file` when a peer has written a file this session also wrote,
    /// `directory` when they merely share a repository.
    level: &'static str,
    /// Session ids of the peers, not keys: an id is what every other field
    /// here is addressed by.
    peers: Vec<String>,
    files: Vec<String>,
}

/// Resolve a collision's peer keys back to the session ids the rest of the
/// document is addressed by. A key a caller cannot look up is worse than no
/// entry, so an unresolvable one is dropped rather than printed raw.
fn json_conflict(sessions: &[Session], c: &crate::collide::Collision) -> JsonConflict {
    JsonConflict {
        level: match c.level {
            crate::collide::Overlap::File => "file",
            crate::collide::Overlap::Directory => "directory",
        },
        peers: c
            .peers
            .iter()
            .filter_map(|key| sessions.iter().find(|s| &s.key() == key))
            .map(|s| s.session_id.clone())
            .collect(),
        files: c.files.clone(),
    }
}

/// Build the `--json` document for a set of sessions.
///
/// Out of `cli`'s `run_json` because the same document is the wire format for
/// two other readers now: `--host`, which parses it back off an ssh pipe, and
/// the web dashboard, which streams it to a browser. Anything that is true of
/// the printed JSON has to stay true of theirs, so there is one builder rather
/// than three that drift.
pub fn sessions(sessions: &[Session], plan: Plan, store: &crate::cache::Store) -> Vec<JsonSession> {
    let claude_account = crate::quota::claude_account();
    let codex_account = crate::quota::codex_account();
    let collisions = crate::collide::detect(sessions);
    let now = chrono::Utc::now();

    sessions
        .iter()
        .map(|s| {
            let data = store.session_data(s);
            let m = &data.metrics;
            let included = s.cost_available && plan.includes(s.provider);
            // The credentials read here are this user's. Another user's
            // session is signed in as whoever they are, and stamping the
            // reader's own email on their row would be a wrong answer where
            // no answer is the true one.
            let account = match s.provider {
                Provider::Claude if s.owner.is_none() => claude_account.as_ref(),
                Provider::Codex if s.owner.is_none() => codex_account.as_ref(),
                Provider::Claude | Provider::Codex => None,
                Provider::Cursor
                | Provider::Devin
                | Provider::Gemini
                | Provider::OpenCode
                | Provider::Pi
                | Provider::Windsurf => None,
            }
            .map(|a| JsonAccount {
                email: a.email.clone(),
                organization: a.organization.clone(),
            });

            JsonSession {
                provider: s.provider.as_str(),
                state: match s.activity_state {
                    crate::session::ActivityState::Working => "working",
                    crate::session::ActivityState::WaitingForInput => "waiting",
                    crate::session::ActivityState::Asking => "asking",
                    crate::session::ActivityState::ApiError => "error",
                },
                asking_for: s.asking_for.clone(),
                asking_question: s.asking_question,
                yolo: s.yolo.as_deref().cloned(),
                surface: match s.surface {
                    crate::session::Surface::Cli => "cli",
                    crate::session::Surface::Editor => "editor",
                    crate::session::Surface::DesktopCode => "desktop-code",
                    crate::session::Surface::DesktopCowork => "desktop-cowork",
                },
                session_id: s.session_id.clone(),
                started_at: s.started_at.clone(),
                last_active: s.last_active.clone(),
                project: (!s.label_source.is_empty()).then(|| s.label_source.clone()),
                title: s.title.clone(),
                user: s.owner.clone(),
                profile: s.profile.clone(),
                sandbox: s.sandbox.as_deref().map(|sandbox| JsonSandbox {
                    host: crate::sandbox::host_of(sandbox).to_string(),
                    path: sandbox
                        .get(crate::sandbox::host_of(sandbox).len() + 1..)
                        .unwrap_or_default()
                        .to_string(),
                }),
                account,
                model: (!s.model.is_empty()).then(|| s.model.clone()),
                harness: (!s.harness.is_empty()).then(|| s.harness.clone()),
                branch: crate::branch::branch_of(s),
                permission: s.permission.map(crate::hook::Permission::label),
                models: data.models.clone(),
                plan: plan.as_str(),
                running: s.is_running(),
                process: s.process.as_ref().map(|p| JsonProcess {
                    cpu: p.cpu,
                    memory: p.memory,
                    command: p.command.clone(),
                    pids: p.pids,
                }),
                cost: JsonCost {
                    available: s.cost_available,
                    total: (s.cost_available && !included).then(|| util::money(data.costs.total)),
                    included,
                    free: s.cost_is_free,
                    this_hour: s.cost_clock_hour(&now),
                    last_hour: s.cost_hour,
                    today: s.cost_today,
                    per_min: s.cost_per_min,
                    by_day: trimmed_buckets(&s.costs_by_day, JSON_DAYS),
                    by_hour: trimmed_buckets(&s.costs_by_hour, JSON_HOURS),
                    breakdown: (s.cost_available && !included).then(|| data.costs.clone()),
                },
                tokens: JsonTokens {
                    input: s.input_tokens,
                    output: s.output_tokens,
                    total: s.input_tokens + s.output_tokens,
                    detail: data.tokens.clone(),
                },
                tokens_per_min: s.tokens_per_min,
                activity: JsonActivity {
                    tool_count: m.tool_count,
                    tool_errors: s.provider.records_tool_outcomes().then_some(m.tool_errors),
                    compactions: (s.provider == Provider::Claude).then_some(data.compactions),
                    tools: m.tools.clone(),
                    skill_count: m.skill_count,
                    skills: m.skills.clone(),
                    web_fetch_count: m.web_fetch_count,
                    web_fetches: m.web_fetches.clone(),
                    web_search_count: m.web_search_count,
                    web_searches: m.web_searches.clone(),
                    mcp_tool_count: m.mcp_tool_count,
                    mcp_tools: m.mcp_tools.clone(),
                    lines_added: m.lines_added,
                    lines_removed: m.lines_removed,
                },
                rates: data.rates,
                subagents: data.subagents.clone(),
                context: s.context,
                conflict: collisions.get(&s.key()).map(|c| json_conflict(sessions, c)),
                error: data.error.clone(),
            }
        })
        .collect()
}
