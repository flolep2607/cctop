//! Terminal UI: application state, the worker thread, and the event loop.

pub mod columns;
mod dirs;
mod input;
pub mod menu;
mod modals;
pub mod panels;
pub mod render;
pub mod spark;
mod table;
pub mod tabs;
pub mod theme;

use crate::cache::UiPrefs;
use crate::cli::Args;
use crate::loader::{Loader, Stats};
use crate::pricing::{Plan, Provider};
use crate::quota::Quota;
use crate::session::{Session, SessionData};
use columns::ColumnId;
use ratatui::crossterm::cursor::Show;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, Event,
};
use ratatui::crossterm::execute;
use spark::History;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

/// Baseline gap between usage checks.
///
/// Quota moves slowly, and the endpoints throttle aggressively — a 30s poll was
/// enough to earn a sustained 429 with a ~15 minute `retry-after`. When a
/// provider asks for longer, `retry_delay_secs` honours that instead.
const QUOTA_INTERVAL_SECS: u64 = 300;

/// How often the poller wakes to see whether any provider is due.
const QUOTA_TICK: Duration = Duration::from_secs(10);

/// Gap between full directory walks.
///
/// Only a walk can notice a session that didn't exist before, and it costs one
/// `stat` per transcript ever recorded — thousands of them, nearly all belonging
/// to sessions that ended long ago. A filesystem watch reports creations and
/// removals as they happen, so this is the safety net for whatever the watch
/// misses (or for when no watch could be established at all) rather than the way
/// new sessions are normally found. `r` still forces one immediately.
const FULL_WALK_INTERVAL: Duration = Duration::from_secs(60);

/// Gap between walks while a created file has yet to become a session.
///
/// Short, because this is the window in which a session the user just started is
/// missing from the table; bounded, because the walk is the expensive one and a
/// file may sit there for a while before the model first answers.
const PENDING_WALK_INTERVAL: Duration = Duration::from_secs(3);

/// How long typing has to pause before the query is scanned for.
///
/// A scan reads every transcript on disk, so it waits for a word rather than
/// chasing each character of one. Short enough that finishing a word and
/// looking up finds the results already there.
const SCAN_DEBOUNCE: Duration = Duration::from_millis(300);

/// How long a freshly launched agent is given before a handoff brief is typed
/// at it.
///
/// Only reached by a harness that takes no opening prompt on its command line —
/// everything in [`handoff::opening_argv`](crate::handoff::opening_argv) is
/// handed the brief as an argument instead, because no delay is long enough to
/// win that race reliably. Too short and the line is lost; too long and the user
/// is left looking at an idle agent wondering whether the handoff worked.
const HANDOFF_SETTLE: Duration = Duration::from_secs(3);

/// Shortest query worth reading every transcript for.
const MIN_SCAN_CHARS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    List,
    Help,
    Search,
    SortBy,
    AgeFilter,
    /// Confirming deletion of the selected session.
    DeleteConfirm,
    /// Explaining why a running session can't be deleted.
    DeleteBlocked,
    /// Confirming termination of the selected live session.
    KillConfirm,
    /// Confirming that a session already running elsewhere should be resumed
    /// anyway, which puts a second agent on the same transcript.
    ResumeConfirm,
    /// Confirming a quit that would take the hosted agent down with it.
    QuitConfirm,
    /// Explaining why a live session cannot be terminated locally.
    KillBlocked,
    /// Confirming a batch action over all marked sessions.
    BatchConfirm,
    /// Explaining why a batch delete couldn't proceed (a marked session is running).
    BatchDeleteBlocked,
    /// Explaining why a batch kill couldn't proceed (a marked session has no root PID).
    BatchKillBlocked,
    /// Numeric input for the cost floor filter.
    CostFilter,
    /// Text input typed into the selected session's rmux pane.
    SendKeys,
    /// Picking which agent a new tab or split should run.
    Launch,
    /// Everything that can be done to the selected row, in one list.
    RowMenu,
    /// Typing the directory the launcher's pick will start in. Drawn as the
    /// launcher with its `in` line in an editable state, so the list of agents
    /// stays visible while the path is being changed.
    LaunchCwd,
    /// The agent-integration panel: what is installed where, and whether the
    /// agents are actually reporting in.
    Hooks,
    /// Offering to install rmux, a launch having found it missing.
    TmuxInstall,
    /// Typing a new name for a workspace tab, opened by right-clicking it.
    RenameTab,
    /// The browser panel: whether this cctop is serving its table to one, on
    /// what links, and whether they leave the machine.
    Serve,
}

/// Everything [`App::open_tab`] needs beyond the command itself.
///
/// A struct rather than six more parameters: they had already outgrown a
/// readable call, and at a call site `verb: "Attached to"` says what the
/// fifth positional string never did.
struct NewTab<'a> {
    cwd: Option<std::path::PathBuf>,
    /// The thing being opened, as a status message would name it.
    what: &'a str,
    own: tabs::Own,
    /// What happened, for the status line: resumed, reattached, attached.
    verb: &'a str,
    /// The session this pane resumes, when it resumes one.
    resumed: Option<String>,
    /// What to call the tab, when the command would call it badly.
    ///
    /// A launch is its command — a `claude` tab is called `claude`, which is
    /// both true and short. A *resume* is not: its command carries the session
    /// id, and `claude --resume 4ebf1ab4-2ef8-4fb2-a7d5-d445b5026dc9` is 45
    /// characters of tab bar whose only variable part is a uuid nobody reads.
    /// The caller that knows which session this is passes its name instead.
    label: Option<String>,
    /// The account this pane runs as, when it is one cctop chose — so the pane
    /// border reports that account's limits rather than the default's.
    profile: Option<String>,
}

/// How much of a session's name a resumed tab's label carries.
///
/// The bar elides labels itself once it is crowded, but only then — with two
/// tabs open there is room for a whole title, and a title can be a sentence.
/// This is the point past which a tab name stops identifying the session and
/// starts being its own paragraph.
pub(super) const TAB_LABEL_CHARS: usize = 24;

/// A launch that stopped to ask about rmux, and how to pick it up again.
///
/// The launch is re-run from the top rather than resumed mid-way, because
/// answering the question changes the first thing it decides — where the agent
/// is going to live. Both entry points derive everything they need from state
/// the modal does not touch (the table selection, the launcher's snapshot), so
/// running them twice starts one agent, not two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deferred {
    /// [`App::resume_selected`], stopped at the ownership decision.
    Resume,
    /// [`App::launch_selected`], stopped at the same place.
    Launch,
}

/// Where the agent picked in `Mode::Launch` ends up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchInto {
    /// A tab of its own.
    Tab,
    /// Alongside the panes already in the current tab, arranged the given way.
    Split { stacked: bool },
}

/// The pending batch action shown in `Mode::BatchConfirm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchKind {
    Delete,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgeFilter {
    Day,
    Week,
    Month,
}

impl AgeFilter {
    pub fn max_age_ms(&self) -> i64 {
        match self {
            AgeFilter::Day => 86_400_000,
            AgeFilter::Week => 604_800_000,
            AgeFilter::Month => 2_592_000_000,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            AgeFilter::Day => "Last 24 hours",
            AgeFilter::Week => "Last 7 days",
            AgeFilter::Month => "Last 30 days",
        }
    }

    pub fn short(&self) -> &'static str {
        match self {
            AgeFilter::Day => "1 day",
            AgeFilter::Week => "1 week",
            AgeFilter::Month => "1 month",
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            AgeFilter::Day => "1d",
            AgeFilter::Week => "1w",
            AgeFilter::Month => "1mo",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "1d" => Some(AgeFilter::Day),
            "1w" => Some(AgeFilter::Week),
            "1mo" => Some(AgeFilter::Month),
            _ => None,
        }
    }
}

/// Options offered by the age-filter modal, "no filter" last.
pub const AGE_OPTIONS: [Option<AgeFilter>; 4] = [
    Some(AgeFilter::Day),
    Some(AgeFilter::Week),
    Some(AgeFilter::Month),
    None,
];

// ---------------------------------------------------------------------------
// Worker protocol
// ---------------------------------------------------------------------------

enum Request {
    Refresh,
    /// Update the running sessions without re-walking every provider directory.
    RefreshLive,
    /// Extract full data for one session, to populate the bottom panels.
    Data(Box<Session>),
    Delete(Box<Session>),
    Terminate {
        session_key: String,
        pid: u32,
    },
    /// Type a line into the terminal hosting a live session.
    SendKeys {
        pid: u32,
        text: String,
    },
    /// Look for `query` inside every listed session's transcript.
    Scan {
        query: String,
        targets: Vec<crate::session::search::Target>,
    },
    Shutdown,
}

enum Response {
    /// Cheap discovery result, shown before transcript extraction completes.
    Discovered(Vec<Session>),
    /// One row whose transcript has finished loading.
    Annotated(Box<Session>),
    Sessions(Box<(Vec<Session>, Stats)>),
    /// Only the rows that moved during a light refresh, plus recomputed totals.
    /// Shipping these instead of the whole table is the point of the light path:
    /// copying thousands of rows back every couple of seconds is the cost being
    /// avoided.
    LiveRows(Box<(Vec<Session>, Stats)>),
    Data(String, Box<SessionData>),
    Quota(Box<Quota>),
    /// Pricing landed, so cached costs are stale and a reload is due.
    PricingReady,
    /// A newer release exists. Reported once; cctop never updates itself.
    UpdateAvailable(String),
    Terminated {
        session_key: String,
        result: Result<(), String>,
    },
    Deleted {
        session_key: String,
        result: Result<(), String>,
    },
    KeysSent {
        result: Result<(), String>,
    },
    /// One remote machine's snapshot, or why it could not be read.
    Remote {
        host: String,
        snapshot: crate::fleet::Snapshot,
    },
    /// A finished transcript scan: session key -> the text around its match.
    /// The query comes back with it, because the user has usually typed more by
    /// the time a scan over thousands of transcripts lands.
    Scanned {
        query: String,
        hits: HashMap<String, String>,
    },
}

/// Remembered scan results, keyed by session and query. `None` is a remembered
/// *miss*, which is the answer worth caching most: a miss costs a full read of
/// the transcript, a hit usually stops early.
type ScanCache = HashMap<(String, String), Option<String>>;

/// Entries kept before the scan cache is dropped wholesale.
///
/// Reached only by someone who has run many distinct queries over many
/// sessions; forgetting everything then costs one re-scan rather than the
/// bookkeeping an eviction policy would need for a cache this cheap to refill.
const MAX_SCAN_CACHE: usize = 20_000;

/// Search every target's transcript for `needle`, in parallel.
///
/// Running sessions are never cached: their transcripts grow, so today's "not
/// found" is not tomorrow's, and the one case where a stale answer is most
/// visible is the session the user is watching right now.
fn scan(
    cache: &mut ScanCache,
    targets: &[crate::session::search::Target],
    needle: &str,
) -> HashMap<String, String> {
    use rayon::prelude::*;
    let found: Vec<(&crate::session::search::Target, Option<String>)> = targets
        .par_iter()
        .map(|target| {
            let memo = (!target.running)
                .then(|| cache.get(&(target.key.clone(), needle.to_string())))
                .flatten();
            match memo {
                Some(remembered) => (target, remembered.clone()),
                None => (
                    target,
                    crate::session::search::find(target, needle).map(|hit| hit.snippet),
                ),
            }
        })
        .collect();

    if cache.len() + found.len() > MAX_SCAN_CACHE {
        cache.clear();
    }
    let mut hits = HashMap::new();
    for (target, snippet) in found {
        if !target.running {
            cache.insert((target.key.clone(), needle.to_string()), snippet.clone());
        }
        if let Some(snippet) = snippet {
            hits.insert(target.key.clone(), snippet);
        }
    }
    hits
}

