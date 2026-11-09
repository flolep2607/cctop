//! What the scheduled agents did while nobody was looking: the `H` view, and
//! the footer's word that there is something in it.
//!
//! The runs and what came of each are [`cctop_core::schedule_log`]'s; this is
//! where they are listed. The runs since the person last closed the view come
//! first, at full strength; older ones follow, dimmed, for the run someone
//! remembers and wants to find again. Closing the view is the look: it moves
//! the "seen" mark to the newest run it showed, so a run that fires while the
//! view is open is still news the next time.
//!
//! Most of a summary is the table's — state, cost, duration, the stuck
//! detector — and is there the moment the view opens. The two that read files,
//! the files the run edited and what it last said, come from the worker a
//! moment later.

use super::*;
use cctop_core::schedule_log::{self, State, Summary};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// The view's state.
#[derive(Debug, Default)]
pub struct Away {
    /// Runs newer than the last look, as counted at startup — what the footer
    /// hint says. Zero once the view has been closed.
    pub unseen: usize,
    /// The runs, newest first, while the view is open.
    pub rows: Vec<Summary>,
    pub selected: usize,
    /// The first line drawn, kept so the selection stays on screen.
    pub scroll: u16,
}

/// The footer's hint, when there are runs the person has not seen: one line,
/// with the key that opens them.
pub(super) fn hint(unseen: usize, key: &str) -> Option<String> {
    match unseen {
        0 => None,
        1 => Some(format!("1 scheduled run while you were away · {key}")),
        n => Some(format!("{n} scheduled runs while you were away · {key}")),
    }
}

impl App {
    /// The key that opens the view now, as the help would spell it.
    pub(super) fn away_key(&self) -> String {
        self.keymap
            .key_of("away")
            .map(cctop_core::settings::key_label)
            .unwrap_or_else(|| "H".into())
    }

    /// Open the view: every recorded run, joined to the table as it stands,
    /// and the worker asked for what reading the transcripts adds.
    pub(super) fn open_away(&mut self) {
        let log = schedule_log::Log::default();
        let seen = log.seen();
        let rows = schedule_log::summarize(
            &log.read(),
            &self.sessions,
            seen.as_deref(),
            chrono::Utc::now(),
        );
        if rows.is_empty() {
            self.set_status(
                "No scheduled runs yet — add a [[schedule]] to config.toml (see `cctop schedule help`)",
            );
            return;
        }
        self.show_away(rows);
        let sessions: Vec<Session> = self
            .away
            .rows
            .iter()
            .filter_map(|r| r.session.as_deref())
            .filter_map(|key| self.sessions.iter().find(|s| s.key() == key))
            .cloned()
            .collect();
        if !sessions.is_empty() && !cctop_core::under_test() {
            let _ = self.tx.send(Request::AwayDetails { sessions });
        }
    }

    /// Put `rows` on screen, the selection on the newest.
    pub(super) fn show_away(&mut self, rows: Vec<Summary>) {
        self.away.rows = rows;
        self.away.selected = 0;
        self.away.scroll = 0;
        self.mode = Mode::Away;
        self.needs_redraw = true;
    }

    /// The worker's reading of the runs' transcripts: by session key, the
    /// files each edited and what it last said.
    pub(super) fn got_away_details(
        &mut self,
        details: Vec<(String, Option<usize>, Option<String>)>,
    ) {
        for (key, files, said) in details {
            for row in self
                .away
                .rows
                .iter_mut()
                .filter(|r| r.session.as_deref() == Some(key.as_str()))
            {
                row.files_changed = files;
                row.last_message = said.clone();
            }
        }
        self.needs_redraw = true;
    }

    /// Close the view, and with it mark every run it showed as seen.
    pub(super) fn close_away(&mut self) {
        if let Some(newest) = self.away.rows.first() {
            let _ = schedule_log::Log::default().mark_seen(&newest.run.at);
        }
        self.away = Away::default();
        self.mode = Mode::List;
        self.needs_redraw = true;
    }

