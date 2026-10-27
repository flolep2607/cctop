//! What an event means: the [`Signal`] a hook event reduces to, the
//! [`Event`] that carries it, and the [`Reports`] a running cctop keeps per
//! session.

use super::*;

/// What an event says about a session, as far as the UI is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// The agent asked something and is blocked on the answer.
    NeedsInput,
    /// The agent finished its turn; the prompt is the user's.
    Idle,
    /// The agent started working, which cancels either of the above.
    Busy,
    /// A tool call has begun and has not come back yet.
    ///
    /// Working, like [`Signal::Busy`] — but told apart from it because it is the
    /// only state a *held permission prompt* can be in, and for most harnesses
    /// nothing reports one in time to be useful. Claude Code is the exception:
    /// it raises `PermissionRequest` the instant the prompt goes up, which is
    /// [`Signal::NeedsInput`] outright and needs no inference. Everything else
    /// either raises a `Notification` on a six-second timer (`NOTIFY_HOOKS` in
    /// Claude Code 2.1.x) — by which point you have answered or gone looking
    /// for the tab yourself — or says nothing at all.
    /// See [`Tab::attention`](cctop_ui::tabs::Tab::attention).
    Acting,
    /// The agent is compacting its context: working, and about to lose history.
    Compacting,
    /// The session has begun. Like [`Signal::Busy`], but there is a row to find.
    Started,
    /// The session is over. There is nothing left to report about it.
    Ended,
}

impl Signal {
    /// Whether the agent is working rather than waiting on you.
    pub fn is_working(self) -> bool {
        matches!(
            self,
            Signal::Busy | Signal::Acting | Signal::Compacting | Signal::Started
        )
    }

    /// Whether typing at the agent is an answer to this state, and so cancels
    /// it. A tool call in flight counts: the answer to a permission prompt is
    /// keystrokes, and no event reports that it arrived — see
    /// [`App::mark_answered`](cctop_ui::App::mark_answered).
    pub fn awaits_you(self) -> bool {
        matches!(self, Signal::NeedsInput | Signal::Idle | Signal::Acting)
    }

    /// Whether this changes *which sessions exist*, and so is worth a rescan
    /// rather than waiting for the next poll to notice.
    pub fn is_lifecycle(self) -> bool {
        matches!(self, Signal::Started | Signal::Ended)
    }

    /// What this says about the row's state, where the transcript is blind.
    ///
    /// Only the two waiting states are answered. A working signal returns
    /// `None` and leaves the transcript's reading alone, which is not
    /// deference so much as division of labour: a transcript reads *working*
    /// perfectly well — it is being written to — and it is the only one of the
    /// two that can see an API error. What it cannot see is either way of
    /// having stopped, because a held permission prompt and a finished turn are
    /// both, on disk, just an agent that stopped writing. (Cursor is the
    /// exception for the finished turn: it writes a `turn_ended` line.)
    pub fn activity(self) -> Option<crate::session::ActivityState> {
        match self {
            Signal::NeedsInput => Some(crate::session::ActivityState::Asking),
            Signal::Idle => Some(crate::session::ActivityState::WaitingForInput),
            Signal::Busy
            | Signal::Acting
            | Signal::Compacting
            | Signal::Started
            | Signal::Ended => None,
        }
    }

    /// A word for the STATE column, the hooks panel, and the option a cctop
    /// records on an rmux session. See [`Signal::from_label`].
    pub fn label(self) -> &'static str {
        match self {
            Signal::NeedsInput => "asking",
            Signal::Idle => "idle",
            Signal::Busy => "working",
            Signal::Acting => "acting",
            Signal::Compacting => "compacting",
            Signal::Started => "started",
            Signal::Ended => "ended",
        }
    }

    /// The inverse of [`Signal::label`], for a state read back off an rmux
    /// session — see [`rmux::State`](crate::rmux::State).
    ///
    /// Words rather than numbers, so the option stays legible to anyone running
    /// `rmux show-options` and so the two cctops on either side of it can be
    /// different versions. A word this one does not know reads as nothing
    /// reported, which falls back to the guess that was there before: a newer
    /// cctop teaching an older one a signal it cannot draw should cost the
    /// older one nothing.
    pub fn from_label(word: &str) -> Option<Signal> {
        match word {
            "asking" => Some(Signal::NeedsInput),
            "idle" => Some(Signal::Idle),
            "working" => Some(Signal::Busy),
            "acting" => Some(Signal::Acting),
            "compacting" => Some(Signal::Compacting),
            "started" => Some(Signal::Started),
            "ended" => Some(Signal::Ended),
            _ => None,
        }
    }

    /// Whether a claim this old can still be true, nothing having been said
    /// since.
    ///
    /// The asymmetry is the whole rule, and it is why this is not a plain
    /// timeout: a finished turn and a held question stay true until the agent
    /// says otherwise, so somebody away from their desk for an afternoon comes
    /// back to the tab that is still asking. Only a *working* claim expires,
    /// because the events that would have closed it — the tool returning, the
    /// turn ending — are exactly the ones a killed session never sends.
    ///
    /// Taken as an elapsed duration rather than read off `self` so that a
    /// report this process heard ([`Reported::is_current`], measuring an
    /// `Instant`) and one recovered from an rmux session ([`rmux::State`],
    /// measuring wall-clock seconds written by another process) are judged by
    /// one rule rather than two that have to be kept in step.
    ///
    /// [`rmux::State`]: crate::rmux::State
    pub fn is_current_after(self, elapsed: std::time::Duration) -> bool {
        !self.is_working() || elapsed < WORKING_TTL
    }
}

