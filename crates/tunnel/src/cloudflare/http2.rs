//! The tunnel over HTTP/2 on TCP: cloudflared's `--protocol http2`, the
//! fallback for a network where QUIC's UDP does not get through or does not
//! stay up (`protocol`).
//!
//! # The wire, as cloudflared speaks it
//!
//! From cloudflared's source (`connection/protocol.go`, `connection/http2.go`,
//! `connection/header.go`, `edgediscovery/dial.go`, `supervisor/tunnel.go`):
//!
//! - **Dial**: TCP to the same edge address QUIC uses (port 7844), then TLS
//!   with SNI `h2.cftunnel.com` (`edgeH2TLSServerName`) against the same
//!   roots. No ALPN: cloudflared's `TLSSettings` for HTTP2 sets no
//!   `NextProtos`, and the edge speaks HTTP/2 on that name regardless.
//! - **Roles**: cloudflared dials, but is the HTTP/2 *server* on the
//!   connection (`http2.Server.ServeConn`). The edge sends every request,
//!   the control stream included, and cloudflared answers.
//! - **Which request is which**: by `Cf-Cloudflared-Proxy-Connection-Upgrade`
//!   — `control-stream`, `update-configuration`, `websocket` — or a
//!   `Cf-Cloudflared-Proxy-Src` header for a TCP stream; anything else is a
//!   plain HTTP request (`determineHTTP2Type`).
//! - **Control stream**: the capnp-RPC `registerConnection`, byte for byte as
//!   on QUIC's first stream, carried in the request body (edge → us) and the
//!   response body (us → edge). The registration handshake therefore waits
//!   for the edge to open that request.
//! - **Configuration push**: a JSON body `{"version", "config"}`, answered with
//!   `{"lastAppliedVersion", "err"}` — the same `updateConfiguration` QUIC
//!   carries as an RPC.
//! - **A proxied request**: the browser's request with its headers as HTTP/2
//!   headers; the internal upgrade header is stripped before it goes on.
//! - **A response**: `:status`, a `Content-Length` when the origin gave one,
//!   and every other origin header serialized into
//!   `Cf-Cloudflared-Response-Headers` (base64 name `:` base64 value, `;`
//!   between pairs, unpadded) so that HTTP/2's header rules never see HTTP/1
//!   values; `Cf-Cloudflared-Response-Meta` says whether the origin or
//!   cloudflared answered. A 101 is sent as 200 — HTTP/2 has no 101 — and the
//!   two bodies then carry the WebSocket's bytes each way.
//!
//! Everything above the framing — routing by hostname, the local pool, the
//! WebSocket upgrade put back for the origin — is `proxy`'s, shared with QUIC:
//! a request here becomes the same `ConnectRequest` a QUIC stream carries.
//!
//! # Liveness
//!
//! QUIC has keep-alives and an idle timeout. A TCP connection whose path went
//! away can sit "established" for many minutes, so this pings the edge every
//! [`PING_EVERY`] and calls the connection dead when a ping goes unanswered
//! for [`PING_TIMEOUT`] — a TCP retransmit storm counts, a gap TCP rides out
//! does not.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::{Context, Poll, ready};
use std::time::Duration;

use base64::Engine;
use bytes::{Bytes, BytesMut};
use futures::AsyncReadExt;
use h2::server::SendResponse;
use h2::{Reason, RecvStream, SendStream};
use http::{HeaderMap, HeaderValue, Request, Response};
use rustls::pki_types::ServerName;
use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;
use tracing::{debug, info, warn};

use super::EdgeSource;
use super::config::{self, IngressTx};
use super::edge::EdgeAddr;
use super::pool::Pool;
use super::proxy;
use super::quic_dial::edge_roots;
use super::stream::{
    ConnectRequest, ConnectionType, EdgeWrite, HTTP_HEADER_KEY, HTTP_HOST_KEY, HTTP_METHOD_KEY,
    HTTP_STATUS_KEY, MetaPair,
};
use super::supervisor::{SupervisorExit, SupervisorMetrics};
use crate::{Error, Routes};

