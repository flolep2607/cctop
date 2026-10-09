//! Command-line parsing and the non-interactive output modes.

use cctop_core::loader::Loader;
use cctop_core::pricing::{Plan, Provider};
use cctop_core::session::Session;
use cctop_core::util;
use clap::Parser;

/// The command index printed after the options by `-h` and `--help` alike.
///
/// The bare words are intercepted in `main` before clap runs, so this text is
/// the only place clap's output admits they exist. One line per command — the
/// detail lives in each command's own --help, and in `long_about` above for
/// the flags that shape them.
const COMMANDS: &str = "\
Commands:
  cctop <agent> [args…]   Start claude, codex, opencode or pi on a pty cctop
                          owns, so the UI can watch it and type into it. Same
                          as `cctop run <agent>`; the args go to the agent.
  cctop attach [pid]      Put a running agent on this terminal — no pid lists
                          them. F12 detaches and leaves it running.
  cctop as <acct> <agent> Start an agent under a named account (see
                          --add-account).
  cctop sandbox <host>:<path> [args…]
                          Run Claude Code here with its Bash commands on
                          <host> over ssh, and <path> mounted here with sshfs.
  cctop serve            Serve the table, and a per-session report, to a
                          browser. Loopback and read-only by default; `serve
                          --help` for the flags. Handy on a phone.
  cctop wait <session>    Block until a session stops working — by id prefix,
                          tab name or pid. For one agent to wait on another.
  cctop doctor            Check this installation and say what is wrong with
                          it: sessions, pricing, hooks, and what `s` reaches.
                          --host also tests an ssh target.
  cctop why [session]     Why a row says a session is running, or is not:
                          every agent process, its session, the rule that
                          matched it.
  cctop optimize          What your sessions spent and did not get back.
                          Reads only.
  cctop compare           How each model did on the work you actually gave
                          it — one-shot rate, cost per file, cache hit.
  cctop yield             Whether what each session spent ended up in a
                          commit on the default branch. Reads only.
  cctop recall <query>    What past sessions, from any agent, said about
                          something — the passages, ranked. Agents get it
                          through --mcp; --install-mcp sets that up.
  cctop burn              What your subscription windows were paid for and
                          did not use.
  cctop log               Print the event stream CCTOP_LOG writes; -f follows
                          it.
  cctop yolo off|status   Switch YOLO off for the Claude Code session this
                          runs in, or say which. On is /yolo, typed there.
  cctop yolo log          Every prompt YOLO allowed and every switch on or
                          off, kept after the session ends. Redacted.
  cctop --trace           Time each stage of a run, for a bug report about
                          slowness.

