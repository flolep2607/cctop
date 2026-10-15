//! Registering a tunnel with the edge and keeping it registered.
//!
//! Given credentials — from trycloudflare or from a tunnel token, the edge
//! cannot tell — [`connect`] runs:
//!
//!   1. edge discovery (`edge::discover`);
//!   2. a dial — QUIC or HTTP/2, as the [`Selector`] says — and
//!      `registerConnection` on connection index 0, awaited, so the caller
//!      learns of a refused tunnel before it hands out a URL;
//!   3. one reactor task per HA connection, each owning its connection: it
//!      serves it, and reconnects with backoff and `replace_existing` when
//!      the edge drops it, until told to stop.
//!
//! Every dial asks the tunnel's one [`Selector`] which transport to use, and
//! tells it how QUIC fared; `protocol` says when `auto` moves.
//!
//! The ingress hostnames the edge pushes to any of those connections land in
//! one shared [`watch`] channel, which is how a dashboard-made tunnel says
//! which name it serves.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use quinn::Endpoint;
use tokio::sync::watch;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::config::{Ingress, IngressTx};
use super::edge::{EdgeAddr, discover};
use super::http2;
use super::pool::Pool;
use super::protocol::{Protocol, Selector, Transport, Trouble};
use super::quic_dial::{self, build_endpoint};
use super::rpc::{
    ConnectionOptions, ControlSession, TunnelAuth, register_connection, register_over,
};
use super::supervisor::{self, SupervisorExit, SupervisorMetrics};
use super::{Credentials, EdgeSource};
use crate::{BoxFuture, Error, Live, Routes};

/// Budget for the `unregisterConnection` RPC on shutdown.
pub const DEFAULT_GRACE_PERIOD: Duration = Duration::from_secs(5);

/// Consecutive reconnect failures before a reactor gives up. Each failure
/// widens the backoff (1s → 30s), so ten span about ninety seconds of trying.
pub const MAX_RECONNECT_ATTEMPTS: u32 = 10;

/// Hard ceiling, as in cloudflared: higher hits diminishing returns, and the
/// edge may rate-limit registrations from one source.
pub const MAX_HA_CONNECTIONS: u8 = 4;

/// Baked into `ConnectionOptions.client.version`.
pub const CLIENT_VERSION: &str = concat!("cctop-tunnel/", env!("CARGO_PKG_VERSION"));

/// How many edge addresses one dial tries. `auto` tries fewer over QUIC, with
/// cloudflared's own handshake timeout (5 s, `quic/constants.go`) rather than
/// ten, so that a network with no UDP to the edge reaches HTTP/2 inside the
/// thirty-second start budget.
const EDGES_PER_DIAL: usize = 5;
const AUTO_QUIC_EDGES: usize = 2;
#[cfg(not(test))]
const AUTO_QUIC_HANDSHAKE: Duration = Duration::from_secs(5);
#[cfg(not(test))]
const QUIC_HANDSHAKE: Duration = quic_dial::DEFAULT_HANDSHAKE_TIMEOUT;
/// Under test the fake edge is on loopback, where a handshake takes
/// milliseconds; the tests of a silent UDP port should not wait out seconds.
#[cfg(test)]
const AUTO_QUIC_HANDSHAKE: Duration = Duration::from_secs(2);
#[cfg(test)]
const QUIC_HANDSHAKE: Duration = Duration::from_secs(2);

/// What every reactor of one tunnel shares.
#[derive(Clone)]
pub(crate) struct Shared {
    endpoint: Endpoint,
    h2_tls: Arc<rustls::ClientConfig>,
    protocol: Protocol,
    selector: Arc<Mutex<Selector>>,
    auth: TunnelAuth,
    tunnel_id: Uuid,
    edge: EdgeSource,
    routes: Routes,
    pool: Arc<Pool>,
    metrics: SupervisorMetrics,
    reconnects: Arc<AtomicU64>,
    ingress: IngressTx,
    stop: watch::Receiver<bool>,
}

