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
//! The first step is one of two [`Method`]s, side by side on a row that Tab
//! moves along: log in through the browser, the way `cloudflared tunnel login`
//! does, or paste a token made from a pre-filled link. The login comes first,
//! since pasting a token is the chore it exists to spare.
//!
//! A login opens the authorize page in this machine's browser, except over
//! ssh, where that browser is not the user's: the address is drawn instead,
//! with a QR code a key away, and the login is waited for from here either
//! way. Both ways then meet at the same domain and hostname steps, carrying an
//! [`Auth`] that says which kind of token they hold.
//!
//! # Who may log in
//!
//! `a` on the connected account opens [`Step::Access`]: Cloudflare Access on
//! or off, the invite list, and whether token links still work from outside.
//! Every edit is [`access_setup::apply`], the one `cctop tunnel access` and the
//! web page make too, on a thread of its own like every other call here; it
//! saves `config.toml` there, so a popup closed mid-call still leaves the file
//! and Cloudflare agreeing.
//!
//! # Credentials on screen
//!
//! Never. The paste field is masked from the first character, the tokens are
//! held only inside the [`Step`]s that need them, and every sentence the popup
//! or the status line says is Cloudflare's error or one written here — neither
//! of which quotes the token.

use super::*;
use cctop_core::cloudflare::access::Level;
use cctop_core::cloudflare::access_setup::{self, Applied, Change};
use cctop_core::cloudflare::login::Login;
use cctop_core::cloudflare::{self, Auth, Pasted, Zone};
use cctop_core::tunnel::Account;
use line_edit::LineEdit;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// The ways to connect an account, in the order the popup offers them.
pub const METHODS: [Method; 2] = [Method::Browser, Method::Paste];

/// One way to connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Log in on Cloudflare's own page and pick the domain there; a
    /// certificate comes back with a token in it.
    Browser,
    /// Make an API token from the pre-filled link, paste it. A tunnel token
    /// from the dashboard works in the same field.
    Paste,
}

impl Method {
    /// What the popup calls it.
    pub fn label(self) -> &'static str {
        match self {
            Method::Browser => "Log in with browser",
            Method::Paste => "Paste a token",
        }
    }

    /// The one after it on the row, round to the first.
    pub fn next(self) -> Method {
        let at = METHODS.iter().position(|m| *m == self).unwrap_or(0);
        METHODS[(at + 1) % METHODS.len()]
    }
}

/// Where the popup is.
pub enum Step {
    /// Already connected: what, and the way to disconnect.
    Connected { account: Account, confirm: bool },
    /// The ways in. For [`Method::Paste`]: the link, and the masked field.
    Start { method: Method, field: LineEdit },
    /// The login page is open, or its address is on screen, and the
    /// certificate is being waited for. `opened` when a browser here was
    /// asked to open it.
    LoggingIn { url: String, opened: bool },
    /// The token worked; which domain.
    Zones {
        auth: Auth,
        zones: Vec<Zone>,
        cursor: usize,
    },
    /// The domain is picked; the hostname, pre-filled. A login whose
    /// domain's name could not be read has a `zone` with no name, and the
    /// whole hostname is typed ([`cloudflare::login_zone_from`]).
    Hostname {
        auth: Auth,
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
    /// What stopped it, in Cloudflare's or cctop's words, and the way in
    /// that Enter starts over at.
    Failed { message: String, then: Method },
    /// Disconnected, and anything left on Cloudflare to delete by hand.
    Disconnected { left: Vec<String>, made_here: bool },
    /// Cloudflare Access on the connected account: on or off, the invites
    /// with one under the cursor, and the token links' switch.
    Access {
        account: Account,
        cursor: usize,
        /// A field open for an address, and what Enter does with it.
        field: Option<(Typing, LineEdit)>,
        /// Asking before Access is turned off.
        confirm: bool,
        /// What the last edit said, or left behind on Cloudflare.
        said: Vec<String>,
    },
}

/// What the Access step's field is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typing {
    /// The owner's email, to turn Access on.
    Owner,
    /// An email or `@domain` to invite, read-only.
    Invite,
}