    pub(super) fn on_key_away(&mut self, key: KeyEvent) {
        let last = self.away.rows.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q' | 'H') => self.close_away(),
            KeyCode::Down | KeyCode::Char('j') => {
                self.away.selected = (self.away.selected + 1).min(last);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.away.selected = self.away.selected.saturating_sub(1);
            }
            KeyCode::Home | KeyCode::Char('g') => self.away.selected = 0,
            KeyCode::End | KeyCode::Char('G') => self.away.selected = last,
            KeyCode::Enter => self.open_away_selected(),
            _ => {}
        }
        self.needs_redraw = true;
    }

    /// Enter: the run's agent in its tab while it is running, else its
    /// conversation in the reader — the two ways there are of seeing what it
    /// did.
    fn open_away_selected(&mut self) {
        let Some(row) = self.away.rows.get(self.away.selected) else {
            return;
        };
        let Some(key) = row.session.clone() else {
            let said = match &row.run.why {
                Some(why) => format!("{}: {why}", row.run.schedule),
                None => format!("{} has no session to open", row.run.schedule),
            };
            self.set_status(said);
            return;
        };
        let Some(session) = self.sessions.iter().find(|s| s.key() == key).cloned() else {
            self.set_status("That run's session is no longer in the table");
            return;
        };
        self.close_away();
        if session.root_pid().is_some() && self.select_session_row(&key) {
            self.attach_selected();
            return;
        }
        self.chat = Some(ChatView::new(session, None));
        self.mode = Mode::Conversation;
        self.fetch_chat(None);
    }

    /// Put the table's selection on the session `key`, if the filters show it.
    fn select_session_row(&mut self, key: &str) -> bool {
        self.reveal(key);
        match self
            .visible
            .iter()
            .position(|&r| matches!(r, Row::Session(i) if self.sessions[i].key() == key))
        {
            Some(row) => {
                self.selected = row;
                true
            }
            None => false,
        }
    }
}

/// The colour a run's state is drawn in: the cost ladder's green, amber and
/// red, as everywhere else on screen — done, at work, wants you.
fn state_style(state: State) -> Style {
    let colors = theme::colors();
    match state {
        State::Finished => Style::default().fg(colors.cost_low),
        State::Working | State::Starting => Style::default().fg(colors.accent),
        State::Permission | State::Question => Style::default()
            .fg(colors.cost_mid)
            .add_modifier(Modifier::BOLD),
        State::ApiError | State::FailedToStart => Style::default().fg(colors.cost_high),
        State::Skipped | State::NoSession => theme::dim(),
    }
}

/// `3h ago`, from the run's timestamp.
fn ago(at: &str, now: &chrono::DateTime<chrono::Utc>) -> String {
    match cctop_core::util::relative_age(at, now).as_str() {
        "now" => "just now".into(),
        "n/a" => String::new(),
        age => format!("{age} ago"),
    }
}

/// One run's two lines: what and when, then what it said or why it did not.
fn run_lines(
    row: &Summary,
    now: &chrono::DateTime<chrono::Utc>,
    text_w: usize,
) -> Vec<Line<'static>> {
    let dim = !row.unseen;
    let fade = |style: Style| match dim {
        true => theme::dim(),
        false => style,
    };
    let mut facts: Vec<String> = Vec::new();
    if row.live && row.state == State::Finished {
        facts.push("idle in its tab".into());
    }
    if let Some(cost) = row.cost {
        facts.push(format!("${cost:.2}"));
    }
    if let Some(ms) = row.duration_ms.filter(|ms| *ms > 0) {
        facts.push(cctop_core::util::compact_duration(ms));
    }
    match row.files_changed {
        Some(1) => facts.push("1 file".into()),
        Some(n) => facts.push(format!("{n} files")),
        None => {}
    }
    let mut first = vec![
        Span::styled(
            format!(
                " {:<18} ",
                cctop_core::util::truncate(&row.run.schedule, 18)
            ),
            fade(theme::value().add_modifier(Modifier::BOLD)),
        ),
        Span::styled(format!("{:>9}  ", ago(&row.run.at, now)), theme::dim()),
        Span::styled(row.state.label().to_string(), fade(state_style(row.state))),
    ];
    if !facts.is_empty() {
        first.push(Span::styled(
            format!(" · {}", facts.join(" · ")),
            fade(theme::label()),
        ));
    }
    if row.run.by_hand {
        first.push(Span::styled(" (by hand)", theme::dim()));
    }
    let mut lines = vec![Line::from(first)];
    if let Some(stuck) = &row.stuck {
        lines.push(Line::from(Span::styled(
            format!("   ⟳ {}", cctop_core::util::truncate(stuck, text_w - 5)),
            fade(Style::default().fg(theme::colors().cost_high)),
        )));
    }
    let said = row.last_message.as_ref().or(row.run.why.as_ref());
    if let Some(said) = said {
        lines.push(Line::from(Span::styled(
            format!("   {}", cctop_core::util::truncate(said, text_w - 3)),
            fade(theme::value()),
        )));
    }
    lines
}

