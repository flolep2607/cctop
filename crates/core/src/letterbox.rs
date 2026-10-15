//! A rmux client that does not hold its window, drawn cleanly anyway.
//!
//! One session can have several clients — a cctop tab here, a tab on another
//! machine reaching it through `cctop mux open`, a browser — and rmux sizes the
//! window to one of them (`window-size latest`, see [`crate::mux`]). Every other
//! client is then a terminal of the wrong size, and rmux does not draw for it:
//! a frame is the window's rows, written at the window's width, cut only to the
//! client's height. Into a narrower terminal every row runs past the edge and
//! wraps onto the next; into a wider one, the cells past the window's edge are
//! never written, and keep whatever the last larger frame left there. That was
//! the stray lines and half-drawn frames of a session open in two tabs of
//! different sizes, on both of them.
//!
//! So `cctop mux attach` sits between the rmux client and the terminal
//! ([`around`]). While the client holds the window, which is nearly always,
//! what rmux sends goes through untouched. Once it does not, the output is read
//! into a screen of the *window's* size, and the terminal is drawn from that:
//! the top-left of it where the terminal is smaller, the whole of it with a
//! blank margin where the terminal is larger — the letterbox a cctop pane
//! already draws when its shim grants less than it asked for.
//!
//! The second half is who holds the window. rmux moves `latest` when a client
//! attaches or resizes, never when one is typed into (`crate::rmux` has the
//! long version, under the UI's `Fit`), so a tab on another machine that you
//! type into keeps the window of whichever tab resized last. With `--follow`
//! the attach watches its own client's activity in rmux, and on input while it
//! is not holding the window takes it back ([`nudge`]). Never without input:
//! a client that took the window back on its own would take it from the one
//! being typed at, and the agent would reflow every time the two disagreed. A
//! cctop pane on this machine does not ask for it: the UI already does the
//! same from the keys it forwards (`tabs::Fit` in cctop-ui).
//!
//! ponytail: a terminal shorter than the window shows the window's top rows,
//! not the rows around the cursor as tmux would. rmux sends a smaller client
//! only its first rows, and cannot pan one, so the rows below are not here to
//! show; typing into it takes the window, and then everything is.

use std::io::Write;
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A terminal's or a window's size, as `(cols, rows)`.
pub type Size = (u16, u16);

/// Draws what rmux sends for a window of one size onto a terminal of another.
///
/// Pure: bytes in, bytes out. [`around`] is what feeds it and writes the
/// result; the tests drive it directly.
pub struct Letterbox {
    parser: vt100::Parser<Echo>,
    client: Size,
    window: Size,
    /// What the terminal shows, as last drawn from here: the screen the next
    /// draw is a difference from. `None` until the first draw after the client
    /// stopped holding the window, and after anything that may have drawn over
    /// it — a full frame from rmux is that.
    shown: Option<vt100::Screen>,
    /// The client left the alternate screen — it is detaching — so the rest
    /// goes through as it is: what follows is its restoring the terminal.
    ended: bool,
}

/// What a screen read here would have swallowed that the terminal must still
/// see while it is drawn from the screen rather than from rmux's bytes.
///
/// A bell, a desktop notification (OSC 9 / 777), a title, a clipboard write
/// (OSC 52), a keyboard protocol switch (`CSI > u`): none of them is a cell, so
/// a redraw from cells would drop every one, and cctop's pane on the far side
/// learns its agent rang, finished or wants Shift+Enter from exactly these.
#[derive(Default)]
struct Echo {
    out: Vec<u8>,
}

