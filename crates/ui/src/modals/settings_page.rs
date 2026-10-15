//! The settings page: every setting at its value, a cursor to change it
//! and a filter to find it.

use super::*;

/// The settings page: every setting, every view choice and every dashboard key
/// at the value it has now, marked where `config.toml` is what set it, with a
/// cursor to change them in place and a filter to find them with.
///
/// A page rather than the 96-column sheet it replaced, because it outgrew one:
/// the settings alone are more rows than a short terminal has, and the view
/// choices and the keybinds were never going to fit either. Being a tab is what
/// gave it the whole frame below the bar, so the row count costs scrolling
/// instead of costing a subset of the knobs.
pub(crate) fn draw_settings(
    frame: &mut Frame,
    area: Rect,
    app: &mut App,
    layout: &mut super::render::Layout,
) {
    let rows = app.settings_shown();
    let shown = rows.len();
    let total = app.settings_items().len();
    let cursor = app.settings_cursor.min(shown.saturating_sub(1));
    // The width the rows are laid out in, so each can be cut to fit rather than
    // wrapped. Passed in rather than measured here because the block has not
    // been built yet, and its border is two of those columns.
    let width = area.width.saturating_sub(2) as usize;
    let (lines, cursor_line, item_lines) = settings_lines(app, &rows, cursor, width);
    let height = lines.len() as u16;

    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .style(theme::canvas())
        .title(Line::from(Span::styled(" Settings ", theme::title())));
    if area.width >= 40 {
        // Only where there is room for both: a 30-column terminal showing the
        // title and a half a filter is worse than the hint on its own.
        block = block.title(
            Line::from(Span::styled(settings_filter_border(app), theme::dim())).right_aligned(),
        );
    }
    let inner = block.inner(area);
    let max_scroll = height.saturating_sub(inner.height);
    // Followed before the page is drawn, not after. Correcting it afterwards
    // drew every frame at the previous key's offset: the cursor walked off the
    // bottom and the page caught up one keypress late, or never, if nothing
    // else asked for a frame.
    let scroll =
        settings_follow(app.settings_scroll, cursor, cursor_line, inner.height).min(max_scroll);
    // The renderer is the only place that knows how tall the page turned out,
    // and it is what an `End` key needs in order to have a ceiling.
    app.settings_max_scroll = max_scroll;
    app.settings_scroll = scroll;
    // The rows a pointer can land on, recorded while they are laid out: the
    // page scrolls inside its block, and which item a screen row shows is
    // arithmetic only this pass knows. Cctop holds the terminal's mouse
    // capture, so a click here is answered here or nowhere.
    layout.settings_inner = Some(inner);
    layout.settings_rows = item_lines
        .into_iter()
        .filter_map(|(line, at)| {
            let on_screen = line.checked_sub(scroll)?;
            (on_screen < inner.height).then_some((inner.y + on_screen, at))
        })
        .collect();
    if max_scroll > 0 {
        // On the border, so it costs no content line and cannot scroll away.
        let count = match app.settings_filter.is_empty() {
            true => format!("{total}"),
            false => format!("{shown} of {total}"),
        };
        block = block.title_bottom(Line::from(Span::styled(
            format!(" {count}   ↑↓ scroll   / filter   Esc back "),
            theme::dim(),
        )));
    }
    frame.render_widget(block, area);
    // No wrapping, deliberately. `settings_lines` returns a line index for the
    // cursor's row and the scroll is corrected against it, which is only true
    // while one line of text is one line on screen: a wrapped line takes two
    // rows and every row below it is out by one. So the rows are cut to fit
    // when they are built, and this is the backstop for the two that are not
    // ours to size — the config path and a problem cctop did not write.
    frame.render_widget(
        Paragraph::new(lines)
            .scroll((scroll, 0))
            .style(theme::canvas()),
        inner,
    );
    super::scrollbar::on_border(
        frame,
        area,
        height as usize,
        inner.height as usize,
        scroll as usize,
    );
}

/// The offset that keeps the cursor's line on screen, moving as little as it
/// can from where the page already is.
///
/// The first row pulls the page right back to the top, because above it are the
/// file it writes to and the first heading, and `Home` landing one line short of
/// them hides exactly what says where a change will go.
fn settings_follow(scroll: u16, cursor: usize, cursor_line: u16, room: u16) -> u16 {
    if cursor == 0 || room == 0 {
        0
    } else if cursor_line < scroll {
        cursor_line
    } else if cursor_line >= scroll + room {
        cursor_line + 1 - room
    } else {
        scroll
    }
}

