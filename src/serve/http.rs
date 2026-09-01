//! Just enough HTTP/1.1 to answer a browser, and no more.
//!
//! The surface cctop needs is three verbs' worth of nothing: `GET`, `HEAD`,
//! `POST`, a path, a query string, and a body in each direction. No routing
//! DSL, no middleware, no keep-alive negotiation, no compression. That is a few
//! hundred lines here against a web framework and its transitive tree in
//! `Cargo.toml` — the same bargain [`crate::mcp`] took with JSON-RPC, for the
//! same reason: a monitoring tool people `cargo install` should not pull in a
//! runtime to draw a table.
//!
//! What it does take seriously is that this socket is the one part of cctop a
//! stranger can reach. Every read is bounded and deadlined, so neither a client
//! that sends a gigabyte of headers nor one that opens a connection and says
//! nothing can cost more than one thread and [`MAX_HEADER_BYTES`]:
//!
//! - the request line and headers are read into a fixed buffer, and a request
//!   that overruns it is answered `431` rather than grown into;
//! - the socket carries a read *and* a write timeout, so a peer that stops
//!   reading an SSE stream cannot pin the thread forever;
//! - a request body is read only when the request line said `POST` and only up
//!   to [`MAX_BODY_BYTES`], which is orders of magnitude more than the few
//!   fields an action route takes and still nothing a peer can grow.
//!
//! An action route is a `POST` with a JSON body, and both halves of that are
//! load-bearing rather than stylistic. A cross-origin form can be made to send
//! a `GET` or a `POST` of form-encoded data without the page ever seeing the
//! answer; it cannot set `Content-Type: application/json` without asking
//! permission first, and this server answers no preflight. So the pairing is
//! what stops a page in another tab from driving an agent on the strength of a
//! token it cannot read — see [`Request::wants_json`].
//!
//! ponytail: HTTP/1.0-style connection-per-request. Keep-alive would save a
//! handshake on a page that makes four requests and then holds one SSE stream
//! open for an hour, which is not a saving worth the state machine.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// The most request line and headers that will be read before giving up.
///
/// A browser's `GET` with cookies and a long `User-Agent` lands under 4 KiB;
/// this is generous to that and still small enough that a malicious peer
/// buys nothing by filling it.
const MAX_HEADER_BYTES: usize = 16 * 1024;

/// The most request body that will be read before giving up.
///
/// An action body is a session id, a target and a line of prose. 64 KiB is
/// room for a prompt someone pasted a stack trace into, and a hard stop well
/// under what a thread can be made to hold.
const MAX_BODY_BYTES: usize = 64 * 1024;

/// The most an image posted to the paste route may be.
///
/// Its own limit because it is the one route whose body is not prose: a
/// screenshot of a wide display is a megabyte or two of PNG, half again as
/// much in base64, and the ordinary 64 KiB would refuse every one of them. Kept
/// to a size a thread can hold without thinking about it, and applied only to
/// this path — every other route keeps the small bound.
const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;

/// The path that carries images, and so the one that may be large.
const IMAGE_PATH: &str = "/api/act/image/";

/// How long a client has to finish sending its request line and headers.
///
/// Deliberately short. A connection that has been accepted but has not asked
/// for anything is either a port scan or a browser that changed its mind, and
/// both should release the thread quickly.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a single write may block before the connection is abandoned.
///
/// Long-lived SSE streams are the reason this exists: a phone that goes to
/// sleep with the dashboard open stops reading, its receive window closes, and
/// without a deadline the writing thread would block on `write` until the
/// process ends.
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// A parsed request: a method, a path, and the query string as a map.
pub struct Request {
    pub method: String,
    /// Path with percent-escapes decoded and the query removed. Always begins
    /// with `/`; see [`Request::parse`] for what it refuses.
    pub path: String,
    pub query: HashMap<String, String>,
    /// The `POST` body, empty for every other method.
    pub body: Vec<u8>,
    /// Whether the body announced itself as JSON.
    ///
    /// Kept as a flag rather than the whole header map because it is the only
    /// header anything here routes on, and it is routed on for a security
    /// reason rather than a parsing one — see the module docs.
    json_content_type: bool,
}

