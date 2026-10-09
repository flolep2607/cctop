//! Cloudflare Access in front of the dashboard: who may log in, and how the
//! server knows a request really comes from them.
//!
//! A token link is a bearer secret: whoever holds it is in, and it has to be
//! handed over to be shared. Access turns that around. The dashboard's
//! hostname sits behind an Access application, Cloudflare's edge asks the
//! visitor to log in — with a one-time PIN sent to their email, which needs no
//! Cloudflare account — and only then forwards the request, with a signed
//! statement of who logged in. cctop trusts that statement and nothing else
//! about the visitor's identity.
//!
//! # What is trusted, exactly
//!
//! The edge adds `Cf-Access-Jwt-Assertion`, a JWT signed RS256 by the team's
//! key. It also adds `Cf-Access-Authenticated-User-Email`, which is a
//! convenience and **never** read here: anything that reaches the loopback
//! port — another process on the machine, a misrouted request — can set any
//! header it likes, and only the signature is something it cannot forge.
//!
//! A JWT is accepted when, all at once ([`check`]):
//!
//! - its header says RS256 and names a key (`kid`) the team publishes at
//!   `https://<team>/cdn-cgi/access/certs` — so `alg: none` and an HMAC signed
//!   with the public key as its secret are both refused before any maths;
//! - the signature over `header.payload` verifies under that key;
//! - `aud` holds this application's AUD tag, so a login to some other Access
//!   application on the same team is not a login to cctop;
//! - `iss` is the team itself, and the token is inside `nbf`..`exp`;
//! - it carries an `email`, and cctop's own list ([`Settings::level_for`])
//!   allows it. The list is checked here as well as in the policy at the edge,
//!   so revoking an invite takes effect at the next request even before the
//!   edge's session cookie runs out.
//!
//! # The keys
//!
//! Access rotates its signing key every six weeks and keeps the old one valid
//! for a week, so the keys are fetched rather than stored, kept for an hour,
//! and fetched again early when a token names a key not seen yet — at most
//! once a minute, so a stream of junk `kid`s cannot turn into a stream of
//! requests to Cloudflare ([`Verifier`]).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde_json::Value;

/// How long fetched keys are used before they are fetched again.
const KEYS_TTL: Duration = Duration::from_secs(60 * 60);

/// The least time between two fetches, however many unknown keys turn up.
const MIN_REFETCH: Duration = Duration::from_secs(60);

/// Clock skew allowed on `exp` and `nbf`. The edge and this machine disagree
/// by a few seconds at most; a minute covers a laptop that slept.
const LEEWAY_SECS: i64 = 60;

/// The fetch of the team's keys: short, since a request waits on it.
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// What an Access login is allowed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Level {
    /// What a read-only token gets: every page, no actions.
    #[default]
    Read,
    /// What the full token gets.
    Full,
}

impl Level {
    /// The word stored in `config.toml` and shown to the user.
    pub fn word(self) -> &'static str {
        match self {
            Level::Read => "read",
            Level::Full => "full",
        }
    }

    pub fn from_word(word: &str) -> Option<Level> {
        match word.trim().to_ascii_lowercase().as_str() {
            "read" | "readonly" | "read-only" | "ro" => Some(Level::Read),
            "full" | "rw" => Some(Level::Full),
            _ => None,
        }
    }
}

/// One entry of the invite list: an email, or a whole domain written
/// `@company.com`, and what it may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    /// Lowercase; a domain starts with `@`.
    pub who: String,
    pub level: Level,
}

impl Invite {
    pub fn is_domain(&self) -> bool {
        self.who.starts_with('@')
    }
}

/// An invite's `who` as typed, in the stored form: `a@b.com` stays an email,
/// and `@b.com` or a bare `b.com` is the whole domain. `None` for anything that
/// is neither, so a typo is refused rather than stored as a rule that matches
/// nobody.
pub fn parse_who(input: &str) -> Option<String> {
    let input = input.trim().to_ascii_lowercase();
    let domain_ok = |d: &str| {
        d.contains('.')
            && !d.starts_with('.')
            && !d.ends_with('.')
            && !d.contains("..")
            && d.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    };
    match input.split_once('@') {
        Some(("", domain)) if domain_ok(domain) => Some(input),
        Some((local, domain))
            if !local.is_empty() && !local.contains(char::is_whitespace) && domain_ok(domain) =>
        {
            Some(input)
        }
        None if domain_ok(&input) => Some(format!("@{input}")),
        _ => None,
    }
}