Each command takes --help for the details.";

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
                      cctop as <account> <agent> [args…]\n       \
                      cctop sandbox <host>:<path> [claude args…]\n       \
                      cctop serve [--bind ADDR] [--port PORT]\n       \
                      cctop tunnel setup | status | remove\n       \
                      cctop optimize | compare | yield | burn | log\n       \
                      cctop yolo off | status\n       \
                      cctop yolo log [--session ID] [--since WHEN] [-n N] [--json] [-f]\n       \
                      cctop recall <query> [--read SESSION PASSAGE]\n       \
                      cctop wait <session> [--until …] [--timeout …]\n       \
                      cctop why [session]\n       \
                      cctop doctor",
    // The one-line summary is the whole of `-h`; the command list is the part
    // that says what cctop is *for*, so both -h and --help carry it — `help`'s
    // line count is short enough that it fits underneath.
    after_help = COMMANDS,
    // --help's own copy of the list: `after_help` is not shown there, and the
    // long description's prose names most of the commands without ever saying
    // they are commands.
    after_long_help = COMMANDS,
    long_about = "cctop — an htop-like monitor for AI coding agent sessions\n\n\
Tracks Claude Code, Codex, Cursor, Devin, Gemini CLI, OpenCode, Pi, and\n    Windsurf\n\
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
a page per session: the conversation, what it edited, what it can reach, and\n  \
where its money went. From there it can send a prompt to a live session,\n  \
resume a dead one, or hand one to a different agent. It listens on 127.0.0.1\n  \
with a per-run access token in the URL, which is the whole credential — so\n  \
whoever holds the link can drive these agents. `--bind` is what puts it on the\n  \
network and `--tunnel` puts it on the internet, both saying so when they do;\n  \
`--no-actions` serves the pages without the buttons.\n\n\
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
naming whose each row is in the USER column; `/user:<name>` filters to one.\n  \
Other homes are only read: cctop writes nothing into them, and refuses to\n  \
delete another user's session. CCTOP_ALL_USERS=0 turns that\n  \
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
    ///
    /// $CCTOP_SETTLE_MS is the other half of the cadence: how long a
    /// transcript that was just written is left to finish being written
    /// before the rows are rebuilt from it (default 2000). 0 rebuilds on
    /// every refresh, at the cost of doing it throughout a live turn
    #[arg(short, long, default_value_t = 2.0, value_parser = parse_delay)]
    pub delay: f64,

    /// Clear persisted session extraction data before starting
    ///
    /// Keeps preferences and pricing
    #[arg(long)]
    pub clear_cache: bool,

    /// Replace this binary with the newest GitHub release and exit
    #[arg(long)]
    pub update: bool,

    /// Internal: the root half of `--update`. Installs the binary at PATH, which
    /// the user's half downloaded and verified, if it still hashes to SHA256
    #[arg(long, num_args = 2, value_names = ["PATH", "SHA256"], hide = true)]
    pub install_update: Option<Vec<String>>,

    /// Internal: the second half of `--update`. The binary just installed brings
    /// the hooks in line with its own form, which the one it replaced could not
    /// know
    #[arg(long, hide = true)]
    pub repair_hooks: bool,

    /// Start on the version already installed, even if a newer one is known
    #[arg(long)]
    pub no_auto_update: bool,

    /// Write the agent aliases into your shell startup files and exit
    #[arg(long)]
    pub install_alias: bool,

    /// Remove the agent aliases from your shell startup files and exit
    #[arg(long)]
    pub remove_alias: bool,

    /// Ask the agents to report session events to cctop, and exit
    ///
    /// Claude Code, Gemini CLI, Cursor, Codex and OpenCode. Takes `user` (the
    /// default) or `project` for the current directory's settings
    #[arg(long, num_args = 0..=1, default_missing_value = "user", value_name = "SCOPE")]
    pub install_hooks: Option<String>,

    /// Stop the agents reporting events to cctop, and exit
    ///
    /// Same scopes as --install-hooks
    #[arg(long, num_args = 0..=1, default_missing_value = "user", value_name = "SCOPE")]
    pub remove_hooks: Option<String>,

    /// Report what is installed where and whether events arrive, then exit
    ///
    /// Also whether each hook still points at this binary
    #[arg(long)]
    pub hooks_status: bool,

    /// Print a context brief for a session as markdown, and exit
    ///
    /// Takes a session id or a unique prefix of one; with no argument, briefs
    /// the most recently active session
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "SESSION")]
    pub handoff: Option<String>,

    /// Copy a session into another harness's store, and exit
    ///
    /// The copy is in the shape that harness's own resume command reads. Takes
    /// a session id or a unique prefix of one, then the harness to convert to
    /// — currently claude and codex, in either direction — optionally with an
    /// account, as `codex:work`, to write into that account's store. The copy keeps the
    /// session's own id where the receiving store has it free, so cctop can
    /// tell the two apart as one piece of work
    #[arg(
        long,
        num_args = 1..=2,
        value_names = ["SESSION", "AGENT"],
        default_missing_value = ""
    )]
    pub convert: Vec<String>,

    /// List the sessions converted as a handoff, and exit
    ///
    /// --remove deletes the copies, leaving the sessions they came from
    #[arg(long)]
    pub converted: bool,

    /// With --converted, remove the converted copies rather than listing them
    #[arg(long, requires = "converted")]
    pub remove: bool,

    /// Print one line for a status bar — tmux, waybar, a shell prompt — and
    /// exit
    ///
    /// How many agents are working, how many are waiting on you, and the
    /// current spend rate
    #[arg(long)]
    pub statusline: bool,

    /// Print one session's report as JSON, and exit
    ///
    /// The document `cctop serve` answers /api/report/<id> with. Takes a
    /// session id or a unique prefix; with no argument, the most recently
    /// active session. A serve answers for a remote row by running this on
    /// the machine the session lives on, over the same ssh channel the row
    /// arrived by
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "SESSION")]
    pub report: Option<String>,

    /// Print one session's conversation as JSON, and exit
    ///
    /// /api/chat/<id> on a serve. With --before, the window of turns ends just
    /// before that sequence number, which is how older turns are reached
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "SESSION")]
    pub chat: Option<String>,

    /// With --chat, end the returned window before this turn's sequence number
    #[arg(long, requires = "chat", value_name = "SEQ")]
    pub before: Option<usize>,

    /// With --chat, one subagent's own conversation instead of the session's
    ///
    /// The id is the subagent's, as `/api/chat/<id>?agent=` takes it and as
    /// the session's `Agent` calls name it in their `agent.id`
    #[arg(long, requires = "chat", value_name = "AGENT")]
    pub agent: Option<String>,

    /// Print one session's whole conversation as markdown, and exit
    ///
    /// Every turn, the words verbatim and each tool call on a line, headed by a
    /// note that it is context rather than instructions — made to paste into
    /// another agent, an issue or a doc. Takes a session id or a unique
    /// prefix; with no argument, the most recently active session. Thinking is
    /// left out, and tool results too unless --tool-output asks for them
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "SESSION")]
    pub export: Option<String>,

    /// With --export, include each tool call's result, cut at 800 characters
    #[arg(long, requires = "export")]
    pub tool_output: bool,

    /// Print what one session can reach, as JSON, and exit
    ///
    /// Instructions, skills, MCP servers — /api/access/<id> on a serve
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "SESSION")]
    pub access: Option<String>,

    /// Download the model the topical search needs (about 30 MB, once), then
    /// exit
    ///
    /// Until this is run, `/` searches transcripts literally and nothing
    /// reaches the network
    #[arg(long)]
    pub fetch_search_model: bool,

    /// Register `cctop --mcp` with Claude Code and Codex, and exit
    ///
    /// Through each agent's own `mcp add`. Takes `user` (the default) or
    /// `project` for the current directory. The agents can then recall what
    /// past sessions decided, and see what the running ones are doing
    #[arg(long, num_args = 0..=1, default_missing_value = "user", value_name = "SCOPE")]
    pub install_mcp: Option<String>,

    /// Serve the Model Context Protocol on stdin/stdout, and exit
    ///
    /// So an agent can ask what the other agents on this machine are doing.
    /// Read-only
    #[arg(long)]
    pub mcp: bool,

    /// Add a Claude account, and exit
    ///
    /// Asks whether it is a full login — its own ~/.claude-<name> via `claude
    /// auth login`, everything works — or a token from `claude setup-token`,
    /// which shares ~/.claude history but has no Remote Control or claude.ai
    /// connectors. Piped, reads one token from stdin. Takes the first
    /// account's name; defaults to `default`. Launch under one with `p` in the
    /// launcher, or `cctop as <name> claude`
    #[arg(long, num_args = 0..=1, default_missing_value = "default", value_name = "PROFILE")]
    pub add_account: Option<String>,

    /// Also show the sessions on another machine, read over ssh. Repeatable
    ///
    /// Takes `[user@]host`, or `[user@]host:/path/to/cctop` where cctop is not
    /// on the PATH a non-interactive ssh gets. $CCTOP_HOSTS adds more, comma
    /// separated. Remote rows are read-only: cctop acts only on this machine
    #[arg(long = "host", value_name = "HOST")]
    pub hosts: Vec<String>,

    /// Time each stage of the run and write the totals to a file on exit
    ///
    /// For sending to a bug report. Takes a path; with no argument, writes
    /// beside the cache and prints where. Carries counts and durations only —
    /// no session titles, project paths or file names
    #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "FILE")]
    pub trace: Option<String>,
}