/// SNI for the HTTP/2 edge: cloudflared's `edgeH2TLSServerName`.
pub const EDGE_SNI: &str = "h2.cftunnel.com";

/// cloudflared's `InternalUpgradeHeader`, and the values it takes.
pub(crate) const UPGRADE_HEADER: &str = "cf-cloudflared-proxy-connection-upgrade";
pub(crate) const UPGRADE_CONTROL: &str = "control-stream";
pub(crate) const UPGRADE_CONFIGURATION: &str = "update-configuration";
pub(crate) const UPGRADE_WEBSOCKET: &str = "websocket";
/// cloudflared's `InternalTCPProxySrcHeader`: present on a TCP stream.
pub(crate) const TCP_SRC_HEADER: &str = "cf-cloudflared-proxy-src";
/// cloudflared's `ResponseUserHeaders` and `ResponseMetaHeader`.
pub(crate) const RESPONSE_USER_HEADERS: &str = "cf-cloudflared-response-headers";
pub(crate) const RESPONSE_META: &str = "cf-cloudflared-response-meta";
const META_ORIGIN: &str = r#"{"src":"origin"}"#;
const META_CLOUDFLARED: &str = r#"{"src":"cloudflared"}"#;

/// Budget for the TCP connect and the TLS handshake to one edge address.
pub const DIAL_TIMEOUT: Duration = Duration::from_secs(10);

/// How long after the HTTP/2 handshake the edge has to open the control
/// stream. It does so at once; this only bounds a broken edge.
const CONTROL_STREAM_TIMEOUT: Duration = Duration::from_secs(15);

/// How often the edge is pinged, and how long an answer may take.
pub(crate) const PING_EVERY: Duration = Duration::from_secs(10);
pub(crate) const PING_TIMEOUT: Duration = Duration::from_secs(15);

/// A request body that arrives with no length is read whole before it goes to
/// the origin, so that the origin gets a `Content-Length` (see
/// [`serve_request`]); this caps it. cctop's own requests are small.
const MAX_UNSIZED_BODY: usize = 8 * 1024 * 1024;

/// A configuration push larger than this is refused.
const MAX_CONFIGURATION: u64 = 1024 * 1024;

/// Flow-control windows offered to the edge, as Go's HTTP/2 server's upload
/// buffers (1 MiB): the default 64 KiB would stall a fast upload on a
/// round trip per window.
const WINDOW: u32 = 1024 * 1024;

/// What every request on a connection needs from the tunnel.
#[derive(Clone)]
pub(crate) struct Serve {
    pub routes: Routes,
    pub metrics: SupervisorMetrics,
    pub pool: Arc<Pool>,
    pub ingress: IngressTx,
}

/// One registered HTTP/2 connection: the task that drives it, and how it
/// ended.
pub(crate) struct Link {
    exit: oneshot::Receiver<SupervisorExit>,
    task: tokio::task::JoinHandle<()>,
}

impl Link {
    /// Wait for the connection to end.
    pub(crate) async fn closed(&mut self) -> SupervisorExit {
        (&mut self.exit)
            .await
            .unwrap_or(SupervisorExit::ConnectionLost)
    }

    /// Drop the connection. Called after unregistering, which the driver must
    /// still be running to carry.
    pub(crate) fn close(self) {
        self.task.abort();
    }
}

/// The TLS configuration for the HTTP/2 edge.
pub(crate) fn tls_config(edge: &EdgeSource) -> Result<Arc<rustls::ClientConfig>, Error> {
    let roots = edge_roots(edge)?;
    let tls = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(tls))
}

/// TCP and TLS to the first of `edges` that answers.
pub(crate) async fn dial_any(
    tls: &Arc<rustls::ClientConfig>,
    edges: &[EdgeAddr],
) -> Result<TlsStream<TcpStream>, Error> {
    let mut last = "no edges provided".to_string();
    for edge in edges {
        let addr = edge.socket();
        match tokio::time::timeout(DIAL_TIMEOUT, dial(tls, addr)).await {
            Ok(Ok(stream)) => {
                info!(%addr, "HTTP/2 edge connected");
                return Ok(stream);
            }
            Ok(Err(e)) => last = format!("{addr}: {e}"),
            Err(_) => last = format!("{addr}: timed out"),
        }
        warn!(%addr, error = %last, "HTTP/2 dial failed");
    }
    Err(Error::Http2Dial(last))
}

