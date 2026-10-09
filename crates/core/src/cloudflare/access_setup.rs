//! Putting the page behind Cloudflare Access, and taking it out again: what
//! `cctop tunnel access on|off` and `cctop tunnel invite` do on Cloudflare.
//!
//! Three things make a hostname ask for a login, all on the account:
//!
//! - an **Access application** on the dashboard's hostname, whose AUD tag is
//!   what the server checks every login's `aud` against
//!   ([`super::access::check`]);
//! - a **reusable policy** attached to it, allowing the owner's email and the
//!   invites — each email, or a whole domain. Reusable rather than inline
//!   because Cloudflare calls the inline kind legacy, and one policy updated
//!   in place is all an invite edit needs;
//! - an **identity provider** to log in with. The one-time PIN is the one
//!   that needs no account anywhere: Access emails a code to whoever asks,
//!   and lets them in if the policy allows their address. The account's own
//!   is used when it has one; cctop makes one only when it does not, and only
//!   then deletes it again.
//!
//! The Zero Trust organization above them — the team name, which becomes
//! `<team>.cloudflareaccess.com` — is the one thing not created here. It asks
//! for a plan to be picked (the free one does), and that is a choice in the
//! dashboard rather than an API default to make on someone's behalf, so its
//! absence is an error that says where to click ([`Error::NoTeam`]).
//!
//! Beside them, outside the application, a **token hostname**
//! (`cctop-link.<domain>`): a DNS record and an ingress entry of the tunnel's
//! own, routed to the page like the dashboard's hostname but asking for no
//! login, so a `?t=` link still works for someone who cannot log in. It is
//! made with the application and removed with it, and "public token links:
//! off" removes it alone ([`Change::PublicLinks`]); the page's server then
//! refuses a token on any tunnel hostname as a second lock.
//!
//! As with the tunnel, only what cctop created is deleted, by the ids it
//! stored, and a setup that fails halfway deletes what it made before
//! returning.
//!
//! The terminal's `cctop tunnel access`, the dashboard's Cloudflare popup and
//! the web page's Access dialog all make their edits through [`apply`], so
//! the three cannot drift apart in what an edit does on Cloudflare.

use serde_json::{Value, json};

use super::access::{Invite, Level, Settings};
use super::{Api, Error};
use crate::tunnel::Account;

/// The two permissions the Access calls need, as the dashboard names them.
pub const PERMISSIONS: [&str; 2] = [
    "Account · Access: Apps and Policies · Edit",
    "Account · Access: Organizations, Identity Providers, and Groups · Edit",
];

/// How long a login lasts before Access asks again. A working day and then
/// some: the page is opened every morning, and a code by email each time it
/// is opened would be the reason nobody uses it.
const SESSION: &str = "24h";

/// The sentence for an account whose Zero Trust has never been opened.
pub const NO_TEAM: &str = "Cloudflare Access is not set up on this account yet. Open \
     https://one.dash.cloudflare.com once, pick a team name and the free plan, then run \
     this again.";

/// The account and hostname Access goes on, or why there are none: the
/// tunnel was connected with a tunnel token, and nothing cctop holds can
/// write to the account.
fn target(account: &Account) -> Result<(&str, &str), Error> {
    account
        .can_rename()
        .map_err(|why| Error::Hostname(why.to_string()))?;
    match (account.account_id.as_deref(), account.hostname.as_deref()) {
        (Some(id), Some(host)) => Ok((id, host)),
        _ => Err(Error::Hostname(crate::tunnel::TOKEN_ONLY.to_string())),
    }
}

/// The policy's `include` rules: any one of them lets a login in.
fn rules(owner: &str, invites: &[Invite]) -> Vec<Value> {
    std::iter::once(json!({"email": {"email": owner}}))
        .chain(
            invites
                .iter()
                .map(|invite| match invite.who.strip_prefix('@') {
                    Some(domain) => json!({"email_domain": {"domain": domain}}),
                    None => json!({"email": {"email": invite.who}}),
                }),
        )
        .collect()
}

fn policy_body(host: &str, owner: &str, invites: &[Invite]) -> Value {
    json!({
        "name": format!("cctop: who may open {host}"),
        "decision": "allow",
        "include": rules(owner, invites),
        "session_duration": SESSION,
    })
}

