//! Routes that exist only in a build that asked for them.
//!
//! ```bash
//! cargo run --features debug -- serve --no-token
//! ```
//!
//! The whole module is behind `#[cfg(feature = "debug")]`, which is not a
//! default feature — so a released cctop does not contain these routes, the
//! strings that name them, or the switch that arms them. That is the condition
//! for having them at all: a debug surface reachable in a shipped binary is a
//! debug surface somebody else can reach, and the token is one link away from
//! anyone the user has shared a page with.
//!
//! Two things live here, and the second is the reason the first was not enough.
//!
//! **Introspection** — `/api/debug/state` and `/api/debug/why` say what the
//! server is holding and how it decided which sessions are running. Both are
//! answerable from outside (`cctop -j`, `cctop why`), but not *from the page's
//! own process*, which is where a disagreement between the two would show.
//!
//! **Fault injection** — `/api/debug/fault` makes subsequent API responses fail
//! in a chosen way. The pages have to survive a tunnel whose far end has gone,
//! a server that is merely slow, and a proxy that answers HTML where JSON was
//! asked for; none of those can be produced on demand by a correct server, and
//! reproducing them used to mean a headless browser rewriting responses, or
//! unplugging something and being quick. Now it is a `curl`.

use std::sync::atomic::{AtomicU8, Ordering};

use super::http::{self, Request};
use super::{Shared, current};
use std::net::TcpStream;

/// How the next API response should fail, if it should.
///
/// Process-wide and a plain atomic: there is one server per process, this is
/// read on every request, and a debug switch is not worth a lock. `0` is off,
/// which is what a build that never calls `arm` leaves it at.
static FAULT: AtomicU8 = AtomicU8::new(0);

const OFF: u8 = 0;
const BAD_GATEWAY: u8 = 1;
const SLOW: u8 = 2;
const HTML: u8 = 3;
const EMPTY: u8 = 4;

/// The Cloudflare error page, in the shape the edge actually serves: an HTML
/// body, an HTML content type, and far longer than anything cctop would send.
///
/// This is the failure that started the error handling in the pages — a page
/// that trusts a body it did not send pastes this into the view.
const CLOUDFLARE_502: &str = concat!(
    "<!DOCTYPE html><html><head><title>trycloudflare.com | 502: Bad gateway",
    "</title></head><body><h1>Bad gateway</h1><span>Error code 502</span>",
    "<div>Visit cloudflare.com for more information.</div></body></html>"
);

/// Answer this request with the armed failure, if one is armed.
///
/// Called before the router, and only for `/api/` paths: faulting the HTML
/// routes too would take the page away rather than break what it fetches, and
/// the page is the thing under test.
pub fn intercept(stream: &mut TcpStream, request: &Request) -> bool {
    if !request.path.starts_with("/api/") || request.path.starts_with("/api/debug/") {
        return false;
    }
    match FAULT.load(Ordering::Relaxed) {
        OFF => false,
        BAD_GATEWAY => {
            http::respond(
                stream,
                Some(request),
                502,
                "text/html; charset=utf-8",
                CLOUDFLARE_502.as_bytes(),
            );
            true
        }
        SLOW => {
            // Longer than any deadline the pages set, and short enough that a
            // test does not look hung.
            std::thread::sleep(std::time::Duration::from_secs(30));
            false
        }
        HTML => {
            // A 200 that is not JSON: what a captive portal or a misconfigured
            // proxy returns, and the case where "check the status code" is not
            // enough on its own.
            http::respond(
                stream,
                Some(request),
                200,
                "text/html; charset=utf-8",
                b"<html><body>not the json you asked for</body></html>",
            );
            true
        }
        EMPTY => {
            http::respond(
                stream,
                Some(request),
                200,
                "application/json; charset=utf-8",
                b"",
            );
            true
        }
        _ => false,
    }
}

/// `/api/debug/*`. Returns false for a path this does not own.
pub fn route(shared: &Shared, stream: &mut TcpStream, request: &Request, rest: &str) -> bool {
    match rest {
        "state" => {
            let snapshot = current(shared);
            let body = serde_json::json!({
                "snapshot_version": snapshot.version,
                "sessions": snapshot.sessions.len(),
                "running": snapshot.sessions.iter().filter(|s| s.is_running()).count(),
                "host_errors": snapshot.host_errors,
                "json_bytes": snapshot.json.len(),
                "actions": shared.actions,
                "tokenless": shared.token.is_empty(),
                "plan": format!("{:?}", shared.plan),
                "fault": fault_name(FAULT.load(Ordering::Relaxed)),
            });
            json(stream, request, &body);
            true
        }
        // The same reasoning `cctop why` prints, from inside the serving
        // process — which is the only place a disagreement between the page and
        // the terminal could be seen.
        "why" => {
            let snapshot = current(shared);
            let rows: Vec<_> = snapshot
                .sessions
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "key": s.key(),
                        "running": s.is_running(),
                        "has_process": s.process.is_some(),
                        "launched_as": s.launched_as(),
                        "forked": s.launched_as() != s.session_id,
                        "last_active": s.last_active,
                        "cwd": s.label_source,
                    })
                })
                .collect();
            json(stream, request, &serde_json::json!({ "sessions": rows }));
            true
        }
        "fault" => {
            let mode = request
                .query
                .get("mode")
                .map_or_else(|| "off".to_string(), |m| m.to_lowercase());
            let armed = match mode.as_str() {
                "off" | "none" => OFF,
                "502" | "gateway" => BAD_GATEWAY,
                "slow" => SLOW,
                "html" => HTML,
                "empty" => EMPTY,
                _ => {
                    http::respond_error(
                        stream,
                        Some(request),
                        400,
                        "mode must be one of: off, 502, slow, html, empty",
                    );
                    return true;
                }
            };
            FAULT.store(armed, Ordering::Relaxed);
            json(
                stream,
                request,
                &serde_json::json!({ "fault": fault_name(armed) }),
            );
            true
        }
        _ => false,
    }
}

fn fault_name(mode: u8) -> &'static str {
    match mode {
        BAD_GATEWAY => "502",
        SLOW => "slow",
        HTML => "html",
        EMPTY => "empty",
        _ => "off",
    }
}

fn json(stream: &mut TcpStream, request: &Request, body: &serde_json::Value) {
    http::respond(
        stream,
        Some(request),
        200,
        "application/json; charset=utf-8",
        body.to_string().as_bytes(),
    );
}
