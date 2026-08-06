//! Command-line parsing and the non-interactive output modes.

use crate::loader::Loader;
use crate::pricing::{Plan, Provider};
use crate::session::Session;
use crate::util;
use clap::Parser;
use serde::Serialize;

#[derive(Parser, Debug)]
#[command(
    name = "cctop",
    about = "An htop-like monitor for AI coding agent sessions",
    // cctop takes no positionals — `run` and `attach` are intercepted in main
    // before clap sees them, so clap cannot know they exist and would otherwise
    // print a usage line claiming options are all there is.
    override_usage = "cctop [OPTIONS]\n       \
                      cctop <agent> [args…]\n       \
                      cctop attach [pid]",
    // Shown by `-h` as well as `--help`: the long description is the only place
    // that mentioned launching agents, and nobody reads `--help` to find out a
    // command exists.
    after_help = "Running agents:\n  \
cctop <agent> [args…]  Start claude, codex, opencode or pi on a pty cctop owns,\n                         \
so the UI can watch it and type into it. Same as\n                         \
`cctop run <agent>`; everything after the name goes to it.\n  \
cctop attach [pid]     Put a running agent on this terminal. With no pid, lists\n                         \
them. F12 detaches and leaves it running.\n\n\
Use --help for the full description.",
    // Otherwise clap repeats the block above under the long description, which
    // covers the same ground at length.
    after_long_help = "",
    long_about = "cctop — an htop-like monitor for AI coding agent sessions\n\n\
Tracks Claude Code, Codex, Cursor, OpenCode, and Pi sessions on your machine, showing\n\
real-time cost estimation, token usage, tool invocations, and OS-level metrics.\n\n\
COST ESTIMATION\n  \
Cost figures are estimates based on per-token API pricing from the LiteLLM\n  \
database (cached locally for 24 hours). Many subscription plans — such as\n  \
Claude Max, Pro, or Team — charge a flat rate or bundle tokens differently,\n  \
so reported costs may not reflect your actual bill. Treat the $ column as a\n  \
rough indicator of resource consumption, not as an authoritative invoice.\n\n\
LAUNCHING AGENTS\n  \
`cctop <command> [args…]`, or `cctop run <command>`, starts the agent on a pty\n  \
cctop owns so the UI can type into it with `s`. Everything after the command,\n  \
flags included, goes to the agent. The first interactive run aliases the known\n  \
agents to this form in your shell startup files; --remove-alias undoes that.\n\n\
ATTACHING\n  \
Agents started that way can be watched and driven from anywhere. Press `a` in\n  \
the UI, or run `cctop attach [pid]` to put one on the terminal directly —\n  \
with no pid it lists what is running. F12 detaches and leaves it running. The\n  \
agent is resized to the smallest window watching it, and gets its size back\n  \
when that one detaches.\n\n\
NOTES\n  \
Session data is read from each agent's standard local session store.\n  \
UI preferences (active tab, sort order, filters) persist across runs.",
    version
)]
pub struct Args {
    /// List sessions in a table and exit
    #[arg(short, long)]
    pub list: bool,

    /// Dump full session data as JSON and exit
    #[arg(short, long)]
    pub json: bool,

    /// Billing plan for cost display: retail, max, or included
    #[arg(short, long, default_value = "retail", value_parser = parse_plan)]
    pub plan: Plan,

    /// Refresh interval in seconds
    #[arg(short, long, default_value_t = 2.0, value_parser = parse_delay)]
    pub delay: f64,

    /// Clear persisted session extraction data before starting (keeps preferences and pricing)
    #[arg(long)]
    pub clear_cache: bool,

    /// Replace this binary with the newest GitHub release and exit
    #[arg(long)]
    pub update: bool,

    /// Write the agent aliases into your shell startup files and exit
    #[arg(long)]
    pub install_alias: bool,

    /// Remove the agent aliases from your shell startup files and exit
    #[arg(long)]
    pub remove_alias: bool,
}