/// Owns the `Loader` and does all filesystem and parsing work off the UI thread,
/// so a slow scan can never stall input or rendering.
fn spawn_worker(
    plan: Plan,
    rx: Receiver<Request>,
    tx: Sender<Response>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut loader = Loader::new();
        let mut sent_initial_discovery = false;
        // The last full walk's rows, kept so a light refresh has something to
        // update in place.
        let mut live_rows: Vec<Session> = Vec::new();
        // What earlier transcript scans found, so refining a query re-reads only
        // what it has to. See `scan`.
        let mut scans: ScanCache = HashMap::new();
        while let Ok(req) = rx.recv() {
            match req {
                Request::Refresh => {
                    // Row-by-row publishing exists so the first table isn't
                    // withheld for the slowest transcript. Later refreshes end
                    // with a `Sessions` payload that replaces everything anyway,
                    // so streaming them too only buys a redundant repaint — at the
                    // cost of one message and one table scan per session, per
                    // refresh, forever.
                    let first_load = !sent_initial_discovery;
                    sent_initial_discovery = true;
                    let sessions = loader.load_progressive(
                        plan,
                        // Only the first table has someone waiting for it.
                        first_load,
                        |sessions| {
                            if first_load {
                                let _ = tx.send(Response::Discovered(sessions.to_vec()));
                            }
                        },
                        |session| {
                            if first_load {
                                let _ = tx.send(Response::Annotated(Box::new(session.clone())));
                            }
                        },
                    );
                    let stats = crate::loader::compute_stats(&sessions);
                    // The light path needs its own copy to carry forward; one clone
                    // per full walk replaces one per refresh.
                    live_rows = sessions.clone();
                    if tx
                        .send(Response::Sessions(Box::new((sessions, stats))))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::RefreshLive => {
                    if live_rows.is_empty() {
                        // Nothing walked yet, so there is nothing to update.
                        continue;
                    }
                    let moved = loader.refresh_live(plan, &mut live_rows);
                    let stats = crate::loader::compute_stats(&live_rows);
                    if tx
                        .send(Response::LiveRows(Box::new((moved, stats))))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Data(session) => {
                    // The open panels are the one view where staleness shows, so
                    // this path never accepts a backed-off entry.
                    let data = loader.store().session_data_fresh(&session);
                    if tx
                        .send(Response::Data(session.key(), Box::new(data)))
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Delete(session) => {
                    let result = match session.provider {
                        Provider::Claude => crate::session::claude::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Codex => crate::session::codex::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Cursor => crate::session::cursor::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Gemini => crate::session::gemini::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::OpenCode => crate::session::opencode::delete(&session)
                            .map_err(|error| error.to_string()),
                        Provider::Pi => {
                            crate::session::pi::delete(&session).map_err(|error| error.to_string())
                        }
                        Provider::Windsurf => crate::session::windsurf::delete(&session)
                            .map_err(|error| error.to_string()),
                    };
                    if result.is_ok() {
                        loader.store().evict(&session);
                    }
                    if tx
                        .send(Response::Deleted {
                            session_key: session.key(),
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Request::Terminate { session_key, pid } => {
                    let result = crate::proc::terminate(pid);
                    if tx
                        .send(Response::Terminated {
                            session_key,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Request::SendKeys { pid, text } => {
                    let result = crate::inject::send_line(pid, &text);
                    if tx.send(Response::KeysSent { result }).is_err() {
                        break;
                    }
                }
                Request::Scan { query, targets } => {
                    let needle = query.to_ascii_lowercase();
                    let hits = loader.gently(|| scan(&mut scans, &targets, &needle));
                    if tx.send(Response::Scanned { query, hits }).is_err() {
                        break;
                    }
                }
                Request::Shutdown => break,
            }
        }
        loader.store().save();
    })
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

/// One line of the table: a session, or a subagent shown beneath its parent.
///
/// Rows rather than session indices, because an expanded session occupies
/// several lines and everything that walks the table — scrolling, the cursor,
/// search, the mouse — has to agree on how many there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Session(usize),
    /// `index` is into the parent's own `subagents`, which is the only place
    /// they exist; they are not sessions and have no entry in `sessions`.
    Subagent {
        parent: usize,
        index: usize,
    },
}

impl Row {
    /// The session this row belongs to, which for a child is its parent.
    ///
    /// Actions are addressed to sessions — a subagent has no process to signal
    /// and no transcript of its own to delete — so every row resolves to one.
    pub fn session(self) -> usize {
        match self {
            Row::Session(i) => i,
            Row::Subagent { parent, .. } => parent,
        }
    }

    pub fn is_subagent(self) -> bool {
        matches!(self, Row::Subagent { .. })
    }
}

pub struct App {
    pub sessions: Vec<Session>,
    /// Whether a first load has landed. Discovery is asynchronous, so an empty
    /// `sessions` means "still looking" until this flips — and "you have none"
    /// is a very different thing to tell someone.
    pub loaded: bool,
    /// The table's lines, after filtering, sorting and expansion.
    pub visible: Vec<Row>,
    /// Subagents whose own `SubagentStop` has arrived.
    ///
    /// Held for the run rather than per session: this is the only word cctop
    /// gets that a background subagent has finished, and the transcript it would
    /// otherwise be inferred from cannot say it. Ids are unique per run, so the
    /// set does not need scoping to a parent.
    pub finished_agents: std::collections::HashSet<String>,
    /// Keys of the sessions showing their subagents.
    ///
    /// Keyed rather than indexed because `sessions` is rebuilt wholesale on
    /// every walk, which would leave an index pointing at whatever sorted into
    /// that slot next.
    pub expanded: std::collections::HashSet<String>,
    pub stats: Stats,
    pub selected: usize,
    pub scroll: usize,
    pub plan: Plan,
    pub mode: Mode,

    pub sort_col: ColumnId,
    pub sort_asc: bool,
    pub sortby_cursor: usize,

    pub search: String,
    /// Search the transcripts as well as the columns.
    ///
    /// Off by default, and deliberately: the metadata filter answers instantly
    /// from memory, while this one reads every transcript on disk. Turning it on
    /// is how you say the reading is worth it.
    pub search_content: bool,
    /// The query [`App::scan_hits`] belongs to.
    ///
    /// Kept because a scan over thousands of transcripts outlives the keystroke
    /// that started it: hits for "flyw" must not be applied to "flywheel".
    pub scan_query: String,
    /// Session key -> the transcript text around its match.
    pub scan_hits: HashMap<String, String>,
    /// A scan is out with the worker.
    pub scanning: bool,
    /// When the query last changed, so a burst of typing costs one scan.
    scan_typed_at: Option<Instant>,
    /// Queries run before, newest first, walked with ↑/↓ in the filter modal.
    pub search_history: Vec<String>,
    /// Where ↑/↓ has walked to in `search_history`, and the query that was
    /// being typed before the walk started, so ↓ can put it back.
    history_cursor: Option<(usize, String)>,
    pub age_filter: Option<AgeFilter>,
    pub age_cursor: usize,
    pub live_only: bool,

    /// Session keys the user has marked (Space) for a batch action.
    pub marked: HashSet<String>,
    /// Sessions whose deletion has been accepted by the worker but not yet
    /// completed. They remain visible until the provider reports success.
    pub deleting: HashSet<String>,
    /// The action pending confirmation in `Mode::BatchConfirm`.
    pub batch: BatchKind,
    /// Follow mode: keep the selected row centered.
    pub follow: bool,
    /// Seconds between automatic refreshes (adjustable live with +/-/=).
    pub refresh_secs: f64,
    /// Only show sessions whose total cost reaches this floor.
    pub cost_floor: f64,
    /// Which profile the launcher will start each harness under, as an index
    /// into that harness's [`crate::config::profiles_for`] list.
    ///
    /// Per harness rather than one index: the launcher cursor moves between
    /// `claude` and `codex`, and an index is only meaningful against the list
    /// it came from — carrying one across would point at whichever account
    /// happened to sit in that position.
    pub launch_profile: HashMap<Provider, usize>,
    /// Which entry of the row menu is under the cursor.
    ///
    /// Only ever points at an entry that can run: the menu draws the blocked
    /// ones to explain them and never lets the cursor rest there. See
    /// [`menu::step`].
    pub menu_cursor: usize,
    /// Raw digits being typed into the cost-floor modal.
    pub cost_input: String,
    /// The directory being typed into the launcher, spelled as the user is
    /// spelling it — `~` and all, expanded only when it is accepted.
    pub launch_cwd_input: String,
    /// Set when the typed directory does not name one, so the field can say so
    /// where it is rather than behind the modal that covers the status line.
    pub launch_cwd_bad: bool,
    /// Directories agents are already known to have run in, newest first, as of
    /// the moment the field opened.
    ///
    /// A snapshot for the same reason the launcher's list of agents is one: a
    /// list that reshuffles under the cursor while a walk lands means Enter
    /// takes a directory other than the one highlighted.
    pub launch_cwd_known: Vec<std::path::PathBuf>,
    /// What the field is currently offering for what has been typed. Every one
    /// of them is a directory that exists, which is what lets a picked
    /// suggestion skip the check the typed path gets.
    pub launch_cwd_hits: Vec<std::path::PathBuf>,
    /// The suggestion under the cursor, when the cursor has left the text.
    /// `None` means the field is being typed in, and Enter takes what is typed.
    pub launch_cwd_pick: Option<usize>,
    /// Line being typed into the selected session's terminal.
    pub send_input: String,
    /// The new name being typed for a tab, and which tab it is for.
    ///
    /// The title as it stood when the rename opened is kept alongside the
    /// index because the bar moves under a modal: a tab whose agent exits is
    /// retired mid-typing and every index after it shifts down one. Checking
    /// the title back means a rename either lands on the tab it was aimed at or
    /// is dropped, rather than renaming whichever tab slid into the slot.
    pub rename_input: String,
    pub rename_tab: usize,
    pub rename_was: String,
    /// Table viewport height (rows), recorded during draw so Ctrl+U/Ctrl+D can
    /// page by half a screen.
    pub list_height: u16,

    /// Columns the user has hidden outright (`$CCTOP_COLUMNS_HIDE`). These win
    /// over the automatic width-based dropping in [`columns::visible_columns`].
    pub hidden_columns: Vec<ColumnId>,

    /// Scroll offset of the help overlay, which is taller than most terminals.
    pub help_scroll: u16,
    /// Last computed bottom of the help overlay, recorded during draw.
    pub help_max_scroll: u16,

    pub bottom_tab: usize,
    pub panel_data: Option<SessionData>,
    panel_key: String,
    /// `last_active` of the session when its panel data was requested, so an
    /// append can be told apart from an unchanged session.
    panel_stamp: String,
    pub info_scroll: u16,
    pub cost_scroll: u16,
    pub config_scroll: u16,
    pub proc_scroll: u16,
    pub context_scroll: u16,
    pub subagent_scroll: u16,
    pub tool_scroll: u16,
    /// Pin the tool log to its newest entry. Tool Activity is an append-only
    /// feed, so following the tail is the useful default; scrolling up releases
    /// the pin, and scrolling back to the bottom restores it.
    pub tool_follow: bool,
    /// Last computed maximum scroll for whichever bottom panel was drawn,
    /// recorded during draw because only the renderer knows how many lines the
    /// panel produced and how tall it ended up. Without it a key handler can
    /// clamp the top but not the bottom, and scrolling past the end banks
    /// invisible offsets that then have to be scrolled back through.
    pub panel_max_scroll: u16,
    pub tool_tab: usize,
    pub tool_live_only: bool,
    /// Show each edit's diff inline beneath its row.
    pub tool_show_diff: bool,
    /// Invocation whose full argument is expanded, keyed by `detail_key`.
    pub tool_expanded: Option<String>,
    /// Which invocation owns each rendered line, so a click maps to an entry.
    pub tool_owners: Vec<Option<String>>,
    pub subagent_sort: (panels::SubagentSort, bool),

    pub cpu_history: HashMap<String, History>,
    pub mem_history: HashMap<String, History>,
    pub global_cpu: History,
    pub global_spend: History,

    pub quota: Quota,
    /// Version of a newer published release, when one exists.
    pub update_available: Option<String>,
    pub status: Option<(String, Instant)>,
    /// When cctop started, used by the tool-activity "live" filter.
    pub started_at: String,
    /// The same moment as an `Instant`, which is what the tab-bar blink is
    /// phased against — a wall clock can jump, and a blink that stutters when
    /// NTP steps the clock looks like a bug.
    started: Instant,

    /// Bell and desktop notifications, and who rang last.
    pub notify: crate::notify::Notifier,

    /// The last snapshot from each machine named with `--host`, keyed by the
    /// target as the user spelled it.
    ///
    /// Held apart from `sessions` rather than merged once, because `sessions`
    /// is replaced wholesale by every walk and would drop them; `merge_remotes`
    /// puts the current snapshots back after each replacement.
    pub remotes: HashMap<String, Vec<Session>>,
    /// Machines that failed their last poll, and why.
    ///
    /// Shown rather than logged. A host that has quietly dropped out is worse
    /// than one that was never added: the totals still look complete.
    pub remote_errors: HashMap<String, String>,

    /// Which live sessions are working the same ground, recomputed whenever
    /// rows move. The level also rides on each row so the table can sort by it;
    /// this holds the part only the footer and the Info panel need — who, and
    /// which files.
    pub collisions: crate::collide::Map,

    /// Workspace tabs beyond the dashboard, each holding one or more terminals.
    pub tabs: Vec<tabs::Tab>,
    /// Which tab is on screen: `0` is the dashboard, `1..=tabs.len()` index
    /// `tabs`. Zero-length `tabs` is the ordinary case: the bar still shows the
    /// dashboard and its new-tab button, so the feature is findable.
    pub tab: usize,
    /// When the tab bar was last reconciled against the rmux sessions on this
    /// machine. See [`App::sync_shared_tabs`].
    shared_at: Option<Instant>,
    /// The tab being dragged along the bar, indexed as the bar is: `1..=len`,
    /// and never `0` because the dashboard does not move.
    ///
    /// Set on the press and cleared on the release, which is also what makes the
    /// release the bar's rather than the agent's: a drag that started on the bar
    /// and ended over a pane must not be delivered as a click inside it.
    pub(super) drag_tab: Option<usize>,
    /// What each session's own hooks last said about it, keyed by session id.
    ///
    /// Only sessions whose agent has cctop's hooks installed appear here, so an
    /// absent entry is the ordinary case and means "fall back to the transcript"
    /// rather than "nothing is happening".
    pub hooked: HashMap<String, crate::hook::Reported>,
    /// The integration's state, as of the last time the panel was opened.
    ///
    /// Rebuilt on opening and after every action rather than every frame: it
    /// reads three files off disk and scans a directory, which is nothing to do
    /// once and wasteful to do sixty times a second behind a closed panel.
    pub hooks: Option<crate::hook::Report>,
    /// The socket the agents push their events to. `None` only where there is
    /// none to be had — a non-unix build — in which case every estimate carries
    /// on exactly as it did before hooks existed.
    pub listener: Option<crate::hook::Listener>,
    /// The command highlighted in the launcher.
    pub launch_cursor: usize,
    /// What the launcher is offering, as it was when it opened.
    ///
    /// A snapshot rather than a live look, for correctness before cost: the list
    /// includes agents that can finish while the modal is up, and a list that
    /// reshuffles under a cursor means Enter starts something other than the row
    /// highlighted. It also keeps a `rmux` subprocess out of the draw loop.
    pub launch_offer: Vec<tabs::Choice>,
    /// Where the launcher's pick will go.
    pub launch_into: LaunchInto,
    /// Directory cctop was started in. Fresh tabs start here, rather than in
    /// whichever historical session happens to be selected in the dashboard.
    pub launch_root: Option<std::path::PathBuf>,
    /// Directory a launched agent starts in, captured when the launcher opens.
    /// Splits retain their tab's directory; a handoff deliberately overrides
    /// this with the source session's project.
    pub launch_cwd: Option<std::path::PathBuf>,
    /// The install the rmux offer is currently showing, so the modal draws the
    /// command that will actually run rather than working it out again.
    pub rmux_install: Option<crate::rmux::Install>,
    /// The launch waiting on the rmux question, or on the install it started.
    pub rmux_deferred: Option<Deferred>,
    /// Whether the offer has been turned down. One "no" holds for the run:
    /// asking again on the next tab would make declining rmux cost more than
    /// accepting it, which is a way of not really offering a choice.
    ///
    /// Not persisted — a decision about this machine belongs in whether rmux is
    /// installed on it, and cctop already reads that directly.
    pub rmux_declined: bool,
    /// The server this cctop is running, when it is running one.
    ///
    /// `cctop serve` is the same server with nobody watching the terminal it
    /// took over. Holding it here is what lets the table and the page be up at
    /// once — and, because the dashboard already walks every session, what lets
    /// the page be fed from the rows on screen rather than from a second scan
    /// of the same disk.
    ///
    /// Dropping it revokes the tunnel and stops the listener, so quitting cctop
    /// takes the page with it. That is the same bargain `serve` makes.
    pub serving: Option<crate::serve::Serving>,
    /// Why the last attempt to serve failed, kept for the panel to show.
    ///
    /// A port in use or a tunnel that would not register are both answers
    /// somebody has to read, and a status line that has since been overwritten
    /// by a refresh is not where they can read it.
    pub serve_error: Option<String>,
    /// The quick tunnel a browser share is reached through, once one has been
    /// opened.
    ///
    /// Held for the life of the run rather than per share: every share on this
    /// machine goes to the same rmux daemon on the same loopback port, so one
    /// tunnel serves all of them, and dropping it would revoke the links
    /// already handed out. Started on the first `W` and never on startup —
    /// registering with Cloudflare's edge is a second of network cctop has no
    /// reason to spend on a run where nobody shares anything.
    pub share_tunnel: Option<crate::serve::tunnel::Tunnel>,
    /// The pane running the install, while one is running.
    ///
    /// Watched for two endings: rmux appearing, which releases the deferred
    /// launch into a rmux-backed pane, and the pane going away without it,
    /// which means the install failed and the launch should stop waiting.
    pub rmux_installing: Option<u32>,
    /// A handoff brief waiting for the agent the launcher is about to start.
    ///
    /// Held across the launcher rather than typed at the moment `H` is pressed,
    /// because the agent that will receive it does not exist yet: `H` writes the
    /// brief and opens the launcher, and whichever agent is picked inherits it.
    pub pending_brief: Option<std::path::PathBuf>,
    /// The transcript behind that brief, when the session it describes is one a
    /// second Claude could be resumed onto directly.
    ///
    /// Held beside the brief rather than instead of it: which of the two is
    /// used is not known until an agent has been picked, and every agent but
    /// Claude still needs the brief. See [`crate::handoff::fork`].
    pub pending_fork: Option<std::path::PathBuf>,
    /// A brief handed to an agent that is still starting up, as
    /// `(pid, line, not before)`.
    ///
    /// An agent cannot be typed at until its TUI is reading the keyboard, and
    /// there is no signal for that — a line sent into the first half-second of
    /// startup is swallowed by whatever the harness prints over it. So the line
    /// waits here and the loop delivers it once the agent has had time to draw.
    pub handoff_send: Option<(u32, String, Instant)>,
    /// The agent this cctop launched, as `(pid, label)`.
    ///
    /// Set only for `cctop <agent>`, and only while it is alive — the loop exits
    /// as soon as it is not. It is what `A` goes back to after F12, and the
    /// reason quitting asks first: the agent is on a pty this process owns and
    /// does not survive it.
    pub hosted: Option<(u32, String)>,

    prefs: UiPrefs,
    tx: Sender<Request>,
    pub needs_redraw: bool,
    pub should_quit: bool,
}

impl App {
    fn new(plan: Plan, tx: Sender<Request>) -> Self {
        Self::with_prefs(plan, tx, UiPrefs::load())
    }

    /// Build with explicit preferences.
    ///
    /// Tests use this with `UiPrefs::default()`; going through `new` would load
    /// whatever is on the developer's disk and make results machine-dependent.
    fn with_prefs(plan: Plan, tx: Sender<Request>, prefs: UiPrefs) -> Self {
        let age_filter = prefs
            .inactivity_filter
            .as_deref()
            .and_then(AgeFilter::parse);
        let age_cursor = AGE_OPTIONS
            .iter()
            .position(|o| *o == age_filter)
            .unwrap_or(AGE_OPTIONS.len() - 1);

        App {
            sessions: Vec::new(),
            loaded: false,
            visible: Vec::new(),
            finished_agents: std::collections::HashSet::new(),
            expanded: prefs
                .expanded
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
            stats: Stats::default(),
            selected: 0,
            scroll: 0,
            plan,
            mode: Mode::List,
            // Newest first: `Last` compares reversed, so ascending is most
            // recently active at the top.
            sort_col: ColumnId::Last,
            sort_asc: true,
            sortby_cursor: 0,
            search: String::new(),
            search_content: false,
            scan_query: String::new(),
            scan_hits: HashMap::new(),
            scanning: false,
            scan_typed_at: None,
            search_history: prefs.search_history.clone(),
            history_cursor: None,
            age_filter,
            age_cursor,
            live_only: prefs.live_only,
            marked: HashSet::new(),
            deleting: HashSet::new(),
            batch: BatchKind::Delete,
            follow: false,
            refresh_secs: 2.0,
            cost_floor: prefs.cost_floor,
            // The one used last, or the default when that profile has since
            // gone: a name that no longer resolves must not silently launch
            // under somebody else's account.
            launch_profile: [
                (Provider::Claude, prefs.claude_profile.as_deref()),
                (Provider::Codex, prefs.codex_profile.as_deref()),
            ]
            .into_iter()
            .map(|(provider, remembered)| {
                let at = remembered
                    .and_then(|name| {
                        crate::config::profiles_for(provider)
                            .iter()
                            .position(|p| p.name == name)
                    })
                    .unwrap_or(0);
                (provider, at)
            })
            .collect(),
            menu_cursor: 0,
            cost_input: String::new(),
            send_input: String::new(),
            rename_input: String::new(),
            rename_tab: 0,
            rename_was: String::new(),
            list_height: 0,
            hidden_columns: hidden_columns(&prefs),
            help_scroll: 0,
            help_max_scroll: 0,
            bottom_tab: prefs.bottom_tab.min(panels::TABS.len() - 1),
            panel_data: None,
            panel_key: String::new(),
            panel_stamp: String::new(),
            info_scroll: 0,
            cost_scroll: 0,
            config_scroll: 0,
            proc_scroll: 0,
            context_scroll: 0,
            subagent_scroll: 0,
            tool_scroll: 0,
            tool_follow: true,
            panel_max_scroll: 0,
            tool_tab: 0,
            tool_live_only: prefs.agent_live_filter,
            tool_show_diff: prefs.tool_show_diff,
            tool_expanded: None,
            tool_owners: Vec::new(),
            subagent_sort: (
                panels::SubagentSort::parse(&prefs.subagent_sort_col),
                prefs.subagent_sort_asc,
            ),
            cpu_history: HashMap::new(),
            mem_history: HashMap::new(),
            global_cpu: History::default(),
            global_spend: History::default(),
            quota: Quota::default(),
            notify: crate::notify::Notifier::new(prefs.notify),
            collisions: crate::collide::Map::new(),
            remotes: HashMap::new(),
            remote_errors: HashMap::new(),
            update_available: None,
            status: None,
            started_at: chrono::Utc::now().to_rfc3339(),
            started: Instant::now(),
            prefs,
            tx,
            tabs: Vec::new(),
            tab: 0,
            shared_at: None,
            drag_tab: None,
            hooked: HashMap::new(),
            hooks: None,
            listener: None,
            launch_cursor: 0,
            launch_offer: Vec::new(),
            launch_into: LaunchInto::Tab,
            launch_root: std::env::current_dir().ok(),
            launch_cwd: None,
            launch_cwd_input: String::new(),
            launch_cwd_bad: false,
            launch_cwd_known: Vec::new(),
            launch_cwd_hits: Vec::new(),
            launch_cwd_pick: None,
            rmux_install: None,
            rmux_deferred: None,
            rmux_declined: false,
            rmux_installing: None,
            serving: None,
            serve_error: None,
            share_tunnel: None,
            pending_brief: None,
            pending_fork: None,
            handoff_send: None,
            hosted: None,
            needs_redraw: true,
            should_quit: false,
        }
    }

    /// The highlighted session, if there is one.
    ///
    /// `visible` holds indices into `sessions`, and a refresh replaces
    /// `sessions` before `refilter` rebuilds `visible` — so between those two
    /// steps the indices can outrun the list. Resolving through `get` keeps this
    /// accessor total instead of panicking on that window.
    pub fn selected_session(&self) -> Option<&Session> {
        self.visible
            .get(self.selected)
            .and_then(|row| self.sessions.get(row.session()))
    }

    /// The footer's warning that two live agents have written the same file.
    ///
    /// Only the file-level overlaps. A shared repository is worth the cell in
    /// the `!` column and no more — agents share repositories all day and
    /// nothing has gone wrong yet, whereas two of them writing one file means
    /// one has already lost an edit or is about to.
    ///
    /// Unlike the bell's line this does not clear when you select the row: the
    /// bell reports a moment that has passed, and this reports a state that is
    /// still true whether or not you are looking at it.
    pub fn conflict_footer(&self) -> Option<String> {
        use std::collections::BTreeSet;
        let mut agents = 0;
        let mut files: BTreeSet<&str> = BTreeSet::new();
        for c in self.collisions.values() {
            if c.level != crate::collide::Overlap::File {
                continue;
            }
            agents += 1;
            files.extend(c.files.iter().map(String::as_str));
        }
        let first = files.iter().next()?;
        let more = match files.len() {
            1 => String::new(),
            n => format!(" +{} more", n - 1),
        };
        Some(format!(
            "Conflict: ⚠ {}{more} — {agents} agents have written it",
            crate::util::path_tail(first, 2)
        ))
    }

    /// Put the current remote snapshots back into the table.
    ///
    /// Every wholesale replacement of `sessions` — a full walk, a discovery —
    /// drops the remote rows, because the loader only ever knows about this
    /// machine. Rather than teaching the loader about ssh, the rows are
    /// re-appended here and the totals recomputed over both.
    ///
    /// A no-op with no hosts configured, so the ordinary single-machine run
    /// pays nothing for this.
    pub fn merge_remotes(&mut self) {
        if self.remotes.is_empty() {
            return;
        }
        self.sessions.retain(|s| s.remote.is_none());
        for rows in self.remotes.values() {
            self.sessions.extend(rows.iter().cloned());
        }
        self.stats = crate::loader::compute_stats(&self.sessions);
        self.refilter();
    }

    /// Take the worker's totals, unless remote rows mean they are not the whole
    /// picture. The worker only ever sees this machine.
    fn adopt_stats(&mut self, stats: Stats) {
        self.stats = match self.remotes.is_empty() {
            true => stats,
            false => crate::loader::compute_stats(&self.sessions),
        };
    }

    /// Whether an action that reaches into this machine can apply to a row.
    ///
    /// Returns the refusal to show, or `None` when the row is local. Every
    /// caller is a path that signals a process, deletes a file, or opens a pty,
    /// and each would otherwise do it to whatever sits at the same path here.
    pub fn remote_refusal(session: &Session) -> Option<String> {
        let r = session.remote.as_ref()?;
        Some(format!(
            "{} is on {} — cctop reads other machines but only acts on this one",
            session.display_label(),
            r.host
        ))
    }

    /// The footer's note that a machine is not answering.
    pub fn remote_footer(&self) -> Option<String> {
        let mut hosts: Vec<&str> = self.remote_errors.keys().map(String::as_str).collect();
        hosts.sort();
        let first = hosts.first()?;
        // One host names its reason, which is nearly always the whole fix
        // ("Permission denied", "command not found"). Several would not fit, so
        // they are counted and the panel is where the rest live.
        Some(match hosts.len() {
            1 => format!("{first}: {}", self.remote_errors[*first]),
            n => format!(
                "{n} hosts unreachable ({first}: {})",
                self.remote_errors[*first]
            ),
        })
    }

    /// What one session collides with, with its peers named the way the table
    /// names them.
    pub fn clash_of(&self, session: &Session) -> Option<panels::Clash> {
        let c = self.collisions.get(&session.key())?;
        let peers = c
            .peers
            .iter()
            .filter_map(|key| self.sessions.iter().find(|s| &s.key() == key))
            .map(|s| s.display_label().to_string())
            .collect();
        Some(panels::Clash {
            level: c.level,
            peers,
            files: c.files.clone(),
        })
    }

    /// The highlighted row, whatever kind it is.
    pub fn selected_row(&self) -> Option<Row> {
        self.visible.get(self.selected).copied()
    }

    /// The highlighted subagent, when the cursor is on a child row.
    pub fn selected_subagent(&self) -> Option<&crate::session::Subagent> {
        match self.selected_row()? {
            Row::Session(_) => None,
            Row::Subagent { parent, index } => self.sessions.get(parent)?.subagents.get(index),
        }
    }

    /// Whether the cursor is on a child row.
    ///
    /// The actions that ask this all address the operating system — a signal, a
    /// file, a terminal — and a subagent has none of its own. Refusing is
    /// clearer than silently acting on the parent, which is a live session the
    /// user did not point at.
    pub fn on_subagent(&self) -> bool {
        self.selected_row().is_some_and(Row::is_subagent)
    }

    /// Whether a session is showing its subagents.
    pub fn is_expanded(&self, session: &Session) -> bool {
        self.expanded.contains(&session.key())
    }

    /// Show or hide the selected session's subagents.
    ///
    /// Anchored on the owning session, so pressing it on a child collapses the
    /// parent that child came from rather than doing nothing — the row the
    /// cursor lands on afterwards is then the parent, not a line that no longer
    /// exists.
    pub(super) fn toggle_expanded(&mut self) {
        let Some(row) = self.selected_row() else {
            return;
        };
        let Some(session) = self.sessions.get(row.session()) else {
            return;
        };
        if session.subagents.is_empty() {
            self.set_status("No subagents to show");
            return;
        }
        let key = session.key();
        if !self.expanded.remove(&key) {
            self.expanded.insert(key);
        }
        if row.is_subagent() {
            self.selected = self.selected.saturating_sub(1);
        }
        self.refilter();
        self.save_prefs();
    }

    /// Expand every session that has subagents, or collapse them all.
    ///
    /// Collapses when anything at all is open: with a mixture on screen, "close
    /// them" is the intent that a single key can satisfy unambiguously.
    fn toggle_expanded_all(&mut self) {
        if self.expanded.is_empty() {
            self.expanded = self
                .sessions
                .iter()
                .filter(|s| !s.subagents.is_empty())
                .map(Session::key)
                .collect();
        } else {
            self.expanded.clear();
        }
        self.refilter();
        self.save_prefs();
    }

    fn save_prefs(&mut self) {
        self.prefs.bottom_tab = self.bottom_tab;
        self.prefs.live_only = self.live_only;
        self.prefs.inactivity_filter = self.age_filter.map(|a| a.key().to_string());
        self.prefs.agent_live_filter = self.tool_live_only;
        self.prefs.tool_show_diff = self.tool_show_diff;
        // Sorted so the file does not churn on every save purely because a
        // HashSet iterated in a different order.
        let mut expanded: Vec<String> = self.expanded.iter().cloned().collect();
        expanded.sort();
        self.prefs.expanded = expanded;
        self.prefs.subagent_sort_col = self.subagent_sort.0.key().to_string();
        self.prefs.subagent_sort_asc = self.subagent_sort.1;
        self.prefs.cost_floor = self.cost_floor;
        self.prefs.claude_profile = self
            .chosen_profile(Provider::Claude)
            .map(|p| p.name.clone());
        self.prefs.codex_profile = self.chosen_profile(Provider::Codex).map(|p| p.name.clone());
        self.prefs.notify = self.notify.enabled;
        self.prefs.search_history = self.search_history.clone();
        self.prefs.save();
    }

    fn set_status(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
        self.needs_redraw = true;
    }

    /// Apply filters and sorting, then rebuild the visible index list.
    ///
    /// Selection is tracked by session key rather than row number, so a refresh
    /// that reorders the table doesn't move the cursor off whatever the user was
    /// looking at.
    pub fn refilter(&mut self) {
        let anchor = self.selected_row().map(|r| self.row_key(r));
        let now = chrono::Utc::now();
        let now_ms = now.timestamp_millis();
        let query = self.search.to_ascii_lowercase();

        let mut visible: Vec<usize> = (0..self.sessions.len())
            .filter(|&i| {
                let s = &self.sessions[i];
                if self.live_only && !s.is_running() {
                    return false;
                }
                if let Some(age) = self.age_filter {
                    let ts = if s.last_active.is_empty() {
                        &s.started_at
                    } else {
                        &s.last_active
                    };
                    let within = crate::util::parse_ts(ts)
                        .map(|d| now_ms - d.timestamp_millis() <= age.max_age_ms())
                        .unwrap_or(false);
                    if !within {
                        return false;
                    }
                }
                if !self.matches_query(s, &query) {
                    return false;
                }
                if self.cost_floor > 0.0 {
                    // Sessions with unknown cost are kept: the floor can't say
                    // they're below it. "Included" cost (None) counts as zero.
                    let cost = if s.cost_available {
                        s.total_cost.unwrap_or(0.0)
                    } else {
                        // Unknown — can't disqualify.
                        return true;
                    };
                    if cost < self.cost_floor {
                        return false;
                    }
                }
                true
            })
            .collect();

        let col = self.sort_col;
        let asc = self.sort_asc;
        visible.sort_by(|&a, &b| {
            let ord = columns::compare(col, &self.sessions[a], &self.sessions[b], &now);
            if asc { ord } else { ord.reverse() }
        });

        // Sessions are sorted first, then each expanded one has its children
        // spliced in beneath it: subagents belong to their parent's position in
        // the table, not to the ordering the sort column would give them.
        self.visible = visible
            .into_iter()
            .flat_map(|i| {
                let session = &self.sessions[i];
                let children = if self.expanded.contains(&session.key()) {
                    session.subagents.len()
                } else {
                    0
                };
                std::iter::once(Row::Session(i))
                    .chain((0..children).map(move |index| Row::Subagent { parent: i, index }))
            })
            .collect();
        self.selected = anchor
            .and_then(|key| self.visible.iter().position(|&r| self.row_key(r) == key))
            .unwrap_or(self.selected)
            .min(self.visible.len().saturating_sub(1));
        self.ensure_available_tab();
        self.needs_redraw = true;
    }

    /// Identity of a row across refreshes.
    ///
    /// A subagent's own id is unique only within its parent, and a session key
    /// alone cannot tell a parent from its children, so the cursor is anchored
    /// on the pair.
    fn row_key(&self, row: Row) -> String {
        let Some(session) = self.sessions.get(row.session()) else {
            return String::new();
        };
        match row {
            Row::Session(_) => session.key(),
            Row::Subagent { index, .. } => match session.subagents.get(index) {
                Some(sub) => format!("{}/{}", session.key(), sub.agent_id),
                None => session.key(),
            },
        }
    }

    /// Fold this refresh's figures into the overview history buffers.
    fn push_history(&mut self) {
        // This is a rate, not a refresh delta, so its meaning is stable when
        // the user changes --delay or a filesystem scan takes longer.
        self.global_spend.push(self.stats.spend_per_min);
        self.global_cpu.push(self.stats.total_cpu as f64);

        for s in &self.sessions {
            let Some(p) = &s.process else { continue };
            let key = s.key();
            self.cpu_history
                .entry(key.clone())
                .or_default()
                .push(p.cpu as f64);
            self.mem_history
                .entry(key)
                .or_default()
                .push(p.memory as f64 / (1024.0 * 1024.0));
        }
    }

    /// What the bottom panels should describe for a row.
    ///
    /// A subagent gets a stand-in `Session` pointed at its own transcript. That
    /// file is the same JSONL a session writes, so the whole extraction path —
    /// worker, cache, every panel — reads it without knowing the difference, and
    /// the panels describe the subagent rather than the parent it ran under.
    fn panel_subject(&self, row: Row) -> Option<Session> {
        let session = self.sessions.get(row.session())?;
        let Row::Subagent { index, .. } = row else {
            return Some(session.clone());
        };
        let sub = session.subagents.get(index)?;
        // A purged transcript leaves nothing to read; the parent's own data is
        // the only thing left that describes the run.
        if sub.ghost {
            return None;
        }

        let mut stand_in = Session::new(session.provider, sub.agent_id.clone());
        stand_in.surface = session.surface;
        stand_in.model = sub.model.clone();
        stand_in.label_source = session.label_source.clone();
        stand_in.harness = session.harness.clone();
        stand_in.title = Some(sub.description.clone()).filter(|d| !d.is_empty());
        stand_in.started_at = sub.started_at.clone().unwrap_or_default();
        stand_in.data_file = session
            .data_file
            .as_ref()
            .map(|f| f.with_extension("").join("subagents"))
            .map(|dir| dir.join(format!("{}.jsonl", sub.agent_id)));
        // Its own mtime, so the panels refresh while the subagent is working and
        // not merely when its parent writes something.
        stand_in.last_active = stand_in
            .data_file
            .as_ref()
            .map(|f| crate::util::ms_to_rfc3339(crate::config::file_mtime_ms(f) as i64))
            .unwrap_or_default();
        Some(stand_in)
    }

    /// Ask the worker for the selected row's full data if it isn't loaded.
    fn sync_panel_data(&mut self) {
        let Some(row) = self.selected_row() else {
            self.panel_data = None;
            self.panel_key.clear();
            return;
        };
        let Some(session) = self.panel_subject(row) else {
            self.panel_data = None;
            self.panel_key.clear();
            return;
        };
        // Nothing to extract for a row read over ssh: the transcript is a file
        // on the other machine and cctop never fetches it. Asking would hand the
        // worker a session with no `data_file` and get an empty result back that
        // the panels would draw as zeroes.
        if session.remote.is_some() {
            self.panel_data = None;
            self.panel_key = session.key();
            self.panel_stamp = session.last_active.clone();
            return;
        }
        let key = session.key();
        let stamp = session.last_active.clone();
        let switched = key != self.panel_key;
        // A live session keeps growing, so re-request whenever its newest
        // activity moves — otherwise the panels freeze at whatever the session
        // looked like when it was selected.
        let grew = !switched && stamp != self.panel_stamp;
        if !switched && !grew {
            return;
        }

        self.panel_key = key;
        self.panel_stamp = stamp;

        if switched {
            // Only blank the panels when moving to a different session; doing it
            // on every append would flash "Loading…" twice a second.
            self.panel_data = None;
            self.info_scroll = 0;
            self.cost_scroll = 0;
            self.config_scroll = 0;
            self.proc_scroll = 0;
            self.subagent_scroll = 0;
            self.tool_scroll = 0;
            self.tool_follow = true;
            self.tool_tab = 0;
            self.tool_expanded = None;
        }
        let _ = self.tx.send(Request::Data(Box::new(session)));
    }

    fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last as isize) as usize;
        self.ensure_available_tab();
        self.needs_redraw = true;
    }

    fn tab_available(&self, tab: usize) -> bool {
        match tab {
            // Performance and Processes read a live process tree.
            1 | 2 => self.selected_session().is_some_and(Session::is_running),
            // Only Claude transcripts report the per-request usage the context
            // breakdown is reconstructed from. Gated on the provider rather than
            // on the extracted data, so the tab doesn't vanish while it loads.
            7 => self
                .selected_session()
                .is_some_and(|s| s.provider == Provider::Claude),
            _ => true,
        }
    }

    fn ensure_available_tab(&mut self) {
        if !self.tab_available(self.bottom_tab) {
            self.bottom_tab = 0;
        }
    }

    fn set_sort(&mut self, col: ColumnId) {
        if self.sort_col == col {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort_col = col;
            self.sort_asc = true;
        }
        self.refilter();
        self.save_prefs();
    }

    /// Expand or collapse the invocation under a clicked log row.
    fn toggle_tool_expansion(&mut self, row_offset: usize) {
        let line = self.tool_scroll as usize + row_offset;
        let Some(Some(key)) = self.tool_owners.get(line) else {
            return;
        };
        let key = key.clone();
        // Clicking the open entry again closes it.
        self.tool_expanded = (self.tool_expanded.as_deref() != Some(key.as_str())).then_some(key);
        // Expanding grows the log, which would otherwise slide the row away.
        self.tool_follow = false;
        self.needs_redraw = true;
    }

    /// Move through the Tool Activity sidebar, which filters the log by tool.
    fn cycle_tool_filter(&mut self, delta: isize) {
        let n = self
            .panel_data
            .as_ref()
            .map(|d| panels::tool_tabs(d).len())
            .unwrap_or(0);
        if n == 0 {
            return;
        }
        let n = n as isize;
        self.tool_tab = (((self.tool_tab as isize + delta) % n + n) % n) as usize;
        // A different filter is a different log, so start at its newest entry.
        self.tool_follow = true;
        self.bottom_tab = 3;
        self.needs_redraw = true;
    }

    /// Move to the next or previous bottom panel, wrapping at both ends.
    fn cycle_tab(&mut self, delta: isize) {
        let n = panels::TABS.len() as isize;
        let mut next = self.bottom_tab;
        for _ in 0..panels::TABS.len() {
            next = (((next as isize + delta) % n + n) % n) as usize;
            if self.tab_available(next) {
                self.bottom_tab = next;
                break;
            }
        }
        self.save_prefs();
        self.needs_redraw = true;
    }

    fn scroll_active_panel(&mut self, delta: i32) {
        // Saturating, because the jumps to either end are i32::MIN and i32::MAX
        // rather than a computed distance: the panel's length is the renderer's
        // to know, and asking for more than there is costs nothing once both
        // ends clamp.
        let max = self.panel_max_scroll as i32;
        let bump = |v: &mut u16| *v = (*v as i32).saturating_add(delta).clamp(0, max) as u16;
        match self.bottom_tab {
            0 => bump(&mut self.info_scroll),
            1 => {} // Performance is a fixed-size chart pair
            2 => bump(&mut self.proc_scroll),
            3 => {
                bump(&mut self.tool_scroll);
                // Re-pin once the user scrolls back down to the newest entry.
                self.tool_follow = self.tool_scroll >= self.panel_max_scroll;
            }
            4 => bump(&mut self.subagent_scroll),
            5 => bump(&mut self.cost_scroll),
            6 => bump(&mut self.config_scroll),
            _ => bump(&mut self.context_scroll),
        }
        self.needs_redraw = true;
    }

    /// Copy something useful about the selection to the clipboard.
    fn copy_selection(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        let text = match self.bottom_tab {
            // From the Info tab, the resume command is the most useful thing —
            // and for the providers that have none, the transcript's path is.
            0 => match s.resume_argv() {
                Some(argv) => argv.join(" "),
                None => s
                    .data_file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| s.session_id.clone()),
            },
            _ => s
                .data_file
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| s.session_id.clone()),
        };
        render::copy_to_clipboard(&text);
        self.set_status(format!("Copied: {}", crate::util::truncate(&text, 60)));
    }

    /// Whether the session matches the active text search.
    ///
    /// `refilter` calls [`matches_query`] directly with a query it lowercases
    /// once; this is the same predicate for callers that only have one session
    /// in hand, so the live filter and the `n`/`N` jump cannot drift apart.
    fn matches_search(&self, s: &Session) -> bool {
        self.matches_query(s, &self.search.to_ascii_lowercase())
    }

    /// Whether a session matches `query`, which must already be lowercase.
    ///
    /// Content search widens the filter rather than replacing it: a query that
    /// names a project still finds that project's sessions, and the transcripts
    /// add whatever else mentions it. Hits only count while they belong to the
    /// query being typed — until the scan for a longer query lands, its rows are
    /// the metadata matches alone, which is a filter narrowing as you type
    /// rather than showing results for a query you have moved on from.
    fn matches_query(&self, s: &Session, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        // Field by field rather than one joined string. This runs per session
        // per refresh, and lowercasing them all was the whole per-refresh
        // allocation; it also stops a query matching across the seam between
        // two unrelated fields.
        let fields: [&str; 8] = [
            s.display_label(),
            &s.model,
            &s.harness,
            s.provider.as_str(),
            &s.session_id,
            &s.label_source,
            // Empty for this user's own rows, which no query can match, so
            // searching a name finds that person's sessions and nothing else.
            s.owner.as_deref().unwrap_or_default(),
            // Likewise empty for every harness but Claude Code, so `work` finds
            // that login's sessions rather than everything that mentions work.
            s.profile.as_deref().unwrap_or_default(),
        ];
        if fields.iter().any(|f| contains_ascii_ci(f, query)) {
            return true;
        }
        // The branch is derived rather than stored, so it is the one field that
        // cannot be borrowed straight off the session.
        if columns::branch_of(s).is_some_and(|b| contains_ascii_ci(&b, query)) {
            return true;
        }
        self.search_content && self.scan_query == query && self.scan_hits.contains_key(&s.key())
    }

    /// The transcript text around the selected session's content match.
    pub fn selected_snippet(&self) -> Option<&str> {
        let s = self.selected_session()?;
        (self.search_content && self.scan_query == self.search.to_ascii_lowercase())
            .then(|| self.scan_hits.get(&s.key()))
            .flatten()
            .map(String::as_str)
    }

    /// Note that the query changed, so the scan can be rescheduled.
    ///
    /// Every edit lands here, including the ones that only shorten the query:
    /// hits for a longer query are not hits for a shorter one, and leaving them
    /// applied would leave rows on screen that no longer match anything.
    pub(super) fn search_edited(&mut self) {
        self.history_cursor = None;
        self.scan_typed_at = Some(Instant::now());
        self.refilter();
    }

    /// Turn transcript searching on or off.
    pub(super) fn toggle_content_search(&mut self) {
        self.search_content = !self.search_content;
        if !self.search_content {
            // Results for a search nobody is running any more; keeping them
            // would make the next toggle show stale rows for an instant.
            self.scan_hits.clear();
            self.scan_query.clear();
        }
        self.scan_typed_at = Some(Instant::now());
        self.refilter();
    }

    /// Send the current query off to be scanned, once the typing has settled.
    ///
    /// Called every loop iteration rather than on each keystroke: a scan reads
    /// every transcript on disk, and firing one per character would spend the
    /// whole budget on prefixes of the word being typed.
    pub(super) fn tick_scan(&mut self) {
        if !self.search_content || self.scanning {
            return;
        }
        let query = self.search.to_ascii_lowercase();
        if query == self.scan_query {
            self.scan_typed_at = None;
            return;
        }
        // A one- or two-character query matches nearly every transcript, so it
        // is the most expensive scan to run and the least useful to read.
        // Deleting back to that length drops the results with it, rather than
        // leaving a count on screen for a query no longer being asked.
        if query.chars().count() < MIN_SCAN_CHARS {
            if !self.scan_query.is_empty() {
                self.scan_query.clear();
                self.scan_hits.clear();
                self.refilter();
                self.needs_redraw = true;
            }
            return;
        }
        match self.scan_typed_at {
            Some(at) if at.elapsed() < SCAN_DEBOUNCE => return,
            _ => {}
        }
        self.scan_typed_at = None;
        let targets: Vec<crate::session::search::Target> = self
            .sessions
            .iter()
            .map(crate::session::search::Target::of)
            .collect();
        if self.tx.send(Request::Scan { query, targets }).is_ok() {
            self.scanning = true;
            self.needs_redraw = true;
        }
    }

    /// Fold in a finished scan.
    fn scanned(&mut self, query: String, hits: HashMap<String, String>) {
        self.scanning = false;
        self.scan_query = query;
        self.scan_hits = hits;
        self.refilter();
        self.needs_redraw = true;
    }

    /// Record the query that was just run, so ↑ can bring it back.
    pub(super) fn remember_query(&mut self) {
        let query = self.search.trim().to_string();
        if query.is_empty() {
            return;
        }
        // Re-running a query moves it to the front rather than adding a second
        // copy, which is what makes a short history worth walking.
        self.search_history.retain(|q| q != &query);
        self.search_history.insert(0, query);
        self.search_history
            .truncate(crate::cache::MAX_SEARCH_HISTORY);
        self.save_prefs();
    }

    /// Walk the query history: `1` towards older entries, `-1` back towards
    /// what was being typed when the walk started.
    pub(super) fn history_step(&mut self, delta: isize) {
        if self.search_history.is_empty() {
            return;
        }
        let (at, typed) = match self.history_cursor.take() {
            Some((at, typed)) => (at as isize + delta, typed),
            // Nothing walked yet: ↓ has nowhere older to come back from.
            None if delta < 0 => return,
            None => (0, self.search.clone()),
        };
        // Stepping back past the newest entry restores the partial query, which
        // is the one thing the history itself cannot hold.
        if at < 0 {
            self.search = typed;
        } else {
            let at = (at as usize).min(self.search_history.len() - 1);
            self.search = self.search_history[at].clone();
            self.history_cursor = Some((at, typed));
        }
        self.scan_typed_at = Some(Instant::now());
        self.refilter();
        self.needs_redraw = true;
    }

    /// Whether the cursor is on a row read from another machine.
    pub fn selected_is_remote(&self) -> bool {
        self.selected_session().is_some_and(|s| s.remote.is_some())
    }

    /// Open the terminate confirmation for the selected session, explaining
    /// itself when there is nothing this cctop can signal.
    fn confirm_terminate(&mut self) {
        // Kept here rather than at the key, because this is now the only way in:
        // `k` moves the cursor, and Ctrl+K is what asks. A subagent has no
        // process of its own to signal — stopping it means stopping its parent,
        // which is not what the cursor is pointing at.
        if self.on_subagent() {
            self.set_status("A subagent cannot be stopped on its own");
            return;
        }
        if let Some(why) = self.selected_session().and_then(App::remote_refusal) {
            self.set_status(why);
            return;
        }
        match self.selected_session() {
            Some(s) if s.root_pid().is_some() => self.mode = Mode::KillConfirm,
            Some(s) if s.is_running() => self.mode = Mode::KillBlocked,
            Some(_) => self.set_status("Selected session is not running"),
            None => {}
        }
    }

    /// Peel off one filter layer, narrowest first, and say which one went.
    ///
    /// One press per layer rather than all at once: filters are combined
    /// deliberately, and clearing four of them on a stray Esc would lose work
    /// that took four deliberate keystrokes to set up. Every layer that paints
    /// a badge in the footer is reachable from here, so nothing can stay on
    /// with no way to turn it off.
    /// Whether any filter layer is on, and so whether `Esc` would do anything.
    ///
    /// The same layers [`clear_one_filter`](Self::clear_one_filter) peels, in
    /// one place: a footer that offered `Esc Clear filter` with nothing to
    /// clear would be teaching a key that does nothing.
    pub(super) fn has_filter(&self) -> bool {
        !self.search.is_empty()
            || self.cost_floor > 0.0
            || self.live_only
            || self.age_filter.is_some()
            || self.tool_tab != 0
            || self.tool_live_only
    }

    fn clear_one_filter(&mut self) {
        let cleared = if !self.search.is_empty() {
            self.search.clear();
            "Search cleared"
        } else if self.cost_floor > 0.0 {
            self.cost_floor = 0.0;
            "Cost floor cleared"
        } else if self.live_only {
            self.live_only = false;
            "Showing stopped sessions too"
        } else if self.age_filter.is_some() {
            self.age_filter = None;
            "Age filter cleared"
        } else if self.tool_tab != 0 || self.tool_live_only {
            // The Tool Activity sidebar filters a panel rather than the table,
            // so it comes last: it is the layer the user is least likely to
            // have forgotten about.
            self.tool_tab = 0;
            self.tool_live_only = false;
            self.tool_follow = true;
            "Tool Activity filter cleared"
        } else {
            return;
        };
        self.refilter();
        self.save_prefs();
        self.set_status(cleared);
    }

    /// Jump to the next/previous session matching the active search, wrapping
    /// around both ends. With no search active every visible session matches.
    fn cycle_matches(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        // Positions rather than rows: a child row matches on its parent's text,
        // so several rows can share one session and `position` would keep
        // sending the cursor back to the first of them.
        let matches: Vec<usize> = self
            .visible
            .iter()
            .enumerate()
            .filter(|(_, row)| self.matches_search(&self.sessions[row.session()]))
            .map(|(at, _)| at)
            .collect();
        if matches.is_empty() {
            return;
        }
        let pos = matches.iter().position(|&at| at == self.selected);
        let n = matches.len() as isize;
        let next = ((pos.unwrap_or(0) as isize + delta).rem_euclid(n)) as usize;
        self.selected = matches[next];
        self.ensure_available_tab();
        self.needs_redraw = true;
    }

    /// Toggle whether the selected session is marked for a batch action.
    pub(super) fn toggle_mark(&mut self) {
        let Some(s) = self.selected_session() else {
            return;
        };
        let key = s.key();
        if !self.marked.remove(&key) {
            self.marked.insert(key);
        }
        self.needs_redraw = true;
    }

    /// The session keys currently marked, in table order for a stable listing.
    fn marked_sessions(&self) -> Vec<&Session> {
        self.visible
            .iter()
            .filter_map(|row| match row {
                // Child rows would list their parent a second time.
                Row::Session(i) => self.sessions.get(*i),
                Row::Subagent { .. } => None,
            })
            .filter(|s| self.marked.contains(&s.key()))
            .collect()
    }

    /// True when every marked session is ready for the given batch action.
    fn batch_ok(&self, kind: BatchKind) -> bool {
        self.marked_sessions().iter().all(|s| match kind {
            BatchKind::Delete => !s.is_running(),
            BatchKind::Kill => s.root_pid().is_some(),
        })
    }

    fn unmark_all(&mut self) {
        if self.marked.is_empty() {
            return;
        }
        self.marked.clear();
        self.needs_redraw = true;
    }

    /// Enter the batch-confirm modal if there's anything to do.
    fn batch(&mut self, kind: BatchKind) {
        if self.marked_sessions().is_empty() {
            self.set_status("No sessions marked — press Space to mark");
            return;
        }
        self.batch = kind;
        self.mode = if self.batch_ok(kind) {
            Mode::BatchConfirm
        } else {
            match kind {
                BatchKind::Delete => Mode::BatchDeleteBlocked,
                BatchKind::Kill => Mode::BatchKillBlocked,
            }
        };
        self.needs_redraw = true;
    }

    /// Confirm and run the pending batch action over all marked sessions.
    fn batch_execute(&mut self) {
        let kind = self.batch;
        let marked: Vec<Session> = self.marked_sessions().into_iter().cloned().collect();
        let mut requested = 0;
        let mut acted_on: Vec<String> = Vec::new();
        let mut failed = 0;
        for s in &marked {
            let key = s.key();
            // Marking spans machines because the table does; acting does not.
            if s.remote.is_some() {
                failed += 1;
                continue;
            }
            match kind {
                BatchKind::Delete => {
                    if self.tx.send(Request::Delete(Box::new(s.clone()))).is_ok() {
                        self.deleting.insert(key.clone());
                        requested += 1;
                        acted_on.push(key);
                    } else {
                        failed += 1;
                    }
                }
                BatchKind::Kill => match s.root_pid() {
                    Some(pid) => {
                        self.tx
                            .send(Request::Terminate {
                                session_key: key.clone(),
                                pid,
                            })
                            .ok();
                        acted_on.push(key);
                    }
                    None => failed += 1,
                },
            }
        }
        for key in &acted_on {
            self.marked.remove(key);
        }
        self.set_status(match kind {
            BatchKind::Delete => {
                if failed == 0 {
                    format!("Deleting {requested} session(s)…")
                } else {
                    format!("Deleting {requested} session(s), {failed} failed to start")
                }
            }
            BatchKind::Kill => format!(
                "Kill sent to {} session(s){}",
                acted_on.len(),
                if failed > 0 {
                    format!(" ({} skipped)", failed)
                } else {
                    String::new()
                }
            ),
        });
    }

    /// Let the notifier see this refresh, and ring if anything crossed.
    ///
    /// Called from the event loop only when the rows actually moved. It has to
    /// run on this thread: the bell and the OSC 9 sequence go straight to
    /// stdout, which ratatui owns, and only here is it certain that no frame is
    /// halfway through being flushed.
    fn check_bells(&mut self) {
        self.notify.observe(&self.sessions);
    }

    /// Turn the bell on or off, and remember which.
    fn toggle_notifications(&mut self) {
        self.notify.enabled = !self.notify.enabled;
        self.save_prefs();
        self.set_status(if self.notify.enabled {
            "Notifications on — bell and desktop alert when a session needs you"
        } else {
            "Notifications off"
        });
    }

    /// Jump the selection to whichever session rang last.
    fn jump_to_bell(&mut self) {
        let Some(key) = self.notify.last.as_ref().map(|r| r.key.clone()) else {
            self.set_status("Nothing has rung yet");
            return;
        };
        // The parent row, not a child of it: the bell rang for the session.
        match self
            .visible
            .iter()
            .position(|&r| !r.is_subagent() && self.sessions[r.session()].key() == key)
        {
            Some(row) => {
                self.selected = row;
                self.ensure_available_tab();
                self.needs_redraw = true;
            }
            // Answering it is the point, so say why it can't be reached rather
            // than moving the cursor somewhere arbitrary.
            None => self.set_status("The session that rang is hidden by the current filter"),
        }
    }

    /// Half the visible table height, used by Ctrl+U/Ctrl+D. Falls back to a
    /// page size before the first draw has recorded a viewport.
    fn half_page(&self) -> usize {
        ((self.list_height as usize / 2).max(1)).min(PAGE as usize)
    }
}

/// Rows moved by PageUp/PageDown and the fallback for half-page scrolls.
const PAGE: isize = 10;

/// Directories the launcher's `in` field will remember, at most.
///
/// The list is a way of not having to recall a path, so it is worth being
/// generous — but it is filtered by what is typed, and a machine with hundreds
/// of transcripts would otherwise stat every project on it on every keystroke.
const MAX_KNOWN_DIRS: usize = 40;

/// Half-period of the tab-bar blink. Slow enough to read the title through,
/// fast enough to catch the eye.
const BLINK_MS: u128 = 600;

/// How often the tab bar is reconciled against the rmux sessions on this
/// machine, so a tab opened in one cctop shows up in the others.
///
/// It costs a `rmux list-panes`, so it cannot ride the draw loop. Two seconds is
/// short enough that a tab opened next door is there before you have switched
/// windows to look for it, and long enough that the subprocess is nothing.
const SHARE_EVERY: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// Workspace tabs
// ---------------------------------------------------------------------------

impl App {
    /// The tab on screen, or `None` on the dashboard.
    pub fn active_tab(&mut self) -> Option<&mut tabs::Tab> {
        self.tabs.get_mut(self.tab.checked_sub(1)?)
    }

    /// The pane the keyboard belongs to, or `None` on the dashboard.
    pub fn focused_pane(&mut self) -> Option<&mut tabs::Pane> {
        self.active_tab()?.focused_mut()
    }

    /// What tab `index` wants, if anything. `0` is the dashboard, which never
    /// asks for itself.
    ///
    /// The tab you are already looking at is excluded — its own focused pane is
    /// in front of you, so blinking its title tells you nothing you cannot see.
    pub fn tab_attention(&self, index: usize) -> Option<tabs::Attention> {
        let tab = self.tabs.get(index.checked_sub(1)?)?;
        tab.attention(index == self.tab, &|pid| self.pane_signal(pid))
    }

    /// Fold in whatever the agents have reported.
    ///
    /// These outrank anything read off disk or off a screen: an agent saying
    /// "my turn is over" is the fact those are both estimating.
    ///
    /// Returns whether anything arrived, and whether the set of sessions itself
    /// changed — one that has just started or just ended is a row to go and
    /// find or forget now, rather than at the next poll.
    fn apply_hooks(&mut self, events: Vec<crate::hook::Event>) -> (bool, bool) {
        let changed = !events.is_empty();
        let mut lifecycle = false;
        for event in events {
            lifecycle |= event.reported.signal.is_lifecycle();
            if let Some(agent) = event.finished_agent {
                self.finished_agents.insert(agent);
            }
            match event.reported.signal {
                // Nothing more will be said about it, and leaving the last
                // signal behind would have the row claim a state forever.
                crate::hook::Signal::Ended => {
                    self.hooked.remove(&event.session_id);
                }
                _ => {
                    self.hooked.insert(event.session_id, event.reported);
                }
            }
        }
        // A working claim that nothing has confirmed for a quarter of an hour is
        // dropped rather than believed: see
        // [`Reported::is_current`](crate::hook::Reported::is_current). Swept
        // here because this is the only place the map grows, and a session that
        // was killed mid-turn will never send the event that would clear it.
        self.hooked.retain(|_, reported| reported.is_current());
        self.apply_finished_agents();
        self.apply_permissions();
        (changed, lifecycle)
    }

    /// Stamp each session with the permission mode its own hooks reported.
    ///
    /// Also run after a walk, because the rows are rebuilt wholesale and a
    /// freshly discovered one has to pick up what was reported before it
    /// existed. `hooked` outlives the rows for exactly this reason.
    fn apply_permissions(&mut self) {
        if self.hooked.is_empty() {
            return;
        }
        for session in &mut self.sessions {
            if let Some(reported) = self.hooked.get(&session.session_id) {
                // Only ever set from a report. A session whose newest event did
                // not carry the field keeps the last mode that did, because the
                // setting has not changed just because one event was quiet
                // about it.
                if reported.permission.is_some() {
                    session.permission = reported.permission;
                }
            }
        }
    }

    /// Mark the subagents whose own hook has reported them finished.
    ///
    /// Stamped onto the rows rather than consulted at each draw, so every reader
    /// of a `Subagent` — the child rows, the Subagents tab, `--json` — agrees
    /// without being handed the UI's state. The hook outranks the transcript
    /// heuristic: it is the agent saying so, where the heuristic is only the
    /// absence of writing.
    fn apply_finished_agents(&mut self) {
        if self.finished_agents.is_empty() {
            return;
        }
        for session in &mut self.sessions {
            for sub in &mut session.subagents {
                // The hook names the bare id; the transcript is `agent-<id>`.
                let id = sub.agent_id.strip_prefix("agent-").unwrap_or(&sub.agent_id);
                if self.finished_agents.contains(id) {
                    sub.status = crate::session::SubagentStatus::Done;
                }
            }
        }
    }

    /// Open the integration panel, reading the current state off disk.
    pub fn open_hooks(&mut self) {
        self.hooks = Some(self.hook_status());
        self.mode = Mode::Hooks;
        self.needs_redraw = true;
    }

    /// The integration's state, scoped to whichever project the cursor is on.
    fn hook_status(&self) -> crate::hook::Report {
        let mut report =
            crate::hook::status(self.hook_project().as_deref(), self.listener.as_ref());
        // Codex's entry carries a reminder to go and trust its hooks, which is
        // advice until it is done and noise afterwards. Only the events can tell
        // which — see [`App::codex_hooks_heard`] — so the panel is where the
        // reminder is dropped rather than where it is written.
        if self.codex_hooks_heard() {
            for entry in &mut report.entries {
                if entry.harness == crate::hook::Harness::Codex {
                    entry.note = None;
                }
            }
        }
        report
    }

    /// Whether Codex's hooks have ever fired into this cctop.
    ///
    /// Codex is the one harness whose hooks sit there inert until a person has
    /// reviewed and trusted them, and where that trust is recorded is not
    /// something Codex documents — so an install that reads as complete on disk
    /// may be delivering nothing at all.
    ///
    /// What answers it is the permission mode. `notify`, Codex's other channel,
    /// carries no such field, and a hook carries it on every event worth having
    /// — so a Codex row that knows how much it asks before it acts is a Codex
    /// whose hooks are firing. Rows from another machine are excluded: their
    /// mode arrived over `--host` from a cctop where the hooks work, which says
    /// nothing about this one.
    fn codex_hooks_heard(&self) -> bool {
        self.sessions.iter().any(|session| {
            session.provider == Provider::Codex
                && session.remote.is_none()
                && session.permission.is_some()
        })
    }

    /// The line a Codex being started needs, when it needs one.
    ///
    /// The moment of starting one is when this is worth saying and cheap to act
    /// on: there is a fresh prompt on screen, `/hooks` costs a keystroke there,
    /// and the alternative is a session that runs for an hour reporting only
    /// that its turns ended. Said only when the hooks are installed and have
    /// never been heard from — an install nobody asked for is not something to
    /// nag about, and one that is already working needs nothing.
    fn codex_trust_hint(&self, installed: bool) -> &'static str {
        match installed && !self.codex_hooks_heard() {
            true => " — run /hooks in it and trust cctop's to see it here",
            false => "",
        }
    }

    /// The project a `project`-scoped install would write into: the directory
    /// of the selected session, when it has one on this machine.
    pub fn hook_project(&self) -> Option<std::path::PathBuf> {
        self.selected_session()
            .map(|s| std::path::PathBuf::from(&s.label_source))
            .filter(|dir| dir.is_dir())
    }

    /// Install or remove from the panel, and show what happened.
    ///
    /// Every harness at once. The status line gets a count rather than five
    /// paths — the panel underneath is redrawn from disk immediately below, and
    /// that is where the detail belongs.
    pub fn set_hooks(&mut self, scope: crate::hook::Scope, install: bool) {
        let done = match install {
            true => crate::hook::install(&scope),
            false => crate::hook::remove(&scope),
        };
        self.set_status(format!(
            "{} {} agents ({})",
            match install {
                true => "Asked",
                false => "Stopped",
            },
            done.len(),
            scope.label()
        ));
        self.hooks = Some(self.hook_status());
    }

    /// What the agents have actually said, newest state per session, as
    /// `(project, state)` pairs for the panel.
    ///
    /// The project rather than the session id: an id names nothing to a reader,
    /// and this list is the answer to "is the thing I just installed working",
    /// which needs a name you recognise.
    pub fn reporting(&self) -> Vec<(String, &'static str)> {
        let mut rows: Vec<(String, &'static str)> = self
            .hooked
            .values()
            .filter(|r| r.is_current())
            .map(|r| {
                let name = std::path::Path::new(&r.cwd)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "—".into());
                (name, r.signal.label())
            })
            .collect();
        rows.sort();
        rows
    }

    /// What a session's own hooks last said about it, if it has any.
    fn hooked_signal(&self, session_id: &str) -> Option<crate::hook::Signal> {
        // Checked here as well as in the sweep: the sweep runs when an event
        // arrives, and a session that has gone silent is precisely the one that
        // sends none — so between events the map still holds the stale claim.
        if let Some(reported) = self.hooked.get(session_id) {
            return reported.is_current().then_some(reported.signal);
        }
        // Gemini CLI reports a full session id, but names the chat file it
        // writes — which is the only identity cctop's rows have, because
        // resuming reuses the id across disjoint files — after the *first eight
        // characters* of it. Without this last step every Gemini event lands on
        // no row at all.
        let tail = gemini_id_tail(session_id)?;
        self.hooked
            .iter()
            .find(|(id, _)| id.starts_with(tail))
            .filter(|(_, reported)| reported.is_current())
            .map(|(_, reported)| reported.signal)
    }

    /// What has been reported about the agent running as `pid`, if anything.
    ///
    /// Hooks first, because the agent said it outright; the transcript second,
    /// which can only report the question and not the finished turn; nothing at
    /// all if the session has not been discovered yet, which leaves the caller
    /// to fall back to the pane's screen.
    ///
    /// Scans the table rather than keeping an index: there are a handful of
    /// panes and this runs once per frame, so a map would be state to keep
    /// correct in exchange for nothing measurable.
    fn pane_signal(&self, pid: u32) -> Option<crate::hook::Signal> {
        self.sessions
            .iter()
            .filter(|session| session.root_pid() == Some(pid))
            .find_map(|session| {
                self.hooked_signal(&session.session_id).or({
                    match session.activity_state {
                        crate::session::ActivityState::WaitingForInput => {
                            Some(crate::hook::Signal::NeedsInput)
                        }
                        _ => None,
                    }
                })
            })
    }

    /// Note that you have just typed into the terminal of the agent running as
    /// `pid`, so it is no longer waiting on you.
    ///
    /// The hooks cannot report this themselves. A permission prompt's answer
    /// produces no event of its own — the next thing Claude Code says is
    /// `PostToolUse`, once the tool it just unblocked has *finished*, which for
    /// a long command is a minute of a tab blinking at you about a question you
    /// already answered.
    ///
    /// Only an existing report is overwritten. Inserting one for an agent
    /// without hooks would shadow the transcript, which is that agent's only
    /// source of state and the thing that would otherwise correct this.
    ///
    /// A tool call in flight is overwritten too, even though it is a working
    /// state: [`Signal::Acting`](crate::hook::Signal::Acting) plus a still
    /// screen is how a held permission prompt is recognised, and the keystroke
    /// that answered it is the only sign the prompt is gone.
    fn mark_answered(&mut self, pid: u32) {
        let answered: Vec<String> = self
            .sessions
            .iter()
            .filter(|session| session.root_pid() == Some(pid))
            .map(|session| session.session_id.clone())
            .collect();
        for id in answered {
            if let Some(reported) = self.hooked.get_mut(&id)
                && reported.signal.awaits_you()
            {
                reported.signal = crate::hook::Signal::Busy;
            }
        }
    }

    /// Whether any hidden tab is explicitly waiting for input and should blink.
    pub fn any_attention(&self) -> bool {
        (1..=self.tabs.len()).any(|i| self.tab_attention(i) == Some(tabs::Attention::NeedsInput))
    }

    /// Which half of the blink cycle we are in.
    pub fn blink_on(&self) -> bool {
        (self.started.elapsed().as_millis() / BLINK_MS).is_multiple_of(2)
    }

    /// Show `tab`, clamped to what exists.
    pub fn show_tab(&mut self, tab: usize) {
        self.go_to_tab(tab.min(self.tabs.len()));
    }

    /// Move `delta` tabs along, wrapping through the dashboard.
    /// Move the tab at `from` to `to`, both indexed the way the bar is: `0` is
    /// the dashboard, which neither moves nor is displaced.
    ///
    /// The view follows the tab it was on rather than the position it was at —
    /// dragging a tab must not move you to a different agent, and neither must
    /// dragging one past the tab you are watching.
    pub fn move_tab(&mut self, from: usize, to: usize) {
        let (Some(a), Some(b)) = (from.checked_sub(1), to.checked_sub(1)) else {
            return;
        };
        if a == b || a >= self.tabs.len() || b >= self.tabs.len() {
            return;
        }
        let moved = self.tabs.remove(a);
        self.tabs.insert(b, moved);
        self.tab = match self.tab {
            here if here == from => to,
            // Everything the tab was lifted out of shifts one place towards the
            // gap it left.
            here if a < b && here > from && here <= to => here - 1,
            here if b < a && here >= to && here < from => here + 1,
            here => here,
        };
        self.needs_redraw = true;
    }

    /// Move the tab on screen one place along the bar, for the keyboard.
    ///
    /// Clamped rather than wrapped, unlike [`App::cycle_workspace`]: wrapping is
    /// natural when you are stepping *through* tabs and disorienting when you
    /// are rearranging them, where a tab at the end jumping to the front reads
    /// as having lost it.
    pub fn move_workspace(&mut self, delta: isize) {
        let Some(from) = (self.tab > 0).then_some(self.tab) else {
            return;
        };
        let to = (from as isize + delta).clamp(1, self.tabs.len() as isize) as usize;
        self.move_tab(from, to);
    }

    pub fn cycle_workspace(&mut self, delta: isize) {
        let count = self.tabs.len() as isize + 1;
        self.go_to_tab((self.tab as isize + delta).rem_euclid(count) as usize);
    }

    /// Move to `want`, taking the rmux client with you.
    ///
    /// This is what makes one set of tabs work across several cctops. Every tab
    /// in the bar is a rmux session any of them can attach to, but only the one
    /// you are looking at is worth holding a client on — several clients on one
    /// window and rmux has to pick a size that suits none of them. So the client
    /// follows the view: the tab arrived at takes one, the tab left behind gives
    /// its up, and the agent in between never notices either.
    ///
    /// The order matters. Attaching first means a session that has ended since
    /// the last sync leaves you where you were, reading why, rather than on a
    /// blank tab with the one you could see now detached as well.
    pub fn go_to_tab(&mut self, want: usize) {
        self.needs_redraw = true;
        if want == self.tab {
            return;
        }
        if let Some(tab) = want.checked_sub(1).and_then(|i| self.tabs.get_mut(i))
            && tab.detached()
        {
            let title = tab.title();
            if let Err(error) = tab.attach() {
                self.set_status(format!("Could not open {title}: {error}"));
                return;
            }
        }
        if let Some(tab) = self.tab.checked_sub(1).and_then(|i| self.tabs.get_mut(i)) {
            tab.detach();
        }
        self.tab = want;
    }

    /// Open the launcher, remembering where the pick should go and which
    /// directory it should start in.
    pub fn launch_prompt(&mut self, into: LaunchInto) {
        if matches!(into, LaunchInto::Split { .. }) && self.active_tab().is_none() {
            self.set_status("Nothing to split — open a tab first");
            return;
        }
        let offer = tabs::choices(&self.open_rmux());
        if offer.is_empty() {
            self.set_status("No agent found in PATH, and $SHELL is not set");
            return;
        }
        self.launch_offer = offer;
        self.launch_into = into;
        self.launch_cursor = 0;
        // A split lands next to an agent already working somewhere; a fresh tab
        // starts where cctop itself was invoked. The selected dashboard row is
        // for inspecting or resuming that session, not an implicit cwd switch.
        self.launch_cwd = match into {
            LaunchInto::Split { .. } => self.launch_cwd.clone(),
            LaunchInto::Tab => self.launch_root.clone(),
        };
        self.mode = Mode::Launch;
    }

    /// Write the selected session's context brief and offer it to a new agent.
    ///
    /// This is the cross-harness counterpart to `R`. Resuming puts the *same*
    /// harness back on the *same* transcript; a handoff carries what the session
    /// was doing across to a different agent entirely, which is the one thing no
    /// harness can do for itself — each one can only read its own transcripts.
    ///
    /// The brief is written before the launcher opens so a failure to write it
    /// is reported instead of starting an agent that then has nothing to read.
    pub(super) fn handoff_selected(&mut self) {
        let Some(session) = self.selected_session().cloned() else {
            return;
        };
        // The panels already hold the selected session's extraction; a brief
        // built while the row is still loading, or while a subagent row owns the
        // panels, falls back to the header alone rather than to another
        // session's data.
        let data = match self.panel_key == session.key() {
            true => self.panel_data.as_ref(),
            false => None,
        };
        let brief = crate::handoff::build(&session, data);
        let path = match crate::handoff::write(&brief) {
            Ok(path) => path,
            Err(error) => {
                self.set_status(format!("Could not write the handoff brief: {error}"));
                return;
            }
        };
        self.pending_brief = Some(path);
        self.pending_fork = crate::handoff::forkable(&session).map(std::path::Path::to_path_buf);
        // The receiving agent belongs in the directory the work is in, whatever
        // row the cursor moves to while the launcher is up.
        self.launch_prompt(LaunchInto::Tab);
        // `launch_prompt` bails on its own when nothing can be launched, and
        // leaving a brief pending for a launcher that never opened would attach
        // it to the next unrelated agent instead.
        if self.mode != Mode::Launch {
            self.pending_brief = None;
            self.pending_fork = None;
            return;
        }
        self.set_status(format!(
            "Handing off {} — pick who takes it",
            brief.summary()
        ));
    }

    /// Deliver a brief to the agent it was launched for, once that agent has had
    /// long enough to start reading its keyboard.
    pub(super) fn tick_handoff(&mut self) {
        let Some((pid, line, due)) = self.handoff_send.clone() else {
            return;
        };
        if Instant::now() < due {
            return;
        }
        self.handoff_send = None;
        match crate::inject::send_line(pid, &line) {
            Ok(()) => self.set_status("Handed the brief over"),
            // The brief is on disk either way, so the failure is recoverable by
            // hand — say where it is rather than only that this did not work.
            Err(error) => self.set_status(format!("Could not hand the brief over: {error}")),
        }
    }

    /// Reopen the selected session in a tab of its own.
    ///
    /// This is the one way into a session cctop did not start. `a` shows an
    /// agent's live terminal, but only for the agents cctop hosts — there is no
    /// pty to borrow otherwise. Resuming instead starts a *new* agent and hands
    /// it the transcript, which is what the harnesses themselves offer and works
    /// whether the session ended an hour ago or is running in another window.
    pub(super) fn resume_selected(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let Some(argv) = session.resume_argv() else {
            self.set_status(format!(
                "{} sessions cannot be resumed from a shell",
                session.provider.as_str()
            ));
            return;
        };
        if !crate::shim::is_command(&argv[0]) {
            self.set_status(format!("{} is not installed on this machine", argv[0]));
            return;
        }
        // Two agents appending to one transcript is not something any of the
        // harnesses coordinate, so the running case asks first.
        if session.is_running() {
            self.mode = Mode::ResumeConfirm;
            return;
        }
        self.resume_now();
    }

    /// Resume the selected session, having decided that it should be.
    pub(super) fn resume_now(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let Some(argv) = session.resume_argv() else {
            return;
        };
        // Resumed under the account the transcript lives in, not under
        // whatever the launcher last chose. For Codex this is the difference
        // between resuming and not: a session id under `~/.codex-work` does not
        // exist under `~/.codex`, so `codex resume <id>` would start a blank
        // session and report nothing wrong. The row already knows which account
        // it came from — `Session::profile` is stamped from the transcript path.
        let profile = session
            .profile
            .as_deref()
            .and_then(|name| crate::config::profile_named(session.provider, name));
        let argv = match profile {
            Some(profile) => crate::config::argv_under_profile(argv, profile),
            None => argv,
        };
        // The transcript is full of paths relative to where the agent ran, so a
        // resumed session belongs in the same directory.
        let cwd = session.work_dir();
        let what = format!("{} · {}", session.display_label(), argv[0]);
        // What the tab is called: the agent, then which session it is. The
        // command cannot say the second half without spelling out a uuid, and
        // the uuid is the half nobody reads.
        let label = format!(
            "{} · {}",
            argv[0],
            crate::util::truncate(session.display_label(), TAB_LABEL_CHARS)
        );
        // Named after the session, so resuming it a second time reattaches to
        // the agent already doing it rather than starting a rival.
        let rmux = crate::rmux::name_for_session(session.provider.as_str(), &session.session_id);

        // Already on screen: switch to it. rmux would attach a second client to
        // the same agent, which works but leaves two panes fighting over one
        // window's size for no reason.
        //
        // Asked of `resumed` as well as of `rmux`, because without rmux
        // installed every pane's `rmux` is `None` and the question would answer
        // "no" every time — putting a second agent on one transcript, which is
        // the thing `ResumeConfirm` exists to warn about and which would happen
        // here with no warning at all, the session having already stopped.
        if let Some(at) = self.tabs.iter().position(|tab| {
            // `sessions`, not just the panes: the tab may be one this cctop has
            // no client on — another cctop's, or one it detached from itself —
            // and resuming into a second agent is exactly what this guards.
            tab.sessions().any(|name| name == rmux)
                || tab
                    .panes
                    .iter()
                    .any(|p| p.resumed.as_deref() == Some(&rmux))
        }) {
            self.go_to_tab(at + 1);
            self.set_status(format!("Already open: {what}"));
            return;
        }

        let Some(own) = self.own_preferring_rmux(Deferred::Resume, || rmux.clone()) else {
            return;
        };
        // Reattaching is not resuming: the agent was never gone, so saying
        // "resumed" would misdescribe what just happened.
        let verb = match &own {
            tabs::Own::Tmux(name) if crate::rmux::exists(name) => "Reattached to",
            _ => "Resumed",
        };
        self.open_tab(
            &argv,
            NewTab {
                cwd,
                what: &what,
                own,
                verb,
                resumed: Some(rmux),
                label: Some(label),
                profile: profile.map(|p| p.name.clone()),
            },
        );
    }

    /// Where the agent about to start should live, offering to install rmux if
    /// that is the only reason it would not be rmux-backed.
    ///
    /// `None` means the question is on screen and the caller must stop. The
    /// launch is not held anywhere in the meantime — [`Deferred`] records only
    /// which of the two entry points to run again once there is an answer.
    ///
    /// The silent fallback is kept for every machine where the question cannot
    /// be usefully asked — no package manager, or no way to reach root. rmux is
    /// how this is *better*, not how it works, and such a machine gets exactly
    /// the behaviour cctop had before rather than a complaint about a program
    /// the user never asked for. The offer exists for the machine where the
    /// fallback would instead quietly cost the user a feature one keypress away.
    fn own_preferring_rmux(
        &mut self,
        deferred: Deferred,
        name: impl FnOnce() -> String,
    ) -> Option<tabs::Own> {
        if crate::rmux::available() {
            return Some(tabs::Own::Tmux(name()));
        }
        // Asked in this order so that installing rmux in another window still
        // works: `available` above is the live check, and neither a previous
        // "no" nor a running install is consulted until it has said no.
        if self.rmux_declined || self.rmux_installing.is_some() {
            return Some(tabs::Own::Cctop);
        }
        // No package manager to offer means there is nothing to ask about, so
        // this is the plain fallback rather than a refusal: `?` here would
        // return `None`, which the caller reads as "the launch is waiting on an
        // answer" — and no answer would ever come, so the tab never opened.
        let Some(install) = crate::rmux::installer() else {
            return Some(tabs::Own::Cctop);
        };
        self.rmux_install = Some(install);
        self.rmux_deferred = Some(deferred);
        self.mode = Mode::TmuxInstall;
        self.needs_redraw = true;
        None
    }

    /// Answer the rmux offer: run the install in a pane, or give up on rmux for
    /// this run and start the agent on cctop's own pty.
    pub(super) fn rmux_install_answer(&mut self, install: bool) {
        self.mode = Mode::List;
        let Some(offer) = self.rmux_install.take() else {
            return;
        };
        if !install {
            self.rmux_declined = true;
            self.run_deferred_launch();
            return;
        }
        // In a pane, not a subprocess: `sudo` wants a password, and a pane is a
        // pty the user can type it into. It also puts the package manager's
        // output somewhere it can be read, which is the difference between a
        // failed install and a tab that closed for no stated reason.
        match tabs::Pane::launch(&offer.argv, None, tabs::Own::Cctop) {
            Ok(pane) => {
                self.rmux_installing = Some(pane.pid);
                self.tabs.push(tabs::Tab::new(pane));
                self.go_to_tab(self.tabs.len());
                self.set_status(format!("Installing rmux with {}", offer.manager));
            }
            Err(error) => {
                self.set_status(format!("Could not run the install: {error}"));
                self.rmux_declined = true;
                self.run_deferred_launch();
            }
        }
    }

    /// Watch a running install to whichever of its two ends it reaches.
    ///
    /// Called from the poll loop after panes are reaped, so "the pane is gone"
    /// is already true here rather than true one tick later.
    pub(super) fn poll_rmux_install(&mut self) {
        let Some(pid) = self.rmux_installing else {
            return;
        };
        if crate::rmux::available() {
            self.rmux_installing = None;
            self.set_status("rmux installed");
            self.run_deferred_launch();
            return;
        }
        // The pane is gone and rmux is still not here: the install failed, or
        // the user closed it. Either way the launch has waited long enough, and
        // it goes where it would have gone had nothing been offered.
        let open = self
            .tabs
            .iter()
            .flat_map(|tab| tab.panes.iter())
            .any(|pane| pane.pid == pid);
        if !open {
            self.rmux_installing = None;
            self.rmux_declined = true;
            if self.rmux_deferred.is_some() {
                self.set_status("rmux was not installed — starting without it");
            }
            self.run_deferred_launch();
        }
    }

    /// Re-run whichever launch stopped to ask about rmux.
    fn run_deferred_launch(&mut self) {
        match self.rmux_deferred.take() {
            Some(Deferred::Resume) => self.resume_now(),
            Some(Deferred::Launch) => self.launch_selected(),
            None => {}
        }
    }

    /// Start `argv` in a new tab, reporting what happened either way.
    ///
    /// `resumed` names the session the tab is going back to, when it is going
    /// back to one — what the next resume of it looks itself up by.
    fn open_tab(&mut self, argv: &[String], tab: NewTab<'_>) {
        let NewTab {
            cwd,
            what,
            own,
            verb,
            resumed,
            label,
            profile,
        } = tab;
        let mut pane = match tabs::Pane::launch(argv, cwd.as_deref(), own) {
            Ok(pane) => pane,
            Err(error) => {
                self.set_status(format!("Could not start {what}: {error}"));
                return;
            }
        };
        pane.resumed = resumed;
        pane.profile = profile;
        if let Some(label) = label {
            pane.label = label;
        }
        // Worth saying once per tab: it changes what quitting cctop means.
        let kept = match pane.outlives_cctop() {
            true => " — it will outlive cctop",
            false => "",
        };
        self.tabs.push(tabs::Tab::new(pane));
        self.go_to_tab(self.tabs.len());
        let where_ = cwd
            .map(|dir| format!(" in {}", crate::util::tildify(&dir.to_string_lossy())))
            .unwrap_or_default();
        self.set_status(format!("{verb} {what}{where_}{kept}"));
    }

    /// Reconcile the tab bar against every cctop-owned rmux session on this
    /// machine, so all the cctops running here show one set of tabs.
    ///
    /// There is no protocol here and no state file, because rmux is already the
    /// shared registry: a tab *is* one of its sessions, the sessions outlive the
    /// cctop that started them, and any cctop can list them. Open a tab in one
    /// window and it appears in the others within [`SHARE_EVERY`]; end its agent
    /// and it leaves them all, for the same reason.
    ///
    /// Only detached tabs are retired here. A tab this cctop is holding a client
    /// on has the reap to notice its agent leaving, which it does the moment the
    /// pty closes rather than at the next sweep.
    ///
    /// New sessions are appended oldest-first, which is the order a cctop that
    /// watched them start already has them in. That is what keeps the bars in
    /// agreement — and with them what Alt+3 means — rather than a cctop opened
    /// later listing the same tabs backwards.
    pub(super) fn sync_shared_tabs(&mut self) {
        if self.shared_at.is_some_and(|at| at.elapsed() < SHARE_EVERY) {
            return;
        }
        self.shared_at = Some(Instant::now());
        let running = crate::rmux::running();

        let was = self.tab;
        let mut index = 0;
        let mut retired: Vec<usize> = Vec::new();
        self.tabs.retain(|tab| {
            index += 1;
            let gone = tab.shared.as_ref().is_some_and(|s| {
                // Asked twice, because the listing failing wholesale and every
                // session having ended look identical from here — an empty
                // answer would otherwise empty the tab bar every time the rmux
                // server was restarted. The second question only gets asked
                // about a tab already on its way out, so it costs nothing per
                // sweep.
                !running.iter().any(|agent| agent.name == s.name) && !crate::rmux::exists(&s.name)
            });
            if !gone {
                return true;
            }
            // Where the view ends up is [`land_after`]'s answer, below: a tab
            // retired out from under it has the same two corrections as one
            // closed by hand.
            //
            // [`land_after`]: Self::land_after
            retired.push(index);
            false
        });
        if !retired.is_empty() {
            self.land_after(was, &retired);
        }

        // What rmux now says about the tabs already here. Activity above all:
        // it is how a tab nobody is attached to knows its agent has stopped, and
        // a reading taken once when the tab appeared would have it idle forever.
        for tab in &mut self.tabs {
            let Some(shared) = tab.shared.as_mut() else {
                continue;
            };
            let Some(agent) = running.iter().find(|a| a.name == shared.name) else {
                continue;
            };
            shared.activity = agent.activity;
            // Both can arrive late: the pid in the moment before rmux has
            // spawned the command, the label when the cctop that owns the tab
            // has not written it yet.
            shared.pid = agent.pid.or(shared.pid);
            // Kept rather than overwritten when rmux has nothing: the pane that
            // launched this agent knew its account before the option landed on
            // the session, and a sweep in that window must not forget it.
            shared.profile = agent.profile.clone().or_else(|| shared.profile.take());
            if let Some(label) = &agent.label {
                shared.label = label.clone();
            }
        }

        let mine = self.open_rmux();
        let mut arrived = false;
        for agent in running.iter().rev() {
            if mine.iter().any(|name| name == &agent.name) {
                continue;
            }
            self.tabs.push(tabs::Tab::shared(agent));
            arrived = true;
        }
        self.needs_redraw |= !retired.is_empty() || arrived;
    }

    /// The rmux sessions this cctop already has a pane onto.
    pub fn open_rmux(&self) -> Vec<String> {
        self.tabs
            .iter()
            .flat_map(tabs::Tab::sessions)
            .map(str::to_string)
            .collect()
    }

    /// What the launcher is offering.
    pub fn launch_choices(&self) -> &[tabs::Choice] {
        &self.launch_offer
    }

    /// The profile a launch would use, or `None` when the highlighted command
    /// takes none — or takes one but has only a single account, so there is
    /// nothing to choose between.
    pub fn launch_profile(&self) -> Option<&'static crate::config::Profile> {
        self.chosen_profile(self.launch_provider()?)
    }

    /// Which harness the highlighted choice would start, when it is one whose
    /// account cctop can pick.
    fn launch_provider(&self) -> Option<Provider> {
        match self.launch_offer.get(self.launch_cursor) {
            Some(tabs::Choice::Start(argv)) => Self::profile_provider(argv),
            _ => None,
        }
    }

    /// The profile `provider` would be started under, or `None` when it has
    /// only the one and so nothing to choose between.
    pub fn chosen_profile(&self, provider: Provider) -> Option<&'static crate::config::Profile> {
        let profiles = crate::config::profiles_for(provider);
        if profiles.len() <= 1 {
            return None;
        }
        let at = self.launch_profile.get(&provider).copied().unwrap_or(0);
        profiles.get(at).copied()
    }

    /// Move to the next profile of the highlighted harness. Wraps, because with
    /// two — which is the case this exists for — a key that toggles is the whole
    /// interaction.
    pub(super) fn cycle_launch_profile(&mut self) {
        let Some(provider) = self.launch_provider() else {
            return;
        };
        let n = crate::config::profiles_for(provider).len();
        if n > 1 {
            let at = self.launch_profile.entry(provider).or_insert(0);
            *at = (*at + 1) % n;
            self.save_prefs();
            self.needs_redraw = true;
        }
    }

    /// Which harness `argv` starts, when it is one whose account is selected by
    /// an environment variable — and so one a profile means something to.
    ///
    /// `$CLAUDE_CONFIG_DIR` is Claude's and `$CODEX_HOME` is Codex's; putting
    /// either in front of anything else would be a promise the env var cannot
    /// keep.
    pub(super) fn profile_provider(argv: &[String]) -> Option<Provider> {
        let command = argv
            .first()
            .map(|c| c.rsplit(['/', '\\']).next().unwrap_or(c))?;
        match command.strip_suffix(".exe").unwrap_or(command) {
            "claude" => Some(Provider::Claude),
            "codex" => Some(Provider::Codex),
            _ => None,
        }
    }

    /// Put the chosen profile in front of the command that will read it.
    ///
    /// `env VAR=value cmd` rather than plumbing an environment through every
    /// spawn path: the same argv is handed to rmux, to a pty cctop owns, and to
    /// `rmux new-session`, and `env` is understood identically by all three.
    /// [`tabs::label_of`] drops the prefix again so the tab is named after the
    /// agent rather than after how it was started.
    fn with_profile(&self, argv: Vec<String>) -> Vec<String> {
        let profile = Self::profile_provider(&argv).and_then(|p| self.chosen_profile(p));
        let Some(profile) = profile else {
            return argv;
        };
        crate::config::argv_under_profile(argv, profile)
    }

    /// The argv that resumes a new agent onto a copy of the session being
    /// handed over, when that is possible and `argv` is the agent that can read
    /// it.
    ///
    /// The copy lands in the *receiving* account's directory, which is not
    /// always the sending one's: handing a personal session to a work login has
    /// to put the transcript where that login will look for it.
    ///
    /// A failure to copy is reported and answered with `None`, which puts the
    /// launch back on the brief — the handoff still happens, with less of the
    /// conversation in it.
    fn fork_pending(&mut self, argv: &[String]) -> Option<Vec<String>> {
        let transcript = self.pending_fork.clone()?;
        if crate::handoff::command_of(argv) != Some("claude") {
            return None;
        }
        let profile = self.chosen_profile(Provider::Claude);
        let config_dir = profile
            .map(|p| p.dir.clone())
            .unwrap_or_else(|| crate::config::CLAUDE_CONFIG_DIR.clone());
        match crate::handoff::fork(&transcript, &config_dir) {
            Ok(id) => {
                let argv = vec!["claude".to_string(), "--resume".to_string(), id];
                Some(match profile {
                    Some(profile) => crate::config::argv_under_profile(argv, profile),
                    None => argv,
                })
            }
            Err(error) => {
                self.set_status(format!("Could not copy the transcript: {error}"));
                None
            }
        }
    }

    /// What a still-running agent in the launcher is doing, if it has said.
    ///
    /// This is the whole reason the offer carries a pid. A list of rmux session
    /// names says which agents exist; this says which one is stuck on a question
    /// and which finished ten minutes ago, from the same hooks the dashboard
    /// reads — so choosing which to go back to is a decision rather than a guess.
    pub fn waiting_state(&self, agent: &crate::rmux::Running) -> Option<crate::hook::Signal> {
        self.pane_signal(agent.pid?)
    }

    /// What to call a still-running agent, when cctop can do better than its
    /// rmux session name.
    ///
    /// That name is an identity and not something written to be read: a resumed
    /// session's carries the whole session id, so it comes out as a timestamp
    /// and a uuid that no two rows differ in until well past the width of the
    /// column. The agent's pid finds its row, and the row already knows what the
    /// dashboard calls it — which is the name the user recognises.
    pub fn waiting_label(&self, agent: &crate::rmux::Running) -> Option<String> {
        let pid = agent.pid?;
        self.sessions
            .iter()
            .find(|session| session.root_pid() == Some(pid))
            .map(|session| session.display_label().to_string())
    }

    /// Whether the launcher's pick is an agent already running somewhere.
    ///
    /// Reattaching lands wherever that agent already is, so a directory typed
    /// for it would be accepted and then ignored — which is worse than the key
    /// not being offered.
    pub(super) fn launch_is_reattach(&self) -> bool {
        matches!(
            self.launch_offer.get(self.launch_cursor),
            Some(tabs::Choice::Waiting(_))
        )
    }

    /// Open the launcher's directory field, prefilled with where it would go.
    ///
    /// Prefilled with `~` spelling rather than the absolute path: that is how
    /// the line already reads, and a field that changed what it showed the
    /// moment it became editable would look like it had lost the setting.
    pub(super) fn edit_launch_cwd(&mut self) {
        self.launch_cwd_input = self
            .launch_cwd
            .as_ref()
            .map(|dir| crate::util::tildify(&dir.to_string_lossy()))
            .unwrap_or_default();
        self.launch_cwd_bad = false;
        self.launch_cwd_known = self.known_dirs();
        self.launch_cwd_suggest();
        self.mode = Mode::LaunchCwd;
        self.needs_redraw = true;
    }

    /// Directories agents are known to have run in, newest first.
    ///
    /// Drawn from the dashboard's own rows, which is the list of projects cctop
    /// has any evidence of, plus where it was started and where this launch was
    /// already headed — those two are what "in this directory" and the line the
    /// field replaces already meant, and a suggestion list that omitted them
    /// would look like it had forgotten them.
    ///
    /// Only directories that still exist: a session's recorded project can have
    /// been moved or deleted since, and offering one leads to the refusal this
    /// list exists to avoid.
    fn known_dirs(&self) -> Vec<std::path::PathBuf> {
        let mut recent: Vec<&Session> = self.sessions.iter().collect();
        // Newest first, so the projects worked on today are the ones on screen
        // before anything is typed.
        recent.sort_by(|a, b| b.last_active.cmp(&a.last_active));

        let mut seen = HashSet::new();
        self.launch_cwd
            .clone()
            .into_iter()
            .chain(self.launch_root.clone())
            .chain(
                self.launch_offer
                    .iter()
                    .filter_map(|c| c.cwd().map(std::path::Path::to_path_buf)),
            )
            .chain(
                recent
                    .iter()
                    .filter(|s| !s.label_source.is_empty())
                    .map(|s| std::path::PathBuf::from(&s.label_source)),
            )
            .filter(|dir| seen.insert(dir.clone()) && dir.is_dir())
            .take(MAX_KNOWN_DIRS)
            .collect()
    }

    /// Recompute what the field is offering, after anything that changed what
    /// is in it.
    ///
    /// The pick goes with it: a suggestion highlighted for the old text would
    /// otherwise still be what Enter took, which is a directory the field is no
    /// longer showing.
    pub(super) fn launch_cwd_suggest(&mut self) {
        self.launch_cwd_hits = dirs::suggest(&self.launch_cwd_input, &self.launch_cwd_known);
        self.launch_cwd_pick = None;
    }

    /// Move the cursor through the suggestions, or back into the text.
    ///
    /// Leaving the list at the top rather than wrapping to the bottom is what
    /// makes the text reachable again: the field is the thing being edited, and
    /// a list that cycled would trap the cursor in it.
    pub(super) fn step_launch_cwd(&mut self, down: bool) {
        let last = self.launch_cwd_hits.len().saturating_sub(1);
        self.launch_cwd_pick = match (self.launch_cwd_pick, down) {
            (_, _) if self.launch_cwd_hits.is_empty() => None,
            (None, true) => Some(0),
            (None, false) => Some(last),
            (Some(i), true) => Some((i + 1).min(last)),
            (Some(0), false) => None,
            (Some(i), false) => Some(i - 1),
        };
        self.needs_redraw = true;
    }

    /// Fill in as much of the path as the suggestions agree on.
    ///
    /// Completing re-suggests: `~/c` completing to `~/cctop/` is only useful if
    /// the list then shows what is inside it, which is how a deep path gets
    /// walked to without being remembered.
    pub(super) fn complete_launch_cwd(&mut self) {
        // The highlighted one, if the cursor is in the list — there Tab means
        // "that one", the same as Enter, minus the launching.
        let filled = match self
            .launch_cwd_pick
            .and_then(|i| self.launch_cwd_hits.get(i))
        {
            Some(dir) => format!("{}/", crate::util::tildify(&dir.to_string_lossy())),
            None => match dirs::complete(&self.launch_cwd_input, &self.launch_cwd_hits) {
                Some(filled) => filled,
                None => return,
            },
        };
        if filled.chars().count() > input::MAX_PATH_INPUT {
            return;
        }
        self.launch_cwd_input = filled;
        self.launch_cwd_bad = false;
        self.launch_cwd_suggest();
        self.needs_redraw = true;
    }

    /// Take the highlighted suggestion, or what is typed if there is none.
    ///
    /// A suggestion is a directory this code listed off the disk moments ago, so
    /// it is taken without the check the typed path gets — and it is taken by
    /// filling the field with it first, so that a suggestion which has since
    /// been deleted is refused in the field like anything else.
    pub(super) fn take_launch_cwd(&mut self) {
        if let Some(dir) = self
            .launch_cwd_pick
            .and_then(|i| self.launch_cwd_hits.get(i))
        {
            self.launch_cwd_input = crate::util::tildify(&dir.to_string_lossy());
        }
        self.accept_launch_cwd();
    }

    /// Take the typed directory, if it names one.
    ///
    /// Checked here rather than at launch. A path that does not exist fails
    /// somewhere inside the shim with a message about spawning, by which point
    /// the launcher is gone and there is nothing left to correct.
    pub(super) fn accept_launch_cwd(&mut self) {
        let typed = self.launch_cwd_input.trim();
        // Empty means "wherever cctop was started", which is what the launcher
        // offers by default and what the footer calls "this directory".
        let taken = match typed.is_empty() {
            true => None,
            false => {
                let path = std::path::PathBuf::from(crate::util::untildify(typed));
                if !path.is_dir() {
                    self.launch_cwd_bad = true;
                    return;
                }
                Some(path)
            }
        };
        // Cleared on the way out, not only on the way in: a path corrected
        // after a refusal would otherwise carry the mark back to a field that
        // now holds something perfectly good.
        self.launch_cwd_bad = false;
        self.launch_cwd = taken;
        self.mode = Mode::Launch;
    }

    /// Start the launcher's pick.
    pub fn launch_selected(&mut self) {
        let Some(choice) = self.launch_offer.get(self.launch_cursor).cloned() else {
            return;
        };
        let cwd = self.launch_cwd.clone();
        // Read off the choice rather than the argv below, which by then carries
        // the `env VAR=value` prefix a profile is passed through.
        let starting = match &choice {
            tabs::Choice::Start(argv) => Self::profile_provider(argv),
            tabs::Choice::Waiting(_) => None,
        };
        let (argv, own) = match &choice {
            // Reattaching: the agent chose its own command long ago, and the
            // argv here only names the tab.
            tabs::Choice::Waiting(agent) => (
                vec![choice.label()],
                tabs::Own::TmuxExisting(agent.name.clone()),
            ),
            // A fresh agent has no identity to be idempotent about — two
            // `claude` tabs are two agents — so this takes the next free name
            // rather than a derived one.
            tabs::Choice::Start(argv) => {
                let own = self.own_preferring_rmux(Deferred::Launch, || {
                    crate::rmux::free_name(&tabs::label_of(argv))
                });
                // The offer went up instead. This runs again from the top when
                // it is answered, and the launcher's snapshot is still here to
                // run it from.
                let Some(own) = own else { return };
                (self.with_profile(argv.clone()), own)
            }
        };
        // The offer is a snapshot, and an agent can finish in the time the modal
        // is up. Attaching to a session that has gone spawns a client that exits
        // at once — a tab that flickers and vanishes, where the truth is simply
        // that the agent ended while being looked at.
        if let tabs::Choice::Waiting(agent) = &choice
            && !crate::rmux::exists(&agent.name)
        {
            self.set_status(format!("{} has ended", choice.label()));
            return;
        }

        // A handoff goes to an agent that is starting fresh. Reattaching lands
        // in a conversation already under way, where a "read this and continue"
        // line would interrupt whatever it is doing mid-turn.
        let fresh = matches!(choice, tabs::Choice::Start(_));
        // Claude to Claude the conversation itself is handed over rather than a
        // summary of it, the receiving agent being resumed onto a copy of the
        // transcript. Everything else gets the brief, which is the only form it
        // can read.
        let forked = fresh.then(|| self.fork_pending(&argv)).flatten();
        let carrying_conversation = forked.is_some();
        let argv = forked.unwrap_or(argv);
        // Only where the fork did not happen: an agent that cannot read the
        // transcript, or one whose copy could not be written.
        let brief = match fresh && !carrying_conversation {
            true => self.pending_brief.clone(),
            false => None,
        };
        let line = brief.as_deref().map(crate::handoff::prompt_for);
        // Handed over in the argv wherever the harness takes an opening prompt;
        // `opening_argv` says why that is not the same as typing it.
        let opening = line
            .as_deref()
            .and_then(|line| crate::handoff::opening_argv(&argv, line));
        let argv = &argv;
        let mut pane =
            match tabs::Pane::launch(opening.as_ref().unwrap_or(argv), cwd.as_deref(), own) {
                Ok(pane) => pane,
                Err(error) => {
                    self.set_status(format!("Could not start {}: {error}", tabs::label_of(argv)));
                    return;
                }
            };
        // The profile is only knowable here: it reached the agent as an
        // environment variable, which nothing downstream can read back. A fresh
        // agent takes the account the launcher was showing; one being reattached
        // takes the one it was started under, which the sweep read back off its
        // rmux session.
        pane.profile = match &choice {
            tabs::Choice::Start(_) => self.launch_profile().map(|p| p.name.clone()),
            tabs::Choice::Waiting(agent) => agent.profile.clone(),
        };
        // The tab is named after the agent, not after the brief it was handed:
        // `Pane::launch` names it from the argv it was given, and that argv now
        // ends in a paragraph — or, for a fork, in the uuid of the copy, which
        // is the half of a resume nobody reads.
        if opening.is_some() {
            pane.label = tabs::label_of(argv);
        } else if carrying_conversation {
            pane.label = "claude".to_string();
        }
        let label = pane.label.clone();
        let mut carried = "";
        if carrying_conversation {
            // Both are spent: the copy is what the agent is reading, and the
            // brief that was written alongside it has no reader left.
            self.pending_fork = None;
            self.pending_brief = None;
            carried = " with the conversation";
        }
        if let Some(line) = line {
            self.pending_brief = None;
            match opening.is_some() {
                // Already in the agent's argv — there is nothing left to send,
                // and the agent opens on the brief instead of on an empty prompt.
                true => carried = " with the brief",
                // No prompt argument to use, so it goes the old way: typed once
                // the agent has had long enough to be listening.
                false => {
                    self.handoff_send = Some((pane.pid, line, Instant::now() + HANDOFF_SETTLE));
                }
            }
        }
        let kept = match pane.outlives_cctop() {
            true => " — it will outlive cctop",
            false => "",
        };
        match self.launch_into {
            LaunchInto::Split { stacked } => {
                let Some(tab) = self.active_tab() else { return };
                tab.stacked = stacked;
                tab.panes.push(pane);
                tab.focus = tab.panes.len() - 1;
            }
            LaunchInto::Tab => {
                self.tabs.push(tabs::Tab::new(pane));
                self.go_to_tab(self.tabs.len());
            }
        }
        // Reattaching is not starting, and it does not land in the launcher's
        // directory: the agent has been working somewhere since before any of
        // this and stays there. Saying "Started ... in ~/here" would be wrong
        // twice over.
        self.set_status(match &choice {
            tabs::Choice::Waiting(agent) => {
                let at = agent
                    .cwd
                    .as_ref()
                    .map(|dir| format!(" in {}", crate::util::tildify(&dir.to_string_lossy())))
                    .unwrap_or_default();
                format!("Reattached to {label}{at} — it was never gone")
            }
            tabs::Choice::Start(_) => {
                let where_ = cwd
                    .map(|dir| format!(" in {}", crate::util::tildify(&dir.to_string_lossy())))
                    .unwrap_or_default();
                // Only for a fresh Codex, and only where its hooks are waiting
                // to be trusted. A reattached one is deliberately left out: it
                // has been running since before whatever is installed now, so
                // the answer for it is a restart rather than a keystroke.
                let trust = match starting {
                    Some(Provider::Codex) => self.codex_trust_hint(
                        crate::hook::codex_hooks_installed(self.hook_project().as_deref()),
                    ),
                    _ => "",
                };
                format!("Started {label}{where_}{carried}{kept}{trust}")
            }
        });
    }

    /// Close the focused pane, ending the agent behind it.
    ///
    /// Closing used to detach from a rmux-backed agent and leave it running,
    /// which meant the tab came back at the next launch and the only way to be
    /// rid of it was a second key. Closing a window is meant to be the end of
    /// it, so this kills the rmux session outright — the same thing Alt+Shift+W
    /// does, which is now a synonym rather than the only way to stop an agent.
    ///
    /// The exception is a pane opened with `a`, which is a window onto somebody
    /// else's agent. There is nothing here to kill and stopping it was never
    /// cctop's to do, so that one is only closed.
    pub fn close_pane(&mut self) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        // A tab standing for a session no client of ours is on has no pane here
        // to close — but the agent is still cctop's to end, and this tab is the
        // only handle on screen for it. So the key means what it means anywhere
        // else, and the tab leaves every cctop rather than just this one.
        if tab.detached()
            && let Some(shared) = tab.shared.take()
        {
            let stopped = crate::rmux::kill(&shared.name);
            self.drop_empty_tabs();
            self.set_status(match stopped {
                Err(error) => format!("Could not stop {}: {error}", shared.label),
                Ok(()) => format!("Stopped {}", shared.label),
            });
            return;
        }
        if tab.focus >= tab.panes.len() {
            return;
        }
        // Out of the tab first: for a cctop-owned pty, dropping the pane is the
        // kill, and it must happen either way rather than only when rmux agrees.
        let pane = tab.panes.remove(tab.focus);
        tab.focus = tab.focus.min(tab.panes.len().saturating_sub(1));
        let label = pane.label.clone();
        let stopped = pane.owns_agent().then(|| pane.kill_agent());
        drop(pane);
        self.drop_empty_tabs();

        self.set_status(match stopped {
            Some(Err(error)) => format!("Closed {label}, but could not stop it: {error}"),
            Some(Ok(())) => format!("Stopped {label}"),
            None => format!("Closed the view of {label} — it is not cctop's to stop"),
        });
    }

    /// End the focused pane's agent outright. A synonym for [`close_pane`],
    /// kept because it is documented and in muscle memory.
    ///
    /// [`close_pane`]: Self::close_pane
    pub fn kill_pane(&mut self) {
        self.close_pane();
    }

    /// Forget the tabs whose agents have all exited, keeping the view on
    /// something that still exists.
    pub fn drop_empty_tabs(&mut self) {
        let was = self.tab;
        let mut gone: Vec<usize> = Vec::new();
        let mut index = 0;
        self.tabs.retain(|tab| {
            index += 1;
            // A detached tab holds no pane on purpose; only the sync retires it.
            if !tab.panes.is_empty() || tab.shared.is_some() {
                return true;
            }
            gone.push(index);
            false
        });
        if gone.is_empty() {
            return;
        }
        self.land_after(was, &gone);
    }

    /// Where the view goes once `gone` — tab numbers, ascending — have left the
    /// bar, given that it was on `was`.
    ///
    /// Two different corrections, which is why closing a tab used to be
    /// confusing. A view on a *later* tab has to slide down by however many
    /// went before it, or closing tab 1 silently moves you to what used to be
    /// tab 2. A view on the tab that just closed has nowhere to slide to: its
    /// number now belongs to the tab that was next to it, which is the one to
    /// land on — walking backwards to the previous tab instead is what dropped
    /// you onto the dashboard every time you closed the first tab.
    ///
    /// And it lands through [`go_to_tab`] rather than by assignment, because a
    /// tab another cctop is on holds no pane until this one attaches to it: set
    /// directly, the view arrives on a tab that draws nothing at all.
    ///
    /// [`go_to_tab`]: Self::go_to_tab
    fn land_after(&mut self, was: usize, gone: &[usize]) {
        let want = Self::landing(was, gone, self.tabs.len());
        // `go_to_tab` answers a move to where it already is with nothing at
        // all, and here the field still holds the number of a tab that is gone.
        self.tab = 0;
        self.go_to_tab(want);
    }

    /// The tab number to land on, kept separate from the move itself so the
    /// arithmetic can be checked without a rmux server to attach to.
    fn landing(was: usize, gone: &[usize], remaining: usize) -> usize {
        let before = gone.iter().filter(|&&i| i < was).count();
        match gone.contains(&was) {
            // Stay on the number, unless it was the last tab in the bar — then
            // the neighbour is the one before it, and with nothing left the
            // dashboard is all there is to land on.
            true => (was - before).min(remaining),
            false => was - before,
        }
    }

    /// Open the selected agent's terminal in a browser, via the multiplexer.
    ///
    /// Only reaches agents cctop handed to the multiplexer, which is the same
    /// limit `a` has and for the same reason: an agent on cctop's own pty is on
    /// no terminal a second viewer can be pointed at, so there is nothing for
    /// `web-share -t` to name.
    ///
    /// The operator link goes to the clipboard and never to the screen. It
    /// grants input to a live coding agent, and a status line is read by
    /// whoever is behind you and survives into a screenshot; the clipboard is
    /// where the user was going to put it anyway. The pairing code is shown,
    /// since it is worth nothing without the link.
    pub(super) fn share_selected(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let label = session.display_label().to_string();
        let Some(pid) = session.root_pid() else {
            self.set_status("Selected session has no local process");
            return;
        };
        let Some(name) = crate::rmux::holding(pid) else {
            self.set_status("Only an agent cctop put in a multiplexer can be shared");
            return;
        };
        let reachable_at = self.share_route();
        match crate::rmux::web_share(&name, reachable_at.as_deref()) {
            Ok(share) => {
                // `web_share` refuses a share with no operator link, so this is
                // the link or the error above it — never the spectator one.
                let Some(operator) = share.operator.as_deref() else {
                    self.set_status(format!("Could not share {label}: no operator link"));
                    return;
                };
                render::copy_to_clipboard(operator);
                let pin = match &share.pin {
                    Some(pin) => format!(" · pin {pin}"),
                    None => String::new(),
                };
                // Where the link reaches from is the one thing about a share
                // that is not on the link: an operator URL looks the same
                // whether its endpoint is a tunnel or this machine's loopback,
                // and sending someone a link that cannot leave the building is
                // a failure they discover instead of being told about.
                let reach = match reachable_at {
                    Some(_) => "",
                    None => " · this machine only",
                };
                self.set_status(format!(
                    "Sharing {label} — operator link copied{pin}{reach}"
                ));
            }
            Err(error) => self.set_status(format!("Could not share {label}: {error}")),
        }
    }

    /// Start serving the table to a browser, or say why not.
    ///
    /// `tunnel` is the difference between a link that works on this machine and
    /// one that works from a phone. It is asked for per start rather than
    /// toggled on a running server: registering with the edge is what mints the
    /// hostname, so turning it on means a new link either way, and a flag that
    /// silently invalidated the link somebody was holding would be worse than a
    /// stop and a start they can see.
    pub(super) fn start_serving(&mut self, tunnel: bool) {
        self.serve_error = None;
        let options = crate::serve::Options {
            tunnel,
            plan: self.plan,
            // Fed from the rows this dashboard already has. Two loaders in one
            // process would walk the same disk twice and, worse, could disagree
            // — a page saying one thing while the table beside it says another
            // is the bug nobody thinks to look for.
            scan: false,
            ..Default::default()
        };
        match crate::serve::start(options) {
            Ok(serving) => {
                // Something to look at immediately: the page's first request
                // would otherwise find the empty snapshot it was built with and
                // report a machine with no sessions on it.
                serving.publish(&self.sessions);
                let where_to = match serving.public.is_some() {
                    true => "on the internet",
                    false => "on this machine",
                };
                self.set_status(format!("Serving {where_to} — B for the link"));
                self.serving = Some(serving);
            }
            Err(error) => {
                let error = format!("{error}");
                self.set_status(format!("Could not serve: {error}"));
                self.serve_error = Some(error);
            }
        }
    }

    /// Stop serving, which un-mints every link handed out.
    pub(super) fn stop_serving(&mut self) {
        if self.serving.take().is_some() {
            self.set_status("Stopped serving — the links no longer answer");
        }
    }

    /// Show the page whatever the table is showing.
    ///
    /// Called wherever the rows change rather than on a timer of its own: the
    /// page's event stream wakes on a new version, so this is also what makes a
    /// browser update when the table does.
    pub(super) fn feed_serving(&self) {
        if let Some(serving) = &self.serving {
            serving.publish(&self.sessions);
        }
    }

    /// The public origin a share's browser should dial, opening a tunnel for it
    /// the first time one is asked for.
    ///
    /// `None` means the share stays on this machine, and is a real answer twice
    /// over: rmux's daemon may not be up yet — in which case there is nothing
    /// to point a tunnel at and no session to share either — and a tunnel can
    /// fail to register. A link that only works from this machine is worth
    /// more than no link, so neither of those stops the share; what they cost
    /// is said on the status line by the caller.
    ///
    /// Blocking, for as long as it takes Cloudflare's edge to answer. That is a
    /// visible pause on the first `W` of a run and nothing on the ones after —
    /// the alternative, a thread and a poll, buys a second of responsiveness at
    /// the price of a share that arrives after the keypress that asked for it.
    fn share_route(&mut self) -> Option<String> {
        if let Some(tunnel) = &self.share_tunnel {
            return Some(tunnel.url.clone());
        }
        let port = crate::rmux::share_port()?;
        let tunnel = crate::serve::tunnel::start(port).ok()?;
        let url = tunnel.url.clone();
        self.share_tunnel = Some(tunnel);
        Some(url)
    }

    /// Put the selected agent's own terminal on screen, in a tab of its own.
    ///
    /// Two ways in, because there are two ways an agent's terminal can belong to
    /// cctop. A shim holding a pty has a copy of the output to give away; an agent
    /// handed to rmux has none, and is reached by becoming another of its clients
    /// instead. Either way this only *looks* at the agent — closing the pane
    /// detaches from it and never ends it.
    pub(super) fn attach_selected(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let label = format!("{} · {}", session.abbrev_label, session.model);
        let title = session.display_label().to_string();
        // How a resume names this same session. Recorded on the pane below so
        // that `R` afterwards finds the agent already on screen instead of
        // starting a second one on one transcript — `a` and `R` reach the same
        // agent by different routes, and only this makes them agree.
        let resumed = crate::rmux::name_for_session(session.provider.as_str(), &session.session_id);
        let Some(pid) = session.root_pid() else {
            self.set_status("Selected session has no local process");
            return;
        };
        if self.open_view(pid, label.clone()) {
            return;
        }
        // Started by cctop and then handed to rmux. Without this the message
        // below would say cctop did not start an agent cctop started, and send
        // the user to relaunch something that is already running.
        if let Some(name) = crate::rmux::holding(pid) {
            // A second client onto one session leaves the two panes arguing over
            // one window's size, so an agent already on screen is switched to.
            if let Some(at) = self
                .tabs
                .iter()
                .position(|tab| tab.sessions().any(|open| open == name))
            {
                self.go_to_tab(at + 1);
                self.set_status(format!("Already open: {label}"));
                return;
            }
            self.open_tab(
                &[title],
                NewTab {
                    cwd: None,
                    what: &label,
                    own: tabs::Own::TmuxExisting(name),
                    verb: "Attached to",
                    resumed: Some(resumed),
                    // Already a bare agent name, so the command names it right.
                    label: None,
                    // Somebody else started it; its account is whatever they
                    // chose, which the rmux session name does not record.
                    profile: None,
                },
            );
            return;
        }
        self.set_status(
            "Only sessions started by cctop can be attached — start them as `cctop claude`",
        );
    }

    /// Go back to the agent this cctop launched.
    ///
    /// Without this, F12 would be a one-way door: a freshly started agent has
    /// written no transcript yet, so it has no row in the table to press `a` on.
    pub(super) fn attach_hosted(&mut self) {
        let Some((pid, label)) = self.hosted.clone() else {
            self.set_status("No agent was launched by this cctop — start one as `cctop claude`");
            return;
        };
        if !self.open_view(pid, label) {
            self.set_status("The agent's terminal is gone");
        }
    }

    /// Take up the rmux-backed agents already running on this machine — the ones
    /// this cctop left alive on a previous exit, and the ones another cctop has
    /// open right now.
    ///
    /// The rmux session is the durable workspace state: it preserves the agent,
    /// its scrollback, and working directory. There is no difference worth
    /// drawing between a session left by a cctop that has quit and one another
    /// cctop is using, so this makes no attempt to: both are tabs, and both
    /// arrive detached. Only the one put on screen takes a client.
    ///
    /// Oldest first, matching [`sync_shared_tabs`] — a cctop opened now and one
    /// that watched these start must number their tabs the same way.
    ///
    /// [`sync_shared_tabs`]: Self::sync_shared_tabs
    pub(super) fn restore_running_tabs(&mut self) {
        for agent in crate::rmux::running().iter().rev() {
            self.tabs.push(tabs::Tab::shared(agent));
        }
        if !self.tabs.is_empty() {
            self.go_to_tab(1);
        }
    }

    /// Show the agent running as `pid`, reusing the pane already on it rather
    /// than opening a second window onto one terminal.
    ///
    /// A pane is a match on either pid it has: the one cctop hosts, and — for a
    /// rmux-backed pane, where that one is only the client — the agent's own.
    /// Asking about the hosted pid alone missed every rmux-backed pane, so an
    /// agent already on screen got a second window onto it.
    fn open_view(&mut self, pid: u32, label: String) -> bool {
        let shows = |pane: &tabs::Pane| pane.pid == pid || pane.agent() == pid;
        if let Some(index) = self.tabs.iter().position(|tab| tab.panes.iter().any(shows)) {
            let tab = &mut self.tabs[index];
            tab.focus = tab.panes.iter().position(shows).unwrap_or(0);
            self.go_to_tab(index + 1);
            return true;
        }
        // The agent may be one of the shared tabs instead, watched by no client
        // of this cctop. Switching there attaches one, which is the same window
        // onto the same agent that the branch above found — and still not a
        // second one.
        if let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.shared.as_ref().is_some_and(|s| s.pid == Some(pid)))
        {
            self.go_to_tab(index + 1);
            return true;
        }
        let Some(pane) = tabs::Pane::view_of(pid, label) else {
            return false;
        };
        self.tabs.push(tabs::Tab::new(pane));
        self.go_to_tab(self.tabs.len());
        true
    }
}

