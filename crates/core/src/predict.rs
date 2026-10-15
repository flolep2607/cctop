//! Predictive local echo for a remote tab, after mosh's — the lite version.
//!
//! A tab on a sibling (`cctop mux reach` / `mux launch`, see
//! [`crate::remote_open`]) is a WebSocket through Cloudflare's edge to an
//! rmux client over there, and every key waits a round trip — about 120 ms
//! measured — before its letter appears. That is slow enough to type ahead
//! of, and typing ahead of a screen you cannot see is how typos happen.
//!
//! So the bridge guesses. A [`Predictor`] keeps a shadow of the screen — a
//! `vt100` parser fed every byte the far side sends — and when a printable
//! key goes out it guesses where its letter will land, and draws it there,
//! underlined, before the far side has said so. The far side's answer is
//! the truth: a guess whose cell now holds its letter is confirmed and
//! dropped; one whose cell turned into something else, or that nothing
//! answered within twice the round trip and some slack, is wrong, and every
//! cell drawn over is painted back from the shadow.
//!
//! Wrong guesses are the cost, so the rules are about not making them:
//!
//! - Only a single printable ASCII byte is guessed. Anything else — Enter,
//!   Tab, Esc, an arrow, a control key, a paste, a burst of bytes in one
//!   read — ends the *epoch*: the screen may now move in ways a guess cannot
//!   follow, so guessing stops until the far side has had a round trip to
//!   catch up.
//! - A new epoch's guesses are made but not shown until one of them is
//!   confirmed. The first letter after an Enter is never drawn early; the
//!   second one is, once the first came back where it was guessed.
//! - A guess needs somewhere to go: the terminal cursor when it is visible,
//!   or — since Claude Code's Ink hides the real one and draws its own as an
//!   inverse cell — the cursor's cell when it is drawn inverse. Never past
//!   the right edge, never onto a box border, never onto a wide character.
//! - `/` or `@` at an empty prompt opens a menu in Claude Code, which moves
//!   the whole screen; it ends the epoch rather than being guessed.
//! - An application that pushes the kitty keyboard protocol (`CSI > … u`)
//!   is sent keys this module does not read, and guessing stops for good.
//! - Shown only when it helps: above 60 ms of round trip, hidden again below
//!   30 ms — two thresholds, so a link near one does not flicker — unless
//!   the setting says `always`.
//!
//! Nothing is sent to the far side for any of this. The guesses are painted
//! on this terminal only, between the far side's own writes, and the shadow
//! is never fed them — it is the far side's screen, which is what a wrong
//! guess is painted back from.

use std::time::{Duration, Instant};

/// `predictive_echo` in `config.toml`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Shown when the round trip is long enough to need it.
    #[default]
    Auto,
    /// Shown whatever the round trip.
    Always,
    /// Never guessed.
    Off,
}

impl Mode {
    /// Every value the setting takes, the default first.
    pub const NAMES: [&'static str; 3] = ["auto", "always", "off"];

    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "auto" => Some(Mode::Auto),
            "always" => Some(Mode::Always),
            "off" => Some(Mode::Off),
            _ => None,
        }
    }

    /// What the file says, or the default.
    pub fn configured() -> Mode {
        crate::settings::Settings::load()
            .predictive_echo
            .as_deref()
            .and_then(Mode::parse)
            .unwrap_or_default()
    }
}

/// Above this round trip, `auto` shows its guesses.
const SHOW_ABOVE: Duration = Duration::from_millis(60);
/// Below this one, it hides them again.
const HIDE_BELOW: Duration = Duration::from_millis(30);
/// Added to twice the round trip before an unanswered guess is wrong: the
/// far side's own time to redraw, which for Ink is not nothing.
const SLACK: Duration = Duration::from_millis(200);
/// The round trip assumed before one has been measured.
const UNMEASURED: Duration = Duration::from_millis(250);

