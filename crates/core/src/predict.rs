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
//!   inverse cell — the cursor's cell when it is drawn inverse. Only onto a
//!   blank: never past the right edge, never onto a box border, a wide
//!   character, or text already there — typing into the middle of a line
//!   shifts what is right of it, which a guess of one cell cannot draw.
//! - A space is guessed onto a blank, so its cell holds its letter before
//!   the far side has answered at all; it is confirmed only once the far
//!   side's cursor has moved past it.
//! - A backspace takes back a guess still out, but its letter was sent and
//!   will be echoed before the backspace is; that letter landing where the
//!   next guess sits is not a wrong guess.
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
    epoch: u64,
    /// Whether this guess's epoch has had one confirmed, so it may be shown.
    ok: bool,
    /// Whether the cursor here is Ink's inverse cell, which is then drawn
    /// one to the right of the guess.
    fake_cursor: bool,
    sent_at: Instant,
}

/// A guess a backspace took back after its key had gone: the far side will
/// still echo it, then erase it.
struct Retracted {
    row: u16,
    col: u16,
    ch: char,
    at: Instant,
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
    Str,
}

pub struct Predictor {
    mode: Mode,
    shadow: vt100::Parser,
    pending: Vec<Guess>,
    retracted: Vec<Retracted>,
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
    /// Whether a [`Predictor::notice`] is up that nothing has taken back.
    noticed: std::cell::Cell<bool>,
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
            retracted: Vec::new(),
            epoch: 0,
            epoch_ok: false,
            quiet_until: None,
            srtt: None,
            slow: false,
            kitty: false,
            pasting: false,
            drawn: Vec::new(),
            noticed: std::cell::Cell::new(false),
            lex: Lex::Ground,
            csi: Vec::new(),
        }
    }

    /// Whether a guess is out, so the bridge wakes to expire it.
    pub fn waiting(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The terminal may have been resized, which the far side redraws for:
    /// if it was, every guess is off. Returns what to write.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Vec<u8> {
        if self.shadow.screen().size() == (rows, cols) {
            return Vec::new();
        }
        self.shadow.screen_mut().set_size(rows, cols);
        self.forget()
    }

    /// A new connection: what was guessed on the old one is moot, its keys
    /// may have been lost with the socket. A resume carries on the same byte
    /// stream; a fresh open starts a new one, at rest. Returns what to write.
    pub fn reconnected(&mut self, resumed: bool) -> Vec<u8> {
        if !resumed {
            self.lex = Lex::Ground;
            self.csi.clear();
        }
        self.forget()
    }

    /// `text` on the bottom row, in reverse video, over whatever the far side
    /// had there — the reconnect notice. Nothing scrolls and the cursor is put
    /// back where the far side left it, so the screen has moved by one row's
    /// paint, which [`Predictor::unnotice`] takes back.
    ///
    /// The cursor is put back from the shadow rather than with DECSC/DECRC:
    /// the far program may be holding a cursor of its own in that one slot.
    // ponytail: a cursor held on the last column, waiting to wrap, comes back
    // as a cursor on that column — the far side's next letter lands there
    // rather than on the next line. Rmux redraws place the cursor themselves.
    pub fn notice(&self, text: &str) -> Vec<u8> {
        self.noticed.set(true);
        let screen = self.shadow.screen();
        let (rows, cols) = screen.size();
        let mut out = Vec::new();
        goto(&mut out, rows.saturating_sub(1), 0);
        out.extend(b"\x1b[0m\x1b[2K\x1b[7m");
        // One short of the edge, so writing it never leaves a wrap pending.
        let room = usize::from(cols.saturating_sub(1));
        out.extend(text.chars().take(room).collect::<String>().as_bytes());
        out.extend(b"\x1b[0m");
        self.put_back(&mut out);
        out
    }

    /// Paint the bottom row back as the far side left it: what
    /// [`Predictor::notice`] drew over is gone, with nothing asked of the far
    /// side. Guesses on that row are the caller's to paint again
    /// ([`Predictor::reconnected`] does).
    pub fn unnotice(&self) -> Vec<u8> {
        self.noticed.set(false);
        let screen = self.shadow.screen();
        let (rows, cols) = screen.size();
        let row = rows.saturating_sub(1);
        let mut out = Vec::new();
        goto(&mut out, row, 0);
        out.extend(b"\x1b[0m\x1b[2K");
        let mut look = String::from("\x1b[0m");
        // Past the last cell with anything in it, the erase has already
        // painted what is there: nothing, in the default colours.
        let end = (0..cols)
            .rev()
            .find(|&col| {
                screen
                    .cell(row, col)
                    .is_some_and(|c| !c.contents().is_empty() || sgr(c) != look)
            })
            .map_or(0, |col| col + 1);
        for col in 0..end {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            let this = sgr(cell);
            if this != look {
                out.extend(this.as_bytes());
                look = this;
            }
            match cell.contents() {
                "" => out.push(b' '),
                s => out.extend(s.as_bytes()),
            }
        }
        self.put_back(&mut out);
        out
    }

    /// Whether a notice is up, for whoever has to take it back.
    pub fn noticed(&self) -> bool {
        self.noticed.get()
    }

    /// [`Predictor::unnotice`] while the socket is live: guesses on the
    /// bottom row, which the repaint wipes, are drawn again over it.
    pub fn take_notice(&mut self) -> Vec<u8> {
        let mut out = self.unnotice();
        let bottom = self.shadow.screen().size().0.saturating_sub(1);
        self.drawn.retain(|&(row, _)| row != bottom);
        out.extend(self.render());
        out
    }

    /// The far side's pen and cursor, as the shadow has them.
    fn put_back(&self, out: &mut Vec<u8>) {
        let screen = self.shadow.screen();
        out.extend(screen.attributes_formatted());
        let (crow, ccol) = screen.cursor_position();
        goto(out, crow, ccol);
    }

    /// Drop every guess and paint back whatever was drawn for them.
    fn forget(&mut self) -> Vec<u8> {
        self.pending.clear();
        self.retracted.clear();
        self.end_epoch(None);
        self.render()
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
        // Blank, and so what it holds until the far side has got there;
        // anything but blank or the letter afterwards was a wrong guess.
        if !screen
            .cell(row, col)
            .is_some_and(|c| plain(c) && blank(c.contents()))
        {
            return self.end_epoch(Some(now));
        }
        self.pending.push(Guess {
            row,
            col,
            ch,
            epoch: self.epoch,
            ok: self.epoch_ok,
            fake_cursor,
            sent_at: now,
        });
    }

    fn backspace(&mut self, now: Instant) {
        match self.pending.last() {
            Some(g) if g.epoch == self.epoch => {
                let g = self.pending.pop().expect("just seen");
                self.retracted.push(Retracted {
                    row: g.row,
                    col: g.col,
                    ch: g.ch,
                    at: now,
                });
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
        // Off guesses nothing, but keeps the shadow all the same: a
        // reconnect's notice is painted back from it.
        self.lex(bytes);
        self.shadow.process(bytes);
        if self.mode == Mode::Off {
            return Vec::new();
        }
        if self.kitty {
            self.pending.clear();
            return self.render();
        }
        let screen = self.shadow.screen();
        let contents = |row, col| {
            screen
                .cell(row, col)
                .map_or(String::new(), |c| c.contents().to_string())
        };
        // Back to blank: the far side has echoed and erased it.
        self.retracted.retain(|r| !blank(&contents(r.row, r.col)));
        let (crow, ccol) = screen.cursor_position();
        let mut i = 0;
        while i < self.pending.len() {
            let g = &self.pending[i];
            let screen = self.shadow.screen();
            let here = screen
                .cell(g.row, g.col)
                .map_or(String::new(), |c| c.contents().to_string());
            // A space's cell was blank already, so holding it says nothing
            // until the cursor has gone past.
            let holds = if g.ch == ' ' {
                blank(&here) && crow == g.row && ccol > g.col
            } else {
                here.chars().eq([g.ch])
            };
            let retracted = self
                .retracted
                .iter()
                .any(|r| (r.row, r.col) == (g.row, g.col) && here.chars().eq([r.ch]));
            if holds {
                let (epoch, rtt) = (g.epoch, now.saturating_duration_since(g.sent_at));
                self.pending.remove(i);
                self.rtt_sample(rtt);
                for g in self.pending.iter_mut().filter(|g| g.epoch == epoch) {
                    g.ok = true;
                }
                if epoch == self.epoch {
                    self.epoch_ok = true;
                }
            } else if blank(&here) || retracted {
                // Not there yet, or the echo of a letter taken back.
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
        self.retracted
            .retain(|r| now.saturating_duration_since(r.at) <= late);
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
                // A blank inverse cell, which is what Ink draws there only
                // when it is the end of the text: anything else there would
                // be hidden behind it.
                if self
                    .shadow
                    .screen()
                    .cell(next.0, next.1)
                    .is_some_and(|c| plain(c) && blank(c.contents()))
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
        // the terminal is holding. `drawn` is kept, and the next write
        // tries again — only a write can change either, and every one ends
        // in a render, so a retry on a timer would find the same.
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
                // An ESC ends a string whatever follows, as vte (and so the
                // shadow) has it: `ESC \\` is the terminator, and anything
                // else is the start of the next sequence.
                (Lex::Osc | Lex::Str, 0x1b) => Lex::Esc,
                (Lex::Osc, _) => Lex::Osc,
                (Lex::Str, _) => Lex::Str,
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

/// An empty cell, or one painted with a space: Ink and ratatui pad with
/// spaces where an erase leaves nothing.
fn blank(s: &str) -> bool {
    s.is_empty() || s == " "
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

    /// The notice goes on the bottom row without scrolling anything, and
    /// taking it back leaves this terminal exactly as the far side drew it —
    /// colours, wide letters and cursor included — whatever the setting.
    #[test]
    fn a_notice_taken_back_leaves_the_far_sides_screen_as_it_was() {
        let t = Instant::now();
        let far: &[u8] = "\x1b[2J\x1b[1;1Htop line\x1b[24;1H\x1b[1;32mok\x1b[0m 漢字 \x1b[44mbar\x1b[0m\x1b[5;3H\x1b[4m"
            .as_bytes();
        for mode in [Mode::Auto, Mode::Off] {
            let mut p = Predictor::new(mode, 24, 80);
            p.on_output(far, t);
            // This terminal: everything the far side said, then the notice.
            let mut here = vt100::Parser::new(24, 80, 0);
            here.process(far);
            let before = here.screen().contents_formatted();
            here.process(&p.notice("lost box: the connection ended · reconnecting"));
            let screen = here.screen();
            assert_eq!(
                screen.contents_between(0, 0, 0, 8),
                "top line",
                "nothing scrolled"
            );
            assert!(
                screen
                    .contents_between(23, 0, 23, 79)
                    .starts_with("lost box:")
            );
            assert!(screen.cell(23, 0).unwrap().inverse());
            assert_eq!(screen.cursor_position(), (4, 2), "{mode:?}");
            here.process(&p.unnotice());
            assert_eq!(
                String::from_utf8_lossy(&here.screen().contents_formatted()),
                String::from_utf8_lossy(&before),
                "{mode:?}"
            );
            assert_eq!(here.screen().cursor_position(), (4, 2));
            assert_eq!(
                here.screen().attributes_formatted(),
                p.shadow.screen().attributes_formatted()
            );
        }
    }

    /// A notice taken back on a live socket leaves a guess on the bottom row
    /// drawn, where the plain repaint would wipe it until its echo.
    #[test]
    fn a_notice_taken_back_live_keeps_the_guesses_under_it() {
        let t = Instant::now();
        let mut p = Predictor::new(Mode::Always, 5, 80);
        p.on_output(b"\x1b[2J\x1b[5;1H> \x1b[?25h", t);
        p.on_key(b"a", t);
        p.on_output(b"\x1b[5;3Ha", t + 100 * MS);
        assert!(shown(&p.on_key(b"b", t + 200 * MS), 'b'));
        let notice = p.notice("box not answering");
        let back = p.take_notice();
        assert!(back.starts_with(&p.unnotice()));
        assert!(shown(&back[p.unnotice().len()..], 'b'));
        let mut here = vt100::Parser::new(5, 80, 0);
        here.process(b"\x1b[2J\x1b[5;1H> a");
        here.process(&notice);
        here.process(&back);
        assert_eq!(here.screen().contents_between(4, 0, 4, 80), "> ab");
    }

    /// A notice wider than the terminal is cut short of its last column, so
    /// it never wraps onto anything or scrolls.
    #[test]
    fn a_long_notice_stops_short_of_the_edge() {
        let p = Predictor::new(Mode::Auto, 5, 10);
        let mut here = vt100::Parser::new(5, 10, 0);
        here.process(b"\x1b[1;1Hfirst");
        here.process(&p.notice("a notice far wider than ten columns"));
        assert_eq!(here.screen().contents_between(4, 0, 4, 10), "a notice ");
        assert_eq!(here.screen().contents_between(0, 0, 0, 5), "first");
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
        // The far side echoes the letter taken back, then erases it: neither
        // is the guess there being wrong.
        let echoed = p.on_output(b"b", t + 320 * MS);
        assert!(p.waiting());
        assert!(shown(&echoed, 'c'), "drawn again over the echo");
        p.on_output(b"\x08 \x08", t + 330 * MS);
        assert!(p.waiting());
        assert!(p.retracted.is_empty(), "erased, so done with");
        p.on_output(b"c", t + 340 * MS);
        assert!(!p.waiting(), "confirmed");
        // A backspace over the far side's own letters is not guessed.
        p.on_key(b"\x7f", t + 350 * MS);
        p.on_key(b"d", t + 360 * MS);
        assert!(!p.waiting(), "the backspace ended the epoch");
    }

    #[test]
    fn a_space_waits_for_the_cursor_to_pass_it() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Auto, t);
        // Padded with spaces, as ratatui paints a blank.
        p.on_output(b"\x1b[5;3H          \x1b[5;3H", t);
        echo(&mut p, 'a', 2, t, 120 * MS);
        let srtt = p.srtt;
        p.on_key(b" ", t + 200 * MS);
        // A spinner elsewhere, the cursor put back: the space's cell holds a
        // space, but that is what it held before.
        p.on_output(b"\x1b[1;1Hspin\x1b[5;4H", t + 210 * MS);
        assert!(p.waiting());
        assert_eq!(p.srtt, srtt);
        // So the next letter goes after the space, not on it.
        let typed = p.on_key(b"b", t + 220 * MS);
        assert!(contains(&typed, b"\x1b[5;5H\x1b[0;4mb"));
        p.on_output(b" ", t + 320 * MS);
        assert_eq!(p.pending.len(), 1, "the space, confirmed by the cursor");
        p.on_output(b"b", t + 340 * MS);
        assert!(!p.waiting());
    }

    #[test]
    fn a_reconnect_paints_back_what_was_drawn() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Always, t);
        echo(&mut p, 'a', 2, t, 120 * MS);
        assert!(shown(&p.on_key(b"b", t + 200 * MS), 'b'));
        assert!(p.resize(24, 80).is_empty(), "the same size is no resize");
        assert!(p.waiting());
        let back = p.reconnected(true);
        assert!(!p.waiting());
        assert!(contains(&back, b"\x1b[5;4H\x1b[0m "));
    }

    #[test]
    fn an_esc_ends_an_unterminated_string_as_vte_does() {
        let t = Instant::now();
        let mut p = ratatui(Mode::Always, t);
        echo(&mut p, 'a', 2, t, 120 * MS);
        p.on_key(b"b", t + 200 * MS);
        // A DCS cut short by a CSI: at rest after the CSI, as the shadow is.
        let out = p.on_output(b"\x1bPq#0\x1b[5;4H", t + 210 * MS);
        assert!(shown(&out, 'b'));
        // A fresh open is a fresh stream, whatever the old one was inside.
        p.on_output(b"\x1b]0;cut off", t + 220 * MS);
        p.reconnected(false);
        p.on_output(b"\x1b[2J\x1b[5;1H> a", t + 230 * MS);
        echo(&mut p, 'b', 3, t + 240 * MS, 120 * MS);
        assert!(shown(&p.on_key(b"c", t + 400 * MS), 'c'));
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
    fn ink_spaces_and_text_mid_line() {
        let t = Instant::now();
        let mut p = Predictor::new(Mode::Auto, 24, 40);
        p.on_output(INK, t);
        p.on_key(b"h", t);
        p.on_output(&ink_line("h"), t + 120 * MS);
        // A space onto Ink's padding, then its spinner.
        p.on_key(b" ", t + 200 * MS);
        p.on_output("\x1b[2;1H✻\x1b[10;6H".as_bytes(), t + 210 * MS);
        assert!(p.waiting());
        let typed = p.on_key(b"i", t + 220 * MS);
        assert!(contains(&typed, b"\x1b[10;7H\x1b[0;4mi"));
        p.on_output(&ink_line("h "), t + 320 * MS);
        assert_eq!(p.pending.len(), 1);
        p.on_output(&ink_line("h i"), t + 340 * MS);
        assert!(!p.waiting());
        // The cursor parked on text: a letter there would be inserted, and
        // what it pushes right cannot be drawn, so it is not guessed.
        let mut p = Predictor::new(Mode::Always, 24, 40);
        p.on_output(
            b"\x1b[?25l\x1b[10;1H| > ab\x1b[7mc\x1b[27md   |\x1b[10;7H",
            t,
        );
        p.on_key(b"x", t);
        assert!(!p.waiting());
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
