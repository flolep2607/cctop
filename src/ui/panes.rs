//! The workspace tab bar: tabs, panes, and which of them is on screen.
//!
//! Tab 0 is the dashboard and never closes, so every index here is one past the
//! `tabs` vector — the arithmetic that keeps the cursor, the bar and the focused
//! pane agreeing after a close, a drag or a workspace switch is the whole reason
//! this is not spread across its callers. Shared tabs reconciled from the
//! multiplexer land here too: they occupy the same bar and obey the same
//! landing rules as tabs this cctop opened itself.

use super::*;

/// How often the tab bar is reconciled against the rmux sessions on this
/// machine, so a tab opened in one cctop shows up in the others.
///
/// It costs a `rmux list-panes`, so it cannot ride the draw loop. Two seconds is
/// short enough that a tab opened next door is there before you have switched
/// windows to look for it, and long enough that the subprocess is nothing.
pub(super) const SHARE_EVERY: Duration = Duration::from_secs(2);

impl App {
    /// The tab on screen, or `None` on the dashboard.
    pub fn active_tab(&mut self) -> Option<&mut tabs::Tab> {
        self.tabs.get_mut(self.tab.checked_sub(1)?)
    }

    /// The pane the keyboard belongs to, or `None` on the dashboard.
    pub fn focused_pane(&mut self) -> Option<&mut tabs::Pane> {
        self.active_tab()?.focused_mut()
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

    /// Write the bar's arrangement onto the sessions, so it survives this cctop.
    ///
    /// Called when a rearrangement finishes rather than while one is happening:
    /// a drag calls [`App::move_tab`] on every pointer movement, and a
    /// `set-option` per tab per movement is a subprocess storm for an
    /// arrangement that is still being decided.
    ///
    /// Every session of every tab, including both halves of a split, so a tab
    /// with two agents in it comes back as one tab rather than as two in
    /// whichever order they were started.
    pub(super) fn save_tab_order(&self) {
        for (at, tab) in self.tabs.iter().enumerate() {
            for name in tab.sessions() {
                crate::rmux::set_order(name, at);
            }
        }
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
        // One keystroke is one finished rearrangement, unlike a drag.
        self.save_tab_order();
    }

    pub fn cycle_workspace(&mut self, delta: isize) {
        let count = self.tabs.len() as isize + 1;
        self.go_to_tab((self.tab as isize + delta).rem_euclid(count) as usize);
    }

    /// The next bar position after the view whose agent wants you, wrapping —
    /// `None` when no tab does.
    ///
    /// Two passes, loudest first: every tab blocked on a question, and only
    /// then a turn that finished while you were not looking. A question is an
    /// agent standing still until you answer; a finished turn is news that
    /// keeps. Plain `Idle` is never a jump — it is context the bar already
    /// carries in green, and it is most tabs most of the time.
    ///
    /// The current tab is checked last rather than skipped: a split's other
    /// pane can be the one asking, and you cannot tell from the bar.
    fn next_waiting(&self) -> Option<usize> {
        let next = |want: tabs::Attention| {
            let wants = |i: usize| self.tab_attention(i) == Some(want);
            (self.tab + 1..=self.tabs.len())
                .find(|&i| wants(i))
                .or_else(|| (1..self.tab).find(|&i| wants(i)))
                .or_else(|| wants(self.tab).then_some(self.tab))
        };
        next(tabs::Attention::NeedsInput).or_else(|| next(tabs::Attention::Done))
    }

    /// `Alt+z`: fill the tab with the focused pane, or put the split back.
    pub fn toggle_zoom(&mut self) {
        let Some(tab) = self.active_tab() else {
            return;
        };
        match tab.toggle_zoom() {
            Some(true) => self.set_status("Zoomed — Alt+z puts the split back"),
            Some(false) => self.set_status("Unzoomed"),
            None => self.set_status("Only one pane — nothing to zoom it over"),
        }
        self.needs_redraw = true;
    }

    /// Jump the view to the next tab that wants you — `Alt+b`, beside the
    /// dashboard's `b`, which answers the same question for the table.
    pub fn next_waiting_tab(&mut self) {
        match self.next_waiting() {
            Some(tab) if tab != self.tab => self.go_to_tab(tab),
            // The only ask in the bar is inside the tab already on screen:
            // a split's other pane. `go_to_tab` would wave the tab through
            // as already there, so the keyboard moves instead. Each press
            // visits the next pane, so an asking one always comes round.
            Some(_) => {
                if let Some(tab) = self.active_tab() {
                    tab.cycle_focus();
                }
            }
            None => self.set_status("Nothing is waiting for you"),
        }
    }

    /// The bar positions the switcher's filter leaves standing, `0` for the
    /// dashboard.
    ///
    /// A plain substring, case-insensitive: the field is typed a letter or
    /// two at a time, and a scored fuzzy match is a ranking nobody asked
    /// for across a dozen names.
    pub(super) fn switch_matches(&self) -> Vec<usize> {
        let needle = self.switch_filter.trim().to_lowercase();
        let matches = |title: &str| needle.is_empty() || title.to_lowercase().contains(&needle);
        let mut found = Vec::new();
        if matches("Dashboard") {
            found.push(0);
        }
        for (i, tab) in self.tabs.iter().enumerate() {
            if matches(&tab.title()) {
                found.push(i + 1);
            }
        }
        found
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
            // The colour is asked of every tab, not only the detached ones: it
            // is the one property of a tab another cctop can change while this
            // one is still holding the pane. Everything else the sweep updates
            // lives on `shared`, which only detached tabs have.
            let painted = tab
                .sessions()
                .find_map(|name| running.iter().find(|a| a.name == name))
                .map(|agent| agent.color.as_deref().and_then(theme::Hue::from_name));
            if let Some(color) = painted {
                tab.color = color;
            }
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

        self.publish_states(&running);

        let mine = self.open_rmux();
        let mut arrived = false;
        for agent in crate::rmux::in_tab_order_of(running.clone()) {
            let agent = &agent;
            if mine.iter().any(|name| name == &agent.name) {
                continue;
            }
            self.tabs.push(tabs::Tab::shared(agent));
            arrived = true;
        }
        self.needs_redraw |= !retired.is_empty() || arrived;
    }

    /// Start recording the focused pane to a `.cast` file, or stop and say
    /// where it went — `Alt+Shift+C`.
    ///
    /// The focused pane, not the whole tab: a split is two terminals of two
    /// sizes, and a cast is one terminal. Each half can be recorded on its own.
    pub fn toggle_recording(&mut self) {
        let Some(pane) = self.focused_pane() else {
            self.set_status("Recording is for a tab's terminal — open one first");
            return;
        };
        if let Some((path, finished)) = pane.view.stop_recording() {
            self.set_status(crate::cast::stopped_message(&path, &finished));
            return;
        }
        let label = pane.label.clone();
        let said = match pane.view.start_recording(&label) {
            Ok(_) => format!("Recording {label} — Alt+Shift+C again to stop"),
            Err(e) => format!("Could not start recording {label}: {e}"),
        };
        self.set_status(said);
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
        let mut pane = tab.panes.remove(tab.focus);
        tab.focus = tab.focus.min(tab.panes.len().saturating_sub(1));
        let label = pane.label.clone();
        let stopped = pane.owns_agent().then(|| pane.kill_agent());
        // Stopped before the pane goes rather than left to its drop, so where
        // the file went is said: closing is how most recordings will end.
        let recorded = pane.view.stop_recording();
        drop(pane);
        self.drop_empty_tabs();
        if let Some((path, finished)) = recorded {
            self.set_status(crate::cast::stopped_message(&path, &finished));
        }

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
    pub(super) fn land_after(&mut self, was: usize, gone: &[usize]) {
        let want = Self::landing(was, gone, self.tabs.len());
        // `go_to_tab` answers a move to where it already is with nothing at
        // all, and here the field still holds the number of a tab that is gone.
        self.tab = 0;
        self.go_to_tab(want);
    }

    /// The tab number to land on, kept separate from the move itself so the
    /// arithmetic can be checked without a rmux server to attach to.
    pub(super) fn landing(was: usize, gone: &[usize], remaining: usize) -> usize {
        let before = gone.iter().filter(|&&i| i < was).count();
        match gone.contains(&was) {
            // Stay on the number, unless it was the last tab in the bar — then
            // the neighbour is the one before it, and with nothing left the
            // dashboard is all there is to land on.
            true => (was - before).min(remaining),
            false => was - before,
        }
    }

    /// Show the agent running as `pid`, reusing the pane already on it rather
    /// than opening a second window onto one terminal.
    ///
    /// A pane is a match on either pid it has: the one cctop hosts, and — for a
    /// rmux-backed pane, where that one is only the client — the agent's own.
    /// Asking about the hosted pid alone missed every rmux-backed pane, so an
    /// agent already on screen got a second window onto it.
    pub(super) fn open_view(&mut self, pid: u32, label: String) -> bool {
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
    /// In the arrangement they were last left in, and oldest first among the
    /// ones nobody has arranged — matching [`sync_shared_tabs`], because a
    /// cctop opened now and one that watched these start must number their tabs
    /// the same way.
    ///
    /// [`sync_shared_tabs`]: Self::sync_shared_tabs
    pub(super) fn restore_running_tabs(&mut self) {
        for agent in crate::rmux::running_in_tab_order() {
            self.tabs.push(tabs::Tab::shared(&agent));
        }
        if !self.tabs.is_empty() {
            self.go_to_tab(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{key, test_app};
    use ratatui::crossterm::event;
    use ratatui::crossterm::event::KeyCode;
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
                order: None,
                state: None,
                color: None,
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
                order: None,
                state: None,
                color: None,
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
                let status = app.status().unwrap_or_default().to_owned();
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
                order: None,
                state: None,
                color: None,
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

        // A terminal that pastes on the right button sends the clipboard along
        // with the click. It is dropped, so the field opens empty; a paste that
        // follows later is the person's own and lands.
        app.on_mouse(click(16), &layout);
        app.on_paste("whatever was on the clipboard");
        assert_eq!(app.rename_input, "");
        app.on_paste("deliberate");
        assert_eq!(app.rename_input, "deliberate");
        app.on_key(key(KeyCode::Esc));

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

    /// The same modal paints the tab: the arrows walk the colour stops and
    /// Enter applies the pick, while Esc leaves the colour it found alone.
    #[test]
    fn a_tab_is_painted_from_the_same_modal_that_names_it() {
        let named = |name: &str| {
            tabs::Tab::shared(&crate::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
                order: None,
                state: None,
                color: None,
            })
        };
        let layout = render::Layout {
            workspace_spans: vec![(0, 11, 0), (11, 15, 1)],
            ..Default::default()
        };
        let click = |column| event::MouseEvent {
            kind: event::MouseEventKind::Down(event::MouseButton::Right),
            column,
            row: 0,
            modifiers: event::KeyModifiers::NONE,
        };

        let mut app = test_app();
        app.tabs = vec![named("a")];

        // Three stops right of "none" is the third hue the picker walks.
        app.on_mouse(click(12), &layout);
        assert_eq!(app.mode, Mode::RenameTab);
        for _ in 0..3 {
            app.on_key(key(KeyCode::Right));
        }
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::List);
        assert_eq!(app.tabs[0].color, Some(theme::Hue::ALL[2]));
        let status = app.status().expect("nothing was said").to_owned();
        assert!(status.contains(theme::Hue::ALL[2].name()), "{status}");

        // The pick opens where the tab already stands — three stops in — so
        // walking left past them reaches "none".
        app.on_mouse(click(12), &layout);
        for _ in 0..3 {
            app.on_key(key(KeyCode::Left));
        }
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.tabs[0].color, None, "the cleared colour survived");

        // And the row wraps: one step left of "none" is the last hue — there
        // is no end worth stopping at.
        app.on_mouse(click(12), &layout);
        app.on_key(key(KeyCode::Left));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.tabs[0].color, Some(theme::Hue::Pink));

        // Esc really does leave it alone.
        app.on_mouse(click(12), &layout);
        app.on_key(key(KeyCode::Right));
        app.on_key(key(KeyCode::Esc));
        assert_eq!(
            app.tabs[0].color,
            Some(theme::Hue::Pink),
            "Esc repainted the tab"
        );
    }

    /// Closing is a kill now, but only of an agent that is cctop's to kill. On a
    /// pane opened with `a` — a window onto an agent cctop never started — there
    /// is nothing to stop, so the window closes and the status says the agent
    /// was left alone rather than claiming a kill that never happened.
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
        let status = app.status().expect("nothing was said").to_owned();
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
            order: None,
            state: None,
            color: None,
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
        let status = app.status().expect("nothing was said").to_owned();
        assert!(status.contains("Improve super cctop"), "{status}");
    }

    /// `Alt+b` walks the bar to the next tab asking for you, wrapping at the
    /// end — and says so plainly when nothing is, rather than jumping
    /// somewhere arbitrary.
    #[test]
    fn the_next_waiting_tab_is_the_next_one_asking() {
        let shared = |name: &str, signal: Option<crate::hook::Signal>| {
            tabs::Tab::shared(&crate::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
                order: None,
                state: signal.map(|signal| crate::rmux::State {
                    signal,
                    at: crate::rmux::now_secs(),
                }),
                color: None,
            })
        };
        let mut app = test_app();
        app.tabs = vec![
            shared("a", Some(crate::hook::Signal::Busy)),
            shared("b", Some(crate::hook::Signal::NeedsInput)),
            shared("c", None),
            shared("d", Some(crate::hook::Signal::NeedsInput)),
        ];

        // Bar positions, not vec indices: "a" is 1, "b" is 2, "d" is 4.
        app.tab = 0;
        assert_eq!(app.next_waiting(), Some(2), "the first ask was skipped");
        app.tab = 2;
        assert_eq!(
            app.next_waiting(),
            Some(4),
            "the next ask is not the same tab"
        );
        app.tab = 4;
        assert_eq!(app.next_waiting(), Some(2), "the walk did not wrap");
        app.tab = 3;
        assert_eq!(app.next_waiting(), Some(4), "the jump went backward first");

        // Nothing asking: the key has to say so rather than go nowhere quiet.
        app.tabs = vec![shared("a", Some(crate::hook::Signal::Busy))];
        app.next_waiting_tab();
        let status = app.status().expect("nothing was said").to_owned();
        assert!(status.contains("Nothing is waiting"), "{status}");
    }