/// One letter drawn ahead of the far side.
struct Guess {
    row: u16,
    col: u16,
    ch: char,
    /// The cell's contents when the guess was made: still that, and the far
    /// side has not got there yet; anything else but `ch`, and it was wrong.
    was: String,
    epoch: u64,
    /// Whether this guess's epoch has had one confirmed, so it may be shown.
    ok: bool,
    /// Whether the cursor here is Ink's inverse cell, which is then drawn
    /// one to the right of the guess.
    fake_cursor: bool,
    sent_at: Instant,
}

/// Where the far side's output is between bytes: only at rest — not inside
/// an escape sequence or a UTF-8 character — may anything be written
/// between its writes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lex {
    Ground,
    Utf8(u8),
    Esc,
    EscInter,
    Csi,
    Osc,
    OscEsc,
    Str,
    StrEsc,
}

pub struct Predictor {
    mode: Mode,
    shadow: vt100::Parser,
    pending: Vec<Guess>,
    epoch: u64,
    /// Whether a guess of the current epoch has been confirmed.
    epoch_ok: bool,
    /// No guess before this: the far side is still answering keys that were
    /// not guessed, and the shadow's cursor is behind them.
    quiet_until: Option<Instant>,
    srtt: Option<Duration>,
    /// `auto`'s hysteresis: whether the round trip last crossed above
    /// [`SHOW_ABOVE`] rather than below [`HIDE_BELOW`].
    slow: bool,
    kitty: bool,
    /// Inside a bracketed paste the keys typed are the paste's.
    pasting: bool,
    /// The cells painted over on the real terminal, to paint back.
    drawn: Vec<(u16, u16)>,
    lex: Lex,
    /// A CSI's bytes so far, enough of them to tell a kitty push.
    csi: Vec<u8>,
}

impl Predictor {
    pub fn new(mode: Mode, rows: u16, cols: u16) -> Predictor {
        Predictor {
            mode,
            shadow: vt100::Parser::new(rows, cols, 0),
            pending: Vec::new(),
            epoch: 0,
            epoch_ok: false,
            quiet_until: None,
            srtt: None,
            slow: false,
            kitty: false,
            pasting: false,
            drawn: Vec::new(),
            lex: Lex::Ground,
            csi: Vec::new(),
        }
    }

    /// Whether a guess is out, so the bridge wakes to expire it.
    pub fn waiting(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The terminal was resized, which the far side redraws for: every guess
    /// is off.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.shadow.screen_mut().set_size(rows, cols);
        self.forget();
    }

    /// A new connection, resumed or not: what was guessed on the old one is
    /// moot, and what is on the screen was not painted by this predictor.
    pub fn reconnected(&mut self) {
        self.forget();
    }

    fn forget(&mut self) {
        self.pending.clear();
        self.drawn.clear();
        self.end_epoch(None);
    }

    /// A round trip measured: a keepalive's pong, or a guess confirmed.
    pub fn rtt_sample(&mut self, rtt: Duration) {
        let srtt = match self.srtt {
            None => rtt,
            Some(s) => (s * 7 + rtt) / 8,
        };
        self.srtt = Some(srtt);
        if srtt > SHOW_ABOVE {
            self.slow = true;
        } else if srtt < HIDE_BELOW {
            self.slow = false;
        }
    }

    fn rtt(&self) -> Duration {
        self.srtt.unwrap_or(UNMEASURED)
    }

    /// Whether confirmed guesses are drawn.
    fn shows(&self) -> bool {
        !self.kitty
            && match self.mode {
                Mode::Off => false,
                Mode::Always => true,
                Mode::Auto => self.slow,
            }
    }

    fn end_epoch(&mut self, now: Option<Instant>) {
        self.epoch += 1;
        self.epoch_ok = false;
        self.quiet_until = now.map(|now| now + self.rtt());
    }