/// Columns the user has hidden outright, which win over the automatic
/// width-based dropping in [`columns::visible_columns`].
///
/// `$CCTOP_COLUMNS_HIDE` is the only source today and is meant to stay an
/// override once a persisted one exists: `UiPrefs` is the natural home for the
/// stored list, but it lives in `cache.rs`, which this module does not own, and
/// carries no such field yet. When it grows one, read it here and let a
/// non-empty env var take precedence.
fn hidden_columns(_prefs: &UiPrefs) -> Vec<ColumnId> {
    columns::parse_hidden(&std::env::var("CCTOP_COLUMNS_HIDE").unwrap_or_default())
}

/// `haystack.to_ascii_lowercase().contains(needle)` without the allocation.
///
/// Comparing bytes is safe on UTF-8 here: ASCII case folding never touches a
/// continuation byte, so a match can only start at a character boundary.
fn contains_ascii_ci(haystack: &str, lowercase_needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), lowercase_needle.as_bytes());
    if n.is_empty() {
        return true;
    }
    h.len() >= n.len()
        && h.windows(n.len())
            .any(|w| w.iter().zip(n).all(|(a, b)| a.to_ascii_lowercase() == *b))
}

/// The eight characters a Gemini chat file is named after, out of the row id
/// that file produced: `session-2026-05-14T17-34-79709c93` yields `79709c93`.
///
/// `None` for every other harness's ids, which is what keeps this from matching
/// on the tail of a uuid that happens to line up: only a stem shaped like
/// Gemini's is looked up loosely, and only ever against a full id's prefix.
fn gemini_id_tail(session_id: &str) -> Option<&str> {
    let tail = session_id.strip_prefix("session-")?.rsplit_once('-')?.1;
    (tail.len() == 8 && tail.chars().all(|c| c.is_ascii_alphanumeric())).then_some(tail)
}

