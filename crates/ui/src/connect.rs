//! Connecting a Cloudflare account from the dashboard: the popup behind `a` in
//! the serve panel and the `cloudflare` row in Settings.
//!
//! Everything it does is [`cctop_core::cloudflare`]'s and
//! [`cctop_core::tunnel`]'s, the same calls `cctop tunnel setup` and `remove`
//! make; this only draws them and keeps them off the UI thread. Every call to
//! Cloudflare runs on a thread of its own with a spinner on screen, the way
//! `t`'s tunnel does ([`share::Opening`](super::share::Opening)), and nothing
//! is written to stderr, which a TUI would have painted over.
//!
//! # Ways in
//!
//! The first step is laid out as one of several [`Method`]s, of which there is
//! one today: paste a token made from a pre-filled link. A browser login is the
//! follow-up the maintainer asked for, since pasting a token can be a chore —
//! it becomes a second variant with its own body under the same title, and the
//! popup grows a row to choose between them. Nothing else moves.
//!
//! # Credentials on screen
//!
//! Never. The paste field is masked from the first character, the tokens are
//! held only inside the [`Step`]s that need them, and every sentence the popup
//! or the status line says is Cloudflare's error or one written here — neither
//! of which quotes the token.

use super::*;
use cctop_core::cloudflare::{self, Pasted, Zone};
use cctop_core::tunnel::Account;
use line_edit::LineEdit;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// The ways to connect an account, in the order the popup offers them.
///
/// ponytail: one, until the browser login lands; see the module docs.
pub const METHODS: [Method; 1] = [Method::Paste];

/// One way to connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Make an API token from the pre-filled link, paste it. A tunnel token
    /// from the dashboard works in the same field.
    Paste,
}

impl Method {
    /// What the popup calls it.
    pub fn label(self) -> &'static str {
        match self {
            Method::Paste => "Paste a token",
        }
    }
}

/// Where the popup is.
pub enum Step {
    /// Already connected: what, and the way to disconnect.
    Connected { account: Account, confirm: bool },
    /// The ways in. For [`Method::Paste`]: the link, and the masked field.
    Start { method: Method, field: LineEdit },
    /// The token worked; which domain. `token` is the API token.
    Zones {
        token: String,
        zones: Vec<Zone>,
        cursor: usize,
    },
    /// The domain is picked; the hostname, pre-filled.
    Hostname {
        token: String,
        zone: Zone,
        field: LineEdit,
    },
    /// A tunnel token was pasted: the hostname it serves, if the user knows
    /// it. Left empty, it is learned from the configuration the edge pushes.
    TunnelHostname { token: String, field: LineEdit },
    /// Connected just now, at this hostname when it is known.
    /// `shares` when it has a share hostname too, which only one cctop
    /// created has.
    Done {
        hostname: Option<String>,
        shares: bool,
    },
    /// What stopped it, in Cloudflare's or cctop's words.
    Failed { message: String },
    /// Disconnected, and anything left on Cloudflare to delete by hand.
    Disconnected { left: Vec<String>, made_here: bool },
}

/// What a worker thread comes back with.
enum Answer {
    Zones(String, Result<Vec<Zone>, cloudflare::Error>),
    Suggested(String, Zone, Result<String, cloudflare::Error>),
    Created(Result<Account, String>),
    Removed(Result<(Vec<String>, bool), String>),
}

/// A call to Cloudflare in flight.
pub struct Working {
    rx: Receiver<Answer>,
    /// What the spinner says.
    pub what: &'static str,
}

/// The popup's state, while `Mode::Connect` is up.
pub struct Connect {
    pub step: Step,
    /// What Esc goes back to: the serve panel, or the table under Settings.
    pub back: Mode,
    pub working: Option<Working>,
    /// Whether the link is drawn as a QR code as well.
    pub qr: bool,
    /// A problem with what was just typed, said under the field it is about
    /// rather than ending the flow.
    pub problem: Option<String>,
}

impl Connect {
    pub(super) fn new(step: Step, back: Mode) -> Connect {
        Connect {
            step,
            back,
            working: None,
            qr: false,
            problem: None,
        }
    }

    /// The first step, empty.
    pub fn start(back: Mode) -> Connect {
        Connect::new(
            Step::Start {
                method: METHODS[0],
                field: LineEdit::default(),
            },
            back,
        )
    }

