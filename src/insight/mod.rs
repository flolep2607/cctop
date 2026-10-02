//! Shared analysis behind `cctop optimize` and `cctop compare`.
//!
//! Both commands ask questions the table cannot: not "what is this session
//! costing" but "what kind of work was it, and was any of it wasted". That
//! needs the individual tool calls with their arguments, and those are
//! [deliberately never cached](crate::session::Metrics::tool_details) — at
//! ~31 KB a session they were 83% of a cache that had to be read in full
//! before the first frame.
//!
//! So these commands re-parse every transcript they look at, in parallel, and
//! are slow in a way the table is not. That is the right trade: the table runs
//! many times a minute and these run when somebody asks a question.
//!
//! Everything here is derived from what the transcript already recorded, plus
//! — for `optimize` only — a read of Claude Code's configuration, to say which
//! file defines a server or skill nothing used (see [`inventory`]). There are
//! no model calls, no heuristic that needs the network, and nothing that
//! writes: both commands read and print.

pub mod compare;
pub mod inventory;
pub mod optimize;
mod unused;

use crate::pricing::{Plan, Provider};
use crate::session::{Session, SessionData, ToolDetail};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

/// What a session was mostly doing.
///
/// Deterministic, from tool composition — no model call, and no cost beyond the
/// parse that already happened. The categories exist because every metric below
/// is meaningless in aggregate: a 30% one-shot rate is alarming for editing and
/// unremarkable for debugging, and "half your spend went to conversation" is
/// only sayable if conversation is a category.
///
/// ponytail: one category per session, not per turn. A session is really a
/// sequence — explore, then code, then test — and the honest unit is the turn.
/// The turn is not reachable here: `tool_details` is grouped by tool name and
/// carries a timestamp but not a turn boundary, so a per-turn split would be
/// invented rather than read. A distribution over turns is the better shape if
/// the transcript ever offers one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Task {
    Coding,
    Debugging,
    Testing,
    Exploration,
    Planning,
    Delegation,
    Git,
    Build,
    Conversation,
    #[default]
    General,
}

impl Task {
    pub fn as_str(&self) -> &'static str {
        match self {
            Task::Coding => "coding",
            Task::Debugging => "debugging",
            Task::Testing => "testing",
            Task::Exploration => "exploration",
            Task::Planning => "planning",
            Task::Delegation => "delegation",
            Task::Git => "git",
            Task::Build => "build",
            Task::Conversation => "conversation",
            Task::General => "general",
        }
    }

    pub const ALL: [Task; 10] = [
        Task::Coding,
        Task::Debugging,
        Task::Testing,
        Task::Exploration,
        Task::Planning,
        Task::Delegation,
        Task::Git,
        Task::Build,
        Task::Conversation,
        Task::General,
    ];
}

/// Directory fragments whose contents an agent almost never needs to read.
///
/// Matched against the path as the transcript spelled it, so a relative
/// `node_modules/x` and an absolute one both hit. Kept deliberately short:
/// every entry here is a directory that is generated, vendored or versioned
/// elsewhere, and a false positive tells someone their real source file is
/// junk.
const JUNK: [&str; 9] = [
    "node_modules/",
    "/.git/",
    "/target/debug/",
    "/target/release/",
    "/dist/",
    "/build/",
    "/vendor/",
    "/.venv/",
    "__pycache__/",
];

fn is_junk(path: &str) -> bool {
    // Both separators, because a Windows transcript spells them the other way
    // and the same directory is no less generated for it.
    let normalised = path.replace('\\', "/");
    let padded = format!("/{normalised}");
    JUNK.iter().any(|j| padded.contains(j))
}

/// Tools that write to a file, across the harnesses that name their tools
/// differently. `apply_patch` is Codex's, `str_replace_editor` an older Claude
/// spelling that still appears in transcripts kept from the time.
const EDIT_TOOLS: [&str; 6] = [
    "Edit",
    "Write",
    "MultiEdit",
    "NotebookEdit",
    "apply_patch",
    "str_replace_editor",
];

const READ_TOOLS: [&str; 4] = ["Read", "NotebookRead", "read_file", "View"];

/// Whether a shell command writes to a file.
///
/// Without this an entire way of working is invisible. A session driven in
/// "do it through Bash" mode edits with `sed -i`, a heredoc and a redirect, and
/// never touches the Edit tool at all — 194 Bash calls, no edits, and cctop
/// filed it as Testing and then reported it as a session that "spent over $0.50
/// and edited no file". On this machine that mistake covered 19 sessions and
/// $216 of ordinary work.
///
/// ponytail: this reads the command, not the filesystem, so it is a heuristic
/// in both directions. A `python3 - <<PY` whose script calls `open(p, "w")`
/// writes a file and is not detectable here — the write is inside the program,
/// and the command line says only that python ran. Detail strings are also
/// capped, so a redirect past the cap is not seen. Both make this an
/// undercount, which is the safe direction: a missed write leaves a session
/// looking quieter than it was, where a false one would accuse somebody of
/// editing something they did not.
fn bash_writes(command: &str) -> bool {
    let cmd = command.to_ascii_lowercase();

    // In-place editors and copiers, each anchored so `grep sed` in a filename
    // cannot trigger one.
    const WRITERS: [&str; 8] = [
        "sed -i",
        "sed --in-place",
        "tee ",
        "patch -p",
        "install -m",
        "mv ",
        "cp ",
        "rsync ",
    ];
    if WRITERS.iter().any(|w| {
        cmd.split([';', '|', '&'])
            .any(|part| part.trim_start().starts_with(w) || part.contains(&format!(" {w}")))
    }) {
        return true;
    }

    // A redirect into something that looks like a path. `2>&1`, `>&2` and
    // `/dev/null` are the three that appear constantly and write nothing worth
    // counting.
    let bytes = cmd.as_bytes();
    for (i, _) in cmd.match_indices('>') {
        // Skip the `>` of `2>&1` and `->`, and the second `>` of `>>`.
        if i > 0 && matches!(bytes[i - 1], b'>' | b'-' | b'=') {
            continue;
        }
        let after = cmd[i..].trim_start_matches('>').trim_start();
        if after.starts_with('&') || after.starts_with("/dev/null") || after.is_empty() {
            continue;
        }
        // A target that names a file rather than a descriptor.
        let target = after.split_whitespace().next().unwrap_or("");
        if target.contains('/') || target.contains('.') {
            return true;
        }
    }
    false
}

/// Whether a shell command tests or builds the work — the commands whose
/// failure says the code is broken, as opposed to a `grep` that found nothing
/// or a `git diff --quiet` that found a change, which also exit non-zero.
///
/// ponytail: a list of the usual runners, so a project that checks itself
/// through a script of its own (`./ci.sh`) is never seen as checked.
fn is_check(command: &str) -> bool {
    const CHECKS: [&str; 20] = [
        "cargo test",
        "cargo nextest",
        "cargo build",
        "cargo check",
        "cargo clippy",
        "pytest",
        "vitest",
        "jest",
        "go test",
        "go build",
        "go vet",
        "npm test",
        "npm run test",
        "npm run build",
        "pnpm test",
        "yarn test",
        "tsc",
        "mypy",
        "phpunit",
        "make",
    ];
    let cmd = command.to_ascii_lowercase();
    cmd.split(['&', ';', '|']).any(|part| {
        // Past the wrappers a check is often run under — `RUST_LOG=x`,
        // `timeout 60`, `env`, `nice` — but never into another command's
        // arguments, where `grep cargo test` would read as a test run.
        let mut rest = part.trim_start();
        loop {
            let word = rest.split_whitespace().next().unwrap_or("");
            let wrapper = word.contains('=')
                || matches!(word, "env" | "nice" | "time" | "timeout" | "command")
                // `timeout`'s duration: `60`, `5m`.
                || word.starts_with(|c: char| c.is_ascii_digit());
            if !wrapper {
                break;
            }
            rest = rest[word.len()..].trim_start();
        }
        CHECKS.iter().any(|c| {
            rest.strip_prefix(c)
                .is_some_and(|after| after.is_empty() || after.starts_with(' '))
        })
    })
}

