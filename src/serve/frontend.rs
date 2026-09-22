//! rmux's browser terminal, served from this page's own origin.
//!
//! The report page frames a session's terminal, and the terminal is rmux's
//! static frontend. Loaded from share.rmux.io it cannot be framed at all — that
//! origin answers with `frame-ancestors 'none'` — and it trips Chrome's Local
//! Network Access as well: a page on a public origin opening a socket to
//! `127.0.0.1` is exactly the crossing Chrome now blocks, so a loopback share
//! showed "Chrome blocked access to 127.0.0.1:9777" instead of a terminal.
//! Served from here, both go away: the frame is same-origin, and a loopback
//! page reaching a loopback socket is not a crossing.
//!
//! Mirrored rather than vendored. These are the bytes share.rmux.io serves —
//! the page the link would otherwise have loaded — so this moves where the
//! frontend is fetched from, not what is trusted. A copy pinned in the crate
//! would tie cctop's releases to rmux's wire protocol and weigh on the crate
//! size limit, for a frontend the machine could already reach.
//!
//! Outside the token, deliberately. Nothing here is private: it is a public
//! CDN's static files, and the frontend names its assets by absolute path, so
//! they cannot carry `?t=`. The share's credential is the link's fragment,
//! which the browser never sends to any server, this one included.
//!
//! ponytail: cached for the life of the process, so a frontend deploy on
//! share.rmux.io mid-run is picked up at the next restart.

use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Arc, Mutex};

use super::http::{self, Request};

const UPSTREAM: &str = "https://share.rmux.io";

/// Where the frontend's page lives on this origin — what a share is minted
/// with as its frontend URL. The page names some files relative to it
/// (`crabs/…`) and its build by absolute path (`/_astro/…`), so both are
/// mirrored.
pub const PREFIX: &str = "/rmux";

/// The upstream path a request maps to, or `None` if it is not the frontend's.
///
/// Names are held to the characters the frontend's files use, so the route
/// mirrors files and not whatever else the upstream would answer to.
pub fn upstream_path(path: &str) -> Option<&str> {
    let file = |rest: &str| {
        !rest.is_empty()
            && rest.split('/').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            })
    };
    match path {
        "/rmux" | "/rmux/" => Some("/"),
        _ if path.strip_prefix("/rmux/").is_some_and(file) => Some(&path[PREFIX.len()..]),
        _ if path.strip_prefix("/_astro/").is_some_and(file) => Some(path),
        _ => None,
    }
}

struct Asset {
    content_type: String,
    /// The upstream's own policy with framing allowed from this origin. The
    /// frontend needs things cctop's pages never do — its own scripts, wasm,
    /// sockets — and the upstream is the one that knows which.
    policy: String,
    body: Vec<u8>,
}

/// Answer one request for the frontend, fetching it on first sight.
pub fn serve(stream: &mut TcpStream, request: &Request, upstream: &str) {
    static CACHE: Mutex<Option<HashMap<String, Arc<Asset>>>> = Mutex::new(None);
    let held = CACHE
        .lock()
        .ok()
        .and_then(|cache| cache.as_ref()?.get(upstream).cloned());
    let asset = match held {
        Some(asset) => asset,
        None => match fetch(upstream) {
            Ok(asset) => {
                let asset = Arc::new(asset);
                if let Ok(mut cache) = CACHE.lock() {
                    cache
                        .get_or_insert_with(HashMap::new)
                        .insert(upstream.to_string(), Arc::clone(&asset));
                }
                asset
            }
            Err(why) => {
                return http::respond_error(
                    stream,
                    Some(request),
                    502,
                    &format!("could not fetch rmux's browser terminal from {UPSTREAM}: {why}"),
                );
            }
        },
    };
    let security = format!(
        "X-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Content-Security-Policy: {}\r\n",
        asset.policy
    );
    http::respond_with(
        stream,
        Some(request),
        200,
        &asset.content_type,
        &asset.body,
        &security,
    );
}

fn fetch(upstream: &str) -> Result<Asset, ureq::Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .into();
    let mut response = agent.get(format!("{UPSTREAM}{upstream}")).call()?;
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let content_type = header("content-type").unwrap_or_else(|| "application/octet-stream".into());
    let policy = header("content-security-policy").map_or_else(
        || "frame-ancestors 'self'".to_string(),
        |p| p.replace("frame-ancestors 'none'", "frame-ancestors 'self'"),
    );
    let body = response.body_mut().read_to_vec()?;
    Ok(Asset {
        content_type,
        policy,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_page_and_its_hashed_assets_are_mirrored() {
        assert_eq!(upstream_path("/rmux/"), Some("/"));
        assert_eq!(upstream_path("/rmux"), Some("/"));
        assert_eq!(
            upstream_path("/_astro/index.D2bSaP4U.css"),
            Some("/_astro/index.D2bSaP4U.css")
        );
        assert_eq!(
            upstream_path("/rmux/crabs/rose-dark.svg"),
            Some("/crabs/rose-dark.svg")
        );
        for refused in [
            "/_astro/",
            "/_astro/a//b.js",
            "/_astro/x?y",
            "/rmux/x%",
            "/rmuxx",
            "/",
        ] {
            assert_eq!(upstream_path(refused), None, "{refused} was mirrored");
        }
    }
}