/// A registered tunnel's moving parts.
pub(crate) struct Connected {
    /// The ingress hostnames the edge last pushed, empty until it does.
    pub ingress: watch::Receiver<Ingress>,
    pub live: Reactors,
}

/// The reactor tasks, and the switch that stops them.
pub(crate) struct Reactors {
    stop: watch::Sender<bool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Live for Reactors {
    fn shutdown(self: Box<Self>) -> BoxFuture<'static, ()> {
        // A watch rather than a `Notify`: a reactor between two awaits — in
        // the middle of a reconnect, say — has no waiter registered, and would
        // miss a notification. A value it can read later is not missed.
        let _ = self.stop.send(true);
        let tasks = self.tasks;
        Box::pin(async move {
            for task in tasks {
                let _ = task.await;
            }
        })
    }
}

/// Register `credentials` with the edge on `ha_connections` connections, over
/// the transport `protocol` allows, and proxy what arrives to the ports
/// `routes` names.
pub(crate) async fn connect(
    credentials: &Credentials,
    routes: Routes,
    ha_connections: u8,
    edge: &EdgeSource,
    protocol: Protocol,
) -> Result<Connected, Error> {
    let endpoint = build_endpoint(edge)?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let (ingress_tx, ingress_rx) = watch::channel(Ingress::default());
    let shared = Shared {
        endpoint,
        h2_tls: http2::tls_config(edge)?,
        protocol,
        selector: Arc::new(Mutex::new(Selector::new(protocol))),
        auth: TunnelAuth {
            account_tag: credentials.account_tag.clone(),
            tunnel_secret: credentials.secret.clone(),
        },
        tunnel_id: credentials.tunnel_id,
        edge: edge.clone(),
        pool: Arc::new(Pool::new()),
        routes,
        metrics: SupervisorMetrics::default(),
        reconnects: Arc::new(AtomicU64::new(0)),
        ingress: Arc::new(ingress_tx),
        stop: stop_rx,
    };

    // The first registration is awaited: it is the one that says whether the
    // edge accepts these credentials at all. When its failure moved `auto` to
    // HTTP/2 — no UDP to the edge — HTTP/2 is tried at once, inside the same
    // start budget, rather than after a backoff.
    let first = match connect_cycle(&shared, 0, false).await {
        Err(e) if !matches!(e, Error::Refused(_)) && shared.transport() == Transport::Http2 => {
            info!(error = %e, "first registration failed over QUIC; trying HTTP/2");
            connect_cycle(&shared, 0, false).await
        }
        result => result,
    };
    let (conn0, control0, location0) = first?;
    info!(%location0, conn_index = 0, "first registration succeeded");

    let ha = ha_connections.clamp(1, MAX_HA_CONNECTIONS);
    let mut tasks = Vec::with_capacity(ha as usize);
    tasks.push(tokio::spawn(reactor_loop(
        shared.clone(),
        0,
        conn0,
        control0,
    )));
    for idx in 1..ha {
        let shared = shared.clone();
        tasks.push(tokio::spawn(async move {
            match connect_cycle(&shared, idx, false).await {
                Ok((conn, control, location)) => {
                    info!(%location, conn_index = idx, "HA registration succeeded");
                    reactor_loop(shared, idx, conn, control).await;
                }
                Err(e) => {
                    warn!(error = %e, conn_index = idx, "HA registration failed; will retry");
                    if let Some((conn, control)) = reconnect(&shared, idx, false).await {
                        reactor_loop(shared, idx, conn, control).await;
                    }
                }
            }
        }));
    }

    Ok(Connected {
        ingress: ingress_rx,
        live: Reactors {
            stop: stop_tx,
            tasks,
        },
    })
}

impl Shared {
    fn transport(&self) -> Transport {
        lock(&self.selector).transport(Instant::now())
    }

    fn http2_serve(&self) -> http2::Serve {
        http2::Serve {
            routes: self.routes.clone(),
            metrics: self.metrics.clone(),
            pool: self.pool.clone(),
            ingress: self.ingress.clone(),
        }
    }
}