fn is_edit(tool: &str) -> bool {
    EDIT_TOOLS.iter().any(|t| t.eq_ignore_ascii_case(tool))
}

/// The tool each harness runs shell commands through.
fn is_shell(tool: &str) -> bool {
    ["Bash", "shell", "run_terminal_cmd", "execute_command"]
        .iter()
        .any(|t| t.eq_ignore_ascii_case(tool))
}

fn is_read(tool: &str) -> bool {
    READ_TOOLS.iter().any(|t| t.eq_ignore_ascii_case(tool))
}

/// One session, reduced to the things both commands ask about.
///
/// No `Default`: there is no default harness, and inventing one would put a
/// real provider's name on a row that came from nowhere.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub provider: Provider,
    pub label: String,
    /// The session's last activity, for `--since`.
    pub last_active: String,
    pub model: String,
    pub cost: f64,
    /// False where the provider records no usage, so a zero cost means "not
    /// said" rather than "free" and must not be averaged into anything.
    pub cost_available: bool,
    pub task: Task,
    pub calls: u64,
    pub errors: u64,
    /// Whether `errors` is a measurement or a provider's silence.
    pub records_outcomes: bool,
    /// Calls to a tool whose job is to write a file.
    pub edits: u64,
    /// Shell calls that wrote a file — see [`bash_writes`]. Kept apart from
    /// `edits` because only one of the two comes with a path, and the one-shot
    /// rate needs the path.
    pub bash_writes: u64,
    pub reads: u64,
    /// Distinct files edited, and how many of them took one contiguous attempt.
    pub files_edited: u64,
    pub files_one_shot: u64,
    /// Distinct paths read, for the cross-session duplicate detector.
    pub read_paths: HashSet<String>,
    /// Reads into generated or vendored directories, with the window growth
    /// they cost where the transcript recorded it.
    pub junk_reads: u64,
    pub junk_tokens: u64,
    /// Window growth spent re-reading a path this session had already read.
    pub reread_tokens: u64,
    pub rereads: u64,
    pub cache_read: u64,
    pub input_total: u64,
    /// Every file edited, resolved against the session's cwd — what
    /// [`mark_rework`] matches across sessions.
    pub edited: HashMap<String, Edited>,
    /// Files this session edited that a later fix session edited again — see
    /// [`mark_rework`]. Zero until that pass runs over the whole set.
    pub files_reworked: u64,
    /// How long the agent was working, in milliseconds — see [`active_ms`].
    pub active_ms: u64,
    /// The session split by which agent did the work — see [`Slice`]. One
    /// entry for a session with no subagents.
    pub slices: Vec<Slice>,
    /// The per-tool history hit its cap, so every count here is a floor.
    pub truncated: bool,
    /// The working directory, unabbreviated: what a project's configuration is
    /// keyed by.
    pub cwd: String,
    /// What the harness loaded before the conversation began — see
    /// [`crate::session::Loadout`]. Empty for anything but Claude Code.
    pub loadout: crate::session::Loadout,
    /// MCP servers this session called, as their tool names spell them.
    pub used_mcp: HashSet<String>,
    /// Skills it invoked, by slash command or through the `Skill` tool.
    pub used_skills: HashSet<String>,
    /// Agent types it delegated to.
    pub used_agents: HashSet<String>,
    /// What this session paid per token read from the cache, which is what
    /// anything sitting in the prompt prefix costs on each request. `None`
    /// where it read nothing from a cache or recorded no price.
    pub cached_rate: Option<f64>,
}

/// One agent's share of a session: the main agent, or one subagent.
///
/// A session that delegates is several models at once, and crediting all of it
/// to whichever cost the most put a subagent's edits, retries and minutes under
/// its parent's name — Opus rated on the work of the Haiku it handed a search
/// to. Each agent's calls carry who made them, and each subagent records its
/// own model and cost, so the work splits cleanly.
///
/// The responsibility does not. A parent that briefs a subagent, waits for it
/// and folds the result back in did real work towards those files, and its
/// tokens paid for it. So the subagent is credited with what it wrote — its
/// 1-shot rate is its own — while the parent's cost and time are spread over
/// its own files *and* the ones it delegated ([`Slice::delegated`]). Leaving
/// the delegated files out would rate a parent that only orchestrates as having
/// spent money on nothing.
#[derive(Debug, Clone, Default)]
pub struct Slice {
    /// `None` for the main agent, else the subagent's id.
    pub origin: Option<String>,
    pub model: String,
    pub cost: f64,
    pub calls: u64,
    pub edits: u64,
    pub files_edited: u64,
    pub files_one_shot: u64,
    /// Filled by [`mark_rework`].
    pub files_reworked: u64,
    pub active_ms: u64,
    /// The agent edited something, and the last test or build it ran after
    /// its final edit failed: it stopped with the work broken. False where
    /// nothing was checked after the last edit, which is "unknown", not "red".
    pub ended_red: bool,
    /// Main agent only: distinct files its subagents wrote on a *different*
    /// model, and that it did not also write itself. A subagent on the parent's
    /// own model folds into the same row, where counting its files here as well
    /// would count them twice.
    pub delegated: u64,
}

/// One file's edits within one session.
#[derive(Debug, Clone, Default)]
pub struct Edited {
    /// Timestamps of the first and last edit.
    pub first: String,
    pub last: String,
    /// The first edit came straight after a shell command by the same agent
    /// failed: something was run, it broke, and this file is what was changed
    /// in answer. The language-free half of "this edit was a fix" — see
    /// [`mark_rework`].
    pub after_failure: bool,
    /// The agent that edited it last — `None` for the main agent — which is
    /// who rework is charged to.
    pub by: Option<String>,
}

/// Every tool call in one session, oldest first.
///
/// `tool_details` is grouped by tool name; almost everything below needs the
/// order calls actually happened in, so it is flattened and sorted once here.
/// Timestamps are ISO-8601 and sort lexically, which is why this can be a
/// string comparison rather than a parse per call.
fn timeline(data: &SessionData) -> Vec<(&str, &ToolDetail)> {
    let mut all: Vec<(&str, &ToolDetail)> = data
        .metrics
        .tool_details
        .iter()
        .flat_map(|(name, list)| list.iter().map(move |d| (name.as_str(), d)))
        .collect();
    all.sort_by(|a, b| a.1.ts.cmp(&b.1.ts));
    all
}

/// A pause between two tool calls longer than this is somebody away from the
/// keyboard, not the agent working.
const IDLE_MS: i64 = 5 * 60 * 1000;

/// How long the agent spent working, from the timestamps of its tool calls.
///
/// The figure `compare` needs so that a cheap model is not reported as the
/// better one when it took a day to do what another did in five minutes. The
/// wall clock between first and last activity cannot answer that: a session
/// left open overnight is twelve hours long and did no work in eleven of them.
/// So the gaps between consecutive calls are summed, and a gap past
/// [`IDLE_MS`] counts only as long as the call before it was itself running —
/// a ten-minute build is the agent working, ten minutes of nothing is not.
///
/// Derived the same way for every harness, deliberately. Claude also records
/// a `turn_duration` per turn, which is closer to the truth, but not for every
/// turn and not anywhere else; a comparison whose clock depends on the
/// provider would be measuring the clock.
///
/// ponytail: only the stretch from the first tool call to the last is seen.
/// The thinking before a turn's first call and the answer after its last are
/// not, so this undercounts every model — by about the same amount, which is
/// what a comparison needs.
fn active_ms(timeline: &[(&str, &ToolDetail)]) -> u64 {
    let stamps: Vec<(i64, i64)> = timeline
        .iter()
        .filter_map(|(_, d)| {
            crate::util::parse_ts(&d.ts)
                .map(|t| (t.timestamp_millis(), d.dur_ms.unwrap_or(0).max(0)))
        })
        .collect();
    let mut total: i64 = stamps.last().map_or(0, |&(_, dur)| dur.min(IDLE_MS));
    for w in stamps.windows(2) {
        let ((at, dur), (next, _)) = (w[0], w[1]);
        let gap = (next - at).max(0);
        total += if gap <= IDLE_MS { gap } else { dur.min(gap) };
    }
    total.max(0) as u64
}