async fn dial(
    tls: &Arc<rustls::ClientConfig>,
    addr: SocketAddr,
) -> io::Result<TlsStream<TcpStream>> {
    let tcp = TcpStream::connect(addr).await?;
    // Terminal keystrokes are small writes that want to leave now.
    tcp.set_nodelay(true)?;
    let name = ServerName::try_from(EDGE_SNI).map_err(io::Error::other)?;
    TlsConnector::from(tls.clone()).connect(name, tcp).await
}

/// Serve HTTP/2 on `io` and wait for the edge's control stream. Returns the
/// running connection and the control stream's two halves, for
/// `rpc::register_over`.
pub(crate) async fn start<T>(io: T, serve: Serve) -> Result<(Link, H2In, H2Out), Error>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let conn = h2::server::Builder::new()
        .initial_window_size(WINDOW)
        .initial_connection_window_size(WINDOW)
        .handshake::<_, Bytes>(io)
        .await
        .map_err(|e| Error::Http2Dial(format!("HTTP/2 handshake: {e}")))?;
    let (control_tx, control_rx) = oneshot::channel();
    let (exit_tx, exit_rx) = oneshot::channel();
    let task = tokio::spawn(drive(conn, serve, control_tx, exit_tx));
    let link = Link {
        exit: exit_rx,
        task,
    };
    let (request, respond) = match tokio::time::timeout(CONTROL_STREAM_TIMEOUT, control_rx).await {
        Ok(Ok(control)) => control,
        Ok(Err(_)) => {
            link.close();
            return Err(Error::Register(
                "the edge closed the HTTP/2 connection before its control stream".into(),
            ));
        }
        Err(_) => {
            link.close();
            return Err(Error::Register(
                "the edge opened no control stream on the HTTP/2 connection".into(),
            ));
        }
    };
    let mut out = H2Out::new(respond);
    out.write_status(200)?;
    Ok((link, H2In::new(request.into_body()), out))
}

/// Accept the edge's requests until the connection ends, and say why it did.
async fn drive<T>(
    mut conn: h2::server::Connection<T, Bytes>,
    serve: Serve,
    control: oneshot::Sender<(Request<RecvStream>, SendResponse<Bytes>)>,
    exit: oneshot::Sender<SupervisorExit>,
) where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let mut control = Some(control);
    let pings = conn.ping_pong();
    let silent = async move {
        let Some(mut pings) = pings else {
            return std::future::pending().await;
        };
        loop {
            tokio::time::sleep(PING_EVERY).await;
            match tokio::time::timeout(PING_TIMEOUT, pings.ping(h2::Ping::opaque())).await {
                Ok(Ok(_)) => {}
                // The connection is closing, which `accept` reports itself.
                Ok(Err(_)) => return std::future::pending().await,
                Err(_) => return,
            }
        }
    };
    tokio::pin!(silent);
    let why = loop {
        tokio::select! {
            next = conn.accept() => match next {
                Some(Ok((request, respond))) => {
                    dispatch(request, respond, &serve, &mut control);
                }
                Some(Err(e)) => {
                    debug!(error = %e, "HTTP/2 connection to the edge ended");
                    break SupervisorExit::ConnectionLost;
                }
                None => {
                    debug!("the edge closed the HTTP/2 connection");
                    break SupervisorExit::ConnectionLost;
                }
            },
            () = &mut silent => {
                warn!("HTTP/2 connection to the edge stopped answering pings");
                break SupervisorExit::TimedOut;
            }
        }
    };
    info!(exit = ?why, "HTTP/2 tunnel connection exited");
    let _ = exit.send(why);
}

/// What a request from the edge is for (cloudflared `determineHTTP2Type`,
/// whose order this keeps).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Control,
    Configuration,
    Websocket,
    Tcp,
    Http,
}