    /// Keys about to be sent, as read in one go. Returns what to write to
    /// this terminal.
    pub fn on_key(&mut self, bytes: &[u8], now: Instant) -> Vec<u8> {
        if self.mode == Mode::Off || self.kitty || bytes.is_empty() {
            return Vec::new();
        }
        if self.pasting || contains(bytes, b"\x1b[200~") {
            self.pasting = !ends_paste(bytes);
            self.end_epoch(Some(now));
            return self.render();
        }
        match bytes {
            [b @ 0x20..=0x7e] => self.printable(*b as char, now),
            [0x7f | 0x08] => self.backspace(now),
            _ => self.end_epoch(Some(now)),
        }
        self.render()
    }

    fn printable(&mut self, ch: char, now: Instant) {
        if self.quiet_until.is_some_and(|t| now < t) {
            // Not guessed, so the far side has one more key to answer
            // before the shadow's cursor is where the next one goes.
            self.quiet_until = Some(now + self.rtt());
            return;
        }
        let after = self
            .pending
            .last()
            .filter(|g| g.epoch == self.epoch)
            .map(|g| (g.row, g.col + 1, g.fake_cursor));
        let Some((row, col, fake_cursor)) = after.or_else(|| self.input_cursor()) else {
            return self.end_epoch(Some(now));
        };
        let fresh = after.is_none();
        if fresh && matches!(ch, '/' | '@') && self.prompt_empty(row, col) {
            return self.end_epoch(Some(now));
        }
        let screen = self.shadow.screen();
        let Some(cell) = screen.cell(row, col).filter(|c| plain(c)) else {
            return self.end_epoch(Some(now));
        };
        let was = cell.contents().to_string();
        self.pending.push(Guess {
            row,
            col,
            ch,
            was,
            epoch: self.epoch,
            ok: self.epoch_ok,
            fake_cursor,
            sent_at: now,
        });
    }

    fn backspace(&mut self, now: Instant) {
        match self.pending.last() {
            Some(g) if g.epoch == self.epoch => {
                self.pending.pop();
            }
            _ => self.end_epoch(Some(now)),
        }
    }

    /// Where the next letter goes, with nothing guessed yet: the cursor, if
    /// it is shown or drawn over by an inverse cell. Says too whether it is
    /// that inverse cell.
    fn input_cursor(&self) -> Option<(u16, u16, bool)> {
        let screen = self.shadow.screen();
        let (row, col) = screen.cursor_position();
        if !screen.hide_cursor() {
            return Some((row, col, false));
        }
        screen
            .cell(row, col)
            .filter(|c| c.inverse())
            .map(|_| (row, col, true))
    }

    /// Whether nothing has been typed at the prompt this row holds: left of
    /// the cursor is only a border, a prompt sign and blanks.
    fn prompt_empty(&self, row: u16, col: u16) -> bool {
        let screen = self.shadow.screen();
        (0..col).all(|c| {
            screen.cell(row, c).is_none_or(|cell| {
                cell.contents()
                    .chars()
                    .all(|ch| ch.is_whitespace() || border(ch) || "|>❯›$#%".contains(ch))
            })
        })
    }

    /// Output from the far side, about to be written to this terminal.
    /// Returns what to write after it.
    pub fn on_output(&mut self, bytes: &[u8], now: Instant) -> Vec<u8> {
        // Off keeps no shadow: nothing will be painted back from it.
        if self.mode == Mode::Off {
            return Vec::new();
        }
        self.lex(bytes);
        self.shadow.process(bytes);
        if self.kitty {
            self.pending.clear();
            return self.render();
        }
        let mut i = 0;
        while i < self.pending.len() {
            let g = &self.pending[i];
            let here = self
                .shadow
                .screen()
                .cell(g.row, g.col)
                .map_or(String::new(), |c| c.contents().to_string());
            if here.chars().eq([g.ch]) {
                let (epoch, rtt) = (g.epoch, now.saturating_duration_since(g.sent_at));
                self.pending.remove(i);
                self.rtt_sample(rtt);
                for g in self.pending.iter_mut().filter(|g| g.epoch == epoch) {
                    g.ok = true;
                }
                if epoch == self.epoch {
                    self.epoch_ok = true;
                }
            } else if here == g.was {
                i += 1;
            } else {
                self.pending.clear();
                self.end_epoch(Some(now));
            }
        }
        self.render()
    }

