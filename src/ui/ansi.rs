//! A tool's output, with the colours it was printed in.
//!
//! cargo, pytest and git colour what they print whenever they think a terminal
//! is watching, and a transcript records the bytes as they came. ratatui drops
//! the ESC of each sequence when it draws — control characters never reach a
//! cell — but not what follows it, so without this a result reads as
//! `[1m[32m   Compiling[0m cctop`, and every wrap is measured over characters
//! that were never meant to take a column.
//!
//! Only SGR (`ESC [ … m`) becomes style. Everything else a program can write is
//! consumed and shown as nothing: cursor movement and erasure, which would have
//! meant something only on the program's own screen; OSC, whose hyperlinks
//! keep their label and lose their target; the DCS/APC string family; and
//! charset shifts. A lone carriage return starts the line over, which is what
//! a progress bar redrawing itself looks like once it has finished.
//!
//! # Why not `ansi-to-tui`
//!
//! It is the obvious crate, and it is on ratatui-core like cctop. It takes an
//! OSC as running to the next BEL, though, and cargo closes its hyperlinks with
//! ST (`ESC \`) — so the text after a linked path went with the link, up to the
//! end of its line. It also leaves tabs, backspaces and charset shifts in the
//! text, and would have brought a second major version of `nom` into the tree.
//! What cctop needs is a small state machine over one SGR vocabulary, and that
//! is this file.

use super::styled;
use super::theme;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// `text` as styled lines, one per line of output. Text no sequence styled is
/// in `base`, and a reset returns to it.
pub(super) fn lines(text: &str, base: Style) -> Vec<Line<'static>> {
    lines_with(text, base, theme::adapt)
}

/// `text` wrapped to `width` with `indent` in front of every row — how the
/// conversation view and the Tool Activity panel lay a result out.
pub(super) fn wrapped(text: &str, base: Style, width: usize, indent: &str) -> Vec<Line<'static>> {
    lines(text, base)
        .iter()
        .flat_map(|line| styled::wrap_line(line, width, indent))
        .collect()
}

/// `text` with every escape sequence and control character taken out, for a
/// one-line cell where colour has no room to mean anything.
pub(super) fn strip(text: &str) -> String {
    lines_with(text, Style::default(), |c| c)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// [`lines`] with the colour fold handed in, so the sixteen-colour path is
/// testable without a terminal that has sixteen colours.
fn lines_with(text: &str, base: Style, adapt: impl Fn(Color) -> Color) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut line: Vec<(char, Style)> = Vec::new();
    let mut style = base;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // Peeked rather than taken, so an ESC with nothing sensible after
            // it costs only itself — never the newline or letter that follows.
            '\x1b' => match chars.peek() {
                Some('[') => {
                    chars.next();
                    csi(&mut chars, &mut style, base, &adapt);
                }
                // OSC, and the strings that end the same way: DCS, SOS, PM, APC.
                Some(']' | 'P' | 'X' | '^' | '_') => {
                    chars.next();
                    string(&mut chars);
                }
                // `ESC ( B` and the like: intermediates, then one final byte.
                Some(' '..='/') => {
                    while chars.next_if(|c| matches!(c, ' '..='/')).is_some() {}
                    chars.next_if(|c| matches!(c, '0'..='~'));
                }
                // `ESC 7`, `ESC M`, `ESC =`: complete in two bytes.
                Some('0'..='~') => {
                    chars.next();
                }
                _ => {}
            },
            // The eight-bit CSI, which a few programs still write.
            '\u{9b}' => csi(&mut chars, &mut style, base, &adapt),
            '\n' => out.push(finish(&mut line)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' => line.clear(),
            '\t' => {
                let to = (line.len() / 8 + 1) * 8;
                line.resize(to, (' ', style));
            }
            // What `man` does for bold: a character, a backspace, the same
            // character again.
            '\x08' => {
                line.pop();
            }
            c if c.is_control() => {}
            c => line.push((c, style)),
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(finish(&mut line));
    }
    out
}

fn finish(line: &mut Vec<(char, Style)>) -> Line<'static> {
    let mut spans = Vec::new();
    let mut i = 0;
    while i < line.len() {
        let style = line[i].1;
        let mut text = String::new();
        while i < line.len() && line[i].1 == style {
            text.push(line[i].0);
            i += 1;
        }
        spans.push(Span::styled(text, style));
    }
    line.clear();
    Line::from(spans)
}

/// Consume a control sequence after its introducer, and apply it if it is SGR.
///
/// Parameter bytes, then intermediates, then one final byte, per ECMA-48. A
/// byte outside those ranges ends the sequence where it stands and is left for
/// the text — which is how a sequence cut off by a truncated result stops at
/// the newline rather than eating the next line.
fn csi(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    style: &mut Style,
    base: Style,
    adapt: &impl Fn(Color) -> Color,
) {
    let mut params = String::new();
    while let Some(c) = chars.next_if(|c| matches!(c, '0'..='?')) {
        params.push(c);
    }
    let mut intermediates = false;
    while chars.next_if(|c| matches!(c, ' '..='/')).is_some() {
        intermediates = true;
    }
    let Some(end) = chars.next_if(|c| matches!(c, '@'..='~')) else {
        return;
    };
    // `ESC [ ? … m` and its kin are private modes that happen to end in `m`.
    let private = params.starts_with(['<', '=', '>', '?']);
    if end == 'm' && !intermediates && !private {
        sgr(&params, style, base, adapt);
    }
}

