//! Cloudflare's tunnels: the quick one and a named one on your own account.
//!
//! They are one protocol. A quick tunnel *is* a named tunnel, on Cloudflare's
//! account instead of yours: `api.trycloudflare.com` hands back an account
//! tag, a tunnel id, a secret and a hostname, and from there the client
//! discovers the edge, dials it over QUIC and registers with exactly the
//! credentials a dashboard tunnel token carries. So the two providers differ
//! only in where the credentials and the hostname come from:
//!
//! | | credentials | hostname |
//! |---|---|---|
//! | [`Quick`] | asked of trycloudflare, every run | assigned with them |
//! | [`Named`] | a tunnel token you hold ([`Credentials::from_token`]) | given, or pushed by the edge from the tunnel's remote configuration |
//!
//! # What it does not do
//!
//! Cloudflare terminates the TLS. Traffic is encrypted from the browser to the
//! edge and from the edge to here, and readable in between by Cloudflare. A
//! tunnel is a way in, not a private channel.
//!
//! The remote configuration's `service` entries are not obeyed: where a
//! request goes is [`Routes`]'s answer, decided in this process. Only the
//! hostnames are read from it, so that a tunnel made in the dashboard can say
//! which name it serves.

mod api;
mod config;
mod connector;
mod edge;
mod pool;
mod proxy;
mod quic_dial;
mod rpc;
mod stream;
mod supervisor;
mod token;

#[cfg(test)]
mod fake_edge;

use std::fmt;
use std::time::Duration;

use uuid::Uuid;

use crate::{BoxFuture, Error, Live, Provider, Routes, Tunnel};
pub use token::{TokenError, looks_like_tunnel_token};

/// Budget for everything up to the first registration: asking for a quick
/// tunnel, discovering the edge, the handshake and the register call.
pub const DEFAULT_START_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a named tunnel with no hostname of its own waits for the edge to
/// push the tunnel's ingress rules. The edge sends them right after the first
/// registration of a dashboard-managed tunnel, so this is generous.
pub const DEFAULT_CONFIG_WAIT: Duration = Duration::from_secs(10);

/// How many QUIC connections to keep registered, each on its own connection
/// index. cloudflared uses four for named tunnels; two masks a single-POP
/// reconnect without doubling the traffic.
pub const DEFAULT_HA_CONNECTIONS: u8 = 2;

/// What the edge needs to accept a connection for a tunnel.
///
/// Its `Debug` prints neither the account tag nor the secret: a `{:?}` in a
/// log line is the easiest way for a credential to reach a file nobody meant
/// it for.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub account_tag: String,
    pub tunnel_id: Uuid,
    pub secret: Vec<u8>,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("account_tag", &"[redacted]")
            .field("tunnel_id", &self.tunnel_id)
            .field("secret", &"[redacted]")
            .finish()
    }
}

impl Credentials {
    /// Read a tunnel token — what the dashboard shows for a tunnel, and what
    /// `cloudflared tunnel run --token` takes.
    pub fn from_token(token: &str) -> Result<Credentials, TokenError> {
        token::decode(token)
    }
}

/// Where the edge is and which certificates it presents. Fixed in tests to a
/// fake edge on loopback; there is no way to set it from outside the crate,
/// because pointing a tunnel at another "edge" is handing it the secret.
#[derive(Clone, Default)]
pub(crate) enum EdgeSource {
    #[default]
    Discover,
    #[cfg(test)]
    Fixed(fake_edge::Seam),
}

/// A quick tunnel: no account, a `*.trycloudflare.com` name that changes on
/// every start.
pub struct Quick {
    service_url: String,
    timeout: Duration,
    ha_connections: u8,
    edge: EdgeSource,
}

impl Default for Quick {
    fn default() -> Self {
        Quick {
            service_url: api::DEFAULT_SERVICE_URL.to_string(),
            timeout: DEFAULT_START_TIMEOUT,
            ha_connections: DEFAULT_HA_CONNECTIONS,
            edge: EdgeSource::Discover,
        }
    }
}

impl Quick {
    pub fn new() -> Quick {
        Quick::default()
    }
}

impl Provider for Quick {
    fn describe(&self) -> String {
        "a quick tunnel".to_string()
    }

