//! Web Push: a phone told that a session needs you, with the page closed.
//!
//! The page's own notifications only fire while a tab is open and awake, and
//! a phone puts a background tab to sleep within minutes. A push subscription
//! outlives the tab: the browser hands the page an endpoint on its vendor's
//! push service (Google's for Chrome and Android, Apple's for Safari,
//! Mozilla's for Firefox), and whatever POSTs there wakes the page's service
//! worker, which shows the notification itself.
//!
//! Two standards decide what that POST is, and both are done here with `ring`,
//! which is already in the tree under rustls — a Web Push crate would bring a
//! second set of the same primitives for the sake of about two hundred lines:
//!
//! - **RFC 8291**: the body is encrypted to the browser's own key, so the push
//!   service carries it without being able to read it. One record of
//!   `aes128gcm` (RFC 8188), keyed by an ECDH with a key made for this one
//!   message and the subscription's auth secret.
//! - **RFC 8292 (VAPID)**: the request is signed with a key this machine keeps,
//!   so the push service knows the sender is the one the browser subscribed
//!   to, and nobody who learns an endpoint can push to it.
//!
//! The VAPID key and the subscriptions are kept sealed in `secrets.enc` — see
//! [`crate::secrets`]. A subscription is a credential of sorts: whoever holds
//! one can put a notification on that phone.
//!
//! What leaves the machine is what [`Payload`] holds and nothing else: a
//! title, the bell's one line, and the session id the tap opens. Encrypted, to
//! the push service of whichever browser subscribed.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::KeyPair;
use ring::{aead, agreement, hkdf, signature};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The VAPID private key, PKCS#8, base64url.
pub const VAPID_KEY: &str = "webpush.vapid_key";

/// Every subscription, as a JSON array.
pub const SUBSCRIPTIONS: &str = "webpush.subscriptions";

/// Who to tell when this sender misbehaves, as RFC 8292 asks for. A push
/// service writes to it, not a user; Apple's refuses a push without one.
const SUBJECT: &str = "https://github.com/flolep2607/cctop";

/// How long a push service keeps a message for a phone that is off. An hour:
/// "waiting on you" from yesterday is not news, and the table says it anyway.
const TTL_SECS: u32 = 3600;

/// How long one POST to a push service is given.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);

/// The record size the header declares. The payload is one record of a few
/// hundred bytes, so this is only the number RFC 8188 wants written down.
const RECORD_SIZE: u32 = 4096;

/// The most of a message a payload carries. One record holds 4096 bytes less
/// the delimiter and the tag; a label longer than this is cut, not refused.
const MAX_PLAINTEXT: usize = 3000;

/// A browser's subscription, in the shape `PushSubscription.toJSON()` gives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub endpoint: String,
    pub keys: Keys,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Keys {
    /// The browser's P-256 public key, uncompressed, base64url.
    pub p256dh: String,
    /// The 16-byte auth secret, base64url.
    pub auth: String,
}

impl Subscription {
    /// Whether this is a subscription a push can be encrypted to and sent to.
    ///
    /// Checked when the page hands one over rather than when a push fails,
    /// so a bad one is refused with a reason instead of failing silently on
    /// every crossing. `https` only: every push service is, and a request
    /// this machine makes on someone's say-so should not reach plain http.
    pub fn check(&self) -> Result<(), &'static str> {
        if !self.endpoint.starts_with("https://") || self.endpoint.len() > 2048 {
            return Err("a push endpoint is an https URL");
        }
        match decode(&self.keys.p256dh) {
            Some(key) if key.len() == 65 && key[0] == 4 => {}
            _ => return Err("p256dh is not an uncompressed P-256 key"),
        }
        match decode(&self.keys.auth) {
            Some(auth) if auth.len() == 16 => Ok(()),
            _ => Err("auth is not a 16-byte secret"),
        }
    }
}

/// What a notification says, and all a push carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    /// "Waiting on you" or "Needs permission".
    pub title: String,
    /// The row's label, as the bell names it.
    pub body: String,
    /// The session the tap opens, at `/session/<id>`.
    pub session: String,
}

fn decode(text: &str) -> Option<Vec<u8>> {
    // Browsers give base64url without padding; some libraries pad it.
    URL_SAFE_NO_PAD.decode(text.trim_end_matches('=')).ok()
}

fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The subscriptions kept in `store`, or none when there are none or the
/// store does not open.
pub fn load_subscriptions(store: &crate::secrets::Store) -> Vec<Subscription> {
    store
        .get(SUBSCRIPTIONS)
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

/// Replace the subscriptions kept in `store` with `subscriptions`.
pub fn save_subscriptions(
    store: &crate::secrets::Store,
    subscriptions: &[Subscription],
) -> anyhow::Result<()> {
    match subscriptions.is_empty() {
        true => store.remove(SUBSCRIPTIONS),
        false => store.set(SUBSCRIPTIONS, &serde_json::to_string(subscriptions)?),
    }
}

/// This machine's VAPID key: the one push services know this sender by.
pub struct Vapid {
    pair: signature::EcdsaKeyPair,
    /// The public key as `applicationServerKey` wants it: uncompressed,
    /// base64url.
    public: String,
}

impl Vapid {
    /// The key kept in `store`, made and kept the first time it is asked for.
    ///
    /// Made on first use rather than at start, because it is only wanted once
    /// somebody turns notifications on, and a serve that never needed one
    /// should not write to `secrets.enc`. Kept rather than made per run,
    /// because a subscription is bound to the key it was made against: a new
    /// key would silently orphan every phone that had subscribed.
    pub fn load_or_create(store: &crate::secrets::Store) -> anyhow::Result<Vapid> {
        if let Some(kept) = store.get(VAPID_KEY)
            && let Some(der) = decode(&kept)
            && let Ok(vapid) = Vapid::from_pkcs8(&der)
        {
            return Ok(vapid);
        }
        let rng = SystemRandom::new();
        let der = signature::EcdsaKeyPair::generate_pkcs8(
            &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            &rng,
        )
        .map_err(|_| anyhow::anyhow!("could not make a VAPID key"))?;
        store.set(VAPID_KEY, &encode(der.as_ref()))?;
        Vapid::from_pkcs8(der.as_ref())
    }

    /// The key kept in `store`, without making one — for a sender, which has
    /// nobody to send to until a page has asked for the key.
    pub fn load(store: &crate::secrets::Store) -> Option<Vapid> {
        Vapid::from_pkcs8(&decode(&store.get(VAPID_KEY)?)?).ok()
    }

    fn from_pkcs8(der: &[u8]) -> anyhow::Result<Vapid> {
        let pair = signature::EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            der,
            &SystemRandom::new(),
        )
        .map_err(|e| anyhow::anyhow!("the kept VAPID key does not load: {e}"))?;
        let public = encode(pair.public_key().as_ref());
        Ok(Vapid { pair, public })
    }

    pub fn public_key(&self) -> &str {
        &self.public
    }

    /// The `Authorization` header for a push to `endpoint`, valid for twelve
    /// hours from `now` (seconds since the epoch) — RFC 8292 allows up to a
    /// day, and a JWT made per push never comes near either.
    pub fn authorization(&self, endpoint: &str, now: u64) -> anyhow::Result<String> {
        let audience = origin(endpoint).ok_or_else(|| anyhow::anyhow!("no origin in endpoint"))?;
        let header = encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = encode(
            serde_json::json!({ "aud": audience, "exp": now + 12 * 3600, "sub": SUBJECT })
                .to_string()
                .as_bytes(),
        );
        let input = format!("{header}.{claims}");
        // The fixed-size signature is ES256's: r and s, 32 bytes each, which
        // is what JWS wants rather than ASN.1.
        let signed = self
            .pair
            .sign(&SystemRandom::new(), input.as_bytes())
            .map_err(|_| anyhow::anyhow!("could not sign the VAPID token"))?;
        Ok(format!(
            "vapid t={input}.{}, k={}",
            encode(signed.as_ref()),
            self.public
        ))
    }
}

/// `scheme://host[:port]` of a URL — the audience a VAPID token names.
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    (!host.is_empty()).then(|| format!("{scheme}://{host}"))
}

/// Encrypt `plaintext` to `subscription`, as an `aes128gcm` body.
pub fn encrypt(subscription: &Subscription, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
    let bad = || anyhow::anyhow!("the subscription's keys do not encrypt");
    let ua_public = decode(&subscription.keys.p256dh).ok_or_else(bad)?;
    let auth = decode(&subscription.keys.auth).ok_or_else(bad)?;
    let rng = SystemRandom::new();
    // A key for this message alone: RFC 8291 has the sender make one per
    // push, which is also all ring's ECDH will do.
    let mine =
        agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rng).map_err(|_| bad())?;
    let as_public = mine.compute_public_key().map_err(|_| bad())?;
    let as_public = as_public.as_ref().to_vec();
    let theirs = agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, &ua_public);
    let ecdh =
        agreement::agree_ephemeral(mine, &theirs, |secret| secret.to_vec()).map_err(|_| bad())?;
    let mut salt = [0u8; 16];
    rng.fill(&mut salt).map_err(|_| bad())?;
    Ok(seal(&ecdh, &as_public, &ua_public, &auth, &salt, plaintext))
}