impl vt100::Callbacks for Echo {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.out.push(0x07);
    }

    fn visual_bell(&mut self, _: &mut vt100::Screen) {
        self.out.extend_from_slice(b"\x1bg");
    }

    fn set_window_icon_name(&mut self, _: &mut vt100::Screen, name: &[u8]) {
        osc(&mut self.out, &[b"1", name]);
    }

    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        osc(&mut self.out, &[b"2", title]);
    }

    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, ty: &[u8], data: &[u8]) {
        osc(&mut self.out, &[b"52", ty, data]);
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        osc(&mut self.out, params);
    }

    fn unhandled_csi(
        &mut self,
        _: &mut vt100::Screen,
        i1: Option<u8>,
        i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        // A private marker (`?`, `>`, `<`, `=`) goes before the parameters and
        // a true intermediate after them; the parser hands both over alike.
        let (before, after): (Vec<u8>, Vec<u8>) = [i1, i2]
            .into_iter()
            .flatten()
            .partition(|b| (0x3c..=0x3f).contains(b));
        let mut seq = b"\x1b[".to_vec();
        seq.extend(before);
        let params: Vec<String> = params
            .iter()
            .map(|sub| sub.iter().map(u16::to_string).collect::<Vec<_>>().join(":"))
            .collect();
        seq.extend(params.join(";").bytes());
        seq.extend(after);
        let mut ch = [0u8; 4];
        seq.extend(c.encode_utf8(&mut ch).bytes());
        self.out.extend(seq);
    }
}

fn osc(out: &mut Vec<u8>, params: &[&[u8]]) {
    out.extend_from_slice(b"\x1b]");
    out.extend(params.join(&b';'));
    out.push(0x07);
}

impl Letterbox {
    /// A client of `client`'s size, taken to hold the window until told
    /// otherwise — which is what attaching makes it.
    pub fn new(client: Size) -> Letterbox {
        Letterbox {
            parser: vt100::Parser::new_with_callbacks(client.1, client.0, 0, Echo::default()),
            client,
            window: client,
            shown: None,
            ended: false,
        }
    }

    /// Whether the client holds the window, so rmux is drawing for it.
    pub fn holding(&self) -> bool {
        self.ended || self.client == self.window
    }

    /// The terminal's size and the window's, as last learned.
    pub fn sizes(&self) -> (Size, Size) {
        (self.client, self.window)
    }

    /// The terminal is `client` and the window `window`. Returns whether that
    /// changed anything — and when it did, rmux's next frame has to be a whole
    /// one for the screen here to be right again ([`around`] asks for it).
    ///
    /// The screen is read at the window's size whichever way round they are:
    /// at the client's, a wider window's rows would wrap inside it the way
    /// they wrapped on the terminal.
    pub fn set_sizes(&mut self, client: Size, window: Size) -> bool {
        if (client, window) == (self.client, self.window) || client.0 == 0 || client.1 == 0 {
            return false;
        }
        self.client = client;
        if window.0 > 0 && window.1 > 0 {
            self.window = window;
        }
        let (cols, rows) = self.window;
        self.parser.screen_mut().set_size(rows, cols);
        self.shown = None;
        true
    }

