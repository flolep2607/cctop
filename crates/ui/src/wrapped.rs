//! `Y` on the table: the year in review, a card at a time.
//!
//! Shaped like the recaps music apps put out each December — one figure per
//! full-screen card, each on a colour of its own, stepped through with the
//! arrows — because a year is a story, and twenty figures in a table are not.
//! The figures are [`cctop_core::insight::wrapped`]'s; this only stages them.
//!
//! Each card's headline number counts up when the card opens. Like the rest
//! of [`super::effects`], that is a pure function of how long the card has
//! been up, so a frame is the same however it was reached, and a test that
//! never starts the clock sees the finished figure.

use super::rave::{hsv, snap};
use super::*;
use cctop_core::insight::Task;
use cctop_core::insight::wrapped::Wrapped;
use cctop_core::util::{compact_tokens, truncate, with_commas};
use chrono::Datelike;
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

/// How long a headline number takes to count up to itself.
const COUNT_UP: Duration = Duration::from_millis(900);

/// The widest the cards' column gets; past this a line is too long to read
/// as a headline.
const COLUMN: u16 = 64;

/// The open recap.
#[derive(Default)]
pub struct Recap {
    /// The year asked for, so an answer for one already left is dropped.
    pub asked: i32,
    /// `None` while the worker is still reading.
    pub year: Option<Box<Wrapped>>,
    pub card: usize,
    /// When the card on screen opened, for the count-up. `None` draws every
    /// figure finished.
    pub shown: Option<Instant>,
}

impl App {
    /// Open the recap on this year.
    ///
    /// It re-reads every transcript, as `optimize` does, so it opens on a
    /// spinner and the figures arrive from the worker.
    pub(super) fn open_wrapped(&mut self, year: i32) {
        self.recap = Recap {
            asked: year,
            ..Default::default()
        };
        self.mode = Mode::Wrapped;
        let _ = self.tx.send(Request::Wrapped {
            year,
            hosts: self.remote_hosts.clone(),
        });
    }

    pub(super) fn got_wrapped(&mut self, year: Box<Wrapped>) {
        if self.mode == Mode::Wrapped && year.year == self.recap.asked {
            self.recap.year = Some(year);
            self.recap.card = 0;
            self.recap.shown = Some(Instant::now());
        }
    }

    /// Whether the recap is moving: its spinner, or a number counting up.
    pub(crate) fn wrapped_animating(&self) -> bool {
        self.mode == Mode::Wrapped
            && match &self.recap.year {
                None => true,
                Some(_) => self.recap.shown.is_some_and(|t| t.elapsed() < COUNT_UP),
            }
    }

