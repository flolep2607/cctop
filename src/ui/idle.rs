//! Live sessions nobody is using, and getting their memory back.
//!
//! An agent left open holds its whole process tree for as long as it stays
//! open: on a shared server that was twenty `claude` processes, one to seven
//! days old, around 450 MB each plus a plugin child — ten gigabytes for
//! sessions nobody was typing into. Nothing about a session says it has been
//! forgotten, so the table cannot show it without being asked.
//!
//! The view is a filter layer beside live-only and the age filter rather than a
//! list of its own, so everything the table already does — marking, the row
//! menu, the panels, Esc — works on it unchanged. What it adds is the one
//! question a filter cannot answer by hiding rows: which of these is it safe to
//! stop. That is [`App::keep_running`], and the batch it feeds skips rather
//! than refuses, because one session still mid-turn is no reason to leave the
//! other eleven holding memory.

use super::*;
use crate::session::ActivityState;
use crate::util;

/// Process-tree CPU, in percent of one core, above which a session that has
/// written nothing is still taken to be doing something.
///
/// A transcript goes quiet for the length of a tool call, and a turn that is
/// over can still have left a background shell running a server or a build.
/// Either shows up here where it does not show up on disk. An idle agent and
/// its MCP children sit well under one percent between them.
const BUSY_CPU: f32 = 5.0;

/// Reclaimable memory at which the overview mentions it. Below a gigabyte the
/// line would be there most of the time and mean little when it was.
pub(super) const HINT_BYTES: u64 = 1 << 30;

/// Why a session in the idle view is left running when the rest are stopped.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Keep {
    /// Its own hooks say a turn is under way.
    Working,
    /// Blocked on a question: a permission prompt or an elicitation, which
    /// someone meant to answer.
    Asking,
    /// Quiet on disk but not in the process table.
    Busy(f32),
    /// On another machine, which cctop reads but does not signal.
    Remote(String),
    /// Nothing here to send a signal to.
    NoProcess,
}

impl Keep {
    pub(super) fn reason(&self) -> String {
        match self {
            Keep::Working => "working".into(),
            Keep::Asking => "asking a question".into(),
            Keep::Busy(cpu) => format!("busy ({cpu:.0}% CPU)"),
            Keep::Remote(host) => format!("on {host}"),
            Keep::NoProcess => "no local process".into(),
        }
    }
}

/// What a reclaim would do, worked out once so the confirmation and the action
/// cannot disagree about it.
pub(super) struct Plan<'a> {
    /// Sessions to stop, biggest first.
    pub stop: Vec<&'a Session>,
    /// Sessions left running, and why.
    pub keep: Vec<(&'a Session, Keep)>,
    /// Memory held by the process trees of `stop`.
    pub bytes: u64,
}

/// The process tree's memory: the root and every child, as the Processes panel
/// lists them. Stopping the root is what lets the rest go.
pub(super) fn tree_memory(s: &Session) -> u64 {
    s.process.as_ref().map_or(0, |p| p.memory)
}

/// When the session last did anything, falling back to when it began for one
/// that has never written — the same timestamp the age filter reads.
pub(super) fn quiet_since(s: &Session) -> &str {
    if s.last_active.is_empty() {
        &s.started_at
    } else {
        &s.last_active
    }
}

/// How long the session has been quiet.
pub(super) fn quiet_ms(s: &Session, now_ms: i64) -> Option<i64> {
    crate::util::parse_ts(quiet_since(s)).map(|d| now_ms - d.timestamp_millis())
}

/// `6h`, `90m`, `2d` — the threshold as a badge spells it.
pub(super) fn threshold_label(ms: i64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        m if m % (24 * 60) == 0 => format!("{}d", m / (24 * 60)),
        m if m % 60 == 0 => format!("{}h", m / 60),
        m => format!("{m}m"),
    }
}

impl App {
    /// Whether a session belongs in the idle view: a live process, and nothing
    /// written for `idle_after`.
    pub(super) fn is_idle(&self, s: &Session, now_ms: i64) -> bool {
        s.process.is_some()
            && quiet_ms(s, now_ms).is_some_and(|q| q >= self.settings.idle_after_ms())
    }

