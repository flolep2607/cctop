//! The Cloudflare tunnel as rows of the settings page: `[tunnel]` read,
//! changed and set up in place, the way every other row is.
//!
//! It used to be one summary row whose Enter opened a popup, and the popup was
//! where everything happened. A row you have to open to change is a row the
//! page cannot show, so each fact is its own row now — the hostname, the zone,
//! the share hostname, Access and its owner, the token links, each invite —
//! with the same verbs as the rest of the page: Enter changes it, Backspace
//! takes it out, a field opens under the cursor for text.
//!
//! Everything it does is [`cctop_core::cloudflare`]'s and
//! [`cctop_core::tunnel`]'s, the same calls `cctop tunnel setup`, `remove`,
//! `access` and `invite` make; this only keeps them off the UI thread. A call
//! in flight is a spinner in its own row's value, and what it came to — a
//! refusal above all — is that row's description until the next change, so
//! nothing about it needs a box over the page.
//!
//! # Setting one up
//!
//! With no account, the section is the two ways in: `login`, which opens
//! Cloudflare's login (copying its address, which over ssh is the way to a
//! browser at all) and waits for it, and `token`, a masked field for an API
//! token or a tunnel token. Either way ends on a `zone` row and a `hostname`
//! field, pre-filled; Enter in it creates the tunnel.
//!
//! # Credentials on screen
//!
//! Never. A token field is drawn as dots from the first character, the tokens
//! are held only in [`Setup`] and the worker threads, and every note a row
//! shows is Cloudflare's error or one written here — neither of which quotes
//! the token.

use super::settings::Item;
use super::*;
use cctop_core::cloudflare::access::{Invite, Level};
use cctop_core::cloudflare::access_setup::{self, Applied, Change};
use cctop_core::cloudflare::login::Login;
use cctop_core::cloudflare::{self, Auth, Pasted, Renamed, Zone};
use cctop_core::tunnel::Account;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// How long a token can be: Cloudflare's are 40 characters and a tunnel
/// token a few hundred. Generous, so a paste is never cut short.
pub const TOKEN_MAX: usize = 4096;
/// A DNS name's limit.
const HOSTNAME_MAX: usize = 253;

/// One row of the `[tunnel]` section.
///
/// Which rows exist depends on the account: a connected one is its facts and
/// the way to remove it, Access's rows only while Access is on; no account is
/// the ways in, and a setup half done is its domain and its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Connected or not: read-only.
    Status,
    /// The dashboard's address, renamed in place.
    Hostname,
    /// The served link as a QR code, for a phone.
    Qr,
    /// The domain, fixed at setup.
    Zone,
    /// The default share hostname, renamed in place.
    Shares,
    /// Cloudflare Access on or off.
    Access,
    /// Who owns Access, while it is on.
    Owner,
    /// Whether token links work from outside, while Access is on.
    Links,
    /// One invite, by index into the account's list.
    Invite(usize),
    /// The field that adds an invite.
    AddInvite,
    /// Delete what setup made, or forget an account it did not make.
    Remove,
    /// Set up by logging in with the browser.
    Login,
    /// Set up with a pasted API token or tunnel token.
    Token,
    /// The domain a setup creates the tunnel on, cycled when there are more.
    SetupZone,
    /// The name a setup creates the tunnel at.
    SetupHostname,
}

impl Row {
    /// The name the row is drawn and filtered under.
    pub fn name(self) -> &'static str {
        match self {
            Row::Status => "status",
            Row::Hostname | Row::SetupHostname => "hostname",
            Row::Qr => "qr",
            Row::Zone | Row::SetupZone => "zone",
            Row::Shares => "share_hostname",
            Row::Access => "access",
            Row::Owner => "owner",
            Row::Links => "public_links",
            Row::Invite(_) | Row::AddInvite => "invite",
            Row::Remove => "remove",
            Row::Login => "login",
            Row::Token => "token",
        }
    }
}

/// What the page's open field is for, while it is on a tunnel row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typing {
    /// The dashboard's new address.
    Hostname,
    /// The default share hostname's new address.
    ShareHostname,
    /// The owner's email, which turns Access on or moves it.
    Owner,
    /// An email or `@domain` to invite, read-only.
    Invite,
    /// An API token to replace a browser login's, which Cloudflare never lets
    /// manage Access. Asked for before Access is turned on, not after
    /// Cloudflare refuses.
    ApiToken,
    /// The token a setup starts from: an API token or a tunnel token.
    SetupToken,
    /// The name a setup creates the tunnel at.
    SetupHostname,
    /// The hostname a pasted tunnel token serves, which may be left empty.
    TunnelHostname,
}

impl Typing {
    /// Whether the field holds a credential, and is drawn as dots.
    pub fn masked(self) -> bool {
        matches!(self, Typing::ApiToken | Typing::SetupToken)
    }

    /// How much it takes.
    pub fn max(self) -> usize {
        match self.masked() {
            true => TOKEN_MAX,
            false => HOSTNAME_MAX,
        }
    }
}

/// A setup under way: the login being waited for, or the domain and name it
/// will create.
pub enum Setup {
    LoggingIn {
        url: String,
    },
    Picked {
        auth: Auth,
        zones: Vec<Zone>,
        at: usize,
        /// The name offered, or the one last typed.
        hostname: String,
    },
    /// A tunnel token was pasted, and its hostname is being typed.
    TunnelToken {
        token: String,
    },
}

/// Which link a QR code drawn over the page encodes. Named rather than held,
/// so the link is read afresh each frame — a serve stopped under it takes the
/// code with it — and no second copy of a credential is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrOf {
    /// The served link, token and all.
    Dashboard,
    /// The login being waited for.
    Login,
}

/// What the last change on a row came to, said where it was made.
pub struct Note {
    pub row: Row,
    pub text: String,
    pub error: bool,
}

/// A call to Cloudflare in flight, and the row that asked for it.
pub struct Working {
    rx: Receiver<Answer>,
    pub row: Row,
    pub what: &'static str,
}

/// What a worker thread comes back with.
enum Answer {
    /// The login's token, and its domain: named, or with no name when it
    /// could not be read.
    LoggedIn(Result<(Auth, Zone), cloudflare::Error>),
    Zones(Auth, Result<Vec<Zone>, cloudflare::Error>),
    Suggested(Auth, Vec<Zone>, usize, Result<String, cloudflare::Error>),
    Created(Result<Account, String>),
    /// The account with a pasted token in place of its login's.
    Adopted(Result<Account, String>),
    Removed(Result<(Vec<String>, bool), String>),
    Access(Result<Applied, String>),
    Renamed(Result<Renamed, String>),
}

/// How an Access edit is made: [`access_setup::apply`], except in a test,
/// which applies it to an account of its own against the fake API.
pub type ApplyAccess = Arc<dyn Fn(Change) -> Result<Applied, String> + Send + Sync>;

/// The tunnel rows' state beyond what `config.toml` holds.
pub struct Rows {
    pub working: Option<Working>,
    pub note: Option<Note>,
    /// A row waiting for `y`: Access off, or remove.
    pub confirm: Option<Row>,
    /// What the page's open field is for, when it is on one of these rows.
    pub typing: Option<Typing>,
    pub setup: Option<Setup>,
    /// A QR code over the page, until any key.
    pub qr: Option<QrOf>,
    /// Set when a login is given up on, so its thread stops polling rather
    /// than waiting out its ten minutes.
    cancel: Arc<AtomicBool>,
    /// Where a login starts: Cloudflare, and in a test the fake, so that no
    /// test polls the real store.
    new_login: Box<dyn Fn() -> Login>,
    apply_access: ApplyAccess,
}

