//! The cursor: what it points at, where it moves, and what the panels follow.
//!
//! The table's rows are not its sessions — a session can expand into subagent
//! rows — so "the selection" is an index into [`App::visible`] that has to
//! survive a resort, a refilter and a session disappearing. Everything that
//! reads or repairs that index is here, together with the bottom panels, which
//! are downstream of it: they show whatever the cursor is on.

use super::*;

/// Rows moved by PageUp/PageDown and the fallback for half-page scrolls.
pub(super) const PAGE: isize = 10;

/// Whether a panels key is the one `Session::key` would make for this session.
///
/// `Session::key` is a `format!`, and a key built to be compared and dropped is
/// still an allocation on a path that runs on every pass of the loop. The panels
/// already hold the key as a `String`, so the format is stripped off that copy
/// and what is left compared against the id — the same comparison, with nothing
/// written to the heap to make it.
fn keyed_as(key: &str, provider: Provider, session_id: &str) -> bool {
    key.strip_prefix(provider.as_str())
        .and_then(|rest| rest.strip_prefix(':'))
        == Some(session_id)
}

impl App {
    /// The highlighted session, if there is one.
    ///
    /// `visible` holds indices into `sessions`, and a refresh replaces
    /// `sessions` before `refilter` rebuilds `visible` — so between those two
    /// steps the indices can outrun the list. Resolving through `get` keeps this
    /// accessor total instead of panicking on that window.
    pub fn selected_session(&self) -> Option<&Session> {
        self.visible
            .get(self.selected)
            .and_then(|row| self.sessions.get(row.session()?))
    }

    /// The highlighted row, whatever kind it is.
    pub fn selected_row(&self) -> Option<Row> {
        self.visible.get(self.selected).copied()
    }