// ---------------------------------------------------------------------------
// Event handling
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the UI. `hosted` is an agent cctop launched for this session, which it
/// shows attached and outlives by nothing: when the agent exits, so does cctop,
/// so `cctop claude` gets you back to your shell the way `claude` would.
pub fn run(args: &Args, hosted: Option<crate::shim::Hosted>) -> anyhow::Result<i32> {
    // Before anything draws, and once: the palette is read by every widget and
    // must not change under them mid-run.
    theme::init_from_env();

    let (req_tx, req_rx) = channel::<Request>();
    let (res_tx, res_rx) = channel::<Response>();
    let worker = spawn_worker(args.plan, req_rx, res_tx.clone());

    // Pricing and quota are network-bound; keep both off the UI thread.
    {
        let tx = res_tx.clone();
        std::thread::spawn(move || {
            crate::pricing::refresh_pricing_blocking();
            let _ = tx.send(Response::PricingReady);
        });
    }
    spawn_quota_poller(res_tx.clone());
    let hosts = crate::fleet::Host::collect(&args.hosts);
    for host in &hosts {
        spawn_host_poller(host.clone(), res_tx.clone());
    }

    // One cached check per day, off the UI thread. Only ever reports: replacing
    // the binary stays behind an explicit `--update`.
    std::thread::spawn(move || {
        if let Some(version) = crate::update::available_update() {
            let _ = res_tx.send(Response::UpdateAvailable(version));
        }
    });

    let mut app = App::new(args.plan, req_tx.clone());
    app.refresh_secs = args.delay;
    // With nothing to put in it, HOST is a column of one repeated word. Hidden
    // through the same mechanism the user has, so `$CCTOP_COLUMNS_HIDE` and this
    // cannot disagree about what is on screen.
    if hosts.is_empty() {
        app.hidden_columns.push(ColumnId::Host);
    }
    // Likewise USER: with only this user's homes in view, every row's owner is
    // the person reading the screen.
    if crate::config::OTHER_HOMES.is_empty() {
        app.hidden_columns.push(ColumnId::User);
    }
    // And PROFILE, which most machines have exactly one of. A column repeating
    // `default` down every row is a column that answers nothing.
    if crate::config::profile_count() <= 1 {
        app.hidden_columns.push(ColumnId::Profile);
    }
    let _ = req_tx.send(Request::Refresh);

    // Tabs backed by rmux outlive cctop. Reattach them before the first frame
    // so reopening the dashboard restores the workspace rather than making the
    // user find and reopen every surviving agent through the launcher.
    app.restore_running_tabs();

    // Attach before the first frame: the agent cctop was asked to launch is the
    // reason it is running, so it should be on screen and not behind a keypress.
    // Its own session row appears later, once it has written a transcript.
    let mut hosted = hosted;
    if let Some(hosted) = hosted.as_ref() {
        app.hosted = Some((hosted.pid, hosted.label.clone()));
        app.attach_hosted();
    }

    // `ratatui::init` installs a hook that leaves the alt screen and raw mode on
    // panic, but it knows nothing about the mouse capture enabled below, nor does
    // its restore path make the cursor visible again. Without this, a panic can
    // leave the user's shell receiving mouse escape sequences or with no cursor.
    // Installed after `init` so it runs before ratatui's restore hook.
    let mut terminal = ratatui::init();
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));
    let _ = execute!(std::io::stdout(), crossterm::style::Print(MOUSE_ON));
    // Bracketed paste, so a paste arrives as one `Event::Paste` instead of as
    // one `Event::Key` per character. Without it there is no way to tell a paste
    // from typing, and the newlines in a pasted message reach the agent as the
    // Enter that submits it — a five-line paste asking five questions.
    let _ = execute!(std::io::stdout(), EnableBracketedPaste);
    // Ask the terminal to tell Shift+Enter apart from Enter, which it otherwise
    // cannot: both are a carriage return, and a terminal has no other way to
    // send the difference. An agent in a pane wants that difference badly —
    // Shift+Enter is how you write a second line of a prompt without submitting
    // the first — and until cctop asks for it, the keypress reaches cctop as a
    // plain Enter and the distinction is lost before any pane could carry it.
    //
    // Only the disambiguating flag, and only where the terminal says it can:
    // the richer flags report key releases and repeats, which would double
    // every keystroke cctop already handles. Terminals without the protocol are
    // left alone, and there Shift+Enter stays what it has always been.
    if crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false) {
        let _ = execute!(
            std::io::stdout(),
            event::PushKeyboardEnhancementFlags(
                event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            )
        );
    }

    // Established before the loop so the first tick already has it; `None` just
    // means discovery falls back to the periodic walk.
    let watch = crate::watch::Watch::start();
    app.listener = crate::hook::Listener::start();
    // A hook naming a cctop that has since been moved or deleted fires nothing
    // at all, so it is repointed here rather than left to look installed while
    // reporting nothing. Anything narrower than that is left for the panel.
    for fixed in crate::hook::repair(app.hook_project().as_deref()) {
        app.set_status(&fixed);
    }
    // What repair deliberately would not touch: an install registering fewer
    // events than this cctop wants, or a settings file that will not parse.
    // Both look installed and quietly deliver less than they should, so they
    // are worth one line on the way in — an install that is simply absent is
    // not, since that is a choice and nagging about it is what makes people
    // stop reading the status line.
    if app
        .hook_status()
        .entries
        .iter()
        .any(|s| s.health.is_problem())
    {
        app.set_status("Agent hooks need attention — press h");
    }

    let result = event_loop(
        &mut app,
        &mut terminal,
        &res_rx,
        &req_tx,
        watch.as_ref(),
        hosted.as_mut(),
    );

    // Before the terminal is restored, so the agent's hangup does not race the
    // screen being handed back. Clearing the tabs only ends the panes; a
    // rmux-backed one is a client, and the agent behind it is left running —
    // which is the point, and so worth saying out loud on the way out.
    let had_tabs = !app.open_rmux().is_empty();
    app.tabs.clear();
    drop(hosted);

    // Asked after the clients are gone, and asked of rmux rather than of the
    // tabs: an agent left running by an earlier cctop is just as reachable as
    // one from this run, and the line below is the only thing that tells anyone
    // they are there at all.
    let left_running = match had_tabs {
        true => crate::rmux::sessions(),
        // Nothing here ever touched rmux, so nothing here is owed an account of
        // what is in it.
        false => Vec::new(),
    };

    restore_terminal();

    // After the restore, so it lands on the terminal the user is handed back
    // rather than inside the alternate screen that is about to be torn down.
    if !left_running.is_empty() {
        println!(
            "{} agent{} still running in rmux; `cctop` then `t` to get back to {}.",
            left_running.len(),
            if left_running.len() == 1 { "" } else { "s" },
            if left_running.len() == 1 {
                "it"
            } else {
                "them"
            },
        );
    }

    let _ = req_tx.send(Request::Shutdown);
    // The worker persists newly extracted transcript data while shutting down.
    // Joining it matters: returning from main immediately would otherwise kill
    // the detached thread mid-save, forcing every launch to parse all sessions
    // from scratch again.
    let _ = worker.join();
    app.save_prefs();
    result
}

