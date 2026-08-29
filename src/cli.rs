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
                      cctop attach [pid]\n       \
                      cctop serve [--bind ADDR] [--port PORT]\n       \
                      cctop doctor",
    // Shown by `-h` as well as `--help`: the long description is the only place
    // that mentioned launching agents, and nobody reads `--help` to find out a
    // command exists.
    after_help = "Running agents:\n  \
cctop <agent> [args…]  Start claude, codex, opencode or pi on a pty cctop owns,\n                         \
so the UI can watch it and type into it. Same as\n                         \
`cctop run <agent>`; everything after the name goes to it.\n  \
cctop attach [pid]     Put a running agent on this terminal. With no pid, lists\n                         \
them. F12 detaches and leaves it running.\n  \
cctop serve            Serve the table, and a per-session report, to a browser.\n                         \
Loopback and read-only by default; `serve --help` for the\n                         \
flags. Handy on a phone for the sessions waiting on you.\n  \
cctop doctor           Check this installation and say what is wrong with it:\n                         \
where sessions are read from, pricing, hooks, and what\n                         \
`s` can reach. --host also tests an ssh target.\n  \
cctop --trace          Time each stage of a run and write the totals to a file\n                         \
on exit, to attach to a bug report about slowness.\n\n\
Use --help for the full description.",
    // Otherwise clap repeats the block above under the long description, which
    // covers the same ground at length.
    after_long_help = "",
    long_about = "cctop — an htop-like monitor for AI coding agent sessions\n\n\
Tracks Claude Code, Codex, Cursor, Gemini CLI, OpenCode, Pi, and Windsurf\n\
sessions on your machine, showing real-time cost estimation, token usage, tool\n\
invocations, and OS-level metrics.\n\n\
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
RESUMING\n  \
`R` in the UI reopens any session in a tab of its own, by running its own\n  \
harness's resume command in the directory it was working in. Unlike `a` this\n  \
needs nothing of cctop at the time the session ran, so it reaches the sessions\n  \
started from anywhere — including ones that ended long ago.\n\n\
IN A BROWSER\n  \
`cctop serve` puts the same table on an HTTP port, streaming it over SSE, plus\n  \
a per-session report — repeated tool failures, where the context window went,\n  \
what each model cost. It listens on 127.0.0.1 with a per-run access token in\n  \
the URL; `--bind` is what puts it on the network, and says so when it does.\n  \
The page is read-only: it starts nothing and types at nothing.\n\n\
SEARCHING\n  \
`/` filters on what the table shows plus the full working directory and the\n  \
branch; `Tab` in that prompt extends the search into the transcripts, which\n  \
reads them off disk and so is opt-in.\n\n\
DIAGNOSING\n  \
`cctop doctor` reports where sessions are read from and how many it found,\n  \
whether pricing loaded, which agent hooks are installed, and what `s` can\n  \
reach. It exits non-zero only for a real fault, so it is usable in a script.\n  \
`cctop doctor --host <host>` additionally makes the ssh round trip.\n  \
--trace answers the other question, which is why a run is slow. It times each\n  \
stage — discovery, transcript parsing, the cache, the pricing fetch — and\n  \
writes the totals to a file when cctop exits, for attaching to a bug report.\n  \
It carries counts and durations only: no session titles, project paths or\n  \
file names, and cctop's own paths are spelled with `~`.\n\n\
EVERY USER\n  \
Run as root and cctop reads every user's sessions rather than root's own,\n  \
naming whose each row is in the USER column. CCTOP_ALL_USERS=0 turns that\n  \
off, =1 turns it on without root, and CCTOP_HOMES names homes that are\n  \
neither in /etc/passwd nor under /home.\n\n\
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

    /// Start on the version already installed, even if a newer one is known
    #[arg(long)]
    pub no_auto_update: bool,

    /// Write the agent aliases into your shell startup files and exit
    #[arg(long)]
    pub install_alias: bool,

    /// Remove the agent aliases from your shell startup files and exit
    #[arg(long)]
    pub remove_alias: bool,

    /// Ask the agents — Claude Code, Gemini CLI, Cursor, Codex and OpenCode —
    /// to report session events to cctop, and exit. Takes `user` (the default)
    /// or `project` for the current directory's settings
    #[arg(long, num_args = 0..=1, default_missing_value = "user", value_name = "SCOPE")]
    pub install_hooks: Option<String>,

    /// Stop the agents reporting events to cctop, and exit. Same scopes as
    /// --install-hooks
    #[arg(long, num_args = 0..=1, default_missing_value = "user", value_name = "SCOPE")]
    pub remove_hooks: Option<String>,

    /// Report what is installed where, whether it still points at this binary,
    /// and whether events are being received, then exit
    #[arg(long)]
    pub hooks_status: bool,

    /// Print a context brief for a session as markdown, and exit. Takes a
    /// session id or a unique prefix of one; with no argument, briefs the most
    /// recently active session
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "SESSION")]
    pub handoff: Option<String>,

    /// Serve the Model Context Protocol on stdin/stdout, so an agent can ask
    /// what the other agents on this machine are doing. Read-only
    #[arg(long)]
    pub mcp: bool,

    /// Store a Claude token (from `claude setup-token`) for a profile in
    /// cctop's config, read from stdin, and exit. Takes the profile name;
    /// defaults to `default`
    #[arg(long, num_args = 0..=1, default_missing_value = "default", value_name = "PROFILE")]
    pub add_account: Option<String>,

    /// Also show the sessions on another machine, read over ssh. Repeatable.
    /// Takes `[user@]host`, or `[user@]host:/path/to/cctop` where cctop is not
    /// on the PATH a non-interactive ssh gets. $CCTOP_HOSTS adds more, comma
    /// separated. Remote rows are read-only: cctop acts only on this machine
    #[arg(long = "host", value_name = "HOST")]
    pub hosts: Vec<String>,

    /// Time each stage of the run and write the totals to a file on exit, for
    /// sending to a bug report. Takes a path; with no argument, writes beside
    /// the cache and prints where. Carries counts and durations only — no
    /// session titles, project paths or file names
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "FILE")]
    pub trace: Option<String>,
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
        // No column of its own here: --list has a fixed layout, and naming the
        // owner in front of the label costs nothing when there is no owner.
        let display = match &s.owner {
            Some(user) => format!("{user}: {display}"),
            None => display,
        };
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
        ("Gemini", Provider::Gemini),
        ("Windsurf", Provider::Windsurf),
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
    this_hour: f64,
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
    session_id: String,
    started_at: String,
    last_active: String,
    project: Option<String>,
    title: Option<String>,
    /// Login name of the user the session belongs to, when cctop is reading
    /// every user's homes and this one is not the reader's own.
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    /// Which `--host` this row was read from. Absent on local sessions, which
    /// is almost every row; present it would otherwise look like the machine
    /// the browser is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    /// Which Claude profile — which `$CLAUDE_CONFIG_DIR` — the session was read
    /// out of. Absent for every other harness, none of which has the concept.
    #[serde(skip_serializing_if = "Option::is_none")]
    profile: Option<String>,
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
    /// Most recently invoked tool, when a live session has one. The name the
    /// dashboard uses to answer "what is it doing" without opening the report.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
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

