//! A Cloudflare Access login as a way in, beside the tokens.
//!
//! The checking is [`cctop_core::cloudflare::access`]'s; this is where the
//! server asks it. A request that carries `Cf-Access-Jwt-Assertion` is
//! checked against the `[tunnel.access]` settings, and a login on the list is
//! worth what a token of its level is worth — no more: a read-only login gets
//! exactly the read-only link's page.
//!
//! Only that one header is read. `Cf-Access-Authenticated-User-Email` and
//! the rest of the `Cf-Access-*` family come with every edge request too,
//! and are ignored here, because anything that reaches the loopback port can
//! write them — see `a_forged_access_header_opens_nothing` in the tests.
//!
//! The settings are read again when `config.toml` changes, so an invite added
//! or removed by `cctop tunnel invite` applies to a serve already running, at
//! its next request.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use cctop_core::cloudflare::access::{Level, Rejected, Settings, Verifier};

/// The request header the edge signs its statement into.
pub const HEADER: &str = "cf-access-jwt-assertion";

/// Who a JWT says logged in, and what they may do here.
pub trait Identities: Send + Sync {
    /// The login `jwt` proves and the level the list gives it, or why not.
    fn check(&self, jwt: &str) -> Result<(String, Level), Why>;
}

/// Why a JWT bought nothing, for the event log. Carries no token.
#[derive(Debug, PartialEq, Eq)]
pub enum Why {
    /// No Access is set up, so no JWT means anything.
    NotSetUp,
    Rejected(Rejected),
    /// A genuine login, by someone not on the list.
    NotInvited,
}

/// No Access: every JWT is ignored. What a test gets unless it asks.
#[cfg(test)]
pub struct Nowhere;

#[cfg(test)]
impl Identities for Nowhere {
    fn check(&self, _: &str) -> Result<(String, Level), Why> {
        Err(Why::NotSetUp)
    }
}

/// The settings with the verifier for their team.
struct Loaded {
    settings: Settings,
    verifier: Arc<Verifier>,
}

impl Loaded {
    fn check(&self, jwt: &str) -> Result<(String, Level), Why> {
        let email = self
            .verifier
            .verify(jwt, &self.settings.aud, &self.settings.issuer())
            .map_err(Why::Rejected)?;
        let level = self.settings.level_for(&email).ok_or(Why::NotInvited)?;
        Ok((email, level))
    }
}

/// When `config.toml` was last read: its modification time and length,
/// which together change on any rewrite cctop makes (a rename into place).
type Stamp = Option<(SystemTime, u64)>;

/// The real thing: the settings `config.toml` holds, read again whenever the
/// file changes.
pub struct Configured {
    path: PathBuf,
    state: Mutex<(Stamp, Option<Arc<Loaded>>)>,
    /// How a team's keys are fetched: over HTTPS, except in a test.
    verifier: fn(&str) -> Verifier,
}

impl Configured {
    pub fn new() -> Configured {
        Configured::at(&cctop_core::config::CONFIG_FILE, Verifier::new)
    }

    fn at(path: &Path, verifier: fn(&str) -> Verifier) -> Configured {
        Configured {
            path: path.to_path_buf(),
            state: Mutex::new((None, None)),
            verifier,
        }
    }

    /// The settings as the file says now. Only a stat when nothing moved.
    fn current(&self) -> Option<Arc<Loaded>> {
        let stamp = std::fs::metadata(&self.path)
            .ok()
            .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.0 != stamp || stamp.is_none() {
            let settings = std::fs::read_to_string(&self.path)
                .ok()
                .and_then(|text| cctop_core::tunnel::access_settings_in(&text));
            state.1 = settings.map(|settings| {
                // The keys already fetched stay with the team: an edit to the
                // list is not a reason to ask Cloudflare for them again.
                let verifier = match &state.1 {
                    Some(old) if old.settings.team == settings.team => old.verifier.clone(),
                    _ => Arc::new((self.verifier)(&settings.certs_url())),
                };
                Arc::new(Loaded { settings, verifier })
            });
            state.0 = stamp;
        }
        state.1.clone()
    }
}

impl Identities for Configured {
    fn check(&self, jwt: &str) -> Result<(String, Level), Why> {
        // The lock is not held while a JWT is checked: a key fetch must not
        // stall every other request's read of the settings.
        self.current().ok_or(Why::NotSetUp)?.check(jwt)
    }
}

/// Fixed settings and a verifier that never touches the network: what the
/// server's tests log in against.
#[cfg(test)]
pub struct Fixed(Loaded);

#[cfg(test)]
impl Fixed {
    pub fn new(settings: Settings) -> Fixed {
        Fixed(Loaded {
            settings,
            verifier: Arc::new(cctop_core::cloudflare::access::fake::verifier()),
        })
    }
}

#[cfg(test)]
impl Identities for Fixed {
    fn check(&self, jwt: &str) -> Result<(String, Level), Why> {
        self.0.check(jwt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cctop_core::cloudflare::access::fake;

    fn offline(_: &str) -> Verifier {
        fake::verifier()
    }

    #[test]
    fn the_list_is_read_again_when_the_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let identities = Configured::at(&path, offline);
        let guest = fake::token(&fake::claims("guest@elsewhere.test"));
        assert_eq!(identities.check(&guest), Err(Why::NotSetUp));

        let write = |invites: &str| {
            let text = format!(
                "[tunnel]\ntoken = \"eyJhIjoi-made-up\"\n\n[tunnel.access]\nteam = \"{}\"\n\
                 aud = \"{}\"\nowner = \"owner@example.test\"\ninvites = [{invites}]\n",
                fake::TEAM,
                fake::AUD
            );
            std::fs::write(&path, text).unwrap();
        };
        write("");
        assert_eq!(identities.check(&guest), Err(Why::NotInvited));
        let owner = fake::token(&fake::claims("owner@example.test"));
        assert_eq!(
            identities.check(&owner),
            Ok(("owner@example.test".to_string(), Level::Full))
        );

        // A longer file, so the stamp moves even inside one mtime tick.
        write("{ who = \"@elsewhere.test\", level = \"read\" }");
        assert_eq!(
            identities.check(&guest),
            Ok(("guest@elsewhere.test".to_string(), Level::Read))
        );
    }
}
