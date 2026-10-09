//! `/api/address`: choosing the dashboard's address, and an agent's, from the
//! page.
//!
//! The same renames the terminal's `e` makes ([`cloudflare::name_dashboard`],
//! [`cloudflare::name_share`]), behind the same guards as every action: the
//! full token, a serve that allows actions, and a JSON `POST` for a write.
//! Even the reads want the full token — the read-only page never shows the
//! controls, so it has nothing to ask — and a read-only link gets 403.
//!
//! - `GET /api/address` — the dashboard's hostname, when it can be chosen.
//! - `POST /api/address` `{"name": …}` — move the dashboard; the answer carries
//!   the new origin, which the page navigates to with its token.
//! - `GET /api/address/<session>` — the hostname `W` uses for that agent.
//! - `POST /api/address/<session>` `{"name": …}` — rename it; empty clears.
//!
//! The web edits the stored name and nothing else: it has no `W` of its own,
//! so a new share link is the terminal's to mint.

use super::http::{self, Request};
use super::{Access, Shared, current, find, json, may_act};
use cctop_core::cloudflare::{self, Renamed};
use cctop_core::tunnel::Account;
use std::net::TcpStream;

/// Where the account and the renames come from: the connected account and
/// Cloudflare, or a test's fake of both.
pub trait Addresses: Send + Sync {
    /// The connected account, as `config.toml` and the environment say.
    fn account(&self) -> Option<Account>;
    /// The hostname the page answers on over the account's tunnel right now,
    /// when it is on one.
    fn page_host(&self) -> Option<String>;
    fn name_share(
        &self,
        session_id: &str,
        input: &str,
        agents: &dyn Fn(&str) -> String,
    ) -> Result<Renamed, String>;
    fn name_dashboard(&self, input: &str) -> Result<Renamed, String>;
}

/// The real thing.
pub struct Connected;

impl Addresses for Connected {
    fn account(&self) -> Option<Account> {
        cctop_core::tunnel::account()
    }
    fn page_host(&self) -> Option<String> {
        cctop_core::tunnel::page_host()
    }
    fn name_share(
        &self,
        session_id: &str,
        input: &str,
        agents: &dyn Fn(&str) -> String,
    ) -> Result<Renamed, String> {
        cloudflare::name_share(session_id, input, agents)
    }
    fn name_dashboard(&self, input: &str) -> Result<Renamed, String> {
        cloudflare::name_dashboard(input)
    }
}

/// What the page is told about an address.
#[derive(serde::Serialize, Debug, PartialEq, Eq)]
struct Address {
    /// The hostname now, when there is one to show.
    host: Option<String>,
    /// The domain a typed label goes under.
    zone: Option<String>,
    /// For an agent: the hostname an empty name sends it back to.
    default: Option<String>,
    /// Whether it can be renamed here.
    renamable: bool,
    /// Why not, when it cannot.
    why: Option<String>,
}

/// What a rename did, for the page.
#[derive(serde::Serialize)]
struct Done {
    host: Option<String>,
    /// The page's new origin, after a dashboard rename.
    origin: Option<String>,
    old: Option<String>,
    changed: bool,
    note: Option<String>,
}

impl From<Renamed> for Done {
    fn from(renamed: Renamed) -> Done {
        Done {
            origin: None,
            host: renamed.new,
            old: renamed.old,
            changed: renamed.changed,
            note: renamed.note,
        }
    }
}

/// A refusal as the sentence alone, which the dialog shows as it is.
fn refuse(stream: &mut TcpStream, request: &Request, status: u16, why: &str) {
    http::respond(
        stream,
        Some(request),
        status,
        "text/plain; charset=utf-8",
        why.as_bytes(),
    );
}

/// `/api/address` and `/api/address/<session>`.
pub fn route(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    rest: &str,
    access: Access,
) {
    if request.method == "POST" {
        let Some(body) = may_act(shared, stream, request, access) else {
            return;
        };
        let name = body
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        return match rest {
            "" => rename_dashboard(shared, stream, request, &name),
            id => rename_share(shared, stream, request, id, &name),
        };
    }
    // The reads behind the full token too: they exist for controls only the
    // full page draws.
    if access != Access::Full || !shared.actions {
        return http::respond_error(
            stream,
            Some(request),
            403,
            "this link cannot choose addresses",
        );
    }
    let addresses = &*shared.addresses;
    match rest {
        "" => json(stream, request, &dashboard(addresses)),
        id => {
            let snapshot = current(shared);
            let Some(session) = find(&snapshot.sessions, id) else {
                return http::respond_error(stream, Some(request), 404, super::NO_SUCH_SESSION);
            };
            json(stream, request, &share(addresses, &session.session_id))
        }
    }
}

/// The dashboard's address: renamable while the page is served on the
/// account's own domain by an account cctop can write DNS for.
fn dashboard(addresses: &dyn Addresses) -> Address {
    let account = addresses.account();
    let page = addresses.page_host();
    let why = match (&account, &page) {
        (None, _) => Some("No Cloudflare account is connected".to_string()),
        (Some(_), None) => Some("The page is not on your own domain".to_string()),
        (Some(account), Some(_)) => account.can_rename().err().map(str::to_string),
    };
    Address {
        host: page,
        zone: account
            .as_ref()
            .and_then(|a| a.zone_name())
            .map(str::to_string),
        default: None,
        renamable: why.is_none(),
        why,
    }
}