/// Which model to credit a session to: the one that cost the most.
///
/// A session that switched models mid-way is credited entirely to its dominant
/// one, because the tool calls cannot be attributed per model — the transcript
/// records which model billed a request, not which model asked for a given
/// call. `compare` says so on its output rather than pretending otherwise.
///
/// What `optimize` names a session by. `compare` splits subagents off first —
/// see [`Slice`] — and this is only the main agent's fallback there.
fn dominant_model(data: &SessionData) -> String {
    data.model_breakdown
        .iter()
        .max_by(|a, b| {
            a.total
                .partial_cmp(&b.total)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|m| m.model.clone())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| data.last_model.clone())
}

/// Classify by what the session did, falling back to what it said it was for.
///
/// Precedence is deliberate and tool-first. Keyword rules are English-shaped
/// and would misfile a French or Chinese prompt; tool composition is the same
/// in every language, so the keywords only ever break a tie between kinds of
/// editing — never decide whether editing happened.
fn classify(data: &SessionData, timeline: &[(&str, &ToolDetail)]) -> Task {
    if timeline.is_empty() && data.metrics.tool_count == 0 {
        return Task::Conversation;
    }
    if !data.subagents.is_empty() {
        return Task::Delegation;
    }

    let mut edits = 0u64;
    let mut reads = 0u64;
    let mut test_cmds = 0u64;
    let mut git_cmds = 0u64;
    let mut build_cmds = 0u64;
    let mut plan_calls = 0u64;
    for (tool, detail) in timeline {
        if is_edit(tool) {
            edits += 1;
        } else if is_read(tool) || tool.eq_ignore_ascii_case("Grep") {
            reads += 1;
        } else if tool.to_ascii_lowercase().contains("plan") {
            plan_calls += 1;
        } else if is_shell(tool) {
            let command = detail.full.as_deref().unwrap_or(&detail.d);
            // A shell that wrote a file is editing, whatever else it did.
            if bash_writes(command) {
                edits += 1;
            }
            let cmd = command.to_ascii_lowercase();
            if [
                "pytest",
                "vitest",
                "jest",
                "cargo test",
                "go test",
                "npm test",
                "phpunit",
            ]
            .iter()
            .any(|t| cmd.contains(t))
            {
                test_cmds += 1;
            } else if cmd.starts_with("git ") || cmd.contains("&& git ") {
                git_cmds += 1;
            } else if ["docker", "npm run build", "cargo build", "make ", "pm2 "]
                .iter()
                .any(|t| cmd.contains(t))
            {
                build_cmds += 1;
            }
        }
    }

    if edits > 0 {
        // A session that edited *and* ran tests is test-driven coding, not
        // testing. Ranking by which happened more often looked reasonable and
        // filed 22 of 57 Claude sessions as Testing on this very repository —
        // a category that swallows the work it was meant to distinguish is
        // worse than no category. Testing means running tests and changing
        // nothing, which is the only case the two are actually distinct.
        let title = data.title.as_deref().unwrap_or("").to_ascii_lowercase();
        if ["fix", "bug", "error", "broken", "fails", "debug"]
            .iter()
            .any(|k| title.contains(k))
        {
            return Task::Debugging;
        }
        return Task::Coding;
    }
    if test_cmds > 0 {
        return Task::Testing;
    }
    if plan_calls > 0 {
        return Task::Planning;
    }
    if git_cmds > 0 && git_cmds >= build_cmds {
        return Task::Git;
    }
    if build_cmds > 0 {
        return Task::Build;
    }
    if reads > 0 {
        return Task::Exploration;
    }
    Task::General
}

impl Analysis {
    /// The session by agent, or the whole of it as one main agent when it was
    /// never split — an `Analysis` built by hand rather than by [`analyse`].
    pub fn agent_slices(&self) -> Vec<Slice> {
        if !self.slices.is_empty() {
            return self.slices.clone();
        }
        vec![Slice {
            origin: None,
            model: self.model.clone(),
            cost: self.cost,
            calls: self.calls,
            edits: self.edits,
            files_edited: self.files_edited,
            files_one_shot: self.files_one_shot,
            files_reworked: self.files_reworked,
            active_ms: self.active_ms,
            ended_red: false,
            delegated: 0,
        }]
    }

    /// Every write this session made, however it made it.
    ///
    /// The thing to ask before saying a session changed nothing. `edits` alone
    /// answers that question wrongly for anyone working through the shell.
    pub fn wrote(&self) -> u64 {
        self.edits + self.bash_writes
    }
}

/// Cached input and total input, which are spelled differently per harness.
///
/// Adding every field together looks harmless and is not: Codex reports
/// `input_total` as the whole prompt with `cached_input` already inside it,
/// while Claude reports `input` as the *uncached* remainder alongside
/// `cache_read`. Summing both shapes counted Codex's cached tokens twice and
/// put every Codex model's cache hit rate over 50% before it had read anything.
fn input_split(provider: Provider, data: &SessionData) -> (u64, u64) {
    let t = &data.tokens;
    match provider {
        // Codex says so directly.
        Provider::Codex if t.input_total > 0 => (t.cached_input, t.input_total),
        // Everything else: fresh input plus what came from the cache, plus what
        // was paid to put it there — a cache write is billed input too.
        _ => (
            t.cache_read,
            t.input + t.cache_read + t.cache_write_5m + t.cache_write_1h,
        ),
    }
}

/// The servers a session called, by tool or through its resources.
fn used_mcp(data: &SessionData) -> HashSet<String> {
    data.metrics
        .mcp_tools
        .iter()
        .filter_map(|t| crate::session::mcp_server_of(t))
        .map(str::to_string)
        .chain(data.loadout.mcp_resources.iter().cloned())
        .collect()
}

/// The skills a session invoked: typed as a slash command, or loaded by the
/// model through the `Skill` tool. Either is a use, and counting only one of
/// them would call a skill the model reaches for on its own unused.
fn used_skills(data: &SessionData) -> HashSet<String> {
    data.metrics
        .skills
        .keys()
        .chain(&data.loadout.skills_invoked)
        .map(|s| s.trim_start_matches('/').to_string())
        .collect()
}

/// Dollars per cached input token, as this session was actually billed.
///
/// Read off the session's own split rather than a price table, for the same
/// reason [`optimize`] prices from the corpus: a bundled plan or a discounted
/// model should not be charged retail for its prompt prefix.
fn cached_rate(session: &Session, data: &SessionData) -> Option<f64> {
    let (t, c) = (&data.tokens, &data.costs);
    (session.cost_available && t.cache_read > 0 && c.cache_read > 0.0)
        .then(|| c.cache_read / t.cache_read as f64)
}

