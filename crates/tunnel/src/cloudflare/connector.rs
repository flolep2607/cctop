//! Registering a tunnel with the edge and keeping it registered.
//!
//! Given credentials — from trycloudflare or from a tunnel token, the edge
//! cannot tell — [`connect`] runs:
//!
//!   1. edge discovery (`edge::discover`);
//!   2. a QUIC dial and `registerConnection` on connection index 0, awaited,
//!      so the caller learns of a refused tunnel before it hands out a URL;
//!   3. one reactor task per HA connection, each owning its QUIC connection:
//!      it runs the supervisor, and reconnects with backoff and
//!      `replace_existing` when the edge drops it, until told to stop.
//!
//! The ingress hostnames the edge pushes to any of those connections land in
//! one shared [`watch`] channel, which is how a dashboard-made tunnel says
//! which name it serves.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use quinn::Endpoint;
use tokio::sync::watch;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::config::{Ingress, IngressTx};
use super::edge::discover;
use super::pool::Pool;
use super::quic_dial::{build_endpoint, dial_any};
use super::rpc::{ConnectionOptions, ControlSession, TunnelAuth, register_connection};
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

/// What every reactor of one tunnel shares.
#[derive(Clone)]
pub(crate) struct Shared {
    endpoint: Endpoint,
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

/// Register `credentials` with the edge on `ha_connections` connections, and
/// proxy what arrives to the ports `routes` names.
pub(crate) async fn connect(
    credentials: &Credentials,
    routes: Routes,
    ha_connections: u8,
    edge: &EdgeSource,
) -> Result<Connected, Error> {
    let endpoint = build_endpoint(edge)?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let (ingress_tx, ingress_rx) = watch::channel(Ingress::default());
    let shared = Shared {
        endpoint,
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
    // edge accepts these credentials at all.
    let (conn0, control0, location0) = connect_cycle(&shared, 0, false).await?;
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

/// Dial the next edge and register on `conn_index`. `replace_existing` on a
/// reconnect, so the edge takes the new connection in place of the one it
/// lost; not on a leg's first registration.
async fn connect_cycle(
    shared: &Shared,
    conn_index: u8,
    replace_existing: bool,
) -> Result<(quinn::Connection, ControlSession, String), Error> {
    let edges = match &shared.edge {
        EdgeSource::Discover => discover().await?,
        #[cfg(test)]
        EdgeSource::Fixed(seam) => seam.edges(),
    };
    let cap = edges.len().min(5);
    let conn = dial_any(&shared.endpoint, &edges[..cap]).await?;

    let mut options = ConnectionOptions::default_for(CLIENT_VERSION);
    options.replace_existing = replace_existing;

    let (details, control) = register_connection(
        &conn,
        &shared.auth,
        shared.tunnel_id,
        conn_index,
        &options,
        shared.ingress.clone(),
    )
    .await?;
    Ok((conn, control, details.location))
}

async fn reactor_loop(
    shared: Shared,
    conn_index: u8,
    mut conn: quinn::Connection,
    mut control: ControlSession,
) {
    debug!(conn_index, "reactor loop started");
    let mut stop = shared.stop.clone();
    loop {
        let exit = tokio::select! {
            biased;
            _ = stop.wait_for(|stopped| *stopped) => SupervisorExit::Shutdown,
            exit = supervisor::run(
                conn.clone(),
                shared.routes.clone(),
                shared.metrics.clone(),
                shared.pool.clone(),
                shared.ingress.clone(),
            ) => exit,
        };
        match exit {
            SupervisorExit::Shutdown => {
                // Unregister first, while the connection can still carry the
                // call, so the edge stops routing here before it is gone; then
                // close with a code rather than leave the edge to time it out.
                control.shutdown_graceful(DEFAULT_GRACE_PERIOD).await;
                conn.close(0u32.into(), b"client shutdown");
                debug!(conn_index, "reactor: clean shutdown");
                return;
            }
            SupervisorExit::ConnectionLost => {
                drop(control);
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
) -> Option<(quinn::Connection, ControlSession)> {
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
