//! Cloudflare edge discovery: DNS SRV
//! (`_v2-origintunneld._tcp.argotunnel.com`) with a DNS-over-TLS
//! fallback through `1.1.1.1:853`. Mirrors the semantics of
//! `cloudflared/edgediscovery/allregions/discovery.go`.
//!
//! The result is a list of `EdgeAddr`s (resolved IPs + port 7844)
//! the caller can hand to `quic_dial::dial_any`. Order is shuffled
//! per-resolution so two adjacent processes don't pin the same
//! edge.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use hickory_resolver::TokioAsyncResolver;
use hickory_resolver::config::{NameServerConfigGroup, ResolverConfig, ResolverOpts};
use tracing::warn;

use crate::Error;

/// SRV record we resolve to discover the v2 origintunneld pool.
pub const SRV_NAME: &str = "_v2-origintunneld._tcp.argotunnel.com";

/// Server name for the DoT fallback resolver.
pub const DOT_SERVER_NAME: &str = "cloudflare-dns.com";

/// DoT endpoint address (Cloudflare public resolver).
pub const DOT_SERVER_ADDR: &str = "1.1.1.1:853";

#[derive(Debug, Clone, Copy)]
pub struct EdgeAddr {
    pub ip: IpAddr,
    pub port: u16,
}

impl EdgeAddr {
    pub fn socket(&self) -> SocketAddr {
        SocketAddr::new(self.ip, self.port)
    }
}

/// One-shot discovery without caching. System resolver first; on
/// failure / empty answer, falls back to DoT via `1.1.1.1`.
pub async fn discover() -> Result<Vec<EdgeAddr>, Error> {
    let primary = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());
    match resolve_srv(&primary).await {
        Ok(edges) if !edges.is_empty() => return Ok(shuffled(&edges)),
        Ok(_) => warn!("system resolver returned zero edges; falling back to DoT"),
        Err(e) => warn!(error = %e, "system resolver SRV failed; falling back to DoT"),
    }

    let dot = build_dot_resolver()?;
    let edges = resolve_srv(&dot).await?;
    if edges.is_empty() {
        return Err(Error::Discovery(format!(
            "DoT fallback also returned no edges for {SRV_NAME}"
        )));
    }
    Ok(shuffled(&edges))
}

// ── Internals ─────────────────────────────────────────────────────────────────

fn build_dot_resolver() -> Result<TokioAsyncResolver, Error> {
    let addr: SocketAddr = DOT_SERVER_ADDR
        .parse()
        .map_err(|e| Error::Discovery(format!("DoT addr parse: {e}")))?;
    let ns = NameServerConfigGroup::from_ips_tls(
        &[addr.ip()],
        addr.port(),
        DOT_SERVER_NAME.into(),
        true,
    );
    let cfg = ResolverConfig::from_parts(None, vec![], ns);
    let mut opts = ResolverOpts::default();
    opts.timeout = Duration::from_secs(15);
    Ok(TokioAsyncResolver::tokio(cfg, opts))
}

async fn resolve_srv(resolver: &TokioAsyncResolver) -> Result<Vec<EdgeAddr>, Error> {
    let srv = resolver
        .srv_lookup(SRV_NAME)
        .await
        .map_err(|e| Error::Discovery(format!("SRV {SRV_NAME}: {e}")))?;

    let mut edges = Vec::new();
    for rec in srv.iter() {
        let target = rec.target().to_utf8();
        let target = target.trim_end_matches('.');
        let port = rec.port();
        match resolver.lookup_ip(target).await {
            Ok(ips) => {
                edges.extend(ips.iter().map(|ip| EdgeAddr { ip, port }));
            }
            Err(e) => warn!(target, error = %e, "IP resolution failed for SRV target"),
        }
    }
    Ok(edges)
}

/// The same edges, rotated by a random amount. `RandomState` is seeded per
/// process, which is all the randomness this needs.
fn shuffled(input: &[EdgeAddr]) -> Vec<EdgeAddr> {
    use std::hash::BuildHasher;
    let n = input.len().max(1);
    let offset = (std::collections::hash_map::RandomState::new().hash_one(n) as usize) % n;
    let mut out = Vec::with_capacity(input.len());
    out.extend_from_slice(&input[offset..]);
    out.extend_from_slice(&input[..offset]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn fake(ip: u8) -> EdgeAddr {
        EdgeAddr {
            ip: IpAddr::V4(Ipv4Addr::new(198, 41, 192, ip)),
            port: 7844,
        }
    }

    #[test]
    fn shuffle_preserves_set() {
        let input: Vec<_> = (0..8).map(fake).collect();
        let out = shuffled(&input);
        assert_eq!(out.len(), input.len());
        let mut in_ips: Vec<_> = input.iter().map(|e| e.ip).collect();
        let mut out_ips: Vec<_> = out.iter().map(|e| e.ip).collect();
        in_ips.sort();
        out_ips.sort();
        assert_eq!(in_ips, out_ips);
    }
}