    /// rmux's bytes in, the terminal's bytes out.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.parser.process(bytes);
        let echoed = std::mem::take(&mut self.parser.callbacks_mut().out);
        if self.holding() {
            return bytes.to_vec();
        }
        if !self.parser.screen().alternate_screen() {
            // rmux restoring the terminal on its way out. From here on the
            // bytes are the terminal's own business again.
            self.ended = true;
            let screen = self.parser.screen();
            let mut out = b"\x1b[0m\x1b[?1049l".to_vec();
            out.extend(screen.input_mode_formatted());
            if !screen.hide_cursor() {
                out.extend_from_slice(b"\x1b[?25h");
            }
            out.extend(echoed);
            return out;
        }
        if whole_frame(bytes, self.window.1.min(self.client.1)) {
            // Whatever was on the terminal, rmux meant all of it to be redrawn —
            // a resumed tab asks for exactly that after writing over its own
            // screen — so a difference from what was drawn here would not do.
            self.shown = None;
        }
        let mut out = echoed;
        out.extend(self.draw());
        out
    }

    /// The window's screen, cut or framed to the terminal: the bytes that
    /// take the terminal from what it shows to that.
    fn draw(&mut self) -> Vec<u8> {
        let (cols, rows) = self.client;
        let screen = self.parser.screen();
        let width = cols.min(self.window.0);
        let mut frame = Vec::new();
        for (row, line) in screen
            .rows_formatted(0, width)
            .enumerate()
            .take(usize::from(rows))
        {
            let _ = write!(frame, "\x1b[0m\x1b[{};1H", row + 1);
            frame.extend(line);
        }
        frame.extend_from_slice(b"\x1b[0m");
        let (at_row, at_col) = screen.cursor_position();
        let inside = at_row < rows && at_col < width;
        if inside {
            let _ = write!(frame, "\x1b[{};{}H", at_row + 1, at_col + 1);
        }
        // A cursor outside the part shown has nowhere honest to be.
        frame.extend_from_slice(match inside && !screen.hide_cursor() {
            true => b"\x1b[?25h",
            false => b"\x1b[?25l",
        });
        frame.extend(screen.input_mode_formatted());
        let mut view = vt100::Parser::new(rows, cols, 0);
        view.process(&frame);
        let next = view.screen().clone();
        let out = match &self.shown {
            // From where the last draw left the cursor: a difference moves it
            // relatively, and a sequence passed on since may have moved it.
            Some(prev) => {
                let (row, col) = prev.cursor_position();
                let mut out = format!("\x1b[{};{}H", row + 1, col + 1).into_bytes();
                out.extend(next.state_diff(prev));
                out
            }
            None => next.state_formatted(),
        };
        self.shown = Some(next);
        out
    }
}

/// Whether `bytes` redraw every row a terminal of `rows` shows: rmux's whole
/// frame positions each row in turn, from the first to the last, where an
/// update positions only the rows that changed.
fn whole_frame(bytes: &[u8], rows: u16) -> bool {
    let last = format!("\x1b[{rows};1H");
    contains(bytes, b"\x1b[1;1H") && contains(bytes, last.as_bytes())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Whether to take the window back, from what the client's activity and the
/// sizes say on each look at rmux.
///
/// Only on input: a client that re-asserted its size on its own would take the
/// window from the one being typed at, and the agent would reflow every time
/// the two disagreed — the flip-flop this exists to end.
#[derive(Default)]
pub struct Follow {
    /// rmux's `client_activity` at the last look.
    activity: Option<u64>,
    /// A nudge went out, and the next look says whether it took.
    nudged: bool,
    /// The terminal's size when a nudge did not take. Something other than
    /// "who resized last" holds the window — `window-size` set otherwise, or
    /// a client that ignores size — and nudging on every keystroke would only
    /// reflow the agent each time. Tried again once the terminal resizes.
    gave_up: Option<Size>,
}

impl Follow {
    /// One look: the client's `activity`, its terminal's size and the
    /// window's. True when it is time to nudge.
    pub fn look(&mut self, activity: u64, client: Size, window: Size) -> bool {
        let typed = self.activity.is_some_and(|before| activity > before);
        self.activity = Some(activity);
        let holding = client == window;
        if self.gave_up.is_some_and(|size| size != client) {
            self.gave_up = None;
        }
        if std::mem::take(&mut self.nudged) {
            if !holding {
                self.gave_up = Some(client);
            }
            return false;
        }
        if typed && !holding && self.gave_up.is_none() {
            self.nudged = true;
            return true;
        }
        false
    }
}

/// How often the window and the client's activity are looked at. Fast enough
/// that typing on the other side takes the window within a beat; a change in
/// size rmux draws a whole frame for is caught sooner, by that frame.
const LOOK_EVERY: Duration = Duration::from_millis(400);

/// The fewest milliseconds between two looks a frame asks for: a burst of
/// output that happens to repaint the screen is one question, not dozens.
const LOOK_AT_MOST: Duration = Duration::from_millis(50);

/// The size of the terminal on `fd`.
fn tty_size(fd: i32) -> Option<libc::winsize> {
    // SAFETY: TIOCGWINSZ writes one `winsize` into the struct it is given.
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ as _, &mut ws) } == 0;
    (ok && ws.ws_col > 0 && ws.ws_row > 0).then_some(ws)
}

