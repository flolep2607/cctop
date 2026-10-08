//! A fake argotunnel edge, for tests that must not reach Cloudflare.
//!
//! A QUIC server on loopback presenting a self-signed certificate for the
//! edge's SNI (`cert.pem`/`key.pem` here: made for these tests, trusted by
//! nothing else, valid for a century). The client under test is pointed at it
//! through [`Seam`], which replaces edge discovery and the trust roots — and
//! exists only under `cfg(test)`, so no build of cctop can be pointed at an
//! "edge" that would be handed the tunnel's secret.
//!
//! It plays the edge's side of what the client speaks: answers
//! `registerConnection` (recording what it was sent), and from the test's
//! side pushes a configuration and opens request streams, as the real edge
//! does when a browser asks for the tunnel's hostname.
//!
//! Every credential it sees is made up by the test that started it.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use capnp::capability::Promise;
use capnp::message::ReaderOptions;
use capnp_futures::serialize;
use capnp_rpc::{RpcSystem, pry, rpc_twoparty_capnp, twoparty};
use futures::{AsyncReadExt, AsyncWriteExt};
use rustls::pki_types::CertificateDer;
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use super::edge::EdgeAddr;
use super::stream::{DATA_STREAM_SIGNATURE, PROTOCOL_V1, RPC_STREAM_SIGNATURE};
use crate::quic_metadata_protocol_capnp::{ConnectionType, connect_request, connect_response};
use crate::tunnelrpc_capnp::{configuration_manager, registration_server};

const CERT: &[u8] = include_bytes!("cert.pem");
const KEY: &[u8] = include_bytes!("key.pem");

/// Where the client under test finds the edge, and the one root it trusts.
#[derive(Clone)]
pub(crate) struct Seam {
    addr: SocketAddr,
}

impl Seam {
    pub(crate) fn edges(&self) -> Vec<EdgeAddr> {
        vec![EdgeAddr {
            ip: self.addr.ip(),
            port: self.addr.port(),
        }]
    }

    pub(crate) fn root(&self) -> CertificateDer<'static> {
        certificate()
    }
}

fn certificate() -> CertificateDer<'static> {
    rustls_pemfile::certs(&mut &CERT[..])
        .next()
        .expect("a certificate in cert.pem")
        .expect("cert.pem parses")
}

/// What one `registerConnection` carried.
#[derive(Clone, Debug)]
pub(crate) struct Registration {
    pub account_tag: String,
    pub tunnel_id: Vec<u8>,
    pub secret: Vec<u8>,
    pub conn_index: u8,
}

/// How the fake answers a registration.
#[derive(Clone, Copy)]
pub(crate) enum Answer {
    Accept,
    /// As for a deleted tunnel: an error the edge says not to retry.
    Refuse,
}

pub(crate) struct FakeEdge {
    seam: Seam,
    pub registrations: Arc<Mutex<Vec<Registration>>>,
    connections: mpsc::UnboundedReceiver<quinn::Connection>,
    _endpoint: quinn::Endpoint,
}

