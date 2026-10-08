//! Answering the edge's configuration push.
//!
//! A tunnel made in the dashboard is "remotely managed": its ingress rules —
//! which hostname goes to which service — live at Cloudflare, and the edge
//! pushes them to a connector by calling `updateConfiguration` on
//! cloudflared's own interface, on a stream it opens with the RPC signature.
//! cloudflared answers with the version it applied. Whether the edge marks a
//! connector unhealthy that answers `unimplemented` instead is not known, so
//! this answers properly rather than find out.
//!
//! What is applied is only the hostnames: they are how a tunnel pasted as a
//! bare token learns which name it serves. The rules' `service` entries are
//! not obeyed — this process decides where a request goes ([`crate::Routes`]),
//! and a rule pointing somewhere else on this machine is not a thing a token
//! holder should be able to arrange from the dashboard.

use std::sync::Arc;
use std::time::Duration;

use capnp::capability::Promise;
use capnp_rpc::{RpcSystem, pry, rpc_twoparty_capnp, twoparty};
use serde::Deserialize;
use tokio::sync::{oneshot, watch};
use tracing::{debug, info, warn};

use super::stream;
use crate::Error;
use crate::tunnelrpc_capnp::{cloudflared_server, configuration_manager, session_manager};

/// How long one RPC stream may stay open. The edge makes its call and closes
/// it; a stream that does neither should not keep a thread forever.
const RPC_STREAM_DEADLINE: Duration = Duration::from_secs(60);

/// The configuration in force: the hostnames of the last push applied, and
/// its version. One per tunnel, shared by every connection and every RPC
/// stream, so a repeated or older push on any of them is acknowledged without
/// being applied over a newer one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ingress {
    pub version: i32,
    pub hostnames: Vec<String>,
}

impl Default for Ingress {
    fn default() -> Self {
        // Below any version the edge sends, as cloudflared starts.
        Ingress {
            version: -1,
            hostnames: Vec::new(),
        }
    }
}

pub(crate) type IngressTx = Arc<watch::Sender<Ingress>>;

/// cloudflared's interface, as far as this client implements it: the
/// configuration push. UDP sessions are left `unimplemented`, as an HTTP-only
/// client has none to offer.
pub(crate) struct ConfigServer {
    ingress: IngressTx,
}

impl ConfigServer {
    pub(crate) fn new(ingress: IngressTx) -> ConfigServer {
        ConfigServer { ingress }
    }
}

impl session_manager::Server for ConfigServer {}
impl cloudflared_server::Server for ConfigServer {}

impl configuration_manager::Server for ConfigServer {
    fn update_configuration(
        &mut self,
        params: configuration_manager::UpdateConfigurationParams,
        mut results: configuration_manager::UpdateConfigurationResults,
    ) -> Promise<(), capnp::Error> {
        let params = pry!(params.get());
        let version = params.get_version();
        let config = pry!(params.get_config());
        let mut result = results.get().init_result();
        let applied = self.ingress.borrow().version;
        if version <= applied {
            result.set_latest_applied_version(applied);
            return Promise::ok(());
        }
        match hostnames(config) {
            Ok(hostnames) => {
                info!(version, count = hostnames.len(), "applied pushed ingress");
                self.ingress.send_replace(Ingress { version, hostnames });
                result.set_latest_applied_version(version);
            }
            Err(why) => {
                warn!(version, %why, "pushed configuration not applied");
                result.set_latest_applied_version(applied);
                result.set_err(why.as_str());
            }
        }
        Promise::ok(())
    }
}

#[derive(Deserialize)]
struct Pushed {
    #[serde(default)]
    ingress: Vec<Rule>,
}

#[derive(Deserialize)]
struct Rule {
    #[serde(default)]
    hostname: Option<String>,
}

/// The hostnames of a pushed configuration's ingress rules, in their order,
/// once each. The catch-all rule has none and is skipped.
fn hostnames(config: &[u8]) -> Result<Vec<String>, String> {
    let pushed: Pushed =
        serde_json::from_slice(config).map_err(|e| format!("configuration is not JSON: {e}"))?;
    let mut hosts: Vec<String> = Vec::new();
    for rule in pushed.ingress {
        if let Some(host) = rule.hostname.map(|h| h.trim().to_ascii_lowercase())
            && !host.is_empty()
            && !hosts.contains(&host)
        {
            hosts.push(host);
        }
    }
    Ok(hosts)
}

/// Serve cloudflared's interface on an RPC stream whose signature has been
/// read, until the edge closes it.
///
/// capnp-rpc's system is `!Send`, so it runs on a thread of its own with a
/// single-threaded runtime, as the control stream's does in `rpc`.
pub(crate) async fn serve<R, W>(mut reader: R, writer: W, ingress: IngressTx) -> Result<(), Error>
where
    R: futures::io::AsyncRead + Unpin + Send + 'static,
    W: futures::io::AsyncWrite + Unpin + Send + 'static,
{
    stream::read_version(&mut reader).await?;
    let (done_tx, done_rx) = oneshot::channel::<()>();
    std::thread::Builder::new()
        .name("cctop-tunnel-config".into())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                let network = Box::new(twoparty::VatNetwork::new(
                    reader,
                    writer,
                    rpc_twoparty_capnp::Side::Server,
                    Default::default(),
                ));
                let server: cloudflared_server::Client =
                    capnp_rpc::new_client(ConfigServer::new(ingress));
                let system = RpcSystem::new(network, Some(server.client));
                if tokio::time::timeout(RPC_STREAM_DEADLINE, system)
                    .await
                    .is_err()
                {
                    debug!("RPC stream outlived its deadline; closed");
                }
            });
            let _ = done_tx.send(());
        })
        .map_err(|e| Error::Internal(format!("spawn config thread: {e}")))?;
    let _ = done_rx.await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hostnames_of_the_ingress_rules_in_order_once_each() {
        let pushed = br#"{
            "ingress": [
                {"hostname": "cctop.example.test", "service": "http://localhost:7777"},
                {"hostname": "Share.Example.Test", "service": "http://localhost:9000"},
                {"hostname": "cctop.example.test", "path": "/x", "service": "http://localhost:1"},
                {"service": "http_status:404"}
            ],
            "warp-routing": {"enabled": false}
        }"#;
        assert_eq!(
            hostnames(pushed).unwrap(),
            ["cctop.example.test", "share.example.test"]
        );
    }

    #[test]
    fn a_configuration_without_ingress_names_nothing() {
        assert!(hostnames(br#"{"warp-routing":{}}"#).unwrap().is_empty());
        assert!(hostnames(b"not json").is_err());
    }
}
