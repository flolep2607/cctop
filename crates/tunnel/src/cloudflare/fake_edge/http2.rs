//! The fake edge over HTTP/2: TLS on loopback TCP, then the edge's side of
//! cloudflared's `http2` protocol — the HTTP/2 *client*, opening the control
//! stream and then a request per browser request (see `cloudflare::http2`).
//!
//! Beside it, a UDP socket that is bound and never answers, so that a QUIC
//! dial to this seam times out the way it does on a network that drops UDP.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures::{AsyncReadExt, AsyncWriteExt};
use h2::RecvStream;
use h2::client::SendRequest;
use http::{Request, Response};
use tokio::sync::mpsc;

use super::{Answer, Registration, Seam, serve_control, server_tls};
use crate::cloudflare::http2::{
    H2In, H2Out, RESPONSE_META, RESPONSE_USER_HEADERS, UPGRADE_CONFIGURATION, UPGRADE_CONTROL,
    UPGRADE_HEADER, UPGRADE_WEBSOCKET, deserialize_headers,
};

pub(crate) struct FakeH2Edge {
    seam: Seam,
    pub registrations: Arc<Mutex<Vec<Registration>>>,
    connections: mpsc::UnboundedReceiver<SendRequest<Bytes>>,
    _silent_udp: std::net::UdpSocket,
}

impl FakeH2Edge {
    pub(crate) fn start(answer: Answer) -> FakeH2Edge {
        let tls = Arc::new(server_tls(&[]));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let tcp = listener.local_addr().unwrap();
        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
        let silent = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp: SocketAddr = silent.local_addr().unwrap();

        let registrations = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::unbounded_channel();
        let recorded = registrations.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let acceptor = tokio_rustls::TlsAcceptor::from(tls.clone());
                let Ok(stream) = acceptor.accept(stream).await else {
                    continue;
                };
                let Ok((send, connection)) = h2::client::handshake(stream).await else {
                    continue;
                };
                tokio::spawn(connection);
                let request = Request::builder()
                    .method("GET")
                    .uri("https://h2.cftunnel.com/")
                    .header(UPGRADE_HEADER, UPGRADE_CONTROL)
                    .body(())
                    .unwrap();
                let Ok((response, body)) = open(&send, request, true).await else {
                    continue;
                };
                let reader = H2In::new(response.into_body());
                let writer = H2Out::body(body.expect("a request body to write"));
                let recorded = recorded.clone();
                let (registered_tx, registered_rx) = tokio::sync::oneshot::channel();
                std::thread::spawn(move || {
                    serve_control(reader, writer, recorded, answer, registered_tx)
                });
                if registered_rx.await.is_ok() {
                    let _ = tx.send(send);
                }
            }
        });
        FakeH2Edge {
            seam: Seam { addr: udp, tcp },
            registrations,
            connections: rx,
            _silent_udp: silent,
        }
    }

    pub(crate) fn seam(&self) -> Seam {
        self.seam.clone()
    }

    /// The next connection that registered, to send requests down.
    pub(crate) async fn registered(&mut self) -> SendRequest<Bytes> {
        tokio::time::timeout(Duration::from_secs(10), self.connections.recv())
            .await
            .expect("a registration within ten seconds")
            .expect("the edge is still accepting")
    }
}

/// Send `request`, and wait for the response head. With `open_body`, the
/// request body stays open for the caller to write.
async fn open(
    send: &SendRequest<Bytes>,
    request: Request<()>,
    open_body: bool,
) -> Result<(Response<RecvStream>, Option<h2::SendStream<Bytes>>), h2::Error> {
    let mut send = send.clone().ready().await?;
    let (response, body) = send.send_request(request, !open_body)?;
    let response = response.await?;
    Ok((response, open_body.then_some(body)))
}

