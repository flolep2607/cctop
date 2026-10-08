//! Workspace tabs: the dashboard, plus a terminal for every agent you open.
//!
//! Nothing here is new machinery. [`shim::host`](cctop_core::shim::host) already puts
//! an agent on a pty cctop owns, and [`attach`](cctop_core::attach) already turns that
//! pty into a screen that can be drawn into any rectangle and resized to it. A
//! tab is a list of those screens; a split is that list drawn side by side
//! instead of one at a time.
//!
//! The dashboard is not in this list — it is tab zero and always there, so
//! [`App::tab`](super::App::tab) is `0` for the session table and `1..=len` for
//! these.

use std::path::Path;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::theme::Hue;

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

/// How often a rmux-backed pane re-asks rmux which process the agent is, until
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
    /// [`Attention::Idle`], on a turn this cctop watched end while you were
    /// not looking at it — news, where plain idle is only context. See
    /// [`seen`](super::seen).
    Done,
    /// The agent has explicitly asked something and is blocked on the answer.
    NeedsInput,
}

/// One terminal on screen.
pub struct Pane {
    /// What this pane started. Dropping it kills that process, which is why a
    /// pane merely *looking at* someone else's session leaves this `None`: `a`
    /// on a session row must not make cctop responsible for its life.
    ///
    /// For a rmux-backed pane the process here is the rmux *client*, not the
    /// agent — see [`rmux`](cctop_core::rmux).
    hosted: Option<cctop_core::shim::Hosted>,
    /// The rmux session the agent is really in, when there is one.
    ///
    /// Its presence is what separates closing a pane from ending an agent: with
    /// it, the two are different acts and only the second needs asking about.
    pub rmux: Option<String>,
    /// The `list-panes` asking which process the agent is, if it has not
    /// answered. Off the draw loop — see [`Pane::find_agent`].
    asked: Option<std::sync::mpsc::Receiver<Option<u32>>>,
    /// The session this pane was opened to resume, named as
    /// [`rmux::name_for_session`](cctop_core::rmux::name_for_session) names it.
    ///
    /// Recorded whether or not rmux is what carries the agent, because it is the
    /// only durable answer to "is this session already open?" — `rmux` alone is
    /// `None` on every pane when rmux is not installed, and resuming one
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
    /// When rmux was last asked who the agent is, so an unanswered question is
    /// retried without being retried every frame.
    asked_at: Option<Instant>,
    pub label: String,
    /// Whether an agent is what this pane started.
    ///
    /// [`harnesses`] offers the login shell alongside the agents, and half of
    /// what a second tab is for is `git diff` — so a pane is not necessarily an
    /// agent, and everything `attention` infers from a still screen assumes it
    /// is one. A shell sitting at its prompt has not *finished a turn*; it has
    /// no turns. Without this it goes green two seconds after you stop typing,
    /// which reads as an agent waiting for you in a tab where there is none.
    is_agent: bool,
    pub view: cctop_core::attach::Attach,
    /// When this pane's screen last changed, which is how idleness is told
    /// without asking the agent or its transcript anything.
    drew_at: Instant,
    /// What the footer rules said the last time this screen was laid out, and
    /// where on the screen that was: the last draw, the width, and how far back
    /// it was scrolled. See [`Pane::read_screen`].
    ///
    /// `read_screen` runs per pane per tick — sixty times a second inside a tab —
    /// and laying a vt100 screen out as a `Vec<String>`, lowercasing the footer
    /// rows and joining them is the whole cost of it. A screen in the same place
    /// cannot have a different answer, so the reading is kept and the layout only
    /// redone when the pane has actually moved.
    read: Option<(ScreenPlace, ScreenReading)>,
    /// Whether this pane's rmux client holds the window; see [`Fit`].
    pub fit: Fit,
}

/// How long after a pane's last keystroke the next one counts as coming back
/// to it — the moment worth checking whether another client took the window.
const FIT_QUIET: std::time::Duration = std::time::Duration::from_millis(1500);

/// How long the nudge holds the client one column narrower. Long enough that
/// rmux sees two resizes rather than coalescing them into none.
const FIT_NUDGE: std::time::Duration = std::time::Duration::from_millis(150);

/// How long after a nudge to look whether rmux took it.
const FIT_VERIFY: std::time::Duration = std::time::Duration::from_millis(1000);

/// Taking a rmux window back for this pane.
///
/// A session can have two clients at once — this pane, and a browser opened on
/// the same agent — and rmux fits the window to one of them. `window-size
/// latest` is meant to mean the one last used, but rmux only moves it when a
/// client attaches or resizes, never on input: type into the pane while the
/// browser holds the window and the agent stays drawn at the browser's size.
/// (tmux follows input; this is rmux's gap, checked against a real server.)
///
/// So the pane does what rmux would have: on the first keystroke after a pause
/// it asks rmux how big the window is, and if that is not this pane's size it
/// resizes its own client one column narrower and back. rmux takes the resize
/// as this client being the latest and fits the window to it. Only on a real
/// mismatch — a nudge on every keystroke would make the agent redraw on each.
#[derive(Default)]
pub struct Fit {
    /// The window's size as rmux last reported it, from the tab sweep or from
    /// the check a keystroke started.
    pub window: Option<(u16, u16)>,
    nudged_at: Option<Instant>,
    typed_at: Option<Instant>,
    asking: Option<std::sync::mpsc::Receiver<Option<(u16, u16)>>>,
    /// When to look whether the last nudge took, and whether the check in
    /// flight is that look.
    verify_at: Option<Instant>,
    verifying: bool,
    /// The size this pane was at when a nudge did not take. While it still is,
    /// no nudge is tried again: something other than "which client was used
    /// last" is holding the window, and nudging on every keystroke would only
    /// make the agent reflow on every keystroke.
    gave_up: Option<(u16, u16)>,
}

impl Fit {
    /// Whether rmux's window already is this pane's `(cols, rows)`. rmux's
    /// status line, where a session has one, takes a row of the client.
    fn matches(window: (u16, u16), cols: u16, rows: u16) -> bool {
        window.0 == cols && (window.1 == rows || window.1 + 1 == rows)
    }
}

/// Where on a screen a reading was taken: the last time it was drawn to, how
/// wide it is, and how far back it is scrolled.
///
/// The last of those three is here because the rows a read sees are the visible
/// window rather than the whole scrollback — so scrolling is a change to what
/// the screen says even though the agent drew nothing.
type ScreenPlace = (Instant, u16, usize);

/// What the footer rules made of a screen: a signal if one of them matched,
/// and the question if the screen was asking one.
///
/// `None` for the signal is not an answer, it is an absence — the still-screen
/// fallback in [`Pane::read_screen`] is what turns it into one, and that is
/// decided by a clock rather than by the pixels.
type ScreenReading = (Option<cctop_core::hook::Signal>, Option<String>, bool);

impl Pane {
    /// A pane labelled `label` with no process behind it, for tests elsewhere
    /// that draw one: the fields that own a process are private to this module.
    #[cfg(test)]
    pub(crate) fn for_test(label: &str) -> Pane {
        Pane {
            hosted: None,
            rmux: None,
            resumed: None,
            profile: None,
            pid: 4321,
            agent: None,
            asked_at: None,
            asked: None,
            label: label.into(),
            is_agent: true,
            view: cctop_core::attach::Attach::for_test(),
            drew_at: Instant::now(),
            read: None,
            fit: Fit::default(),
        }
    }

    /// Someone typed, clicked or scrolled in this pane. On the first such
    /// after a pause, ask rmux whether this pane's client still holds the
    /// window — off the draw loop, since it is a subprocess. `force` is for
    /// arriving at the pane, which counts whatever the clock says.
    pub fn note_input(&mut self, force: bool) {
        let back = force || self.fit.typed_at.is_none_or(|t| t.elapsed() >= FIT_QUIET);
        self.fit.typed_at = Some(Instant::now());
        if !back || self.fitting() || self.fit.gave_up == Some(self.view.size) {
            return;
        }
        self.ask_window(false);
    }

