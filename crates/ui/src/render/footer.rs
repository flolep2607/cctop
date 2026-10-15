//! The footer: the keys that apply now, the badges, and the share corner.

use super::*;

/// One key hint, and how hard it fights for a place on the footer.
///
/// The footer used to be a fixed list of twelve, which meant a narrow terminal
/// lost whichever three happened to be last and a wide one still never showed
/// `Enter`. Ordering the hints by what they earn, and fitting them to the
/// width that is actually left, fixes both: the row menu is always there and
/// the speed control is there only when nothing better wants the space.
struct Hint {
    key: &'static str,
    name: &'static str,
}

impl Hint {
    /// `key` + a space + `name` + a trailing space, matching how it renders.
    fn width(&self) -> usize {
        self.key.chars().count() + self.name.chars().count() + 2
    }

    /// The keystroke this hint stands for, so a click on it can be answered by
    /// pressing that key — one dispatch, and a hint cannot drift from what
    /// clicking it does.
    ///
    /// `None` for the hints that name more than one key: `↑↓`, `Alt+←→` and
    /// `Alt+v/s` are maps of a pair, and a click cannot say which half was
    /// meant. They stay labels, which costs nothing — a mouse already moves the
    /// selection by clicking a row and switches tab by clicking the bar.
    fn event(&self) -> Option<event::KeyEvent> {
        use event::{KeyCode, KeyModifiers as Mods};
        let (code, mods) = match self.key {
            "↑↓" | "Alt+←→" | "Alt+v/s" => return None,
            "↵" => (KeyCode::Enter, Mods::NONE),
            "Esc" => (KeyCode::Esc, Mods::NONE),
            "Tab" => (KeyCode::Tab, Mods::NONE),
            "Space" => (KeyCode::Char(' '), Mods::NONE),
            // Any function key, read off its number: a list of the ones in use
            // left `F9 Image` a label that did nothing when clicked.
            f if f.starts_with('F') && f.len() > 1 => {
                (KeyCode::F(f[1..].parse().ok()?), Mods::NONE)
            }
            // `Alt+n` and friends. The letter is the last character, and the
            // handler for these reads the modifier as well as the code.
            alt if alt.starts_with("Alt+") => {
                let c = alt.chars().next_back()?;
                (KeyCode::Char(c), Mods::ALT)
            }
            // Everything else is the single character it prints: `/`, `?`, `a`,
            // `D`. Uppercase as written, which is what the list handler matches.
            one => match (one.chars().next(), one.chars().count()) {
                (Some(c), 1) => (KeyCode::Char(c), Mods::NONE),
                _ => return None,
            },
        };
        Some(event::KeyEvent::new(code, mods))
    }
}

const fn hint(key: &'static str, name: &'static str) -> Hint {
    Hint { key, name }
}

/// Lay hints out left to right until the next one would not fit, then stop.
///
/// Dropping from the tail rather than truncating mid-word is what makes the
/// priority order mean anything: a half-drawn `Qu` teaches nobody that `q`
/// quits, so the hint that cannot fit whole is simply not shown.
fn fit_hints(hints: &[Hint], room: usize) -> Vec<Span<'static>> {
    fit_hints_at(hints, room, None, 0, 0)
}

/// The hints as spans, recording where each clickable one landed.
///
/// `x` and `row` are where the first span will be drawn, so a hit region is in
/// screen coordinates rather than in offsets a caller would have to add up
/// again. Passing no `hits` draws without recording, which is what the tests
/// and the width arithmetic want.
fn fit_hints_at(
    hints: &[Hint],
    mut room: usize,
    mut hits: Option<&mut Vec<(u16, u16, u16, event::KeyEvent)>>,
    x: u16,
    row: u16,
) -> Vec<Span<'static>> {
    let key_style = theme::key_cap();
    let label_style = Style::default().fg(theme::colors().dim);
    let mut spans = Vec::new();
    let mut at = x;
    for h in hints {
        let w = h.width();
        if w > room {
            break;
        }
        room -= w;
        // The whole chip, key and label together: `↵ Actions` reads as one
        // button, and a region that covered only the `↵` would miss most of
        // what a pointer is aimed at.
        if let Some(hits) = hits.as_deref_mut()
            && let Some(key) = h.event()
        {
            hits.push((row, at, at + w as u16, key));
        }
        at += w as u16;
        spans.push(Span::styled(h.key, key_style));
        spans.push(Span::styled(format!(" {} ", h.name), label_style));
    }
    spans
}

