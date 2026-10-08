//! Putting local ports on public URLs.
//!
//! cctop has two things to show to a phone: its own page (`cctop serve`) and a
//! terminal share from rmux. Both live on loopback, and a tunnel is what makes
//! them reachable from anywhere without opening a port. This crate is that
//! tunnel, behind an interface that does not say whose it is:
//!
//! - a [`Provider`] knows how to register with somebody's edge and hands back
//!   a live [`Tunnel`];
//! - [`Routes`] say which local port answers which public hostname, and can
//!   change while the tunnel is up — a daemon that restarts on another port
//!   moves its entry rather than costing a new registration.
//!
//! The two providers today are both Cloudflare's, in [`cloudflare`]: a quick
//! tunnel (`*.trycloudflare.com`, no account, a new name every run) and a named
//! tunnel on the user's own account (a stable hostname on their domain). A
//! provider from someone else implements the same trait and the callers above
//! do not change.
//!
//! # Where this came from
//!
//! The Cloudflare client began as `cloudflare-quick-tunnel` 0.3.1 by
//! lordmacu, which speaks the argotunnel protocol natively — QUIC to the edge
//! and Cap'n Proto RPC over it — so that nobody has to install `cloudflared`
//! and scrape a URL off its stderr. cctop carried it as a `[patch]` to fix
//! WebSockets, which reached the repository's own builds and not
//! `cargo install cctop`. It is a crate of the workspace now so that the fix,
//! named tunnels and host routing reach everyone; see `CHANGELOG.md`.

// Cap'n Proto-generated bindings. Shipped pre-generated under `src/proto_gen/`
// so building needs no `capnp` toolchain. They live at the crate root because
// the generator emits absolute `crate::<schema>_capnp::…` paths between
// schemas — hoisting them keeps the output usable verbatim.
#[allow(
    clippy::all,
    unused,
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    unused_qualifications,
    unsafe_op_in_unsafe_fn
)]
mod tunnelrpc_capnp {
    include!("proto_gen/tunnelrpc_capnp.rs");
}
#[allow(
    clippy::all,
    unused,
    non_camel_case_types,
    non_upper_case_globals,
    non_snake_case,
    unused_qualifications,
    unsafe_op_in_unsafe_fn
)]
mod quic_metadata_protocol_capnp {
    include!("proto_gen/quic_metadata_protocol_capnp.rs");
}

pub mod cloudflare;
mod error;
mod routes;

use std::future::Future;
use std::pin::Pin;

pub use error::Error;
pub use routes::Routes;

/// A boxed future, so [`Provider`] stays usable as a trait object.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Something that can register a tunnel and carry requests over it.
///
/// One method, because registering is the only thing a caller asks of a
/// provider: everything after that — which hostname goes where, shutting down
/// — is the [`Tunnel`]'s, and the same for every provider.
pub trait Provider: Send + Sync {
    /// What to call this tunnel in a sentence: "a quick tunnel",
    /// "your Cloudflare tunnel". Never carries a credential.
    fn describe(&self) -> String;

    /// Register, and start sending each request to the port [`Routes`] names
    /// for its hostname. Resolves once the public URL answers, so the URL is
    /// never handed out before it works.
    fn open(&self, routes: Routes) -> BoxFuture<'_, Result<Tunnel, Error>>;
}

/// What a provider keeps running behind a [`Tunnel`].
pub trait Live: Send {
    /// Stop carrying requests and tell the edge, waiting for it no longer than
    /// the provider thinks reasonable.
    ///
    /// The stop is signalled before this returns, and the future only waits
    /// for it to finish: [`Tunnel`]'s `Drop` cannot await, so it calls this and
    /// drops the future, and that must still stop the tunnel.
    fn shutdown(self: Box<Self>) -> BoxFuture<'static, ()>;
}

/// A registered tunnel, reachable at [`url`](Tunnel::url) until it is shut
/// down or dropped.
pub struct Tunnel {
    url: String,
    routes: Routes,
    live: Option<Box<dyn Live>>,
}

impl Tunnel {
    /// For providers: the tunnel `live` keeps running, reached at `url`.
    pub fn new(url: String, routes: Routes, live: Box<dyn Live>) -> Tunnel {
        Tunnel {
            url,
            routes,
            live: Some(live),
        }
    }

    /// The `https://` origin of the primary route, with no trailing slash.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The hostname [`url`](Tunnel::url) names.
    pub fn hostname(&self) -> &str {
        self.url.strip_prefix("https://").unwrap_or(&self.url)
    }

    /// Where requests go. Shared with the running tunnel: an entry added here
    /// is answered by the next request for it.
    pub fn routes(&self) -> &Routes {
        &self.routes
    }

    /// Unregister and wait for it.
    pub async fn shutdown(mut self) {
        if let Some(live) = self.live.take() {
            live.shutdown().await;
        }
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        // Dropping without `shutdown` still unregisters: the provider's tasks
        // see their signal and wind down on their own runtime, which is the
        // one thing a synchronous drop cannot wait for.
        if let Some(live) = self.live.take() {
            drop(live.shutdown());
        }
    }
}
