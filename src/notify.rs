//! Telling you when a session needs you.
//!
//! cctop is a monitor you look away from, so the one thing it owes you is a
//! nudge when an agent stops working and starts waiting. Both channels are the
//! terminal's own: `BEL`, which rmux turns into a `monitor-bell` window flag,
//! and OSC 9, which iTerm2, Ghostty, kitty, WezTerm and Windows Terminal raise
//! as a real desktop notification. Neither needs a daemon, a D-Bus connection,
//! or a crate.
//!
//! The ring is an edge, never a level. A session waiting for input is still
//! waiting for input on the next refresh, and on the one after that — ringing
//! for the state rather than the crossing would be an alarm clock, not a
//! notification. So [`Notifier`] keeps last refresh's state for each *running*
//! session and fires only where the two disagree.

use crate::session::{ActivityState, Session};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// How long the session that rang keeps its marker in the table.
///
/// Long enough to still be there when you look back at the window, short enough
/// that it doesn't linger into the next thing you do.
pub const MARK_FOR: Duration = Duration::from_secs(30);

/// What a session was doing at a refresh, as far as ringing is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Busy,
    /// The turn is over and the prompt is the user's.
    Waiting,
    /// The agent is blocked on a question and cannot go on until it is
    /// answered. Rung for separately, because "your move" and "I am stuck" are
    /// not the same news.
    Asking,
    Stopped,
}

/// Why a session rang. The two are worth different sentences: one of them is an
/// agent holding a question for you, the other is an agent that is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NeedsInput,
    /// Blocked on a permission prompt or an elicitation.
    Asking,
    Stopped,
}

/// The session that rang, kept around so the bell can be traced to a row.
#[derive(Debug, Clone)]
pub struct Rang {
    pub key: String,
    pub label: String,
    pub reason: Reason,
    pub at: Instant,
}

/// How many ringing sessions are remembered at once.
///
/// One bell per refresh however many crossed in it is the right amount of
/// *noise*; the queue is what keeps the "+3 more" in that ring findable
/// rather than a count with no address. Past this the table itself is the
/// answer — more sessions finishing at once than this is a fleet event,
/// not a notification.
const MAX_RANG: usize = 16;

#[derive(Default)]
pub struct Notifier {
    /// Opt-in, and persisted: an unasked-for bell in a shared office is worse
    /// than a missed one.
    pub enabled: bool,
    /// Where crossings are POSTed, from `$CCTOP_NOTIFY_URL` — the same webhook
    /// `cctop serve --notify` answers to. Deliberately independent of
    /// `enabled`: the bell is for the person at the keyboard and the webhook
    /// is for the one who is not, so each gets its own switch.
    pub webhook: Option<String>,
    /// The link a running `B`-serve hands out — `origin/?t=token` — kept so a
    /// webhook can carry a link that opens the session it is about. `None`
    /// while nothing is being served; the dashboard sets it on each refresh.
    pub link_base: Option<String>,
    /// Last state of every session that was running at the previous refresh,
    /// with the label to name it by. The label is carried because a session
    /// that exits between refreshes still has to be nameable, and by then the
    /// only thing left of it is this entry.
    watched: HashMap<String, (State, String)>,
    /// The sessions that rang, oldest first. An entry leaves when the session
    /// goes back to work — the definition of answered — or outlives
    /// [`MARK_FOR`]; being *looked at* hides the footer but keeps the row's
    /// marker, so `b` can still land on it.
    recent: VecDeque<Rang>,
    /// Which quota windows were saturated at the last reading, keyed by
    /// `provider/profile/window` — the edge the quota ring fires on, and the
    /// only state that edge needs.
    quota_limited: HashMap<String, bool>,
    /// What this refresh's bell will say, one clause per kind of news —
    /// sessions that crossed, alerts that fired, quota that freed up.
    ///
    /// Queued rather than rung where each is found, because they are found
    /// in different places on the same pass of the event loop, and each
    /// ringing for itself put two bells and two desktop notifications on the
    /// screen for one moment. [`Notifier::ring_pending`] says all of it once.
    pending: Vec<String>,
}

