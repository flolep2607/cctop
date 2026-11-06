//! `/api/push/…`: the phones that asked to be told, and telling them.
//!
//! The page's Notify button subscribes the browser to Web Push when it can
//! (see `web/src/pages/dashboard/use-wanting-notifications.ts`), and hands the
//! subscription here. From then on the crossing the bell and `--notify` fire
//! on — a session going from working to waiting or asking — is also sent to
//! every subscription, through [`cctop_core::webpush`], so the phone hears it
//! with the page closed.
//!
//! The routes take the full token only. Subscribing is a write that outlives
//! the request, and it has this machine make requests to an address the
//! caller chose; neither is something a read-only link was minted to do.
//! Unlike the actions, a `--no-actions` serve still takes them: being told is
//! not acting on a session.

use crate::Access;
use crate::http::{self, Request, json};
use cctop_core::secrets::Store;
use cctop_core::session::Session;
use cctop_core::webpush::{self, Payload, Subscription, Vapid};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

/// The subscriptions and the key they were made against, kept in memory and
/// written through to `secrets.enc`.
///
/// In memory because the publish path asks [`Push::wanted`] on every refresh,
/// and opening the sealed file once a refresh to learn that nobody subscribed
/// would be the whole cost of a feature most serves never use.
pub struct Push {
    store: Store,
    subscriptions: Mutex<Vec<Subscription>>,
    vapid: Mutex<Option<Arc<Vapid>>>,
}

/// More subscriptions than anyone has browsers is somebody filling the file.
const MAX_SUBSCRIPTIONS: usize = 32;

impl Push {
    /// What `store` already holds. Makes no key: see [`Vapid::load_or_create`].
    pub fn new(store: Store) -> Push {
        Push {
            subscriptions: Mutex::new(webpush::load_subscriptions(&store)),
            vapid: Mutex::new(Vapid::load(&store).map(Arc::new)),
            store,
        }
    }

    /// Whether anyone would hear a push, so the publish path can skip
    /// working out what crossed when nobody would.
    pub fn wanted(&self) -> bool {
        self.subscriptions.lock().is_ok_and(|s| !s.is_empty())
    }

    /// The public key a browser subscribes against, made on first asking.
    fn key(&self) -> anyhow::Result<String> {
        let mut slot = self.vapid.lock().map_err(|_| anyhow::anyhow!("poisoned"))?;
        if let Some(vapid) = slot.as_ref() {
            return Ok(vapid.public_key().to_string());
        }
        let vapid = Arc::new(Vapid::load_or_create(&self.store)?);
        let key = vapid.public_key().to_string();
        *slot = Some(vapid);
        Ok(key)
    }

    /// Keep `subscription`, replacing one with the same endpoint — a browser
    /// that subscribes again gets fresh keys for the same address.
    fn subscribe(&self, subscription: Subscription) -> anyhow::Result<()> {
        let mut subscriptions = self
            .subscriptions
            .lock()
            .map_err(|_| anyhow::anyhow!("poisoned"))?;
        subscriptions.retain(|s| s.endpoint != subscription.endpoint);
        if subscriptions.len() >= MAX_SUBSCRIPTIONS {
            anyhow::bail!("{MAX_SUBSCRIPTIONS} browsers are already subscribed");
        }
        subscriptions.push(subscription);
        webpush::save_subscriptions(&self.store, &subscriptions)
    }

    /// Forget the subscriptions at `endpoints`.
    fn forget(&self, endpoints: &[String]) -> anyhow::Result<()> {
        let mut subscriptions = self
            .subscriptions
            .lock()
            .map_err(|_| anyhow::anyhow!("poisoned"))?;
        let before = subscriptions.len();
        subscriptions.retain(|s| !endpoints.contains(&s.endpoint));
        if subscriptions.len() == before {
            return Ok(());
        }
        webpush::save_subscriptions(&self.store, &subscriptions)
    }

    /// Tell every subscription that `session` crossed into `event` —
    /// `waiting` or `asking`, as [`cctop_core::notify::Reason::event`] names
    /// them.
    pub fn send(self: &Arc<Self>, event: &str, session: &Session) {
        let subscriptions = match self.subscriptions.lock() {
            Ok(s) if !s.is_empty() => s.clone(),
            _ => return,
        };
        // No key means no browser ever subscribed against this store, so the
        // subscriptions — if any — are from a key since lost, and cannot be
        // signed for.
        let Some(vapid) = self.vapid.lock().ok().and_then(|v| v.clone()) else {
            return;
        };
        let me = Arc::clone(self);
        webpush::push_all(vapid, subscriptions, payload(event, session), move |gone| {
            let _ = me.forget(&gone);
        });
    }
}

