//! The accept loop of one registered QUIC connection.
//!
//! After registration the edge opens a bidirectional stream for everything it
//! wants of us, and the first six bytes say which kind:
//!
//! - a **data** stream is one request from the internet, handed to
//!   [`proxy::handle_inbound_stream`];
//! - an **RPC** stream is the edge calling cloudflared's own interface — for a
//!   dashboard-managed tunnel, `updateConfiguration` with the ingress rules —
//!   handed to [`config::serve`].
//!
//! Returns when the connection closes, with a [`SupervisorExit`] so the
//! reactor can tell a stop it asked for from a drop to reconnect after. A
//! stop it asked for never reaches here: the reactor drops this future.
//!
//! QUIC keep-alive (`keep_alive_interval = 1s` in `quic_dial`) keeps the
//! connection itself alive; without it the edge's 5 s idle timeout would end
//! it once the control stream went quiet.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use futures::AsyncReadExt;
use tracing::{debug, info, warn};

use super::config::{self, IngressTx};
use super::pool::Pool;
use super::proxy::{self, StreamCounters};
use super::stream::{self, DATA_STREAM_SIGNATURE, RPC_STREAM_SIGNATURE};
use crate::Routes;

#[derive(Debug, Default, Clone)]
pub struct SupervisorMetrics {
    pub streams_total: Arc<AtomicU64>,
    pub bytes_in: Arc<AtomicU64>,
    pub bytes_out: Arc<AtomicU64>,
}

impl SupervisorMetrics {
    fn stream_counters(&self) -> StreamCounters {
        StreamCounters {
            bytes_in: self.bytes_in.clone(),
            bytes_out: self.bytes_out.clone(),
        }
    }
}

/// Why the accept loop returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorExit {
    /// The tunnel was asked to stop.
    Shutdown,
    /// The edge or the local stack closed the connection. Reconnect.
    ConnectionLost,
}

pub async fn run(
    conn: quinn::Connection,
    routes: Routes,
    metrics: SupervisorMetrics,
    pool: Arc<Pool>,
    ingress: IngressTx,
) -> SupervisorExit {
    info!("tunnel supervisor running");
    let exit = loop {
        match conn.accept_bi().await {
            Ok((send, recv)) => {
                metrics.streams_total.fetch_add(1, Ordering::Relaxed);
                let counters = metrics.stream_counters();
                let (pool, routes, ingress) = (pool.clone(), routes.clone(), ingress.clone());
                tokio::spawn(async move {
                    let (mut reader, writer) = stream::split(send, recv);
                    let mut signature = [0u8; 6];
                    if let Err(e) = reader.read_exact(&mut signature).await {
                        warn!(error = %e, "stream closed before its signature");
                        return;
                    }
                    let result = match signature {
                        DATA_STREAM_SIGNATURE => {
                            proxy::handle_inbound_stream(&routes, reader, writer, counters, pool)
                                .await
                        }
                        RPC_STREAM_SIGNATURE => config::serve(reader, writer, ingress).await,
                        other => {
                            warn!(signature = ?other, "unknown stream kind; dropped");
                            Ok(())
                        }
                    };
                    if let Err(e) = result {
                        warn!(error = %e, "stream failed");
                    }
                });
            }
            Err(e) => {
                debug!(error = %e, "accept_bi ended; connection lost");
                break SupervisorExit::ConnectionLost;
            }
        }
    };
    info!(?exit, "tunnel supervisor exited");
    exit
}