impl Api {
    /// The Zero Trust team's domain, `<team>.cloudflareaccess.com`.
    fn team_domain(&self, account_id: &str) -> Result<String, Error> {
        let found = self.call(
            "GET",
            &format!("/accounts/{account_id}/access/organizations"),
            None,
            PERMISSIONS[1],
        );
        match found {
            Ok(org) => org["auth_domain"]
                .as_str()
                .filter(|d| !d.is_empty())
                .map(String::from)
                .ok_or(Error::NoTeam),
            // Cloudflare answers an account with no organization with an
            // error rather than an empty one.
            Err(Error::Api(_)) => Err(Error::NoTeam),
            Err(e) => Err(e),
        }
    }

    /// The one-time PIN provider: the account's own, or a new one. The id
    /// is returned with whether cctop made it, which decides whether cctop
    /// may delete it.
    fn one_time_pin(&self, account_id: &str) -> Result<(String, bool), Error> {
        let path = format!("/accounts/{account_id}/access/identity_providers");
        let existing = self.call("GET", &path, None, PERMISSIONS[1])?;
        let found = existing.as_array().into_iter().flatten().find_map(|idp| {
            (idp["type"].as_str() == Some("onetimepin"))
                .then(|| idp["id"].as_str().map(String::from))
                .flatten()
        });
        if let Some(id) = found {
            return Ok((id, false));
        }
        let made = self.call(
            "POST",
            &path,
            Some(json!({"name": "One-time PIN", "type": "onetimepin", "config": {}})),
            PERMISSIONS[1],
        )?;
        made["id"]
            .as_str()
            .map(|id| (id.to_string(), true))
            .ok_or_else(|| Error::Api("the identity provider came back without an id".into()))
    }