/// Consume an OSC-style string up to its terminator: BEL, ST (`ESC \`), or the
/// eight-bit ST.
///
/// Also stopped by a newline, which it leaves for the text. A terminal would
/// keep swallowing, but a terminal is not reading a result cut off at a size
/// limit, where an unterminated sequence would take everything after it.
fn string(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(c) = chars.next_if(|c| *c != '\n') {
        match c {
            '\x07' | '\u{9c}' => return,
            '\x1b' => {
                chars.next_if_eq(&'\\');
                return;
            }
            _ => {}
        }
    }
}

/// Apply one SGR parameter list to `style`.
///
/// Colon sub-parameters (`38:2::r:g:b`, `4:3` for a curly underline) are the
/// newer spelling of the same things, and are read as one group each.
fn sgr(params: &str, style: &mut Style, base: Style, adapt: &impl Fn(Color) -> Color) {
    if params.is_empty() {
        *style = base;
        return;
    }
    let groups: Vec<&str> = params.split(';').collect();
    let mut i = 0;
    while i < groups.len() {
        let group = groups[i];
        i += 1;
        if group.contains(':') {
            let parts: Vec<u16> = group.split(':').map(|p| p.parse().unwrap_or(0)).collect();
            match parts[0] {
                4 => match parts.get(1) {
                    Some(0) => *style = style.remove_modifier(Modifier::UNDERLINED),
                    _ => *style = style.add_modifier(Modifier::UNDERLINED),
                },
                code @ (38 | 48) => {
                    // `38:2:<colourspace>:r:g:b`, with the colourspace often
                    // left empty — and sometimes left out altogether.
                    let color = match parts.get(1) {
                        Some(5) => parts.get(2).map(|&n| Color::Indexed(n as u8)),
                        Some(2) => {
                            let rgb = &parts[2..];
                            let rgb = if rgb.len() >= 4 { &rgb[1..] } else { rgb };
                            (rgb.len() >= 3)
                                .then(|| Color::Rgb(rgb[0] as u8, rgb[1] as u8, rgb[2] as u8))
                        }
                        _ => None,
                    };
                    if let Some(color) = color {
                        paint(style, code == 38, adapt(color));
                    }
                }
                _ => {}
            }
            continue;
        }
        let code: u16 = group.parse().unwrap_or(0);
        match code {
            0 => *style = base,
            1 => *style = style.add_modifier(Modifier::BOLD),
            2 => *style = style.add_modifier(Modifier::DIM),
            3 => *style = style.add_modifier(Modifier::ITALIC),
            4 => *style = style.add_modifier(Modifier::UNDERLINED),
            7 => *style = style.add_modifier(Modifier::REVERSED),
            9 => *style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => *style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => *style = style.remove_modifier(Modifier::ITALIC),
            24 => *style = style.remove_modifier(Modifier::UNDERLINED),
            27 => *style = style.remove_modifier(Modifier::REVERSED),
            29 => *style = style.remove_modifier(Modifier::CROSSED_OUT),
            30..=37 => paint(style, true, adapt(named(code - 30))),
            40..=47 => paint(style, false, adapt(named(code - 40))),
            90..=97 => paint(style, true, adapt(named(code - 90 + 8))),
            100..=107 => paint(style, false, adapt(named(code - 100 + 8))),
            // "Default" is the base's colour, not the terminal's: the text
            // around the output is dimmed, and the output rejoins it.
            39 => style.fg = base.fg,
            49 => style.bg = base.bg,
            38 | 48 => {
                let fg = code == 38;
                match groups.get(i).and_then(|g| g.parse::<u16>().ok()) {
                    Some(5) => {
                        if let Some(n) = groups.get(i + 1).and_then(|g| g.parse::<u8>().ok()) {
                            paint(style, fg, adapt(Color::Indexed(n)));
                        }
                        i += 2;
                    }
                    Some(2) => {
                        let c: Vec<u8> = groups
                            .iter()
                            .skip(i + 1)
                            .take(3)
                            .filter_map(|g| g.parse().ok())
                            .collect();
                        if let [r, g, b] = c[..] {
                            paint(style, fg, adapt(Color::Rgb(r, g, b)));
                        }
                        i += 4;
                    }
                    _ => i += 1,
                }
            }
            // Underline colour carries its colour the same way and is skipped
            // the same way; ratatui's underline takes the text colour anyway.
            58 => match groups.get(i).and_then(|g| g.parse::<u16>().ok()) {
                Some(5) => i += 2,
                Some(2) => i += 4,
                _ => i += 1,
            },
            // Blink and conceal are left out on purpose: a blinking cell in a
            // dashboard is noise, and hidden text in a log reader is a line
            // that looks empty for no reason.
            _ => {}
        }
    }
}