/// What came back for one request.
pub(crate) struct Answered {
    pub status: u16,
    pub meta: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// One request with no body down the tunnel, as the edge forwards a
/// browser's.
pub(crate) async fn fetch(conn: &SendRequest<Bytes>, request: Request<()>) -> Answered {
    let (response, _) = open(conn, request, false).await.unwrap();
    answered(response).await
}

/// As [`fetch`], for a request with a body: the body is written before the
/// response head is awaited, which an origin that reads it all first needs.
pub(crate) async fn fetch_with_body(
    conn: &SendRequest<Bytes>,
    request: Request<()>,
    body: &[u8],
) -> Answered {
    let mut send = conn.clone().ready().await.unwrap();
    let (response, sink) = send.send_request(request, false).unwrap();
    let mut writer = H2Out::body(sink);
    writer.write_all(body).await.unwrap();
    writer.close().await.unwrap();
    answered(response.await.unwrap()).await
}

async fn answered(response: Response<RecvStream>) -> Answered {
    let status = response.status().as_u16();
    let meta = response
        .headers()
        .get(RESPONSE_META)
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    let headers = response
        .headers()
        .get(RESPONSE_USER_HEADERS)
        .map(|v| deserialize_headers(v.to_str().unwrap()))
        .unwrap_or_default();
    let mut body = String::new();
    H2In::new(response.into_body())
        .read_to_string(&mut body)
        .await
        .unwrap();
    Answered {
        status,
        meta,
        headers,
        body,
    }
}

pub(crate) fn get_request(host: &str, path: &str) -> Request<()> {
    Request::builder()
        .method("GET")
        .uri(format!("https://{host}{path}"))
        .body(())
        .unwrap()
}

/// Push a configuration as the edge does over HTTP/2, and return the version
/// the connector says it applied.
pub(crate) async fn push_config(conn: &SendRequest<Bytes>, version: i32, config: &str) -> i32 {
    let request = Request::builder()
        .method("PUT")
        .uri("https://h2.cftunnel.com/config")
        .header(UPGRADE_HEADER, UPGRADE_CONFIGURATION)
        .body(())
        .unwrap();
    let body = format!(r#"{{"version":{version},"config":{config}}}"#);
    let answered = fetch_with_body(conn, request, body.as_bytes()).await;
    assert_eq!(answered.status, 200);
    let reply: serde_json::Value = serde_json::from_str(&answered.body).unwrap();
    reply["lastAppliedVersion"].as_i64().unwrap() as i32
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use base64::Engine;

    use super::*;
    use crate::cloudflare::fake_edge::origin;
    use crate::cloudflare::{Credentials, Named, Protocol, Quick};
    use crate::{Error, Provider, Routes};

    const TUNNEL: &str = "6ff42ae2-765d-4adf-8112-31c55c1551ef";

    fn made_up_credentials() -> Credentials {
        let secret =
            base64::engine::general_purpose::STANDARD.encode(b"a made-up tunnel secret, 32 b.");
        let json = format!(r#"{{"a":"made-up-account","t":"{TUNNEL}","s":"{secret}"}}"#);
        Credentials::from_token(&base64::engine::general_purpose::STANDARD.encode(json)).unwrap()
    }

    fn named(edge: &FakeH2Edge, protocol: Protocol) -> Named {
        Named::new(made_up_credentials(), Some("cctop.example.test".into()))
            .against(edge.seam())
            .with_protocol(protocol)
    }

    /// Read a request head off `stream`, up to and including its blank line.
    fn read_head(stream: &mut std::net::TcpStream) -> String {
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).unwrap_or(0) == 0 {
                break;
            }
            head.push(byte[0]);
        }
        String::from_utf8_lossy(&head).into_owned()
    }

    /// An origin that runs `answer` on each connection, on a thread.
    fn origin_with(answer: impl Fn(std::net::TcpStream) + Send + Sync + 'static) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let answer = Arc::new(answer);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let answer = answer.clone();
                std::thread::spawn(move || answer(stream));
            }
        });
        port
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn registration_over_http2_carries_the_credentials_and_routes_by_host() {
        let mut edge = FakeH2Edge::start(Answer::Accept);
        let serve = origin("the page");
        let tunnel = named(&edge, Protocol::Http2)
            .open(Routes::new(serve))
            .await
            .unwrap();
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

        let page = fetch(&conn, get_request("cctop.example.test", "/")).await;
        assert_eq!((page.status, page.body.as_str()), (200, "the page"));
        assert_eq!(page.meta, r#"{"src":"origin"}"#);
        assert!(
            page.headers
                .contains(&("Content-Length".into(), "8".into())),
            "{:?}",
            page.headers
        );
        let elsewhere = fetch(&conn, get_request("elsewhere.example.test", "/")).await;
        assert_eq!(elsewhere.status, 404);
        tunnel.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_refusal_over_http2_is_still_a_refusal() {
        let edge = FakeH2Edge::start(Answer::Refuse);
        let err = named(&edge, Protocol::Http2)
            .open(Routes::new(1))
            .await
            .err()
            .expect("refused");
        assert!(matches!(err, Error::Refused(_)), "{err:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_configuration_pushed_over_http2_names_the_hostname() {
        let mut edge = FakeH2Edge::start(Answer::Accept);
        let serve = origin("the page");
        let named = Named::new(made_up_credentials(), None)
            .against(edge.seam())
            .with_protocol(Protocol::Http2);
        let opening = tokio::spawn(async move { named.open(Routes::new(serve)).await });

        let conn = edge.registered().await;
        let pushed = r#"{"ingress":[
            {"hostname":"cctop.example.test","service":"http://localhost:7777"},
            {"service":"http_status:404"}
        ]}"#;
        assert_eq!(push_config(&conn, 3, pushed).await, 3);
        assert_eq!(push_config(&conn, 2, r#"{"ingress":[]}"#).await, 3);

        let tunnel = opening.await.unwrap().unwrap();
        assert_eq!(tunnel.url(), "https://cctop.example.test");
        tunnel.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn auto_falls_back_to_http2_when_udp_to_the_edge_is_silent() {
        let mut edge = FakeH2Edge::start(Answer::Accept);
        let serve = origin("the page");
        let tunnel = named(&edge, Protocol::Auto)
            .open(Routes::new(serve))
            .await
            .unwrap();
        let conn = edge.registered().await;
        let page = fetch(&conn, get_request("cctop.example.test", "/")).await;
        assert_eq!((page.status, page.body.as_str()), (200, "the page"));
        tunnel.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn quic_alone_does_not_fall_back() {
        let edge = FakeH2Edge::start(Answer::Accept);
        let err = named(&edge, Protocol::Quic)
            .open(Routes::new(1))
            .await
            .err()
            .expect("no QUIC edge answers");
        assert!(
            matches!(err, Error::QuicDial { .. } | Error::Internal(_)),
            "{err:?}"
        );
        assert!(edge.registrations.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_quick_tunnel_runs_over_http2_too() {
        let mut edge = FakeH2Edge::start(Answer::Accept);
        let (api, _) = crate::cloudflare::api::tests::fake_api(vec![(
            200,
            crate::cloudflare::api::tests::sample_ok_body(),
        )]);
        let serve = origin("the page");
        let tunnel = Quick::against(edge.seam(), api)
            .with_protocol(Protocol::Http2)
            .open(Routes::new(serve))
            .await
            .unwrap();
        let conn = edge.registered().await;
        let page = fetch(&conn, get_request("abc-123.trycloudflare.com", "/x")).await;
        assert_eq!((page.status, page.body.as_str()), (200, "the page"));
        tunnel.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_websocket_over_http2_is_upgraded_at_the_origin_and_carries_bytes() {
        let heads = Arc::new(Mutex::new(Vec::new()));
        let seen = heads.clone();
        let ws = origin_with(move |mut stream| {
            seen.lock().unwrap().push(read_head(&mut stream));
            let _ = stream.write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                  Connection: Upgrade\r\nSec-WebSocket-Accept: accepted\r\n\r\n",
            );
            // Echo until the edge closes its side.
            let mut buf = [0u8; 1024];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 || stream.write_all(&buf[..n]).is_err() {
                    return;
                }
            }
        });
        let mut edge = FakeH2Edge::start(Answer::Accept);
        let tunnel = named(&edge, Protocol::Http2)
            .open(Routes::new(ws))
            .await
            .unwrap();
        let conn = edge.registered().await;

        let request = Request::builder()
            .method("GET")
            .uri("https://cctop.example.test/share?token=t")
            .header(UPGRADE_HEADER, UPGRADE_WEBSOCKET)
            .header("sec-websocket-key", "dGhlIHNhbXBsZQ==")
            .body(())
            .unwrap();
        let (response, sink) = open(&conn, request, true).await.unwrap();
        assert_eq!(response.status(), 200, "a 101 travels as 200");
        let headers =
            deserialize_headers(response.headers()[RESPONSE_USER_HEADERS].to_str().unwrap());
        assert!(
            headers.contains(&("Sec-WebSocket-Accept".into(), "accepted".into())),
            "{headers:?}"
        );
        let mut to_origin = H2Out::body(sink.unwrap());
        let mut from_origin = H2In::new(response.into_body());
        for word in ["first", "second"] {
            to_origin.write_all(word.as_bytes()).await.unwrap();
            let mut echoed = vec![0u8; word.len()];
            tokio::time::timeout(Duration::from_secs(5), from_origin.read_exact(&mut echoed))
                .await
                .expect("the echo comes back while the socket is open")
                .unwrap();
            assert_eq!(echoed, word.as_bytes());
        }

        let head = heads.lock().unwrap()[0].clone();
        assert!(
            head.starts_with("GET /share?token=t HTTP/1.1\r\n"),
            "{head}"
        );
        assert!(head.contains("Connection: Upgrade\r\n"), "{head}");
        assert!(head.contains("Upgrade: websocket\r\n"), "{head}");
        assert!(
            head.contains("sec-websocket-key: dGhlIHNhbXBsZQ==\r\n"),
            "{head}"
        );
        assert!(
            !head.to_ascii_lowercase().contains(UPGRADE_HEADER),
            "the internal header stays at the edge: {head}"
        );
        tunnel.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn server_sent_events_arrive_as_they_are_written() {
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let go_rx = Mutex::new(go_rx);
        let events = origin_with(move |mut stream| {
            read_head(&mut stream);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                  Cache-Control: no-cache\r\nConnection: close\r\n\r\ndata: one\n\n",
            );
            // The second event waits for the test to have seen the first.
            let _ = go_rx.lock().unwrap().recv();
            let _ = stream.write_all(b"data: two\n\n");
        });
        let mut edge = FakeH2Edge::start(Answer::Accept);
        let tunnel = named(&edge, Protocol::Http2)
            .open(Routes::new(events))
            .await
            .unwrap();
        let conn = edge.registered().await;

        let (response, _) = open(&conn, get_request("cctop.example.test", "/events"), false)
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let mut body = H2In::new(response.into_body());
        let mut first = vec![0u8; "data: one\n\n".len()];
        tokio::time::timeout(Duration::from_secs(5), body.read_exact(&mut first))
            .await
            .expect("the first event arrives before the response ends")
            .unwrap();
        assert_eq!(first, b"data: one\n\n");
        go_tx.send(()).unwrap();
        let mut rest = String::new();
        body.read_to_string(&mut rest).await.unwrap();
        assert_eq!(rest, "data: two\n\n");
        tunnel.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_body_sent_without_a_length_reaches_the_origin_with_one() {
        let echo = origin_with(|mut stream| {
            let head = read_head(&mut stream);
            let length: usize = head
                .lines()
                .find_map(|l| {
                    let (name, value) = l.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().ok())?
                })
                .unwrap_or(0);
            let mut body = vec![0u8; length];
            let _ = stream.read_exact(&mut body);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        });
        let mut edge = FakeH2Edge::start(Answer::Accept);
        let tunnel = named(&edge, Protocol::Http2)
            .open(Routes::new(echo))
            .await
            .unwrap();
        let conn = edge.registered().await;

        let request = Request::builder()
            .method("POST")
            .uri("https://cctop.example.test/api/send")
            .body(())
            .unwrap();
        let answered = fetch_with_body(&conn, request, b"keys for the terminal").await;
        assert_eq!(
            (answered.status, answered.body.as_str()),
            (200, "keys for the terminal")
        );
        tunnel.shutdown().await;
    }
}