    /// Read-only: every key steps, changes year, or closes.
    pub(super) fn on_key_wrapped(&mut self, key: KeyEvent) {
        let last = self
            .recap
            .year
            .as_deref()
            .map_or(0, |w| cards(w).len().saturating_sub(1));
        let card = match key.code {
            KeyCode::Esc | KeyCode::Char('q' | 'Y') => {
                self.mode = Mode::List;
                self.recap = Recap::default();
                return;
            }
            KeyCode::Right | KeyCode::Char('l' | ' ' | 'n') | KeyCode::Enter => {
                (self.recap.card + 1).min(last)
            }
            KeyCode::Left | KeyCode::Char('h' | 'p') | KeyCode::Backspace => {
                self.recap.card.saturating_sub(1)
            }
            KeyCode::Home | KeyCode::Char('g') => 0,
            KeyCode::End | KeyCode::Char('G') => last,
            // The year before — the one that wants looking back on in January.
            KeyCode::Char('[') => return self.open_wrapped(self.recap.asked - 1),
            KeyCode::Char(']') if self.recap.asked < chrono::Local::now().year() => {
                return self.open_wrapped(self.recap.asked + 1);
            }
            _ => return,
        };
        if card != self.recap.card {
            self.recap.card = card;
            self.recap.shown = Some(Instant::now());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Card {
    Intro,
    Hours,
    Months,
    Clock,
    Spend,
    Models,
    Projects,
    Work,
    Commands,
    Keep,
    Summary,
    Empty,
}

impl Card {
    /// Each card's own colour, as hue in degrees.
    fn hue(self) -> f32 {
        match self {
            Card::Intro => 280.0,
            Card::Hours => 150.0,
            Card::Months => 200.0,
            Card::Clock => 235.0,
            Card::Spend => 35.0,
            Card::Models => 330.0,
            Card::Projects => 175.0,
            Card::Work => 10.0,
            Card::Commands => 260.0,
            Card::Keep => 50.0,
            Card::Summary => 300.0,
            Card::Empty => 220.0,
        }
    }
}

/// The cards this year has something to say on, in order. A card with nothing
/// to show — no spend for a harness that records none — is left out rather
/// than drawn as a zero.
fn cards(w: &Wrapped) -> Vec<Card> {
    if w.since.is_none() {
        return vec![Card::Empty];
    }
    let mut out = vec![Card::Intro, Card::Hours, Card::Months, Card::Clock];
    if w.cost > 0.0 {
        out.push(Card::Spend);
    }
    if !w.models.is_empty() {
        out.push(Card::Models);
    }
    out.push(Card::Projects);
    if !w.tasks.is_empty() {
        out.push(Card::Work);
    }
    if !w.commands.is_empty() {
        out.push(Card::Commands);
    }
    if w.cleaned_up() {
        out.push(Card::Keep);
    }
    out.push(Card::Summary);
    out
}

/// The colours of one card: a ground of its hue, and white on it. On a
/// terminal asked for no colour, the theme's own styles on a cleared screen.
struct Ink {
    hue: f32,
    /// `None` for no colour, else whether the terminal takes truecolor.
    colour: Option<bool>,
}

impl Ink {
    fn fg(&self, rgb: (u8, u8, u8), fallback: Style) -> Style {
        match self.colour {
            Some(tc) => Style::default().fg(snap(rgb, tc)),
            None => fallback,
        }
    }

    fn text(&self) -> Style {
        self.fg((245, 245, 245), theme::value())
    }

    fn dim(&self) -> Style {
        self.fg(hsv(self.hue, 0.25, 0.80), theme::dim())
    }

    fn accent(&self) -> Style {
        self.fg(hsv(self.hue + 20.0, 0.45, 1.0), theme::title())
            .add_modifier(Modifier::BOLD)
    }

    fn bold(&self) -> Style {
        self.text().add_modifier(Modifier::BOLD)
    }

    /// A vertical gradient of the card's hue, darkest at the foot so the
    /// figures near the top carry the colour.
    fn ground(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);
        let Some(tc) = self.colour else {
            return;
        };
        let buf = frame.buffer_mut();
        let rows = area.height.max(2) as f32 - 1.0;
        for y in area.top()..area.bottom() {
            let t = (y - area.top()) as f32 / rows;
            let bg = match tc {
                true => hsv(self.hue + 40.0 * t, 0.75, 0.42 - 0.24 * t),
                false => flat(self.hue),
            };
            let bg = snap(bg, tc);
            for x in area.left()..area.right() {
                buf[(x, y)].set_bg(bg);
            }
        }
    }
}

/// A card's ground without truecolor: one flat colour, bright enough to land
/// on the xterm cube. Its darkest coloured step is 95 of 255, so anything
/// darker snaps to the grey ramp and the card loses its hue.
fn flat(hue: f32) -> (u8, u8, u8) {
    hsv(hue, 0.85, 0.45)
}

pub(super) fn draw(frame: &mut Frame, area: Rect, app: &App) {
    let colour = (!theme::no_color()).then(theme::truecolor);
    let Some(w) = app.recap.year.as_deref() else {
        let ink = Ink {
            hue: Card::Intro.hue(),
            colour,
        };
        ink.ground(frame, area);
        let lines = vec![
            Line::styled(
                format!("C C T O P   W R A P P E D   {}", app.recap.asked),
                ink.dim(),
            ),
            Line::default(),
            Line::from(vec![
                Span::styled(format!("{}  ", share::spinner_frame()), ink.accent()),
                Span::styled("Reading every transcript of the year…", ink.text()),
            ]),
        ];
        place(frame, area, lines);
        return;
    };

    let all = cards(w);
    let at = app.recap.card.min(all.len() - 1);
    let card = all[at];
    let ink = Ink {
        hue: card.hue(),
        colour,
    };
    ink.ground(frame, area);

    // A count-up eases out, so it races through the small numbers and
    // settles on the real one rather than crawling up to it.
    let t = app.recap.shown.map_or(1.0, |s| {
        (s.elapsed().as_secs_f64() / COUNT_UP.as_secs_f64()).min(1.0)
    });
    let reveal = 1.0 - (1.0 - t).powi(3);

    let lines = match card {
        Card::Intro => intro(w, &ink),
        Card::Hours => hours(w, &ink, reveal),
        Card::Months => months(w, &ink),
        Card::Clock => clock(w, &ink),
        Card::Spend => spend(w, &ink, reveal),
        Card::Models => models(w, &ink),
        Card::Projects => projects(w, &ink),
        Card::Work => work(w, &ink, reveal),
        Card::Commands => commands(w, &ink, reveal),
        Card::Keep => keep(w, &ink),
        Card::Summary => summary(w, &ink),
        Card::Empty => empty(w, &ink),
    };
    place(frame, area, lines);

    // Progress across the top, one segment a card, the way a story shows how
    // many are left.
    let width = area.width.saturating_sub(4).min(COLUMN) as usize;
    let n = all.len();
    let seg = (width.saturating_sub(n - 1) / n).max(1);
    let mut bar = Vec::new();
    for i in 0..n {
        let (glyph, style) = match i <= at {
            true => ("━", ink.accent()),
            false => ("─", ink.dim()),
        };
        bar.push(Span::styled(glyph.repeat(seg), style));
        if i + 1 < n {
            bar.push(Span::raw(" "));
        }
    }
    if area.height > 2 {
        let row = Rect::new(area.x, area.y + 1, area.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(bar)).alignment(Alignment::Center),
            row,
        );
    }
    if area.height > 4 {
        let hint = format!(
            "←  →  step  ·  [ ]  year  ·  esc  close        {}/{}",
            at + 1,
            n
        );
        let row = Rect::new(area.x, area.bottom() - 2, area.width, 1);
        frame.render_widget(
            Paragraph::new(Line::styled(hint, ink.dim())).alignment(Alignment::Center),
            row,
        );
    }
}

/// `lines` in the card's column, centred on the screen.
fn place(frame: &mut Frame, area: Rect, lines: Vec<Line<'static>>) {
    let width = area.width.saturating_sub(4).min(COLUMN);
    let height = (lines.len() as u16).min(area.height.saturating_sub(4));
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height.saturating_sub(height) / 2).max(2.min(area.height));
    let rect = Rect::new(x, y, width, height).intersection(area);
    frame.render_widget(Paragraph::new(lines), rect);
}