/// What `cctop tunnel access` set up: the `[tunnel.access]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The team domain, `<team>.cloudflareaccess.com`: where the keys are,
    /// and the issuer every token must name.
    pub team: String,
    /// The application's AUD tag.
    pub aud: String,
    /// The owner's email: always full, whatever the list says.
    pub owner: String,
    pub invites: Vec<Invite>,
    /// What cctop created, to update and delete by: never anything else.
    pub app_id: Option<String>,
    pub policy_id: Option<String>,
    /// The one-time PIN provider, only when cctop created it.
    pub idp_id: Option<String>,
    /// Whether a token link still opens the page through the tunnel. With
    /// Access on, the dashboard's hostname asks for a login before anything
    /// reaches cctop, so a token is only any use to someone who cannot log
    /// in on a hostname of its own — [`Settings::link_hostname`]. Off, the
    /// page's server refuses every token that comes through the tunnel, on
    /// any hostname, and only a login gets in from outside; loopback keeps
    /// its token. On unless turned off, which is how Access behaved before
    /// there was a switch.
    pub public_links: bool,
    /// The hostname token links go on while Access is on: beside the
    /// dashboard's and outside the Access application, so it asks for no
    /// login. Only while [`Settings::public_links`] is on, and only when
    /// cctop made its DNS record.
    pub link_hostname: Option<String>,
    /// That record, to delete it by.
    pub link_record_id: Option<String>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            team: String::new(),
            aud: String::new(),
            owner: String::new(),
            invites: Vec::new(),
            app_id: None,
            policy_id: None,
            idp_id: None,
            public_links: true,
            link_hostname: None,
            link_record_id: None,
        }
    }
}

impl Settings {
    /// What `email` may do, or `None` when it is not on the list. An exact
    /// email wins over its domain, so `boss@company.com: full` beside
    /// `@company.com: read` reads the way it is written.
    pub fn level_for(&self, email: &str) -> Option<Level> {
        let email = email.trim().to_ascii_lowercase();
        let (_, domain) = email.split_once('@')?;
        if !self.owner.is_empty() && email == self.owner.to_ascii_lowercase() {
            return Some(Level::Full);
        }
        if let Some(invite) = self.invites.iter().find(|i| i.who == email) {
            return Some(invite.level);
        }
        let at_domain = format!("@{domain}");
        self.invites
            .iter()
            .find(|i| i.who == at_domain)
            .map(|i| i.level)
    }

    /// The `iss` every token must carry.
    pub fn issuer(&self) -> String {
        format!("https://{}", self.team)
    }

    /// Where the team publishes its signing keys.
    pub fn certs_url(&self) -> String {
        format!("https://{}/cdn-cgi/access/certs", self.team)
    }

    /// Whether `email` is the owner: the one login that may change who else
    /// can log in, as the full token may.
    pub fn is_owner(&self, email: &str) -> bool {
        !self.owner.is_empty() && email.trim().eq_ignore_ascii_case(&self.owner)
    }

    /// Whether `host` is the token links' hostname.
    pub fn is_link_host(&self, host: &str) -> bool {
        let host = host.split(':').next().unwrap_or_default();
        self.link_hostname.as_deref().is_some_and(|link| {
            link.trim_end_matches('.')
                .eq_ignore_ascii_case(host.trim_end_matches('.'))
        })
    }

    /// The origin a token link is printed on while Access is on: the token
    /// hostname when there is one, the dashboard's `page` origin when Access
    /// was set up by hand and has none, and `None` when public links are
    /// off — a token that comes through the tunnel is refused then, so there
    /// is no link to hand out.
    pub fn token_origin(&self, page: &str) -> Option<String> {
        if !self.public_links {
            return None;
        }
        Some(match &self.link_hostname {
            Some(link) => format!("https://{link}"),
            None => page.to_string(),
        })
    }

    /// Whether there is enough to check a login against.
    pub fn usable(&self) -> bool {
        !self.team.is_empty() && !self.aud.is_empty() && !self.owner.is_empty()
    }
}

/// Why a JWT was not accepted. Never carries the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    /// Not three base64url parts of JSON.
    Malformed,
    /// Anything but RS256, or no `kid`.
    Algorithm,
    /// A `kid` the team does not publish.
    UnknownKey,
    Signature,
    Audience,
    Issuer,
    Expired,
    NotYet,
    /// No `email` claim: a service token, which cctop does not take.
    NoEmail,
    /// The keys could not be had, so nothing can be checked.
    Keys(String),
}

