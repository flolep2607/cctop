//! The Preview panel: the selected session's tab, watched from the dashboard.
//!
//! Read-only by construction. Nothing here holds a way to write to a pane, and
//! nothing asks for a size: a pane is drawn at whatever size its tab last gave
//! it, clipped to the panel, because resizing it to fit would make the agent
//! redraw for a window nobody is typing into — and then again when you go back
//! to its tab.
//!
//! There are two places a screen can come from, and on the dashboard the second
//! is the usual one:
//!
//! - **A pane with a client of ours on it** — a tab opened without rmux, a
//!   split, or one being recorded. Its vt100 parser is pumped every tick for
//!   every tab (see the run loop), so the screen is already there to draw.
//! - **A detached rmux tab**, which gave up its client when you switched away
//!   ([`Tab::detach`](super::tabs::Tab::detach)). There is no parser behind it,
//!   so the screen is asked of rmux instead, off the draw thread and at most
//!   twice a second, and replayed into one parser kept here.
//!
//! When the screen is taller than the panel, the bottom of what the agent has
//! drawn is what stays: that is where its prompt and its question are, and the
//! top of a coding agent's screen is scrollback you already read.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::App;
use super::tabs::Tab;
use super::theme;

/// This panel's place in [`panels::TABS`](super::panels::TABS).
pub const TAB: usize = 8;

/// How often a detached tab's screen is asked of rmux while it is on show.
///
/// Two captures a second is enough to watch a spinner turn and a prompt appear,
/// and each costs two short-lived rmux commands — cheap, but not something to do
/// on every one of the dashboard's five wakes a second.
const REFRESH: Duration = Duration::from_millis(500);

/// Where the selected session's screen can be read from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    /// A pane with a live parser: `tabs[tab].panes[pane]`.
    Pane { tab: usize, pane: usize },
    /// A tab standing for a rmux session no client of ours is on.
    Detached { tab: usize, name: String },
    /// A row read from another machine, where cctop hosts nothing.
    Remote(String),
    /// Not in any tab here. `running` picks the hint: `a` opens a live
    /// session's terminal, `R` resumes a stopped one.
    Nowhere { running: bool },
}

/// The tab showing the agent running as `pid`, or resumed as the rmux session
/// `resumed` would be named.
///
/// The pid is how a live row is matched, as [`App::tab_running`] matches it. The
/// name covers the moments the pid does not: a pane `R` has just opened, whose
/// rmux-hosted agent has not been found yet, and a detached tab whose session
/// the sweep has not read a pid off. A pane is asked before a placeholder, so a
/// tab being attached never answers as both.
fn target_in(tabs: &[Tab], pid: Option<u32>, resumed: &str) -> Option<Target> {
    tabs.iter().enumerate().find_map(|(at, tab)| {
        let pane = tab.panes.iter().position(|pane| {
            pid.is_some_and(|pid| pane.agent() == pid)
                || pane.resumed.as_deref() == Some(resumed)
                || pane.rmux.as_deref() == Some(resumed)
        });
        if let Some(pane) = pane {
            return Some(Target::Pane { tab: at, pane });
        }
        let shared = tab.shared.as_ref().filter(|_| tab.detached())?;
        (pid.is_some_and(|pid| shared.pid == Some(pid)) || shared.name == resumed).then(|| {
            Target::Detached {
                tab: at,
                name: shared.name.clone(),
            }
        })
    })
}

/// The first screen row to draw when `rows` of them must fit in `height`.
///
/// Anchored on the lower of the cursor and the last row with anything on it,
/// so the panel ends where the agent's drawing does — a shell that has printed
/// three lines of a fifty-row screen shows those three, not forty blank rows
/// beneath them. Both are `None` on a blank screen, which shows its top.
pub(super) fn first_row(rows: u16, height: u16, cursor: Option<u16>, drawn: Option<u16>) -> u16 {
    if rows <= height {
        return 0;
    }
    let bottom = cursor.max(drawn).map_or(0, |row| row.saturating_add(1));
    bottom.saturating_sub(height).min(rows - height)
}