impl Default for Rows {
    fn default() -> Rows {
        Rows {
            working: None,
            note: None,
            confirm: None,
            typing: None,
            setup: None,
            qr: None,
            cancel: Arc::default(),
            new_login: Box::new(Login::new),
            apply_access: Arc::new(access_setup::apply),
        }
    }
}

impl Drop for Rows {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Rows {
    /// Whether the open field holds a credential.
    pub fn masked(&self) -> bool {
        self.typing.is_some_and(Typing::masked)
    }

    /// How much the open field takes, when it is one of these.
    pub fn max(&self) -> Option<usize> {
        self.typing.map(Typing::max)
    }
}

/// How a row's value is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Text,
    Bool(bool),
    /// Enter turns it to the next one.
    Cycle,
}

/// How a row's description is inked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Dim,
    /// A question waiting for `y`.
    Ask,
    Error,
}

/// One row as the page draws it.
pub struct Shown {
    pub value: String,
    pub shape: Shape,
    /// Whether `config.toml` holds it, which is the page's `*`.
    pub set: bool,
    pub what: String,
    pub tone: Tone,
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
    /// Who else may log in, while Access is on.
    pub invites: Vec<Invite>,
    /// Connected by browser login, whose token cannot manage Access.
    pub login: bool,
    /// Made by cctop's setup, which holds an API token: removing it deletes
    /// the tunnel. Otherwise removing only forgets it.
    pub made_here: bool,
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
            invites: account
                .access
                .as_ref()
                .map(|a| a.invites.clone())
                .unwrap_or_default(),
            login: account.login,
            made_here: account.api_token.is_some(),
        }
    }
}

/// A refusal, short. The two whose way out is a new token say so rather than
/// spelling out the link, which `token`'s `Ctrl+O` copies.
fn short(error: &cloudflare::Error) -> String {
    match error {
        cloudflare::Error::MissingPermission(permission) => {
            format!("token lacks \"{permission}\" · Ctrl+O on token: link")
        }
        cloudflare::Error::LoginRefused(permission) => {
            format!("login can't \"{permission}\" · use token")
        }
        other => other.to_string(),
    }
}

/// `host` without its zone, which is what a field for a name under it holds.
fn label_of(host: &str, zone: Option<&str>) -> String {
    match zone {
        Some(zone) => host
            .strip_suffix(&format!(".{zone}"))
            .unwrap_or(host)
            .to_string(),
        None => host.to_string(),
    }
}

impl App {
    /// Read the connected account again: after a change here, and whenever
    /// `config.toml` changed, since `cctop tunnel setup` in another terminal
    /// writes the same table.
    pub(super) fn refresh_connected(&mut self) {
        self.connected = cctop_core::tunnel::account().map(|a| Connected::from(&a));
    }

    /// The section's rows, for the account as it is now.
    pub(super) fn tunnel_rows(&self) -> Vec<Row> {
        let mut rows = vec![Row::Status];
        match &self.connected {
            Some(c) => {
                rows.extend([Row::Hostname, Row::Qr, Row::Zone, Row::Shares, Row::Access]);
                if c.access_owner.is_some() {
                    rows.extend([Row::Owner, Row::Links]);
                    rows.extend((0..c.invites.len()).map(Row::Invite));
                    rows.push(Row::AddInvite);
                }
                rows.push(Row::Remove);
            }
            None => match &self.cf.setup {
                Some(Setup::Picked { .. }) => rows.extend([Row::SetupZone, Row::SetupHostname]),
                _ => rows.extend([Row::Login, Row::Token]),
            },
        }
        rows
    }

    /// `a` in the serve panel: the page, on the first tunnel row.
    pub(super) fn open_tunnel_settings(&mut self) {
        self.mode = Mode::List;
        if !self.on_settings() {
            self.goto_settings();
        }
        self.settings_filter.clear();
        self.settings_typing = false;
        self.tunnel_focus(Row::Status);
    }

    /// Put the cursor on `row`, clearing a filter that hides it.
    fn tunnel_focus(&mut self, row: Row) {
        let find = |app: &App| {
            app.settings_shown()
                .iter()
                .position(|i| *i == Item::Tunnel(row))
        };
        let at = match find(self) {
            Some(at) => Some(at),
            None => {
                self.settings_filter.clear();
                self.settings_typing = false;
                find(self)
            }
        };
        if let Some(at) = at {
            self.settings_cursor = at;
        }
        self.needs_redraw = true;
    }

    /// Keep the cursor on the page after the rows changed under it.
    fn tunnel_clamp(&mut self) {
        let shown = self.settings_shown().len();
        self.settings_cursor = self.settings_cursor.min(shown.saturating_sub(1));
    }

    fn tunnel_note(&mut self, row: Row, text: impl Into<String>, error: bool) {
        self.cf.note = Some(Note {
            row,
            text: text.into(),
            error,
        });
        self.needs_redraw = true;
    }

    /// Open the page's field on the cursor's row, for `typing`.
    fn tunnel_type(&mut self, typing: Typing, text: &str) {
        self.cf.typing = Some(typing);
        self.settings_input = Some(text.to_string().into());
        self.needs_redraw = true;
    }

    /// Close the page's field without committing it, and whatever it was the
    /// last step of.
    pub(super) fn settings_drop_input(&mut self) {
        self.settings_input = None;
        self.cf.confirm = None;
        if self.cf.typing.take() == Some(Typing::TunnelHostname) {
            self.cf.setup = None;
        }
    }