    /// The highlighted subagent, when the cursor is on a child row.
    pub fn selected_subagent(&self) -> Option<&cctop_core::session::Subagent> {
        match self.selected_row()? {
            Row::Session(_) | Row::Group(_) => None,
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
        let Some(session) = row.session().and_then(|i| self.sessions.get(i)) else {
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
    pub(super) fn toggle_expanded_all(&mut self) {
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

    /// Identity of a row across refreshes.
    ///
    /// A subagent's own id is unique only within its parent, and a session key
    /// alone cannot tell a parent from its children, so the cursor is anchored
    /// on the pair.
    pub(super) fn row_key(&self, row: Row) -> String {
        if let Row::Group(g) = row {
            // Prefixed so a heading can never share a key with a session.
            return match self.groups.get(g) {
                Some(g) => format!("group:{}", g.key),
                None => String::new(),
            };
        }
        let Some(session) = row.session().and_then(|i| self.sessions.get(i)) else {
            return String::new();
        };
        match row {
            Row::Session(_) | Row::Group(_) => session.key(),
            Row::Subagent { index, .. } => match session.subagents.get(index) {
                Some(sub) => format!("{}/{}", session.key(), sub.agent_id),
                None => session.key(),
            },
        }
    }

    /// What the bottom panels should describe for a row.
    ///
    /// A subagent gets a stand-in `Session` pointed at its own transcript. That
    /// file is the same JSONL a session writes, so the whole extraction path —
    /// worker, cache, every panel — reads it without knowing the difference, and
    /// the panels describe the subagent rather than the parent it ran under.
    ///
    /// Building this is deliberately the *last* step of [`Self::sync_panel_data`]
    /// and not the first: what it is built from is borrowed, and a row the panels
    /// already hold must not be copied to find that out.
    pub(super) fn panel_subject(&self, row: Row) -> Option<Session> {
        let session = self.sessions.get(row.session()?)?;
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
        // Found among the session's transcripts rather than rebuilt from the
        // id: a workflow's agents sit a run directory further down.
        stand_in.data_file = session.data_file.as_ref().and_then(|f| {
            cctop_core::session::transcript_files(f)
                .into_iter()
                .skip(1)
                .find(|t| t.file_stem().is_some_and(|s| *s == *sub.agent_id))
        });
        // Its own mtime, so the panels refresh while the subagent is working and
        // not merely when its parent writes something.
        stand_in.last_active = stand_in
            .data_file
            .as_ref()
            .map(|f| cctop_core::util::ms_to_rfc3339(cctop_core::config::file_mtime_ms(f) as i64))
            .unwrap_or_default();
        Some(stand_in)
    }

    /// Whether the panels already hold `row` as it stands.
    ///
    /// The question [`Self::sync_panel_data`] has to answer before it sends
    /// anything, asked against the borrowed session: a `Session` carries two cost
    /// maps, every subagent and the process tree behind the row, and this runs on
    /// every pass of the loop — sixty times a second inside a tab — where the
    /// answer is almost always that nothing has changed.
    ///
    /// `false` only means "not settled here": every row this cannot answer for
    /// falls through to the slow path, which decides it exactly as it always did.
    /// A child row is one of those, because its newest activity is the mtime of a
    /// transcript that has to be found on disk first — the same lookup that
    /// building the stand-in does.
    fn panels_already_hold(&self, row: Row) -> bool {
        let Some(session) = row.session().and_then(|i| self.sessions.get(i)) else {
            return false;
        };
        match row {
            Row::Subagent { .. } => false,
            _ => {
                keyed_as(&self.panel_key, session.provider, &session.session_id)
                    && session.last_active == self.panel_stamp
            }
        }
    }

    /// Ask the worker for the selected row's full data if it isn't loaded.
    pub(super) fn sync_panel_data(&mut self) {
        let Some(row) = self.selected_row() else {
            self.panel_data = None;
            self.panel_key.clear();
            return;
        };
        // Before the subject, not after it: what decides whether there is
        // anything to send is the key and the stamp, and both are fields of the
        // row. Building the session first and comparing it afterwards meant every
        // pass of the loop deep-copied the session under the cursor — and threw
        // it away — while the user was looking at a still screen.
        if self.panels_already_hold(row) {
            return;
        }
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

    pub(super) fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last as isize) as usize;
        self.ensure_available_tab();
        self.needs_redraw = true;
    }

    pub(super) fn tab_available(&self, tab: usize) -> bool {
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

    pub(super) fn ensure_available_tab(&mut self) {
        if !self.tab_available(self.bottom_tab) {
            self.bottom_tab = 0;
        }
    }

    pub(super) fn set_sort(&mut self, col: ColumnId) {
        if self.sort_col == col {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort_col = col;
            self.sort_asc = true;
        }
        self.refilter();
        self.save_prefs();
    }

    /// The Subagents panel's sort, with the table's toggle semantics: the
    /// column already sorted by flips direction, a new one starts ascending.
    /// The settings page still offers the explicit pairs — it is a list of
    /// labelled choices — but a click on the panel's own header is the
    /// interaction the table has, and it should not mean something else.
    pub(super) fn set_subagent_sort(&mut self, col: panels::SubagentSort) {
        let (cur, asc) = self.subagent_sort;
        self.subagent_sort = if cur == col { (col, !asc) } else { (col, true) };
        self.save_prefs();
        self.needs_redraw = true;
    }

    /// Expand or collapse the invocation under a clicked log row.
    pub(super) fn toggle_tool_expansion(&mut self, row_offset: usize) {
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
    pub(super) fn cycle_tool_filter(&mut self, delta: isize) {
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
    pub(super) fn cycle_tab(&mut self, delta: isize) {
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

    pub(super) fn scroll_active_panel(&mut self, delta: i32) {
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
            // Preview is a screen clipped to the panel, not a list of lines.
            super::preview::TAB => {}
            _ => bump(&mut self.context_scroll),
        }
        self.needs_redraw = true;
    }

    /// Copy something useful about the selection to the clipboard.
    pub(super) fn copy_selection(&mut self) {
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
        self.set_status(format!("Copied: {}", cctop_core::util::truncate(&text, 60)));
    }

    /// Half the visible table height, used by Ctrl+U/Ctrl+D. Falls back to a
    /// page size before the first draw has recorded a viewport.
    pub(super) fn half_page(&self) -> usize {
        ((self.list_height as usize / 2).max(1)).min(PAGE as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{session, test_app};
    fn with_subagents(id: &str, names: &[&str]) -> Session {
        let mut s = session(id, true, id);
        s.subagents = names
            .iter()
            .map(|n| cctop_core::session::Subagent {
                agent_id: format!("agent-{n}"),
                agent_type: "general-purpose".into(),
                description: (*n).into(),
                model: "claude-opus-5".into(),
                started_at: None,
                last_active: None,
                duration_ms: 0,
                status: cctop_core::session::SubagentStatus::Running,
                cost: 0.0,
                tool_count: 0,
                turns: 0,
                tokens: 0,
                last_text: None,
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

    /// The reader opened on a child row reads that agent's own turns, through
    /// the session that lists it.
    #[test]
    fn reading_a_child_row_opens_that_agents_turns() {
        let mut app = test_app();
        app.sessions = vec![with_subagents("a", &["one", "two"])];
        app.expanded.insert(app.sessions[0].key());
        app.refilter();
        app.selected = 2;

        app.open_conversation();

        let view = app.chat.as_ref().expect("the reader opened");
        assert_eq!(view.session.key(), app.sessions[0].key());
        let agent = view.agent.as_ref().expect("on the agent");
        assert_eq!(agent.id, "agent-two");
        assert_eq!(agent.title(), "general-purpose — two");
        assert!(view.parent.is_none(), "Esc leaves the reader");
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
                .all(|s| s.status == cctop_core::session::SubagentStatus::Running)
        );

        // The hook names the bare id; the transcript is stored as `agent-<id>`.
        app.finished_agents.insert("one".into());
        app.apply_finished_agents();

        let status = |i: usize| app.sessions[0].subagents[i].status;
        assert_eq!(status(0), cctop_core::session::SubagentStatus::Done);
        assert_eq!(
            status(1),
            cctop_core::session::SubagentStatus::Running,
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
        assert!(app.status().is_some(), "the refusal has to be visible");
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

    /// The panel's sort answers its header clicks the way the table's does:
    /// the column already sorted by flips, a new one starts ascending.
    #[test]
    fn subagent_sort_toggles_on_repeat_and_resets_on_change() {
        let mut app = test_app();
        app.subagent_sort = (panels::SubagentSort::Cost, true);
        app.set_subagent_sort(panels::SubagentSort::Cost);
        assert!(!app.subagent_sort.1, "same column must flip direction");
        app.set_subagent_sort(panels::SubagentSort::Type);
        assert!(app.subagent_sort.1, "new column starts ascending");
        assert_eq!(app.subagent_sort.0, panels::SubagentSort::Type);
    }

    /// What decides a refresh is the key and the stamp, and both are fields of
    /// the row — so a session that has not moved is asked for once and then left
    /// alone. That question used to be answered after building the session to
    /// compare it against, which meant a still screen deep-copied the session
    /// under the cursor on every pass of the loop, sixty times a second inside a
    /// tab, and threw it away.
    #[test]
    fn an_unmoved_session_is_asked_for_once() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.sessions = vec![session("a", true, "/x")];
        app.refilter();
        let asked = || {
            rx.try_iter()
                .filter(|r| matches!(r, Request::Data(_)))
                .count()
        };

        app.sync_panel_data();
        assert_eq!(asked(), 1, "a row nobody has looked at is asked for");
        app.sync_panel_data();
        app.sync_panel_data();
        assert_eq!(asked(), 0, "nothing moved, so nothing is asked for again");

        // A live session keeps growing, and that is the one thing that has to
        // bring the panels back: an append moves the stamp.
        app.sessions[0].last_active = chrono::Utc::now().to_rfc3339();
        app.sync_panel_data();
        assert_eq!(asked(), 1, "newest activity moved, so the panels are stale");

        app.sync_panel_data();
        assert_eq!(asked(), 0);

        // Moving the cursor is the other way a refresh is earned.
        app.sessions.push(session("b", true, "/y"));
        app.refilter();
        app.selected = app
            .visible
            .iter()
            .position(|&row| row == Row::Session(1))
            .expect("the session just added has a row");
        app.sync_panel_data();
        assert_eq!(
            asked(),
            1,
            "a different session is a different set of panels"
        );
        app.sync_panel_data();
        assert_eq!(asked(), 0);
    }
}