impl Notifier {
    pub fn new(enabled: bool) -> Self {
        Notifier {
            enabled,
            webhook: std::env::var("CCTOP_NOTIFY_URL")
                .ok()
                .filter(|url| !url.is_empty()),
            ..Default::default()
        }
    }

    /// Fold a refresh into the state machine, ringing for whatever crossed.
    ///
    /// Runs whether or not notifications are enabled: the machine has to stay
    /// warm. Seeding it only on `n` would mean the first refresh after the
    /// toggle sees every idle session as a fresh transition and rings for all
    /// of them at once.
    pub fn observe(&mut self, sessions: &[Session]) {
        self.recent.retain(|r| r.at.elapsed() < MARK_FOR);
        let mut crossed: Vec<Rang> = Vec::new();
        // The same crossings, for the webhook — kept as sessions because the
        // payload wants fields a `Rang` deliberately does not carry.
        let mut posts: Vec<(&'static str, &Session)> = Vec::new();
        // Only running sessions are tracked, which is what keeps this cheap:
        // the map holds a handful of entries, not one per transcript ever
        // written, and only those few need a formatted key per refresh.
        let mut next = HashMap::with_capacity(self.watched.len());
        for session in sessions.iter().filter(|s| s.is_running()) {
            let key = session.key();
            let state = state_of(session);
            let label = session.display_label().to_string();
            // Removing as we go leaves `watched` holding exactly the sessions
            // that were running last time and are not running now.
            // Only out of `Busy`, so answering a permission prompt and having
            // the turn end a moment later is one ring rather than two: the
            // second crossing is `Asking` to `Waiting`, and it is the same turn
            // you have already been told about.
            if let Some((State::Busy, _)) = self.watched.remove(&key)
                && let Some(reason) = reason_for(state)
            {
                crossed.push(Rang {
                    key: key.clone(),
                    label: label.clone(),
                    reason,
                    at: Instant::now(),
                });
                // A stopped session is announced by its exit; the serve's
                // webhook draws the same line, so only the two waits POST.
                let event = match reason {
                    Reason::NeedsInput => Some("waiting"),
                    Reason::Asking => Some("asking"),
                    Reason::Stopped => None,
                };
                if let Some(event) = event {
                    posts.push((event, session));
                }
            }
            // Back to work is the definition of answered, however it happened —
            // through cctop, or in the terminal the agent actually lives in.
            if state == State::Busy {
                self.recent.retain(|r| r.key != key);
            }
            next.insert(key, (state, label));
        }

        for (key, (state, label)) in self.watched.drain() {
            // A session that was already waiting when it exited has rung once
            // already; ringing again for the exit would double-report the same
            // turn ending.
            if state == State::Busy {
                crossed.push(Rang {
                    key,
                    label,
                    reason: Reason::Stopped,
                    at: Instant::now(),
                });
            }
        }
        self.watched = next;

        if let Some(target) = &self.webhook {
            for (event, session) in posts {
                self.post_session(target.clone(), event, session);
            }
        }

        if !self.enabled || crossed.is_empty() {
            return;
        }
        // One bell for the refresh, no matter how many sessions finished in it:
        // three bells in a row is noise, and the count says the same thing.
        let extra = crossed.len() - 1;
        let rang = crossed.swap_remove(0);
        self.pending.push(crossing_text(&rang, extra));
        // Every crossing is remembered, not only the one the bell named:
        // each still needs finding, which is what the marker and `b` are for.
        self.recent.push_back(rang);
        self.recent.extend(crossed);
        while self.recent.len() > MAX_RANG {
            self.recent.pop_front();
        }
    }

    /// Add a clause to this refresh's bell, when the bell is on: `first`
    /// names the news, and `extra` counts the rest of its kind, which the
    /// bell cannot fit and the toasts already name.
    pub fn chime(&mut self, first: &str, extra: usize) {
        if self.enabled {
            self.pending.push(format!("{first}{}", more(extra)));
        }
    }

    /// Ring once for everything queued since the last ring, if anything was.
    ///
    /// The clauses are joined rather than the first one winning: the bell
    /// is one interruption, but "api is waiting" and "web passed $20" are
    /// both reasons to look, and a notification that dropped one would be
    /// the only place the user heard about it while away from the screen.
    pub fn ring_pending(&mut self) {
        if let Some(text) = bell_text(&self.pending) {
            ring(&text);
        }
        self.pending.clear();
    }

    /// Put a session on the queue as though it had just crossed — for the
    /// UI tests, which exercise `b` and the marker rather than the machine.
    #[cfg(test)]
    pub(crate) fn record_for_test(&mut self, rang: Rang) {
        self.recent.push_back(rang);
    }

    /// The session `b` should land on: the oldest ring still fresh that is
    /// not the row already selected, so pressing it again walks the queue.
    /// When nothing but the selected row is left — `b` pressed on the only
    /// ringing session — the newest is the answer, rather than "nothing".
    pub fn unanswered(&self, selected: Option<&str>) -> Option<&Rang> {
        self.recent
            .iter()
            .find(|r| r.at.elapsed() < MARK_FOR && Some(r.key.as_str()) != selected)
            .or_else(|| self.recent.iter().rev().find(|r| r.at.elapsed() < MARK_FOR))
    }

    /// True while this session's row should still carry the bell marker.
    pub fn rang_recently(&self, key: &str) -> bool {
        self.recent
            .iter()
            .any(|r| r.key == key && r.at.elapsed() < MARK_FOR)
    }

    /// The footer's reminder of who rang, or `None` when there is nothing to
    /// answer.
    ///
    /// It disappears once the selection is on that session, which is the
    /// cheapest possible definition of "answered": you are looking at it, so
    /// the footer has done its job and does not need any state of its own to
    /// know that.
    pub fn footer(&self, selected: Option<&str>) -> Option<String> {
        let rang = self.unanswered(selected)?;
        if Some(rang.key.as_str()) == selected {
            return None;
        }
        let secs = rang.at.elapsed().as_secs();
        let ago = if secs < 60 {
            format!("{secs}s")
        } else {
            format!("{}m", secs / 60)
        };
        // The others still waiting for a look, counted the way the bell's own
        // text counts them: the number is all that fits, and `b` walks them.
        let extra = self
            .recent
            .iter()
            .filter(|r| r.at.elapsed() < MARK_FOR && r.key != rang.key)
            .count();
        let more = match extra {
            0 => String::new(),
            n => format!(" · +{n} more"),
        };
        Some(format!(
            "Bell: ◉ {} · {} · {ago} ago{more} · b jumps to it",
            rang.label,
            match rang.reason {
                Reason::NeedsInput => "waiting for input",
                Reason::Asking => "needs permission",
                Reason::Stopped => "stopped",
            }
        ))
    }

    /// Fold a quota reading into the watch, returning the windows that just
    /// freed up — the ones worth telling somebody about.
    ///
    /// Same edge rule as the sessions: the first reading only establishes
    /// where things stand, so a window that has been free all along is never
    /// announced, and a saturated one rings once when it clears — not on every
    /// poll that keeps finding it clear.
    pub fn observe_quota(&mut self, quota: &crate::quota::Quota) -> Vec<String> {
        let mut freed: Vec<String> = Vec::new();
        // The state kept is one bool per window — saturated or not — because
        // the crossing is all that is news. A window missing from a reading
        // (a throttled provider answers nothing) keeps its last state, which
        // is the honest one: unknown is not "freed".
        for (provider, profiles) in [("claude", &quota.claude), ("codex", &quota.codex)] {
            for profile in profiles {
                let crate::quota::ProviderStatus::Ok(q) = &profile.status else {
                    continue;
                };
                // The provider's own flag counts too: Codex reports a held
                // limit before any window reads 100, and the crossing back is
                // the same news either way.
                let mut watch = |key: String, limited: bool, what: String| {
                    if self.quota_limited.insert(key, limited) == Some(true) && !limited {
                        freed.push(what);
                    }
                };
                watch(
                    format!("{provider}/{}", profile.profile),
                    q.limit_reached,
                    format!(
                        "{provider} · {}: the rate limit has lifted",
                        profile.profile
                    ),
                );
                for window in &q.windows {
                    watch(
                        format!("{provider}/{}/{}", profile.profile, window.label),
                        window.pct >= 100,
                        format!(
                            "{provider} · {}: the {} window is open again",
                            profile.profile, window.label
                        ),
                    );
                }
            }
        }
        freed
    }

    /// POST one crossing — through the same send `cctop serve --notify` uses,
    /// so a webhook sees the same document either way.
    fn post_session(&self, target: String, event: &'static str, session: &Session) {
        let mut body = serde_json::json!({
            "event": event,
            "session_id": session.session_id,
            "project": session.label_source,
            "title": session.title,
        });
        if let Some(base) = &self.link_base {
            body["url"] = session_link(base, &session.session_id).into();
        }
        crate::serve::notify::post(
            target,
            body.to_string(),
            serde_json::json!({ "event": event, "session": session.session_id }),
        );
    }

    /// POST a quota window opening back up, when a webhook is configured.
    ///
    /// The event has no session and no link — there is no row it is about —
    /// which is also what distinguishes it from a crossing at the far end.
    pub fn post_event(&self, event: &'static str, text: &str) {
        let Some(target) = &self.webhook else {
            return;
        };
        crate::serve::notify::post(
            target.clone(),
            serde_json::json!({ "event": event, "text": text }).to_string(),
            serde_json::json!({ "event": event }),
        );
    }
}

/// The page link for a session, built off the link a serve hands out:
/// `origin/?t=token` becomes `origin/session/<id>?t=token`.
fn session_link(base: &str, id: &str) -> String {
    match base.split_once("/?") {
        Some((origin, query)) => format!("{origin}/session/{id}?{query}"),
        None => format!("{base}session/{id}"),
    }
}

/// Which crossings are worth a bell, and what to call them.
fn reason_for(state: State) -> Option<Reason> {
    match state {
        State::Asking => Some(Reason::Asking),
        State::Waiting => Some(Reason::NeedsInput),
        State::Busy | State::Stopped => None,
    }
}

/// The session's state as the bell sees it.
///
/// Both waiting states arrive from the row, which is where a hook report has
/// already been stamped — see
/// [`App::apply_reports`](crate::ui::App::apply_reports). That matters most for
/// the permission prompt: it is the moment an agent is most obviously blocked
/// and the one a transcript cannot see at all, so before the hooks it read as
/// ordinary work and never rang.
///
/// ponytail: with no hooks installed, an agent that has simply finished its
/// turn still reads as `Working` here for the harnesses whose transcripts do
/// not mark the end of one, so the ring is missed. Ceiling accepted rather than
/// guessed at: a quiet-timer on `last_active` would fire in the middle of every
/// long reasoning turn. `cctop hook --install` is the fix, and the doctor says
/// so.
fn state_of(session: &Session) -> State {
    if !session.is_running() {
        return State::Stopped;
    }
    match session.activity_state {
        ActivityState::Asking => State::Asking,
        ActivityState::WaitingForInput => State::Waiting,
        // An API error is the agent's problem, not yet the user's: it retries,
        // and the red dot in the table is already saying so.
        ActivityState::Working | ActivityState::ApiError => State::Busy,
    }
}

/// The bell's clause for the sessions that crossed: the first by name, the
/// rest as a count.
fn crossing_text(rang: &Rang, extra: usize) -> String {
    let what = match rang.reason {
        Reason::NeedsInput => "is waiting for input",
        Reason::Asking => "needs permission",
        Reason::Stopped => "stopped",
    };
    format!("{} {what}{}", rang.label, more(extra))
}

/// The " (+2 more)" after a clause, or nothing when it stands alone.
fn more(extra: usize) -> String {
    match extra {
        0 => String::new(),
        n => format!(" (+{n} more)"),
    }
}

/// The one notification a refresh raises, or `None` when nothing rang.
fn bell_text(clauses: &[String]) -> Option<String> {
    (!clauses.is_empty()).then(|| format!("cctop: {}", clauses.join(" · ")))
}

/// Ring the terminal and raise a desktop notification.
///
/// Safe from the UI thread between frames, and from nowhere else. ratatui
/// buffers a whole frame and flushes it at the end of `Terminal::draw`, so
/// stdout is quiescent in between; and neither BEL nor an OSC string paints a
/// cell or moves the cursor, so what is on screen — alternate screen included —
/// is untouched. Written from the worker thread instead, it could land in the
/// middle of a flush and cut somebody's escape sequence in half.
pub(crate) fn ring(text: &str) {
    // The state-machine tests drive real crossings, and stdout under `cargo
    // test` is the developer's terminal: without this the suite beeps at them
    // and leaves escape sequences among the results. What would have been
    // written is kept instead, so a test can count the bells.
    #[cfg(test)]
    RUNG.with(|rung| rung.borrow_mut().push(text.to_string()));
    #[cfg(not(test))]
    {
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = write!(out, "\x07\x1b]9;{}\x07", sanitize(text));
        let _ = out.flush();
    }
}

/// Strip what would end the OSC string early.
///
/// The text carries a session label, which is a directory name the user chose.
/// A BEL in it would terminate the notification and hand the remainder to the
/// terminal as commands; an ESC would do worse.
fn sanitize(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
thread_local! {
    /// What [`ring`] would have written, for the tests that count bells.
    /// Per thread, because the test harness runs tests side by side.
    static RUNG: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Every bell rung on this thread since the last call, emptied as it is read.
#[cfg(test)]
pub(crate) fn take_rung() -> Vec<String> {
    RUNG.with(|rung| std::mem::take(&mut *rung.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;

    fn session(id: &str, running: bool, state: ActivityState) -> Session {
        let mut s = Session::new(Provider::Claude, id.into());
        s.label_source = format!("/home/x/{id}");
        s.abbrev_label = id.into();
        s.activity_state = state;
        if running {
            s.process = Some(crate::proc::ProcInfo::default());
        }
        s
    }

    /// The complaint this split exists for: before it, an agent holding a
    /// permission prompt and an agent whose turn was simply over were the same
    /// fact — the same colour on the row, and the same sentence in the bell.
    #[test]
    fn a_held_permission_prompt_is_not_a_finished_turn() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.observe(&[session("a", true, ActivityState::Asking)]);
        let rang = n.recent.back().expect("a blocked agent rings");
        assert_eq!(rang.reason, Reason::Asking);
        assert!(
            crossing_text(rang, 0).contains("needs permission"),
            "the bell says which kind of waiting it is: {}",
            crossing_text(rang, 0)
        );

        // And the other kind still reads as the other kind.
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.observe(&[session("a", true, ActivityState::WaitingForInput)]);
        assert_eq!(n.recent.back().map(|r| r.reason), Some(Reason::NeedsInput));
    }

    /// Answering the prompt and having the turn end a moment later is one
    /// event, not two: only a crossing out of `Busy` rings, so the second
    /// transition is silent.
    #[test]
    fn answering_a_prompt_does_not_ring_again_when_the_turn_ends() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.observe(&[session("a", true, ActivityState::Asking)]);
        n.recent.clear();
        n.observe(&[session("a", true, ActivityState::WaitingForInput)]);
        assert!(n.recent.is_empty(), "the same turn, reported twice");
    }

    /// The whole point of the feature: one ring on the crossing, silence on
    /// every refresh after it, however long the session sits there waiting.
    #[test]
    fn a_session_that_stops_working_rings_once_and_not_again() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        let busy = vec![session("a", true, ActivityState::Working)];
        n.observe(&busy);
        assert!(n.recent.is_empty(), "still working, nothing to say");

        let waiting = vec![session("a", true, ActivityState::WaitingForInput)];
        n.observe(&waiting);
        let first = n.recent.back().cloned().expect("the crossing rings");
        assert_eq!(first.reason, Reason::NeedsInput);

        n.observe(&waiting);
        assert_eq!(
            n.recent.back().map(|r| r.at),
            Some(first.at),
            "a session that is still waiting must not ring again"
        );
    }

    #[test]
    fn a_busy_session_that_disappears_rings_as_stopped() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        // Same session, its process gone.
        n.observe(&[session("a", false, ActivityState::Working)]);
        let rang = n.recent.back().expect("an agent that exits is news");
        assert_eq!(rang.reason, Reason::Stopped);
        assert_eq!(rang.label, "a");

        // Gone from the table entirely: nothing left to cross.
        n.recent.clear();
        n.observe(&[]);
        assert!(n.recent.is_empty());
    }

    /// A session already idle when cctop starts — or when `n` is pressed — has
    /// not transitioned in front of us, so it must stay silent.
    #[test]
    fn a_session_first_seen_idle_never_rings_for_being_idle() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        let waiting = vec![session("a", true, ActivityState::WaitingForInput)];
        n.observe(&waiting);
        n.observe(&waiting);
        assert!(n.recent.is_empty());
    }

