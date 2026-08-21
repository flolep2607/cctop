//! Workspace tabs: the dashboard, plus a terminal for every agent you open.
//!
//! Nothing here is new machinery. [`shim::host`](crate::shim::host) already puts
//! an agent on a pty cctop owns, and [`attach`](crate::attach) already turns that
//! pty into a screen that can be drawn into any rectangle and resized to it. A
//! tab is a list of those screens; a split is that list drawn side by side
//! instead of one at a time.
//!
//! The dashboard is not in this list — it is tab zero and always there, so
//! [`App::tab`](super::App::tab) is `0` for the session table and `1..=len` for
//! these.

use std::path::Path;
use std::time::{Duration, Instant};

/// How long a pane's screen has to sit still before the agent counts as idle.
///
/// An agent that is working repaints constantly — Claude Code's "✻ Baked for
/// 5s" alone ticks every second — so silence is the signal, and it needs no
/// per-harness parsing. Two seconds clears the gap between a spinner's frames
/// without waiting so long that a finished turn goes unnoticed. A blinking
/// cursor does not count: the terminal draws that, not the agent.
///
// ponytail: an agent that redraws nothing while thinking would read as idle.
// None of the four do; if one appears, its transcript's activity state is the
// tiebreak.
const QUIET_IS_IDLE: Duration = Duration::from_secs(2);

/// How often a tmux-backed pane re-asks tmux which process the agent is, until
/// it gets an answer.
///
/// Bounded because the question is asked from the draw loop. It is only asked at
/// all while unanswered, which in practice is the first moment of a pane's life.
const FIND_AGENT_EVERY: Duration = Duration::from_millis(500);

/// Why a tab is asking to be looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attention {
    /// The agent has stopped drawing: its turn is over and the prompt is yours.
    Idle,
    /// The agent has explicitly asked something and is blocked on the answer.
    NeedsInput,
}

/// One terminal on screen.
pub struct Pane {
    /// What this pane started. Dropping it kills that process, which is why a
    /// pane merely *looking at* someone else's session leaves this `None`: `a`
    /// on a session row must not make cctop responsible for its life.
    ///
    /// For a tmux-backed pane the process here is the tmux *client*, not the
    /// agent — see [`tmux`](crate::tmux).
    hosted: Option<crate::shim::Hosted>,
    /// The tmux session the agent is really in, when there is one.
    ///
    /// Its presence is what separates closing a pane from ending an agent: with
    /// it, the two are different acts and only the second needs asking about.
    pub tmux: Option<String>,
    /// The session this pane was opened to resume, named as
    /// [`tmux::name_for_session`](crate::tmux::name_for_session) names it.
    ///
    /// Recorded whether or not tmux is what carries the agent, because it is the
    /// only durable answer to "is this session already open?" — `tmux` alone is
    /// `None` on every pane when tmux is not installed, and resuming one
    /// transcript into two agents is precisely what that question guards.
    pub resumed: Option<String>,
    /// The Claude profile this pane's agent was started under, when it is not
    /// the default one.
    ///
    /// Kept on the pane because that is the only thing that still knows: the
    /// profile reaches the agent as an environment variable, which is invisible
    /// from the outside, and the border needs it to show the right account's
    /// limits rather than whichever account cctop itself would have used.
    ///
    /// Filled in by the caller that chose it, like `resumed` below.
    pub profile: Option<String>,
    /// The pid cctop hosts. A second request to view the same agent finds this
    /// pane rather than opening a duplicate onto one terminal.
    pub pid: u32,
    /// The agent's own pid, once known, for a pane that is not hosting it
    /// directly. See [`Pane::agent`] for why this is worth chasing.
    agent: Option<u32>,
    /// When tmux was last asked who the agent is, so an unanswered question is
    /// retried without being retried every frame.
    asked_at: Option<Instant>,
    pub label: String,
    pub view: crate::attach::Attach,
    /// When this pane's screen last changed, which is how idleness is told
    /// without asking the agent or its transcript anything.
    drew_at: Instant,
}

impl Pane {
    /// Whether the agent has gone quiet long enough to count as waiting for you.
    fn idle(&self) -> bool {
        self.drew_at.elapsed() >= QUIET_IS_IDLE
    }

    /// Whether the agent in this pane has rung and not yet been looked at.
    ///
    /// The one signal in `attention` that the agent sends deliberately. Every
    /// other input there is inference — a hook event, or a screen that stopped
    /// moving — and this is the agent saying it outright, in the language every
    /// terminal has understood for fifty years.
    fn rang(&self) -> bool {
        self.view.rang().is_some()
    }

    /// Answer the bell, because this pane is the one being looked at, and give
    /// back what the agent said when it rang.
    pub fn answer_bell(&mut self) -> Option<String> {
        self.view.answer()
    }

    /// The pid of the agent this pane shows.
    ///
    /// The same thing as [`Pane::pid`] everywhere except under tmux, where that
    /// is the client and this is the agent behind it. Everything asking what the
    /// agent *is doing* — its hooks, its transcript, its row in the table —
    /// wants this one, because that is the process all of it is keyed by.
    ///
    /// Falls back to the hosted pid while the answer is still unknown. That
    /// finds nothing, which is right: nothing is better than the wrong agent,
    /// and the caller already has a fallback for a pane it knows nothing about.
    pub fn agent(&self) -> u32 {
        self.agent.unwrap_or(self.pid)
    }