/// The team's published public keys, by `kid`: RSA modulus and exponent.
#[derive(Debug, Clone, Default)]
pub struct Keys(HashMap<String, (Vec<u8>, Vec<u8>)>);

impl Keys {
    /// Read the certs endpoint's JSON. Keys that are not RSA signing keys are
    /// skipped rather than refused, so a key type added later does not stop
    /// the RSA ones from working.
    pub fn parse(jwks: &str) -> Result<Keys, String> {
        let doc: Value =
            serde_json::from_str(jwks).map_err(|_| "the keys were not JSON".to_string())?;
        let keys = doc["keys"]
            .as_array()
            .ok_or_else(|| "the answer had no keys".to_string())?;
        let mut out = HashMap::new();
        for key in keys {
            if key["kty"].as_str() != Some("RSA") {
                continue;
            }
            let (Some(kid), Some(n), Some(e)) =
                (key["kid"].as_str(), key["n"].as_str(), key["e"].as_str())
            else {
                continue;
            };
            let (Ok(n), Ok(e)) = (b64(n), b64(e)) else {
                continue;
            };
            out.insert(kid.to_string(), (n, e));
        }
        Ok(Keys(out))
    }

    fn get(&self, kid: &str) -> Option<&(Vec<u8>, Vec<u8>)> {
        self.0.get(kid)
    }
}

/// base64url, padded or not: JWTs are unpadded, and a JWK's fields may be
/// either.
fn b64(text: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    const LENIENT: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::URL_SAFE,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    LENIENT.decode(text)
}

/// The seconds since the epoch, as the claims count them.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The header's `kid`, once the header has said RS256 — the only algorithm
/// Access signs with, and the only one accepted, whatever the token claims.
fn kid_of(jwt: &str) -> Result<String, Rejected> {
    let header = jwt.split('.').next().ok_or(Rejected::Malformed)?;
    let header: Value = serde_json::from_slice(&b64(header).map_err(|_| Rejected::Malformed)?)
        .map_err(|_| Rejected::Malformed)?;
    if header["alg"].as_str() != Some("RS256") {
        return Err(Rejected::Algorithm);
    }
    header["kid"]
        .as_str()
        .map(String::from)
        .ok_or(Rejected::Algorithm)
}

/// Check `jwt` against `keys` for the application `aud` of the team that
/// issues as `issuer`, at `now`, and return the email it was issued to.
///
/// The signature is checked before a single claim is believed.
pub fn check(
    keys: &Keys,
    jwt: &str,
    aud: &str,
    issuer: &str,
    now: i64,
) -> Result<String, Rejected> {
    let mut parts = jwt.trim().split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Rejected::Malformed);
    };
    let kid = kid_of(jwt.trim())?;
    let (n, e) = keys.get(&kid).ok_or(Rejected::UnknownKey)?;
    let signature = b64(signature).map_err(|_| Rejected::Malformed)?;
    let signed = format!("{header}.{payload}");
    ring::signature::RsaPublicKeyComponents { n, e }
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            signed.as_bytes(),
            &signature,
        )
        .map_err(|_| Rejected::Signature)?;

    let claims: Value = serde_json::from_slice(&b64(payload).map_err(|_| Rejected::Malformed)?)
        .map_err(|_| Rejected::Malformed)?;
    // `aud` is an array in Access's tokens, and a string is allowed by the
    // standard; either way this application's tag has to be in it.
    let audience = match &claims["aud"] {
        Value::String(one) => one == aud,
        Value::Array(many) => many.iter().any(|a| a.as_str() == Some(aud)),
        _ => false,
    };
    if aud.is_empty() || !audience {
        return Err(Rejected::Audience);
    }
    if claims["iss"].as_str() != Some(issuer) {
        return Err(Rejected::Issuer);
    }
    // A token with no expiry is not one Access issues, and would be good
    // forever: refused like an expired one.
    match claims["exp"].as_i64() {
        Some(exp) if now <= exp + LEEWAY_SECS => {}
        _ => return Err(Rejected::Expired),
    }
    if let Some(nbf) = claims["nbf"].as_i64()
        && now + LEEWAY_SECS < nbf
    {
        return Err(Rejected::NotYet);
    }
    claims["email"]
        .as_str()
        .filter(|e| e.contains('@'))
        .map(str::to_ascii_lowercase)
        .ok_or(Rejected::NoEmail)
}

