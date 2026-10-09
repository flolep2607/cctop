//! `/api/access`: who may log in through Cloudflare Access, seen and changed
//! from the page.
//!
//! The same edits `cctop tunnel access` and the dashboard's Cloudflare popup
//! make ([`access_setup::apply`]), behind a narrower gate than any other
//! action: the full token, or the **owner's** Access login. A full invite may
//! act on sessions but not hand out access — an invite that could invite
//! would make every full invite an owner — and a read-only token or login is
//! refused the list as well as the edits, since it has no controls to fill.
//!
//! - `GET /api/access` — on or off, the owner, the invites, the token links.
//! - `POST /api/access` — one edit, a JSON object with `op`:
//!   `{"op":"on","owner":…}`, `{"op":"off"}`,
//!   `{"op":"invite","who":…,"level":"read"|"full"}`,
//!   `{"op":"remove","who":…}`, `{"op":"links","on":true|false}`.
//!   The answer is what happened and the list as it is now.

use super::http::{self, Request};
use super::{Access, Shared, json, may_act};
use cctop_core::cloudflare::access::Level;
use cctop_core::cloudflare::access_setup::Change;
use cctop_core::tunnel::Account;
use std::net::TcpStream;

/// What the page is told.
#[derive(serde::Serialize, Debug, PartialEq, Eq)]
struct State {
    on: bool,
    owner: Option<String>,
    invites: Vec<Invited>,
    /// Whether a token link still opens the page from outside.
    public_links: bool,
    /// Where token links go while Access is on.
    link_host: Option<String>,
    /// The dashboard's hostname, the one Access asks a login for.
    page_host: Option<String>,
    /// Whether cctop can make edits on this account, or why not.
    editable: bool,
    why: Option<String>,
}

#[derive(serde::Serialize, Debug, PartialEq, Eq)]
struct Invited {
    who: String,
    level: &'static str,
}

#[derive(serde::Serialize)]
struct Done {
    said: String,
    /// What could not be deleted from Cloudflare, to delete by hand.
    left: Vec<String>,
    state: State,
}

fn state(account: Option<&Account>) -> State {
    let why = match account {
        None => Some("No Cloudflare account is connected".to_string()),
        Some(account) => account.can_rename().err().map(str::to_string),
    };
    let access = account.and_then(|a| a.access.as_deref());
    State {
        on: access.is_some(),
        owner: access.map(|a| a.owner.clone()),
        invites: access
            .map(|a| {
                a.invites
                    .iter()
                    .map(|i| Invited {
                        who: i.who.clone(),
                        level: i.level.word(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        public_links: access.is_none_or(|a| a.public_links),
        link_host: access.and_then(|a| a.link_hostname.clone()),
        page_host: account.and_then(|a| a.hostname.clone()),
        editable: why.is_none(),
        why,
    }
}

/// The edit a POST asks for, or the sentence saying what is wrong with it.
fn change_of(body: &serde_json::Value) -> Result<Change, &'static str> {
    let text = |key: &str| {
        body.get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    match body.get("op").and_then(serde_json::Value::as_str) {
        Some("on") => Ok(Change::On {
            owner: text("owner"),
        }),
        Some("off") => Ok(Change::Off),
        Some("invite") => Ok(Change::Invite {
            who: text("who"),
            level: match body.get("level").and_then(serde_json::Value::as_str) {
                None => Level::Read,
                Some(word) => Level::from_word(word).ok_or("level is read or full")?,
            },
        }),
        Some("remove") => Ok(Change::Uninvite { who: text("who") }),
        Some("links") => body
            .get("on")
            .and_then(serde_json::Value::as_bool)
            .map(Change::PublicLinks)
            .ok_or("links wants \"on\": true or false"),
        _ => Err("op is one of on, off, invite, remove, links"),
    }
}

/// `/api/access`. `admin` is the full token or the owner's login, as the
/// gate decided it; nothing else gets past the first line.
pub fn route(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    access: Access,
    admin: bool,
) {
    if !admin || access != Access::Full {
        return http::respond_error(
            stream,
            Some(request),
            403,
            "only the full link or the owner's login can see or change who may log in",
        );
    }
    if request.method != "POST" {
        if !shared.actions {
            return http::respond_error(
                stream,
                Some(request),
                403,
                "this cctop serve is read-only — restart it without --no-actions to \
                 change who may log in",
            );
        }
        return json(stream, request, &state(shared.addresses.account().as_ref()));
    }
    let Some(body) = may_act(shared, stream, request, access) else {
        return;
    };
    let change = match change_of(&body) {
        Ok(change) => change,
        Err(why) => return http::respond_error(stream, Some(request), 400, why),
    };
    match shared.addresses.change_access(change) {
        Ok(applied) => json(
            stream,
            request,
            &Done {
                said: applied.said,
                left: applied.left,
                state: state(Some(&applied.account)),
            },
        ),
        // The sentence alone, which the dialog shows as it is.
        Err(why) => http::respond(
            stream,
            Some(request),
            400,
            "text/plain; charset=utf-8",
            why.as_bytes(),
        ),
    }
}