/// Set a colour, unless the fold had none to give — then the base's stays.
fn paint(style: &mut Style, fg: bool, color: Color) {
    if color == Color::Reset {
        return;
    }
    match fg {
        true => style.fg = Some(color),
        false => style.bg = Some(color),
    }
}

/// The sixteen by SGR number: the eight, then their bright forms.
fn named(n: u16) -> Color {
    const SIXTEEN: [Color; 16] = [
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Yellow,
        Color::Blue,
        Color::Magenta,
        Color::Cyan,
        Color::Gray,
        Color::DarkGray,
        Color::LightRed,
        Color::LightGreen,
        Color::LightYellow,
        Color::LightBlue,
        Color::LightMagenta,
        Color::LightCyan,
        Color::White,
    ];
    SIXTEEN[n as usize % 16]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn keep(c: Color) -> Color {
        c
    }

    /// cargo's own header, bold green then back to the base, is two spans in
    /// two styles and no escape left in either.
    #[test]
    fn sgr_becomes_style_and_the_reset_returns_to_the_base() {
        let base = Style::default().fg(Color::Indexed(245));
        let out = lines_with("\x1b[1m\x1b[32m   Compiling\x1b[0m cctop", base, keep);
        assert_eq!(out.len(), 1);
        let spans = &out[0].spans;
        assert_eq!(spans[0].content, "   Compiling");
        assert_eq!(
            spans[0].style,
            base.fg(Color::Green).add_modifier(Modifier::BOLD)
        );
        assert_eq!(spans[1].content, " cctop");
        assert_eq!(spans[1].style, base);
    }

    /// 256-colour and 24-bit colours, in both the semicolon and the colon
    /// spelling, land on the foreground and background they name.
    #[test]
    fn extended_colours_in_either_spelling() {
        let out = lines_with(
            "\x1b[38;5;196ma\x1b[48;2;1;2;3mb\x1b[0;38:2::4:5:6mc",
            Style::default(),
            keep,
        );
        let spans = &out[0].spans;
        assert_eq!(spans[0].style.fg, Some(Color::Indexed(196)));
        assert_eq!(spans[1].style.bg, Some(Color::Rgb(1, 2, 3)));
        assert_eq!(spans[2].style.fg, Some(Color::Rgb(4, 5, 6)));
        assert_eq!(spans[2].style.bg, None);
    }

    /// Whatever the fold says a colour is, that is what is drawn: this is the
    /// seam the sixteen-colour theme goes through.
    #[test]
    fn every_colour_goes_through_the_fold() {
        let out = lines_with("\x1b[38;5;196mx\x1b[91my", Style::default(), |_| {
            Color::Magenta
        });
        assert!(
            out[0]
                .spans
                .iter()
                .all(|s| s.style.fg == Some(Color::Magenta))
        );
        // No colour at all leaves the base's in place, emphasis intact.
        let base = Style::default().fg(Color::Gray);
        let out = lines_with("\x1b[1;31mz", base, |_| Color::Reset);
        assert_eq!(out[0].spans[0].style, base.add_modifier(Modifier::BOLD));
    }

    /// Cursor moves, erasures, private modes, OSC in both terminations, and a
    /// charset shift all vanish — and the text between them stays.
    #[test]
    fn everything_that_is_not_sgr_is_consumed() {
        let raw = "\x1b[2K\x1b[1Ga\x1b[?25l\x1b]8;;file:///x\x1b\\b\x1b]8;;\x1b\\\x1b]0;title\x07c\x1b(Bd\x1bPq#0\x1b\\e";
        let out = lines_with(raw, Style::default(), keep);
        assert_eq!(out.len(), 1);
        assert_eq!(plain(&out[0]), "abcde");
    }

    /// A progress bar's carriage returns leave its last frame; CRLF is only a
    /// newline; tabs become the columns they stood for.
    #[test]
    fn carriage_returns_tabs_and_backspaces() {
        let out = lines_with("10%\r55%\r100%\r\nx\ty\nab\x08c", Style::default(), keep);
        let texts: Vec<String> = out.iter().map(plain).collect();
        assert_eq!(texts, ["100%", "x       y", "ac"]);
    }

    /// A sequence cut off by the result's size limit stops at the newline
    /// rather than taking the next line with it.
    #[test]
    fn an_unterminated_sequence_does_not_eat_the_next_line() {
        let out = lines_with("a\x1b]8;;http://cut\nnext\x1b[3", Style::default(), keep);
        let texts: Vec<String> = out.iter().map(plain).collect();
        assert_eq!(texts, ["a", "next"]);
    }

    /// The one-line form has no escape left in it at all.
    #[test]
    fn strip_leaves_only_the_text() {
        assert_eq!(strip("\x1b[31mcargo\x1b[0m test\x07"), "cargo test");
    }
}
