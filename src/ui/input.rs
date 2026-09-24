//! Key and mouse handling: translate input events into state changes.

use super::columns::COLUMNS;
use super::select::PAGE;
use super::{AGE_OPTIONS, App, BatchKind, LaunchInto, Mode, Request, render, theme};
/// Longest path the launcher's directory field accepts.
///
/// Comfortably past any real working directory — Linux caps a path at 4096
/// bytes and this is about the width of four terminals — while still bounding
/// what a runaway paste can put in one line.
pub(super) const MAX_PATH_INPUT: usize = 512;

/// Longest name a tab will take.
///
/// The bar truncates well before this, so a longer name is one nobody can read
/// anyway; the cap is here so a paste cannot fill the rmux option with a
/// transcript.
pub(super) const TAB_NAME_MAX: usize = 64;

/// Longest query the tab switcher takes — the same reasoning as the name it
/// is matched against, minus the need to store it anywhere.
const SWITCH_FILTER_MAX: usize = 64;

/// How long after a right-click a paste still counts as that click's echo.
///
/// One frame's worth of slack: the terminal writes the click and the clipboard
/// back to back, so anything this close arrived with the button, while a person
/// reaching for Ctrl+Shift+V cannot be here yet.
pub(super) const RIGHT_CLICK_PASTE: Duration = Duration::from_millis(250);

