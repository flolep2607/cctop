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
//! Everything here is derived from what the transcript already recorded. There
//! are no model calls, no heuristic that needs the network, and nothing that
//! writes: both commands read and print.

pub mod compare;
pub mod optimize;

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

fn is_edit(tool: &str) -> bool {
    EDIT_TOOLS.iter().any(|t| t.eq_ignore_ascii_case(tool))
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
    pub edits: u64,
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
    /// The per-tool history hit its cap, so every count here is a floor.
    pub truncated: bool,
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

/// Which model to credit a session to: the one that cost the most.
///
/// A session that switched models mid-way is credited entirely to its dominant
/// one, because the tool calls cannot be attributed per model — the transcript
/// records which model billed a request, not which model asked for a given
/// call. `compare` says so on its output rather than pretending otherwise.
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
        } else if tool.eq_ignore_ascii_case("Bash") || tool.eq_ignore_ascii_case("shell") {
            let cmd = detail.d.to_ascii_lowercase();
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

/// Reduce one session's freshly-parsed data to an [`Analysis`].
pub fn analyse(session: &Session, data: &SessionData) -> Analysis {
    let timeline = timeline(data);
    let task = classify(data, &timeline);
    let (cached, billed_in) = input_split(session.provider, data);

    let mut out = Analysis {
        provider: session.provider,
        label: session.abbrev_label.clone(),
        model: dominant_model(data),
        cost: data.costs.total,
        cost_available: session.cost_available,
        task,
        calls: data.metrics.tool_count,
        errors: data.metrics.tool_errors,
        records_outcomes: session.provider.records_tool_outcomes(),
        cache_read: cached,
        input_total: billed_in,
        edits: 0,
        reads: 0,
        files_edited: 0,
        files_one_shot: 0,
        read_paths: HashSet::new(),
        junk_reads: 0,
        junk_tokens: 0,
        reread_tokens: 0,
        rereads: 0,
        truncated: false,
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
    let mut edit_order: HashMap<&str, Vec<usize>> = HashMap::new();

    for (i, (tool, detail)) in timeline.iter().enumerate() {
        let path = detail.d.as_str();
        if is_edit(tool) {
            out.edits += 1;
            edit_order.entry(path).or_default().push(i);
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

    // A file is one-shot when its edits form a single run with no other tool
    // call in between. An intervening call is what makes a second edit a
    // *retry* — the agent looked at something and came back — where two edits
    // in a row are one turn writing twice.
    for positions in edit_order.values() {
        out.files_edited += 1;
        let contiguous = positions.windows(2).all(|w| w[1] == w[0] + 1);
        if contiguous {
            out.files_one_shot += 1;
        }
    }

    out
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
    sessions
        .par_iter()
        .map(|s| {
            let data = store.session_data_fresh(s);
            analyse(s, &data)
        })
        .collect()
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
    analyses
        .iter()
        .filter(|a| provider.is_none_or(|p| a.provider == p))
        .collect()
}

pub const HELP: &str = "\
cctop optimize — what your sessions spent and did not get back
cctop compare  — how each model behaved on the work you gave it

USAGE:
  cctop optimize [--provider NAME] [--json]
  cctop compare  [--provider NAME] [--json]

Both re-read every transcript rather than using the session cache, because the
individual tool calls are the thing they reason about and those are never
cached. Expect them to take a few seconds on a large machine.

OPTIONS:
  --provider NAME  Only this harness: claude, codex, cursor, gemini, opencode,
                   pi, windsurf.
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
            other => {
                eprintln!("cctop {which}: unexpected argument `{other}`; see --help");
                return 2;
            }
        }
    }

    let analyses = scan(Plan::Retail);
    let selected = only(&analyses, provider);

    match (which, json) {
        ("optimize", false) => print!("{}", optimize::report(&selected)),
        ("compare", false) => print!("{}", compare::report(&selected)),
        ("optimize", true) => println!("{}", optimize::as_json(&selected)),
        (_, true) => println!("{}", compare::as_json(&selected)),
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

    /// The distinction the whole one-shot rate rests on. Editing a file, going
    /// away to run something, and editing it again is a retry; editing two
    /// different files in a row is progress and must not be counted as one.
    #[test]
    fn a_retry_is_the_same_file_edited_after_looking_elsewhere() {
        let retried = analysed(
            Provider::Claude,
            &[
                call("Edit", "/a.rs", "01"),
                call("Bash", "cargo test", "02"),
                call("Edit", "/a.rs", "03"),
            ],
        );
        assert_eq!(retried.files_edited, 1);
        assert_eq!(
            retried.files_one_shot, 0,
            "the same file, twice, is a retry"
        );

        let progress = analysed(
            Provider::Claude,
            &[
                call("Edit", "/a.rs", "01"),
                call("Bash", "cargo test", "02"),
                call("Edit", "/b.rs", "03"),
            ],
        );
        assert_eq!(progress.files_edited, 2);
        assert_eq!(
            progress.files_one_shot, 2,
            "two different files is two first attempts, not a retry"
        );

        // Two edits in a row are one turn writing twice, not a second attempt:
        // nothing was learned in between.
        let burst = analysed(
            Provider::Claude,
            &[call("Edit", "/a.rs", "01"), call("Edit", "/a.rs", "02")],
        );
        assert_eq!(burst.files_one_shot, 1);
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

    /// A generated directory is no less generated on Windows.
    #[test]
    fn junk_is_recognised_with_either_separator() {
        assert!(is_junk("node_modules/react/index.js"));
        assert!(is_junk(r"C:\repo\node_modules\react\index.js"));
        assert!(is_junk("/home/x/repo/.git/config"));
        assert!(!is_junk("/home/x/repo/src/target_picker.rs"));
        assert!(!is_junk("/home/x/dist_report.md"));
    }
}