    /// Learn which process the agent is, if that is not known yet.
    ///
    /// It cannot be settled at launch. cctop spawns a tmux *client* and returns;
    /// the server creating the session and spawning the agent inside it happens
    /// on its own time, so for the first moments of a pane's life there is no
    /// pane to ask about. Once found it is kept — a tmux pane's command outlives
    /// every client that ever looks at it, so the answer cannot go stale while
    /// this pane is alive to hold it.
    fn find_agent(&mut self) {
        let Some(name) = self.tmux.as_deref() else {
            return;
        };
        if self.agent.is_some()
            || self
                .asked_at
                .is_some_and(|at| at.elapsed() < FIND_AGENT_EVERY)
        {
            return;
        }
        self.asked_at = Some(Instant::now());
        self.agent = crate::tmux::agent_pid(name);
        // The first moment the session is known to be there is the first moment
        // its options can be set, and settling it here means it is done once per
        // pane rather than on a timer. Every attach passes through, so a session
        // left by an older cctop is quieted when it is picked up again.
        if self.agent.is_some() {
            crate::tmux::quiet(name);
            crate::tmux::mouse(name);
            // The one moment the session exists and this pane's label is settled
            // — the callers that rename a pane do it before it is ever pumped.
            // Every other cctop reads the tab's name back off the session, so
            // without this a resumed agent would be one thing here and a uuid
            // next door.
            crate::tmux::set_label(name, &self.label);
            // Only when there is one to record: an unset option reads back as
            // "the default account", which is exactly what `None` means here.
            if let Some(profile) = &self.profile {
                crate::tmux::set_profile(name, profile);
            }
        }
    }

    /// Whether the agent survives this pane going away.
    pub fn outlives_cctop(&self) -> bool {
        self.tmux.is_some()
    }

    /// Whether ending this pane's agent is cctop's to do.
    ///
    /// False for a pane opened with `a`, which is a window onto an agent started
    /// somewhere else: there is no pty to drop and no tmux session to kill, so
    /// the pane can be closed but the agent cannot be reached. Asking first is
    /// the difference between a key that declines and a key that appears to work
    /// and does nothing.
    pub fn owns_agent(&self) -> bool {
        self.hosted.is_some() || self.tmux.is_some()
    }
}

/// Who owns the agent a pane is opened onto.
#[derive(Debug, Clone)]
pub enum Own {
    /// A tmux session of this name, so the agent outlives cctop. An existing
    /// session of that name is attached to rather than replaced.
    Tmux(String),
    /// A tmux session that is already running: attach, never create. Picking one
    /// from the launcher that has since ended must fail and say so, not quietly
    /// start something new under its name.
    TmuxExisting(String),
    /// A pty cctop owns, which ends when cctop does.
    Cctop,
}

impl Pane {
    /// Start `argv` and open a pane onto it.
    pub fn launch(argv: &[String], cwd: Option<&Path>, own: Own) -> anyhow::Result<Pane> {
        let tmux = match &own {
            Own::Tmux(name) | Own::TmuxExisting(name) => Some(name.clone()),
            Own::Cctop => None,
        };
        let spawn = match &own {
            Own::Tmux(name) => {
                // Before the client, not after: the pane's scrollback is fixed
                // the moment it is made. See [`tmux::prepare`].
                crate::tmux::prepare(argv, name, cwd);
                crate::tmux::attach_or_create(argv, name, cwd)
            }
            Own::TmuxExisting(name) => crate::tmux::attach(name),
            Own::Cctop => argv.to_vec(),
        };
        let hosted = crate::shim::host(&spawn, cwd)?;
        // The shim binds and serves the socket before returning, so there is
        // something to connect to even though the agent has drawn nothing yet.
        let mut view = crate::attach::attach(hosted.pid).ok_or_else(|| {
            anyhow::anyhow!(
                "{} started but its terminal could not be opened",
                label_of(argv)
            )
        })?;
        // With tmux in the middle the agent's keyboard-protocol request never
        // reaches this end; see [`attach::Attach::assume_extended_keys`].
        if tmux.is_some() {
            view.assume_extended_keys();
        }
        Ok(Pane {
            pid: hosted.pid,
            // The agent's name, not the wrapper's: a tab reading `tmux
            // new-session -A -s cctop-claude-32cca860` names the plumbing.
            label: label_of(argv),
            view,
            tmux,
            // Filled in by the caller that knows: launching is not resuming, and
            // most launches are not any session in particular.
            resumed: None,
            profile: None,
            agent: None,
            asked_at: None,
            hosted: Some(hosted),
            drew_at: Instant::now(),
        })
    }

    /// Open a pane onto an agent something else is responsible for.
    pub fn view_of(pid: u32, label: String) -> Option<Pane> {
        Some(Pane {
            hosted: None,
            tmux: None,
            resumed: None,
            // Nothing was launched here, so there is no choice to record.
            profile: None,
            pid,
            // Nothing stands between this pane and the agent: the pid asked for
            // is the agent's, which is what makes this the answer already.
            agent: Some(pid),
            asked_at: None,
            label,
            view: crate::attach::attach(pid)?,
            drew_at: Instant::now(),
        })
    }

    /// End the agent behind this pane for good.
    ///
    /// Only tmux-backed panes need this. Everywhere else, dropping the pane
    /// already is the kill — which is the whole reason the two have to be told
    /// apart once tmux is in the picture.
    pub fn kill_agent(&self) -> Result<(), String> {
        match &self.tmux {
            Some(name) => crate::tmux::kill(name),
            None => Ok(()),
        }
    }

    /// Whether the agent behind this pane has gone.
    fn finished(&mut self) -> bool {
        match self.hosted.as_mut() {
            Some(hosted) => hosted.finished().is_some(),
            // Nothing here owns the process, so the connection is the only
            // evidence there is: a shim that exited closes it.
            None => self.view.closed(),
        }
    }
}