/// Undo every terminal mode the TUI may have changed.
///
/// Mouse tracking, asked for by hand rather than through crossterm's
/// `EnableMouseCapture`.
///
/// The difference is `?1003h`, which crossterm turns on and this does not. That
/// is *any-motion* tracking: the terminal reports a bare hover, one sequence per
/// cell the pointer crosses, with no button held. cctop has never used one —
/// `on_mouse` drops movement, and switching tabs on a hover would drag you out
/// of the agent you are typing into — so the whole stream was noise.
///
/// Noise with a cost, though. A hover report is `\x1b[<35;79;14M`, and a reader
/// that takes it across two reads loses the `\x1b[` and keeps the rest, which
/// lands in whatever is being typed into as the literal text `<35;79;14M`. That
/// needs a machine lagging enough to split a read mid-sequence and a pointer
/// moving through it — which is exactly when it was reported. Presses, drags and
/// the wheel are all still asked for, so nothing cctop reads is lost; there is
/// simply no longer a report for every pixel of hover to be torn in half.
///
/// `?1000h` presses and releases, `?1002h` drags, `?1006h` the SGR encoding that
/// can name a column past 223.
const MOUSE_ON: &str = "\x1b[?1000h\x1b[?1002h\x1b[?1006h";

/// `ratatui::restore` intentionally only disables raw mode and leaves the
/// alternate screen; it does not restore cursor visibility. Keep this separate
/// so regular exits, input errors, and panics all use the same cleanup path.
fn restore_terminal() {
    // Popped unconditionally: a push that never happened pops nothing, and a
    // terminal left in the protocol would report keys to the user's shell in a
    // form it does not read.
    let _ = execute!(std::io::stdout(), event::PopKeyboardEnhancementFlags);
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    let _ = execute!(std::io::stdout(), Show);
    ratatui::restore();
    // Send Show once more after leaving the alternate screen. Some terminals
    // scope cursor state to the active screen buffer.
    let _ = execute!(std::io::stdout(), Show);
}

/// Read one machine on a timer until cctop exits.
///
/// A thread rather than a slot in the worker's queue: an ssh round trip can
/// take seconds or hang until its timeout, and the worker is what answers the
/// keyboard's refresh. One wedged host must cost only itself.
fn spawn_host_poller(host: crate::fleet::Host, tx: Sender<Response>) {
    std::thread::spawn(move || {
        loop {
            let snapshot = host.poll();
            if tx
                .send(Response::Remote {
                    host: host.target.clone(),
                    snapshot,
                })
                .is_err()
            {
                // The UI has gone; so should this.
                return;
            }
            std::thread::sleep(crate::fleet::POLL);
        }
    });
}

fn spawn_quota_poller(tx: Sender<Response>) {
    std::thread::spawn(move || {
        let mut quota = Quota::default();
        let (mut claude_due, mut codex_due) = (Instant::now(), Instant::now());

        loop {
            let now = Instant::now();
            let mut changed = false;

            // Each provider is paced by its own last outcome: a throttled one
            // backs off without stalling the other.
            if now >= claude_due {
                // Each profile is its own account with its own limits, so each
                // is asked separately. They share one due time: the interval
                // exists to be polite to the provider, and a machine with two
                // logins is not entitled to twice the requests.
                quota.claude = crate::config::profiles_for(Provider::Claude)
                    .iter()
                    .map(|profile| crate::quota::ProfileQuota {
                        profile: profile.name.clone(),
                        status: crate::quota::fetch_claude(profile),
                    })
                    .collect();
                // Paced by whichever account is most throttled, so backing off
                // for one does not keep asking on behalf of another.
                let delay = quota
                    .claude
                    .iter()
                    .map(|q| q.status.retry_delay_secs(QUOTA_INTERVAL_SECS))
                    .max()
                    .unwrap_or(QUOTA_INTERVAL_SECS);
                claude_due = now + Duration::from_secs(delay);
                changed = true;
            }
            if now >= codex_due {
                // Per account for the same reason as Claude's, and sharing one
                // due time for the same reason too.
                quota.codex = crate::config::profiles_for(Provider::Codex)
                    .iter()
                    .map(|profile| crate::quota::ProfileQuota {
                        profile: profile.name.clone(),
                        status: crate::quota::fetch_codex(profile),
                    })
                    .collect();
                let delay = quota
                    .codex
                    .iter()
                    .map(|q| q.status.retry_delay_secs(QUOTA_INTERVAL_SECS))
                    .max()
                    .unwrap_or(QUOTA_INTERVAL_SECS);
                codex_due = now + Duration::from_secs(delay);
                changed = true;
            }

            if changed {
                quota.fetched = true;
                if tx.send(Response::Quota(Box::new(quota.clone()))).is_err() {
                    break;
                }
            }
            std::thread::sleep(QUOTA_TICK);
        }
    });
}