/// The filter, on the border, and the hint for it while it is empty.
///
/// On the border rather than as the first row, for the reason the help's is: a
/// filter that scrolls off the top of the page it is filtering is worse than no
/// filter at all, because it is still narrowing the rows and no longer says so.
fn settings_filter_border(app: &App) -> String {
    if app.settings_typing {
        let accent = Style::default().fg(theme::colors().accent);
        let text: String = app
            .settings_filter
            .spans(40, accent, accent, "█")
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        return format!(" {text} ");
    }
    match app.settings_filter.is_empty() {
        true => " / filter ".to_string(),
        false => format!(" /{} ", app.settings_filter),
    }
}

/// What a value on the settings page is, which is what colours it.
///
/// Decided where the value comes from rather than from its text, because the
/// text does not say: a `[settings]` entry takes its type from the schema (the
/// parser for bools, the shape of the default for number-versus-string), a view
/// choice from its `View` variant, a tunnel row from its `Shape`, and a keybind
/// is neither config-typed nor cycled. A bool carries its state because both
/// channels read it — the hue, green against red, and the checkbox glyph the
/// row draws in place of the word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Bool(bool),
    Cycle,
    Number,
    Text,
}

/// The page's rows, grouped under the three places a value can be written, and
/// the line the cursor's row ended up on.
///
/// The groups are the point. `config.toml` and `ui-prefs.json` are different
/// files with different rules — one is hand-written and shared, the other is
/// cctop's own and rewritten on every change — and a page showing them as one
/// undifferentiated list would be asking somebody to change a number without
/// telling them where it lands.
/// Columns a name is padded to, so that every row's description starts in the
/// same place whatever the name happens to be.
const NAME_W: usize = 18;

/// Columns a value is padded to, for the same reason.
const VALUE_W: usize = 14;

/// What the filter marks its match with: a modifier, so it is still a mark
/// where a hue would be nothing — the mono theme, and any terminal that owns
/// the palette. Underlined as well as bold because the mono theme already draws
/// a whole row in bold, and one of two shades of the same weight is not a
/// distinction.
const MATCHED: Modifier = Modifier::BOLD.union(Modifier::UNDERLINED);

/// What a row's value is, as far as drawing it goes.
///
/// A key is a cap because a key is a cap everywhere else in cctop — the footer
/// hints and the mode line both draw bindings as keys, and this page was the
/// one place a binding read as prose. A choice carries the mark that says Enter
/// turns it, because it is the only kind of row where Enter's effect is
/// otherwise invisible: a flag says on or off, a number opens a field, a
/// keybind says it is waiting, and only a choice changes without saying so.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plain,
    Key,
    Cycles,
}

/// Where `needle` first matches `text` without regard to case, as byte offsets
/// into `text`.
///
/// Walked a character at a time rather than by lowercasing both and slicing the
/// original, because lowercasing can change a string's length in bytes — an
/// offset taken from the lowered string and applied to the original cuts a
/// character in half. An empty needle returns `None`: it matches everywhere, so
/// it has nothing to point at.
fn find_fold(text: &str, needle: &str) -> Option<(usize, usize)> {
    let want: Vec<char> = needle.chars().collect();
    if want.is_empty() {
        return None;
    }
    let at: Vec<(usize, char)> = text.char_indices().collect();
    let last = at.len().checked_sub(want.len() - 1)?;
    (0..last)
        .find(|start| {
            at[*start..*start + want.len()]
                .iter()
                .zip(&want)
                .all(|((_, got), w)| got.to_lowercase().eq(w.to_lowercase()))
        })
        .map(|start| {
            let stop = at.get(start + want.len()).map_or(text.len(), |(i, _)| *i);
            (at[start].0, stop)
        })
}

/// A row's text, split so the part the filter matched can carry a mark.
///
/// The filter searches the name, the default, the description and the value,
/// and only the name and the value are on the screen — so a row that matched
/// somewhere else comes back unmarked rather than second-guessed. Marking the
/// description as well would mean the elision and the mark agreeing about where
/// the cut fell, which buys a column that moves the moment a filter narrows the
/// page. The name and the value are what the row is; that is enough to see.
fn marked(text: &str, needle: &str, base: Style) -> Vec<Span<'static>> {
    let Some((at, end)) = find_fold(text, needle) else {
        return vec![Span::styled(text.to_string(), base)];
    };
    vec![
        Span::styled(text[..at].to_string(), base),
        Span::styled(text[at..end].to_string(), base.add_modifier(MATCHED)),
        Span::styled(text[end..].to_string(), base),
    ]
}

/// A row's gutter, its name and the padding that holds the columns, with the
/// filter's match marked on the name.
///
/// The padding is a raw span of its own rather than part of the marked text, so
/// a needle can never be drawn into the space between two names.
fn name_spans(name: &str, needle: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(4);
    spans.push(Span::raw("  ".to_string()));
    spans.extend(marked(name, needle, Style::default()));
    spans.push(Span::raw(
        " ".repeat(NAME_W.saturating_sub(name.chars().count())),
    ));
    spans
}