    /// A live row for the agent running as `pid`, doing `state`.
    fn agent_row(id: &str, pid: u32, state: crate::session::ActivityState) -> Session {
        let mut row = crate::ui::tests::session(id, true, id);
        row.process.as_mut().unwrap().process_list = vec![crate::proc::ProcEntry {
            pid,
            is_root: true,
            ghost: false,
            cpu: 0.0,
            memory: 0,
            args: String::new(),
        }];
        row.activity_state = state;
        row
    }

    /// A tab no pane of this cctop's is attached to, for the agent `pid`.
    fn detached_tab(name: &str, pid: u32, signal: Option<crate::hook::Signal>) -> tabs::Tab {
        tabs::Tab::shared(&crate::rmux::Running {
            name: format!("cctop-{name}"),
            pid: Some(pid),
            cwd: None,
            attached: false,
            activity: None,
            label: Some(name.to_string()),
            profile: None,
            order: None,
            state: signal.map(|signal| crate::rmux::State {
                signal,
                at: crate::rmux::now_secs(),
            }),
            color: None,
        })
    }

    /// The whole of the unseen mark as the app sees it: a turn watched ending
    /// off screen marks the row and its tab, `Alt+b` goes to a question
    /// before it, and looking at the row takes the mark down.
    #[test]
    fn a_turn_that_ended_unseen_is_marked_and_visited_after_a_question() {
        use crate::session::ActivityState::{Asking, WaitingForInput, Working};
        let mut app = test_app();
        app.sessions = vec![agent_row("a", 101, Working), agent_row("b", 202, Working)];
        app.tabs = vec![detached_tab("a", 101, None), detached_tab("b", 202, None)];
        app.tab = 0;
        app.observe_seen();

        // "a" finishes its turn; "b" stops on a question. Nobody is looking:
        // the dashboard has no row selected until the table is filtered.
        app.sessions[0].activity_state = WaitingForInput;
        app.sessions[1].activity_state = Asking;
        assert!(app.observe_seen(), "a new mark owed no frame");
        assert!(app.seen.is_done(&app.sessions[0].key()));
        assert!(!app.seen.is_done(&app.sessions[1].key()), "a question is not done");
        assert_eq!(app.tab_attention(1), Some(tabs::Attention::Done));
        assert_eq!(app.tab_attention(2), Some(tabs::Attention::NeedsInput));

        // The question first, however the bar is ordered; the news after.
        assert_eq!(app.next_waiting(), Some(2));
        app.sessions[1].activity_state = Working;
        assert_eq!(app.next_waiting(), Some(1), "a done tab was not a jump");

        // Selecting its row on the dashboard is looking at it.
        app.refilter();
        let row = app
            .visible
            .iter()
            .position(|r| matches!(r, crate::ui::Row::Session(0)))
            .expect("the row is not in the table");
        app.selected = row;
        assert!(app.observe_seen(), "clearing the mark owed no frame");
        assert!(!app.seen.is_done(&app.sessions[0].key()));
        assert_eq!(app.tab_attention(1), Some(tabs::Attention::Idle));
        assert_eq!(app.next_waiting(), None, "a read turn is still a jump");
    }