/// Take the window for this client: a resize, since rmux moves `latest` on
/// nothing else: one column narrower and back, the UI's nudge, far enough
/// apart that the client reads two sizes rather than only the last.
///
/// A change of pixel size alone would have been a resize the agent never
/// saw, but rmux does not move `latest` for one (checked against the daemon,
/// with a client attached with pixels), so the agent is drawn one column
/// narrower for a moment — once, when the window changes hands.
fn nudge() {
    let Some(mut ws) = tty_size(0) else {
        return;
    };
    let set = |ws: &libc::winsize| {
        // SAFETY: fd 0 is this client's terminal and `ws` outlives the call.
        unsafe { libc::ioctl(0, libc::TIOCSWINSZ as _, ws) };
    };
    let before = ws;
    ws.ws_col = ws.ws_col.saturating_sub(1).max(1);
    set(&ws);
    std::thread::sleep(Duration::from_millis(150));
    // Only back to where it was if nothing else resized it meanwhile.
    if tty_size(0).is_some_and(|now| now.ws_col == ws.ws_col && now.ws_row == ws.ws_row) {
        set(&before);
    }
}

/// Ask the rmux client for a whole frame: the same SIGWINCH a resize sends,
/// which it answers by telling rmux its size, which rmux answers by redrawing
/// it. The size has not changed, so this takes nothing from anyone.
fn ask_for_a_frame() {
    // SAFETY: signalling this process; the rmux client handles SIGWINCH.
    unsafe { libc::kill(libc::getpid(), libc::SIGWINCH) };
}

/// Block SIGWINCH on the calling thread.
///
/// The rmux client waits for it on a thread of its own, with it blocked
/// everywhere else it made. A thread made here would otherwise be a thread
/// it is *not* blocked on, and the kernel hands a process's signal to any
/// such thread: the resize would land here, with the default action of
/// ignoring it, and the client would never learn the terminal changed.
fn leave_winch_alone() {
    // SAFETY: a sigset built and applied to this thread only.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGWINCH);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// What rmux says of this client: its activity — only asked for when it is
/// followed, since it costs a request of its own — and its window's size.
fn look_up(name: &str, follow: bool) -> Option<(u64, Size)> {
    match follow {
        true => crate::mux::client_view(name, std::process::id()),
        false => crate::rmux::window_size(name).map(|window| (0, window)),
    }
}

struct Shared {
    letterbox: Letterbox,
    /// When the output last asked for a look.
    looked: Instant,
}

/// Run `attach` — the rmux client, on this process's stdin and stdout — with
/// its output drawn through a [`Letterbox`], and with `follow`, taking the
/// window back on input ([`Follow`]).
///
/// Only for a terminal: output going anywhere else is left as it is.
pub fn around<T>(name: &str, follow: bool, attach: impl FnOnce() -> T) -> T {
    use std::io::IsTerminal;
    let Some(ws) = tty_size(0).filter(|_| std::io::stdout().is_terminal()) else {
        return attach();
    };
    let Some((terminal, pipe)) = divert_stdout() else {
        return attach();
    };
    let shared = Arc::new(Mutex::new(Shared {
        letterbox: Letterbox::new((ws.ws_col, ws.ws_row)),
        looked: Instant::now(),
    }));
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let output = {
        let shared = Arc::clone(&shared);
        let name = name.to_string();
        std::thread::spawn(move || {
            leave_winch_alone();
            pump(pipe, terminal, &shared, &name)
        })
    };
    {
        let shared = Arc::clone(&shared);
        let done = Arc::clone(&done);
        let name = name.to_string();
        std::thread::spawn(move || {
            leave_winch_alone();
            let mut follow = follow.then(Follow::default);
            loop {
                std::thread::sleep(LOOK_EVERY);
                if done.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let Some(ws) = tty_size(0) else { continue };
                let client = (ws.ws_col, ws.ws_row);
                let Some((activity, window)) = look_up(&name, follow.is_some()) else {
                    continue;
                };
                let changed = shared
                    .lock()
                    .map(|mut s| s.letterbox.set_sizes(client, window))
                    .unwrap_or(false);
                if changed {
                    ask_for_a_frame();
                }
                if let Some(follow) = &mut follow
                    && follow.look(activity, client, window)
                {
                    nudge();
                }
            }
        });
    }
    let result = attach();
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    restore_stdout();
    // The client's writer may still hold the pipe open for a moment; what it
    // wrote by now is drained, and nothing after it is waited for.
    let deadline = Instant::now() + Duration::from_millis(300);
    while !output.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    result
}

/// The terminal stdout was, kept aside, and the read end of a pipe stdout is
/// now. The pipe's write end lives on as fd 1 only.
fn divert_stdout() -> Option<(std::fs::File, std::fs::File)> {
    let mut fds = [0; 2];
    // SAFETY: plain fd juggling; every fd made here is owned by one value.
    unsafe {
        let saved = libc::fcntl(1, libc::F_DUPFD_CLOEXEC, 3);
        if saved < 0 {
            return None;
        }
        SAVED.store(saved, std::sync::atomic::Ordering::Relaxed);
        if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) != 0 {
            libc::close(saved);
            return None;
        }
        if libc::dup2(fds[1], 1) < 0 {
            libc::close(fds[0]);
            libc::close(fds[1]);
            libc::close(saved);
            return None;
        }
        libc::close(fds[1]);
        let terminal = libc::fcntl(saved, libc::F_DUPFD_CLOEXEC, 3);
        if terminal < 0 {
            restore_stdout();
            libc::close(fds[0]);
            return None;
        }
        Some((
            std::fs::File::from(OwnedFd::from_raw_fd(terminal)),
            std::fs::File::from(OwnedFd::from_raw_fd(fds[0])),
        ))
    }
}