pub(crate) fn kind(headers: &HeaderMap) -> Kind {
    let upgrade = headers.get(UPGRADE_HEADER).and_then(|v| v.to_str().ok());
    match upgrade {
        Some(UPGRADE_CONFIGURATION) => Kind::Configuration,
        Some(UPGRADE_WEBSOCKET) => Kind::Websocket,
        _ if headers.contains_key(TCP_SRC_HEADER) => Kind::Tcp,
        Some(UPGRADE_CONTROL) => Kind::Control,
        _ => Kind::Http,
    }
}

fn dispatch(
    request: Request<RecvStream>,
    mut respond: SendResponse<Bytes>,
    serve: &Serve,
    control: &mut Option<oneshot::Sender<(Request<RecvStream>, SendResponse<Bytes>)>>,
) {
    match kind(request.headers()) {
        Kind::Control => match control.take() {
            Some(tx) => {
                let _ = tx.send((request, respond));
            }
            None => {
                warn!("a second control stream on one connection; refused");
                respond.send_reset(Reason::REFUSED_STREAM);
            }
        },
        Kind::Configuration => {
            let ingress = serve.ingress.clone();
            tokio::spawn(configuration(request, respond, ingress));
        }
        kind => {
            serve.metrics.streams_total.fetch_add(1, Ordering::Relaxed);
            let serve = serve.clone();
            tokio::spawn(async move {
                if let Err(e) = serve_request(kind, request, respond, &serve).await {
                    warn!(error = %e, "stream failed");
                }
            });
        }
    }
}

/// One request from the internet, handed to the proxy QUIC's go to.
async fn serve_request(
    kind: Kind,
    request: Request<RecvStream>,
    respond: SendResponse<Bytes>,
    serve: &Serve,
) -> Result<(), Error> {
    let (parts, body) = request.into_parts();
    let mut connect = connect_request(kind, &parts);
    let mut input = H2In::new(body);
    let mut out = H2Out::new(respond);
    // HTTP/2 needs no length for a body; HTTP/1.1 does, and the proxy sends
    // the origin only what a `Content-Length` says. So a body without one is
    // read here and given one, rather than reach the origin as no body at all.
    if kind == Kind::Http && !input.is_end_stream() && !has_length(&connect) {
        match input.read_whole(MAX_UNSIZED_BODY).await {
            Ok(0) => {}
            Ok(n) => connect
                .metadata
                .push((format!("{HTTP_HEADER_KEY}:Content-Length"), n.to_string())),
            Err(e) => {
                out.write_head(&format!("request body: {e}"), &[]).await?;
                return Ok(());
            }
        }
    }
    proxy::serve_request(
        &serve.routes,
        connect,
        input,
        out,
        serve.metrics.stream_counters(),
        serve.pool.clone(),
    )
    .await
}

fn has_length(connect: &ConnectRequest) -> bool {
    connect.metadata.iter().any(|(k, _)| {
        k.strip_prefix(HTTP_HEADER_KEY)
            .and_then(|k| k.strip_prefix(':'))
            .is_some_and(|name| {
                name.eq_ignore_ascii_case("content-length")
                    || name.eq_ignore_ascii_case("transfer-encoding")
            })
    })
}