type Fetch = Box<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

struct Cache {
    keys: Keys,
    fetched: Option<Instant>,
}

/// [`check`] with the team's keys fetched, kept, and refreshed — see the
/// module docs for when.
pub struct Verifier {
    certs_url: String,
    fetch: Fetch,
    cache: Mutex<Cache>,
}

impl Verifier {
    /// For the team at `certs_url`, fetched over HTTPS.
    pub fn new(certs_url: &str) -> Verifier {
        Verifier::with_fetch(certs_url, Box::new(fetch_https))
    }

    /// With the fetch given: a test's keys, never the network.
    pub fn with_fetch(certs_url: &str, fetch: Fetch) -> Verifier {
        Verifier {
            certs_url: certs_url.to_string(),
            fetch,
            cache: Mutex::new(Cache {
                keys: Keys::default(),
                fetched: None,
            }),
        }
    }

    /// The email `jwt` was issued to, when it is a valid login to `aud`.
    ///
    /// The lock is held across a fetch on purpose: a burst of requests after
    /// the keys expire makes one request to Cloudflare, not one each.
    pub fn verify(&self, jwt: &str, aud: &str, issuer: &str) -> Result<String, Rejected> {
        let kid = kid_of(jwt.trim())?;
        let mut cache = self.cache.lock().unwrap_or_else(|p| p.into_inner());
        let stale = cache.fetched.is_none_or(|at| at.elapsed() >= KEYS_TTL);
        let unknown = cache.keys.get(&kid).is_none()
            && cache.fetched.is_none_or(|at| at.elapsed() >= MIN_REFETCH);
        if stale || unknown {
            match (self.fetch)(&self.certs_url).and_then(|text| Keys::parse(&text)) {
                Ok(keys) => {
                    cache.keys = keys;
                    cache.fetched = Some(Instant::now());
                }
                // Keys already held stay in use: a blip at Cloudflare should
                // not log everyone out. With none held, nothing can be
                // checked, and the answer says why.
                Err(why) if cache.fetched.is_none() => return Err(Rejected::Keys(why)),
                Err(_) => cache.fetched = Some(Instant::now()),
            }
        }
        check(&cache.keys, jwt, aud, issuer, now())
    }
}

fn fetch_https(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(FETCH_TIMEOUT))
        .build()
        .into();
    agent
        .get(url)
        .call()
        .map_err(|e| format!("could not fetch the Access keys ({e})"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("could not read the Access keys ({e})"))
}