fn center(line: Line<'static>) -> Line<'static> {
    line.alignment(Alignment::Center)
}

fn said(text: impl Into<String>, style: Style) -> Line<'static> {
    center(Line::styled(text.into(), style))
}

/// The small spaced capitals over each card.
fn kicker(text: &str, ink: &Ink) -> Line<'static> {
    let spaced: String = text
        .chars()
        .map(|c| match c {
            ' ' => "   ".to_string(),
            c => format!("{c} "),
        })
        .collect();
    said(spaced.trim_end().to_string(), ink.dim())
}

/// `1 day`, `163 days`, `1,159 sessions`.
fn count(n: usize, one: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{} {one}s", with_commas(n as u64)),
    }
}

fn date(d: chrono::NaiveDate) -> String {
    d.format("%-d %B").to_string()
}

fn intro(w: &Wrapped, ink: &Ink) -> Vec<Line<'static>> {
    let mut out = vec![kicker("CCTOP WRAPPED", ink), Line::default()];
    out.extend(big(&w.year.to_string(), ink.accent()));
    out.extend([
        Line::default(),
        said("Your year with the agents.", ink.bold()),
        Line::default(),
        said(
            format!(
                "{} · {} · {}",
                count(w.days, "day"),
                count(w.sessions, "session"),
                count(w.projects, "project")
            ),
            ink.text(),
        ),
    ]);
    if let Some(since) = w.since {
        let machines = match w.machines {
            0 | 1 => String::new(),
            n => format!(", on {n} machines"),
        };
        out.push(said(format!("from {}{machines}", date(since)), ink.dim()));
    }
    // A machine missing from the year is said here, on the first card, rather
    // than left for the totals to pass as complete.
    for (host, why) in &w.unreachable {
        out.push(said(
            format!("not counted: {host} — {}", truncate(why, 40)),
            ink.dim(),
        ));
    }
    out.extend([
        Line::default(),
        Line::default(),
        said("press → to begin", ink.accent()),
    ]);
    out
}