/// `cctop wait`'s own flags.
///
/// A parser of its own, like `serve`'s, because `wait` is a bare word and
/// [`Args`] takes no positionals — `main` intercepts the word and hands the
/// rest here. Derived rather than hand-rolled because this one is meant to be
/// called from scripts, where a clap usage error that names the flag is worth
/// more than the few lines saved.
#[derive(Parser, Debug)]
#[command(
    name = "cctop wait",
    about = "Block until a session stops working",
    long_about = "Block until a session stops working, then exit.\n\n\
Reads what the dashboard reads: the transcripts, the agents' own hooks \
(heard live while waiting), and what a cctop recorded on the agent's rmux \
session. Useful for one agent to wait on another, or for a script that has \
just typed a prompt at one.",
    after_help = "Exit status: 0 when the condition is met, 1 when the session ended \
first without meeting it, 2 when the target names no session (or more than \
one), 124 on timeout."
)]
pub struct WaitArgs {
    /// The session: a prefix of its id, the name of its tab, or a pid in its
    /// process tree
    pub target: String,

    /// What to wait for
    #[arg(long, value_enum, default_value = "any-stop")]
    pub until: crate::wait::Until,

    /// Give up after this long: 90, 30s, 10m, 2h. 0 waits forever
    #[arg(long, default_value = "10m", value_parser = crate::wait::parse_timeout)]
    pub timeout: std::time::Duration,