impl FakeEdge {
    pub(crate) fn start(answer: Answer) -> FakeEdge {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let key = rustls_pemfile::private_key(&mut &KEY[..])
            .expect("key.pem parses")
            .expect("a key in key.pem");
        let mut tls = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate()], key)
            .expect("a usable test certificate");
        tls.alpn_protocols = vec![super::quic_dial::ALPN.to_vec()];
        let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls).unwrap();
        let config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
        let endpoint =
            quinn::Endpoint::server(config, "127.0.0.1:0".parse().unwrap()).expect("bind");
        let addr = endpoint.local_addr().unwrap();

        let registrations = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::unbounded_channel();
        let accepting = endpoint.clone();
        let recorded = registrations.clone();
        tokio::spawn(async move {
            while let Some(incoming) = accepting.accept().await {
                let Ok(conn) = incoming.await else { continue };
                let Ok((send, recv)) = conn.accept_bi().await else {
                    continue;
                };
                // The control stream's RPC system is `!Send`, so it gets a
                // thread of its own, as on the client's side.
                let recorded = recorded.clone();
                let (registered_tx, registered_rx) = tokio::sync::oneshot::channel();
                std::thread::spawn(move || {
                    serve_control(send, recv, recorded, answer, registered_tx)
                });
                if registered_rx.await.is_ok() {
                    let _ = tx.send(conn);
                }
            }
        });
        FakeEdge {
            seam: Seam { addr },
            registrations,
            connections: rx,
            _endpoint: endpoint,
        }
    }

    pub(crate) fn seam(&self) -> Seam {
        self.seam.clone()
    }

    /// The next connection that registered, to send requests down.
    pub(crate) async fn registered(&mut self) -> quinn::Connection {
        tokio::time::timeout(Duration::from_secs(10), self.connections.recv())
            .await
            .expect("a registration within ten seconds")
            .expect("the edge is still accepting")
    }
}

struct Registrar {
    recorded: Arc<Mutex<Vec<Registration>>>,
    answer: Answer,
    registered: Option<tokio::sync::oneshot::Sender<()>>,
}

impl registration_server::Server for Registrar {
    fn register_connection(
        &mut self,
        params: registration_server::RegisterConnectionParams,
        mut results: registration_server::RegisterConnectionResults,
    ) -> Promise<(), capnp::Error> {
        let params = pry!(params.get());
        let auth = pry!(params.get_auth());
        self.recorded.lock().unwrap().push(Registration {
            account_tag: pry!(pry!(auth.get_account_tag()).to_string()),
            tunnel_id: pry!(params.get_tunnel_id()).to_vec(),
            secret: pry!(auth.get_tunnel_secret()).to_vec(),
            conn_index: params.get_conn_index(),
        });
        let result = results.get().init_result().init_result();
        match self.answer {
            Answer::Accept => {
                let mut details = result.init_connection_details();
                details.set_uuid(&[7u8; 16]);
                details.set_location_name("tst01");
                details.set_tunnel_is_remotely_managed(true);
                if let Some(tx) = self.registered.take() {
                    let _ = tx.send(());
                }
            }
            Answer::Refuse => {
                let mut error = result.init_error();
                error.set_cause("Unauthorized: Tunnel not found");
                error.set_should_retry(false);
            }
        }
        Promise::ok(())
    }

    fn unregister_connection(
        &mut self,
        _: registration_server::UnregisterConnectionParams,
        _: registration_server::UnregisterConnectionResults,
    ) -> Promise<(), capnp::Error> {
        Promise::ok(())
    }
}

fn serve_control(
    send: quinn::SendStream,
    recv: quinn::RecvStream,
    recorded: Arc<Mutex<Vec<Registration>>>,
    answer: Answer,
    registered: tokio::sync::oneshot::Sender<()>,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, async move {
        let network = Box::new(twoparty::VatNetwork::new(
            recv.compat(),
            send.compat_write(),
            rpc_twoparty_capnp::Side::Server,
            Default::default(),
        ));
        let registrar: registration_server::Client = capnp_rpc::new_client(Registrar {
            recorded,
            answer,
            registered: Some(registered),
        });
        let _ = RpcSystem::new(network, Some(registrar.client)).await;
    });
}

