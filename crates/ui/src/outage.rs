//! Whether a provider outage explains the sessions failing on screen: the
//! footer's line, and the panel `!` opens with the vendor's own account.
//!
//! What the pages say and how it is matched to sessions is
//! [`cctop_core::provider_status`]; this is where it is polled and drawn.

use super::theme;
use super::{App, Mode};
use cctop_core::provider_status::{self as ps, Alert, Level, Page, PageStatus};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// What the pages last said, and the panel's own state.
#[derive(Default)]
pub struct Outage {
    pub status: ps::Status,
    /// Set to ask the poller for an immediate recheck. A flag rather than a
    /// channel: the poller has nothing else to hear, and a set it misses is
    /// picked up on its next tick.
    pub recheck: Arc<AtomicBool>,
    pub scroll: u16,
    /// The panel's largest useful scroll, recorded during draw.
    pub max_scroll: u16,
}

/// Start the shared poller, its answers arriving as worker responses.
///
/// The loop and its pacing are core's, so a standalone `cctop serve` asks the
/// vendor on exactly the same schedule; see [`ps::poll`].
pub(super) fn spawn_poller(
    tx: std::sync::mpsc::Sender<super::worker::Response>,
    recheck: Arc<AtomicBool>,
) {
    ps::spawn_poller(recheck, move |page, status| {
        tx.send(super::worker::Response::ProviderStatus(
            page,
            Box::new(status),
        ))
        .is_ok()
    });
}

impl App {
    /// What the pages say about the sessions on screen, most pressing first.
    pub(super) fn provider_alerts(&self) -> Vec<Alert> {
        ps::alerts(
            &self.outage.status,
            &ps::erroring_by_page(self.sessions.iter()),
        )
    }

    /// Open the panel, and ask for fresh figures while it is read: it is opened
    /// precisely when the cached ones are being doubted.
    pub(super) fn open_provider_status(&mut self) {
        self.outage.recheck.store(true, Ordering::Relaxed);
        self.outage.scroll = 0;
        self.mode = Mode::ProviderStatus;
        self.needs_redraw = true;
    }

    /// Read-only, so every key scrolls, rechecks, or closes it.
    pub(super) fn on_key_provider_status(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | '!') => self.mode = Mode::List,
            KeyCode::Char('r') | KeyCode::F(5) => {
                self.outage.recheck.store(true, Ordering::Relaxed);
                self.set_status("Rechecking the status pages…");
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.outage.scroll = (self.outage.scroll + 1).min(self.outage.max_scroll);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.outage.scroll = self.outage.scroll.saturating_sub(1);
            }
            _ => {}
        }
    }
}

/// The footer line, when there is anything to say: core's summary, which the
/// web dashboard shows too, plus the key that opens the panel. See
/// [`ps::summary_line`] for when it is silent.
pub(super) fn footer_line(alerts: &[Alert]) -> Option<(String, Level)> {
    ps::summary_line(alerts).map(|(text, level)| (format!("{text} · !"), level))
}

/// The footer's style for a line at `level`: red for an outage, amber for
/// maintenance and for the "probably local" hint, which is a warning rather than
/// a verdict.
pub(super) fn footer_style(level: Level) -> Style {
    let fg = match level.is_outage() {
        true => theme::colors().cost_high,
        false => theme::colors().cost_mid,
    };
    Style::default().fg(fg).add_modifier(Modifier::BOLD)
}

/// Colour for a severity, reusing the cost ladder rather than inventing a
/// palette entry: green, amber and red already mean fine, watch and bad
/// everywhere else on screen.
fn level_color(level: Level) -> Color {
    match level {
        Level::Operational => theme::colors().cost_low,
        Level::Maintenance | Level::Minor => theme::colors().cost_mid,
        Level::Major | Level::Critical => theme::colors().cost_high,
    }
}

/// "since 13:55 (1h20m)" for an incident's start, in local time.
fn since(started_at: i64, now: i64) -> String {
    let local = chrono::DateTime::from_timestamp(started_at, 0)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_else(|| "?".into());
    match now - started_at {
        elapsed if elapsed > 0 => format!(
            "since {local} ({})",
            cctop_core::util::compact_duration(elapsed * 1000)
        ),
        _ => format!("since {local}"),
    }
}