    fn create_policy(&self, account_id: &str, body: Value) -> Result<String, Error> {
        let made = self.call(
            "POST",
            &format!("/accounts/{account_id}/access/policies"),
            Some(body),
            PERMISSIONS[0],
        )?;
        made["id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| Error::Api("the Access policy came back without an id".into()))
    }

    /// The application on `host`, with the policy attached: its id and AUD.
    fn create_app(
        &self,
        account_id: &str,
        host: &str,
        policy: &str,
    ) -> Result<(String, String), Error> {
        let made = self.call(
            "POST",
            &format!("/accounts/{account_id}/access/apps"),
            Some(json!({
                "name": format!("cctop on {host}"),
                "domain": host,
                "type": "self_hosted",
                "session_duration": SESSION,
                "app_launcher_visible": false,
                "policies": [{"id": policy, "precedence": 1}],
            })),
            PERMISSIONS[0],
        )?;
        match (made["id"].as_str(), made["aud"].as_str()) {
            (Some(id), Some(aud)) => Ok((id.to_string(), aud.to_string())),
            _ => Err(Error::Api(
                "the Access application came back without its id or AUD tag".into(),
            )),
        }
    }

    fn delete_access(
        &self,
        account_id: &str,
        kind: &str,
        id: &str,
        needs: &'static str,
    ) -> Result<(), Error> {
        self.call(
            "DELETE",
            &format!("/accounts/{account_id}/access/{kind}/{id}"),
            None,
            needs,
        )
        .map(drop)
    }
}

/// Point the token hostname at the tunnel: refused before any write when
/// the name already has a record cctop did not make. Returns the hostname and
/// what to delete its record by.
fn add_link(api: &Api, account: &Account) -> Result<(String, String), Error> {
    let (zone, tunnel_id) = super::writable_zone(account)?;
    let page = account
        .hostname
        .as_deref()
        .ok_or_else(|| Error::Hostname(crate::tunnel::TOKEN_ONLY.to_string()))?;
    let host = super::link_hostname(page);
    if api.record_exists(&zone, &host)? {
        return Err(Error::Hostname(format!(
            "{host}, where token links would go, already has a DNS record that cctop did \
             not make. Delete it, or turn public token links off."
        )));
    }
    let record = api.add_cname(&zone, &host, &tunnel_id)?;
    Ok((host, record))
}

/// Delete the token hostname's record, or the sentence saying it is left.
fn delete_link(api: &Api, account: &Account, settings: &Settings) -> Option<String> {
    let record = settings.link_record_id.as_ref()?;
    let (zone_id, tunnel_id) = (account.zone_id.as_ref()?, account.tunnel_id.as_ref()?);
    api.delete_record(zone_id, record, tunnel_id)
        .err()
        .map(|e| {
            format!(
                "the DNS record {record} of {} ({e})",
                settings
                    .link_hostname
                    .as_deref()
                    .unwrap_or("the token hostname")
            )
        })
}

/// Write the tunnel's ingress list for `account` as it now is. Only the
/// dashboard reads that list — cctop routes by its own table — so it is
/// written for the display's sake, and a failure is the caller's to weigh.
fn configure(api: &Api, account: &Account) -> Result<(), Error> {
    let (zone, tunnel_id) = super::writable_zone(account)?;
    let names = super::hostnames(account);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    api.configure(&zone.account_id, &tunnel_id, &names)
}

/// `account` with `settings` as its Access.
fn with_settings(account: &Account, settings: &Settings) -> Account {
    Account {
        access: Some(Box::new(settings.clone())),
        ..account.clone()
    }
}

/// Put `account`'s page behind Access for `owner`, keeping `invites`, and
/// return the settings to store. An account already behind Access has its
/// policy updated instead, so `access on` run twice changes the owner rather
/// than making a second application.
pub fn enable(
    api: &Api,
    account: &Account,
    owner: &str,
    invites: Vec<Invite>,
) -> Result<Settings, Error> {
    let owner = super::access::parse_who(owner)
        .filter(|o| !o.starts_with('@'))
        .ok_or_else(|| Error::Hostname(format!("{owner} is not an email address")))?;
    if let Some(existing) = account.access.as_ref().filter(|a| a.app_id.is_some()) {
        let settings = Settings {
            owner,
            invites,
            ..(**existing).clone()
        };
        update(api, account, &settings)?;
        return Ok(settings);
    }
    let (account_id, host) = target(account)?;
    let team = api.team_domain(account_id)?;
    let (idp, made_idp) = api.one_time_pin(account_id)?;
    let mut made_policy = None;
    let mut made_app = None;
    let mut made_link = None;
    let built = (|| {
        let policy = api.create_policy(account_id, policy_body(host, &owner, &invites))?;
        made_policy = Some(policy.clone());
        let (app, aud) = api.create_app(account_id, host, &policy)?;
        made_app = Some(app.clone());
        // The token hostname with it, so `access on` changes nothing for a
        // token link but its address; `links off` is the step that does.
        let (link, record) = add_link(api, account)?;
        made_link = Some(record.clone());
        let settings = Settings {
            team: team.clone(),
            aud,
            owner: owner.clone(),
            invites: invites.clone(),
            app_id: Some(app),
            policy_id: Some(policy),
            idp_id: made_idp.then(|| idp.clone()),
            public_links: true,
            link_hostname: Some(link),
            link_record_id: Some(record),
        };
        configure(api, &with_settings(account, &settings))?;
        Ok(settings)
    })();
    match built {
        Ok(settings) => Ok(settings),
        Err(e) => {
            // Best effort, newest first; the error worth reporting is the
            // one that stopped the setup.
            let undo = Settings {
                link_hostname: None,
                link_record_id: made_link,
                ..Settings::default()
            };
            let _ = delete_link(api, account, &undo);
            if let Some(app) = made_app {
                let _ = api.delete_access(account_id, "apps", &app, PERMISSIONS[0]);
            }
            if let Some(policy) = made_policy {
                let _ = api.delete_access(account_id, "policies", &policy, PERMISSIONS[0]);
            }
            if made_idp {
                let _ = api.delete_access(account_id, "identity_providers", &idp, PERMISSIONS[1]);
            }
            Err(e)
        }
    }
}

/// Write `settings`' owner and invites to the policy cctop made, so the edge
/// lets in exactly who the server will.
pub fn update(api: &Api, account: &Account, settings: &Settings) -> Result<(), Error> {
    let (account_id, host) = target(account)?;
    let Some(policy) = &settings.policy_id else {
        // Set up by hand: cctop's list still decides who the server lets in,
        // and the edge's policy is the user's to keep in step.
        return Ok(());
    };
    api.call(
        "PUT",
        &format!("/accounts/{account_id}/access/policies/{policy}"),
        Some(policy_body(host, &settings.owner, &settings.invites)),
        PERMISSIONS[0],
    )
    .map(drop)
}

/// Delete what [`enable`] made — the application, then its policy, then the
/// one-time PIN provider if cctop made that too — and return what could not
/// be deleted, for the user to delete by hand.
pub fn disable(api: &Api, account: &Account, settings: &Settings) -> Vec<String> {
    let mut left = Vec::new();
    let Some(account_id) = account.account_id.as_deref() else {
        return left;
    };
    // The application first: a policy still attached to one cannot go.
    let made = [
        (
            "apps",
            "the Access application",
            &settings.app_id,
            PERMISSIONS[0],
        ),
        (
            "policies",
            "the Access policy",
            &settings.policy_id,
            PERMISSIONS[0],
        ),
        (
            "identity_providers",
            "the one-time PIN provider",
            &settings.idp_id,
            PERMISSIONS[1],
        ),
    ];
    for (kind, what, id, needs) in made {
        if let Some(id) = id
            && let Err(e) = api.delete_access(account_id, kind, id, needs)
        {
            left.push(format!("{what} {id} ({e})"));
        }
    }
    // The token hostname last: with the application gone the dashboard's
    // own hostname takes a token again, and a second one is no use.
    if settings.link_record_id.is_some() {
        let _ = configure(api, &without_access(account));
        left.extend(delete_link(api, account, settings));
    }
    left
}

fn without_access(account: &Account) -> Account {
    Account {
        access: None,
        ..account.clone()
    }
}

/// Turn public token links on or off: the token hostname made or deleted,
/// and the switch stored with it. Off, the ingress entry goes before the
/// record, so nothing answers on a name about to vanish; on, the record comes
/// first, so the entry never names a hostname with no record. Access set up
/// by hand, with no application cctop made, has no token hostname of cctop's
/// to touch: only the switch moves, and the server's lock follows it.
pub fn set_public_links(
    api: &Api,
    account: &Account,
    settings: &Settings,
    on: bool,
) -> Result<(Settings, Vec<String>), Error> {
    let mut next = Settings {
        public_links: on,
        ..settings.clone()
    };
    let made_here = settings.app_id.is_some();
    match on {
        true if made_here && settings.link_record_id.is_none() => {
            let (host, record) = add_link(api, account)?;
            next.link_hostname = Some(host);
            next.link_record_id = Some(record.clone());
            if let Err(e) = configure(api, &with_settings(account, &next)) {
                let _ = delete_link(api, account, &next);
                return Err(e);
            }
            Ok((next, Vec::new()))
        }
        false if settings.link_record_id.is_some() => {
            next.link_hostname = None;
            next.link_record_id = None;
            configure(api, &with_settings(account, &next))?;
            Ok((
                next,
                delete_link(api, account, settings).into_iter().collect(),
            ))
        }
        _ => Ok((next, Vec::new())),
    }
}

/// One edit to Access, the same from every face that makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Put the page behind Access for this owner, or change the owner.
    On {
        owner: String,
    },
    Off,
    /// Add `who`, or change its level in place.
    Invite {
        who: String,
        level: Level,
    },
    Uninvite {
        who: String,
    },
    PublicLinks(bool),
}