impl Request {
    /// Read and parse one request from `stream`, or return why not.
    ///
    /// The error is a status code and a message, already in the shape the
    /// caller has to send back — a malformed request still deserves an answer,
    /// and deciding what that answer is belongs next to the parsing that
    /// rejected it.
    pub fn parse(stream: &TcpStream) -> Result<Request, (u16, &'static str)> {
        // Both directions, before the first read: the timeouts are the whole
        // defence against a peer that connects and then does nothing.
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
        let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));

        // `take` is the bound that matters. Without it a peer that never sends
        // a blank line grows the buffer until the process dies, and a read
        // timeout would not save us — a slow trickle of bytes resets it. The
        // allowance covers a body as well, so the header half is bounded by
        // counting bytes as they are read rather than by the reader itself.
        let mut reader = BufReader::new(stream.take((MAX_HEADER_BYTES + MAX_IMAGE_BYTES) as u64));
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.is_empty() {
            return Err((400, "malformed request line"));
        }

        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let target = parts.next().unwrap_or_default();
        if method != "GET" && method != "HEAD" && method != "POST" {
            return Err((405, "this server answers GET, HEAD and POST"));
        }
        if target.is_empty() {
            return Err((400, "malformed request line"));
        }

        let (raw_path, raw_query) = match target.split_once('?') {
            Some((p, q)) => (p, q),
            None => (target, ""),
        };
        let path = percent_decode(raw_path);
        // Refused rather than normalised. Nothing here serves a file off disk,
        // so a `..` in a path is not a traversal — but it is also not a route
        // that exists, and a request shaped like an attack should be answered
        // like one rather than quietly rewritten into something that works.
        if !path.starts_with('/') || path.contains("..") || path.contains('\0') {
            return Err((400, "unacceptable path"));
        }

        let query = raw_query
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|pair| match pair.split_once('=') {
                Some((k, v)) => (percent_decode(k), percent_decode(v)),
                None => (percent_decode(pair), String::new()),
            })
            .collect();

        // Headers are read to the blank line and mostly discarded: the token is
        // a query parameter precisely so that a link is the whole credential.
        // Two of them are kept, because a body cannot be read without knowing
        // how long it is and must not be trusted without knowing what it claims
        // to be. They all have to leave the socket either way, or a client that
        // pipelines sees its next request answered with the tail of this one's
        // headers.
        let mut header_bytes = line.len();
        let mut length: Option<usize> = None;
        let mut json_content_type = false;
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(0) => break,
                Ok(_) if header.trim().is_empty() => break,
                Ok(n) => {
                    header_bytes += n;
                    if header_bytes > MAX_HEADER_BYTES {
                        return Err((431, "request headers too large"));
                    }
                    let Some((name, value)) = header.split_once(':') else {
                        continue;
                    };
                    let value = value.trim();
                    if name.eq_ignore_ascii_case("content-length") {
                        // A length that is not a number is not a length. Left
                        // as `None` so the body reads as absent rather than as
                        // whatever the digits before the junk happened to say.
                        length = value.parse::<usize>().ok();
                    } else if name.eq_ignore_ascii_case("content-type") {
                        // `application/json; charset=utf-8` is the same claim as
                        // `application/json`, and a browser sends either.
                        json_content_type = value
                            .split(';')
                            .next()
                            .unwrap_or_default()
                            .trim()
                            .eq_ignore_ascii_case("application/json");
                    }
                }
                // Hitting the `take` limit surfaces here, as does a header that
                // is not UTF-8. Both are `431` rather than `400`: it is the
                // size of the field, not the shape of it, that we objected to.
                Err(_) => return Err((431, "request headers too large")),
            }
        }

        // Only for the method that has one. A `Content-Length` on a `GET` is
        // either a mistake or an attempt at request smuggling, and reading the
        // bytes it names would make this server agree with the wrong one of two
        // hops about where the next request starts.
        let mut body = Vec::new();
        if method == "POST" {
            let want = length.ok_or((411, "a POST needs a Content-Length"))?;
            let cap = match path.starts_with(IMAGE_PATH) {
                true => MAX_IMAGE_BYTES,
                false => MAX_BODY_BYTES,
            };
            if want > cap {
                return Err((413, "request body too large"));
            }
            body = vec![0u8; want];
            if reader.read_exact(&mut body).is_err() {
                return Err((400, "request body ended early"));
            }
        }

        Ok(Request {
            method,
            path,
            query,
            body,
            json_content_type,
        })
    }

    /// Whether this is a `POST` whose body announced itself as JSON.
    ///
    /// The one guard that a route taking an action checks before anything else.
    /// See the module docs for why the content type is what makes a token in a
    /// URL safe to act on.
    pub fn wants_json(&self) -> bool {
        self.method == "POST" && self.json_content_type
    }

    /// The body parsed as a JSON object, or why it could not be.
    pub fn json(&self) -> Result<serde_json::Value, (u16, &'static str)> {
        let value: serde_json::Value =
            serde_json::from_slice(&self.body).map_err(|_| (400, "body is not JSON"))?;
        match value.is_object() {
            true => Ok(value),
            false => Err((400, "body is not a JSON object")),
        }
    }

    /// The `t` query parameter, which is where the access token lives.
    pub fn token(&self) -> &str {
        self.query.get("t").map_or("", String::as_str)
    }
}

