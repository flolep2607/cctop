//! Key and mouse handling: translate input events into state changes.

use super::columns::{COLUMNS, ColumnId};
use super::{AGE_OPTIONS, App, BatchKind, Mode, PAGE, Request, render, session_root_pid};
use ratatui::crossterm::event::{
    self, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

impl App {
    pub(super) fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        self.needs_redraw = true;

        // While attached, the keyboard belongs to the agent — including Ctrl-C,
        // which is how you interrupt it. Only the detach key is cctop's, and it
        // is a function key because those are the ones never forwarded: any
        // Ctrl- combination worth pressing is one an agent might want.
        if self.attached.is_some() {
            if key.code == KeyCode::F(12) {
                self.attached = None;
                return;
            }
            if let Some(attached) = self.attached.as_mut()
                && !attached.send_key(key)
            {
                self.attached = None;
                self.set_status("The agent's terminal closed");
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
            Mode::QuitConfirm => self.on_key_quit(key),
            Mode::BatchConfirm | Mode::BatchDeleteBlocked | Mode::BatchKillBlocked => {
                self.on_key_batch(key)
            }
            Mode::CostFilter => self.on_key_cost(key),
            Mode::SendKeys => self.on_key_send(key),
            Mode::Help | Mode::DeleteBlocked | Mode::KillBlocked => self.mode = Mode::List,
            Mode::List => self.on_key_list(key),
        }
    }

    fn on_key_search(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.mode = Mode::List,
            KeyCode::Backspace => {
                self.search.pop();
                self.refilter();
            }
            KeyCode::Char(c) => {
                self.search.push(c);
                self.refilter();
            }
            _ => {}
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
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp | KeyCode::Char('b') => self.move_selection(-PAGE),
            KeyCode::PageDown => self.move_selection(PAGE),
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
            KeyCode::Char('U') => self.unmark_all(),
            KeyCode::Char('D') => self.batch(BatchKind::Delete),
            KeyCode::Char('K') => self.batch(BatchKind::Kill),
            KeyCode::Char('n') => self.cycle_matches(1),
            KeyCode::Char('N') => self.cycle_matches(-1),
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
            KeyCode::Char(c @ '1'..='7') => {
                let tab = c as usize - '1' as usize;
                if self.tab_available(tab) {
                    self.bottom_tab = tab;
                    self.save_prefs();
                }
            }
            KeyCode::Char('`') => {
                self.live_only = !self.live_only;
                self.refilter();
                self.save_prefs();
            }

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
            KeyCode::Char('d') => match self.selected_session() {
                Some(s) if self.deleting.contains(&s.key()) => {
                    self.set_status("Session deletion is already in progress")
                }
                Some(s) if s.is_running() => self.mode = Mode::DeleteBlocked,
                Some(_) => self.mode = Mode::DeleteConfirm,
                None => {}
            },
            KeyCode::Char('k') => match self.selected_session() {
                Some(s) if session_root_pid(s).is_some() => self.mode = Mode::KillConfirm,
                Some(s) if s.is_running() => self.mode = Mode::KillBlocked,
                Some(_) => self.set_status("Selected session is not running"),
                None => {}
            },
            // Prefilled with the answer a stalled session usually wants, so
            // s-Enter is the whole interaction.
            KeyCode::Char('s') => match self.selected_session() {
                Some(s) if session_root_pid(s).is_some() => {
                    self.send_input = "continue".into();
                    self.mode = Mode::SendKeys;
                }
                Some(_) => self.set_status("Selected session has no local process to type into"),
                None => {}
            },
            KeyCode::Char('a') => self.attach_selected(),
            KeyCode::Char('A') => self.attach_hosted(),
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
            KeyCode::Esc => {
                // Clear the narrowest active filter first.
                if !self.search.is_empty() {
                    self.search.clear();
                    self.refilter();
                } else if self.age_filter.is_some() {
                    self.age_filter = None;
                    self.refilter();
                    self.save_prefs();
                }
            }
            _ => {}
        }
    }

    pub(super) fn on_mouse(&mut self, ev: event::MouseEvent, layout: &render::Layout) {
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
                if self.bottom_tab == 3
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