    fn open(&self, routes: Routes) -> BoxFuture<'_, Result<Tunnel, Error>> {
        Box::pin(async move {
            tokio::time::timeout(self.timeout, async {
                let quick = api::request_tunnel(&self.service_url, api::DEFAULT_USER_AGENT).await?;
                let tunnel_id = Uuid::parse_str(&quick.id)
                    .map_err(|e| Error::Internal(format!("tunnel.id is not a uuid: {e}")))?;
                let credentials = Credentials {
                    account_tag: quick.account_tag.clone(),
                    tunnel_id,
                    secret: quick.secret.clone(),
                };
                let url = match quick.hostname.starts_with("https://") {
                    true => quick.hostname.clone(),
                    false => format!("https://{}", quick.hostname),
                };
                // The hostname was assigned, so every request on this tunnel
                // is for it: there is nothing else to route.
                routes.route_any_host();
                let connected = connector::connect(
                    &credentials,
                    routes.clone(),
                    self.ha_connections,
                    &self.edge,
                )
                .await?;
                Ok(Tunnel::new(url, routes, Box::new(connected.live)))
            })
            .await
            .map_err(|_| Error::Internal("the tunnel did not come up in time".into()))?
        })
    }
}

/// A named tunnel on the user's own Cloudflare account.
pub struct Named {
    credentials: Credentials,
    hostname: Option<String>,
    timeout: Duration,
    config_wait: Duration,
    ha_connections: u8,
    edge: EdgeSource,
}

impl Named {
    /// The tunnel these credentials name, at `hostname` — or, with `None`, at
    /// the first hostname of the ingress rules the edge pushes.
    pub fn new(credentials: Credentials, hostname: Option<String>) -> Named {
        Named {
            credentials,
            hostname: hostname
                .map(|h| h.trim().trim_end_matches('/').to_string())
                .map(|h| h.strip_prefix("https://").map(str::to_string).unwrap_or(h))
                .filter(|h| !h.is_empty()),
            timeout: DEFAULT_START_TIMEOUT,
            config_wait: DEFAULT_CONFIG_WAIT,
            ha_connections: DEFAULT_HA_CONNECTIONS,
            edge: EdgeSource::Discover,
        }
    }

    /// The tunnel's id, which is not a secret: it is in the DNS record.
    pub fn tunnel_id(&self) -> Uuid {
        self.credentials.tunnel_id
    }
}

impl Provider for Named {
    fn describe(&self) -> String {
        "your Cloudflare tunnel".to_string()
    }

    fn open(&self, routes: Routes) -> BoxFuture<'_, Result<Tunnel, Error>> {
        Box::pin(async move {
            let mut connected = tokio::time::timeout(
                self.timeout,
                connector::connect(
                    &self.credentials,
                    routes.clone(),
                    self.ha_connections,
                    &self.edge,
                ),
            )
            .await
            .map_err(|_| Error::Internal("the tunnel did not come up in time".into()))??;
            let hostname = match &self.hostname {
                Some(given) => given.clone(),
                None => {
                    let wait = connected
                        .ingress
                        .wait_for(|pushed| !pushed.hostnames.is_empty());
                    // Copied out before anything else is awaited: the borrow
                    // of the channel is not something to hold across a yield.
                    let pushed = match tokio::time::timeout(self.config_wait, wait).await {
                        Ok(Ok(pushed)) => Some(pushed.hostnames[0].clone()),
                        _ => None,
                    };
                    match pushed {
                        Some(host) => host,
                        None => {
                            Box::new(connected.live).shutdown().await;
                            return Err(Error::NoHostname);
                        }
                    }
                }
            };
            routes.insert(&hostname, routes.primary());
            Ok(Tunnel::new(
                format!("https://{hostname}"),
                routes,
                Box::new(connected.live),
            ))
        })
    }
}

#[cfg(test)]
impl Quick {
    pub(crate) fn against(seam: fake_edge::Seam, service_url: String) -> Quick {
        Quick {
            service_url,
            timeout: Duration::from_secs(10),
            ha_connections: 1,
            edge: EdgeSource::Fixed(seam),
        }
    }
}

#[cfg(test)]
impl Named {
    pub(crate) fn against(mut self, seam: fake_edge::Seam) -> Named {
        self.edge = EdgeSource::Fixed(seam);
        self.ha_connections = 1;
        self.timeout = Duration::from_secs(10);
        self.config_wait = Duration::from_secs(5);
        self
    }
}
