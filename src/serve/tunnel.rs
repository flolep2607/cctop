//! Putting the local port on `*.trycloudflare.com`.
//!
//! A quick tunnel needs no Cloudflare account and no DNS: the client asks
//! `api.trycloudflare.com` for a hostname and a secret, dials the argotunnel
//! edge, and registers. It stops existing when the process does. That is the
//! right shape for "let me look at this from my phone for ten minutes" and the
//! wrong shape for anything permanent — the URL changes every run, which is also
//! why it is not a substitute for the token.
//!
//! The client is [`cloudflare_quick_tunnel`], which speaks that protocol itself —
//! QUIC to the edge and capnp-RPC over it — rather than shelling out to
//! `cloudflared` and reading a URL off its stderr. So `--tunnel` needs nothing
//! installed and cctop stays one binary, which is the whole reason it ships as
//! one.
//!
//! The consequence worth knowing: the tunnel's data path is *inside this
//! process*. Every request from the internet arrives as a QUIC stream, gets
//! proxied to the loopback listener by a task on the runtime below, and lands on
//! the same socket a local browser would use. So the connection cap, the token
//! check and the deadlines in [`super::http`] all still apply — the tunnel adds
//! a route in, not a second server.
//!
//! # What it does not do
//!
//! Cloudflare terminates the TLS. The traffic is encrypted from the browser to
//! the edge and from the edge to here, and readable in between by the party
//! carrying it — which is worth saying because the opposite is easy to assume of
//! anything with a `https://` URL. A quick tunnel is a way to reach your own
//! machine from a phone, not a private channel, and the announcement in
//! [`super::announce`] says so where someone will actually read it.

use cloudflare_quick_tunnel::{QuickTunnelHandle, QuickTunnelManager};

/// A live tunnel, reachable at [`url`](Tunnel::url) until it is dropped.
pub struct Tunnel {
    /// The `https://…trycloudflare.com` origin the edge assigned this run.
    pub url: String,
    /// Declared before the runtime so it drops first: the handle signals the
    /// edge reactors to wind down, which needs the runtime they are still
    /// running on.
    _handle: QuickTunnelHandle,
    /// Not a handle to park — this *is* the tunnel. The tasks it drives accept
    /// the edge's streams and proxy them; if it stops turning, the public URL
    /// stops answering.
    _runtime: tokio::runtime::Runtime,
}

/// Register a tunnel to `port` and return it, or say why not.
///
/// Blocking, and deliberately so: there is nothing to serve over a tunnel that
/// does not exist yet, and a URL printed before the edge has the registration is
/// a link that 404s for whoever opens it first.
pub fn start(port: u16) -> anyhow::Result<Tunnel> {
    // Two workers, because the proxying happens here rather than in somebody
    // else's process: one accepts streams while the other is still writing a
    // response.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    eprintln!("cctop: opening a trycloudflare tunnel…");
    let _ = std::io::Write::flush(&mut std::io::stderr());
    // ponytail: the crate's default HA connection count, untuned. It trades a
    // second QUIC connection for masking a single-POP reconnect, and the page
    // reports a gap of its own anyway.
    let handle = runtime
        .block_on(QuickTunnelManager::new(port).start())
        .map_err(|e| anyhow::anyhow!("{e}\nDrop --tunnel to serve on this machine only."))?;
    Ok(Tunnel {
        url: handle.url.clone(),
        _handle: handle,
        _runtime: runtime,
    })
}