/// Reduce one session's freshly-parsed data to an [`Analysis`].
pub fn analyse(session: &Session, data: &SessionData) -> Analysis {
    let timeline = timeline(data);
    let task = classify(data, &timeline);
    let (cached, billed_in) = input_split(session.provider, data);

    let mut out = Analysis {
        provider: session.provider,
        label: session.abbrev_label.clone(),
        last_active: session.last_active.clone(),
        model: dominant_model(data),
        cost: data.costs.total,
        cost_available: session.cost_available,
        task,
        calls: data.metrics.tool_count,
        errors: data.metrics.tool_errors,
        records_outcomes: session.provider.records_tool_outcomes(),
        cache_read: cached,
        input_total: billed_in,
        active_ms: active_ms(&timeline),
        slices: Vec::new(),
        edited: HashMap::new(),
        files_reworked: 0,
        edits: 0,
        bash_writes: 0,
        reads: 0,
        files_edited: 0,
        files_one_shot: 0,
        read_paths: HashSet::new(),
        junk_reads: 0,
        junk_tokens: 0,
        reread_tokens: 0,
        rereads: 0,
        truncated: false,
        cwd: session.label_source.clone(),
        loadout: data.loadout.clone(),
        used_mcp: used_mcp(data),
        used_skills: used_skills(data),
        used_agents: data
            .subagents
            .iter()
            .map(|s| s.agent_type.clone())
            .filter(|t| !t.is_empty())
            .collect(),
        cached_rate: cached_rate(session, data),
    };

    // A tool whose history filled its cap has older calls dropped, so every
    // count derived from it is a floor rather than a total. Said once here so
    // both commands can label the row rather than quietly under-report it.
    out.truncated = data
        .metrics
        .tool_details
        .values()
        .any(|l| l.len() >= crate::config::MAX_TOOL_DETAILS);

    let mut seen_reads: HashSet<&str> = HashSet::new();
    // Edits per file in call order, so a file's attempts can be found later.
    let mut edit_order: HashMap<String, Vec<usize>> = HashMap::new();
    // Whether each agent's most recent shell command failed.
    let mut shell_failed: HashMap<&Option<String>, bool> = HashMap::new();

    for (i, (tool, detail)) in timeline.iter().enumerate() {
        let path = detail.d.as_str();
        if is_edit(tool) {
            out.edits += 1;
            for file in edited_files(detail) {
                let file = crate::collide::normalise(file, &session.label_source);
                let after_failure = shell_failed.get(&detail.origin) == Some(&true);
                let span = out
                    .edited
                    .entry(checkout_path(&file))
                    .or_insert_with(|| Edited {
                        first: detail.ts.clone(),
                        last: String::new(),
                        after_failure,
                        by: None,
                    });
                span.last.clone_from(&detail.ts);
                span.by.clone_from(&detail.origin);
                edit_order.entry(file).or_default().push(i);
            }
        } else if is_shell(tool) {
            shell_failed.insert(&detail.origin, detail.failed);
            // The full text when there is one: the short form is capped, and a
            // redirect past the cap would go unseen.
            let command = detail.full.as_deref().unwrap_or(path);
            if bash_writes(command) {
                out.bash_writes += 1;
            }
        } else if is_read(tool) {
            out.reads += 1;
            let growth = detail.window_growth.unwrap_or(0);
            if is_junk(path) {
                out.junk_reads += 1;
                out.junk_tokens += growth;
            }
            if !seen_reads.insert(path) {
                out.rereads += 1;
                out.reread_tokens += growth;
            }
            // Kept whole, not just counted: the cross-session detector needs to
            // know *which* file, because a path read once in each of six
            // sessions is a missing note in CLAUDE.md, and the same count spread
            // over six different files is nothing at all.
            out.read_paths.insert(path.to_string());
        }
    }

    for positions in edit_order.values() {
        out.files_edited += 1;
        let retried = positions
            .windows(2)
            .any(|w| is_retry(&timeline, w[0], w[1], out.records_outcomes));
        if !retried {
            out.files_one_shot += 1;
        }
    }

    out.slices = slices(session, data, &timeline, out.records_outcomes);
    out
}

/// Split a session by the agent that did each part — see [`Slice`].
fn slices(
    session: &Session,
    data: &SessionData,
    timeline: &[(&str, &ToolDetail)],
    outcomes: bool,
) -> Vec<Slice> {
    let sub_cost: f64 = data.subagents.iter().map(|s| s.cost).sum();
    let mut out = vec![Slice {
        origin: None,
        model: main_model(data),
        cost: (data.costs.total - sub_cost).max(0.0),
        ..Default::default()
    }];
    for sub in &data.subagents {
        let model = match sub.model.as_str() {
            "" | "?" => out[0].model.clone(),
            m => m.to_string(),
        };
        out.push(Slice {
            origin: Some(sub.agent_id.clone()),
            model,
            cost: sub.cost,
            ..Default::default()
        });
    }

    // Each agent's own calls, in order, and the files each edited.
    let mut files: Vec<HashSet<String>> = vec![HashSet::new(); out.len()];
    for (k, slice) in out.iter_mut().enumerate() {
        let own: Vec<(&str, &ToolDetail)> = timeline
            .iter()
            .copied()
            .filter(|(_, d)| d.origin == slice.origin)
            .collect();
        slice.calls = own.len() as u64;
        // The parent's clock is the session's: while a subagent works the
        // parent is waiting on it, and a job that took an hour end to end took
        // its owner an hour however it was divided up. Counting only the
        // parent's own calls lost every background subagent's run to the idle
        // cap. A subagent's clock is its own calls.
        slice.active_ms = match slice.origin {
            None => active_ms(timeline),
            Some(_) => active_ms(&own),
        };
        let mut order: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, (tool, detail)) in own.iter().enumerate() {
            if is_edit(tool) {
                slice.edits += 1;
                for file in edited_files(detail) {
                    let file =
                        checkout_path(&crate::collide::normalise(file, &session.label_source));
                    order.entry(file).or_default().push(i);
                }
            }
        }
        // Whether the last check after the last edit failed. Only where the
        // harness records outcomes: without them every check reads as passed.
        if outcomes && let Some(last_edit) = own.iter().rposition(|(t, _)| is_edit(t)) {
            slice.ended_red = own[last_edit + 1..]
                .iter()
                .rfind(|(tool, d)| is_shell(tool) && is_check(d.full.as_deref().unwrap_or(&d.d)))
                .is_some_and(|(_, d)| d.failed);
        }
        for (file, positions) in order {
            slice.files_edited += 1;
            if !positions
                .windows(2)
                .any(|w| is_retry(&own, w[0], w[1], outcomes))
            {
                slice.files_one_shot += 1;
            }
            files[k].insert(file);
        }
    }

    let main_model = out[0].model.clone();
    let delegated: HashSet<&String> = out
        .iter()
        .zip(&files)
        .skip(1)
        .filter(|(slice, _)| slice.model != main_model)
        .flat_map(|(_, f)| f)
        .filter(|f| !files[0].contains(*f))
        .collect();
    out[0].delegated = delegated.len() as u64;

    // A subagent that made no call and cost nothing is a row of zeros.
    let mut k = 0;
    out.retain(|s| {
        k += 1;
        k == 1 || s.calls > 0 || s.cost > 0.0
    });
    out
}

/// The main agent's model: the one that cost the most once every subagent's
/// spend is taken off the model it ran on.
///
/// Without the subtraction a parent that delegated heavily to a cheaper model
/// could be named after that model, because the breakdown is the whole
/// session's.
fn main_model(data: &SessionData) -> String {
    let mut by_model: HashMap<&str, f64> = data
        .model_breakdown
        .iter()
        .map(|m| (m.model.as_str(), m.total))
        .collect();
    for sub in &data.subagents {
        if let Some(c) = by_model.get_mut(sub.model.as_str()) {
            *c -= sub.cost;
        }
    }
    by_model
        .into_iter()
        .filter(|(m, c)| !m.is_empty() && *c > 1e-9)
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(m, _)| m.to_string())
        .unwrap_or_else(|| dominant_model(data))
}

