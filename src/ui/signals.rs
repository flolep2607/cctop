//! What running agents report about themselves, and how the user is told.
//!
//! A transcript says what an agent did; only a hook says what it is doing right
//! now — waiting for permission, idle, or working — and that word has to be
//! believed for a while, reconciled against a pane's own pid, and dropped when
//! nothing confirms it. Attention is the other half of the same story: a
//! reported state is what makes a tab blink, ring a bell, or answer `b`. Both
//! belong to the reports rather than to the table they decorate.

use super::*;

/// Half-period of the tab-bar blink. Slow enough to read the title through,
/// fast enough to catch the eye.
const BLINK_MS: u128 = 600;

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

impl App {
    /// Promote every held permission prompt whose grace has expired, and say
    /// whether any had.
    ///
    /// Auto mode's answer arrives as its own event and replaces the report, so
    /// a prompt still sitting here when the grace runs out is one a person has
    /// to answer. Cleared rather than re-tested every tick: once a prompt is
    /// real it stays real, and leaving the flag up would have the whole table
    /// recomputed on every pass of the loop for the rest of the session.
    pub(super) fn promote_matured_prompts(&mut self) -> bool {
        let mut matured = false;
        for reported in self.hooked.values_mut() {
            if reported.provisional && reported.at.elapsed() >= crate::hook::PERMISSION_GRACE {
                reported.provisional = false;
                matured = true;
            }
        }
        self.needs_redraw |= matured;
        matured
    }

    /// Let the notifier see this refresh, and ring if anything crossed.
    ///
    /// Called from the event loop only when the rows actually moved. It has to
    /// run on this thread: the bell and the OSC 9 sequence go straight to
    /// stdout, which ratatui owns, and only here is it certain that no frame is
    /// halfway through being flushed.
    pub(super) fn check_bells(&mut self) {
        self.notify.observe(&self.sessions);
    }

    /// Turn the bell on or off, and remember which.
    pub(super) fn toggle_notifications(&mut self) {
        self.notify.enabled = !self.notify.enabled;
        self.save_prefs();
        self.set_status(if self.notify.enabled {
            "Notifications on — bell and desktop alert when a session needs you"
        } else {
            "Notifications off"
        });
    }

