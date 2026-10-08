//! What cctop just said, held on screen long enough to be read.
//!
//! Every message used to share the footer's one status line, so each replaced
//! the last: "Restarted 3 tabs, skipped 1 mid-turn" was gone within a second,
//! overwritten by whatever the next event had to say. A few recent messages are
//! kept instead, each for as long as it takes to read, stacked in the top-right
//! of the Overview.
//!
//! The Overview rather than the bottom-right corner most toasts use, because it
//! is the one place that is cctop's own on every screen. Inside a tab the
//! bottom of the frame is the agent's — its prompt and its cursor sit there —
//! and the bottom-right corner already belongs to the pasted-image preview,
//! with the footer's share link under it. On the dashboard the Overview is
//! above the table, so no toast lands on the selected row either. What a toast
//! covers there is the machine's CPU and memory figures, for a few seconds.
//!
//! Built here rather than taken from a crate. `ratatui-toaster` holds a single
//! toast, the opposite of what this is for, and paints it in fixed ANSI blues
//! and reds that ignore the palette. `tui-overlay` is a positioning primitive
//! with a slide animation; it owns no lifecycle, so the stack, the expiry and
//! the collapsing would all be written here anyway, around a `Rect`
//! calculation this module does in three lines. Neither says anything about
//! clicks, which is right: a toast records nothing in [`Layout`], so a click on
//! one reaches whatever is under it exactly as it did before it appeared.
//!
//! [`Layout`]: super::render::Layout

use super::theme;
use crate::util;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Widget};
use std::time::{Duration, Instant};

/// How many messages are held at once.
///
/// The Overview has four rows inside its border, and a stack taller than the
/// room it is drawn in only holds messages nobody can see.
const HELD: usize = 4;

/// The widest a toast grows before its text wraps. Wide enough for nearly
/// every message cctop writes on one line, narrow enough that the Overview's
/// spend figures on the left stay readable on a normal-width terminal.
const MAX_WIDTH: u16 = 72;

/// How a message reads: news, or something that did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Info,
    Failure,
}

