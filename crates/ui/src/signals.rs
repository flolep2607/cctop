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

/// How often a detached tab's screen is re-read. Each read is a `capture-pane`
/// subprocess, which is not a per-frame cost the way an attached pane's own
/// parser is — and a permission prompt can wait a second to be seen.
const PEEK_EVERY: Duration = Duration::from_secs(1);

/// What the stall alert needs from a session's hooks.
///
/// Read off the last report whether or not it is still believed. A working
/// claim lapses after a quarter of an hour of silence so that the row stops
/// saying "busy" about a session nobody is running — but for the stall alert
/// that silence is the finding, not a reason to stop looking. The row's own
/// state, which does honour the lapse, is checked beside it.
fn stall_view(reports: &cctop_core::hook::Reports, session_id: &str) -> cctop_core::alert::Hooked {
    use cctop_core::alert::Hooked;
    use cctop_core::hook::Signal;
    match reports.report(session_id).map(|r| r.signal) {
        None => Hooked::Unknown,
        Some(Signal::Busy | Signal::Started) => Hooked::Working,
        Some(Signal::Acting | Signal::Compacting) => Hooked::InFlight,
        Some(Signal::NeedsInput | Signal::Idle | Signal::Ended) => Hooked::Quiet,
    }
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
        let matured = self.reports.promote_matured();
        self.needs_redraw |= matured;
        matured
    }

    /// Let the notifier see this refresh, and queue a bell for anything that
    /// crossed or fired.
    ///
    /// Called from the event loop only when the rows actually moved, which
    /// then rings once for the whole pass with
    /// [`Notifier::ring_pending`](cctop_core::notify::Notifier::ring_pending). That
    /// has to run on this thread: the bell and the OSC 9 sequence go straight
    /// to stdout, which ratatui owns, and only here is it certain that no
    /// frame is halfway through being flushed.
    pub(super) fn check_bells(&mut self) {
        // The serve's link is what a webhook POST points at, so it is refreshed
        // with the crossings: starting `B` mid-run must not leave every future
        // notification saying there is nowhere to look.
        self.notify.link_base = self.serving.as_ref().map(|s| s.best().to_string());
        self.notify.observe(&self.sessions);

        let reports = &self.reports;
        let fired = self.alerts.observe(
            &self.settings.alert_rules(),
            &self.sessions,
            self.stats.spend_today,
            |s| stall_view(reports, &s.session_id),
            Instant::now(),
            chrono::Utc::now(),
        );
        self.announce_alerts(fired);
    }

    /// Tell the user about the alerts that just fired: a toast each, whether
    /// or not `w` is on — the thresholds are the opt-in, and a toast is quiet —
    /// and one clause in the refresh's bell for the lot when it is.
    ///
    /// One clause, as with the sessions that finish together: three rings in
    /// a row is noise, and the toasts already name every one.
    fn announce_alerts(&mut self, fired: Vec<cctop_core::alert::Fired>) {
        let Some(first) = fired.first() else {
            return;
        };
        self.notify.chime(&first.text, fired.len() - 1);
        for alert in fired {
            cctop_core::elog::event(
                "alert",
                alert.kind_name(),
                serde_json::json!({ "text": alert.text, "session": alert.key }),
            );
            self.notify.post_event("alert", &alert.text);
            self.set_status(alert.text);
        }
    }

    /// A quota window that just opened back up.
    ///
    /// The same machinery as a session crossing — the bell, the webhook, the
    /// log — pointed at the other thing worth being interrupted for: "you can
    /// spend again" is the one event a monitor of agent spend owes a person
    /// who has stepped away.
    pub(super) fn announce_quota_freed(&mut self, text: &str) {
        cctop_core::elog::event("quota", "freed", serde_json::json!({ "text": text }));
        self.notify.post_event("quota-freed", text);
        self.notify.chime(text, 0);
        self.set_status(text.to_string());
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
        self.reveal(&key);
        // The parent row, not a child of it: the bell rang for the session.
        match self
            .visible
            .iter()
            .position(|&r| matches!(r, Row::Session(i) if self.sessions[i].key() == key))
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
        let watched = index == self.tab;
        match tab.attention(watched, &|pid| self.pane_signal(pid))? {
            // Upgraded here rather than decided inside the tab: whether a turn
            // has been seen is the app's memory, not anything a pane holds —
            // see [`seen`](super::seen). Only a tab that already reads as idle
            // is, so a disagreement between the row and the screen leaves the
            // screen's answer standing.
            tabs::Attention::Idle
                if tab
                    .unwatched_agents(watched)
                    .into_iter()
                    .any(|pid| self.pid_done(pid)) =>
            {
                Some(tabs::Attention::Done)
            }
            attention => Some(attention),
        }
    }

    /// Whether the agent running as `pid` finished a turn you have not looked
    /// at.
    pub(super) fn pid_done(&self, pid: u32) -> bool {
        self.sessions
            .iter()
            .filter(|session| session.root_pid() == Some(pid))
            .any(|session| self.seen.is_done(&session.key()))
    }

    /// The key of the session being looked at, as [`seen`](super::seen) counts
    /// looking: the focused pane's agent in a tab, the selected row on the
    /// dashboard.
    ///
    /// One `String`, where the walk below used to build one per live session.
    fn viewed_session(&self) -> Option<String> {
        let pid = match self.tab {
            0 => return self.selected_session().map(Session::key),
            tab => {
                let tab = self.tabs.get(tab - 1)?;
                tab.panes.get(tab.focus)?.agent()
            }
        };
        self.sessions
            .iter()
            .find(|session| session.root_pid() == Some(pid))
            .map(Session::key)
    }

    /// Bring the unseen marks up to date with what the rows say and where you
    /// are looking. Returns whether any mark came or went.
    ///
    /// Every pass of the loop rather than on refresh alone: looking is a
    /// keypress, and a mark that outlived the look by a refresh interval would
    /// be a mark on the thing you are reading.
    pub(super) fn observe_seen(&mut self) -> bool {
        let viewed = self.viewed_session();
        self.seen.observe(
            self.sessions
                .iter()
                .filter(|session| session.is_running())
                .map(|session| (session, seen::Phase::of(session.activity_state))),
            viewed.as_deref(),
        )
    }

    /// Fold in whatever the agents have reported.
    ///
    /// These outrank anything read off disk or off a screen: an agent saying
    /// "my turn is over" is the fact those are both estimating.
    ///
    /// Returns whether anything arrived, and whether the set of sessions itself
    /// changed — one that has just started or just ended is a row to go and
    /// find or forget now, rather than at the next poll.
    pub(super) fn apply_hooks(&mut self, events: Vec<cctop_core::hook::Event>) -> (bool, bool) {
        let changed = !events.is_empty();
        let mut lifecycle = false;
        let mut moved = false;
        for event in events {
            cctop_core::elog::event(
                "hook",
                "recv",
                serde_json::json!({
                    "session": event.session_id,
                    "signal": format!("{:?}", event.reported.signal),
                    // Which subagent spoke, if one did: a question the tab
                    // failed to show is only diagnosable from which agent
                    // asked and which sibling spoke over it.
                    "agent": event.agent.as_deref().unwrap_or_default(),
                    "provisional": event.reported.provisional,
                    "pids": event.pids.len(),
                    "cwd": &event.reported.cwd,
                }),
            );
            if let Some(agent) = event.finished_agent.clone() {
                self.finished_agents.insert(agent);
            }
            // The folding — open questions standing, stale claims swept, ended
            // sessions forgotten — lives in [`Reports::observe`], shared with
            // a standalone `cctop serve`, which has to agree with this table.
            let (is_lifecycle, claims_moved) = self.reports.observe(&event);
            lifecycle |= is_lifecycle;
            moved |= claims_moved;
        }
        if moved {
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
        cctop_core::hook::save_claims(&self.reports.claims);
        let _ = self.tx.send(super::worker::Request::HookClaims(
            self.reports.claims.clone(),
        ));
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
        // Always run, even with nothing new to stamp: `asking_for` is only
        // ever set here, and a row has to lose it when the prompt that named
        // it went away — including by the last report being swept.
        let peeked = &self.peeked;
        let screens = &self.screen_read;
        for session in &mut self.sessions {
            // The screen, when it is being read and says something, outranks
            // every report: it is what the agent is showing *now*, where a hook
            // event is what it said last — late for a permission prompt, stale
            // for a question already answered, absent when hooks are not
            // installed at all. See [`App::read_screens`] and [`Self::screened`].
            let screened = session
                .root_pid()
                .and_then(|pid| peeked.get(&pid).or_else(|| screens.get(&pid)));
            session.apply_reports(self.reports.report(&session.session_id), screened);
            self.reports.stamp_sandbox(session);
        }
        // Here because every rebuild of the rows passes through here, and a
        // row that loses its YOLO mark for one publish is a badge that
        // flickers off on every page watching it.
        self.yolo.stamp(&mut self.sessions);
    }

    /// Switch YOLO for the selected row, through the same check the page's
    /// switch goes through — see [`cctop_core::actions::yolo`].
    pub(super) fn toggle_yolo(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let said = match cctop_core::actions::yolo(session, session.yolo.is_none()) {
            Ok(done) => done.message,
            Err((_, why)) => why,
        };
        self.set_status(said);
        // Read on the next pass rather than half a second from now.
        self.yolo_at = None;
    }

    /// Read the YOLO switch and answer what it covers. `now` skips the slow
    /// clock. Returns whether a row changed.
    ///
    /// On the draw loop, like the key press it may make. The screen read it
    /// may make first is as rare: only a Claude YOLO session with a prompt the
    /// hook did not answer, and once a second at most — see
    /// [`cctop_core::yolo::FALLBACK_AFTER`].
    pub(super) fn tick_yolo(&mut self, now: bool) -> bool {
        const EVERY: std::time::Duration = std::time::Duration::from_millis(500);
        if !now && self.yolo_at.is_some_and(|at| at.elapsed() < EVERY) {
            return false;
        }
        self.yolo_at = Some(std::time::Instant::now());
        let changed = self.yolo.tick(&mut self.sessions, &self.reports);
        self.needs_redraw |= changed;
        changed
    }

    /// What the screen of the agent running as `pid` last said, from whichever
    /// of the two sources has it.
    ///
    /// The detached sweep wins, as it did when its answers were copied over the
    /// pane reads into one map — a pid it can name is a pane rmux owns the
    /// screen of, not one this process has a parser for.
    fn screened(&self, pid: u32) -> Option<&cctop_core::peek::Screened> {
        self.peeked.get(&pid).or_else(|| self.screen_read.get(&pid))
    }

    /// Read every tab's agent off its screen, when the setting asks for it. True when any verdict changed, so the rows are restamped.
    ///
    /// The hooks are how an agent says what it is doing, and they can fail in
    /// ways that look exactly like an agent with nothing to say: not installed,
    /// installed at a binary that has since moved, a session started before
    /// they were, a permission prompt announced six seconds late. The screen
    /// has none of those failure modes, only its own — it has to be one cctop
    /// holds, and in words cctop knows — which is why it is opt-in rather than
    /// the default.
    ///
    /// A detached rmux tab's screen is read too, just on a slower clock: each
    /// one is a `capture-pane` of a session this cctop holds no parser for,
    /// where an attached pane's screen is already in memory. Both answers are
    /// merged by [`Self::screened`] — a pid knows one verdict however its screen
    /// was reached — rather than into one map rebuilt every tick.
    pub(super) fn read_screens(&mut self) -> bool {
        if self.settings.read_screen != Some(true) {
            let had = !self.screen_read.is_empty() || !self.peeked.is_empty();
            self.screen_read.clear();
            self.peeked.clear();
            self.peeked_at = None;
            self.peeked_listing = None;
            return had;
        }
        let read: HashMap<u32, cctop_core::peek::Screened> = self
            .tabs
            .iter_mut()
            .flat_map(|tab| tab.panes.iter_mut())
            .filter_map(|pane| Some((pane.agent(), pane.read_screen()?)))
            .collect();
        // A detached sweep that has landed. Its answers are merged at the lookup
        // rather than copied into `screen_read` every tick: with two panes at
        // 60 Hz that is 120 copies a second of a map nothing has changed, and
        // the comparison that decides whether a row is restamped wanted the
        // answer to have moved, not a fresh copy of it.
        let mut peeked_changed = false;
        if let Some(rx) = &self.peeked_listing {
            match rx.try_recv() {
                Ok(peeked) => {
                    self.peeked_listing = None;
                    peeked_changed = peeked != self.peeked;
                    self.peeked = peeked;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.peeked_listing = None,
            }
        }
        let due = self.peeked_at.is_none_or(|at| at.elapsed() >= PEEK_EVERY);
        if due && self.peeked_listing.is_none() {
            self.peeked_at = Some(Instant::now());
            let names: Vec<(u32, String)> = self
                .tabs
                .iter()
                .filter_map(|tab| {
                    Some((tab.shared.as_ref()?.pid?, tab.shared.as_ref()?.name.clone()))
                })
                .collect();
            let (tx, rx) = std::sync::mpsc::channel();
            self.peeked_listing = Some(rx);
            std::thread::spawn(move || {
                // Each one is a `capture-pane` subprocess, so the sweep is off
                // the draw loop: a slow rmux delays the reading, never the frame.
                let peeked = names
                    .into_iter()
                    .filter_map(|(pid, name)| Some((pid, cctop_core::peek::named(&name)?)))
                    .collect();
                let _ = tx.send(peeked);
            });
        }
        // The pane half on its own: it is what moved, and comparing it is what
        // says whether anything needs restamping. A changed detached reading
        // counts too, which is why both are folded in before the verdict.
        let changed = peeked_changed || read != self.screen_read;
        self.screen_read = read;
        changed
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
                    sub.status = cctop_core::session::SubagentStatus::Done;
                }
            }
        }
    }

    /// What a session's own hooks last said about it, if it has any.
    pub(super) fn hooked_signal(&self, session_id: &str) -> Option<cctop_core::hook::Signal> {
        // Checked here as well as in the sweep: the sweep runs when an event
        // arrives, and a session that has gone silent is precisely the one that
        // sends none — so between events the map still holds the stale claim.
        self.reports
            .report(session_id)
            .filter(|reported| reported.is_current() && reported.is_settled())
            .map(|reported| reported.signal)
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
    pub(super) fn pane_signal(&self, pid: u32) -> Option<cctop_core::hook::Signal> {
        let screen = self.screened(pid).map(|read| read.signal);
        screen.or_else(|| self.reported_by(pid)).or_else(|| {
            self.sessions
                .iter()
                .filter(|session| session.root_pid() == Some(pid))
                .find_map(|session| {
                    match session.activity_state {
                        // Both of the row's waiting states are a tab worth
                        // colouring; which colour is the caller's business.
                        cctop_core::session::ActivityState::Asking => {
                            Some(cctop_core::hook::Signal::NeedsInput)
                        }
                        cctop_core::session::ActivityState::WaitingForInput => {
                            Some(cctop_core::hook::Signal::Idle)
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
    pub(super) fn reported_by(&self, pid: u32) -> Option<cctop_core::hook::Signal> {
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
    /// [`rmux::set_state`](cctop_core::rmux::set_state) for why the agent's deadline
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
    pub(super) fn publish_states(&self, running: &[cctop_core::rmux::Running]) {
        for (name, signal) in self.states_to_publish(running, cctop_core::rmux::now_secs()) {
            cctop_core::rmux::set_state(&name, signal);
        }
    }

    /// Which sessions are out of date and what to say about them — the half of
    /// [`Self::publish_states`] with the judgement in it, split out so it can be
    /// tested without a daemon to write to.
    pub(super) fn states_to_publish(
        &self,
        running: &[cctop_core::rmux::Running],
        now: u64,
    ) -> Vec<(String, cctop_core::hook::Signal)> {
        if self.reports.hooked.is_empty() {
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
    /// state: [`Signal::Acting`](cctop_core::hook::Signal::Acting) plus a still
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
            if let Some(reported) = self.reports.hooked.get_mut(&id)
                && reported.signal.awaits_you()
            {
                reported.signal = cctop_core::hook::Signal::Busy;
            }
        }
    }

    /// Whether any hidden tab is explicitly waiting for input and should blink.
    pub fn any_attention(&self) -> bool {
        (1..=self.tabs.len()).any(|i| self.tab_attention(i) == Some(tabs::Attention::NeedsInput))
    }

    /// Which half of the blink cycle we are in. The pulse's fallback, for the
    /// tabs whose colours cannot be eased between — see [`effects`].
    pub fn blink_on(&self) -> bool {
        (self.started.elapsed().as_millis() / BLINK_MS).is_multiple_of(2)
    }

    /// Where the needs-you pulse is, `0.0` at rest to `1.0` at its alert tone.
    /// On the same clock as the blink, so every tab asking pulses in step.
    pub fn pulse_level(&self) -> f32 {
        effects::pulse_level(self.started.elapsed())
    }

    /// How long ago tab `index` (in [`App::tab`]'s numbering) was restarted,
    /// while its sweep is still running.
    pub fn restart_flash(&self, index: usize) -> Option<Duration> {
        let tab = self.tabs.get(index.checked_sub(1)?)?;
        let since = tab.restarted?.elapsed();
        (since < effects::FLASH && !theme::no_color()).then_some(since)
    }

    /// Whether anything in the tab bar is moving, and the loop should be
    /// producing frames rather than waiting on input.
    ///
    /// Without colour there is nothing to ease: the pulse is the blink again,
    /// which asks for its own frames on the half-cycle it flips, and a restart
    /// is said by the status line alone.
    pub fn animating(&self) -> bool {
        (1..=self.tabs.len()).any(|i| self.restart_flash(i).is_some())
            || (!theme::no_color() && self.any_attention())
    }

    /// What a still-running agent in the launcher is doing, if it has said.
    ///
    /// This is the whole reason the offer carries a pid. A list of rmux session
    /// names says which agents exist; this says which one is stuck on a question
    /// and which finished ten minutes ago, from the same hooks the dashboard
    /// reads — so choosing which to go back to is a decision rather than a guess.
    pub fn waiting_state(
        &self,
        agent: &cctop_core::rmux::Running,
    ) -> Option<cctop_core::hook::Signal> {
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
    pub fn waiting_label(&self, agent: &cctop_core::rmux::Running) -> Option<String> {
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

    /// One hook event for session `a`, from `agent` when it is a subagent's.
    fn heard(signal: cctop_core::hook::Signal, agent: Option<&str>) -> cctop_core::hook::Event {
        cctop_core::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                signal,
                cwd: "/w/proj".into(),
                permission: None,
                ask: None,
                question: false,
                at: std::time::Instant::now(),
                provisional: false,
            },
            finished_agent: None,
            sandbox: None,
            agent: agent.map(str::to_string),
        }
    }

    /// The bug: two subagents running side by side, one stopped on a
    /// permission prompt — and the other's next tool call overwrote the
    /// question, so the parent's tab stopped asking while the prompt was still
    /// up. The question stands until the subagent that asked it moves on.
    #[test]
    fn a_subagents_question_outlives_what_its_sibling_does_next() {
        use cctop_core::hook::Signal::{Acting, Busy, Idle, NeedsInput};
        let mut app = test_app();
        let now = |app: &App| app.reports.hooked.get("a").map(|r| r.signal);

        app.apply_hooks(vec![heard(NeedsInput, Some("sub-1"))]);
        app.apply_hooks(vec![heard(Acting, Some("sub-2"))]);
        assert_eq!(
            now(&app),
            Some(NeedsInput),
            "a sibling's tool call hid the question"
        );
        app.apply_hooks(vec![heard(Busy, None)]);
        assert_eq!(
            now(&app),
            Some(NeedsInput),
            "the parent's own work hid the question"
        );

        // Answered: the subagent that asked runs its tool.
        app.apply_hooks(vec![heard(Busy, Some("sub-1"))]);
        assert_eq!(now(&app), Some(Busy));

        // Dismissed with Esc, which sends nothing from the subagent: the
        // parent's turn ending is what closes it.
        app.apply_hooks(vec![heard(NeedsInput, Some("sub-1"))]);
        app.apply_hooks(vec![heard(Idle, None)]);
        assert_eq!(now(&app), Some(Idle), "a dismissed question stayed up");
        assert!(!app.reports.asking_agents.contains_key("a"));
    }
    use crate::tests::{session, test_app};
    /// What gets written onto a session, and — mostly — what does not.
    ///
    /// The sweep runs every [`SHARE_EVERY`] and a turn is largely the same
    /// signal repeated, so "the session already says this" has to be the common
    /// answer. Each write is a process spawn and a round trip to the daemon;
    /// doing one per sweep per tab would make the tab bar pay for the feature.
    #[test]
    fn a_session_is_only_written_to_when_it_is_out_of_date() {
        let agent = |state| cctop_core::rmux::Running {
            name: "cctop-a".into(),
            pid: Some(4321),
            cwd: None,
            attached: false,
            activity: None,
            label: None,
            profile: None,
            order: None,
            state,
            color: None,
            tab: None,
            pane: None,
            axis: None,
            window: None,
        };
        let now = 1_700_000_000;
        let recorded = |signal, ago: u64| {
            Some(cctop_core::rmux::State {
                signal,
                at: now - ago,
            })
        };

        let mut app = test_app();
        let mut row = session("a", true, "proj");
        row.process.as_mut().unwrap().process_list = vec![cctop_core::proc::ProcEntry {
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

        app.apply_hooks(vec![cctop_core::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                signal: cctop_core::hook::Signal::NeedsInput,
                cwd: "/w/proj".into(),
                permission: None,
                ask: None,
                question: false,
                at: std::time::Instant::now(),
                provisional: false,
            },
            finished_agent: None,
            sandbox: None,
            agent: None,
        }]);

        assert_eq!(
            app.states_to_publish(&[agent(None)], now),
            vec![("cctop-a".to_string(), cctop_core::hook::Signal::NeedsInput)],
            "a session saying nothing is told"
        );
        assert!(
            app.states_to_publish(
                &[agent(recorded(cctop_core::hook::Signal::NeedsInput, 30))],
                now
            )
            .is_empty(),
            "a session already saying it is left alone"
        );
        assert_eq!(
            app.states_to_publish(&[agent(recorded(cctop_core::hook::Signal::Busy, 30))], now)
                .len(),
            1,
            "a session saying something else is corrected"
        );
        // A row whose pid nothing has reported for is not published from
        // somebody else's report.
        assert!(
            app.states_to_publish(
                &[cctop_core::rmux::Running {
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
        app.apply_hooks(vec![cctop_core::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                signal: cctop_core::hook::Signal::Busy,
                cwd: "/w/proj".into(),
                permission: None,
                ask: None,
                question: false,
                at: std::time::Instant::now(),
                provisional: false,
            },
            finished_agent: None,
            sandbox: None,
            agent: None,
        }]);
        assert!(
            app.states_to_publish(&[agent(recorded(cctop_core::hook::Signal::Busy, 30))], now)
                .is_empty(),
            "a fresh agreement needs no write"
        );
        assert_eq!(
            app.states_to_publish(
                &[agent(recorded(cctop_core::hook::Signal::Busy, 3 * 3_600))],
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
        let event = |signal| cctop_core::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                provisional: false,
                signal,
                cwd: "/w/proj".into(),
                permission: None,
                ask: None,
                question: false,
                at: std::time::Instant::now(),
            },
            finished_agent: None,
            sandbox: None,
            agent: None,
        };
        let mut app = test_app();
        app.sessions = vec![session("a", true, "proj")];
        // What the walk left behind: a tool call in flight.
        app.sessions[0].activity_state = cctop_core::session::ActivityState::Working;

        app.apply_hooks(vec![event(cctop_core::hook::Signal::NeedsInput)]);
        assert_eq!(
            app.sessions[0].activity_state,
            cctop_core::session::ActivityState::Asking
        );

        app.apply_hooks(vec![event(cctop_core::hook::Signal::Idle)]);
        assert_eq!(
            app.sessions[0].activity_state,
            cctop_core::session::ActivityState::WaitingForInput,
            "a finished turn is the quieter of the two waits"
        );

        // A working report leaves the row alone rather than overwriting it: the
        // transcript reads work correctly and is the only one of the two that
        // can see an API error.
        app.sessions[0].activity_state = cctop_core::session::ActivityState::ApiError;
        app.apply_hooks(vec![event(cctop_core::hook::Signal::Busy)]);
        assert_eq!(
            app.sessions[0].activity_state,
            cctop_core::session::ActivityState::ApiError
        );
    }

    /// The whole way from bytes on a pane to a verdict: a Claude tab whose
    /// footer holds a prompt reads as asking, but only with the setting on; the
    /// same words are a turn in flight in Gemini; and a shell is not read.
    #[test]
    fn a_tab_is_read_off_its_screen_when_asked() {
        use super::tabs::{Pane, Tab};
        use cctop_core::hook::Signal;
        let footer = " Do you want to proceed?\r\n \u{276f} 1. Yes\r\n\r\n Esc to cancel \u{b7} Tab to amend";
        let tab = |label: &str, text: &str| {
            let mut pane = Pane::for_test(label);
            pane.view.parser.process(text.as_bytes());
            Tab::new(pane)
        };
        let mut app = test_app();
        app.tabs = vec![tab("claude", footer)];
        assert!(!app.read_screens(), "off unless the file says so");
        assert!(app.screen_read.is_empty());

        app.settings.read_screen = Some(true);
        assert!(app.read_screens(), "a new verdict is a change");
        assert_eq!(
            app.screen_read.get(&4321).map(|s| s.signal),
            Some(Signal::NeedsInput)
        );
        assert_eq!(app.pane_signal(4321), Some(Signal::NeedsInput));
        assert!(!app.read_screens(), "the same verdict twice is not");

        app.tabs = vec![tab("gemini", " \u{280f} Reading files (esc to cancel, 3s)")];
        assert!(app.read_screens());
        assert_eq!(
            app.screen_read.get(&4321).map(|s| s.signal),
            Some(Signal::Busy),
            "Gemini's working hint"
        );

        app.tabs = vec![tab("zsh", footer)];
        assert!(app.read_screens());
        assert!(app.screen_read.is_empty(), "a shell has no footer to read");
    }

    /// With the screen read, what it shows outranks what the hook last said —
    /// in both directions — and a screen with nothing recognisable on it
    /// leaves the hook's word standing.
    #[test]
    fn the_screen_outranks_a_stale_report() {
        use cctop_core::hook::Signal;
        use cctop_core::session::ActivityState;
        let mut app = test_app();
        let mut row = session("a", true, "proj");
        row.process.as_mut().unwrap().process_list = vec![cctop_core::proc::ProcEntry {
            pid: 4321,
            is_root: true,
            ghost: false,
            cpu: 0.0,
            memory: 0,
            args: String::new(),
        }];
        app.sessions = vec![row];
        app.apply_hooks(vec![cctop_core::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                signal: Signal::Idle,
                cwd: "/w/proj".into(),
                permission: None,
                ask: None,
                question: false,
                at: std::time::Instant::now(),
                provisional: false,
            },
            finished_agent: None,
            sandbox: None,
            agent: None,
        }]);
        app.apply_reports();
        assert_eq!(
            app.sessions[0].activity_state,
            ActivityState::WaitingForInput
        );

        // A permission prompt the hook has not announced yet.
        app.screen_read.insert(
            4321,
            cctop_core::peek::Screened {
                signal: Signal::NeedsInput,
                ask: None,
                question: false,
            },
        );
        app.apply_reports();
        assert_eq!(app.sessions[0].activity_state, ActivityState::Asking);
        assert_eq!(app.pane_signal(4321), Some(Signal::NeedsInput));

        // Answered, and back to work: the waiting state goes with it.
        app.screen_read.insert(
            4321,
            cctop_core::peek::Screened {
                signal: Signal::Busy,
                ask: None,
                question: false,
            },
        );
        app.apply_reports();
        assert_eq!(app.sessions[0].activity_state, ActivityState::Working);

        // Nothing on screen to read: the hook's word again.
        app.screen_read.clear();
        app.apply_reports();
        assert_eq!(
            app.sessions[0].activity_state,
            ActivityState::WaitingForInput
        );
        assert_eq!(app.pane_signal(4321), Some(Signal::Idle));
    }

    /// The mode is reported by a live agent but drawn on a row rebuilt by every
    /// walk, so the two have to survive arriving in either order.
    #[test]
    fn the_permission_mode_survives_the_rows_being_rebuilt() {
        let reported = |mode: Option<cctop_core::hook::Permission>| cctop_core::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                provisional: false,
                signal: cctop_core::hook::Signal::Busy,
                cwd: "/w/proj".into(),
                permission: mode,
                ask: None,
                question: false,
                at: std::time::Instant::now(),
            },
            finished_agent: None,
            sandbox: None,
            agent: None,
        };
        let mut app = test_app();

        // Reported before the row exists, which is the ordinary order: a
        // `SessionStart` beats the walk that discovers its transcript.
        app.apply_hooks(vec![reported(Some(cctop_core::hook::Permission::Bypass))]);
        app.sessions = vec![session("a", true, "proj")];
        assert_eq!(app.sessions[0].permission, None, "not stamped yet");

        app.apply_reports();
        assert_eq!(
            app.sessions[0].permission,
            Some(cctop_core::hook::Permission::Bypass),
            "a row discovered after the report still picks it up"
        );

        // An event that says nothing about the mode must not erase it: the
        // setting has not changed just because one event was quiet.
        app.apply_hooks(vec![reported(None)]);
        assert_eq!(
            app.sessions[0].permission,
            Some(cctop_core::hook::Permission::Bypass)
        );

        // A real change is followed.
        app.apply_hooks(vec![reported(Some(cctop_core::hook::Permission::Plan))]);
        assert_eq!(
            app.sessions[0].permission,
            Some(cctop_core::hook::Permission::Plan)
        );
    }

    /// A reported state is kept until the session says it is over, and the
    /// events that change *which sessions exist* ask for a rescan while the
    /// ones that only change a state do not.
    #[test]
    fn a_reported_state_is_kept_until_the_session_ends() {
        let event = |id: &str, signal: cctop_core::hook::Signal| cctop_core::hook::Event {
            session_id: id.into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                provisional: false,
                signal,
                cwd: "/w/proj".into(),
                permission: None,
                ask: None,
                question: false,
                at: std::time::Instant::now(),
            },
            // This test is about the session's own state; subagent events are
            // covered where subagents are.
            finished_agent: None,
            sandbox: None,
            agent: None,
        };
        let mut app = test_app();

        // Nothing arriving is not "nothing is happening": an absent entry means
        // fall back to the transcript, so it must stay absent.
        assert_eq!(app.apply_hooks(Vec::new()), (false, false));
        assert!(app.hooked_signal("a").is_none());

        // A start is a row to go and find now.
        assert_eq!(
            app.apply_hooks(vec![event("a", cctop_core::hook::Signal::Started)]),
            (true, true)
        );
        // Ordinary state changes are not worth a rescan of the disk.
        assert_eq!(
            app.apply_hooks(vec![
                event("a", cctop_core::hook::Signal::Busy),
                event("a", cctop_core::hook::Signal::Idle),
            ]),
            (true, false)
        );
        assert_eq!(app.hooked_signal("a"), Some(cctop_core::hook::Signal::Idle));
        assert_eq!(app.reporting(), vec![("proj".to_string(), "idle")]);

        // And an ended session is forgotten rather than left claiming its last
        // state forever — which is also a rescan, since the row is going.
        assert_eq!(
            app.apply_hooks(vec![event("a", cctop_core::hook::Signal::Ended)]),
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
        let stale = |id: &str, signal: cctop_core::hook::Signal| cctop_core::hook::Event {
            session_id: id.into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                provisional: false,
                signal,
                cwd: "/w/proj".into(),
                permission: None,
                ask: None,
                question: false,
                at: std::time::Instant::now() - std::time::Duration::from_secs(60 * 60),
            },
            finished_agent: None,
            sandbox: None,
            agent: None,
        };
        let mut app = test_app();

        // An hour-old tool call in flight: the tool is not coming back, and
        // reading it as a held question is how a dead tab keeps blinking.
        app.apply_hooks(vec![stale("gone", cctop_core::hook::Signal::Acting)]);
        assert!(app.hooked_signal("gone").is_none());
        assert!(
            app.reporting().is_empty(),
            "the panel listed a state nothing was in"
        );

        // A question that old is still a question: it is waiting on a person,
        // and people take longer than an hour.
        app.apply_hooks(vec![stale("asking", cctop_core::hook::Signal::NeedsInput)]);
        assert_eq!(
            app.hooked_signal("asking"),
            Some(cctop_core::hook::Signal::NeedsInput)
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
        app.apply_hooks(vec![cctop_core::hook::Event {
            session_id: "79709c93-1111-4111-8111-111111111111".into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                provisional: false,
                signal: cctop_core::hook::Signal::Idle,
                cwd: "/w/proj".into(),
                permission: None,
                ask: None,
                question: false,
                at: std::time::Instant::now(),
            },
            finished_agent: None,
            sandbox: None,
            agent: None,
        }]);

        assert_eq!(
            app.hooked_signal("session-2026-05-14T17-34-79709c93"),
            Some(cctop_core::hook::Signal::Idle)
        );
        // And only that one: a stem whose tail belongs to another session, or an
        // id that is not shaped like Gemini's at all, must not borrow it.
        assert!(
            app.hooked_signal("session-2026-05-14T17-34-deadbeef")
                .is_none()
        );
        assert!(app.hooked_signal("79709c93").is_none());
        assert_eq!(
            cctop_core::hook::gemini_id_tail("session-2026-05-14T17-34-79709c93"),
            Some("79709c93")
        );
        assert_eq!(
            cctop_core::hook::gemini_id_tail("019fda22-5315-7580-84de-033e4f6835b5"),
            None
        );
    }

    /// Answering a prompt in a pane stops the tab asking about it, without
    /// waiting for a hook that only fires once the unblocked tool has finished.
    #[test]
    fn typing_into_a_pane_settles_the_question_it_answers() {
        let mut app = test_app();
        let mut session = session("a", true, "proj");
        session.process.as_mut().unwrap().process_list = vec![cctop_core::proc::ProcEntry {
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

        app.apply_hooks(vec![cctop_core::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                provisional: false,
                signal: cctop_core::hook::Signal::NeedsInput,
                cwd: "/w/proj".into(),
                permission: None,
                ask: None,
                question: false,
                at: std::time::Instant::now(),
            },
            finished_agent: None,
            sandbox: None,
            agent: None,
        }]);
        assert_eq!(
            app.hooked_signal("a"),
            Some(cctop_core::hook::Signal::NeedsInput)
        );

        // The keystroke is the answer, so the agent is working again.
        app.mark_answered(7);
        assert_eq!(app.hooked_signal("a"), Some(cctop_core::hook::Signal::Busy));

        // A different agent's keys settle nothing here.
        app.apply_hooks(vec![cctop_core::hook::Event {
            session_id: "a".into(),
            pids: Vec::new(),
            reported: cctop_core::hook::Reported {
                provisional: false,
                signal: cctop_core::hook::Signal::NeedsInput,
                cwd: "/w/proj".into(),
                permission: None,
                ask: None,
                question: false,
                at: std::time::Instant::now(),
            },
            finished_agent: None,
            sandbox: None,
            agent: None,
        }]);
        app.mark_answered(8);
        assert_eq!(
            app.hooked_signal("a"),
            Some(cctop_core::hook::Signal::NeedsInput)
        );
    }

    /// A question's own words survive the follow-up event that does not carry
    /// them, and are dropped with the prompt itself.
    ///
    /// Claude Code fires `PermissionRequest` with the tool named, then a
    /// `permission_prompt` notification for the same question that names
    /// nothing — only the first knows what the prompt is about.
    #[test]
    fn a_questions_detail_survives_its_followup_notification() {
        use cctop_core::hook::{Event, Reported, Signal};

        let raised = |signal: Signal, ask: Option<&str>| Event {
            session_id: "a".to_string(),
            pids: Vec::new(),
            finished_agent: None,
            sandbox: None,
            agent: None,
            reported: Reported {
                signal,
                cwd: String::new(),
                permission: None,
                ask: ask.map(str::to_string),
                question: false,
                at: std::time::Instant::now(),
                provisional: false,
            },
        };

        let mut app = test_app();
        app.sessions = vec![session("a", true, "proj")];
        app.apply_hooks(vec![raised(Signal::NeedsInput, Some("Bash: rm -rf build"))]);
        // Claude Code's `permission_prompt` notification: same question, no tool.
        app.apply_hooks(vec![raised(Signal::NeedsInput, None)]);
        assert_eq!(
            app.sessions[0].asking_for.as_deref(),
            Some("Bash: rm -rf build")
        );

        app.apply_hooks(vec![raised(Signal::Busy, None)]);
        assert_eq!(app.sessions[0].asking_for, None);
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
        use cctop_core::hook::{Event, Reported, Signal};
        use cctop_core::session::ActivityState;

        let raised = |signal: Signal, provisional: bool| Event {
            session_id: "a".to_string(),
            pids: Vec::new(),
            finished_agent: None,
            sandbox: None,
            agent: None,
            reported: Reported {
                signal,
                cwd: String::new(),
                permission: None,
                ask: None,
                question: false,
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
        let held = app.reports.hooked.get_mut("a").expect("the report");
        held.at = std::time::Instant::now()
            - (cctop_core::hook::PERMISSION_GRACE + std::time::Duration::from_secs(1));
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
        app.notify.record_for_test(cctop_core::notify::Rang {
            key: target.clone(),
            label: "b".into(),
            reason: cctop_core::notify::Reason::NeedsInput,
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
        assert!(app.status().is_some());
    }

    /// An alert reaches the user as a toast on the refresh that sees the
    /// crossing, and the row keeps its marker after the toast has gone — but
    /// the next refresh, still over, says nothing new.
    #[test]
    fn an_alert_crossing_is_toasted_once_and_marked_while_it_holds() {
        let mut app = test_app();
        app.settings = cctop_core::settings::Settings::parse("[settings]\nalert_cost = 5\n");
        app.sessions = vec![session("a", true, "proj")];
        app.sessions[0].total_cost = Some(1.0);
        app.check_bells();
        assert_eq!(app.toasts.latest(), None);

        app.sessions[0].total_cost = Some(6.0);
        app.check_bells();
        let said = app.toasts.latest().map(str::to_string);
        assert!(
            said.as_deref().is_some_and(|t| t.contains("$6.00")),
            "{said:?}"
        );
        let key = app.sessions[0].key();
        assert_eq!(app.alerts.marker(&key), Some(cctop_core::alert::Kind::Cost));

        app.toasts = toast::Toasts::default();
        app.check_bells();
        assert_eq!(app.toasts.latest(), None, "still over is not news");
        assert_eq!(app.alerts.marker(&key), Some(cctop_core::alert::Kind::Cost));
    }

    /// A session finishing its turn on the same refresh an alert fires is one
    /// moment, and it used to be two bells and two desktop notifications. It
    /// is one now, naming both.
    #[test]
    fn a_crossing_and_an_alert_on_one_refresh_ring_once() {
        let mut app = test_app();
        app.notify.enabled = true;
        app.settings = cctop_core::settings::Settings::parse("[settings]\nalert_cost = 5\n");
        app.sessions = vec![session("a", true, "api"), session("b", true, "web")];
        app.sessions[1].total_cost = Some(1.0);
        app.check_bells();
        app.notify.ring_pending();
        assert!(
            cctop_core::notify::take_rung().is_empty(),
            "nothing has happened"
        );

        app.sessions[0].activity_state = cctop_core::session::ActivityState::WaitingForInput;
        app.sessions[1].total_cost = Some(6.0);
        app.check_bells();
        app.notify.ring_pending();
        let rung = cctop_core::notify::take_rung();
        assert_eq!(rung.len(), 1, "one refresh, one bell: {rung:?}");
        assert!(
            rung[0].starts_with("cctop: ") && rung[0].contains("waiting for input · "),
            "{rung:?}"
        );
        assert!(rung[0].contains("$6.00"), "the alert went unsaid: {rung:?}");

        // And the next pass, with nothing new, is silent.
        app.check_bells();
        app.notify.ring_pending();
        assert!(cctop_core::notify::take_rung().is_empty());
    }

    /// The stall alert reads the hooks' last word even once the row has
    /// stopped believing it, and a finished turn is never a stall.
    #[test]
    fn a_stall_reads_the_last_hook_report_however_old() {
        use cctop_core::alert::Hooked;
        use cctop_core::hook::Signal;
        let report = |signal, age| cctop_core::hook::Reported {
            signal,
            cwd: String::new(),
            permission: None,
            ask: None,
            question: false,
            at: Instant::now() - std::time::Duration::from_secs(age),
            provisional: false,
        };
        let mut reports = cctop_core::hook::Reports::default();
        assert_eq!(stall_view(&reports, "a"), Hooked::Unknown);
        reports
            .hooked
            .insert("a".to_string(), report(Signal::Busy, 3_600));
        assert_eq!(stall_view(&reports, "a"), Hooked::Working);
        reports
            .hooked
            .insert("a".to_string(), report(Signal::Acting, 0));
        assert_eq!(stall_view(&reports, "a"), Hooked::InFlight);
        reports
            .hooked
            .insert("a".to_string(), report(Signal::Idle, 0));
        assert_eq!(stall_view(&reports, "a"), Hooked::Quiet);
    }
}