/// One event, resolved to the session it concerns.
#[derive(Debug, Clone)]
pub struct Event {
    pub session_id: String,
    /// The processes the hook that reported this ran under, nearest first.
    ///
    /// Empty from a harness whose hook is not `cctop hook`.
    /// The agent's own pid is in here somewhere; which one it is, is a question
    /// only the process table can answer — see
    /// [`Collector::collect`](crate::proc::Collector::collect).
    pub pids: Vec<u32>,
    /// `host:path` when the session was launched by `cctop sandbox` — see
    /// [`Session::sandbox`](crate::session::Session::sandbox).
    pub sandbox: Option<String>,
    /// What the agent last said about itself, and where it is working.
    pub reported: Reported,
    /// The subagent this event is about, when it is about one.
    ///
    /// `SubagentStop` is the only end-of-subagent signal that exists. A
    /// background subagent's tool_result arrives at *launch* — it says the agent
    /// started, not that it finished — so a transcript alone cannot tell a
    /// working subagent from a finished one.
    pub finished_agent: Option<String>,
    /// The subagent this event came from, on any event fired inside one.
    ///
    /// What lets a question one subagent is waiting on outlive the next thing
    /// a sibling does: see `App::apply_hooks`.
    pub agent: Option<String>,
}

/// The last thing a session reported, kept per session.
///
/// The directory comes along because it is the only human-readable name an
/// event carries: a session id says nothing, and the row it belongs to may not
/// have been discovered yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    pub signal: Signal,
    /// Empty when the agent did not say.
    pub cwd: String,
    /// How much the session asks before it acts, when it has said.
    pub permission: Option<Permission>,
    /// When it arrived, which is what bounds how long it is believed. See
    /// [`Reported::is_current`].
    pub at: std::time::Instant,
    /// Whether this is a permission prompt that may yet answer itself.
    ///
    /// Claude Code's auto mode puts a permission request to a model before it
    /// puts it to the person, and that takes a few seconds. The hook fires when
    /// the request is raised, not when it is decided, so every auto-answered
    /// tool call rang the bell, blinked the tab and lit the attention card for
    /// a question nobody was ever going to be asked. Held back for
    /// [`PERMISSION_GRACE`]; if the decision lands first — `PermissionDenied`,
    /// or the tool simply running — the newer event replaces this one and it is
    /// never shown at all. See [`Reported::is_settled`].
    pub provisional: bool,
    /// What the prompt is asking to do, when this is one: see [`ask_of`].
    pub ask: Option<String>,
    /// Whether what is held is a question with choices — Claude Code's
    /// AskUserQuestion — rather than a permission prompt. Allow and Deny would
    /// press keys that *pick an option* there, so nothing may offer them.
    pub question: bool,
    /// The tool the prompt is for and what it was asked to touch, in full
    /// and redacted, when the payload named a tool: see
    /// [`crate::yolo_log::call_of`]. What a key press records in the YOLO
    /// log, which the one-line `ask` is too short and too unredacted for.
    pub call: Option<crate::yolo_log::Call>,
}

/// How long a permission prompt is given to answer itself before it is somebody
/// else's problem.
///
/// Long enough for auto mode's round trip, short enough that a prompt which
/// really is waiting for a person is not sat on. Wrong in one direction it
/// costs a few seconds' notice; wrong in the other it cries wolf on every tool
/// call, which is what teaches people to ignore the bell.
pub const PERMISSION_GRACE: std::time::Duration = std::time::Duration::from_secs(4);

/// How long "the agent is working" is believed with nothing said since.
///
/// A working claim has a shelf life and a waiting one does not, which is the
/// whole asymmetry. An agent that is thinking, or mid tool call, says something
/// again within a turn: the next tool starts, the tool comes back, the turn
/// ends. Silence this long means the event that would have closed it never
/// arrived — the session was killed, the terminal closed, the turn was
/// interrupted — and none of those produce an event of their own. Believing the
/// stale claim forever is how a session nobody is running goes on being drawn as
/// busy, and how a tool call that failed goes on being drawn as a held question.
///
/// Generous on purpose: a long turn of quiet thinking is exactly the case hooks
/// exist to report, and falling back to guessing at it early would undo that.
const WORKING_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

impl Reported {
    /// Whether this can still be true, nothing having been said since.
    ///
    /// A finished turn and a held question are stable facts: they stay true
    /// until the agent says otherwise, however long that takes — somebody away
    /// from their desk for an afternoon should still come back to the tab that
    /// is asking. Only [`Signal::is_working`] expires. See [`WORKING_TTL`].
    pub fn is_current(&self) -> bool {
        self.signal.is_current_after(self.at.elapsed())
    }

    /// Whether this can be shown yet.
    ///
    /// Everything but a fresh permission prompt can: see [`provisional`], and
    /// [`PERMISSION_GRACE`] for how long the exception lasts.
    ///
    /// [`provisional`]: Reported::provisional
    pub fn is_settled(&self) -> bool {
        !self.provisional || self.at.elapsed() >= PERMISSION_GRACE
    }
}

/// What the agents have reported, folded together the way both readers need it.
///
/// The dashboard's `App` and a standalone `cctop serve` each keep one, because
/// the folding is the subtle part and is not written twice: a subagent's open
/// question outlives whatever a sibling says next ([`still_asking`]), a prompt
/// inherits the description only its first event carried, a working claim
/// expires where a waiting one stands, and an `Ended` erases the lot.
#[derive(Default)]
pub struct Reports {
    /// The last report each session made, however old — callers gate on
    /// [`Reported::is_current`] and [`Reported::is_settled`].
    pub hooked: HashMap<String, Reported>,
    /// The questions a session's subagents are waiting on, by subagent id.
    ///
    /// `hooked` holds one report per session and every event replaces it, so a
    /// subagent's permission prompt was overwritten by whatever a sibling
    /// running beside it did next — and the session stopped asking while the
    /// question was still on screen. A question is kept here until the
    /// subagent that asked it says something else. See [`still_asking`].
    ///
    /// Visible to the crate for the tests that assert a question was closed,
    /// not for other writes — only [`Reports::observe`] folds these.
    pub asking_agents: HashMap<String, HashMap<String, Reported>>,
    /// The process tree each session's hooks reported running under, keyed by
    /// session id.
    ///
    /// Kept apart from `hooked` because it answers a different question and
    /// changes on a different clock: `hooked` is what the agent is *doing* and
    /// turns over constantly, while this is *where it is* and is written once
    /// and then repeated. Readers republish only the changes — see
    /// [`save_claims`].
    pub claims: HashMap<String, Vec<u32>>,
    /// Which sessions are working on another machine through `cctop
    /// sandbox`, as `host:path`, keyed by session id.
    ///
    /// Apart from `hooked` for the reason `claims` is: it is where the session
    /// is, said on every event and never changing, and an event that did not
    /// carry it — one from a cctop too old to send it — must not wipe it.
    pub sandboxes: HashMap<String, String>,
    /// Prompts `cctop yolo-hook` allowed before the observer's report of them
    /// arrived, by session: what was asked, and when the allow was heard.
    ///
    /// The two hooks fire together and race to every cctop, so the report
    /// that the prompt went up can land after the news that it was answered.
    /// Left to stand, it would be held for the grace, then shown — a row
    /// asking about a prompt that never reached the screen, for as long as
    /// the tool it allowed runs. See [`HOOK_RACE`].
    hook_allowed: HashMap<String, (Option<String>, std::time::Instant)>,
}

