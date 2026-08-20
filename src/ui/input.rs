//! Key and mouse handling: translate input events into state changes.

use super::columns::{COLUMNS, ColumnId};
use super::{
    AGE_OPTIONS, App, BatchKind, LaunchInto, Mode, PAGE, Request, render, session_root_pid,
};
/// Longest path the launcher's directory field accepts.
///
/// Comfortably past any real working directory — Linux caps a path at 4096
/// bytes and this is about the width of four terminals — while still bounding
/// what a runaway paste can put in one line.
pub(super) const MAX_PATH_INPUT: usize = 512;

/// Longest name a tab will take.
///
/// The bar truncates well before this, so a longer name is one nobody can read
/// anyway; the cap is here so a paste cannot fill the tmux option with a
/// transcript.
pub(super) const TAB_NAME_MAX: usize = 64;

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

        // Inside a pane the keyboard belongs to the agent — every key but the
        // function keys, which are cctop's wherever you are. The footer offers
        // them from inside a pane, so one that reached the agent instead would
        // be a promise the pane quietly broke.
        if self.tab > 0 && self.mode == Mode::List {
            if matches!(key.code, KeyCode::F(_)) {
                self.on_key_function(key);
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
            Mode::RenameTab => self.on_key_rename(key),
            Mode::Launch => self.on_key_launch(key),
            Mode::RowMenu => self.on_key_menu(key),
            Mode::LaunchCwd => self.on_key_launch_cwd(key),
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
            Mode::RenameTab => {
                let room = TAB_NAME_MAX.saturating_sub(self.rename_input.chars().count());
                self.rename_input.push_str(&flatten(text, room));
            }
            // Pasting a path in is the point of this field: a directory deep
            // enough to be worth typing is one you copied from somewhere.
            Mode::LaunchCwd => {
                let room = MAX_PATH_INPUT.saturating_sub(self.launch_cwd_input.chars().count());
                self.launch_cwd_input.push_str(&flatten(text, room));
                self.launch_cwd_bad = false;
                self.launch_cwd_suggest();
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
            // Shifted, the arrows carry the tab instead of moving between them
            // — the keyboard's half of dragging one along the bar.
            KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => self.move_workspace(-1),
            KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => self.move_workspace(1),
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
            // Only where there is more than one account to be in, so the key is
            // absent rather than inert on the machines that have never had two.
            KeyCode::Char('p') => self.cycle_launch_profile(),
            // `c` for the directory it will start in. Not offered while
            // reattaching: that agent is already somewhere, and the footer says
            // so — a path typed there would be quietly ignored.
            KeyCode::Char('c') if !self.launch_is_reattach() => self.edit_launch_cwd(),
            KeyCode::Enter => {
                self.mode = Mode::List;
                self.launch_selected();
            }
            _ => {}
        }
    }

    /// Typing the launcher's working directory.
    ///
    /// Accepting is where the path is checked, not where it is typed: a
    /// directory half-spelled is not yet wrong, and colouring it red on the way
    /// through would be noise on every keystroke.
    fn on_key_launch_cwd(&mut self, key: KeyEvent) {
        match key.code {
            // Back to the list with the old directory intact. Cancelling has to
            // leave the launch exactly as it was found, or Esc becomes a way to
            // lose the setting you were trying to change.
            KeyCode::Esc => self.mode = Mode::Launch,
            KeyCode::Enter => self.take_launch_cwd(),
            // Tab fills in what the suggestions agree on. The list below the
            // field is what makes the key discoverable; without it a path still
            // has to be spelled to the last character.
            KeyCode::Tab => self.complete_launch_cwd(),
            // Into the suggestions and back out again. Nothing else in this
            // field wanted the arrows, and the launcher's own list is not being
            // moved while its directory is being typed.
            KeyCode::Down => self.step_launch_cwd(true),
            KeyCode::Up => self.step_launch_cwd(false),
            KeyCode::Backspace => {
                self.launch_cwd_input.pop();
                self.launch_cwd_bad = false;
                self.launch_cwd_suggest();
            }
            // Bounded like every other one-line input here: a path longer than
            // this is not one anybody typed on purpose.
            KeyCode::Char(c) if self.launch_cwd_input.chars().count() < MAX_PATH_INPUT => {
                self.launch_cwd_input.push(c);
                self.launch_cwd_bad = false;
                self.launch_cwd_suggest();
            }
            _ => {}
        }
        self.needs_redraw = true;
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
    /// A function key pressed inside a pane.
    ///
    /// None of them is passed on. Agents do not read them — nothing in
    /// `claude`, `codex`, or a shell binds one — and cctop's own map is written
    /// in them, which is why they were the keys it kept.
    ///
    /// Most act on the dashboard: a search box, a sort order, or the help sheet
    /// drawn over a pane would be a modal on a screen the agent is repainting
    /// underneath, and the thing being filtered is not on screen at all. So the
    /// dashboard comes forward first and the key then does exactly what it does
    /// there. The three that need no dashboard stay where they are pressed.
    fn on_key_function(&mut self, key: KeyEvent) {
        match key.code {
            // Back to the dashboard, which is the one function key that only
            // means anything inside a pane.
            KeyCode::F(12) => self.show_tab(0),
            // Quitting is the pane's own key, and refreshing acts on the walk
            // rather than on anything drawn — pulling the dashboard forward for
            // it would take you off the agent you are watching in order to
            // reload a table you were not looking at.
            KeyCode::F(10) | KeyCode::F(5) => self.on_key_list(key),
            // The keys the dashboard binds, on the dashboard.
            KeyCode::F(1) | KeyCode::F(3) | KeyCode::F(6) | KeyCode::F(7) | KeyCode::F(8) => {
                self.show_tab(0);
                self.on_key_list(key);
            }
            // Everything else: swallowed. An unbound function key does nothing
            // here rather than arriving at the agent as an escape sequence it
            // will print or misread.
            _ => {}
        }
    }

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

    /// The tab-rename field. Empty is not a name, so Enter with nothing typed
    /// backs out the same way Esc does rather than blanking the tab bar.
    fn on_key_rename(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Enter => {
                let name = self.rename_input.trim().to_string();
                self.mode = Mode::List;
                if name.is_empty() {
                    return;
                }
                // The bar may have moved while the field was open — see
                // [`App::rename_was`]. A tab that is no longer the one the
                // right-click landed on keeps its name.
                let Some(tab) = self
                    .tabs
                    .get_mut(self.rename_tab.saturating_sub(1))
                    .filter(|tab| tab.title() == self.rename_was)
                else {
                    self.set_status("That tab is gone; nothing was renamed");
                    return;
                };
                tab.rename(name.clone());
                self.set_status(format!("Tab renamed to {name}"));
            }
            KeyCode::Backspace => {
                self.rename_input.pop();
            }
            KeyCode::Char(c) if self.rename_input.chars().count() < TAB_NAME_MAX => {
                self.rename_input.push(c)
            }
            _ => {}
        }
    }

    /// Ask for a new name for a tab, addressed the way the bar numbers them:
    /// tab 0 is the dashboard, which is not a tab anything renames.
    fn rename_prompt(&mut self, tab: usize) {
        let Some(target) = self.tabs.get(tab.saturating_sub(1)) else {
            return;
        };
        self.rename_tab = tab;
        self.rename_was = target.title();
        self.rename_input.clear();
        self.mode = Mode::RenameTab;
        self.needs_redraw = true;
    }

    /// Open the row menu on the first entry that can actually run.
    pub(super) fn open_row_menu(&mut self) {
        let items = super::menu::items(self);
        if items.is_empty() {
            return;
        }
        self.menu_cursor = super::menu::first_enabled(&items);
        self.mode = Mode::RowMenu;
        self.needs_redraw = true;
    }

    /// Keys inside the row menu.
    ///
    /// The shortcut letters stay live in here too, so `Enter d` and a plain `d`
    /// are the same two keystrokes and neither has to be unlearned — the menu
    /// shows the letters precisely so they get used.
    fn on_key_menu(&mut self, key: KeyEvent) {
        let items = super::menu::items(self);
        if items.is_empty() {
            self.mode = Mode::List;
            return;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::List,
            KeyCode::Up | KeyCode::Char('k') => {
                self.menu_cursor = super::menu::step(&items, self.menu_cursor, -1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.menu_cursor = super::menu::step(&items, self.menu_cursor, 1);
            }
            KeyCode::Enter => {
                if let Some(item) = items.get(self.menu_cursor)
                    && item.enabled()
                {
                    let action = item.action;
                    self.mode = Mode::List;
                    self.run_menu_action(action);
                }
            }
            // A blocked entry's letter says why rather than doing nothing,
            // which is the same answer the table gives for the same key.
            KeyCode::Char(c) => {
                let hit = items
                    .iter()
                    .find(|i| i.key.len() == 1 && i.key.starts_with(c));
                if let Some(item) = hit {
                    let action = item.action;
                    let blocked = item.blocked.clone();
                    self.mode = Mode::List;
                    match blocked {
                        Some(why) => self.set_status(why),
                        None => self.run_menu_action(action),
                    }
                }
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    /// Run one menu entry, through the same method its key calls.
    fn run_menu_action(&mut self, action: super::menu::Action) {
        use super::menu::Action;
        match action {
            Action::Resume => self.resume_selected(),
            Action::Attach => self.attach_selected(),
            Action::Send => self.send_prompt(),
            Action::Handoff => self.handoff_selected(),
            Action::Expand => self.toggle_expanded(),
            Action::Mark => self.toggle_mark(),
            Action::Terminate => self.confirm_terminate(),
            Action::Delete => self.delete_selected(),
        }
    }

    /// Start deleting the selected session's transcript, or say why not.
    ///
    /// Split out from the `d` arm so the row menu runs the identical path. Two
    /// routes to one action must not have two ideas of when it is allowed.
    pub(super) fn delete_selected(&mut self) {
        if self.on_subagent() {
            self.set_status("A subagent cannot be deleted on its own");
            return;
        }
        match self.selected_session() {
            Some(s) if self.deleting.contains(&s.key()) => {
                self.set_status("Session deletion is already in progress")
            }
            Some(s) if s.is_running() => self.mode = Mode::DeleteBlocked,
            Some(_) => self.mode = Mode::DeleteConfirm,
            None => {}
        }
    }

    /// Open the send box on the selected session, or say why not.
    ///
    /// Prefilled with the answer a stalled session usually wants, so s-Enter is
    /// the whole interaction.
    pub(super) fn send_prompt(&mut self) {
        if self.on_subagent() {
            self.set_status("A subagent cannot be typed into on its own");
            return;
        }
        match self.selected_session() {
            Some(s) if session_root_pid(s).is_some() => {
                self.send_input = "continue".into();
                self.mode = Mode::SendKeys;
            }
            Some(_) => self.set_status("Selected session has no local process to type into"),
            None => {}
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
            // Enter is the one key in this map that does not do a thing so
            // much as show what the others do. It was free, it is what "open
            // this row" means everywhere else, and unlike a bare modifier every
            // terminal actually delivers it.
            KeyCode::Enter if self.selected_session().is_some() => self.open_row_menu(),
            KeyCode::Char('d') => self.delete_selected(),
            KeyCode::Char('s') => self.send_prompt(),
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

    /// The workspace bar's mouse: picking a tab, the new-tab button, and
    /// dragging a tab to a different place in the bar. Returns whether the
    /// event belonged to the bar, in which case nothing downstream sees it.
    ///
    /// Consuming the whole drag, not just the part over the bar, is what keeps a
    /// rearrangement out of the agents: a press on a tab followed by a pointer
    /// that wanders down into a pane would otherwise arrive there as a click and
    /// a release the agent never saw the press for.
    fn on_mouse_workspace(&mut self, ev: event::MouseEvent, layout: &render::Layout) -> bool {
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(tab) = layout.workspace_at(ev.column, ev.row) {
                    self.show_tab(tab);
                    // The dashboard is tab zero wherever the bar is drawn and
                    // stays there, so only a real tab is picked up.
                    self.drag_tab = (tab > 0).then_some(tab);
                    return true;
                }
                if layout.workspace_new_at(ev.column, ev.row) {
                    self.launch_prompt(LaunchInto::Tab);
                    return true;
                }
                false
            }
            // Reordering as the pointer moves rather than on the release: the
            // bar is the only feedback there is for where the tab will land, and
            // one that only redraws at the end is a drag you have to guess at.
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(from) = self.drag_tab else {
                    return false;
                };
                if let Some(to) = layout.workspace_at(ev.column, ev.row).filter(|to| *to > 0) {
                    self.move_tab(from, to);
                    self.drag_tab = Some(to);
                    self.needs_redraw = true;
                }
                true
            }
            MouseEventKind::Up(MouseButton::Left) => self.drag_tab.take().is_some(),
            // Right-click renames. The bar is the only place cctop answers the
            // right button at all — inside a pane it is deliberately dropped
            // (see [`App::on_mouse`]) — so there is nothing here to compete
            // with, and a tab called `3:claude-4` is precisely the thing you
            // want to rename by pointing at it.
            MouseEventKind::Down(MouseButton::Right) => {
                match layout
                    .workspace_at(ev.column, ev.row)
                    .filter(|tab| *tab > 0)
                {
                    Some(tab) => {
                        self.rename_prompt(tab);
                        true
                    }
                    None => false,
                }
            }
            _ => false,
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
            // The row menu answers a single click: unlike the launcher, every
            // entry is an action the keyboard reaches in one keystroke too, and
            // the destructive two both stop at a confirmation of their own.
            if self.mode == Mode::RowMenu {
                if let Some(i) = layout.menu_row_at(ev.column, ev.row) {
                    let items = super::menu::items(self);
                    if let Some(item) = items.get(i)
                        && item.enabled()
                    {
                        let action = item.action;
                        self.mode = Mode::List;
                        self.run_menu_action(action);
                    }
                } else if !layout.in_modal(ev.column, ev.row) {
                    self.mode = Mode::List;
                }
                return;
            }
            // A click on a suggestion is answered before the choices behind
            // it: the two lists are drawn in one modal, and while the field is
            // open the lower rows are the directories, not the agents.
            if self.mode == Mode::LaunchCwd
                && let Some(i) = layout.launch_cwd_row_at(ev.column, ev.row)
            {
                // One click picks, a second takes it — the same two-step the
                // choices above use, for the same reason: a stray click must
                // not silently move where the agent will start.
                match self.launch_cwd_pick == Some(i) {
                    true => self.take_launch_cwd(),
                    false => self.launch_cwd_pick = Some(i),
                }
                self.needs_redraw = true;
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

        // The workspace bar owns its own row wherever it is drawn — over the
        // dashboard as much as over a set of panes — so it is asked before
        // either. Clicks and drags only: mouse capture also reports movement,
        // and switching tabs on a hover means the pointer resting anywhere near
        // the bar drags you out of the agent you are typing into.
        if self.on_mouse_workspace(ev, layout) {
            return;
        }

        // Inside a tab the rest of the mouse is the agents'.
        if self.tab > 0 {
            // Inside a pane the mouse is the agent's. Claude Code, opencode and
            // pi all ask for mouse reporting and act on it — placing the cursor
            // in the composer, picking a file, choosing from the agents list —
            // and cctop holds the terminal's capture, so without forwarding the
            // click simply went nowhere.
            //
            // The right button is the exception, and tmux is the reason. With
            // `mouse` on — which cctop turns on so the wheel scrolls — tmux
            // binds `MouseDown3Pane` to its own pane menu, so a right-click
            // inside an agent opened a tmux popup whose entries split the pane
            // in two. The binding lives in the server's `root` key table, not in
            // the session, so cctop cannot unbind it without changing the user's
            // own tmux sessions too; not sending the button is the fix that
            // stays inside cctop. No agent cctop hosts asks for right-click, so
            // nothing is lost by keeping it.
            let button = |b| match b {
                MouseButton::Left => Some(crate::attach::MouseButton::Left),
                MouseButton::Middle => Some(crate::attach::MouseButton::Middle),
                MouseButton::Right => None,
            };
            let action = match ev.kind {
                MouseEventKind::Down(b) => button(b).map(|b| (crate::attach::MouseKind::Press, b)),
                MouseEventKind::Up(b) => button(b).map(|b| (crate::attach::MouseKind::Release, b)),
                MouseEventKind::Drag(b) => button(b).map(|b| (crate::attach::MouseKind::Drag, b)),
                _ => None,
            };
            if let Some((kind, b)) = action {
                if let Some((i, col, row)) = layout.pane_at(ev.column, ev.row) {
                    // A press also moves the keyboard there, which is what
                    // clicking a pane means everywhere else. Only a press: a
                    // release ending a drag that wandered out of the pane it
                    // started in must not hand focus to whatever it landed on.
                    if kind == crate::attach::MouseKind::Press
                        && let Some(tab) = self.active_tab()
                    {
                        tab.focus = i;
                    }
                    if let Some(pane) = self.active_tab().and_then(|t| t.panes.get_mut(i)) {
                        // A failed send means that agent has gone, which the
                        // reaper already watches for.
                        let _ = pane.view.mouse(kind, b, col, row);
                    }
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