fn hours(w: &Wrapped, ink: &Ink, reveal: f64) -> Vec<Line<'static>> {
    let mut out = vec![
        kicker("TIME TOGETHER", ink),
        Line::default(),
        said("You spent", ink.text()),
        Line::default(),
    ];
    out.extend(big(&figure(w.hours as f64 * reveal), ink.accent()));
    out.extend([
        Line::default(),
        said(
            match w.hours {
                1 => "hour with an agent at work",
                _ => "hours with an agent at work",
            },
            ink.bold(),
        ),
        Line::default(),
        said(
            format!(
                "That's {:.1} forty-hour weeks, across {}.",
                w.hours as f64 / 40.0,
                count(w.days, "day")
            ),
            ink.text(),
        ),
    ]);
    if let (n, Some(end)) = w.streak
        && n > 1
    {
        out.push(Line::default());
        out.push(said(
            format!("Longest streak: {n} days in a row, to {}.", date(end)),
            ink.accent(),
        ));
    }
    out
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn months(w: &Wrapped, ink: &Ink) -> Vec<Line<'static>> {
    let peak = w.peak_month();
    let name = peak.map_or("—", |m| {
        [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ][m]
    });
    let mut out = vec![
        kicker("MONTH BY MONTH", ink),
        Line::default(),
        said(format!("{name} was your biggest month."), ink.bold()),
        Line::default(),
    ];
    out.extend(bars(&w.by_month, 8, 3, peak, ink));
    out.push(center(Line::styled(MONTHS.join(" "), ink.dim())));
    if let Some((day, n)) = w.busiest_day {
        out.push(Line::default());
        out.push(said(
            format!(
                "Your longest day: {}, {} with agents.",
                day.format("%a %-d %B"),
                count(n, "hour")
            ),
            ink.text(),
        ));
    }
    out
}

/// Who someone is, by the hour they work most.
fn rhythm(hour: usize) -> &'static str {
    match hour {
        5..=8 => "an early bird",
        9..=11 => "a morning person",
        12..=17 => "an afternoon builder",
        18..=21 => "an evening coder",
        _ => "a night owl",
    }
}

fn clock(w: &Wrapped, ink: &Ink) -> Vec<Line<'static>> {
    let Some(peak) = w.peak_hour() else {
        return vec![kicker("YOUR RHYTHM", ink)];
    };
    let mut out = vec![
        kicker("YOUR RHYTHM", ink),
        Line::default(),
        said(format!("You're {}.", rhythm(peak)), ink.bold()),
        Line::default(),
    ];
    out.extend(big(&format!("{peak:02}:00"), ink.accent()));
    out.extend([
        Line::default(),
        said("is when you and your agents were busiest", ink.text()),
        Line::default(),
    ]);
    out.extend(bars(&w.by_hour, 6, 1, Some(peak), ink));
    let mut axis = String::new();
    for h in (0..24).step_by(6) {
        axis.push_str(&format!("{:<12}", format!("{h}h")));
    }
    out.push(center(Line::styled(
        format!("{:<47}", axis.trim_end()),
        ink.dim(),
    )));
    out
}