    /// `Alt+z` fills the tab with the focused pane and back, only where there
    /// is something to fill it over, and a new split is never born hidden.
    #[test]
    fn zoom_toggles_only_over_a_split_and_a_new_split_unzooms() {
        let mut app = test_app();
        app.tabs = vec![tabs::Tab::new(tabs::Pane::for_test("one"))];
        app.tab = 1;
        let alt = |code| event::KeyEvent::new(code, event::KeyModifiers::ALT);

        app.on_key(alt(KeyCode::Char('z')));
        assert!(!app.tabs[0].zoomed(), "a lone pane was zoomed");
        let status = app.status().expect("nothing was said").to_owned();
        assert!(status.contains("nothing to zoom"), "{status}");

        app.tabs[0].split(tabs::Pane::for_test("two"), false);
        app.on_key(alt(KeyCode::Char('z')));
        assert!(app.tabs[0].zoomed());
        // Focus moves under the zoom rather than ending it.
        app.on_key(alt(KeyCode::Char('o')));
        assert!(app.tabs[0].zoomed());
        assert_eq!(app.tabs[0].focus, 0);
        app.on_key(alt(KeyCode::Char('z')));
        assert!(!app.tabs[0].zoomed());

        app.on_key(alt(KeyCode::Char('z')));
        app.tabs[0].split(tabs::Pane::for_test("three"), true);
        assert!(!app.tabs[0].zoomed(), "the new pane was started out of sight");

        // The dashboard has no pane to zoom, and does not claim the key.
        app.tab = 0;
        app.on_key(alt(KeyCode::Char('z')));
        assert!(!app.tabs[0].zoomed());
    }