/// RFC 8291 §3.4 and RFC 8188 from the ECDH secret on: the part that is a
/// pure function of its inputs, which is what the RFC's own example tests.
fn seal(
    ecdh_secret: &[u8],
    as_public: &[u8],
    ua_public: &[u8],
    auth: &[u8],
    salt: &[u8; 16],
    plaintext: &[u8],
) -> Vec<u8> {
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(as_public);
    let ikm = derive(auth, ecdh_secret, &key_info, 32);
    let cek = derive(salt, &ikm, b"Content-Encoding: aes128gcm\0", 16);
    let nonce = derive(salt, &ikm, b"Content-Encoding: nonce\0", 12);

    // One record, so it is the last one, and its delimiter is 2.
    let mut record = plaintext[..plaintext.len().min(MAX_PLAINTEXT)].to_vec();
    record.push(2);
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_128_GCM, &cek).expect("16 bytes is an AES-128 key"),
    );
    key.seal_in_place_append_tag(
        aead::Nonce::try_assume_unique_for_key(&nonce).expect("12 bytes is a GCM nonce"),
        aead::Aad::empty(),
        &mut record,
    )
    .expect("one short record always seals");

    let mut body = Vec::with_capacity(16 + 4 + 1 + as_public.len() + record.len());
    body.extend_from_slice(salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(as_public.len() as u8);
    body.extend_from_slice(as_public);
    body.extend_from_slice(&record);
    body
}

/// HKDF-SHA256, `len` bytes of it.
fn derive(salt: &[u8], ikm: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    struct Len(usize);
    impl hkdf::KeyType for Len {
        fn len(&self) -> usize {
            self.0
        }
    }
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, salt).extract(ikm);
    let info = [info];
    let okm = prk
        .expand(&info, Len(len))
        .expect("at most 32 bytes is in HKDF's range");
    let mut out = vec![0u8; len];
    okm.fill(&mut out).expect("the length asked for");
    out
}

/// What became of one push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    Delivered,
    /// The push service says the subscription is no more — the browser
    /// unsubscribed, or the site's data was cleared. It is dropped.
    Gone,
    /// Anything else, said without the endpoint, which is itself a credential.
    Failed(String),
}

/// A push, shaped and signed: where it goes, its headers and its body.
#[derive(Debug, Clone)]
pub struct Request {
    pub endpoint: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

/// Encrypt and sign `payload` for `subscription`.
pub fn request(
    vapid: &Vapid,
    subscription: &Subscription,
    payload: &Payload,
    now: u64,
) -> anyhow::Result<Request> {
    let plaintext = serde_json::to_vec(payload)?;
    Ok(Request {
        endpoint: subscription.endpoint.clone(),
        headers: vec![
            ("TTL", TTL_SECS.to_string()),
            // A person is waiting to be told; a phone may hold back a
            // `normal` push for battery, and that is the delay this is for.
            ("Urgency", "high".to_string()),
            ("Content-Encoding", "aes128gcm".to_string()),
            ("Content-Type", "application/octet-stream".to_string()),
            (
                "Authorization",
                vapid.authorization(&subscription.endpoint, now)?,
            ),
        ],
        body: encrypt(subscription, &plaintext)?,
    })
}

/// POST one push, on the caller's thread.
pub fn send(request: &Request) -> Sent {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(SEND_TIMEOUT))
        // The status is the answer — 404 and 410 mean "drop it" — so it is
        // read rather than turned into an error.
        .http_status_as_error(false)
        .build()
        .into();
    let mut post = agent.post(&request.endpoint);
    for (name, value) in &request.headers {
        post = post.header(*name, value.as_str());
    }
    let origin = origin(&request.endpoint).unwrap_or_default();
    match post.send(&request.body[..]) {
        Ok(response) => match response.status().as_u16() {
            200..=299 => Sent::Delivered,
            404 | 410 => Sent::Gone,
            status => Sent::Failed(format!("{origin} answered {status}")),
        },
        Err(why) => Sent::Failed(format!(
            "{origin}: {}",
            why.to_string().replace(request.endpoint.as_str(), &origin)
        )),
    }
}