    /// One row as the page draws it.
    pub(super) fn tunnel_view(&self, row: Row) -> Shown {
        let c = self.connected.as_ref();
        let text = |value: &str, what: &str| (value.to_string(), Shape::Text, what.to_string());
        let or_dash = |v: Option<&String>| v.cloned().unwrap_or_else(|| "—".to_string());
        let (mut value, mut shape, mut what) = match row {
            Row::Status => match c {
                Some(c) => {
                    let value = match c.from_env {
                        true => "connected · env",
                        false => "connected",
                    };
                    let what = match c.rename {
                        Err(why) => why,
                        Ok(()) => "Cloudflare tunnel",
                    };
                    text(value, what)
                }
                None => text("not connected", "links that stay"),
            },
            Row::Hostname => (
                or_dash(c.and_then(|c| c.hostname.as_ref())),
                Shape::Text,
                "dashboard address".to_string(),
            ),
            Row::Qr => match self.qr_link(QrOf::Dashboard) {
                Some(_) => text("↵ show", "dashboard link · has token"),
                None => text("—", "nothing served · t"),
            },
            Row::Zone => (
                or_dash(c.and_then(|c| c.zone.as_ref())),
                Shape::Text,
                "domain · fixed".to_string(),
            ),
            Row::Shares => (
                or_dash(c.and_then(|c| c.share_hostname.as_ref())),
                Shape::Text,
                "W shares · rename breaks links".to_string(),
            ),
            Row::Access => (
                String::new(),
                Shape::Bool(c.is_some_and(|c| c.access_owner.is_some())),
                "Cloudflare Access login".to_string(),
            ),
            Row::Owner => (
                or_dash(c.and_then(|c| c.access_owner.as_ref())),
                Shape::Text,
                "Access owner, full".to_string(),
            ),
            Row::Links => {
                let on = c.is_none_or(|c| c.public_links);
                let what = match c.and_then(|c| c.link_hostname.as_ref()) {
                    Some(host) if on => format!("token links · {host}"),
                    _ => "token links from outside".to_string(),
                };
                (String::new(), Shape::Bool(on), what)
            }
            Row::Invite(i) => match c.and_then(|c| c.invites.get(i)) {
                Some(invite) => (
                    format!("{} · {}", invite.who, invite.level.word()),
                    Shape::Cycle,
                    "↵ read/full · ⌫ remove".to_string(),
                ),
                None => text("—", ""),
            },
            Row::AddInvite => text("+ add", "email or @domain"),
            Row::Remove => match c.is_some_and(|c| c.made_here) {
                true => text("↵ delete", "tunnel + DNS records"),
                false => text("↵ forget", "stays on Cloudflare"),
            },
            Row::Login => text("↵ browser", "log in, pick domain"),
            Row::Token => text("↵ paste", "API or tunnel token · Ctrl+O link"),
            Row::SetupZone => match &self.cf.setup {
                Some(Setup::Picked { zones, at, .. }) => {
                    let name = zones.get(*at).map_or("", |z| z.name.as_str());
                    let value = match name.is_empty() {
                        true => "from login",
                        false => name,
                    };
                    let shape = match zones.len() > 1 {
                        true => Shape::Cycle,
                        false => Shape::Text,
                    };
                    (value.to_string(), shape, "domain".to_string())
                }
                _ => text("—", "domain"),
            },
            Row::SetupHostname => match &self.cf.setup {
                Some(Setup::Picked { hostname, .. }) => text(hostname, "↵ create · ⌫ cancel"),
                _ => text("—", ""),
            },
        };
        if row == Row::Token && self.cf.typing == Some(Typing::TunnelHostname) {
            what = "hostname it serves · empty: learned".to_string();
        }
        let mut tone = Tone::Dim;
        if self.cf.confirm == Some(row) {
            value = "y / n".to_string();
            shape = Shape::Text;
            tone = Tone::Ask;
            what = match row {
                Row::Access => "Access off? invites forgotten".to_string(),
                _ => {
                    let mut ask = match c.is_some_and(|c| c.made_here) {
                        true => "delete tunnel + DNS?".to_string(),
                        false => "forget it here?".to_string(),
                    };
                    if self.serving_on_account() {
                        ask.push_str(" · stops serve");
                    }
                    ask
                }
            };
        } else if let Some(working) = self.cf.working.as_ref().filter(|w| w.row == row) {
            value = format!("{} {}", share::spinner_frame(), working.what);
            shape = Shape::Text;
        }
        if let Some(note) = self.cf.note.as_ref().filter(|n| n.row == row)
            && self.cf.confirm != Some(row)
        {
            what = note.text.clone();
            if note.error {
                tone = Tone::Error;
            }
        }
        let set = match row {
            Row::Hostname => c.is_some_and(|c| c.hostname.is_some()),
            Row::Zone => c.is_some_and(|c| c.zone.is_some()),
            Row::Shares => c.is_some_and(|c| c.share_hostname.is_some()),
            Row::Access | Row::Owner | Row::Invite(_) => {
                c.is_some_and(|c| c.access_owner.is_some())
            }
            Row::Links => c.is_some_and(|c| !c.public_links),
            _ => false,
        };
        Shown {
            value,
            shape,
            set,
            what,
            tone,
        }
    }