    /// `Alt+r` opens the same rename the right-click does, on the tab being
    /// watched — the dashboard excepted, which is nobody's to name.
    #[test]
    fn the_keyboard_can_rename_the_tab_it_is_on() {
        let named = |name: &str| {
            tabs::Tab::shared(&crate::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
                order: None,
                state: None,
                color: None,
            })
        };
        let alt = |code| event::KeyEvent::new(code, event::KeyModifiers::ALT);

        let mut app = test_app();
        app.tabs = vec![named("a"), named("b")];

        app.on_key(alt(KeyCode::Char('r')));
        assert_eq!(app.mode, Mode::List, "the dashboard offered a rename");

        app.tab = 2;
        app.on_key(alt(KeyCode::Char('r')));
        assert_eq!(app.mode, Mode::RenameTab);
        assert_eq!(app.rename_tab, 2);
        assert_eq!(app.rename_was, "b");
    }

    /// `Alt+t` lists the bar, typing narrows it, and Enter goes to the pick.
    /// The cursor opens on the tab being watched, so Down walks the bar from
    /// where you are rather than from the dashboard.
    #[test]
    fn the_switcher_narrows_the_bar_and_goes_to_the_pick() {
        let named = |name: &str| {
            tabs::Tab::shared(&crate::rmux::Running {
                name: format!("cctop-{name}"),
                pid: None,
                cwd: None,
                attached: false,
                activity: None,
                label: Some(name.to_string()),
                profile: None,
                order: None,
                state: None,
                color: None,
            })
        };
        let alt = |code| event::KeyEvent::new(code, event::KeyModifiers::ALT);
        let typed = |app: &mut App, text: &str| {
            for c in text.chars() {
                app.on_key(key(KeyCode::Char(c)));
            }
        };

        let mut app = test_app();
        app.tabs = vec![named("claude"), named("codex"), named("review")];

        app.on_key(alt(KeyCode::Char('t')));
        assert_eq!(app.mode, Mode::SwitchTab);
        assert_eq!(app.switch_matches(), vec![0, 1, 2, 3]);

        typed(&mut app, "cod");
        assert_eq!(app.switch_matches(), vec![2]);
        // Case-insensitive, and the dashboard is a row like any other.
        app.switch_filter.clear();
        typed(&mut app, "DASH");
        assert_eq!(app.switch_matches(), vec![0]);

        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::List);

        // Enter goes to the pick — here the dashboard, the only landing that
        // needs no agent behind it.
        app.tab = 1;
        app.on_key(alt(KeyCode::Char('t')));
        assert_eq!(
            app.switch_matches()[app.switch_cursor],
            1,
            "the cursor did not open on the current tab"
        );
        typed(&mut app, "dash");
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::List);
        assert_eq!(app.tab, 0, "Enter did not take the pick");
    }
}