/// The last row of `screen` with anything on it, scanning up from the bottom.
///
/// Cell by cell rather than through `Screen::rows`, which builds a `String` a
/// row; this runs every frame the panel is on and allocates nothing.
fn last_drawn(screen: &vt100::Screen) -> Option<u16> {
    let (rows, cols) = screen.size();
    (0..rows).rev().find(|&row| {
        (0..cols).any(|col| screen.cell(row, col).is_some_and(vt100::Cell::has_contents))
    })
}

/// A screen seen from row `top` down, for `tui_term` to draw — it always draws
/// from a screen's first row, and has no offset of its own.
struct Window<'a> {
    screen: &'a vt100::Screen,
    top: u16,
}

impl tui_term::widget::Screen for Window<'_> {
    type C = vt100::Cell;

    fn cell(&self, row: u16, col: u16) -> Option<&vt100::Cell> {
        self.screen.cell(row.checked_add(self.top)?, col)
    }

    fn hide_cursor(&self) -> bool {
        self.screen.hide_cursor()
    }

    /// Shifted by the same `top`, and pushed out of range when the cursor is
    /// above the window: the renderer bounds-checks it and draws nothing.
    fn cursor_position(&self) -> (u16, u16) {
        let (row, col) = tui_term::widget::Screen::cursor_position(self.screen);
        (row.checked_sub(self.top).unwrap_or(u16::MAX), col)
    }
}

/// The screen of the detached tab last previewed, kept between captures.
#[derive(Default)]
pub struct Capture {
    /// The rmux session this is a screen of. A different one starts over.
    name: String,
    parser: Option<vt100::Parser>,
    /// The last capture replayed, so an unchanged screen costs no redraw.
    last: Option<crate::rmux::Capture>,
    /// rmux answered, but not with a screen — the session went, most likely.
    failed: bool,
    asked_at: Option<Instant>,
    pending: Option<Receiver<Option<crate::rmux::Capture>>>,
}

impl Capture {
    /// Replay `capture` into the parser, returning whether anything changed.
    ///
    /// One parser, resized and cleared rather than rebuilt, so a panel left on
    /// for an afternoon does not allocate a fresh grid twice a second.
    fn apply(&mut self, capture: crate::rmux::Capture) -> bool {
        if self.last.as_ref() == Some(&capture) {
            return false;
        }
        let parser = self
            .parser
            .get_or_insert_with(|| vt100::Parser::new(capture.rows, capture.cols, 0));
        parser.screen_mut().set_size(capture.rows, capture.cols);
        parser.process(b"\x1b[0m\x1b[H\x1b[2J");
        parser.process(&capture.bytes);
        match capture.cursor {
            Some((row, col)) => {
                parser.process(format!("\x1b[{};{}H\x1b[?25h", row + 1, col + 1).as_bytes());
            }
            None => parser.process(b"\x1b[?25l"),
        }
        self.last = Some(capture);
        true
    }
}

impl App {
    /// Where the selected row's screen can be read from, or `None` with no
    /// session under the cursor.
    pub(super) fn preview_target(&self) -> Option<Target> {
        let session = self.selected_session()?;
        if let Some(remote) = &session.remote {
            return Some(Target::Remote(remote.host.clone()));
        }
        let resumed = crate::rmux::name_for_session(session.provider.as_str(), &session.session_id);
        Some(
            target_in(&self.tabs, session.root_pid(), &resumed).unwrap_or(Target::Nowhere {
                running: session.is_running(),
            }),
        )
    }