    /// Print the outcome as one line of JSON on stdout
    #[arg(short, long)]
    pub json: bool,
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

/// The provider groups `--list` prints, in the order it prints them.
///
/// [`Provider::ALL`], so there is one list of harnesses rather than two that
/// have to be kept in step. This one had grown by insertion and read as
/// Codex, Claude, Cursor, Devin, OpenCode, Pi, Gemini, Windsurf — an order no
/// comment claimed to mean anything, and doctor's parsers section above its
/// partner listed the same eight differently. Group order is a presentation
/// choice rather than a fact about any harness, so it follows the enum.
pub fn list_order() -> impl Iterator<Item = Provider> {
    Provider::ALL.into_iter()
}

pub fn run_list(sessions: &[Session], plan: Plan) {
    let width = crossterm::terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(100)
        .max(60);
    let cost_label = if plan == Plan::Retail { "est" } else { "cost" };

    let mut offset = 0;
    for provider in list_order() {
        let group: Vec<&Session> = sessions.iter().filter(|s| s.provider == provider).collect();
        if group.is_empty() {
            continue;
        }
        if offset > 0 {
            println!();
        }
        print_group(provider.display_name(), &group, offset, cost_label, width);
        offset += group.len();
    }
}

// ---------------------------------------------------------------------------
// --json
// ---------------------------------------------------------------------------

/// Resolve `which` to one session — a full id, an unambiguous prefix, or the
/// empty string for the most recently active — or bail saying why.
///
/// Shared by every flag that prints one session's detail (`--handoff`,
/// `--report`, `--chat`, `--export`, `--access`), so they all answer an ambiguous prefix
/// the same way and all agree on what no argument means.
fn find_session<'a>(sessions: &'a [Session], which: &str) -> anyhow::Result<&'a Session> {
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

    match matched.as_slice() {
        [only] => Ok(*only),
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
    }
}

/// Print one session's context brief to stdout.
///
/// The non-interactive half of `O` in the UI, and the half a script can use:
/// the brief is plain markdown on stdout, so piping it into another agent's own
/// prompt flag needs nothing of cctop beyond this call.
pub fn run_handoff(sessions: &[Session], which: &str, loader: &Loader) -> anyhow::Result<()> {
    let session = find_session(sessions, which)?;
    // The brief is built out of the tool history, which the cache does not
    // carry, so this is one of the two callers that needs a real parse.
    let data = loader.store().session_data_fresh(session);
    let brief = cctop_core::handoff::build(session, Some(&data));
    // Printed *and* written: the record of the conversation is a file, and a
    // brief that named one it had not left would send its reader looking.
    print!("{}", cctop_core::handoff::rendered(&brief));
    Ok(())
}

// ---------------------------------------------------------------------------
// --convert
// ---------------------------------------------------------------------------

