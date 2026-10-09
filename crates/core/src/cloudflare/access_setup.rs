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
//! As with the tunnel, only what cctop created is deleted, by the ids it
//! stored, and a setup that fails halfway deletes what it made before
//! returning.

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
    let built = (|| {
        let policy = api.create_policy(account_id, policy_body(host, &owner, &invites))?;
        made_policy = Some(policy.clone());
        let (app, aud) = api.create_app(account_id, host, &policy)?;
        Ok((policy, app, aud))
    })();
    let (policy, app, aud) = match built {
        Ok(made) => made,
        Err(e) => {
            // Best effort, newest first; the error worth reporting is the
            // one that stopped the setup.
            if let Some(policy) = made_policy {
                let _ = api.delete_access(account_id, "policies", &policy, PERMISSIONS[0]);
            }
            if made_idp {
                let _ = api.delete_access(account_id, "identity_providers", &idp, PERMISSIONS[1]);
            }
            return Err(e);
        }
    };
    Ok(Settings {
        team,
        aud,
        owner,
        invites,
        app_id: Some(app),
        policy_id: Some(policy),
        idp_id: made_idp.then_some(idp),
    })
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
    left
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
            ]
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
            ]
        );
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