/// What a worker thread comes back with.
enum Answer {
    /// The login's token, and its domain: named, or with no name when it
    /// could not be read.
    LoggedIn(Result<(Auth, Zone), cloudflare::Error>),
    Zones(Auth, Result<Vec<Zone>, cloudflare::Error>),
    Suggested(Auth, Zone, Result<String, cloudflare::Error>),
    Created(Result<Account, String>),
    Removed(Result<(Vec<String>, bool), String>),
    Access(Result<Applied, String>),
}

/// How an Access edit is made: [`access_setup::apply`], except in a test,
/// which applies it to an account of its own against the fake API.
pub type ApplyAccess = Arc<dyn Fn(Change) -> Result<Applied, String> + Send + Sync>;

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
    /// Set when this popup goes, so a login still waiting stops polling
    /// rather than holding a thread for the ten minutes it would wait.
    cancel: Arc<AtomicBool>,
    /// Where a login starts: Cloudflare, and in a test the fake, so that no
    /// test polls the real store.
    new_login: Box<dyn Fn() -> Login>,
    apply_access: ApplyAccess,
}

impl Drop for Connect {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Connect {
    pub(super) fn new(step: Step, back: Mode) -> Connect {
        Connect {
            step,
            back,
            working: None,
            qr: false,
            problem: None,
            cancel: Arc::default(),
            new_login: Box::new(Login::new),
            apply_access: Arc::new(access_setup::apply),
        }
    }

    /// The first step, empty.
    pub fn start(back: Mode) -> Connect {
        Connect::start_at(back, METHODS[0])
    }

    /// The first step, empty, with `method` chosen.
    fn start_at(back: Mode, method: Method) -> Connect {
        Connect::new(
            Step::Start {
                method,
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
                Step::Start {
                    method: Method::Paste,
                    ..
                } | Step::Hostname { .. }
                    | Step::TunnelHostname { .. }
                    | Step::Access { field: Some(_), .. }
            )
    }

    /// The field that has the keyboard, if one has.
    pub fn field(&mut self) -> Option<&mut LineEdit> {
        if self.working.is_some() {
            return None;
        }
        match &mut self.step {
            Step::Start {
                field,
                method: Method::Paste,
            }
            | Step::Hostname { field, .. }
            | Step::TunnelHostname { field, .. }
            | Step::Access {
                field: Some((_, field)),
                ..
            } => Some(field),
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
    /// Who owns the page's Cloudflare Access, while it is on.
    pub access_owner: Option<String>,
    /// Where token links go while Access is on, and whether they work from
    /// outside at all.
    pub link_hostname: Option<String>,
    pub public_links: bool,
}

impl From<&Account> for Connected {
    fn from(account: &Account) -> Connected {
        Connected {
            hostname: account.hostname.clone(),
            from_env: account.from_env,
            share_hostname: account.share_hostname.clone(),
            zone: account.zone_name().map(str::to_string),
            rename: account.can_rename(),
            access_owner: account.access.as_ref().map(|a| a.owner.clone()),
            link_hostname: account
                .access
                .as_ref()
                .and_then(|a| a.link_hostname.clone()),
            public_links: account.access.as_ref().is_none_or(|a| a.public_links),
        }
    }
}

/// A refusal in the popup's words: Cloudflare's sentence, except that one
/// whose way out is a new token points at the token link one Enter away
/// rather than spelling out its five hundred characters of query string, which
/// wrap across the popup's border and cannot be copied or clicked.
fn said(error: &cloudflare::Error) -> String {
    match error {
        cloudflare::Error::MissingPermission(permission) => format!(
            "That token lacks the permission \"{permission}\". Enter goes back to the \
             link, which makes one with all three."
        ),
        cloudflare::Error::LoginRefused(permission) => format!(
            "Cloudflare did not let the browser login do this (it needs \
             \"{permission}\"). Enter goes to the token link instead, which makes a \
             token that can."
        ),
        other => other.to_string(),
    }
}

/// The failed step for `error`: back to the token link when a token is the
/// way out, back to the first way in otherwise.
fn failed(error: &cloudflare::Error) -> Step {
    let then = match error {
        cloudflare::Error::MissingPermission(_) | cloudflare::Error::LoginRefused(_) => {
            Method::Paste
        }
        _ => METHODS[0],
    };
    Step::Failed {
        message: said(error),
        then,
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

    /// Enter on the first step: log in, or tell the paste apart and go on.
    fn submit_token(&mut self) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::Start { field, method } = &mut flow.step else {
            return;
        };
        if *method == Method::Browser {
            self.log_in();
            return;
        }
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
                    Answer::Zones(Auth::Pasted(token), zones)
                }
            }),
        }
    }

