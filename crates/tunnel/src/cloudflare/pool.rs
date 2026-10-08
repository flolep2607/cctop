//! Bounded idle-TCP-connection pools against `127.0.0.1:<port>`, one
//! per local port a tunnel routes to.
//!
//! Reduces socket() + connect() overhead per inbound stream by
//! reusing keep-alive connections to the local origin. Each pooled
//! entry carries a release timestamp; entries older than `idle_ttl`
//! are reaped on acquire so we never hand back a connection the
//! origin has already half-closed by idle timeout.
//!
//! The pool only stores connections that are known to be in a
//! healthy "ready for next request" state — the proxy must
//! call [`Pool::release`] **only** after fully reading a response
//! whose framing it understood (Content-Length-bounded).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::trace;

/// Idle socket TTL — most servers (e.g. axum, hyper, nginx) idle
/// out keep-alive connections at 60-75s; 30s gives us a safe
/// buffer and matches the typical client-side default.
pub const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(30);

/// Soft cap on idle sockets per port. Beyond this we drop the
/// freshly-released socket on the floor; the next acquire will
/// open a new one if needed.
pub const DEFAULT_MAX_IDLE: usize = 16;

struct Idle {
    stream: TcpStream,
    released_at: Instant,
}

pub struct Pool {
    idle_ttl: Duration,
    max_idle: usize,
    idle: Mutex<HashMap<u16, Vec<Idle>>>,
}

impl Pool {
    pub fn new() -> Self {
        Self {
            idle_ttl: DEFAULT_IDLE_TTL,
            max_idle: DEFAULT_MAX_IDLE,
            idle: Mutex::new(HashMap::new()),
        }
    }

    /// Acquire a socket: pop a fresh idle entry if available,
    /// otherwise open a new TCP connection.
    pub async fn acquire(&self, port: u16) -> std::io::Result<TcpStream> {
        // Drain stale entries up-front so callers never see a
        // stream older than `idle_ttl`. Locking inside the loop
        // gives the reaper write-access without blocking other
        // acquires for the connect path.
        {
            let mut all = self.idle.lock().await;
            let g = all.entry(port).or_default();
            while let Some(entry) = g.last() {
                if entry.released_at.elapsed() <= self.idle_ttl {
                    let entry = g.pop().expect("checked not-empty");
                    trace!(port, pool_size = g.len(), "pool hit");
                    return Ok(entry.stream);
                }
                g.pop();
            }
        }
        trace!(port, "pool miss; opening fresh TCP");
        TcpStream::connect(("127.0.0.1", port)).await
    }

    /// Return a socket to the pool. Drop on overflow.
    pub async fn release(&self, port: u16, stream: TcpStream) {
        let mut all = self.idle.lock().await;
        let g = all.entry(port).or_default();
        if g.len() >= self.max_idle {
            trace!(port, "pool full; dropping released stream");
            return;
        }
        g.push(Idle {
            stream,
            released_at: Instant::now(),
        });
        trace!(port, pool_size = g.len(), "pool released");
    }
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    /// Pool hits should reuse the same socket.
    #[tokio::test]
    async fn acquire_after_release_returns_same_stream() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut s, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let _ = s.write_all(b"ping").await;
                });
            }
        });

        let pool = Pool::new();
        let s1 = pool.acquire(port).await.unwrap();
        let s1_local = s1.local_addr().unwrap();
        pool.release(port, s1).await;

        let s2 = pool.acquire(port).await.unwrap();
        assert_eq!(s2.local_addr().unwrap(), s1_local, "should reuse socket");
    }

    #[tokio::test]
    async fn pool_evicts_stale_entries() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let _ = listener.accept().await;
            }
        });
        let mut pool = Pool::new();
        pool.idle_ttl = Duration::from_millis(50);

        let s1 = pool.acquire(port).await.unwrap();
        let s1_local = s1.local_addr().unwrap();
        pool.release(port, s1).await;

        tokio::time::sleep(Duration::from_millis(100)).await;
        let s2 = pool.acquire(port).await.unwrap();
        assert_ne!(
            s2.local_addr().unwrap(),
            s1_local,
            "stale entry should have been evicted"
        );
    }
}