impl Severity {
    /// Read off the message's own wording.
    ///
    /// cctop's failures already say so in their first words — "Could not
    /// save", "Could not open a tunnel" — so the words are the classification.
    /// An explicit `set_error` would mean revisiting every call site to repeat
    /// what the sentence already says, and a new failure written the same way
    /// is caught without anybody remembering to.
    fn of(text: &str) -> Severity {
        const OPENINGS: [&str; 4] = ["Could not", "Couldn't", "Cannot", "Failed"];
        match OPENINGS.iter().any(|o| text.starts_with(o)) || text.contains(" failed") {
            true => Severity::Failure,
            false => Severity::Info,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    /// How many times in a row this was said. Shown as `×N` once it is past one.
    pub count: u32,
    /// When it was last said: a repeat restarts the clock, since it is news
    /// again.
    pub at: Instant,
    pub severity: Severity,
}

impl Toast {
    /// Long enough to read. A short confirmation goes in a few seconds and a
    /// long sentence stays for as long as it takes to read it; a failure is
    /// given longer again, because it is the message most likely to need
    /// acting on.
    fn ttl(&self) -> Duration {
        let reading = 3.0 + util::cells(&self.text) as f64 / 15.0;
        let extra = match self.severity {
            Severity::Info => 0.0,
            Severity::Failure => 3.0,
        };
        Duration::from_secs_f64(reading.min(10.0) + extra)
    }

    /// A message that says something is under way — "Refreshing…", "Opening a
    /// tunnel…" — and is over as soon as anything else is said.
    fn provisional(&self) -> bool {
        self.text.ends_with('…')
    }

    fn label(&self) -> String {
        match self.count {
            1 => self.text.clone(),
            n => format!("{} ×{n}", self.text),
        }
    }
}

/// The recent messages, newest first.
#[derive(Debug, Default)]
pub struct Toasts {
    stack: Vec<Toast>,
}

impl Toasts {
    pub fn push(&mut self, text: String) {
        self.push_at(text, Instant::now());
    }

    fn push_at(&mut self, text: String, now: Instant) {
        if let Some(newest) = self.stack.first_mut() {
            // The same thing said again — a key held down, a refresh that
            // keeps failing — is one message said often, not a stack of copies
            // pushing everything else out.
            if newest.text == text {
                newest.count += 1;
                newest.at = now;
                return;
            }
            // "Deleting session…" followed by "Deleted session" is one event.
            // Keeping both would spend a row of the stack on a sentence the
            // next one has already made untrue.
            if newest.provisional() {
                self.stack.remove(0);
            }
        }
        let severity = Severity::of(&text);
        self.stack.insert(
            0,
            Toast {
                text,
                count: 1,
                at: now,
                severity,
            },
        );
        self.stack.truncate(HELD);
    }

    /// Drop what has been up long enough. Whether anything went, so the loop
    /// knows the frame changed.
    pub fn expire(&mut self, now: Instant) -> bool {
        let before = self.stack.len();
        self.stack
            .retain(|t| now.saturating_duration_since(t.at) <= t.ttl());
        self.stack.len() != before
    }

    /// The last thing said, while it is still up — what a test asks when it
    /// wants to know what the user was told.
    #[cfg(test)]
    pub fn latest(&self) -> Option<&str> {
        self.stack.first().map(|t| t.text.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = &Toast> {
        self.stack.iter()
    }
}

/// A toast's ground: a panel-coloured pill that stands off whatever it covers,
/// or reverse video without colour, where nothing else would separate it from
/// the figures underneath.
fn pill(severity: Severity) -> Style {
    let style = match theme::no_color() {
        true => Style::default().add_modifier(Modifier::REVERSED),
        false => Style::default()
            .bg(theme::colors().header_bg)
            .fg(theme::colors().value),
    };
    match severity {
        Severity::Info => style,
        Severity::Failure => style.add_modifier(Modifier::BOLD),
    }
}

/// The bar down a toast's left edge: green for news, as the status line was,
/// amber for a failure. Without colour the glyph changes instead, so a failure
/// is still told apart by its shape.
fn marker(severity: Severity) -> Span<'static> {
    let (glyph, color) = match (severity, theme::no_color()) {
        (Severity::Info, _) => ("▌", theme::colors().cost_low),
        (Severity::Failure, true) => ("!", theme::colors().cost_mid),
        (Severity::Failure, false) => ("▌", theme::colors().cost_mid),
    };
    Span::styled(glyph, pill(severity).fg(color))
}

/// Wrap onto at most two lines of `width` cells.
///
/// Two, because a toast that wraps further is an error message long enough to
/// push the rest of the stack off the Overview.
///
/// ponytail: past two lines the tail is cut at a `…`. The messages long enough
/// to reach it quote an OS or network error whole, and the start of one is
/// the part that says what went wrong.
fn wrap(text: &str, width: usize) -> Vec<String> {
    if util::cells(text) <= width {
        return vec![text.to_string()];
    }
    let mut first = String::new();
    let mut rest = text;
    for word in text.split_inclusive(' ') {
        if util::cells(&first) + util::cells(word.trim_end()) > width {
            break;
        }
        first.push_str(word);
        rest = &rest[word.len()..];
    }
    // One word wider than the toast has nowhere to break.
    if first.is_empty() {
        return vec![util::truncate(text, width)];
    }
    vec![
        first.trim_end().to_string(),
        util::truncate(rest.trim_start(), width),
    ]
}

/// Draw the stack into `region`, newest at the top, each toast against its
/// right edge. Whatever does not fit in `region`'s rows is not drawn; it is
/// older than everything that is, and expires on its own.
pub fn draw(buf: &mut Buffer, region: Rect, toasts: &Toasts) {
    let region = region.intersection(buf.area);
    // The marker, a space either side of the text.
    const CHROME: u16 = 3;
    let width = MAX_WIDTH.min(region.width);
    if width <= CHROME {
        return;
    }
    let mut y = region.y;
    for toast in toasts.iter() {
        let lines = wrap(&toast.label(), (width - CHROME) as usize);
        let height = lines.len() as u16;
        if y + height > region.bottom() {
            break;
        }
        let w = lines.iter().map(|l| util::cells(l)).max().unwrap_or(0) as u16 + CHROME;
        let rect = Rect {
            x: region.right() - w,
            y,
            width: w,
            height,
        };
        let style = pill(toast.severity);
        let text: Vec<Line> = lines
            .into_iter()
            .map(|l| {
                Line::from(vec![
                    marker(toast.severity),
                    Span::styled(format!(" {l}"), style),
                ])
            })
            .collect();
        Clear.render(rect, buf);
        Paragraph::new(text).style(style).render(rect, buf);
        y += height;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(toasts: &Toasts) -> Vec<String> {
        toasts.iter().map(Toast::label).collect()
    }

    /// Every row of the buffer, so an assertion can say where a toast is.
    fn rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn toasts_stack_newest_first_and_hold_only_a_few() {
        let mut toasts = Toasts::default();
        for n in 1..=6 {
            toasts.push(format!("message {n}"));
        }
        assert_eq!(
            said(&toasts),
            ["message 6", "message 5", "message 4", "message 3"]
        );
        assert_eq!(toasts.latest(), Some("message 6"));
    }

    /// The same message twice in a row is one toast counting, and a repeat
    /// separated by something else is news again.
    #[test]
    fn a_repeated_message_collapses_into_a_count() {
        let mut toasts = Toasts::default();
        toasts.push("Refresh failed".into());
        toasts.push("Refresh failed".into());
        toasts.push("Refresh failed".into());
        assert_eq!(said(&toasts), ["Refresh failed ×3"]);

        toasts.push("Saved".into());
        toasts.push("Refresh failed".into());
        assert_eq!(
            said(&toasts),
            ["Refresh failed", "Saved", "Refresh failed ×3"]
        );
    }

    #[test]
    fn a_message_under_way_gives_way_to_the_next() {
        let mut toasts = Toasts::default();
        toasts.push("Tab renamed to api".into());
        toasts.push("Deleting session 1234…".into());
        toasts.push("Deleted session".into());
        assert_eq!(said(&toasts), ["Deleted session", "Tab renamed to api"]);
    }

    /// A toast goes on its own, and a longer one — or a failure — stays up
    /// for longer than a short confirmation.
    #[test]
    fn toasts_expire_and_longer_ones_last_longer() {
        let start = Instant::now();
        let mut toasts = Toasts::default();
        toasts.push_at("Saved".into(), start);
        toasts.push_at(
            "Restarted 3 tabs, skipped 1 mid-turn — it restarts when the turn ends".into(),
            start,
        );
        toasts.push_at("Could not save: permission denied".into(), start);

        assert!(!toasts.expire(start + Duration::from_secs(2)));
        assert_eq!(toasts.iter().count(), 3);

        assert!(toasts.expire(start + Duration::from_secs(4)));
        assert_eq!(
            toasts.latest(),
            Some("Could not save: permission denied"),
            "the failure should outlast the confirmation"
        );
        assert!(!said(&toasts).contains(&"Saved".to_string()));
        assert_eq!(toasts.iter().count(), 2);

        assert!(toasts.expire(start + Duration::from_secs(30)));
        assert_eq!(toasts.latest(), None);
    }

    /// Saying it again is news again: the clock restarts rather than the
    /// repeat vanishing with the first time it was said.
    #[test]
    fn a_repeat_restarts_the_clock() {
        let start = Instant::now();
        let mut toasts = Toasts::default();
        toasts.push_at("Saved".into(), start);
        toasts.push_at("Saved".into(), start + Duration::from_secs(3));
        toasts.expire(start + Duration::from_secs(5));
        assert_eq!(said(&toasts), ["Saved ×2"]);
    }

    #[test]
    fn failures_are_read_off_their_wording() {
        assert_eq!(
            Severity::of("Could not serve: port in use"),
            Severity::Failure
        );
        assert_eq!(
            Severity::of("Restarted 2 tabs, 1 failed"),
            Severity::Failure
        );
        assert_eq!(Severity::of("Saved ~/.config/cctop"), Severity::Info);
    }

    #[test]
    fn the_stack_draws_newest_at_the_top_against_the_right_edge() {
        let mut toasts = Toasts::default();
        toasts.push("older".into());
        toasts.push("newer".into());
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 4));
        draw(&mut buf, Rect::new(0, 0, 40, 4), &toasts);
        let rows = rows(&buf);
        assert!(rows[0].ends_with("▌ newer "), "{rows:?}");
        assert!(rows[1].ends_with("▌ older "), "{rows:?}");
        assert!(rows[2].trim().is_empty(), "{rows:?}");
    }

    /// A message longer than the toast wraps onto a second line rather than
    /// losing its end, and a stack taller than its region drops the oldest.
    #[test]
    fn a_long_message_wraps_and_the_region_bounds_the_stack() {
        let mut toasts = Toasts::default();
        toasts.push("third".into());
        toasts.push("second".into());
        toasts.push("Could not open a tunnel: cloudflared exited".into());
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 3));
        draw(&mut buf, Rect::new(0, 0, 30, 3), &toasts);
        let rows = rows(&buf);
        assert!(rows[0].contains("Could not open a tunnel:"), "{rows:?}");
        assert!(rows[1].contains("cloudflared exited"), "{rows:?}");
        assert!(rows[2].contains("second"), "{rows:?}");
        assert!(!rows.concat().contains("third"), "{rows:?}");
    }
}