fn parse_plan(s: &str) -> Result<Plan, String> {
    Plan::parse(s)
        .ok_or_else(|| format!("unsupported plan '{s}'; use one of: included, max, retail"))
}

fn parse_delay(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|_| "must be a number".to_string())?;
    // `Duration::from_secs_f64` panics for values that do not fit its seconds
    // field, so reject them here along with non-finite values.
    if !v.is_finite() || !(1.0..(u64::MAX as f64)).contains(&v) {
        return Err("must be a finite number from 1 up to the maximum duration (seconds)".into());
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// --list
// ---------------------------------------------------------------------------

/// Column widths shared by the header and the data rows so they stay aligned.
const W_IDX: usize = 3;
const W_AGE: usize = 5;
const W_TOK: usize = 7;
const W_COST: usize = 9;

/// Fixed width consumed before the flexible session/model columns.
const fn fixed_width() -> usize {
    (W_IDX + 2) + W_AGE + 2 + W_AGE + 2 + W_TOK + 2 + W_TOK + 2 + W_COST + 2
}

/// Split the remaining width between the session label and the model name.
fn flex_widths(width: usize) -> (usize, usize) {
    let remaining = width.saturating_sub(fixed_width()).max(16);
    let model_w = (remaining / 3).clamp(8, 22);
    let label_w = remaining.saturating_sub(model_w + 2).max(8);
    (label_w, model_w)
}

fn format_row(index: usize, s: &Session, label: &str, width: usize) -> String {
    let now = chrono::Utc::now();
    let (label_w, model_w) = flex_widths(width);
    let cost = match s.total_cost {
        _ if !s.cost_available => "—".into(),
        _ if s.cost_is_free => "FREE".into(),
        Some(c) => util::compact_usd(c),
        None => "incl".into(),
    };

    format!(
        // Index is right-aligned so columns don't shift once it reaches 10.
        "{index:>W_IDX$}. {:>W_AGE$}  {:>W_AGE$}  {:>W_TOK$}  {:>W_TOK$}  {:>W_COST$}  {:<label_w$}  {}",
        util::relative_age(&s.started_at, &now),
        util::relative_age(&s.last_active, &now),
        util::compact_tokens(s.input_tokens),
        util::compact_tokens(s.output_tokens),
        cost,
        util::truncate(label, label_w),
        util::truncate(&s.model, model_w),
    )
    .trim_end()
    .to_string()
}

fn print_group(
    name: &str,
    sessions: &[&Session],
    start_index: usize,
    cost_label: &str,
    width: usize,
) {
    println!("{name}:");
    if sessions.is_empty() {
        println!("  (none)");
        return;
    }
    let (label_w, _) = flex_widths(width);
    println!(
        "{:W_IDX$}  {:>W_AGE$}  {:>W_AGE$}  {:>W_TOK$}  {:>W_TOK$}  {:>W_COST$}  {:<label_w$}  model",
        " ", "start", "last", "in", "out", cost_label, "session"
    );
    let labels = util::abbreviate_paths(
        &sessions
            .iter()
            .map(|s| s.label_source.clone())
            .collect::<Vec<_>>(),
    );
    for (i, (s, label)) in sessions.iter().zip(labels).enumerate() {
        let display = s.title.clone().unwrap_or(label);
        println!("{}", format_row(start_index + i + 1, s, &display, width));
    }
}

pub fn run_list(sessions: &[Session], plan: Plan) {
    let width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(100)
        .max(60);
    let cost_label = if plan == Plan::Retail { "est" } else { "cost" };

    let mut offset = 0;
    for (name, provider) in [
        ("Codex", Provider::Codex),
        ("Claude", Provider::Claude),
        ("Cursor", Provider::Cursor),
        ("OpenCode", Provider::OpenCode),
        ("Pi", Provider::Pi),
    ] {
        let group: Vec<&Session> = sessions.iter().filter(|s| s.provider == provider).collect();
        if group.is_empty() {
            continue;
        }
        if offset > 0 {
            println!();
        }
        print_group(name, &group, offset, cost_label, width);
        offset += group.len();
    }
}

// ---------------------------------------------------------------------------
// --json
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct JsonAccount {
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    organization: Option<String>,
}

#[derive(Serialize)]
struct JsonCost {
    available: bool,
    /// `null` when the plan bundles this provider's usage.
    total: Option<String>,
    included: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    breakdown: Option<crate::session::Costs>,
}

#[derive(Serialize)]
struct JsonTokens {
    input: u64,
    output: u64,
    total: u64,
    detail: crate::session::Tokens,
}

#[derive(Serialize)]
struct JsonActivity {
    tool_count: u64,
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
struct JsonSession {
    provider: &'static str,
    surface: &'static str,
    session_id: String,
    started_at: String,
    last_active: String,
    project: Option<String>,
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<JsonAccount>,
    model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    harness: Option<String>,
    models: Vec<String>,
    plan: &'static str,
    running: bool,
    cost: JsonCost,
    tokens: JsonTokens,
    activity: JsonActivity,
    #[serde(skip_serializing_if = "Option::is_none")]
    rates: Option<crate::session::CodexRates>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subagents: Vec<crate::session::Subagent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<crate::session::ContextUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub fn run_json(sessions: &[Session], plan: Plan, loader: &Loader) -> anyhow::Result<()> {
    let claude_account = crate::quota::claude_account();
    let codex_account = crate::quota::codex_account();

    let out: Vec<JsonSession> = sessions
        .iter()
        .map(|s| {
            let data = loader.store().session_data(s);
            let m = &data.metrics;
            let included = s.cost_available && plan.includes(s.provider);
            let account = match s.provider {
                Provider::Claude => claude_account.as_ref(),
                Provider::Codex => codex_account.as_ref(),
                Provider::Cursor | Provider::OpenCode | Provider::Pi => None,
            }
            .map(|a| JsonAccount {
                email: a.email.clone(),
                organization: a.organization.clone(),
            });

            JsonSession {
                provider: s.provider.as_str(),
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
                account,
                model: (!s.model.is_empty()).then(|| s.model.clone()),
                harness: (!s.harness.is_empty()).then(|| s.harness.clone()),
                models: data.models.clone(),
                plan: plan.as_str(),
                running: s.is_running(),
                cost: JsonCost {
                    available: s.cost_available,
                    total: (s.cost_available && !included).then(|| util::money(data.costs.total)),
                    included,
                    breakdown: (s.cost_available && !included).then(|| data.costs.clone()),
                },
                tokens: JsonTokens {
                    input: s.input_tokens,
                    output: s.output_tokens,
                    total: s.input_tokens + s.output_tokens,
                    detail: data.tokens.clone(),
                },
                activity: JsonActivity {
                    tool_count: m.tool_count,
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
                error: data.error.clone(),
            }
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Args::command().debug_assert();
    }

    #[test]
    fn delay_floor_enforced() {
        assert!(parse_delay("0.5").is_err());
        assert!(parse_delay("abc").is_err());
        assert!(parse_delay("NaN").is_err());
        assert!(parse_delay("inf").is_err());
        assert!(parse_delay("-inf").is_err());
        assert!(parse_delay(&f64::MAX.to_string()).is_err());
        assert_eq!(parse_delay("2.5").unwrap(), 2.5);
    }

    #[test]
    fn plan_parsing_rejects_unknown() {
        assert!(parse_plan("max").is_ok());
        assert!(parse_plan("nonsense").is_err());
    }

    #[test]
    fn clear_cache_flag_is_accepted() {
        let args = Args::try_parse_from(["cctop", "--clear-cache"]).expect("valid args");
        assert!(args.clear_cache);
    }
}