/// The keys a terminal tab keeps while the rest of the keyboard belongs to the
/// focused agent, most-used first.
fn tab_hints() -> Vec<Hint> {
    vec![
        hint("F12", "Dashboard"),
        hint("Alt+←→", "Tabs"),
        hint("Alt+n", "New"),
        hint("Alt+w", "Close"),
        hint("Alt+o", "Focus"),
        hint("F1", "Help"),
        hint("F9", "Image"),
        hint("Alt+v/s", "Split"),
        hint("F10", "Quit"),
        hint("F4", "Overview"),
    ]
}

/// The dashboard's hints for the state it is actually in.
///
/// Context, not a fixed list: `Enter` is worth nothing with an empty table,
/// `Esc` is worth nothing with no filter to clear, and once rows are marked the
/// batch actions matter more than the mark key that is already doing its job.
fn list_hints(app: &App) -> Vec<Hint> {
    let mut hints = vec![hint("↑↓", "Move")];
    if app.selected_session().is_some() {
        // First, and by some distance. Every per-row action is behind it, and
        // it is the one key that teaches the others their letters.
        hints.push(hint("↵", "Actions"));
    }
    hints.push(hint("/", "Filter"));
    if app.idle_only {
        // The view's whole purpose is one key, and it does not mean here what
        // it means elsewhere: it stops the view, not only the marks.
        hints.push(hint(
            "K",
            if app.marked.is_empty() {
                "Stop idle"
            } else {
                "Stop marked"
            },
        ));
        hints.push(hint("Space", "Mark"));
        hints.push(hint("I", "Back"));
    } else if app.marked.is_empty() {
        hints.push(hint("Space", "Mark"));
    } else {
        // Marking is done; what is unobvious now is how to act on the marks.
        hints.push(hint("D", "Delete marked"));
        hints.push(hint("K", "Kill marked"));
        hints.push(hint("U", "Unmark"));
    }
    if app.has_filter() {
        hints.push(hint("Esc", "Clear filter"));
    }
    if app.selected_session().is_some() {
        hints.push(hint("a", "Attach"));
        hints.push(hint("R", "Resume"));
    }
    // Ahead of the rest, because it is the key that teaches the rest: behind
    // them, an 80-column footer never showed it. `t` is also on the tab bar's
    // `+ Tab (t)`, so it is the one that can best afford to go.
    hints.push(hint("?", "Help"));
    hints.push(hint("t", "New tab"));
    hints.push(hint("Tab", "Panel"));
    hints.push(hint("S", "Sort"));
    hints.push(hint("q", "Quit"));
    hints
}

