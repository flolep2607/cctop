//! The session routes for a row this machine does not own.
//!
//! A row from `--host` is answered by running cctop over ssh on that host, and
//! a row from a sibling serve on the account's tunnel by forwarding the request
//! to it. Both live apart from [`crate::api`] because what they share is the
//! trip, not the route.

use crate::api::{chat_json, find, may_act};
use crate::{Access, Shared, current, http};
use cctop_core::reach::Reach;
use cctop_core::session::Session;
use cctop_core::{actions, chat, export};
use http::Request;
use std::net::TcpStream;
use std::sync::Arc;

/// The sibling whose row a session route names, when it is one — not this
/// machine's row, and not an ssh host's, which [`remote_json`] reads.
pub(crate) fn peer_of(shared: &Shared, path: &str) -> Option<Arc<cctop_core::peer::Peer>> {
    let id = [
        "/api/chat/",
        "/api/report/",
        "/api/access/",
        "/api/handoff/",
    ]
    .iter()
    .find_map(|prefix| path.strip_prefix(prefix))
    .map(|rest| rest.trim_end_matches("/markdown"))
    .or_else(|| {
        path.strip_prefix("/api/act/")
            .and_then(|rest| rest.split_once('/'))
            .map(|(_, id)| id)
    })?;
    let snapshot = current(shared);
    let host = &find(&snapshot.sessions, id)?.remote.as_ref()?.host;
    // An offline sibling is not forwarded to: the route falls through to
    // [`remote_text`], which says it is offline rather than timing out on it.
    match cctop_core::reach::reach(host, shared.hosts.get(host)) {
        Ok(Reach::Peer(peer)) => Some(peer),
        Ok(Reach::Ssh(_)) | Err(_) => None,
    }
}

/// Relay a request to a sibling cctop, signed as this machine, and hand back
/// what it answered.
///
/// The sibling takes the signature at full level, so this server decides who
/// may ask: a read for any link, an action (a JSON `POST`) only through
/// [`may_act`], and a terminal's socket only for a full link with actions on
/// — the same lines [`relay_terminal`] draws for this machine's own. Only the
/// sibling's API and terminal sockets are relayed, never its pages, and not
/// its event stream, which this server's own already carries the rows of.
///
/// The browser's credentials stay here: the `t` query parameter is dropped
/// and no header of the browser's but a socket handshake's goes on.
pub(crate) fn forward(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    peer: &cctop_core::peer::Peer,
    path: &str,
    access: Access,
) {
    if path.starts_with("/rmux-ws/") {
        if access != Access::Full || !shared.actions {
            return http::respond_error(
                stream,
                Some(request),
                403,
                "this link cannot open terminals",
            );
        }
        if !request.is_websocket() {
            return http::respond_error(
                stream,
                Some(request),
                400,
                "a terminal socket is a WebSocket",
            );
        }
        return match peer.socket(path, &without_token(request.raw_query()), request.headers()) {
            Ok(upstream) => upstream.pump(stream),
            Err(why) => http::respond_error(stream, Some(request), 502, &why),
        };
    }
    if !path.starts_with("/api/") || path == "/api/events" {
        return http::respond_error(
            stream,
            Some(request),
            404,
            "only a sibling's API is relayed",
        );
    }
    let mut body = None;
    let mut page_origin = None;
    if request.method == "POST" {
        let Some(mut json) = may_act(shared, stream, request, access) else {
            return;
        };
        // A terminal is minted for the sibling's own copy of the frame app
        // and its own relay; the link that comes back is pointed at this
        // page's relay below. A tab's terminal too — what the launcher opens
        // after starting an agent there, which has no session yet to name.
        let tab_terminal = path.starts_with("/api/tab/") && path.ends_with("/terminal");
        if path.starts_with("/api/act/terminal/") || tab_terminal {
            page_origin = json
                .get("origin")
                .and_then(serde_json::Value::as_str)
                .filter(|o| actions::frontend_for(o).is_some())
                .map(str::to_string);
            json["origin"] = serde_json::Value::String(peer.url.clone());
        }
        body = Some(json.to_string().into_bytes());
    }
    let method = match request.method.as_str() {
        "POST" => "POST",
        _ => "GET",
    };
    let answer = match peer.send(
        method,
        path,
        &without_token(request.raw_query()),
        body.as_deref(),
    ) {
        Ok(answer) => answer,
        Err(why) => {
            return http::respond_error(
                stream,
                Some(request),
                502,
                &format!("{} could not answer: {why}", peer.machine),
            );
        }
    };
    let mut answered = answer.body;
    if let Some(origin) = page_origin
        && answer.status == 200
        && let Ok(mut terminal) = serde_json::from_slice::<serde_json::Value>(&answered)
    {
        let relayed = terminal
            .get("url")
            .and_then(serde_json::Value::as_str)
            .and_then(|url| cctop_core::peer::relayed_link(url, &peer.url, &origin, &peer.machine));
        match relayed {
            Some(url) => terminal["url"] = serde_json::Value::String(url),
            // A link whose socket does not go through the sibling's relay is
            // one this page cannot reach: said, rather than framed dead.
            None => {
                return http::respond_error(
                    stream,
                    Some(request),
                    502,
                    &format!("{}'s terminal link cannot be relayed", peer.machine),
                );
            }
        }
        answered = terminal.to_string().into_bytes();
    }
    http::respond(
        stream,
        Some(request),
        answer.status,
        &answer.content_type,
        &answered,
    );
}

