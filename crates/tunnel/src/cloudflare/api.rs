//! POST `/tunnel` client for `api.trycloudflare.com`.
//!
//! Returns the credentials the edge expects on the subsequent
//! `RegisterConnection` RPC: a UUID-shaped `id`, the public
//! `hostname` (`<sub>.trycloudflare.com`), the `account_tag` to
//! quote on RPC, and 32 random bytes of `secret` that double as the
//! `TunnelSecret` in the auth blob.
//!
//! Mirrors `cmd/cloudflared/tunnel/quick_tunnel.go` upstream.

use std::time::Duration;

use serde::Deserialize;
use tokio::time::sleep;
use tracing::{debug, warn};

use crate::Error;
use crate::error::QuickTunnelApiError;

/// Public-facing JSON envelope returned by `POST /tunnel`.
#[derive(Deserialize)]
pub struct QuickTunnelResponse {
    pub success: bool,
    #[serde(default)]
    pub result: Option<QuickTunnel>,
    #[serde(default)]
    pub errors: Vec<QuickTunnelApiError>,
}

/// The bits the QUIC + capnp-RPC dance needs.
///
/// `secret` is delivered as a base64 string in the JSON body; the
/// `serde_bytes_b64` helper decodes it back to raw bytes so callers
/// can stuff them straight into the capnp `TunnelAuth.tunnelSecret`
/// field. Mirror of cloudflared's `QuickTunnel` Go struct.
#[derive(Deserialize)]
pub struct QuickTunnel {
    pub id: String,
    pub hostname: String,
    pub account_tag: String,
    #[serde(with = "serde_bytes_b64")]
    pub secret: Vec<u8>,
}

/// By hand, because a derived one prints the secret. A quick tunnel's secret
/// is worth less than an account's, but it is still what lets someone else
/// register as this tunnel and answer its URL.
impl std::fmt::Debug for QuickTunnel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuickTunnel")
            .field("id", &self.id)
            .field("hostname", &self.hostname)
            .field("account_tag", &"[redacted]")
            .field("secret", &"[redacted]")
            .finish()
    }
}

/// Default endpoint (the public trycloudflare API).
pub const DEFAULT_SERVICE_URL: &str = "https://api.trycloudflare.com";

/// User-Agent we send. Mimic a recent `cloudflared` so the edge
/// doesn't trip a novelty filter. Bump in lockstep with the
/// schema commit pinned in `THIRD_PARTY_NOTICES.md`.
pub const DEFAULT_USER_AGENT: &str = "cloudflared/2024.12.0";

/// HTTP-level deadline for the POST.
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(15);

/// How many times to retry on transient 5xx / network errors.
pub const MAX_RETRIES: u32 = 3;

/// Fetch a fresh quick-tunnel handshake. Retries 5xx + network
/// errors with exponential backoff (1s → 2s → 4s); never retries
/// 4xx. Business errors inside a 200 response surface as
/// [`Error::ApiBusiness`].
pub async fn request_tunnel(service_url: &str, user_agent: &str) -> Result<QuickTunnel, Error> {
    let url = format!("{}/tunnel", service_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(DEFAULT_HTTP_TIMEOUT)
        .build()
        .map_err(Error::Api)?;

    let mut backoff = Duration::from_secs(1);
    let mut last_err: Option<Error> = None;

    for attempt in 0..=MAX_RETRIES {
        debug!(attempt, %url, "POST /tunnel");
        match try_once(&client, &url).await {
            Ok(tunnel) => return Ok(tunnel),
            Err(err) => {
                if !err.is_transient() || attempt == MAX_RETRIES {
                    return Err(err);
                }
                warn!(
                    attempt,
                    error = %err,
                    backoff_ms = backoff.as_millis() as u64,
                    "POST /tunnel transient failure; retrying"
                );
                last_err = Some(err);
                sleep(backoff).await;
                backoff = backoff.saturating_mul(2);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| {
        Error::Internal("request_tunnel: retry loop fell through without an error".into())
    }))
}

async fn try_once(client: &reqwest::Client, url: &str) -> Result<QuickTunnel, Error> {
    let resp = client
        .post(url)
        .header("Content-Type", "application/json")
        .send()
        .await?;

    let status = resp.status();
    let body = resp.bytes().await?;

    // 5xx with non-JSON body must surface as a transient error so
    // the retry loop kicks in. JSON 5xx envelopes are rare but
    // possible — they go through the parse path below.
    if status.is_server_error() && !looks_like_json(&body) {
        let snippet_len = 200usize.min(body.len());
        let body_snippet = String::from_utf8_lossy(&body[..snippet_len]).into_owned();
        return Err(Error::ApiNonJson {
            status: status.as_u16(),
            body_snippet,
        });
    }

    // The edge sometimes hands back HTML when rate-limiting; surface
    // a snippet so the operator can read the actual reason instead
    // of staring at a bare "expected value at line 1 column 1".
    if !looks_like_json(&body) {
        let snippet_len = 200usize.min(body.len());
        let body_snippet = String::from_utf8_lossy(&body[..snippet_len]).into_owned();
        return Err(Error::ApiNonJson {
            status: status.as_u16(),
            body_snippet,
        });
    }

    let envelope: QuickTunnelResponse = serde_json::from_slice(&body)
        .map_err(|e| Error::Internal(format!("malformed JSON from /tunnel: {e}")))?;

    if !envelope.success {
        return Err(Error::ApiBusiness(envelope.errors));
    }

    envelope.result.ok_or_else(|| {
        Error::Internal("POST /tunnel returned success=true but no `result` body".into())
    })
}

fn looks_like_json(body: &[u8]) -> bool {
    body.iter()
        .find(|b| !b.is_ascii_whitespace())
        .is_some_and(|b| *b == b'{' || *b == b'[')
}

mod serde_bytes_b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s: String = Deserialize::deserialize(d)?;
        STANDARD.decode(s).map_err(serde::de::Error::custom)
    }
}

