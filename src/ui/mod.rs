//! Terminal UI: the application state every other module in here operates on.
//!
//! [`App`] is one struct with a field for everything on screen, and the methods
//! that change it are split across this directory by what they are *about*
//! rather than by type: [`select`] owns the cursor, [`filter`] what the table
//! shows, [`launch`] and [`panes`] the agents and the tabs they run in,
//! [`signals`] what those agents report, [`worker`] the thread that does the
//! filesystem work and [`runloop`] the terminal and the event loop. They are
//! `impl App` blocks in sibling modules, so nothing here had to become public
//! to make the split; what stays is the state itself and the few things — the
//! modes, the row type, construction and the status line — that every one of
//! them touches.

mod batch;
pub mod columns;
mod dirs;
mod filter;
mod hooks;
mod input;
mod launch;
mod launch_cwd;
pub mod menu;
mod modals;
pub mod panels;
mod panes;
mod profiles;
mod remote;
pub mod render;
mod runloop;
mod select;
mod share;
mod signals;
pub mod spark;
mod table;
pub mod tabs;
pub mod theme;
mod worker;

pub use runloop::run;
use share::Opening;
use worker::Request;

use crate::cache::UiPrefs;
use crate::loader::Stats;
use crate::pricing::{Plan, Provider};
use crate::quota::Quota;
use crate::session::{Session, SessionData};
use columns::ColumnId;
use spark::History;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

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
    /// `cctop optimize` or `cctop compare`, drawn over the table.
    Insight,
    /// Offering to install rmux, a launch having found it missing.
    TmuxInstall,
    /// Typing a new name for a workspace tab, opened by right-clicking it.
    RenameTab,
    /// The browser panel: whether this cctop is serving its table to one, on
    /// what links, and whether they leave the machine.
    Serve,
}

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
    /// When a right-click opened the rename prompt.
    ///
    /// Terminals that paste on the right button — Windows Terminal does, and
    /// keeps doing it while mouse capture is on — send the click *and* the
    /// clipboard, so the prompt opens with somebody's last copy already typed
    /// into it. A paste that lands in the same instant as the click that
    /// opened the field is that echo, not a person, and is dropped.
    pub rename_opened_by_click: Option<Instant>,
    /// Whether the footer's `q Quit` has been clicked once already.
    ///
    /// The share corner's `share_arm` for the other irreversible thing a
    /// pointer can reach. Cleared by any other click, so it only ever spans one
    /// deliberate pair.
    pub quit_arm: bool,
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
    /// The process tree each session's hooks reported running under, keyed by
    /// session id.
    ///
    /// Kept apart from `hooked` because it answers a different question and
    /// changes on a different clock: `hooked` is what the agent is *doing* and
    /// turns over constantly, while this is *where it is* and is written once
    /// and then repeated. Only the changes go to the worker, which is what makes
    /// storing it separately worth a field — see [`App::note_hook_pids`].
    pub(super) hook_pids: HashMap<String, Vec<u32>>,
    /// The integration's state, as of the last time the panel was opened.
    ///
    /// Rebuilt on opening and after every action rather than every frame: it
    /// reads three files off disk and scans a directory, which is nothing to do
    /// once and wasteful to do sixty times a second behind a closed panel.
    pub hooks: Option<crate::hook::Report>,
    /// Readings of each account's rate-limit windows, for the unused-allowance
    /// figure on the Limits pane. Reloaded when a quota response arrives rather
    /// than held by the poller, so the file has exactly one writer.
    pub burn: crate::burn::Log,
    /// The open report, and how far it is scrolled. `None` while the worker is
    /// still building it, which is what the overlay draws as "working".
    pub insight: Option<String>,
    pub insight_scroll: u16,
    /// Which report was asked for, so the overlay can name itself before the
    /// text arrives.
    pub insight_kind: &'static str,
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
    /// A tunnel being registered on a thread of its own.
    ///
    /// Registering with Cloudflare's edge is a second or more of network, and
    /// doing it on the UI thread stopped the dashboard dead: no frame, no
    /// keyboard, and nothing on screen saying why. It runs beside the loop
    /// instead, and the corner spins while it does.
    pub share_opening: Option<Opening>,
    /// Whether the footer's share button has been armed by a first click.
    ///
    /// The second click is what opens the tunnel. Not persisted and not carried
    /// between clicks on anything else: arming is a promise about the click
    /// happening now, and one left standing from ten minutes ago is exactly the
    /// stray click it exists to prevent.
    pub share_arm: bool,
    /// Why the last attempt to serve failed, kept for the panel to show.
    ///
    /// A port in use or a tunnel that would not register are both answers
    /// somebody has to read, and a status line that has since been overwritten
    /// by a refresh is not where they can read it.
    pub serve_error: Option<String>,
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
            burn: crate::burn::Log::load(),
            insight: None,
            insight_scroll: 0,
            insight_kind: "optimize",
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
            rename_opened_by_click: None,
            quit_arm: false,
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
            // Loaded rather than started empty, because the row most likely to
            // want a tab blinking is the one blocked on a question — and that
            // is exactly the row that sends nothing until it is answered. See
            // [`hook::load_claims`](crate::hook::load_claims).
            hook_pids: crate::hook::load_claims(),
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
            share_opening: None,
            share_arm: false,
            pending_brief: None,
            pending_fork: None,
            handoff_send: None,
            hosted: None,
            needs_redraw: true,
            should_quit: false,
        }
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

    /// Open `optimize` or `compare` over the table.
    ///
    /// The report is built on the worker and arrives later, so this opens the
    /// overlay empty rather than blocking the render loop for the second or two
    /// a full re-parse takes.
    fn open_insight(&mut self, which: &'static str) {
        self.insight = None;
        self.insight_scroll = 0;
        self.insight_kind = which;
        self.mode = Mode::Insight;
        let _ = self.tx.send(Request::Insight { which });
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::sync::mpsc::channel;

    pub(super) fn test_app() -> App {
        let (tx, rx) = channel();
        // Keep the receiver alive so sends in tests don't fail.
        std::mem::forget(rx);
        App::with_prefs(Plan::Retail, tx, UiPrefs::default())
    }

    /// The Limits pane answers "is there room to keep working". This adds the
    /// other question nobody is shown — whether the plan was worth buying —
    /// and it must stay quiet until it has enough windows to be worth saying.
    #[test]
    fn the_limits_pane_reports_unused_allowance_once_it_has_enough_windows() {
        use crate::burn::{Log, Sample, key};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = test_app();
        app.quota = Quota {
            fetched: true,
            claude: vec![crate::quota::ProfileQuota {
                profile: "default".into(),
                source: crate::config::AccountSource::Directory,
                status: crate::quota::ProviderStatus::Ok(crate::quota::ProviderQuota {
                    plan: Some("max_20x".into()),
                    windows: vec![crate::quota::Window {
                        label: "7d",
                        pct: 20,
                        duration: Some(Duration::from_secs(7 * 24 * 3600)),
                        resets_at: None,
                    }],
                    limit_reached: false,
                }),
            }],
            codex: Vec::new(),
        };

        let screen_of = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(160, 40)).expect("backend");
            terminal
                .draw(|frame| {
                    render::draw(frame, app);
                })
                .expect("draw");
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };

        // One completed window is one quiet week, not a pattern, so nothing is
        // claimed from it.
        let mut log = Log::default();
        let k = key("claude", "default", "7d");
        let week = 7 * 24 * 3600;
        let push = |log: &mut Log, w: i64, peak: u32| {
            let start = 1_000_000 + w * week;
            for i in 0..10 {
                log.record(
                    k.clone(),
                    Sample {
                        at: start + i * (week / 10),
                        pct: (peak as i64 * (i + 1) / 10) as u32,
                        resets_at: Some(start + week),
                        plan: Some("max_20x".into()),
                    },
                );
            }
        };
        push(&mut log, 0, 60);
        push(&mut log, 1, 50); // opens the second window, completing the first
        app.burn = log.clone();
        assert!(
            !screen_of(&mut app).contains("unused"),
            "one completed window is not a pattern"
        );

        // A third window completes the second, and now there is something to
        // say: 40% and 50% unused, averaging 45%.
        push(&mut log, 2, 10);
        app.burn = log;
        let screen = screen_of(&mut app);
        assert!(
            screen.contains("~45% of 7d unused"),
            "the unused share never reached the pane:\n{screen}"
        );
    }

    /// The report overlay draws the text the command prints, and says so while
    /// it is still being built — the wait is a full re-parse of every
    /// transcript, and an empty box would read as a broken one.
    #[test]
    fn the_report_overlay_draws_its_text_and_says_when_it_is_working() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let screen_of = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("backend");
            terminal
                .draw(|frame| {
                    render::draw(frame, app);
                })
                .expect("draw");
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };

        let mut app = test_app();
        app.mode = Mode::Insight;
        app.insight_kind = "optimize";
        app.insight = None;
        let waiting = screen_of(&mut app);
        assert!(
            waiting.contains("This is the slow one"),
            "an empty overlay must explain itself: {waiting}"
        );

        app.insight = Some("  fix    something worth doing\n  habit  something else\n".into());
        let drawn = screen_of(&mut app);
        assert!(drawn.contains("something worth doing"), "{drawn}");
        assert!(drawn.contains("esc close"), "the way out is on the frame");

        // Scrolling past the end must not empty the frame: the clamp is what
        // stops a held-down key from leaving a box with nothing in it and no
        // sign it is still open.
        app.insight_scroll = u16::MAX;
        let scrolled = screen_of(&mut app);
        assert!(scrolled.contains("something else"), "{scrolled}");
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

    pub(super) fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    pub(super) fn session(id: &str, running: bool, label: &str) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.label_source = label.into();
        s.last_active = chrono::Utc::now().to_rfc3339();
        s.started_at = s.last_active.clone();
        if running {
            s.process = Some(crate::proc::ProcInfo::default());
        }
        s
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
    fn age_filter_roundtrips_through_prefs_keys() {
        for f in [AgeFilter::Day, AgeFilter::Week, AgeFilter::Month] {
            assert_eq!(AgeFilter::parse(f.key()), Some(f));
        }
        assert_eq!(AgeFilter::parse("nope"), None);
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