    /// Jump the selection to whichever session the bell is still owed to.
    ///
    /// Several can have rung since the last press, so this asks the notifier
    /// which one it has not landed on yet — a second `b` walks to the next
    /// rather than re-selecting the same row.
    pub(super) fn jump_to_bell(&mut self) {
        let on = self.selected_session().map(|s| s.key());
        let Some(rang) = self.notify.unanswered(on.as_deref()) else {
            self.set_status("Nothing has rung yet");
            return;
        };
        let key = rang.key.clone();
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
    pub(super) fn apply_hooks(&mut self, events: Vec<crate::hook::Event>) -> (bool, bool) {
        let changed = !events.is_empty();
        let mut lifecycle = false;
        let mut moved = false;
        for event in events {
            crate::elog::event(
                "hook",
                "recv",
                serde_json::json!({
                    "session": event.session_id,
                    "signal": format!("{:?}", event.reported.signal),
                    "pids": event.pids.len(),
                    "cwd": &event.reported.cwd,
                }),
            );
            lifecycle |= event.reported.signal.is_lifecycle();
            if let Some(agent) = event.finished_agent {
                self.finished_agents.insert(agent);
            }
            match event.reported.signal {
                // Nothing more will be said about it, and leaving the last
                // signal behind would have the row claim a state forever.
                crate::hook::Signal::Ended => {
                    self.hooked.remove(&event.session_id);
                    moved |= self.hook_pids.remove(&event.session_id).is_some();
                }
                _ => {
                    if !event.pids.is_empty()
                        && self.hook_pids.get(&event.session_id) != Some(&event.pids)
                    {
                        self.hook_pids.insert(event.session_id.clone(), event.pids);
                        moved = true;
                    }
                    self.hooked.insert(event.session_id, event.reported);
                }
            }
        }
        if moved {
            self.note_hook_pids();
        }
        // A working claim that nothing has confirmed for a quarter of an hour is
        // dropped rather than believed: see
        // [`Reported::is_current`](crate::hook::Reported::is_current). Swept
        // here because this is the only place the map grows, and a session that
        // was killed mid-turn will never send the event that would clear it.
        self.hooked.retain(|_, reported| reported.is_current());
        // A chain outlives its usefulness exactly when the report it came with
        // does, and a session that has been swept must stop claiming a pid —
        // otherwise a reused pid would be handed to a session that is gone. A
        // waiting session is never swept, which is the case this exists for.
        let before = self.hook_pids.len();
        self.hook_pids.retain(|id, _| self.hooked.contains_key(id));
        if self.hook_pids.len() != before {
            self.note_hook_pids();
        }
        self.apply_finished_agents();
        self.apply_reports();
        (changed, lifecycle)
    }

    /// Tell the worker which processes the agents say they are running under.
    ///
    /// This is the only thing that binds a session to a *process* by something
    /// an agent stated rather than something cctop inferred. Without it a plain
    /// `claude` — which carries no session id on its command line — is paired
    /// with a transcript by working directory and recency, so two agents in one
    /// checkout swap rows whenever their activity order changes, and the alerts
    /// follow the swap onto the wrong tab.
    ///
    /// Sent whole rather than incrementally: it is a handful of entries, and a
    /// replacement cannot drift from what this cctop believes the way a stream
    /// of deltas could.
    fn note_hook_pids(&self) {
        crate::hook::save_claims(&self.hook_pids);
        let _ = self
            .tx
            .send(super::worker::Request::HookClaims(self.hook_pids.clone()));
    }

    /// Stamp each session with what its own hooks reported: the permission mode
    /// it runs under, and whether it is blocked on a question.
    ///
    /// Also run after a walk, because the rows are rebuilt wholesale and a
    /// freshly discovered one has to pick up what was reported before it
    /// existed. `hooked` outlives the rows for exactly this reason.
    ///
    /// This is what puts a held permission prompt on a row at all. Everything
    /// downstream of the row reads it for free — the STATE dot, the bell, the
    /// web report, `--json`, and a fleet peer — rather than each of them being
    /// handed the UI's map of live reports and asked to agree with the others.
    pub(crate) fn apply_reports(&mut self) {
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
                // The report is only allowed to say the two things the
                // transcript cannot — see
                // [`Signal::activity`](crate::hook::Signal::activity) — so a
                // stale-but-not-yet-expired working claim cannot talk a row out
                // of an API error it is genuinely sitting in.
                // A permission prompt auto mode may still answer is not yet
                // news: see
                // [`Reported::is_settled`](crate::hook::Reported::is_settled).
                // The row keeps whatever the transcript makes of it — which is
                // "working", because that is what the agent is doing.
                if reported.is_settled()
                    && let Some(state) = reported.signal.activity()
                {
                    session.activity_state = state;
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
    pub(super) fn apply_finished_agents(&mut self) {
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

    /// What a session's own hooks last said about it, if it has any.
    pub(super) fn hooked_signal(&self, session_id: &str) -> Option<crate::hook::Signal> {
        // Checked here as well as in the sweep: the sweep runs when an event
        // arrives, and a session that has gone silent is precisely the one that
        // sends none — so between events the map still holds the stale claim.
        if let Some(reported) = self.hooked.get(session_id) {
            let show = reported.is_current() && reported.is_settled();
            return show.then_some(reported.signal);
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
            .filter(|(_, reported)| reported.is_current() && reported.is_settled())
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
    pub(super) fn pane_signal(&self, pid: u32) -> Option<crate::hook::Signal> {
        self.reported_by(pid).or_else(|| {
            self.sessions
                .iter()
                .filter(|session| session.root_pid() == Some(pid))
                .find_map(|session| {
                    match session.activity_state {
                        // Both of the row's waiting states are a tab worth
                        // colouring; which colour is the caller's business.
                        crate::session::ActivityState::Asking => {
                            Some(crate::hook::Signal::NeedsInput)
                        }
                        crate::session::ActivityState::WaitingForInput => {
                            Some(crate::hook::Signal::Idle)
                        }
                        _ => None,
                    }
                })
        })
    }

    /// What the agent running as `pid` last reported over its own hooks.
    ///
    /// The half of [`Self::pane_signal`] with no inference in it, split out
    /// because it is the only half worth writing onto an rmux session: a
    /// transcript reading is one every cctop on the machine can take for
    /// itself, off the same files, so recording one would be publishing a guess
    /// that the reader could already have made.
    pub(super) fn reported_by(&self, pid: u32) -> Option<crate::hook::Signal> {
        self.sessions
            .iter()
            .filter(|session| session.root_pid() == Some(pid))
            .find_map(|session| self.hooked_signal(&session.session_id))
    }

    /// Write what this cctop has heard onto the rmux sessions that carry it.
    ///
    /// The other direction of [`Shared::recorded`](tabs::Shared): every cctop
    /// reads these, so somebody has to write them, and the one that heard the
    /// event is the only one that can. It is not the hook that writes — see
    /// [`rmux::set_state`](crate::rmux::set_state) for why the agent's deadline
    /// must not pay for this.
    ///
    /// Driven off `running` rather than off the tabs, so a session no tab of
    /// this cctop's stands for is still kept up to date: the row exists and its
    /// hooks report here whether or not anybody has opened a tab on it.
    ///
    /// The write condition is the whole of the rate limiting, and it is just
    /// "the session does not already say this". An unchanged state costs
    /// nothing, which matters because the sweep runs every
    /// [`SHARE_EVERY`] and a turn is mostly the same signal repeated. An
    /// expired one reads as saying nothing, so a working claim this cctop still
    /// believes is rewritten rather than allowed to lapse.
    pub(super) fn publish_states(&self, running: &[crate::rmux::Running]) {
        for (name, signal) in self.states_to_publish(running, crate::rmux::now_secs()) {
            crate::rmux::set_state(&name, signal);
        }
    }

    /// Which sessions are out of date and what to say about them — the half of
    /// [`Self::publish_states`] with the judgement in it, split out so it can be
    /// tested without a daemon to write to.
    pub(super) fn states_to_publish(
        &self,
        running: &[crate::rmux::Running],
        now: u64,
    ) -> Vec<(String, crate::hook::Signal)> {
        if self.hooked.is_empty() {
            return Vec::new();
        }
        running
            .iter()
            .filter_map(|agent| {
                let signal = agent.pid.and_then(|pid| self.reported_by(pid))?;
                let recorded = agent
                    .state
                    .filter(|state| state.is_current(now))
                    .map(|state| state.signal);
                (recorded != Some(signal)).then(|| (agent.name.clone(), signal))
            })
            .collect()
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
    pub(super) fn mark_answered(&mut self, pid: u32) {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{session, test_app};
    /// What gets written onto a session, and — mostly — what does not.
    ///
    /// The sweep runs every [`SHARE_EVERY`] and a turn is largely the same
    /// signal repeated, so "the session already says this" has to be the common
    /// answer. Each write is a process spawn and a round trip to the daemon;
    /// doing one per sweep per tab would make the tab bar pay for the feature.
    #[test]
    fn a_session_is_only_written_to_when_it_is_out_of_date() {
        let agent = |state| crate::rmux::Running {
            name: "cctop-a".into(),
            pid: Some(4321),
            cwd: None,
            attached: false,
            activity: None,
            label: None,
            profile: None,
            order: None,
            state,
        };
        let now = 1_700_000_000;
        let recorded = |signal, ago: u64| {
            Some(crate::rmux::State {
                signal,
                at: now - ago,
            })
        };

        let mut app = test_app();
        let mut row = session("a", true, "proj");
        row.process.as_mut().unwrap().process_list = vec![crate::proc::ProcEntry {
            pid: 4321,
            is_root: true,
            ghost: false,
            cpu: 0.0,
            memory: 0,
            args: String::new(),
        }];
        app.sessions = vec![row];

        // Nothing heard yet: nothing to say, whatever the session claims.
        assert!(app.states_to_publish(&[agent(None)], now).is_empty());

        app.apply_hooks(vec![crate::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: crate::hook::Reported {
                signal: crate::hook::Signal::NeedsInput,
                cwd: "/w/proj".into(),
                permission: None,
                at: std::time::Instant::now(),
                provisional: false,
            },
            finished_agent: None,
        }]);

        assert_eq!(
            app.states_to_publish(&[agent(None)], now),
            vec![("cctop-a".to_string(), crate::hook::Signal::NeedsInput)],
            "a session saying nothing is told"
        );
        assert!(
            app.states_to_publish(&[agent(recorded(crate::hook::Signal::NeedsInput, 30))], now)
                .is_empty(),
            "a session already saying it is left alone"
        );
        assert_eq!(
            app.states_to_publish(&[agent(recorded(crate::hook::Signal::Busy, 30))], now)
                .len(),
            1,
            "a session saying something else is corrected"
        );
        // A row whose pid nothing has reported for is not published from
        // somebody else's report.
        assert!(
            app.states_to_publish(
                &[crate::rmux::Running {
                    pid: Some(9999),
                    ..agent(None)
                }],
                now
            )
            .is_empty()
        );

        // An expired record reads as saying nothing, so a claim this cctop
        // still believes is rewritten rather than left to lapse under it. This
        // is the one case where the session and the report agree and a write
        // happens anyway — without it, a long quiet turn would go dark at
        // fifteen minutes for every cctop but this one.
        app.apply_hooks(vec![crate::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: crate::hook::Reported {
                signal: crate::hook::Signal::Busy,
                cwd: "/w/proj".into(),
                permission: None,
                at: std::time::Instant::now(),
                provisional: false,
            },
            finished_agent: None,
        }]);
        assert!(
            app.states_to_publish(&[agent(recorded(crate::hook::Signal::Busy, 30))], now)
                .is_empty(),
            "a fresh agreement needs no write"
        );
        assert_eq!(
            app.states_to_publish(
                &[agent(recorded(crate::hook::Signal::Busy, 3 * 3_600))],
                now
            )
            .len(),
            1,
            "an agreement old enough to have lapsed is renewed"
        );
    }

    /// A permission prompt leaves no trace in a transcript — the newest record
    /// is a tool call in flight, which reads as ordinary work — so before the
    /// hooks reached the row, an agent stopped dead waiting for you was drawn
    /// exactly like an agent that was busy, and a finished turn was drawn like
    /// both. Only the report can tell the three apart.
    #[test]
    fn a_row_learns_from_its_hooks_what_a_transcript_cannot_say() {
        let event = |signal| crate::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: crate::hook::Reported {
                provisional: false,
                signal,
                cwd: "/w/proj".into(),
                permission: None,
                at: std::time::Instant::now(),
            },
            finished_agent: None,
        };
        let mut app = test_app();
        app.sessions = vec![session("a", true, "proj")];
        // What the walk left behind: a tool call in flight.
        app.sessions[0].activity_state = crate::session::ActivityState::Working;

        app.apply_hooks(vec![event(crate::hook::Signal::NeedsInput)]);
        assert_eq!(
            app.sessions[0].activity_state,
            crate::session::ActivityState::Asking
        );

        app.apply_hooks(vec![event(crate::hook::Signal::Idle)]);
        assert_eq!(
            app.sessions[0].activity_state,
            crate::session::ActivityState::WaitingForInput,
            "a finished turn is the quieter of the two waits"
        );

        // A working report leaves the row alone rather than overwriting it: the
        // transcript reads work correctly and is the only one of the two that
        // can see an API error.
        app.sessions[0].activity_state = crate::session::ActivityState::ApiError;
        app.apply_hooks(vec![event(crate::hook::Signal::Busy)]);
        assert_eq!(
            app.sessions[0].activity_state,
            crate::session::ActivityState::ApiError
        );
    }

    /// The mode is reported by a live agent but drawn on a row rebuilt by every
    /// walk, so the two have to survive arriving in either order.
    #[test]
    fn the_permission_mode_survives_the_rows_being_rebuilt() {
        let reported = |mode: Option<crate::hook::Permission>| crate::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: crate::hook::Reported {
                provisional: false,
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

        app.apply_reports();
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
            pids: Vec::new(),
            reported: crate::hook::Reported {
                provisional: false,
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
            pids: Vec::new(),
            reported: crate::hook::Reported {
                provisional: false,
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
            pids: Vec::new(),
            reported: crate::hook::Reported {
                provisional: false,
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
            pids: Vec::new(),
            reported: crate::hook::Reported {
                provisional: false,
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
            pids: Vec::new(),
            reported: crate::hook::Reported {
                provisional: false,
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

    /// A permission prompt is not put in front of anyone until auto mode has
    /// had its few seconds to answer it.
    ///
    /// Claude Code's auto mode asks a model whether to allow a tool call, which
    /// takes a moment; the hook fires when the prompt is raised, not when it is
    /// decided. Ringing the bell there means ringing it for every auto-approved
    /// call — the surest way to teach someone to ignore the bell.
    #[test]
    fn a_held_prompt_becomes_news_only_if_nothing_answers_it() {
        use crate::hook::{Event, Reported, Signal};
        use crate::session::ActivityState;

        let raised = |signal: Signal, provisional: bool| Event {
            session_id: "a".to_string(),
            pids: Vec::new(),
            finished_agent: None,
            reported: Reported {
                signal,
                cwd: String::new(),
                permission: None,
                at: std::time::Instant::now(),
                provisional,
            },
        };

        let mut app = test_app();
        app.sessions = vec![session("a", true, "proj")];

        // The prompt goes up. Nothing on the row says so yet.
        app.apply_hooks(vec![raised(Signal::NeedsInput, true)]);
        assert_eq!(app.sessions[0].activity_state, ActivityState::Working);
        assert_eq!(app.hooked_signal("a"), None, "the prompt was shown at once");

        // Auto mode answers inside the grace: the row never reported it, and
        // the newer event has replaced it, so nothing is left to mature.
        app.apply_hooks(vec![raised(Signal::Busy, false)]);
        assert!(!app.promote_matured_prompts());
        assert_eq!(app.sessions[0].activity_state, ActivityState::Working);

        // A prompt nobody answered. Once its grace runs out it is a person's
        // problem, and no event is coming to say so.
        app.apply_hooks(vec![raised(Signal::NeedsInput, true)]);
        let held = app.hooked.get_mut("a").expect("the report");
        held.at = std::time::Instant::now()
            - (crate::hook::PERMISSION_GRACE + std::time::Duration::from_secs(1));
        assert!(app.promote_matured_prompts(), "the grace never ran out");
        app.apply_reports();
        assert_eq!(app.sessions[0].activity_state, ActivityState::Asking);
        assert_eq!(app.hooked_signal("a"), Some(Signal::NeedsInput));

        // And it is promoted once, not on every pass of the event loop.
        assert!(!app.promote_matured_prompts());
    }

    /// The other half of the bell: hearing it is useless if the row it came
    /// from is somewhere in a list of a dozen.
    #[test]
    fn b_jumps_the_selection_to_the_session_that_rang() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "/x/a"), session("b", true, "/x/b")];
        app.refilter();
        let target = app.sessions[1].key();
        app.notify.record_for_test(crate::notify::Rang {
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
}