use ratatui::crossterm::event::{
    self, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use std::time::{Duration, Instant};

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
        // Except while the settings panel waits for a key to bind: then Alt+n
        // is the answer, not a new tab.
        if key.modifiers.contains(KeyModifiers::ALT)
            && !self.settings_capture
            && self.on_key_workspace(key)
        {
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
            // Ctrl+V with a picture on the clipboard, which is the gesture
            // anyone reaches for before they reach for F9. Taken only when
            // there is an image to take it for: with text on the clipboard the
            // key goes to the agent untouched, as it always did.
            //
            // Not every terminal sends it: Windows Terminal binds Ctrl+V to
            // its own paste and the application is never told, so there F9 is
            // the only way in. Unbinding it in the terminal's settings gives
            // this back.
            if key.code == KeyCode::Char('v')
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && self.image_gesture_into_pane()
            {
                return;
            }
            // Through the torn-report filter first: an Esc may be the front of
            // a mouse report split across two reads. See [`super::torn`].
            for key in self.torn.feed(key, Instant::now()) {
                self.key_into_pane(key);
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
                self.rmux_install_answer(key.code == KeyCode::Char('y'));
            }
            Mode::Serve => self.on_key_serve(key),
            Mode::QuitConfirm => self.on_key_quit(key),
            Mode::BatchConfirm | Mode::BatchDeleteBlocked | Mode::BatchKillBlocked => {
                self.on_key_batch(key)
            }
            Mode::CostFilter => self.on_key_cost(key),
            Mode::SendKeys => self.on_key_send(key),
            Mode::RenameTab => self.on_key_rename(key),
            Mode::SwitchTab => self.on_key_switch(key),
            Mode::AddAccount => self.on_key_add_account(key),
            Mode::Launch => self.on_key_launch(key),
            Mode::RowMenu => self.on_key_menu(key),
            Mode::LaunchCwd => self.on_key_launch_cwd(key),
            Mode::Hooks => self.on_key_hooks(key),
            Mode::Insight => self.on_key_insight(key),
            Mode::Conversation => self.on_key_conversation(key),
            Mode::Help => self.on_key_help(key),
            Mode::Settings => self.on_key_settings(key),
            Mode::DeleteBlocked | Mode::KillBlocked => self.mode = Mode::List,
            // Any key, like the other panels that only have something to say;
            // and the link let go of with it, there being nothing left to draw.
            Mode::ShareQr => {
                self.mode = Mode::List;
                self.share_qr = None;
            }
            Mode::List => {
                self.reload_settings();
                if let Some(key) = self.keymap.apply(key) {
                    self.on_key_list(key);
                }
            }
        }
    }

    /// One key for the focused pane's agent, once it is known not to be part of
    /// a torn mouse report.
    fn key_into_pane(&mut self, key: KeyEvent) {
        let Some(pane) = self.focused_pane() else {
            return;
        };
        // Paging through history, which the wheel could already do and the
        // keyboard could not. Both ask the screen first, so an agent in
        // fullscreen still gets these keys for its own scrolling.
        let scrolled = match pane.rmux.as_deref() {
            Some(name) => key.modifiers.is_empty() && crate::rmux::scroll_key(name, key.code),
            None => pane.view.scroll_key(key),
        };
        if scrolled {
            return;
        }
        // The agent this key is going to, taken before the borrow ends:
        // answering its question is the one thing no hook reports.
        let agent = pane.agent();
        let alive = pane.view.send_key(pane.translate_key(key));
        self.mark_answered(agent);
        if !alive {
            self.close_pane();
            self.set_status("The agent's terminal closed");
        }
    }

    /// Deliver the keys the torn-report filter held, once it has waited long
    /// enough to know they were typed.
    pub(super) fn tick_torn(&mut self) {
        for key in self.torn.expire(Instant::now()) {
            // Only where they were headed: a pane still on screen. A key held
            // across a switch to the dashboard is dropped rather than read there
            // as a command nobody meant for it.
            if self.tab > 0 && self.mode == Mode::List {
                self.key_into_pane(key);
            }
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

        // An image that arrived as text — one of the ways one reaches a cctop
        // running over ssh, where the clipboard is on the machine the ssh was
        // typed on and no helper on this side can see it. What is pasted is a
        // file here, and what the agent is given is its path — the same as F9,
        // by a different road.
        if let Some(image) = crate::clipboard::image_from_paste(text) {
            match crate::clipboard::write_image(&image) {
                Ok(path) => {
                    let shown = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.set_status(format!("Pasted {shown}"));
                    self.show_paste_preview(&path);
                    self.paste_text(&format!("{} ", path.display()));
                }
                // The paste is not put through as text on a failure: 100KB of
                // base64 in front of an agent is worse than nothing having
                // happened, and the status line says what did.
                Err(e) => self.set_status(format!("Could not save the pasted image: {e}")),
            }
            return;
        }
        self.paste_text(text);
    }

    /// A paste, once it is known to be text.
    fn paste_text(&mut self, text: &str) {
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
            // To `setup-token` once it is running — it may ask for the code
            // the browser showed — and to the name field before.
            Mode::AddAccount => match self.add_account.pane.as_mut() {
                Some(pane) => {
                    pane.view.send_paste(text);
                }
                None if self.add_account.outcome.is_none() && !self.add_account.named => {
                    let room = TAB_NAME_MAX.saturating_sub(self.add_account.name.chars().count());
                    self.add_account.name.push_str(&flatten(text, room));
                }
                None => {}
            },
            Mode::Search => {
                self.search.push_str(&flatten(text, usize::MAX));
                self.search_edited();
            }
            Mode::SendKeys => {
                let room = 500usize.saturating_sub(self.send_input.len());
                self.send_input.push_str(&flatten(text, room));
            }
            Mode::RenameTab => {
                // The clipboard a right-click brought along with it, not a
                // paste anyone asked for. See `rename_opened_by_click`.
                if let Some(at) = self.rename_opened_by_click.take()
                    && at.elapsed() < RIGHT_CLICK_PASTE
                {
                    return;
                }
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
            Mode::SwitchTab => {
                let room = SWITCH_FILTER_MAX.saturating_sub(self.switch_filter.chars().count());
                self.switch_filter.push_str(&flatten(text, room));
                self.switch_cursor = 0;
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
            Mode::Settings => {
                if let Some(input) = &mut self.settings_input {
                    let room = 200usize.saturating_sub(input.len());
                    input.push_str(&flatten(text, room));
                }
            }
            _ => {}
        }
    }

    /// The clipboard's image written to a file, and its path — the form an
    /// agent can actually read one in. Says why when there is nothing to paste.
    ///
    /// Every failure is reported in the status line rather than swallowed: a
    /// key that silently does nothing is indistinguishable from one that is not
    /// bound, and the two answers this can give — an empty clipboard, and a
    /// machine with no helper installed — are things the user can act on.
    fn image_paste(&mut self) -> Option<String> {
        self.needs_redraw = true;
        match crate::clipboard::image_to_file(true) {
            Ok(path) => {
                let shown = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.set_status(format!("Pasted {shown}"));
                self.show_paste_preview(&path);
                Some(path.display().to_string())
            }
            Err(why) => {
                self.set_status(why.message());
                None
            }
        }
    }

    /// F9 on the dashboard, where there is no composer to type into.
    ///
    /// The image is read first and the destination decided after, which is the
    /// whole of what was wrong here to begin with: asking `send_prompt` first
    /// meant a row with no local process refused the key with "no local process
    /// to type into" — a true sentence about a question nobody asked, and no
    /// clue that it was the image key that had just been pressed.
    ///
    /// With a session that can be typed into, the path goes into the box that
    /// types for it, the field left open for the sentence that follows. With
    /// none — a finished session, a subagent, a row on another machine — the
    /// file has still been written and the status line names it in full, which
    /// is the most that can be done with an image nobody is waiting for.
    ///
    /// Not copied to the clipboard, tempting as it is: the clipboard is where
    /// the image just came from, and putting a path there would delete the
    /// screenshot the next F9 was going to read. A key that destroys its own
    /// input on the way past is worse than one that only reports.
    fn paste_image_from_the_list(&mut self) {
        let Some(path) = self.image_paste() else {
            return;
        };
        self.send_prompt();
        if self.mode == Mode::SendKeys {
            self.send_input = format!("{path} ");
            return;
        }
        // `send_prompt` will have said why it could not open, which is no
        // longer the news.
        self.set_status(format!("Saved {path}"));
    }

    /// The image paste as a gesture rather than as a key: pastes and returns
    /// true when the clipboard holds an image, and says nothing at all when it
    /// does not.
    ///
    /// Silence is the point. `F9` is pressed to paste an image and so reports
    /// when there is none; `Ctrl+V` is pressed to paste *whatever* is there,
    /// and a status line saying "no image on the clipboard" every time someone
    /// pastes a line of text would be cctop talking over the thing they were
    /// doing.
    ///
    /// ponytail: the right button is not one of these, and was for a day. In
    /// Windows Terminal it *copies* when there is a selection and pastes only
    /// when there is not — so a user selecting output and right-clicking to
    /// copy it got an image pasted into their agent instead. A gesture whose
    /// meaning depends on a selection cctop cannot see is not one it can take.
    fn image_gesture_into_pane(&mut self) -> bool {
        // No terminal ask on this path: Ctrl+V is pressed for whatever is on
        // the clipboard, usually text, and a clipboard-read escape plus a wait
        // on every paste would stall the common case for the rare one.
        let Ok(path) = crate::clipboard::image_to_file(false) else {
            return false;
        };
        self.needs_redraw = true;
        let shown = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.send_to_focused_pane(&format!("{} ", path.display()));
        self.set_status(format!("Pasted {shown}"));
        self.show_paste_preview(&path);
        true
    }

    /// Text typed at the focused pane's agent, with the bookkeeping a paste
    /// carries: what is put in front of an agent answers whatever it asked.
    fn send_to_focused_pane(&mut self, text: &str) {
        let Some(pane) = self.focused_pane() else {
            return;
        };
        let agent = pane.agent();
        let alive = pane.view.send_paste(text);
        self.mark_answered(agent);
        if !alive {
            self.close_pane();
            self.set_status("The agent's terminal closed");
        }
    }

    /// F9 inside a pane: the image, then a space, typed at the agent.
    ///
    /// A space after it because the path is the start of a sentence, not the
    /// whole of one — "what is wrong with this?" follows, and every harness
    /// needs the path separated from it.
    fn paste_image_into_pane(&mut self) {
        let Some(path) = self.image_paste() else {
            return;
        };
        self.send_to_focused_pane(&format!("{path} "));
    }

    /// Hold the image that was just filed up in the corner for a few seconds.
    ///
    /// Decode failure is silent on purpose: the path is already on its way to
    /// the agent and the status line has already named the file, so a preview
    /// that cannot decode is a nicety lost, not part of the paste.
    fn show_paste_preview(&mut self, path: &std::path::Path) {
        let Ok(image) = image::open(path) else {
            return;
        };
        self.paste_preview = Some(super::PastePreview {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            at: Instant::now(),
            image: ratatui_image::picker::Picker::halfblocks().new_resize_protocol(image),
        });
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
            // The tab half of `b` on the dashboard: both go to what rang.
            KeyCode::Char('b') => self.next_waiting_tab(),
            // The keyboard's way into what a right-click opens — the modal
            // renames and paints, and the bar is not the only way to reach it.
            KeyCode::Char('r') => match self.tab {
                0 => self.set_status("The dashboard is not a tab to rename"),
                tab => self.rename_prompt(tab),
            },
            KeyCode::Char('t') => self.switch_prompt(),
            KeyCode::Char('v') => self.launch_prompt(LaunchInto::Split { stacked: false }),
            KeyCode::Char('s') => self.launch_prompt(LaunchInto::Split { stacked: true }),
            KeyCode::Char('o') => match self.active_tab() {
                Some(tab) => tab.cycle_focus(),
                None => return false,
            },
            KeyCode::Char('w') => self.close_pane(),
            // Shifted for the same reason as `W`, and because `r` renames: the
            // agent is ended and resumed, on whatever version is now installed.
            // On the dashboard it restarts the selected row's tab, and only from
            // the table itself: over a modal the selection is not what is on
            // screen, and an agent must not be ended by a key aimed at a dialog.
            KeyCode::Char('R') if self.tab == 0 && self.mode != Mode::List => {}
            KeyCode::Char('R') => self.restart_pane(),
            // Shifted, because it is the irreversible one: `w` on a rmux-backed
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
                self.pending_fork = None;
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
    /// Scroll the report, or close it. Nothing here can change a session: both
    /// reports are read-only and the overlay is too.
    fn on_key_insight(&mut self, key: KeyEvent) {
        // Lines that fit on a normal terminal, used as the page step. The
        // overlay does not know the frame height here, so a page is a sensible
        // fixed jump rather than a wrong computed one.
        const PAGE: u16 = 20;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('b') => {
                self.mode = Mode::List;
                self.insight = None;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.insight_scroll = self.insight_scroll.saturating_add(1)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.insight_scroll = self.insight_scroll.saturating_sub(1)
            }
            KeyCode::PageDown | KeyCode::Char(' ') => {
                self.insight_scroll = self.insight_scroll.saturating_add(PAGE)
            }
            KeyCode::PageUp => self.insight_scroll = self.insight_scroll.saturating_sub(PAGE),
            KeyCode::Home => self.insight_scroll = 0,
            // The other report, without going back to the table first: the two
            // answer halves of the same question.
            KeyCode::Char('o') => self.open_insight("optimize"),
            KeyCode::Char('c') => self.open_insight("compare"),
            _ => {}
        }
    }

    /// Scroll the conversation, page further back into it, or close it.
    ///
    /// Like the insight overlay there is nothing here that can touch the
    /// session: it is a transcript being read, not a terminal being driven.
    /// `back` is a distance from the end rather than a position from the top,
    /// so a turn landing mid-read does not shift the text under the cursor.
    fn on_key_conversation(&mut self, key: KeyEvent) {
        const PAGE: u16 = 20;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = Mode::List;
                self.chat = None;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(view) = &mut self.chat {
                    view.back = view.back.saturating_sub(1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(view) = &mut self.chat {
                    view.back = (view.back + 1).min(view.max_back);
                }
            }
            KeyCode::PageDown | KeyCode::Char(' ') => {
                if let Some(view) = &mut self.chat {
                    view.back = view.back.saturating_sub(PAGE);
                }
            }
            KeyCode::PageUp => {
                if let Some(view) = &mut self.chat {
                    view.back = (view.back + PAGE).min(view.max_back);
                }
            }
            // Home is the transcript's start, End the live edge it opened on.
            KeyCode::Home => {
                if let Some(view) = &mut self.chat {
                    view.back = view.max_back;
                }
            }
            KeyCode::End => {
                if let Some(view) = &mut self.chat {
                    view.back = 0;
                }
            }
            // A turn at a time: the reply you opened on is usually one `[`
            // away, however much tool output sits under it.
            KeyCode::Char('[') => {
                if let Some(view) = &mut self.chat {
                    let older = view.turn_backs.iter().copied().filter(|&b| b > view.back);
                    if let Some(back) = older.min() {
                        view.back = back;
                    }
                }
            }
            KeyCode::Char(']') => {
                if let Some(view) = &mut self.chat {
                    let newer = view.turn_backs.iter().copied().filter(|&b| b < view.back);
                    view.back = newer.max().unwrap_or(0);
                }
            }
            // The source, for when what matters is the exact characters.
            KeyCode::Char('m') => {
                if let Some(view) = &mut self.chat {
                    view.raw = !view.raw;
                }
            }
            // `u` for "earlier": the window grows at the top, which a
            // bottom-anchored scroll survives without moving a line.
            KeyCode::Char('u') => {
                let wants = self.chat.as_ref().is_some_and(|v| {
                    !v.fetching && v.conversation.as_ref().is_some_and(|c| c.earlier > 0)
                });
                if wants {
                    let before = self
                        .chat
                        .as_ref()
                        .and_then(|v| v.conversation.as_ref())
                        .and_then(|c| c.turns.first().map(|t| t.seq));
                    if let Some(seq) = before {
                        self.fetch_chat(Some(seq));
                    }
                }
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

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

    /// The browser panel's keys.
    ///
    /// `l` and `t` start rather than toggle, and both are offered while a
    /// server is up: switching between them is a stop and a start, which is
    /// what changing whether the page is on the internet actually is.
    fn on_key_serve(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.mode = Mode::List,
            KeyCode::Char('l') => self.start_serving(false),
            KeyCode::Char('t') => self.start_serving(true),
            KeyCode::Char('x') => self.stop_serving(),
            // Only a tunnel's link is worth a code: the loopback one is the
            // link a phone cannot open.
            KeyCode::Char('c') => match self.serving.as_ref().is_some_and(|s| s.public.is_some()) {
                true => self.serve_qr = !self.serve_qr,
                false => self.set_status("A QR code is for the tunnel's link — t opens one"),
            },
            KeyCode::Char('o') => match self.serving.as_ref().map(|s| s.best().to_string()) {
                Some(link) => match crate::serve::open_in_browser(&link) {
                    true => self.set_status("Opening the page in your browser"),
                    false => self.set_status("No browser to open it with — y copies the link"),
                },
                None => self.set_status("Nothing is being served yet"),
            },
            KeyCode::Char('y') => match self.serving.as_ref().map(|s| s.best().to_string()) {
                Some(link) => {
                    crate::ui::render::copy_to_clipboard(&link);
                    // The token is in the link, so this is a credential leaving
                    // the process. Said plainly rather than a silent "copied".
                    self.set_status("Link copied — it carries the token that opens it");
                }
                None => self.set_status("Nothing is being served yet"),
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

    /// The settings panel: a cursor over every setting and keybind, changed
    /// in place, with `e` for the whole file in an editor.
    fn on_key_settings(&mut self, key: KeyEvent) {
        // Waiting for a key takes every key, including the ones that would
        // otherwise move or close the panel — they are what may be bound.
        if self.settings_capture {
            return self.settings_capture_key(key);
        }
        if let Some(input) = &mut self.settings_input {
            match key.code {
                KeyCode::Esc => self.settings_input = None,
                KeyCode::Enter => self.settings_commit_input(),
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) if input.len() < 200 => input.push(c),
                _ => {}
            }
            return;
        }
        let rows = crate::settings::SETTINGS.len() + crate::settings::BINDINGS.len();
        let step = |app: &mut App, delta: i32| {
            app.settings_cursor =
                (app.settings_cursor as i32 + delta).clamp(0, rows as i32 - 1) as usize;
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => step(self, -1),
            KeyCode::Down | KeyCode::Char('j') => step(self, 1),
            KeyCode::PageUp => step(self, -(PAGE as i32)),
            KeyCode::PageDown => step(self, PAGE as i32),
            KeyCode::Home | KeyCode::Char('g') => self.settings_cursor = 0,
            KeyCode::End | KeyCode::Char('G') => self.settings_cursor = rows - 1,
            KeyCode::Enter | KeyCode::Char(' ') => self.settings_activate(),
            KeyCode::Backspace | KeyCode::Delete => self.settings_reset(),
            KeyCode::Char('e') => self.edit_settings(),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char(',') => self.mode = Mode::List,
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
            && let Some(pid) = self.selected_session().and_then(|s| s.root_pid())
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
            // The clipboard's image, as a path the agent can open. Stays in
            // the pane: the image is for the agent being typed at, and pulling
            // the dashboard forward would take the composer off screen at the
            // moment something is being put into it.
            KeyCode::F(9) => self.paste_image_into_pane(),
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
                    && let Some(pid) = self.selected_session().and_then(|s| s.root_pid())
                {
                    let _ = self.tx.send(Request::SendKeys { pid, text });
                    self.set_status("Sending…");
                }
                self.mode = Mode::List;
            }
            KeyCode::Backspace => {
                self.send_input.pop();
            }
            KeyCode::F(9) => {
                if let Some(path) = self.image_paste() {
                    let room = 500usize.saturating_sub(self.send_input.len());
                    if path.len() < room {
                        self.send_input.push_str(&path);
                        self.send_input.push(' ');
                    }
                }
            }
            KeyCode::Char(c) if self.send_input.len() < 500 => self.send_input.push(c),
            _ => {}
        }
    }

    /// The tab-rename field, which is also the tab-colour field.
    ///
    /// The name half has no cursor — text only ever appends — so the arrows
    /// were free for the colour row, which is what they drive. Enter applies
    /// whichever half changed; an empty name is still not a name, so pressing
    /// it with nothing typed only ever moved the colour, never blanks the tab.
    fn on_key_rename(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Left => self.step_rename_color(-1),
            KeyCode::Right => self.step_rename_color(1),
            KeyCode::Enter => {
                let name = self.rename_input.trim().to_string();
                let color = self.rename_color;
                self.mode = Mode::List;
                // The bar may have moved while the field was open — see
                // [`App::rename_was`]. A tab that is no longer the one the
                // right-click landed on keeps its name and its colour alike,
                // and the answer says so when anything was being asked of it.
                let Some(tab) = self
                    .tabs
                    .get_mut(self.rename_tab.saturating_sub(1))
                    .filter(|tab| tab.title() == self.rename_was)
                else {
                    if !name.is_empty() || color.is_some() {
                        self.set_status("That tab is gone; nothing was changed");
                    }
                    return;
                };
                let repainted = tab.color != color;
                if repainted {
                    tab.recolor(color);
                }
                if !name.is_empty() {
                    tab.rename(name.clone());
                }
                // Say what changed in the terms it was changed in: a name, a
                // colour, or both. Nothing at all is a cancel, not a clear.
                match (name.is_empty(), repainted, color) {
                    (true, false, _) => {}
                    (true, true, Some(hue)) => {
                        self.set_status(format!("Tab coloured {}", hue.name()))
                    }
                    (true, true, None) => self.set_status("Tab colour cleared".to_string()),
                    (false, false, _) => self.set_status(format!("Tab renamed to {name}")),
                    (false, true, Some(hue)) => {
                        self.set_status(format!("Tab renamed to {name}, coloured {}", hue.name()))
                    }
                    (false, true, None) => {
                        self.set_status(format!("Tab renamed to {name}, colour cleared"))
                    }
                }
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

    /// Move the colour pick in the rename modal one stop along.
    ///
    /// The stops are "no colour" and then [`theme::Hue::ALL`] in the order it
    /// lists them, wrapping at both ends: a row of swatches has no end worth
    /// stopping at, and holding Right down is how you look at them all.
    fn step_rename_color(&mut self, delta: isize) {
        let stops = theme::Hue::ALL.len() + 1;
        let at = self
            .rename_color
            .and_then(|hue| theme::Hue::ALL.iter().position(|h| *h == hue))
            .map(|i| i + 1)
            .unwrap_or(0);
        let next = (at as isize + delta).rem_euclid(stops as isize) as usize;
        self.rename_color = (next > 0).then(|| theme::Hue::ALL[next - 1]);
    }

    /// The add-account popup: the name, then which kind of account, then the
    /// terminal that makes it, then what came of it.
    fn on_key_add_account(&mut self, key: KeyEvent) {
        let flow = &mut self.add_account;
        if flow.outcome.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                *flow = super::AddAccount::default();
                self.mode = Mode::List;
            }
            return;
        }
        if let Some(pane) = flow.pane.as_mut() {
            match key.code {
                // Esc is cctop's here, not `setup-token`'s: it is the only way
                // out of a popup whose every other key goes to the terminal.
                KeyCode::Esc => {
                    *flow = super::AddAccount::default();
                    self.mode = Mode::List;
                }
                KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(link) = flow.link.clone() {
                        render::copy_to_clipboard(&link);
                        // Over ssh a browser opened here is on the wrong
                        // machine, so the clipboard is the whole answer.
                        let opened = !render::over_ssh() && crate::serve::open_in_browser(&link);
                        self.set_status(match opened {
                            true => "Copied the sign-in link, and asked the browser to open it",
                            false => "Copied the sign-in link — paste it into your browser",
                        });
                    }
                }
                _ => {
                    pane.view.send_key(key);
                }
            }
            return;
        }
        // The name is in, and the popup is asking which kind of account.
        if flow.named {
            match key.code {
                KeyCode::Char('f') => self.start_add_account(super::AccountKind::Login),
                KeyCode::Char('t') => self.start_add_account(super::AccountKind::Token),
                // Back to the name rather than out: a typo in it is the likely
                // reason for stopping here.
                KeyCode::Backspace => flow.named = false,
                KeyCode::Esc => {
                    *flow = super::AddAccount::default();
                    self.mode = Mode::List;
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Enter => self.accept_account_name(),
            KeyCode::Backspace => {
                flow.name.pop();
            }
            KeyCode::Char(c) if flow.name.chars().count() < TAB_NAME_MAX => flow.name.push(c),
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
        // The pick starts where the tab already is: Enter that only meant to
        // fix a typo must not strip a colour it never touched.
        self.rename_color = target.color;
        self.rename_input.clear();
        self.rename_opened_by_click = None;
        self.mode = Mode::RenameTab;
        self.needs_redraw = true;
    }

    /// The tab switcher: letters narrow the list, arrows move in it, Enter
    /// goes to the pick. Letters type into the filter rather than stepping
    /// the cursor — unlike every other list here, the thing being spelled
    /// is the thing being searched, so `j` and `k` belong to the name.
    fn on_key_switch(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Up => self.step_switch(-1),
            KeyCode::Down => self.step_switch(1),
            KeyCode::Enter => {
                self.mode = Mode::List;
                if let Some(&tab) = self.switch_matches().get(self.switch_cursor) {
                    self.go_to_tab(tab);
                }
            }
            KeyCode::Backspace => {
                self.switch_filter.pop();
                // Back to the top: the list just widened, and a cursor kept
                // at its old depth is pointing at a name nobody picked.
                self.switch_cursor = 0;
            }
            KeyCode::Char(c) if self.switch_filter.chars().count() < SWITCH_FILTER_MAX => {
                self.switch_filter.push(c);
                self.switch_cursor = 0;
            }
            _ => {}
        }
    }

    /// Move the switcher's cursor within the narrowed list, wrapping.
    fn step_switch(&mut self, delta: isize) {
        let n = self.switch_matches().len();
        if n == 0 {
            self.switch_cursor = 0;
            return;
        }
        self.switch_cursor = (self.switch_cursor as isize + delta).rem_euclid(n as isize) as usize;
    }

    /// Open the switcher on the tab being watched: Down walks the bar from
    /// where you are, and Enter on an unchanged pick is `go_to_tab` seeing
    /// the tab it is already on.
    fn switch_prompt(&mut self) {
        self.switch_filter.clear();
        self.switch_cursor = self
            .switch_matches()
            .iter()
            .position(|&i| i == self.tab)
            .unwrap_or(0);
        self.mode = Mode::SwitchTab;
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
            Action::Restart => self.restart_selected(),
            Action::Attach => self.attach_selected(),
            Action::Send => self.send_prompt(),
            Action::Handoff => self.handoff_selected(),
            Action::Read => self.open_conversation(),
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
            Some(s) if s.root_pid().is_some() => {
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
        // arrows are taken by list navigation and panel switching. Home and End
        // join it under the same modifier for the same reason — unshifted they
        // jump the session list — and the ends are asked for as a distance no
        // panel can be longer than, since only the renderer knows the real one.
        if shift {
            match key.code {
                KeyCode::Up => return self.scroll_active_panel(-1),
                KeyCode::Down => return self.scroll_active_panel(1),
                KeyCode::Home => return self.scroll_active_panel(i32::MIN),
                KeyCode::End => return self.scroll_active_panel(i32::MAX),
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::F(10) => self.request_quit(),
            KeyCode::Char('+') => self.open_add_account(),
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
            // Home and End sit beside PgUp and PgDn on the keyboard and mean
            // the same thing one step further, so they land here rather than
            // only under Shift — which is where they were, next to a comment
            // claiming the unshifted pair already did this.
            KeyCode::Home | KeyCode::Char('g') => {
                self.selected = 0;
                self.ensure_available_tab();
                self.needs_redraw = true;
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.selected = self.visible.len().saturating_sub(1);
                self.ensure_available_tab();
                self.needs_redraw = true;
            }
            KeyCode::Char('u') if ctrl => {
                self.move_selection(-(self.half_page() as isize));
            }
            KeyCode::Char('d') if ctrl => {
                self.move_selection(self.half_page() as isize);
            }
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
            // `W` next to it, since both are about an agent reaching you rather
            // than you reaching it.
            KeyCode::Char('W') => self.share_selected(),
            // The two reports, on the letters they are named for. Both are
            // read-only and both open over the table rather than replacing it,
            // because the question they answer is about the rows underneath.
            KeyCode::Char('o') => self.open_insight("optimize"),
            KeyCode::Char('c') => self.open_insight("compare"),
            // `B` for browser, beside the two keys that are also about reaching
            // this machine from somewhere else. `W` puts one agent's terminal
            // in a browser; this puts the whole table in one.
            KeyCode::Char('B') => {
                self.mode = Mode::Serve;
                self.serve_qr = false;
            }
            KeyCode::Char('#') => {
                self.cost_input = if self.cost_floor > 0.0 {
                    format!("{:.2}", self.cost_floor)
                } else {
                    String::new()
                };
                self.mode = Mode::CostFilter;
            }
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
            KeyCode::Char(',') => self.mode = Mode::Settings,
            // One way in, rather than the six single-letter sort keys this
            // replaced. `P`/`M`/`T` were htop's, and `H`/`X`/`S` were three
            // more that only cctop has columns for: six keys spent on an
            // ordering you set once, none of them guessable without the help.
            // The panel names every column, says which is current, and `S` is
            // the letter anyone tries first.
            KeyCode::Char('S') | KeyCode::Char('>') | KeyCode::Char('<') | KeyCode::F(6) => {
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
            // Ctrl, because it stops agents — the same reasoning as Ctrl+K —
            // and `r`, because Alt+Shift+R is the one-tab version and `R`
            // already resumes. Above the refresh arm, which would take it.
            KeyCode::Char('r') if ctrl => self.restart_all(),
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
            KeyCode::F(9) => self.paste_image_from_the_list(),
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
            // `i` for "inspect": the conversation the report page shows, over
            // the table. Deliberately not in the remote-refusal list — this is
            // the one reader that works on a remote row, over the ssh channel
            // the row arrived by.
            KeyCode::Char('i') => self.open_conversation(),
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
            // The end of a drag, and the moment its arrangement is worth
            // writing down: `move_tab` has been called on every pointer
            // movement between the press and here.
            MouseEventKind::Up(MouseButton::Left) => match self.drag_tab.take() {
                Some(_) => {
                    self.save_tab_order();
                    true
                }
                None => false,
            },
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
                        self.rename_opened_by_click = Some(Instant::now());
                        true
                    }
                    None => false,
                }
            }
            _ => false,
        }
    }

    /// A click on the footer's corner: open the page, or open a tunnel.
    ///
    /// The link opens the browser, as `o` does in the serve panel. The button
    /// takes two clicks, and `armed` says which this is — the first only lights
    /// the corner amber and changes what it says, because publishing every
    /// session on this machine to the internet is not something a slipped
    /// pointer should be able to do. The second is the one that registers with
    /// Cloudflare's edge, which is a second of network cctop spends in front of
    /// the person who asked for it.
    pub(super) fn on_share_corner(&mut self, armed: bool) {
        // Already dialling. The corner is spinning and says so; a click at it
        // is impatience, not a second instruction.
        if self.share_opening.is_some() {
            return;
        }
        if let Some(link) = self.serving.as_ref().and_then(|s| s.public.clone()) {
            match crate::serve::open_in_browser(&link) {
                true => self.set_status("Opening the shared page in your browser"),
                false => self.set_status("No browser to open it with — B then y copies the link"),
            }
            return;
        }
        if !armed {
            self.share_arm = true;
            self.set_status("Click again to put this table on the internet");
            return;
        }
        self.start_serving(true);
    }

    /// A click on a key written on screen — a footer hint, or the `[y]` in a
    /// confirmation — answered by pressing that key.
    ///
    /// Going through `on_key` rather than calling the action is what keeps the
    /// two in step: the hint says `R Resume`, and clicking it does whatever `R`
    /// does in the state the app is actually in, including opening the
    /// confirmation that `R` opens.
    ///
    /// `q` is the exception, and takes two clicks. It is the one key on the
    /// footer whose action cannot be undone — there is no confirmation behind
    /// it unless cctop is hosting an agent — so a slipped pointer must not end
    /// the session, in the same way one must not put a tunnel on the internet.
    fn on_hint_click(&mut self, key: KeyEvent, quit_armed: bool) {
        if key.code == KeyCode::Char('q') && key.modifiers.is_empty() && !quit_armed {
            self.quit_arm = true;
            // Said in the footer's badges rather than with `set_status`, which
            // draws over the hints — including the `q Quit` the second click has
            // to land on. Arming that hid its own button was a button that could
            // only ever be clicked once.
            self.needs_redraw = true;
            return;
        }
        self.on_key(key);
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
            // A `[y]` in a confirmation, answered by pressing what it says.
            // Single-click, unlike the launcher's two: the dialog is itself the
            // second step — a click got here by asking for something that
            // stopped to ask, and asking twice about the same pointer teaches
            // nothing.
            if let Some(key) = layout.key_at(ev.column, ev.row) {
                self.on_key(key);
                return;
            }
            // A click beside the popup must not cancel a sign-in half done in
            // the browser; only its own Esc does.
            if self.mode == Mode::AddAccount {
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

        // The footer's corner: the tunnel's link while there is one, and the
        // button that opens one while there is not. cctop holds the terminal's
        // mouse capture, so in most terminals a plain click on an OSC 8 link
        // never reaches the terminal that would follow it — answering it here is
        // what makes the link clickable without a modifier held down. Drawn over
        // a tab as much as over the dashboard, so answered before either.
        if ev.kind == MouseEventKind::Down(MouseButton::Left) {
            let on_corner = layout.share_corner_at(ev.column, ev.row);
            // Arming is about the click being made now. A click anywhere else
            // takes it back, which is what stops a forgotten first click from
            // turning an unrelated one into a tunnel.
            let armed = std::mem::replace(&mut self.share_arm, false);
            // Same rule as the corner's: arming is about the click being made
            // now, so any click that is not the second one on `q Quit` takes it
            // back. A first click that quits on the next unrelated one would be
            // worse than no button at all.
            let quit_armed = std::mem::replace(&mut self.quit_arm, false);
            if let Some(key) = layout.key_at(ev.column, ev.row) {
                self.on_hint_click(key, quit_armed);
                return;
            }
            if on_corner {
                self.on_share_corner(armed);
                return;
            }
        }

        // Inside a tab the rest of the mouse is the agents'.
        if self.tab > 0 {
            // Inside a pane the mouse is the agent's. Claude Code, opencode and
            // pi all ask for mouse reporting and act on it — placing the cursor
            // in the composer, picking a file, choosing from the agents list —
            // and cctop holds the terminal's capture, so without forwarding the
            // click simply went nowhere.
            //
            // The right button is the one rmux can intercept: its
            // `MouseDown3Pane` binding reads `#{mouse_any_flag}` — the pane's
            // own request for mouse reporting — and forwards the click to the
            // agent when it is set, but opens rmux's pane menu, splits and
            // kills included, when it is not. So the flag is asked before the
            // click is written: an agent that asked gets its right-click, and
            // a pane that did not keeps the menu out of reach.
            let button = |b| match b {
                MouseButton::Left => Some(crate::attach::MouseButton::Left),
                MouseButton::Middle => Some(crate::attach::MouseButton::Middle),
                MouseButton::Right => Some(crate::attach::MouseButton::Right),
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
                        // Right-click is the agent's only where rmux will hand
                        // it over: without the flag the same click raises the
                        // pane menu this guard exists to keep away. A pane with
                        // no multiplexer behind it has no menu to raise —
                        // `encode_mouse` already gates on the agent's own mode.
                        let wanted = b != crate::attach::MouseButton::Right
                            || match &pane.rmux {
                                Some(name) => crate::rmux::mouse_wanted(name),
                                None => true,
                            };
                        // A failed send means that agent has gone, which the
                        // reaper already watches for.
                        if wanted {
                            let _ = pane.view.mouse(kind, b, col, row);
                        }
                    }
                }
                return;
            }
            // The wheel is the agent's, wherever it is pointed — cctop keeps no
            // scrollback of its own, so a pane's history lives in the agent (or
            // in the rmux around it) and only the agent can scroll it. The pane
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
            // Right-click a row for its menu, as the tab bar's right button
            // renames a tab: the actions are already a click away once the menu
            // is up, and reaching for Enter to open it was the one step in that
            // path a mouse could not take.
            MouseEventKind::Down(MouseButton::Right) => {
                if let Some(row) = layout.row_at(ev.row) {
                    let idx = self.scroll + row;
                    if idx < self.visible.len() {
                        self.selected = idx;
                        self.ensure_available_tab();
                        self.open_row_menu();
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::{Plan, Provider};
    use crate::ui::tests::{key, session, test_app};
    use crate::ui::{Row, menu, panels};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::sync::mpsc::channel;
    /// A paste on the dashboard is typing into whichever one-line box is open,
    /// and the line breaks in it must not go in: none of these inputs can show a
    /// second row or let you delete back onto one.
    #[test]
    fn a_paste_types_into_the_open_input_as_one_line() {
        let mut app = test_app();

        app.mode = Mode::Search;
        app.on_paste("fix the\nlogin bug\r\n");
        assert_eq!(app.search, "fix the login bug ");

        app.mode = Mode::SendKeys;
        app.send_input = "continue".into();
        app.on_paste(" and\ttidy\x07 up");
        assert_eq!(app.send_input, "continue and tidy up");

        // The cost floor is a number, so a paste is filtered the way typing one
        // is rather than flattened.
        app.mode = Mode::CostFilter;
        app.on_paste("$12.50 or so");
        assert_eq!(app.cost_input, "12.50");
    }

    /// The caps the typed path enforces are the paste's too, and a paste with
    /// nowhere to land does nothing rather than something surprising.
    #[test]
    fn a_paste_respects_the_caps_and_does_nothing_with_no_input_open() {
        let mut app = test_app();

        app.mode = Mode::SendKeys;
        app.send_input = "x".repeat(495);
        app.on_paste(&"y".repeat(50));
        assert_eq!(app.send_input.len(), 500);

        app.mode = Mode::CostFilter;
        app.on_paste("123456789012345");
        assert_eq!(app.cost_input, "123456789012");

        // A modal is on screen, so there is no box to type into — and a paste
        // must never stand in for the key one of these is waiting for.
        app.mode = Mode::DeleteConfirm;
        app.on_paste("y");
        assert_eq!(app.mode, Mode::DeleteConfirm);
        app.mode = Mode::List;
        app.on_paste("q");
        assert!(!app.should_quit);
    }

    /// Regression: a click on the launcher used to be answered twice — once by
    /// the modal and once by the dashboard drawn under it — so picking an agent
    /// also switched the bottom panel the modal happened to cover.
    #[test]
    fn a_click_on_a_modal_does_not_reach_what_it_covers() {
        use ratatui::layout::Rect;

        let mut app = test_app();
        app.mode = Mode::Launch;
        let layout = render::Layout {
            modal_rect: Some(Rect::new(10, 8, 20, 6)),
            launch_rows: vec![(9, 0), (10, 1)],
            // The panel tabs sit on a row the modal is covering.
            tab_row: 10,
            tab_spans: vec![(10, 20, 3)],
            ..Default::default()
        };
        let click = |col, row| crossterm::event::MouseEvent {
            kind: event::MouseEventKind::Down(event::MouseButton::Left),
            column: col,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        app.on_mouse(click(15, 10), &layout);
        assert_eq!(app.bottom_tab, 0, "the click went through to the panels");
        assert_eq!(app.launch_cursor, 1, "the click did not pick a choice");
        assert_eq!(app.mode, Mode::Launch, "the launcher closed on a pick");

        // Off the modal dismisses it, and still does not reach the panels.
        app.needs_redraw = false;
        app.on_mouse(click(40, 10), &layout);
        assert_eq!(app.mode, Mode::List);
        assert_eq!(app.bottom_tab, 0);
        // Regression: dismissing it asked for no frame, so the modal stayed
        // drawn over a dashboard that was already taking the clicks again.
        assert!(app.needs_redraw, "the dismissal never repainted");
    }

    /// Regression: the guard above was keyed on "any mode but List", which took
    /// the mouse away from the search box too — an overlay a few lines tall over
    /// a table still being scrolled and clicked while the query is typed. Only a
    /// modal that recorded its rectangle can claim the mouse, because that
    /// rectangle is the only way to tell its clicks from the ones underneath.
    /// Home and End sit next to PgUp and PgDn and were the only pair of the
    /// four that did nothing: the list bound `g`/`G` and, under Shift, the
    /// panel scroll, but never the plain keys.
    #[test]
    fn home_and_end_jump_to_the_first_and_last_row() {
        let mut app = test_app();
        for id in ["a", "b", "c"] {
            app.sessions
                .push(crate::session::Session::new(Provider::Claude, id.into()));
        }
        app.visible = vec![Row::Session(0), Row::Session(1), Row::Session(2)];
        app.selected = 1;

        app.on_key(key(KeyCode::End));
        assert_eq!(app.selected, 2);
        app.on_key(key(KeyCode::Home));
        assert_eq!(app.selected, 0);

        // Shift still belongs to the bottom panel, which is why the plain keys
        // were free to mean this.
        app.selected = 1;
        app.on_key(KeyEvent::new(KeyCode::Home, KeyModifiers::SHIFT));
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn the_search_box_leaves_the_table_its_mouse() {
        let mut app = test_app();
        for id in ["a", "b", "c"] {
            app.sessions
                .push(crate::session::Session::new(Provider::Claude, id.into()));
        }
        app.visible = vec![Row::Session(0), Row::Session(1), Row::Session(2)];
        // What `draw_search` leaves behind: no rectangle, so no claim.
        let layout = render::Layout {
            rows_start: 7,
            rows_end: 12,
            // Below the table, or the wheel scrolls a panel instead of the list.
            bottom_start: 14,
            modal_rect: None,
            ..Default::default()
        };
        let at = |kind, row| crossterm::event::MouseEvent {
            kind,
            column: 5,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        app.mode = Mode::Search;
        app.on_mouse(at(event::MouseEventKind::ScrollDown, 9), &layout);
        assert_eq!(app.selected, 1, "the wheel is dead while searching");
        app.on_mouse(
            at(event::MouseEventKind::Down(event::MouseButton::Left), 9),
            &layout,
        );
        assert_eq!(app.selected, 2, "a click cannot reach the row it landed on");
        assert_eq!(app.mode, Mode::Search, "the click closed the search box");
    }

    /// Regression: `launch_prompt` set the mode and nothing asked for a frame, so
    /// clicking the bar's new-tab button — the one advertisement the feature has
    /// — looked like a dead button until an unrelated event repainted.
    #[test]
    fn clicking_the_new_tab_button_paints_the_launcher() {
        let mut app = test_app();
        let layout = render::Layout {
            workspace_new: Some((12, 23)),
            ..Default::default()
        };
        app.needs_redraw = false;
        app.on_mouse(
            crossterm::event::MouseEvent {
                kind: event::MouseEventKind::Down(event::MouseButton::Left),
                column: 15,
                row: 0,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
            &layout,
        );
        // The launcher opens only where there is something to launch, which on a
        // machine with no agent and no $SHELL there is not — but either way the
        // click has to have asked for the frame that says so.
        assert!(app.needs_redraw, "the click asked for no frame");
        assert!(matches!(app.mode, Mode::Launch | Mode::List));
    }

    /// Regression: the launcher sized itself to its list and let `centered` clamp
    /// the result, so on a short terminal the rows past the bottom were dropped —
    /// and once the cursor walked into them, nothing on screen said what Enter
    /// The browser panel shows where the page is, and does not show the token
    /// that opens it.
    ///
    /// A pointer can do everything the footer and the confirmations say.
    ///
    /// Three separate paths, one test, because they are one claim: cctop holds
    /// the terminal's mouse capture, so anything drawn as a button has to be
    /// answered here or the click is simply lost. The regions come out of a real
    /// draw rather than being asserted at, since a hint's column is whatever the
    /// footer's arithmetic made it.
    #[test]
    fn the_mouse_can_press_the_keys_that_are_drawn_on_screen() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (cols, rows) = (140u16, 30u16);
        let draw = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
            let mut layout = render::Layout::default();
            terminal
                .draw(|frame| layout = render::draw(frame, app))
                .expect("draw");
            let screen: String = (0..rows)
                .map(|y| {
                    (0..cols)
                        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            (layout, screen)
        };
        // The column a piece of text starts at, on the row it was drawn on.
        let at = |screen: &str, text: &str| -> (u16, u16) {
            let (row, line) = screen
                .lines()
                .enumerate()
                .find(|(_, l)| l.contains(text))
                .unwrap_or_else(|| panic!("{text:?} was never drawn:\n{screen}"));
            // Char columns, not byte offsets: the row is full of box-drawing
            // characters, each three bytes wide, so a byte index is several
            // columns to the right of where the text actually is.
            let col = line
                .char_indices()
                .position(|(i, _)| line[i..].starts_with(text))
                .expect("found above");
            (col as u16, row as u16)
        };
        let click = |col, row| event::MouseEvent {
            kind: event::MouseEventKind::Down(event::MouseButton::Left),
            column: col,
            row,
            modifiers: event::KeyModifiers::NONE,
        };

        let mut app = test_app();
        app.sessions = vec![session("a", false, "proj")];
        app.visible = vec![Row::Session(0)];
        app.selected = 0;

        // The footer's `?` opens the help it names.
        let (layout, screen) = draw(&mut app);
        let (col, row) = at(&screen, "? Help");
        app.on_mouse(click(col, row), &layout);
        assert_eq!(app.mode, Mode::Help, "clicking `? Help` did nothing");
        app.on_key(key(KeyCode::Esc));

        // `q Quit` takes two, like the share corner: the first says so.
        let (layout, screen) = draw(&mut app);
        let (col, row) = at(&screen, "q Quit");
        app.on_mouse(click(col, row), &layout);
        assert!(!app.should_quit, "one click on `q Quit` quit cctop");
        assert!(app.quit_arm);
        // Said in the footer, beside the hint the second click has to land on —
        // and the hint is still there to land on, which a status message drawn
        // over the whole footer would have taken away. Redrawn every time, so
        // the region a click is tested against is the one now on screen.
        let (layout, screen) = draw(&mut app);
        assert!(
            screen.contains("click q again to quit"),
            "the footer never asked:\n{screen}"
        );
        // Any other click takes the arming back, and the one after it is a
        // first click again rather than the second of a pair.
        app.on_mouse(click(0, layout.rows_start), &layout);
        assert!(!app.quit_arm);
        let (layout, screen) = draw(&mut app);
        let (col, row) = at(&screen, "q Quit");
        app.on_mouse(click(col, row), &layout);
        assert!(!app.should_quit);
        let (layout, screen) = draw(&mut app);
        let (col, row) = at(&screen, "q Quit");
        app.on_mouse(click(col, row), &layout);
        assert!(app.should_quit, "two deliberate clicks did not quit");
        app.should_quit = false;

        // Right-clicking a row opens that row's menu, having selected it.
        let (layout, _) = draw(&mut app);
        app.on_mouse(
            event::MouseEvent {
                kind: event::MouseEventKind::Down(event::MouseButton::Right),
                column: 4,
                row: layout.rows_start,
                modifiers: event::KeyModifiers::NONE,
            },
            &layout,
        );
        assert_eq!(app.mode, Mode::RowMenu, "right-click opened no menu");
        assert_eq!(app.selected, 0);
        app.on_key(key(KeyCode::Esc));

        // A confirmation's `[n / Esc]` cancels it, and its `[y]` is the answer.
        app.mode = Mode::DeleteConfirm;
        let (layout, screen) = draw(&mut app);
        let (col, row) = at(&screen, "[n / Esc]");
        app.on_mouse(click(col, row), &layout);
        assert_eq!(app.mode, Mode::List, "clicking cancel left the dialog up");
        assert!(app.deleting.is_empty(), "cancel deleted the session");

        app.mode = Mode::DeleteConfirm;
        let (layout, screen) = draw(&mut app);
        let (col, row) = at(&screen, "[y]");
        app.on_mouse(click(col, row), &layout);
        assert_eq!(app.mode, Mode::List);
        assert!(!app.deleting.is_empty(), "clicking `[y]` deleted nothing");

        // And a click beside the dialog is a cancel, not a click on the table
        // it is drawn over: the selection used to move under the question.
        app.deleting.clear();
        app.mode = Mode::KillConfirm;
        let (layout, _) = draw(&mut app);
        app.on_mouse(click(1, 1), &layout);
        assert_eq!(app.mode, Mode::List, "a click off the dialog was swallowed");
    }

    #[test]
    fn opening_the_menu_needs_a_row_and_lands_on_something_runnable() {
        let mut app = App::new(Plan::Retail, channel().0);
        // No rows: Enter must not open an empty box.
        app.open_row_menu();
        assert_eq!(app.mode, Mode::List);

        app.sessions = vec![session("a", false, "/repo")];
        app.refilter();
        app.selected = 0;
        app.open_row_menu();
        assert_eq!(app.mode, Mode::RowMenu);
        let items = menu::items(&app);
        assert!(items[app.menu_cursor].enabled());
    }

    /// The row exists because a process does: its `_pid_` id names no
    /// transcript, so Resume has to say that rather than launch `claude
    /// --resume _pid_42` at a conversation that is not there.
    #[test]
    fn a_process_only_row_cannot_be_resumed() {
        let mut app = test_app();
        app.sessions = vec![session("_pid_42", true, "/repo")];
        app.refilter();
        app.selected = 0;

        let items = menu::items(&app);
        let resume = items
            .iter()
            .find(|i| i.action == menu::Action::Resume)
            .expect("the menu always carries a Resume entry");
        assert_eq!(
            resume.blocked.as_deref(),
            Some("no transcript claims this process")
        );

        app.resume_selected();
        let status = app.status().expect("nothing was said").to_owned();
        assert!(status.contains("Nothing to resume"), "{status}");
    }

    /// The whole point of the rebinding: a vim reflex moves the cursor and
    /// cannot reach a live agent.
    #[test]
    fn k_moves_up_and_never_terminates() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "/x"), session("b", true, "/y")];
        app.refilter();
        app.selected = 1;

        app.on_key(key(KeyCode::Char('k')));
        assert_eq!(app.selected, 0, "k must move up like every modal here");
        assert_eq!(app.mode, Mode::List, "k must not open a kill dialog");

        app.on_key(key(KeyCode::Char('j')));
        assert_eq!(app.selected, 1);

        // Terminate still exists, behind a modifier. The fixture's process has
        // no root PID, so it stops at the explanation rather than the confirm —
        // either way, Ctrl+K is what reaches the terminate path at all.
        app.on_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(app.mode, Mode::KillBlocked);
    }

    #[test]
    fn f10_quits_from_an_agent_tab_instead_of_reaching_the_agent() {
        let mut app = test_app();
        app.tab = 1;

        app.on_key(key(KeyCode::F(10)));

        assert!(app.should_quit);
    }

    /// The footer inside a pane advertises the function keys, so none of them
    /// may reach the agent — and the ones that open something have to bring the
    /// dashboard with them, or they land on a screen the agent is repainting.
    #[test]
    fn function_keys_are_cctops_inside_a_pane_and_bring_the_dashboard_with_them() {
        // Each key, and the dashboard state it must leave behind.
        for (code, mode) in [
            (KeyCode::F(1), Mode::Help),
            (KeyCode::F(3), Mode::Search),
            (KeyCode::F(6), Mode::SortBy),
            (KeyCode::F(7), Mode::AgeFilter),
        ] {
            let mut app = test_app();
            app.tab = 1;
            app.on_key(key(code));
            assert_eq!(app.tab, 0, "{code:?} left the dashboard behind");
            assert_eq!(app.mode, mode, "{code:?} did not open its modal");
        }

        // F12 is the pane's own key and F5 acts on the walk, so neither takes
        // you off the agent — F5 says so on the footer instead.
        let mut app = test_app();
        app.tab = 1;
        app.on_key(key(KeyCode::F(5)));
        assert_eq!(app.tab, 1, "refreshing must not leave the agent");
        assert!(app.status().is_some(), "the refresh said nothing");
        app.on_key(key(KeyCode::F(12)));
        assert_eq!(app.tab, 0);

        // An unbound one is swallowed rather than delivered as an escape
        // sequence for the agent to print.
        let mut app = test_app();
        app.tab = 1;
        app.on_key(key(KeyCode::F(2)));
        assert_eq!(app.tab, 1);
        assert_eq!(app.mode, Mode::List);
    }

    /// Regression: F10 in a pane with a launched agent asks before it quits, and
    /// the question was drawn on the dashboard only — so from a pane the key
    /// looked dead while the keyboard was in fact waiting for `y`.
    #[test]
    fn the_quit_question_is_drawn_over_the_pane_that_raised_it() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = test_app();
        app.tab = 1;
        app.hosted = Some((1234, "claude".into()));

        app.on_key(key(KeyCode::F(10)));
        assert!(!app.should_quit, "an owned agent is worth a question");
        assert_eq!(app.mode, Mode::QuitConfirm);

        let (cols, rows) = (80u16, 24u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        terminal
            .draw(|frame| {
                render::draw(frame, &mut app);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let text: String = (0..rows)
            .map(|y| {
                (0..cols)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        assert!(
            text.contains("quit anyway"),
            "the question is not on the pane's screen"
        );

        // And the answer still lands: the modal owns the keyboard, not the pane.
        app.on_key(key(KeyCode::Char('y')));
        assert!(app.should_quit);
    }

    /// Panel keys are bounded by the tab list, not by a literal that drifts.
    #[test]
    fn number_keys_cover_every_panel_and_nothing_more() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "/x")];
        app.refilter();
        for (i, _) in panels::TABS.iter().enumerate() {
            let digit = char::from_digit(i as u32 + 1, 10).unwrap();
            app.on_key(key(KeyCode::Char(digit)));
            assert_eq!(app.bottom_tab, i, "key {digit} must select panel {i}");
        }
        // One past the end changes nothing rather than selecting a phantom tab.
        let past = char::from_digit(panels::TABS.len() as u32 + 1, 10).unwrap();
        let before = app.bottom_tab;
        app.on_key(key(KeyCode::Char(past)));
        assert_eq!(app.bottom_tab, before);
    }
}