/// Copy a session into another harness's store so its own resume command can
/// pick the conversation up, and say where it went.
///
/// The transcript form of a handoff: where `--handoff` writes a brief for an
/// agent to read, this writes the conversation itself in the shape the
/// receiving harness reads back. That is worth doing only for a pair cctop can
/// transcode between, and `cctop_core::convert` is what knows which those are —
/// OpenCode keeps its transcripts in SQLite, so a conversion to or from it
/// answers "not yet" rather than half-doing it.
pub fn run_convert(sessions: &[Session], which: &str, agent: &str) -> anyhow::Result<()> {
    // A converted copy is excluded: converting one would hand over a
    // conversation cctop had already transcoded, losing whatever the first
    // conversion dropped, and the source it names is the session to convert
    // instead. Said here rather than left to chance, because the copy shares
    // its source's id and a bare prefix matches both.
    let session = find_original(sessions, which)?;
    if !cctop_core::convert::convertible_session(session) {
        anyhow::bail!(
            "{} has no transcript on this machine cctop can convert",
            session.provider.as_str()
        );
    }
    // `codex:spare` names the account as well as the harness: the copy is
    // written for whoever resumes it, and a session moved because one login is
    // out of window has to land where the other login looks.
    let (agent, account) = match agent.split_once(':') {
        Some((agent, account)) => (agent, Some(account)),
        None => (agent, None),
    };
    let target = cctop_core::pricing::Provider::parse(agent)
        .ok_or_else(|| anyhow::anyhow!("{agent} is not a harness cctop knows"))?;
    let profile = match account {
        Some(name) => Some(
            cctop_core::config::launchable_named(target, name).ok_or_else(|| {
                anyhow::anyhow!("{agent} has no account named '{name}' on this machine")
            })?,
        ),
        None => None,
    };
    if !cctop_core::convert::convertible(session.provider, target) {
        anyhow::bail!(
            "cctop cannot convert {} sessions into {}",
            session.provider.as_str(),
            target.as_str()
        );
    }
    let transcript = session
        .data_file
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("that session has no transcript on this machine"))?;
    let home = match (profile, target) {
        (Some(profile), _) => profile.dir.clone(),
        (None, cctop_core::pricing::Provider::Claude) => {
            cctop_core::config::CLAUDE_CONFIG_DIR.clone()
        }
        (None, cctop_core::pricing::Provider::Codex) => cctop_core::config::CODEX_HOME.clone(),
        _ => anyhow::bail!("that harness has no store cctop writes into"),
    };
    let written = cctop_core::convert::convert(session.provider, transcript, target, &home)
        .ok_or_else(|| anyhow::anyhow!("the transcript could not be converted"))?;
    // The id, because it is usually the session's own and the receiving store
    // may be a long way from the one it came from. `--json` and the tests read
    // this line, so it is the machine-readable part of the answer.
    println!(
        "{} {} {} {}",
        target.as_str(),
        written.session_id,
        if written.id_kept { "kept" } else { "renamed" },
        written.path.display()
    );
    Ok(())
}

#[cfg(test)]
mod convert_tests {
    use super::*;
    use cctop_core::session::Session;

    /// A session with just enough shape for the resolution rules to act on.
    fn session(id: &str, provider: cctop_core::pricing::Provider, converted: bool) -> Session {
        let mut s = Session::new(provider, id.into());
        s.label_source = "/tmp/proj".into();
        s.started_at = "2026-09-28T00:00:00.000Z".into();
        s.last_active = "2026-09-28T00:00:00.000Z".into();
        if converted {
            s.converted_from = Some(cctop_core::convert::Provenance {
                harness: "claude".into(),
                session_id: id.into(),
                session_path: "/tmp/proj/source.jsonl".into(),
                converted_at: "2026-09-28T00:00:00.000Z".into(),
            });
        }
        s
    }

    const ID: &str = "5049dbcd-8ef7-412f-bf57-589e61c41d0e";

    #[test]
    fn a_prefix_matching_a_session_and_a_copy_of_it_resolves_to_the_session() {
        let sessions = vec![
            session(ID, cctop_core::pricing::Provider::Claude, false),
            session(ID, cctop_core::pricing::Provider::Codex, true),
        ];
        // Without the originals-first rule this is an ambiguity error, which is
        // the wrong complaint: the user named a session, and the copy is not it.
        assert_eq!(find_original(&sessions, "5049d").unwrap().session_id, ID);
        assert!(
            find_original(&sessions, ID)
                .unwrap()
                .converted_from
                .is_none()
        );
    }

    #[test]
    fn an_id_that_is_only_a_copy_says_where_the_session_really_is() {
        let sessions = vec![session(ID, cctop_core::pricing::Provider::Codex, true)];
        let why = find_original(&sessions, ID).unwrap_err().to_string();
        assert!(why.contains("copy of a claude session"), "{why}");
        // Not "no session id starts with" — the id is right there.
        assert!(!why.contains("no session id starts with"), "{why}");
    }

    #[test]
    fn an_id_matching_nothing_says_so_plainly() {
        let sessions = vec![session(ID, cctop_core::pricing::Provider::Claude, false)];
        let why = find_original(&sessions, "zzz").unwrap_err().to_string();
        assert_eq!(why, "no session id starts with 'zzz'");
    }

