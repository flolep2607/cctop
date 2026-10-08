//! Which local port answers which public hostname.
//!
//! One tunnel can carry more than one service: cctop's page on one hostname
//! and rmux's share listener on another, over the same registration. The edge
//! says which hostname a request was for, and this table says where it goes.
//! It is shared between the caller and the tunnel's tasks, so a route added or
//! moved while the tunnel is up is used by the next request — a daemon that
//! comes back on a new port costs a table entry, not a re-registration.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// The routing table of one tunnel. Cheap to clone; clones share the table.
#[derive(Clone)]
pub struct Routes {
    table: Arc<RwLock<Table>>,
}

struct Table {
    /// The port the tunnel's own URL lands on.
    primary: u16,
    /// Whether a hostname nobody named still reaches `primary`. True for a
    /// quick tunnel, whose one hostname is whatever the edge assigned; false
    /// for a named one, where an unknown host is a misconfigured DNS record
    /// and gets a 404 rather than somebody's page.
    any_host: bool,
    by_host: HashMap<String, u16>,
}

impl Routes {
    /// A table whose primary service listens on `primary`. Until a provider
    /// names the hostname, nothing is routed.
    pub fn new(primary: u16) -> Routes {
        Routes {
            table: Arc::new(RwLock::new(Table {
                primary,
                any_host: false,
                by_host: HashMap::new(),
            })),
        }
    }

    /// The port the tunnel's URL lands on.
    pub fn primary(&self) -> u16 {
        self.read(|t| t.primary)
    }

    /// Send `host` to `port`, replacing whatever it went to before.
    pub fn insert(&self, host: &str, port: u16) {
        let host = normalise(host);
        self.write(|t| {
            t.by_host.insert(host, port);
        });
    }

    /// Stop answering `host`.
    pub fn remove(&self, host: &str) {
        let host = normalise(host);
        self.write(|t| {
            t.by_host.remove(&host);
        });
    }

    /// The port for a request whose `Host` was `host`, or `None` for a 404.
    pub fn port_for(&self, host: &str) -> Option<u16> {
        let host = normalise(host);
        self.read(|t| {
            t.by_host
                .get(&host)
                .copied()
                .or(t.any_host.then_some(t.primary))
        })
    }

    /// Route every hostname to the primary port. For a provider whose
    /// hostname is assigned rather than chosen.
    pub fn route_any_host(&self) {
        self.write(|t| t.any_host = true);
    }

    fn read<T>(&self, f: impl FnOnce(&Table) -> T) -> T {
        // A poisoned table is one a panicking writer left mid-insert; the map
        // is still a map, and a tunnel that stops routing over it helps no one.
        f(&self.table.read().unwrap_or_else(|e| e.into_inner()))
    }

    fn write(&self, f: impl FnOnce(&mut Table)) {
        f(&mut self.table.write().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Hostnames compare without case, a port or a trailing dot: `Host:
/// CCTOP.example.com:443` is the same site as `cctop.example.com`.
fn normalise(host: &str) -> String {
    let host = host.trim();
    let host = match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_named_table_answers_only_the_hosts_it_was_given() {
        let routes = Routes::new(7777);
        routes.insert("cctop.example.test", 7777);
        routes.insert("share.example.test", 9000);
        assert_eq!(routes.port_for("cctop.example.test"), Some(7777));
        assert_eq!(routes.port_for("share.example.test"), Some(9000));
        assert_eq!(routes.port_for("other.example.test"), None);
    }

    #[test]
    fn hosts_compare_without_case_port_or_trailing_dot() {
        let routes = Routes::new(1);
        routes.insert("Cctop.Example.Test", 7777);
        assert_eq!(routes.port_for("cctop.example.test:443"), Some(7777));
        assert_eq!(routes.port_for("CCTOP.example.test."), Some(7777));
    }

    #[test]
    fn a_quick_table_sends_any_host_to_the_primary() {
        let routes = Routes::new(7777);
        routes.route_any_host();
        assert_eq!(routes.port_for("whatever.trycloudflare.com"), Some(7777));
    }

    #[test]
    fn a_moved_route_is_used_by_every_clone() {
        let routes = Routes::new(7777);
        let held_by_the_tunnel = routes.clone();
        routes.insert("share.example.test", 9000);
        routes.insert("share.example.test", 9001);
        assert_eq!(
            held_by_the_tunnel.port_for("share.example.test"),
            Some(9001)
        );
        routes.remove("share.example.test");
        assert_eq!(held_by_the_tunnel.port_for("share.example.test"), None);
    }
}