    /// Why an idle session should be left running, or `None` to stop it.
    ///
    /// The hooks are asked first because they are the agent saying so; the
    /// transcript second, which can see a held question but not a turn in
    /// progress — on disk a finished turn and a long tool call both look like
    /// an agent that stopped writing, and the CPU check is what tells them
    /// apart.
    ///
    /// ponytail: a prompt typed into the agent and not sent is invisible from
    /// here, and is lost with the process. The confirmation says so.
    pub(super) fn keep_running(&self, s: &Session) -> Option<Keep> {
        if let Some(remote) = &s.remote {
            return Some(Keep::Remote(remote.host.clone()));
        }
        if s.root_pid().is_none() {
            return Some(Keep::NoProcess);
        }
        match self.hooked_signal(&s.session_id) {
            Some(crate::hook::Signal::NeedsInput) => return Some(Keep::Asking),
            Some(signal) if signal.is_working() => return Some(Keep::Working),
            _ => {}
        }
        if s.activity_state == ActivityState::Asking {
            return Some(Keep::Asking);
        }
        let cpu = s.process.as_ref().map_or(0.0, |p| p.cpu);
        (cpu >= BUSY_CPU).then_some(Keep::Busy(cpu))
    }

    /// Every idle session on the machine that a reclaim would stop, and the
    /// memory they hold — whether or not the idle view is open, which is what
    /// lets the overview mention it.
    pub(super) fn reclaimable(&self) -> (usize, u64) {
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.sessions
            .iter()
            .filter(|s| self.is_idle(s, now_ms) && self.keep_running(s).is_none())
            .fold((0, 0), |(n, bytes), s| (n + 1, bytes + tree_memory(s)))
    }

    /// Open or close the idle view.
    ///
    /// Opening sorts by memory, since the point is the biggest first, and
    /// closing puts back whatever sort was there before: the view borrowed the
    /// table, and should hand it back as it found it.
    pub(super) fn toggle_idle_view(&mut self) {
        if self.idle_only {
            self.leave_idle_view();
            self.refilter();
            self.set_status("Idle view closed");
            return;
        }
        self.idle_only = true;
        self.idle_sort = Some((self.sort_col, self.sort_asc));
        self.sort_col = ColumnId::Memory;
        self.sort_asc = false;
        self.refilter();
        let (n, bytes) = self.reclaimable();
        let after = threshold_label(self.settings.idle_after_ms());
        self.set_status(match n {
            0 => format!("No live session is idle for {after} and safe to stop"),
            _ => format!(
                "{n} idle for {after}, {} to reclaim — K stops them",
                util::compact_bytes(bytes)
            ),
        });
    }

    /// Close the idle view without refiltering, for callers that are about to.
    pub(super) fn leave_idle_view(&mut self) {
        self.idle_only = false;
        if let Some((col, asc)) = self.idle_sort.take() {
            self.sort_col = col;
            self.sort_asc = asc;
        }
    }