/// The badges: what cctop is doing to the table right now, and anything that
/// wants answering. These take their width before the hints do, because a hint
/// is a thing you could learn from the help and a badge is a fact about this
/// screen that is on show nowhere else.
fn footer_badges(app: &App) -> Vec<Span<'static>> {
    // Each badge names itself so `footer_hide` can leave it off.
    let mut badges: Vec<(&'static str, Span<'static>)> = Vec::new();
    // First, and in the colour the costly things use: it is a question waiting
    // for an answer, and the answer is a second click on the hint beside it.
    if app.quit_arm {
        badges.push((
            "quit",
            Span::styled(
                " click q again to quit ",
                Style::default().fg(theme::colors().cost_high),
            ),
        ));
    }
    if app.idle_only {
        badges.push((
            "idle",
            Span::styled(
                format!(
                    " Idle≥{} ",
                    super::idle::threshold_label(app.settings.idle_after_ms())
                ),
                Style::default().fg(theme::colors().panel_title),
            ),
        ));
    }
    if let Some(age) = app.age_filter {
        badges.push((
            "age",
            Span::styled(
                format!(" Age<{} ", age.short()),
                Style::default().fg(theme::colors().panel_title),
            ),
        ));
    }
    if !app.search.is_empty() {
        // The filter stays on the footer once the modal closes, so it has to
        // say whether transcripts are in scope — the two searches can return
        // very different tables for the same word.
        let scope = match (app.search_content, app.scanning) {
            (false, _) => String::new(),
            (true, true) => " +transcripts…".to_string(),
            (true, false) => format!(" +transcripts({})", app.scan_hits.len()),
        };
        badges.push((
            "filter",
            Span::styled(
                format!(" Filter: {}{scope} ", app.search),
                Style::default().fg(theme::colors().filter_badge),
            ),
        ));
    }
    if app.cost_floor > 0.0 {
        badges.push((
            "cost",
            Span::styled(
                format!(" ≥${:.2} ", app.cost_floor),
                Style::default().fg(theme::colors().cost_high),
            ),
        ));
    }
    if !app.marked.is_empty() {
        badges.push((
            "marked",
            Span::styled(
                format!(" [{} marked] ", app.marked.len()),
                Style::default()
                    .fg(theme::colors().accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ));
    }
    if app.follow {
        badges.push((
            "follow",
            Span::styled(" FOLLOW ", Style::default().fg(theme::colors().cost_mid)),
        ));
    }
    // The user's own line, next to what cctop is doing to the table.
    if let Some(note) = app.footer_extra.text() {
        let note: String = note.chars().take(48).collect();
        badges.push(("note", Span::styled(format!(" {note} "), theme::dim())));
    }
    // Who rang, kept there until you are looking at them. A bell you heard from
    // the next room has to still be answerable when you come back.
    if let Some(bell) = app
        .notify
        .footer(app.selected_session().map(|s| s.key()).as_deref())
    {
        badges.push((
            "bell",
            Span::styled(
                format!(" {bell} "),
                Style::default()
                    .fg(theme::colors().accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ));
    }
    // A dot per machine the table reads: green answering, red not. The rows
    // of a machine that dropped out keep their last reading, so the totals
    // would look complete without it. Why one is down is said when one of its
    // rows is opened.
    for (host, up) in app.remote_presence() {
        let color = match up {
            true => theme::colors().cost_low,
            false => theme::colors().cost_high,
        };
        badges.push((
            "remote",
            Span::styled(format!(" ● {host} "), Style::default().fg(color)),
        ));
    }
    // Two agents writing one file is the only thing here that is a fault rather
    // than a state, so it sits with the bell rather than among the badges.
    if let Some(clash) = app.conflict_footer() {
        badges.push((
            "conflict",
            Span::styled(
                format!(" {clash} "),
                Style::default()
                    .fg(theme::colors().cost_high)
                    .add_modifier(Modifier::BOLD),
            ),
        ));
    }
    // A vendor incident, or sessions failing against a page that reports all
    // clear. With the faults rather than the states, since either one explains
    // something red on screen.
    if let Some((text, level)) = super::outage::footer_line(&app.provider_alerts()) {
        badges.push((
            "provider",
            Span::styled(format!(" {text} "), super::outage::footer_style(level)),
        ));
    }
    if let Some(version) = &app.update_available {
        badges.push((
            "update",
            Span::styled(
                format!(" v{version} available — cctop --update "),
                Style::default()
                    .fg(theme::colors().cost_mid)
                    .add_modifier(Modifier::BOLD),
            ),
        ));
    }
    badges
        .into_iter()
        .filter(|(name, _)| !app.hidden_footer.iter().any(|h| h == name))
        .map(|(_, span)| span)
        .collect()
}

/// What the footer's right-hand corner is showing.
///
/// The corner is one thing in two states, not two things: a tunnel that exists
/// is a link, and a tunnel that does not is the button that would make one.
/// Sharing this machine has never been reachable except through `B` and a
/// second key, which is a feature nobody finds by using cctop.
enum Corner {
    /// A tunnel is up. The label is its host; the link behind it carries the
    /// token, which is what opens the page.
    Link(String, String),
    /// No tunnel, and a click would open one — after a second click, because
    /// the first only arms it. The label says which of the two it is asking
    /// for.
    Button(&'static str),
    /// A tunnel is being registered. Cloudflare's edge takes a second or so to
    /// answer, and a spinner is the difference between a wait and a hang.
    Working(String),
}

impl Corner {
    fn label(&self) -> &str {
        match self {
            Self::Link(label, _) => label,
            Self::Button(label) => label,
            Self::Working(label) => label,
        }
    }
}

/// The corner for the state this cctop is in.
///
/// The link is the tunnel's and not the loopback one: `127.0.0.1:7777` is
/// something anyone can retype, while a quick tunnel's hostname is words
/// Cloudflare picked and changes every run. That is the one worth a click.
///
/// The button is armed by its first click and fires on the second — the same
/// two-step the launcher uses, and for a stronger reason: this one puts every
/// session on this machine behind a URL on the internet, and a stray click is
/// not consent to that. The armed label is where that is said, because it is
/// the sentence being clicked through.
fn share_corner(app: &App) -> Corner {
    // Before the states below, because it outranks all of them: a tunnel being
    // dialled is what the corner is doing, whatever it was showing before.
    if let Some(opening) = &app.share_opening {
        return Corner::Working(format!("{} opening a tunnel…", opening.frame()));
    }
    match app.serving.as_ref().and_then(|s| s.public.as_deref()) {
        Some(url) => Corner::Link(link_label(url), url.to_string()),
        // Armed, nothing served yet: the corner becomes two doors — this
        // machine, or the internet. Two halves, one for each answer.
        None if app.share_arm => Corner::Button("⧉ local  ⧉ worldwide"),
        // Serving already, just not off this machine: the click adds the tunnel
        // to the server that is up rather than starting one.
        None if app.serving.is_some() => Corner::Button("⧉ + tunnel"),
        None => Corner::Button("⧉ share"),
    }
}

/// Columns a share label may take before it is shortened.
///
/// A quick tunnel's hostname is four words and a domain —
/// `tribute-resistance-resolved-moscow.trycloudflare.com` is a real one — which
/// at full length took a quarter of a wide footer and half a narrow one, and
/// pushed the key hints off the row it was borrowing.
const LINK_MAX: usize = 30;

/// `https://abc-def.trycloudflare.com/?t=secret` → `⧉ abc-def.trycloudflare.com`.
///
/// The host and not the query. The token is what opens the page, and the footer
/// is on screen for the whole run — in front of whoever walks past it and in
/// every screenshot that happens to have cctop in it. The link behind the label
/// still carries the token, because a link without it opens nothing.
///
/// Shortened from the middle when the host is long, which keeps both ends that
/// say something: the first words, which are what distinguishes one tunnel from
/// another, and `trycloudflare.com`, which is what says where it goes.
fn link_label(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?']).next().unwrap_or(rest);
    format!("⧉ {}", shorten_host(host))
}

fn shorten_host(host: &str) -> String {
    let chars: Vec<char> = host.chars().collect();
    if chars.len() <= LINK_MAX {
        return host.to_string();
    }
    // The tail is the domain, which is fixed and worth keeping whole; the head
    // takes whatever the ellipsis leaves.
    let tail = "trycloudflare.com";
    let keep = match host.ends_with(tail) {
        true => tail.chars().count(),
        false => LINK_MAX / 2,
    };
    let head = LINK_MAX.saturating_sub(keep + 1);
    let front: String = chars[..head].iter().collect();
    let back: String = chars[chars.len() - keep..].iter().collect();
    format!("{front}…{back}")
}

/// Columns the hints keep for themselves before the share link is offered any.
///
/// Enough for the first three, which is the same floor the badges respect: a
/// footer that has given its width away to a link and cannot say how to quit
/// has the priorities backwards.
const LINK_MIN_HINTS: usize = 26;

/// The footer, with the share link in its right-hand corner when one exists.
///
/// The link takes its columns before the hints and badges do, and is drawn after
/// them, so nothing lands on top of a cell holding an escape sequence.
pub(super) fn draw_footer(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    // `footer_hide = "share"` leaves the whole corner off.
    if app.hidden_footer.iter().any(|h| h == "share") {
        draw_footer_keys(frame, area, app, layout);
        return;
    }
    let corner = share_corner(app);
    let width = corner.label().chars().count();
    // Shown only with a footer's worth of keys still beside it. A column of gap
    // as well, so the corner never abuts the last badge.
    // Two columns of gap: one before it, so the corner never abuts the last
    // badge, and one after, which is the margin below.
    let reserved = match area.width as usize >= width + 2 + LINK_MIN_HINTS {
        true => width as u16 + 2,
        false => 0,
    };
    draw_footer_keys(
        frame,
        Rect {
            width: area.width - reserved,
            ..area
        },
        app,
        layout,
    );
    if reserved == 0 {
        return;
    }
    // One column short of the edge. The bottom-right cell is the one a terminal
    // is least willing to paint — writing it can scroll the screen — and a
    // label that ends there also reads as if it had been cut off.
    let x = area.right() - width as u16 - 1;
    match &corner {
        Corner::Link(label, url) => hyperlink::draw(
            frame.buffer_mut(),
            x,
            area.y,
            label,
            url,
            Style::default().fg(theme::colors().accent),
        ),
        // Amber while armed: the next click is the one that publishes, and the
        // colour cctop uses for "this wants reading" is the honest one for it.
        Corner::Button(label) => {
            // Armed with nothing served: two chips, one per door. The
            // internet's gets the colour that means "this wants reading";
            // the local one is what stays on the machine, so it stays dim.
            let choosing = app.share_arm && app.serving.is_none() && app.share_opening.is_none();
            if choosing {
                const LOCAL: &str = "⧉ local";
                frame
                    .buffer_mut()
                    .set_string(x, area.y, LOCAL, theme::dim());
                let rest: String = label.chars().skip(LOCAL.chars().count()).collect();
                frame.buffer_mut().set_string(
                    x + LOCAL.chars().count() as u16,
                    area.y,
                    rest,
                    Style::default()
                        .fg(theme::colors().cost_mid)
                        .add_modifier(Modifier::BOLD),
                );
                layout.share_local = Some((area.y, x, x + LOCAL.chars().count() as u16));
            } else {
                let style = match app.share_arm {
                    true => Style::default().fg(theme::colors().cost_mid),
                    false => theme::dim(),
                };
                frame.buffer_mut().set_string(x, area.y, label, style);
            }
        }
        Corner::Working(label) => {
            frame.buffer_mut().set_string(
                x,
                area.y,
                label,
                Style::default().fg(theme::colors().cost_mid),
            );
        }
    }
    layout.share_corner = Some((area.y, x, x + width as u16));
}

/// The footer's key hints, which are always there.
///
/// Messages used to take this row over for a few seconds, which cost the keys
/// they covered and was lost to the next message anyway. They are toasts now
/// — see [`super::toast`] — and the footer stays a legend.
/// The settings page's hints, which are the page's own rather than the
/// dashboard's or a terminal's.
///
/// Its own, because a page that showed the dashboard's keys would be promising
/// a selection and filters that are not on this screen, and showing a
/// terminal's would be promising keys for a pty that is not behind it. The
/// one thing a page must never do is describe somebody else's screen.
fn settings_hints() -> Vec<Hint> {
    vec![
        hint("/", "Filter"),
        hint("↵", "Change"),
        hint("⌫", "Reset"),
        hint("e", "Edit file"),
        hint(",", "Back"),
        hint("Alt+←→", "Tabs"),
        hint("F1", "Help"),
        hint("F10", "Quit"),
    ]
}

fn draw_footer_keys(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let total = area.width as usize;
    let (hints, badges) = if app.on_settings() {
        (settings_hints(), Vec::new())
    } else if app.tab > 0 {
        // A terminal tab has no dashboard selection, filters or panels to act
        // on, so it has no badges either — just the compact map of the keys
        // cctop keeps.
        (tab_hints(), Vec::new())
    } else {
        (list_hints(app), footer_badges(app))
    };
    // `footer_hide` names get left off the hints too, spelled as the label
    // shows them — "sort" hides `S Sort`.
    let hints: Vec<Hint> = hints
        .into_iter()
        .filter(|h| {
            !app.hidden_footer
                .iter()
                .any(|hide| hide == &h.name.to_lowercase())
        })
        .collect();

    let badge_w: usize = badges.iter().map(|s| s.content.chars().count()).sum();
    // The badges never squeeze the hints below the first few: a footer that is
    // all state and no keys is what the terminal is for. Past that the badges
    // win, and the tail hints are the ones that go.
    let floor = hints.iter().take(3).map(Hint::width).sum::<usize>();
    let room = total.saturating_sub(badge_w).max(floor.min(total));

    // Only while the dashboard is what the keys act on. A hint clicked with a
    // modal up would be dispatched into that modal's handler — the `?` under a
    // search box types a question mark into the query — and the footer is drawn
    // beneath every modal, where it is a legend rather than a row of buttons.
    let mut spans = match app.mode {
        Mode::List => fit_hints_at(&hints, room, Some(&mut layout.key_hits), area.x, area.y),
        _ => fit_hints(&hints, room),
    };
    spans.extend(badges);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;

    /// The label is what is on screen and the token is not, but the link a
    /// click follows still carries it.
    #[test]
    fn the_share_label_shows_the_host_and_the_link_keeps_the_token() {
        let url = "https://few-words-here.trycloudflare.com/?t=secret";
        assert_eq!(link_label(url), "⧉ few-words-he…trycloudflare.com");
        // A short host is left alone; a long one keeps its first words and its
        // domain, and never grows past the width the footer set aside.
        assert_eq!(link_label("http://127.0.0.1:7777/?t=x"), "⧉ 127.0.0.1:7777");
        let long = link_label("https://tribute-resistance-resolved-moscow.trycloudflare.com/?t=x");
        assert!(long.starts_with("⧉ tribute"), "{long:?}");
        assert!(long.ends_with("trycloudflare.com"), "{long:?}");
        assert!(long.chars().count() <= LINK_MAX + 2, "{long:?}");

        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 1));
        hyperlink::draw(&mut buf, 2, 0, &link_label(url), url, Style::default());
        let opening = buf.cell((2, 0)).unwrap();
        assert_eq!(hyperlink::target_of(opening.symbol()), Some(url));
        assert!(opening.symbol().ends_with("\x1b]8;;\x1b\\"));
    }

    /// One cell carries the escape sequences, and the columns it paints over
    /// hold the plain label — so the diff spends nothing on them while the link
    /// is up, and knows to erase them once it is gone.
    #[test]
    fn the_share_link_costs_one_cell_and_still_erases_cleanly() {
        let url = "https://few-words-here.trycloudflare.com/?t=secret";
        let label = link_label(url);

        let mut linked = Buffer::empty(Rect::new(0, 0, 40, 1));
        hyperlink::draw(&mut linked, 2, 0, &label, url, Style::default());
        // The columns the label covers hold its characters, not its escapes.
        assert_eq!(linked.cell((3, 0)).unwrap().symbol(), " ");
        assert_eq!(linked.cell((4, 0)).unwrap().symbol(), "f");

        // Drawing it twice writes nothing: the forced width keeps the covered
        // columns out of the diff entirely.
        let mut again = Buffer::empty(Rect::new(0, 0, 40, 1));
        hyperlink::draw(&mut again, 2, 0, &label, url, Style::default());
        assert!(linked.diff(&again).is_empty());

        // And when the tunnel goes, every column it painted is erased rather
        // than left holding half a hostname — the label's own blank column
        // too, since a blank the terminal printed inside the link still opens it.
        let blank = Buffer::empty(Rect::new(0, 0, 40, 1));
        let erased: Vec<u16> = linked.diff(&blank).iter().map(|(x, _, _)| *x).collect();
        for (i, ch) in label.chars().enumerate() {
            let x = 2 + i as u16;
            assert!(
                erased.contains(&x),
                "{ch} at column {x} survived: {erased:?}"
            );
        }
    }

    /// The corner is drawn in the corner: last row, hard against the right
    /// edge, and hit-testable where it was drawn.
    #[test]
    fn the_share_button_sits_in_the_bottom_right_corner() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        let (cols, rows) = (80u16, 24u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let mut layout = Layout::default();
        terminal
            .draw(|frame| layout = draw(frame, &mut app))
            .expect("draw");

        let buffer = terminal.backend().buffer().clone();
        let footer: String = (0..cols).map(|x| buffer[(x, rows - 1)].symbol()).collect();
        assert!(
            footer.ends_with("⧉ share "),
            "the share button is not in the corner: {footer:?}"
        );
        // The keys it took the columns from are still there beside it.
        assert!(footer.contains("Quit"), "{footer:?}");
        let (row, a, b) = layout.share_corner.expect("no share hit region");
        // One column short of the edge, and the hit region covers the label
        // rather than the margin beside it.
        assert_eq!((row, b), (rows - 1, cols - 1));
        assert!(layout.share_corner_at(a, row));
        assert!(layout.share_corner_at(b - 1, row));
        assert!(!layout.share_corner_at(b, row));
        assert!(!layout.share_corner_at(a - 1, row));
    }

    /// With no tunnel the corner is the button that would open one, and it
    /// says so twice: the first click only arms it.
    #[test]
    fn the_share_corner_asks_before_it_publishes() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());

        assert!(app.serving.is_none());
        assert_eq!(share_corner(&app).label(), "⧉ share");

        app.share_arm = true;
        let armed = share_corner(&app);
        assert_eq!(armed.label(), "⧉ local  ⧉ worldwide");
        // Nothing to open yet, so the corner is a button and not a link: a
        // click on it starts a tunnel rather than a browser.
        assert!(matches!(armed, Corner::Button(_)));
    }

    /// Armed, the corner is two doors and each has to say which machine the
    /// table would land on: local stays here, worldwide is the tunnel.
    #[test]
    fn the_armed_corner_offers_local_and_worldwide() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.share_arm = true;
        let (cols, rows) = (80u16, 24u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let mut layout = Layout::default();
        terminal
            .draw(|frame| layout = draw(frame, &mut app))
            .expect("draw");

        let buffer = terminal.backend().buffer().clone();
        let footer: String = (0..cols).map(|x| buffer[(x, rows - 1)].symbol()).collect();
        assert!(
            footer.contains("⧉ local") && footer.contains("⧉ worldwide"),
            "the two doors are not on the corner: {footer:?}"
        );
        let (row, a, b) = layout.share_local.expect("no local hit region");
        assert!(layout.share_corner_at(a, row));
        assert!(layout.share_local_at(a, row));
        assert!(layout.share_local_at(b - 1, row));
        assert!(!layout.share_local_at(b, row));
        // The worldwide half is on the corner but not the local side.
        let (_, ca, cb) = layout.share_corner.expect("no corner");
        assert!(layout.share_corner_at(cb - 1, row));
        assert!(!layout.share_local_at(cb - 1, row));
        assert!(ca <= a);
    }

    /// `footer_hide` takes a badge or a hint name, and `share` takes the
    /// corner: a footer is the user's, and it has to be shorten-able.
    #[test]
    fn footer_hide_leaves_the_named_items_off() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.follow = true;
        app.hidden_footer = vec!["follow".into(), "sort".into()];
        let badges = footer_badges(&app);
        assert!(!badges.iter().any(|s| s.content.contains("FOLLOW")));

        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let (cols, rows) = (80u16, 24u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let mut layout = Layout::default();
        terminal
            .draw(|frame| layout = draw(frame, &mut app))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let footer: String = (0..cols).map(|x| buffer[(x, rows - 1)].symbol()).collect();
        assert!(
            !footer.contains("Sort"),
            "the hidden hint is on the footer: {footer:?}"
        );
    }

    /// And `footer_hide = "share"` leaves the corner off altogether.
    #[test]
    fn the_share_corner_can_be_hidden() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        app.hidden_footer = vec!["share".into()];
        let (cols, rows) = (80u16, 24u16);
        let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
        let mut layout = Layout::default();
        terminal
            .draw(|frame| layout = draw(frame, &mut app))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let footer: String = (0..cols).map(|x| buffer[(x, rows - 1)].symbol()).collect();
        assert!(
            !footer.contains("⧉"),
            "the corner is on the footer: {footer:?}"
        );
        assert!(layout.share_corner.is_none());
    }

    /// The footer's own line is drawn as a badge, whichever of the two ways
    /// the user supplied it.
    #[test]
    fn the_footer_note_shows_as_a_badge() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        assert!(
            footer_badges(&app)
                .iter()
                .all(|s| !s.content.contains("on call"))
        );
        app.footer_extra.apply(Some("on call"), None);
        assert!(
            footer_badges(&app)
                .iter()
                .any(|s| s.content.contains("on call"))
        );
    }

    /// While the edge is being dialled the corner spins, because the second
    /// that takes used to be a second of a frozen dashboard.
    #[test]
    fn the_share_corner_spins_while_the_tunnel_is_being_opened() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        let (_done_tx, done_rx) = std::sync::mpsc::channel();
        app.share_opening = Some(crate::Opening {
            rx: done_rx,
            since: std::time::Instant::now(),
        });

        let label = share_corner(&app).label().to_string();
        assert!(label.ends_with(" opening a tunnel…"), "{label:?}");
        // A frame of the spinner leads it, and it is one column like the rest.
        assert_eq!(
            label.chars().count(),
            " opening a tunnel…".chars().count() + 1
        );

        // And a click at it while it spins is impatience, not a second tunnel.
        app.on_share_corner(true, false);
        assert!(app.share_opening.is_some());
        assert!(app.serving.is_none());
    }

    /// A hint that does not fit whole is dropped, not cut: half a key cap
    /// teaches the wrong key.
    #[test]
    fn a_narrow_footer_drops_whole_hints_from_the_tail() {
        let hints = [hint("↑↓", "Move"), hint("↵", "Actions"), hint("q", "Quit")];
        // Two spans per hint that fits, so the count says how many made it.
        assert_eq!(fit_hints(&hints, 100).len(), 6);
        // "↑↓ Move " is 8, "↵ Actions " is 10: room for the first, and one
        // column short of the second.
        assert_eq!(fit_hints(&hints, 17).len(), 2);
        assert_eq!(fit_hints(&hints, 18).len(), 4);
        assert!(fit_hints(&hints, 3).is_empty());
    }

    /// Every hint a tab shows that names one key is a button for that key.
    #[test]
    fn every_single_key_tab_hint_can_be_clicked() {
        for h in tab_hints() {
            assert!(
                h.event().is_some() || h.key.contains('/') || h.key.contains('→'),
                "`{} {}` is drawn as a hint but clicking it does nothing",
                h.key,
                h.name
            );
        }
        assert_eq!(
            hint("F9", "Image").event().map(|k| k.code),
            Some(event::KeyCode::F(9))
        );
    }

    /// The footer is a map of the state you are in, not a fixed list. Marking
    /// rows swaps the mark key for the things you can now do with the marks,
    /// and a filter that is on brings `Esc` forward.
    #[test]
    fn the_footer_hints_follow_what_the_dashboard_is_doing() {
        use cctop_core::cache::UiPrefs;
        use cctop_core::pricing::Plan;

        let (tx, _rx) = std::sync::mpsc::channel();
        let mut app = App::with_prefs(Plan::Retail, tx, UiPrefs::default());
        let names =
            |app: &App| -> Vec<&'static str> { list_hints(app).iter().map(|h| h.name).collect() };

        // Nothing selected: no per-row key is offered, because none would work.
        assert!(app.selected_session().is_none());
        let empty = names(&app);
        assert!(!empty.contains(&"Actions"), "{empty:?}");
        assert!(!empty.contains(&"Attach"), "{empty:?}");
        assert!(!empty.contains(&"Clear filter"), "{empty:?}");
        assert!(
            empty.contains(&"Help") && empty.contains(&"Quit"),
            "{empty:?}"
        );

        app.marked.insert("session-key".into());
        let marked = names(&app);
        assert!(!marked.contains(&"Mark"), "{marked:?}");
        assert!(marked.contains(&"Delete marked"), "{marked:?}");
        assert!(marked.contains(&"Unmark"), "{marked:?}");

        app.search = "web".into();
        assert!(names(&app).contains(&"Clear filter"));
    }
}
