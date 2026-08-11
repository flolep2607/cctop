//! Terminal UI: application state, the worker thread, and the event loop.

pub mod columns;
mod input;
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
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event,
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
/// Tuned against Claude Code and Codex, both of which print a banner and build
/// their prompt before the first keystroke registers. Too short and the line is
/// lost; too long and the user is left looking at an idle agent wondering
/// whether the handoff worked.
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
    /// Text input typed into the selected session's tmux pane.
    SendKeys,
    /// Picking which agent a new tab or split should run.
    Launch,
    /// The agent-integration panel: what is installed where, and whether the
    /// agents are actually reporting in.
    Hooks,
    /// Offering to install tmux, a launch having found it missing.
    TmuxInstall,
}

/// A launch that stopped to ask about tmux, and how to pick it up again.
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
    /// Raw digits being typed into the cost-floor modal.
    pub cost_input: String,
    /// Line being typed into the selected session's terminal.
    pub send_input: String,
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
    /// Last computed maximum scroll for the tool log, recorded during draw so
    /// the key handler knows where the bottom is.
    pub tool_max_scroll: u16,
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
    /// highlighted. It also keeps a `tmux` subprocess out of the draw loop.
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
    /// The install the tmux offer is currently showing, so the modal draws the
    /// command that will actually run rather than working it out again.
    pub tmux_install: Option<crate::tmux::Install>,
    /// The launch waiting on the tmux question, or on the install it started.
    pub tmux_deferred: Option<Deferred>,
    /// Whether the offer has been turned down. One "no" holds for the run:
    /// asking again on the next tab would make declining tmux cost more than
    /// accepting it, which is a way of not really offering a choice.
    ///
    /// Not persisted — a decision about this machine belongs in whether tmux is
    /// installed on it, and cctop already reads that directly.
    pub tmux_declined: bool,
    /// The pane running the install, while one is running.
    ///
    /// Watched for two endings: tmux appearing, which releases the deferred
    /// launch into a tmux-backed pane, and the pane going away without it,
    /// which means the install failed and the launch should stop waiting.
    pub tmux_installing: Option<u32>,
    /// A handoff brief waiting for the agent the launcher is about to start.
    ///
    /// Held across the launcher rather than typed at the moment `H` is pressed,
    /// because the agent that will receive it does not exist yet: `H` writes the
    /// brief and opens the launcher, and whichever agent is picked inherits it.
    pub pending_brief: Option<std::path::PathBuf>,
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
            cost_input: String::new(),
            send_input: String::new(),
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
            tool_max_scroll: 0,
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
            update_available: None,
            status: None,
            started_at: chrono::Utc::now().to_rfc3339(),
            started: Instant::now(),
            prefs,
            tx,
            tabs: Vec::new(),
            tab: 0,
            hooked: HashMap::new(),
            hooks: None,
            listener: None,
            launch_cursor: 0,
            launch_offer: Vec::new(),
            launch_into: LaunchInto::Tab,
            launch_root: std::env::current_dir().ok(),
            launch_cwd: None,
            tmux_install: None,
            tmux_deferred: None,
            tmux_declined: false,
            tmux_installing: None,
            pending_brief: None,
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
    fn toggle_expanded(&mut self) {
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
        let bump = |v: &mut u16| *v = (*v as i32 + delta).max(0) as u16;
        match self.bottom_tab {
            0 => bump(&mut self.info_scroll),
            1 => {} // Performance is a fixed-size chart pair
            2 => bump(&mut self.proc_scroll),
            3 => {
                let next = (self.tool_scroll as i32 + delta).clamp(0, self.tool_max_scroll as i32);
                self.tool_scroll = next as u16;
                // Re-pin once the user scrolls back down to the newest entry.
                self.tool_follow = self.tool_scroll >= self.tool_max_scroll;
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
        let fields: [&str; 6] = [
            s.display_label(),
            &s.model,
            &s.harness,
            s.provider.as_str(),
            &s.session_id,
            &s.label_source,
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
        match self.selected_session() {
            Some(s) if session_root_pid(s).is_some() => self.mode = Mode::KillConfirm,
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
    fn toggle_mark(&mut self) {
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
            BatchKind::Kill => session_root_pid(s).is_some(),
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
                BatchKind::Kill => match session_root_pid(s) {
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

    /// Adjust the live refresh interval, clamping to sane bounds.
    fn adjust_refresh(&mut self, delta: f64) {
        self.refresh_secs = (self.refresh_secs + delta).clamp(0.5, 60.0);
        self.needs_redraw = true;
    }

    /// Half the visible table height, used by Ctrl+U/Ctrl+D. Falls back to a
    /// page size before the first draw has recorded a viewport.
    fn half_page(&self) -> usize {
        ((self.list_height as usize / 2).max(1)).min(PAGE as usize)
    }
}

/// Rows moved by PageUp/PageDown and the fallback for half-page scrolls.
const PAGE: isize = 10;

/// Half-period of the tab-bar blink. Slow enough to read the title through,
/// fast enough to catch the eye.
const BLINK_MS: u128 = 600;

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
        crate::hook::status(self.hook_project().as_deref(), self.listener.as_ref())
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
        if let Some(reported) = self.hooked.get(session_id) {
            return Some(reported.signal);
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
            .filter(|session| session_root_pid(session) == Some(pid))
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
    fn mark_answered(&mut self, pid: u32) {
        let answered: Vec<String> = self
            .sessions
            .iter()
            .filter(|session| session_root_pid(session) == Some(pid))
            .map(|session| session.session_id.clone())
            .collect();
        for id in answered {
            if let Some(reported) = self.hooked.get_mut(&id)
                && !reported.signal.is_working()
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
        self.tab = tab.min(self.tabs.len());
        self.needs_redraw = true;
    }

    /// Move `delta` tabs along, wrapping through the dashboard.
    pub fn cycle_workspace(&mut self, delta: isize) {
        let count = self.tabs.len() as isize + 1;
        self.tab = (self.tab as isize + delta).rem_euclid(count) as usize;
        self.needs_redraw = true;
    }

    /// Open the launcher, remembering where the pick should go and which
    /// directory it should start in.
    pub fn launch_prompt(&mut self, into: LaunchInto) {
        if matches!(into, LaunchInto::Split { .. }) && self.active_tab().is_none() {
            self.set_status("Nothing to split — open a tab first");
            return;
        }
        let offer = tabs::choices(&self.open_tmux());
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
        // The receiving agent belongs in the directory the work is in, whatever
        // row the cursor moves to while the launcher is up.
        self.launch_prompt(LaunchInto::Tab);
        // `launch_prompt` bails on its own when nothing can be launched, and
        // leaving a brief pending for a launcher that never opened would attach
        // it to the next unrelated agent instead.
        if self.mode != Mode::Launch {
            self.pending_brief = None;
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
        // The transcript is full of paths relative to where the agent ran, so a
        // resumed session belongs in the same directory.
        let cwd = session.work_dir();
        let what = format!("{} · {}", session.display_label(), argv[0]);
        // Named after the session, so resuming it a second time reattaches to
        // the agent already doing it rather than starting a rival.
        let tmux = crate::tmux::name_for_session(session.provider.as_str(), &session.session_id);

        // Already on screen: switch to it. tmux would attach a second client to
        // the same agent, which works but leaves two panes fighting over one
        // window's size for no reason.
        //
        // Asked of `resumed` as well as of `tmux`, because without tmux
        // installed every pane's `tmux` is `None` and the question would answer
        // "no" every time — putting a second agent on one transcript, which is
        // the thing `ResumeConfirm` exists to warn about and which would happen
        // here with no warning at all, the session having already stopped.
        if let Some(at) = self.tabs.iter().position(|tab| {
            tab.panes
                .iter()
                .any(|p| p.tmux.as_deref() == Some(&tmux) || p.resumed.as_deref() == Some(&tmux))
        }) {
            self.tab = at + 1;
            self.needs_redraw = true;
            self.set_status(format!("Already open: {what}"));
            return;
        }

        let Some(own) = self.own_preferring_tmux(Deferred::Resume, || tmux.clone()) else {
            return;
        };
        // Reattaching is not resuming: the agent was never gone, so saying
        // "resumed" would misdescribe what just happened.
        let verb = match &own {
            tabs::Own::Tmux(name) if crate::tmux::exists(name) => "Reattached to",
            _ => "Resumed",
        };
        self.open_tab(&argv, cwd, &what, own, verb, Some(tmux));
    }

    /// Where the agent about to start should live, offering to install tmux if
    /// that is the only reason it would not be tmux-backed.
    ///
    /// `None` means the question is on screen and the caller must stop. The
    /// launch is not held anywhere in the meantime — [`Deferred`] records only
    /// which of the two entry points to run again once there is an answer.
    ///
    /// The silent fallback is kept for every machine where the question cannot
    /// be usefully asked — no package manager, or no way to reach root. tmux is
    /// how this is *better*, not how it works, and such a machine gets exactly
    /// the behaviour cctop had before rather than a complaint about a program
    /// the user never asked for. The offer exists for the machine where the
    /// fallback would instead quietly cost the user a feature one keypress away.
    fn own_preferring_tmux(
        &mut self,
        deferred: Deferred,
        name: impl FnOnce() -> String,
    ) -> Option<tabs::Own> {
        if crate::tmux::available() {
            return Some(tabs::Own::Tmux(name()));
        }
        // Asked in this order so that installing tmux in another window still
        // works: `available` above is the live check, and neither a previous
        // "no" nor a running install is consulted until it has said no.
        if self.tmux_declined || self.tmux_installing.is_some() {
            return Some(tabs::Own::Cctop);
        }
        // No package manager to offer means there is nothing to ask about, so
        // this is the plain fallback rather than a refusal: `?` here would
        // return `None`, which the caller reads as "the launch is waiting on an
        // answer" — and no answer would ever come, so the tab never opened.
        let Some(install) = crate::tmux::installer() else {
            return Some(tabs::Own::Cctop);
        };
        self.tmux_install = Some(install);
        self.tmux_deferred = Some(deferred);
        self.mode = Mode::TmuxInstall;
        self.needs_redraw = true;
        None
    }

    /// Answer the tmux offer: run the install in a pane, or give up on tmux for
    /// this run and start the agent on cctop's own pty.
    pub(super) fn tmux_install_answer(&mut self, install: bool) {
        self.mode = Mode::List;
        let Some(offer) = self.tmux_install.take() else {
            return;
        };
        if !install {
            self.tmux_declined = true;
            self.run_deferred_launch();
            return;
        }
        // In a pane, not a subprocess: `sudo` wants a password, and a pane is a
        // pty the user can type it into. It also puts the package manager's
        // output somewhere it can be read, which is the difference between a
        // failed install and a tab that closed for no stated reason.
        match tabs::Pane::launch(&offer.argv, None, tabs::Own::Cctop) {
            Ok(pane) => {
                self.tmux_installing = Some(pane.pid);
                self.tabs.push(tabs::Tab::new(pane));
                self.tab = self.tabs.len();
                self.set_status(format!("Installing tmux with {}", offer.manager));
            }
            Err(error) => {
                self.set_status(format!("Could not run the install: {error}"));
                self.tmux_declined = true;
                self.run_deferred_launch();
            }
        }
    }

    /// Watch a running install to whichever of its two ends it reaches.
    ///
    /// Called from the poll loop after panes are reaped, so "the pane is gone"
    /// is already true here rather than true one tick later.
    pub(super) fn poll_tmux_install(&mut self) {
        let Some(pid) = self.tmux_installing else {
            return;
        };
        if crate::tmux::available() {
            self.tmux_installing = None;
            self.set_status("tmux installed");
            self.run_deferred_launch();
            return;
        }
        // The pane is gone and tmux is still not here: the install failed, or
        // the user closed it. Either way the launch has waited long enough, and
        // it goes where it would have gone had nothing been offered.
        let open = self
            .tabs
            .iter()
            .flat_map(|tab| tab.panes.iter())
            .any(|pane| pane.pid == pid);
        if !open {
            self.tmux_installing = None;
            self.tmux_declined = true;
            if self.tmux_deferred.is_some() {
                self.set_status("tmux was not installed — starting without it");
            }
            self.run_deferred_launch();
        }
    }

    /// Re-run whichever launch stopped to ask about tmux.
    fn run_deferred_launch(&mut self) {
        match self.tmux_deferred.take() {
            Some(Deferred::Resume) => self.resume_now(),
            Some(Deferred::Launch) => self.launch_selected(),
            None => {}
        }
    }

    /// Start `argv` in a new tab, reporting what happened either way.
    ///
    /// `resumed` names the session the tab is going back to, when it is going
    /// back to one — what the next resume of it looks itself up by.
    fn open_tab(
        &mut self,
        argv: &[String],
        cwd: Option<std::path::PathBuf>,
        what: &str,
        own: tabs::Own,
        verb: &str,
        resumed: Option<String>,
    ) {
        let mut pane = match tabs::Pane::launch(argv, cwd.as_deref(), own) {
            Ok(pane) => pane,
            Err(error) => {
                self.set_status(format!("Could not start {what}: {error}"));
                return;
            }
        };
        pane.resumed = resumed;
        // Worth saying once per tab: it changes what quitting cctop means.
        let kept = match pane.outlives_cctop() {
            true => " — it will outlive cctop",
            false => "",
        };
        self.tabs.push(tabs::Tab::new(pane));
        self.tab = self.tabs.len();
        let where_ = cwd
            .map(|dir| format!(" in {}", crate::util::tildify(&dir.to_string_lossy())))
            .unwrap_or_default();
        self.set_status(format!("{verb} {what}{where_}{kept}"));
    }

    /// The tmux sessions this cctop already has a pane onto.
    pub fn open_tmux(&self) -> Vec<String> {
        self.tabs
            .iter()
            .flat_map(|tab| tab.panes.iter())
            .filter_map(|pane| pane.tmux.clone())
            .collect()
    }

    /// What the launcher is offering.
    pub fn launch_choices(&self) -> &[tabs::Choice] {
        &self.launch_offer
    }

    /// What a still-running agent in the launcher is doing, if it has said.
    ///
    /// This is the whole reason the offer carries a pid. A list of tmux session
    /// names says which agents exist; this says which one is stuck on a question
    /// and which finished ten minutes ago, from the same hooks the dashboard
    /// reads — so choosing which to go back to is a decision rather than a guess.
    pub fn waiting_state(&self, agent: &crate::tmux::Running) -> Option<crate::hook::Signal> {
        self.pane_signal(agent.pid?)
    }

    /// What to call a still-running agent, when cctop can do better than its
    /// tmux session name.
    ///
    /// That name is an identity and not something written to be read: a resumed
    /// session's carries the whole session id, so it comes out as a timestamp
    /// and a uuid that no two rows differ in until well past the width of the
    /// column. The agent's pid finds its row, and the row already knows what the
    /// dashboard calls it — which is the name the user recognises.
    pub fn waiting_label(&self, agent: &crate::tmux::Running) -> Option<String> {
        let pid = agent.pid?;
        self.sessions
            .iter()
            .find(|session| session_root_pid(session) == Some(pid))
            .map(|session| session.display_label().to_string())
    }

    /// Start the launcher's pick.
    pub fn launch_selected(&mut self) {
        let Some(choice) = self.launch_offer.get(self.launch_cursor).cloned() else {
            return;
        };
        let cwd = self.launch_cwd.clone();
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
                let own = self.own_preferring_tmux(Deferred::Launch, || {
                    crate::tmux::free_name(&tabs::label_of(argv))
                });
                // The offer went up instead. This runs again from the top when
                // it is answered, and the launcher's snapshot is still here to
                // run it from.
                let Some(own) = own else { return };
                (argv.clone(), own)
            }
        };
        // The offer is a snapshot, and an agent can finish in the time the modal
        // is up. Attaching to a session that has gone spawns a client that exits
        // at once — a tab that flickers and vanishes, where the truth is simply
        // that the agent ended while being looked at.
        if let tabs::Choice::Waiting(agent) = &choice
            && !crate::tmux::exists(&agent.name)
        {
            self.set_status(format!("{} has ended", choice.label()));
            return;
        }

        let argv = &argv;
        let pane = match tabs::Pane::launch(argv, cwd.as_deref(), own) {
            Ok(pane) => pane,
            Err(error) => {
                self.set_status(format!("Could not start {}: {error}", tabs::label_of(argv)));
                return;
            }
        };
        let label = pane.label.clone();
        // A brief goes to an agent that is starting fresh. Reattaching lands in
        // a conversation already under way, where typing a "read this and
        // continue" line would interrupt whatever it is doing mid-turn.
        if let Some(path) = self.pending_brief.take()
            && matches!(choice, tabs::Choice::Start(_))
        {
            self.handoff_send = Some((
                pane.pid,
                crate::handoff::prompt_for(&path),
                Instant::now() + HANDOFF_SETTLE,
            ));
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
                self.tab = self.tabs.len();
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
                format!("Started {label}{where_}{kept}")
            }
        });
    }

    /// Close the focused pane, ending the agent behind it.
    ///
    /// Closing used to detach from a tmux-backed agent and leave it running,
    /// which meant the tab came back at the next launch and the only way to be
    /// rid of it was a second key. Closing a window is meant to be the end of
    /// it, so this kills the tmux session outright — the same thing Alt+Shift+W
    /// does, which is now a synonym rather than the only way to stop an agent.
    ///
    /// The exception is a pane opened with `a`, which is a window onto somebody
    /// else's agent. There is nothing here to kill and stopping it was never
    /// cctop's to do, so that one is only closed.
    pub fn close_pane(&mut self) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        if tab.focus >= tab.panes.len() {
            return;
        }
        // Out of the tab first: for a cctop-owned pty, dropping the pane is the
        // kill, and it must happen either way rather than only when tmux agrees.
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
        let mut index = 0;
        self.tabs.retain(|tab| {
            index += 1;
            if !tab.panes.is_empty() {
                return true;
            }
            // The tabs after this one shift down by one, so a view sitting on
            // any of them has to follow — otherwise closing tab 1 silently
            // moves you to what used to be tab 2.
            if self.tab >= index {
                self.tab -= 1;
            }
            false
        });
        self.tab = self.tab.min(self.tabs.len());
    }

    /// Put the selected agent's own terminal on screen, in a tab of its own.
    ///
    /// Two ways in, because there are two ways an agent's terminal can belong to
    /// cctop. A shim holding a pty has a copy of the output to give away; an agent
    /// handed to tmux has none, and is reached by becoming another of its clients
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
        let resumed = crate::tmux::name_for_session(session.provider.as_str(), &session.session_id);
        let Some(pid) = session_root_pid(session) else {
            self.set_status("Selected session has no local process");
            return;
        };
        if self.open_view(pid, label.clone()) {
            return;
        }
        // Started by cctop and then handed to tmux. Without this the message
        // below would say cctop did not start an agent cctop started, and send
        // the user to relaunch something that is already running.
        if let Some(name) = crate::tmux::holding(pid) {
            // A second client onto one session leaves the two panes arguing over
            // one window's size, so an agent already on screen is switched to.
            if let Some(at) = self
                .tabs
                .iter()
                .position(|tab| tab.panes.iter().any(|p| p.tmux.as_deref() == Some(&name)))
            {
                self.tab = at + 1;
                self.needs_redraw = true;
                self.set_status(format!("Already open: {label}"));
                return;
            }
            self.open_tab(
                &[title],
                None,
                &label,
                tabs::Own::TmuxExisting(name),
                "Attached to",
                Some(resumed),
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

    /// Restore the tmux-backed agents cctop left alive on a previous exit.
    ///
    /// The tmux session is the durable workspace state: it preserves the agent,
    /// its scrollback, and working directory. Recreating a client for each one
    /// makes reopening cctop resume the tabs the user had open, while leaving a
    /// failed or concurrently ended session out of the workspace.
    pub(super) fn restore_running_tabs(&mut self) {
        for agent in crate::tmux::running() {
            let choice = tabs::Choice::Waiting(agent.clone());
            let label = choice.label();
            if let Ok(pane) =
                tabs::Pane::launch(&[label], None, tabs::Own::TmuxExisting(agent.name))
            {
                self.tabs.push(tabs::Tab::new(pane));
            }
        }
        if !self.tabs.is_empty() {
            self.tab = 1;
        }
    }

    /// Show the agent running as `pid`, reusing the pane already on it rather
    /// than opening a second window onto one terminal.
    ///
    /// A pane is a match on either pid it has: the one cctop hosts, and — for a
    /// tmux-backed pane, where that one is only the client — the agent's own.
    /// Asking about the hosted pid alone missed every tmux-backed pane, so an
    /// agent already on screen got a second window onto it.
    fn open_view(&mut self, pid: u32, label: String) -> bool {
        let shows = |pane: &tabs::Pane| pane.pid == pid || pane.agent() == pid;
        if let Some((index, tab)) = self
            .tabs
            .iter_mut()
            .enumerate()
            .find(|(_, tab)| tab.panes.iter().any(shows))
        {
            tab.focus = tab.panes.iter().position(shows).unwrap_or(0);
            self.tab = index + 1;
            self.needs_redraw = true;
            return true;
        }
        let Some(pane) = tabs::Pane::view_of(pid, label) else {
            return false;
        };
        self.tabs.push(tabs::Tab::new(pane));
        self.tab = self.tabs.len();
        self.needs_redraw = true;
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

/// PID of the currently live agent root, excluding briefly retained exits.
fn session_root_pid(session: &Session) -> Option<u32> {
    session
        .process
        .as_ref()?
        .process_list
        .iter()
        .find_map(|process| (process.is_root && !process.ghost).then_some(process.pid))
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

    // One cached check per day, off the UI thread. Only ever reports: replacing
    // the binary stays behind an explicit `--update`.
    std::thread::spawn(move || {
        if let Some(version) = crate::update::available_update() {
            let _ = res_tx.send(Response::UpdateAvailable(version));
        }
    });

    let mut app = App::new(args.plan, req_tx.clone());
    app.refresh_secs = args.delay;
    let _ = req_tx.send(Request::Refresh);

    // Tabs backed by tmux outlive cctop. Reattach them before the first frame
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
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    // Bracketed paste, so a paste arrives as one `Event::Paste` instead of as
    // one `Event::Key` per character. Without it there is no way to tell a paste
    // from typing, and the newlines in a pasted message reach the agent as the
    // Enter that submits it — a five-line paste asking five questions.
    let _ = execute!(std::io::stdout(), EnableBracketedPaste);

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
    // tmux-backed one is a client, and the agent behind it is left running —
    // which is the point, and so worth saying out loud on the way out.
    let had_tabs = !app.open_tmux().is_empty();
    app.tabs.clear();
    drop(hosted);

    // Asked after the clients are gone, and asked of tmux rather than of the
    // tabs: an agent left running by an earlier cctop is just as reachable as
    // one from this run, and the line below is the only thing that tells anyone
    // they are there at all.
    let left_running = match had_tabs {
        true => crate::tmux::sessions(),
        // Nothing here ever touched tmux, so nothing here is owed an account of
        // what is in it.
        false => Vec::new(),
    };

    restore_terminal();

    // After the restore, so it lands on the terminal the user is handed back
    // rather than inside the alternate screen that is about to be torn down.
    if !left_running.is_empty() {
        println!(
            "{} agent{} still running in tmux; `cctop` then `t` to get back to {}.",
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
/// `ratatui::restore` intentionally only disables raw mode and leaves the
/// alternate screen; it does not restore cursor visibility. Keep this separate
/// so regular exits, input errors, and panics all use the same cleanup path.
fn restore_terminal() {
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    let _ = execute!(std::io::stdout(), Show);
    ratatui::restore();
    // Send Show once more after leaving the alternate screen. Some terminals
    // scope cursor state to the active screen buffer.
    let _ = execute!(std::io::stdout(), Show);
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
                let status = crate::quota::fetch_claude();
                claude_due =
                    now + Duration::from_secs(status.retry_delay_secs(QUOTA_INTERVAL_SECS));
                quota.claude = status;
                changed = true;
            }
            if now >= codex_due {
                let status = crate::quota::fetch_codex();
                codex_due = now + Duration::from_secs(status.retry_delay_secs(QUOTA_INTERVAL_SECS));
                quota.codex = status;
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
                    rows_changed = true;
                }
                Ok(Response::Annotated(session)) => {
                    // Match on the key's two fields rather than on `key()`: that
                    // formats a String per candidate, so a scan over thousands of
                    // rows allocated thousands of times — per arriving row, on the
                    // thread that also has to answer the keyboard.
                    let found = app.sessions.iter_mut().find(|s| {
                        s.provider == session.provider && s.session_id == session.session_id
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
                    app.push_history();
                    app.refilter();
                    refresh_in_flight = false;
                    annotated_rows_changed = false;
                    rows_changed = true;
                }
                Ok(Response::LiveRows(payload)) => {
                    let (rows, stats) = *payload;
                    for row in rows {
                        let found = app
                            .sessions
                            .iter_mut()
                            .find(|s| s.provider == row.provider && s.session_id == row.session_id);
                        match found {
                            Some(existing) => *existing = row,
                            // A session that started since the last full walk.
                            None => app.sessions.push(row),
                        }
                    }
                    app.stats = stats;
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

        app.sync_panel_data();
        app.tick_scan();

        // Every tab, not just the visible one: an agent whose output nobody
        // reads eventually blocks on writing it.
        let mut drawn = false;
        for tab in &mut app.tabs {
            drawn |= tab.pump();
        }
        let closed = app.tabs.iter_mut().fold(false, |any, tab| tab.reap() | any);
        if closed {
            app.drop_empty_tabs();
        }
        // After the reap, so a finished install is seen as finished on the same
        // tick its pane goes away.
        app.poll_tmux_install();
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

    /// Regression: the "already open" guard asked only about `tmux`, which is
    /// `None` on every pane when tmux is not installed — so `R` on a session
    /// already resumed in a tab started a second agent on the one transcript,
    /// and being stopped, it did so without even the confirmation.
    #[cfg(target_os = "linux")]
    #[test]
    fn resuming_a_session_already_in_a_tab_goes_to_that_tab() {
        let (mut child, pid) = crate::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let mut pane = tabs::Pane::view_of(pid, "claude".into()).expect("attach");
        // What a resumed tab records regardless of who carries the agent. The
        // pane has no tmux, standing in for a machine without it.
        pane.resumed = Some(crate::tmux::name_for_session("claude", "abc"));
        assert!(pane.tmux.is_none());

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

    /// The three ways the ownership decision can go, since only one of them is
    /// new: tmux present is unchanged, tmux absent and uninstallable is the old
    /// silent fallback, and only tmux absent but installable stops to ask.
    #[test]
    fn ownership_asks_only_when_tmux_could_actually_be_installed() {
        let mut app = test_app();
        let own = app.own_preferring_tmux(Deferred::Launch, || "cctop-x".into());
        match (crate::tmux::available(), crate::tmux::installer()) {
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
        app.tmux_declined = true;
        let own = app.own_preferring_tmux(Deferred::Launch, || "cctop-x".into());
        assert!(own.is_some(), "the launch goes ahead without asking");
        assert_ne!(app.mode, Mode::TmuxInstall);
    }

    /// Declining still starts the agent — the offer interrupted a launch, and
    /// saying no to tmux is not saying no to the agent.
    #[test]
    fn declining_the_offer_releases_the_launch() {
        let mut app = test_app();
        app.mode = Mode::TmuxInstall;
        app.tmux_install = Some(crate::tmux::Install {
            manager: "apt",
            argv: vec!["sh".into(), "-c".into(), "apt-get install -y tmux".into()],
        });
        app.tmux_deferred = Some(Deferred::Launch);

        app.tmux_install_answer(false);

        assert!(app.tmux_declined);
        assert!(app.tmux_install.is_none());
        assert!(
            app.tmux_deferred.is_none(),
            "the launch was run, not dropped"
        );
        assert_eq!(app.mode, Mode::List);
    }

    /// The failure that would otherwise be invisible: an install that ends
    /// without tmux — it errored, or the user closed the tab — leaves a launch
    /// waiting on a pane that no longer exists.
    #[test]
    fn an_install_that_ends_without_tmux_releases_the_launch() {
        let mut app = test_app();
        // A pid no pane has, standing in for the install tab having gone.
        app.tmux_installing = Some(u32::MAX);
        app.tmux_deferred = Some(Deferred::Launch);

        app.poll_tmux_install();

        assert!(app.tmux_installing.is_none());
        assert!(app.tmux_deferred.is_none());
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

    #[test]
    fn session_root_pid_excludes_ghost_and_child_processes() {
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

        assert_eq!(session_root_pid(&session), Some(3));
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

    #[test]
    fn refresh_interval_adjusts_and_clamps() {
        let mut app = test_app();
        app.refresh_secs = 2.0;
        app.adjust_refresh(0.5);
        assert_eq!(app.refresh_secs, 2.5);
        app.adjust_refresh(-10.0);
        assert_eq!(app.refresh_secs, 0.5);
        app.adjust_refresh(100.0);
        assert_eq!(app.refresh_secs, 60.0);
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
