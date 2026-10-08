//! Dropping a paste that arrives twice.
//!
//! Somewhere between a terminal and an agent, a paste sometimes gets delivered
//! twice: whole sentences doubled back to back, seen on WSL under Windows
//! Terminal on a machine under heavy load. Which layer does it — the terminal,
//! WSL, rmux — is not something cctop can fix, but it is something cctop can
//! absorb: nobody pastes the same text twice within a quarter of a second on
//! purpose, so the second copy is dropped. Like a debounced switch, the rule is
//! about timing alone, never about content beyond "is it the same".
//!
//! Two shapes of input go through it. The TUI is handed whole pastes by
//! crossterm and asks [`Debounce`] directly. `cctop run` and `cctop attach`
//! copy raw stdin, where a paste is only bytes between `ESC[200~` and
//! `ESC[201~` and may be split across reads anywhere; [`PasteFilter`] finds
//! those blocks and asks the same [`Debounce`] about each.

use std::time::{Duration, Instant};

/// How long after an identical paste was let through a kept one is still worth
/// a line in the log. A duplicate that arrives this late is not dropped, but a
/// log full of them says the window is too short for that machine.
const KEPT_NOTE: Duration = Duration::from_secs(5);

/// The decision about whether a paste is a repeat of the one before.
///
/// A paste is dropped when it is byte for byte the previous one and it starts
/// within `window` of the previous one having been handed on. The window runs
/// from the end of delivery ([`Debounce::delivered`]), not from arrival, so a
/// first paste that took a while to get through on a loaded machine does not
/// use up the time its echo is caught in.
///
/// **A keystroke resets it** ([`Debounce::interrupt`]). A key typed between two
/// identical pastes means someone was there, so the second paste was meant. A
/// mouse event or a resize does not count: a right-click paste in Windows
/// Terminal brings a click with it, and that click must not undo the debounce.
///
/// The comparison is exact. Nothing is trimmed or normalised first: a paste
/// that differs in any byte is a different paste.
///
// ponytail: only a bracketed duplicate is caught. A second copy that arrives as
// plain keystrokes would need every key after a paste held back to see whether
// it spells the paste again — a `torn.rs`-style filter on all typing, which
// risks delaying or eating real input.
#[derive(Debug, Clone)]
pub struct Debounce {
    /// How close a repeat must come to be dropped. Zero turns it off.
    pub window: Duration,
    /// Which input path this is, for the log: `tui`, `run` or `attach`.
    path: &'static str,
    /// The last paste let through, and when it was finished with.
    last: Option<(Vec<u8>, Instant)>,
}

impl Debounce {
    pub fn new(window: Duration, path: &'static str) -> Debounce {
        Debounce {
            window,
            path,
            last: None,
        }
    }

    /// Whether `body`, starting at `now`, should go through. A dropped one is
    /// logged, and so is an identical one kept for having come late.
    pub fn admit(&mut self, body: &[u8], now: Instant) -> bool {
        if self.window.is_zero() {
            self.last = None;
            return true;
        }
        if let Some((last, end)) = &mut self.last
            && last.as_slice() == body
        {
            let gap = now.saturating_duration_since(*end);
            if gap < self.window {
                // An echo of an echo is measured from this one: a third copy
                // right behind the second is the same fault.
                *end = now;
                self.note("duplicate-dropped", body.len(), gap);
                return false;
            }
            if gap < KEPT_NOTE {
                self.note("duplicate-kept", body.len(), gap);
            }
        }
        self.last = Some((body.to_vec(), now));
        true
    }

    /// The paste just admitted has been handed on, at `now`.
    pub fn delivered(&mut self, now: Instant) {
        if let Some((_, end)) = &mut self.last {
            *end = now;
        }
    }

    /// Someone typed: the next paste is meant, whatever it is.
    pub fn interrupt(&mut self) {
        self.last = None;
    }