/// An agent's share address, as `W` would use it: its own name or the
/// default share hostname. Nothing to show without an account that has
/// share hostnames, since `W` then goes out on a quick tunnel.
fn share(addresses: &dyn Addresses, session_id: &str) -> Address {
    let Some(account) = addresses.account().filter(|a| a.share_hostname.is_some()) else {
        return Address {
            host: None,
            zone: None,
            default: None,
            renamable: false,
            why: Some("Shares go out on a quick tunnel without your own domain".to_string()),
        };
    };
    let why = account.can_rename().err().map(str::to_string);
    Address {
        host: account.share_host_for(session_id),
        zone: account.zone_name().map(str::to_string),
        default: account.share_hostname.clone(),
        renamable: why.is_none(),
        why,
    }
}

fn rename_dashboard(shared: &Shared, stream: &mut TcpStream, request: &Request, name: &str) {
    let addresses = &*shared.addresses;
    let state = dashboard(addresses);
    if let Some(why) = state.why {
        return refuse(stream, request, 409, &why);
    }
    match addresses.name_dashboard(name) {
        Ok(renamed) => {
            let mut done = Done::from(renamed);
            done.origin = done.host.as_ref().map(|host| format!("https://{host}"));
            json(stream, request, &done)
        }
        Err(why) => refuse(stream, request, 400, &why),
    }
}

fn rename_share(shared: &Shared, stream: &mut TcpStream, request: &Request, id: &str, name: &str) {
    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, super::NO_SUCH_SESSION);
    };
    let addresses = &*shared.addresses;
    if let Some(why) = share(addresses, &session.session_id).why {
        return refuse(stream, request, 409, &why);
    }
    let agents = |other: &str| match snapshot.sessions.iter().find(|s| s.session_id == other) {
        Some(s) => format!("{}'s shares", s.display_label()),
        None => "another agent's shares".to_string(),
    };
    match addresses.name_share(&session.session_id, name, &agents) {
        Ok(renamed) => json(stream, request, &Done::from(renamed)),
        Err(why) => refuse(stream, request, 400, &why),
    }
}

/// Nothing connected: what a test's server answers by default.
#[cfg(test)]
pub struct Nowhere;

#[cfg(test)]
impl Addresses for Nowhere {
    fn account(&self) -> Option<Account> {
        None
    }
    fn page_host(&self) -> Option<String> {
        None
    }
    fn name_share(&self, _: &str, _: &str, _: &dyn Fn(&str) -> String) -> Result<Renamed, String> {
        Err("nothing connected".into())
    }
    fn name_dashboard(&self, _: &str) -> Result<Renamed, String> {
        Err("nothing connected".into())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use cctop_core::cloudflare::{Api, fake};
    use cctop_core::tunnel::ShareName;
    use std::sync::Mutex;

    /// An account held in memory and renamed against the fake Cloudflare: the
    /// real rename, with nothing written to `config.toml`.
    pub struct Fake {
        pub base: String,
        pub account: Mutex<Account>,
        pub page: Mutex<Option<String>>,
    }

    impl Addresses for Fake {
        fn account(&self) -> Option<Account> {
            Some(self.account.lock().unwrap().clone())
        }
        fn page_host(&self) -> Option<String> {
            self.page.lock().unwrap().clone()
        }
        fn name_share(
            &self,
            session_id: &str,
            input: &str,
            agents: &dyn Fn(&str) -> String,
        ) -> Result<Renamed, String> {
            let mut account = self.account.lock().unwrap();
            let api = Api::fake(&self.base, account.api_token.as_deref().unwrap_or_default());
            let renamed = cloudflare::name_share_with(&api, &account, session_id, input, agents)
                .map_err(|e| e.to_string())?;
            *account = renamed.account.clone();
            Ok(renamed)
        }
        fn name_dashboard(&self, input: &str) -> Result<Renamed, String> {
            let mut account = self.account.lock().unwrap();
            let api = Api::fake(&self.base, account.api_token.as_deref().unwrap_or_default());
            let renamed = cloudflare::name_dashboard_with(&api, &account, input)
                .map_err(|e| e.to_string())?;
            *account = renamed.account.clone();
            *self.page.lock().unwrap() = renamed.new.clone();
            Ok(renamed)
        }
    }

    /// An account cctop set up, serving on its own domain, against a fake
    /// Cloudflare that accepts everything. Every token is made up.
    pub fn fake() -> (Fake, fake::Seen) {
        let (base, seen) = fake::api(fake::accepting(None));
        let account = Account {
            token: "eyJhIjoi-made-up".into(),
            hostname: Some("cctop.example.test".into()),
            share_hostname: Some("cctop-share.example.test".into()),
            account_id: Some("acct1".into()),
            zone_id: Some("zone1".into()),
            tunnel_id: Some(fake::TUNNEL_ID.into()),
            dns_record_ids: vec!["rec-page".into(), "rec-share".into()],
            api_token: Some("made-up-api-token".into()),
            login: false,
            share_names: [(
                "other".to_string(),
                ShareName {
                    label: "taken".into(),
                    record_id: Some("rec-other".into()),
                },
            )]
            .into(),
            access: None,
            from_env: false,
        };
        let fake = Fake {
            base,
            account: Mutex::new(account),
            page: Mutex::new(Some("cctop.example.test".into())),
        };
        (fake, seen)
    }
}