fn event_loop(
    app: &mut App,
    terminal: &mut ratatui::DefaultTerminal,
    res_rx: &Receiver<Response>,
    req_tx: &Sender<Request>,
    watch: Option<&crate::watch::Watch>,
    mut hosted: Option<&mut crate::shim::Hosted>,
) -> anyhow::Result<i32> {
    let mut last_refresh = Instant::now();
    let mut last_full_walk = Instant::now();
    let mut layout = render::Layout::default();
    let mut refresh_in_flight = true;
    let mut last_blink = true;

    loop {
        // Drain everything the workers have produced.
        // Whether any of it changed the table, which is also the question
        // "does the page need telling" — see the feed below the loop.
        let mut annotated_rows_changed = false;
        // Only a refresh can move a session between busy and waiting, so the
        // notifier is fed here rather than once per loop iteration — that would
        // rebuild its map five times a second over rows that hadn't moved.
        let mut rows_changed = false;
        loop {
            match res_rx.try_recv() {
                Ok(Response::Discovered(sessions)) => {
                    app.sessions = sessions;
                    app.loaded = true;
                    app.stats = crate::loader::compute_stats(&app.sessions);
                    app.refilter();
                    app.merge_remotes();
                    rows_changed = true;
                }
                Ok(Response::Annotated(session)) => {
                    // Match on the key's two fields rather than on `key()`: that
                    // formats a String per candidate, so a scan over thousands of
                    // rows allocated thousands of times — per arriving row, on the
                    // thread that also has to answer the keyboard.
                    // `remote.is_none()` is part of the identity, not a
                    // nicety: the worker only ever reports local rows, and a
                    // remote session that happened to share an id would be
                    // overwritten by one from this machine.
                    let found = app.sessions.iter_mut().find(|s| {
                        s.remote.is_none()
                            && s.provider == session.provider
                            && s.session_id == session.session_id
                    });
                    if let Some(existing) = found {
                        *existing = *session;
                    } else {
                        app.sessions.push(*session);
                    }
                    annotated_rows_changed = true;
                }
                Ok(Response::Sessions(payload)) => {
                    let (sessions, stats) = *payload;
                    app.sessions = sessions;
                    app.loaded = true;
                    app.stats = stats;
                    app.merge_remotes();
                    app.push_history();
                    app.refilter();
                    refresh_in_flight = false;
                    annotated_rows_changed = false;
                    rows_changed = true;
                }
                Ok(Response::LiveRows(payload)) => {
                    let (rows, stats) = *payload;
                    for row in rows {
                        let found = app.sessions.iter_mut().find(|s| {
                            s.remote.is_none()
                                && s.provider == row.provider
                                && s.session_id == row.session_id
                        });
                        match found {
                            Some(existing) => *existing = row,
                            // A session that started since the last full walk.
                            None => app.sessions.push(row),
                        }
                    }
                    app.adopt_stats(stats);
                    app.loaded = true;
                    app.push_history();
                    app.refilter();
                    refresh_in_flight = false;
                    rows_changed = true;
                }
                Ok(Response::Data(key, data)) => {
                    // Discard results for a session the user has already left.
                    if key == app.panel_key {
                        app.panel_data = Some(*data);
                        app.needs_redraw = true;
                    }
                }
                Ok(Response::Quota(q)) => {
                    app.quota = *q;
                    app.needs_redraw = true;
                }
                Ok(Response::UpdateAvailable(version)) => {
                    app.update_available = Some(version);
                    app.needs_redraw = true;
                }
                Ok(Response::PricingReady) => {
                    // Cached costs were computed without rates; recompute them.
                    let _ = req_tx.send(Request::Refresh);
                    refresh_in_flight = true;
                }
                Ok(Response::Terminated {
                    session_key,
                    result,
                }) => match result {
                    Ok(()) => {
                        app.set_status("Termination signal sent");
                        let _ = req_tx.send(Request::Refresh);
                        refresh_in_flight = true;
                    }
                    Err(error) => {
                        app.set_status(format!("Could not stop {session_key}: {error}"));
                    }
                },
                Ok(Response::Deleted {
                    session_key,
                    result,
                }) => {
                    app.deleting.remove(&session_key);
                    match result {
                        Ok(()) => {
                            app.sessions.retain(|session| session.key() != session_key);
                            app.marked.remove(&session_key);
                            app.stats = crate::loader::compute_stats(&app.sessions);
                            app.refilter();
                            app.set_status("Deleted session");
                        }
                        Err(error) => {
                            app.set_status(format!("Could not delete {session_key}: {error}"))
                        }
                    }
                }
                Ok(Response::KeysSent { result }) => match result {
                    Ok(()) => app.set_status("Sent to the session's terminal"),
                    Err(error) => app.set_status(error),
                },
                Ok(Response::Remote { host, snapshot }) => {
                    match snapshot {
                        crate::fleet::Snapshot::Rows(rows) => {
                            app.remote_errors.remove(&host);
                            app.remotes.insert(host, rows);
                        }
                        // The last good snapshot is kept rather than blanked: a
                        // dropped ssh connection has not stopped those agents,
                        // and an empty machine is a stronger claim than a stale
                        // one. The footer says the reading is old.
                        crate::fleet::Snapshot::Failed(why) => {
                            app.remote_errors.insert(host, why);
                        }
                    }
                    app.merge_remotes();
                    rows_changed = true;
                }
                Ok(Response::Scanned { query, hits }) => app.scanned(query, hits),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        if annotated_rows_changed || rows_changed {
            // Extraction rebuilds each subagent from its transcript, which
            // cannot know what a hook already reported, so the hook's answer is
            // reapplied to every batch of rows that replaces them. The same
            // goes for the permission mode, which no transcript records at all.
            app.apply_finished_agents();
            app.apply_permissions();
            // After the hooks, because a row's liveness is what decides whether
            // it can still race anyone, and cheap enough to redo wholesale:
            // it compares paths already in memory and reads no transcript.
            app.collisions = crate::collide::apply(&mut app.sessions);
        }
        if annotated_rows_changed {
            // A burst can contain hundreds of rows. Recompute and sort once
            // after draining it rather than once per transcript.
            app.stats = crate::loader::compute_stats(&app.sessions);
            app.refilter();
        }
        if rows_changed {
            app.check_bells();
        }
        // The page gets what the table has, and only when it changed. This is
        // also what wakes a browser: its event stream is parked on the version
        // this bumps, so a page updates when the table does rather than on a
        // clock of its own.
        if rows_changed || annotated_rows_changed {
            app.feed_serving();
        }

        app.sync_panel_data();
        app.tick_scan();

        // Every tab, not just the visible one: an agent whose output nobody
        // reads eventually blocks on writing it.
        let mut drawn = false;
        for tab in &mut app.tabs {
            drawn |= tab.pump();
        }
        // An agent that said what it wanted says it once, to the person who
        // just looked: the tab colour is the alarm, this is the message. Here
        // rather than on the keypress that focused the pane — a bell arriving
        // while you are already on it has also been seen, and a tab you switch
        // away from must keep its bell rather than have it cleared by the tick.
        if let Some(note) = app.focused_pane().and_then(tabs::Pane::answer_bell) {
            app.set_status(note);
        }
        let closed = app.tabs.iter_mut().fold(false, |any, tab| tab.reap() | any);
        if closed {
            app.drop_empty_tabs();
        }
        // After the reap, so a finished install is seen as finished on the same
        // tick its pane goes away.
        app.poll_rmux_install();
        // And after both, so a tab this cctop has just lost is not immediately
        // re-added by a listing taken before its session went.
        app.sync_shared_tabs();
        if drawn || closed {
            app.needs_redraw = true;
        }

        // Hook events arrive whenever an agent hits one, which is not on any
        // tick of ours, so they are drained here alongside everything else.
        if let Some(events) = app.listener.as_ref().map(crate::hook::Listener::drain) {
            let (changed, lifecycle) = app.apply_hooks(events);
            app.needs_redraw |= changed;
            // A session that has just begun or ended is a row to find or forget
            // now. Waiting for the next poll would leave an agent the user just
            // started missing from the table for as long as the interval, which
            // is precisely the moment they are looking for it.
            if lifecycle && !refresh_in_flight {
                let _ = req_tx.send(Request::Refresh);
                refresh_in_flight = true;
                last_refresh = Instant::now();
            }
        }

        // A brief for a just-launched agent comes due on a timer rather than an
        // event, so the loop is the only thing that can notice.
        app.tick_handoff();

        // A blinking tab is the one thing on screen that changes with no event
        // behind it, so the loop has to ask for the frame itself — but only on
        // the half-cycle it actually flips, not on every poll.
        let phase = app.blink_on();
        if phase != last_blink && app.any_attention() {
            app.needs_redraw = true;
        }
        last_blink = phase;

        // Expire the transient status line.
        if let Some((_, at)) = &app.status
            && at.elapsed() > Duration::from_secs(3)
        {
            app.status = None;
            app.needs_redraw = true;
        }

        if app.needs_redraw {
            terminal.draw(|frame| layout = render::draw(frame, app))?;
            app.needs_redraw = false;
        }

        // Wait for input, but never past the next scheduled refresh. The
        // interval is read live so +/- changes apply on the very next poll.
        let refresh_every = Duration::from_secs_f64(app.refresh_secs);
        // Attached, the same wait is what stands between a keystroke and seeing
        // it echoed, so it drops to a frame's worth.
        let idle_wait = match app.tab {
            0 => Duration::from_millis(200),
            _ => Duration::from_millis(16),
        };
        let wait = refresh_every
            .checked_sub(last_refresh.elapsed())
            .unwrap_or(Duration::ZERO)
            .min(idle_wait);
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(key) => app.on_key(key),
                Event::Paste(text) => app.on_paste(&text),
                Event::Mouse(m) => app.on_mouse(m, &layout),
                Event::Resize(_, _) => app.needs_redraw = true,
                _ => {}
            }
        }

        // The agent cctop was launched to run has finished, so cctop has nothing
        // left to do either: hand its exit code back and get out of the way.
        if let Some(hosted) = hosted.as_mut()
            && let Some(code) = hosted.finished()
        {
            return Ok(code);
        }

        if app.should_quit {
            break;
        }

        // Only one refresh in flight: a scan slower than the interval must not
        // queue up behind itself.
        let refresh_every = Duration::from_secs_f64(app.refresh_secs);
        if last_refresh.elapsed() >= refresh_every && !refresh_in_flight {
            last_refresh = Instant::now();
            refresh_in_flight = true;
            // Walking every provider directory is what scales with the number of
            // sessions ever created, while what the user watches scales with the
            // number running now. So the fast tick updates the running rows and
            // the walk — the only thing that can notice a *new* session — runs on
            // its own slower cadence.
            let watched_change = watch.is_some_and(crate::watch::Watch::took_structural_change);
            // A transcript is created before it is summarizable — the model name
            // only arrives with the first assistant message — so the walk the
            // create earned can find nothing. Keep walking, at a cadence between
            // the fast tick and the safety net, until the file becomes a session.
            let awaiting = !watched_change
                && last_full_walk.elapsed() >= PENDING_WALK_INTERVAL
                && watch.is_some_and(|w| {
                    w.awaiting_discovery(|path| {
                        app.sessions
                            .iter()
                            .any(|s| s.data_file.as_deref() == Some(path))
                    })
                });
            let full_due =
                watched_change || awaiting || last_full_walk.elapsed() >= FULL_WALK_INTERVAL;
            if full_due {
                last_full_walk = Instant::now();
            }
            let _ = req_tx.send(if full_due {
                Request::Refresh
            } else {
                Request::RefreshLive
            });
        }
    }
    // Quit was pressed rather than the agent exiting, so there is no exit code
    // to inherit.
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn test_app() -> App {
        let (tx, rx) = channel();
        // Keep the receiver alive so sends in tests don't fail.
        std::mem::forget(rx);
        App::with_prefs(Plan::Retail, tx, UiPrefs::default())
    }

    /// Closing a tab leaves you on the tab beside it, not on the dashboard.
    ///
    /// The bug this closes: the view was corrected by decrementing, which is
    /// right for a tab that merely shifted down and wrong for the one that
    /// closed — so closing tab 2 of three landed on tab 1, and closing the only
    /// tab or the first one dropped you onto the dashboard with the agents you
    /// were watching still there in the bar.
    #[test]
    fn closing_a_tab_lands_on_its_neighbour() {
        // Three tabs, the middle one closed: its number now belongs to what was
        // the third, which is the tab next to the one that went.
        assert_eq!(App::landing(2, &[2], 2), 2);
        // The last one closed: there is no tab to the right, so the neighbour
        // is the one before it.
        assert_eq!(App::landing(3, &[3], 2), 2);
        // The first of several: still a neighbour, still not the dashboard.
        assert_eq!(App::landing(1, &[1], 2), 1);
        // The only tab: the dashboard is all that is left.
        assert_eq!(App::landing(1, &[1], 0), 0);
        // A tab closing before the one being watched slides the view down, so
        // it stays on the same agent rather than the same number.
        assert_eq!(App::landing(3, &[1], 2), 2);
        // Several at once, from either side of the view.
        assert_eq!(App::landing(4, &[1, 2], 3), 2);
        assert_eq!(App::landing(2, &[2, 3], 1), 1);
        // The dashboard is not a tab and never moves.
        assert_eq!(App::landing(0, &[1], 1), 0);
    }

    /// And the whole way through, on real panes: the view ends up on the
    /// neighbour's agent rather than on the dashboard.
    #[cfg(target_os = "linux")]
    #[test]
    fn closing_a_pane_leaves_the_next_agent_on_screen() {
        let mut app = test_app();
        let mut children = Vec::new();
        for name in ["first", "second"] {
            let (child, pid) = crate::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
            children.push(child);
            let pane = tabs::Pane::view_of(pid, name.to_string()).expect("attach");
            app.tabs.push(tabs::Tab::new(pane));
        }
        // Watching the first of the two.
        app.tab = 1;
        app.close_pane();

        assert_eq!(app.tabs.len(), 1, "the closed tab stayed in the bar");
        assert_eq!(app.tab, 1, "closing the first tab left the view nowhere");
        assert_eq!(
            app.tabs[0].title(),
            "second",
            "the view landed on the wrong agent"
        );
        for child in &mut children {
            let _ = child.kill();
        }
    }

    /// A tab dragged along the bar takes its place there, and the view stays on
    /// the agent it was watching — whether that is the tab that moved or one the
    /// move shifted past. Getting the second wrong drops you into somebody
    /// else's terminal for rearranging the bar around it.
    #[test]
    fn a_dragged_tab_moves_without_moving_the_view() {
        let named = |name: &str| {
            tabs::Tab::shared(&crate::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
            })
        };
        let titles = |app: &App| -> Vec<String> { app.tabs.iter().map(tabs::Tab::title).collect() };

        let mut app = test_app();
        app.tabs = vec![named("a"), named("b"), named("c")];

        // The tab you are on, dragged to the end: it goes there and you go with
        // it.
        app.tab = 1;
        app.move_tab(1, 3);
        assert_eq!(titles(&app), ["b", "c", "a"]);
        assert_eq!(app.tab, 3);

        // A tab dragged past the one you are watching: the bar changes, the
        // agent in front of you does not.
        app.tab = 1; // "b"
        app.move_tab(3, 1); // "a" back to the front
        assert_eq!(titles(&app), ["a", "b", "c"]);
        assert_eq!(app.tab, 2, "the view followed the position, not the tab");

        // The dashboard is not a tab in the list, and neither end of a move can
        // be it.
        app.move_tab(0, 2);
        app.move_tab(2, 0);
        assert_eq!(titles(&app), ["a", "b", "c"]);

        // The keyboard's half, clamped at both ends rather than wrapping.
        app.tab = 1;
        app.move_workspace(-1);
        assert_eq!(
            titles(&app),
            ["a", "b", "c"],
            "the first tab has nowhere to go"
        );
        app.move_workspace(1);
        assert_eq!(titles(&app), ["b", "a", "c"]);
        assert_eq!(app.tab, 2);
    }

    /// The drag itself: pressing a tab picks it up, moving over another one
    /// carries it there, and the release ends it. The whole gesture is the
    /// bar's, so none of it reaches the agents underneath.
    #[test]
    fn dragging_a_tab_along_the_bar_reorders_it() {
        let named = |name: &str| {
            tabs::Tab::shared(&crate::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
            })
        };
        let layout = render::Layout {
            // The dashboard, then a tab per name, as the bar draws them.
            workspace_spans: vec![(0, 11, 0), (11, 15, 1), (15, 19, 2), (19, 23, 3)],
            ..Default::default()
        };
        let at = |kind, column| event::MouseEvent {
            kind,
            column,
            row: 0,
            modifiers: event::KeyModifiers::NONE,
        };
        let press = event::MouseEventKind::Down(event::MouseButton::Left);
        let drag = event::MouseEventKind::Drag(event::MouseButton::Left);
        let release = event::MouseEventKind::Up(event::MouseButton::Left);

        let mut app = test_app();
        app.tabs = vec![named("a"), named("b"), named("c")];
        let titles = |app: &App| -> Vec<String> { app.tabs.iter().map(tabs::Tab::title).collect() };

        // Press on the first tab: it is picked up, and shown — where there is
        // anything to show. These tabs are shared ones, which is to say rmux
        // sessions, and `go_to_tab` attaches before it switches: on a machine
        // with no rmux the attach cannot succeed, and the documented outcome is
        // to stay put and say why rather than to open a blank tab. Both are the
        // gesture working; only one of them is reachable on a given runner.
        app.on_mouse(at(press, 12), &layout);
        assert_eq!(app.drag_tab, Some(1), "the press did not pick the tab up");
        match crate::rmux::available() {
            true => assert_eq!(app.tab, 1, "the press did not show the tab"),
            false => {
                assert_eq!(app.tab, 0, "a tab that cannot be attached moved the view");
                let status = app
                    .status
                    .as_ref()
                    .map(|(s, _)| s.clone())
                    .unwrap_or_default();
                assert!(status.contains("Could not open"), "silently: {status:?}");
            }
        }

        // Carried to the third slot, one tab at a time as the pointer crosses
        // them, with the view following it. The view is what is under test from
        // here, so it starts where the press would have put it — which on a
        // machine without rmux is somewhere the press could not reach.
        app.tab = 1;
        app.on_mouse(at(drag, 16), &layout);
        app.on_mouse(at(drag, 20), &layout);
        assert_eq!(titles(&app), ["b", "c", "a"]);
        assert_eq!(app.tab, 3);

        app.on_mouse(at(release, 20), &layout);
        assert_eq!(app.drag_tab, None);
        // A later drag with nothing picked up moves nothing.
        app.on_mouse(at(drag, 12), &layout);
        assert_eq!(titles(&app), ["b", "c", "a"]);

        // The dashboard is not draggable, and nothing can be dropped onto it.
        app.on_mouse(at(press, 4), &layout);
        assert_eq!(app.tab, 0);
        assert_eq!(app.drag_tab, None);
        app.on_mouse(at(press, 12), &layout);
        app.on_mouse(at(drag, 4), &layout);
        assert_eq!(titles(&app), ["b", "c", "a"]);
        assert_eq!(app.drag_tab, Some(1), "the tab is still in hand");
    }

    /// Right-clicking a tab asks for a new name, and Enter puts it in the bar.
    ///
    /// The dashboard is not renamable and a right-click on it must fall through
    /// untouched — it is the one entry in the bar that is not a tab.
    #[test]
    fn right_clicking_a_tab_renames_it() {
        let named = |name: &str| {
            tabs::Tab::shared(&crate::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
            })
        };
        let layout = render::Layout {
            workspace_spans: vec![(0, 11, 0), (11, 15, 1), (15, 19, 2)],
            ..Default::default()
        };
        let click = |column| event::MouseEvent {
            kind: event::MouseEventKind::Down(event::MouseButton::Right),
            column,
            row: 0,
            modifiers: event::KeyModifiers::NONE,
        };
        let typed = |app: &mut App, text: &str| {
            for c in text.chars() {
                app.on_key(key(KeyCode::Char(c)));
            }
        };

        let mut app = test_app();
        app.tabs = vec![named("a"), named("b")];

        app.on_mouse(click(4), &layout);
        assert_eq!(app.mode, Mode::List, "the dashboard offered to be renamed");

        app.on_mouse(click(16), &layout);
        assert_eq!(app.mode, Mode::RenameTab);
        assert_eq!(app.rename_tab, 2);
        assert_eq!(app.rename_was, "b");
        typed(&mut app, "review");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::List);
        assert_eq!(
            app.tabs.iter().map(tabs::Tab::title).collect::<Vec<_>>(),
            ["a", "review"]
        );

        // Esc leaves the name alone, and so does an empty field: blanking a tab
        // would leave a numbered gap in the bar and no way back to it.
        app.on_mouse(click(16), &layout);
        typed(&mut app, "x");
        app.on_key(key(KeyCode::Esc));
        app.on_mouse(click(16), &layout);
        app.on_key(key(KeyCode::Enter));
        assert_eq!(
            app.tabs.iter().map(tabs::Tab::title).collect::<Vec<_>>(),
            ["a", "review"]
        );

        // The tab the right-click landed on has gone; the name goes nowhere
        // rather than onto whichever tab took its place.
        app.on_mouse(click(16), &layout);
        app.tabs.remove(1);
        typed(&mut app, "late");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(
            app.tabs.iter().map(tabs::Tab::title).collect::<Vec<_>>(),
            ["a"]
        );
    }

    /// A paste on the dashboard is typing into whichever one-line box is open,
    /// and the line breaks in it must not go in: none of these inputs can show a
    /// second row or let you delete back onto one.
    #[test]
    fn a_paste_types_into_the_open_input_as_one_line() {
        let mut app = test_app();

        app.mode = Mode::Search;
        app.on_paste("fix the\nlogin bug\r\n");
        assert_eq!(app.search, "fix the login bug ");

        app.mode = Mode::SendKeys;
        app.send_input = "continue".into();
        app.on_paste(" and\ttidy\x07 up");
        assert_eq!(app.send_input, "continue and tidy up");

        // The cost floor is a number, so a paste is filtered the way typing one
        // is rather than flattened.
        app.mode = Mode::CostFilter;
        app.on_paste("$12.50 or so");
        assert_eq!(app.cost_input, "12.50");
    }

    /// The caps the typed path enforces are the paste's too, and a paste with
    /// nowhere to land does nothing rather than something surprising.
    #[test]
    fn a_paste_respects_the_caps_and_does_nothing_with_no_input_open() {
        let mut app = test_app();

        app.mode = Mode::SendKeys;
        app.send_input = "x".repeat(495);
        app.on_paste(&"y".repeat(50));
        assert_eq!(app.send_input.len(), 500);

        app.mode = Mode::CostFilter;
        app.on_paste("123456789012345");
        assert_eq!(app.cost_input, "123456789012");

        // A modal is on screen, so there is no box to type into — and a paste
        // must never stand in for the key one of these is waiting for.
        app.mode = Mode::DeleteConfirm;
        app.on_paste("y");
        assert_eq!(app.mode, Mode::DeleteConfirm);
        app.mode = Mode::List;
        app.on_paste("q");
        assert!(!app.should_quit);
    }

    /// Which harness an argv names, and so which variable may be put in front
    /// of it. Getting this wrong is not a cosmetic error: `CODEX_HOME` in front
    /// of `claude` is ignored, and the agent then runs as an account the pane
    /// says it is not.
    #[test]
    fn only_a_harness_with_a_config_variable_takes_a_profile() {
        let of = |c: &str| App::profile_provider(&[c.to_string()]);
        assert_eq!(of("claude"), Some(Provider::Claude));
        assert_eq!(of("codex"), Some(Provider::Codex));
        // Found by basename, however it was spelled on the way in.
        assert_eq!(of("/usr/local/bin/codex"), Some(Provider::Codex));
        assert_eq!(of("codex.exe"), Some(Provider::Codex));
        assert_eq!(of(r"C:\tools\claude.exe"), Some(Provider::Claude));
        // No such variable: a profile would be a setting that did nothing.
        assert_eq!(of("cursor-agent"), None);
        assert_eq!(of("gemini"), None);
        // Not a prefix match — `codex-something` is not codex.
        assert_eq!(of("codexa"), None);
        assert_eq!(App::profile_provider(&[]), None);
    }

    /// The prefix a launch actually carries, per harness.
    #[test]
    fn a_profile_reaches_the_agent_as_its_own_variable() {
        let under = |dir: &str, provider, command: &str| {
            let profile = crate::config::Profile {
                provider,
                name: "work".to_string(),
                dir: std::path::PathBuf::from(dir),
            };
            crate::config::argv_under_profile(vec![command.to_string()], &profile)
        };
        assert_eq!(
            under("/home/x/.codex-work", Provider::Codex, "codex"),
            ["env", "CODEX_HOME=/home/x/.codex-work", "codex"]
        );
        assert_eq!(
            under("/home/x/.claude-work", Provider::Claude, "claude"),
            ["env", "CLAUDE_CONFIG_DIR=/home/x/.claude-work", "claude"]
        );
        // A harness with no variable is handed its command untouched rather
        // than an `env` prefix that promises something.
        assert_eq!(
            under("/home/x/.cursor", Provider::Cursor, "cursor-agent"),
            ["cursor-agent"]
        );
    }

    /// Regression: a click on the launcher used to be answered twice — once by
    /// the modal and once by the dashboard drawn under it — so picking an agent
    /// also switched the bottom panel the modal happened to cover.
    #[test]
    fn a_click_on_a_modal_does_not_reach_what_it_covers() {
        use ratatui::layout::Rect;

        let mut app = test_app();
        app.mode = Mode::Launch;
        let layout = render::Layout {
            modal_rect: Some(Rect::new(10, 8, 20, 6)),
            launch_rows: vec![(9, 0), (10, 1)],
            // The panel tabs sit on a row the modal is covering.
            tab_row: 10,
            tab_spans: vec![(10, 20, 3)],
            ..Default::default()
        };
        let click = |col, row| crossterm::event::MouseEvent {
            kind: event::MouseEventKind::Down(event::MouseButton::Left),
            column: col,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        app.on_mouse(click(15, 10), &layout);
        assert_eq!(app.bottom_tab, 0, "the click went through to the panels");
        assert_eq!(app.launch_cursor, 1, "the click did not pick a choice");
        assert_eq!(app.mode, Mode::Launch, "the launcher closed on a pick");

        // Off the modal dismisses it, and still does not reach the panels.
        app.needs_redraw = false;
        app.on_mouse(click(40, 10), &layout);
        assert_eq!(app.mode, Mode::List);
        assert_eq!(app.bottom_tab, 0);
        // Regression: dismissing it asked for no frame, so the modal stayed
        // drawn over a dashboard that was already taking the clicks again.
        assert!(app.needs_redraw, "the dismissal never repainted");
    }

    /// Regression: the guard above was keyed on "any mode but List", which took
    /// the mouse away from the search box too — an overlay a few lines tall over
    /// a table still being scrolled and clicked while the query is typed. Only a
    /// modal that recorded its rectangle can claim the mouse, because that
    /// rectangle is the only way to tell its clicks from the ones underneath.
    #[test]
    fn the_search_box_leaves_the_table_its_mouse() {
        let mut app = test_app();
        for id in ["a", "b", "c"] {
            app.sessions
                .push(crate::session::Session::new(Provider::Claude, id.into()));
        }
        app.visible = vec![Row::Session(0), Row::Session(1), Row::Session(2)];
        // What `draw_search` leaves behind: no rectangle, so no claim.
        let layout = render::Layout {
            rows_start: 7,
            rows_end: 12,
            // Below the table, or the wheel scrolls a panel instead of the list.
            bottom_start: 14,
            modal_rect: None,
            ..Default::default()
        };
        let at = |kind, row| crossterm::event::MouseEvent {
            kind,
            column: 5,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        app.mode = Mode::Search;
        app.on_mouse(at(event::MouseEventKind::ScrollDown, 9), &layout);
        assert_eq!(app.selected, 1, "the wheel is dead while searching");
        app.on_mouse(
            at(event::MouseEventKind::Down(event::MouseButton::Left), 9),
            &layout,
        );
        assert_eq!(app.selected, 2, "a click cannot reach the row it landed on");
        assert_eq!(app.mode, Mode::Search, "the click closed the search box");
    }

    /// Regression: `launch_prompt` set the mode and nothing asked for a frame, so
    /// clicking the bar's new-tab button — the one advertisement the feature has
    /// — looked like a dead button until an unrelated event repainted.
    #[test]
    fn clicking_the_new_tab_button_paints_the_launcher() {
        let mut app = test_app();
        let layout = render::Layout {
            workspace_new: Some((12, 23)),
            ..Default::default()
        };
        app.needs_redraw = false;
        app.on_mouse(
            crossterm::event::MouseEvent {
                kind: event::MouseEventKind::Down(event::MouseButton::Left),
                column: 15,
                row: 0,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            &layout,
        );
        // The launcher opens only where there is something to launch, which on a
        // machine with no agent and no $SHELL there is not — but either way the
        // click has to have asked for the frame that says so.
        assert!(app.needs_redraw, "the click asked for no frame");
        assert!(matches!(app.mode, Mode::Launch | Mode::List));
    }

    /// Closing is a kill now, but only of an agent that is cctop's to kill. On a
    /// pane opened with `a` — a window onto an agent cctop never started — there
    /// is nothing to stop, so the window closes and the status says the agent
    /// was left alone rather than claiming a kill that never happened.
    #[cfg(target_os = "linux")]
    #[test]
    fn closing_a_borrowed_pane_says_the_agent_was_left_running() {
        let (mut child, pid) = crate::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let pane = tabs::Pane::view_of(pid, "agent".into()).expect("attach");
        assert!(
            !pane.owns_agent(),
            "a borrowed pane claimed the agent as cctop's"
        );

        let mut app = test_app();
        app.tabs.push(tabs::Tab::new(pane));
        app.tab = 1;
        app.kill_pane();

        // The window is gone, the agent is not, and the status says which.
        assert!(app.tabs.is_empty(), "the view outlived its close");
        let (status, _) = app.status.clone().expect("nothing was said");
        assert!(status.contains("not cctop's to stop"), "{status}");
        assert!(
            child.try_wait().ok().flatten().is_none(),
            "closing a borrowed view stopped somebody else's agent"
        );

        let _ = child.kill();
        let _ = child.wait();
        let _ = crate::shim::socket_path(pid).map(std::fs::remove_file);
    }

    /// A tab standing for another cctop's agent has no pane here, so every key
    /// that works through the focused pane does nothing on it. Closing has to
    /// keep working anyway: the tab is the only handle on screen for that agent,
    /// and a `w` that silently did nothing would read as a stuck tab.
    #[test]
    fn a_shared_tab_can_be_closed_without_a_pane_to_close() {
        let mut app = test_app();
        app.tabs.push(tabs::Tab::shared(&crate::rmux::Running {
            name: "cctop-claude-nosuchsession".into(),
            pid: Some(4321),
            cwd: None,
            attached: false,
            activity: None,
            label: Some("claude · Improve super cctop".into()),
            profile: None,
        }));
        app.tab = 1;
        // Nothing has emptied it: a tab with no pane is still a tab, or every
        // cctop would drop the ones it is not looking at.
        app.drop_empty_tabs();
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.tabs[0].title(), "claude · Improve super cctop");

        app.close_pane();
        assert!(
            app.tabs.is_empty(),
            "closing a shared tab left it in the bar"
        );
        // And the view followed it back rather than pointing past the end.
        assert_eq!(app.tab, 0);
        let (status, _) = app.status.clone().expect("nothing was said");
        assert!(status.contains("Improve super cctop"), "{status}");
    }

    /// Regression: the "already open" guard asked only about `rmux`, which is
    /// `None` on every pane when rmux is not installed — so `R` on a session
    /// already resumed in a tab started a second agent on the one transcript,
    /// and being stopped, it did so without even the confirmation.
    #[cfg(target_os = "linux")]
    #[test]
    fn resuming_a_session_already_in_a_tab_goes_to_that_tab() {
        let (mut child, pid) = crate::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let mut pane = tabs::Pane::view_of(pid, "claude".into()).expect("attach");
        // What a resumed tab records regardless of who carries the agent. The
        // pane has no rmux, standing in for a machine without it.
        pane.resumed = Some(crate::rmux::name_for_session("claude", "abc"));
        assert!(pane.rmux.is_none());

        let mut app = test_app();
        app.sessions
            .push(crate::session::Session::new(Provider::Claude, "abc".into()));
        app.visible = vec![Row::Session(0)];
        app.selected = 0;
        app.tabs.push(tabs::Tab::new(pane));
        app.tab = 0;

        app.resume_now();

        assert_eq!(
            app.tabs.len(),
            1,
            "a second agent was put on one transcript"
        );
        assert_eq!(app.tab, 1, "the tab already holding it was not shown");
        let (status, _) = app.status.clone().expect("nothing was said");
        assert!(status.contains("Already open"), "{status}");

        let _ = child.kill();
        let _ = child.wait();
        let _ = crate::shim::socket_path(pid).map(std::fs::remove_file);
    }

    /// Regression: the launcher sized itself to its list and let `centered` clamp
    /// the result, so on a short terminal the rows past the bottom were dropped —
    /// and once the cursor walked into them, nothing on screen said what Enter
    /// The browser panel shows where the page is, and does not show the token
    /// that opens it.
    ///
    /// Both halves matter. A panel that cannot say where the page is has not
    /// answered the question it was opened to answer; a panel that prints the
    /// token puts a credential into every screenshot of it. The link is drawn
    /// as its origin and the token lives in the clipboard and the escape.
    #[test]
    fn the_serve_panel_names_the_page_without_naming_its_token() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = test_app();
        let serving = crate::serve::start(crate::serve::Options {
            // A port nobody asked for, so a busy one is stepped past rather
            // than failing a test on whatever else is running here.
            port_given: false,
            ..Default::default()
        })
        .expect("a loopback server");
        let token = serving
            .local
            .split_once("?t=")
            .map(|(_, token)| token.to_string())
            .expect("a tokenised link");
        app.serving = Some(serving);
        app.mode = Mode::Serve;

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("backend");
        let mut layout = render::Layout::default();
        terminal
            .draw(|frame| layout = render::draw(frame, &mut app))
            .expect("draw");
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();

        assert!(
            screen.contains("http://127.0.0.1:"),
            "the panel never said where the page is:\n{screen}"
        );
        assert!(
            !screen.contains(&token),
            "the token was drawn on screen:\n{screen}"
        );
    }

    /// was about to start.
    #[test]
    fn the_launcher_keeps_its_cursor_on_screen() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = test_app();
        // More choices than a short terminal can hold at once.
        app.launch_offer = (0..20)
            .map(|i| tabs::Choice::Start(vec![format!("agent-{i}")]))
            .collect();
        app.mode = Mode::Launch;
        let (cols, rows) = (80u16, 14u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");

        // Every choice, including the ones far past the bottom of the window.
        for cursor in 0..app.launch_offer.len() {
            app.launch_cursor = cursor;
            let mut layout = render::Layout::default();
            terminal
                .draw(|frame| layout = render::draw(frame, &mut app))
                .expect("draw");

            let row = layout
                .launch_rows
                .iter()
                .find(|(_, i)| *i == cursor)
                .map(|(row, _)| *row);
            let row = row.unwrap_or_else(|| panic!("choice {cursor} was not drawn"));
            assert!(row < rows, "choice {cursor} drawn off screen at row {row}");

            // Drawn, and drawn as the selection: the highlight is the only thing
            // saying which of twenty agents Enter starts.
            let buffer = terminal.backend().buffer().clone();
            let label = format!("agent-{cursor}");
            let line: String = (0..cols).map(|x| buffer[(x, row)].symbol()).collect();
            assert!(
                line.contains(&label),
                "row {row} is not choice {cursor}: {line:?}"
            );

            // And what is under the list stays under it, never scrolled away.
            let text: String = (0..rows)
                .map(|y| {
                    (0..cols)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect();
            assert!(text.contains("Enter start"), "the keys scrolled off");
            assert!(
                text.contains("this directory") || text.contains(" in "),
                "where it would run scrolled off"
            );
        }
    }

    /// The suggestions are part of the footer, not part of the list: a short
    /// terminal has to lose choices before it loses the paths being offered,
    /// because they are what the field is being typed against.
    #[test]
    fn the_directory_suggestions_stay_on_screen_under_a_long_list() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let root = tempfile::tempdir().expect("tempdir");
        let project = root.path().join("chosen-project");
        std::fs::create_dir(&project).expect("mkdir");

        let mut app = test_app();
        app.launch_offer = (0..20)
            .map(|i| tabs::Choice::Start(vec![format!("agent-{i}")]))
            .collect();
        app.launch_cwd_known = vec![project.clone()];
        app.launch_cwd_input = String::new();
        app.launch_cwd_suggest();
        app.launch_cwd_pick = Some(0);
        app.mode = Mode::LaunchCwd;

        let (cols, rows) = (80u16, 14u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let mut layout = render::Layout::default();
        terminal
            .draw(|frame| layout = render::draw(frame, &mut app))
            .expect("draw");

        let buffer = terminal.backend().buffer().clone();
        let text: String = (0..rows)
            .map(|y| {
                (0..cols)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(
            text.contains("chosen-project"),
            "the suggestion is not drawn"
        );
        assert!(text.contains("Tab fill in"), "the key that fills it in");

        // And it is clickable where it was drawn, rather than on a row the
        // choices above it also claim.
        let (row, i) = *layout
            .launch_cwd_rows
            .first()
            .expect("no clickable suggestion");
        assert_eq!(i, 0);
        assert!(row < rows, "the suggestion is drawn off screen");
        assert!(
            !layout.launch_rows.iter().any(|(y, _)| *y == row),
            "a choice and a suggestion share row {row}"
        );
    }

    /// The three ways the ownership decision can go, since only one of them is
    /// new: rmux present is unchanged, rmux absent and uninstallable is the old
    /// silent fallback, and only rmux absent but installable stops to ask.
    #[test]
    fn ownership_asks_only_when_rmux_could_actually_be_installed() {
        let mut app = test_app();
        let own = app.own_preferring_rmux(Deferred::Launch, || "cctop-x".into());
        match (crate::rmux::available(), crate::rmux::installer()) {
            (true, _) => assert!(matches!(own, Some(tabs::Own::Tmux(_)))),
            (false, Some(_)) => {
                assert!(own.is_none(), "the launch waits for the answer");
                assert_eq!(app.mode, Mode::TmuxInstall);
            }
            (false, None) => assert!(matches!(own, Some(tabs::Own::Cctop))),
        }
    }

    /// One "no" holds for the run. Asking again on the next tab would make
    /// declining cost more than accepting, which is not offering a choice.
    #[test]
    fn a_declined_offer_is_not_made_again() {
        let mut app = test_app();
        app.rmux_declined = true;
        let own = app.own_preferring_rmux(Deferred::Launch, || "cctop-x".into());
        assert!(own.is_some(), "the launch goes ahead without asking");
        assert_ne!(app.mode, Mode::TmuxInstall);
    }

    /// Declining still starts the agent — the offer interrupted a launch, and
    /// saying no to rmux is not saying no to the agent.
    #[test]
    fn declining_the_offer_releases_the_launch() {
        let mut app = test_app();
        app.mode = Mode::TmuxInstall;
        app.rmux_install = Some(crate::rmux::Install {
            manager: "apt",
            argv: vec!["sh".into(), "-c".into(), "apt-get install -y rmux".into()],
        });
        app.rmux_deferred = Some(Deferred::Launch);

        app.rmux_install_answer(false);

        assert!(app.rmux_declined);
        assert!(app.rmux_install.is_none());
        assert!(
            app.rmux_deferred.is_none(),
            "the launch was run, not dropped"
        );
        assert_eq!(app.mode, Mode::List);
    }

    /// The failure that would otherwise be invisible: an install that ends
    /// without rmux — it errored, or the user closed the tab — leaves a launch
    /// waiting on a pane that no longer exists.
    #[test]
    fn an_install_that_ends_without_rmux_releases_the_launch() {
        let mut app = test_app();
        // A pid no pane has, standing in for the install tab having gone.
        app.rmux_installing = Some(u32::MAX);
        app.rmux_deferred = Some(Deferred::Launch);

        app.poll_rmux_install();

        assert!(app.rmux_installing.is_none());
        assert!(app.rmux_deferred.is_none());
    }

    /// Regression: these tests once read the developer's real prefs file, so a
    /// persisted `live_only` or age filter silently failed unrelated assertions.
    #[test]
    fn test_app_starts_from_default_prefs() {
        let app = test_app();
        assert!(!app.live_only);
        assert!(app.age_filter.is_none());
        assert!(app.search.is_empty());
        assert_eq!(app.sort_col, ColumnId::Last);
    }

    /// Codex's hooks are written to disk and then sit there doing nothing until
    /// a person has trusted them, and where that trust is recorded is not
    /// something Codex documents — so the only way to know is whether anything
    /// has arrived.
    ///
    /// The permission mode is what answers it: `notify`, Codex's other channel,
    /// carries no such field, so a Codex row that knows its mode is a Codex
    /// whose hooks are firing. Said when a Codex is *started*, which is the
    /// moment `/hooks` is a keystroke away rather than a thing to remember.
    #[test]
    fn a_codex_whose_hooks_are_untrusted_is_told_so_when_it_starts() {
        let mut app = test_app();
        let mut codex = session("c", true, "proj");
        codex.provider = Provider::Codex;
        app.sessions = vec![codex];

        assert!(!app.codex_hooks_heard(), "nothing has reported yet");
        assert!(
            !app.codex_trust_hint(true).is_empty(),
            "a written-but-silent install is exactly the case worth saying"
        );
        assert!(
            app.codex_trust_hint(false).is_empty(),
            "an install nobody asked for is not something to nag about"
        );

        // One hook event carries the mode, which nothing but a hook does.
        app.sessions[0].permission = Some(crate::hook::Permission::Ask);
        assert!(app.codex_hooks_heard());
        assert!(
            app.codex_trust_hint(true).is_empty(),
            "the reminder outlived its purpose"
        );

        // A row from another machine says nothing about this one's hooks: its
        // mode came over `--host` from a cctop where they work.
        app.sessions[0].remote = Some(crate::session::Remote {
            host: "elsewhere".into(),
            branch: None,
        });
        assert!(!app.codex_hooks_heard());

        // And a Claude Code row reporting its mode is not Codex's answer.
        app.sessions[0].remote = None;
        app.sessions[0].provider = Provider::Claude;
        assert!(!app.codex_hooks_heard());
    }

    /// The mode is reported by a live agent but drawn on a row rebuilt by every
    /// walk, so the two have to survive arriving in either order.
    #[test]
    fn the_permission_mode_survives_the_rows_being_rebuilt() {
        let reported = |mode: Option<crate::hook::Permission>| crate::hook::Event {
            session_id: "a".into(),
            reported: crate::hook::Reported {
                signal: crate::hook::Signal::Busy,
                cwd: "/w/proj".into(),
                permission: mode,
                at: std::time::Instant::now(),
            },
            finished_agent: None,
        };
        let mut app = test_app();

        // Reported before the row exists, which is the ordinary order: a
        // `SessionStart` beats the walk that discovers its transcript.
        app.apply_hooks(vec![reported(Some(crate::hook::Permission::Bypass))]);
        app.sessions = vec![session("a", true, "proj")];
        assert_eq!(app.sessions[0].permission, None, "not stamped yet");

        app.apply_permissions();
        assert_eq!(
            app.sessions[0].permission,
            Some(crate::hook::Permission::Bypass),
            "a row discovered after the report still picks it up"
        );

        // An event that says nothing about the mode must not erase it: the
        // setting has not changed just because one event was quiet.
        app.apply_hooks(vec![reported(None)]);
        assert_eq!(
            app.sessions[0].permission,
            Some(crate::hook::Permission::Bypass)
        );

        // A real change is followed.
        app.apply_hooks(vec![reported(Some(crate::hook::Permission::Plan))]);
        assert_eq!(
            app.sessions[0].permission,
            Some(crate::hook::Permission::Plan)
        );
    }

    /// A reported state is kept until the session says it is over, and the
    /// events that change *which sessions exist* ask for a rescan while the
    /// ones that only change a state do not.
    #[test]
    fn a_reported_state_is_kept_until_the_session_ends() {
        let event = |id: &str, signal: crate::hook::Signal| crate::hook::Event {
            session_id: id.into(),
            reported: crate::hook::Reported {
                signal,
                cwd: "/w/proj".into(),
                permission: None,
                at: std::time::Instant::now(),
            },
            // This test is about the session's own state; subagent events are
            // covered where subagents are.
            finished_agent: None,
        };
        let mut app = test_app();

        // Nothing arriving is not "nothing is happening": an absent entry means
        // fall back to the transcript, so it must stay absent.
        assert_eq!(app.apply_hooks(Vec::new()), (false, false));
        assert!(app.hooked_signal("a").is_none());

        // A start is a row to go and find now.
        assert_eq!(
            app.apply_hooks(vec![event("a", crate::hook::Signal::Started)]),
            (true, true)
        );
        // Ordinary state changes are not worth a rescan of the disk.
        assert_eq!(
            app.apply_hooks(vec![
                event("a", crate::hook::Signal::Busy),
                event("a", crate::hook::Signal::Idle),
            ]),
            (true, false)
        );
        assert_eq!(app.hooked_signal("a"), Some(crate::hook::Signal::Idle));
        assert_eq!(app.reporting(), vec![("proj".to_string(), "idle")]);

        // And an ended session is forgotten rather than left claiming its last
        // state forever — which is also a rescan, since the row is going.
        assert_eq!(
            app.apply_hooks(vec![event("a", crate::hook::Signal::Ended)]),
            (true, true)
        );
        assert!(app.hooked_signal("a").is_none());
        assert!(app.reporting().is_empty());
    }

    /// A session that was killed, interrupted, or had its terminal closed sends
    /// no event saying so — the tool never comes back, the turn never ends — and
    /// a claim to be working is the one thing that cannot quietly stay true.
    ///
    /// Regression. Before this, whatever a session last said it was doing was
    /// believed for as long as cctop ran: a tab whose agent had been dead for an
    /// hour was still drawn as busy, and a tool call cut off mid-flight still
    /// read as a permission prompt waiting for an answer.
    #[test]
    fn a_working_claim_nothing_confirms_is_dropped() {
        let stale = |id: &str, signal: crate::hook::Signal| crate::hook::Event {
            session_id: id.into(),
            reported: crate::hook::Reported {
                signal,
                cwd: "/w/proj".into(),
                permission: None,
                at: std::time::Instant::now() - std::time::Duration::from_secs(60 * 60),
            },
            finished_agent: None,
        };
        let mut app = test_app();

        // An hour-old tool call in flight: the tool is not coming back, and
        // reading it as a held question is how a dead tab keeps blinking.
        app.apply_hooks(vec![stale("gone", crate::hook::Signal::Acting)]);
        assert!(app.hooked_signal("gone").is_none());
        assert!(
            app.reporting().is_empty(),
            "the panel listed a state nothing was in"
        );

        // A question that old is still a question: it is waiting on a person,
        // and people take longer than an hour.
        app.apply_hooks(vec![stale("asking", crate::hook::Signal::NeedsInput)]);
        assert_eq!(
            app.hooked_signal("asking"),
            Some(crate::hook::Signal::NeedsInput)
        );
    }

    /// Gemini CLI is the one harness whose rows are not named after the id it
    /// reports: a chat file is named after the first eight characters of the
    /// session id, and that filename is the row's identity because resuming
    /// reuses the id across disjoint files. Without the loose match every Gemini
    /// event would land on no row at all.
    #[test]
    fn a_gemini_event_finds_the_chat_file_it_belongs_to() {
        let mut app = test_app();
        app.apply_hooks(vec![crate::hook::Event {
            session_id: "79709c93-1111-4111-8111-111111111111".into(),
            reported: crate::hook::Reported {
                signal: crate::hook::Signal::Idle,
                cwd: "/w/proj".into(),
                permission: None,
                at: std::time::Instant::now(),
            },
            finished_agent: None,
        }]);

        assert_eq!(
            app.hooked_signal("session-2026-05-14T17-34-79709c93"),
            Some(crate::hook::Signal::Idle)
        );
        // And only that one: a stem whose tail belongs to another session, or an
        // id that is not shaped like Gemini's at all, must not borrow it.
        assert!(
            app.hooked_signal("session-2026-05-14T17-34-deadbeef")
                .is_none()
        );
        assert!(app.hooked_signal("79709c93").is_none());
        assert_eq!(
            gemini_id_tail("session-2026-05-14T17-34-79709c93"),
            Some("79709c93")
        );
        assert_eq!(gemini_id_tail("019fda22-5315-7580-84de-033e4f6835b5"), None);
    }

    /// Answering a prompt in a pane stops the tab asking about it, without
    /// waiting for a hook that only fires once the unblocked tool has finished.
    #[test]
    fn typing_into_a_pane_settles_the_question_it_answers() {
        let mut app = test_app();
        let mut session = session("a", true, "proj");
        session.process.as_mut().unwrap().process_list = vec![crate::proc::ProcEntry {
            pid: 7,
            is_root: true,
            ghost: false,
            cpu: 0.0,
            memory: 0,
            args: String::new(),
        }];
        app.sessions = vec![session];

        // An agent with no hooks has only its transcript to speak for it, and
        // fabricating a report here would shadow it for the rest of the session.
        app.mark_answered(7);
        assert!(app.hooked_signal("a").is_none());

        app.apply_hooks(vec![crate::hook::Event {
            session_id: "a".into(),
            reported: crate::hook::Reported {
                signal: crate::hook::Signal::NeedsInput,
                cwd: "/w/proj".into(),
                permission: None,
                at: std::time::Instant::now(),
            },
            finished_agent: None,
        }]);
        assert_eq!(
            app.hooked_signal("a"),
            Some(crate::hook::Signal::NeedsInput)
        );

        // The keystroke is the answer, so the agent is working again.
        app.mark_answered(7);
        assert_eq!(app.hooked_signal("a"), Some(crate::hook::Signal::Busy));

        // A different agent's keys settle nothing here.
        app.apply_hooks(vec![crate::hook::Event {
            session_id: "a".into(),
            reported: crate::hook::Reported {
                signal: crate::hook::Signal::NeedsInput,
                cwd: "/w/proj".into(),
                permission: None,
                at: std::time::Instant::now(),
            },
            finished_agent: None,
        }]);
        app.mark_answered(8);
        assert_eq!(
            app.hooked_signal("a"),
            Some(crate::hook::Signal::NeedsInput)
        );
    }

    fn session(id: &str, running: bool, label: &str) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.label_source = label.into();
        s.last_active = chrono::Utc::now().to_rfc3339();
        s.started_at = s.last_active.clone();
        if running {
            s.process = Some(crate::proc::ProcInfo::default());
        }
        s
    }

    /// Two live agents in one checkout, both having written the same file —
    /// the arrangement the whole warning exists for.
    #[test]
    fn a_contested_file_reaches_the_footer_and_the_info_panel() {
        let repo = std::env::temp_dir().join(format!("cctop-ui-clash-{}", std::process::id()));
        std::fs::create_dir_all(repo.join(".git")).expect("checkout");
        let dir = repo.to_string_lossy().into_owned();
        let contested = crate::collide::normalise("src/ui/mod.rs", &dir);

        let mut app = test_app();
        app.sessions = vec![session("a", true, &dir), session("b", true, &dir)];
        for s in app.sessions.iter_mut() {
            s.recent_writes = vec![contested.clone()];
        }
        app.collisions = crate::collide::apply(&mut app.sessions);

        // On the rows, so the column can colour and sort by it…
        for s in &app.sessions {
            assert_eq!(s.conflict, Some(crate::collide::Overlap::File));
        }
        // …and in the footer, which names the file rather than a count.
        let footer = app.conflict_footer().expect("a warning");
        assert!(footer.contains("ui/mod.rs"), "{footer}");
        assert!(footer.contains("2 agents"), "{footer}");

        // The panel names the peer, which is the part that says what to do.
        let clash = app.clash_of(&app.sessions[0]).expect("a clash");
        assert_eq!(clash.peers, vec![app.sessions[1].display_label()]);
        assert_eq!(clash.files, vec![contested]);

        // A repository shared without a shared file is the quieter finding, and
        // deliberately does not reach the footer.
        app.sessions[1].recent_writes = vec![crate::collide::normalise("other.rs", &dir)];
        app.collisions = crate::collide::apply(&mut app.sessions);
        assert_eq!(
            app.sessions[0].conflict,
            Some(crate::collide::Overlap::Directory)
        );
        assert!(app.conflict_footer().is_none());

        std::fs::remove_dir_all(&repo).ok();
    }

    /// A remote row survives the walk that replaces the table, counts towards
    /// the totals, and refuses every key that would reach into this machine.
    #[test]
    fn remote_rows_outlive_a_walk_and_stay_read_only() {
        let mut app = test_app();
        app.sessions = vec![session("local", true, "/here")];

        let mut away = session("away", true, "/srv/work");
        away.remote = Some(crate::session::Remote {
            host: "box".into(),
            branch: Some("main".into()),
        });
        away.total_cost = Some(3.0);
        app.remotes.insert("box".into(), vec![away]);
        app.merge_remotes();
        assert_eq!(app.sessions.len(), 2);
        assert!(
            (app.stats.spend_claude - 3.0).abs() < 1e-9,
            "totals span hosts"
        );

        // A full walk replaces the table with this machine's rows only. The
        // remote ones have to come back, or a host would blink out every
        // refresh.
        app.sessions = vec![session("local", true, "/here")];
        app.merge_remotes();
        assert_eq!(app.sessions.len(), 2);
        assert_eq!(
            app.sessions.iter().filter(|s| s.remote.is_some()).count(),
            1
        );

        // And nothing here may act on it.
        let remote = app
            .sessions
            .iter()
            .find(|s| s.remote.is_some())
            .expect("the remote row");
        let why = App::remote_refusal(remote).expect("a refusal");
        assert!(why.contains("box"), "the refusal has to name the machine");
        assert!(App::remote_refusal(&app.sessions[0]).is_none());

        // The branch comes from the far side rather than from this filesystem,
        // where the same path may well exist and mean something else.
        assert_eq!(
            crate::ui::columns::branch_of(remote).as_deref(),
            Some("main")
        );

        // A host that stops answering keeps its rows and says so.
        app.remote_errors
            .insert("box".into(), "Permission denied".into());
        let footer = app.remote_footer().expect("a warning");
        assert!(footer.contains("box"), "{footer}");
        assert!(footer.contains("Permission denied"), "{footer}");
    }

    /// With no host configured the column is one repeated word down every row,
    /// so it is hidden — through the user's own mechanism, so the two cannot
    /// disagree about what is on screen.
    #[test]
    fn the_host_column_stays_off_a_single_machine() {
        let ids = |hidden: &[ColumnId]| -> Vec<ColumnId> {
            columns::visible_columns(300, hidden)
                .iter()
                .map(|c| c.id)
                .collect()
        };
        assert!(ids(&[]).contains(&ColumnId::Host));
        assert!(!ids(&[ColumnId::Host]).contains(&ColumnId::Host));
    }

    fn with_subagents(id: &str, names: &[&str]) -> Session {
        let mut s = session(id, true, id);
        s.subagents = names
            .iter()
            .map(|n| crate::session::Subagent {
                agent_id: format!("agent-{n}"),
                agent_type: "general-purpose".into(),
                description: (*n).into(),
                model: "claude-opus-5".into(),
                started_at: None,
                last_active: None,
                duration_ms: 0,
                status: crate::session::SubagentStatus::Running,
                cost: 0.0,
                tool_count: 0,
                tool_use_id: None,
                context: None,
                ghost: false,
            })
            .collect();
        s
    }

    /// Children belong under the parent they ran for. Sorting them as peers
    /// would scatter one session's subagents down a table ordered by cost or
    /// age, which is the one arrangement that makes the tree meaningless.
    #[test]
    fn expanded_subagents_sit_directly_under_their_parent() {
        let mut app = test_app();
        app.sessions = vec![
            with_subagents("a", &["one", "two"]),
            session("b", true, "b"),
        ];
        app.refilter();
        assert_eq!(app.visible.len(), 2, "collapsed: one row per session");

        app.expanded.insert(app.sessions[0].key());
        app.refilter();

        // Asserted as a position relative to the parent rather than as a fixed
        // list: where the parent lands is the sort's business, and these two
        // sessions are equally recent.
        let at = app
            .visible
            .iter()
            .position(|&r| r == Row::Session(0))
            .expect("parent row");
        assert_eq!(app.visible.len(), 4);
        assert_eq!(
            &app.visible[at..at + 3],
            &[
                Row::Session(0),
                Row::Subagent {
                    parent: 0,
                    index: 0
                },
                Row::Subagent {
                    parent: 0,
                    index: 1
                },
            ]
        );
    }

    /// The cursor is anchored on what it was pointing at, and a child is only
    /// identified by its parent *and* its own id — anchoring on the session key
    /// alone would snap the cursor back to the parent on every refresh, two
    /// times a second, while the user was reading a child row.
    #[test]
    fn the_cursor_stays_on_a_child_row_across_a_refresh() {
        let mut app = test_app();
        app.sessions = vec![with_subagents("a", &["one", "two"])];
        app.expanded.insert(app.sessions[0].key());
        app.refilter();
        app.selected = 2;

        app.refilter();

        assert_eq!(
            app.visible[app.selected],
            Row::Subagent {
                parent: 0,
                index: 1
            }
        );
        assert_eq!(
            app.selected_subagent().map(|s| s.description.clone()),
            Some("two".into())
        );
    }

    /// A child row resolves to its parent for anything addressed to a session,
    /// so every existing action keeps working — but the destructive ones have to
    /// know the difference, because the cursor is on a subagent and the session
    /// they would signal is not what the user pointed at.
    #[test]
    fn a_child_row_owns_its_parent_but_is_not_it() {
        let mut app = test_app();
        app.sessions = vec![with_subagents("a", &["one"])];
        app.expanded.insert(app.sessions[0].key());
        app.refilter();

        app.selected = 0;
        assert!(!app.on_subagent());
        assert!(app.selected_subagent().is_none());

        app.selected = 1;
        assert!(app.on_subagent());
        assert_eq!(
            app.selected_session().map(|s| s.session_id.clone()),
            Some("a".into()),
            "a child still resolves to the session that owns it"
        );
        assert!(app.selected_subagent().is_some());
    }

    /// Collapsing from a child row must leave the cursor somewhere that still
    /// exists; the row it was on is about to be removed.
    #[test]
    fn collapsing_from_a_child_lands_the_cursor_on_its_parent() {
        let mut app = test_app();
        app.sessions = vec![with_subagents("a", &["one", "two"])];
        app.expanded.insert(app.sessions[0].key());
        app.refilter();
        app.selected = 1;

        app.toggle_expanded();

        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.visible[app.selected], Row::Session(0));
        assert!(app.expanded.is_empty());
    }

    /// A background subagent is acknowledged by its parent the moment it
    /// starts, so the transcript says "finished" while it is still working. The
    /// hook is the agent reporting for itself, and it has to win — this is the
    /// difference between a child row that tracks a live agent and one that
    /// reads `done` for the whole run.
    #[test]
    fn a_hooks_word_retires_a_subagent_the_transcript_still_calls_running() {
        let mut app = test_app();
        app.sessions = vec![with_subagents("a", &["one", "two"])];
        assert!(
            app.sessions[0]
                .subagents
                .iter()
                .all(|s| s.status == crate::session::SubagentStatus::Running)
        );

        // The hook names the bare id; the transcript is stored as `agent-<id>`.
        app.finished_agents.insert("one".into());
        app.apply_finished_agents();

        let status = |i: usize| app.sessions[0].subagents[i].status;
        assert_eq!(status(0), crate::session::SubagentStatus::Done);
        assert_eq!(
            status(1),
            crate::session::SubagentStatus::Running,
            "only the subagent named may be retired"
        );
    }

    /// A session with nothing to show must not swallow the key and leave the
    /// user pressing it at a row that never opens.
    #[test]
    fn expanding_a_session_without_subagents_says_so() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "a")];
        app.refilter();

        app.toggle_expanded();

        assert!(app.expanded.is_empty());
        assert!(app.status.is_some(), "the refusal has to be visible");
    }

    #[test]
    fn live_filter_hides_stopped_sessions() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "x"), session("b", false, "y")];
        app.live_only = true;
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0].session()].session_id, "a");
    }

    #[test]
    fn live_filter_includes_transcript_inferred_cursor_session() {
        let mut app = test_app();
        let mut cursor = Session::new(Provider::Cursor, "cursor".into());
        cursor.started_at = chrono::Utc::now().to_rfc3339();
        cursor.last_active = cursor.started_at.clone();
        cursor.inferred_running = true;
        app.sessions = vec![cursor];
        app.live_only = true;
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert!(app.sessions[app.visible[0].session()].is_running());
        assert!(app.sessions[app.visible[0].session()].process.is_none());
    }

    /// The launcher's directory is a field, not a caption. A `claude` opened on
    /// the wrong project reads its way into the wrong repository before anyone
    /// notices, and until now the only way to change it was to restart cctop
    /// somewhere else.
    #[test]
    fn the_launchers_directory_can_be_typed_and_is_checked_before_it_is_taken() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::new(Plan::Retail, channel().0);
        app.launch_cwd = Some(dir.path().to_path_buf());

        // Opens prefilled with what it would have used, so nothing looks lost.
        app.edit_launch_cwd();
        assert_eq!(app.mode, Mode::LaunchCwd);
        assert_eq!(
            app.launch_cwd_input,
            crate::util::tildify(&dir.path().to_string_lossy())
        );

        // A directory that is not one is refused where it was typed, and the
        // field stays open. Failing at launch instead would report it from
        // inside the shim, after the launcher had gone.
        app.launch_cwd_input = dir.path().join("nope").to_string_lossy().into_owned();
        app.accept_launch_cwd();
        assert!(app.launch_cwd_bad);
        assert_eq!(app.mode, Mode::LaunchCwd, "the field stays open");
        assert_eq!(app.launch_cwd.as_deref(), Some(dir.path()), "unchanged");

        // A real one is taken.
        let sub = dir.path().join("work");
        std::fs::create_dir(&sub).expect("mkdir");
        app.launch_cwd_input = sub.to_string_lossy().into_owned();
        app.accept_launch_cwd();
        assert!(!app.launch_cwd_bad);
        assert_eq!(app.mode, Mode::Launch);
        assert_eq!(app.launch_cwd.as_deref(), Some(sub.as_path()));

        // Empty means where cctop was started, which is what the footer calls
        // "this directory" — not an error, and not the previous value.
        app.edit_launch_cwd();
        app.launch_cwd_input = "   ".into();
        app.accept_launch_cwd();
        assert_eq!(app.launch_cwd, None);
        assert_eq!(app.mode, Mode::Launch);
    }

    /// The field is the only place in cctop where a path has to be produced from
    /// memory, so it offers what it can see: the projects agents have run in
    /// before anything is typed, the directories under whatever is typed after.
    #[test]
    fn the_directory_field_offers_paths_instead_of_asking_you_to_recall_them() {
        let root = tempfile::tempdir().expect("tempdir");
        let project = root.path().join("api");
        let deep = project.join("service");
        std::fs::create_dir_all(&deep).expect("mkdir");
        let gone = root.path().join("deleted");

        let mut app = test_app();
        // One project that still exists and one that does not: a recorded
        // directory outlives the checkout it named.
        app.sessions = vec![
            session("a", true, &project.to_string_lossy()),
            session("b", false, &gone.to_string_lossy()),
        ];
        app.launch_root = None;
        app.edit_launch_cwd();

        // Nothing typed, and the project is already on screen. The one that has
        // since been deleted is not, because taking it could only fail.
        assert_eq!(app.launch_cwd_hits, vec![project.clone()]);
        assert!(!app.launch_cwd_hits.contains(&gone));

        // The arrows move into the list and back out of it. Out, because the
        // field is still what is being typed in.
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.launch_cwd_pick, Some(0));
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.launch_cwd_pick, None);

        // Tab on the highlighted project fills it in and then shows what is
        // inside it, which is how a path nobody remembers gets walked to.
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Tab));
        assert_eq!(
            app.launch_cwd_input,
            format!("{}/", project.to_string_lossy())
        );
        assert_eq!(app.launch_cwd_hits, vec![deep.clone()]);
        assert_eq!(app.launch_cwd_pick, None, "a fresh list picks nothing");

        // Enter on a suggestion takes it without the refusal a mistyped path
        // gets — it was listed off the disk moments ago.
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Launch);
        assert!(!app.launch_cwd_bad);
        assert_eq!(app.launch_cwd.as_deref(), Some(deep.as_path()));

        // Typing still decides on its own: a path with no match offers nothing
        // and is refused where it was typed, exactly as before.
        app.edit_launch_cwd();
        app.launch_cwd_input = root.path().join("nowhere").to_string_lossy().into_owned();
        app.launch_cwd_suggest();
        assert!(app.launch_cwd_hits.is_empty());
        app.on_key(key(KeyCode::Enter));
        assert!(app.launch_cwd_bad);
        assert_eq!(app.mode, Mode::LaunchCwd);
    }

    /// Esc has to leave the launch as it was found, or it becomes a way to lose
    /// the setting you opened the field to change.
    #[test]
    fn cancelling_the_directory_field_keeps_the_old_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut app = App::new(Plan::Retail, channel().0);
        app.launch_cwd = Some(dir.path().to_path_buf());
        app.edit_launch_cwd();
        app.launch_cwd_input = "/somewhere/else".into();
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Launch);
        assert_eq!(app.launch_cwd.as_deref(), Some(dir.path()));
    }

    /// The menu and the keyboard must never disagree about what is possible.
    /// Both ask the same predicates; this pins that they still do.
    #[test]
    fn the_menu_refuses_a_remote_row_the_way_the_keys_do() {
        let mut app = App::new(Plan::Retail, channel().0);
        let mut s = session("a", true, "/repo");
        s.remote = Some(crate::session::Remote {
            host: "devbox".into(),
            branch: None,
        });
        app.sessions = vec![s];
        app.refilter();
        app.selected = 0;

        let items = menu::items(&app);
        assert!(!items.is_empty(), "a selected row has a menu");

        // Everything that reaches into this filesystem is refused, and every
        // refusal names the host — the same answer pressing the key gives.
        for item in &items {
            match item.action {
                menu::Action::Expand | menu::Action::Mark => {
                    assert!(item.enabled(), "{} works on a remote row", item.label);
                }
                _ => {
                    let why = item.blocked.as_deref().unwrap_or("");
                    assert!(
                        why.contains("devbox"),
                        "{} must name the host, said {why:?}",
                        item.label
                    );
                }
            }
        }
        // And the cursor never rests on one of the refusals.
        assert!(items[menu::first_enabled(&items)].enabled());
    }

    #[test]
    fn opening_the_menu_needs_a_row_and_lands_on_something_runnable() {
        let mut app = App::new(Plan::Retail, channel().0);
        // No rows: Enter must not open an empty box.
        app.open_row_menu();
        assert_eq!(app.mode, Mode::List);

        app.sessions = vec![session("a", false, "/repo")];
        app.refilter();
        app.selected = 0;
        app.open_row_menu();
        assert_eq!(app.mode, Mode::RowMenu);
        let items = menu::items(&app);
        assert!(items[app.menu_cursor].enabled());
    }

    #[test]
    fn the_root_pid_excludes_ghost_and_child_processes() {
        let mut session = session("a", true, "x");
        session.process.as_mut().unwrap().process_list = vec![
            crate::proc::ProcEntry {
                pid: 1,
                is_root: true,
                ghost: true,
                cpu: 0.0,
                memory: 0,
                args: String::new(),
            },
            crate::proc::ProcEntry {
                pid: 2,
                is_root: false,
                ghost: false,
                cpu: 0.0,
                memory: 0,
                args: String::new(),
            },
            crate::proc::ProcEntry {
                pid: 3,
                is_root: true,
                ghost: false,
                cpu: 0.0,
                memory: 0,
                args: String::new(),
            },
        ];

        assert_eq!(session.root_pid(), Some(3));
    }

    #[test]
    fn runtime_tabs_are_unavailable_for_stopped_sessions() {
        let mut app = test_app();
        app.sessions = vec![session("stopped", false, "x")];
        app.refilter();
        assert!(!app.tab_available(1));
        assert!(!app.tab_available(2));

        app.bottom_tab = 0;
        app.cycle_tab(1);
        assert_eq!(app.bottom_tab, 3);
    }

    #[test]
    fn shift_home_and_end_reach_both_ends_of_a_panel() {
        let mut app = test_app();
        app.bottom_tab = 0;
        // What a draw would have left behind; nothing else knows the length.
        app.panel_max_scroll = 40;

        app.scroll_active_panel(i32::MAX);
        assert_eq!(
            app.info_scroll, 40,
            "End stops at the last line, not past it"
        );
        app.scroll_active_panel(i32::MIN);
        assert_eq!(app.info_scroll, 0);

        // The clamp is the point: without it a scroll past the bottom banks an
        // offset that takes as many presses to come back through.
        app.scroll_active_panel(999);
        app.scroll_active_panel(-1);
        assert_eq!(app.info_scroll, 39);
    }

    #[test]
    fn selecting_stopped_session_leaves_runtime_tab() {
        let mut app = test_app();
        app.sessions = vec![
            session("running", true, "x"),
            session("stopped", false, "y"),
        ];
        app.visible = vec![Row::Session(0), Row::Session(1)];
        app.bottom_tab = 1;
        app.move_selection(1);
        assert_eq!(app.bottom_tab, 0);
    }

    #[test]
    fn search_matches_label_and_id_case_insensitively() {
        let mut app = test_app();
        app.sessions = vec![
            session("aaa", false, "/home/x/alpha"),
            session("bbb", false, "/home/x/beta"),
        ];
        app.search = "alpha".into();
        app.refilter();
        assert_eq!(app.visible.len(), 1);

        app.search = "BBB".into();
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0].session()].session_id, "bbb");
    }

    /// The table abbreviates the working directory to fit its column, so the
    /// filter has to match the full path — otherwise the directory someone
    /// types is one the row is not admitting to.
    #[test]
    fn search_matches_the_full_working_directory() {
        let mut app = test_app();
        let mut deep = session("aaa", false, "/home/x/work/api/services/billing");
        // What the table actually shows for that row.
        deep.abbrev_label = "…/billing".into();
        app.sessions = vec![deep, session("bbb", false, "/home/x/other")];

        app.search = "work/api".into();
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0].session()].session_id, "aaa");
    }

    /// Content hits widen the filter, and only for the query they were found
    /// for: a scan that lands after the user has typed another character must
    /// not put its rows back on screen.
    #[test]
    fn transcript_hits_widen_the_filter_for_their_own_query_only() {
        let mut app = test_app();
        app.sessions = vec![
            session("aaa", false, "/home/x/alpha"),
            session("bbb", false, "/home/x/beta"),
        ];
        let hit = |key: &str| HashMap::from([(key.to_string(), "…flywheel…".to_string())]);

        // No content search: a word only the transcript knows finds nothing.
        app.search = "flywheel".into();
        app.refilter();
        assert!(app.visible.is_empty());

        // With one, the session whose transcript matched joins the metadata
        // matches rather than replacing them.
        app.search_content = true;
        app.scanned("flywheel".into(), hit("claude:bbb"));
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0].session()].session_id, "bbb");

        app.search = "alpha".into();
        app.scanned("alpha".into(), hit("claude:bbb"));
        assert_eq!(
            app.visible.len(),
            2,
            "metadata and content matches, not one"
        );

        // Hits belonging to a query that has since been extended are ignored.
        app.search = "alphabet".into();
        app.refilter();
        assert!(app.visible.is_empty());
        assert!(app.selected_snippet().is_none());
    }

    /// Turning content search off has to take its results with it, or the rows
    /// it found stay on screen with nothing matching them.
    #[test]
    fn leaving_content_search_drops_its_rows() {
        let mut app = test_app();
        app.sessions = vec![session("aaa", false, "/home/x/alpha")];
        app.search = "flywheel".into();
        app.search_content = true;
        app.scanned(
            "flywheel".into(),
            HashMap::from([("claude:aaa".into(), "…flywheel…".into())]),
        );
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.selected_snippet(), Some("…flywheel…"));

        app.toggle_content_search();
        assert!(app.visible.is_empty());
        assert!(app.selected_snippet().is_none());
    }

    /// A scan is worth its cost only once there is a word to look for, and only
    /// for a query that isn't already answered.
    #[test]
    fn a_scan_waits_for_a_word_and_for_the_typing_to_settle() {
        let mut app = test_app();
        app.sessions = vec![session("aaa", false, "/home/x/alpha")];
        app.search_content = true;

        // Too short to be worth reading every transcript for.
        app.search = "fl".into();
        app.search_edited();
        app.scan_typed_at = None;
        app.tick_scan();
        assert!(!app.scanning);

        // Long enough, but the user is still typing.
        app.search = "flywheel".into();
        app.search_edited();
        app.tick_scan();
        assert!(!app.scanning, "fired before the debounce elapsed");

        // Settled.
        app.scan_typed_at = Some(Instant::now() - SCAN_DEBOUNCE);
        app.tick_scan();
        assert!(app.scanning);

        // And the answer to a query already scanned for is not scanned again.
        app.scanned("flywheel".into(), HashMap::new());
        app.tick_scan();
        assert!(!app.scanning);
    }

    /// ↑ walks back through past queries and ↓ returns, ending on whatever was
    /// half-typed when the walk began.
    #[test]
    fn the_query_history_walks_both_ways() {
        let mut app = test_app();
        app.search_history = vec!["newest".into(), "older".into()];

        app.search = "half-typ".into();
        app.history_step(1);
        assert_eq!(app.search, "newest");
        app.history_step(1);
        assert_eq!(app.search, "older");
        // The end of the history is a floor, not a wrap.
        app.history_step(1);
        assert_eq!(app.search, "older");

        app.history_step(-1);
        assert_eq!(app.search, "newest");
        app.history_step(-1);
        assert_eq!(app.search, "half-typ");
        // Nothing older to come back from any more.
        app.history_step(-1);
        assert_eq!(app.search, "half-typ");
    }

    /// Re-running a query moves it to the front rather than filling the history
    /// with copies of the search someone runs most.
    #[test]
    fn a_repeated_query_is_remembered_once() {
        let mut app = test_app();
        app.search = "alpha".into();
        app.remember_query();
        app.search = "beta".into();
        app.remember_query();
        app.search = "alpha".into();
        app.remember_query();
        assert_eq!(app.search_history, vec!["alpha", "beta"]);

        // An abandoned modal leaves nothing behind.
        app.search = "   ".into();
        app.remember_query();
        assert_eq!(app.search_history, vec!["alpha", "beta"]);
    }

    #[test]
    fn selection_follows_the_session_across_a_resort() {
        let mut app = test_app();
        app.sessions = vec![session("a", false, "/x/a"), session("b", false, "/x/b")];
        app.sort_col = ColumnId::Project;
        app.sort_asc = true;
        app.refilter();
        app.selected = 1;
        let before = app.selected_session().unwrap().key();

        app.sort_asc = false;
        app.refilter();
        assert_eq!(app.selected_session().unwrap().key(), before);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn selection_stays_in_bounds_when_sessions_disappear() {
        let mut app = test_app();
        app.sessions = (0..5)
            .map(|i| session(&i.to_string(), false, "/x"))
            .collect();
        app.refilter();
        app.selected = 4;
        app.sessions.truncate(2);
        app.refilter();
        assert!(app.selected < app.visible.len());
    }

    #[test]
    fn empty_list_does_not_panic_on_navigation() {
        let mut app = test_app();
        app.refilter();
        app.move_selection(1);
        app.move_selection(-1);
        assert_eq!(app.selected, 0);
        assert!(app.selected_session().is_none());
    }

    #[test]
    fn sort_toggles_on_repeat_and_resets_on_change() {
        let mut app = test_app();
        app.sort_col = ColumnId::Cost;
        app.sort_asc = true;
        app.set_sort(ColumnId::Cost);
        assert!(!app.sort_asc, "same column must flip direction");
        app.set_sort(ColumnId::Cpu);
        assert!(app.sort_asc, "new column starts ascending");
        assert_eq!(app.sort_col, ColumnId::Cpu);
    }

    #[test]
    fn age_filter_roundtrips_through_prefs_keys() {
        for f in [AgeFilter::Day, AgeFilter::Week, AgeFilter::Month] {
            assert_eq!(AgeFilter::parse(f.key()), Some(f));
        }
        assert_eq!(AgeFilter::parse("nope"), None);
    }

    #[test]
    fn age_filter_excludes_old_sessions() {
        let mut app = test_app();
        let mut old = session("old", false, "/x");
        old.last_active = (chrono::Utc::now() - chrono::Duration::days(10)).to_rfc3339();
        app.sessions = vec![session("new", false, "/x"), old];
        app.age_filter = Some(AgeFilter::Day);
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0].session()].session_id, "new");
    }

    #[test]
    fn cost_floor_filters_by_total_cost() {
        let mut app = test_app();
        let mut cheap = session("cheap", false, "/x");
        cheap.total_cost = Some(0.50);
        let mut pricey = session("pricey", false, "/x");
        pricey.total_cost = Some(5.00);
        app.sessions = vec![cheap, pricey];
        app.cost_floor = 1.0;
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.sessions[app.visible[0].session()].session_id, "pricey");
    }

    #[test]
    fn cost_floor_zero_shows_everything() {
        let mut app = test_app();
        app.sessions = vec![session("a", false, "/x"), session("b", false, "/x")];
        app.cost_floor = 0.0;
        app.refilter();
        assert_eq!(app.visible.len(), 2);
    }

    #[test]
    fn cycle_matches_wraps_around_search_matches() {
        let mut app = test_app();
        let now = chrono::Utc::now().to_rfc3339();
        let mk = |id: &str, label: &str| {
            let mut s = session(id, false, label);
            s.last_active = now.clone();
            s.started_at = now.clone();
            s
        };
        app.sessions = vec![mk("a", "/x/match"), mk("b", "/y"), mk("c", "/z/match")];
        app.search = "match".into();
        app.refilter();
        assert_eq!(app.visible.len(), 2);
        app.selected = 0;
        app.cycle_matches(1);
        assert_eq!(app.selected_session().unwrap().session_id, "c");
        app.cycle_matches(1);
        assert_eq!(app.selected_session().unwrap().session_id, "a");
        app.cycle_matches(-1);
        assert_eq!(app.selected_session().unwrap().session_id, "c");
    }

    #[test]
    fn batch_delete_keeps_marked_sessions_visible_until_worker_confirms() {
        let mut app = test_app();
        app.sessions = vec![
            session("a", false, "/x"),
            session("b", false, "/x"),
            session("c", false, "/x"),
        ];
        app.refilter();
        // Mark a and c by identity, not by row: the three fixtures are created in
        // the same instant, so which row each lands on depends on how the clock
        // happened to tick. Selecting by index made this assert the sort order,
        // and it failed on hosts where those timestamps came out equal or out of
        // creation order.
        let row = |app: &App, id: &str| {
            app.visible
                .iter()
                .position(|&r| app.sessions[r.session()].session_id == id)
                .expect("fixture is visible")
        };
        app.selected = row(&app, "a");
        app.toggle_mark();
        app.selected = row(&app, "c");
        app.toggle_mark();
        assert_eq!(app.marked.len(), 2);
        assert_eq!(app.marked_sessions().len(), 2);

        app.batch(BatchKind::Delete);
        assert_eq!(app.mode, Mode::BatchConfirm);
        app.batch_execute();
        assert_eq!(app.sessions.len(), 3);
        assert_eq!(app.deleting.len(), 2);
        assert!(app.marked.is_empty());
    }

    #[test]
    fn batch_delete_refuses_when_marked_session_is_running() {
        let mut app = test_app();
        app.sessions = vec![session("a", false, "/x"), session("b", true, "/x")];
        app.refilter();
        app.selected = 0;
        app.toggle_mark();
        app.selected = 1;
        app.toggle_mark();
        app.batch(BatchKind::Delete);
        assert_eq!(app.mode, Mode::BatchDeleteBlocked);
    }

    /// The other half of the bell: hearing it is useless if the row it came
    /// from is somewhere in a list of a dozen.
    #[test]
    fn b_jumps_the_selection_to_the_session_that_rang() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "/x/a"), session("b", true, "/x/b")];
        app.refilter();
        let target = app.sessions[1].key();
        app.notify.last = Some(crate::notify::Rang {
            key: target.clone(),
            label: "b".into(),
            reason: crate::notify::Reason::NeedsInput,
            at: Instant::now(),
        });

        app.selected = 0;
        app.jump_to_bell();
        assert_eq!(app.selected_session().map(Session::key), Some(target));

        // Filtered out of the table, the bell has nowhere to land — and says so
        // rather than moving the cursor to an unrelated row.
        app.search = "x/a".into();
        app.refilter();
        app.selected = 0;
        app.jump_to_bell();
        assert_eq!(app.selected, 0);
        assert!(app.status.is_some());
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// The whole point of the rebinding: a vim reflex moves the cursor and
    /// cannot reach a live agent.
    #[test]
    fn k_moves_up_and_never_terminates() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "/x"), session("b", true, "/y")];
        app.refilter();
        app.selected = 1;

        app.on_key(key(KeyCode::Char('k')));
        assert_eq!(app.selected, 0, "k must move up like every modal here");
        assert_eq!(app.mode, Mode::List, "k must not open a kill dialog");

        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected, 1);

        // Terminate still exists, behind a modifier. The fixture's process has
        // no root PID, so it stops at the explanation rather than the confirm —
        // either way, Ctrl+K is what reaches the terminate path at all.
        app.on_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(app.mode, Mode::KillBlocked);
    }

    #[test]
    fn f10_quits_from_an_agent_tab_instead_of_reaching_the_agent() {
        let mut app = test_app();
        app.tab = 1;

        app.on_key(key(KeyCode::F(10)));

        assert!(app.should_quit);
    }

    /// The footer inside a pane advertises the function keys, so none of them
    /// may reach the agent — and the ones that open something have to bring the
    /// dashboard with them, or they land on a screen the agent is repainting.
    #[test]
    fn function_keys_are_cctops_inside_a_pane_and_bring_the_dashboard_with_them() {
        // Each key, and the dashboard state it must leave behind.
        for (code, mode) in [
            (KeyCode::F(1), Mode::Help),
            (KeyCode::F(3), Mode::Search),
            (KeyCode::F(6), Mode::SortBy),
            (KeyCode::F(7), Mode::AgeFilter),
        ] {
            let mut app = test_app();
            app.tab = 1;
            app.on_key(key(code));
            assert_eq!(app.tab, 0, "{code:?} left the dashboard behind");
            assert_eq!(app.mode, mode, "{code:?} did not open its modal");
        }

        // F12 is the pane's own key and F5 acts on the walk, so neither takes
        // you off the agent — F5 says so on the footer instead.
        let mut app = test_app();
        app.tab = 1;
        app.on_key(key(KeyCode::F(5)));
        assert_eq!(app.tab, 1, "refreshing must not leave the agent");
        assert!(app.status.is_some(), "the refresh said nothing");
        app.on_key(key(KeyCode::F(12)));
        assert_eq!(app.tab, 0);

        // An unbound one is swallowed rather than delivered as an escape
        // sequence for the agent to print.
        let mut app = test_app();
        app.tab = 1;
        app.on_key(key(KeyCode::F(2)));
        assert_eq!(app.tab, 1);
        assert_eq!(app.mode, Mode::List);
    }

    /// Regression: F10 in a pane with a launched agent asks before it quits, and
    /// the question was drawn on the dashboard only — so from a pane the key
    /// looked dead while the keyboard was in fact waiting for `y`.
    #[test]
    fn the_quit_question_is_drawn_over_the_pane_that_raised_it() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = test_app();
        app.tab = 1;
        app.hosted = Some((1234, "claude".into()));

        app.on_key(key(KeyCode::F(10)));
        assert!(!app.should_quit, "an owned agent is worth a question");
        assert_eq!(app.mode, Mode::QuitConfirm);

        let (cols, rows) = (80u16, 24u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        terminal
            .draw(|frame| {
                render::draw(frame, &mut app);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let text: String = (0..rows)
            .map(|y| {
                (0..cols)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(
            text.contains("quit anyway"),
            "the question is not on the pane's screen"
        );

        // And the answer still lands: the modal owns the keyboard, not the pane.
        app.on_key(key(KeyCode::Char('y')));
        assert!(app.should_quit);
    }

    /// Every filter that paints a badge must be reachable from Esc, one press
    /// at a time.
    #[test]
    fn esc_clears_one_filter_layer_per_press() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "/x")];
        app.search = "x".into();
        app.cost_floor = 1.0;
        app.live_only = true;
        app.age_filter = Some(AgeFilter::Day);
        app.tool_tab = 2;
        app.refilter();

        for expected in 1..=5 {
            app.on_key(key(KeyCode::Esc));
            let left = [
                !app.search.is_empty(),
                app.cost_floor > 0.0,
                app.live_only,
                app.age_filter.is_some(),
                app.tool_tab != 0,
            ]
            .iter()
            .filter(|on| **on)
            .count();
            assert_eq!(
                left,
                5 - expected,
                "press {expected} cleared the wrong count"
            );
        }
        // A sixth press is harmless.
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::List);
    }

    /// Panel keys are bounded by the tab list, not by a literal that drifts.
    #[test]
    fn number_keys_cover_every_panel_and_nothing_more() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "/x")];
        app.refilter();
        for (i, _) in panels::TABS.iter().enumerate() {
            let digit = char::from_digit(i as u32 + 1, 10).unwrap();
            app.on_key(key(KeyCode::Char(digit)));
            assert_eq!(app.bottom_tab, i, "key {digit} must select panel {i}");
        }
        // One past the end changes nothing rather than selecting a phantom tab.
        let past = char::from_digit(panels::TABS.len() as u32 + 1, 10).unwrap();
        let before = app.bottom_tab;
        app.on_key(key(KeyCode::Char(past)));
        assert_eq!(app.bottom_tab, before);
    }

    /// The live filter and the n/N jump must agree, because they are now the
    /// same predicate.
    #[test]
    fn refilter_and_matches_search_agree() {
        let mut app = test_app();
        app.sessions = vec![
            session("aaa", false, "/home/x/Alpha"),
            session("bbb", false, "/home/x/beta"),
        ];
        for query in ["alpha", "ALPHA", "x/", "", "nomatch"] {
            app.search = query.into();
            app.refilter();
            let by_predicate: Vec<usize> = (0..app.sessions.len())
                .filter(|&i| app.matches_search(&app.sessions[i]))
                .collect();
            assert_eq!(app.visible.len(), by_predicate.len(), "query {query:?}");
        }
    }

    #[test]
    fn case_insensitive_contains_matches_std() {
        for (h, n) in [
            ("Alpha/Beta", "beta"),
            ("Alpha", "alpha"),
            ("Alpha", ""),
            ("a", "aa"),
            ("héllo-World", "world"),
            ("nope", "zz"),
        ] {
            assert_eq!(
                contains_ascii_ci(h, n),
                h.to_ascii_lowercase().contains(n),
                "{h:?} / {n:?}"
            );
        }
    }

    /// An empty table before the first load means "still looking", and the
    /// table draws a different thing for each.
    #[test]
    fn sessions_are_not_reported_empty_before_the_first_load() {
        let app = test_app();
        assert!(!app.loaded);
        assert!(app.sessions.is_empty());
    }
}