/// A signing key of the tests' own, and tokens made with it, so every check
/// above runs against real RS256 signatures and never a real Access team.
///
/// The two keys were generated for this purpose (`openssl genpkey`, then
/// `openssl rsa -traditional -outform DER`, the PKCS#1 form ring reads) and
/// sign nothing anywhere else. `B` signs forgeries: a token naming `A`'s `kid`
/// with a signature `A` never made.
#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    use base64::Engine;
    use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
    use serde_json::{Value, json};

    pub const TEAM: &str = "made-up-team.cloudflareaccess.com";
    pub const AUD: &str = "made-up-aud-tag";
    pub const KID: &str = "made-up-kid";

    const KEY_A: &[u8] = include_bytes!("testdata/access-test-key-a.der");
    const KEY_B: &[u8] = include_bytes!("testdata/access-test-key-b.der");

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    fn pair(der: &[u8]) -> RsaKeyPair {
        RsaKeyPair::from_der(der).expect("the test key parses")
    }

    /// The certs endpoint's answer, publishing key `A` as [`KID`].
    pub fn jwks() -> String {
        let key = pair(KEY_A);
        let public = ring::rsa::PublicKeyComponents::<Vec<u8>>::from(key.public());
        json!({"keys": [{
            "kid": KID,
            "kty": "RSA",
            "alg": "RS256",
            "use": "sig",
            "n": b64(&public.n),
            "e": b64(&public.e),
        }]})
        .to_string()
    }

    /// The claims Access would issue to `email` for [`AUD`] on [`TEAM`],
    /// good for ten minutes from now.
    pub fn claims(email: &str) -> Value {
        let now = super::now();
        json!({
            "aud": [AUD],
            "email": email,
            "exp": now + 600,
            "iat": now,
            "nbf": now,
            "iss": format!("https://{TEAM}"),
            "type": "app",
            "sub": "made-up-subject",
        })
    }

    fn sign_with(der: &[u8], header: &Value, claims: &Value) -> String {
        let signed = format!(
            "{}.{}",
            b64(header.to_string().as_bytes()),
            b64(claims.to_string().as_bytes())
        );
        let key = pair(der);
        let mut signature = vec![0; key.public().modulus_len()];
        key.sign(
            &RSA_PKCS1_SHA256,
            &ring::rand::SystemRandom::new(),
            signed.as_bytes(),
            &mut signature,
        )
        .expect("signing works");
        format!("{signed}.{}", b64(&signature))
    }

    fn header() -> Value {
        json!({"alg": "RS256", "kid": KID, "typ": "JWT"})
    }

    /// A genuine token for `claims`, signed by the published key.
    pub fn token(claims: &Value) -> String {
        sign_with(KEY_A, &header(), claims)
    }

    /// A token naming the published key but signed by another one.
    pub fn forged(claims: &Value) -> String {
        sign_with(KEY_B, &header(), claims)
    }

    /// A token that says it needs no signature at all.
    pub fn unsigned(claims: &Value) -> String {
        format!(
            "{}.{}.",
            b64(json!({"alg": "none", "kid": KID}).to_string().as_bytes()),
            b64(claims.to_string().as_bytes())
        )
    }

    /// The settings a test's Access is set up with: `owner@example.test` and
    /// `@example.test` read-only.
    pub fn settings() -> super::Settings {
        super::Settings {
            team: TEAM.to_string(),
            aud: AUD.to_string(),
            owner: "owner@example.test".to_string(),
            invites: vec![super::Invite {
                who: "@example.test".to_string(),
                level: super::Level::Read,
            }],
            ..Default::default()
        }
    }

    /// A verifier that answers from [`jwks`] and never the network.
    pub fn verifier() -> super::Verifier {
        super::Verifier::with_fetch(&settings().certs_url(), Box::new(|_: &str| Ok(jwks())))
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{self, AUD, TEAM};
    use super::*;
    use serde_json::json;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn issuer() -> String {
        format!("https://{TEAM}")
    }

    fn keys() -> Keys {
        Keys::parse(&fake::jwks()).unwrap()
    }

    fn checked(jwt: &str) -> Result<String, Rejected> {
        check(&keys(), jwt, AUD, &issuer(), now())
    }

    #[test]
    fn a_genuine_login_names_its_email() {
        let jwt = fake::token(&fake::claims("Owner@Example.test"));
        assert_eq!(checked(&jwt), Ok("owner@example.test".to_string()));
    }

    #[test]
    fn a_signature_from_another_key_is_refused() {
        let jwt = fake::forged(&fake::claims("owner@example.test"));
        assert_eq!(checked(&jwt), Err(Rejected::Signature));
    }

    #[test]
    fn a_token_that_asks_for_no_signature_is_refused_before_any_check() {
        let jwt = fake::unsigned(&fake::claims("owner@example.test"));
        assert_eq!(checked(&jwt), Err(Rejected::Algorithm));
    }

    #[test]
    fn an_hmac_token_is_refused_whatever_its_secret() {
        // The classic confusion: HS256 "signed" with the public key as the
        // secret. The algorithm is refused before anything is computed.
        let claims = fake::claims("owner@example.test");
        let jwt = fake::token(&claims);
        let (_, rest) = jwt.split_once('.').unwrap();
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(json!({"alg": "HS256", "kid": fake::KID}).to_string());
        assert_eq!(
            checked(&format!("{header}.{rest}")),
            Err(Rejected::Algorithm)
        );
    }

    #[test]
    fn a_claim_changed_after_signing_breaks_the_signature() {
        let jwt = fake::token(&fake::claims("someone@elsewhere.test"));
        let mut parts: Vec<&str> = jwt.split('.').collect();
        let swapped = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(fake::claims("owner@example.test").to_string());
        parts[1] = &swapped;
        assert_eq!(checked(&parts.join(".")), Err(Rejected::Signature));
    }

    #[test]
    fn each_claim_is_checked() {
        let base = fake::claims("owner@example.test");
        let with = |key: &str, value: Value| {
            let mut claims = base.clone();
            claims[key] = value;
            checked(&fake::token(&claims))
        };
        assert_eq!(with("aud", json!(["another-app"])), Err(Rejected::Audience));
        assert_eq!(with("aud", json!(AUD)), checked(&fake::token(&base)));
        assert_eq!(
            with("iss", json!("https://other.cloudflareaccess.com")),
            Err(Rejected::Issuer)
        );
        assert_eq!(with("exp", json!(now() - 3600)), Err(Rejected::Expired));
        assert_eq!(with("exp", Value::Null), Err(Rejected::Expired));
        assert_eq!(with("nbf", json!(now() + 3600)), Err(Rejected::NotYet));
        assert_eq!(with("email", Value::Null), Err(Rejected::NoEmail));
    }

    #[test]
    fn junk_is_malformed_not_a_panic() {
        for junk in ["", "a", "a.b", "a.b.c.d", "!!!.???.***", "e30.e30.e30"] {
            assert!(checked(junk).is_err(), "{junk}");
        }
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let jwt = fake::token(&fake::claims("owner@example.test"));
        assert_eq!(
            check(&Keys::default(), &jwt, AUD, &issuer(), now()),
            Err(Rejected::UnknownKey)
        );
    }

    #[test]
    fn the_list_says_who_gets_what() {
        let mut settings = fake::settings();
        settings.invites.push(Invite {
            who: "boss@example.test".into(),
            level: Level::Full,
        });
        settings.invites.push(Invite {
            who: "guest@elsewhere.test".into(),
            level: Level::Read,
        });
        assert_eq!(settings.level_for("OWNER@example.test"), Some(Level::Full));
        assert_eq!(settings.level_for("boss@example.test"), Some(Level::Full));
        assert_eq!(settings.level_for("anyone@example.test"), Some(Level::Read));
        assert_eq!(
            settings.level_for("guest@elsewhere.test"),
            Some(Level::Read)
        );
        assert_eq!(settings.level_for("other@elsewhere.test"), None);
        // A domain is matched whole, not as a suffix.
        assert_eq!(settings.level_for("x@notexample.test"), None);
        assert_eq!(settings.level_for("x@sub.example.test"), None);
        assert_eq!(settings.level_for("not-an-email"), None);
    }

    #[test]
    fn invites_are_read_as_an_email_or_a_domain() {
        assert_eq!(parse_who("A@B.com"), Some("a@b.com".into()));
        assert_eq!(parse_who("@company.com"), Some("@company.com".into()));
        assert_eq!(parse_who("company.com"), Some("@company.com".into()));
        for bad in [
            "",
            "@",
            "a@",
            "a@b",
            "nodot",
            "a b@c.com",
            "@.com",
            "a@b..com",
        ] {
            assert_eq!(parse_who(bad), None, "{bad}");
        }
    }

    #[test]
    fn keys_are_fetched_once_and_again_only_for_a_new_kid_at_most_once_a_minute() {
        let fetches = Arc::new(AtomicUsize::new(0));
        let counted = fetches.clone();
        let verifier = Verifier::with_fetch(
            "https://made-up/cdn-cgi/access/certs",
            Box::new(move |_: &str| {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(fake::jwks())
            }),
        );
        let jwt = fake::token(&fake::claims("owner@example.test"));
        for _ in 0..3 {
            assert!(verifier.verify(&jwt, AUD, &issuer()).is_ok());
        }
        assert_eq!(fetches.load(Ordering::SeqCst), 1);

        // A kid nobody publishes, twice: the first was already covered by the
        // fetch a moment ago, so neither asks again.
        let mut header = json!({"alg": "RS256", "kid": "junk"}).to_string();
        header = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(header);
        let (_, rest) = jwt.split_once('.').unwrap();
        let junk = format!("{header}.{rest}");
        for _ in 0..2 {
            assert_eq!(
                verifier.verify(&junk, AUD, &issuer()),
                Err(Rejected::UnknownKey)
            );
        }
        assert_eq!(fetches.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn with_no_keys_to_be_had_nothing_is_accepted() {
        let verifier = Verifier::with_fetch(
            "https://made-up/cdn-cgi/access/certs",
            Box::new(|_: &str| Err("made-up outage".to_string())),
        );
        let jwt = fake::token(&fake::claims("owner@example.test"));
        assert!(matches!(
            verifier.verify(&jwt, AUD, &issuer()),
            Err(Rejected::Keys(_))
        ));
    }
}
