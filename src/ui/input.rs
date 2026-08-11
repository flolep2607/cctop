//! Key and mouse handling: translate input events into state changes.

use super::columns::{COLUMNS, ColumnId};
use super::{
    AGE_OPTIONS, App, BatchKind, LaunchInto, Mode, PAGE, Request, render, session_root_pid,
};
use ratatui::crossterm::event::{
    self, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

/// The pasted text as a single line, within `room` more bytes — the budget being
/// in bytes because the caps it enforces are the ones `on_key_send` and
/// `on_key_cost` already apply to a `String`'s length.
///
/// Every input on the dashboard is one line drawn in one strip, and none of them
/// has a notion of a cursor on a second row — a newline dropped straight in would
/// be a character the box can neither show nor let you delete past. Each run of
/// line breaks and tabs becomes the one space it stands for, so pasting a wrapped
/// sentence into the search box searches for the sentence rather than for a
/// string no session's text contains. Every other control character is dropped:
/// none of them is anything a query or a message meant to contain, and an escape
/// among them would repaint the strip it landed in.
fn flatten(text: &str, room: usize) -> String {
    let mut out = String::new();
    let mut last_was_break = false;
    for c in text.chars() {
        if out.len() + c.len_utf8() > room {
            break;
        }
        match c {
            // A CRLF is one break, not two spaces, and neither is a line that
            // was indented after it.
            '\n' | '\r' | '\t' => {
                if !last_was_break {
                    out.push(' ');
                }
                last_was_break = true;
            }
            c if c.is_control() => {}
            c => {
                out.push(c);
                last_was_break = false;
            }
        }
    }
    out
}

impl App {
    pub(super) fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        self.needs_redraw = true;

        // Moving between tabs and panes has to work from inside a pane, where
        // every other key belongs to the agent. Alt is the modifier left over:
        // Ctrl- is the agent's (Ctrl-C interrupts it), and the function keys are
        // too few to also carry the splits.
        if key.modifiers.contains(KeyModifiers::ALT) && self.on_key_workspace(key) {
            return;
        }

        // Inside a pane the keyboard belongs to the agent. F10 and F12 remain
        // cctop's because the dashboard promises them as quit and back, and
        // function keys otherwise go straight through to the focused agent.
        if self.tab > 0 && self.mode == Mode::List {
            if key.code == KeyCode::F(10) {
                self.request_quit();
                return;
            }
            if key.code == KeyCode::F(12) {
                self.show_tab(0);
                return;
            }
            if let Some(pane) = self.focused_pane() {
                // The agent this key is going to, taken before the borrow ends:
                // answering its question is the one thing no hook reports.
                let agent = pane.agent();
                let alive = pane.view.send_key(key);
                self.mark_answered(agent);
                if !alive {
                    self.close_pane();
                    self.set_status("The agent's terminal closed");
                }
            }
            return;
        }

        // Ctrl-C quits from any mode.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.request_quit();
            return;
        }

        match self.mode {
            Mode::Search => self.on_key_search(key),
            Mode::SortBy => self.on_key_sortby(key),
            Mode::AgeFilter => self.on_key_age(key),
            Mode::DeleteConfirm => self.on_key_delete(key),
            Mode::KillConfirm => self.on_key_kill(key),
            Mode::ResumeConfirm => self.on_key_resume(key),
            Mode::TmuxInstall => {
                self.tmux_install_answer(key.code == KeyCode::Char('y'));
            }
            Mode::QuitConfirm => self.on_key_quit(key),
            Mode::BatchConfirm | Mode::BatchDeleteBlocked | Mode::BatchKillBlocked => {
                self.on_key_batch(key)
            }
            Mode::CostFilter => self.on_key_cost(key),
            Mode::SendKeys => self.on_key_send(key),
            Mode::Launch => self.on_key_launch(key),
            Mode::Hooks => self.on_key_hooks(key),
            Mode::Help => self.on_key_help(key),
            Mode::DeleteBlocked | Mode::KillBlocked => self.mode = Mode::List,
            Mode::List => self.on_key_list(key),
        }
    }

    /// A paste, which the terminal hands over whole rather than as the keys it
    /// spells.
    ///
    /// Inside a pane it belongs to the agent and goes down the pty in one write;
    /// see [`Attach::send_paste`](crate::attach::Attach::send_paste) for what
    /// happens to it on the way. Everywhere else the only thing on screen that
    /// can hold text is whichever one-line input is open, so a paste is typing
    /// into that and nothing at all when none is open. It is deliberately not a
    /// shortcut for anything: pasting into the dashboard is somebody aiming at a
    /// box, and answering it with an action would be a command nobody typed.
    pub(super) fn on_paste(&mut self, text: &str) {
        self.needs_redraw = true;

        if self.tab > 0 && self.mode == Mode::List {
            if let Some(pane) = self.focused_pane() {
                // The same bookkeeping a keystroke does: text put in front of an
                // agent is an answer to whatever it asked, and no hook reports
                // that.
                let agent = pane.agent();
                let alive = pane.view.send_paste(text);
                self.mark_answered(agent);
                if !alive {
                    self.close_pane();
                    self.set_status("The agent's terminal closed");
                }
            }
            return;
        }

        match self.mode {
            Mode::Search => {
                self.search.push_str(&flatten(text, usize::MAX));
                self.search_edited();
            }
            Mode::SendKeys => {
                let room = 500usize.saturating_sub(self.send_input.len());
                self.send_input.push_str(&flatten(text, room));
            }
            // The cost floor is a number, so a paste is filtered the way typing
            // one is rather than flattened: anything that is not a digit or a
            // point could not have been typed here either.
            Mode::CostFilter => {
                let room = 12usize.saturating_sub(self.cost_input.len());
                let digits: String = text
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == '.')
                    .take(room)
                    .collect();
                self.cost_input.push_str(&digits);
            }
            _ => {}
        }
    }

    /// The multiplexer keys, live everywhere including inside a pane. Returns
    /// false for an Alt- combination that means nothing here, so it still
    /// reaches the agent.
    fn on_key_workspace(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Left => self.cycle_workspace(-1),
            KeyCode::Right => self.cycle_workspace(1),
            // The dashboard is tab 1, matching where it sits in the tab bar.
            KeyCode::Char(c @ '1'..='9') => self.show_tab(c as usize - '1' as usize),
            KeyCode::Char('n') => self.launch_prompt(LaunchInto::Tab),
            KeyCode::Char('v') => self.launch_prompt(LaunchInto::Split { stacked: false }),
            KeyCode::Char('s') => self.launch_prompt(LaunchInto::Split { stacked: true }),
            KeyCode::Char('o') => match self.active_tab() {
                Some(tab) => tab.cycle_focus(),
                None => return false,
            },
            KeyCode::Char('w') => self.close_pane(),
            // Shifted, because it is the irreversible one: `w` on a tmux-backed
            // pane only detaches, and the key that ends the agent should not be
            // the same key with a slip of a finger.
            KeyCode::Char('W') if key.modifiers.contains(KeyModifiers::SHIFT) => self.kill_pane(),
            _ => return false,
        }
        true
    }

    fn on_key_launch(&mut self, key: KeyEvent) {
        let n = self.launch_choices().len().max(1);
        match key.code {
            // Backing out of the launcher abandons the handoff with it: a brief
            // left pending would be typed at whatever agent is started next,
            // which by then is an unrelated one.
            KeyCode::Esc => {
                self.mode = Mode::List;
                self.pending_brief = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.launch_cursor = (self.launch_cursor + n - 1) % n
            }
            KeyCode::Down | KeyCode::Char('j') => self.launch_cursor = (self.launch_cursor + 1) % n,
            KeyCode::Enter => {
                self.mode = Mode::List;
                self.launch_selected();
            }
            _ => {}
        }
    }

    /// The integration panel. Every action rewrites somebody's settings file,
    /// so each is a distinct letter — there is no cursor to land on the wrong
    /// row and no Enter that does whatever was last highlighted.
    fn on_key_hooks(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                self.mode = Mode::List;
                self.hooks = None;
            }
            KeyCode::Char('i') => self.set_hooks(crate::hook::Scope::User, true),
            KeyCode::Char('x') => self.set_hooks(crate::hook::Scope::User, false),
            KeyCode::Char('p') | KeyCode::Char('P') => match self.hook_project() {
                Some(dir) => self.set_hooks(
                    crate::hook::Scope::Project(dir),
                    key.code == KeyCode::Char('p'),
                ),
                None => self.set_status("The selected session has no project directory here"),
            },
            _ => {}
        }
    }

    fn on_key_search(&mut self, key: KeyEvent) {
        match key.code {
            // Enter is "I meant that one"; Esc is backing out. Only the former
            // is worth remembering, or the history fills with abandoned
            // prefixes typed on the way to somewhere else.
            KeyCode::Enter => {
                self.remember_query();
                self.mode = Mode::List;
            }
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Backspace => {
                self.search.pop();
                self.search_edited();
            }
            // Tab rather than a letter: every printable character belongs to the
            // query being typed.
            KeyCode::Tab => self.toggle_content_search(),
            KeyCode::Up => self.history_step(1),
            KeyCode::Down => self.history_step(-1),
            KeyCode::Char(c) => {
                self.search.push(c);
                self.search_edited();
            }
            _ => {}
        }
    }

    /// The resume confirmation, shown only when the session is already running.
    fn on_key_resume(&mut self, key: KeyEvent) {
        self.mode = Mode::List;
        if key.code == KeyCode::Char('y') {
            self.resume_now();
        }
    }

    /// The help text is longer than most terminals are tall, so the navigation
    /// keys scroll it and everything else still dismisses it.
    fn on_key_help(&mut self, key: KeyEvent) {
        let step = |app: &mut App, delta: i32| {
            app.help_scroll =
                (app.help_scroll as i32 + delta).clamp(0, app.help_max_scroll as i32) as u16;
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => step(self, -1),
            KeyCode::Down | KeyCode::Char('j') => step(self, 1),
            KeyCode::PageUp => step(self, -(PAGE as i32)),
            KeyCode::PageDown | KeyCode::Char(' ') => step(self, PAGE as i32),
            KeyCode::Home | KeyCode::Char('g') => self.help_scroll = 0,
            KeyCode::End | KeyCode::Char('G') => self.help_scroll = self.help_max_scroll,
            _ => {
                self.mode = Mode::List;
                self.help_scroll = 0;
            }
        }
    }

    fn on_key_sortby(&mut self, key: KeyEvent) {
        let n = COLUMNS.len();
        match key.code {
            KeyCode::Esc | KeyCode::F(6) => self.mode = Mode::List,
            KeyCode::Up | KeyCode::Char('k') => {
                self.sortby_cursor = (self.sortby_cursor + n - 1) % n
            }
            KeyCode::Down | KeyCode::Char('j') => self.sortby_cursor = (self.sortby_cursor + 1) % n,
            KeyCode::Enter => {
                self.set_sort(COLUMNS[self.sortby_cursor].id);
                self.mode = Mode::List;
            }
            KeyCode::Char('q') => self.request_quit(),
            _ => {}
        }
    }

    fn on_key_age(&mut self, key: KeyEvent) {
        let n = AGE_OPTIONS.len();
        match key.code {
            KeyCode::Esc | KeyCode::F(7) => self.mode = Mode::List,
            KeyCode::Up | KeyCode::Char('k') => self.age_cursor = (self.age_cursor + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') => self.age_cursor = (self.age_cursor + 1) % n,
            KeyCode::Enter => {
                self.age_filter = AGE_OPTIONS[self.age_cursor];
                self.refilter();
                self.save_prefs();
                self.mode = Mode::List;
            }
            _ => {}
        }
    }

    fn on_key_delete(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('y')
            && let Some(s) = self.selected_session().cloned()
        {
            if self.tx.send(Request::Delete(Box::new(s.clone()))).is_ok() {
                self.deleting.insert(s.key());
                self.set_status(format!("Deleting session {}…", s.session_id));
            } else {
                self.set_status("Could not start session deletion");
            }
        }
        self.mode = Mode::List;
    }

    fn on_key_kill(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('y')
            && let Some(pid) = self.selected_session().and_then(session_root_pid)
            && let Some(session) = self.selected_session()
        {
            let _ = self.tx.send(Request::Terminate {
                session_key: session.key(),
                pid,
            });
            self.set_status(format!("Stopping session {}…", session.session_id));
        }
        self.mode = Mode::List;
    }

    /// Quit, or ask first when it would take the hosted agent down.
    ///
    /// `q` is muscle memory in an htop-like list, and here it would end a live
    /// coding session: the agent runs on a pty this process owns, so there is
    /// nothing left of it once cctop is gone.
    fn request_quit(&mut self) {
        match self.hosted.is_some() {
            true => self.mode = Mode::QuitConfirm,
            false => self.should_quit = true,
        }
    }

    fn on_key_quit(&mut self, key: KeyEvent) {
        self.mode = Mode::List;
        match key.code {
            KeyCode::Char('y') => self.should_quit = true,
            KeyCode::Char('A') => self.attach_hosted(),
            _ => {}
        }
    }

    fn on_key_batch(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('y') && self.mode == Mode::BatchConfirm {
            self.batch_execute();
        }
        self.mode = Mode::List;
    }

    fn on_key_cost(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Enter => {
                if let Ok(v) = self.cost_input.parse::<f64>() {
                    self.cost_floor = v.max(0.0);
                    self.refilter();
                    self.save_prefs();
                    self.set_status(if v > 0.0 {
                        format!("Cost floor: ${v:.2}")
                    } else {
                        "Cost floor cleared".into()
                    });
                }
                self.mode = Mode::List;
            }
            KeyCode::Backspace => {
                self.cost_input.pop();
            }
            KeyCode::Char(c) if (c.is_ascii_digit() || c == '.') && self.cost_input.len() < 12 => {
                self.cost_input.push(c);
            }
            _ => {}
        }
    }

    fn on_key_send(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Enter => {
                let text = self.send_input.clone();
                if !text.is_empty()
                    && let Some(pid) = self.selected_session().and_then(session_root_pid)
                {
                    let _ = self.tx.send(Request::SendKeys { pid, text });
                    self.set_status("Sending…");
                }
                self.mode = Mode::List;
            }
            KeyCode::Backspace => {
                self.send_input.pop();
            }
            KeyCode::Char(c) if self.send_input.len() < 500 => self.send_input.push(c),
            _ => {}
        }
    }

    fn on_key_list(&mut self, key: KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Shift+Up/Down scrolls inside the active bottom panel, since the plain
        // arrows are taken by list navigation and panel switching.
        if shift && matches!(key.code, KeyCode::Up | KeyCode::Down) {
            self.scroll_active_panel(if key.code == KeyCode::Up { -1 } else { 1 });
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::F(10) => self.request_quit(),
            // Terminate is deliberately behind a modifier: `k` is vim's "up",
            // and every modal in this file already binds it that way, so a
            // plain `k` aimed at the cursor must never reach a live agent.
            // Plain `K` is taken by the batch kill, hence Ctrl.
            KeyCode::Char('k') if ctrl => self.confirm_terminate(),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-PAGE),
            KeyCode::PageDown => self.move_selection(PAGE),
            // `b` was a second name for PageUp; answering a bell is worth more
            // than a third way to scroll up, and PageUp and Ctrl+U both remain.
            KeyCode::Char('b') => self.jump_to_bell(),
            KeyCode::Char('g') => {
                self.selected = 0;
                self.ensure_available_tab();
                self.needs_redraw = true;
            }
            KeyCode::Char('G') => {
                self.selected = self.visible.len().saturating_sub(1);
                self.ensure_available_tab();
                self.needs_redraw = true;
            }
            KeyCode::Home => {
                self.selected = 0;
                self.ensure_available_tab();
            }
            KeyCode::End => {
                self.selected = self.visible.len().saturating_sub(1);
                self.ensure_available_tab();
            }
            KeyCode::Char('u') if ctrl => {
                self.move_selection(-(self.half_page() as isize));
            }
            KeyCode::Char('d') if ctrl => {
                self.move_selection(self.half_page() as isize);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => self.adjust_refresh(0.5),
            KeyCode::Char('-') | KeyCode::Char('_') => self.adjust_refresh(-0.5),
            KeyCode::Char('f') => {
                self.follow = !self.follow;
                self.set_status(if self.follow {
                    "Follow mode on"
                } else {
                    "Follow mode off"
                });
            }
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Char('e') => self.toggle_expanded(),
            KeyCode::Char('E') => self.toggle_expanded_all(),
            KeyCode::Char('U') => self.unmark_all(),
            KeyCode::Char('D') => self.batch(BatchKind::Delete),
            KeyCode::Char('K') => self.batch(BatchKind::Kill),
            KeyCode::Char('n') => self.cycle_matches(1),
            KeyCode::Char('N') => self.cycle_matches(-1),
            // `w` for the bell, not `n`: n/N is next/previous match everywhere
            // a search exists, and there were free letters to spend instead.
            KeyCode::Char('w') => self.toggle_notifications(),
            KeyCode::Char('#') => {
                self.cost_input = if self.cost_floor > 0.0 {
                    format!("{:.2}", self.cost_floor)
                } else {
                    String::new()
                };
                self.mode = Mode::CostFilter;
            }
            KeyCode::Char('H') => self.set_sort(ColumnId::Harness),
            KeyCode::Char('X') => self.set_sort(ColumnId::Context),
            KeyCode::Char('S') => self.set_sort(ColumnId::Tools),

            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            // Bounded by the tab list rather than a literal range, so a panel
            // added to `panels::TABS` gets its number key for free.
            KeyCode::Char(c @ '1'..='9') => {
                let tab = c as usize - '1' as usize;
                if tab < super::panels::TABS.len() && self.tab_available(tab) {
                    self.bottom_tab = tab;
                    self.save_prefs();
                }
            }
            KeyCode::Char('`') => {
                self.live_only = !self.live_only;
                self.refilter();
                self.save_prefs();
            }

            // `h` rather than `H`, which already sorts by harness.
            KeyCode::Char('h') | KeyCode::F(8) => self.open_hooks(),
            KeyCode::Char('/') | KeyCode::F(3) => self.mode = Mode::Search,
            KeyCode::Char('?') | KeyCode::F(1) => self.mode = Mode::Help,
            KeyCode::Char('>') | KeyCode::Char('<') | KeyCode::F(6) => {
                self.sortby_cursor = COLUMNS
                    .iter()
                    .position(|c| c.id == self.sort_col)
                    .unwrap_or(0);
                self.mode = Mode::SortBy;
            }
            KeyCode::F(7) => {
                self.age_cursor = AGE_OPTIONS
                    .iter()
                    .position(|o| *o == self.age_filter)
                    .unwrap_or(AGE_OPTIONS.len() - 1);
                self.mode = Mode::AgeFilter;
            }
            KeyCode::Char('r') | KeyCode::F(5) => {
                let _ = self.tx.send(Request::Refresh);
                self.set_status("Refreshing…");
            }
            // Everything below reaches into *this* machine — a signal to a
            // process, a transcript on disk, a pty. A row read over ssh has
            // none of those here, and the same path on this filesystem is a
            // different file. Refused with the host named, rather than left to
            // fail further in with a message about a missing process.
            KeyCode::Char('d' | 's' | 'a' | 'R' | 'O') if self.selected_is_remote() => {
                let why = self.selected_session().and_then(App::remote_refusal);
                if let Some(why) = why {
                    self.set_status(why);
                }
            }
            KeyCode::Char('d') if self.on_subagent() => {
                self.set_status("A subagent cannot be deleted on its own")
            }
            KeyCode::Char('d') => match self.selected_session() {
                Some(s) if self.deleting.contains(&s.key()) => {
                    self.set_status("Session deletion is already in progress")
                }
                Some(s) if s.is_running() => self.mode = Mode::DeleteBlocked,
                Some(_) => self.mode = Mode::DeleteConfirm,
                None => {}
            },
            // Prefilled with the answer a stalled session usually wants, so
            // s-Enter is the whole interaction.
            KeyCode::Char('s') if self.on_subagent() => {
                self.set_status("A subagent cannot be typed into on its own")
            }
            KeyCode::Char('s') => match self.selected_session() {
                Some(s) if session_root_pid(s).is_some() => {
                    self.send_input = "continue".into();
                    self.mode = Mode::SendKeys;
                }
                Some(_) => self.set_status("Selected session has no local process to type into"),
                None => {}
            },
            KeyCode::Char('a') if self.on_subagent() => {
                self.set_status("A subagent has no terminal of its own to attach to")
            }
            KeyCode::Char('a') => self.attach_selected(),
            KeyCode::Char('A') => self.attach_hosted(),
            // `R`, because `r` refreshes. Capital also matches how the other
            // keys that start something irreversible are spelled.
            KeyCode::Char('R') => self.resume_selected(),
            // `O` for hand-off, capitalised alongside `R`: both take a session
            // somewhere else and both start an agent, so neither belongs on a
            // lowercase key. `H` was already sort-by-harness.
            KeyCode::Char('O') if self.on_subagent() => {
                self.set_status("Hand off the session, not one of its subagents")
            }
            KeyCode::Char('O') => self.handoff_selected(),
            // Alt+n does this too and works from inside a pane; here on the
            // dashboard, where nothing is competing for the keyboard, a plain
            // letter is what anyone will try first.
            KeyCode::Char('t') => self.launch_prompt(LaunchInto::Tab),
            KeyCode::Char('y') => self.copy_selection(),
            KeyCode::Char('L') => {
                self.tool_live_only = !self.tool_live_only;
                self.save_prefs();
            }
            KeyCode::Char('v') => {
                self.tool_show_diff = !self.tool_show_diff;
                self.save_prefs();
            }
            // Move through the Tool Activity filter sidebar.
            KeyCode::Char('[') => self.cycle_tool_filter(-1),
            KeyCode::Char(']') => self.cycle_tool_filter(1),

            // htop muscle memory.
            KeyCode::Char('P') => self.set_sort(ColumnId::Status),
            KeyCode::Char('M') => self.set_sort(ColumnId::Memory),
            KeyCode::Char('T') => self.set_sort(ColumnId::Cost),

            // Arrows move between bottom panels; Shift+arrows scroll within one.
            KeyCode::Left => self.cycle_tab(-1),
            KeyCode::Right => self.cycle_tab(1),
            KeyCode::Esc => self.clear_one_filter(),
            _ => {}
        }
    }

    pub(super) fn on_mouse(&mut self, ev: event::MouseEvent, layout: &render::Layout) {
        // Anything the mouse actually does changes the screen, and unlike
        // `on_key` there is nothing downstream to rely on for the frame:
        // `launch_prompt` opening the launcher and `set_sort` reordering the
        // table both leave the previous picture up until some unrelated event
        // repaints it, which reads as a dead click. Movement is left out —
        // capture reports it continuously and it changes nothing.
        if matches!(
            ev.kind,
            MouseEventKind::Down(_) | MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            self.needs_redraw = true;
        }

        // A modal owns the mouse while it is up. Without this the dashboard
        // underneath still answers, so a click on a launcher row lands on the
        // panel tab or session row the modal is drawn over.
        //
        // Only a modal that recorded its rectangle, though, since that rectangle
        // is the whole means of telling a click meant for the modal from one
        // meant for what it covers. The search box records none: it is a strip
        // over a table that is still being scrolled and clicked while the query
        // is typed, and swallowing the wheel there strands the filter it exists
        // to drive.
        if self.mode != Mode::List && layout.modal_rect.is_some() {
            if ev.kind != MouseEventKind::Down(MouseButton::Left) {
                return;
            }
            // One click picks; Enter, or a second click on the row already
            // picked, starts it — so no single stray click starts an agent.
            if let Some(i) = layout.launch_row_at(ev.column, ev.row) {
                match i == self.launch_cursor {
                    true => {
                        self.mode = Mode::List;
                        self.launch_selected();
                    }
                    false => {
                        self.launch_cursor = i;
                        self.needs_redraw = true;
                    }
                }
            } else if layout.modal_rect.is_some() && !layout.in_modal(ev.column, ev.row) {
                // Clicking off a modal is how everyone dismisses one. A modal
                // that did not record its rectangle swallows the click instead
                // of guessing that it was aimed elsewhere.
                self.mode = Mode::List;
            }
            return;
        }

        // Inside a tab the mouse has two targets: the tab bar, and the agents
        // themselves. Clicks are the bar's — mouse capture also reports
        // movement, and switching tabs on a hover means the pointer resting
        // anywhere near the bar drags you out of the agent you are typing into.
        if self.tab > 0 {
            if ev.kind == MouseEventKind::Down(MouseButton::Left) {
                if let Some(tab) = layout.workspace_at(ev.column, ev.row) {
                    self.show_tab(tab);
                } else if layout.workspace_new_at(ev.column, ev.row) {
                    self.launch_prompt(LaunchInto::Tab);
                }
                return;
            }
            // The wheel is the agent's, wherever it is pointed — cctop keeps no
            // scrollback of its own, so a pane's history lives in the agent (or
            // in the tmux around it) and only the agent can scroll it. The pane
            // under the pointer, not the focused one, because the wheel says
            // where it is aimed and stealing focus to answer it would move the
            // keyboard out from under someone mid-sentence.
            let up = match ev.kind {
                MouseEventKind::ScrollUp => true,
                MouseEventKind::ScrollDown => false,
                _ => return,
            };
            if let Some((i, col, row)) = layout.pane_at(ev.column, ev.row)
                && let Some(pane) = self.active_tab().and_then(|t| t.panes.get_mut(i))
            {
                // A failed send means that agent is gone, which the reaper is
                // already watching for. Unlike a keystroke there is nothing to
                // report: a scroll that landed on a dead pane asked for nothing.
                let _ = pane.view.wheel(up, col, row);
            }
            return;
        }
        match ev.kind {
            MouseEventKind::ScrollDown => {
                if layout.in_bottom_panel(ev.row) {
                    self.scroll_active_panel(1);
                } else {
                    self.move_selection(1);
                }
            }
            MouseEventKind::ScrollUp => {
                if layout.in_bottom_panel(ev.row) {
                    self.scroll_active_panel(-1);
                } else {
                    self.move_selection(-1);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(tab) = layout.workspace_at(ev.column, ev.row) {
                    self.show_tab(tab);
                } else if layout.workspace_new_at(ev.column, ev.row) {
                    self.launch_prompt(LaunchInto::Tab);
                } else if self.bottom_tab == 3
                    && let Some(offset) = layout.tool_log_row_at(ev.column, ev.row)
                {
                    self.toggle_tool_expansion(offset);
                } else if let Some(idx) = layout.tool_sidebar_at(ev.column, ev.row) {
                    self.tool_tab = idx;
                    self.tool_follow = true;
                    self.needs_redraw = true;
                } else if let Some(tab) = layout.tab_at(ev.column, ev.row) {
                    self.bottom_tab = tab;
                    self.save_prefs();
                    self.needs_redraw = true;
                } else if let Some(col) = layout.header_column_at(ev.column, ev.row) {
                    self.set_sort(col);
                } else if let Some(row) = layout.row_at(ev.row) {
                    let idx = self.scroll + row;
                    if idx < self.visible.len() {
                        self.selected = idx;
                        self.ensure_available_tab();
                        self.needs_redraw = true;
                    }
                }
            }
            _ => {}
        }
    }
}