    /// Time passing with nothing read: a guess nothing answered in time is
    /// wrong. Returns what to write.
    pub fn tick(&mut self, now: Instant) -> Vec<u8> {
        let late = self.rtt() * 2 + SLACK;
        if self
            .pending
            .first()
            .is_some_and(|g| now.saturating_duration_since(g.sent_at) > late)
        {
            self.pending.clear();
            self.end_epoch(Some(now));
            return self.render();
        }
        Vec::new()
    }

    /// Paint what should be over the far side's screen now, and paint back
    /// what should not be any more.
    fn render(&mut self) -> Vec<u8> {
        let mut want: Vec<(u16, u16, &str)> = Vec::new();
        if self.shows() {
            for g in self.pending.iter().filter(|g| g.ok) {
                want.retain(|w| (w.0, w.1) != (g.row, g.col));
                want.push((g.row, g.col, ""));
            }
            if let Some(last) = self.pending.iter().rfind(|g| g.ok && g.fake_cursor) {
                let next = (last.row, last.col + 1);
                if self.shadow.screen().cell(next.0, next.1).is_some_and(plain)
                    && !want.iter().any(|w| (w.0, w.1) == next)
                {
                    want.push((next.0, next.1, "\x1b[0;7m "));
                }
            }
        }
        if want.is_empty() && self.drawn.is_empty() {
            return Vec::new();
        }
        let screen = self.shadow.screen();
        let (_, cols) = screen.size();
        let (crow, ccol) = screen.cursor_position();
        // Mid-sequence, a write here would land inside the far side's; and
        // with the cursor on the last column, moving it would lose a wrap
        // the terminal is holding. The next write tries again.
        //
        // ponytail: origin mode and scroll regions are not consulted — CUP is
        // taken as absolute, which is what an rmux client redraw uses.
        if self.lex != Lex::Ground || ccol + 1 >= cols {
            return Vec::new();
        }
        let mut out = Vec::new();
        for &(row, col) in &self.drawn {
            if want.iter().any(|w| (w.0, w.1) == (row, col)) {
                continue;
            }
            goto(&mut out, row, col);
            if let Some(cell) = screen.cell(row, col) {
                out.extend(sgr(cell).as_bytes());
                match cell.contents() {
                    "" => out.push(b' '),
                    s => out.extend(s.as_bytes()),
                }
            }
        }
        for &(row, col, paint) in &want {
            goto(&mut out, row, col);
            match paint {
                "" => {
                    let ch = self
                        .pending
                        .iter()
                        .rfind(|g| (g.row, g.col) == (row, col))
                        .map_or(' ', |g| g.ch);
                    out.extend(format!("\x1b[0;4m{ch}").as_bytes());
                }
                fake => out.extend(fake.as_bytes()),
            }
        }
        out.extend(screen.attributes_formatted());
        goto(&mut out, crow, ccol);
        self.drawn = want.iter().map(|w| (w.0, w.1)).collect();
        out
    }