/// How long after a `yolo-hook` allow a report of the same prompt is taken to
/// be the one it answered.
///
/// Both hooks are bounded by [`DEADLINE`], so their events land within it of
/// each other; four times that covers a busy machine. Not longer, because a
/// genuine second prompt in the same words would be answered by the same hook
/// anyway, and one that was not — YOLO switched off in between — should show.
const HOOK_RACE: std::time::Duration = std::time::Duration::from_millis(1000);

impl Reports {
    /// A fresh memory, seeded with the pid claims some cctop last wrote.
    ///
    /// The file is the only way to know where a session runs when it has said
    /// nothing since this process started — which is exactly what a session
    /// blocked on a question does. See [`load_claims`].
    pub fn new() -> Reports {
        Reports {
            claims: load_claims(),
            ..Default::default()
        }
    }

    /// Fold one event into what each session is taken to be doing.
    ///
    /// Returns `(lifecycle, claims_moved)`: lifecycle when the set of sessions
    /// itself changed — a reason to rescan now rather than at the next poll —
    /// and claims_moved when the session→pid map did, which is the caller's
    /// cue to republish it (`save_claims`, and the worker/loader that resolves
    /// pids by it).
    pub fn observe(&mut self, event: &Event) -> (bool, bool) {
        let lifecycle = event.reported.signal.is_lifecycle();
        let mut moved = false;
        match event.reported.signal {
            // Nothing more will be said about it, and leaving the last signal
            // behind would have the row claim a state forever.
            Signal::Ended => {
                self.hooked.remove(&event.session_id);
                self.asking_agents.remove(&event.session_id);
                self.sandboxes.remove(&event.session_id);
                moved = self.claims.remove(&event.session_id).is_some();
            }
            _ => {
                if let Some(sandbox) = &event.sandbox {
                    self.sandboxes
                        .insert(event.session_id.clone(), sandbox.clone());
                }
                if !event.pids.is_empty() && self.claims.get(&event.session_id) != Some(&event.pids)
                {
                    self.claims
                        .insert(event.session_id.clone(), event.pids.clone());
                    moved = true;
                }
                let reported = self.after_yolo_hook(event);
                let mut reported = still_asking(
                    &mut self.asking_agents,
                    &event.session_id,
                    event.agent.clone(),
                    reported,
                );
                // Claude Code follows a `PermissionRequest` with a
                // `permission_prompt` notification for the same question, and
                // only the first says what the question is. Without this the
                // second wiped it a few seconds in — exactly when the prompt
                // stopped being provisional and was shown.
                if reported.signal == Signal::NeedsInput
                    && reported.ask.is_none()
                    && let Some(before) = self.hooked.get(&event.session_id)
                    && before.signal == Signal::NeedsInput
                {
                    reported.ask = before.ask.clone();
                    reported.call = reported.call.take().or_else(|| before.call.clone());
                    // And what kind of prompt it is: the notification knows
                    // even less about that than about the question's words.
                    reported.question = reported.question || before.question;
                }
                self.hooked.insert(event.session_id.clone(), reported);
            }
        }
        // A working claim that nothing has confirmed for a quarter of an hour
        // is dropped rather than believed: see [`Reported::is_current`]. Swept
        // here because this is the only place the map grows, and a session that
        // was killed mid-turn will never send the event that would clear it.
        self.hooked.retain(|_, reported| reported.is_current());
        // A chain outlives its usefulness exactly when the report it came with
        // does, and a session that has been swept must stop claiming a pid —
        // otherwise a reused pid would be handed to a session that is gone. A
        // waiting session is never swept, which is the case this exists for.
        let before = self.claims.len();
        self.claims.retain(|id, _| self.hooked.contains_key(id));
        self.sandboxes.retain(|id, _| self.hooked.contains_key(id));
        (lifecycle, moved || self.claims.len() != before)
    }

    /// What `event` says once `cctop yolo-hook`'s allows are taken into
    /// account: see [`Reports::hook_allowed`].
    ///
    /// The allow is recognised by its ask. [`envelope`] gives an event one only
    /// when it is a permission prompt, so a working event carrying one is the
    /// allow and nothing else; the ask is kept to match the prompt it answered.
    fn after_yolo_hook(&mut self, event: &Event) -> Reported {
        let mut reported = event.reported.clone();
        self.hook_allowed
            .retain(|_, (_, at)| at.elapsed() < HOOK_RACE);
        let ask = || reported.ask.clone().filter(|a| !a.is_empty());
        if reported.signal == Signal::Busy && reported.ask.is_some() {
            // Remembered only when it came first. Arriving second, it simply
            // replaces the report it answers, and a memory of it would swallow
            // the next prompt in the same words.
            let answers = |held: &Reported| {
                held.signal == Signal::NeedsInput
                    && held.ask.clone().filter(|a| !a.is_empty()) == ask()
            };
            let held = match &event.agent {
                Some(agent) => self
                    .asking_agents
                    .get(&event.session_id)
                    .and_then(|open| open.get(agent)),
                None => self.hooked.get(&event.session_id),
            };
            if !held.is_some_and(answers) {
                self.hook_allowed
                    .insert(event.session_id.clone(), (ask(), reported.at));
            }
            reported.ask = None;
        } else if reported.signal == Signal::NeedsInput
            && reported.provisional
            && self
                .hook_allowed
                .get(&event.session_id)
                .is_some_and(|(allowed, _)| *allowed == ask())
        {
            // The report of a prompt the hook has already answered: the tool
            // is running, which is what the allow said.
            self.hook_allowed.remove(&event.session_id);
            reported.signal = Signal::Busy;
            reported.provisional = false;
            reported.ask = None;
            reported.question = false;
        }
        reported
    }