/// A tmux session a tab stands for while this cctop holds no client on it.
///
/// Every cctop on this machine shows a tab for every cctop-owned tmux session,
/// including the ones another cctop started. Holding a client on all of them
/// would be the wrong way to do it: tmux would have several clients on one
/// window, and the size they argue their way to is nobody's. So an unwatched tab
/// keeps only this — enough to name it, to say when its agent wants you, and to
/// attach the moment you switch to it.
#[derive(Debug, Clone)]
pub struct Shared {
    /// The tmux session, which is the tab's identity across every cctop.
    pub name: String,
    /// What the cctop that started it called the tab. See
    /// [`tmux::set_label`](crate::tmux::set_label).
    pub label: String,
    /// The agent's own pid, so a tab nobody is attached to can still say that it
    /// is waiting on you — the hooks report under this and need no pane.
    pub pid: Option<u32>,
    /// When the session last drew anything, in unix seconds, as of the last
    /// sync. Stands in for [`Pane::drew_at`] on a tab that has no pane to watch.
    pub activity: Option<u64>,
    /// The account the agent was started under, carried across the trade so the
    /// pane this becomes again reports the same limits it did before. See
    /// [`Pane::profile`].
    pub profile: Option<String>,
}

impl Shared {
    /// Whether the agent has gone quiet long enough to count as waiting for you.
    ///
    /// The same judgement [`Pane::idle`] makes, from tmux's record of the
    /// session rather than from a screen — an unwatched tab has no screen. A
    /// second of slack on top of [`QUIET_IS_IDLE`], because tmux reports this to
    /// the second and the sweep that read it is already up to
    /// [`SHARE_EVERY`](super::SHARE_EVERY) old: without it a busy agent flickers
    /// idle between sweeps.
    fn idle(&self) -> bool {
        let Some(activity) = self.activity else {
            return false;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(activity) > QUIET_IS_IDLE.as_secs()
    }
}

/// One workspace tab: the panes shown together, and which of them has the
/// keyboard.
pub struct Tab {
    pub panes: Vec<Pane>,
    pub focus: usize,
    /// Panes stacked top to bottom rather than laid out left to right.
    ///
    /// One direction for the whole tab. A split *tree* would let every pane
    /// divide differently, and nothing has needed that yet — this covers the
    /// side-by-side and the stacked case with a bool.
    pub stacked: bool,
    /// Set only while `panes` is empty: the session this tab is a placeholder
    /// for. [`Tab::attach`] trades it for a pane and [`Tab::detach`] trades it
    /// back, so the two are never both true and an empty tab with no `shared` is
    /// still an agent that has exited.
    pub shared: Option<Shared>,
}

impl Tab {
    pub fn new(pane: Pane) -> Tab {
        Tab {
            panes: vec![pane],
            focus: 0,
            stacked: false,
            shared: None,
        }
    }

    /// A tab for a tmux session this cctop has not attached to — one another
    /// cctop started, or one this cctop left when you switched away.
    pub fn shared(agent: &crate::tmux::Running) -> Tab {
        Tab {
            panes: Vec::new(),
            focus: 0,
            stacked: false,
            shared: Some(Shared {
                label: agent.label.clone().unwrap_or_else(|| {
                    // No label recorded: an agent from a cctop older than this,
                    // or one whose `set-option` did not land. The session name is
                    // the fallback, minus the prefix every one of them carries.
                    agent
                        .name
                        .strip_prefix("cctop-")
                        .unwrap_or(&agent.name)
                        .to_string()
                }),
                name: agent.name.clone(),
                pid: agent.pid,
                activity: agent.activity,
                profile: agent.profile.clone(),
            }),
        }
    }

    /// Whether this tab is a session with no client of ours on it.
    pub fn detached(&self) -> bool {
        self.panes.is_empty() && self.shared.is_some()
    }

    /// The tmux sessions this tab stands for, whether attached or not.
    pub fn sessions(&self) -> impl Iterator<Item = &str> {
        self.panes
            .iter()
            .filter_map(|pane| pane.tmux.as_deref())
            .chain(self.shared.iter().map(|s| s.name.as_str()))
    }

    /// Put a client of this cctop back on the session this tab stands for.
    ///
    /// Only ever called on the way to looking at the tab, which is what makes
    /// the trade sound: at most one cctop is being *used* on a session at a time,
    /// so at most one of them holds the client whose size tmux fits the window
    /// to.
    pub fn attach(&mut self) -> anyhow::Result<()> {
        let Some(shared) = self.shared.clone() else {
            return Ok(());
        };
        // `TmuxExisting` never creates: a session that ended between the sync
        // that found it and this must fail and say so, not silently start a new
        // agent under a dead agent's name.
        let argv = [shared.label.clone()];
        let mut pane = Pane::launch(&argv, None, Own::TmuxExisting(shared.name.clone()))?;
        // The label the other cctop chose, not one reconstructed from the argv
        // above — which is the label already, but only by coincidence.
        pane.label = shared.label;
        // Reattaching is not relaunching, so nothing here chose an account —
        // this is the one the session was started under, read back off tmux by
        // the sweep that found it or kept from the pane this tab last had.
        pane.profile = shared.profile;
        self.panes = vec![pane];
        self.focus = 0;
        self.shared = None;
        Ok(())
    }

    /// Give up this cctop's client on the session, keeping the tab.
    ///
    /// Dropping the pane kills the tmux client and nothing else — the session,
    /// and the agent in it, carry on for whichever cctop looks next. Declines for
    /// anything that could not be rebuilt from a session name: a split, a pty
    /// cctop owns and would therefore *end* here, or a pane merely looking at
    /// somebody else's agent.
    pub fn detach(&mut self) -> bool {
        let [pane] = &self.panes[..] else {
            return false;
        };
        let Some(name) = pane.tmux.clone() else {
            return false;
        };
        self.shared = Some(Shared {
            name,
            label: pane.label.clone(),
            pid: Some(pane.agent()),
            // Nothing has been read off tmux for this tab yet, and the pane it
            // is replacing was on screen a moment ago. The next sweep fills it.
            activity: None,
            profile: pane.profile.clone(),
        });
        self.panes.clear();
        self.focus = 0;
        true
    }

