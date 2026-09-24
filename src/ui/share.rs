//! Handing a terminal to someone else: the shared link and the served page.
//!
//! Two different things that read as one to the user and share the same
//! spinner: `web-share` puts a live pane on a URL, and `cctop serve` publishes
//! the dashboard. Both are slow enough to be started on another thread and
//! reported back through [`Opening`], and both produce a link that goes to the
//! clipboard rather than to the screen — a status line is read over your
//! shoulder and survives into a screenshot.

use super::*;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// A tunnel registration in flight, and when it started.
///
/// The instant is what the spinner is drawn from — a frame counter would have
/// to be advanced by whoever happens to redraw, and the loop redraws on events
/// that have nothing to do with this.
pub struct Opening {
    pub(super) rx: Receiver<Result<crate::serve::Serving, String>>,
    pub(super) since: Instant,
}

/// The spinner's alphabet, shared by everything that has to keep saying it is
/// alive.
const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

impl Opening {
    /// The spinner's current frame.
    pub fn frame(&self) -> char {
        let i = self.since.elapsed().as_millis() / 100;
        FRAMES[i as usize % FRAMES.len()]
    }
}

/// The same spinner, for a wait that is not an [`Opening`].
///
/// Read off one clock for the process rather than off an `Instant` each caller
/// would have to carry: whatever asks gets the current frame, and every
/// spinner on screen ticks together.
pub(super) fn spinner_frame() -> char {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    FRAMES[(start.elapsed().as_millis() / 100) as usize % FRAMES.len()]
}

/// A terminal share, held for the panel that shows it as a QR code.
///
/// Only a share that reaches off this machine gets one. The code exists to be
/// scanned by a phone, and a loopback link scanned by a phone opens nothing —
/// the status line saying "this machine only" is the whole answer there.
pub struct ShareQr {
    /// The agent's label, for the panel's first line.
    pub label: String,
    /// The operator link. A credential; see [`App::share_selected`] for why it
    /// may be drawn here at all.
    pub link: String,
    pub pin: Option<String>,
}

impl App {
    /// Open the selected agent's terminal in a browser, via the multiplexer.
    ///
    /// Only reaches agents cctop handed to the multiplexer, which is the same
    /// limit `a` has and for the same reason: an agent on cctop's own pty is on
    /// no terminal a second viewer can be pointed at, so there is nothing for
    /// `web-share -t` to name.
    ///
    /// The operator link goes to the clipboard and never to the screen. It
    /// grants input to a live coding agent, and a status line is read by
    /// whoever is behind you and survives into a screenshot; the clipboard is
    /// where the user was going to put it anyway. The pairing code is shown,
    /// since it is worth nothing without the link.
    ///
    /// The one exception is the QR code, which is the link in a form a camera
    /// reads: a tunnelled share opens a panel with it, because a phone is where
    /// a link that leaves the machine is usually headed. It is drawn only here,
    /// right after `W` has already handed the same link over, and only until
    /// the panel is closed — the key press is the deliberate act, and the panel
    /// says beside the code what holding it grants.
    pub(super) fn share_selected(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let label = session.display_label().to_string();
        let Some(pid) = session.root_pid() else {
            self.set_status("Selected session has no local process");
            return;
        };
        let Some(name) = crate::rmux::holding(pid) else {
            self.set_status("Only an agent cctop put in a multiplexer can be shared");
            return;
        };
        // Tunnelled where the machine can be reached and this machine only
        // where it cannot; the status line below says which came back. Pressing
        // `W` twice reuses the first share rather than minting a second.
        let mut reachable = false;
        let share = crate::rmux::share_link(&name, false).map(|(share, tunnelled)| {
            reachable = tunnelled;
            share
        });
        match share {
            Ok(share) => {
                // `web_share` refuses a share with no operator link, so this is
                // the link or the error above it — never the spectator one.
                let Some(operator) = share.operator.as_deref() else {
                    self.set_status(format!("Could not share {label}: no operator link"));
                    return;
                };
                render::copy_to_clipboard(operator);
                let pin = match &share.pin {
                    Some(pin) => format!(" · pin {pin}"),
                    None => String::new(),
                };
                // Where the link reaches from is the one thing about a share
                // that is not on the link: an operator URL looks the same
                // whether its endpoint is a tunnel or this machine's loopback,
                // and sending someone a link that cannot leave the building is
                // a failure they discover instead of being told about.
                let reach = match reachable {
                    true => "",
                    false => " · this machine only",
                };
                self.set_status(format!(
                    "Sharing {label} — operator link copied{pin}{reach}"
                ));
                if reachable {
                    self.share_qr = Some(ShareQr {
                        label,
                        link: operator.to_string(),
                        pin: share.pin.clone(),
                    });
                    self.mode = Mode::ShareQr;
                }
            }
            Err(error) => self.set_status(format!("Could not share {label}: {error}")),
        }
    }