fn spend(w: &Wrapped, ink: &Ink, reveal: f64) -> Vec<Line<'static>> {
    let mut out = vec![
        kicker("AT LIST PRICE", ink),
        Line::default(),
        said("Your agents went through", ink.text()),
        Line::default(),
    ];
    out.extend(big(&format!("${}", figure(w.cost * reveal)), ink.accent()));
    out.extend([
        Line::default(),
        said("of tokens, at API prices", ink.bold()),
        Line::default(),
        said(
            format!(
                "{} tokens · {} tool calls",
                compact_tokens(w.tokens),
                with_commas(w.calls)
            ),
            ink.text(),
        ),
    ]);
    if let Some((title, usd)) = &w.biggest {
        out.push(said(
            format!("Priciest session: {} — ${usd:.0}", truncate(title, 36)),
            ink.text(),
        ));
    }
    if w.cleaned_up()
        && let Some(since) = w.claude_since
    {
        out.push(Line::default());
        out.push(said(
            format!(
                "Claude Code's from {}: its oldest transcript left.",
                date(since)
            ),
            ink.dim(),
        ));
    }
    out
}

fn models(w: &Wrapped, ink: &Ink) -> Vec<Line<'static>> {
    let top = w.models.first().map_or("", |m| m.0.as_str());
    let mut out = vec![
        kicker("TOP MODELS", ink),
        Line::default(),
        said(format!("You and {top} were inseparable."), ink.bold()),
        Line::default(),
    ];
    let rows: Vec<(String, f64, String)> = w
        .models
        .iter()
        .map(|(m, usd)| (m.clone(), *usd, format!("${usd:.0}")))
        .collect();
    out.extend(ranked(&rows, ink, true));
    out
}

fn projects(w: &Wrapped, ink: &Ink) -> Vec<Line<'static>> {
    let top = w.top_projects.first().map_or("", |p| p.0.as_str());
    let mut out = vec![
        kicker("TOP PROJECTS", ink),
        Line::default(),
        said(format!("{top} had most of your hours."), ink.bold()),
        said(
            format!("{} in all.", count(w.projects, "project")),
            ink.text(),
        ),
        Line::default(),
    ];
    let rows: Vec<(String, f64, String)> = w
        .top_projects
        .iter()
        .map(|(p, h)| (p.clone(), *h as f64, format!("{h}h")))
        .collect();
    out.extend(ranked(&rows, ink, false));
    out
}

/// A title for someone whose sessions were mostly `task`, and a line on it.
fn persona(task: Task) -> (&'static str, &'static str) {
    match task {
        Task::Coding => ("The Builder", "Most of your sessions shipped code."),
        Task::Debugging => ("The Bug Hunter", "You went where things were broken."),
        Task::Testing => ("The Tester", "You made sure it worked."),
        Task::Exploration => ("The Explorer", "You read before you wrote."),
        Task::Planning => ("The Architect", "You planned before you built."),
        Task::Delegation => ("The Conductor", "You had agents running agents."),
        Task::Git => ("The Gardener", "You kept the history tidy."),
        Task::Build => ("The Mechanic", "You kept the build green."),
        Task::Conversation => ("The Thinker", "You talked it through."),
        Task::General => ("The All-Rounder", "A bit of everything."),
    }
}

fn work(w: &Wrapped, ink: &Ink, reveal: f64) -> Vec<Line<'static>> {
    let (title, line) = w.tasks.first().map_or(("", ""), |t| persona(t.0));
    let mut out = vec![
        kicker("HOW YOU WORK", ink),
        Line::default(),
        said(format!("You're {title}."), ink.bold()),
        said(line, ink.text()),
        Line::default(),
    ];
    out.extend(big(&figure(w.files_edited as f64 * reveal), ink.accent()));
    out.push(Line::default());
    let first_time = match w.files_edited {
        0 => String::new(),
        n => format!(
            " — {:.0}% right the first time",
            100.0 * w.files_one_shot as f64 / n as f64
        ),
    };
    out.push(said(format!("files edited{first_time}"), ink.text()));
    out.push(Line::default());
    let rows: Vec<(String, f64, String)> = w
        .tasks
        .iter()
        .map(|(t, n)| (t.as_str().to_string(), *n as f64, format!("{n}")))
        .collect();
    out.extend(ranked(&rows, ink, false));
    out
}