    /// Start a browser login: open the page where a browser here would be
    /// the user's, and wait for the certificate off the UI thread.
    fn log_in(&mut self) {
        let Some(flow) = self.connect.as_ref() else {
            return;
        };
        let login = (flow.new_login)();
        let url = login.url().to_string();
        // A test drives this with no browser to bother.
        let opened = !cfg!(test) && !render::over_ssh() && share::open_in_browser(&url);
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        flow.step = Step::LoggingIn { url, opened };
        // Over ssh the code is the quickest way to a browser, on a phone.
        flow.qr = !opened;
        let cancel = flow.cancel.clone();
        self.connect_work("Waiting for the login in the browser…", move || {
            let logged_in = login.wait(&|| cancel.load(Ordering::Relaxed)).map(|cert| {
                let auth = cert.auth();
                let zone = cloudflare::Api::with(&auth)
                    .login_zone(&cert)
                    .unwrap_or_else(|| Zone {
                        id: cert.zone_id.clone(),
                        name: String::new(),
                        account_id: cert.account_id.clone(),
                        active: true,
                    });
                (auth, zone)
            });
            Answer::LoggedIn(logged_in)
        });
    }

    /// Copy the login page's address, the way [`App::copy_token_link`]
    /// copies the token link — the one thing to do with it over ssh.
    fn copy_login_link(&mut self) {
        let Some(Step::LoggingIn { url, .. }) = self.connect.as_ref().map(|c| &c.step) else {
            return;
        };
        render::copy_to_clipboard(url);
        self.set_status("Copied the login link — open it in your browser");
    }