    /// What `K` in the idle view would do: the marked rows if any are marked,
    /// otherwise every row the view shows.
    ///
    /// Every row, because the view is already the selection — marking twelve
    /// rows one by one to say "all of these" is work the filter has done. The
    /// confirmation is what stands between that and a stray key.
    pub(super) fn reclaim_plan(&self) -> Plan<'_> {
        let marked = self.marked_sessions();
        let targets: Vec<&Session> = if marked.is_empty() {
            self.visible
                .iter()
                .filter_map(|row| match row {
                    Row::Session(i) => self.sessions.get(*i),
                    Row::Subagent { .. } | Row::Group(_) => None,
                })
                .collect()
        } else {
            marked
        };
        let mut plan = Plan {
            stop: Vec::new(),
            keep: Vec::new(),
            bytes: 0,
        };
        for s in targets {
            match self.keep_running(s) {
                Some(why) => plan.keep.push((s, why)),
                None => {
                    plan.bytes += tree_memory(s);
                    plan.stop.push(s);
                }
            }
        }
        plan.stop.sort_by_key(|s| std::cmp::Reverse(tree_memory(s)));
        plan
    }

    /// `K` in the idle view: confirm, or say why there is nothing to confirm.
    pub(super) fn confirm_reclaim(&mut self) {
        let plan = self.reclaim_plan();
        if plan.stop.is_empty() {
            let said = match plan.keep.len() {
                0 => "No idle sessions to stop".to_string(),
                n => format!("Nothing to stop — all {n} idle sessions are in use"),
            };
            self.set_status(said);
            return;
        }
        self.batch = BatchKind::Reclaim;
        self.mode = Mode::BatchConfirm;
        self.needs_redraw = true;
    }

    /// Send SIGTERM to every session the plan stops, and nothing else.
    ///
    /// Terminate only, never kill: the agent gets to write its transcript out
    /// and take its children with it, which is what makes it resumable with
    /// `R` afterwards. Recomputed rather than carried over from the modal, so a
    /// session that started working while the question was open is skipped.
    pub(super) fn reclaim_execute(&mut self) {
        let plan = self.reclaim_plan();
        let orders: Vec<(String, u32)> = plan
            .stop
            .iter()
            .filter_map(|s| s.root_pid().map(|pid| (s.key(), pid)))
            .collect();
        let (bytes, kept) = (plan.bytes, plan.keep.len());
        for (session_key, pid) in orders.iter().cloned() {
            self.tx.send(Request::Terminate { session_key, pid }).ok();
        }
        for (key, _) in &orders {
            self.marked.remove(key);
        }
        let mut said = format!(
            "Stopping {} idle session(s), {} to reclaim",
            orders.len(),
            util::compact_bytes(bytes)
        );
        if kept > 0 {
            said.push_str(&format!(" — {kept} left running"));
        }
        self.set_status(said);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{session, test_app};

    const MB: u64 = 1024 * 1024;

    /// A live session quiet for `hours`, its tree holding `mb` megabytes.
    fn live(id: &str, hours: i64, mb: u64) -> Session {
        let mut s = session(id, true, "/x");
        s.last_active = (chrono::Utc::now() - chrono::Duration::hours(hours)).to_rfc3339();
        s.started_at = s.last_active.clone();
        let p = s.process.as_mut().unwrap();
        p.memory = mb * MB;
        p.process_list = vec![crate::proc::ProcEntry {
            pid: 4000 + mb as u32,
            cpu: 0.0,
            memory: mb * MB,
            args: "claude".into(),
            is_root: true,
            ghost: false,
        }];
        s
    }

    fn shown(app: &App) -> Vec<String> {
        app.visible
            .iter()
            .filter_map(|r| r.session())
            .map(|i| app.sessions[i].session_id.clone())
            .collect()
    }

    #[test]
    fn the_view_lists_quiet_live_sessions_biggest_first() {
        let mut app = test_app();
        app.sessions = vec![
            live("small", 30, 300),
            live("fresh", 1, 900),
            live("big", 8, 600),
            session("stopped", false, "/x"),
        ];
        app.refilter();
        app.toggle_idle_view();
        assert_eq!(shown(&app), ["big", "small"]);
        assert_eq!(app.reclaimable(), (2, 900 * MB));

        // Closing hands the table back sorted the way it was.
        app.toggle_idle_view();
        assert!(!app.idle_only);
        assert_eq!(app.sort_col, ColumnId::Last);
        assert_eq!(shown(&app).len(), 4);
    }

    #[test]
    fn the_threshold_comes_from_settings() {
        let mut app = test_app();
        app.settings = crate::settings::Settings::parse("[settings]\nidle_after = 0.5\n");
        app.sessions = vec![live("a", 1, 100)];
        app.idle_only = true;
        app.refilter();
        assert_eq!(shown(&app), ["a"]);
        assert_eq!(threshold_label(app.settings.idle_after_ms()), "30m");
    }

    #[test]
    fn esc_peels_the_idle_view_like_any_other_filter() {
        let mut app = test_app();
        app.sessions = vec![live("a", 10, 100), live("b", 0, 100)];
        app.toggle_idle_view();
        assert!(app.has_filter());
        app.clear_one_filter();
        assert!(!app.idle_only);
        assert_eq!(app.sort_col, ColumnId::Last);
        assert_eq!(shown(&app).len(), 2);
    }

    #[test]
    fn a_reclaim_skips_what_is_in_use_and_says_why() {
        let mut app = test_app();
        let mut asking = live("asking", 10, 200);
        asking.activity_state = ActivityState::Asking;
        let mut busy = live("busy", 10, 300);
        busy.process.as_mut().unwrap().cpu = 40.0;
        let mut far = live("far", 10, 400);
        far.remote = Some(crate::session::Remote {
            host: "box".into(),
            branch: None,
            ..Default::default()
        });
        app.sessions = vec![
            live("done", 10, 500),
            asking,
            busy,
            far,
            live("also", 20, 100),
        ];
        app.toggle_idle_view();

        let plan = app.reclaim_plan();
        let stop: Vec<&str> = plan.stop.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(stop, ["done", "also"]);
        assert_eq!(plan.bytes, 600 * MB);
        let keep: Vec<(&str, String)> = plan
            .keep
            .iter()
            .map(|(s, why)| (s.session_id.as_str(), why.reason()))
            .collect();
        assert!(keep.contains(&("asking", "asking a question".into())));
        assert!(keep.contains(&("busy", "busy (40% CPU)".into())));
        assert!(keep.contains(&("far", "on box".into())));
        // The overview's figure is the same sum, over the same rule.
        assert_eq!(app.reclaimable(), (2, 600 * MB));
    }

    #[test]
    fn a_session_whose_hooks_say_it_is_working_is_left_alone() {
        let mut app = test_app();
        app.sessions = vec![live("mid", 10, 500)];
        app.hooked.insert(
            "mid".into(),
            crate::hook::Reported {
                provisional: false,
                signal: crate::hook::Signal::Acting,
                cwd: "/x".into(),
                permission: None,
                at: std::time::Instant::now(),
            },
        );
        assert_eq!(app.keep_running(&app.sessions[0]), Some(Keep::Working));
    }

    #[test]
    fn marks_narrow_a_reclaim_and_none_means_the_whole_view() {
        let mut app = test_app();
        app.sessions = vec![live("a", 10, 100), live("b", 10, 200), live("c", 10, 300)];
        app.toggle_idle_view();
        assert_eq!(app.reclaim_plan().stop.len(), 3);

        app.marked.insert(app.sessions[1].key());
        let plan = app.reclaim_plan();
        assert_eq!(plan.stop.len(), 1);
        assert_eq!(plan.stop[0].session_id, "b");

        app.confirm_reclaim();
        assert_eq!(app.mode, Mode::BatchConfirm);
        assert_eq!(app.batch, BatchKind::Reclaim);
        app.reclaim_execute();
        assert!(app.marked.is_empty(), "the stopped row is unmarked");
    }

    /// The overview says so past a gigabyte and not before: below that the
    /// line would be on screen most of the time.
    #[test]
    fn the_overview_mentions_reclaimable_memory_past_a_gigabyte() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let screen = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(160, 40)).unwrap();
            terminal
                .draw(|f| {
                    render::draw(f, app);
                })
                .unwrap();
            let buffer = terminal.backend().buffer().clone();
            buffer
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
        };
        let mut app = test_app();
        app.loaded = true;
        app.sessions = vec![live("a", 10, 600)];
        app.refilter();
        assert!(!screen(&mut app).contains("idle (I)"));
        app.sessions.push(live("b", 10, 600));
        app.refilter();
        assert!(screen(&mut app).contains("1.2G idle (I)"));
    }

    /// `K` means "stop the view" inside it and "kill the marks" outside it;
    /// the two must not leak into each other.
    #[test]
    fn capital_i_opens_the_view_and_k_in_it_asks_about_all_of_it() {
        use crate::ui::tests::key;
        use ratatui::crossterm::event::KeyCode;
        let mut app = test_app();
        app.sessions = vec![live("a", 10, 100), live("b", 10, 200)];
        app.refilter();
        app.on_key(key(KeyCode::Char('K')));
        assert_eq!(app.mode, Mode::List, "outside it, K wants marks");

        app.on_key(key(KeyCode::Char('I')));
        assert!(app.idle_only);
        app.on_key(key(KeyCode::Char('K')));
        assert_eq!(
            (app.mode, app.batch),
            (Mode::BatchConfirm, BatchKind::Reclaim)
        );
        app.on_key(key(KeyCode::Char('n')));
        assert_eq!(app.mode, Mode::List);
        app.on_key(key(KeyCode::Char('I')));
        assert!(!app.idle_only);
    }

    #[test]
    fn nothing_to_stop_opens_no_confirmation() {
        let mut app = test_app();
        let mut asking = live("asking", 10, 200);
        asking.activity_state = ActivityState::Asking;
        app.sessions = vec![asking];
        app.toggle_idle_view();
        app.mode = Mode::List;
        app.confirm_reclaim();
        assert_eq!(app.mode, Mode::List);
    }
}