fn commands(w: &Wrapped, ink: &Ink, reveal: f64) -> Vec<Line<'static>> {
    let top = w.commands.first().map_or("", |c| c.0.as_str());
    let mut out = vec![
        kicker("ON REPEAT", ink),
        Line::default(),
        said(format!("Your most-typed command was {top}."), ink.bold()),
        Line::default(),
    ];
    out.extend(big(&figure(w.prompts as f64 * reveal), ink.accent()));
    out.extend([
        Line::default(),
        said("prompts typed into Claude Code", ink.text()),
        Line::default(),
    ]);
    let rows: Vec<(String, f64, String)> = w
        .commands
        .iter()
        .map(|(c, n)| (c.clone(), *n as f64, format!("×{n}")))
        .collect();
    out.extend(ranked(&rows, ink, false));
    out
}

/// Said only when Claude Code's transcripts start well after its history:
/// why its spend begins late, and the one line that keeps next year's whole.
fn keep(w: &Wrapped, ink: &Ink) -> Vec<Line<'static>> {
    let from = w.claude_since.map_or("nowhere".into(), date);
    vec![
        kicker("ONE MORE THING", ink),
        Line::default(),
        said(
            format!("Claude Code's spend only goes back to {from}."),
            ink.bold(),
        ),
        Line::default(),
        said(
            "Claude Code deletes old transcripts — after 30 days,",
            ink.text(),
        ),
        said(
            "unless told otherwise — so its spend, models and files",
            ink.text(),
        ),
        said(
            "start there. Its prompt history is kept, and the",
            ink.text(),
        ),
        said("hours, days and projects here come from it.", ink.text()),
        Line::default(),
        said("To keep all of next year, put this in", ink.text()),
        said("~/.claude/settings.json:", ink.text()),
        Line::default(),
        said("\"cleanupPeriodDays\": 3650", ink.accent()),
    ]
}

/// The one to keep: everything on a single card.
fn summary(w: &Wrapped, ink: &Ink) -> Vec<Line<'static>> {
    let half = (COLUMN / 2) as usize;
    let pair = |left: Line<'static>, right: Line<'static>| -> Line<'static> {
        let pad = half.saturating_sub(left.width());
        let mut spans = left.spans;
        spans.push(Span::raw(" ".repeat(pad)));
        spans.extend(right.spans);
        Line::from(spans)
    };
    let heading = |text: &str| Line::styled(text.to_string(), ink.dim());
    let entry = |i: usize, name: &str| {
        Line::from(vec![
            Span::styled(format!("{} ", i + 1), ink.accent()),
            Span::styled(truncate(name, half - 4), ink.bold()),
        ])
    };

    let mut out = vec![
        kicker(&format!("MY {} WRAPPED", w.year), ink),
        Line::default(),
        pair(heading("Top models"), heading("Top projects")),
    ];
    for i in 0..3 {
        let model = w.models.get(i).map(|m| entry(i, &m.0)).unwrap_or_default();
        let project = w
            .top_projects
            .get(i)
            .map(|p| entry(i, &p.0))
            .unwrap_or_default();
        out.push(pair(model, project));
    }
    let figure_line = |text: String| Line::styled(text, ink.accent());
    out.extend([
        Line::default(),
        pair(heading("Hours with agents"), heading("Days")),
        pair(
            figure_line(with_commas(w.hours as u64)),
            figure_line(w.days.to_string()),
        ),
        Line::default(),
        pair(heading("Sessions"), heading("At list price")),
        pair(
            figure_line(with_commas(w.sessions as u64)),
            figure_line(match w.cost > 0.0 {
                true => format!("${}", with_commas(w.cost.round() as u64)),
                false => "—".into(),
            }),
        ),
        Line::default(),
        pair(heading("Rhythm"), heading("Style")),
        pair(
            Line::styled(w.peak_hour().map_or("—", rhythm).to_string(), ink.bold()),
            Line::styled(
                w.tasks.first().map_or("—", |t| persona(t.0).0).to_string(),
                ink.bold(),
            ),
        ),
        Line::default(),
        said(format!("cctop wrapped · {}", w.year), ink.dim()),
    ]);
    // Left-aligned as a block, and the block centred: the two columns only
    // line up if every row starts at the same place.
    let pad = (COLUMN as usize).saturating_sub(half * 2) / 2 + 4;
    out.into_iter()
        .map(|l| match l.alignment {
            Some(_) => l,
            None => {
                let mut spans = vec![Span::raw(" ".repeat(pad))];
                spans.extend(l.spans);
                Line::from(spans)
            }
        })
        .collect()
}