    /// Sizes and gaps only. What was pasted is already in the `io` level for
    /// anyone who asked for it.
    fn note(&self, kind: &'static str, bytes: usize, gap: Duration) {
        crate::elog::event(
            "paste",
            kind,
            serde_json::json!({
                "bytes": bytes,
                "gap_ms": gap.as_millis() as u64,
                "path": self.path,
            }),
        );
    }
}

const START: &[u8] = b"\x1b[200~";
const END: &[u8] = b"\x1b[201~";

/// How much of an open paste is held before it is let go unchecked. A paste
/// bigger than this is forwarded as it comes, so a huge one never sits in
/// memory whole and an unterminated `ESC[200~` cannot hold typing back forever.
const CAP: usize = 1 << 20;

/// How long a paste may stay open before the next read lets it go unchecked.
/// A terminal writes a paste in one go, so a block still open this long after
/// it started is a lost `ESC[201~`, and what follows is typing.
const MAX_OPEN: Duration = Duration::from_secs(1);

/// [`Debounce`] for a raw byte stream: `cctop run`'s and `cctop attach`'s stdin.
///
/// Everything outside a bracketed paste goes through at once, in order. Only
/// the inside of an open `ESC[200~ … ESC[201~` block is held, until it closes:
/// then it goes through whole, markers and all, or not at all.
///
/// Bytes outside a block are keystrokes and reset the debounce, except mouse
/// and focus reports — the click that comes with a right-click paste. Hover
/// reports are stripped before this sees anything (see
/// `attach::strip_hover_reports`), which also means no mouse report reaches it
/// split across reads.
///
/// The start marker is held when it is split across reads, except for its
/// first two bytes: `ESC` and `ESC [` at the end of a read are also the Escape
/// key and Alt+[, and holding those until the next read would delay a key.
/// They go through at once, and a block they turn out to open is then
/// forwarded whatever it holds — it has already begun to arrive.
pub struct PasteFilter {
    pub debounce: Debounce,
    /// Outside a block: how much of [`START`] the stream ends with.
    start_seen: usize,
    /// How many of those bytes have already gone through.
    start_sent: usize,
    /// Bytes forwarded outside a block since they were last looked at for a key.
    outside: Vec<u8>,
    block: Option<Block>,
    /// Whether a paste was admitted that [`PasteFilter::delivered`] should time.
    admitted: bool,
}

struct Block {
    /// Everything after the start marker, the end marker too once it is in.
    body: Vec<u8>,
    end_seen: usize,
    /// When the read that opened it came in: the start of the paste.
    opened: Instant,
    /// Forwarded as it arrives rather than held: past the cap, too old, or
    /// begun before it was known to be a paste. Still remembered when it closes.
    streaming: bool,
}

impl PasteFilter {
    pub fn new(window: Duration, path: &'static str) -> PasteFilter {
        PasteFilter {
            debounce: Debounce::new(window, path),
            start_seen: 0,
            start_sent: 0,
            outside: Vec::new(),
            block: None,
            admitted: false,
        }
    }

    /// One read's worth of input, and what of it to forward now.
    pub fn feed(&mut self, chunk: &[u8], now: Instant) -> Vec<u8> {
        let mut out = Vec::with_capacity(chunk.len());
        if let Some(block) = &mut self.block
            && !block.streaming
            && now.saturating_duration_since(block.opened) >= MAX_OPEN
        {
            block.streaming = true;
            out.extend_from_slice(START);
            out.extend_from_slice(&block.body);
        }
        for &b in chunk {
            match self.block.is_some() {
                true => self.inside(b, &mut out),
                false => self.outside(b, now, &mut out),
            }
        }
        // `ESC` or `ESC [` at the end of a read may be a key, and a key is
        // never held. Whether it was one is left for the next read to decide.
        if self.start_seen <= 2 {
            out.extend_from_slice(&START[self.start_sent..self.start_seen]);
            self.start_sent = self.start_seen;
        }
        self.typed();
        out
    }