/// Push `payload` to every one of `subscriptions`, on a thread of its own,
/// then hand `gone` the endpoints the push services said are no more.
///
/// The caller is the publish path, which the refresh runs on, so nothing here
/// may block it. A failure is logged and otherwise silent: there is no one
/// to say it to on a phone that was not reached, and the next crossing tries
/// again.
pub fn push_all(
    vapid: std::sync::Arc<Vapid>,
    subscriptions: Vec<Subscription>,
    payload: Payload,
    gone: impl FnOnce(Vec<String>) + Send + 'static,
) {
    // A test that drives a crossing must never reach a push service: what
    // would have gone is kept for it to read.
    #[cfg(any(test, feature = "test-support"))]
    if crate::under_test() {
        PUSHED.with(|pushed| pushed.borrow_mut().push((subscriptions.len(), payload)));
        return;
    }
    std::thread::spawn(move || {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mut dropped = Vec::new();
        for subscription in subscriptions {
            let sent = match request(&vapid, &subscription, &payload, now) {
                Ok(request) => send(&request),
                Err(why) => Sent::Failed(why.to_string()),
            };
            crate::elog::event(
                "webpush",
                "send",
                serde_json::json!({
                    "outcome": match &sent {
                        Sent::Delivered => "delivered".to_string(),
                        Sent::Gone => "gone".to_string(),
                        Sent::Failed(why) => why.clone(),
                    },
                    "session": payload.session,
                }),
            );
            if sent == Sent::Gone {
                dropped.push(subscription.endpoint);
            }
        }
        if !dropped.is_empty() {
            gone(dropped);
        }
    });
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// What [`push_all`] would have sent, as (how many subscriptions, what).
    static PUSHED: std::cell::RefCell<Vec<(usize, Payload)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Every push made on this thread since the last call, emptied as read.
#[cfg(any(test, feature = "test-support"))]
pub fn take_pushed() -> Vec<(usize, Payload)> {
    PUSHED.with(|pushed| std::mem::take(&mut *pushed.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(text: &str) -> Vec<u8> {
        decode(&text.replace([' ', '\n'], "")).expect("base64url")
    }

    /// RFC 8291's own example (§5 and Appendix A), from the shared secret on:
    /// ring will not do ECDH with a fixed private key, so the ECDH step is the
    /// one thing taken from the RFC rather than computed.
    #[test]
    fn the_rfc_8291_example_encrypts_to_the_rfc_bytes() {
        let ecdh = b64("kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs");
        let as_public = b64("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIg
                             Dll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8");
        let ua_public = b64("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcx
                             aOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4");
        let auth = b64("BTBZMqHH6r4Tts7J_aSIgg");
        let salt: [u8; 16] = b64("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let body = seal(
            &ecdh,
            &as_public,
            &ua_public,
            &auth,
            &salt,
            b"When I grow up, I want to be a watermelon",
        );
        let expected = b64(
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml
             mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT
             pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN",
        );
        assert_eq!(encode(&body), encode(&expected));
    }

    /// The intermediate keys, for when the whole message disagrees and the
    /// question is which step did.
    #[test]
    fn the_rfc_8291_intermediate_keys_match() {
        let ecdh = b64("kyrL1jIIOHEzg3sM2ZWRHDRB62YACZhhSlknJ672kSs");
        let auth = b64("BTBZMqHH6r4Tts7J_aSIgg");
        let key_info = b64("V2ViUHVzaDogaW5mbwAEJXGyvs3942BVG
            q8e0PTNNmwR zr5VX4m8t7GGpTM5FzFo7OLr4BhZe9MEebhuPI-OztV3
            ylkYfpJGmQ22ggCLDgT-M_SrDepxkU21WCP3O1SUj0Ew
            bZIHMtu5pZpTKGSCIA5Zent7wmC6HCJ5mFgJkuk5cwAv MBKiiujwa7t45ewP");
        let ikm = derive(&auth, &ecdh, &key_info, 32);
        assert_eq!(encode(&ikm), "S4lYMb_L0FxCeq0WhDx813KgSYqU26kOyzWUdsXYyrg");
        let salt = b64("DGv6ra1nlYgDCS1FRnbzlw");
        let cek = derive(&salt, &ikm, b"Content-Encoding: aes128gcm\0", 16);
        assert_eq!(encode(&cek), "oIhVW04MRdy2XN9CiKLxTg");
        let nonce = derive(&salt, &ikm, b"Content-Encoding: nonce\0", 12);
        assert_eq!(encode(&nonce), "4h_95klXJ5E_qnoN");
    }

    /// A message encrypted the real way — a fresh key, a random salt —
    /// decrypts with the browser's side of the exchange. ring's ECDH only
    /// takes keys it made itself, so the "browser" here is one too.
    #[test]
    fn an_encrypted_push_has_the_aes128gcm_header() {
        let rng = SystemRandom::new();
        let browser =
            agreement::EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rng).unwrap();
        let public = browser.compute_public_key().unwrap();
        let subscription = Subscription {
            endpoint: "https://push.example.test/abc".into(),
            keys: Keys {
                p256dh: encode(public.as_ref()),
                auth: encode(&[7u8; 16]),
            },
        };
        assert_eq!(subscription.check(), Ok(()));
        let body = encrypt(&subscription, b"hello").unwrap();
        // salt, record size, key length, the sender's key, then one record of
        // the text, its delimiter and a 16-byte tag.
        assert_eq!(&body[16..20], &RECORD_SIZE.to_be_bytes());
        assert_eq!(body[20], 65);
        assert_eq!(body[21], 4, "an uncompressed point");
        assert_eq!(body.len(), 16 + 4 + 1 + 65 + 5 + 1 + 16);
        let sender = &body[21..86];
        let ecdh = agreement::agree_ephemeral(
            browser,
            &agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, sender),
            |s| s.to_vec(),
        )
        .unwrap();
        // Re-sealing with the browser's view of the secret and the same salt
        // gives the same bytes: both sides derived one key.
        let salt: [u8; 16] = body[..16].try_into().unwrap();
        let again = seal(&ecdh, sender, public.as_ref(), &[7u8; 16], &salt, b"hello");
        assert_eq!(again, body);
    }

    /// The VAPID token is an ES256 JWT for the endpoint's origin, signed with
    /// the key whose public half rides beside it.
    #[test]
    fn the_vapid_header_is_a_signed_jwt_for_the_push_origin() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::secrets::Store::in_dir(dir.path());
        std::fs::write(&store.machine_ids[0], "0123456789abcdef0123456789abcdef\n").unwrap();
        assert!(Vapid::load(&store).is_none(), "nothing is made until asked");
        let vapid = Vapid::load_or_create(&store).unwrap();
        let again = Vapid::load_or_create(&store).unwrap();
        assert_eq!(vapid.public_key(), again.public_key(), "kept, not remade");

        let header = vapid
            .authorization("https://fcm.googleapis.com/fcm/send/abc", 1_000)
            .unwrap();
        let (token, key) = header
            .strip_prefix("vapid t=")
            .unwrap()
            .split_once(", k=")
            .unwrap();
        assert_eq!(key, vapid.public_key());
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let claims: serde_json::Value = serde_json::from_slice(&b64(parts[1])).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        assert_eq!(claims["exp"], 1_000 + 12 * 3600);
        assert_eq!(claims["sub"], SUBJECT);
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, b64(key))
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &b64(parts[2]),
            )
            .expect("the signature verifies");
    }

    #[test]
    fn a_subscription_without_usable_keys_is_refused() {
        let good = Subscription {
            endpoint: "https://push.example.test/x".into(),
            keys: Keys {
                p256dh: encode(&[4u8; 65]),
                auth: encode(&[1u8; 16]),
            },
        };
        assert_eq!(good.check(), Ok(()));
        let mut plain = good.clone();
        plain.endpoint = "http://push.example.test/x".into();
        assert!(plain.check().is_err());
        let mut short = good.clone();
        short.keys.auth = encode(&[1u8; 8]);
        assert!(short.check().is_err());
        let mut compressed = good;
        compressed.keys.p256dh = encode(&[2u8; 33]);
        assert!(compressed.check().is_err());
    }

    /// The status a push service answers is what decides: 201 delivered,
    /// 404 and 410 gone, anything else a failure that does not name the
    /// endpoint's path.
    #[test]
    fn a_push_service_answer_is_read_by_its_status() {
        use std::io::{BufRead, BufReader, Read, Write};
        for (status, want) in [
            (201, "delivered"),
            (410, "gone"),
            (404, "gone"),
            (500, "failed"),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/secret-path", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut head = Vec::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                    head.push(line.trim().to_string());
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                write!(
                    reader.into_inner(),
                    "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                (head, body)
            });
            let request = Request {
                endpoint: endpoint.clone(),
                headers: vec![
                    ("TTL", "3600".into()),
                    ("Content-Encoding", "aes128gcm".into()),
                ],
                body: vec![1, 2, 3],
            };
            let sent = send(&request);
            let (head, body) = server.join().unwrap();
            assert_eq!(head[0], "POST /secret-path HTTP/1.1");
            assert!(
                head.iter().any(|h| h == "TTL: 3600" || h == "ttl: 3600"),
                "{head:?}"
            );
            assert_eq!(body, [1, 2, 3]);
            match (sent, want) {
                (Sent::Delivered, "delivered") | (Sent::Gone, "gone") => {}
                (Sent::Failed(why), "failed") => assert!(!why.contains("secret-path"), "{why}"),
                (sent, want) => panic!("{status}: {sent:?}, wanted {want}"),
            }
        }
    }
}