    #[test]
    fn the_state_machine_stays_warm_while_notifications_are_off() {
        let mut n = Notifier::default();
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.enabled = true;
        n.observe(&[session("a", true, ActivityState::WaitingForInput)]);
        assert!(
            !n.recent.is_empty(),
            "the busy state seen before the toggle still counts"
        );
    }

    #[test]
    fn going_back_to_work_answers_the_bell() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.observe(&[session("a", true, ActivityState::WaitingForInput)]);
        assert!(!n.recent.is_empty());
        n.observe(&[session("a", true, ActivityState::Working)]);
        assert!(n.recent.is_empty(), "answered, so stop naming it");
    }

    #[test]
    fn the_footer_names_the_session_until_it_is_selected() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[session("a", true, ActivityState::Working)]);
        n.observe(&[session("a", true, ActivityState::WaitingForInput)]);
        let key = n.recent.back().unwrap().key.clone();
        assert!(n.footer(None).is_some_and(|t| t.contains('a')));
        assert!(n.footer(Some("claude:other")).is_some());
        assert!(n.footer(Some(&key)).is_none());
        assert!(n.rang_recently(&key));
        assert!(!n.rang_recently("claude:other"));
    }

    /// The label is a path the user chose, and it is interpolated into an
    /// escape sequence. Anything that could close that sequence early has to be
    /// gone before it reaches the terminal.
    #[test]
    fn a_label_cannot_break_out_of_the_notification() {
        let text = sanitize("proj\x07\x1b]0;pwned\x07");
        assert!(!text.contains('\x07'));
        assert!(!text.contains('\x1b'));
        assert_eq!(text, "proj]0;pwned");
    }

    #[test]
    fn one_bell_for_a_refresh_that_finishes_several_sessions() {
        let rang = Rang {
            key: "claude:a".into(),
            label: "alpha".into(),
            reason: Reason::NeedsInput,
            at: Instant::now(),
        };
        let bell = |extra| bell_text(&[crossing_text(&rang, extra)]);
        assert_eq!(
            bell(0).as_deref(),
            Some("cctop: alpha is waiting for input")
        );
        assert_eq!(
            bell(2).as_deref(),
            Some("cctop: alpha is waiting for input (+2 more)")
        );
    }

    /// A refresh that finishes two sessions owes the user two places to
    /// look, not one: the bell says "+1 more", and every one of them keeps
    /// its marker and its turn under `b` — not only the one the bell named.
    #[test]
    fn every_session_that_crossed_stays_findable() {
        let mut n = Notifier {
            enabled: true,
            ..Default::default()
        };
        n.observe(&[
            session("a", true, ActivityState::Working),
            session("b", true, ActivityState::Working),
        ]);
        n.observe(&[
            session("a", true, ActivityState::WaitingForInput),
            session("b", true, ActivityState::Asking),
        ]);

        assert!(n.rang_recently("claude:a"));
        assert!(
            n.rang_recently("claude:b"),
            "the crossing the bell did not name lost its marker"
        );

        // `b` walks the queue rather than parking on the first answer: each
        // press names a session it is not already on, and cycles back around
        // once every one has had its turn.
        assert_eq!(n.unanswered(None).map(|r| r.key.as_str()), Some("claude:a"));
        assert_eq!(
            n.unanswered(Some("claude:a")).map(|r| r.key.as_str()),
            Some("claude:b")
        );
        assert_eq!(
            n.unanswered(Some("claude:b")).map(|r| r.key.as_str()),
            Some("claude:a")
        );

        // And the footer says the same thing the bell did: the one `b` lands
        // on, plus a count of the rest.
        let footer = n.footer(None).expect("someone rang");
        assert!(footer.contains('a'), "{footer}");
        assert!(footer.contains("+1 more"), "{footer}");
    }

    /// The quota ring is an edge, not a level: the first reading only
    /// establishes where things stand, the saturated→free crossing is the one
    /// announcement, and every reading after it is silence.
    #[test]
    fn a_quota_window_rings_once_on_the_way_back() {
        let mut n = Notifier::default();
        let at = |pct: u32| crate::quota::Quota {
            fetched: true,
            claude: vec![crate::quota::ProfileQuota {
                profile: "default".into(),
                status: crate::quota::ProviderStatus::Ok(crate::quota::ProviderQuota {
                    plan: None,
                    windows: vec![crate::quota::Window {
                        label: "5h",
                        pct,
                        duration: None,
                        resets_at: None,
                    }],
                    limit_reached: false,
                }),
                source: crate::config::AccountSource::Directory,
            }],
            codex: Vec::new(),
        };

        // Arriving already free is not news — the first observation only sets
        // the baseline, exactly like a session first seen idle.
        assert!(n.observe_quota(&at(40)).is_empty());
        assert!(
            n.observe_quota(&at(100)).is_empty(),
            "filling is not the edge"
        );
        let freed = n.observe_quota(&at(60));
        assert_eq!(freed.len(), 1, "the crossing is the announcement");
        assert!(freed[0].contains("5h"), "{freed:?}");
        assert!(
            n.observe_quota(&at(60)).is_empty(),
            "a window that stays open stays quiet"
        );

        // A window missing from a reading is not a freed one: a throttled
        // provider answers nothing, and unknown is not "back".
        let mut gone = at(60);
        gone.claude[0].status = crate::quota::ProviderStatus::RateLimited { retry_at: None };
        assert!(n.observe_quota(&gone).is_empty());
        assert!(
            n.observe_quota(&at(60)).is_empty(),
            "still nothing new to say"
        );
    }

    /// The provider's own `limit_reached` is watched beside the windows:
    /// Codex reports the hold before any window reads full, and the lift is
    /// the same piece of news.
    #[test]
    fn a_provider_level_limit_rings_on_lift() {
        let mut n = Notifier::default();
        let quota = |held: bool| crate::quota::Quota {
            fetched: true,
            claude: Vec::new(),
            codex: vec![crate::quota::ProfileQuota {
                profile: "default".into(),
                status: crate::quota::ProviderStatus::Ok(crate::quota::ProviderQuota {
                    plan: None,
                    windows: Vec::new(),
                    limit_reached: held,
                }),
                source: crate::config::AccountSource::Directory,
            }],
        };
        assert!(n.observe_quota(&quota(true)).is_empty());
        let freed = n.observe_quota(&quota(false));
        assert_eq!(freed.len(), 1);
        assert!(freed[0].contains("limit has lifted"), "{freed:?}");
    }

    /// `origin/?t=tok` becomes `origin/session/<id>?t=tok`, keeping the token
    /// the link needs to open anything.
    #[test]
    fn a_session_link_keeps_the_token_in_the_query() {
        assert_eq!(
            session_link("https://x.trycloudflare.com/?t=abc", "s1"),
            "https://x.trycloudflare.com/session/s1?t=abc"
        );
        assert_eq!(
            session_link("http://127.0.0.1:7777/", "s1"),
            "http://127.0.0.1:7777/session/s1"
        );
    }
}
