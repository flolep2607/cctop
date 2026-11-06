//! `/api/move/…`: a session moving here from a sibling on the account — see
//! [`cctop_core::migrate`] for what a move is, and
//! [`cctop_core::migrate::wire::Tunnel`] for the side that asks.
//!
//! Only another cctop may ask. A move writes into this machine's
//! repositories and starts an agent, so of all the ways into this server only
//! a sibling's signature opens it: not a token link, not a cookie, not an
//! Access login, however much they are worth elsewhere. A person moves a
//! session from the cctop that has it, which is a sibling of this one; nobody
//! moves one here by hand.
//!
//! The signature covers the method and the path, not the body, so every body
//! is named by its sha256 as the path's last word and refused when it does not
//! match. What was signed then pins what was sent.

use std::net::TcpStream;

use cctop_core::migrate::wire::{body_sha, chunk_data};
use cctop_core::migrate::{Answer, Ask, Manifest, Ticket, receive};

use crate::Shared;
use crate::http::{self, Request};

pub(crate) fn route(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    rest: &str,
    from_peer: bool,
) {
    if !from_peer {
        return http::respond_error(
            stream,
            Some(request),
            403,
            "a session moves here only from another cctop on your account",
        );
    }
    if !shared.actions {
        return http::respond_error(
            stream,
            Some(request),
            403,
            "this cctop was started read-only and takes no moved sessions",
        );
    }
    let words: Vec<&str> = rest.split('/').collect();
    if request.method == "GET" {
        return match words.as_slice() {
            ["hello"] => http::json(
                stream,
                request,
                &Answer::Ok(receive::hello(cctop_core::update::current_version())),
            ),
            _ => http::respond_error(stream, Some(request), 404, "no such page"),
        };
    }
    if request.method != "POST" {
        return http::respond_error(stream, Some(request), 405, "method not allowed");
    }
    let Some((sha, words)) = words.split_last() else {
        return http::respond_error(stream, Some(request), 404, "no such page");
    };
    if body_sha(&request.body) != *sha {
        return http::respond_error(
            stream,
            Some(request),
            400,
            "the body is not the one that was signed",
        );
    }
    let body = &request.body;
    let answer = match words {
        ["plan"] => to_json(parse::<Ask>(body).and_then(|ask| receive::plan(&ask))),
        ["begin"] => to_json(parse::<Manifest>(body).and_then(|m| receive::begin(&m))),
        ["chunk", id, seq] => to_json(
            seq.parse::<u64>()
                .map_err(|_| "that is not a chunk number".to_string())
                .and_then(|seq| Ok((seq, chunk_data(body)?)))
                .and_then(|(seq, data)| receive::chunk(id, seq, &data)),
        ),
        ["commit", id] => to_json(receive::commit(id)),
        ["status", id] => to_json(receive::status(id)),
        ["finish"] => to_json(parse::<Ticket>(body).and_then(|t| receive::finish(&t))),
        ["abort"] => to_json(parse::<Ticket>(body).and_then(|t| receive::abort(&t))),
        _ => return http::respond_error(stream, Some(request), 404, "no such page"),
    };
    http::respond_json(stream, request, &answer);
}

fn parse<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, String> {
    serde_json::from_slice(body).map_err(|e| format!("the request: {e}"))
}

/// Every answer, refusals included, is a 200 carrying `{"ok": …}` or
/// `{"error": "…"}`: the sender shows the error as it is, and keeps the
/// statuses for what went wrong before anyone here understood the question.
fn to_json<T: serde::Serialize>(result: Result<T, String>) -> String {
    serde_json::to_string(&Answer::from(result))
        .unwrap_or_else(|e| format!("{{\"error\":\"could not render the answer: {e}\"}}"))
}