/// Print one session's context brief to stdout.
///
/// The non-interactive half of `O` in the UI, and the half a script can use:
/// the brief is plain markdown on stdout, so piping it into another agent's own
/// prompt flag needs nothing of cctop beyond this call.
///
/// `which` is a session id or any unique prefix of one; empty means the most
/// recently active session, which is nearly always the one just left.
pub fn run_handoff(sessions: &[Session], which: &str, loader: &Loader) -> anyhow::Result<()> {
    let matched: Vec<&Session> = match which.is_empty() {
        true => {
            // `max_by_key` on the timestamps rather than on load order: the
            // loader groups by provider, so "last in the list" is whichever
            // provider sorted last, not whichever session ran last.
            sessions
                .iter()
                .max_by_key(|s| s.last_active.clone())
                .into_iter()
                .collect()
        }
        false => sessions
            .iter()
            .filter(|s| s.session_id.starts_with(which))
            .collect(),
    };

    let session = match matched.as_slice() {
        [only] => *only,
        [] if which.is_empty() => anyhow::bail!("no sessions found"),
        [] => anyhow::bail!("no session id starts with '{which}'"),
        // Listing them is what makes the error actionable — a prefix is only
        // ambiguous relative to sessions the user cannot see from here.
        many => anyhow::bail!(
            "'{which}' matches {} sessions:\n{}",
            many.len(),
            many.iter()
                .map(|s| format!("  {} ({})", s.session_id, s.provider.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };

    // The brief is built out of the tool history, which the cache does not
    // carry, so this is one of the two callers that needs a real parse.
    let data = loader.store().session_data_fresh(session);
    let brief = crate::handoff::build(session, Some(&data));
    print!("{}", brief.to_markdown());
    Ok(())
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
/// Split out from [`run_json`] because the same document is the wire format for
/// two other readers now: `--host`, which parses it back off an ssh pipe, and
/// the web dashboard, which streams it to a browser. Anything that is true of
/// the printed JSON has to stay true of theirs, so there is one builder rather
/// than three that drift.
pub fn json_sessions(sessions: &[Session], plan: Plan, loader: &Loader) -> Vec<JsonSession> {
    let claude_account = crate::quota::claude_account();
    let codex_account = crate::quota::codex_account();
    let collisions = crate::collide::detect(sessions);

    sessions
        .iter()
        .map(|s| {
            let data = loader.store().session_data(s);
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
                    crate::session::ActivityState::ApiError => "error",
                },
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
                host: s.remote.as_ref().map(|r| r.host.clone()),
                profile: s.profile.clone(),
                account,
                model: (!s.model.is_empty()).then(|| s.model.clone()),
                harness: (!s.harness.is_empty()).then(|| s.harness.clone()),
                branch: crate::ui::columns::branch_of(s),
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
                    this_hour: s.cost_hour,
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
                last_tool: (!s.last_tool.is_empty()).then(|| s.last_tool.clone()),
                error: data.error.clone(),
            }
        })
        .collect()
}

pub fn run_json(sessions: &[Session], plan: Plan, loader: &Loader) -> anyhow::Result<()> {
    let out = json_sessions(sessions, plan, loader);
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

    /// The web dashboard answers "what is it doing" and "which machine" from
    /// these two fields. They have to be omitted when empty: an older `--host`
    /// peer must still parse, and a local session must not grow a blank host.
    #[test]
    fn json_names_the_last_tool_and_the_host_only_when_they_exist() {
        let mut live = Session::new(Provider::Claude, "abc".into());
        live.last_tool = "Bash".into();
        live.remote = Some(crate::session::Remote {
            host: "devbox".into(),
            branch: None,
        });
        let local = Session::new(Provider::Codex, "def".into());
        let loader = Loader::new();
        let doc = serde_json::to_value(json_sessions(&[live, local], Plan::Retail, &loader))
            .expect("serialises");
        assert_eq!(doc[0]["last_tool"], "Bash");
        assert_eq!(doc[0]["host"], "devbox");
        assert!(doc[1].get("last_tool").is_none());
        assert!(doc[1].get("host").is_none());
    }
}