/// Push a configuration to the connector, as the edge does for a
/// dashboard-managed tunnel, and return the version it says it applied.
pub(crate) async fn push_config(conn: &quinn::Connection, version: i32, config: &str) -> i32 {
    let (mut send, recv) = conn.open_bi().await.unwrap();
    send.write_all(&RPC_STREAM_SIGNATURE).await.unwrap();
    send.write_all(&PROTOCOL_V1).await.unwrap();
    let config = config.as_bytes().to_vec();
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let local = tokio::task::LocalSet::new();
        local.block_on(&rt, async move {
            let network = Box::new(twoparty::VatNetwork::new(
                recv.compat(),
                send.compat_write(),
                rpc_twoparty_capnp::Side::Client,
                Default::default(),
            ));
            let mut system = RpcSystem::new(network, None);
            let manager: configuration_manager::Client =
                system.bootstrap(rpc_twoparty_capnp::Side::Server);
            tokio::task::spawn_local(system);
            let mut request = manager.update_configuration_request();
            request.get().set_version(version);
            request.get().set_config(&config);
            let reply = request.send().promise.await.unwrap();
            let applied = reply
                .get()
                .unwrap()
                .get_result()
                .unwrap()
                .get_latest_applied_version();
            let _ = tx.send(applied);
        });
    });
    tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("an answer to the push")
        .unwrap()
}

/// One HTTP GET down the tunnel, as the edge forwards a browser's: the status
/// and the body the connector sent back.
pub(crate) async fn get(conn: &quinn::Connection, host: &str, path: &str) -> (u16, String) {
    let (send, recv) = conn.open_bi().await.unwrap();
    let mut send = send.compat_write();
    let mut recv = recv.compat();
    send.write_all(&DATA_STREAM_SIGNATURE).await.unwrap();
    send.write_all(&PROTOCOL_V1).await.unwrap();
    let mut message = capnp::message::Builder::new_default();
    {
        let mut request: connect_request::Builder = message.init_root();
        request.set_dest(format!("https://{host}{path}").as_str());
        request.set_type(ConnectionType::Http);
        let mut meta = request.init_metadata(2);
        meta.reborrow().get(0).set_key("HttpMethod");
        meta.reborrow().get(0).set_val("GET");
        meta.reborrow().get(1).set_key("HttpHost");
        meta.reborrow().get(1).set_val(host);
    }
    serialize::write_message(&mut send, &message).await.unwrap();
    send.flush().await.unwrap();
    send.close().await.unwrap();

    let mut preamble = [0u8; 8];
    recv.read_exact(&mut preamble).await.unwrap();
    assert_eq!(preamble[..6], DATA_STREAM_SIGNATURE);
    let reply = serialize::read_message(&mut recv, ReaderOptions::new())
        .await
        .unwrap();
    let response: connect_response::Reader = reply.get_root().unwrap();
    assert_eq!(response.get_error().unwrap().to_str().unwrap(), "");
    let mut status = 0;
    for entry in response.get_metadata().unwrap() {
        if entry.get_key().unwrap().to_str().unwrap() == "HttpStatus" {
            status = entry.get_val().unwrap().to_str().unwrap().parse().unwrap();
        }
    }
    let mut body = String::new();
    recv.read_to_string(&mut body).await.unwrap();
    (status, body)
}

/// A local origin that answers every request with `body`, as cctop's server
/// or rmux's listener would.
pub(crate) fn origin(body: &'static str) -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let mut seen = Vec::new();
                // Keep-alive: answer each request head on the socket until
                // the pool lets it go.
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => seen.extend_from_slice(&buf[..n]),
                    }
                    while let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                        seen.drain(..end + 4);
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        );
                    }
                }
            });
        }
    });
    port
}

#[cfg(test)]
mod tests {
    use base64::Engine;

    use super::*;
    use crate::cloudflare::{Credentials, Named, Quick};
    use crate::{Error, Provider, Routes};

    const TUNNEL: &str = "6ff42ae2-765d-4adf-8112-31c55c1551ef";