    /// Keep a detached tab's screen fresh while the Preview panel shows it,
    /// returning whether there is something new to draw.
    ///
    /// Only while it is on screen: a capture nobody sees is two processes
    /// spawned for nothing. The capture itself runs on a thread of its own, so a
    /// slow rmux delays the preview and never the dashboard.
    pub(super) fn pump_preview(&mut self) -> bool {
        if self.tab != 0 || self.bottom_tab != TAB {
            return false;
        }
        let Some(Target::Detached { name, .. }) = self.preview_target() else {
            return false;
        };
        let capture = &mut self.preview;
        if capture.name != name {
            *capture = Capture {
                name: name.clone(),
                ..Capture::default()
            };
        }
        let mut changed = false;
        if let Some(pending) = &capture.pending {
            match pending.try_recv() {
                Ok(answer) => {
                    capture.pending = None;
                    changed = match answer {
                        Some(screen) => {
                            let drew = capture.apply(screen);
                            let recovered = std::mem::take(&mut capture.failed);
                            drew || recovered
                        }
                        None => !std::mem::replace(&mut capture.failed, true),
                    };
                }
                Err(TryRecvError::Empty) => return false,
                Err(TryRecvError::Disconnected) => capture.pending = None,
            }
        }
        if capture.asked_at.is_none_or(|at| at.elapsed() >= REFRESH) {
            capture.asked_at = Some(Instant::now());
            let (tx, rx) = mpsc::channel();
            capture.pending = Some(rx);
            std::thread::spawn(move || {
                // The receiver is gone if the cursor moved on meanwhile; the
                // answer is simply not wanted any more.
                let _ = tx.send(crate::rmux::capture(&name));
            });
        }
        changed
    }
}

/// One dim line, which is everything the panel says when it has no screen.
fn say(frame: &mut Frame, inner: Rect, text: String) {
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(text, theme::dim()))),
        inner,
    );
}

/// The key that goes to tab `at` of `tabs`, while there is one.
fn alt_key(at: usize) -> Option<String> {
    // `Alt+1` is the dashboard, so the first tab is `Alt+2`.
    (at + 2 <= 9).then(|| format!("Alt+{}", at + 2))
}