    /// The paste [`PasteFilter::feed`] last let through has been written on.
    pub fn delivered(&mut self, now: Instant) {
        if std::mem::take(&mut self.admitted) {
            self.debounce.delivered(now);
        }
    }

    fn outside(&mut self, b: u8, now: Instant, out: &mut Vec<u8>) {
        if b == START[self.start_seen] {
            self.start_seen += 1;
            if self.start_seen == START.len() {
                // Whatever was typed before the paste counts before it.
                self.typed();
                let streaming = self.start_sent > 0;
                if streaming {
                    out.extend_from_slice(&START[self.start_sent..]);
                }
                self.start_seen = 0;
                self.start_sent = 0;
                self.block = Some(Block {
                    body: Vec::new(),
                    end_seen: 0,
                    opened: now,
                    streaming,
                });
            }
            return;
        }
        if self.start_seen > 0 {
            // Not a paste after all: what was held goes through, and all of it
            // was typed.
            out.extend_from_slice(&START[self.start_sent..self.start_seen]);
            self.outside.extend_from_slice(&START[..self.start_seen]);
            self.start_seen = 0;
            self.start_sent = 0;
            return self.outside(b, now, out);
        }
        out.push(b);
        self.outside.push(b);
    }

    fn inside(&mut self, b: u8, out: &mut Vec<u8>) {
        let Some(block) = &mut self.block else {
            return;
        };
        block.body.push(b);
        if block.streaming {
            out.push(b);
        }
        block.end_seen = match b == END[block.end_seen] {
            true => block.end_seen + 1,
            false => usize::from(b == END[0]),
        };
        if block.end_seen == END.len() {
            let Some(mut block) = self.block.take() else {
                return;
            };
            block.body.truncate(block.body.len() - END.len());
            if block.streaming {
                // Already through; remembered so its own echo is caught.
                self.debounce.interrupt();
                self.debounce.admit(&block.body, block.opened);
                self.admitted = true;
            } else if self.debounce.admit(&block.body, block.opened) {
                out.extend_from_slice(START);
                out.extend_from_slice(&block.body);
                out.extend_from_slice(END);
                self.admitted = true;
            }
            return;
        }
        if !block.streaming && block.body.len() > CAP {
            block.streaming = true;
            out.extend_from_slice(START);
            out.extend_from_slice(&block.body);
        }
    }

    /// Look at what went through outside a paste, and reset the debounce if
    /// any of it was a key.
    fn typed(&mut self) {
        if !self.outside.is_empty() && !only_reports(&self.outside) {
            self.debounce.interrupt();
        }
        self.outside.clear();
    }
}