    /// A tunnel token written here, naming nobody's tunnel.
    fn made_up_credentials() -> Credentials {
        let secret =
            base64::engine::general_purpose::STANDARD.encode(b"a made-up tunnel secret, 32 b.");
        let json = format!(r#"{{"a":"made-up-account","t":"{TUNNEL}","s":"{secret}"}}"#);
        Credentials::from_token(&base64::engine::general_purpose::STANDARD.encode(json)).unwrap()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn registration_carries_the_tokens_credentials() {
        let mut edge = FakeEdge::start(Answer::Accept);
        let serve = origin("the page");
        let named = Named::new(made_up_credentials(), Some("cctop.example.test".into()))
            .against(edge.seam());
        let tunnel = named.open(Routes::new(serve)).await.unwrap();
        assert_eq!(tunnel.url(), "https://cctop.example.test");

        let conn = edge.registered().await;
        let seen = edge.registrations.lock().unwrap().clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].account_tag, "made-up-account");
        assert_eq!(
            seen[0].tunnel_id,
            uuid::Uuid::parse_str(TUNNEL).unwrap().as_bytes()
        );
        assert_eq!(seen[0].secret, b"a made-up tunnel secret, 32 b.");
        assert_eq!(seen[0].conn_index, 0);

        assert_eq!(
            get(&conn, "cctop.example.test", "/").await,
            (200, "the page".into())
        );
        tunnel.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_pushed_configuration_is_acknowledged_and_names_the_hostname() {
        let mut edge = FakeEdge::start(Answer::Accept);
        let serve = origin("the page");
        let named = Named::new(made_up_credentials(), None).against(edge.seam());
        let opening = tokio::spawn(async move { named.open(Routes::new(serve)).await });

        let conn = edge.registered().await;
        let pushed = r#"{"ingress":[
            {"hostname":"cctop.example.test","service":"http://localhost:7777"},
            {"service":"http_status:404"}
        ]}"#;
        assert_eq!(push_config(&conn, 3, pushed).await, 3);
        // An older version is acknowledged with the one in force, not applied.
        assert_eq!(push_config(&conn, 2, r#"{"ingress":[]}"#).await, 3);

        let tunnel = opening.await.unwrap().unwrap();
        assert_eq!(tunnel.url(), "https://cctop.example.test");
        assert_eq!(
            get(&conn, "cctop.example.test", "/").await,
            (200, "the page".into())
        );
        tunnel.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn requests_are_routed_by_hostname_and_an_unknown_one_is_a_404() {
        let mut edge = FakeEdge::start(Answer::Accept);
        let (serve, share) = (origin("the page"), origin("the share"));
        let named = Named::new(made_up_credentials(), Some("cctop.example.test".into()))
            .against(edge.seam());
        let tunnel = named.open(Routes::new(serve)).await.unwrap();
        tunnel.routes().insert("share.example.test", share);
        let conn = edge.registered().await;

        assert_eq!(
            get(&conn, "cctop.example.test", "/").await,
            (200, "the page".into())
        );
        assert_eq!(
            get(&conn, "share.example.test", "/").await,
            (200, "the share".into())
        );
        assert_eq!(get(&conn, "elsewhere.example.test", "/").await.0, 404);
        tunnel.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refused_registration_is_told_apart_from_a_failed_one() {
        let edge = FakeEdge::start(Answer::Refuse);
        let named = Named::new(made_up_credentials(), Some("cctop.example.test".into()))
            .against(edge.seam());
        let err = named.open(Routes::new(1)).await.err().expect("refused");
        assert!(matches!(err, Error::Refused(_)), "{err:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_quick_tunnel_takes_its_credentials_from_the_api_and_any_host() {
        let mut edge = FakeEdge::start(Answer::Accept);
        let (api, _) = super::super::api::tests::fake_api(vec![(
            200,
            super::super::api::tests::sample_ok_body(),
        )]);
        let serve = origin("the page");
        let tunnel = Quick::against(edge.seam(), api)
            .open(Routes::new(serve))
            .await
            .unwrap();
        assert_eq!(tunnel.url(), "https://abc-123.trycloudflare.com");
        let conn = edge.registered().await;
        assert_eq!(
            edge.registrations.lock().unwrap()[0].account_tag,
            "deadbeefcafef00d"
        );
        assert_eq!(
            get(&conn, "abc-123.trycloudflare.com", "/x").await,
            (200, "the page".into())
        );
        tunnel.shutdown().await;
    }
}