    /// Whether a text field has the keyboard, so that letters are text.
    pub fn typing(&self) -> bool {
        self.working.is_none()
            && matches!(
                self.step,
                Step::Start { .. } | Step::Hostname { .. } | Step::TunnelHostname { .. }
            )
    }

    /// The field that has the keyboard, if one has.
    pub fn field(&mut self) -> Option<&mut LineEdit> {
        if self.working.is_some() {
            return None;
        }
        match &mut self.step {
            Step::Start { field, .. }
            | Step::Hostname { field, .. }
            | Step::TunnelHostname { field, .. } => Some(field),
            _ => None,
        }
    }
}

/// What the dashboard knows of the connected account, read when it may have
/// changed rather than on every frame — it lives in `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected {
    pub hostname: Option<String>,
    pub from_env: bool,
    /// The default share hostname, where an agent with no name goes.
    pub share_hostname: Option<String>,
    /// The domain addresses are chosen under.
    pub zone: Option<String>,
    /// Whether cctop can write the account's DNS, or why not.
    pub rename: Result<(), &'static str>,
}

impl From<&Account> for Connected {
    fn from(account: &Account) -> Connected {
        Connected {
            hostname: account.hostname.clone(),
            from_env: account.from_env,
            share_hostname: account.share_hostname.clone(),
            zone: account.zone_name().map(str::to_string),
            rename: account.can_rename(),
        }
    }
}

/// A refusal in the popup's words: Cloudflare's sentence, except that a
/// missing permission points at the link one Enter away rather than spelling
/// out its two hundred characters of query string.
fn said(error: &cloudflare::Error) -> String {
    match error {
        cloudflare::Error::MissingPermission(permission) => format!(
            "That token lacks the permission \"{permission}\". Enter goes back to the \
             link, which makes one with all three."
        ),
        other => other.to_string(),
    }
}

/// How long a token can be: Cloudflare's are 40 characters and a tunnel
/// token a few hundred. Generous, so a paste is never cut short.
const TOKEN_MAX: usize = 4096;
/// A DNS name's limit.
const HOSTNAME_MAX: usize = 253;

impl App {
    /// Read the connected account again: after a connect or a disconnect here,
    /// and whenever `config.toml` changed, since `cctop tunnel setup` in
    /// another terminal writes the same table.
    pub(super) fn refresh_connected(&mut self) {
        self.connected = cctop_core::tunnel::account().map(|a| Connected::from(&a));
    }

    /// Open the popup over `back`: managing the account when one is
    /// connected, the first step of connecting one when not.
    pub(super) fn open_connect(&mut self, back: Mode) {
        let account = cctop_core::tunnel::account();
        self.connected = account.as_ref().map(Connected::from);
        self.connect = Some(match account {
            Some(account) => Connect::new(
                Step::Connected {
                    account,
                    confirm: false,
                },
                back,
            ),
            None => Connect::start(back),
        });
        self.mode = Mode::Connect;
        self.needs_redraw = true;
    }

    /// Close the popup, back to wherever it was opened from. A call still in
    /// flight finishes on its own thread: the two that write (create, remove)
    /// write `config.toml` there too, so closing never leaves Cloudflare and
    /// the file disagreeing.
    pub(super) fn close_connect(&mut self) {
        let back = self.connect.take().map_or(Mode::List, |c| c.back);
        self.mode = back;
        self.needs_redraw = true;
    }