/// The files one edit call wrote: every one a patch names, or the one the
/// call's detail string is.
///
/// A patch displays as `first.rs (+2 more)`, and keying on that counted a
/// three-file patch as one file that does not exist — so Codex, which edits
/// almost only through patches, was rated on names rather than files.
fn edited_files(detail: &ToolDetail) -> Vec<&str> {
    let files: Vec<&str> = if detail.paths.is_empty() {
        vec![detail.d.trim()]
    } else {
        detail.paths.iter().map(|p| p.trim()).collect()
    };
    // Deduplicated, because a patch can name one file twice — one hunk to
    // add a function and one to call it — and that is one write, not two.
    let mut out: Vec<&str> = Vec::with_capacity(files.len());
    for f in files {
        if !f.is_empty() && !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// Whether the edit at `next` is a second attempt at the edit at `prev` on
/// the same file.
///
/// Where the harness records outcomes, an attempt is a retry when something
/// told the agent the first one was wrong: the edit itself failed, or a shell
/// command or another edit failed between the two. Editing, running the tests
/// green and editing the same file again is the next step, not a retry, and
/// the older rule — any call in between at all — rated that as a miss.
///
/// Only calls by the same agent count. Subagents' calls are interleaved into
/// the same history, so a subagent's grep landing between two of the main
/// agent's edits made a clean edit look like a retry.
///
/// Where the harness records no outcomes there is no failure to see, and the
/// old rule stands: any call between the two by the same agent.
///
/// ponytail: a failure is the only evidence read. A user typing "no, not
/// like that" between two edits is a retry this cannot see, and an edit whose
/// test fails with nothing edited after is counted as a first-time success.
fn is_retry(timeline: &[(&str, &ToolDetail)], prev: usize, next: usize, outcomes: bool) -> bool {
    let (_, first) = timeline[prev];
    let origin = &timeline[next].1.origin;
    let between = timeline[(prev + 1).min(next)..next]
        .iter()
        .filter(|(_, d)| &d.origin == origin);
    if !outcomes {
        return between.count() > 0;
    }
    first.failed
        || between
            .filter(|(_, d)| d.failed)
            .any(|(tool, _)| is_shell(tool) || is_edit(tool))
}

/// A path as it would be in the main checkout rather than in a worktree of it.
///
/// One file is one file across sessions only if it is spelled the same, and a
/// session in `repo/.claude/worktrees/agent-x/` edits `src/ui/mod.rs` at a
/// different absolute path from a session in `repo/`. On this machine that
/// was every repeat edit of cctop's own files: rework found no file two
/// sessions had both touched, though they had touched dozens.
///
/// ponytail: only the `.claude/worktrees/<name>/` layout Claude Code makes. A
/// worktree elsewhere is a different directory as far as this can tell, and
/// asking git which repository it belongs to is a filesystem walk per path.
fn checkout_path(path: &str) -> String {
    const MARK: &str = "/.claude/worktrees/";
    match path.find(MARK) {
        Some(at) => {
            let rest = &path[at + MARK.len()..];
            match rest.find('/') {
                Some(slash) => format!("{}{}", &path[..at], &rest[slash..]),
                None => path.to_string(),
            }
        }
        None => path.to_string(),
    }
}

/// A day: how long after a session's last edit to a file a fix session can
/// still be blamed on it.
const REWORK_WINDOW_SECS: i64 = 24 * 60 * 60;

/// Who last wrote a file in one session, and when: the session's index, the
/// agent (`None` for the main one), and the Unix second of that last edit.
type Writer<'a> = (usize, Option<&'a str>, i64);

/// Charge each file a later fix session edited to the session that wrote it
/// last.
///
/// The question 1-shot cannot answer from inside one session: did the work
/// hold? A file a model wrote that a later session had to fix within a day is
/// the closest a transcript gets to "it was broken". Only the most recent
/// writer before the fix is charged — the session whose version was the one
/// being fixed — and only once per file however many fixes followed.
///
/// A later edit is a fix when it came straight after one of that session's
/// own commands failed ([`Edited::after_failure`]), or when the whole session
/// is [`Task::Debugging`]. The title alone was the first version, and on this
/// machine it found no fix session at all: titles say "Improve cctop" and
/// "cctop merge conflict", hardly ever "fix". A failing command is the same in
/// every language and needs no title.
///
/// An edit that is not a fix charges nobody. Editing `main.rs` every day is
/// normal, and charging that to whoever touched it last would rate every model
/// on how central its files were.
///
/// ponytail: a failure followed by an edit says the file was changed in
/// answer, not that it was the file at fault — a test can fail because the
/// test was wrong. And a project whose test suite was red before the session
/// began charges its first edit to whoever wrote that file last.
pub fn mark_rework(analyses: &mut [Analysis]) {
    use crate::util::parse_ts;
    // path -> every (session, agent, last edit) that wrote it.
    let mut writers: HashMap<&str, Vec<Writer>> = HashMap::new();
    for (i, a) in analyses.iter().enumerate() {
        for (path, e) in &a.edited {
            if let Some(t) = parse_ts(&e.last) {
                writers
                    .entry(path)
                    .or_default()
                    .push((i, e.by.as_deref(), t.timestamp()));
            }
        }
    }
    let mut charged: HashSet<(usize, &str, Option<&str>)> = HashSet::new();
    for (j, fix) in analyses.iter().enumerate() {
        let debugging = fix.task == Task::Debugging;
        for (path, e) in &fix.edited {
            if !debugging && !e.after_failure {
                continue;
            }
            let Some(at) = parse_ts(&e.first).map(|t| t.timestamp()) else {
                continue;
            };
            let blamed = writers.get(path.as_str()).and_then(|ws| {
                ws.iter()
                    .filter(|&&(i, _, t)| i != j && t <= at && at - t <= REWORK_WINDOW_SECS)
                    .max_by_key(|&&(_, _, t)| t)
            });
            if let Some(&(i, by, _)) = blamed {
                charged.insert((i, path.as_str(), by));
            }
        }
    }
    // Owned before the counts go back in, since `charged` borrows the set.
    let charges: Vec<(usize, Option<String>)> = charged
        .into_iter()
        .map(|(i, _, by)| (i, by.map(str::to_string)))
        .collect();
    for a in analyses.iter_mut() {
        a.files_reworked = 0;
        for s in &mut a.slices {
            s.files_reworked = 0;
        }
    }
    for (i, by) in charges {
        let a = &mut analyses[i];
        a.files_reworked += 1;
        // The agent that wrote it, or the main agent when that subagent's
        // slice was dropped for having done nothing else.
        let at = a.slices.iter().position(|s| s.origin == by).unwrap_or(0);
        if let Some(s) = a.slices.get_mut(at) {
            s.files_reworked += 1;
        }
    }
}

/// Freshly parse and analyse every session, in parallel.
///
/// `Store::session_data_fresh` is the only path that returns the tool history —
/// a cached copy always has it stripped — so this cannot be served from the
/// cache however warm it is.
pub fn scan(plan: Plan) -> Vec<Analysis> {
    let mut loader = crate::loader::Loader::new();
    let walked = loader.load(plan);
    from_store(&walked, loader.store())
}

/// Analyse an already-walked set against an existing store.
///
/// Split from [`scan`] so the UI worker can use the loader it already has. That
/// loader's walk is warm, so opening this from the table costs the fresh
/// re-parse and nothing else.
pub fn from_store(sessions: &[Session], store: &crate::cache::Store) -> Vec<Analysis> {
    let mut analyses: Vec<Analysis> = sessions
        .par_iter()
        .map(|s| {
            let data = store.session_data_fresh(s);
            analyse(s, &data)
        })
        .collect();
    mark_rework(&mut analyses);
    analyses
}

/// Sessions worth reasoning about.
///
/// A session with no tool calls at all is either pure conversation or a
/// transcript cctop cannot read the calls out of, and the two are not
/// distinguishable here. Both would drag every average toward zero, so they are
/// counted separately rather than mixed in.
pub fn substantive(a: &Analysis) -> bool {
    a.calls > 0
}

pub fn only(analyses: &[Analysis], provider: Option<Provider>) -> Vec<&Analysis> {
    since(analyses, provider, None)
}

/// Sessions of `provider`, active at or after `cutoff`.
///
/// A session with no timestamp at all is kept: dropping it would make a filter
/// meant to narrow the window silently discard whatever cctop could not date.
pub fn since(
    analyses: &[Analysis],
    provider: Option<Provider>,
    cutoff: Option<chrono::DateTime<chrono::Utc>>,
) -> Vec<&Analysis> {
    analyses
        .iter()
        .filter(|a| provider.is_none_or(|p| a.provider == p))
        .filter(|a| {
            cutoff.is_none_or(|c| crate::util::parse_ts(&a.last_active).is_none_or(|t| t >= c))
        })
        .collect()
}

/// `--since`: a span back from now (`90m`, `24h`, `7d`, `2w`) or a date
/// (`2026-09-01`, midnight UTC).
fn parse_since(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let now = chrono::Utc::now();
    if let Ok(day) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return day.and_hms_opt(0, 0, 0).map(|t| t.and_utc());
    }
    let unit = value.chars().last()?;
    let n: i64 = value[..value.len() - unit.len_utf8()].parse().ok()?;
    let span = match unit {
        'm' => chrono::Duration::minutes(n),
        'h' => chrono::Duration::hours(n),
        'd' => chrono::Duration::days(n),
        'w' => chrono::Duration::weeks(n),
        _ => return None,
    };
    (n > 0).then(|| now - span)
}