pub(super) fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    const WIDTH: u16 = 92;
    let width = WIDTH.min(area.width.saturating_sub(2));
    let text_w = width.saturating_sub(4) as usize;
    let now = chrono::Utc::now();
    let mut lines: Vec<Line<'static>> = Vec::new();
    // Where each run's lines start and end, to keep the selected one in view.
    let mut spans_of: Vec<(usize, usize)> = Vec::new();
    let unseen = app.away.rows.iter().filter(|r| r.unseen).count();
    let heading = |text: &str| Line::from(Span::styled(format!(" {text}"), theme::label()));
    if unseen > 0 {
        lines.push(heading("Since you last looked"));
    }
    for (i, row) in app.away.rows.iter().enumerate() {
        if !row.unseen && (i == 0 || app.away.rows[i - 1].unseen) {
            if i > 0 {
                lines.push(Line::default());
            }
            lines.push(heading("Earlier"));
        }
        let start = lines.len();
        let mut these = run_lines(row, &now, text_w);
        if i == app.away.selected {
            these = these
                .into_iter()
                .map(|l| l.style(theme::selected()))
                .collect();
        }
        lines.extend(these);
        spans_of.push((start, lines.len()));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " ↵ open its tab or conversation   ↑↓ move   Esc close, marking these seen",
        theme::dim(),
    )));

    // The rows the box has inside its border, as `centered` will clamp it.
    let inner = (lines.len() as u16).min(area.height.saturating_sub(4));
    if let Some(&(start, end)) = spans_of.get(app.away.selected) {
        let (start, end) = (start as u16, end as u16);
        if start < app.away.scroll {
            app.away.scroll = start;
        } else if end > app.away.scroll + inner {
            app.away.scroll = end.saturating_sub(inner);
        }
    }
    let title = match unseen {
        0 => " Scheduled runs ".to_string(),
        n => format!(" Scheduled runs · {n} new "),
    };
    super::modals::scrollable_modal_titled(
        frame,
        area,
        vec![Line::from(Span::styled(title, theme::title()))],
        lines,
        WIDTH,
        app.away.scroll,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{key, test_app};

    #[test]
    fn the_hint_counts_the_runs_and_names_the_key() {
        assert_eq!(hint(0, "H"), None);
        assert_eq!(
            hint(1, "H").as_deref(),
            Some("1 scheduled run while you were away · H")
        );
        assert_eq!(
            hint(3, "J").as_deref(),
            Some("3 scheduled runs while you were away · J")
        );
    }

    fn summary(name: &str, unseen: bool, session: Option<&str>) -> Summary {
        Summary {
            run: schedule_log::Run {
                at: schedule_log::now(),
                schedule: name.into(),
                start: schedule_log::Start::Started,
                why: None,
                dir: "/x".into(),
                harness: "claude".into(),
                account: None,
                model: None,
                tab: format!("cctop-schedule-{name}"),
                by_hand: false,
                pid: None,
            },
            state: State::Finished,
            unseen,
            session: session.map(str::to_string),
            session_id: None,
            live: false,
            cost: None,
            duration_ms: None,
            files_changed: None,
            last_message: None,
            stuck: None,
        }
    }

    /// The keys move within the list, and Esc closes it and clears the hint.
    #[test]
    fn the_view_moves_and_closes() {
        let mut app = test_app();
        app.away.unseen = 2;
        app.show_away(vec![
            summary("a", true, None),
            summary("b", true, None),
            summary("c", false, None),
        ]);
        assert_eq!(app.mode, Mode::Away);
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.away.selected, 2, "stops at the last");
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.away.selected, 1);
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::List);
        assert_eq!(app.away.unseen, 0);
        assert!(app.away.rows.is_empty());
    }

    /// A run that never started has nothing to open, and says why rather
    /// than doing nothing; one whose session is gone from the table says so.
    #[test]
    fn enter_on_a_run_without_a_session_says_why() {
        let mut app = test_app();
        let mut failed = summary("deps", true, None);
        failed.state = State::FailedToStart;
        failed.run.why = Some("~/src/app is not a directory on this machine".into());
        app.show_away(vec![failed, summary("gone", true, Some("claude:nope"))]);
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Away, "stays open");
        app.on_key(key(KeyCode::Down));
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Away);
    }

    /// A finished run's session opens in the reader.
    #[test]
    fn enter_opens_a_finished_runs_conversation() {
        let mut app = test_app();
        app.sessions = vec![crate::tests::session("done", false, "/x")];
        let key_of = app.sessions[0].key();
        app.refilter();
        app.show_away(vec![summary("deps", true, Some(&key_of))]);
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Conversation);
        assert_eq!(
            app.chat.as_ref().map(|c| c.session.key()),
            Some(key_of),
            "the run's own session"
        );
    }
}