    /// Follow the far side's bytes far enough to know when it is between
    /// sequences, and to see a kitty keyboard push go by.
    fn lex(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.lex = match (self.lex, b) {
                (Lex::Ground, 0x1b) => Lex::Esc,
                (Lex::Ground, 0xc0..=0xdf) => Lex::Utf8(1),
                (Lex::Ground, 0xe0..=0xef) => Lex::Utf8(2),
                (Lex::Ground, 0xf0..=0xf7) => Lex::Utf8(3),
                (Lex::Ground, _) => Lex::Ground,
                (Lex::Utf8(n), 0x80..=0xbf) if n > 1 => Lex::Utf8(n - 1),
                (Lex::Utf8(_), _) => Lex::Ground,
                (Lex::Esc, b'[') => {
                    self.csi.clear();
                    Lex::Csi
                }
                (Lex::Esc, b']') => Lex::Osc,
                (Lex::Esc, b'P' | b'X' | b'^' | b'_') => Lex::Str,
                (Lex::Esc | Lex::EscInter, 0x20..=0x2f) => Lex::EscInter,
                (Lex::Esc | Lex::EscInter, _) => Lex::Ground,
                (Lex::Csi, 0x1b) => Lex::Esc,
                (Lex::Csi, 0x40..=0x7e) => {
                    // ponytail: a pop (`CSI < u`) does not bring guessing
                    // back — one app that wanted kitty keys may want them
                    // again, and this tab is not worth guessing wrong in.
                    if b == b'u' && self.csi.first() == Some(&b'>') {
                        self.kitty = true;
                    }
                    Lex::Ground
                }
                (Lex::Csi, _) => {
                    if self.csi.len() < 32 {
                        self.csi.push(b);
                    }
                    Lex::Csi
                }
                (Lex::Osc, 0x07) => Lex::Ground,
                (Lex::Osc, 0x1b) => Lex::OscEsc,
                (Lex::Osc, _) => Lex::Osc,
                (Lex::OscEsc, _) => Lex::Ground,
                (Lex::Str, 0x1b) => Lex::StrEsc,
                (Lex::Str, _) => Lex::Str,
                (Lex::StrEsc, b'\\') => Lex::Ground,
                (Lex::StrEsc, _) => Lex::Str,
            };
        }
    }
}

/// A cell a letter may be guessed onto, or Ink's cursor drawn onto: one
/// column wide, and not part of a box.
fn plain(cell: &vt100::Cell) -> bool {
    !cell.is_wide() && !cell.is_wide_continuation() && !cell.contents().chars().any(border)
}

/// The edges of a box — Claude Code's prompt is one — which typing does not
/// push across.
fn border(ch: char) -> bool {
    matches!(ch, '│' | '┃' | '║' | '|') || ('\u{2500}'..='\u{259f}').contains(&ch)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Whether a read inside a paste closes it: its end marker after its last
/// start marker.
fn ends_paste(bytes: &[u8]) -> bool {
    let last = |needle: &[u8]| bytes.windows(needle.len()).rposition(|w| w == needle);
    match (last(b"\x1b[200~"), last(b"\x1b[201~")) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(start), Some(end)) => end > start,
    }
}

fn goto(out: &mut Vec<u8>, row: u16, col: u16) {
    out.extend(format!("\x1b[{};{}H", row + 1, col + 1).as_bytes());
}