pub const HELP: &str = "\
cctop optimize — what your sessions spent and did not get back
cctop compare  — how each model behaved on the work you gave it

USAGE:
  cctop optimize [--provider NAME] [--since SPAN] [--json]
  cctop compare  [--provider NAME] [--since SPAN] [--rate USD_PER_HOUR] [--json]

Both re-read every transcript rather than using the session cache, because the
individual tool calls are the thing they reason about and those are never
cached. Expect them to take a few seconds on a large machine.

OPTIONS:
  --provider NAME  Only this harness: claude, codex, cursor, gemini, opencode,
                   pi, windsurf.
  --since SPAN     Only sessions active in the last SPAN — 24h, 7d, 2w — or
                   since a date, 2026-09-01. Models change, and so does what
                   you give them; last month's sessions blur this week's.
  --rate USD       compare only: what an hour of agent time is worth to you.
                   Adds a column of dollars plus time per file, and ranks by
                   it — a free model that takes a day can lose to a $10 one
                   that takes five minutes.
  --json           Machine-readable, for scripting.
  -h, --help       This.

Both read and print. Neither writes anything, to your configuration or
anywhere else.
";

/// `cctop optimize` and `cctop compare`.
pub fn run(which: &str, argv: &[String]) -> i32 {
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return 0;
    }

    let mut provider = None;
    let mut cutoff = None;
    let mut rate = None;
    let mut json = false;
    let mut args = argv.iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => json = true,
            "--provider" => match args.next().and_then(|p| provider_named(p)) {
                Some(p) => provider = Some(p),
                None => {
                    eprintln!("cctop {which}: --provider needs a harness name; see --help");
                    return 2;
                }
            },
            "--since" => match args.next().and_then(|v| parse_since(v)) {
                Some(c) => cutoff = Some(c),
                None => {
                    eprintln!(
                        "cctop {which}: --since needs a span or a date, e.g. 7d or 2026-09-01"
                    );
                    return 2;
                }
            },
            "--rate" if which == "compare" => {
                match args
                    .next()
                    .and_then(|r| r.trim_start_matches('$').parse::<f64>().ok())
                {
                    Some(r) if r.is_finite() && r >= 0.0 => rate = Some(r),
                    _ => {
                        eprintln!("cctop compare: --rate needs dollars per hour, e.g. --rate 60");
                        return 2;
                    }
                }
            }
            other => {
                eprintln!("cctop {which}: unexpected argument `{other}`; see --help");
                return 2;
            }
        }
    }

    // Both re-parse every transcript, and a parse prices each request as it
    // reads it, so the table has to be in before the scan rather than after.
    // Dispatched ahead of `main`'s own load, these ran on the handful of rates
    // compiled in and priced every newer model at nothing: 9 of 11 models on
    // `compare` read as unpriced while the cached LiteLLM table listed all 9.
    crate::pricing::refresh_pricing_blocking();

    let analyses = scan(Plan::Retail);
    let selected = since(&analyses, provider, cutoff);

    match (which, json) {
        ("optimize", false) => print!("{}", optimize::report(&selected)),
        ("compare", false) => print!("{}", compare::report_at(&selected, rate)),
        ("optimize", true) => println!("{}", optimize::as_json(&selected)),
        (_, true) => println!("{}", compare::as_json(&selected, rate)),
        _ => unreachable!("only optimize and compare reach here"),
    }
    0
}

/// `1 session` / `2 sessions`, because a report that says "1 sessions" reads as
/// one nobody proof-read.
pub fn plural(n: usize, noun: &str) -> String {
    match n {
        1 => format!("1 {noun}"),
        _ => format!("{n} {noun}s"),
    }
}