/// What an edit came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    /// The account to store.
    pub account: Account,
    /// What happened, as a sentence for whoever asked.
    pub said: String,
    /// What could not be deleted from Cloudflare, for the user to delete by
    /// hand.
    pub left: Vec<String>,
}

/// The sentence for an edit that needs Access on first.
pub const OFF: &str = "Cloudflare Access is off; turn it on with an owner email first.";

/// Make `change` on Cloudflare and return the account to store. Every refusal
/// comes before any write; nothing is saved here.
pub fn apply_with(api: &Api, account: &Account, change: Change) -> Result<Applied, Error> {
    let done = |settings: Option<Settings>, said: String, left: Vec<String>| Applied {
        account: Account {
            access: settings.map(Box::new),
            ..account.clone()
        },
        said,
        left,
    };
    let current = account.access.as_deref().cloned();
    match (change, current) {
        (Change::On { owner }, current) => {
            let invites = current.map(|c| c.invites).unwrap_or_default();
            let settings = enable(api, account, &owner, invites)?;
            let said = format!(
                "Access is on: {} logs in at https://{} with a code Cloudflare emails.",
                settings.owner,
                account.hostname.as_deref().unwrap_or_default()
            );
            Ok(done(Some(settings), said, Vec::new()))
        }
        (Change::Off, None) => Ok(done(
            None,
            "Cloudflare Access is already off.".into(),
            vec![],
        )),
        (Change::Off, Some(settings)) => {
            let left = disable(api, account, &settings);
            Ok(done(
                None,
                "Access is off: the dashboard is opened with its token link again.".into(),
                left,
            ))
        }
        (_, None) => Err(Error::Hostname(OFF.to_string())),
        (Change::Invite { who, level }, Some(mut settings)) => {
            let who = super::access::parse_who(&who).ok_or_else(|| {
                Error::Hostname(format!(
                    "{} is neither an email address nor a domain like @company.com",
                    who.trim()
                ))
            })?;
            if who == settings.owner {
                return Err(Error::Hostname(format!(
                    "{who} is the owner, who always has full access."
                )));
            }
            settings.invites = with_invite(settings.invites, who.clone(), level);
            update(api, account, &settings)?;
            let said = match level {
                Level::Full => format!("Invited {who}, with full access."),
                Level::Read => format!("Invited {who}, read-only."),
            };
            Ok(done(Some(settings), said, Vec::new()))
        }
        (Change::Uninvite { who }, Some(mut settings)) => {
            let who = super::access::parse_who(&who).unwrap_or_else(|| who.trim().to_string());
            let before = settings.invites.len();
            settings.invites.retain(|i| i.who != who);
            if settings.invites.len() == before {
                let said = format!("{who} was not invited; nothing changed.");
                return Ok(done(Some(settings), said, Vec::new()));
            }
            update(api, account, &settings)?;
            Ok(done(
                Some(settings),
                format!("{who} can no longer log in."),
                Vec::new(),
            ))
        }
        (Change::PublicLinks(on), Some(settings)) => {
            let (settings, left) = set_public_links(api, account, &settings, on)?;
            let said = match (on, &settings.link_hostname) {
                (true, Some(host)) => format!("Token links work again, on https://{host}."),
                (true, None) => "Token links work again.".to_string(),
                (false, _) => {
                    "Token links are off: from outside, only an Access login gets in.".to_string()
                }
            };
            Ok(done(Some(settings), said, left))
        }
    }
}