pub(super) fn draw(frame: &mut Frame, area: Rect, app: &mut App) {
    const WIDTH: u16 = 78;
    // Two for the border, five for the indent the incident lines carry.
    let text_w = WIDTH as usize - 7;

    let now = chrono::Utc::now().timestamp();
    let erroring = ps::erroring_by_page(app.sessions.iter());
    let alerts = ps::alerts(&app.outage.status, &erroring);
    let mut lines: Vec<Line<'static>> = Vec::new();

    // The answer to "is it me?" comes first, before any of the vendor's detail,
    // because on a bad day it is the only line that gets read.
    let verdict = ps::verdict(&alerts, &erroring);
    lines.push(Line::from(Span::styled(
        format!(" {}", verdict.text),
        match verdict.tone {
            ps::Tone::Outage => footer_style(Level::Major),
            ps::Tone::Warning => footer_style(Level::Maintenance),
            ps::Tone::Quiet => theme::dim(),
        },
    )));
    if let Some(detail) = verdict.detail {
        lines.push(Line::from(Span::styled(
            format!("   {detail}"),
            theme::dim(),
        )));
    }

    for page in Page::ALL {
        lines.push(Line::default());
        let mine = erroring
            .iter()
            .find(|(p, _)| *p == page)
            .map_or(0, |(_, n)| *n);
        let yours = match mine {
            0 => String::new(),
            n => format!("   ({n} of yours failing)"),
        };
        let (summary, style) = match app.outage.status.get(page) {
            PageStatus::Pending => ("checking…".to_string(), theme::dim()),
            // Kept apart from an outage: cctop not reaching the page says
            // nothing about the provider, and offline it is the first thing to
            // notice.
            PageStatus::Unavailable(why) => {
                (format!("status page unreachable ({why})"), theme::dim())
            }
            PageStatus::Ok(r) => (
                r.description.clone(),
                Style::default().fg(level_color(r.level)),
            ),
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {:<10}", page.label()), theme::label()),
            Span::styled(summary, style),
            Span::styled(yours, theme::dim()),
        ]));

        let Some(report) = app.outage.status.get(page).report() else {
            continue;
        };
        for incident in &report.incidents {
            lines.push(Line::from(vec![
                Span::styled("   ▸ ", Style::default().fg(level_color(incident.level))),
                Span::styled(
                    cctop_core::util::truncate(&incident.name, text_w),
                    theme::value().add_modifier(Modifier::BOLD),
                ),
            ]));
            let mut meta = vec![incident.stage.clone()];
            if let Some(at) = incident.started_at {
                meta.push(since(at, now));
            }
            meta.retain(|m| !m.is_empty());
            if !meta.is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("     {}", meta.join(" · ")),
                    theme::dim(),
                )));
            }
            // The update body is the "why": the one thing here the red dot in
            // the table could never have said.
            if let Some(update) = &incident.update {
                for line in super::panels::wrap(update, text_w) {
                    lines.push(Line::from(Span::styled(
                        format!("     {line}"),
                        theme::value(),
                    )));
                }
            }
            if !incident.components.is_empty() {
                for line in super::panels::wrap(&incident.components.join(", "), text_w - 6) {
                    lines.push(Line::from(vec![
                        Span::styled("     on ", theme::dim()),
                        Span::styled(line, theme::value()),
                    ]));
                }
            }
        }
        if !report.degraded.is_empty() {
            for line in super::panels::wrap(&report.degraded.join(", "), text_w - 6) {
                lines.push(Line::from(vec![
                    Span::styled("   affected  ", theme::dim()),
                    Span::styled(line, Style::default().fg(theme::colors().cost_mid)),
                ]));
            }
        }
        if report.incidents.is_empty() && report.degraded.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("   nothing open · {}", page.site()),
                theme::dim(),
            )));
        }
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " r recheck now   ↑↓ scroll   Esc close",
        theme::dim(),
    )));

    app.outage.max_scroll = super::modals::scrollable_modal_titled(
        frame,
        area,
        vec![Line::from(Span::styled(
            " Provider status ",
            theme::title(),
        ))],
        lines,
        WIDTH,
        app.outage.scroll,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use cctop_core::provider_status::fixtures;
    use cctop_core::session::ActivityState;

    fn alert(page: Page, erroring: usize, level: Level, headline: &str) -> Alert {
        Alert {
            page,
            erroring,
            level,
            headline: headline.into(),
        }
    }

    #[test]
    fn the_footer_line_says_who_is_to_blame() {
        // Nothing to say: no line at all.
        assert!(footer_line(&[]).is_none());

        let (text, level) = footer_line(&[alert(
            Page::Anthropic,
            0,
            Level::Major,
            "Elevated errors on Claude Opus",
        )])
        .unwrap();
        assert_eq!(
            text,
            "⚠ Anthropic incident: Elevated errors on Claude Opus · !"
        );
        assert_eq!(level, Level::Major);

        let (text, _) = footer_line(&[alert(
            Page::OpenAi,
            2,
            Level::Minor,
            "Increased error rates",
        )])
        .unwrap();
        assert_eq!(
            text,
            "⚠ OpenAI incident: Increased error rates — 2 of yours failing · !"
        );

        let (text, _) = footer_line(&[alert(
            Page::Anthropic,
            3,
            Level::Operational,
            "All Systems Operational",
        )])
        .unwrap();
        assert_eq!(
            text,
            "⚠ 3 failing, Anthropic reports all clear: probably this machine · !"
        );
    }

    fn render(app: &mut App) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 40)).unwrap();
        terminal.draw(|f| draw(f, f.area(), app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn failing_opus() -> cctop_core::session::Session {
        let mut s = crate::tests::session("s1", true, "one");
        s.model = "claude-opus-5".into();
        s.activity_state = ActivityState::ApiError;
        s
    }

    /// The panel exists to answer "is it me?", so the answer is its first line,
    /// and it has to be the right answer in both directions.
    #[test]
    fn the_panel_leads_with_whether_the_provider_is_to_blame() {
        let mut app = crate::tests::test_app();
        app.sessions = vec![failing_opus()];
        app.outage
            .status
            .set(Page::Anthropic, ps::parse(fixtures::MAJOR));

        let screen = render(&mut app);
        assert!(
            screen.contains("1 of your sessions is failing, and Anthropic reports an incident"),
            "{screen}"
        );
        // The vendor's own account: the incident, its latest update, when it
        // started and what it touches.
        assert!(
            screen.contains("Elevated errors on Claude Opus"),
            "{screen}"
        );
        assert!(
            screen.contains("A bad deploy is being rolled back."),
            "{screen}"
        );
        assert!(screen.contains("identified · since"), "{screen}");
        assert!(
            screen.contains("on Claude API (api.anthropic.com), Claude Code"),
            "{screen}"
        );

        // Same failure against a clean page: point the search at this machine.
        app.outage
            .status
            .set(Page::Anthropic, ps::parse(fixtures::OPERATIONAL));
        let screen = render(&mut app);
        assert!(
            screen.contains("but Anthropic reports all clear"),
            "{screen}"
        );
        assert!(screen.contains("suspect this machine"), "{screen}");
    }

    #[test]
    fn offline_the_footer_says_nothing_and_the_panel_says_why() {
        let mut app = crate::tests::test_app();
        app.sessions = vec![failing_opus()];
        for page in Page::ALL {
            app.outage
                .status
                .set(page, PageStatus::Unavailable("connection refused".into()));
        }
        assert!(footer_line(&app.provider_alerts()).is_none());
        let screen = render(&mut app);
        assert!(screen.contains("status page unreachable"), "{screen}");
        assert!(!screen.contains("all clear"), "{screen}");
    }

    #[test]
    fn the_footer_line_comes_and_goes_with_the_incident() {
        let mut app = crate::tests::test_app();
        assert!(footer_line(&app.provider_alerts()).is_none());
        app.outage
            .status
            .set(Page::OpenAi, ps::parse(fixtures::DEGRADED));
        let (text, _) = footer_line(&app.provider_alerts()).unwrap();
        assert!(
            text.contains("OpenAI incident: Increased error rates"),
            "{text}"
        );
        app.outage
            .status
            .set(Page::OpenAi, ps::parse(fixtures::OPERATIONAL));
        assert!(footer_line(&app.provider_alerts()).is_none());
    }

    #[test]
    fn the_dashboard_footer_carries_the_line() {
        let mut app = crate::tests::test_app();
        app.outage
            .status
            .set(Page::Anthropic, ps::parse(fixtures::MAJOR));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(200, 30)).unwrap();
        terminal
            .draw(|f| {
                crate::render::draw(f, &mut app);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let footer: String = (0..buf.area.width)
            .map(|x| buf[(x, buf.area.height - 1)].symbol().to_string())
            .collect();
        assert!(
            footer.contains("Anthropic incident: Elevated errors on Claude Opus"),
            "{footer}"
        );
    }

    #[test]
    fn bang_opens_the_panel_and_asks_for_a_recheck() {
        let mut app = crate::tests::test_app();
        app.on_key(crate::tests::key(KeyCode::Char('!')));
        assert_eq!(app.mode, Mode::ProviderStatus);
        assert!(app.outage.recheck.load(Ordering::Relaxed));
        app.on_key(crate::tests::key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::List);
    }
}