/// Whether `bytes` is nothing but mouse and focus reports — what a click or a
/// window coming into focus sends, as opposed to a key.
fn only_reports(mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        let n = if bytes.starts_with(b"\x1b[I") || bytes.starts_with(b"\x1b[O") {
            3
        } else if bytes.starts_with(b"\x1b[M") && bytes.len() >= 6 {
            6
        } else if let Some(rest) = bytes.strip_prefix(b"\x1b[<") {
            match rest
                .iter()
                .position(|b| !(b.is_ascii_digit() || *b == b';'))
            {
                Some(i) if matches!(rest[i], b'M' | b'm') => 3 + i + 1,
                _ => return false,
            }
        } else {
            return false;
        };
        bytes = &bytes[n..];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Duration = Duration::from_millis(250);

    fn ms(t0: Instant, n: u64) -> Instant {
        t0 + Duration::from_millis(n)
    }

    #[test]
    fn a_repeat_within_the_window_is_dropped() {
        let t0 = Instant::now();
        let mut d = Debounce::new(W, "test");
        assert!(d.admit(b"hello", t0));
        d.delivered(ms(t0, 10));
        assert!(!d.admit(b"hello", ms(t0, 200)));
    }

    #[test]
    fn the_window_runs_from_delivery_not_arrival() {
        let t0 = Instant::now();
        let mut d = Debounce::new(W, "test");
        assert!(d.admit(b"hello", t0));
        // A slow delivery on a loaded machine.
        d.delivered(ms(t0, 400));
        assert!(!d.admit(b"hello", ms(t0, 600)));
    }

    #[test]
    fn the_same_paste_after_the_window_is_kept() {
        let t0 = Instant::now();
        let mut d = Debounce::new(W, "test");
        assert!(d.admit(b"hello", t0));
        d.delivered(t0);
        assert!(d.admit(b"hello", ms(t0, 250)));
    }

    #[test]
    fn a_different_paste_is_kept_however_soon() {
        let t0 = Instant::now();
        let mut d = Debounce::new(W, "test");
        assert!(d.admit(b"hello", t0));
        assert!(d.admit(b"hello ", t0));
        assert!(d.admit(b"Hello ", t0));
    }

    #[test]
    fn a_zero_window_keeps_everything() {
        let t0 = Instant::now();
        let mut d = Debounce::new(Duration::ZERO, "test");
        assert!(d.admit(b"hello", t0));
        assert!(d.admit(b"hello", t0));
    }

    #[test]
    fn a_key_between_two_pastes_keeps_both() {
        let t0 = Instant::now();
        let mut d = Debounce::new(W, "test");
        assert!(d.admit(b"hello", t0));
        d.interrupt();
        assert!(d.admit(b"hello", ms(t0, 10)));
    }

    #[test]
    fn a_third_copy_behind_a_dropped_second_is_dropped_too() {
        let t0 = Instant::now();
        let mut d = Debounce::new(W, "test");
        assert!(d.admit(b"hello", t0));
        assert!(!d.admit(b"hello", ms(t0, 200)));
        assert!(!d.admit(b"hello", ms(t0, 400)));
    }

    fn bracket(body: &[u8]) -> Vec<u8> {
        [START, body, END].concat()
    }

    /// Feed `chunks` one read at a time, all at `now`, delivering after each.
    fn run(f: &mut PasteFilter, chunks: &[&[u8]], now: Instant) -> Vec<u8> {
        let mut out = Vec::new();
        for c in chunks {
            out.extend(f.feed(c, now));
            f.delivered(now);
        }
        out
    }

    #[test]
    fn a_block_split_anywhere_reads_as_it_does_whole() {
        let t0 = Instant::now();
        let paste = bracket(b"two\rlines");
        let stream = [b"a".as_slice(), &paste, b"b"].concat();
        for cut in 0..=stream.len() {
            let mut f = PasteFilter::new(W, "test");
            let (x, y) = stream.split_at(cut);
            assert_eq!(run(&mut f, &[x, y], t0), stream, "cut at {cut}");
        }
    }

    #[test]
    fn a_duplicate_split_anywhere_is_dropped() {
        let t0 = Instant::now();
        let paste = bracket(b"two\rlines");
        // The second copy's first two bytes are the one split that cannot be
        // held without holding back an Escape key; see `PasteFilter`.
        for cut in (0..=paste.len()).filter(|c| !matches!(c, 1 | 2)) {
            let mut f = PasteFilter::new(W, "test");
            assert_eq!(run(&mut f, &[&paste], t0), paste);
            let (x, y) = paste.split_at(cut);
            assert_eq!(run(&mut f, &[x, y], ms(t0, 50)), b"", "cut at {cut}");
        }
    }

    #[test]
    fn a_marker_split_after_escape_is_forwarded_and_remembered() {
        let t0 = Instant::now();
        let paste = bracket(b"hi");
        let mut f = PasteFilter::new(W, "test");
        let (x, y) = paste.split_at(1);
        assert_eq!(f.feed(x, t0), b"\x1b", "an Escape is never held");
        assert_eq!(f.feed(y, t0), &paste[1..]);
        f.delivered(t0);
        // Its own echo is still caught.
        assert_eq!(run(&mut f, &[&paste], ms(t0, 50)), b"");
    }

    #[test]
    fn two_copies_in_one_read_are_one() {
        let t0 = Instant::now();
        let paste = bracket(b"Draft PR: open a draft PR");
        let mut f = PasteFilter::new(W, "test");
        let both = [paste.as_slice(), &paste].concat();
        assert_eq!(run(&mut f, &[&both], t0), paste);
    }

    #[test]
    fn typing_between_two_copies_keeps_both() {
        let t0 = Instant::now();
        let paste = bracket(b"x");
        let mut f = PasteFilter::new(W, "test");
        let stream = [paste.as_slice(), b" ", &paste].concat();
        assert_eq!(run(&mut f, &[&stream], t0), stream);
        // An Escape in a read of its own, which is also the one way a paste's
        // start marker gets forwarded before it is complete.
        let mut f = PasteFilter::new(W, "test");
        let stream = [paste.as_slice(), b"\x1b", &paste].concat();
        assert_eq!(run(&mut f, &[&paste, b"\x1b", &paste], t0), stream);
    }

    #[test]
    fn a_click_between_two_copies_does_not_keep_the_second() {
        let t0 = Instant::now();
        let paste = bracket(b"x");
        let click = b"\x1b[<2;10;5M\x1b[<2;10;5m\x1b[I".as_slice();
        let mut f = PasteFilter::new(W, "test");
        let out = run(&mut f, &[&paste, click, &paste], t0);
        assert_eq!(out, [paste.as_slice(), click].concat());
    }

    #[test]
    fn keys_around_a_paste_go_through_at_once_and_in_order() {
        let t0 = Instant::now();
        let mut f = PasteFilter::new(W, "test");
        assert_eq!(f.feed(b"ab\x1b[A", t0), b"ab\x1b[A");
        assert_eq!(f.feed(b"c\x1b[200~par", t0), b"c", "only the paste is held");
        assert_eq!(
            f.feed(b"t\x1b[201~d\x1b", t0),
            b"\x1b[200~part\x1b[201~d\x1b"
        );
        assert_eq!(f.feed(b"[2~", t0), b"[2~", "Insert, after an Escape split");
    }

    #[test]
    fn an_unterminated_block_past_the_cap_is_let_go() {
        let t0 = Instant::now();
        let mut f = PasteFilter::new(W, "test");
        let mut out = f.feed(START, t0);
        let big = vec![b'a'; CAP + 10];
        out.extend(f.feed(&big, t0));
        assert_eq!(out, [START, &big].concat());
        assert_eq!(f.feed(b"typed", t0), b"typed");
    }

    #[test]
    fn an_unterminated_block_is_let_go_by_the_next_late_read() {
        let t0 = Instant::now();
        let mut f = PasteFilter::new(W, "test");
        assert_eq!(f.feed(b"\x1b[200~lost", t0), b"");
        assert_eq!(f.feed(b"k", ms(t0, 1500)), b"\x1b[200~lostk");
    }

    #[test]
    fn a_zero_window_forwards_every_copy() {
        let t0 = Instant::now();
        let paste = bracket(b"x");
        let mut f = PasteFilter::new(Duration::ZERO, "test");
        let both = [paste.as_slice(), &paste].concat();
        assert_eq!(run(&mut f, &[&both], t0), both);
    }

    #[test]
    fn reports_are_not_keys() {
        assert!(only_reports(b"\x1b[<0;1;1M\x1b[<0;1;1m\x1b[M !!\x1b[O"));
        assert!(!only_reports(b"\x1b[<0;1;1Mx"));
        assert!(!only_reports(b"\x1b"));
        assert!(!only_reports(b"\x1b[A"));
    }
}