    /// Run `call` off the UI thread, with `what` on the spinner.
    fn connect_work(&mut self, what: &'static str, call: impl FnOnce() -> Answer + Send + 'static) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(call());
        });
        flow.problem = None;
        flow.working = Some(Working { rx, what });
        self.needs_redraw = true;
    }

    /// Enter on the first step: tell the paste apart and go on.
    fn submit_token(&mut self) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::Start { field, .. } = &mut flow.step else {
            return;
        };
        let pasted = field.trim().to_string();
        if pasted.is_empty() {
            flow.problem = Some("Paste the token first.".to_string());
            return;
        }
        field.clear();
        match cloudflare::classify(&pasted) {
            Pasted::Tunnel(token) => match cloudflare::check_tunnel_token(&token) {
                Ok(()) => {
                    flow.step = Step::TunnelHostname {
                        token,
                        field: LineEdit::default(),
                    };
                    flow.problem = None;
                }
                Err(why) => flow.problem = Some(why),
            },
            Pasted::Api(token) => self.connect_work("Checking the token with Cloudflare…", {
                move || {
                    let api = cloudflare::Api::new(&token);
                    let zones = api
                        .verify()
                        .and_then(|()| api.zones())
                        .and_then(cloudflare::usable);
                    Answer::Zones(token, zones)
                }
            }),
        }
    }

    /// A domain is picked: find the name to offer for it.
    fn pick_zone(&mut self) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::Zones {
            token,
            zones,
            cursor,
        } = &flow.step
        else {
            return;
        };
        let (token, zone) = (token.clone(), zones[(*cursor).min(zones.len() - 1)].clone());
        self.suggest_for(token, zone);
    }

    fn suggest_for(&mut self, token: String, zone: Zone) {
        self.connect_work("Looking for a free name…", move || {
            let api = cloudflare::Api::new(&token);
            let suggested = cloudflare::suggest_hostname(&api, &zone, &cloudflare::machine_label());
            Answer::Suggested(token, zone, suggested)
        });
    }

    /// Enter on the hostname: create everything, and store it.
    fn create_tunnel(&mut self) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::Hostname { token, zone, field } = &flow.step else {
            return;
        };
        // Checked here first, so a typo is a line under the field and not a
        // spinner followed by a dead end.
        if let Err(e) = cloudflare::check_hostname(field, zone) {
            flow.problem = Some(e.to_string());
            return;
        }
        let (token, zone, hostname) = (token.clone(), zone.clone(), field.to_string());
        self.connect_work("Creating the tunnel and its DNS records…", move || {
            let api = cloudflare::Api::new(&token);
            let made = cloudflare::create(&api, &zone, &hostname, &cloudflare::machine_label())
                .map_err(|e| e.to_string())
                .and_then(|account| {
                    // Saved here and not by whoever receives this: the popup
                    // may be closed by now, and a tunnel made on Cloudflare
                    // that nothing remembers is one nothing can remove.
                    cctop_core::tunnel::save_account(&account)
                        .map(|()| account)
                        .map_err(|e| format!("Could not write config.toml: {e}"))
                });
            Answer::Created(made)
        });
    }

    /// Enter on a tunnel token's hostname: store it. Nothing on Cloudflare is
    /// created for one, so there is nothing to wait for.
    fn save_tunnel_token(&mut self) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::TunnelHostname { token, field } = &flow.step else {
            return;
        };
        let hostname =
            Some(field.trim().trim_end_matches('.').to_ascii_lowercase()).filter(|h| !h.is_empty());
        let account = Account {
            token: token.clone(),
            hostname: hostname.clone(),
            ..Account::default()
        };
        match cctop_core::tunnel::save_account(&account) {
            Ok(()) => {
                flow.step = Step::Done {
                    hostname,
                    shares: false,
                };
                flow.problem = None;
                self.connected = Some(Connected::from(&account));
            }
            Err(e) => flow.problem = Some(format!("Could not write config.toml: {e}")),
        }
    }

    /// `y` on the confirmation: delete what setup made, and forget it.
    fn disconnect(&mut self) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::Connected { account, .. } = &flow.step else {
            return;
        };
        let account = account.clone();
        if account.from_env {
            flow.step = Step::Failed {
                message: "That tunnel comes from CCTOP_TUNNEL_TOKEN, not from setup: unset \
                          it to stop using it, and delete the tunnel in the Cloudflare \
                          dashboard."
                    .to_string(),
            };
            return;
        }
        // This dashboard's own serve would be pulled out from under itself;
        // it is stopped first, which the confirmation said.
        if self.serving_on_account() {
            self.stop_serving();
        }
        if cctop_core::tunnel::in_use(&account) {
            if let Some(flow) = self.connect.as_mut() {
                flow.step = Step::Failed {
                    message: "Another cctop on this machine is serving over this tunnel. \
                              Stop it first, then disconnect."
                        .to_string(),
                };
            }
            return;
        }
        self.connect_work("Deleting the tunnel and its DNS records…", move || {
            let left = cloudflare::remove(&account);
            Answer::Removed(
                cctop_core::tunnel::clear_account()
                    .map(|()| (left.0, account.api_token.is_some()))
                    .map_err(|e| format!("Could not update config.toml: {e}")),
            )
        });
    }

    /// Whether this dashboard is serving over the account's tunnel right now,
    /// rather than a quick one.
    pub(super) fn serving_on_account(&self) -> bool {
        let Some(public) = self.serving.as_ref().and_then(|s| s.public.as_deref()) else {
            return false;
        };
        let host = public
            .strip_prefix("https://")
            .unwrap_or(public)
            .split(['/', '?'])
            .next()
            .unwrap_or_default();
        self.connected
            .as_ref()
            .and_then(|c| c.hostname.as_deref())
            .is_some_and(|h| h.eq_ignore_ascii_case(host))
    }

    /// Take a worker's answer, if one has come. Returns whether the screen
    /// changed — every tick while one is in flight, for the spinner.
    pub(super) fn tick_connect(&mut self) -> bool {
        let Some(flow) = self.connect.as_mut() else {
            return false;
        };
        let Some(working) = &flow.working else {
            return false;
        };
        let answer = match working.rx.try_recv() {
            Err(TryRecvError::Empty) => return true,
            Ok(answer) => answer,
            Err(TryRecvError::Disconnected) => {
                flow.working = None;
                flow.step = Step::Failed {
                    message: "The call to Cloudflare gave no answer.".to_string(),
                };
                return true;
            }
        };
        flow.working = None;
        self.connect_answer(answer);
        true
    }

    fn connect_answer(&mut self, answer: Answer) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        match answer {
            Answer::Zones(token, Ok(zones)) => match zones.len() {
                // One domain is no choice: straight on to its name.
                1 => {
                    let zone = zones.into_iter().next().expect("one zone");
                    self.suggest_for(token, zone);
                }
                _ => {
                    flow.step = Step::Zones {
                        token,
                        zones,
                        cursor: 0,
                    }
                }
            },
            Answer::Suggested(token, zone, Ok(name)) => {
                flow.step = Step::Hostname {
                    token,
                    zone,
                    field: name.into(),
                };
            }
            // Every name it would offer is taken: an empty field to type one.
            Answer::Suggested(token, zone, Err(cloudflare::Error::Hostname(why))) => {
                flow.step = Step::Hostname {
                    token,
                    zone,
                    field: LineEdit::default(),
                };
                flow.problem = Some(why);
            }
            Answer::Zones(_, Err(e)) | Answer::Suggested(_, _, Err(e)) => {
                flow.step = Step::Failed { message: said(&e) };
            }
            Answer::Created(Ok(account)) => {
                flow.step = Step::Done {
                    hostname: account.hostname.clone(),
                    shares: account.share_hostname.is_some(),
                };
                self.connected = Some(Connected::from(&account));
                self.set_status("Cloudflare account connected");
            }
            Answer::Created(Err(message)) => {
                // A taken or malformed name is the user's to fix where they
                // typed it, not a reason to start over.
                if let Step::Hostname { .. } = flow.step
                    && message.contains("DNS record")
                {
                    flow.problem = Some(message);
                } else {
                    flow.step = Step::Failed { message };
                }
            }
            Answer::Removed(Ok((left, made_here))) => {
                flow.step = Step::Disconnected { left, made_here };
                self.connected = None;
                self.set_status("Cloudflare account disconnected");
            }
            Answer::Removed(Err(message)) => flow.step = Step::Failed { message },
        }
    }

    /// `s` on the last step: serve over the account now.
    fn serve_now(&mut self) {
        self.connect = None;
        // A serve already up is on a quick tunnel or none; this one is the
        // point of having connected.
        self.stop_serving();
        self.start_serving(true);
        self.mode = Mode::Serve;
        self.serve_qr = false;
    }

    /// Copy the token link, and open it where a browser would be on this
    /// machine — the add-account popup's `Ctrl+O`.
    fn copy_token_link(&mut self) {
        let link = cloudflare::token_link();
        render::copy_to_clipboard(&link);
        let opened = !render::over_ssh() && share::open_in_browser(&link);
        self.set_status(match opened {
            true => "Copied the token link, and asked the browser to open it",
            false => "Copied the token link — paste it into your browser",
        });
    }

    pub(super) fn on_key_connect(&mut self, key: KeyEvent) {
        let Some(flow) = self.connect.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if flow.working.is_some() {
            if key.code == KeyCode::Esc {
                self.close_connect();
            }
            return;
        }
        match &mut flow.step {
            Step::Start { field, .. } => match key.code {
                KeyCode::Esc => self.close_connect(),
                KeyCode::Enter => self.submit_token(),
                KeyCode::Char('o') if ctrl => self.copy_token_link(),
                KeyCode::Char('q') if ctrl => flow.qr = !flow.qr,
                _ => {
                    if field.key(key, TOKEN_MAX).changed() {
                        flow.problem = None;
                    }
                }
            },
            Step::Zones { zones, cursor, .. } => match key.code {
                KeyCode::Esc => self.close_connect(),
                KeyCode::Up | KeyCode::Char('k') => *cursor = cursor.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    *cursor = (*cursor + 1).min(zones.len().saturating_sub(1))
                }
                KeyCode::Enter => self.pick_zone(),
                _ => {}
            },
            Step::Hostname { field, .. } => match key.code {
                KeyCode::Esc => self.close_connect(),
                KeyCode::Enter => self.create_tunnel(),
                _ => {
                    if field.key(key, HOSTNAME_MAX).changed() {
                        flow.problem = None;
                    }
                }
            },
            Step::TunnelHostname { field, .. } => match key.code {
                KeyCode::Esc => self.close_connect(),
                KeyCode::Enter => self.save_tunnel_token(),
                _ => {
                    field.key(key, HOSTNAME_MAX);
                }
            },
            Step::Done { .. } => match key.code {
                KeyCode::Char('s') => self.serve_now(),
                KeyCode::Esc | KeyCode::Enter => self.close_connect(),
                _ => {}
            },
            Step::Failed { .. } => match key.code {
                // Back to the start rather than out: the usual next move is
                // another token, made with the permission that was missing.
                KeyCode::Enter => {
                    let back = flow.back;
                    *flow = Connect::start(back);
                }
                KeyCode::Esc => self.close_connect(),
                _ => {}
            },
            Step::Connected { confirm, .. } => match (key.code, *confirm) {
                (KeyCode::Char('d'), false) => *confirm = true,
                (KeyCode::Char('y'), true) => self.disconnect(),
                (KeyCode::Char('n') | KeyCode::Esc, true) => *confirm = false,
                (KeyCode::Esc | KeyCode::Enter, false) => self.close_connect(),
                _ => {}
            },
            Step::Disconnected { .. } => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                    self.close_connect();
                }
            }
        }
    }

    /// A paste goes to whichever field has the keyboard.
    pub(super) fn paste_connect(&mut self, text: &str) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let cap = match flow.step {
            Step::Start { .. } => TOKEN_MAX,
            _ => HOSTNAME_MAX,
        };
        if let Some(field) = flow.field() {
            // A token has no inner whitespace, and a paste from a web page
            // often brings a newline with it.
            let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            field.insert_str(&text, cap);
            flow.problem = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::tests::test_app;

    /// The popup over the serve panel, opened without reading anybody's
    /// config.
    fn app() -> App {
        let mut app = test_app();
        app.open_connect_at(Connect::start(Mode::Serve));
        app
    }

    impl App {
        pub(crate) fn open_connect_at(&mut self, flow: Connect) {
            self.connect = Some(flow);
            self.mode = Mode::Connect;
        }
    }

    fn zone(name: &str) -> Zone {
        Zone {
            id: format!("id-{name}"),
            name: name.to_string(),
            account_id: "acct".to_string(),
            active: true,
        }
    }

    fn step(app: &App) -> &Step {
        &app.connect.as_ref().expect("the popup").step
    }

    #[test]
    fn a_paste_is_masked_trimmed_and_never_on_screen() {
        let mut app = app();
        app.paste_connect("  made-up-api-token\n");
        let Step::Start { field, .. } = step(&app) else {
            panic!("not the first step");
        };
        assert_eq!(field.to_string(), "made-up-api-token");
    }

    #[test]
    fn an_empty_paste_is_a_problem_not_a_call() {
        let mut app = app();
        app.on_key_connect(KeyCode::Enter.into());
        let flow = app.connect.as_ref().unwrap();
        assert!(flow.working.is_none());
        assert!(flow.problem.is_some());
    }

    #[test]
    fn a_bad_tunnel_token_is_said_without_repeating_it() {
        let mut app = app();
        app.paste_connect("eyJhIjoimade-up-and-broken");
        app.on_key_connect(KeyCode::Enter.into());
        let flow = app.connect.as_ref().unwrap();
        let problem = flow.problem.clone().expect("a problem");
        assert!(!problem.contains("made-up"), "{problem}");
        assert!(matches!(flow.step, Step::Start { .. }));
    }

    #[test]
    fn several_domains_are_a_choice_and_one_is_not() {
        let mut app = app();
        app.connect_answer(Answer::Zones(
            "made-up".into(),
            Ok(vec![zone("a.test"), zone("b.test")]),
        ));
        assert!(matches!(step(&app), Step::Zones { zones, .. } if zones.len() == 2));
        app.on_key_connect(KeyCode::Down.into());
        assert!(matches!(step(&app), Step::Zones { cursor: 1, .. }));

        let mut app = self::app();
        app.connect_answer(Answer::Zones("made-up".into(), Ok(vec![zone("a.test")])));
        // Straight on to finding a name, without asking which of one.
        let flow = app.connect.as_ref().unwrap();
        assert_eq!(
            flow.working.as_ref().map(|w| w.what),
            Some("Looking for a free name…")
        );
    }

    #[test]
    fn each_refusal_ends_on_its_own_sentence() {
        for error in [
            cloudflare::Error::NoDomain,
            cloudflare::Error::ZonePending("example.test".into()),
            cloudflare::Error::TokenRefused,
            cloudflare::Error::MissingPermission(cloudflare::PERMISSIONS[0]),
        ] {
            let mut app = app();
            app.connect_answer(Answer::Zones("made-up".into(), Err(error.clone())));
            let Step::Failed { message } = step(&app) else {
                panic!("{error:?} did not fail");
            };
            assert_eq!(*message, said(&error));
            assert!(!message.contains("permissionGroupKeys"), "{message}");
            // Enter starts over, with a fresh field.
            app.on_key_connect(KeyCode::Enter.into());
            assert!(matches!(step(&app), Step::Start { field, .. } if field.is_empty()));
        }
    }

    #[test]
    fn a_taken_name_stays_on_the_name() {
        let mut app = app();
        app.connect_answer(Answer::Suggested(
            "made-up".into(),
            zone("example.test"),
            Ok("cctop.example.test".into()),
        ));
        assert!(
            matches!(step(&app), Step::Hostname { field, .. } if *field == "cctop.example.test")
        );
        app.connect_answer(Answer::Created(Err(
            "cctop.example.test already has a DNS record that cctop did not make".into(),
        )));
        let flow = app.connect.as_ref().unwrap();
        assert!(matches!(flow.step, Step::Hostname { .. }));
        assert!(
            flow.problem
                .as_deref()
                .is_some_and(|p| p.contains("DNS record"))
        );
    }

    #[test]
    fn a_deeper_name_is_refused_before_any_call() {
        let mut app = app();
        app.connect_answer(Answer::Suggested(
            "made-up".into(),
            zone("example.test"),
            Ok("a.b.example.test".into()),
        ));
        app.on_key_connect(KeyCode::Enter.into());
        let flow = app.connect.as_ref().unwrap();
        assert!(flow.working.is_none());
        assert!(
            flow.problem
                .as_deref()
                .is_some_and(|p| p.contains("two levels"))
        );
    }

    #[test]
    fn created_is_done_and_remembered() {
        let mut app = app();
        app.connect_answer(Answer::Created(Ok(Account {
            token: "made-up".into(),
            hostname: Some("cctop.example.test".into()),
            ..Account::default()
        })));
        assert!(
            matches!(step(&app), Step::Done { hostname: Some(h), .. } if h == "cctop.example.test")
        );
        assert_eq!(
            app.connected.as_ref().and_then(|c| c.hostname.as_deref()),
            Some("cctop.example.test")
        );
        app.on_key_connect(KeyCode::Esc.into());
        assert!(app.connect.is_none());
        assert_eq!(app.mode, Mode::Serve, "back where it was opened");
    }

    #[test]
    fn disconnect_asks_first() {
        let mut app = test_app();
        app.open_connect_at(Connect::new(
            Step::Connected {
                account: Account {
                    token: "made-up".into(),
                    from_env: true,
                    ..Account::default()
                },
                confirm: false,
            },
            Mode::List,
        ));
        app.on_key_connect(KeyCode::Char('y').into());
        assert!(matches!(step(&app), Step::Connected { confirm: false, .. }));
        app.on_key_connect(KeyCode::Char('d').into());
        assert!(matches!(step(&app), Step::Connected { confirm: true, .. }));
        app.on_key_connect(KeyCode::Char('n').into());
        assert!(matches!(step(&app), Step::Connected { confirm: false, .. }));
        // One from the environment is not cctop's to delete.
        app.on_key_connect(KeyCode::Char('d').into());
        app.on_key_connect(KeyCode::Char('y').into());
        assert!(
            matches!(step(&app), Step::Failed { message } if message.contains("CCTOP_TUNNEL_TOKEN"))
        );
    }
}