/// The selector, poisoned or not: it holds counters and timestamps, and a
/// panic elsewhere leaves nothing in them half-written.
fn lock(selector: &Mutex<Selector>) -> MutexGuard<'_, Selector> {
    selector.lock().unwrap_or_else(|e| e.into_inner())
}

/// One registered connection to the edge, over either transport.
pub(crate) enum Link {
    Quic(quinn::Connection),
    Http2(http2::Link),
}

impl Link {
    fn transport(&self) -> Transport {
        match self {
            Link::Quic(_) => Transport::Quic,
            Link::Http2(_) => Transport::Http2,
        }
    }

    /// Carry requests until the connection ends.
    async fn serve(&mut self, shared: &Shared) -> SupervisorExit {
        match self {
            Link::Quic(conn) => {
                supervisor::run(
                    conn.clone(),
                    shared.routes.clone(),
                    shared.metrics.clone(),
                    shared.pool.clone(),
                    shared.ingress.clone(),
                )
                .await
            }
            Link::Http2(link) => link.closed().await,
        }
    }

    /// Close with a code rather than leave the edge to time it out.
    fn close(self) {
        match self {
            Link::Quic(conn) => conn.close(0u32.into(), b"client shutdown"),
            Link::Http2(link) => link.close(),
        }
    }
}

/// Dial the next edge over the transport the selector picks, and register on
/// `conn_index`. `replace_existing` on a reconnect, so the edge takes the new
/// connection in place of the one it lost — over the other transport as
/// readily as the same one; not on a leg's first registration.
async fn connect_cycle(
    shared: &Shared,
    conn_index: u8,
    replace_existing: bool,
) -> Result<(Link, ControlSession, String), Error> {
    let transport = shared.transport();
    let result = dial_and_register(shared, transport, conn_index, replace_existing).await;
    let mut selector = lock(&shared.selector);
    match &result {
        Ok(_) => {
            info!(conn_index, %transport, "registered");
            selector.connected(transport);
        }
        // A refusal is about the credentials, which no transport changes.
        Err(Error::Refused(_)) => {}
        Err(e) if transport == Transport::Quic => {
            let trouble = match e {
                Error::QuicDial { .. } => Trouble::Dial,
                _ => Trouble::Register,
            };
            selector.quic_trouble(trouble, Instant::now());
        }
        Err(_) => {}
    }
    result
}

async fn dial_and_register(
    shared: &Shared,
    transport: Transport,
    conn_index: u8,
    replace_existing: bool,
) -> Result<(Link, ControlSession, String), Error> {
    let edges = edges(shared, transport).await?;
    let mut options = ConnectionOptions::default_for(CLIENT_VERSION);
    options.replace_existing = replace_existing;

    match transport {
        Transport::Quic => {
            let (cap, handshake) = match shared.protocol {
                Protocol::Auto => (AUTO_QUIC_EDGES, AUTO_QUIC_HANDSHAKE),
                _ => (EDGES_PER_DIAL, QUIC_HANDSHAKE),
            };
            let cap = edges.len().min(cap);
            let conn = quic_dial::dial_any(&shared.endpoint, &edges[..cap], handshake).await?;
            let (details, control) = register_connection(
                &conn,
                &shared.auth,
                shared.tunnel_id,
                conn_index,
                &options,
                shared.ingress.clone(),
            )
            .await?;
            Ok((Link::Quic(conn), control, details.location))
        }
        Transport::Http2 => {
            let cap = edges.len().min(EDGES_PER_DIAL);
            let io = http2::dial_any(&shared.h2_tls, &edges[..cap]).await?;
            let (link, reader, writer) = http2::start(io, shared.http2_serve()).await?;
            let registered = register_over(
                reader,
                writer,
                &shared.auth,
                shared.tunnel_id,
                conn_index,
                &options,
                shared.ingress.clone(),
            )
            .await;
            match registered {
                Ok((details, control)) => Ok((Link::Http2(link), control, details.location)),
                Err(e) => {
                    link.close();
                    Err(e)
                }
            }
        }
    }
}