fn empty(w: &Wrapped, ink: &Ink) -> Vec<Line<'static>> {
    vec![
        kicker("CCTOP WRAPPED", ink),
        Line::default(),
        said(
            format!("Nothing to look back on in {} yet.", w.year),
            ink.bold(),
        ),
        Line::default(),
        said(
            "Neither the transcripts on this machine nor Claude",
            ink.text(),
        ),
        said(
            "Code's prompt history has anything from that year.",
            ink.text(),
        ),
        Line::default(),
        said("[ for the year before", ink.dim()),
    ]
}

/// Ranked rows: place, name, a bar to scale against the first, and a label.
/// `models` colours each bar as the table colours that model.
fn ranked(rows: &[(String, f64, String)], ink: &Ink, models: bool) -> Vec<Line<'static>> {
    const NAME: usize = 18;
    const BAR: usize = 24;
    let max = rows.first().map_or(1.0, |r| r.1).max(f64::MIN_POSITIVE);
    rows.iter()
        .enumerate()
        .map(|(i, (name, value, label))| {
            let fill = ((value / max) * BAR as f64).round().max(1.0) as usize;
            let bar_style = match (models, ink.colour) {
                (true, Some(_)) => Style::default().fg(theme::model_color(name)),
                _ => match i {
                    0 => ink.accent(),
                    _ => ink.text(),
                },
            };
            center(Line::from(vec![
                Span::styled(format!("{:>2}  ", i + 1), ink.accent()),
                // A cell short of the column, so a name cut at its width
                // still has a gap before its bar.
                Span::styled(format!("{:<NAME$}", truncate(name, NAME - 1)), ink.bold()),
                Span::styled("█".repeat(fill.min(BAR)), bar_style),
                Span::raw(" ".repeat(BAR - fill.min(BAR))),
                Span::styled(format!(" {label:>6}"), ink.text()),
            ]))
        })
        .collect()
}

/// A bar chart `height` rows tall, in eighths of a row, one `width`-wide bar
/// per value with a space between, `peak` picked out.
fn bars(
    values: &[u32],
    height: usize,
    width: usize,
    peak: Option<usize>,
    ink: &Ink,
) -> Vec<Line<'static>> {
    const EIGHTHS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
    let max = values.iter().copied().max().unwrap_or(0).max(1) as f64;
    (0..height)
        .map(|row| {
            let floor = (height - 1 - row) * 8;
            let mut spans = Vec::new();
            for (i, v) in values.iter().enumerate() {
                let total = (*v as f64 / max * (height * 8) as f64).round() as usize;
                // A month with any work shows at least a sliver.
                let total = match *v > 0 {
                    true => total.max(1),
                    false => 0,
                };
                let level = total.saturating_sub(floor).min(8);
                let style = match Some(i) == peak {
                    true => ink.accent(),
                    false => ink.text(),
                };
                spans.push(Span::styled(EIGHTHS[level].repeat(width), style));
                if i + 1 < values.len() {
                    spans.push(Span::raw(" "));
                }
            }
            center(Line::from(spans))
        })
        .collect()
}