    /// Promote every held permission prompt whose grace has expired, and say
    /// whether any had.
    ///
    /// Auto mode's answer arrives as its own event and replaces the report, so
    /// a prompt still sitting here when the grace runs out is one a person has
    /// to answer. Cleared rather than re-tested every tick: once a prompt is
    /// real it stays real.
    pub fn promote_matured(&mut self) -> bool {
        let mut matured = false;
        for reported in self.hooked.values_mut() {
            if reported.provisional && reported.at.elapsed() >= PERMISSION_GRACE {
                reported.provisional = false;
                matured = true;
            }
        }
        matured
    }

    /// Stamp `session` with the machine it works on, when its hooks said.
    ///
    /// Beside [`Session::apply_reports`](crate::session::Session::apply_reports)
    /// at both of its callers, and for the same reason: rows are rebuilt from
    /// transcripts, which know nothing of how the agent was launched.
    pub fn stamp_sandbox(&self, session: &mut crate::session::Session) {
        if let Some(sandbox) = self.sandboxes.get(&session.session_id) {
            session.sandbox = Some(sandbox.clone());
        }
    }

    /// Whether any session's hook last said it is waiting on the person.
    pub fn any_waiting(&self) -> bool {
        self.hooked.values().any(|r| r.signal == Signal::NeedsInput)
    }

    /// The report `session_id`'s hooks last made, however old, if they made one.
    pub fn report(&self, session_id: &str) -> Option<&Reported> {
        if let Some(reported) = self.hooked.get(session_id) {
            return Some(reported);
        }
        // Gemini CLI reports a full session id, but names the chat file it
        // writes — which is the only identity cctop's rows have, because
        // resuming reuses the id across disjoint files — after the *first
        // eight characters* of it. Without this last step every Gemini event
        // lands on no row at all.
        let tail = gemini_id_tail(session_id)?;
        self.hooked
            .iter()
            .find(|(id, _)| id.starts_with(tail))
            .map(|(_, reported)| reported)
    }
}

/// The report a session's hooks last made, however old, if they made one.
///
/// Gemini CLI reports a full session id, but names the chat file it writes —
/// which is the only identity cctop's rows have, because resuming reuses the
/// id across disjoint files — after the *first eight characters* of it.
/// Without this fallback every Gemini event lands on no row at all.
///
/// `None` for every other harness's ids, which is what keeps the tail from
/// matching on the end of a uuid that happens to line up: only a stem shaped
/// like Gemini's is looked up loosely, and only ever against a full id's
/// prefix.
pub fn gemini_id_tail(session_id: &str) -> Option<&str> {
    let tail = session_id.strip_prefix("session-")?.rsplit_once('-')?.1;
    (tail.len() == 8 && tail.chars().all(|c| c.is_ascii_alphanumeric())).then_some(tail)
}

/// What a session should be taken to be doing after `reported`, given the
/// questions its subagents are still waiting on.
///
/// A subagent's question stands until *that* subagent says something else —
/// its tool running, or being denied, or it stopping — however busy the
/// agents beside it are. While any stands, the session is asking: the most
/// recent open question is what it reports, so the tab stays lit and the
/// bell's grace period is the question's own.
fn still_asking(
    asking_agents: &mut HashMap<String, HashMap<String, Reported>>,
    session: &str,
    agent: Option<String>,
    reported: Reported,
) -> Reported {
    let open = asking_agents.entry(session.to_string()).or_default();
    match agent {
        Some(agent) => match reported.signal {
            Signal::NeedsInput => {
                open.insert(agent, reported.clone());
            }
            _ => {
                open.remove(&agent);
            }
        },
        // The session's own turn ending. A subagent it was waiting on cannot
        // still be asking once it has — a prompt dismissed with Esc ends the
        // turn and sends nothing from the subagent — and a question kept past
        // that would light the tab for good, since a question is never aged
        // out.
        //
        // ponytail: a *background* subagent asking across its parent's `Stop`
        // is taken to have been answered.
        None if reported.signal == Signal::Idle => open.clear(),
        None => {}
    }
    if reported.signal == Signal::NeedsInput {
        return reported;
    }
    match open.values().max_by_key(|asked| asked.at) {
        Some(asked) => asked.clone(),
        None => {
            asking_agents.remove(session);
            reported
        }
    }
}

/// How much a session asks before it acts.
///
/// Only a live agent's own hooks can answer this — it is a setting, not an
/// event, and no transcript records it. That is what makes it worth a column:
/// an agent running with its permission prompts turned off looks exactly like
/// every other agent right up until it does something you would have refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    /// Asks before anything it is not already allowed to do.
    Ask,
    /// Files are written without asking; everything else still asks.
    AcceptEdits,
    /// Reading and planning only — it cannot act at all yet.
    Plan,
    /// Asks about nothing. `--dangerously-skip-permissions`.
    Bypass,
}

impl Permission {
    /// Parse the mode a harness reported, or `None` for one cctop has not seen.
    ///
    /// An unknown mode is deliberately not folded into [`Permission::Ask`]: a
    /// future mode is at least as likely to be a *looser* one, and quietly
    /// drawing it as the safe end is the wrong way to be wrong about this.
    /// The spellings cctop's own [`label`](Permission::label) uses are accepted
    /// too, so a mode that has been through `--json` and back — which is how a
    /// remote machine's rows arrive — reads as the mode it left as.
    pub fn parse(word: &str) -> Option<Permission> {
        match word {
            "default" | "ask" => Some(Permission::Ask),
            "acceptEdits" | "auto" | "edits" => Some(Permission::AcceptEdits),
            "plan" => Some(Permission::Plan),
            "bypassPermissions" | "dontAsk" | "BYPASS" => Some(Permission::Bypass),
            _ => None,
        }
    }

