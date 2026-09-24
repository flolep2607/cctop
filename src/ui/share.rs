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
}