/// A headline figure, short enough to set large: `1234`, `12.3k`, `4.5M`.
fn figure(value: f64) -> String {
    match value {
        v if v < 10_000.0 => format!("{v:.0}"),
        v if v < 1_000_000.0 => format!("{:.1}k", v / 1e3),
        v => format!("{:.1}M", v / 1e6),
    }
}

/// `text` three rows tall in block glyphs, centred.
///
/// Only what a headline figure is made of; anything else is drawn as a gap
/// its width rather than guessed at.
fn big(text: &str, style: Style) -> Vec<Line<'static>> {
    let glyph = |c: char| -> [&'static str; 3] {
        match c {
            '0' => ["█▀█", "█ █", "▀▀▀"],
            '1' => ["▀█ ", " █ ", "▀▀▀"],
            '2' => ["▀▀█", "█▀▀", "▀▀▀"],
            '3' => ["▀▀█", " ▀█", "▀▀▀"],
            '4' => ["█ █", "▀▀█", "  ▀"],
            '5' => ["█▀▀", "▀▀█", "▀▀▀"],
            '6' => ["█▀▀", "█▀█", "▀▀▀"],
            '7' => ["▀▀█", "  █", "  ▀"],
            '8' => ["█▀█", "█▀█", "▀▀▀"],
            '9' => ["█▀█", "▀▀█", "▀▀▀"],
            '$' => ["▄█▀", "▀█▄", "▀▀ "],
            '.' => [" ", " ", "▀"],
            ':' => [" ", "▀", "▀"],
            'k' => ["█  ", "█▄▀", "▀ ▀"],
            'M' => ["█▄ ▄█", "█ ▀ █", "▀   ▀"],
            _ => ["   ", "   ", "   "],
        }
    };
    (0..3)
        .map(|row| {
            let text: Vec<&str> = text.chars().map(|c| glyph(c)[row]).collect();
            center(Line::styled(text.join(" "), style))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_figure_stays_short_enough_to_set_large() {
        assert_eq!(figure(0.0), "0");
        assert_eq!(figure(9_999.4), "9999");
        assert_eq!(figure(12_345.0), "12.3k");
        assert_eq!(figure(4_500_000.0), "4.5M");
    }

    /// A year with no spend has no spend card to show a zero on, and one
    /// with nothing at all is a single card that says so.
    #[test]
    fn a_card_with_nothing_to_say_is_left_out() {
        let empty = Wrapped::default();
        assert_eq!(cards(&empty), vec![Card::Empty]);

        let hours_only = Wrapped {
            since: chrono::NaiveDate::from_ymd_opt(2026, 1, 2),
            ..Default::default()
        };
        assert_eq!(
            cards(&hours_only),
            vec![
                Card::Intro,
                Card::Hours,
                Card::Months,
                Card::Clock,
                Card::Projects,
                Card::Summary
            ]
        );
    }

    /// On a 256-colour terminal every card still has its own colour: a
    /// ground that snapped to grey would make the recap one grey card after
    /// another.
    #[test]
    fn every_card_keeps_its_hue_without_truecolor() {
        use ratatui::style::Color;
        let all = [
            Card::Intro,
            Card::Hours,
            Card::Months,
            Card::Clock,
            Card::Spend,
            Card::Models,
            Card::Projects,
            Card::Work,
            Card::Commands,
            Card::Keep,
            Card::Summary,
            Card::Empty,
        ];
        for card in all {
            let Color::Indexed(i) = snap(flat(card.hue()), false) else {
                panic!("{card:?}: not an index");
            };
            let cube = i.checked_sub(16).filter(|c| *c < 216);
            let grey = cube.is_none_or(|c| c / 36 == c / 6 % 6 && c / 6 % 6 == c % 6);
            assert!(!grey, "{card:?} snapped to grey {i}");
        }
    }
}