    /// A word for the column, short enough for it.
    pub fn label(self) -> &'static str {
        match self {
            Permission::Ask => "ask",
            Permission::AcceptEdits => "edits",
            Permission::Plan => "plan",
            Permission::Bypass => "BYPASS",
        }
    }

    /// Whether this mode lets the agent act without asking at all — the case
    /// the column exists to make visible.
    pub fn is_unrestricted(self) -> bool {
        matches!(self, Permission::Bypass)
    }
}

/// What a `Notification` is actually about.
///
/// The event is not one thing. Claude Code raises it for a permission prompt,
/// for a login succeeding, for an MCP server answering an elicitation, and for
/// the nudge it sends when a finished turn has sat there for a minute — all
/// down the same hook, told apart only by `notification_type`.
///
/// Reading every one of them as a held question is what makes a session that
/// merely finished go amber and stay amber: `idle_prompt` arrives *after*
/// `Stop`, so the nudge overwrites the truth with something more alarming than
/// it and nothing corrects it until the next turn.
///
/// The default is still [`Signal::NeedsInput`]. An unrecognised type — a newer
/// Claude Code's, or none at all from one too old to send the field — is more
/// likely to be a prompt worth showing than not, and that is the behaviour
/// every version had before this field was read.
fn notification_signal(kind: &str) -> Option<Signal> {
    match kind {
        // The turn ended a minute ago and nobody is blocked: this is the same
        // fact `Stop` already reported, said again more loudly.
        "idle_prompt" => Some(Signal::Idle),
        // An MCP elicitation was answered, which is the answer arriving rather
        // than the question — the agent is moving again.
        "elicitation_response" => Some(Signal::Busy),
        // None of these say anything about whether *this* agent is waiting on
        // you: a login, a computer-use session changing hands, an MCP server
        // acknowledging, or another session finishing. Dropped rather than
        // mapped, so a true state already on the row survives them.
        //
        // `agent_needs_input` and `agent_completed` are the pair that made this
        // list necessary. Both are raised by whichever session has agent view
        // open, *about a background session that is not it* — and the payload
        // carries the watcher's `session_id`, not the waiting one's. Reading
        // either as news about the sender turns the session that is merely
        // watching amber. The session actually blocked reports for itself, over
        // its own hooks, on the row it belongs to.
        //
        // `plugin_notification` is any installed plugin saying something, and
        // `auth_storage_failure` is a credential that could not be saved:
        // neither holds the turn. Both came out of Claude Code 2.1.300's own
        // list of types, after a row with plugins installed kept going amber
        // for no prompt anyone could find.
        "auth_success"
        | "auth_storage_failure"
        | "plugin_notification"
        | "computer_use_enter"
        | "computer_use_exit"
        | "elicitation_complete"
        | "agent_needs_input"
        | "agent_completed"
        | "push_notification" => None,
        // `permission_prompt`, `worker_permission_prompt`, the elicitation
        // dialogs, and whatever comes next: the case the amber exists for.
        _ => Some(Signal::NeedsInput),
    }
}