fn settings_lines(
    app: &App,
    rows: &[super::settings::Item],
    cursor: usize,
    width: usize,
) -> (Vec<Line<'static>>, u16, Vec<(u16, usize)>) {
    use super::settings::{Item, VIEWS, View};
    // Columns a row spends before its description: the gutter, the name, the
    // value, and the set-marker.
    const LEAD: usize = 2 + NAME_W + VALUE_W + 3;
    let section = |t: &str| Line::from(Span::styled(t.to_string(), theme::title()));
    let mut lines = Vec::new();
    let mut cursor_line = 0u16;
    // `(line index, item index)` for the lines that are a row of the list,
    // which is what turns a screen position back into the item drawn there —
    // the headings and the file line are lines with no item behind them.
    let mut item_lines = Vec::new();

    // Cut rather than wrapped, for the reason given on the Paragraph. A config
    // path is as long as somebody's home directory made it, and a problem
    // carries whatever the TOML parser had to say.
    lines.push(Line::from(vec![
        Span::styled("  File ", theme::dim()),
        Span::raw(util::truncate(
            &cctop_core::util::tildify(&cctop_core::config::CONFIG_FILE.to_string_lossy()),
            width.saturating_sub(8),
        )),
    ]));
    for problem in &app.settings.problems {
        lines.push(Line::from(Span::styled(
            util::truncate(&format!("  ! {problem}"), width),
            theme::failed(),
        )));
    }

    let accent = Style::default().fg(theme::colors().accent);
    // The filter's needle, folded once for the whole page. `settings_shown`
    // decides what survives with this same string; the mark says which part of
    // the row is why.
    let needle = app.settings_filter.to_string().trim().to_lowercase();
    // A value may claim the frame's spare room, but never a description's. A
    // tunnel row's value is a hostname, and at eighty columns a long one and
    // its description do not both fit: the value is cut first. The floor is
    // what a tunnel row's description needs to stay a phrase rather than a
    // stub — and descriptions were already elided to whatever room was left
    // over, so this only stops a long value making that worse.
    const DESC_MIN: usize = 24;
    let max_value = VALUE_W.saturating_add(width.saturating_sub(LEAD + DESC_MIN));
    // The value column's width, which a section may widen for itself: the
    // tunnel's values are hostnames, and a column that fits them keeps its
    // descriptions in one column rather than each pushed out by its own.
    let value_w = std::cell::Cell::new(VALUE_W);
    // One row, in every section. `at` is the position in the *filtered* list,
    // which is what the cursor counts in, so a filter that moved the page
    // moves the cursor with it.
    let mut row = |lines: &mut Vec<Line<'static>>,
                   at: usize,
                   name: &str,
                   ty: ValueKind,
                   value: String,
                   set: bool,
                   what: &str,
                   tone: Style,
                   kind: Kind| {
        let here = at == cursor;
        // The type's own ink, and for a set value a wash beside it. Bold stays
        // what it was — the wash adds a second reading of "not the default"
        // without taking the first away.
        let style = {
            let pal = theme::colors();
            let ink = match ty {
                ValueKind::Bool(on) => match on {
                    true => pal.bool_on,
                    false => pal.bool_off,
                },
                ValueKind::Cycle => pal.cycle_hue,
                ValueKind::Number => pal.number_hue,
                ValueKind::Text => pal.text_hue,
            };
            match set {
                true => Style::default()
                    .fg(ink)
                    .add_modifier(Modifier::BOLD)
                    .patch(theme::edited()),
                false => Style::default().fg(ink),
            }
        };
        // A bool is a checkbox rather than the word: `[x]` and `[ ]` are three
        // cells of ASCII, which every terminal draws at one cell each. Emoji
        // would say the same thing at a width no terminal agrees on, and the
        // word was the only thing a reader without colour had.
        let text = match ty {
            ValueKind::Bool(on) => match on {
                true => "[x]".to_string(),
                false => "[ ]".to_string(),
            },
            ValueKind::Cycle | ValueKind::Number | ValueKind::Text => value,
        };
        // Padded out to the value column from whatever was drawn, so the marker
        // and the description do not shift when a value is longer than its cap.
        // The padding wears the value's style, so a set value's wash fills the
        // column — except under a key, whose cap must not read as fourteen wide.
        let pad_style = match kind {
            Kind::Key => Style::default(),
            Kind::Plain | Kind::Cycles => style,
        };
        let pad_to = |spans: &mut Vec<Span<'static>>, width: usize| {
            let used: usize = spans.iter().map(Span::width).sum();
            spans.push(Span::styled(
                " ".repeat(width.saturating_sub(used)),
                pad_style,
            ));
        };
        // Three ways a row can be mid-edit, in the order they can happen: a key
        // being bound, a number typed, or nothing at all.
        let value = if here && app.settings_capture {
            // The waiting state is a cap of its own, in the amber the tabs use
            // when a session needs you — this is the one row on the page that
            // is waiting on the person in front of it. Deliberately not blinked:
            // the blink phase runs off the wall clock, so no frame of it could
            // be a snapshot, and a row that moves is a row nobody can read
            // while it waits for them.
            vec![Span::styled(
                "press a key… (Esc cancels)".to_string(),
                theme::attention_lit(theme::colors().cost_mid),
            )]
        } else if here && let Some(input) = &app.settings_input {
            // A token is dots from the first character: a paste is seen to
            // land whole, and nothing on screen can be read off it.
            let mut spans = match app.cf.masked() {
                true => vec![
                    Span::styled("•".repeat(input.chars().count().min(24)), style),
                    Span::styled("█".to_string(), accent),
                ],
                false => input.spans(usize::MAX, style, accent, "█"),
            };
            // Padded back out to the column the other rows fill, so the marker
            // and the description do not shift as the value is typed. Styled
            // like the text above: a set value keeps its wash while edited.
            pad_to(&mut spans, value_w.get());
            spans
        } else {
            // A key's cap covers the key and nothing else — capping the padding
            // too would read as a fourteen-column key. A choice's mark takes the
            // value field's last column rather than sitting after it, so that
            // saying "Enter cycles this" does not move the description sideways.
            // A choice gives its mark the last of the columns it may claim, so
            // the mark is never the thing that gets cut.
            let cut = match kind {
                Kind::Cycles => max_value.saturating_sub(1),
                _ => max_value,
            };
            let shown = util::truncate(&text, cut);
            let mut spans = match kind {
                Kind::Plain => marked(&shown, &needle, style),
                Kind::Key => marked(&shown, &needle, theme::key_cap()),
                Kind::Cycles => marked(&shown, &needle, style),
            };
            pad_to(
                &mut spans,
                match kind {
                    Kind::Cycles => value_w.get() - 1,
                    _ => value_w.get(),
                },
            );
            if kind == Kind::Cycles {
                spans.push(Span::styled("↻".to_string(), theme::dim()));
            }
            spans
        };
        // The description is cut to the room left over rather than wrapped onto
        // a second line. A wrap is the more generous-looking choice and reads
        // far worse: a 80-column terminal strands one word on its own line for
        // half the rows, so the eye ends up scanning a shape rather than a
        // list, and the cursor's row becomes two rows tall — which breaks the
        // arithmetic this page scrolls by.
        //
        // A value wider than `VALUE_W` — or a section's wider column — is
        // charged its own overflow, so that a tunnel row can carry a whole
        // hostname without pushing the description off the frame. Every other
        // row pads to exactly `VALUE_W`, so their room — and so every existing
        // row — is untouched by this.
        let drawn: usize = value.iter().map(Span::width).sum();
        let room = width.saturating_sub(LEAD + drawn.saturating_sub(VALUE_W));
        let mut line = Line::from_iter(name_spans(name, &needle).into_iter().chain(value).chain([
            Span::styled(if set { " * " } else { "   " }, theme::dim()),
            Span::styled(util::truncate(what, room), tone),
        ]));
        if here {
            cursor_line = lines.len() as u16;
            line = line.style(theme::selected());
        }
        item_lines.push((lines.len() as u16, at));
        lines.push(line);
    };

    // A heading with nothing under it is a heading about nothing, which is
    // what a filter that left out one whole section would otherwise leave
    // behind.
    let wanted = |f: fn(&Item) -> bool| rows.iter().any(f);
    if wanted(|r| matches!(r, Item::Setting(_))) {
        lines.push(Line::default());
        lines.push(section("config.toml · [settings]"));
        for (at, entry) in rows.iter().enumerate() {
            let Item::Setting(i) = entry else { continue };
            let (name, default, what) = cctop_core::settings::SETTINGS[*i];
            let (value, set) = app.settings.value_of(name);
            // The schema decides the type: a name with a fixed list of
            // `choices` turns under Enter like a view choice, so it carries the
            // same mark and drops the quotes that would make it read as a field
            // to type into; the parser is what knows a bool (`is_toggle`), a
            // default the file would spell as a number makes the row numeric,
            // and anything else is a string to read.
            let (kind, mark) = if cctop_core::settings::choices(name).is_some() {
                (ValueKind::Cycle, Kind::Cycles)
            } else if cctop_core::settings::is_toggle(name) {
                (ValueKind::Bool(value == "true"), Kind::Plain)
            } else if default.parse::<f64>().is_ok() {
                (ValueKind::Number, Kind::Plain)
            } else {
                (ValueKind::Text, Kind::Plain)
            };
            let value = match kind {
                ValueKind::Cycle => value.trim_matches('"').to_string(),
                _ => value,
            };
            // Each set drawn in itself rather than described: a font without
            // octants shows boxes here, before they are chosen, rather than
            // across every chart after.
            let previews;
            let what = match name {
                "graph_glyphs" => {
                    previews = crate::spark::Glyphs::ALL
                        .map(|g| format!("{} {}", g.name(), crate::spark::preview(g)))
                        .join("  ");
                    previews.as_str()
                }
                _ => what,
            };
            row(
                &mut lines,
                at,
                name,
                kind,
                value,
                set,
                what,
                theme::dim(),
                mark,
            );
        }
    }

    if wanted(|r| matches!(r, Item::View(_))) {
        lines.push(Line::default());
        lines.push(section("Remembered · view"));
        for (at, entry) in rows.iter().enumerate() {
            let Item::View(i) = entry else { continue };
            let (name, what, view) = &VIEWS[*i];
            let value = app.view_value(view);
            let set = app.view_is_set(view);
            // The variant is the type: a flag flips, a choice turns, a number
            // is typed. Only a choice turns quietly under Enter, so only it
            // carries the mark; the flag and the number say on screen what they
            // are about to do.
            let (ty, kind) = match view {
                View::Flag { value: get, .. } => (ValueKind::Bool(get(app)), Kind::Plain),
                View::Choice { .. } => (ValueKind::Cycle, Kind::Cycles),
                View::Number { .. } => (ValueKind::Number, Kind::Plain),
            };
            row(
                &mut lines,
                at,
                name,
                ty,
                value,
                set,
                what,
                theme::dim(),
                kind,
            );
        }
    }

    if wanted(|r| matches!(r, Item::Tunnel(_))) {
        use crate::connect::{Shape, Tone};
        lines.push(Line::default());
        lines.push(section("config.toml · [tunnel]"));
        let widest = rows
            .iter()
            .filter_map(|r| match r {
                Item::Tunnel(t) => Some(app.tunnel_view(*t).value.chars().count() + 2),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        value_w.set(widest.clamp(VALUE_W, max_value.max(VALUE_W)));
        for (at, entry) in rows.iter().enumerate() {
            let Item::Tunnel(tunnel) = entry else {
                continue;
            };
            let shown = app.tunnel_view(*tunnel);
            let (ty, kind) = match shown.shape {
                Shape::Text => (ValueKind::Text, Kind::Plain),
                Shape::Bool(on) => (ValueKind::Bool(on), Kind::Plain),
                Shape::Cycle => (ValueKind::Cycle, Kind::Cycles),
            };
            let tone = match shown.tone {
                Tone::Dim => theme::dim(),
                Tone::Ask => Style::default().fg(theme::colors().cost_mid),
                Tone::Error => theme::failed(),
            };
            row(
                &mut lines,
                at,
                tunnel.name(),
                ty,
                shown.value,
                shown.set,
                &shown.what,
                tone,
                kind,
            );
        }
        value_w.set(VALUE_W);
    }

    if wanted(|r| matches!(r, Item::Key(_))) {
        lines.push(Line::default());
        lines.push(section("config.toml · [keys], on the session table"));
        for (at, entry) in rows.iter().enumerate() {
            let Item::Key(i) = entry else { continue };
            let (action, default, what) = cctop_core::settings::BINDINGS[*i];
            let key = app.settings.key_for(action).to_string();
            let set = key != *default;
            row(
                &mut lines,
                at,
                action,
                ValueKind::Text,
                key,
                set,
                what,
                theme::dim(),
                Kind::Key,
            );
        }
    }

    // A filter that matched nothing has to say so, or a blank page is a hang.
    if rows.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            util::truncate(
                &format!("  Nothing matches {}.", app.settings_filter),
                width,
            ),
            theme::dim(),
        )));
        lines.push(Line::from(Span::styled(
            "  / clears it. Esc goes back to the table.",
            theme::dim(),
        )));
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        util::truncate(
            "  ↵ change   ⌫ reset   e open the file in an editor   Esc back to the table",
            width,
        ),
        theme::dim(),
    )));
    (lines, cursor_line, item_lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frame a key asks for shows where that key put the cursor. The page
    /// used to correct its scroll after drawing, so `End` drew the top of the
    /// page with the cursor somewhere below it, and only the next keypress's
    /// frame caught up.
    #[test]
    fn the_settings_page_shows_the_cursor_in_the_frame_that_moved_it() {
        use crate::settings::Item;
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
        app.settings_open = true;
        let mut terminal = Terminal::new(TestBackend::new(100, 15)).expect("backend");
        let mut frame_text = |app: &mut App| {
            terminal
                .draw(|frame| draw_settings(frame, frame.area(), app, &mut Layout::default()))
                .expect("draw");
            let buffer = terminal.backend().buffer();
            buffer
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect::<String>()
        };

        let rows = app.settings_shown();
        let Some(Item::Key(last)) = rows.last() else {
            panic!("the page no longer ends on a keybind")
        };
        let last = cctop_core::settings::BINDINGS[*last].0;
        app.settings_cursor = rows.len() - 1;
        let text = frame_text(&mut app);
        assert!(text.contains(last), "End did not bring {last} on screen");
        assert!(!text.contains("File "), "and the top is still drawn");

        app.settings_cursor = 0;
        let text = frame_text(&mut app);
        assert!(text.contains("File "), "Home did not go back to the top");
    }

    /// The spans of the row the cursor is on that begin inside a column range,
    /// so that a style can be asked about without going through a terminal.
    fn column(app: &App, from: usize, len: usize) -> Vec<(String, Style)> {
        let rows = app.settings_shown();
        let cursor = app.settings_cursor.min(rows.len().saturating_sub(1));
        let (lines, at, _) = super::settings_lines(app, &rows, cursor, 100);
        let mut col = 0;
        let mut out = Vec::new();
        for span in &lines[at as usize].spans {
            let start = col;
            col += span.width();
            if start >= from && start < from + len {
                out.push((span.content.to_string(), span.style));
            }
        }
        out
    }

    /// The value column, as drawn.
    fn value(app: &App) -> Vec<(String, Style)> {
        column(app, 2 + NAME_W, VALUE_W)
    }

    /// The name column, as drawn.
    fn name(app: &App) -> Vec<(String, Style)> {
        column(app, 2, NAME_W)
    }

    /// What the filter marked, put back together.
    fn hits(spans: &[(String, Style)]) -> String {
        spans
            .iter()
            .filter(|(_, s)| s.add_modifier.contains(MATCHED))
            .map(|(t, _)| t.as_str())
            .collect()
    }

    fn drawn_width(spans: &[(String, Style)]) -> usize {
        spans.iter().map(|(t, _)| t.chars().count()).sum()
    }

    /// Put the cursor on the first row of a kind, which is where the page
    /// already put it when nothing has moved it.
    fn at_row(app: &mut App, want: fn(&crate::settings::Item) -> bool) {
        app.settings_cursor = app
            .settings_shown()
            .iter()
            .position(want)
            .unwrap_or_else(|| panic!("the page has no row of that kind"));
    }

    /// A binding is a cap everywhere else in cctop, and this page was the one
    /// place it read as prose. The cap covers the key and nothing else —
    /// padding it would read as a fourteen-column key.
    #[test]
    fn a_keybind_is_a_cap_covering_only_the_key() {
        let mut app = crate::tests::test_app();
        app.settings_open = true;
        at_row(&mut app, |i| matches!(i, crate::settings::Item::Key(_)));
        let value = value(&app);
        assert_eq!(value[0].1, theme::key_cap(), "not a cap: {value:?}");
        assert!(
            value[1..]
                .iter()
                .all(|(t, s)| t.chars().all(|c| c == ' ') && *s == Style::default()),
            "the padding carried the cap too: {value:?}"
        );
        assert_eq!(drawn_width(&value), VALUE_W, "the column moved: {value:?}");
    }

    /// Enter turns a choice and says nothing while it does. It is the only row
    /// on the page whose effect is invisible, so it is the only one that needs
    /// the mark — and the mark takes the value column's last cell rather than
    /// sitting after it, so the description does not move sideways.
    #[test]
    fn a_choice_says_enter_cycles_it_without_moving_the_column() {
        use crate::settings::{VIEWS, View};
        let mut app = crate::tests::test_app();
        app.settings_open = true;
        at_row(&mut app, |i| {
            matches!(i, crate::settings::Item::View(v)
                if matches!(VIEWS[*v].2, View::Choice { .. }))
        });
        let value = value(&app);
        assert_eq!(
            value.last().map(|(t, _)| t.as_str()),
            Some("↻"),
            "no cycle mark on a choice: {value:?}"
        );
        assert_eq!(drawn_width(&value), VALUE_W, "the column moved: {value:?}");
    }

    /// A `[settings]` name with a fixed list turns under Enter just as a view
    /// choice does, so it owes the reader the same mark, and no quotes that
    /// would make it look like a field to type into.
    #[test]
    fn a_settings_choice_carries_the_cycle_mark_without_quotes() {
        let mut app = crate::tests::test_app();
        app.settings_open = true;
        at_row(&mut app, |i| {
            matches!(i, crate::settings::Item::Setting(s)
                if cctop_core::settings::SETTINGS[*s].0 == "graph_glyphs")
        });
        let value = value(&app);
        assert_eq!(
            value.last().map(|(t, _)| t.as_str()),
            Some("↻"),
            "no cycle mark on graph_glyphs: {value:?}"
        );
        assert!(
            value.iter().all(|(t, _)| !t.contains('"')),
            "a choice drawn as a quoted string: {value:?}"
        );
        assert_eq!(drawn_width(&value), VALUE_W, "the column moved: {value:?}");
    }

    /// A filter that matched something should say which part of the row is
    /// why, so the page can be read rather than scanned.
    #[test]
    fn a_filter_marks_exactly_the_text_that_matched() {
        let mut app = crate::tests::test_app();
        app.settings_open = true;
        app.settings_filter = "alert_cost".into();
        app.settings_cursor = 0;
        let name = name(&app);
        assert_eq!(
            hits(&name),
            "alert_cost",
            "the name was not marked: {name:?}"
        );
        assert_eq!(
            name.iter()
                .map(|(t, _)| t.as_str())
                .collect::<String>()
                .trim(),
            "alert_cost",
            "marking moved the text"
        );
        assert_eq!(drawn_width(&name), NAME_W, "the column moved: {name:?}");
    }

    /// The filter searches the description too, and a row that matched only
    /// there is drawn unmarked rather than second-guessed: the description is
    /// elided, and a mark that guessed at the cut would move the column.
    #[test]
    fn a_row_that_matched_only_in_its_description_is_drawn_unmarked() {
        let mut app = crate::tests::test_app();
        app.settings_open = true;
        app.settings_filter = "alert when".into();
        app.settings_cursor = 0;
        let marked = !hits(&name(&app)).is_empty() || !hits(&value(&app)).is_empty();
        assert!(
            !marked,
            "a row that matched nothing on screen was still marked"
        );
    }

    /// The needle is found case-insensitively, and never by an offset taken
    /// from a lowered copy of the text: `İ` is two bytes and lowers to three,
    /// so the byte index of everything after it moves.
    #[test]
    fn the_match_never_splits_a_character() {
        assert_eq!(super::find_fold("Alert When", "alert when"), Some((0, 10)));
        assert_eq!(super::find_fold("kÜber", "über"), Some((1, 6)));
        assert_eq!(super::find_fold("İx", "x"), Some((2, 3)));
        // The same answer is (3, 4) if you lowercase the haystack first and take
        // the offset from that, which would slice the last byte off the `x`.
        assert_eq!("İx".to_lowercase().find('x'), Some(3));
        assert_eq!(
            super::find_fold("abc", ""),
            None,
            "an empty needle matches all"
        );
        assert_eq!(super::find_fold("ab", "abc"), None);
    }

    /// A tunnel row's value is a hostname, wider than its column: it is
    /// charged its own overflow, so the description stays on an 80-column
    /// frame.
    #[test]
    fn a_long_value_leaves_the_description_on_the_frame() {
        use crate::connect::{Connected, Row};
        let mut app = crate::tests::test_app();
        app.settings_open = true;
        app.connected = Some(Connected::from(&cctop_core::tunnel::Account {
            token: "made-up".into(),
            hostname: Some("cctop-a-long-name.example.com".into()),
            ..Default::default()
        }));
        at_row(&mut app, |i| {
            *i == crate::settings::Item::Tunnel(Row::Hostname)
        });
        let rows = app.settings_shown();
        let cursor = app.settings_cursor;
        let (lines, at, _) = super::settings_lines(&app, &rows, cursor, 80);
        let line = &lines[at as usize];
        let drawn = line.spans.iter().map(|s| s.width()).sum::<usize>();
        assert!(drawn <= 80, "the row ran off the frame: {line:?}");
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(
            text.contains("cctop-a-long-name.example.com"),
            "the hostname was cut: {line:?}"
        );
        assert!(
            text.contains("dashboard address"),
            "the description left the frame: {line:?}"
        );
    }

    /// A token being typed is dots, never the token.
    #[test]
    fn a_token_field_is_drawn_as_dots() {
        use crate::connect::Row;
        let mut app = crate::tests::test_app();
        app.settings_open = true;
        at_row(&mut app, |i| {
            *i == crate::settings::Item::Tunnel(Row::Token)
        });
        app.settings_activate();
        app.settings_input = Some("made-up-secret".into());
        let value = value(&app);
        let text: String = value.iter().map(|(t, _)| t.as_str()).collect();
        assert!(!text.contains("made-up"), "{text}");
        assert!(text.starts_with(&"•".repeat(14)), "{text}");
    }

    /// Each value type in its own ink, the wash a set value wears, and a bool
    /// as a checkbox — in every palette the page can be drawn in: dark, light,
    /// and no colour at all.
    ///
    /// The glyph replaced the word, so the column arithmetic is asserted with
    /// it: `[x]` is three cells where `true` was four, and a glyph that did not
    /// pad back out to the value column would shift the marker and the
    /// description under it on every row at once.
    #[test]
    fn each_value_type_is_painted_and_the_columns_hold() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let start = theme::variant();
        let _restore = theme::tests::Restore(start);

        let mut app = crate::tests::test_app();
        // One bool and one number off their defaults, so the wash has something
        // to sit behind; the rest of the page is defaults.
        app.settings.notify = Some(true);
        app.settings.idle_after = Some(3.0);

        // The shape a palette change must not disturb: the row from just inside
        // the gutter to the end of the value column, text only.
        let mut shape = String::new();
        for named in ["dark", "light", "mono"] {
            theme::set_theme(Some(named));
            let mut terminal = Terminal::new(TestBackend::new(100, 50)).expect("backend");
            terminal
                .draw(|frame| draw_settings(frame, frame.area(), &mut app, &mut Layout::default()))
                .expect("draw");
            let buffer = terminal.backend().buffer();
            let pal = theme::colors();
            // The block's border is column 0, so the row's two-space gutter
            // puts the name at 3 and the value — `  {name:<18}` — at 21, with
            // the three-cell marker at 35 and the description at 38.
            let row = |name: &str| {
                (0..buffer.area.height)
                    .find(|&y| {
                        (3u16..21)
                            .map(|x| buffer[(x, y)].symbol())
                            .collect::<String>()
                            .starts_with(name)
                    })
                    .unwrap_or_else(|| panic!("{named}: no row for {name}"))
            };

            // A true bool at its default's opposite: the glyph, the green, and
            // the wash of a value that is not the default.
            let y = row("notify");
            let value: String = (21u16..35).map(|x| buffer[(x, y)].symbol()).collect();
            assert_eq!(value, format!("[x]{}", " ".repeat(11)), "{named}");
            let cell = &buffer[(21, y)];
            assert_eq!(cell.fg, pal.bool_on, "{named}");
            // The marker and the description are where the word left them:
            // this value is set, so the marker cell carries its `*`.
            let marker: String = (35u16..38).map(|x| buffer[(x, y)].symbol()).collect();
            assert_eq!(marker, " * ", "{named}: the marker column moved");
            assert_eq!(buffer[(38, y)].symbol(), "B", "{named}: descriptions");
            match theme::no_color() {
                true => assert!(
                    cell.modifier.contains(Modifier::UNDERLINED),
                    "mono: a set value lost its second channel"
                ),
                false => assert_eq!(cell.bg, pal.edited_bg, "{named}: no wash"),
            }

            // A false bool, still on its default: red, and on the ground
            // rather than on the wash.
            let y = row("warn_agents");
            assert_eq!(buffer[(21, y)].symbol(), "[", "{named}");
            let cell = &buffer[(21, y)];
            assert_eq!(cell.fg, pal.bool_off, "{named}");
            assert_eq!(cell.bg, pal.ground, "{named}: a default wore the wash");

            // The other three types, one hue each.
            assert_eq!(
                buffer[(21, row("compact_threshold"))].fg,
                pal.number_hue,
                "{named}"
            );
            assert_eq!(
                buffer[(21, row("hide_columns"))].fg,
                pal.text_hue,
                "{named}"
            );
            assert_eq!(
                buffer[(21, row("bottom_panel"))].fg,
                pal.cycle_hue,
                "{named}"
            );
            // A number off its default wears the wash under its own ink.
            assert_eq!(buffer[(21, row("idle_after"))].bg, pal.edited_bg, "{named}");

            // Same page, same shape, whatever the palette: mono may only change
            // how it is inked, never where anything sits.
            let line: String = (1u16..38).map(|x| buffer[(x, y)].symbol()).collect();
            if shape.is_empty() {
                shape = line;
            } else {
                assert_eq!(shape, line, "{named}: the row moved");
            }
        }
    }

    /// The picker row draws every swatch, brackets the pick, and spells its
    /// name — under NO_COLOR the name is the only thing there is to read.
    #[test]
    fn the_colour_row_brackets_the_pick_and_names_it() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = crate::tests::test_app();
        app.rename_was = "claude-4".to_string();
        app.rename_color = Some(theme::Hue::Cyan);

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("backend");
        let mut layout = crate::render::Layout::default();
        terminal
            .draw(|frame| draw_rename_tab(frame, frame.area(), &app, &mut layout))
            .expect("draw");
        let buf = terminal.backend().buffer();
        let text = buf
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(
            text.contains("cyan"),
            "the pick's name is not spelled: {text}"
        );
        assert!(text.contains("[●]"), "the pick is not bracketed: {text}");
        assert!(text.contains('○'), "the none stop is missing: {text}");
        // The picked swatch carries the hue's colour; under the default
        // palette that is an indexed colour, not the terminal's default ink.
        let cyan = buf
            .content()
            .iter()
            .find(|cell| cell.symbol() == "●" && cell.fg == theme::Hue::Cyan.color());
        assert!(cyan.is_some(), "no swatch wears the picked hue");
    }
}