    #[test]
    fn no_argument_takes_the_most_recent_original() {
        let mut older = session(ID, cctop_core::pricing::Provider::Claude, false);
        older.last_active = "2026-09-28T00:00:00.000Z".into();
        let mut newer = session(
            "aa4ff133-cde9-470f-aaff-1fd6ac2da49e",
            cctop_core::pricing::Provider::Codex,
            false,
        );
        newer.last_active = "2026-09-29T00:00:00.000Z".into();
        let want = newer.session_id.clone();
        let sessions = vec![older, newer];
        assert_eq!(find_original(&sessions, "").unwrap().session_id, want);
    }

    #[test]
    fn only_copies_and_no_argument_says_rather_than_picking_one() {
        let sessions = vec![session(ID, cctop_core::pricing::Provider::Codex, true)];
        let why = find_original(&sessions, "").unwrap_err().to_string();
        assert!(why.contains("copy of a claude session"), "{why}");
    }
}

// ---------------------------------------------------------------------------
// --converted
// ---------------------------------------------------------------------------

/// Resolve `which` to the session a conversion should read, preferring the
/// original over any copy made from it.
///
/// A converted session carries its source's id, so the same prefix matches both
/// and [`find_session`] would report the pair as ambiguous — a message about an
/// id that is genuinely shared, sent to a command that has an obvious answer.
/// The original is the one to convert: it holds the accounting the copy dropped
/// and is the session the user is thinking of.
fn find_original<'a>(sessions: &'a [Session], which: &str) -> anyhow::Result<&'a Session> {
    // `find_session`'s own rules — empty means the most recently active, a
    // prefix otherwise, several matches is an error — applied to the originals
    // alone. Filtering first is the whole point: a copy shares its source's id,
    // so the ambiguity has to be judged with the copies out of the way.
    if which.is_empty() {
        // Most recently active rather than first found, for the reason
        // `find_session` gives: the loader groups by provider, so load order is
        // whichever provider sorted last.
        return sessions
            .iter()
            .filter(|s| s.converted_from.is_none())
            .max_by_key(|s| s.last_active.clone())
            .ok_or_else(|| no_original(sessions, which));
    }
    let matched: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.converted_from.is_none() && s.session_id.starts_with(which))
        .collect();
    let found = match matched.as_slice() {
        [only] => *only,
        [] => return Err(no_original(sessions, which)),
        many => anyhow::bail!(
            "'{which}' matches {} sessions:\n{}",
            many.len(),
            many.iter()
                .map(|s| format!("  {} ({})", s.session_id, s.provider.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    };
    Ok(found)
}

/// Why no original session matched, said in terms of the copy that did.
///
/// Without this the command would report "no session id starts with" for an id
/// that is right there, one directory over, and the user would be sent looking
/// for a typo rather than told the session they have is a converted copy.
fn no_original(sessions: &[Session], which: &str) -> anyhow::Error {
    let copy = sessions.iter().find(|s| {
        s.converted_from.is_some() && (which.is_empty() || s.session_id.starts_with(which))
    });
    match copy {
        Some(copy) => {
            let source = copy.converted_from.as_ref().expect("filtered above");
            anyhow::anyhow!(
                "{} is a copy of a {} session that is not on this machine — \
                 convert that one where it lives",
                copy.session_id,
                source.harness
            )
        }
        None if which.is_empty() => anyhow::anyhow!("no sessions found"),
        None => anyhow::anyhow!("no session id starts with '{which}'"),
    }
}

/// List the sessions cctop converted as a handoff, or remove them.
///
/// A converted copy is an ordinary session to the harness that reads it, which
/// is the point — but it is not work anybody did here, and a list of sessions
/// that grows every time somebody hands work over is a list nobody reads. This
/// is how they are found again, and how the copies are cleared out.
pub fn run_converted(sessions: &[Session], remove: bool) -> anyhow::Result<()> {
    let converted: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.converted_from.is_some())
        .collect();
    if converted.is_empty() {
        println!("no converted sessions");
        return Ok(());
    }
    for session in &converted {
        let from = session.converted_from.as_ref().expect("filtered above");
        // The path is the transcript itself, which is the thing `--remove`
        // deletes. A row with no file is one a store keeps elsewhere, and
        // saying so beats printing a directory as though it were a file.
        let where_ = match session.data_file.as_deref() {
            Some(path) => path.display().to_string(),
            None => "no transcript on this machine".to_string(),
        };
        println!(
            "{} {} from {} {} ({where_})",
            session.provider.as_str(),
            session.session_id,
            from.harness,
            from.session_id,
        );
    }
    if !remove {
        return Ok(());
    }
    for session in &converted {
        // A session with a process behind it is somebody's *current* work now:
        // they resumed the copy and carried on, and deleting it would take the
        // transcript out from under a running agent.
        if session.is_running() {
            println!(
                "kept {} — a {} is using it",
                session.session_id,
                session.provider.as_str()
            );
            continue;
        }
        let Some(path) = session.data_file.as_deref() else {
            continue;
        };
        match std::fs::remove_file(path) {
            Ok(()) => println!("removed {}", path.display()),
            // The source is gone, the transcript lives on, and one failure
            // should not stop the rest of the list.
            Err(e) => println!("could not remove {}: {e}", path.display()),
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// --statusline
// ---------------------------------------------------------------------------

/// Print the one line a status bar shows, and exit.
///
/// `--list` answers "what is running" for a person and `--json` for a program;
/// this answers it for a tmux `status-right` or a waybar module, which has
/// room for a line and already knows how to keep one fresh. The counts use the
/// same state names the status dot and the `--json` document do.
pub fn run_statusline(sessions: &[Session]) {
    println!("{}", statusline(sessions));
}

/// The line `run_statusline` prints, split out so the tests can read it.
fn statusline(sessions: &[Session]) -> String {
    use cctop_core::session::ActivityState;
    let live: Vec<&Session> = sessions.iter().filter(|s| s.is_running()).collect();
    if live.is_empty() {
        return "no agents running".to_string();
    }
    let mut parts = Vec::new();
    for (state, word) in [
        (ActivityState::Working, "working"),
        (ActivityState::WaitingForInput, "waiting"),
        (ActivityState::Asking, "asking"),
        (ActivityState::ApiError, "error"),
    ] {
        let n = live.iter().filter(|s| s.activity_state == state).count();
        if n > 0 {
            parts.push(format!("{n} {word}"));
        }
    }
    // The rate is the number a line refreshed every few seconds is there to
    // move: today's total is in the table for whoever wants it.
    let per_hour = live.iter().map(|s| s.cost_per_min * 60.0).sum::<f64>();
    if per_hour >= 0.005 {
        parts.push(format!("{}/h", util::adaptive_usd(per_hour)));
    }
    parts.join(" · ")
}

// ---------------------------------------------------------------------------
// --report / --chat / --access: the per-session documents, as commands
// ---------------------------------------------------------------------------
//
// The same three documents `cctop serve` answers `/api/report`, `/api/chat`
// and `/api/access` with. They exist as commands so the serve on *this*
// machine can answer for a remote row by asking the cctop that actually has
// the transcript — `ssh host cctop --report <id>` over the channel `--host`
// already opens — rather than by reading a path on the wrong filesystem.

/// Print one session's report as JSON, and exit.
pub fn run_report(
    sessions: &[Session],
    which: &str,
    plan: Plan,
    loader: &Loader,
) -> anyhow::Result<()> {
    let session = find_session(sessions, which)?;
    let data = loader.store().session_data_fresh(session);
    let report = cctop_core::report::build(session, &data, plan);
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

/// Print one session's conversation as JSON, and exit.
pub fn run_chat(
    sessions: &[Session],
    which: &str,
    before: Option<usize>,
    agent: Option<&str>,
) -> anyhow::Result<()> {
    let session = find_session(sessions, which)?;
    // Deliberately not the cache: a conversation is the text the cache drops.
    let conversation = match agent {
        Some(agent) => cctop_core::chat::build_agent(session, agent, before)
            .ok_or_else(|| anyhow::anyhow!("no subagent {agent} in this session"))?,
        None => cctop_core::chat::build(session, before),
    };
    println!("{}", serde_json::to_string(&conversation)?);
    Ok(())
}

/// Print one session's whole conversation as markdown, and exit.
///
/// `/api/chat/<id>/markdown` on a serve, and what that route runs on a remote
/// row's machine. The whole transcript, not the page's window: see
/// [`cctop_core::chat::whole`].
pub fn run_export(sessions: &[Session], which: &str, tool_output: bool) -> anyhow::Result<()> {
    let session = find_session(sessions, which)?;
    let conversation = cctop_core::chat::whole(session);
    let options = cctop_core::export::Options { tool_output };
    print!(
        "{}",
        cctop_core::export::render(session, &conversation, options)
    );
    Ok(())
}

/// Print what one session can reach — instructions, skills, MCP servers —
/// as JSON, and exit.
pub fn run_access(sessions: &[Session], which: &str, loader: &Loader) -> anyhow::Result<()> {
    let session = find_session(sessions, which)?;
    let data = loader.store().session_data_fresh(session);
    println!(
        "{}",
        serde_json::to_string(&cctop_core::access::build(session, Some(&data)))?
    );
    Ok(())
}

pub fn run_json(sessions: &[Session], plan: Plan, loader: &Loader) -> anyhow::Result<()> {
    let out = cctop_core::json::sessions(sessions, plan, loader.store());
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// The binary's half of `pricing`'s every-provider check: the lists it
    /// keeps by hand rather than by iterating the enum.
    #[test]
    fn every_provider_has_a_list_group_and_a_session_directory() {
        for p in Provider::ALL {
            let where_ = p.as_str();
            // `--list` groups, which iterate the same list.
            assert!(
                list_order().any(|q| q == p),
                "`--list` has no group for {where_}"
            );
            // Doctor's sources section, which names a directory per provider.
            assert!(
                !crate::doctor::sessions_root(p).as_os_str().is_empty(),
                "{where_} has no session directory"
            );
        }
    }

    #[test]
    fn cli_definition_is_valid() {
        Args::command().debug_assert();
        WaitArgs::command().debug_assert();
    }

    /// `cctop wait`'s defaults are the ones a script wants without thinking:
    /// any stop, ten minutes — and every flag takes the spelling the docs use.
    #[test]
    fn wait_parses_its_target_and_flags() {
        use crate::wait::Until;
        let parse = |argv: &[&str]| {
            WaitArgs::try_parse_from(std::iter::once("cctop wait").chain(argv.iter().copied()))
        };
        let plain = parse(&["3f2a"]).expect("a bare target");
        assert_eq!(plain.target, "3f2a");
        assert_eq!(plain.until, Until::AnyStop);
        assert_eq!(plain.timeout, std::time::Duration::from_secs(600));
        assert!(!plain.json);

        let full =
            parse(&["reviewer", "--until", "done", "--timeout", "30s", "-j"]).expect("every flag");
        assert_eq!(full.until, Until::Done);
        assert_eq!(full.timeout, std::time::Duration::from_secs(30));
        assert!(full.json);
        assert_eq!(
            parse(&["1", "--until", "any-stop"]).unwrap().until,
            Until::AnyStop
        );
        assert!(parse(&["1", "--timeout", "0"]).unwrap().timeout.is_zero());

        assert!(parse(&[]).is_err(), "no target");
        assert!(parse(&["1", "--until", "never"]).is_err());
        assert!(parse(&["1", "--timeout", "soon"]).is_err());
        // A usage error exits 2, which is also an unknown session's code.
        assert_eq!(parse(&[]).unwrap_err().exit_code(), 2);
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

    /// The line a status bar is handed: live counts in the words the status
    /// dot uses, the burn rate while money is moving, and a quiet answer when
    /// nothing is running — stale rows must not count.
    #[test]
    fn the_statusline_names_live_states_and_the_burn_rate() {
        use cctop_core::session::ActivityState;
        let live = |state| {
            let mut s = Session::new(cctop_core::pricing::Provider::Claude, "x".into());
            s.inferred_running = true;
            s.activity_state = state;
            s
        };

        assert_eq!(statusline(&[]), "no agents running");
        assert_eq!(
            statusline(&[Session::new(
                cctop_core::pricing::Provider::Claude,
                "x".into()
            )]),
            "no agents running",
            "a stale transcript is not an agent at work"
        );

        let mut working = live(ActivityState::Working);
        working.cost_per_min = 0.20; // $12/h
        let line = statusline(&[
            working,
            live(ActivityState::WaitingForInput),
            live(ActivityState::WaitingForInput),
            live(ActivityState::Asking),
        ]);
        assert!(line.contains("1 working"), "{line}");
        assert!(line.contains("2 waiting"), "{line}");
        assert!(line.contains("1 asking"), "{line}");
        assert!(line.contains("$12"), "{line}");
    }
}