/// Decode `%XX` escapes and `+`, leaving anything malformed as written.
///
/// A stray `%` in a path is far likelier to be a literal than a truncated
/// escape, and turning it into a replacement character would make the path fail
/// to match a route for a reason nobody could see.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The reason phrase for the statuses this server actually sends.
fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// Headers sent on every response, whatever it carries.
///
/// The page is entirely self-contained — its CSS and JS are inlined by the
/// build, and it fetches nothing — so the strictest possible policy costs
/// nothing and closes the gap where a session title, a branch name or a file
/// path from someone's transcript is rendered as markup.
///
/// `frame-ancestors 'none'` and `X-Content-Type-Options` are the pair that
/// matter beyond that: without them a page on another origin can frame this one
/// and read what it renders, or talk a browser into sniffing a JSON response as
/// something executable.
///
/// The session's terminal is a link out of this page and not a frame in it:
/// rmux's browser terminal answers with `frame-ancestors 'none'`, so no policy
/// written here could embed it. The policy therefore stays at `default-src
/// 'none'` with nothing framed at all.
fn common_headers(out: &mut String) {
    out.push_str(
        "X-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'none'; \
         style-src 'unsafe-inline'; \
         script-src 'unsafe-inline'; \
         img-src data:; \
         connect-src 'self'; \
         base-uri 'none'; \
         form-action 'none'; \
         frame-ancestors 'none'\r\n",
    );
}

/// Write a complete response and let the connection close.
///
/// `HEAD` is answered with the headers a `GET` would have carried, length
/// included, and no body — which is what a browser preflighting a link expects,
/// and costs one branch here rather than a route that has to know about it.
pub fn respond(
    stream: &mut TcpStream,
    request: Option<&Request>,
    status: u16,
    content_type: &str,
    body: &[u8],
) {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         Cache-Control: no-store\r\n",
        reason(status),
        body.len(),
    );
    common_headers(&mut head);
    head.push_str("\r\n");

    let head_only = request.is_some_and(|r| r.method == "HEAD");
    // One write where the platform allows it: a header block and a small body
    // in two syscalls arrive as two segments, and the browser paints the second
    // one a round trip later.
    let mut buf = head.into_bytes();
    if !head_only {
        buf.extend_from_slice(body);
    }
    let _ = stream.write_all(&buf);
    let _ = stream.flush();
}

/// Send a plain-text error, the shape every refusal in the router takes.
pub fn respond_error(stream: &mut TcpStream, request: Option<&Request>, status: u16, msg: &str) {
    respond(
        stream,
        request,
        status,
        "text/plain; charset=utf-8",
        format!("{status} {}: {msg}\n", reason(status)).as_bytes(),
    );
}

/// An open `text/event-stream`, held for as long as the client keeps reading.
pub struct EventStream<'a> {
    stream: &'a mut TcpStream,
}

impl<'a> EventStream<'a> {
    /// Send the SSE preamble, or fail if the client has already gone.
    pub fn open(stream: &'a mut TcpStream) -> std::io::Result<EventStream<'a>> {
        let mut head = String::from(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: text/event-stream; charset=utf-8\r\n\
             Cache-Control: no-store\r\n\
             Connection: close\r\n",
        );
        common_headers(&mut head);
        head.push_str("\r\n");
        stream.write_all(head.as_bytes())?;
        stream.flush()?;
        Ok(EventStream { stream })
    }

    /// Send one named event carrying `data`.
    ///
    /// An error here means the client is gone — a phone that locked, a tab that
    /// closed — which is the ordinary way one of these ends rather than a
    /// fault, so the caller's job on `Err` is to return, not to report.
    pub fn send(&mut self, event: &str, data: &str) -> std::io::Result<()> {
        let mut frame = format!("event: {event}\n");
        // Every line of the payload needs its own `data:` prefix or the stream
        // desynchronises. JSON from `to_string` holds no newlines today, which
        // is exactly the kind of thing that stops being true quietly.
        for line in data.split('\n') {
            frame.push_str("data: ");
            frame.push_str(line);
            frame.push('\n');
        }
        frame.push('\n');
        self.stream.write_all(frame.as_bytes())?;
        self.stream.flush()
    }

    /// Send a comment, which SSE ignores and every hop in between does not.
    ///
    /// The point is the bytes, not the content: a proxy or a phone radio that
    /// drops an idle connection needs traffic to count the stream as alive, and
    /// a browser that has genuinely gone away only surfaces as a write error
    /// once something is written to it.
    pub fn keepalive(&mut self) -> std::io::Result<()> {
        self.stream.write_all(b": keepalive\n\n")?;
        self.stream.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding_handles_escapes_and_plus() {
        assert_eq!(percent_decode("/a%2Fb"), "/a/b");
        assert_eq!(percent_decode("hello+world"), "hello world");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("caf%C3%A9"), "café");
    }

    #[test]
    fn a_truncated_escape_stays_literal() {
        // The trailing `%2` cannot be an escape, and turning it into one would
        // silently change the path being asked for.
        assert_eq!(percent_decode("/x%2"), "/x%2");
    }

    #[test]
    fn every_status_the_router_sends_has_a_reason() {
        for status in [200, 400, 403, 404, 405, 409, 411, 413, 431, 503] {
            assert_ne!(reason(status), "Error", "status {status} has no reason");
        }
    }

    #[test]
    fn the_policy_forbids_loading_anything_off_the_network() {
        let mut headers = String::new();
        common_headers(&mut headers);
        assert!(headers.contains("default-src 'none'"));
        assert!(headers.contains("frame-ancestors 'none'"));
        assert!(headers.contains("nosniff"));
    }
}