    /// Call the tab something other than the command that started it.
    ///
    /// Written onto the tmux session as well as the pane, because the label is
    /// how every *other* cctop names this tab — and how this one names it again
    /// after a detach. A rename only the pane remembered would come back as the
    /// old name the moment either happened.
    ///
    /// The first pane, not the focused one: a split tab is titled after its
    /// first pane with a count of the rest, so that is the label the bar shows.
    pub fn rename(&mut self, name: String) {
        if let Some(pane) = self.panes.first_mut() {
            if let Some(session) = pane.tmux.as_deref() {
                crate::tmux::set_label(session, &name);
            }
            pane.label = name;
        } else if let Some(shared) = self.shared.as_mut() {
            crate::tmux::set_label(&shared.name, &name);
            shared.label = name;
        }
    }

    /// What the tab bar calls this tab.
    pub fn title(&self) -> String {
        match self.panes.len() {
            0 | 1 => self
                .panes
                .first()
                .map(|p| p.label.clone())
                .or_else(|| self.shared.as_ref().map(|s| s.label.clone()))
                .unwrap_or_default(),
            n => format!("{} +{}", self.panes[0].label, n - 1),
        }
    }

    pub fn focused_mut(&mut self) -> Option<&mut Pane> {
        self.panes.get_mut(self.focus)
    }

    /// Move the keyboard to the next pane, wrapping.
    pub fn cycle_focus(&mut self) {
        if !self.panes.is_empty() {
            self.focus = (self.focus + 1) % self.panes.len();
        }
    }

    /// Fold in whatever the agents have drawn. True when anything changed.
    ///
    /// Every pane is pumped, not just the focused one: the shim's output has to
    /// be read whether or not it is on screen, and a background pane whose
    /// buffer filled up would stall the agent behind it.
    pub fn pump(&mut self) -> bool {
        self.panes.iter_mut().fold(false, |changed, pane| {
            // Not `||`: that short-circuits, and every pane must be drained.
            let drew = pane.view.pump();
            if drew {
                pane.drew_at = Instant::now();
            }
            // Here because it is the one thing every pane does every tick, and
            // the answer has to be chased rather than waited for — see
            // [`Pane::find_agent`].
            pane.find_agent();
            drew | changed
        })
    }

    /// What this tab wants, if anything — the most urgent of its panes.
    ///
    /// `known` is what has actually been *reported* about a pane's agent, from
    /// its hooks or its transcript. When it answers, it wins: an agent saying
    /// its turn is over beats any inference drawn from the pixels. When it does
    /// not — the agent has no hooks and has written nothing yet — the pane's own
    /// screen is the fallback, and the only thing that can be read off a screen
    /// is that it stopped moving.
    ///
    /// The focused pane never asks: you are looking straight at it.
    ///
    /// `known` is asked about [`Pane::agent`] and not the pid cctop hosts, which
    /// is the difference between a tmux-backed tab that knows what its agent is
    /// doing and one reduced to guessing from its screen.
    pub fn attention(
        &self,
        focused: bool,
        known: &dyn Fn(u32) -> Option<crate::hook::Signal>,
    ) -> Option<Attention> {
        // A tab nobody is attached to has no screen to read, so `known` is not
        // the tiebreak here — it is the whole answer. Which is enough for the
        // case that matters: an agent another cctop started, blocked on a
        // question, still blinks at you here.
        if let Some(shared) = &self.shared {
            return match shared.pid.and_then(&known) {
                Some(crate::hook::Signal::NeedsInput) => Some(Attention::NeedsInput),
                // The held-prompt shape, read off tmux's record of the session
                // instead of a screen. See the pane arm below for why a tool in
                // flight over a still terminal is a question.
                Some(crate::hook::Signal::Acting) => shared.idle().then_some(Attention::NeedsInput),
                Some(signal) if signal.is_working() => None,
                Some(_) => Some(Attention::Idle),
                // No hooks, so the fallback is the same one a pane uses — that
                // the thing has stopped drawing — asked of tmux instead of a
                // screen this cctop does not have.
                None => shared.idle().then_some(Attention::Idle),
            };
        }
        self.panes
            .iter()
            .enumerate()
            .filter(|(i, _)| !(focused && *i == self.focus))
            .filter_map(|(_, pane)| match known(pane.agent()) {
                // A bell over a turn the agent has already reported as finished
                // is the nudge, not a question: Claude Code rings its
                // `idle_prompt` notification a minute after `Stop` down the
                // same bell it rings a permission prompt with, and reading that
                // as a held question is how a tab that is merely done goes
                // amber and stays amber. The agent's own word for its state is
                // the one thing that can tell the two apart.
                Some(crate::hook::Signal::Idle) if pane.rang() => Some(Attention::Idle),
                // Otherwise, before anything inferred: the agent rang. A harness
                // rings when it is blocked on you — see [`Pane::rang`].
                _ if pane.rang() => Some(Attention::NeedsInput),
                Some(crate::hook::Signal::NeedsInput) => Some(Attention::NeedsInput),
                // A tool call that started, has not come back, and has stopped
                // repainting is a permission prompt waiting on you. Claude Code
                // says so outright — `PermissionRequest` arrives as
                // `NeedsInput`, matched above — but it is the only harness that
                // does: the rest raise a `Notification` on a six-second timer or
                // nothing at all, and a permission prompt leaves no trace in a
                // transcript either. So without this a tab blocked on one is
                // drawn as merely idle, the same green as a tab whose turn is
                // simply over.
                //
                // The two halves are both needed. A tool in flight alone is the
                // ordinary case; a still screen alone is the finished turn the
                // green already covers. Together they are the one thing that
                // holds an agent mid-tool without it drawing anything.
                //
                // ponytail: a tool that runs long *and* silently reads the same
                // way. Claude Code, Gemini and Cursor all tick an elapsed timer
                // while a tool runs, so in practice the screen is only still
                // when the agent is blocked.
                Some(crate::hook::Signal::Acting) => pane.idle().then_some(Attention::NeedsInput),
                // Reported as working — compacting and just-started included:
                // the screen is irrelevant, and this is the case the heuristic
                // gets wrong for an agent that thinks quietly.
                Some(signal) if signal.is_working() => None,
                // Its turn is over, or the session is.
                Some(_) => Some(Attention::Idle),
                None => pane.idle().then_some(Attention::Idle),
            })
            // A held question outranks a finished turn: one of them is blocking
            // an agent, the other is only waiting on you when you get to it.
            .max_by_key(|a| matches!(a, Attention::NeedsInput))
    }