    /// Ask rmux for the window's size off the draw loop; `verifying` marks the
    /// look that follows a nudge.
    fn ask_window(&mut self, verifying: bool) {
        let Some(name) = self.rmux.clone() else {
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(cctop_core::rmux::window_size(&name));
        });
        self.fit.asking = Some(rx);
        self.fit.verifying = verifying;
    }

    /// The size to ask this pane's client to be, given the `(cols, rows)` it
    /// is drawn in — which is that size, except for the moment of a nudge.
    pub fn fit_request(&mut self, cols: u16, rows: u16) -> (u16, u16) {
        // Compared against what this client was granted, not what the pane
        // asked for: another watcher can hold the client smaller, and then the
        // window rightly matches the grant — measuring it against the request
        // would read as stolen on every check and nudge forever.
        let (granted_cols, granted_rows) = self.view.size;
        if self.fit.gave_up.is_some_and(|size| size != self.view.size) {
            self.fit.gave_up = None;
        }
        if let Some(rx) = &self.fit.asking {
            match rx.try_recv() {
                Ok(window) => {
                    self.fit.asking = None;
                    let verifying = std::mem::take(&mut self.fit.verifying);
                    if let Some(window) = window {
                        self.fit.window = Some(window);
                        let held = Fit::matches(window, granted_cols, granted_rows);
                        if verifying {
                            if !held {
                                self.fit.gave_up = Some(self.view.size);
                            }
                        } else if !held && cols > 1 && self.fit.gave_up.is_none() {
                            self.fit.nudged_at = Some(Instant::now());
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.fit.asking = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(at) = self.fit.nudged_at {
            if at.elapsed() < FIT_NUDGE {
                return (cols - 1, rows);
            }
            self.fit.nudged_at = None;
            self.fit.verify_at = Some(Instant::now() + FIT_VERIFY);
        }
        if self.fit.verify_at.is_some_and(|at| Instant::now() >= at) && self.fit.asking.is_none() {
            self.fit.verify_at = None;
            self.ask_window(true);
        }
        (cols, rows)
    }

    /// Whether a check, a nudge or the look after one is under way, which
    /// needs frames to finish.
    pub fn fitting(&self) -> bool {
        self.fit.asking.is_some() || self.fit.nudged_at.is_some() || self.fit.verify_at.is_some()
    }

    /// What this pane's agent says it is doing, read off its screen.
    ///
    /// Only for a harness with a row in [`cctop_core::screen::FOOTERS`]: that row is what makes a
    /// pane an agent worth reading, where `is_agent` only knows the harnesses
    /// cctop aliases, and `Esc to cancel` in a shell is nobody's question.
    ///
    /// A screen none of its rules match is idle once it has gone still — the
    /// same inference the tab bar makes from silence, and sound for the same
    /// reason: every one of these harnesses ticks a timer or a spinner while it
    /// works, so a still screen with no working hint on it is a turn that ended.
    /// Still, and not merely unmatched: see [`cctop_core::screen::FOOTERS`] for the frame mid-turn
    /// that has neither.
    pub fn read_screen(&mut self) -> Option<cctop_core::peek::Screened> {
        if !cctop_core::screen::screenable(self.harness()) {
            return None;
        }
        // Only a screen that has changed since the last reading can have a
        // different answer, so an unchanged one is answered from what was
        // already read. This is the per-tick path: `runloop` asks every pane
        // every tick, sixty times a second inside a tab, and laying the screen
        // out again for the same pixels is what made it expensive.
        //
        // "Changed" is the last draw, the width the rows are cut to, and how far
        // back the screen is scrolled: the rows a read sees are the visible
        // window, and a resize or a scrollback move changes that window without
        // the agent having drawn anything.
        let screen = self.view.parser.screen();
        let (_, cols) = screen.size();
        let at = (self.drew_at, cols, screen.scrollback());
        let read = match self.read.as_ref() {
            Some((where_, found)) if *where_ == at => found.clone(),
            _ => {
                let rows: Vec<String> = screen.rows(0, cols).collect();
                let signal = cctop_core::screen::screen_state(self.harness(), &rows);
                let asking = signal == Some(cctop_core::hook::Signal::NeedsInput);
                let ask = asking
                    .then(|| cctop_core::screen::screen_ask(self.harness(), &rows))
                    .flatten();
                let question = asking && cctop_core::screen::screen_question(self.harness(), &rows);
                let found = (signal, ask, question);
                self.read = Some((at, found.clone()));
                found
            }
        };
        let (signal, ask, question) = read;
        let signal = signal.or_else(|| self.idle().then_some(cctop_core::hook::Signal::Idle))?;
        Some(cctop_core::peek::Screened {
            signal,
            ask,
            question,
        })
    }

    /// Whether the agent has gone quiet long enough to count as waiting for you.
    fn idle(&self) -> bool {
        self.drew_at.elapsed() >= QUIET_IS_IDLE
    }

    /// Drop the cached reading, so the next [`Self::read_screen`] lays the
    /// screen out again.
    ///
    /// For the one thing that changes what a reading means without the screen
    /// changing: the harness a pane with no rmux session is named for, which a
    /// rename moves.
    fn forget_screen(&mut self) {
        self.read = None;
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
    /// The same thing as [`Pane::pid`] everywhere except under rmux, where that
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
    /// It cannot be settled at launch. cctop spawns a rmux *client* and returns;
    /// the server creating the session and spawning the agent inside it happens
    /// on its own time, so for the first moments of a pane's life there is no
    /// pane to ask about. Once found it is kept — a rmux pane's command outlives
    /// every client that ever looks at it, so the answer cannot go stale while
    /// this pane is alive to hold it.
    fn find_agent(&mut self) {
        if self.agent.is_some() || self.rmux.is_none() {
            return;
        }
        // An answer already on its way: take it, or wait for it. This is the
        // whole of the cost — the question is a `list-panes` subprocess, and it
        // used to be run on the thread that draws, from a path every pane takes
        // every tick, so a slow rmux stalled the frame rather than this pane's
        // own pid. Nothing is asked here that was not asked before.
        if let Some(rx) = &mut self.asked {
            match rx.try_recv() {
                Ok(found) => {
                    self.asked = None;
                    let name = self.rmux.clone().expect("checked above");
                    self.settle_agent(&name, found);
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                // The thread died without answering; fall through and ask again
                // on the interval rather than never.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.asked = None,
            }
        }
        if self
            .asked_at
            .is_some_and(|at| at.elapsed() < FIND_AGENT_EVERY)
        {
            return;
        }
        self.asked_at = Some(Instant::now());
        let name = self.rmux.clone().expect("checked above");
        let (tx, rx) = std::sync::mpsc::channel();
        self.asked = Some(rx);
        std::thread::spawn(move || {
            // The receiver is gone if the pane closed while rmux was thinking;
            // the answer is then simply not wanted any more.
            let _ = tx.send(cctop_core::rmux::agent_pid(&name));
        });
    }

    /// Record what rmux said the agent is, and settle the session now that it
    /// is known.
    fn settle_agent(&mut self, name: &str, found: Option<u32>) {
        self.agent = found;
        // The first moment the session is known to be there is the first moment
        // its options can be set, and settling it here means it is done once per
        // pane rather than on a timer. Every attach passes through, so a session
        // left by an older cctop is quieted when it is picked up again.
        if self.agent.is_some() {
            cctop_core::rmux::quiet(name);
            cctop_core::rmux::mouse(name);
            // The one moment the session exists and this pane's label is settled
            // — the callers that rename a pane do it before it is ever pumped.
            // Every other cctop reads the tab's name back off the session, so
            // without this a resumed agent would be one thing here and a uuid
            // next door.
            cctop_core::rmux::set_label(name, &self.label);
            // Only when there is one to record: an unset option reads back as
            // "the default account", which is exactly what `None` means here.
            if let Some(profile) = &self.profile {
                cctop_core::rmux::set_profile(name, profile);
            }
        }
    }

    /// Whether the agent survives this pane going away.
    pub fn outlives_cctop(&self) -> bool {
        self.rmux.is_some()
    }

    /// Whether ending this pane's agent is cctop's to do.
    ///
    /// False for a pane opened with `a`, which is a window onto an agent started
    /// somewhere else: there is no pty to drop and no rmux session to kill, so
    /// the pane can be closed but the agent cannot be reached. Asking first is
    /// the difference between a key that declines and a key that appears to work
    /// and does nothing.
    pub fn owns_agent(&self) -> bool {
        self.hosted.is_some() || self.rmux.is_some()
    }

    /// The harness running in this pane, as one word — `"claude"`, `"zsh"`.
    ///
    /// Read off the rmux session name where there is one: it is spelled
    /// `cctop-<harness>-…` at creation and never rewritten, so it still says
    /// "claude" after the tab has been renamed to anything at all. A pane with
    /// no session — one cctop hosts itself, or a window opened with `a` —
    /// falls back to the first word of its label, which a rename does clobber.
    ///
    /// ponytail: a renamed pane on cctop's own pty answers with its new name.
    pub fn harness(&self) -> &str {
        cctop_core::screen::harness_of(self.rmux.as_deref().unwrap_or(self.label.as_str()))
    }

    /// Whether an agent is what this pane started, rather than a shell or an
    /// editor — see the field of the same name.
    pub fn is_agent(&self) -> bool {
        self.is_agent
    }

    /// The key to actually send this pane's agent, once pane-specific
    /// translation is done.
    ///
    /// There is exactly one translation: Claude Code puts the top and bottom
    /// of the chat on Ctrl+Home and Ctrl+End — the right action, a chord too
    /// many for keys already named after where they go. In one of its panes
    /// the bare keys are promoted to the shifted forms on the way in; anywhere
    /// else they keep their line-editing meaning, which a shell reaches for
    /// constantly.
    pub fn translate_key(&self, key: KeyEvent) -> KeyEvent {
        match key.code {
            KeyCode::Home | KeyCode::End
                if key.modifiers.is_empty() && self.harness() == "claude" =>
            {
                KeyEvent::new(key.code, KeyModifiers::CONTROL)
            }
            _ => key,
        }
    }
}

/// Who owns the agent a pane is opened onto.
#[derive(Debug, Clone)]
pub enum Own {
    /// A rmux session of this name, so the agent outlives cctop. An existing
    /// session of that name is attached to rather than replaced.
    Tmux(String),
    /// A rmux session that is already running: attach, never create. Picking one
    /// from the launcher that has since ended must fail and say so, not quietly
    /// start something new under its name.
    TmuxExisting(String),
    /// A pty cctop owns, which ends when cctop does.
    Cctop,
}

impl Pane {
    /// Start `argv` and open a pane onto it.
    pub fn launch(argv: &[String], cwd: Option<&Path>, own: Own) -> anyhow::Result<Pane> {
        let rmux = match &own {
            Own::Tmux(name) | Own::TmuxExisting(name) => Some(name.clone()),
            Own::Cctop => None,
        };
        let spawn = match &own {
            Own::Tmux(name) => {
                // Before the client, not after: the pane's scrollback is fixed
                // the moment it is made. See [`rmux::prepare`].
                cctop_core::rmux::prepare(argv, name, cwd);
                cctop_core::rmux::attach_or_create(argv, name, cwd)
            }
            Own::TmuxExisting(name) => cctop_core::rmux::attach(name),
            Own::Cctop => argv.to_vec(),
        };
        let hosted = cctop_core::shim::host(&spawn, cwd, super::render::pane_size())?;
        // The shim binds and serves the socket before returning, so there is
        // something to connect to even though the agent has drawn nothing yet.
        let mut view = cctop_core::attach::attach(hosted.pid).ok_or_else(|| {
            anyhow::anyhow!(
                "{} started but its terminal could not be opened",
                label_of(argv)
            )
        })?;
        // With rmux in the middle the agent's keyboard-protocol request never
        // reaches this end; see [`attach::Attach::assume_extended_keys`].
        if rmux.is_some() {
            view.assume_extended_keys();
        }
        Ok(Pane {
            pid: hosted.pid,
            // The agent's name, not the wrapper's: a tab reading `rmux
            // new-session -A -s cctop-claude-32cca860` names the plumbing.
            label: label_of(argv),
            is_agent: starts_an_agent(argv),
            view,
            rmux,
            // Filled in by the caller that knows: launching is not resuming, and
            // most launches are not any session in particular.
            resumed: None,
            profile: None,
            agent: None,
            asked_at: None,
            asked: None,
            hosted: Some(hosted),
            drew_at: Instant::now(),
            read: None,
            fit: Fit::default(),
        })
    }

    /// Open a pane onto an agent something else is responsible for.
    pub fn view_of(pid: u32, label: String) -> Option<Pane> {
        Some(Pane {
            hosted: None,
            rmux: None,
            resumed: None,
            // Nothing was launched here, so there is no choice to record.
            profile: None,
            pid,
            // Nothing stands between this pane and the agent: the pid asked for
            // is the agent's, which is what makes this the answer already.
            agent: Some(pid),
            asked_at: None,
            asked: None,
            label,
            // `a` on a session row is the only way here, and a session row is
            // an agent.
            is_agent: true,
            view: cctop_core::attach::attach(pid)?,
            drew_at: Instant::now(),
            read: None,
            fit: Fit::default(),
        })
    }

    /// End the agent behind this pane for good.
    ///
    /// Only rmux-backed panes need this. Everywhere else, dropping the pane
    /// already is the kill — which is the whole reason the two have to be told
    /// apart once rmux is in the picture.
    pub fn kill_agent(&self) -> Result<(), String> {
        match &self.rmux {
            Some(name) => cctop_core::rmux::kill(name),
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

/// A rmux session a tab stands for while this cctop holds no client on it.
///
/// Every cctop on this machine shows a tab for every cctop-owned rmux session,
/// including the ones another cctop started. Holding a client on all of them
/// would be the wrong way to do it: rmux would have several clients on one
/// window, and the size they argue their way to is nobody's. So an unwatched tab
/// keeps only this — enough to name it, to say when its agent wants you, and to
/// attach the moment you switch to it.
#[derive(Debug, Clone)]
pub struct Shared {
    /// The rmux session, which is the tab's identity across every cctop.
    pub name: String,
    /// What the cctop that started it called the tab. See
    /// [`rmux::set_label`](cctop_core::rmux::set_label).
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
    /// What the agent last reported about itself, as recorded on the rmux
    /// session — see [`rmux::State`](cctop_core::rmux::State).
    ///
    /// This is the agent's own word, and it outranks [`Shared::idle`] below,
    /// which is only ever an inference from a clock. It is also the only thing
    /// either of them can say about a session this cctop was not running for:
    /// the live report went to whoever was listening at the time, and that was
    /// nobody.
    pub state: Option<cctop_core::rmux::State>,
}

impl Shared {
    /// What a [`cctop_core::rmux::Running`] becomes when no client of ours is on it.
    pub fn of(agent: &cctop_core::rmux::Running) -> Shared {
        Shared {
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
            state: agent.state,
        }
    }

    /// Whether the session holds an agent rather than a shell.
    ///
    /// Read off the session name, the one thing about it that is never
    /// rewritten — the same place [`Pane::harness`] looks — because a tab with
    /// no pane has no argv left to ask [`starts_an_agent`] about.
    pub fn is_agent(&self) -> bool {
        let harness = cctop_core::screen::harness_of(&self.name);
        cctop_core::alias::AGENTS
            .split_whitespace()
            .any(|agent| agent == harness)
    }

    /// What the rmux session records about its agent, if it is still true.
    ///
    /// Aged out by the same asymmetric rule a live report is — see
    /// [`Signal::is_current_after`](cctop_core::hook::Signal::is_current_after) — so
    /// a session killed mid-turn stops claiming to be working, while one that
    /// has been asking since yesterday still is.
    fn recorded(&self) -> Option<cctop_core::hook::Signal> {
        let state = self.state?;
        state
            .is_current(cctop_core::rmux::now_secs())
            .then_some(state.signal)
    }

    /// Whether the agent has gone quiet long enough to count as waiting for you.
    ///
    /// The same judgement [`Pane::idle`] makes, from rmux's record of the
    /// session rather than from a screen — an unwatched tab has no screen. A
    /// second of slack on top of [`QUIET_IS_IDLE`], because rmux reports this to
    /// the second and the sweep that read it is already up to
    /// [`SHARE_EVERY`](super::panes::SHARE_EVERY) old: without it a busy agent flickers
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
    /// The *rest* of a split's sessions, for a tab holding no client on any of
    /// them.
    ///
    /// `shared` is the leading pane and stays that way; this is the other panes,
    /// in order. Both are set only while `panes` is empty, and both are traded
    /// for panes together by [`Tab::attach`].
    ///
    /// A split used to have no form here at all: [`Tab::detach`] declined one, so
    /// a tab of two sessions could not be put down and picked up, and came back
    /// from a restart as two tabs that had never met. Carrying the rest is what
    /// lets one tab hold them, and it is why a tab's identity is now recorded on
    /// rmux rather than implied by what happens to be attached.
    pub extra: Vec<Shared>,
    /// The colour the tab was painted, if any.
    ///
    /// Kept on the tab rather than on a pane or a `Shared`, because it is the
    /// one place that survives the trade between them: a pane is swapped for a
    /// `Shared` on every switch away, and a property either side held would
    /// have to be handed across both ways.
    pub color: Option<Hue>,
    /// When this tab's agent was last restarted, for the sweep across its
    /// label that says so. On the tab for the reason `color` is: a restart
    /// swaps the pane or the `Shared` underneath, and the one thing still
    /// standing afterwards is the tab.
    pub restarted: Option<Instant>,
    /// Whether the focused pane is drawn over the whole tab — `Alt+z`.
    ///
    /// A mode of the tab rather than a mark on one pane, so it follows the
    /// keyboard: `Alt+o` while zoomed shows the next pane zoomed, the way
    /// flipping through full-screen terminals should feel, and `Alt+b`'s walk
    /// through a split lands on the pane that asked with it on screen. See
    /// [`Tab::zoomed`] for why it is read through a method.
    zoom: bool,
}

impl Tab {
    pub fn new(pane: Pane) -> Tab {
        Tab {
            panes: vec![pane],
            focus: 0,
            stacked: false,
            shared: None,
            extra: Vec::new(),
            color: None,
            restarted: None,
            zoom: false,
        }
    }

    /// A tab for a rmux session this cctop has not attached to — one another
    /// cctop started, or one this cctop left when you switched away.
    ///
    /// A tab of one, which is every session nobody has split. It takes its axis
    /// off the session rather than assuming side by side, so a session carrying
    /// `@cctop_axis` from a split it has since been pulled out of comes back the
    /// way it was left.
    /// A tab for a rmux session this cctop has not attached to — one another
    /// cctop started, or one this cctop left when you switched away.
    ///
    /// A tab of one, which is every session nobody has split. Its axis is read
    /// off the session rather than assumed, so a session still carrying
    /// `@cctop_axis` from a split it has since been pulled out of comes back the
    /// way it was left.
    #[cfg(test)]
    pub fn for_agent(agent: &cctop_core::rmux::Running) -> Tab {
        Tab::split_of(agent, &[], agent.axis())
    }

    /// A tab standing for one leading session and however many more belong to
    /// the same tab beside it.
    ///
    /// `others` is the rest of the split in pane order, and is empty for every
    /// session that has never been split — which is the shape the whole codebase
    /// assumed before this existed, so nothing has to ask which case it is in.
    ///
    /// The axis is read off the sessions themselves rather than passed in, so
    /// that a tab comes back laid out the way it was left and no caller has to
    /// remember to ask. A word this cctop does not know reads as side by side,
    /// which is the default and not a claim.
    pub fn split_of(
        leader: &cctop_core::rmux::Running,
        others: &[&cctop_core::rmux::Running],
        axis: cctop_core::rmux::Axis,
    ) -> Tab {
        Tab {
            panes: Vec::new(),
            focus: 0,
            stacked: axis == cctop_core::rmux::Axis::Stacked,
            shared: Some(Shared::of(leader)),
            extra: others.iter().map(|agent| Shared::of(agent)).collect(),
            color: leader.color.as_deref().and_then(Hue::from_name),
            restarted: None,
            zoom: false,
        }
    }

    /// Whether this tab is a session with no client of ours on it.
    pub fn detached(&self) -> bool {
        self.panes.is_empty() && self.shared.is_some()
    }

    /// The rmux sessions this tab stands for, whether attached or not.
    pub fn sessions(&self) -> impl Iterator<Item = &str> {
        self.panes
            .iter()
            .filter_map(|pane| pane.rmux.as_deref())
            .chain(self.shared.iter().map(|s| s.name.as_str()))
            .chain(self.extra.iter().map(|s| s.name.as_str()))
    }

    /// Put a client of this cctop back on the session this tab stands for.
    ///
    /// Only ever called on the way to looking at the tab, which is what makes
    /// the trade sound: at most one cctop is being *used* on a session at a time,
    /// so at most one of them holds the client whose size rmux fits the window
    /// to.
    ///
    /// A split attaches every pane at once, which is what it already does when
    /// the tab was never put down — the panes are peers and all of them are
    /// being looked at.
    pub fn attach(&mut self) -> anyhow::Result<()> {
        let Some(shared) = self.shared.clone() else {
            return Ok(());
        };
        // Leading pane first, so the tab keeps the shape it was left in whatever
        // order the sessions were listed back in.
        let mut panes = vec![Self::attach_one(&shared)?];
        for extra in &self.extra {
            panes.push(Self::attach_one(extra)?);
        }
        self.panes = panes;
        self.focus = 0;
        self.shared = None;
        self.extra.clear();
        Ok(())
    }

    /// One `Shared` as the pane it becomes when this cctop takes a client on it.
    fn attach_one(shared: &Shared) -> anyhow::Result<Pane> {
        // `TmuxExisting` never creates: a session that ended between the sync
        // that found it and this must fail and say so, not silently start a new
        // agent under a dead agent's name.
        let argv = [shared.label.clone()];
        let mut pane = Pane::launch(&argv, None, Own::TmuxExisting(shared.name.clone()))?;
        // The label the other cctop chose, not one reconstructed from the argv
        // above — which is the label already, but only by coincidence.
        pane.label = shared.label.clone();
        // Reattaching is not relaunching, so nothing here chose an account —
        // this is the one the session was started under, read back off rmux by
        // the sweep that found it or kept from the pane this tab last had.
        pane.profile = shared.profile.clone();
        Ok(pane)
    }

    /// Give up this cctop's client on the session, keeping the tab.
    ///
    /// Dropping the pane kills the rmux client and nothing else — the session,
    /// and the agent in it, carry on for whichever cctop looks next. Declines for
    /// anything that could not be rebuilt from a session name: a pty cctop owns
    /// and would therefore *end* here, or a pane merely looking at somebody
    /// else's agent.
    ///
    /// A split is now rebuildable, so it detaches like anything else — each pane
    /// keeps its own [`Shared`] and the whole thing comes back together on the
    /// way in. It used to decline, which is why a split could not survive a
    /// restart at all: the tab held clients it had no way of giving up, and the
    /// only record of the arrangement was the process about to exit.
    pub fn detach(&mut self) -> bool {
        // Nothing to give up. Without this a tab that was *already* detached —
        // empty `panes`, holding its `Shared`s — would read as a successful
        // detach and be cleared into a tab with no session behind it at all,
        // which `drop_empty_tabs` then throws away.
        if self.panes.is_empty() {
            return false;
        }
        // A recording is fed by this client, so giving it up would end the
        // recording — and switching tabs is not asking for that. The tab keeps
        // its client until the recording is stopped, at the cost of rmux
        // sizing the window for it meanwhile.
        if self.panes.iter().any(|pane| pane.view.recording()) {
            return false;
        }
        // Every pane has to be one that can come back. A tab with a pty cctop
        // owns anywhere in it keeps its clients rather than losing the pane.
        if self.panes.iter().any(|pane| pane.rmux.is_none()) {
            return false;
        }
        let panes = std::mem::take(&mut self.panes);
        let mut shared = Vec::with_capacity(panes.len());
        for pane in &panes {
            shared.push(Shared {
                name: pane.rmux.clone().expect("checked above"),
                label: pane.label.clone(),
                pid: Some(pane.agent()),
                // Nothing has been read off rmux for this tab yet, and the pane
                // it is replacing was on screen a moment ago. The next sweep
                // fills it.
                activity: None,
                profile: pane.profile.clone(),
                state: None,
            });
        }
        // The leading pane is the tab; the rest follow it in order.
        self.shared = shared.first().cloned();
        self.extra = shared.into_iter().skip(1).collect();
        self.focus = 0;
        true
    }

    /// Call the tab something other than the command that started it.
    ///
    /// Written onto the rmux session as well as the pane, because the label is
    /// how every *other* cctop names this tab — and how this one names it again
    /// after a detach. A rename only the pane remembered would come back as the
    /// old name the moment either happened.
    ///
    /// The first pane, not the focused one: a split tab is titled after its
    /// first pane with a count of the rest, so that is the label the bar shows.
    pub fn rename(&mut self, name: String) {
        if let Some(pane) = self.panes.first_mut() {
            if let Some(session) = pane.rmux.as_deref() {
                cctop_core::rmux::set_label(session, &name);
            }
            pane.label = name;
            // A pane with no rmux session takes its harness from the label, so
            // the rename can change which footer's rules its cached reading was
            // made under. See [`Pane::read_screen`].
            pane.forget_screen();
        } else if let Some(shared) = self.shared.as_mut() {
            cctop_core::rmux::set_label(&shared.name, &name);
            shared.label = name;
        }
    }

    /// Paint the tab, the way [`Tab::rename`] names it.
    ///
    /// Written onto every rmux session the tab stands for — both halves of a
    /// split included, which is why `sessions` rather than the first pane —
    /// so every cctop that lists them paints it the same way, and so the
    /// colour is still there after a detach. A pane with no session behind it
    /// keeps the colour in memory only, as far as its label gets either.
    ///
    /// `None` hands the tab back to the default ink.
    pub fn recolor(&mut self, color: Option<Hue>) {
        for session in self.sessions() {
            cctop_core::rmux::set_color(session, color.map(Hue::name).unwrap_or(""));
        }
        self.color = color;
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

    /// Whether any pane of this tab is being recorded, for the bar to say so.
    pub fn recording(&self) -> bool {
        self.panes.iter().any(|pane| pane.view.recording())
    }

    pub fn focused_mut(&mut self) -> Option<&mut Pane> {
        self.panes.get_mut(self.focus)
    }

    /// Whether one pane is filling the tab.
    ///
    /// Only while there is more than one: zooming a lone pane changes nothing
    /// on screen, and a marker in the bar that says otherwise would be a lie
    /// — nor should closing a split down to one leave the flag set for the
    /// next split to inherit as a surprise.
    pub fn zoomed(&self) -> bool {
        self.zoom && self.panes.len() > 1
    }

    /// Zoom the focused pane, or put the split back. Returns whether it is
    /// zoomed now; `None` when there is nothing to zoom it over.
    pub fn toggle_zoom(&mut self) -> Option<bool> {
        if self.panes.len() < 2 {
            self.zoom = false;
            return None;
        }
        self.zoom = !self.zoom;
        Some(self.zoom)
    }

    /// Add a pane to the split and give it the keyboard.
    ///
    /// Unzooms: asking for a split is asking to see both halves of it, and a
    /// new pane born hidden behind a zoom would be an agent started out of
    /// sight.
    ///
    /// Records the tab's shape onto rmux as it goes, so this arrangement is
    /// still here after a restart: which tab each session belongs to, which pane
    /// of it each is, and which way the tab divides. Written on every pane
    /// rather than the new one alone, because the two sessions were not a tab
    /// before this call and both have to be able to say so afterwards.
    pub fn split(&mut self, pane: Pane, stacked: bool) {
        self.stacked = stacked;
        self.panes.push(pane);
        self.focus = self.panes.len() - 1;
        self.zoom = false;
        self.record_shape();
    }

    /// Write the tab's membership and layout onto every session it stands for.
    ///
    /// The leading pane's name is the tab's name — it is unique already, it is
    /// stable for as long as the session is, and it costs nothing to mint. It is
    /// written onto every pane including the leader, so that closing the leader
    /// leaves the rest still able to say which tab they are.
    ///
    /// Best effort and silent, like every other `@cctop_*` write: a shape that
    /// failed to save is a split that comes back as two tabs, which is where a
    /// split was before any of this.
    pub fn record_shape(&self) {
        let names: Vec<&str> = self.sessions().collect();
        let Some(leader) = names.first().copied() else {
            return;
        };
        let axis = match self.stacked {
            true => cctop_core::rmux::Axis::Stacked,
            false => cctop_core::rmux::Axis::Side,
        };
        for (index, name) in names.iter().enumerate() {
            // A tab of one records nothing: it is the shape every session had
            // before this option existed, and writing it would mean a later split
            // has to take it back off.
            if names.len() < 2 {
                cctop_core::rmux::set_tab(name, "");
            } else {
                cctop_core::rmux::set_tab(name, leader);
                cctop_core::rmux::set_pane(name, index);
            }
        }
        if names.len() > 1 {
            cctop_core::rmux::set_axis(&names, axis);
        }
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
    /// is the difference between a rmux-backed tab that knows what its agent is
    /// doing and one reduced to guessing from its screen.
    pub fn attention(
        &self,
        focused: bool,
        known: &dyn Fn(u32) -> Option<cctop_core::hook::Signal>,
    ) -> Option<Attention> {
        // A tab nobody is attached to has no screen to read, so `known` is not
        // the tiebreak here — it is the whole answer. Which is enough for the
        // case that matters: an agent another cctop started, blocked on a
        // question, still blinks at you here.
        if let Some(shared) = &self.shared {
            // Three answers, in order of how directly they know. A live report
            // is this cctop hearing the agent itself; the session's record is
            // some *other* cctop having heard it, which is the only answer that
            // survives this one restarting or having been closed at the time;
            // and `idle()` below is nobody having heard anything, reading a
            // clock instead.
            let reported = shared.pid.and_then(known).or_else(|| shared.recorded());
            return match reported {
                Some(cctop_core::hook::Signal::NeedsInput) => Some(Attention::NeedsInput),
                // The held-prompt shape, read off rmux's record of the session
                // instead of a screen. See the pane arm below for why a tool in
                // flight over a still terminal is a question.
                Some(cctop_core::hook::Signal::Acting) => {
                    shared.idle().then_some(Attention::NeedsInput)
                }
                Some(signal) if signal.is_working() => None,
                Some(_) => Some(Attention::Idle),
                // No hooks, so the fallback is the same one a pane uses — that
                // the thing has stopped drawing — asked of rmux instead of a
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
                Some(cctop_core::hook::Signal::Idle) if pane.rang() => Some(Attention::Idle),
                // Otherwise, before anything inferred: the agent rang. A harness
                // rings when it is blocked on you — see [`Pane::rang`].
                _ if pane.rang() => Some(Attention::NeedsInput),
                // Everything below infers a turn from a screen or a hook, and a
                // pane that is not an agent has no turns to infer. A shell left
                // at its prompt is still by definition, so the quiet-means-idle
                // fallback fires on every one of them: the tab goes green two
                // seconds after you stop typing and reads as an agent waiting
                // for you, in a tab holding no agent at all.
                //
                // Above rather than below the bell on purpose — a bell is the
                // thing in the pane asking for you outright, and a long build
                // that finishes with a `\a` means it as much as an agent does.
                _ if !pane.is_agent => None,
                Some(cctop_core::hook::Signal::NeedsInput) => Some(Attention::NeedsInput),
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
                Some(cctop_core::hook::Signal::Acting) => {
                    pane.idle().then_some(Attention::NeedsInput)
                }
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

    /// The agents this tab speaks for in the bar: every pane's but the one you
    /// are looking straight at, which is the same exclusion
    /// [`Tab::attention`] makes. A detached tab speaks for the agent its
    /// session carries.
    pub fn unwatched_agents(&self, focused: bool) -> Vec<u32> {
        if let Some(shared) = &self.shared {
            return shared.pid.into_iter().collect();
        }
        self.panes
            .iter()
            .enumerate()
            .filter(|(i, pane)| pane.is_agent && !(focused && *i == self.focus))
            .map(|(_, pane)| pane.agent())
            .collect()
    }

    /// Drop the panes whose agents have exited. True once nothing is left.
    ///
    /// A detached tab is never nothing left: it holds no pane by design, and
    /// what becomes of it is the sync's to decide — the session it stands for
    /// outlives every client, this cctop's included.
    ///
    /// A pane that goes while it is being recorded has its recording stopped
    /// here, and where the file went is added to `saved` for the caller to say:
    /// the agent exiting is the commonest way a recording ends, and the one
    /// nobody pressed a key for.
    pub fn reap(&mut self, saved: &mut Vec<(std::path::PathBuf, std::io::Result<()>)>) -> bool {
        self.panes.retain_mut(|pane| {
            if !pane.finished() {
                return true;
            }
            saved.extend(pane.view.stop_recording());
            false
        });
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
    let mut found: Vec<Vec<String>> = cctop_core::alias::AGENTS
        .split_whitespace()
        .filter(|agent| cctop_core::shim::is_command(agent))
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
    /// An agent cctop left running in rmux, from this run or an earlier one.
    ///
    /// Boxed because a launcher row only needs four of a session's dozen fields
    /// — its name, where it works, what account it runs under, and whether
    /// somebody is already looking at it — and `Running` grew past the size
    /// where holding one of these inline doubles the enum. The box is per row,
    /// there are as many rows as there are live agents, and each is a handful of
    /// strings.
    Waiting(Box<cctop_core::rmux::Running>),
    /// A command to start fresh.
    Start(Vec<String>),
    /// Somewhere a handoff can send the session: an agent, under an account.
    ///
    /// Its own variant rather than a `Start` whose argv already carries the
    /// account, because the launcher reads the account back off the choice —
    /// to name the tab's account, and to put a copied transcript where that
    /// account looks — and an `env` prefix is not something to parse back.
    Handoff(cctop_core::handoff::Target),
}

impl Choice {
    pub fn label(&self) -> String {
        match self {
            // The rmux name is `cctop-<what>`; the prefix is true of every one
            // of them and so tells the reader nothing.
            Choice::Waiting(agent) => agent
                .name
                .strip_prefix("cctop-")
                .unwrap_or(&agent.name)
                .to_string(),
            Choice::Start(argv) => label_of(argv),
            Choice::Handoff(target) => target.label(),
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
            Choice::Start(_) | Choice::Handoff(_) => None,
        }
    }
}

/// What the launcher offers: the agents still running in rmux first, then the
/// commands that start a new one.
///
/// Agents outliving cctop is only half of the bargain — the other half is being
/// able to get back to them. Without this they survive somewhere unnameable,
/// reachable only by knowing to run `rmux attach` yourself, which is a worse
/// deal than the panes that simply died.
///
/// `open` is the rmux sessions already on screen in this cctop; they are left
/// out, since a second client onto one agent only makes the two panes argue
/// about the window size.
///
/// A session attached from *elsewhere* — a `rmux attach` in another terminal —
/// is still offered. It has the same problem, but hiding a running agent is the
/// worse of the two failures, so it is shown and labelled instead.
pub fn choices(open: &[String]) -> Vec<Choice> {
    cctop_core::rmux::running()
        .into_iter()
        .filter(|agent| !open.contains(&agent.name))
        .map(|agent| Choice::Waiting(Box::new(agent)))
        .chain(harnesses().into_iter().map(Choice::Start))
        .collect()
}

/// How a command picked from the launcher is named on screen: the command as
/// typed, minus any path, which matches what [`shim::host`](cctop_core::shim::host)
/// calls it once it is running.
pub fn label_of(argv: &[String]) -> String {
    // `env VAR=value claude` is a `claude` tab. The prefix is how the agent was
    // started, which is plumbing, and naming a tab after its plumbing is the
    // same mistake as calling one `rmux new-session -A -s cctop-claude`.
    cctop_core::config::without_launch_prefix(argv)
        .iter()
        .map(|arg| arg.rsplit('/').next().unwrap_or(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `argv` starts one of the agents cctop knows about.
///
/// Asked of the same list the aliases are built from, so a harness cctop can
/// shim is one it will read a turn off — and anything else, the login shell
/// included, is a terminal cctop is merely holding.
fn starts_an_agent(argv: &[String]) -> bool {
    // Through `label_of` so that `env FOO=1 claude` is still claude, which is
    // exactly how the profile-carrying launches spell themselves.
    let label = label_of(argv);
    let mut words = label.split_whitespace();
    let Some(command) = words.next() else {
        return false;
    };
    // `cctop sandbox` is an agent with its work elsewhere, and its screen and
    // its turns are that agent's — what a remote launch starts.
    if command.starts_with("cctop") && words.next() == Some("sandbox") {
        return true;
    }
    cctop_core::alias::AGENTS
        .split_whitespace()
        .any(|agent| agent == command)
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_question_with_choices_is_told_from_a_permission_prompt() {
        let rows = |text: &str| -> Vec<String> { text.lines().map(String::from).collect() };
        // The shape of the screen that had Allow put next to it: the agent's
        // message, a multi-question chip, the question, and its menu.
        let question = rows(
            "\
 2. What would ship. None of this is committed, and the working tree holds other work.
──────────────────────────────────────
 ☐ Deploy

How should the deploy step (between migration 1 and the rest) happen?

❯ 1. You deploy, I run SQL (Recommended)
     I run step 1 now and tell you; you deploy the code your usual way.
  2. I restart the worker
  3. Hold everything
  4. Type something.
──────────────────────────────────────
  5. Chat about this

Enter to select · ↑/↓ to navigate · Esc to cancel",
        );
        assert_eq!(
            cctop_core::screen::screen_state("claude", &question),
            Some(cctop_core::hook::Signal::NeedsInput)
        );
        assert!(cctop_core::screen::screen_question("claude", &question));
        assert_eq!(
            cctop_core::screen::screen_ask("claude", &question).as_deref(),
            Some("How should the deploy step (between migration 1 and the rest) happen?"),
            "the question's own words, never the agent's message above it"
        );
        let permission = rows(
            "\
 Bash command
   rm -rf build
 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and always allow access to this folder
   3. No

 Esc to cancel · Tab to amend",
        );
        assert!(!cctop_core::screen::screen_question("claude", &permission));
        assert!(
            !cctop_core::screen::screen_question("codex", &question),
            "claude's menus only"
        );
    }

    #[test]
    fn a_window_counts_as_this_panes_when_only_a_status_line_differs() {
        assert!(Fit::matches((120, 40), 120, 40));
        // rmux's status line takes a row of the client, where a session shows one.
        assert!(Fit::matches((120, 39), 120, 40));
        assert!(!Fit::matches((80, 24), 120, 40));
        assert!(!Fit::matches((120, 38), 120, 40));
    }

    #[test]
    fn a_mismatched_window_is_taken_back_with_one_narrow_frame_and_then_the_real_size() {
        let mut pane = Pane::for_test("agent");
        // What a keystroke's check reports when a browser holds the window.
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Some((60, 15))).expect("send");
        pane.fit.asking = Some(rx);
        assert_eq!(
            pane.fit_request(80, 24),
            (79, 24),
            "the nudge narrows by one column"
        );
        assert!(pane.fitting());
        assert_eq!(
            pane.fit_request(80, 24),
            (79, 24),
            "and holds it until rmux has seen it"
        );
        // The nudge's age is what the rule reads, so it is aged rather than
        // waited out.
        pane.fit.nudged_at = pane.fit.nudged_at.map(|at| at - FIT_NUDGE);
        assert_eq!(pane.fit_request(80, 24), (80, 24));
        // Then a look a moment later at whether it took.
        assert!(pane.fit.verify_at.is_some());
    }

    #[test]
    fn a_nudge_that_did_not_take_is_not_tried_again_at_the_same_size() {
        let mut pane = Pane::for_test("agent");
        let size = pane.view.size;
        // The look after a nudge finds the window still someone else's.
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Some((size.0.saturating_sub(10), size.1)))
            .expect("send");
        pane.fit.asking = Some(rx);
        pane.fit.verifying = true;
        pane.fit_request(size.0, size.1);
        assert_eq!(pane.fit.gave_up, Some(size));
        // A later keystroke at the same size asks nothing, so nudges nothing.
        pane.fit.typed_at = None;
        pane.note_input(false);
        assert!(!pane.fitting());
    }

    #[test]
    fn a_window_that_is_already_this_panes_is_left_alone() {
        let mut pane = Pane::for_test("agent");
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(Some((80, 24))).expect("send");
        pane.fit.asking = Some(rx);
        // No nudge: one would make the agent redraw on every keystroke.
        assert_eq!(pane.fit_request(80, 24), (80, 24));
        assert!(!pane.fitting());
    }
    /// The screens below are Claude Code 2.1.283's own, captured from a real
    /// session: at its prompt, mid-turn, on a permission prompt and on an
    /// AskUserQuestion — plus a reply quoting the phrases, which must not count.
    /// One footer per state for every other harness, spelled the way herdr's
    /// manifests say they are drawn — so a row edited into the table is checked
    /// against the words it was meant to catch, and the harnesses whose words
    /// overlap stay apart.
    #[test]
    fn each_harness_is_read_by_its_own_words() {
        use cctop_core::hook::Signal::{Busy, NeedsInput};
        let read = |harness: &str, text: &str| {
            let rows: Vec<String> = text.lines().map(String::from).collect();
            cctop_core::screen::screen_state(harness, &rows)
        };
        let cases = [
            (
                "codex",
                "Allow command?\n  Yes (y)   No (n)",
                Some(NeedsInput),
            ),
            ("gemini", "⠏ Reading files (esc to cancel, 3s)", Some(Busy)),
            (
                "gemini",
                "│ Allow execution of: 'touch x'?\n│ ● 1. Yes",
                Some(NeedsInput),
            ),
            ("opencode", "■■■■⬝⬝⬝⬝  esc interrupt", Some(Busy)),
            (
                "opencode",
                "△ Permission required\n  bash touch x",
                Some(NeedsInput),
            ),
            ("cursor", "⬢ Generating  ctrl+c to stop", Some(Busy)),
            (
                "cursor",
                "Run this command?\n → Run (once) (y)\n   Skip (esc or n)",
                Some(NeedsInput),
            ),
            ("devin", "Running tools · esc to interrupt", Some(Busy)),
            (
                "devin",
                "Approve once\nSelect · Confirm · Esc cancel",
                Some(NeedsInput),
            ),
            ("devin", "❭ \n  Context: 12% used", None),
            ("droid", "⠋ Thinking…  (esc to stop)", Some(Busy)),
            (
                "droid",
                "> Yes, allow\n  No, cancel\nEnter to select · ↑↓ to navigate · Esc to cancel",
                Some(NeedsInput),
            ),
            ("pi", "── ⠋ Working... ──", Some(Busy)),
            ("pi", "> hello", None),
            ("aider", "esc to interrupt", None),
        ];
        for (harness, text, want) in cases {
            assert_eq!(read(harness, text), want, "{harness}: {text:?}");
        }
    }

    /// Codex 0.157.1's own screens: `? for shortcuts` on the bottom line
    /// throughout, which is why it is not taken for idle; the working line
    /// above the prompt box; and an approval.
    #[test]
    fn codex_says_what_it_is_doing_above_its_prompt() {
        use cctop_core::hook::Signal;
        let screen = |text: &str| {
            let rows: Vec<String> = text.lines().map(String::from).collect();
            cctop_core::screen::screen_state("codex", &rows)
        };
        let busy = "\
› Run the shell command: touch hello.txt
• I’ll create hello.txt in the current workspace.
◦ Working (5s • esc to interrupt)

› Ask Codex to do anything

  GPT-5.6-Terra high · ~/play · No changes
  ← for agents · ? for shortcuts";
        assert_eq!(screen(busy), Some(Signal::Busy));
        let between = "\
› Run the shell command: touch hello.txt
• I’ll create hello.txt in the current workspace.

› Ask Codex to do anything

  GPT-5.6-Terra high · ~/play · No changes
  ← for agents · ? for shortcuts";
        assert_eq!(screen(between), None, "mid-turn, and no phrase says so");
        let approval = "\
• Running touch ../outside.txt
  Would you like to run the following command?
  $ touch ../outside.txt
› 1. Yes, proceed (y)
  2. Yes, and don't ask again for commands that start with `touch ../outside.txt` (p)
  3. No, and tell Codex what to do differently (esc)
  Press enter to confirm or esc to cancel";
        assert_eq!(screen(approval), Some(Signal::NeedsInput));
        let hooks = "\
  Hooks need review
› 1. Review hooks
  2. Trust all and continue
  3. Continue without trusting (hooks won't run)
  enter confirm · esc skip";
        assert_eq!(screen(hooks), Some(Signal::NeedsInput));
    }

    #[test]
    fn claude_says_what_it_is_doing_in_its_footer() {
        use cctop_core::hook::Signal;
        let screen = |text: &str| {
            let rows: Vec<String> = text.lines().map(String::from).collect();
            cctop_core::screen::screen_state("claude", &rows)
        };
        let idle = "\
────────────────────────────
❯ Try \"create a util logging.py that...\"
────────────────────────────
  ⚠ Transcript saving is off
  ⏸ manual mode on · ? for shortcuts

";
        assert_eq!(screen(idle), None, "idle is stillness, not a phrase");
        let busy = "\
✢ Quantumizing… (2s · thinking)
────────────────────────────
❯
────────────────────────────
  ⚠ Transcript saving is off
  ⏸ manual mode on · esc to interrupt";
        assert_eq!(screen(busy), Some(Signal::Busy));
        let permission = "\
 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and always allow access to this folder
   3. No

 Esc to cancel · Tab to amend
";
        assert_eq!(screen(permission), Some(Signal::NeedsInput));
        let question = "\
❯ 1. Tea
  2. Coffee
────────────────────────────
  4. Chat about this
Enter to select · ↑/↓ to navigate · Esc to cancel";
        assert_eq!(screen(question), Some(Signal::NeedsInput));
        let quoted = "\
⏺ The footer reads `Esc to cancel` on a prompt and `esc to interrupt` mid-turn.
  That is how the state is told.
❯ fix the parser
  ⏸ manual mode on";
        assert_eq!(screen(quoted), None, "a reply is not the footer");
        assert_eq!(screen(""), None);
    }

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
            cctop_core::util::truncate(
                "Improve super cctop",
                super::super::launch::TAB_LABEL_CHARS
            )
        );
        assert_eq!(label, "claude · Improve super cctop");
        assert!(!label.contains("4ebf1ab4"));
    }

    /// A tab launched under a profile is still a `claude` tab. The `env` prefix
    /// is how it was started, and naming a tab after its plumbing is the same
    /// mistake as calling one `rmux new-session -A -s cctop-claude`.
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
                cctop_core::shim::is_command(&argv[0]),
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
        // Whatever rmux happens to be holding, the "start new" block is intact
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

    /// A session as rmux would describe it, with `ago` seconds since it last
    /// drew anything.
    fn session(name: &str, label: Option<&str>, ago: u64) -> cctop_core::rmux::Running {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        cctop_core::rmux::Running {
            name: name.to_string(),
            pid: Some(4321),
            cwd: None,
            attached: false,
            activity: Some(now.saturating_sub(ago)),
            label: label.map(str::to_string),
            profile: None,
            order: None,
            state: None,
            color: None,
            tab: None,
            pane: None,
            axis: None,
            window: None,
        }
    }

    /// The point of writing the state onto the session: a cctop that was not
    /// running when the agent asked still finds the question waiting for it.
    ///
    /// `known` answers `None` throughout, which is what a fresh cctop knows —
    /// the live event went over a socket to whoever was listening at the time,
    /// and that was somebody else. Before the session carried it, the only
    /// thing left to go on was the activity clock, and a held prompt over a
    /// still screen reads there as merely idle.
    #[test]
    fn a_tab_finds_a_question_that_was_asked_before_this_cctop_started() {
        let recorded = |signal, ago: u64| {
            let mut agent = session("cctop-x", Some("claude"), 0);
            agent.state = Some(cctop_core::rmux::State {
                signal,
                at: cctop_core::rmux::now_secs().saturating_sub(ago),
            });
            Tab::for_agent(&agent)
        };
        let unheard = &|_| None;

        assert_eq!(
            recorded(cctop_core::hook::Signal::NeedsInput, 300).attention(false, unheard),
            Some(Attention::NeedsInput),
            "the session remembered what this cctop never heard"
        );
        assert_eq!(
            recorded(cctop_core::hook::Signal::Idle, 300).attention(false, unheard),
            Some(Attention::Idle),
            "and a finished turn is still the quieter of the two"
        );
        assert_eq!(
            recorded(cctop_core::hook::Signal::Busy, 5).attention(false, unheard),
            None,
            "an agent recorded as working is left alone"
        );

        // A working claim nobody has confirmed for hours is the shape a session
        // killed mid-turn leaves behind. It lapses back to the screen-reading
        // fallback rather than asserting itself forever — and this tab's clock
        // says it last drew a moment ago, so that fallback is "not idle".
        assert_eq!(
            recorded(cctop_core::hook::Signal::Busy, 3 * 3_600).attention(false, unheard),
            None,
        );

        // A live report is fresher than anything written down, and wins.
        let stale = recorded(cctop_core::hook::Signal::NeedsInput, 300);
        assert_eq!(
            stale.attention(false, &|_| Some(cctop_core::hook::Signal::Busy)),
            None,
            "this cctop heard the agent go back to work"
        );
    }

    /// A pane holding `label`, drawn `ago` seconds back.
    fn pane(label: &str, is_agent: bool, ago: u64) -> Pane {
        Pane {
            hosted: None,
            rmux: None,
            resumed: None,
            profile: None,
            pid: 4321,
            agent: None,
            asked_at: None,
            asked: None,
            label: label.into(),
            is_agent,
            view: cctop_core::attach::Attach::for_test(),
            drew_at: Instant::now() - Duration::from_secs(ago),
            read: None,
            fit: Fit::default(),
        }
    }

    /// A pane whose agent rang, with everything else saying it is busy.
    fn ringing_tab(bell: &[u8]) -> Tab {
        let mut pane = pane("claude", true, 0);
        pane.view.parser.process(bell);
        Tab::new(pane)
    }

    /// The launcher offers the login shell, and a shell has no turns to finish.
    ///
    /// Every inference in `attention` reads a still screen as an agent waiting
    /// for you. A shell at its prompt is still by definition, so a tab opened
    /// for `git diff` — or to run cctop in — went green two seconds after the
    /// last keystroke and sat there claiming an agent wanted something.
    #[test]
    fn a_tab_that_is_not_an_agent_reports_no_turn() {
        let unreported = &|_: u32| None;
        assert_eq!(
            Tab::new(pane("zsh", false, 5)).attention(false, unreported),
            None,
            "a quiet shell is a shell, not a finished turn"
        );
        assert_eq!(
            Tab::new(pane("claude", true, 5)).attention(false, unreported),
            Some(Attention::Idle),
            "and the same silence from an agent still means what it did"
        );

        // Nor does the permission-prompt shape apply: a tool in flight over a
        // still terminal is a question only where a terminal holds an agent.
        assert_eq!(
            Tab::new(pane("zsh", false, 5))
                .attention(false, &|_| Some(cctop_core::hook::Signal::Acting)),
            None
        );

        // The bell survives, because that is the thing in the pane asking
        // outright rather than cctop reading its pixels.
        let mut rang = pane("zsh", false, 5);
        rang.view.parser.process(b"\x07");
        assert_eq!(
            Tab::new(rang).attention(false, unreported),
            Some(Attention::NeedsInput)
        );
    }

    /// What counts as an agent is the list the aliases are built from, so a
    /// harness cctop can shim is one it will read a turn off.
    #[test]
    fn an_agent_is_recognised_through_the_way_it_was_launched() {
        let argv = |s: &str| -> Vec<String> { s.split(' ').map(str::to_string).collect() };
        assert!(starts_an_agent(&argv("claude")));
        assert!(starts_an_agent(&argv("claude --resume 4ebf1ab4")));
        assert!(starts_an_agent(&argv("/usr/local/bin/codex")));
        // How a launch under a named account spells itself.
        assert!(starts_an_agent(&argv(
            "env CLAUDE_CONFIG_DIR=/tmp/x claude"
        )));
        // A remote launch: an agent, with its work on a host.
        assert!(starts_an_agent(&argv(
            "env CCTOP_SANDBOX_HOLD=1 /usr/local/bin/cctop sandbox --agent opencode devbox:~"
        )));
        assert!(!starts_an_agent(&argv("/usr/local/bin/cctop doctor")));
        assert!(!starts_an_agent(&argv("/bin/zsh")));
        assert!(!starts_an_agent(&argv("")));
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
            ringing.attention(false, &|_| Some(cctop_core::hook::Signal::Busy)),
            Some(Attention::NeedsInput),
        );
        assert_eq!(
            ringing.attention(false, &|_| Some(cctop_core::hook::Signal::Idle)),
            Some(Attention::Idle),
            "the idle nudge was read as a question"
        );
        // A tool call held over a rung bell is the permission prompt itself.
        assert_eq!(
            ringing.attention(false, &|_| Some(cctop_core::hook::Signal::Acting)),
            Some(Attention::NeedsInput),
        );

        // The control: the same freshly drawn pane, silent, is left alone.
        let quiet = ringing_tab(b"thinking");
        assert_eq!(
            quiet.attention(false, &|_| Some(cctop_core::hook::Signal::Busy)),
            None,
        );

        // And the pane you are looking at is never the one blinking at you.
        assert_eq!(
            ringing.attention(true, &|_| Some(cctop_core::hook::Signal::Busy)),
            None,
        );
    }

    /// The reading is kept for as long as the screen is in the same place, so
    /// it must be redone whenever the screen moves — and an unmoved screen must
    /// not be re-read either, since that is the whole of the per-tick cost.
    ///
    /// What the shortcut must never do is answer from a screen that has since
    /// changed: an agent that finished its turn is idle now, not working then.
    #[test]
    fn a_reading_follows_the_screen_it_was_made_from() {
        let busy = " \u{280f} Reading files (esc to interrupt)".as_bytes();
        let mut pane = pane("claude", true, 0);
        pane.view.parser.process(busy);
        assert_eq!(
            pane.read_screen().map(|read| read.signal),
            Some(cctop_core::hook::Signal::Busy),
            "a working hint on the footer"
        );

        // The same pixels, read again: the same answer, and nothing laid out.
        assert_eq!(
            pane.read_screen().map(|read| read.signal),
            Some(cctop_core::hook::Signal::Busy)
        );

        // A pane that draws again is read afresh, so the answer moves with it.
        pane.view
            .parser
            .process(" Esc to cancel \u{b7} Tab to amend".as_bytes());
        pane.drew_at = Instant::now();
        assert_eq!(
            pane.read_screen().map(|read| read.signal),
            Some(cctop_core::hook::Signal::NeedsInput),
            "the footer changed under a pane that drew"
        );
    }

    /// A pane with no rmux session is named for its harness, so a rename moves
    /// which rules its cached reading was made under — and a reading kept across
    /// one would answer for a harness this pane is no longer.
    #[test]
    fn a_rename_makes_the_pane_read_again() {
        let mut pane = pane("claude", true, 0);
        pane.view
            .parser
            .process(" \u{280f} Reading files (esc to interrupt)".as_bytes());
        assert_eq!(
            pane.read_screen().map(|read| read.signal),
            Some(cctop_core::hook::Signal::Busy)
        );

        // The same screen under a harness with no footer rules to read is no
        // longer a screen worth reading.
        let mut tab = Tab::new(pane);
        tab.panes[0].forget_screen();
        tab.rename("zsh".into());
        assert_eq!(
            tab.panes[0].read_screen().map(|read| read.signal),
            None,
            "read under the harness it is now named for"
        );
    }

    /// A resize and a scroll move the window the rows are read from without the
    /// agent drawing anything, so a reading taken before either is a reading of
    /// a screen that is no longer on the pane.
    #[test]
    fn a_moved_screen_is_read_again() {
        let working = " \u{280f} Reading files (esc to interrupt)";
        let asking = " Esc to cancel \u{b7} Tab to amend";
        // Four rows, six lines: the last two are a prompt on the live screen,
        // and the two the scrollback reveals above it are a turn in flight.
        let mut screen =
            vt100::Parser::new_with_callbacks(4, 80, 5_000, cctop_core::attach::Signals::default());
        screen.process(
            format!("{working}\r\n{working}\r\n{working}\r\n{working}\r\n{asking}\r\n{asking}")
                .as_bytes(),
        );
        let mut pane = pane("claude", true, 0);
        pane.view.parser = screen;
        assert_eq!(
            pane.read_screen().map(|read| read.signal),
            Some(cctop_core::hook::Signal::NeedsInput),
            "the prompt is the last line on screen"
        );

        // Scrolled back off it, the same screen is showing a turn in flight.
        pane.view.parser.screen_mut().set_scrollback(2);
        assert_eq!(
            pane.read_screen().map(|read| read.signal),
            Some(cctop_core::hook::Signal::Busy),
            "a screen scrolled onto other lines says what they say"
        );
    }

    /// The point of writing the label onto the session: two cctops showing the
    /// same agent must call it the same thing. The one that started it knows the
    /// conversation's title; every other one has only the session name, which
    /// for a resume is a sanitised uuid.
    #[test]
    fn a_shared_tab_is_called_what_the_cctop_that_started_it_called_it() {
        let tab = Tab::for_agent(&session(
            "cctop-claude-4ebf1ab4-2ef8-4fb2-a7d5-d445b5026dc9",
            Some("claude · Improve super cctop"),
            0,
        ));
        assert!(tab.detached());
        assert_eq!(tab.title(), "claude · Improve super cctop");

        // Nothing recorded — an agent left by a cctop older than this. The
        // session name stands in, minus the prefix every one of them carries.
        let tab = Tab::for_agent(&session("cctop-claude-32cca860", None, 0));
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
        let adopted = Tab::for_agent(&running);
        assert_eq!(
            adopted.shared.as_ref().and_then(|s| s.profile.as_deref()),
            Some("work"),
        );

        // And nothing invented for the ordinary account, which writes no option
        // and so reads back as none.
        let plain = Tab::for_agent(&session("cctop-claude-4e2b1c90", Some("claude"), 0));
        assert_eq!(plain.shared.as_ref().and_then(|s| s.profile.clone()), None);
    }

    /// A tab this cctop holds no client on still has to blink: that is the whole
    /// point of showing another cctop's agents. It has no screen to read, so the
    /// hooks answer, and rmux's own record of the session answers when they
    /// cannot.
    #[test]
    fn an_unwatched_tab_still_says_when_its_agent_wants_you() {
        let quiet = Tab::for_agent(&session("cctop-claude-a", None, 30));
        let busy = Tab::for_agent(&session("cctop-claude-b", None, 0));
        let pid = quiet.shared.as_ref().and_then(|s| s.pid).expect("pid");

        // A held question outranks everything, and being reported as working
        // outranks a session that merely looks quiet.
        assert_eq!(
            quiet.attention(false, &|_| Some(cctop_core::hook::Signal::NeedsInput)),
            Some(Attention::NeedsInput)
        );
        assert_eq!(
            quiet.attention(false, &|_| Some(cctop_core::hook::Signal::Busy)),
            None
        );
        // Asked about the agent's pid, which is the only one its hooks mention.
        assert_eq!(
            quiet.attention(false, &|asked| (asked == pid)
                .then_some(cctop_core::hook::Signal::NeedsInput)),
            Some(Attention::NeedsInput)
        );

        // A tool call in flight is working *and* a question, depending on
        // whether the terminal is still: that is the shape of a permission
        // prompt, which nothing else reports in time to blink about.
        assert_eq!(
            quiet.attention(false, &|_| Some(cctop_core::hook::Signal::Acting)),
            Some(Attention::NeedsInput)
        );
        assert_eq!(
            busy.attention(false, &|_| Some(cctop_core::hook::Signal::Acting)),
            None,
            "an agent still repainting mid-tool is working, not asking"
        );

        // No hooks: rmux's last-output time is the fallback the missing screen
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
        let mut tab = Tab::for_agent(&session("cctop-claude-a", None, 0));
        assert!(
            !tab.reap(&mut Vec::new()),
            "a shared tab was reaped for having no pane"
        );
        // Once it stands for nothing, it is nothing.
        tab.shared = None;
        assert!(tab.reap(&mut Vec::new()));
    }

    /// A still-running agent in the launcher, as rmux would have described it.
    fn waiting(name: &str) -> Choice {
        Choice::Waiting(Box::new(cctop_core::rmux::Running {
            pid: Some(4321),
            cwd: Some(std::path::PathBuf::from("/home/x/proj")),
            ..cctop_core::rmux::Running::unrecorded(name)
        }))
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

    /// The thing rmux breaks if nobody accounts for it: the pid cctop hosts is
    /// the rmux client, so everything the agent reports about itself is filed
    /// under a pid nothing on this side would ever ask about. A tab that asked
    /// the wrong one would fall back to reading the screen for exactly the panes
    /// where the agent is talking.
    #[test]
    fn a_rmux_backed_tab_asks_about_the_agent_and_not_the_client() {
        // A pane standing in for a rmux client: what cctop hosts is one pid, and
        // the agent behind it is another.
        let (_child, client_pid) =
            cctop_core::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let mut pane = Pane::view_of(client_pid, "claude".into()).expect("attach");
        pane.rmux = Some("cctop-claude-abc".into());
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
                .then_some(cctop_core::hook::Signal::NeedsInput)),
            Some(Attention::NeedsInput)
        );
        // The client's pid says nothing about the agent, and must not be taken
        // for it — this is the assertion the fix exists for.
        assert_ne!(
            tab.attention(true, &|pid| (pid == client_pid)
                .then_some(cctop_core::hook::Signal::NeedsInput)),
            Some(Attention::NeedsInput)
        );

        // Reported as working: the screen is irrelevant, however long the client
        // has sat still.
        assert_eq!(
            tab.attention(true, &|pid| (pid == agent_pid)
                .then_some(cctop_core::hook::Signal::Busy)),
            None
        );
    }

    /// A pane whose agent is still drawing must not claim to be idle, a held
    /// question must outrank a finished turn, and the tab you are looking at
    /// must stay quiet — blinking the title of the pane in front of you is
    /// noise, and it is the case that fires most often.
    #[test]
    fn a_tab_asks_for_attention_only_when_it_has_something_you_cannot_see() {
        let mut kids = Vec::new();
        let mut pane = |script: &str| {
            let (child, pid) = cctop_core::shim::test_session(&["sh", "-c", script], (80, 24));
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
        //
        // The threshold is a rule about how long ago a pane last drew, so the
        // quiet pane's last drawing is moved back past it rather than waited
        // out — two seconds a run, and under load a busy pane that had not
        // painted often enough over them read as idle too. What is waited for
        // is only what has to have happened first: each pane has drawn what
        // it was going to, so nothing arriving later resets the clock moved
        // here.
        cctop_core::test_wait::eventually_true("both panes to draw", || {
            tab.pump();
            let drawn =
                |pane: &Pane, what: &str| pane.view.parser.screen().contents().contains(what);
            drawn(&tab.panes[0], ".") && drawn(&tab.panes[1], "done")
        });
        tab.panes[1].drew_at -= QUIET_IS_IDLE;
        assert_eq!(tab.attention(false, &unreported), Some(Attention::Idle));

        // The busy pane holding a question outranks the quiet one being idle.
        assert_eq!(
            tab.attention(false, &|pid| (pid == busy_pid)
                .then_some(cctop_core::hook::Signal::NeedsInput)),
            Some(Attention::NeedsInput)
        );
        // Focused tab: the focused pane is excluded, so the quiet one is left.
        tab.focus = 0;
        assert_eq!(
            tab.attention(true, &|pid| (pid == busy_pid)
                .then_some(cctop_core::hook::Signal::NeedsInput)),
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
            let _ = cctop_core::shim::socket_path(pid).map(std::fs::remove_file);
        }
    }

    /// The trade that keeps several cctops off one rmux window: the tab you
    /// leave gives up its client and keeps everything needed to take one back.
    /// Only rmux-backed panes may do it — a pty cctop owns would be *ended* by
    /// this — and a split of them detaches whole, keeping each session's place.
    #[test]
    fn leaving_a_rmux_tab_gives_up_its_client_and_nothing_else() {
        let (mut child, pid) = cctop_core::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let mut pane = Pane::view_of(pid, "claude · Improve super cctop".into()).expect("attach");
        pane.rmux = Some("cctop-claude-abc".into());
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

        // A pane with no rmux behind it: dropping it is the kill, so it stays.
        let mut owned = Tab::new(Pane::view_of(pid, "claude".into()).expect("attach"));
        assert!(!owned.detach());
        assert!(owned.shared.is_none());

        // A split detaches too, and keeps both of its sessions: the whole point
        // of the second `Shared` is that a tab of two can be put down and taken
        // up again, in order, rather than coming back as two tabs that never met.
        let mut split = Tab::new(Pane::view_of(pid, "claude".into()).expect("attach"));
        split.panes[0].rmux = Some("cctop-claude-abc".into());
        split
            .panes
            .push(Pane::view_of(pid, "shell".into()).expect("attach"));
        split.panes[1].rmux = Some("cctop-zsh".into());
        assert!(split.detach());
        assert_eq!(split.panes.len(), 0);
        assert_eq!(
            split.shared.as_ref().map(|s| s.name.as_str()),
            Some("cctop-claude-abc")
        );
        assert_eq!(split.extra.len(), 1);
        assert_eq!(
            split.extra[0].name, "cctop-zsh",
            "the second pane kept its place"
        );
        // Both halves still answer, which is what a tab bar reads.
        assert_eq!(
            split.sessions().collect::<Vec<_>>(),
            ["cctop-claude-abc", "cctop-zsh"]
        );

        // One pane with no rmux behind it anywhere in the tab is still the kill,
        // so a split containing one keeps its clients rather than losing it.
        let mut mixed = Tab::new(Pane::view_of(pid, "claude".into()).expect("attach"));
        mixed.panes[0].rmux = Some("cctop-claude-abc".into());
        mixed
            .panes
            .push(Pane::view_of(pid, "local shell".into()).expect("attach"));
        assert!(!mixed.detach());
        assert_eq!(mixed.panes.len(), 2, "nothing was given up");

        let _ = child.kill();
        let _ = child.wait();
        let _ = cctop_core::shim::socket_path(pid).map(std::fs::remove_file);
    }

    /// The whole reason a tab has an identity recorded on rmux rather than
    /// implied by what happens to be attached: a split put down must come back
    /// up as the one tab it was, in the order it was, divided the same way.
    ///
    /// The bug: a tab was "whatever session is attached here", so a split had
    /// no form to be stored in — it could not be detached, and came back from a
    /// restart as two tabs that had never met, with the arrangement lost.
    #[test]
    fn a_split_survives_being_put_down_and_picked_up() {
        let (mut child, pid) = cctop_core::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let mut tab = Tab::new(Pane::view_of(pid, "claude".into()).expect("attach"));
        tab.panes[0].rmux = Some("cctop-claude-abc".into());
        let mut second = Pane::view_of(pid, "shell".into()).expect("attach");
        second.rmux = Some("cctop-zsh".into());
        tab.split(second, true);

        assert!(tab.detach(), "a split of rmux panes is now detachable");
        assert_eq!(
            tab.sessions().collect::<Vec<_>>(),
            ["cctop-claude-abc", "cctop-zsh"]
        );
        assert!(tab.stacked, "and it comes back divided the same way");

        let _ = child.kill();
        let _ = child.wait();
        let _ = cctop_core::shim::socket_path(pid).map(std::fs::remove_file);
    }

    /// `Tab::split` writes the tab's membership onto rmux, so the tab has an
    /// answer to "which sessions are mine" that does not live in this process.
    #[test]
    fn a_split_records_itself_onto_its_sessions() {
        if !cctop_core::rmux::available() {
            eprintln!("skipping: rmux not installed");
            return;
        }
        // The daemon is machine-wide, so this serialises against the other
        // tests that talk to it.
        let _turn = cctop_core::rmux::test_lock();
        let leader = format!("cctop-leader-{}", std::process::id());
        let other = format!("cctop-other-{}", std::process::id());
        for name in [&leader, &other] {
            let _ = cctop_core::rmux::start_detached(&["sleep".into(), "30".into()], name, None);
        }
        let (mut child, pid) = cctop_core::shim::test_session(&["sh", "-c", "sleep 30"], (80, 24));
        let mut tab = Tab::new(Pane::view_of(pid, "claude".into()).expect("attach"));
        tab.panes[0].rmux = Some(leader.clone());
        let mut second = Pane::view_of(pid, "shell".into()).expect("attach");
        second.rmux = Some(other.clone());
        tab.split(second, false);
        let read = |name: &str| {
            cctop_core::rmux::running()
                .into_iter()
                .find(|s| s.name == name)
        };
        assert_eq!(
            read(&leader).as_ref().and_then(|s| s.tab.as_deref()),
            Some(&leader[..])
        );
        assert_eq!(
            read(&other).as_ref().and_then(|s| s.tab.as_deref()),
            Some(&leader[..])
        );
        // Ordered, so the listing can put them back the right way round.
        assert_eq!(read(&leader).as_ref().and_then(|s| s.pane), Some(0));
        assert_eq!(read(&other).as_ref().and_then(|s| s.pane), Some(1));
        assert_eq!(
            read(&other).map(|s| s.axis()),
            Some(cctop_core::rmux::Axis::Side)
        );

        // A pane pulled back out of the tab stops claiming membership, rather
        // than pointing at a tab it is no longer part of.
        let mut alone = Tab::new(Pane::view_of(pid, "shell".into()).expect("attach"));
        alone.panes[0].rmux = Some(other.clone());
        alone.record_shape();
        assert_eq!(read(&other).and_then(|s| s.tab), None);

        let _ = cctop_core::rmux::kill(&leader);
        let _ = cctop_core::rmux::kill(&other);
        let _ = child.kill();
        let _ = child.wait();
        let _ = cctop_core::shim::socket_path(pid).map(std::fs::remove_file);
    }

    #[test]
    fn a_command_is_labelled_by_its_name_not_its_path() {
        assert_eq!(label_of(&["/usr/bin/claude".into()]), "claude");
        assert_eq!(
            label_of(&["codex".into(), "--full-auto".into()]),
            "codex --full-auto"
        );
    }

    /// The colour a session records becomes the tab's, and a word this cctop
    /// does not know is no colour rather than a wrong one.
    #[test]
    fn a_sessions_colour_paints_its_tab() {
        let mut agent = session("cctop-claude-x", Some("claude"), 0);
        agent.color = Some("violet".to_string());
        assert_eq!(Tab::for_agent(&agent).color, Some(Hue::Violet));
        agent.color = Some("chartreuse".to_string());
        assert_eq!(
            Tab::for_agent(&agent).color,
            None,
            "a newer cctop's hue was guessed at"
        );
    }

    /// A tab keeps the colour it was painted even where there is no session to
    /// hold it: a pane on cctop's own pty is painted in memory only, which is
    /// as far as its name travels too.
    #[test]
    fn a_tab_with_no_session_still_keeps_its_colour() {
        let mut tab = Tab::new(pane("claude", true, 0));
        tab.recolor(Some(Hue::Cyan));
        assert_eq!(tab.color, Some(Hue::Cyan));
        tab.recolor(None);
        assert_eq!(tab.color, None);
    }

    /// The harness is read off the session name when there is one — it
    /// survives a rename, which is the whole point — and off the label's first
    /// word when there is not.
    #[test]
    fn a_panes_harness_comes_from_its_session_before_its_label() {
        let mut pane = pane("renamed work", true, 0);
        assert_eq!(pane.harness(), "renamed");
        pane.rmux = Some("cctop-claude-32cca860-b503".into());
        assert_eq!(pane.harness(), "claude", "a rename hid the harness");
        pane.rmux = Some("cctop-zsh".into());
        assert_eq!(pane.harness(), "zsh");
    }

    /// In a claude pane Home and End go in as the Ctrl- forms the agent binds
    /// to the top and bottom of the chat; anywhere else they are the
    /// line-editing keys they say, and a modified Home keeps its own meaning
    /// even there.
    #[test]
    fn home_and_end_are_promoted_only_in_a_claude_pane() {
        let claude = pane("claude", true, 0);
        for code in [KeyCode::Home, KeyCode::End] {
            let sent = claude.translate_key(KeyEvent::new(code, KeyModifiers::NONE));
            assert_eq!(sent.code, code);
            assert_eq!(sent.modifiers, KeyModifiers::CONTROL);
        }
        // Shift+Home is still "select to the start of the line", not a jump.
        let shifted = claude.translate_key(KeyEvent::new(KeyCode::Home, KeyModifiers::SHIFT));
        assert_eq!(shifted.modifiers, KeyModifiers::SHIFT);
        // And a shell's Home is the shell's own.
        let shell = pane("zsh", false, 0);
        let sent = shell.translate_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        assert_eq!(sent.modifiers, KeyModifiers::NONE);
    }
}