    /// Start serving the table to a browser, or say why not.
    ///
    /// `tunnel` is the difference between a link that works on this machine and
    /// one that works from a phone. It is asked for per start rather than
    /// toggled on a running server: registering with the edge is what mints the
    /// hostname, so turning it on means a new link either way, and a flag that
    /// silently invalidated the link somebody was holding would be worse than a
    /// stop and a start they can see.
    /// A tunnel is registered off the UI thread; a loopback listener is not.
    /// Binding a socket is instant, and waiting a millisecond for it costs less
    /// than a state the rest of the code has to know about. The edge is the slow
    /// half — see [`Opening`].
    pub(super) fn start_serving(&mut self, tunnel: bool) {
        self.serve_error = None;
        let options = crate::serve::Options {
            tunnel,
            plan: self.plan,
            // Fed from the rows this dashboard already has. Two loaders in one
            // process would walk the same disk twice and, worse, could disagree
            // — a page saying one thing while the table beside it says another
            // is the bug nobody thinks to look for.
            scan: false,
            // The dashboard's rows include the remote ones, and the serve owes
            // them the same answers — the `Host`s are how it reaches back.
            hosts: self.remote_hosts.clone(),
            ..Default::default()
        };
        if tunnel {
            // One at a time. A second click while the first is still dialling
            // would register a second tunnel and throw the first away.
            if self.share_opening.is_some() {
                return;
            }
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                // The receiver is gone if cctop quit while this was dialling,
                // and the tunnel then drops here — which unregisters it, which
                // is the right ending for a link nobody is holding.
                let _ = tx.send(crate::serve::start(options).map_err(|e| format!("{e}")));
            });
            self.share_opening = Some(Opening {
                rx,
                since: Instant::now(),
            });
            self.set_status("Opening a tunnel to trycloudflare…");
            return;
        }
        match crate::serve::start(options) {
            Ok(serving) => {
                // Something to look at immediately: the page's first request
                // would otherwise find the empty snapshot it was built with and
                // report a machine with no sessions on it.
                serving.publish_with_quota(&self.sessions, &self.quota);
                let where_to = match serving.public.is_some() {
                    true => "on the internet",
                    false => "on this machine",
                };
                self.set_status(format!("Serving {where_to} — B for the link"));
                self.serving = Some(serving);
            }
            Err(error) => {
                let error = format!("{error}");
                self.set_status(format!("Could not serve: {error}"));
                self.serve_error = Some(error);
            }
        }
    }

    /// Take the tunnel from the thread opening one, if it has finished.
    ///
    /// Returns whether the screen has changed — which, while one is in flight,
    /// is every tick: the spinner is the thing saying cctop has not hung.
    pub(super) fn tick_share(&mut self) -> bool {
        let Some(opening) = &self.share_opening else {
            return false;
        };
        let done = match opening.rx.try_recv() {
            Err(TryRecvError::Empty) => return true,
            Ok(done) => done,
            // The thread went without answering, which it has no path to do.
            // Reported rather than left spinning for ever.
            Err(TryRecvError::Disconnected) => Err("the tunnel gave no answer".to_string()),
        };
        self.share_opening = None;
        match done {
            Ok(serving) => {
                // Something to look at immediately: the page's first request
                // would otherwise find the empty snapshot it was built with.
                serving.publish_with_quota(&self.sessions, &self.quota);
                self.set_status("On the internet — click the link, or B to copy it");
                self.serving = Some(serving);
            }
            Err(error) => {
                self.set_status(format!("Could not open a tunnel: {error}"));
                self.serve_error = Some(error);
            }
        }
        true
    }

    /// Stop serving, which un-mints every link handed out.
    pub(super) fn stop_serving(&mut self) {
        if self.serving.take().is_some() {
            self.set_status("Stopped serving — the links no longer answer");
        }
    }

    /// Show the page whatever the table is showing.
    ///
    /// Called wherever the rows change rather than on a timer of its own: the
    /// page's event stream wakes on a new version, so this is also what makes a
    /// browser update when the table does.
    pub(super) fn feed_serving(&self) {
        if let Some(serving) = &self.serving {
            serving.publish_with_quota(&self.sessions, &self.quota);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::test_app;
    /// Both halves matter. A panel that cannot say where the page is has not
    /// answered the question it was opened to answer; a panel that prints the
    /// token puts a credential into every screenshot of it. The link is drawn
    /// as its origin and the token lives in the clipboard and the escape.
    #[test]
    fn the_serve_panel_names_the_page_without_naming_its_token() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut app = test_app();
        let serving = crate::serve::start(crate::serve::Options {
            // A port nobody asked for, so a busy one is stepped past rather
            // than failing a test on whatever else is running here.
            port_given: false,
            // The panel is what is under test; scanning would walk the disk
            // and poll the quota endpoints for a serve nobody opens.
            scan: false,
            ..Default::default()
        })
        .expect("a loopback server");
        let token = serving
            .local
            .split_once("?t=")
            .map(|(_, token)| token.to_string())
            .expect("a tokenised link");
        app.serving = Some(serving);
        app.mode = Mode::Serve;

        let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("backend");
        let mut layout = render::Layout::default();
        terminal
            .draw(|frame| layout = render::draw(frame, &mut app))
            .expect("draw");
        // What is on screen, which is the label of each link and not the URL
        // behind it — the token is meant to be in there, and not in view.
        let screen = crate::ui::hyperlink::visible(terminal.backend().buffer());
        let linked = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter_map(|cell| crate::ui::hyperlink::target_of(cell.symbol()))
            .any(|url| url.ends_with(&token));

        assert!(linked, "the drawn origin does not open the page");
        assert!(
            screen.contains("http://127.0.0.1:"),
            "the panel never said where the page is:\n{screen}"
        );
        assert!(
            !screen.contains(&token),
            "the token was drawn on screen:\n{screen}"
        );
    }

    use ratatui::buffer::Buffer;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    /// A serve with a tunnel link on it, without dialling Cloudflare: the
    /// panel reads `public` and nothing else of the tunnel.
    fn tunnelled_app() -> (App, String) {
        let mut app = test_app();
        let mut serving = crate::serve::start(crate::serve::Options {
            port_given: false,
            scan: false,
            ..Default::default()
        })
        .expect("a loopback server");
        let token = serving
            .local
            .split_once("?t=")
            .map(|(_, token)| token.to_string())
            .expect("a tokenised link");
        serving.public = Some(format!(
            "https://tribute-resistance-resolved-moscow.trycloudflare.com/?t={token}"
        ));
        app.serving = Some(serving);
        app.mode = Mode::Serve;
        (app, token)
    }

    fn draw(app: &mut App, width: u16, height: u16) -> (Buffer, render::Layout) {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("backend");
        let mut layout = render::Layout::default();
        terminal
            .draw(|frame| layout = render::draw(frame, app))
            .expect("draw");
        (terminal.backend().buffer().clone(), layout)
    }

    /// Whether a row holds any of a code's cells, which are the only ones on
    /// screen painted on the code's white.
    fn is_code_row(buf: &Buffer, y: u16) -> bool {
        (0..buf.area.width).any(|x| buf[(x, y)].bg == ratatui::style::Color::Indexed(231))
    }

    fn code_rows(buf: &Buffer) -> usize {
        (0..buf.area.height)
            .filter(|&y| is_code_row(buf, y))
            .count()
    }

    fn row_text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    /// The text inside the panel titled `title`, row by row, with the code's
    /// rows and blank rows left out — what has to read the same with a code in
    /// it as without.
    fn text_rows(buf: &Buffer, title: &str) -> Vec<String> {
        let top = format!("╭ {title} ");
        let (y0, x0) = (0..buf.area.height)
            .find_map(|y| {
                let row = row_text(buf, y);
                row.find(&top)
                    .map(|at| (y, row[..at].chars().count() as u16))
            })
            .expect("the panel is on screen");
        let x1 = (x0 + 1..buf.area.width)
            .find(|&x| buf[(x, y0)].symbol() == "╮")
            .expect("the panel's right edge");
        (y0 + 1..buf.area.height)
            .take_while(|&y| buf[(x0, y)].symbol() != "╰")
            .filter(|&y| !is_code_row(buf, y))
            .map(|y| {
                (x0 + 1..x1)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .replace("c hide QR", "c QR code")
                    .trim()
                    .to_string()
            })
            .filter(|row| !row.is_empty())
            .collect()
    }

    const SERVE: &str = "Serve this table to a browser";
    const SHARE: &str = "Open this terminal elsewhere";

    /// `c` draws the tunnel link as a code when the screen has room for it, and
    /// the panel's text is the same text either way — the code is added to the
    /// panel, not swapped in for any of it.
    #[test]
    fn the_serve_panel_draws_the_tunnel_link_as_a_code_on_request() {
        let (mut app, token) = tunnelled_app();
        let expected = crate::ui::qr::encode(app.serving.as_ref().unwrap().best())
            .expect("encodes")
            .height as usize;

        let (plain, _) = draw(&mut app, 120, 50);
        assert_eq!(code_rows(&plain), 0, "a code before anyone asked for one");

        app.on_key(key('c'));
        assert!(app.serve_qr);
        let (coded, _) = draw(&mut app, 120, 50);
        assert_eq!(code_rows(&coded), expected, "the whole code, and only it");
        assert_eq!(text_rows(&coded, SERVE), text_rows(&plain, SERVE));
        // In the panel, that is: the footer's link carries it in an escape,
        // which is not text anyone reads.
        assert!(
            !text_rows(&coded, SERVE)
                .iter()
                .any(|row| row.contains(&token)),
            "the token was drawn as text"
        );

        // And `c` again takes it away.
        app.on_key(key('c'));
        let (again, _) = draw(&mut app, 120, 50);
        assert_eq!(code_rows(&again), 0);
    }

    /// On a screen too short for it the code is not drawn at all, and the panel
    /// says why instead of answering `c` with nothing.
    #[test]
    fn the_serve_panel_leaves_the_code_out_where_it_does_not_fit() {
        let (mut app, _) = tunnelled_app();
        let (plain, _) = draw(&mut app, 120, 30);
        app.serve_qr = true;
        let (squeezed, _) = draw(&mut app, 120, 30);
        assert_eq!(code_rows(&squeezed), 0);
        let mut said = text_rows(&squeezed, SERVE);
        let note = said
            .iter()
            .position(|row| row.starts_with("No room here for a QR code"))
            .expect("the panel said nothing about the code");
        said.remove(note);
        assert_eq!(said, text_rows(&plain, SERVE));
        // Too narrow is the same answer as too short.
        let (narrow, _) = draw(&mut app, 50, 60);
        assert_eq!(code_rows(&narrow), 0);
    }

    /// Without a tunnel there is no link a phone could open, so no code.
    #[test]
    fn a_loopback_serve_has_no_code_to_offer() {
        let (mut app, _) = tunnelled_app();
        app.serving.as_mut().unwrap().public = None;
        app.on_key(key('c'));
        assert!(!app.serve_qr);
        app.serve_qr = true;
        let (buf, _) = draw(&mut app, 120, 50);
        assert_eq!(code_rows(&buf), 0);
    }

    /// Reopening the panel starts without the code, whatever it was left with.
    #[test]
    fn the_serve_panel_opens_without_a_code() {
        let (mut app, _) = tunnelled_app();
        app.serve_qr = true;
        app.mode = Mode::List;
        app.on_key(key('B'));
        assert_eq!(app.mode, Mode::Serve);
        assert!(!app.serve_qr);
    }

    fn shared_app() -> App {
        let mut app = test_app();
        app.share_qr = Some(ShareQr {
            label: "fix the flaky test".to_string(),
            link: format!(
                "https://abcdef0123456789.lhr.life/s/{}#t={}&k={}",
                "0123456789abcdef",
                "fedcba9876543210".repeat(2),
                "00112233445566778899aabbccddeeff"
            ),
            pin: Some("482913".to_string()),
        });
        app.mode = Mode::ShareQr;
        app
    }

    /// A terminal shared with `W` comes up as a code when there is room, with
    /// the pairing code beside it and its dismiss chip still a click target.
    #[test]
    fn a_shared_terminal_is_offered_as_a_code() {
        let mut app = shared_app();
        let link = app.share_qr.as_ref().unwrap().link.clone();
        let expected = crate::ui::qr::encode(&link).expect("encodes").height as usize;
        let (buf, layout) = draw(&mut app, 120, 50);
        assert_eq!(code_rows(&buf), expected);
        let text = text_rows(&buf, SHARE);
        assert!(text.iter().any(|row| row.contains("482913")), "{text:#?}");
        assert!(
            !text.iter().any(|row| row.contains("fedcba98")),
            "the link drawn as text: {text:#?}"
        );

        let (row, x) = (0..buf.area.height)
            .find_map(|y| {
                let line = row_text(&buf, y);
                line.find("[any key]")
                    .map(|at| (y, line[..at].chars().count() as u16))
            })
            .expect("a dismiss chip");
        let chip = layout.key_at(x + 1, row).expect("the chip is clickable");
        app.on_key(chip);
        assert_eq!(app.mode, Mode::List);
        assert!(app.share_qr.is_none(), "the link outlived its panel");
    }

    /// Too short for the code: the panel still opens — the share happened —
    /// and says the code is what is missing.
    #[test]
    fn a_shared_terminal_without_the_room_says_so_instead() {
        let mut app = shared_app();
        let (buf, layout) = draw(&mut app, 120, 24);
        assert_eq!(code_rows(&buf), 0);
        let text = text_rows(&buf, SHARE);
        assert!(
            text.iter()
                .any(|row| row.starts_with("No room here for a QR code")),
            "{text:#?}"
        );
        assert!(text.iter().any(|row| row.contains("482913")));
        assert!(layout.modal_rect.is_some());
        assert!(!layout.key_hits.is_empty());
    }
}