/// Map an event name onto what it means. Unknown names are dropped rather than
/// guessed at, so a newer agent can add events without confusing this one.
///
/// Every harness's vocabulary lands in the same match. They do not collide —
/// Claude Code and Gemini CLI capitalise their events, Cursor lower-cases its,
/// Codex and OpenCode use hyphens and dots — so one table can hold the lot, and
/// a fact means the same thing to the UI whichever agent reported it.
///
/// `notification` is the `notification_type` of a `Notification`, and empty for
/// every other event.
pub(super) fn signal_of(event: &str, notification: &str) -> Option<Signal> {
    match event {
        // The turn is over. This is the event the whole feature exists for:
        // nothing in a transcript distinguishes it from the agent still working.
        // Codex's `notify` fires once per turn and says only this; Gemini CLI
        // calls the end of its agent loop `AfterAgent`; Cursor and OpenCode say
        // it in their own spelling.
        // `StopFailure` is the same fact arrived at badly: the turn is over
        // because the API refused it — rate limited, overloaded, out of
        // credit — and the prompt is the user's again either way. Told apart
        // from `Stop` only in that it is the one Claude Code fires instead,
        // never as well, so a turn that dies this way has no other ending.
        "Stop" | "StopFailure" | "agent-turn-complete" | "AfterAgent" | "stop"
        | "session.idle" => Some(Signal::Idle),
        // Several different facts share this event; which one is in the payload.
        "Notification" => notification_signal(notification),
        // All of these mean work has started, which answers whatever came
        // before. `SubagentStop` included: a subagent finishing tells you the
        // agent that spawned it is still going.
        //
        // `PostToolUse` is here for the permission prompt specifically. The
        // sequence is `PreToolUse`, then `Notification` because the tool needs
        // an answer, then — once you give it — the tool runs and this fires.
        // Nothing between the answer and this event says the answer happened,
        // so without it a prompt you have already dealt with keeps its tab
        // blinking until the *next* tool call or the end of the turn.
        // `PostToolUseFailure` sits here for the same reason `PostToolUse`
        // does: a tool that errored is a tool that came back. Claude Code
        // fires one or the other and never both, and erroring is an ordinary
        // way for a call to end — a grep that matched nothing, a test that
        // failed, a command that exited 1 — so leaving this out left a tool
        // call in flight for as long as the turn ran.
        // `UserPromptExpansion` is the path `UserPromptSubmit` does not cover:
        // typing `/skillname` expands into a prompt without submitting one, so
        // a session driven entirely by slash commands never reported starting
        // work at all.
        //
        // `PostToolBatch` fires once after every call in a batch has resolved,
        // where `PostToolUse` fires once per call and concurrently. It is the
        // moment the model is about to be asked again — the same "still going"
        // fact, arrived at once instead of five times.
        //
        // `SubagentStart` pairs with the `SubagentStop` already here. Without
        // it the Subagents panel learned of a subagent only when it ended,
        // which is the half of its life nobody needs to watch.
        //
        // `ElicitationResult` closes out the `Elicitation` below: the answer
        // has been given and the MCP call is moving again.
        //
        // `TaskCreated` and `TaskCompleted` are the agent's own plan changing
        // under it. Neither is a state cctop draws differently, but both happen
        // only while an agent is working, and both are moments a turn that
        // looked stalled is demonstrably not.
        "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" | "SubagentStop"
        | "UserPromptExpansion" | "PostToolBatch" | "SubagentStart"
        | "ElicitationResult" | "TaskCreated" | "TaskCompleted"
        // Gemini CLI: a prompt submitted, and a tool call coming back.
        | "BeforeAgent" | "AfterTool"
        // OpenCode: the same, from the plugin.
        | "tool.execute.after"
        // Cursor: the same two moments, lower-cased.
        | "beforeSubmitPrompt" | "subagentStop" => Some(Signal::Busy),
        // A tool call has started, which is the same "working" fact with one
        // more thing known about it: it is the state a permission prompt is
        // held in. See [`Signal::Acting`].
        //
        // Cursor's shell command is the one tool call it reports, which is
        // enough for the cue — and the one harness with nothing to close it out,
        // so there its turn ending is what clears it.
        "PreToolUse" | "BeforeTool" | "beforeShellExecution" | "tool.execute.before" => {
            Some(Signal::Acting)
        }
        // A held permission prompt, in the harnesses that have an event for it.
        // Cursor and Gemini both raise `Notification` the way Claude Code does,
        // and it is matched above.
        //
        // `PermissionRequest` is Claude Code's, and it is the exact fact rather
        // than an inference: it fires the moment the prompt goes up. The
        // `Notification` for the same prompt is on a six-second timer, by which
        // point cctop has usually already guessed it from a tool call held over
        // a still screen — a guess that is wrong for a tool that runs long and
        // silently. See [`Signal::Acting`].
        // An MCP server asking the user something mid-task, which is a held
        // prompt of a kind cctop had no way to see: it is not a permission
        // dialog, so `PermissionRequest` never fires, and the screen goes still
        // the same way it does for a tool that runs long and quietly. A session
        // blocked on one of these looked idle.
        "permission.asked" | "PermissionRequest" | "Elicitation" => Some(Signal::NeedsInput),
        // The answer to one of those, from a harness that has no event for it.
        // OpenCode 2 raises nothing when a prompt is answered — the request
        // simply stops being pending — so its plugin says so from the API
        // instead, which is the same closing-out a `PostToolUse` does for Claude
        // Code: without it a tab keeps asking about a question that was
        // answered minutes ago, and the only thing that clears it is the tool
        // call that follows, or the end of the turn.
        "permission.replied" | "permission.rejected" => Some(Signal::Busy),
        // Auto mode refused a tool call. The turn continues — the model is told
        // it may retry — so this is not the end of anything, but it is the one
        // event that says a refusal happened at all. Without it the only trace
        // is a tool that never ran.
        "PermissionDenied" => Some(Signal::Busy),
        // cctop's own, sent by [`announce_answer`] when the page answered a
        // prompt. Allowed, the tool runs and its own events follow. Denied is
        // Esc, which ends the turn and sends nothing at all — so without this
        // the row asked a question that was no longer on screen, and the next
        // Allow typed a `1` into the composer.
        ANSWERED_ALLOW | YOLO_ALLOWED => Some(Signal::Busy),
        ANSWERED_DENY => Some(Signal::Idle),
        // Compaction is the one kind of work worth naming separately: the
        // context panel is about to lurch, and it is not the agent stalling.
        "PreCompact" | "PreCompress" | "preCompact" | "session.compacted" => {
            Some(Signal::Compacting)
        }
        // And the other end of it: compaction is over and the agent is moving
        // again. Claude Code also raises a `SessionStart` after compacting,
        // which says the same thing — but `SessionStart` is `Started`, and a
        // session that has merely compacted is not one that has just begun.
        "PostCompact" => Some(Signal::Busy),
        // A teammate in an agent team finishing its turn. The team's own
        // `Stop`, one member at a time — and the moment a team stops being
        // something to watch and starts being something to answer.
        "TeammateIdle" => Some(Signal::Idle),
        // Everything below happens only while a session is alive and doing
        // something, and none of it is a state cctop draws differently. They
        // are here because a turn can otherwise go minutes without an event:
        // a long answer with no tool calls in it raises nothing at all between
        // `UserPromptSubmit` and `Stop`, and a session with nothing to report
        // is one whose liveness falls back to guessing at a still screen.
        //
        // `MessageDisplay` is the frequent one — once per batch of streamed
        // lines, several times per message — and the only event that fires
        // *during* an answer. It is display-only and cctop returns no
        // `displayContent`, so what is on screen is untouched.
        //
        // `CwdChanged` and `DirectoryAdded` also move the ground under a row:
        // the project a session is drawn against comes from its working
        // directory, and an agent that `cd`s has been drawn in the wrong place
        // until its next transcript write said otherwise.
        //
        // `FileChanged` fires only once something has named a file to watch,
        // which cctop does not — so it is registered against the day something
        // does, and costs nothing until then.
        "MessageDisplay" | "InstructionsLoaded" | "CwdChanged" | "DirectoryAdded"
        | "ConfigChange" | "FileChanged" | "WorktreeRemove" => Some(Signal::Busy),
        // Claude Code fires this only under `--init-only`, `--init` or
        // `--maintenance`, never on a normal start. A session that begins this
        // way is still a session beginning.
        "Setup" => Some(Signal::Started),
        "SessionStart" | "sessionStart" | "session.created" => Some(Signal::Started),
        "SessionEnd" | "sessionEnd" | "session.deleted" => Some(Signal::Ended),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Five vocabularies share one table, so the thing that can go wrong is two
    /// harnesses spelling different facts the same way.
    #[test]
    fn no_two_harnesses_disagree_about_a_shared_event_name() {
        let mut seen: Vec<(&str, Signal)> = Vec::new();
        for event in CLAUDE_EVENTS
            .iter()
            .chain(GEMINI_EVENTS)
            .chain(CURSOR_EVENTS)
        {
            let Some(signal) = signal_of(event, "") else {
                panic!("{event} is installed but means nothing to cctop");
            };
            if let Some((_, other)) = seen.iter().find(|(name, _)| name == event) {
                assert_eq!(*other, signal, "{event} means two things");
            }
            seen.push((event, signal));
        }
    }

    #[test]
    fn an_mcp_elicitation_is_a_session_waiting_on_you() {
        assert_eq!(signal_of("Elicitation", ""), Some(Signal::NeedsInput));
        assert_eq!(signal_of("ElicitationResult", ""), Some(Signal::Busy));
        assert!(CLAUDE_EVENTS.contains(&"Elicitation"));
    }

    /// A permission prompt is the one exchange with no event of its own for the
    /// *answer*, so the tool running afterwards has to carry that news.
    ///
    /// The prompt itself is reported three times over: `PreToolUse` says a tool
    /// is in flight, which is enough for the tab to blink as soon as the screen
    /// goes still; `PermissionRequest` says outright that Claude Code is asking;
    /// and the `Notification` confirms it six seconds later.
    #[test]
    fn an_answered_permission_prompt_stops_asking() {
        assert_eq!(signal_of("PreToolUse", ""), Some(Signal::Acting));
        assert_eq!(signal_of("PermissionRequest", ""), Some(Signal::NeedsInput));
        assert_eq!(signal_of("Notification", ""), Some(Signal::NeedsInput));
        assert_eq!(
            signal_of("PostToolUse", ""),
            Some(Signal::Busy),
            "without this the prompt stays 'asking' until the next tool or the \
             end of the turn"
        );
        for event in ["PermissionRequest", "PostToolUse"] {
            assert!(
                CLAUDE_EVENTS.contains(&event),
                "{event} has to be installed to arrive at all"
            );
        }
    }

    /// A permission prompt is held back; every other question is not.
    ///
    /// Claude Code's auto mode puts the request to a model first, which takes a
    /// few seconds, so the hook fires for prompts nobody is ever asked. An
    /// elicitation and a notification have no such second opinion coming and
    /// are shown at once.
    #[test]
    fn a_permission_prompt_waits_and_the_other_questions_do_not() {
        let of = |line: &str| parse(line).expect("a parseable event").reported;
        let event = |name: &str| format!(r#"{{"session_id":"abc","event":"{name}","cwd":"/tmp"}}"#);

        let asked = of(&event("PermissionRequest"));
        assert_eq!(asked.signal, Signal::NeedsInput);
        assert!(asked.provisional, "the prompt was not held");
        assert!(
            !asked.is_settled(),
            "a prompt shown the instant it was raised"
        );

        // Held, not lost: it becomes real when the grace runs out with no
        // answer having arrived.
        let matured = Reported {
            call: None,
            at: std::time::Instant::now() - (PERMISSION_GRACE + std::time::Duration::from_secs(1)),
            ..asked
        };
        assert!(matured.is_settled(), "a prompt held past its grace");

        for name in ["Elicitation", "Notification"] {
            let other = of(&event(name));
            assert!(
                other.is_settled(),
                "{name} was held back, and nothing is going to answer it for you"
            );
        }
        // And the answer itself, which is what replaces a held prompt before
        // anyone is told about it.
        let denied = of(&event("PermissionDenied"));
        assert_eq!(denied.signal, Signal::Busy);
        assert!(denied.is_settled());
    }

    /// The states that stick, and the events that are the only thing that ends
    /// them.
    ///
    /// Regression, and the reason a session could sit on a state it was not in.
    /// Claude Code fires `PostToolUse` only for a tool that *succeeded* and
    /// `Stop` only for a turn that ended *cleanly*, and cctop asked for neither
    /// partner: so a grep that matched nothing left a tool call in flight — read
    /// as a held permission prompt the moment the screen went still — and a
    /// rate-limited turn left the session working with nobody working.
    #[test]
    fn a_tool_that_failed_and_a_turn_that_died_both_end() {
        assert_eq!(
            signal_of("PostToolUseFailure", ""),
            Some(Signal::Busy),
            "a tool that errored is a tool that came back"
        );
        assert_eq!(
            signal_of("StopFailure", ""),
            Some(Signal::Idle),
            "the turn is over, badly, and the prompt is the user's again"
        );
        for event in ["PostToolUseFailure", "StopFailure"] {
            assert!(
                CLAUDE_EVENTS.contains(&event),
                "{event} has to be installed to arrive at all"
            );
        }
    }

    /// A working claim goes stale; a waiting one does not.
    ///
    /// The events that would close out "working" — the tool coming back, the
    /// turn ending — are exactly the ones that never arrive when a session is
    /// killed, interrupted, or has its terminal closed. A held question has no
    /// such partner and no shelf life: somebody back from lunch should still
    /// find the tab that is asking.
    #[test]
    fn a_stale_claim_to_be_working_stops_being_believed() {
        let aged = |signal: Signal, ago: std::time::Duration| Reported {
            call: None,
            provisional: false,
            ask: None,
            question: false,
            signal,
            cwd: "/w".into(),
            permission: None,
            at: std::time::Instant::now() - ago,
        };
        let stale = WORKING_TTL + std::time::Duration::from_secs(1);
        for signal in [
            Signal::Busy,
            Signal::Acting,
            Signal::Compacting,
            Signal::Started,
        ] {
            assert!(aged(signal, std::time::Duration::ZERO).is_current());
            assert!(
                !aged(signal, stale).is_current(),
                "{} was believed after {stale:?} of silence",
                signal.label()
            );
        }
        for signal in [Signal::NeedsInput, Signal::Idle] {
            assert!(
                aged(signal, stale).is_current(),
                "{} expired, and nothing would have said it again",
                signal.label()
            );
        }
    }

    /// `Notification` is several unrelated facts sharing one event, and reading
    /// them all as a held question is what leaves a finished turn amber.
    #[test]
    fn only_a_notification_that_blocks_the_agent_asks_for_you() {
        let notification = |kind: &str| signal_of("Notification", kind);

        // The 60-second nudge, which arrives *after* `Stop` and would otherwise
        // overwrite a finished turn with something more alarming than it is.
        assert_eq!(notification("idle_prompt"), Some(Signal::Idle));
        // A real block, in this session.
        assert_eq!(
            notification("worker_permission_prompt"),
            Some(Signal::NeedsInput)
        );
        assert_eq!(notification("permission_prompt"), Some(Signal::NeedsInput));
        // An elicitation answered is the answer, not the question.
        assert_eq!(notification("elicitation_response"), Some(Signal::Busy));
        // These say nothing about whether the agent is waiting, so they must not
        // be allowed to overwrite what is already known about it.
        // `agent_needs_input` and `agent_completed` are the pair worth naming:
        // both are raised by the session watching a *background* one, and carry
        // the watcher's session id rather than the waiting session's. Read as
        // news about the sender, the first turns a session that is only
        // watching amber — while the session actually blocked reports for
        // itself, on its own row.
        for quiet in [
            "auth_success",
            "auth_storage_failure",
            "plugin_notification",
            "computer_use_enter",
            "computer_use_exit",
            "elicitation_complete",
            "agent_needs_input",
            "agent_completed",
            "push_notification",
        ] {
            assert_eq!(notification(quiet), None, "{quiet} claimed a state");
        }
        // A type cctop has never seen, and a Claude Code too old to send one at
        // all, both keep the behaviour every version had before this was read.
        assert_eq!(notification("something_new"), Some(Signal::NeedsInput));
        assert_eq!(notification(""), Some(Signal::NeedsInput));
    }

    /// The lifecycle events are the ones that change which rows exist, and the
    /// table has to be told to go and look rather than wait for its poll.
    #[test]
    fn only_the_lifecycle_events_ask_for_a_rescan() {
        assert!(signal_of("SessionStart", "").unwrap().is_lifecycle());
        assert!(signal_of("SessionEnd", "").unwrap().is_lifecycle());
        assert!(!signal_of("PreToolUse", "").unwrap().is_lifecycle());
        assert!(!signal_of("Stop", "").unwrap().is_lifecycle());
        // Compaction is work, not a stalled agent, and a finished subagent means
        // the one that spawned it is still going.
        assert!(signal_of("PreCompact", "").unwrap().is_working());
        assert!(signal_of("SubagentStop", "").unwrap().is_working());
        assert!(!signal_of("Stop", "").unwrap().is_working());
    }

    /// The moment a prompt is answered, which for OpenCode 2 cctop has to be
    /// told: a tab that keeps asking about a question that was answered two
    /// minutes ago is worse than one that never noticed.
    #[test]
    fn an_answered_prompt_says_the_agent_is_working_again() {
        assert_eq!(
            signal_of("permission.replied", ""),
            Some(Signal::Busy),
            "the answer closes the prompt out, the way a tool call does"
        );
        assert_eq!(signal_of("permission.rejected", ""), Some(Signal::Busy));
        assert_eq!(signal_of("permission.asked", ""), Some(Signal::NeedsInput));
    }

    /// The hook's allow, landing on either side of the observer's report of
    /// the same prompt, leaves the session working — and a prompt it did not
    /// answer still asks.
    #[test]
    fn a_prompt_the_yolo_hook_allowed_never_reads_as_asking() {
        let raised = |ask: &str| {
            parse(&format!(
                r#"{{"session_id":"s","event":"PermissionRequest","cwd":"/w","ask":"{ask}"}}"#
            ))
            .unwrap()
        };
        let allowed = |ask: Option<&str>| {
            let line = yolo_allowed_line("s", None, ask);
            parse(std::str::from_utf8(&line).unwrap()).unwrap()
        };

        // The allow first, then the report it raced.
        let mut reports = Reports::default();
        reports.observe(&allowed(Some("Bash: ls")));
        reports.observe(&raised("Bash: ls"));
        assert_eq!(reports.report("s").unwrap().signal, Signal::Busy);
        // Used up: the next prompt in the same words asks.
        reports.observe(&raised("Bash: ls"));
        assert_eq!(reports.report("s").unwrap().signal, Signal::NeedsInput);

        // The report first, then the allow.
        let mut reports = Reports::default();
        reports.observe(&raised("Bash: ls"));
        reports.observe(&allowed(Some("Bash: ls")));
        let now = reports.report("s").unwrap();
        assert_eq!((now.signal, now.ask.as_deref()), (Signal::Busy, None));
        // Nothing remembered from an allow that came second.
        reports.observe(&raised("Bash: ls"));
        assert_eq!(reports.report("s").unwrap().signal, Signal::NeedsInput);

        // A prompt in other words is not the one allowed.
        let mut reports = Reports::default();
        reports.observe(&allowed(Some("Bash: ls")));
        reports.observe(&raised("Bash: rm -rf /"));
        assert_eq!(reports.report("s").unwrap().signal, Signal::NeedsInput);

        // A prompt that named nothing, allowed as nothing.
        let mut reports = Reports::default();
        reports.observe(&allowed(None));
        reports.observe(
            &parse(r#"{"session_id":"s","event":"PermissionRequest","cwd":"/w"}"#).unwrap(),
        );
        assert_eq!(reports.report("s").unwrap().signal, Signal::Busy);
    }

    /// A subagent's prompt the hook allowed is closed out by the allow, so the
    /// session does not go on asking on its behalf.
    #[test]
    fn a_subagents_prompt_the_yolo_hook_allowed_is_closed() {
        let mut reports = Reports::default();
        reports.observe(
            &parse(
                r#"{"session_id":"s","event":"PermissionRequest","cwd":"/w","agent_id":"sub","ask":"Bash: ls"}"#,
            )
            .unwrap(),
        );
        assert_eq!(reports.report("s").unwrap().signal, Signal::NeedsInput);
        let line = yolo_allowed_line("s", Some("sub"), Some("Bash: ls"));
        reports.observe(&parse(std::str::from_utf8(&line).unwrap()).unwrap());
        assert_eq!(reports.report("s").unwrap().signal, Signal::Busy);
        assert!(
            reports
                .asking_agents
                .get("s")
                .is_none_or(|open| open.is_empty())
        );
    }
}