/// A raw query string less its `t` parameter, which is this server's
/// credential and nobody else's business.
pub(crate) fn without_token(query: &str) -> String {
    query
        .split('&')
        .filter(|pair| !pair.is_empty() && *pair != "t" && !pair.starts_with("t="))
        .collect::<Vec<_>>()
        .join("&")
}

/// A remote session's conversation, narrowed to what the page lacks.
///
/// A remote row has no file here to stamp, so its stamp is a hash of the
/// answer itself: the same conversation hashes the same, which is all a stamp
/// has to do.
///
/// Parsed even when there is nothing to narrow, because the replies are
/// rendered to HTML here and not on the far side — see [`chat_json`].
pub(crate) fn remote_chat(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    session: &Session,
    args: &[String],
    since: &chat::Since,
) {
    let Some(host) = session
        .remote
        .as_ref()
        .and_then(|r| shared.hosts.get(&r.host))
    else {
        return remote_json(shared, stream, request, session, args);
    };
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let body = match host.run(&argv) {
        Ok(body) => body,
        Err(_) => return remote_json(shared, stream, request, session, args),
    };
    let Ok(mut conversation) = serde_json::from_str::<chat::Conversation>(&body) else {
        return http::respond_json(stream, request, &body);
    };
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    body.hash(&mut hasher);
    conversation.stamp = Some(format!("r{:016x}", hasher.finish()));
    chat_json(stream, request, conversation.narrowed(since));
}

/// A remote row's export, rendered by the cctop that holds the transcript.
///
/// A peer older than `--export` rejects the flag, and an export that refused
/// to work against it would be the only session route that did. So it falls
/// back to the question every peer answers — `--chat`, the page's window — and
/// renders that here, which the document then says is partial.
pub(crate) fn remote_markdown(
    shared: &Shared,
    session: &Session,
    options: export::Options,
) -> Result<String, String> {
    let id = session.session_id.as_str();
    let mut args = vec!["--export", id];
    if options.tool_output {
        args.push("--tool-output");
    }
    let first = match remote_text(shared, session, &args) {
        Ok(text) => return Ok(text),
        Err(why) => why,
    };
    let body = remote_text(shared, session, &["--chat", id]).map_err(|_| first.clone())?;
    let conversation = serde_json::from_str::<chat::Conversation>(&body).map_err(|_| first)?;
    Ok(export::render(session, &conversation, options))
}

/// Run a cctop command on the machine a remote row came from, and return what
/// it printed — [`remote_json`] for a caller that wants the text in hand.
pub(crate) fn remote_text(
    shared: &Shared,
    session: &Session,
    args: &[&str],
) -> Result<String, String> {
    let remote = session
        .remote
        .as_ref()
        .expect("only a remote row reaches remote_text or remote_json");
    let host = match cctop_core::reach::reach(&remote.host, shared.hosts.get(&remote.host)) {
        Ok(Reach::Ssh(host)) => host,
        // A sibling's routes are forwarded to it whole before they get here
        // ([`peer_of`]), so this is one asked between its discovery and the
        // route's — the next request goes through.
        Ok(Reach::Peer(_)) => {
            return Err(format!(
                "{} was found on the Cloudflare account mid-request — ask again",
                remote.host
            ));
        }
        Err(why) => return Err(format!("cannot reach this session: {why}")),
    };
    // The far side's own words are usually the answer: "command not found",
    // "no session id starts with …", or — for a cctop older than these flags —
    // clap's "unexpected argument".
    host.run(args)
        .map_err(|why| format!("{} could not answer: {why}", remote.host))
}

/// Answer a session route for a remote row by asking the cctop on the machine
/// it came from, and relay the body verbatim.
///
/// The far side runs `cctop --report`, `--chat` or `--access` over the same ssh
/// channel `--host` polls with, so a remote report is as reachable as the row
/// was. Building the answer here instead would read whatever happens to live
/// at the same path on *this* filesystem — the failure `Session::remote`
/// exists to prevent.
pub(crate) fn remote_json(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    session: &Session,
    args: &[String],
) {
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    match remote_text(shared, session, &argv) {
        Ok(body) => http::respond_json(stream, request, &body),
        // 502 for both of `remote_text`'s failures. The row is real — it
        // arrived over ssh — so a host this run does not know is a spec that
        // was dropped between polls, not a session that does not exist, and
        // a 404 would say the wrong one of those.
        Err(why) => http::respond_error(stream, Some(request), 502, &why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hubs_token_stays_on_the_hub() {
        assert_eq!(without_token("t=secret&before=4"), "before=4");
        assert_eq!(without_token("agent=x&t=secret"), "agent=x");
        assert_eq!(without_token("t"), "");
        assert_eq!(without_token("top=1"), "top=1");
    }
}