    /// A domain is picked: find the name to offer for it.
    fn pick_zone(&mut self) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::Zones {
            auth,
            zones,
            cursor,
        } = &flow.step
        else {
            return;
        };
        let (auth, zone) = (auth.clone(), zones[(*cursor).min(zones.len() - 1)].clone());
        self.suggest_for(auth, zone);
    }

    fn suggest_for(&mut self, auth: Auth, zone: Zone) {
        self.connect_work("Looking for a free name…", move || {
            let api = cloudflare::Api::with(&auth);
            let suggested = cloudflare::suggest_hostname(&api, &zone, &cloudflare::machine_label());
            Answer::Suggested(auth, zone, suggested)
        });
    }

    /// Enter on the hostname: create everything, and store it.
    fn create_tunnel(&mut self) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::Hostname { auth, zone, field } = &flow.step else {
            return;
        };
        // A login's domain with no name takes it from what was typed.
        let zone = match zone.name.is_empty() {
            true => match cloudflare::login_zone_from(&zone.id, &zone.account_id, field) {
                Ok(zone) => zone,
                Err(e) => {
                    flow.problem = Some(e.to_string());
                    return;
                }
            },
            false => zone.clone(),
        };
        // Checked here first, so a typo is a line under the field and not a
        // spinner followed by a dead end.
        if let Err(e) = cloudflare::check_hostname(field, &zone) {
            flow.problem = Some(e.to_string());
            return;
        }
        let (auth, hostname) = (auth.clone(), field.to_string());
        self.connect_work("Creating the tunnel and its DNS records…", move || {
            let api = cloudflare::Api::with(&auth);
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
                then: METHODS[0],
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
                    then: METHODS[0],
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
        self.connected.as_ref().is_some_and(|c| {
            [&c.hostname, &c.link_hostname]
                .into_iter()
                .flatten()
                .any(|h| h.eq_ignore_ascii_case(host))
        })
    }

    /// `a` on the connected account: its Access, as `config.toml` has it.
    fn open_access(&mut self) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::Connected { account, .. } = &flow.step else {
            return;
        };
        flow.step = Step::Access {
            account: account.clone(),
            cursor: 0,
            field: None,
            confirm: false,
            said: Vec::new(),
        };
        flow.problem = None;
    }

    /// Make `change` off the UI thread.
    fn access_work(&mut self, what: &'static str, change: Change) {
        let Some(flow) = self.connect.as_ref() else {
            return;
        };
        let apply = flow.apply_access.clone();
        self.connect_work(what, move || Answer::Access(apply(change)));
    }

    /// A key on the Access step, with no field open.
    fn on_key_access(&mut self, key: KeyEvent) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::Access {
            account,
            cursor,
            field,
            confirm,
            ..
        } = &mut flow.step
        else {
            return;
        };
        let settings = account.access.as_deref().cloned();
        if *confirm {
            match key.code {
                KeyCode::Char('y') => {
                    *confirm = false;
                    self.access_work("Taking the page out of Access…", Change::Off);
                }
                KeyCode::Char('n') | KeyCode::Esc => *confirm = false,
                _ => {}
            }
            return;
        }
        let selected = settings
            .as_ref()
            .and_then(|s| s.invites.get(*cursor).cloned());
        match (key.code, &settings) {
            (KeyCode::Esc, _) => {
                let account = account.clone();
                flow.step = Step::Connected {
                    account,
                    confirm: false,
                };
                flow.problem = None;
            }
            (KeyCode::Char('o'), None) => *field = Some((Typing::Owner, LineEdit::default())),
            (KeyCode::Char('o'), Some(_)) => *confirm = true,
            (KeyCode::Char('i'), Some(_)) => *field = Some((Typing::Invite, LineEdit::default())),
            (KeyCode::Up | KeyCode::Char('k'), Some(_)) => *cursor = cursor.saturating_sub(1),
            (KeyCode::Down | KeyCode::Char('j'), Some(s)) => {
                *cursor = (*cursor + 1).min(s.invites.len().saturating_sub(1))
            }
            (KeyCode::Char('x') | KeyCode::Delete, Some(_)) => {
                if let Some(invite) = selected {
                    self.access_work(
                        "Updating who may log in…",
                        Change::Uninvite { who: invite.who },
                    );
                }
            }
            (KeyCode::Char('f'), Some(_)) => {
                if let Some(invite) = selected {
                    let level = match invite.level {
                        Level::Read => Level::Full,
                        Level::Full => Level::Read,
                    };
                    self.access_work(
                        "Updating who may log in…",
                        Change::Invite {
                            who: invite.who,
                            level,
                        },
                    );
                }
            }
            (KeyCode::Char('l'), Some(s)) => {
                let on = !s.public_links;
                self.access_work(
                    match on {
                        true => "Making the token hostname…",
                        false => "Deleting the token hostname…",
                    },
                    Change::PublicLinks(on),
                );
            }
            _ => {}
        }
    }

    /// Enter in the Access step's field.
    fn submit_access_field(&mut self) {
        let Some(flow) = self.connect.as_mut() else {
            return;
        };
        let Step::Access { field, .. } = &mut flow.step else {
            return;
        };
        let Some((typing, text)) = field.as_ref() else {
            return;
        };
        let typed = text.trim().to_string();
        let checked = cctop_core::cloudflare::access::parse_who(&typed)
            .filter(|who| *typing == Typing::Invite || !who.starts_with('@'));
        let Some(who) = checked else {
            flow.problem = Some(match typing {
                Typing::Owner => "The owner is one email address.".to_string(),
                Typing::Invite => {
                    "An email address, or a whole domain written @company.com.".to_string()
                }
            });
            return;
        };
        let typing = *typing;
        *field = None;
        match typing {
            Typing::Owner => self.access_work(
                "Putting the page behind Cloudflare Access…",
                Change::On { owner: who },
            ),
            Typing::Invite => self.access_work(
                "Updating who may log in…",
                Change::Invite {
                    who,
                    level: Level::Read,
                },
            ),
        }
    }

    /// The links this dashboard hands out, after Access or its token links
    /// changed under a serve on the account: the token hostname while public
    /// links are on, the page's bare address while they are off — a token
    /// gets nothing through the tunnel then.
    fn retarget_links(&mut self, account: &Account) {
        if !self.serving_on_account() {
            return;
        }
        let Some(page) = account.hostname.as_ref().map(|h| format!("https://{h}")) else {
            return;
        };
        let Some(serving) = self.serving.as_mut() else {
            return;
        };
        let token = |link: &str| link.split_once("?t=").map(|(_, t)| t.to_string());
        let full = token(&serving.local);
        let readonly = token(&serving.readonly);
        let origin = match account.access.as_deref() {
            Some(settings) => settings.token_origin(&page),
            None => Some(page.clone()),
        };
        match (origin, full) {
            (Some(origin), Some(full)) => {
                serving.public = Some(format!("{origin}/?t={full}"));
                if let Some(readonly) = readonly {
                    serving.readonly = format!("{origin}/?t={readonly}");
                }
            }
            (Some(origin), None) => serving.public = Some(format!("{origin}/")),
            (None, _) => serving.public = Some(format!("{page}/")),
        }
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
                    then: METHODS[0],
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
            // The domain was picked in the browser: on to its name, or —
            // when its name could not be read — to typing the whole address.
            Answer::LoggedIn(Ok((auth, zone))) => {
                flow.qr = false;
                match zone.name.is_empty() {
                    true => {
                        flow.step = Step::Hostname {
                            auth,
                            zone,
                            field: LineEdit::default(),
                        }
                    }
                    false => self.suggest_for(auth, zone),
                }
            }
            Answer::Zones(auth, Ok(zones)) => match zones.len() {
                // One domain is no choice: straight on to its name.
                1 => {
                    let zone = zones.into_iter().next().expect("one zone");
                    self.suggest_for(auth, zone);
                }
                _ => {
                    flow.step = Step::Zones {
                        auth,
                        zones,
                        cursor: 0,
                    }
                }
            },
            Answer::Suggested(auth, zone, Ok(name)) => {
                flow.step = Step::Hostname {
                    auth,
                    zone,
                    field: name.into(),
                };
            }
            // Every name it would offer is taken: an empty field to type one.
            Answer::Suggested(auth, zone, Err(cloudflare::Error::Hostname(why))) => {
                flow.step = Step::Hostname {
                    auth,
                    zone,
                    field: LineEdit::default(),
                };
                flow.problem = Some(why);
            }
            Answer::LoggedIn(Err(e))
            | Answer::Zones(_, Err(e))
            | Answer::Suggested(_, _, Err(e)) => {
                flow.qr = false;
                flow.step = failed(&e);
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
                    flow.step = Step::Failed {
                        message,
                        then: METHODS[0],
                    };
                }
            }
            Answer::Removed(Ok((left, made_here))) => {
                flow.step = Step::Disconnected { left, made_here };
                self.connected = None;
                self.set_status("Cloudflare account disconnected");
            }
            Answer::Removed(Err(message)) => {
                flow.step = Step::Failed {
                    message,
                    then: METHODS[0],
                }
            }
            Answer::Access(Ok(applied)) => {
                let Step::Access {
                    account,
                    cursor,
                    said,
                    ..
                } = &mut flow.step
                else {
                    return;
                };
                *account = applied.account.clone();
                let count = account.access.as_ref().map_or(0, |a| a.invites.len());
                *cursor = (*cursor).min(count.saturating_sub(1));
                *said = std::iter::once(applied.said.clone())
                    .chain(
                        applied
                            .left
                            .iter()
                            .map(|item| format!("Left on Cloudflare to delete by hand: {item}")),
                    )
                    .collect();
                self.connected = Some(Connected::from(&applied.account));
                self.retarget_links(&applied.account);
                self.set_status(applied.said);
            }
            Answer::Access(Err(message)) => flow.problem = Some(message),
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
            // Waiting on a login is waiting on the user, who may need the
            // link again or its code; nothing else waits on anyone.
            let login = matches!(flow.step, Step::LoggingIn { .. });
            match key.code {
                KeyCode::Esc => self.close_connect(),
                KeyCode::Char('o') if ctrl && login => self.copy_login_link(),
                KeyCode::Char('q') if ctrl && login => flow.qr = !flow.qr,
                _ => {}
            }
            return;
        }
        match &mut flow.step {
            Step::Start { field, method } => match key.code {
                KeyCode::Esc => self.close_connect(),
                KeyCode::Enter => self.submit_token(),
                KeyCode::Tab | KeyCode::BackTab => {
                    *method = method.next();
                    flow.problem = None;
                    flow.qr = false;
                }
                KeyCode::Char('o') if ctrl && *method == Method::Paste => self.copy_token_link(),
                KeyCode::Char('q') if ctrl && *method == Method::Paste => flow.qr = !flow.qr,
                // The login has no field: keys are not text there.
                _ if *method == Method::Browser => {}
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
            Step::Failed { then, .. } => match key.code {
                // Back to the start rather than out: the usual next move is
                // another token, made with the permission that was missing.
                KeyCode::Enter => {
                    let (back, then) = (flow.back, *then);
                    *flow = Connect::start_at(back, then);
                }
                KeyCode::Esc => self.close_connect(),
                _ => {}
            },
            Step::Access { field: Some(_), .. } => match key.code {
                KeyCode::Esc => {
                    if let Step::Access { field, .. } = &mut flow.step {
                        *field = None;
                    }
                    flow.problem = None;
                }
                KeyCode::Enter => self.submit_access_field(),
                _ => {
                    if let Some(field) = flow.field()
                        && field.key(key, HOSTNAME_MAX).changed()
                    {
                        flow.problem = None;
                    }
                }
            },
            Step::Access { .. } => self.on_key_access(key),
            Step::Connected { confirm, .. } => match (key.code, *confirm) {
                (KeyCode::Char('a'), false) => self.open_access(),
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
            // Only while working, handled above; a login that came back has
            // moved on to another step.
            Step::LoggingIn { .. } => {
                if key.code == KeyCode::Esc {
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
        app.open_connect_at(Connect::new(
            Step::Start {
                method: Method::Paste,
                field: LineEdit::default(),
            },
            Mode::Serve,
        ));
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
        // Each with the way in Enter starts over at: the token link when a
        // token is the way out, the first way in otherwise.
        for (error, then) in [
            (cloudflare::Error::NoDomain, METHODS[0]),
            (
                cloudflare::Error::ZonePending("example.test".into()),
                METHODS[0],
            ),
            (cloudflare::Error::TokenRefused, METHODS[0]),
            (
                cloudflare::Error::MissingPermission(cloudflare::PERMISSIONS[0]),
                Method::Paste,
            ),
            (
                cloudflare::Error::LoginRefused(cloudflare::PERMISSIONS[0]),
                Method::Paste,
            ),
        ] {
            let mut app = app();
            app.connect_answer(Answer::Zones("made-up".into(), Err(error.clone())));
            let Step::Failed { message, .. } = step(&app) else {
                panic!("{error:?} did not fail");
            };
            assert_eq!(*message, said(&error));
            assert!(!message.contains("permissionGroupKeys"), "{message}");
            app.on_key_connect(KeyCode::Enter.into());
            assert!(
                matches!(step(&app), Step::Start { field, method } if field.is_empty() && *method == then),
                "{error:?}"
            );
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
    fn tab_moves_between_the_ways_in_and_the_login_has_no_field() {
        let mut app = test_app();
        app.open_connect_at(Connect::start(Mode::Serve));
        assert!(matches!(
            step(&app),
            Step::Start {
                method: Method::Browser,
                ..
            }
        ));
        let flow = app.connect.as_ref().unwrap();
        assert!(!flow.typing(), "letters are not text on the login");
        app.paste_connect("made-up-api-token");
        app.on_key_connect(KeyCode::Char('x').into());
        app.on_key_connect(KeyCode::Tab.into());
        let Step::Start { method, field } = step(&app) else {
            panic!("not the first step");
        };
        assert_eq!(*method, Method::Paste);
        assert!(field.is_empty(), "nothing reached the paste field");
        assert!(app.connect.as_ref().unwrap().typing());
        app.on_key_connect(KeyCode::Tab.into());
        assert!(matches!(
            step(&app),
            Step::Start {
                method: Method::Browser,
                ..
            }
        ));
    }

    #[test]
    fn enter_on_the_login_waits_and_esc_cancels_it() {
        let mut app = test_app();
        let mut flow = Connect::start(Mode::Serve);
        let store = cctop_core::cloudflare::login::fake::server(
            cctop_core::cloudflare::login::fake::Store::Working,
        );
        flow.new_login = Box::new(move || Login::fake(&store));
        app.open_connect_at(flow);
        app.on_key_connect(KeyCode::Enter.into());
        let flow = app.connect.as_ref().unwrap();
        let Step::LoggingIn { url, opened } = &flow.step else {
            panic!("not waiting on the login");
        };
        assert!(url.contains("/argotunnel?aud=&callback="), "{url}");
        assert!(!opened);
        assert!(flow.working.is_some());
        assert!(flow.qr, "with no browser here, the code is up");
        let cancel = flow.cancel.clone();
        app.on_key_connect(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
        assert!(!app.connect.as_ref().unwrap().qr);
        app.on_key_connect(KeyCode::Esc.into());
        assert!(app.connect.is_none());
        assert!(cancel.load(Ordering::Relaxed), "the wait was told to stop");
    }

    fn login_zone(name: &str) -> Zone {
        Zone {
            name: name.to_string(),
            ..zone("x")
        }
    }

    #[test]
    fn a_login_goes_on_to_the_name_or_asks_for_the_whole_address() {
        let auth = Auth::Login("made-up-login-token".into());
        let mut app = app();
        app.connect_answer(Answer::LoggedIn(Ok((
            auth.clone(),
            login_zone("example.test"),
        ))));
        assert_eq!(
            app.connect
                .as_ref()
                .unwrap()
                .working
                .as_ref()
                .map(|w| w.what),
            Some("Looking for a free name…")
        );

        let mut app = self::app();
        app.connect_answer(Answer::LoggedIn(Ok((auth.clone(), login_zone("")))));
        let Step::Hostname {
            auth: kept, field, ..
        } = step(&app)
        else {
            panic!("not the hostname");
        };
        assert_eq!(*kept, auth);
        assert!(field.is_empty());
        // A bare label cannot say which domain: refused before any call.
        app.paste_connect("cctop");
        app.on_key_connect(KeyCode::Enter.into());
        let flow = app.connect.as_ref().unwrap();
        assert!(flow.working.is_none());
        assert!(
            flow.problem
                .as_deref()
                .is_some_and(|p| p.contains("whole address"))
        );

        let mut app = self::app();
        app.connect_answer(Answer::LoggedIn(Err(cloudflare::Error::Login(
            "No login arrived within ten minutes; start again.".into(),
        ))));
        assert!(
            matches!(step(&app), Step::Failed { message, .. } if message.contains("ten minutes"))
        );
    }

    /// The Access step over an account cctop set up, whose edits go to the
    /// fake Cloudflare API rather than `config.toml`. Every token is made up.
    fn access_app() -> (App, cctop_core::cloudflare::fake::Seen) {
        use cctop_core::cloudflare::{Api, fake};
        let (base, seen) = fake::api(fake::accepting(None));
        let account = Account {
            token: "eyJhIjoi-made-up".into(),
            hostname: Some("cctop.example.test".into()),
            share_hostname: Some("cctop-share.example.test".into()),
            account_id: Some("acct1".into()),
            zone_id: Some("zone1".into()),
            tunnel_id: Some(fake::TUNNEL_ID.into()),
            api_token: Some("made-up-api-token".into()),
            ..Account::default()
        };
        let held = Arc::new(std::sync::Mutex::new(account.clone()));
        let mut flow = Connect::new(
            Step::Connected {
                account,
                confirm: false,
            },
            Mode::List,
        );
        flow.apply_access = Arc::new(move |change| {
            let mut account = held.lock().unwrap();
            let api = Api::fake(&base, "made-up-api-token");
            let applied =
                access_setup::apply_with(&api, &account, change).map_err(|e| e.to_string())?;
            *account = applied.account.clone();
            Ok(applied)
        });
        let mut app = test_app();
        app.open_connect_at(flow);
        (app, seen)
    }

    /// Wait for the call in flight, as the tick would.
    fn settle(app: &mut App) {
        cctop_core::test_wait::eventually_true("the Access edit", || {
            app.tick_connect();
            app.connect.as_ref().is_some_and(|c| c.working.is_none())
        });
    }

    fn settings(app: &App) -> Option<cctop_core::cloudflare::access::Settings> {
        match step(app) {
            Step::Access { account, .. } => account.access.as_deref().cloned(),
            _ => panic!("not the Access step"),
        }
    }

    fn type_in(app: &mut App, text: &str) {
        app.paste_connect(text);
        app.on_key_connect(KeyCode::Enter.into());
    }

    #[test]
    fn access_goes_on_invites_change_and_the_edge_follows() {
        let (mut app, seen) = access_app();
        app.on_key_connect(KeyCode::Char('a').into());
        assert_eq!(settings(&app), None);
        // A domain is no owner: refused under the field, before any call.
        app.on_key_connect(KeyCode::Char('o').into());
        assert!(app.connect.as_ref().unwrap().typing());
        type_in(&mut app, "@example.test");
        assert!(app.connect.as_ref().unwrap().problem.is_some());
        assert!(seen.lock().unwrap().is_empty());
        app.on_key_connect(KeyCode::Esc.into());
        app.on_key_connect(KeyCode::Char('o').into());
        type_in(&mut app, "Owner@Example.test");
        settle(&mut app);
        let on = settings(&app).expect("Access is on");
        assert_eq!(on.owner, "owner@example.test");
        assert_eq!(on.link_hostname.as_deref(), Some("cctop-link.example.test"));
        assert_eq!(
            app.connected
                .as_ref()
                .and_then(|c| c.access_owner.as_deref()),
            Some("owner@example.test")
        );

        app.on_key_connect(KeyCode::Char('i').into());
        type_in(&mut app, "guest@elsewhere.test");
        settle(&mut app);
        app.on_key_connect(KeyCode::Char('f').into());
        settle(&mut app);
        let invites = settings(&app).unwrap().invites;
        assert_eq!(invites.len(), 1);
        assert_eq!(invites[0].level, Level::Full);
        let policy = seen
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(m, p, _)| m == "PUT" && p.contains("/access/policies/"))
            .map(|(_, _, b)| b.clone())
            .expect("the policy was rewritten");
        assert!(policy.contains("guest@elsewhere.test"), "{policy}");

        // Token links off and on: the hostname goes and comes back.
        app.on_key_connect(KeyCode::Char('l').into());
        settle(&mut app);
        let off = settings(&app).unwrap();
        assert!(!off.public_links);
        assert_eq!(off.link_hostname, None);
        assert!(!app.connected.as_ref().unwrap().public_links);
        app.on_key_connect(KeyCode::Char('l').into());
        settle(&mut app);
        assert!(settings(&app).unwrap().public_links);

        app.on_key_connect(KeyCode::Char('x').into());
        settle(&mut app);
        assert!(settings(&app).unwrap().invites.is_empty());

        // Off asks first.
        app.on_key_connect(KeyCode::Char('o').into());
        assert!(settings(&app).is_some());
        app.on_key_connect(KeyCode::Char('y').into());
        settle(&mut app);
        assert_eq!(settings(&app), None);
        app.on_key_connect(KeyCode::Esc.into());
        assert!(matches!(step(&app), Step::Connected { .. }));
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
            matches!(step(&app), Step::Failed { message, .. } if message.contains("CCTOP_TUNNEL_TOKEN"))
        );
    }
}