/// The `ConnectRequest` a QUIC stream would have carried for this request:
/// `dest` the full URL (the host alone for TCP), `HttpMethod`, `HttpHost`,
/// and an `HttpHeader:<name>` per header but the internal upgrade one, which
/// cloudflared strips before proxying (`stripWebsocketUpgradeHeader`).
pub(crate) fn connect_request(kind: Kind, parts: &http::request::Parts) -> ConnectRequest {
    let host = parts
        .uri
        .authority()
        .map(|a| a.as_str().to_string())
        .or_else(|| {
            parts
                .headers
                .get(http::header::HOST)
                .and_then(|h| h.to_str().ok())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let conn_type = match kind {
        Kind::Websocket => ConnectionType::Websocket,
        Kind::Tcp => ConnectionType::Tcp,
        _ => ConnectionType::Http,
    };
    let dest = match kind {
        Kind::Tcp => host.clone(),
        _ => parts.uri.to_string(),
    };
    let mut metadata = Vec::with_capacity(parts.headers.len() + 2);
    metadata.push((
        HTTP_METHOD_KEY.to_string(),
        parts.method.as_str().to_string(),
    ));
    metadata.push((HTTP_HOST_KEY.to_string(), host));
    for (name, value) in &parts.headers {
        if name.as_str() == UPGRADE_HEADER {
            continue;
        }
        metadata.push((
            format!("{HTTP_HEADER_KEY}:{name}"),
            String::from_utf8_lossy(value.as_bytes()).into_owned(),
        ));
    }
    ConnectRequest {
        dest,
        conn_type,
        metadata,
    }
}

/// A configuration push (cloudflared `handleConfigurationUpdate`).
async fn configuration(
    request: Request<RecvStream>,
    respond: SendResponse<Bytes>,
    ingress: IngressTx,
) {
    #[derive(Deserialize)]
    struct Update {
        version: i32,
        config: serde_json::Value,
    }
    let mut out = H2Out::new(respond);
    let mut body = Vec::new();
    let read = H2In::new(request.into_body())
        .take(MAX_CONFIGURATION)
        .read_to_end(&mut body)
        .await;
    let update = read
        .map_err(|e| e.to_string())
        .and_then(|_| serde_json::from_slice::<Update>(&body).map_err(|e| e.to_string()));
    let update = match update {
        Ok(update) => update,
        Err(why) => {
            warn!(%why, "configuration push not understood");
            let _ = out.write_head(&why, &[]).await;
            return;
        }
    };
    let config = serde_json::to_vec(&update.config).unwrap_or_default();
    let (applied, err) = config::apply(&ingress, update.version, &config);
    let reply = serde_json::json!({ "lastAppliedVersion": applied, "err": err });
    let reply = serde_json::to_vec(&reply).unwrap_or_default();
    if out.write_status(200).is_ok() {
        let _ = futures::AsyncWriteExt::write_all(&mut out, &reply).await;
        let _ = futures::AsyncWriteExt::close(&mut out).await;
    }
}

// ── Response head ───────────────────────────────────────────────────────────

/// The response HEADERS for the proxy's metadata, as cloudflared's
/// `WriteRespHeaders` builds them — or for an `error`, its
/// `WriteErrorResponse`: a 502 the edge turns into its own error page.
pub(crate) fn response_head(error: &str, metadata: &[MetaPair<'_>]) -> Response<()> {
    let mut response = Response::new(());
    let headers = response.headers_mut();
    if !error.is_empty() {
        *response.status_mut() = http::StatusCode::BAD_GATEWAY;
        response
            .headers_mut()
            .insert(RESPONSE_META, HeaderValue::from_static(META_CLOUDFLARED));
        return response;
    }
    let mut status = 200u16;
    let mut user: Vec<(&str, &str)> = Vec::new();
    for (key, value) in metadata {
        if *key == HTTP_STATUS_KEY {
            status = value.parse().unwrap_or(502);
            continue;
        }
        let Some(name) = key
            .strip_prefix(HTTP_HEADER_KEY)
            .and_then(|k| k.strip_prefix(':'))
        else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if lower == "content-length"
            && let Ok(length) = HeaderValue::from_str(value)
        {
            // It means something to HTTP/2 and to the edge, so it goes as a
            // real header too.
            headers.insert(http::header::CONTENT_LENGTH, length);
        }
        if !is_control_response_header(&lower) || is_websocket_client_header(&lower) {
            user.push((name, value));
        }
    }
    if let Ok(serialized) = HeaderValue::from_str(&serialize_headers(&user)) {
        headers.insert(RESPONSE_USER_HEADERS, serialized);
    }
    headers.insert(RESPONSE_META, HeaderValue::from_static(META_ORIGIN));
    // HTTP/2 has no 101 (RFC 7540 §8.1.1): the upgrade is a 200 whose two
    // bodies are the WebSocket.
    if status == 101 {
        status = 200;
    }
    *response.status_mut() =
        http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::BAD_GATEWAY);
    response
}

/// cloudflared `IsControlResponseHeader`.
fn is_control_response_header(lower: &str) -> bool {
    lower.starts_with(':')
        || lower.starts_with("cf-int-")
        || lower.starts_with("cf-cloudflared-")
        || lower.starts_with("cf-proxy-")
}

/// cloudflared `IsWebsocketClientHeader`.
fn is_websocket_client_header(lower: &str) -> bool {
    matches!(lower, "sec-websocket-accept" | "connection" | "upgrade")
}

/// cloudflared `SerializeHeaders`: `b64(name):b64(value)` joined by `;`, in
/// unpadded standard base64.
pub(crate) fn serialize_headers(headers: &[(&str, &str)]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD_NO_PAD;
    let mut out = String::new();
    for (name, value) in headers {
        if !out.is_empty() {
            out.push(';');
        }
        out.push_str(&b64.encode(name));
        out.push(':');
        out.push_str(&b64.encode(value));
    }
    out
}

/// cloudflared `DeserializeHeaders`, for the tests' edge.
#[cfg(test)]
pub(crate) fn deserialize_headers(serialized: &str) -> Vec<(String, String)> {
    let b64 = base64::engine::general_purpose::STANDARD_NO_PAD;
    serialized
        .split(';')
        .filter(|pair| !pair.is_empty())
        .filter_map(|pair| {
            let (name, value) = pair.split_once(':')?;
            Some((
                String::from_utf8(b64.decode(name).ok()?).ok()?,
                String::from_utf8(b64.decode(value).ok()?).ok()?,
            ))
        })
        .collect()
}

// ── Bodies as byte streams ──────────────────────────────────────────────────

/// A request body (or, at the tests' edge, a response body) read as bytes.
pub(crate) struct H2In {
    pending: Bytes,
    recv: Option<RecvStream>,
}

impl H2In {
    pub(crate) fn new(recv: RecvStream) -> H2In {
        H2In {
            pending: Bytes::new(),
            recv: Some(recv),
        }
    }

    fn is_end_stream(&self) -> bool {
        self.recv.as_ref().is_none_or(RecvStream::is_end_stream)
    }

    /// Read the rest of the body into memory, up to `max` bytes, to be read
    /// back from there: its length.
    async fn read_whole(&mut self, max: usize) -> io::Result<usize> {
        let mut whole = BytesMut::from(&self.pending[..]);
        while let Some(chunk) = futures::future::poll_fn(|cx| self.poll_chunk(cx)).await? {
            whole.extend_from_slice(&chunk);
            if whole.len() > max {
                return Err(io::Error::other(format!(
                    "a body of unknown length over {max} bytes"
                )));
            }
        }
        self.recv = None;
        self.pending = whole.freeze();
        Ok(self.pending.len())
    }

    /// The next DATA frame's bytes, `None` at the end of the body.
    fn poll_chunk(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<Option<Bytes>>> {
        let Some(recv) = self.recv.as_mut() else {
            return Poll::Ready(Ok(None));
        };
        match ready!(recv.poll_data(cx)) {
            Some(Ok(data)) => {
                // Handed on at once, so the window opens again at once.
                let _ = recv.flow_control().release_capacity(data.len());
                Poll::Ready(Ok(Some(data)))
            }
            // A reset that says "no error" is how a peer ends a body it has no
            // more of without a final frame.
            Some(Err(e)) if e.reason() == Some(Reason::NO_ERROR) => Poll::Ready(Ok(None)),
            Some(Err(e)) => Poll::Ready(Err(io::Error::other(e))),
            None => Poll::Ready(Ok(None)),
        }
    }
}

impl futures::io::AsyncRead for H2In {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        while this.pending.is_empty() {
            match ready!(this.poll_chunk(cx))? {
                Some(chunk) => this.pending = chunk,
                None => {
                    this.recv = None;
                    return Poll::Ready(Ok(0));
                }
            }
        }
        let n = buf.len().min(this.pending.len());
        buf[..n].copy_from_slice(&this.pending.split_to(n));
        Poll::Ready(Ok(n))
    }
}

/// A response: its head ([`EdgeWrite::write_head`]), then its body as bytes.
///
/// Every write goes out as a DATA frame at once — nothing is held back for a
/// fuller frame — which is what Server-Sent Events and a terminal need, and
/// what cloudflared gets by flushing after each write of a streamed type.
///
/// Dropped without having been closed, it ends the stream cleanly; dropped
/// before any head, it answers 502 as cloudflared does for a handler that
/// failed before writing one.
pub(crate) struct H2Out {
    respond: Option<SendResponse<Bytes>>,
    body: Option<SendStream<Bytes>>,
}

impl H2Out {
    pub(crate) fn new(respond: SendResponse<Bytes>) -> H2Out {
        H2Out {
            respond: Some(respond),
            body: None,
        }
    }

    /// A body to write into with no head to send first: a request's, at the
    /// tests' edge.
    #[cfg(test)]
    pub(crate) fn body(body: SendStream<Bytes>) -> H2Out {
        H2Out {
            respond: None,
            body: Some(body),
        }
    }

    /// A bare `:status` head, as Go's server sends for a handler that writes
    /// a body without headers: the control stream's and a configuration
    /// push's answer.
    fn write_status(&mut self, status: u16) -> Result<(), Error> {
        let mut response = Response::new(());
        *response.status_mut() =
            http::StatusCode::from_u16(status).map_err(|e| Error::Internal(e.to_string()))?;
        self.send(response)
    }

    fn send(&mut self, response: Response<()>) -> Result<(), Error> {
        let mut respond = self
            .respond
            .take()
            .ok_or_else(|| Error::Internal("a response head was sent twice".into()))?;
        let body = respond
            .send_response(response, false)
            .map_err(|e| Error::Internal(format!("send response head: {e}")))?;
        self.body = Some(body);
        Ok(())
    }
}

impl EdgeWrite for H2Out {
    async fn write_head(&mut self, error: &str, metadata: &[MetaPair<'_>]) -> Result<(), Error> {
        self.send(response_head(error, metadata))
    }
}

impl futures::io::AsyncWrite for H2Out {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let Some(body) = self.get_mut().body.as_mut() else {
            return Poll::Ready(Err(io::Error::other("body written before its head")));
        };
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        body.reserve_capacity(buf.len());
        loop {
            match ready!(body.poll_capacity(cx)) {
                Some(Ok(0)) => continue,
                Some(Ok(n)) => {
                    let n = n.min(buf.len());
                    body.send_data(Bytes::copy_from_slice(&buf[..n]), false)
                        .map_err(io::Error::other)?;
                    return Poll::Ready(Ok(n));
                }
                Some(Err(e)) => return Poll::Ready(Err(io::Error::other(e))),
                None => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "the edge closed the stream",
                    )));
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        // The connection's task writes frames out as they are queued.
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some(mut body) = self.get_mut().body.take() {
            body.send_data(Bytes::new(), true)
                .map_err(io::Error::other)?;
        }
        Poll::Ready(Ok(()))
    }
}

impl Drop for H2Out {
    fn drop(&mut self) {
        if let Some(body) = self.body.as_mut() {
            let _ = body.send_data(Bytes::new(), true);
        } else if let Some(respond) = self.respond.as_mut() {
            let _ = respond.send_response(response_head("no response", &[]), true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.append(
                http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        map
    }

    #[test]
    fn requests_are_told_apart_in_cloudflareds_order() {
        assert_eq!(kind(&headers(&[])), Kind::Http);
        assert_eq!(
            kind(&headers(&[(UPGRADE_HEADER, "control-stream")])),
            Kind::Control
        );
        assert_eq!(
            kind(&headers(&[(UPGRADE_HEADER, "update-configuration")])),
            Kind::Configuration
        );
        assert_eq!(
            kind(&headers(&[(UPGRADE_HEADER, "websocket")])),
            Kind::Websocket
        );
        assert_eq!(kind(&headers(&[(TCP_SRC_HEADER, "1.2.3.4")])), Kind::Tcp);
        // A TCP source wins over a control-stream upgrade, as upstream.
        assert_eq!(
            kind(&headers(&[
                (UPGRADE_HEADER, "control-stream"),
                (TCP_SRC_HEADER, "x")
            ])),
            Kind::Tcp
        );
    }

    #[test]
    fn a_request_becomes_the_connect_request_quic_would_carry() {
        let request = Request::builder()
            .method("POST")
            .uri("https://cctop.example.test/api/x?y=1")
            .header(UPGRADE_HEADER, "websocket")
            .header("sec-websocket-key", "abc")
            .header("cookie", "a=1")
            .header("cookie", "b=2")
            .body(())
            .unwrap();
        let (parts, ()) = request.into_parts();
        let req = connect_request(Kind::Websocket, &parts);
        assert_eq!(req.dest, "https://cctop.example.test/api/x?y=1");
        assert_eq!(req.conn_type, ConnectionType::Websocket);
        assert_eq!(req.meta(HTTP_METHOD_KEY), Some("POST"));
        assert_eq!(req.meta(HTTP_HOST_KEY), Some("cctop.example.test"));
        assert_eq!(req.meta("HttpHeader:sec-websocket-key"), Some("abc"));
        let cookies: Vec<_> = req
            .metadata
            .iter()
            .filter(|(k, _)| k == "HttpHeader:cookie")
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(cookies, ["a=1", "b=2"]);
        assert!(
            !req.metadata.iter().any(|(k, _)| k.contains(UPGRADE_HEADER)),
            "the internal upgrade header is not the origin's: {:?}",
            req.metadata
        );
    }

    #[test]
    fn origin_headers_travel_serialized_and_101_becomes_200() {
        let meta = [
            (HTTP_STATUS_KEY, "101"),
            ("HttpHeader:Upgrade", "websocket"),
            ("HttpHeader:Connection", "Upgrade"),
            (
                "HttpHeader:Sec-WebSocket-Accept",
                "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=",
            ),
            ("HttpHeader:Cf-Cloudflared-Secret", "kept back"),
            ("HttpHeader:Set-Cookie", "a=1"),
        ];
        let head = response_head("", &meta);
        assert_eq!(head.status(), 200);
        assert_eq!(head.headers()[RESPONSE_META], META_ORIGIN);
        assert!(head.headers().get("upgrade").is_none());
        let user = deserialize_headers(head.headers()[RESPONSE_USER_HEADERS].to_str().unwrap());
        assert_eq!(
            user,
            [
                ("Upgrade".to_string(), "websocket".to_string()),
                ("Connection".into(), "Upgrade".into()),
                (
                    "Sec-WebSocket-Accept".into(),
                    "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=".into()
                ),
                ("Set-Cookie".into(), "a=1".into()),
            ]
        );
    }

    #[test]
    fn content_length_goes_both_ways_and_an_error_is_cloudflareds_502() {
        let head = response_head(
            "",
            &[(HTTP_STATUS_KEY, "404"), ("HttpHeader:Content-Length", "0")],
        );
        assert_eq!(head.status(), 404);
        assert_eq!(head.headers()[http::header::CONTENT_LENGTH], "0");
        assert_eq!(
            deserialize_headers(head.headers()[RESPONSE_USER_HEADERS].to_str().unwrap()),
            [("Content-Length".to_string(), "0".to_string())]
        );

        let failed = response_head("local connect: refused", &[(HTTP_STATUS_KEY, "502")]);
        assert_eq!(failed.status(), 502);
        assert_eq!(failed.headers()[RESPONSE_META], META_CLOUDFLARED);
    }

    #[test]
    fn serialization_matches_cloudflareds() {
        // `SerializeHeaders` of {"Content-Type": "text/plain"}, by hand:
        // base64("Content-Type") and base64("text/plain"), no padding.
        assert_eq!(
            serialize_headers(&[("Content-Type", "text/plain")]),
            "Q29udGVudC1UeXBl:dGV4dC9wbGFpbg"
        );
        assert_eq!(serialize_headers(&[]), "");
    }
}