/// [`apply_with`] on the connected account, then the account saved and the
/// token hostname's route brought in line while this process holds the
/// tunnel. An error is a sentence for the user.
///
/// ponytail: two edits at once from two faces each read the account, write
/// Cloudflare and save; the second save wins. One person edits one list.
pub fn apply(change: Change) -> Result<Applied, String> {
    let (account, api) = super::connected()?;
    let applied = apply_with(&api, &account, change).map_err(|e| e.to_string())?;
    crate::tunnel::save_account(&applied.account).map_err(|e| {
        format!(
            "Cloudflare has the change, but cctop could not remember it ({e}); `cctop tunnel \
             remove` will not find what it made"
        )
    })?;
    crate::tunnel::sync_link();
    Ok(applied)
}

/// `invites` with `who` set to `level`: added, or changed in place so the
/// list keeps its order.
pub fn with_invite(mut invites: Vec<Invite>, who: String, level: Level) -> Vec<Invite> {
    match invites.iter_mut().find(|i| i.who == who) {
        Some(invite) => invite.level = level,
        None => invites.push(Invite { who, level }),
    }
    invites
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloudflare::fake::{Seen, accepting, api as fake_api, ok};

    fn account() -> Account {
        Account {
            token: "eyJhIjoi-made-up".into(),
            hostname: Some("cctop.example.test".into()),
            share_hostname: Some("cctop-share.example.test".into()),
            account_id: Some("acct1".into()),
            zone_id: Some("zone1".into()),
            tunnel_id: Some(crate::cloudflare::fake::TUNNEL_ID.into()),
            api_token: Some("made-up-api-token".into()),
            ..Account::default()
        }
    }

    fn calls(seen: &Seen) -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .map(|(m, p, _)| format!("{m} {p}"))
            .collect()
    }

    fn body_of(seen: &Seen, call: &str) -> Value {
        let seen = seen.lock().unwrap();
        let (_, _, body) = seen
            .iter()
            .find(|(m, p, _)| format!("{m} {p}") == call)
            .unwrap_or_else(|| panic!("no {call}"));
        serde_json::from_str(body).unwrap()
    }

    #[test]
    fn enabling_makes_a_pin_provider_a_policy_and_the_app_and_disabling_deletes_them() {
        let (base, seen) = fake_api(accepting(None));
        let api = Api::at(&base, "made-up-api-token");
        let invites = vec![Invite {
            who: "@company.test".into(),
            level: Level::Read,
        }];
        let settings = enable(&api, &account(), "Owner@Example.test", invites).unwrap();
        assert_eq!(settings.team, "made-up-team.cloudflareaccess.com");
        assert_eq!(settings.aud, "made-up-aud");
        assert_eq!(settings.owner, "owner@example.test");
        assert_eq!(settings.app_id.as_deref(), Some("app1"));
        assert_eq!(settings.policy_id.as_deref(), Some("pol1"));
        assert_eq!(settings.idp_id.as_deref(), Some("idp1"));
        assert_eq!(
            calls(&seen),
            [
                "GET /accounts/acct1/access/organizations",
                "GET /accounts/acct1/access/identity_providers",
                "POST /accounts/acct1/access/identity_providers",
                "POST /accounts/acct1/access/policies",
                "POST /accounts/acct1/access/apps",
                // The token hostname, beside the application and outside it.
                "GET /zones/zone1/dns_records?name=cctop-link.example.test",
                "POST /zones/zone1/dns_records",
                &format!("PUT {INGRESS}"),
            ]
        );
        assert!(settings.public_links);
        assert_eq!(
            settings.link_hostname.as_deref(),
            Some("cctop-link.example.test")
        );
        let record = settings.link_record_id.clone().expect("its record id");
        let cname = body_of(&seen, "POST /zones/zone1/dns_records");
        assert_eq!(cname["name"], "cctop-link.example.test");
        let ingress = body_of(&seen, &format!("PUT {INGRESS}"));
        assert!(
            ingress.to_string().contains("cctop-link.example.test"),
            "{ingress}"
        );
        let policy = body_of(&seen, "POST /accounts/acct1/access/policies");
        assert_eq!(policy["decision"], "allow");
        assert_eq!(
            policy["include"],
            json!([
                {"email": {"email": "owner@example.test"}},
                {"email_domain": {"domain": "company.test"}},
            ])
        );
        let app = body_of(&seen, "POST /accounts/acct1/access/apps");
        assert_eq!(app["domain"], "cctop.example.test");
        assert_eq!(app["type"], "self_hosted");
        assert_eq!(app["policies"], json!([{"id": "pol1", "precedence": 1}]));

        seen.lock().unwrap().clear();
        assert!(disable(&api, &account(), &settings).is_empty());
        assert_eq!(
            calls(&seen),
            [
                "DELETE /accounts/acct1/access/apps/app1",
                "DELETE /accounts/acct1/access/policies/pol1",
                "DELETE /accounts/acct1/access/identity_providers/idp1",
                &format!("PUT {INGRESS}"),
                &format!("DELETE /zones/zone1/dns_records/{record}"),
            ]
        );
        let ingress = body_of(&seen, &format!("PUT {INGRESS}"));
        assert!(!ingress.to_string().contains("cctop-link"), "{ingress}");
    }

    /// The tunnel's ingress list, as the fake API is asked to write it.
    const INGRESS: &str =
        "/accounts/acct1/cfd_tunnel/6ff42ae2-765d-4adf-8112-31c55c1551ef/configurations";

    #[test]
    fn public_links_off_takes_the_token_hostname_away_and_on_brings_it_back() {
        let (base, seen) = fake_api(accepting(None));
        let api = Api::at(&base, "made-up-api-token");
        let settings = enable(&api, &account(), "owner@example.test", Vec::new()).unwrap();
        let record = settings.link_record_id.clone().unwrap();
        let on = Account {
            access: Some(Box::new(settings)),
            ..account()
        };
        seen.lock().unwrap().clear();
        let off = apply_with(&api, &on, Change::PublicLinks(false)).unwrap();
        let stored = off.account.access.as_deref().unwrap();
        assert!(!stored.public_links);
        assert_eq!(stored.link_hostname, None);
        assert_eq!(stored.link_record_id, None);
        // The ingress entry first, then the record.
        assert_eq!(
            calls(&seen),
            [
                format!("PUT {INGRESS}"),
                format!("DELETE /zones/zone1/dns_records/{record}"),
            ]
        );
        assert!(
            !body_of(&seen, &format!("PUT {INGRESS}"))
                .to_string()
                .contains("cctop-link")
        );
        // The Access application is left alone: only the token's way in goes.
        assert_eq!(stored.app_id.as_deref(), Some("app1"));

        seen.lock().unwrap().clear();
        let back = apply_with(&api, &off.account, Change::PublicLinks(true)).unwrap();
        let stored = back.account.access.as_deref().unwrap();
        assert!(stored.public_links);
        assert_eq!(
            stored.link_hostname.as_deref(),
            Some("cctop-link.example.test")
        );
        assert!(calls(&seen).contains(&"POST /zones/zone1/dns_records".to_string()));
        assert!(back.said.contains("https://cctop-link.example.test"));
    }

    #[test]
    fn a_taken_token_hostname_undoes_the_whole_setup() {
        let accept = accepting(None);
        let (base, seen) =
            fake_api(
                move |method: &str, path: &str, body: &str| match (method, path) {
                    ("GET", "/zones/zone1/dns_records?name=cctop-link.example.test") => {
                        ok(json!([{"id": "theirs", "type": "A"}]))
                    }
                    _ => accept(method, path, body),
                },
            );
        let api = Api::at(&base, "made-up-api-token");
        let err = enable(&api, &account(), "owner@example.test", Vec::new()).unwrap_err();
        assert!(err.to_string().contains("cctop-link.example.test"), "{err}");
        let calls = calls(&seen);
        for undone in [
            "DELETE /accounts/acct1/access/apps/app1",
            "DELETE /accounts/acct1/access/policies/pol1",
            "DELETE /accounts/acct1/access/identity_providers/idp1",
        ] {
            assert!(calls.contains(&undone.to_string()), "{undone}: {calls:?}");
        }
        assert!(!calls.iter().any(|c| c.contains("theirs")), "{calls:?}");
    }

    #[test]
    fn an_edit_with_access_off_or_a_bad_address_writes_nothing() {
        let (base, seen) = fake_api(accepting(None));
        let api = Api::at(&base, "made-up-api-token");
        let err = apply_with(
            &api,
            &account(),
            Change::Invite {
                who: "a@b.test".into(),
                level: Level::Read,
            },
        )
        .unwrap_err();
        assert_eq!(err.to_string(), OFF);
        let on = Account {
            access: Some(Box::new(crate::cloudflare::access::fake::settings())),
            ..account()
        };
        for who in ["not an address", "owner@example.test"] {
            let change = Change::Invite {
                who: who.into(),
                level: Level::Full,
            };
            assert!(apply_with(&api, &on, change).is_err(), "{who}");
        }
        assert!(calls(&seen).is_empty(), "{:?}", calls(&seen));
    }

    #[test]
    fn removing_the_tunnel_takes_access_with_it_first() {
        let (base, seen) = fake_api(accepting(None));
        let settings = enable(
            &Api::at(&base, "made-up-api-token"),
            &account(),
            "owner@example.test",
            Vec::new(),
        )
        .unwrap();
        let connected = Account {
            access: Some(Box::new(settings)),
            dns_record_ids: vec!["rec1".into()],
            ..account()
        };
        seen.lock().unwrap().clear();
        let left = crate::cloudflare::remove_with(&connected, |token| Api::at(&base, token));
        assert_eq!(left.0, Vec::<String>::new());
        let calls = calls(&seen);
        assert_eq!(
            calls[0], "DELETE /accounts/acct1/access/apps/app1",
            "{calls:?}"
        );
        assert!(
            calls.contains(&"DELETE /zones/zone1/dns_records/rec1".to_string()),
            "{calls:?}"
        );
        assert!(calls.last().unwrap().contains("/cfd_tunnel/"), "{calls:?}");
    }

    #[test]
    fn an_existing_pin_provider_is_used_and_never_deleted() {
        let accept = accepting(None);
        let (base, seen) =
            fake_api(
                move |method: &str, path: &str, body: &str| match (method, path) {
                    ("GET", "/accounts/acct1/access/identity_providers") => ok(json!([
                        {"id": "theirs-github", "type": "github"},
                        {"id": "theirs-otp", "type": "onetimepin"},
                    ])),
                    _ => accept(method, path, body),
                },
            );
        let api = Api::at(&base, "made-up-api-token");
        let settings = enable(&api, &account(), "owner@example.test", Vec::new()).unwrap();
        assert_eq!(settings.idp_id, None);
        assert!(
            !calls(&seen)
                .iter()
                .any(|c| c.starts_with("POST") && c.contains("identity"))
        );
        seen.lock().unwrap().clear();
        disable(&api, &account(), &settings);
        assert!(!calls(&seen).iter().any(|c| c.contains("identity")));
    }

    #[test]
    fn a_failed_app_takes_the_policy_and_provider_it_made_with_it() {
        let (base, seen) = fake_api(accepting(Some("/accounts/acct1/access/apps")));
        let api = Api::at(&base, "made-up-api-token");
        assert!(enable(&api, &account(), "owner@example.test", Vec::new()).is_err());
        let calls = calls(&seen);
        assert!(
            calls.contains(&"DELETE /accounts/acct1/access/policies/pol1".to_string()),
            "{calls:?}"
        );
        assert!(
            calls.contains(&"DELETE /accounts/acct1/access/identity_providers/idp1".to_string()),
            "{calls:?}"
        );
    }

    #[test]
    fn an_account_without_zero_trust_is_told_where_to_click() {
        let accept = accepting(None);
        let (base, _) = fake_api(move |method: &str, path: &str, body: &str| match path {
            "/accounts/acct1/access/organizations" => (
                404,
                json!({"success": false, "errors": [{"code": 9999, "message": "made-up: no organization"}]}),
            ),
            _ => accept(method, path, body),
        });
        let api = Api::at(&base, "made-up-api-token");
        let err = enable(&api, &account(), "owner@example.test", Vec::new()).unwrap_err();
        assert_eq!(err, Error::NoTeam);
        assert!(err.to_string().contains("one.dash.cloudflare.com"));
    }

    #[test]
    fn a_token_without_the_access_permissions_names_them() {
        let accept = accepting(None);
        let (base, _) = fake_api(move |method: &str, path: &str, body: &str| match path {
            p if p.contains("/access/") => (
                403,
                json!({"success": false, "errors": [{"code": 10000, "message": "made-up: forbidden"}]}),
            ),
            _ => accept(method, path, body),
        });
        let api = Api::at(&base, "made-up-api-token");
        let err = enable(&api, &account(), "owner@example.test", Vec::new()).unwrap_err();
        assert_eq!(err, Error::MissingPermission(PERMISSIONS[1]));
        assert!(err.to_string().contains("Access: Organizations"), "{err}");
    }

    #[test]
    fn enabling_twice_updates_the_policy_in_place() {
        let (base, seen) = fake_api(accepting(None));
        let api = Api::at(&base, "made-up-api-token");
        let first = enable(&api, &account(), "owner@example.test", Vec::new()).unwrap();
        let connected = Account {
            access: Some(Box::new(first.clone())),
            ..account()
        };
        seen.lock().unwrap().clear();
        let invites = with_invite(Vec::new(), "boss@company.test".into(), Level::Full);
        let second = enable(&api, &connected, "new@example.test", invites).unwrap();
        assert_eq!(second.app_id, first.app_id);
        assert_eq!(second.owner, "new@example.test");
        assert_eq!(calls(&seen), ["PUT /accounts/acct1/access/policies/pol1"]);
        let policy = body_of(&seen, "PUT /accounts/acct1/access/policies/pol1");
        assert_eq!(
            policy["include"],
            json!([
                {"email": {"email": "new@example.test"}},
                {"email": {"email": "boss@company.test"}},
            ])
        );
    }

    #[test]
    fn a_tunnel_token_account_cannot_have_access_set_up_from_here() {
        let (base, seen) = fake_api(accepting(None));
        let api = Api::at(&base, "made-up-api-token");
        let tunnel_only = Account {
            token: "eyJhIjoi-made-up".into(),
            hostname: Some("cctop.example.test".into()),
            ..Account::default()
        };
        assert!(enable(&api, &tunnel_only, "owner@example.test", Vec::new()).is_err());
        assert!(calls(&seen).is_empty());
    }

    #[test]
    fn an_invite_changed_keeps_its_place() {
        let list = with_invite(Vec::new(), "@a.test".into(), Level::Read);
        let list = with_invite(list, "b@c.test".into(), Level::Read);
        let list = with_invite(list, "@a.test".into(), Level::Full);
        assert_eq!(
            list,
            [
                Invite {
                    who: "@a.test".into(),
                    level: Level::Full
                },
                Invite {
                    who: "b@c.test".into(),
                    level: Level::Read
                },
            ]
        );
    }
}