/// A harness name as somebody would type it at `--provider`.
fn provider_named(name: &str) -> Option<Provider> {
    let name = name.to_ascii_lowercase();
    [
        Provider::Claude,
        Provider::Codex,
        Provider::Cursor,
        Provider::Gemini,
        Provider::OpenCode,
        Provider::Pi,
        Provider::Windsurf,
    ]
    .into_iter()
    .find(|p| p.as_str() == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Metrics, ToolDetail};

    fn call(name: &str, arg: &str, ts: &str) -> (String, ToolDetail) {
        (
            name.to_string(),
            ToolDetail {
                d: arg.to_string(),
                ts: ts.to_string(),
                ..Default::default()
            },
        )
    }

    fn data_of(calls: &[(String, ToolDetail)]) -> SessionData {
        let mut details: HashMap<String, Vec<ToolDetail>> = HashMap::new();
        for (name, d) in calls {
            details.entry(name.clone()).or_default().push(d.clone());
        }
        SessionData {
            metrics: Metrics {
                tool_count: calls.len() as u64,
                tool_details: details,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn analysed(provider: Provider, calls: &[(String, ToolDetail)]) -> Analysis {
        let mut s = Session::new(provider, "sid".into());
        s.cost_available = true;
        analyse(&s, &data_of(calls))
    }

    fn failing(name: &str, arg: &str, ts: &str) -> (String, ToolDetail) {
        let (n, mut d) = call(name, arg, ts);
        d.failed = true;
        (n, d)
    }

    /// The distinction the whole one-shot rate rests on. Editing a file, seeing
    /// something fail, and editing it again is a retry; editing two different
    /// files is progress, and editing again after the tests passed is the next
    /// step — neither may be counted as one.
    #[test]
    fn a_retry_is_the_same_file_edited_after_something_failed() {
        let retried = analysed(
            Provider::Claude,
            &[
                call("Edit", "/a.rs", "01"),
                failing("Bash", "cargo test", "02"),
                call("Edit", "/a.rs", "03"),
            ],
        );
        assert_eq!(retried.files_edited, 1);
        assert_eq!(retried.files_one_shot, 0, "a failure, then the same file");

        let next_step = analysed(
            Provider::Claude,
            &[
                call("Edit", "/a.rs", "01"),
                call("Bash", "cargo test", "02"),
                call("Edit", "/a.rs", "03"),
            ],
        );
        assert_eq!(next_step.files_one_shot, 1, "the tests passed in between");

        let failed_edit = analysed(
            Provider::Claude,
            &[failing("Edit", "/a.rs", "01"), call("Edit", "/a.rs", "02")],
        );
        assert_eq!(
            failed_edit.files_one_shot, 0,
            "the first edit itself failed"
        );

        let progress = analysed(
            Provider::Claude,
            &[
                call("Edit", "/a.rs", "01"),
                failing("Bash", "cargo test", "02"),
                call("Edit", "/b.rs", "03"),
            ],
        );
        assert_eq!(progress.files_edited, 2);
        assert_eq!(
            progress.files_one_shot, 2,
            "two different files is two first attempts, not a retry"
        );
    }

    /// A harness that records no outcomes has no failure to see, so any call
    /// between two edits of a file still makes the second a retry.
    #[test]
    fn without_outcomes_any_call_between_is_a_retry() {
        let calls = [
            call("Edit", "/a.rs", "01"),
            call("Bash", "cargo test", "02"),
            call("Edit", "/a.rs", "03"),
        ];
        assert_eq!(analysed(Provider::Cursor, &calls).files_one_shot, 0);
        let burst = [call("Edit", "/a.rs", "01"), call("Edit", "/a.rs", "02")];
        assert_eq!(analysed(Provider::Cursor, &burst).files_one_shot, 1);
    }

    /// Subagents' calls are interleaved into the parent's history. One of
    /// theirs failing between two of the parent's edits says nothing about the
    /// parent's attempt.
    #[test]
    fn another_agents_failure_is_not_a_retry() {
        let mut theirs = failing("Bash", "cargo test", "02");
        theirs.1.origin = Some("sub".into());
        let a = analysed(
            Provider::Claude,
            &[
                call("Edit", "/a.rs", "01"),
                theirs,
                call("Edit", "/a.rs", "03"),
            ],
        );
        assert_eq!(a.files_one_shot, 1);
    }

    /// A patch displays as its first file and `(+N more)`. Keying on that
    /// counted a three-file patch as one file that does not exist.
    #[test]
    fn a_patch_counts_every_file_it_touched() {
        let (n, mut d) = call("apply_patch", "a.rs (+2 more)", "01");
        // `a.rs` twice: two hunks in one file, which once indexed past
        // itself and panicked on a real transcript.
        d.paths = vec!["a.rs".into(), "b.rs".into(), "a.rs".into(), "c.rs".into()];
        let a = analysed(Provider::Codex, &[(n, d)]);
        assert_eq!(a.files_edited, 3);
        assert_eq!(a.files_one_shot, 3);
    }

    fn edited_at(task: Task, files: &[(&str, &str)]) -> Analysis {
        let mut a = analysed(Provider::Claude, &[]);
        a.task = task;
        for (path, ts) in files {
            a.edited.insert(
                path.to_string(),
                Edited {
                    first: ts.to_string(),
                    last: ts.to_string(),
                    after_failure: false,
                    by: None,
                },
            );
        }
        a
    }

    /// A parent on one model that hands the editing to a subagent on another.
    /// The subagent is credited with its own work; the parent with its own
    /// edits plus the files it delegated, which its cost is spread over.
    fn delegating_session() -> Analysis {
        let sub_call = |name: &str, arg: &str, ts: &str, failed: bool| {
            let (n, mut d) = call(name, arg, ts);
            d.origin = Some("agent-1".into());
            d.failed = failed;
            (n, d)
        };
        let calls = vec![
            call("Edit", "/p.rs", "2026-01-01T10:00:00Z"),
            call("Task", "go", "2026-01-01T10:01:00Z"),
            sub_call("Edit", "/a.rs", "2026-01-01T10:02:00Z", false),
            sub_call("Bash", "cargo test", "2026-01-01T10:03:00Z", true),
            sub_call("Edit", "/a.rs", "2026-01-01T10:04:00Z", false),
            sub_call("Edit", "/b.rs", "2026-01-01T10:05:00Z", false),
            // The parent writing the subagent's file is the parent's own edit,
            // and that file is no longer one it merely delegated.
            call("Edit", "/b.rs", "2026-01-01T10:06:00Z"),
        ];
        let mut data = data_of(&calls);
        let breakdown = |model: &str, total: f64| crate::session::ModelBreakdown {
            model: model.into(),
            tokens: Default::default(),
            costs: Default::default(),
            total,
        };
        data.model_breakdown = vec![breakdown("big", 8.0), breakdown("small", 3.0)];
        data.costs.total = 11.0;
        data.subagents = vec![crate::session::Subagent {
            agent_id: "agent-1".into(),
            agent_type: "general-purpose".into(),
            description: String::new(),
            model: "small".into(),
            started_at: None,
            last_active: None,
            duration_ms: 0,
            status: crate::session::SubagentStatus::Done,
            cost: 3.0,
            tool_count: 4,
            tool_use_id: None,
            context: None,
            ghost: false,
        }];
        let mut s = Session::new(Provider::Claude, "sid".into());
        s.cost_available = true;
        analyse(&s, &data)
    }

    #[test]
    fn a_subagent_is_credited_to_its_own_model() {
        let a = delegating_session();
        assert_eq!(a.slices.len(), 2);
        let (main, sub) = (&a.slices[0], &a.slices[1]);

        assert_eq!(main.model, "big");
        assert_eq!(main.cost, 8.0, "the session's cost less the subagent's");
        assert_eq!((main.files_edited, main.files_one_shot), (2, 2));
        assert_eq!(main.delegated, 1, "a.rs; b.rs the parent wrote itself");
        assert_eq!(main.calls, 3);

        assert_eq!(sub.model, "small");
        assert_eq!(sub.cost, 3.0);
        assert_eq!(sub.files_edited, 2);
        assert_eq!(
            sub.files_one_shot, 1,
            "a.rs was retried after its test failed"
        );
        assert_eq!(sub.active_ms, 3 * 60_000);
    }

    /// A subagent on its parent's own model lands in the parent's row, so its
    /// files must not also be counted as delegated there.
    #[test]
    fn a_subagent_on_the_parents_model_is_not_delegation() {
        let mut a = delegating_session();
        for s in &mut a.slices {
            s.model = "big".into();
        }
        a.slices[0].delegated = 0;
        let table = compare::rows(&[&a], None);
        assert_eq!(table.len(), 1);
        assert_eq!(table[0].files_delegated, 0);
        assert_eq!(
            table[0].files_edited, 4,
            "p, b by the parent; a, b by the subagent"
        );
        assert_eq!(table[0].sessions, 1, "one session, however many agents");
        assert_eq!(
            table[0].active_ms, a.slices[0].active_ms,
            "the parent's time already covers its subagent's"
        );
    }

    #[test]
    fn the_parent_spreads_its_cost_over_what_it_delegated() {
        let a = delegating_session();
        let table = compare::rows(&[&a], None);
        let big = table.iter().find(|r| r.model == "big").unwrap();
        let small = table.iter().find(|r| r.model == "small").unwrap();
        assert_eq!(big.files_produced(), 3);
        assert_eq!(big.per_edit(), Some(8.0 / 3.0));
        assert_eq!(small.per_edit(), Some(3.0 / 2.0));
        assert_eq!(big.one_shot(), None, "two files of its own is not a rate");
    }

    /// Red means the last check after the final edit failed. A failing grep
    /// is not a check, and a session that never checked is unknown, not red.
    #[test]
    fn an_agent_that_stopped_on_a_failing_check_ended_red() {
        let red = analysed(
            Provider::Claude,
            &[
                call("Edit", "/a.rs", "01"),
                call("Bash", "cargo test", "02"),
                call("Edit", "/a.rs", "03"),
                failing("Bash", "cargo test 2>&1 | tail -5", "04"),
                failing("Bash", "grep -n nothing src/a.rs", "05"),
            ],
        );
        assert!(red.slices[0].ended_red);

        let fixed = analysed(
            Provider::Claude,
            &[
                call("Edit", "/a.rs", "01"),
                failing("Bash", "cargo test", "02"),
                call("Bash", "cargo test", "03"),
            ],
        );
        assert!(!fixed.slices[0].ended_red, "the last check passed");

        let unchecked = analysed(
            Provider::Claude,
            &[
                failing("Bash", "cargo test", "01"),
                call("Edit", "/a.rs", "02"),
            ],
        );
        assert!(!unchecked.slices[0].ended_red, "nothing ran after the edit");

        assert!(is_check("cd x && cargo clippy --all-targets"));
        assert!(is_check("make"));
        assert!(is_check("RUST_LOG=debug timeout 60 cargo test -q"));
        assert!(!is_check("makefile-lint"), "a check is a whole word");
        assert!(!is_check("grep -n cargo test src/a.rs | head"));
        assert!(!is_check("git diff --quiet"));
    }

    #[test]
    fn since_takes_a_span_or_a_date() {
        let now = chrono::Utc::now();
        let week = parse_since("7d").unwrap();
        assert!((now - week - chrono::Duration::days(7)).num_seconds().abs() < 5);
        assert_eq!(
            parse_since("2026-09-01").unwrap().to_rfc3339(),
            "2026-09-01T00:00:00+00:00"
        );
        for bad in ["", "d", "7", "7y", "-3d", "0d", "yesterday"] {
            assert!(parse_since(bad).is_none(), "{bad}");
        }

        let mut old = analysed(Provider::Claude, &[]);
        old.last_active = "2020-01-01T00:00:00Z".into();
        let mut undated = analysed(Provider::Claude, &[]);
        undated.last_active = String::new();
        let set = [old, undated];
        let kept = since(&set, None, Some(week));
        assert_eq!(kept.len(), 1, "the old one goes, the undated one stays");
        assert!(kept[0].last_active.is_empty());
    }

    /// A worktree's file is the main checkout's file, for matching edits
    /// across sessions.
    #[test]
    fn a_worktree_path_folds_into_its_checkout() {
        assert_eq!(
            checkout_path("/r/cctop/.claude/worktrees/agent-9/src/ui/mod.rs"),
            "/r/cctop/src/ui/mod.rs"
        );
        assert_eq!(
            checkout_path("/r/cctop/src/ui/mod.rs"),
            "/r/cctop/src/ui/mod.rs"
        );
        assert_eq!(
            checkout_path("/r/cctop/.claude/worktrees/agent-9"),
            "/r/cctop/.claude/worktrees/agent-9"
        );
    }

    /// Titles hardly ever say "fix", so a fix is also an edit made straight
    /// after the session's own command failed — in any session, whatever it
    /// is called.
    #[test]
    fn an_edit_after_a_failure_is_a_fix_without_a_title() {
        let fix = analysed(
            Provider::Claude,
            &[
                call("Bash", "cargo test", "2026-01-01T12:00:00Z"),
                failing("Bash", "cargo test", "2026-01-01T12:01:00Z"),
                call("Edit", "/a.rs", "2026-01-01T12:02:00Z"),
                call("Bash", "cargo test", "2026-01-01T12:03:00Z"),
                call("Edit", "/b.rs", "2026-01-01T12:04:00Z"),
            ],
        );
        assert_eq!(fix.task, Task::Coding, "no title says it was a fix");
        assert!(fix.edited["/a.rs"].after_failure);
        assert!(!fix.edited["/b.rs"].after_failure, "the tests passed first");

        let mut set = vec![
            edited_at(
                Task::Coding,
                &[
                    ("/a.rs", "2026-01-01T09:00:00Z"),
                    ("/b.rs", "2026-01-01T09:00:00Z"),
                ],
            ),
            fix,
        ];
        mark_rework(&mut set);
        assert_eq!(
            set[0].files_reworked, 1,
            "a.rs was fixed, b.rs was just changed"
        );
    }

    /// A fix session reopening a file within a day charges whoever wrote it
    /// last — once — and nobody else. Ordinary work reopening it charges no
    /// one, because editing the same central file every day is normal.
    #[test]
    fn rework_is_charged_to_the_last_writer_before_a_fix() {
        let mut set = vec![
            edited_at(Task::Coding, &[("/a.rs", "2026-01-01T09:00:00Z")]),
            edited_at(Task::Coding, &[("/a.rs", "2026-01-01T10:00:00Z")]),
            edited_at(Task::Coding, &[("/b.rs", "2026-01-01T10:00:00Z")]),
            edited_at(
                Task::Debugging,
                &[
                    ("/a.rs", "2026-01-01T12:00:00Z"),
                    ("/c.rs", "2026-01-01T12:00:00Z"),
                ],
            ),
            edited_at(Task::Debugging, &[("/a.rs", "2026-01-01T13:00:00Z")]),
            // Two days on: too late to blame anyone.
            edited_at(Task::Debugging, &[("/b.rs", "2026-01-03T12:00:00Z")]),
        ];
        mark_rework(&mut set);
        let charged: Vec<u64> = set.iter().map(|a| a.files_reworked).collect();
        // The second writer of a.rs is charged once although two fixes
        // followed — the second fix blames the first fix, which wrote a.rs
        // last before it.
        assert_eq!(charged, [0, 1, 0, 1, 0, 0]);
    }

    /// Codex reports the whole prompt with the cached part already inside it,
    /// where Claude reports the uncached remainder alongside it. Adding every
    /// field together counted Codex's cached tokens twice and put its cache hit
    /// rate over 50% before it had read anything.
    #[test]
    fn cached_input_is_not_counted_twice_for_codex() {
        let codex = SessionData {
            tokens: crate::session::Tokens {
                input_total: 1000,
                cached_input: 900,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(input_split(Provider::Codex, &codex), (900, 1000));

        let claude = SessionData {
            tokens: crate::session::Tokens {
                input: 100,
                cache_read: 900,
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(input_split(Provider::Claude, &claude), (900, 1000));
    }

    /// Ranking testing above coding by call volume filed 22 of 57 sessions on
    /// this repository as Testing. A category that swallows the work it was
    /// meant to distinguish is worse than not having it.
    #[test]
    fn a_session_that_edits_and_tests_is_coding() {
        let calls = vec![
            call("Edit", "/a.rs", "01"),
            call("Bash", "cargo test", "02"),
            call("Bash", "cargo test", "03"),
            call("Bash", "cargo test", "04"),
        ];
        let data = data_of(&calls);
        assert_eq!(classify(&data, &timeline(&data)), Task::Coding);

        // Running tests and changing nothing is the only case where the two are
        // actually distinct.
        let only_running = vec![call("Bash", "cargo test", "01")];
        let data = data_of(&only_running);
        assert_eq!(classify(&data, &timeline(&data)), Task::Testing);
    }

    /// The bug this closes. A session driven entirely through the shell edits
    /// with `sed -i`, a heredoc and a redirect and never touches the Edit tool.
    /// cctop counted no edits, filed 194 Bash calls as Testing, and then
    /// reported the session as having "spent over $0.50 and edited no file" —
    /// 19 sessions and $216 of ordinary work on this machine.
    #[test]
    fn a_shell_that_writes_a_file_is_editing() {
        for writing in [
            "sed -i 's/a/b/' src/main.rs",
            "cat > /tmp/x.py <<'EOF'",
            "cargo build 2>&1 > build.log",
            "echo hi >> notes.md",
            "grep -r foo . | tee results.txt",
            "mv old.rs new.rs",
            "cp a.toml b.toml",
        ] {
            assert!(bash_writes(writing), "should count as a write: {writing}");
        }

        // The commands a session runs constantly and that write nothing worth
        // counting. A false positive here accuses somebody of editing a file
        // they only looked at.
        for reading in [
            "cargo test 2>&1 | tail -3",
            "grep -n foo src/main.rs | head -20",
            "ls -la >/dev/null 2>&1",
            "awk '{print $1}' file | sort | uniq -c",
            "git status --short",
            "if [ $a > $b ]; then echo yes; fi",
        ] {
            assert!(
                !bash_writes(reading),
                "should not count as a write: {reading}"
            );
        }
    }

    /// A session that only ever wrote through the shell still edited, and the
    /// two are counted in one place so nothing has to remember both.
    #[test]
    fn shell_writes_count_as_having_changed_something() {
        let shell_only = analysed(
            Provider::Claude,
            &[
                call("Bash", "sed -i 's/a/b/' src/main.rs", "01"),
                call("Bash", "cargo test", "02"),
            ],
        );
        assert_eq!(shell_only.edits, 0, "no edit tool was used");
        assert_eq!(shell_only.bash_writes, 1);
        assert_eq!(shell_only.wrote(), 1, "but something was written");
        assert_eq!(
            shell_only.task,
            Task::Coding,
            "and that makes it coding, not testing"
        );

        // The path-based figures stay honest: a shell write carries no file
        // name, so it must not inflate the one-shot rate.
        assert_eq!(shell_only.files_edited, 0);
    }

    /// A session left open overnight did not work overnight. A long pause
    /// counts only while the call before it was still running.
    #[test]
    fn idle_time_is_not_agent_time() {
        let at = |name: &str, ts: &str, dur: Option<i64>| {
            let (n, mut d) = call(name, "x", ts);
            d.dur_ms = dur;
            (n, d)
        };
        let calls = [
            at("Read", "2026-01-01T10:00:00Z", None),
            // Thirty seconds of work.
            at("Edit", "2026-01-01T10:00:30Z", None),
            // Then a night away from the keyboard.
            at("Bash", "2026-01-01T22:00:00Z", Some(600_000)),
            // A ten-minute build is the agent working, however long it is.
            at("Edit", "2026-01-01T22:10:00Z", None),
        ];
        let data = data_of(&calls);
        assert_eq!(active_ms(&timeline(&data)), 30_000 + 600_000);
    }

    /// Either separator: a path in a transcript is spelled however the harness
    /// that wrote it spelled it.
    #[test]
    fn junk_is_recognised_with_either_separator() {
        assert!(is_junk("node_modules/react/index.js"));
        assert!(is_junk(r"C:\repo\node_modules\react\index.js"));
        assert!(is_junk("/home/x/repo/.git/config"));
        assert!(!is_junk("/home/x/repo/src/target_picker.rs"));
        assert!(!is_junk("/home/x/dist_report.md"));
    }
}