    /// Drop the panes whose agents have exited. True once nothing is left.
    ///
    /// A detached tab is never nothing left: it holds no pane by design, and
    /// what becomes of it is the sync's to decide — the session it stands for
    /// outlives every client, this cctop's included.
    pub fn reap(&mut self) -> bool {
        self.panes.retain_mut(|pane| !pane.finished());
        self.focus = self.focus.min(self.panes.len().saturating_sub(1));
        self.panes.is_empty() && self.shared.is_none()
    }
}

/// The agents a new pane can be started with: the ones cctop already knows how
/// to alias, filtered to what is actually installed, plus the login shell.
///
/// The shell earns its place — half of what a tab is wanted for next to an agent
/// is `git diff`, and a tab that can only hold an agent would send you back out
/// to another window for it.
pub fn harnesses() -> Vec<Vec<String>> {
    let mut found: Vec<Vec<String>> = crate::alias::AGENTS
        .split_whitespace()
        .filter(|agent| crate::shim::is_command(agent))
        .map(|agent| vec![agent.to_string()])
        .collect();
    if let Some(shell) = std::env::var("SHELL").ok().filter(|s| !s.is_empty()) {
        found.push(vec![shell]);
    }
    found
}

/// One line of the launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// An agent cctop left running in tmux, from this run or an earlier one.
    Waiting(crate::tmux::Running),
    /// A command to start fresh.
    Start(Vec<String>),
}

impl Choice {
    pub fn label(&self) -> String {
        match self {
            // The tmux name is `cctop-<what>`; the prefix is true of every one
            // of them and so tells the reader nothing.
            Choice::Waiting(agent) => agent
                .name
                .strip_prefix("cctop-")
                .unwrap_or(&agent.name)
                .to_string(),
            Choice::Start(argv) => label_of(argv),
        }
    }

    /// Where picking this lands, when that is known and worth saying.
    ///
    /// A still-running agent brings its own: it has been working somewhere since
    /// before this launcher was opened, and `-c` cannot move it. Saying so is the
    /// only way to tell two `claude`s apart when both are just called claude.
    pub fn cwd(&self) -> Option<&Path> {
        match self {
            Choice::Waiting(agent) => agent.cwd.as_deref(),
            Choice::Start(_) => None,
        }
    }
}

/// What the launcher offers: the agents still running in tmux first, then the
/// commands that start a new one.
///
/// Agents outliving cctop is only half of the bargain — the other half is being
/// able to get back to them. Without this they survive somewhere unnameable,
/// reachable only by knowing to run `tmux attach` yourself, which is a worse
/// deal than the panes that simply died.
///
/// `open` is the tmux sessions already on screen in this cctop; they are left
/// out, since a second client onto one agent only makes the two panes argue
/// about the window size.
///
/// A session attached from *elsewhere* — a `tmux attach` in another terminal —
/// is still offered. It has the same problem, but hiding a running agent is the
/// worse of the two failures, so it is shown and labelled instead.
pub fn choices(open: &[String]) -> Vec<Choice> {
    crate::tmux::running()
        .into_iter()
        .filter(|agent| !open.contains(&agent.name))
        .map(Choice::Waiting)
        .chain(harnesses().into_iter().map(Choice::Start))
        .collect()
}