/// Draw the panel into `inner`, captioning `area`'s bottom border.
pub(super) fn draw(frame: &mut Frame, area: Rect, inner: Rect, app: &App) {
    let Some(target) = app.preview_target() else {
        return;
    };
    let (screen, at) = match &target {
        Target::Pane { tab, pane } => (app.tabs[*tab].panes[*pane].view.parser.screen(), *tab),
        Target::Detached { tab, name } => {
            let capture = &app.preview;
            let ours = capture.name == *name;
            if ours && capture.failed {
                let how = alt_key(*tab).map_or(String::new(), |k| format!(" — {k} opens it"));
                let label = app.tabs[*tab].title();
                return say(
                    frame,
                    inner,
                    format!("rmux did not hand over {label}'s screen{how}"),
                );
            }
            match &capture.parser {
                Some(parser) if ours => (parser.screen(), *tab),
                _ => return say(frame, inner, "Loading…".to_string()),
            }
        }
        Target::Remote(host) => {
            return say(
                frame,
                inner,
                format!("This session is on {host}; cctop only hosts tabs for this machine."),
            );
        }
        Target::Nowhere { running: true } => {
            return say(
                frame,
                inner,
                "Not in a tab here — a opens its terminal in one, and it shows here live."
                    .to_string(),
            );
        }
        Target::Nowhere { running: false } => {
            return say(
                frame,
                inner,
                "Not in a tab here — R resumes it in one, and it shows here live.".to_string(),
            );
        }
    };

    let (rows, cols) = screen.size();
    let cursor = (!screen.hide_cursor()).then(|| screen.cursor_position().0);
    let top = first_row(rows, inner.height, cursor, last_drawn(screen));
    let area_drawn = Rect {
        width: cols.min(inner.width),
        height: rows.saturating_sub(top).min(inner.height),
        ..inner
    };
    frame.render_widget(
        tui_term::widget::PseudoTerminal::new(&Window { screen, top }),
        area_drawn,
    );

    // Said on the border, where it costs the screen nothing: which tab this is,
    // that typing here does not reach it, and how to get to where it does.
    let mut caption = format!(" {} · read-only", app.tabs[at].title());
    if let Some(key) = alt_key(at) {
        caption.push_str(&format!(" · {key} to drive"));
    }
    caption.push(' ');
    if area.height >= 2 && area.width > 4 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(caption, theme::dim())).right_aligned()),
            Rect {
                x: area.x + 2,
                y: area.y + area.height - 1,
                width: area.width - 4,
                height: 1,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tabs::Pane;

    fn running(name: &str, pid: Option<u32>) -> crate::rmux::Running {
        crate::rmux::Running {
            name: name.to_string(),
            pid,
            cwd: None,
            attached: false,
            activity: None,
            label: Some("claude".into()),
            profile: None,
            order: None,
            state: None,
            color: None,
        }
    }

    #[test]
    fn the_panel_is_where_its_number_key_says() {
        assert_eq!(super::super::panels::TABS[TAB], "Preview");
    }

    #[test]
    fn a_row_finds_the_pane_its_agent_is_in() {
        // A test pane knows no agent pid of its own, so it answers to the pid
        // it hosts, the way a directly hosted agent does.
        let pane = |pid| {
            let mut pane = Pane::for_test("claude");
            pane.pid = pid;
            pane
        };
        let mut split = Tab::new(pane(1));
        split.panes.push(pane(4321));
        let tabs = vec![Tab::new(pane(2)), split];
        assert_eq!(
            target_in(&tabs, Some(4321), "cctop-claude-x"),
            Some(Target::Pane { tab: 1, pane: 1 })
        );
        assert_eq!(target_in(&tabs, Some(99), "cctop-claude-x"), None);
    }

    #[test]
    fn a_pane_just_resumed_is_found_before_its_agent_pid_is_known() {
        let mut pane = Pane::for_test("claude");
        pane.pid = 1;
        pane.resumed = Some("cctop-claude-abc".into());
        let tabs = vec![Tab::new(pane)];
        assert_eq!(
            target_in(&tabs, Some(4321), "cctop-claude-abc"),
            Some(Target::Pane { tab: 0, pane: 0 })
        );
        assert_eq!(
            target_in(&tabs, None, "cctop-claude-abc"),
            Some(Target::Pane { tab: 0, pane: 0 })
        );
        assert_eq!(target_in(&tabs, None, "cctop-claude-def"), None);
    }

    #[test]
    fn a_detached_tab_is_found_by_pid_or_by_session_name() {
        let tabs = vec![
            Tab::shared(&running("cctop-claude-one", Some(10))),
            Tab::shared(&running("cctop-claude-two", None)),
        ];
        assert_eq!(
            target_in(&tabs, Some(10), "cctop-claude-zzz"),
            Some(Target::Detached {
                tab: 0,
                name: "cctop-claude-one".into()
            })
        );
        assert_eq!(
            target_in(&tabs, Some(11), "cctop-claude-two"),
            Some(Target::Detached {
                tab: 1,
                name: "cctop-claude-two".into()
            })
        );
        // No pid read yet must not match a row that has none either.
        assert_eq!(target_in(&tabs, None, "cctop-claude-zzz"), None);
    }

    #[test]
    fn a_screen_that_fits_is_drawn_from_its_top() {
        assert_eq!(first_row(10, 10, Some(9), Some(9)), 0);
        assert_eq!(first_row(4, 10, None, None), 0);
    }

    #[test]
    fn a_tall_screen_keeps_the_rows_the_agent_drew_last() {
        // A 40-row screen whose prompt is on row 30, in an 8-row panel.
        assert_eq!(first_row(40, 8, Some(30), Some(33)), 26);
        // The cursor below the last drawn row wins, so a fresh prompt shows.
        assert_eq!(first_row(40, 8, Some(39), Some(20)), 32);
        // A short burst of output at the top of a tall screen stays at the top
        // rather than scrolling to the blank rows beneath it.
        assert_eq!(first_row(40, 8, Some(3), Some(2)), 0);
        // Nothing drawn anywhere: the top, not a window of blank bottom rows.
        assert_eq!(first_row(40, 8, None, None), 0);
    }

    #[test]
    fn the_window_shifts_cells_and_cursor_together() {
        let mut parser = vt100::Parser::new(6, 10, 0);
        parser.process(b"a\r\nb\r\nc\r\nd\r\ne\r\nf");
        let screen = parser.screen();
        assert_eq!(last_drawn(screen), Some(5));
        let window = Window { screen, top: 3 };
        use tui_term::widget::Screen;
        assert_eq!(window.cell(0, 0).map(vt100::Cell::contents), Some("d"));
        assert_eq!(window.cursor_position(), (2, 1));
        // A cursor above the window is out of range, never at row zero.
        parser.process(b"\x1b[1;1H");
        let window = Window {
            screen: parser.screen(),
            top: 3,
        };
        assert_eq!(window.cursor_position().0, u16::MAX);
    }

    fn drawn(app: &mut App, (cols, rows): (u16, u16)) -> Vec<String> {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        terminal
            .draw(|frame| {
                super::super::render::draw(frame, app);
            })
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..rows)
            .map(|y| (0..cols).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    /// The whole path, drawn: a row whose session is in a tab shows that tab's
    /// screen, cut to its bottom rows, with nothing asked of the pane's size.
    #[test]
    fn the_panel_shows_the_bottom_of_the_tab_the_row_is_in() {
        let mut app = super::super::tests::test_app();
        app.sessions = vec![super::super::tests::session("abc", false, "/x")];
        app.refilter();
        let mut pane = Pane::for_test("claude");
        pane.resumed = Some(crate::rmux::name_for_session("claude", "abc"));
        let lines: Vec<String> = (0..24).map(|i| format!("line-{i:02}")).collect();
        pane.view.parser.process(lines.join("\r\n").as_bytes());
        let size = pane.view.size;
        app.tabs.push(Tab::new(pane));
        app.bottom_tab = TAB;

        let screen = drawn(&mut app, (100, 40));
        let text = screen.join("\n");
        assert!(
            text.contains("line-23"),
            "the bottom row is missing:\n{text}"
        );
        assert!(
            !text.contains("line-00"),
            "the top should be clipped:\n{text}"
        );
        assert!(text.contains("read-only"), "no caption:\n{text}");
        assert_eq!(app.tabs[0].panes[0].view.size, size, "the pane was resized");
    }

    #[test]
    fn a_row_with_no_tab_says_how_to_open_one() {
        let mut app = super::super::tests::test_app();
        app.sessions = vec![super::super::tests::session("abc", false, "/x")];
        app.refilter();
        app.bottom_tab = TAB;
        let text = drawn(&mut app, (100, 40)).join("\n");
        assert!(text.contains("R resumes it in one"), "{text}");
    }

    #[test]
    fn a_capture_replays_into_one_parser_and_skips_a_repeat() {
        let mut capture = Capture::default();
        let screen = crate::rmux::Capture {
            cols: 20,
            rows: 3,
            cursor: Some((2, 4)),
            bytes: b"one\r\n\x1b[31mtwo\r\nthree".to_vec(),
        };
        assert!(capture.apply(screen.clone()));
        let parsed = capture.parser.as_ref().unwrap().screen();
        assert_eq!(parsed.size(), (3, 20));
        assert_eq!(parsed.cell(1, 0).unwrap().fgcolor(), vt100::Color::Idx(1));
        assert_eq!(parsed.cursor_position(), (2, 4));
        assert!(!capture.apply(screen), "an unchanged screen is not redrawn");

        // A smaller screen clears what the bigger one left behind.
        assert!(capture.apply(crate::rmux::Capture {
            cols: 10,
            rows: 2,
            cursor: None,
            bytes: b"x".to_vec(),
        }));
        let parsed = capture.parser.as_ref().unwrap().screen();
        assert_eq!(parsed.size(), (2, 10));
        assert_eq!(parsed.contents(), "x");
        assert!(parsed.hide_cursor());
    }
}