/// The escape that sets a cell's look, from nothing.
fn sgr(cell: &vt100::Cell) -> String {
    let mut s = String::from("\x1b[0");
    if cell.bold() {
        s.push_str(";1");
    }
    if cell.dim() {
        s.push_str(";2");
    }
    if cell.italic() {
        s.push_str(";3");
    }
    if cell.underline() {
        s.push_str(";4");
    }
    if cell.inverse() {
        s.push_str(";7");
    }
    for (color, base) in [(cell.fgcolor(), 30u16), (cell.bgcolor(), 40)] {
        match color {
            vt100::Color::Default => {}
            vt100::Color::Idx(i) if i < 8 => s.push_str(&format!(";{}", base + u16::from(i))),
            vt100::Color::Idx(i) if i < 16 => {
                s.push_str(&format!(";{}", base + 60 + u16::from(i) - 8))
            }
            vt100::Color::Idx(i) => s.push_str(&format!(";{};5;{i}", base + 8)),
            vt100::Color::Rgb(r, g, b) => s.push_str(&format!(";{};2;{r};{g};{b}", base + 8)),
        }
    }
    s.push('m');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    fn shown(out: &[u8], ch: char) -> bool {
        contains(out, format!("\x1b[0;4m{ch}").as_bytes())
    }

    /// A ratatui-like screen: a prompt drawn by absolute moves, the real
    /// cursor shown after it.
    fn ratatui(mode: Mode, t: Instant) -> Predictor {
        let mut p = Predictor::new(mode, 24, 80);
        p.on_output(b"\x1b[2J\x1b[5;1H> \x1b[?25h", t);
        p
    }

    /// Type `ch` at `t` and have it come back `rtt` later at row 5.
    fn echo(
        p: &mut Predictor,
        ch: char,
        col: u16,
        t: Instant,
        rtt: Duration,
    ) -> (Vec<u8>, Vec<u8>) {
        let typed = p.on_key(&[ch as u8], t);
        let back = p.on_output(format!("\x1b[5;{}H{ch}", col + 1).as_bytes(), t + rtt);
        (typed, back)
    }

    #[test]
    fn a_confirmed_epoch_shows_the_next_letter_and_its_echo_clears_it() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Auto, t);
        // The first letter of an epoch is guessed, not drawn.
        let (typed, _) = echo(&mut p, 'a', 2, t, 120 * MS);
        assert!(!shown(&typed, 'a'));
        assert!(!p.waiting(), "its echo confirmed it");
        // Confirmed, and 120 ms is slow: the next one is drawn at once,
        // where the cursor is now.
        let typed = p.on_key(b"b", t + 200 * MS);
        assert!(shown(&typed, 'b'));
        assert!(contains(&typed, b"\x1b[5;4H\x1b[0;4mb"));
        // The overlay ends with the cursor put back where the far side left it.
        assert!(typed.ends_with(b"\x1b[5;4H"));
        let back = p.on_output(b"b", t + 320 * MS);
        assert!(!p.waiting());
        assert!(contains(&back, b"\x1b[5;4H\x1b[0mb"), "painted back plain");
    }

    #[test]
    fn a_wrong_guess_is_painted_back_and_ends_the_epoch() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Auto, t);
        echo(&mut p, 'a', 2, t, 120 * MS);
        assert!(shown(&p.on_key(b"b", t + 200 * MS), 'b'));
        let back = p.on_output(b"x", t + 300 * MS);
        assert!(!p.waiting());
        assert!(contains(&back, b"\x1b[5;4H\x1b[0mx"));
        // The next letter is not guessed until the far side has caught up.
        p.on_key(b"c", t + 310 * MS);
        assert!(!p.waiting());
    }

    #[test]
    fn a_guess_nothing_answers_is_taken_back() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Always, t);
        echo(&mut p, 'a', 2, t, 100 * MS);
        assert!(shown(&p.on_key(b"b", t + 200 * MS), 'b'));
        assert!(p.tick(t + 500 * MS).is_empty(), "not late yet");
        let back = p.tick(t + 700 * MS);
        assert!(!p.waiting());
        assert!(contains(&back, b"\x1b[5;4H\x1b[0m "));
    }

    #[test]
    fn a_control_key_ends_the_epoch_and_the_next_one_waits_for_a_confirm() {
        let t = Instant::now();
        for key in [&b"\r"[..], b"\t", b"\x1b", b"\x1b[D", b"\x03"] {
            let mut p = ratatui(Mode::Auto, t);
            echo(&mut p, 'a', 2, t, 120 * MS);
            p.on_key(key, t + 130 * MS);
            // Within a round trip: the far side is still answering the key.
            p.on_key(b"b", t + 140 * MS);
            assert!(!p.waiting(), "{key:?}");
            // After it: guessed, but a new epoch, so not drawn yet.
            let later = t + 600 * MS;
            p.on_output(b"\x1b[6;1H> ", later);
            let typed = p.on_key(b"c", later);
            assert!(p.waiting(), "{key:?}");
            assert!(!shown(&typed, 'c'), "{key:?}");
        }
    }

    #[test]
    fn a_paste_or_a_burst_is_never_guessed() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Always, t);
        echo(&mut p, 'a', 2, t, 120 * MS);
        p.on_key(b"\x1b[200~hel", t + 200 * MS);
        assert!(!p.waiting());
        // Still inside the paste, read a byte at a time.
        p.on_key(b"l", t + 900 * MS);
        assert!(!p.waiting());
        p.on_key(b"o\x1b[201~", t + 900 * MS);
        assert!(!p.waiting());
        // Two letters in one read are a burst.
        p.on_key(b"xy", t + 2000 * MS);
        assert!(!p.waiting());
        // Out of it, after a quiet round trip, guessing goes on.
        p.on_key(b"z", t + 3000 * MS);
        assert!(p.waiting());
    }

    #[test]
    fn a_kitty_keyboard_push_stops_guessing() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Always, t);
        // Split across two reads, as a socket may.
        p.on_output(b"\x1b[>", t);
        p.on_output(b"1u", t);
        p.on_key(b"a", t + 1000 * MS);
        assert!(!p.waiting());
    }

    #[test]
    fn auto_shows_above_60ms_and_hides_only_below_30ms() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Auto, t);
        assert!(!p.shows(), "nothing measured yet");
        p.rtt_sample(120 * MS);
        assert!(p.shows());
        for _ in 0..40 {
            p.rtt_sample(40 * MS);
        }
        assert!(p.shows(), "40 ms is between the two");
        for _ in 0..40 {
            p.rtt_sample(10 * MS);
        }
        assert!(!p.shows());
        for _ in 0..40 {
            p.rtt_sample(45 * MS);
        }
        assert!(!p.shows(), "45 ms does not bring it back");
        for _ in 0..40 {
            p.rtt_sample(100 * MS);
        }
        assert!(p.shows());
        let mut always = Predictor::new(Mode::Always, 24, 80);
        always.rtt_sample(MS);
        assert!(always.shows());
    }

    #[test]
    fn a_fast_link_guesses_but_draws_nothing() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Auto, t);
        echo(&mut p, 'a', 2, t, 5 * MS);
        let typed = p.on_key(b"b", t + 10 * MS);
        assert!(p.waiting());
        assert!(typed.is_empty());
    }

    #[test]
    fn off_guesses_nothing() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Off, t);
        echo(&mut p, 'a', 2, t, 120 * MS);
        assert!(p.on_key(b"b", t + 200 * MS).is_empty());
        assert!(!p.waiting());
    }

    #[test]
    fn backspace_takes_back_only_a_guess_still_out() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Auto, t);
        echo(&mut p, 'a', 2, t, 120 * MS);
        assert!(shown(&p.on_key(b"b", t + 200 * MS), 'b'));
        let erased = p.on_key(b"\x7f", t + 210 * MS);
        assert!(!p.waiting());
        assert!(contains(&erased, b"\x1b[5;4H\x1b[0m "));
        // The guess there is gone, so the next goes in the same cell.
        assert!(contains(
            &p.on_key(b"c", t + 220 * MS),
            b"\x1b[5;4H\x1b[0;4mc"
        ));
        // A backspace over the far side's own letters is not guessed.
        p.on_output(b"c", t + 340 * MS);
        p.on_key(b"\x7f", t + 350 * MS);
        p.on_key(b"d", t + 360 * MS);
        assert!(!p.waiting(), "the backspace ended the epoch");
    }

    #[test]
    fn nothing_is_written_inside_the_far_sides_escape() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Always, t);
        echo(&mut p, 'a', 2, t, 120 * MS);
        p.on_key(b"b", t + 200 * MS);
        assert!(p.on_output(b"\x1b[1;1Hspin\x1b[3", t + 210 * MS).is_empty());
        let rest = p.on_output(b"1mx\x1b[0m\x1b[5;4H", t + 220 * MS);
        assert!(shown(&rest, 'b'), "drawn again once it is at rest");
    }

    /// An Ink-like prompt: a box, the real cursor hidden but parked on the
    /// inverse cell Claude Code draws as its own.
    const INK: &[u8] = "\x1b[?25l\x1b[2J\x1b[10;1H│ > \x1b[7m \x1b[27m      │\x1b[10;5H".as_bytes();

    fn ink_line(typed: &str) -> Vec<u8> {
        let pad = 6 - typed.len();
        format!(
            "\x1b[10;1H│ > {typed}\x1b[7m \x1b[27m{}│\x1b[10;{}H",
            " ".repeat(pad),
            5 + typed.len()
        )
        .into_bytes()
    }

    #[test]
    fn ink_letters_go_on_its_fake_cursor_and_move_it() {
        let t = Instant::now();
        let mut p = Predictor::new(Mode::Auto, 24, 40);
        p.on_output(INK, t);
        p.on_key(b"h", t);
        assert!(p.waiting(), "the inverse cell is a cursor");
        p.on_output(&ink_line("h"), t + 120 * MS);
        assert!(!p.waiting());
        let typed = p.on_key(b"i", t + 200 * MS);
        assert!(contains(&typed, b"\x1b[10;6H\x1b[0;4mi"));
        assert!(
            contains(&typed, b"\x1b[10;7H\x1b[0;7m "),
            "the fake cursor, one on"
        );
        // A spinner elsewhere redraws the frame: the guess is drawn again.
        let spun = p.on_output("\x1b[2;1H✻ Thinking\x1b[10;6H".as_bytes(), t + 250 * MS);
        assert!(shown(&spun, 'i'));
        let back = p.on_output(&ink_line("hi"), t + 320 * MS);
        assert!(!p.waiting());
        assert!(!shown(&back, 'i'));
    }

    #[test]
    fn ink_menus_and_borders_end_the_epoch() {
        let t = Instant::now();
        for key in [b"/", b"@"] {
            let mut p = Predictor::new(Mode::Always, 24, 40);
            p.on_output(INK, t);
            p.on_key(key, t);
            assert!(!p.waiting(), "{key:?} at an empty prompt opens a menu");
        }
        // A slash after text is a letter like any other.
        let mut p = Predictor::new(Mode::Always, 24, 40);
        p.on_output(&[INK, &ink_line("a")[..]].concat(), t);
        p.on_key(b"/", t);
        assert!(p.waiting());
        // Seven letters fill the box; the eighth would land on its border.
        let mut p = Predictor::new(Mode::Always, 24, 40);
        p.on_output(INK, t);
        for (i, ch) in "abcdefg".chars().enumerate() {
            p.on_key(&[ch as u8], t + i as u32 * MS);
        }
        assert_eq!(p.pending.len(), 7);
        p.on_key(b"h", t + 10 * MS);
        assert_eq!(p.pending.len(), 7, "not past the border");
    }

    #[test]
    fn a_hidden_cursor_on_a_plain_cell_is_not_a_prompt() {
        let t = Instant::now();
        let mut p = Predictor::new(Mode::Always, 24, 40);
        p.on_output(b"\x1b[?25l\x1b[3;3H", t);
        p.on_key(b"a", t);
        assert!(!p.waiting());
    }

    #[test]
    fn the_right_edge_is_not_crossed() {
        let t = Instant::now();
        let mut p = Predictor::new(Mode::Always, 5, 10);
        p.on_output(b"\x1b[1;8H", t);
        for ch in "abcd".chars() {
            p.on_key(&[ch as u8], t);
        }
        assert_eq!(p.pending.len(), 3, "columns 8 to 10 only");
    }

    #[test]
    fn the_setting_takes_its_three_values() {
        for name in Mode::NAMES {
            assert!(Mode::parse(name).is_some());
        }
        assert_eq!(Mode::parse(Mode::NAMES[0]), Some(Mode::default()));
        assert_eq!(Mode::parse("on"), None);
    }
}