/// How a command picked from the launcher is named on screen: the command as
/// typed, minus any path, which matches what [`shim::host`](crate::shim::host)
/// calls it once it is running.
pub fn label_of(argv: &[String]) -> String {
    // `env VAR=value claude` is a `claude` tab. The prefix is how the agent was
    // started, which is plumbing, and naming a tab after its plumbing is the
    // same mistake as calling one `tmux new-session -A -s cctop-claude`.
    let mut argv = argv;
    if argv.first().map(String::as_str) == Some("env") {
        let mut rest = &argv[1..];
        while rest
            .first()
            .is_some_and(|a| a.contains('=') && !a.starts_with('-'))
        {
            rest = &rest[1..];
        }
        // Only when a command is left. `env` alone is a command in its own
        // right, and a tab with no name at all is worse than one named oddly.
        if !rest.is_empty() {
            argv = rest;
        }
    }
    argv.iter()
        .map(|arg| arg.rsplit('/').next().unwrap_or(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {

    /// A resumed tab is named after its session, not its command.
    ///
    /// `claude --resume 4ebf1ab4-2ef8-4fb2-a7d5-d445b5026dc9` is 45 characters
    /// of tab bar whose only variable part is a uuid nobody reads. The label
    /// the resume path builds is what the bar should show instead.
    #[test]
    fn a_resume_command_is_not_a_tab_name() {
        let argv: Vec<String> = ["claude", "--resume", "4ebf1ab4-2ef8-4fb2-a7d5-d445b5026dc9"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // What the command alone would give: the uuid, in full.
        let from_argv = super::label_of(&argv);
        assert!(from_argv.contains("4ebf1ab4"), "{from_argv}");
        assert!(from_argv.chars().count() > 40, "{from_argv}");

        // What the resume path builds instead: the agent, then the session.
        let label = format!(
            "{} · {}",
            argv[0],
            crate::util::truncate("Improve super cctop", super::super::TAB_LABEL_CHARS)
        );
        assert_eq!(label, "claude · Improve super cctop");
        assert!(!label.contains("4ebf1ab4"));
    }

    /// A tab launched under a profile is still a `claude` tab. The `env` prefix
    /// is how it was started, and naming a tab after its plumbing is the same
    /// mistake as calling one `tmux new-session -A -s cctop-claude`.
    #[test]
    fn a_profile_prefix_does_not_become_the_tab_name() {
        let argv: Vec<String> = ["env", "CLAUDE_CONFIG_DIR=/home/x/.claude-work", "claude"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(label_of(&argv), "claude");

        // Several, and a flag the agent owns, which must survive.
        let argv: Vec<String> = ["env", "A=1", "B=2", "claude", "--resume"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(label_of(&argv), "claude --resume");

        // Codex is selected by a different variable and must strip the same.
        let argv: Vec<String> = ["env", "CODEX_HOME=/home/x/.codex-work", "codex"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(label_of(&argv), "codex");

        // A command that merely happens to be called `env` keeps its name.
        assert_eq!(label_of(&["env".to_string()]), "env");
        assert_eq!(label_of(&["claude".to_string()]), "claude");
    }
    use super::*;

    /// The launcher must never offer something that isn't there — picking it
    /// would open a tab onto an immediate "command not found".
    #[test]
    fn the_launcher_only_offers_commands_that_exist() {
        for argv in harnesses() {
            assert!(
                crate::shim::is_command(&argv[0]),
                "offered a command that is not installed: {argv:?}"
            );
        }
    }

    /// The launcher puts still-running agents above the commands that start a
    /// new one, and never offers a second pane onto an agent already on screen.
    #[test]
    fn the_launcher_offers_what_is_running_before_what_is_new() {
        let starts = harnesses().len();
        let plain = choices(&[]);
        // Whatever tmux happens to be holding, the "start new" block is intact
        // and comes last.
        assert_eq!(
            plain
                .iter()
                .filter(|c| matches!(c, Choice::Start(_)))
                .count(),
            starts
        );
        let first_start = plain
            .iter()
            .position(|c| matches!(c, Choice::Start(_)))
            .unwrap_or(0);
        assert!(
            plain[first_start..]
                .iter()
                .all(|c| matches!(c, Choice::Start(_))),
            "a running agent appeared below the new-launch commands"
        );

        // An agent already on screen is not offered again.
        if let Some(Choice::Waiting(agent)) = plain.iter().find(|c| matches!(c, Choice::Waiting(_)))
        {
            let hidden = choices(std::slice::from_ref(&agent.name));
            assert!(!hidden.contains(&Choice::Waiting(agent.clone())));
        }
    }

    /// A session as tmux would describe it, with `ago` seconds since it last
    /// drew anything.
    fn session(name: &str, label: Option<&str>, ago: u64) -> crate::tmux::Running {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        crate::tmux::Running {
            name: name.to_string(),
            pid: Some(4321),
            cwd: None,
            attached: false,
            activity: Some(now.saturating_sub(ago)),
            label: label.map(str::to_string),
            profile: None,
        }
    }

    /// A pane whose agent rang, with everything else saying it is busy.
    fn ringing_tab(bell: &[u8]) -> Tab {
        let mut pane = Pane {
            hosted: None,
            tmux: None,
            resumed: None,
            profile: None,
            pid: 4321,
            agent: None,
            asked_at: None,
            label: "claude".into(),
            view: crate::attach::Attach::for_test(),
            drew_at: Instant::now(),
        };
        pane.view.parser.process(bell);
        Tab::new(pane)
    }

    /// The bell outranks every inference. A harness rings when it is blocked on
    /// you, and it keeps drawing its spinner and reporting itself as working
    /// while it waits — which is exactly the state the rest of `attention` reads
    /// as "leave it alone". Before the bell was kept, a tab blocked on a
    /// permission prompt was drawn as busy for as long as it sat there.
    ///
    /// What it does not outrank is the agent saying its turn is over. Claude
    /// Code rings the idle nudge a minute after `Stop`, down the same bell, and
    /// a finished turn drawn as a held question is the false alarm that teaches
    /// people to ignore the amber.
    #[test]
    fn an_agent_that_rang_outranks_looking_busy() {
        let ringing = ringing_tab(b"\x07");
        assert_eq!(
            ringing.attention(false, &|_| Some(crate::hook::Signal::Busy)),
            Some(Attention::NeedsInput),
        );
        assert_eq!(
            ringing.attention(false, &|_| Some(crate::hook::Signal::Idle)),
            Some(Attention::Idle),
            "the idle nudge was read as a question"
        );
        // A tool call held over a rung bell is the permission prompt itself.
        assert_eq!(
            ringing.attention(false, &|_| Some(crate::hook::Signal::Acting)),
            Some(Attention::NeedsInput),
        );

        // The control: the same freshly drawn pane, silent, is left alone.
        let quiet = ringing_tab(b"thinking");
        assert_eq!(
            quiet.attention(false, &|_| Some(crate::hook::Signal::Busy)),
            None,
        );

        // And the pane you are looking at is never the one blinking at you.
        assert_eq!(
            ringing.attention(true, &|_| Some(crate::hook::Signal::Busy)),
            None,
        );
    }

    /// The point of writing the label onto the session: two cctops showing the
    /// same agent must call it the same thing. The one that started it knows the
    /// conversation's title; every other one has only the session name, which
    /// for a resume is a sanitised uuid.
    #[test]
    fn a_shared_tab_is_called_what_the_cctop_that_started_it_called_it() {
        let tab = Tab::shared(&session(
            "cctop-claude-4ebf1ab4-2ef8-4fb2-a7d5-d445b5026dc9",
            Some("claude · Improve super cctop"),
            0,
        ));
        assert!(tab.detached());
        assert_eq!(tab.title(), "claude · Improve super cctop");

        // Nothing recorded — an agent left by a cctop older than this. The
        // session name stands in, minus the prefix every one of them carries.
        let tab = Tab::shared(&session("cctop-claude-32cca860", None, 0));
        assert_eq!(tab.title(), "claude-32cca860");
    }

    /// Switching tabs must not silently move an agent to another account.
    ///
    /// `go_to_tab` trades the pane you leave for a `Shared` and trades it back
    /// when you return, and a tab this cctop never launched is built from the
    /// session alone. The account used to survive neither, and a lost account is
    /// not "unknown" on the border — it reads as the default one, so a pane
    /// running as a second login was shown the first login's remaining budget,
    /// which is the figure you decide by.
    #[test]
    fn the_account_a_pane_runs_as_survives_a_tab_switch() {
        let mut running = session("cctop-claude-32cca860", Some("claude"), 0);
        running.profile = Some("work".into());
        let adopted = Tab::shared(&running);
        assert_eq!(
            adopted.shared.as_ref().and_then(|s| s.profile.as_deref()),
            Some("work"),
        );

        // And nothing invented for the ordinary account, which writes no option
        // and so reads back as none.
        let plain = Tab::shared(&session("cctop-claude-4e2b1c90", Some("claude"), 0));
        assert_eq!(plain.shared.as_ref().and_then(|s| s.profile.clone()), None);
    }

    /// A tab this cctop holds no client on still has to blink: that is the whole
    /// point of showing another cctop's agents. It has no screen to read, so the
    /// hooks answer, and tmux's own record of the session answers when they
    /// cannot.
    #[test]
    fn an_unwatched_tab_still_says_when_its_agent_wants_you() {
        let quiet = Tab::shared(&session("cctop-claude-a", None, 30));
        let busy = Tab::shared(&session("cctop-claude-b", None, 0));
        let pid = quiet.shared.as_ref().and_then(|s| s.pid).expect("pid");

        // A held question outranks everything, and being reported as working
        // outranks a session that merely looks quiet.
        assert_eq!(
            quiet.attention(false, &|_| Some(crate::hook::Signal::NeedsInput)),
            Some(Attention::NeedsInput)
        );
        assert_eq!(
            quiet.attention(false, &|_| Some(crate::hook::Signal::Busy)),
            None
        );
        // Asked about the agent's pid, which is the only one its hooks mention.
        assert_eq!(
            quiet.attention(false, &|asked| (asked == pid)
                .then_some(crate::hook::Signal::NeedsInput)),
            Some(Attention::NeedsInput)
        );

        // A tool call in flight is working *and* a question, depending on
        // whether the terminal is still: that is the shape of a permission
        // prompt, which nothing else reports in time to blink about.
        assert_eq!(
            quiet.attention(false, &|_| Some(crate::hook::Signal::Acting)),
            Some(Attention::NeedsInput)
        );
        assert_eq!(
            busy.attention(false, &|_| Some(crate::hook::Signal::Acting)),
            None,
            "an agent still repainting mid-tool is working, not asking"
        );

        // No hooks: tmux's last-output time is the fallback the missing screen
        // would have provided.
        let unreported = |_: u32| None;
        assert_eq!(quiet.attention(false, &unreported), Some(Attention::Idle));
        assert_eq!(busy.attention(false, &unreported), None);
    }

    /// A detached tab is not an empty one. `reap` reporting it as finished would
    /// have every cctop drop the tabs it is not looking at, one tick after it
    /// stopped looking.
    #[test]
    fn a_detached_tab_is_not_reaped() {
        let mut tab = Tab::shared(&session("cctop-claude-a", None, 0));
        assert!(!tab.reap(), "a shared tab was reaped for having no pane");
        // Once it stands for nothing, it is nothing.
        tab.shared = None;
        assert!(tab.reap());
    }

    /// A still-running agent in the launcher, as tmux would have described it.
    fn waiting(name: &str) -> Choice {
        Choice::Waiting(crate::tmux::Running {
            name: name.to_string(),
            pid: Some(4321),
            cwd: Some(std::path::PathBuf::from("/home/x/proj")),
            attached: false,
            activity: None,
            label: None,
            profile: None,
        })
    }

    /// The tab is named after the agent, not the plumbing that carries it.
    #[test]
    fn a_choice_is_named_for_the_agent_not_the_wrapper() {
        assert_eq!(waiting("cctop-claude-32cca860").label(), "claude-32cca860");
        assert_eq!(
            Choice::Start(vec!["/usr/bin/claude".into()]).label(),
            "claude"
        );
    }

    /// A running agent brings the directory it has been working in; a fresh one
    /// has none of its own, and takes whatever the launcher was opened on.
    #[test]
    fn only_a_running_agent_names_its_own_directory() {
        assert_eq!(
            waiting("cctop-claude-abc").cwd(),
            Some(Path::new("/home/x/proj"))
        );
        assert_eq!(Choice::Start(vec!["claude".into()]).cwd(), None);
    }

    /// The thing tmux breaks if nobody accounts for it: the pid cctop hosts is
    /// the tmux client, so everything the agent reports about itself is filed
    /// under a pid nothing on this side would ever ask about. A tab that asked
    /// the wrong one would fall back to reading the screen for exactly the panes
    /// where the agent is talking.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_tmux_backed_tab_asks_about_the_agent_and_not_the_client() {
        // A pane standing in for a tmux client: what cctop hosts is one pid, and
        // the agent behind it is another.
        let (_child, client_pid) = crate::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let mut pane = Pane::view_of(client_pid, "claude".into()).expect("attach");
        pane.tmux = Some("cctop-claude-abc".into());
        let agent_pid = client_pid + 1_000;
        pane.agent = Some(agent_pid);
        assert_eq!(pane.agent(), agent_pid);
        assert!(pane.outlives_cctop());

        let mut tab = Tab::new(pane);
        // Focus is elsewhere, so the pane is allowed to ask for attention.
        tab.focus = 1;

        // The agent says it is blocked on a question. It arrives under the
        // agent's pid, which is the only pid its hooks ever mention.
        assert_eq!(
            tab.attention(true, &|pid| (pid == agent_pid)
                .then_some(crate::hook::Signal::NeedsInput)),
            Some(Attention::NeedsInput)
        );
        // The client's pid says nothing about the agent, and must not be taken
        // for it — this is the assertion the fix exists for.
        assert_ne!(
            tab.attention(true, &|pid| (pid == client_pid)
                .then_some(crate::hook::Signal::NeedsInput)),
            Some(Attention::NeedsInput)
        );

        // Reported as working: the screen is irrelevant, however long the client
        // has sat still.
        assert_eq!(
            tab.attention(true, &|pid| (pid == agent_pid)
                .then_some(crate::hook::Signal::Busy)),
            None
        );
    }

    /// A pane whose agent is still drawing must not claim to be idle, a held
    /// question must outrank a finished turn, and the tab you are looking at
    /// must stay quiet — blinking the title of the pane in front of you is
    /// noise, and it is the case that fires most often.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_tab_asks_for_attention_only_when_it_has_something_you_cannot_see() {
        let mut kids = Vec::new();
        let mut pane = |script: &str| {
            let (child, pid) = crate::shim::test_session(&["sh", "-c", script], (80, 24));
            kids.push(child);
            (pid, Pane::view_of(pid, "agent".into()).expect("attach"))
        };
        // One agent that keeps painting, one that drew once and went quiet.
        let (busy_pid, busy) = pane("while :; do printf '.'; sleep 0.2; done");
        let (quiet_pid, quiet) = pane("printf 'done'; sleep 30");

        let mut tab = Tab::new(busy);
        tab.panes.push(quiet);
        let unreported = |_: u32| None;

        // Both panes have just been created, so neither has been quiet yet.
        assert_eq!(tab.attention(false, &unreported), None);

        // Past the threshold, only the one that stopped drawing is idle.
        let deadline = Instant::now() + QUIET_IS_IDLE + Duration::from_secs(2);
        while Instant::now() < deadline {
            tab.pump();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(tab.attention(false, &unreported), Some(Attention::Idle));

        // The busy pane holding a question outranks the quiet one being idle.
        assert_eq!(
            tab.attention(false, &|pid| (pid == busy_pid)
                .then_some(crate::hook::Signal::NeedsInput)),
            Some(Attention::NeedsInput)
        );
        // Focused tab: the focused pane is excluded, so the quiet one is left.
        tab.focus = 0;
        assert_eq!(
            tab.attention(true, &|pid| (pid == busy_pid)
                .then_some(crate::hook::Signal::NeedsInput)),
            Some(Attention::Idle)
        );
        // Focus the quiet one instead: it is the only pane with anything to
        // report, and you are looking straight at it, so the tab stays quiet.
        tab.focus = 1;
        assert_eq!(tab.attention(true, &unreported), None);

        drop(tab);
        for child in &mut kids {
            let _ = child.kill();
            let _ = child.wait();
        }
        for pid in [busy_pid, quiet_pid] {
            let _ = crate::shim::socket_path(pid).map(std::fs::remove_file);
        }
    }

    /// The trade that keeps several cctops off one tmux window: the tab you
    /// leave gives up its client and keeps everything needed to take one back.
    /// Only a lone tmux-backed pane may do it — a pty cctop owns would be
    /// *ended* by this, and a split cannot be rebuilt from one session name.
    #[cfg(target_os = "linux")]
    #[test]
    fn leaving_a_tmux_tab_gives_up_its_client_and_nothing_else() {
        let (mut child, pid) = crate::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let mut pane = Pane::view_of(pid, "claude · Improve super cctop".into()).expect("attach");
        pane.tmux = Some("cctop-claude-abc".into());
        let agent_pid = pid + 1_000;
        pane.agent = Some(agent_pid);

        let mut tab = Tab::new(pane);
        assert!(tab.detach());
        assert!(tab.detached());
        let shared = tab.shared.clone().expect("nothing was kept");
        assert_eq!(shared.name, "cctop-claude-abc");
        // The label survives, so the tab does not rename itself on the way out —
        // and the agent's pid does, so it can still blink.
        assert_eq!(shared.label, "claude · Improve super cctop");
        assert_eq!(shared.pid, Some(agent_pid));
        assert_eq!(tab.title(), "claude · Improve super cctop");

        // A pane with no tmux behind it: dropping it is the kill, so it stays.
        let mut owned = Tab::new(Pane::view_of(pid, "claude".into()).expect("attach"));
        assert!(!owned.detach());
        assert!(owned.shared.is_none());

        // A split has two sessions and one tab; there is nothing to attach back.
        let mut split = Tab::new(Pane::view_of(pid, "claude".into()).expect("attach"));
        split.panes[0].tmux = Some("cctop-claude-abc".into());
        split
            .panes
            .push(Pane::view_of(pid, "shell".into()).expect("attach"));
        split.panes[1].tmux = Some("cctop-zsh".into());
        assert!(!split.detach());
        assert_eq!(split.panes.len(), 2);

        let _ = child.kill();
        let _ = child.wait();
        let _ = crate::shim::socket_path(pid).map(std::fs::remove_file);
    }

    #[test]
    fn a_command_is_labelled_by_its_name_not_its_path() {
        assert_eq!(label_of(&["/usr/bin/claude".into()]), "claude");
        assert_eq!(
            label_of(&["codex".into(), "--full-auto".into()]),
            "codex --full-auto"
        );
    }
}