/// Where the edge is. One answer serves both transports: QUIC over UDP and
/// HTTP/2 over TCP listen on the same addresses and port (7844).
async fn edges(shared: &Shared, transport: Transport) -> Result<Vec<EdgeAddr>, Error> {
    match &shared.edge {
        EdgeSource::Discover => {
            let _ = transport;
            discover().await
        }
        #[cfg(test)]
        EdgeSource::Fixed(seam) => Ok(match transport {
            Transport::Quic => seam.edges(),
            Transport::Http2 => seam.tcp_edges(),
        }),
    }
}

async fn reactor_loop(shared: Shared, conn_index: u8, mut conn: Link, mut control: ControlSession) {
    debug!(conn_index, "reactor loop started");
    let mut stop = shared.stop.clone();
    loop {
        let up_since = Instant::now();
        let exit = tokio::select! {
            biased;
            _ = stop.wait_for(|stopped| *stopped) => SupervisorExit::Shutdown,
            exit = conn.serve(&shared) => exit,
        };
        match exit {
            SupervisorExit::Shutdown => {
                // Unregister first, while the connection can still carry the
                // call, so the edge stops routing here before it is gone.
                control.shutdown_graceful(DEFAULT_GRACE_PERIOD).await;
                conn.close();
                debug!(conn_index, "reactor: clean shutdown");
                return;
            }
            SupervisorExit::ConnectionLost | SupervisorExit::TimedOut => {
                let transport = conn.transport();
                warn!(
                    conn_index,
                    %transport,
                    ?exit,
                    up_for = ?up_since.elapsed(),
                    "tunnel connection lost"
                );
                // Only silence counts against QUIC: a connection the edge
                // closed on purpose says nothing about this network.
                if exit == SupervisorExit::TimedOut && transport == Transport::Quic {
                    lock(&shared.selector).quic_trouble(Trouble::TimedOut, Instant::now());
                }
                drop(control);
                conn.close();
                match reconnect(&shared, conn_index, true).await {
                    Some((new_conn, new_control)) => {
                        shared.reconnects.fetch_add(1, Ordering::Relaxed);
                        conn = new_conn;
                        control = new_control;
                    }
                    None => return,
                }
            }
        }
    }
}

/// Retry a registration with backoff until it succeeds, the attempts run out,
/// or the tunnel is stopped — `None` for the last two.
async fn reconnect(
    shared: &Shared,
    conn_index: u8,
    replace_existing: bool,
) -> Option<(Link, ControlSession)> {
    let mut stop = shared.stop.clone();
    for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
        let delay = backoff(attempt);
        warn!(conn_index, attempt, ?delay, "reactor: scheduling reconnect");
        tokio::select! {
            biased;
            _ = stop.wait_for(|stopped| *stopped) => return None,
            _ = tokio::time::sleep(delay) => {}
        }
        match connect_cycle(shared, conn_index, replace_existing).await {
            Ok((conn, control, location)) => {
                info!(conn_index, attempt, %location, "reactor: reconnect succeeded");
                return Some((conn, control));
            }
            Err(e) => warn!(conn_index, attempt, error = %e, "reactor: reconnect failed"),
        }
    }
    warn!(
        conn_index,
        "reactor: giving up after {MAX_RECONNECT_ATTEMPTS} reconnect attempts"
    );
    None
}

/// Exponential backoff with a 30s ceiling: 1s, 2s, 4s, 8s, 16s, 30s, 30s, …
fn backoff(attempt: u32) -> Duration {
    let secs = 1u64.checked_shl(attempt.saturating_sub(1)).unwrap_or(30);
    Duration::from_secs(secs.min(30))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_curve() {
        assert_eq!(backoff(1), Duration::from_secs(1));
        assert_eq!(backoff(2), Duration::from_secs(2));
        assert_eq!(backoff(3), Duration::from_secs(4));
        assert_eq!(backoff(4), Duration::from_secs(8));
        assert_eq!(backoff(5), Duration::from_secs(16));
        assert_eq!(backoff(6), Duration::from_secs(30));
        assert_eq!(backoff(20), Duration::from_secs(30));
    }
}