/// The terminal [`divert_stdout`] put aside, so it can be put back.
static SAVED: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

fn restore_stdout() {
    let saved = SAVED.swap(-1, std::sync::atomic::Ordering::Relaxed);
    if saved >= 0 {
        // SAFETY: `saved` is the duplicate made in `divert_stdout`.
        unsafe {
            libc::dup2(saved, 1);
            libc::close(saved);
        }
    }
}

/// Copy the client's output to the terminal through the letterbox until the
/// pipe closes.
fn pump(mut pipe: std::fs::File, mut terminal: std::fs::File, shared: &Mutex<Shared>, name: &str) {
    use std::io::Read;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match pipe.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        let bytes = &buf[..n];
        let Ok(mut s) = shared.lock() else { return };
        // A whole frame is what rmux sends every client when the window
        // changes size. Ask what it is now, before reading the frame, so the
        // frame is read at the size it was drawn for rather than repaired
        // after; the periodic look would get there, a frame of garbage later.
        if contains(bytes, b"\x1b[1;1H") && s.looked.elapsed() >= LOOK_AT_MOST {
            s.looked = Instant::now();
            if let (Some(ws), Some(window)) = (tty_size(0), crate::rmux::window_size(name)) {
                s.letterbox.set_sizes((ws.ws_col, ws.ws_row), window);
            }
        }
        let out = s.letterbox.feed(bytes);
        drop(s);
        if terminal.write_all(&out).is_err() {
            return;
        }
        let _ = terminal.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a terminal of `size` shows after `bytes`.
    fn shown(size: Size, bytes: &[u8]) -> Vec<String> {
        let mut term = vt100::Parser::new(size.1, size.0, 0);
        term.process(bytes);
        term.screen()
            .rows(0, size.0)
            .map(|row| row.trim_end().to_string())
            .collect()
    }

    /// A whole frame the way rmux draws one: each of `rows` rows positioned
    /// and written at the window's full `cols`.
    fn frame(cols: u16, rows: u16, line: impl Fn(u16) -> String) -> Vec<u8> {
        let mut out = b"\x1b[s\x1b[?25l\x1b[0m".to_vec();
        for row in 0..rows {
            let text = format!("{:<width$}", line(row), width = usize::from(cols));
            out.extend(format!("\x1b[{};1H\x1b[0m{text}", row + 1).bytes());
        }
        out.extend(b"\x1b[0m\x1b[u\x1b[1;1H\x1b[?25h");
        out
    }

    fn attached(client: Size) -> (Letterbox, Vec<u8>) {
        let mut lb = Letterbox::new(client);
        let start = b"\x1b[?1049h\x1b[H\x1b[2J".to_vec();
        let out = lb.feed(&start);
        (lb, out)
    }

    #[test]
    fn a_client_holding_the_window_gets_rmuxs_bytes_untouched() {
        let (mut lb, out) = attached((80, 24));
        assert_eq!(out, b"\x1b[?1049h\x1b[H\x1b[2J");
        let f = frame(80, 24, |r| format!("row {r}"));
        assert_eq!(lb.feed(&f), f);
        assert!(lb.holding());
    }

    /// The bug: a 90-column window's frame on a 60-column terminal wrapped
    /// every row onto the next. Letterboxed, each row is cut at the edge.
    #[test]
    fn a_narrower_terminal_shows_the_window_cut_at_its_edge() {
        let (mut lb, mut screen) = attached((60, 15));
        assert!(lb.set_sizes((60, 15), (90, 26)));
        let f = frame(90, 15, |r| format!("{r:02}{}", "=".repeat(85)));
        // Unboxed, it is garbage: the last row runs off the edge, wraps, and
        // scrolls the whole screen up a line.
        let raw = shown((60, 15), &[&screen[..], &f[..]].concat());
        assert!(raw[0].starts_with("01"), "{raw:?}");
        screen.extend(lb.feed(&f));
        let rows = shown((60, 15), &screen);
        for (r, row) in rows.iter().enumerate() {
            assert_eq!(row, &format!("{r:02}{}", "=".repeat(58)), "row {r}");
        }
    }

    /// The other bug: cells past a smaller window's edge kept what an older,
    /// wider frame put there. Letterboxed, the margin is blank.
    #[test]
    fn a_wider_terminal_shows_the_window_with_a_blank_margin() {
        let (mut lb, mut screen) = attached((120, 30));
        screen.extend(lb.feed(&frame(120, 30, |_| "#".repeat(120))));
        lb.set_sizes((120, 30), (82, 24));
        screen.extend(lb.feed(&frame(82, 24, |r| format!("line {r}"))));
        let rows = shown((120, 30), &screen);
        assert_eq!(rows[0], "line 0");
        assert_eq!(rows[23], "line 23");
        for row in &rows[24..] {
            assert_eq!(row, "", "below the window is blank");
        }
    }

    #[test]
    fn an_update_after_the_first_draw_is_a_difference_not_a_redraw() {
        let (mut lb, mut screen) = attached((60, 15));
        lb.set_sizes((60, 15), (90, 26));
        screen.extend(lb.feed(&frame(90, 15, |r| format!("r{r}"))));
        let update = b"\x1b[5;1H\x1b[0mchanged";
        let out = lb.feed(update);
        assert!(!contains(&out, b"\x1b[2J"), "no clear for a one-row change");
        screen.extend(out);
        assert_eq!(shown((60, 15), &screen)[4], "changed");
    }

    /// Bells, notifications and keyboard-protocol switches are not cells, and
    /// a far cctop learns its agent rang or wants Shift+Enter from them.
    #[test]
    fn what_is_not_a_cell_still_reaches_the_terminal() {
        let (mut lb, _) = attached((60, 15));
        lb.set_sizes((60, 15), (90, 26));
        let out = lb.feed(b"\x07\x1b]9;done\x07\x1b[>1u\x1b]2;title\x07");
        for want in [
            &b"\x07"[..],
            b"\x1b]9;done\x07",
            b"\x1b[>1u",
            b"\x1b]2;title\x07",
        ] {
            assert!(contains(&out, want), "{want:?} in {out:?}");
        }
    }

    #[test]
    fn the_cursor_outside_the_part_shown_is_hidden() {
        let (mut lb, mut screen) = attached((60, 15));
        lb.set_sizes((60, 15), (90, 26));
        let mut f = frame(90, 15, |_| String::new());
        f.extend(b"\x1b[3;80H\x1b[?25h");
        screen.extend(lb.feed(&f));
        let mut term = vt100::Parser::new(15, 60, 0);
        term.process(&screen);
        assert!(term.screen().hide_cursor());
        screen.extend(lb.feed(b"\x1b[3;10H"));
        let mut term = vt100::Parser::new(15, 60, 0);
        term.process(&screen);
        assert!(!term.screen().hide_cursor());
        assert_eq!(term.screen().cursor_position(), (2, 9));
    }

    /// Detaching while letterboxed has to leave the terminal as rmux would:
    /// off the alternate screen, the cursor back.
    #[test]
    fn leaving_restores_the_terminal() {
        let (mut lb, _) = attached((60, 15));
        lb.set_sizes((60, 15), (90, 26));
        let out = lb.feed(b"\x1b[?1049l\x1b[?25h");
        assert!(contains(&out, b"\x1b[?1049l"));
        assert!(lb.holding(), "the rest passes through");
        assert_eq!(lb.feed(b"bye"), b"bye");
    }

    /// Taking the window back makes rmux's bytes right for the terminal
    /// again, and they go through untouched.
    #[test]
    fn holding_again_passes_through_again() {
        let (mut lb, _) = attached((60, 15));
        lb.set_sizes((60, 15), (90, 26));
        assert!(!lb.holding());
        assert!(lb.set_sizes((60, 15), (60, 15)));
        let f = frame(60, 15, |r| format!("r{r}"));
        assert_eq!(lb.feed(&f), f);
    }

    #[test]
    fn nothing_changes_nothing() {
        let (mut lb, _) = attached((60, 15));
        assert!(!lb.set_sizes((60, 15), (60, 15)));
        assert!(lb.set_sizes((60, 15), (61, 15)));
        assert!(!lb.set_sizes((60, 15), (61, 15)));
    }

    #[test]
    fn input_while_another_client_holds_the_window_takes_it() {
        let mut follow = Follow::default();
        // The first look only learns where activity stands.
        assert!(!follow.look(100, (80, 24), (120, 40)));
        // No input since: the window stays where it is, however wrong.
        assert!(!follow.look(100, (80, 24), (120, 40)));
        // Typed at: take it.
        assert!(follow.look(101, (80, 24), (120, 40)));
        // It took; typing on while holding asks nothing.
        assert!(!follow.look(102, (80, 24), (80, 24)));
        assert!(!follow.look(103, (80, 24), (80, 24)));
    }

    /// The flip-flop: a passive client that re-took the window on its own
    /// would fight the one being typed at. Without input, never.
    #[test]
    fn a_passive_client_never_takes_the_window_back_on_its_own() {
        let mut follow = Follow::default();
        follow.look(5, (80, 24), (80, 24));
        for _ in 0..50 {
            assert!(!follow.look(5, (80, 24), (120, 40)));
        }
    }

    #[test]
    fn a_nudge_that_does_not_take_is_not_tried_again_at_that_size() {
        let mut follow = Follow::default();
        follow.look(1, (80, 24), (120, 40));
        assert!(follow.look(2, (80, 24), (120, 40)));
        // Still not ours: something else decides the window.
        assert!(!follow.look(3, (80, 24), (120, 40)));
        assert!(!follow.look(4, (80, 24), (120, 40)));
        // A new size is a new chance.
        assert!(follow.look(5, (100, 30), (120, 40)));
    }
}