impl Error {
    /// Errors a retry could plausibly recover (network / 5xx).
    pub(crate) fn is_transient(&self) -> bool {
        match self {
            Error::Api(e) => {
                e.is_timeout()
                    || e.is_connect()
                    || e.is_request()
                    || e.status().is_some_and(|s| s.is_server_error())
            }
            Error::ApiNonJson { status, .. } => (500..600).contains(status),
            _ => false,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A stand-in for `api.trycloudflare.com`: answers each POST with the next
    /// of `replies` (status, body), repeating the last, and counts the hits.
    /// Plain blocking sockets on a thread, as the client is all this exercises.
    pub(crate) fn fake_api(replies: Vec<(u16, String)>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counted = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut request = Vec::new();
                let mut buf = [0u8; 4096];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&buf[..n]),
                    }
                }
                let n = counted.fetch_add(1, Ordering::SeqCst);
                let (status, body) = &replies[n.min(replies.len() - 1)];
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (url, hits)
    }

    pub(crate) fn sample_ok_body() -> String {
        serde_json::json!({
            "success": true,
            "result": {
                "id": "8f6d3c2a-1111-4d2e-9b9b-aaaaaaaaaaaa",
                "name": "quick-tunnel-abc",
                "hostname": "abc-123.trycloudflare.com",
                "account_tag": "deadbeefcafef00d",
                "secret": "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA="
            },
            "errors": []
        })
        .to_string()
    }

    #[tokio::test]
    async fn happy_path_parses_credentials() {
        let (url, hits) = fake_api(vec![(200, sample_ok_body())]);
        let t = request_tunnel(&url, DEFAULT_USER_AGENT)
            .await
            .expect("happy path");
        assert_eq!(t.hostname, "abc-123.trycloudflare.com");
        assert_eq!(t.account_tag, "deadbeefcafef00d");
        assert_eq!(t.secret.len(), 32);
        assert_eq!(t.secret[0..4], [1, 2, 3, 4]);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn business_error_does_not_retry() {
        let body = serde_json::json!({
            "success": false,
            "errors": [{ "code": 1003, "message": "tunnel quota exceeded" }]
        });
        let (url, hits) = fake_api(vec![(200, body.to_string())]);
        let err = request_tunnel(&url, DEFAULT_USER_AGENT)
            .await
            .expect_err("should fail");
        match err {
            Error::ApiBusiness(errs) => {
                assert_eq!(errs.len(), 1);
                assert_eq!(errs[0].code, 1003);
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn html_body_surfaces_snippet() {
        let (url, _) = fake_api(vec![(429, "<html><body>rate limited</body></html>".into())]);
        let err = request_tunnel(&url, DEFAULT_USER_AGENT)
            .await
            .expect_err("should fail");
        match err {
            Error::ApiNonJson {
                status,
                body_snippet,
            } => {
                assert_eq!(status, 429);
                assert!(body_snippet.contains("rate limited"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn five_xx_retries_then_succeeds() {
        // The first backoff is a second and not configurable, so this test
        // sits for one.
        let (url, hits) = fake_api(vec![
            (503, "service unavailable".into()),
            (200, sample_ok_body()),
        ]);
        let t = request_tunnel(&url, DEFAULT_USER_AGENT)
            .await
            .expect("retry should succeed");
        assert_eq!(t.hostname, "abc-123.trycloudflare.com");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn four_xx_does_not_retry() {
        let body = serde_json::json!({
            "success": false,
            "errors": [{ "code": 400, "message": "bad request" }]
        });
        let (url, hits) = fake_api(vec![(400, body.to_string())]);
        let err = request_tunnel(&url, DEFAULT_USER_AGENT)
            .await
            .expect_err("should fail");
        assert!(matches!(err, Error::ApiBusiness(_)));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn looks_like_json_handles_leading_whitespace() {
        assert!(looks_like_json(b"  \n  {"));
        assert!(looks_like_json(b"["));
        assert!(!looks_like_json(b"<html>"));
        assert!(!looks_like_json(b""));
    }

    #[test]
    fn debug_prints_no_secret() {
        let t: QuickTunnel = serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(&sample_ok_body()).unwrap()["result"].clone(),
        )
        .unwrap();
        let shown = format!("{t:?}");
        assert!(!shown.contains("deadbeefcafef00d"), "{shown}");
        assert!(!shown.contains("[1, 2, 3"), "{shown}");
    }
}