/// The notification for a crossing: the same two titles the page's own
/// notification uses, and the row's label — never the prompt, the last
/// message or anything else out of the transcript.
fn payload(event: &str, session: &Session) -> Payload {
    Payload {
        title: match event {
            "asking" => "Needs permission",
            "looping" => "Going round in circles",
            _ => "Waiting on you",
        }
        .to_string(),
        body: session.display_label().to_string(),
        session: session.session_id.clone(),
    }
}

/// `/api/push/<what>`.
pub(crate) fn route(
    push: &Push,
    stream: &mut TcpStream,
    request: &Request,
    what: &str,
    access: Access,
) {
    if access != Access::Full {
        return http::respond_error(
            stream,
            Some(request),
            403,
            "phone notifications need the full link, not a read-only one",
        );
    }
    if what == "key" {
        return match push.key() {
            Ok(key) => json(stream, request, &serde_json::json!({ "key": key })),
            Err(why) => http::respond_error(stream, Some(request), 503, &why.to_string()),
        };
    }
    // A POST with a JSON body, for the reason every action is one: a form on
    // another origin cannot send that — see the module docs in `http`.
    if !request.wants_json() {
        return http::respond_error(stream, Some(request), 405, "a POST with a JSON body");
    }
    let body = match request.json() {
        Ok(body) => body,
        Err((status, why)) => return http::respond_error(stream, Some(request), status, why),
    };
    let done = match what {
        "subscribe" => {
            let subscription: Subscription = match serde_json::from_value(body) {
                Ok(s) => s,
                Err(_) => {
                    return http::respond_error(
                        stream,
                        Some(request),
                        400,
                        "not a push subscription",
                    );
                }
            };
            if let Err(why) = subscription.check() {
                return http::respond_error(stream, Some(request), 400, why);
            }
            push.subscribe(subscription)
        }
        "unsubscribe" => {
            let Some(endpoint) = body.get("endpoint").and_then(|e| e.as_str()) else {
                return http::respond_error(stream, Some(request), 400, "no endpoint");
            };
            push.forget(&[endpoint.to_string()])
        }
        _ => return http::respond_error(stream, Some(request), 404, "no such route"),
    };
    match done {
        Ok(()) => json(stream, request, &serde_json::json!({ "ok": true })),
        Err(why) => http::respond_error(stream, Some(request), 503, &why.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{response_of, session, shared};
    use crate::{Remotes, Shared};
    use cctop_core::session::ActivityState;

    /// A serve whose subscriptions live in `dir`, with both tokens.
    fn serving(dir: &std::path::Path) -> Shared {
        let store = Store::in_dir(dir);
        std::fs::write(&store.machine_ids[0], "0123456789abcdef0123456789abcdef\n").unwrap();
        Shared {
            push: Arc::new(Push::new(store)),
            ..shared("full", "view")
        }
    }

    fn post(shared: &Shared, route: &str, token: &str, body: &str) -> String {
        response_of(
            shared,
            "POST",
            route,
            &format!(
                "Authorization: Bearer {token}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\n\r\n{body}",
                body.len()
            ),
        )
    }

    /// A subscription as a browser's `toJSON()` spells it, with keys of the
    /// right shapes.
    fn subscription(endpoint: &str) -> String {
        // 65 bytes of 0x04 and 16 of 0x01, base64url.
        let p256dh = "BAQE".repeat(22);
        serde_json::json!({
            "endpoint": endpoint,
            "expirationTime": null,
            "keys": { "p256dh": &p256dh[..87], "auth": "AQEBAQEBAQEBAQEBAQEBAQ" },
        })
        .to_string()
    }

    /// The full link gets the key and subscribes; the subscription is kept
    /// across a restart and goes when asked to. A read-only link does none of
    /// it, and a body that is not a usable subscription is refused.
    #[test]
    fn the_full_link_subscribes_and_unsubscribes() {
        let dir = tempfile::tempdir().unwrap();
        let guarded = serving(dir.path());
        let bearer = |token: &str| format!("Authorization: Bearer {token}\r\n");

        let key = response_of(&guarded, "GET", "/api/push/key", &bearer("full"));
        assert!(key.starts_with("HTTP/1.1 200 "), "{key}");
        let view = response_of(&guarded, "GET", "/api/push/key", &bearer("view"));
        assert!(view.starts_with("HTTP/1.1 403 "), "{view}");

        let endpoint = "https://push.example.test/abc";
        let refused = post(
            &guarded,
            "/api/push/subscribe",
            "view",
            &subscription(endpoint),
        );
        assert!(refused.starts_with("HTTP/1.1 403 "), "{refused}");
        assert!(!guarded.push.wanted());

        let plain = post(
            &guarded,
            "/api/push/subscribe",
            "full",
            &subscription("http://x.test/a"),
        );
        assert!(plain.starts_with("HTTP/1.1 400 "), "{plain}");
        let junk = post(&guarded, "/api/push/subscribe", "full", r#"{"endpoint":1}"#);
        assert!(junk.starts_with("HTTP/1.1 400 "), "{junk}");

        let ok = post(
            &guarded,
            "/api/push/subscribe",
            "full",
            &subscription(endpoint),
        );
        assert!(ok.starts_with("HTTP/1.1 200 "), "{ok}");
        // Again: the same browser, not a second subscription.
        post(
            &guarded,
            "/api/push/subscribe",
            "full",
            &subscription(endpoint),
        );
        assert!(guarded.push.wanted());

        // A restart reads them back, under the same key.
        let restarted = serving(dir.path());
        assert_eq!(restarted.push.subscriptions.lock().unwrap().len(), 1);
        let again = response_of(&restarted, "GET", "/api/push/key", &bearer("full"));
        assert_eq!(
            key.rsplit("\r\n\r\n").next(),
            again.rsplit("\r\n\r\n").next()
        );

        let gone = post(
            &restarted,
            "/api/push/unsubscribe",
            "full",
            &serde_json::json!({ "endpoint": endpoint }).to_string(),
        );
        assert!(gone.starts_with("HTTP/1.1 200 "), "{gone}");
        assert!(!restarted.push.wanted());
        assert!(!serving(dir.path()).push.wanted(), "and stays gone");
    }

    /// The worker is a script on this origin, behind the token like the
    /// page, and the page's policy lets it be registered.
    #[test]
    fn the_service_worker_is_served_behind_the_token() {
        let guarded = shared("full", "view");
        let worker = response_of(&guarded, "GET", "/sw.js?t=full", "");
        assert!(worker.starts_with("HTTP/1.1 200 "), "{worker}");
        assert!(worker.contains("text/javascript"), "{worker}");
        assert!(worker.contains("showNotification"), "{worker}");
        let anonymous = response_of(&guarded, "GET", "/sw.js", "");
        assert!(anonymous.starts_with("HTTP/1.1 403 "), "{anonymous}");
        let page = response_of(&guarded, "GET", "/api/config?t=full", "");
        assert!(page.contains("worker-src 'self'"), "{page}");
    }

    /// The push goes out on the crossing the webhook and the bell fire on —
    /// once, with the label and nothing out of the transcript — and never to
    /// the network under test.
    #[test]
    fn a_crossing_is_pushed_once_to_the_subscribers() {
        let dir = tempfile::tempdir().unwrap();
        let guarded = serving(dir.path());
        response_of(
            &guarded,
            "GET",
            "/api/push/key",
            "Authorization: Bearer full\r\n",
        );
        post(
            &guarded,
            "/api/push/subscribe",
            "full",
            &subscription("https://push.example.test/abc"),
        );
        let _ = webpush::take_pushed();

        let row = |state| {
            let mut s = session("s1");
            s.abbrev_label = "api".into();
            s.title = None;
            s.activity_state = state;
            s.process = Some(cctop_core::proc::ProcInfo::default());
            s
        };
        let remotes = Mutex::new(Remotes::default());
        let mut version = 0;
        let mut publish = |rows: &[Session]| {
            crate::pollers::publish(
                &guarded,
                &remotes,
                rows,
                cctop_core::pricing::Plan::Retail,
                None,
                &mut version,
                None,
            );
        };
        publish(&[row(ActivityState::Working)]);
        publish(&[row(ActivityState::Asking)]);
        publish(&[row(ActivityState::Asking)]);
        assert_eq!(
            webpush::take_pushed(),
            vec![(
                1,
                Payload {
                    title: "Needs permission".into(),
                    body: "api".into(),
                    session: "s1".into(),
                }
            )]
        );
    }
}