    /// Run `call` off the UI thread, with `what` spinning in `row`'s value.
    fn tunnel_work(
        &mut self,
        row: Row,
        what: &'static str,
        call: impl FnOnce() -> Answer + Send + 'static,
    ) {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(call());
        });
        self.cf.note = None;
        self.cf.working = Some(Working { rx, row, what });
        self.needs_redraw = true;
    }

    /// Make an Access `change` off the UI thread.
    fn access_work(&mut self, row: Row, what: &'static str, change: Change) {
        let apply = self.cf.apply_access.clone();
        self.tunnel_work(row, what, move || Answer::Access(apply(change)));
    }

    /// Enter on a tunnel row.
    pub(super) fn tunnel_activate(&mut self, row: Row) {
        let logging_in = matches!(self.cf.setup, Some(Setup::LoggingIn { .. }));
        if self.cf.working.is_some() && !(row == Row::Login && logging_in) {
            return self.set_status("Cloudflare is still answering");
        }
        self.cf.note = None;
        let Some(c) = self.connected.clone() else {
            return self.setup_activate(row);
        };
        // Every Access row writes Cloudflare through the account's API token,
        // so one that cannot is refused here and not after typing.
        let writes = matches!(
            row,
            Row::Hostname
                | Row::Shares
                | Row::Access
                | Row::Owner
                | Row::Links
                | Row::Invite(_)
                | Row::AddInvite
        );
        if writes && let Err(why) = c.rename {
            return self.tunnel_note(row, why, true);
        }
        match row {
            Row::Hostname => {
                let label = label_of(c.hostname.as_deref().unwrap_or_default(), c.zone.as_deref());
                self.tunnel_type(Typing::Hostname, &label);
            }
            Row::Qr => match self.qr_link(QrOf::Dashboard) {
                Some(_) => self.cf.qr = Some(QrOf::Dashboard),
                None => self.tunnel_note(row, "nothing served · t first", true),
            },
            Row::Zone => self.tunnel_note(row, "fixed · remove, set up again", false),
            Row::Shares => {
                let label = label_of(
                    c.share_hostname.as_deref().unwrap_or_default(),
                    c.zone.as_deref(),
                );
                self.tunnel_type(Typing::ShareHostname, &label);
                // Said before the Enter that does it, not after.
                self.tunnel_note(row, "old W links break · per agent: e in W", false);
            }
            Row::Access => match (&c.access_owner, c.login) {
                (Some(_), _) => self.cf.confirm = Some(Row::Access),
                // A browser login's token cannot manage Access; an API token
                // can, and keeps the tunnel as it is.
                (None, true) => {
                    self.tunnel_type(Typing::ApiToken, "");
                    self.tunnel_note(row, "login can't: paste API token · Ctrl+O link", false);
                }
                (None, false) => {
                    self.tunnel_type(Typing::Owner, "");
                    self.tunnel_note(row, "owner email", false);
                }
            },
            Row::Owner => {
                let owner = c.access_owner.clone().unwrap_or_default();
                self.tunnel_type(Typing::Owner, &owner);
            }
            Row::Links => {
                self.access_work(row, "saving", Change::PublicLinks(!c.public_links));
            }
            Row::Invite(i) => {
                if let Some(invite) = c.invites.get(i) {
                    let level = match invite.level {
                        Level::Read => Level::Full,
                        Level::Full => Level::Read,
                    };
                    let who = invite.who.clone();
                    self.access_work(row, "saving", Change::Invite { who, level });
                }
            }
            Row::AddInvite => self.tunnel_type(Typing::Invite, ""),
            Row::Remove => self.cf.confirm = Some(Row::Remove),
            _ => {}
        }
    }

    /// Enter on a row of a setup.
    fn setup_activate(&mut self, row: Row) {
        match row {
            Row::Login => match &self.cf.setup {
                Some(Setup::LoggingIn { url }) => {
                    render::copy_to_clipboard(url);
                    self.tunnel_note(row, "link copied · q QR · ⌫ cancel", false);
                }
                _ => self.log_in(),
            },
            Row::Token => {
                self.cf.setup = None;
                self.tunnel_type(Typing::SetupToken, "");
            }
            Row::SetupZone => {
                let Some(Setup::Picked {
                    auth, zones, at, ..
                }) = &self.cf.setup
                else {
                    return;
                };
                if zones.len() > 1 {
                    let next = (*at + 1) % zones.len();
                    let (auth, zones) = (auth.clone(), zones.clone());
                    self.suggest_for(auth, zones, next);
                }
            }
            Row::SetupHostname => {
                if let Some(Setup::Picked { hostname, .. }) = &self.cf.setup {
                    let hostname = hostname.clone();
                    self.tunnel_type(Typing::SetupHostname, &hostname);
                }
            }
            _ => {}
        }
    }

    /// Backspace on a tunnel row: an invite goes, and a setup is given up.
    pub(super) fn tunnel_reset(&mut self, row: Row) {
        match row {
            Row::Invite(i) => {
                if self.cf.working.is_some() {
                    return self.set_status("Cloudflare is still answering");
                }
                let who = self
                    .connected
                    .as_ref()
                    .and_then(|c| c.invites.get(i))
                    .map(|i| i.who.clone());
                if let Some(who) = who {
                    self.access_work(row, "removing", Change::Uninvite { who });
                }
            }
            Row::Login | Row::Token | Row::SetupZone | Row::SetupHostname
                if self.cf.setup.is_some() =>
            {
                self.cancel_setup();
            }
            _ => {}
        }
    }

    /// Give up on a setup, telling a login still waiting to stop.
    fn cancel_setup(&mut self) {
        self.cf.cancel.store(true, Ordering::Relaxed);
        self.cf.cancel = Arc::default();
        self.cf.working = None;
        self.cf.setup = None;
        self.cf.note = None;
        self.tunnel_focus(Row::Login);
        self.set_status("Tunnel setup cancelled");
    }

    /// `y` on a row that asked.
    fn tunnel_confirmed(&mut self, row: Row) {
        match row {
            Row::Access => self.access_work(row, "turning off", Change::Off),
            Row::Remove => self.tunnel_remove(),
            _ => {}
        }
    }

    /// The key after a row asked: `y` does it, anything else keeps it.
    pub(super) fn tunnel_answer_key(&mut self, key: KeyEvent) {
        let Some(row) = self.cf.confirm.take() else {
            return;
        };
        if key.code == KeyCode::Char('y') {
            self.tunnel_confirmed(row);
        }
        self.needs_redraw = true;
    }

    /// Enter in the field on a tunnel row.
    pub(super) fn tunnel_commit(&mut self, row: Row, text: &str) {
        let Some(typing) = self.cf.typing.take() else {
            return;
        };
        // A refusal keeps the field open on what was typed, with the reason
        // where the description was.
        let refuse = |app: &mut App, why: &str| {
            app.tunnel_type(typing, text);
            app.tunnel_note(row, why, true);
        };
        let who = cctop_core::cloudflare::access::parse_who(text);
        match typing {
            Typing::Hostname | Typing::ShareHostname if text.is_empty() => {
                refuse(self, "type a name")
            }
            Typing::Hostname => {
                let typed = text.to_string();
                self.tunnel_work(row, "moving", move || {
                    Answer::Renamed(cloudflare::name_dashboard(&typed))
                });
            }
            Typing::ShareHostname => {
                let typed = text.to_string();
                self.tunnel_work(row, "moving", move || {
                    Answer::Renamed(cloudflare::name_default_share(&typed))
                });
            }
            Typing::Owner => match who {
                Some(owner) if !owner.starts_with('@') => {
                    self.access_work(row, "saving", Change::On { owner })
                }
                _ => refuse(self, "one email address"),
            },
            Typing::Invite => match who {
                Some(who) => self.access_work(
                    row,
                    "inviting",
                    Change::Invite {
                        who,
                        level: Level::Read,
                    },
                ),
                None => refuse(self, "email or @domain"),
            },
            Typing::ApiToken | Typing::SetupToken if text.is_empty() => {
                refuse(self, "paste the token")
            }
            Typing::ApiToken => {
                let typed = text.to_string();
                self.tunnel_work(row, "checking", move || {
                    Answer::Adopted(cloudflare::adopt_token(&typed))
                });
            }
            Typing::SetupToken => self.submit_token(row, text),
            Typing::SetupHostname => self.create_tunnel(row, text),
            Typing::TunnelHostname => self.save_tunnel_token(row, text),
        }
    }

    /// A pasted token: a tunnel token goes on to its hostname, an API token
    /// to Cloudflare to find its domains.
    fn submit_token(&mut self, row: Row, pasted: &str) {
        match cloudflare::classify(pasted) {
            Pasted::Tunnel(token) => match cloudflare::check_tunnel_token(&token) {
                Ok(()) => {
                    self.cf.setup = Some(Setup::TunnelToken { token });
                    self.tunnel_type(Typing::TunnelHostname, "");
                }
                Err(why) => self.tunnel_note(row, why, true),
            },
            Pasted::Api(token) => self.tunnel_work(row, "checking", move || {
                let api = cloudflare::Api::new(&token);
                let zones = api
                    .verify()
                    .and_then(|()| api.zones())
                    .and_then(cloudflare::usable);
                Answer::Zones(Auth::Pasted(token), zones)
            }),
        }
    }

    /// Start a browser login: open the page where a browser here would be
    /// the user's, copy its address for one elsewhere, and wait off the UI
    /// thread.
    fn log_in(&mut self) {
        let login = (self.cf.new_login)();
        let url = login.url().to_string();
        // A test drives this with no browser to bother.
        let opened = !cfg!(test) && !render::over_ssh() && share::open_in_browser(&url);
        render::copy_to_clipboard(&url);
        self.cf.cancel = Arc::default();
        let cancel = self.cf.cancel.clone();
        self.cf.setup = Some(Setup::LoggingIn { url });
        self.tunnel_work(Row::Login, "waiting", move || {
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
        self.tunnel_note(
            Row::Login,
            match opened {
                true => "browser opened · link copied · q QR · ⌫ cancel",
                false => "link copied · ↵ copy · q QR · ⌫ cancel",
            },
            false,
        );
    }

    fn suggest_for(&mut self, auth: Auth, zones: Vec<Zone>, at: usize) {
        let row = match self.cf.setup {
            Some(Setup::Picked { .. }) => Row::SetupZone,
            _ => self.cf.working.as_ref().map_or(Row::Login, |w| w.row),
        };
        let zone = zones[at].clone();
        self.tunnel_work(row, "naming", move || {
            let api = cloudflare::Api::with(&auth);
            let suggested = cloudflare::suggest_hostname(&api, &zone, &cloudflare::machine_label());
            Answer::Suggested(auth, zones, at, suggested)
        });
    }

    /// Enter in the setup's hostname: create everything, and store it.
    fn create_tunnel(&mut self, row: Row, typed: &str) {
        let Some(Setup::Picked {
            auth,
            zones,
            at,
            hostname,
        }) = &mut self.cf.setup
        else {
            return;
        };
        *hostname = typed.to_string();
        let zone = zones[*at].clone();
        let auth = auth.clone();
        // A login's domain with no name takes it from what was typed.
        let zone = match zone.name.is_empty() {
            true => match cloudflare::login_zone_from(&zone.id, &zone.account_id, typed) {
                Ok(zone) => zone,
                Err(e) => {
                    self.tunnel_type(Typing::SetupHostname, typed);
                    return self.tunnel_note(row, e.to_string(), true);
                }
            },
            false => zone,
        };
        // Checked here first, so a typo is a note and not a spinner followed
        // by a dead end.
        if let Err(e) = cloudflare::check_hostname(typed, &zone) {
            self.tunnel_type(Typing::SetupHostname, typed);
            return self.tunnel_note(row, e.to_string(), true);
        }
        let hostname = typed.to_string();
        self.tunnel_work(row, "creating", move || {
            let api = cloudflare::Api::with(&auth);
            let made = cloudflare::create(&api, &zone, &hostname, &cloudflare::machine_label())
                .map_err(|e| e.to_string())
                .and_then(|account| {
                    // Saved here and not by whoever receives this: the page
                    // may have moved on, and a tunnel made on Cloudflare that
                    // nothing remembers is one nothing can remove.
                    cctop_core::tunnel::save_account(&account)
                        .map(|()| account)
                        .map_err(|e| format!("could not write config.toml: {e}"))
                });
            Answer::Created(made)
        });
    }

    /// Enter on a tunnel token's hostname: store it. Nothing on Cloudflare is
    /// created for one, so there is nothing to wait for.
    fn save_tunnel_token(&mut self, row: Row, typed: &str) {
        let Some(Setup::TunnelToken { token }) = self.cf.setup.take() else {
            return;
        };
        let hostname =
            Some(typed.trim_end_matches('.').to_ascii_lowercase()).filter(|h| !h.is_empty());
        let account = Account {
            token,
            hostname,
            ..Account::default()
        };
        match cctop_core::tunnel::save_account(&account) {
            Ok(()) => {
                self.connected = Some(Connected::from(&account));
                self.tunnel_focus(Row::Status);
                self.tunnel_note(Row::Status, "saved · t serves on it", false);
            }
            Err(e) => self.tunnel_note(row, format!("could not write config.toml: {e}"), true),
        }
    }

    /// `y` on remove: delete what setup made, and forget it.
    fn tunnel_remove(&mut self) {
        let row = Row::Remove;
        if self.connected.as_ref().is_some_and(|c| c.from_env) {
            return self.tunnel_note(row, "from CCTOP_TUNNEL_TOKEN: unset it", true);
        }
        let Some(account) = cctop_core::tunnel::account() else {
            self.refresh_connected();
            return self.tunnel_clamp();
        };
        // This dashboard's own serve would be pulled out from under itself;
        // it is stopped first, which the question said.
        if self.serving_on_account() {
            self.stop_serving();
        }
        if cctop_core::tunnel::in_use(&account) {
            return self.tunnel_note(row, "another cctop serves on it: stop it first", true);
        }
        self.tunnel_work(row, "deleting", move || {
            let left = cloudflare::remove(&account);
            Answer::Removed(
                cctop_core::tunnel::clear_account()
                    .map(|()| (left.0, account.api_token.is_some()))
                    .map_err(|e| format!("could not update config.toml: {e}")),
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
    pub(super) fn tick_tunnel(&mut self) -> bool {
        let Some(working) = &self.cf.working else {
            return false;
        };
        let row = working.row;
        let answer = match working.rx.try_recv() {
            Err(TryRecvError::Empty) => return true,
            Ok(answer) => answer,
            Err(TryRecvError::Disconnected) => {
                self.cf.working = None;
                self.cf.setup = None;
                self.tunnel_note(row, "no answer from Cloudflare", true);
                return true;
            }
        };
        self.cf.working = None;
        self.tunnel_answer(row, answer);
        true
    }

    fn tunnel_answer(&mut self, row: Row, answer: Answer) {
        match answer {
            // The domain was picked in the browser: on to its name, or —
            // when its name could not be read — to typing the whole address.
            Answer::LoggedIn(Ok((auth, zone))) => match zone.name.is_empty() {
                true => {
                    self.cf.setup = Some(Setup::Picked {
                        auth,
                        zones: vec![zone],
                        at: 0,
                        hostname: String::new(),
                    });
                    self.tunnel_focus(Row::SetupHostname);
                    self.tunnel_type(Typing::SetupHostname, "");
                    self.tunnel_note(
                        Row::SetupHostname,
                        "whole address: cctop.example.com",
                        false,
                    );
                }
                false => self.suggest_for(auth, vec![zone], 0),
            },
            Answer::Zones(auth, Ok(zones)) => self.suggest_for(auth, zones, 0),
            Answer::Suggested(auth, zones, at, suggested) => {
                let (hostname, problem) = match suggested {
                    Ok(name) => (name, None),
                    // Every name it would offer is taken: an empty field.
                    Err(cloudflare::Error::Hostname(why)) => (String::new(), Some(why)),
                    Err(e) => {
                        self.cf.setup = None;
                        self.tunnel_clamp();
                        return self.tunnel_note(row, short(&e), true);
                    }
                };
                self.cf.setup = Some(Setup::Picked {
                    auth,
                    zones,
                    at,
                    hostname: hostname.clone(),
                });
                self.tunnel_focus(Row::SetupHostname);
                self.tunnel_type(Typing::SetupHostname, &hostname);
                if let Some(why) = problem {
                    self.tunnel_note(Row::SetupHostname, why, true);
                }
            }
            Answer::LoggedIn(Err(e)) | Answer::Zones(_, Err(e)) => {
                self.cf.setup = None;
                self.tunnel_clamp();
                self.tunnel_note(row, short(&e), true);
            }
            Answer::Created(Ok(account)) => {
                self.cf.setup = None;
                self.connected = Some(Connected::from(&account));
                self.tunnel_focus(Row::Status);
                self.tunnel_note(Row::Status, "set up · t serves on it", false);
            }
            Answer::Created(Err(message)) => {
                if let Some(Setup::Picked { hostname, .. }) = &self.cf.setup {
                    let hostname = hostname.clone();
                    self.tunnel_focus(Row::SetupHostname);
                    self.tunnel_type(Typing::SetupHostname, &hostname);
                }
                self.tunnel_note(Row::SetupHostname, message, true);
            }
            Answer::Removed(Ok((left, made_here))) => {
                self.connected = None;
                self.tunnel_focus(Row::Status);
                match left.is_empty() {
                    true => self.tunnel_note(
                        Row::Status,
                        match made_here {
                            true => "removed",
                            false => "forgotten · still on Cloudflare",
                        },
                        false,
                    ),
                    false => self.tunnel_note(
                        Row::Status,
                        format!("left on Cloudflare: {}", left.join(", ")),
                        true,
                    ),
                }
            }
            Answer::Removed(Err(message)) => self.tunnel_note(row, message, true),
            Answer::Access(Ok(applied)) => {
                self.connected = Some(Connected::from(&applied.account));
                self.retarget_links(&applied.account);
                self.tunnel_clamp();
                if !applied.left.is_empty() {
                    let row = match self.tunnel_rows().contains(&row) {
                        true => row,
                        false => Row::Access,
                    };
                    self.tunnel_note(
                        row,
                        format!("left on Cloudflare: {}", applied.left.join(", ")),
                        true,
                    );
                }
            }
            Answer::Access(Err(message)) => self.tunnel_note(row, message, true),
            Answer::Adopted(Ok(adopted)) => {
                self.connected = Some(Connected::from(&adopted));
                // Straight on to what the token was for.
                self.tunnel_focus(Row::Access);
                self.tunnel_type(Typing::Owner, "");
                self.tunnel_note(Row::Access, "token saved · owner email", false);
            }
            Answer::Adopted(Err(message)) => self.tunnel_note(row, message, true),
            Answer::Renamed(Ok(renamed)) => {
                self.refresh_connected();
                self.follow_page_host();
                match (&renamed.note, &renamed.old, renamed.changed) {
                    (Some(note), ..) => self.tunnel_note(row, note.clone(), true),
                    (None, Some(_), true) if row == Row::Shares => {
                        self.tunnel_note(row, "moved · old W links dead", false)
                    }
                    (None, Some(old), true) => {
                        self.tunnel_note(row, format!("moved · {old} stops"), false)
                    }
                    _ => self.tunnel_note(row, "unchanged", false),
                }
            }
            Answer::Renamed(Err(message)) => self.tunnel_note(row, message, true),
        }
    }

    /// The link `of` encodes, while there is one: the served link through
    /// the tunnel — `y` in the serve panel copies the same — and the login's
    /// while it is waited for. The loopback link is never one: a phone cannot
    /// reach it.
    pub(super) fn qr_link(&self, of: QrOf) -> Option<&str> {
        match of {
            QrOf::Dashboard => self.serving.as_ref()?.public.as_deref(),
            QrOf::Login => match &self.cf.setup {
                Some(Setup::LoggingIn { url }) => Some(url),
                _ => None,
            },
        }
    }

    /// A key on the settings page that belongs to a QR code: any key puts
    /// one away, and `q` on a login being waited for draws its link. Returns
    /// whether the key was taken.
    ///
    /// `q` and not the page's quit only there, where the login is the one
    /// thing on screen to scan and a quit would give it up besides.
    ///
    /// A code whose link has gone is dropped and the key goes on to the page:
    /// nothing was on screen for it to close. That is the login's own happy
    /// path — scanned, finished on the phone, and the next key is the one
    /// that accepts the suggested hostname.
    pub(super) fn tunnel_qr_key(&mut self, key: KeyEvent) -> bool {
        if let Some(of) = self.cf.qr.take()
            && self.qr_link(of).is_some()
        {
            self.needs_redraw = true;
            return true;
        }
        let on_login = self.settings_item() == Some(Item::Tunnel(Row::Login));
        if key.code == KeyCode::Char('q') && on_login && self.qr_link(QrOf::Login).is_some() {
            self.cf.qr = Some(QrOf::Login);
            self.needs_redraw = true;
            return true;
        }
        false
    }

    /// Copy the token link, and open it where a browser would be on this
    /// machine — `Ctrl+O` in a token field.
    pub(super) fn copy_token_link(&mut self) {
        let link = cloudflare::token_link();
        render::copy_to_clipboard(&link);
        let opened = !cfg!(test) && !render::over_ssh() && share::open_in_browser(&link);
        self.set_status(match opened {
            true => "Token link copied and opened",
            false => "Token link copied",
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::line_edit::LineEdit;
    use crate::tests::test_app;
    use ratatui::crossterm::event::KeyModifiers;

    fn zone(name: &str) -> Zone {
        Zone {
            id: format!("id-{name}"),
            name: name.to_string(),
            account_id: "acct".to_string(),
            active: true,
        }
    }

    /// The settings page, with nothing connected and the cursor on `row`.
    fn on(app: &mut App, row: Row) {
        app.settings_open = true;
        app.tunnel_focus(row);
    }

    fn key(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent::from(code));
    }

    fn note(app: &App) -> Option<(Row, String, bool)> {
        app.cf
            .note
            .as_ref()
            .map(|n| (n.row, n.text.clone(), n.error))
    }

    fn type_in(app: &mut App, text: &str) {
        app.on_paste(text);
        key(app, KeyCode::Enter);
    }

    #[test]
    fn with_nothing_connected_the_section_is_the_ways_in() {
        let app = test_app();
        assert_eq!(app.tunnel_rows(), [Row::Status, Row::Login, Row::Token]);
        assert_eq!(app.tunnel_view(Row::Status).value, "not connected");
    }

    #[test]
    fn a_token_is_masked_and_a_bad_one_is_refused_without_repeating_it() {
        let mut app = test_app();
        on(&mut app, Row::Token);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cf.typing, Some(Typing::SetupToken));
        assert!(app.cf.masked());
        // An empty paste is a refusal, not a call.
        key(&mut app, KeyCode::Enter);
        assert!(app.cf.working.is_none());
        assert!(note(&app).is_some_and(|(_, _, error)| error));
        assert!(app.settings_input.is_some(), "the field stays open");

        app.settings_input = Some(LineEdit::default());
        // Longer than any other field takes, as a tunnel token is.
        let long = format!("eyJhIjoimade-up-and-broken{}", "x".repeat(400));
        type_in(&mut app, &long);
        let (row, text, error) = note(&app).expect("a refusal");
        assert_eq!(row, Row::Token);
        assert!(error);
        assert!(!text.contains("made-up"), "{text}");
    }

    #[test]
    fn several_domains_cycle_and_the_name_opens_ready_to_create() {
        let mut app = test_app();
        on(&mut app, Row::Token);
        app.tunnel_answer(
            Row::Token,
            Answer::Suggested(
                "made-up".into(),
                vec![zone("a.test"), zone("b.test")],
                0,
                Ok("cctop.a.test".into()),
            ),
        );
        assert_eq!(
            app.tunnel_rows(),
            [Row::Status, Row::SetupZone, Row::SetupHostname]
        );
        assert_eq!(app.settings_item(), Some(Item::Tunnel(Row::SetupHostname)));
        assert_eq!(app.cf.typing, Some(Typing::SetupHostname));
        assert_eq!(
            app.settings_input.as_ref().map(|i| i.to_string()),
            Some("cctop.a.test".into())
        );
        let zone_row = app.tunnel_view(Row::SetupZone);
        assert_eq!(zone_row.value, "a.test");
        assert_eq!(zone_row.shape, Shape::Cycle);

        // A deeper name is refused before any call, the field still open.
        app.settings_input = Some("a.b.a.test".into());
        key(&mut app, KeyCode::Enter);
        assert!(app.cf.working.is_none());
        assert!(note(&app).is_some_and(|(_, t, e)| e && t.contains("two levels")));
        assert!(app.settings_input.is_some());

        // Esc closes the field and keeps the setup; Backspace gives it up.
        key(&mut app, KeyCode::Esc);
        assert!(app.settings_input.is_none());
        assert!(matches!(app.cf.setup, Some(Setup::Picked { .. })));
        key(&mut app, KeyCode::Backspace);
        assert!(app.cf.setup.is_none());
        assert_eq!(app.tunnel_rows(), [Row::Status, Row::Login, Row::Token]);
    }

    #[test]
    fn a_taken_name_stays_on_the_name_and_created_is_connected() {
        let mut app = test_app();
        on(&mut app, Row::Token);
        app.tunnel_answer(
            Row::Token,
            Answer::Suggested(
                "made-up".into(),
                vec![zone("example.test")],
                0,
                Ok("cctop.example.test".into()),
            ),
        );
        app.tunnel_answer(
            Row::SetupHostname,
            Answer::Created(Err(
                "cctop.example.test already has a DNS record that cctop did not make".into(),
            )),
        );
        assert!(
            note(&app)
                .is_some_and(|(r, t, e)| r == Row::SetupHostname && e && t.contains("DNS record"))
        );
        assert_eq!(app.cf.typing, Some(Typing::SetupHostname));

        app.tunnel_answer(
            Row::SetupHostname,
            Answer::Created(Ok(Account {
                token: "made-up".into(),
                hostname: Some("cctop.example.test".into()),
                ..Account::default()
            })),
        );
        assert!(app.cf.setup.is_none());
        assert_eq!(app.settings_item(), Some(Item::Tunnel(Row::Status)));
        assert_eq!(
            app.tunnel_view(Row::Hostname).value,
            "cctop.example.test",
            "the account's rows are up"
        );
    }

    #[test]
    fn each_refusal_is_said_short_in_the_row() {
        for error in [
            cloudflare::Error::NoDomain,
            cloudflare::Error::TokenRefused,
            cloudflare::Error::MissingPermission(cloudflare::PERMISSIONS[0]),
            cloudflare::Error::LoginRefused(cloudflare::PERMISSIONS[0]),
        ] {
            let mut app = test_app();
            on(&mut app, Row::Token);
            app.tunnel_answer(
                Row::Token,
                Answer::Zones("made-up".into(), Err(error.clone())),
            );
            let (row, text, e) = note(&app).expect("a note");
            assert_eq!(row, Row::Token);
            assert!(e);
            assert_eq!(text, short(&error));
            assert!(!text.contains("permissionGroupKeys"), "{text}");
        }
    }

    #[test]
    fn enter_on_login_waits_and_backspace_cancels_it() {
        let mut app = test_app();
        let store = cctop_core::cloudflare::login::fake::server(
            cctop_core::cloudflare::login::fake::Store::Working,
        );
        app.cf.new_login = Box::new(move || Login::fake(&store));
        on(&mut app, Row::Login);
        key(&mut app, KeyCode::Enter);
        let Some(Setup::LoggingIn { url }) = &app.cf.setup else {
            panic!("not waiting on the login");
        };
        assert!(url.contains("/argotunnel?aud=&callback="), "{url}");
        assert!(app.cf.working.is_some());
        assert!(app.tunnel_view(Row::Login).value.ends_with("waiting"));
        let cancel = app.cf.cancel.clone();
        // Enter again copies the address rather than starting another.
        key(&mut app, KeyCode::Enter);
        assert!(app.cf.working.is_some());
        key(&mut app, KeyCode::Backspace);
        assert!(app.cf.setup.is_none());
        assert!(app.cf.working.is_none());
        assert!(cancel.load(Ordering::Relaxed), "the wait was told to stop");
    }

    #[test]
    fn a_login_with_no_domain_name_asks_for_the_whole_address() {
        let mut app = test_app();
        on(&mut app, Row::Login);
        let nameless = Zone {
            name: String::new(),
            ..zone("x")
        };
        app.tunnel_answer(
            Row::Login,
            Answer::LoggedIn(Ok((Auth::Login("made-up".into()), nameless))),
        );
        assert_eq!(app.cf.typing, Some(Typing::SetupHostname));
        assert_eq!(app.tunnel_view(Row::SetupZone).value, "from login");
        // A bare label cannot say which domain: refused before any call.
        app.settings_input = Some("cctop".into());
        key(&mut app, KeyCode::Enter);
        assert!(app.cf.working.is_none());
        assert!(note(&app).is_some_and(|(_, t, e)| e && t.contains("whole address")));
    }

    /// A connected account whose Access edits go to the fake Cloudflare API
    /// rather than `config.toml`. Every token is made up.
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
        let mut app = test_app();
        app.connected = Some(Connected::from(&account));
        let held = Arc::new(std::sync::Mutex::new(account));
        app.cf.apply_access = Arc::new(move |change| {
            let mut account = held.lock().unwrap();
            let api = Api::fake(&base, "made-up-api-token");
            let applied =
                access_setup::apply_with(&api, &account, change).map_err(|e| e.to_string())?;
            *account = applied.account.clone();
            Ok(applied)
        });
        app.settings_open = true;
        (app, seen)
    }

    /// Wait for the call in flight, as the tick would.
    fn settle(app: &mut App) {
        cctop_core::test_wait::eventually_true("the Access edit", || {
            app.tick_tunnel();
            app.cf.working.is_none()
        });
    }

    #[test]
    fn access_goes_on_invites_change_and_the_edge_follows() {
        let (mut app, seen) = access_app();
        assert_eq!(
            app.tunnel_rows(),
            [
                Row::Status,
                Row::Hostname,
                Row::Qr,
                Row::Zone,
                Row::Shares,
                Row::Access,
                Row::Remove
            ]
        );
        app.tunnel_focus(Row::Access);
        assert_eq!(app.tunnel_view(Row::Access).shape, Shape::Bool(false));
        // A domain is no owner: refused in the row, before any call.
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cf.typing, Some(Typing::Owner));
        type_in(&mut app, "@example.test");
        assert!(note(&app).is_some_and(|(_, _, e)| e));
        assert!(seen.lock().unwrap().is_empty());
        app.settings_input = Some(LineEdit::default());
        type_in(&mut app, "Owner@Example.test");
        settle(&mut app);
        let c = app.connected.clone().unwrap();
        assert_eq!(c.access_owner.as_deref(), Some("owner@example.test"));
        assert_eq!(c.link_hostname.as_deref(), Some("cctop-link.example.test"));
        assert_eq!(app.tunnel_view(Row::Owner).value, "owner@example.test");

        // Invite, read-only; Enter on it makes it full.
        app.tunnel_focus(Row::AddInvite);
        key(&mut app, KeyCode::Enter);
        type_in(&mut app, "guest@elsewhere.test");
        settle(&mut app);
        assert!(app.tunnel_rows().contains(&Row::Invite(0)));
        assert_eq!(
            app.tunnel_view(Row::Invite(0)).value,
            "guest@elsewhere.test · read"
        );
        app.tunnel_focus(Row::Invite(0));
        key(&mut app, KeyCode::Enter);
        settle(&mut app);
        assert_eq!(
            app.connected.as_ref().unwrap().invites[0].level,
            Level::Full
        );
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
        app.tunnel_focus(Row::Links);
        key(&mut app, KeyCode::Enter);
        settle(&mut app);
        let c = app.connected.clone().unwrap();
        assert!(!c.public_links);
        assert_eq!(c.link_hostname, None);
        assert_eq!(app.tunnel_view(Row::Links).shape, Shape::Bool(false));
        key(&mut app, KeyCode::Enter);
        settle(&mut app);
        assert!(app.connected.as_ref().unwrap().public_links);

        // Backspace on an invite takes it out.
        app.tunnel_focus(Row::Invite(0));
        key(&mut app, KeyCode::Backspace);
        settle(&mut app);
        assert!(app.connected.as_ref().unwrap().invites.is_empty());

        // Off asks first: `n` keeps it, `y` turns it off.
        app.tunnel_focus(Row::Access);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cf.confirm, Some(Row::Access));
        assert_eq!(app.tunnel_view(Row::Access).value, "y / n");
        key(&mut app, KeyCode::Char('n'));
        assert!(app.cf.confirm.is_none());
        assert!(app.connected.as_ref().unwrap().access_owner.is_some());
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('y'));
        settle(&mut app);
        assert_eq!(app.connected.as_ref().unwrap().access_owner, None);
        assert!(!app.tunnel_rows().contains(&Row::Owner));
    }

    #[test]
    fn a_login_account_is_asked_for_a_token_before_access_not_after() {
        let mut app = test_app();
        app.connected = Some(Connected::from(&Account {
            hostname: Some("cctop.example.test".into()),
            api_token: Some("made-up-login-token".into()),
            account_id: Some("acct1".into()),
            zone_id: Some("zone1".into()),
            tunnel_id: Some("tunnel1".into()),
            login: true,
            ..Account::default()
        }));
        on(&mut app, Row::Access);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cf.typing, Some(Typing::ApiToken));
        assert!(app.cf.masked());
        app.on_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        assert!(
            app.settings_input.as_ref().is_some_and(|i| i.is_empty()),
            "Ctrl+O copies the link, it is not typed"
        );
        // A token far longer than an email still fits.
        for _ in 0..300 {
            key(&mut app, KeyCode::Char('x'));
        }
        assert_eq!(
            app.settings_input.as_ref().map(|i| i.chars().count()),
            Some(300)
        );

        // Once the token is taken, the owner's email is next.
        app.settings_input = None;
        app.cf.typing = None;
        app.tunnel_answer(
            Row::Access,
            Answer::Adopted(Ok(Account {
                hostname: Some("cctop.example.test".into()),
                api_token: Some("made-up-api-token".into()),
                ..Account::default()
            })),
        );
        assert_eq!(app.cf.typing, Some(Typing::Owner));
        assert_eq!(app.settings_item(), Some(Item::Tunnel(Row::Access)));
    }

    #[test]
    fn remove_asks_first_and_an_env_tunnel_is_not_ours() {
        let mut app = test_app();
        app.connected = Some(Connected::from(&Account {
            token: "made-up".into(),
            from_env: true,
            ..Account::default()
        }));
        on(&mut app, Row::Remove);
        key(&mut app, KeyCode::Char('y'));
        assert!(app.cf.confirm.is_none(), "y alone does nothing");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cf.confirm, Some(Row::Remove));
        assert_eq!(app.tunnel_view(Row::Remove).tone, Tone::Ask);
        key(&mut app, KeyCode::Esc);
        assert!(app.cf.confirm.is_none());
        assert!(app.on_settings(), "Esc answered the question, not the page");
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Char('y'));
        assert!(
            note(&app)
                .is_some_and(|(r, t, e)| r == Row::Remove && e && t.contains("CCTOP_TUNNEL_TOKEN"))
        );
        assert!(app.connected.is_some());
    }

    #[test]
    fn a_dns_refusal_is_said_on_the_rows_that_would_write() {
        let mut app = test_app();
        let mut c = Connected::from(&Account {
            token: "made-up".into(),
            hostname: Some("cctop.example.test".into()),
            ..Account::default()
        });
        c.rename = Err("the token cannot write DNS");
        app.connected = Some(c);
        on(&mut app, Row::Hostname);
        key(&mut app, KeyCode::Enter);
        assert!(app.settings_input.is_none());
        assert!(
            note(&app)
                .is_some_and(|(r, t, e)| r == Row::Hostname && e && t.contains("cannot write DNS"))
        );
    }

    #[test]
    fn a_rename_opens_on_the_label_and_its_answer_lands_in_the_row() {
        let mut app = test_app();
        app.connected = Some(Connected::from(&Account {
            token: "made-up".into(),
            hostname: Some("cctop.example.test".into()),
            account_id: Some("acct1".into()),
            zone_id: Some("zone1".into()),
            tunnel_id: Some("tunnel1".into()),
            api_token: Some("made-up-api-token".into()),
            ..Account::default()
        }));
        on(&mut app, Row::Hostname);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cf.typing, Some(Typing::Hostname));
        assert!(!app.cf.masked());
        // The zone comes from the account's stored zone name, which this
        // made-up one does not have, so the whole hostname is the field.
        assert_eq!(
            app.settings_input.as_ref().map(|i| i.to_string()),
            Some(label_of(
                "cctop.example.test",
                app.connected.as_ref().unwrap().zone.as_deref()
            ))
        );
        key(&mut app, KeyCode::Esc);
        app.tunnel_answer(
            Row::Hostname,
            Answer::Renamed(Err(
                "new.example.test is already used by the dashboard".into()
            )),
        );
        assert!(note(&app).is_some_and(|(r, _, e)| r == Row::Hostname && e));
        assert_eq!(app.tunnel_view(Row::Hostname).tone, Tone::Error);
    }

    /// The default share hostname is renamed in place like the dashboard's,
    /// and the row says before the Enter that does it that old links break.
    #[test]
    fn the_share_hostname_is_renamed_in_place_and_says_old_links_break() {
        let (mut app, _) = access_app();
        on(&mut app, Row::Shares);
        assert!(
            app.tunnel_view(Row::Shares).what.contains("breaks links"),
            "the row says it before anyone opens it"
        );
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cf.typing, Some(Typing::ShareHostname));
        assert!(note(&app).is_some_and(|(r, t, e)| r == Row::Shares && !e && t.contains("break")));
        // Empty is refused in the row, the field still open.
        app.settings_input = Some(LineEdit::default());
        key(&mut app, KeyCode::Enter);
        assert!(app.cf.working.is_none());
        assert!(note(&app).is_some_and(|(r, _, e)| r == Row::Shares && e));
        assert_eq!(app.cf.typing, Some(Typing::ShareHostname));
        key(&mut app, KeyCode::Esc);

        // What the worker comes back with lands in the row: the move, and
        // what it cost; a refusal in red.
        let renamed = |old: Option<&str>| Renamed {
            account: Account::default(),
            old: old.map(str::to_string),
            new: Some("w.example.test".into()),
            changed: true,
            note: None,
        };
        app.tunnel_answer(
            Row::Shares,
            Answer::Renamed(Ok(renamed(Some("cctop-share.example.test")))),
        );
        assert!(
            note(&app).is_some_and(|(r, t, e)| r == Row::Shares && !e && t.contains("old W links"))
        );
        app.tunnel_answer(
            Row::Shares,
            Answer::Renamed(Err(
                "cctop.example.test is already used by the dashboard".into()
            )),
        );
        assert_eq!(app.tunnel_view(Row::Shares).tone, Tone::Error);
    }

    /// A served stand-in, its tunnel link made up.
    fn serve(app: &mut App) {
        app.serving = Some(share::Served {
            local: "http://127.0.0.1:7777/?t=made-up".into(),
            public: Some("https://cctop.example.test/?t=made-up".into()),
            tunnel_fallback: None,
            readonly: String::new(),
            actions: true,
            publish: Box::new(|_, _, _| {}),
        });
    }

    #[test]
    fn the_qr_row_draws_the_served_link_until_any_key() {
        let (mut app, _) = access_app();
        on(&mut app, Row::Qr);
        // Nothing served: no code, and the row says what would make one.
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cf.qr, None);
        assert!(note(&app).is_some_and(|(r, _, e)| r == Row::Qr && e));

        serve(&mut app);
        assert_eq!(app.tunnel_view(Row::Qr).value, "↵ show");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cf.qr, Some(QrOf::Dashboard));
        assert_eq!(
            app.qr_link(QrOf::Dashboard),
            Some("https://cctop.example.test/?t=made-up")
        );
        // Any key puts it away and does nothing else: not even `q`'s quit.
        key(&mut app, KeyCode::Char('q'));
        assert_eq!(app.cf.qr, None);
        assert!(app.on_settings());
    }

    #[test]
    fn q_on_a_login_being_waited_for_draws_its_link() {
        let mut app = test_app();
        on(&mut app, Row::Login);
        app.cf.setup = Some(Setup::LoggingIn {
            url: "https://dash.cloudflare.com/argotunnel?aud=&callback=made-up".into(),
        });
        key(&mut app, KeyCode::Char('q'));
        assert_eq!(app.cf.qr, Some(QrOf::Login));
        key(&mut app, KeyCode::Down);
        assert_eq!(app.cf.qr, None);
        assert_eq!(
            app.settings_item(),
            Some(Item::Tunnel(Row::Login)),
            "the key that closed it moved nothing"
        );
    }

    /// The login finished on the phone takes its code with it, and the next
    /// key — the first letter of the address it now asks for — is typed
    /// rather than spent closing a code no longer on screen.
    #[test]
    fn a_login_finished_under_its_code_lets_the_next_key_through() {
        let mut app = test_app();
        on(&mut app, Row::Login);
        app.cf.setup = Some(Setup::LoggingIn {
            url: "https://dash.cloudflare.com/argotunnel?aud=&callback=made-up".into(),
        });
        key(&mut app, KeyCode::Char('q'));
        assert_eq!(app.cf.qr, Some(QrOf::Login));
        let nameless = Zone {
            name: String::new(),
            ..zone("x")
        };
        app.tunnel_answer(
            Row::Login,
            Answer::LoggedIn(Ok((Auth::Login("made-up".into()), nameless))),
        );
        assert_eq!(app.cf.typing, Some(Typing::SetupHostname));
        key(&mut app, KeyCode::Char('c'));
        assert_eq!(app.cf.qr, None);
        assert_eq!(
            app.settings_input.as_ref().map(|f| f.to_string()),
            Some("c".to_string()),
            "the key reached the field"
        );
    }

    /// A serve stopped under the dashboard's code: the key moves the cursor.
    #[test]
    fn a_serve_stopped_under_its_code_lets_the_next_key_through() {
        let (mut app, _) = access_app();
        serve(&mut app);
        on(&mut app, Row::Qr);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.cf.qr, Some(QrOf::Dashboard));
        app.serving = None;
        key(&mut app, KeyCode::Down);
        assert_eq!(app.cf.qr, None);
        assert_eq!(app.settings_item(), Some(Item::Tunnel(Row::Zone)));
    }
}
