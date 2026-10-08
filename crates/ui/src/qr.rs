//! A link drawn as a QR code, for the panels that hand one to another device.
//!
//! Two links in cctop are meant to be opened somewhere else: the served table's
//! tunnel (`B`, then `c`) and a shared terminal's operator link (`W`). Typing a
//! token off a screen into a phone is nobody's idea of a feature; pointing the
//! phone at the screen is.
//!
//! **A QR code is the link, legibly.** Anything that can photograph it can open
//! it, so it is drawn only where the link has already been handed over — the
//! operator link after `W` has put it on the clipboard, the served link when
//! `c` asks for it — and never left standing where a link is merely *about*
//! (the footer corner, the panel as it first opens). A screenshot of either is
//! a screenshot of the credential, and the panels say so beside the code.

use qrcode::{EcLevel, QrCode};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;
use tui_qrcode::{Colors, QrCodeWidget, QuietZone};

/// A link encoded and ready to draw, with the cells it will take.
pub(super) struct Qr {
    widget: QrCodeWidget,
    pub width: u16,
    pub height: u16,
}

/// Encode `url`, or `None` for a link too long for any QR version.
///
/// Error correction `L`, the lowest. Its reason to exist is a code printed on
/// something that gets scuffed; a code on a screen is either all there or not
/// there at all, and every level up adds rows that a 24-line terminal does not
/// have — a tunnel link with its token is version 5 at `L` and version 6 at
/// `M`, four rows of difference.
///
/// Half-blocks, one module wide and two tall per cell, because that is the
/// densest rendering the widget has and the squarest: a terminal cell is about
/// twice as tall as it is wide. And the full four-module quiet zone the
/// standard asks for — on a dark theme the modal behind the code is dark, and a
/// code with no light margin of its own runs its finder patterns into the
/// border, which is the failure scanners are worst at.
///
/// Encoded afresh on every frame the panel is up rather than cached: a few
/// hundred bytes through the encoder is well under a millisecond, and a cache
/// is one more copy of a credential to remember to drop.
pub(super) fn encode(url: &str) -> Option<Qr> {
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::L).ok()?;
    let widget = QrCodeWidget::new(code)
        .quiet_zone(QuietZone::Enabled)
        .colors(colors())
        .style(style());
    let size = widget.size(Rect::default());
    Some(Qr {
        widget,
        width: size.width,
        height: size.height,
    })
}

impl Qr {
    /// Whether it fits in `width` × `height` cells at its natural size.
    ///
    /// Never scaled down to fit: a module is already one cell wide, and there is
    /// no smaller unit to shrink it into. A code that does not fit is not
    /// drawn at all, rather than drawn cropped — half a QR code scans as
    /// nothing, and looks like something.
    pub fn fits(&self, width: u16, height: u16) -> bool {
        self.width <= width && self.height <= height
    }

    /// Draw it centred horizontally in `area`, starting at its top row.
    ///
    /// Callers check [`Qr::fits`] first; this draws nothing into an area it
    /// would not fit, for the same reason.
    pub fn draw(&self, area: Rect, buf: &mut Buffer) {
        if !self.fits(area.width, area.height) {
            return;
        }
        let at = Rect {
            x: area.x + (area.width - self.width) / 2,
            y: area.y,
            width: self.width,
            height: self.height,
        };
        (&self.widget).render(at, buf);
    }
}

/// Dark modules on a light ground, whatever cctop's theme is.
///
/// Scanners are written for ink on paper. Most read an inverted code as well,
/// but "most" is the wrong promise for the one thing on screen whose whole job
/// is to be read by somebody else's camera — so the dark theme does not get a
/// light-on-dark code just because that is what the rest of its screen is.
///
/// The widget's `Normal` draws a dark module as a glyph in the foreground
/// colour and a light one as a blank on the background, so the foreground is
/// the black. 256-colour indices 16 and 231 rather than the ANSI black and
/// white, which themes remap freely — Solarized's "white" is a grey, and a
/// grey quiet zone is contrast given away.
fn style() -> Style {
    match super::theme::no_color() {
        // No colour means none of ours: the terminal's own pair, see below.
        true => Style::default(),
        false => Style::default()
            .fg(Color::Indexed(16))
            .bg(Color::Indexed(231)),
    }
}

/// Which of the widget's two polarities to draw in.
///
/// Under `NO_COLOR` there is no black to ask for, only the terminal's
/// foreground and background, and the code is drawn with the light modules in
/// the foreground — dark-on-light on the light-text-on-dark terminal nearly
/// everyone runs.
///
/// ponytail: under `NO_COLOR` on a dark-text-on-light terminal that comes out
/// inverted. Phone cameras read it anyway; a scanner that insists on polarity
/// will not, and the fix is the colour `NO_COLOR` asked cctop not to use.
fn colors() -> Colors {
    match super::theme::no_color() {
        true => Colors::Inverted,
        false => Colors::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The size is the code's and not the area's: modules plus eight columns of
    /// quiet zone across, half that down, rounded up.
    #[test]
    fn a_code_is_as_big_as_its_link_needs_and_no_bigger() {
        let short = encode("https://example.com/").expect("encodes");
        // Twenty bytes is version 2 at `L`: 25 modules, and with the quiet
        // zone 33 by 17 cells.
        assert_eq!((short.width, short.height), (33, 17));
        let link = format!(
            "https://tribute-resistance-resolved-moscow.trycloudflare.com/?t={}",
            "0123456789abcdef".repeat(2)
        );
        let long = encode(&link).expect("encodes");
        assert!(long.width > short.width && long.height > short.height);
        assert_eq!(long.height, long.width.div_ceil(2));
    }

    /// Too small is nothing at all, not the top-left of a code.
    #[test]
    fn a_code_that_does_not_fit_draws_nothing() {
        let qr = encode("https://example.com/").expect("encodes");
        let area = Rect::new(0, 0, qr.width - 1, qr.height);
        let mut buf = Buffer::empty(area);
        qr.draw(area, &mut buf);
        assert_eq!(buf, Buffer::empty(area));

        let area = Rect::new(0, 0, qr.width + 4, qr.height);
        let mut buf = Buffer::empty(area);
        qr.draw(area, &mut buf);
        assert_ne!(buf, Buffer::empty(area));
        // Centred: two columns either side are left alone.
        assert_eq!(buf[(1, 0)], Buffer::empty(area)[(1, 0)]);
        assert_eq!(buf[(qr.width + 2, 0)], Buffer::empty(area)[(0, 0)]);
    }

    /// Dark ink on a light ground, even on the dark theme the tests run under.
    #[test]
    fn a_code_is_drawn_dark_on_light() {
        let qr = encode("https://example.com/").expect("encodes");
        let area = Rect::new(0, 0, qr.width, qr.height);
        let mut buf = Buffer::empty(area);
        qr.draw(area, &mut buf);
        // The top-left corner is quiet zone: a light module, drawn as a blank
        // on the light background.
        let corner = &buf[(0, 0)];
        assert_eq!(corner.symbol(), " ");
        assert_eq!(corner.bg, Color::Indexed(231));
        assert_eq!(corner.fg, Color::Indexed(16));
        // Row 2 is modules 4 and 5, the top of the finder pattern, which is
        // dark: the glyph carries it, in black.
        assert_eq!(buf[(4, 2)].symbol(), "█");
    }
}
