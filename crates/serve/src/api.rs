//! The session API's handlers, one function per route the router dispatches
//! to.
//!
//! The gate is not here: [`crate::serve_connection`] has already decided who
//! is asking and with what [`Access`] before any of these runs, which is what
//! keeps a forgotten check in one handler from being a hole.

use crate::pollers::lock_provider_status;
use crate::remote::{remote_chat, remote_json, remote_markdown, remote_text};
use crate::{Access, NO_SUCH_SESSION, Shared, current, http, markdown, search};
use cctop_core::session::Session;
use cctop_core::{actions, chat, export, report};
use http::{Request, json};
use std::net::TcpStream;

/// `/api/provider-status`: the pages' last answers, set against the rows being
/// published now.
///
/// Served from memory: a request never waits on a status page, and offline it
/// answers as fast as any other route — every page pending or unreachable, and
/// nothing to say.
pub(crate) fn provider_status_route(shared: &Shared, stream: &mut TcpStream, request: &Request) {
    let status = lock_provider_status(shared).clone();
    let snapshot = current(shared);
    let document = cctop_core::provider_status::document(&status, &snapshot.sessions);
    json(stream, request, &document);
}

/// Both transcript-search tiers over the current snapshot.
pub(crate) fn api_search(shared: &Shared, stream: &mut TcpStream, request: &Request) {
    let needle = request
        .query
        .get("q")
        .map(|q| q.trim().to_lowercase())
        .unwrap_or_default();
    // Under a few characters a query is a keystroke rather than a question;
    // answering one costs a scan of every transcript for a result that is
    // nearly all of them.
    if needle.chars().count() < search::MIN_CHARS {
        return json(stream, request, &serde_json::json!({ "hits": [] }));
    }
    let snapshot = current(shared);
    let hits = search::run(&shared.topics, &snapshot.sessions, &needle);
    json(stream, request, &serde_json::json!({ "hits": hits }))
}

/// The text `cctop optimize` or `cctop compare` would print, as plain text.
///
/// Priced the way the report route is: the analysis re-parses every transcript
/// because the tool calls it reasons about are never cached, so it happens on
/// request rather than on the refresh clock — the same cost the CLI pays when
/// asked the same question.
pub(crate) fn api_insight(shared: &Shared, stream: &mut TcpStream, request: &Request, which: &str) {
    let analyses = cctop_core::insight::scan(shared.plan);
    let selected = cctop_core::insight::only(&analyses, None);
    let text = match which {
        "optimize" => cctop_core::insight::optimize::report(&selected),
        _ => cctop_core::insight::compare::report(&selected),
    };
    http::respond(
        stream,
        Some(request),
        200,
        "text/plain; charset=utf-8",
        text.as_bytes(),
    );
}

/// Serve the conversation for one session.
///
/// `?before=<n>` pages backwards: the window returned ends just before turn
/// `n` rather than at the newest, which is how the page reaches turns the
/// first response counted but did not send.
pub(crate) fn api_chat(shared: &Shared, stream: &mut TcpStream, request: &Request, id: &str) {
    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
    };
    let before = request
        .query
        .get("before")
        .and_then(|v| v.parse::<usize>().ok());
    let since = chat::Since {
        stamp: request
            .query
            .get("since")
            .cloned()
            .filter(|s| !s.is_empty()),
        after: request.query.get("after").and_then(|v| v.parse().ok()),
    };
    let agent = request.query.get("agent").filter(|a| !a.is_empty());
    // Checked here as well as by the read, so a remote row never forwards an
    // id the session does not list to the far machine's command line.
    if let Some(agent) = agent
        && !session.subagents.iter().any(|s| &s.agent_id == agent)
    {
        return http::respond_error(stream, Some(request), 404, "no such agent in this session");
    }
    if session.remote.is_some() {
        let mut args = vec!["--chat".to_string(), session.session_id.clone()];
        if let Some(before) = before {
            args.extend(["--before".to_string(), before.to_string()]);
        }
        // A peer older than `--agent` rejects it, and the page says so in the
        // agent's block rather than anywhere that would break the session.
        if let Some(agent) = agent {
            args.extend(["--agent".to_string(), agent.clone()]);
        }
        // The peer is asked the old question, because a cctop older than
        // `since` would reject the flag and the chat would not open at all.
        // The narrowing happens here instead: the ssh read is still whole,
        // but the browser gets — and redraws — only what changed.
        return remote_chat(shared, stream, request, session, &args, &since);
    }
    if let Some(agent) = agent {
        return match chat::build_agent(session, agent, before) {
            Some(conversation) => chat_json(stream, request, conversation.narrowed(&since)),
            None => {
                http::respond_error(stream, Some(request), 404, "no such agent in this session")
            }
        };
    }
    // Before the read, because skipping the read is the point: an unchanged
    // file is answered from its metadata alone.
    if let Some(path) = session.data_file.as_deref()
        && since.stamp.is_some()
        && since.stamp == chat::stamp_of(path)
    {
        return json(
            stream,
            request,
            &chat::Conversation {
                supported: true,
                stamp: since.stamp,
                unchanged: true,
                ..Default::default()
            },
        );
    }
    chat_json(
        stream,
        request,
        chat::build(session, before).narrowed(&since),
    );
}

/// Answer with a conversation, its replies rendered to HTML for the page —
/// see [`markdown`]. Rendered here, on the way out, rather than where it is
/// read: a peer's `--chat` and the TUI have no use for it, and a remote
/// session's turns arrive without it from a cctop of any age.
pub(crate) fn chat_json(
    stream: &mut TcpStream,
    request: &Request,
    mut conversation: chat::Conversation,
) {
    markdown::render(&mut conversation);
    json(stream, request, &conversation);
}

/// Serve one session's conversation as a markdown document to paste elsewhere.
///
/// `?variant=` picks the document: `conversation` (the default) is every turn
/// with its calls listed, `tools` adds each call's result, and `brief` is the
/// handoff brief — the summary [`cctop_core::handoff`] writes for exactly the
/// paste-into-another-agent case, offered beside the transcript because on a
/// long session it is the one that fits.
///
/// Text rather than JSON, because the body *is* the answer: the page copies or
/// downloads it as it arrives, and `curl` gets a file it can use.
pub(crate) fn api_chat_markdown(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    id: &str,
) {
    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
    };
    let variant = request.query.get("variant").map(String::as_str);
    let options = export::Options {
        tool_output: variant == Some("tools"),
    };
    let markdown = match (variant, session.remote.is_some()) {
        (Some("brief"), false) => {
            let data = shared.store.session_data_fresh(session);
            cctop_core::handoff::build(session, Some(&data)).to_markdown()
        }
        (Some("brief"), true) => {
            match remote_text(shared, session, &["--handoff", &session.session_id]) {
                Ok(text) => text,
                Err(why) => return http::respond_error(stream, Some(request), 502, &why),
            }
        }
        (_, false) => export::render(session, &chat::whole(session), options),
        (_, true) => match remote_markdown(shared, session, options) {
            Ok(text) => text,
            Err(why) => return http::respond_error(stream, Some(request), 502, &why),
        },
    };
    http::respond(
        stream,
        Some(request),
        200,
        "text/markdown; charset=utf-8",
        markdown.as_bytes(),
    );
}

/// Serve what one session can reach.
pub(crate) fn api_access(shared: &Shared, stream: &mut TcpStream, request: &Request, id: &str) {
    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
    };
    if session.remote.is_some() {
        return remote_json(
            shared,
            stream,
            request,
            session,
            &["--access".to_string(), session.session_id.clone()],
        );
    }
    // The tool counts are the one part of this that needs the transcript, and
    // the rest of the answer is worth having without it — so a session whose
    // extraction fails still reports its instructions, skills and servers.
    let data = shared.store.session_data_fresh(session);
    json(
        stream,
        request,
        &cctop_core::access::build(session, Some(&data)),
    );
}

/// The guards every acting route shares, answered here once: a read-only link,
/// a `--no-actions` serve, and a request that is not a JSON POST each get their
/// own answer rather than a generic refusal. Returns the parsed body when the
/// request may act — a route that forgets to go through this is the bug the
/// shared shape exists to prevent.
pub(crate) fn may_act(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    access: Access,
) -> Option<serde_json::Value> {
    // Before every other guard, because it is not one: this is the link doing
    // what it was minted to do, and the answer names that rather than leaning
    // on a flag the read-only page never had.
    if access == Access::ReadOnly {
        http::respond_error(stream, Some(request), 403, "this link is read-only");
        return None;
    }
    if !shared.actions {
        http::respond_error(
            stream,
            Some(request),
            403,
            "this cctop serve is read-only — restart it without --no-actions, \
             and with a token, to act on a session",
        );
        return None;
    }
    // A `POST` with a JSON body, both of which are load-bearing: see the
    // module docs in `http` for why a form on another origin cannot be one.
    if !request.wants_json() {
        http::respond_error(
            stream,
            Some(request),
            405,
            "an action is a POST with a JSON body",
        );
        return None;
    }
    match request.json() {
        Ok(body) => Some(body),
        Err((status, why)) => {
            http::respond_error(stream, Some(request), status, why);
            None
        }
    }
}

/// Do something to a session: `/api/act/<verb>/<id>`.
///
/// One route for the three verbs rather than three, because the guards in front
/// of them are the whole security surface and they are identical — a route added
/// later that forgets one of them is the bug this shape prevents.
/// `POST /api/tab/<name>/terminal`: a tab's terminal, behind the same guards
/// as every action — the token, a JSON POST, a serve that allows actions.
pub(crate) fn api_tab(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    rest: &str,
    access: Access,
) {
    let Some(body) = may_act(shared, stream, request, access) else {
        return;
    };
    let Some(name) = rest.strip_suffix("/terminal") else {
        return http::respond_error(stream, Some(request), 404, "no such action");
    };
    let frontend = body
        .get("origin")
        .and_then(serde_json::Value::as_str)
        .and_then(actions::frontend_for);
    let fresh = body.get("fresh").and_then(serde_json::Value::as_bool) == Some(true);
    match actions::tab_terminal(name, frontend.as_deref(), fresh) {
        Ok(terminal) => json(stream, request, &terminal),
        Err((status, why)) => http::respond_error(stream, Some(request), status, &why),
    }
}

pub(crate) fn api_act(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    rest: &str,
    access: Access,
) {
    let Some(body) = may_act(shared, stream, request, access) else {
        return;
    };
    let Some((verb, id)) = rest.split_once('/') else {
        return http::respond_error(stream, Some(request), 404, "no such action");
    };

    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
    };

    let field = |name: &str| {
        body.get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    // Also out of shape, and also not about the session it names: an image is
    // filed on this machine and the answer is where. The session id rides along
    // so the route is the same shape as its neighbours — one place that checks
    // the token, insists on a JSON POST, and refuses a read-only serve.
    if verb == "image" {
        return match actions::image(&field("data")) {
            Ok(filed) => json(stream, request, &filed),
            Err((status, why)) => http::respond_error(stream, Some(request), status, &why),
        };
    }
    // Answered before the others because it does not answer in their shape: a
    // terminal is a link and a reach, not a sentence about what was done.
    if verb == "terminal" {
        let frontend = actions::frontend_for(&field("origin"));
        let fresh = body.get("fresh").and_then(serde_json::Value::as_bool) == Some(true);
        return match actions::terminal(session, frontend.as_deref(), fresh) {
            Ok(terminal) => json(stream, request, &terminal),
            Err((status, why)) => http::respond_error(stream, Some(request), status, &why),
        };
    }
    let outcome = match verb {
        "send" => actions::send(session, &field("text")),
        "answer" => actions::answer(session, &field("choice")),
        "yolo" => match body.get("on").and_then(serde_json::Value::as_bool) {
            Some(on) => actions::yolo(session, on, cctop_core::yolo_log::Origin::Web),
            None => Err((400, "YOLO is `{\"on\": true}` or `{\"on\": false}`".into())),
        },
        "resume" => actions::resume(session),
        "handoff" => {
            // The brief is built from the extraction, so this one pays for a
            // transcript read — the same read the report route makes, and for
            // the same reason: a brief assembled from the row alone carries the
            // header and none of the work.
            let data = shared.store.session_data_fresh(session);
            actions::handoff(
                session,
                Some(&data),
                &field("agent"),
                Some(&field("account")),
            )
        }
        _ => return http::respond_error(stream, Some(request), 404, "no such action"),
    };
    match outcome {
        Ok(done) => json(stream, request, &done),
        Err((status, why)) => http::respond_error(stream, Some(request), status, &why),
    }
}

/// Build and send one session's report.
pub(crate) fn api_report(shared: &Shared, stream: &mut TcpStream, request: &Request, id: &str) {
    let snapshot = current(shared);
    let Some(session) = find(&snapshot.sessions, id) else {
        return http::respond_error(stream, Some(request), 404, NO_SUCH_SESSION);
    };

    // A remote row names a transcript on the machine it came from, so the
    // report is built there and the body relayed — never parsed here.
    if session.remote.is_some() {
        return remote_json(
            shared,
            stream,
            request,
            session,
            &["--report".to_string(), session.session_id.clone()],
        );
    }

    // The one expensive call in the whole server, and the reason it is on this
    // route alone: the report is built from tool arguments and per-request
    // context readings, neither of which the cache carries.
    let data = shared.store.session_data_fresh(session);
    let built = report::build(session, &data, shared.plan);
    match serde_json::to_string(&built) {
        Ok(body) => http::respond_json(stream, request, &body),
        Err(e) => http::respond_error(
            stream,
            Some(request),
            503,
            &format!("could not render the report: {e}"),
        ),
    }
}

/// Find a session by id, or by a prefix that matches exactly one.
///
/// Prefixes because session ids are uuids and the report link is a thing people
/// paste into chat. An ambiguous prefix resolves to nothing rather than to the
/// first match — the wrong session's costs is a worse answer than none. The
/// rules are core's, shared with every other face that takes an id.
pub(crate) fn find<'a>(sessions: &'a [Session], id: &str) -> Option<&'a Session> {
    cctop_core::session::lookup::lookup(sessions, id).found()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Snapshot;
    use crate::tests::{response_of, session, shared, status_of};
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::Arc;

    #[test]
    fn sessions_resolve_by_id_and_by_unambiguous_prefix() {
        let rows = vec![session("abc123"), session("abd999"), session("zz")];
        assert_eq!(find(&rows, "abc123").unwrap().session_id, "abc123");
        assert_eq!(find(&rows, "abc").unwrap().session_id, "abc123");
        assert_eq!(find(&rows, "zz").unwrap().session_id, "zz");
    }

    #[test]
    fn an_ambiguous_prefix_resolves_to_nothing() {
        let rows = vec![session("abc123"), session("abd999")];
        // `ab` matches both. Answering with either one would put another
        // session's costs under this one's heading.
        assert!(find(&rows, "ab").is_none());
        assert!(find(&rows, "").is_none());
        assert!(find(&rows, "nope").is_none());
    }

    /// Ask `api_search` the way the router does — a request parsed off a real
    /// socket — and hand back whatever it wrote.
    fn api_search_body(shared: &Shared, target: &str) -> String {
        use std::io::Read;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client
            .write_all(format!("GET {target} HTTP/1.1\r\n\r\n").as_bytes())
            .unwrap();
        let request = Request::parse(&server).unwrap();
        api_search(shared, &mut server, &request);
        // Closing the answering half is what lets the client read the response
        // to its end rather than waiting out the connection.
        drop(server);
        let mut raw = String::new();
        client.read_to_string(&mut raw).unwrap();
        raw
    }

    /// A request with no `q` — or one shorter than a question — is an empty
    /// answer, not an error and not a scan of every transcript.
    #[test]
    fn a_search_without_a_real_query_finds_nothing() {
        let shared = shared("", "");
        for target in ["/api/search", "/api/search?q=", "/api/search?q=ab"] {
            let body = api_search_body(&shared, target);
            assert!(body.contains("\"hits\":[]"), "{target}: {body}");
        }
    }

    /// The route end to end, minus the listener: a session whose transcript
    /// holds the word comes back under `session_id`, the field the page joins
    /// its rows on.
    ///
    /// Only the hit direction is pinned here. A miss asks the topical tier,
    /// which on a machine with the model fetched would load it and rewrite the
    /// real embedding cache — a side effect no unit test should have; the
    /// literal miss itself is covered in `session::search`'s tests.
    #[test]
    fn a_search_names_the_session_whose_transcript_matched() {
        let path =
            std::env::temp_dir().join(format!("cctop-serve-search-{}.jsonl", std::process::id()));
        std::fs::write(&path, "{\"text\":\"please fix the flywheel\"}\n").unwrap();
        let mut s = Session::new(cctop_core::pricing::Provider::Claude, "sess-1".into());
        s.data_file = Some(path.clone());

        let shared = shared("", "");
        *shared.latest.lock().unwrap() = Arc::new(Snapshot {
            version: 1,
            json: "[]".to_string(),
            rows: Vec::new(),
            sessions: vec![s],
            host_errors: Vec::new(),
        });

        let body = api_search_body(&shared, "/api/search?q=flywheel");
        let _ = std::fs::remove_file(path);
        assert!(body.contains("\"session_id\":\"sess-1\""), "{body}");
        assert!(body.contains("flywheel"), "{body}");
    }

    /// The `/api/provider-status` document for this status and these rows,
    /// asked with the read-only token — the route is a GET like any other.
    fn provider_status_of(
        status: cctop_core::provider_status::Status,
        sessions: Vec<Session>,
    ) -> serde_json::Value {
        let guarded = shared("full", "view");
        *guarded.provider_status.lock().unwrap() = status;
        *guarded.latest.lock().unwrap() = Arc::new(Snapshot {
            version: 1,
            json: "[]".to_string(),
            rows: Vec::new(),
            sessions,
            host_errors: Vec::new(),
        });
        let raw = response_of(
            &guarded,
            "GET",
            "/api/provider-status",
            "Authorization: Bearer view\r\n",
        );
        assert!(raw.starts_with("HTTP/1.1 200 "), "{raw}");
        let body = raw.split("\r\n\r\n").nth(1).unwrap_or_default();
        serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {body}"))
    }

    /// A running Opus session whose last request failed, as it would arrive
    /// from this machine or from a `--host`.
    fn failing_opus() -> Session {
        let mut s = Session::new(cctop_core::pricing::Provider::Claude, "sess-err".into());
        s.model = "claude-opus-5".into();
        s.inferred_running = true;
        s.activity_state = cctop_core::session::ActivityState::ApiError;
        s
    }

    fn fixture_status(anthropic: &str, openai: &str) -> cctop_core::provider_status::Status {
        use cctop_core::provider_status::parse;
        cctop_core::provider_status::Status {
            anthropic: parse(anthropic),
            openai: parse(openai),
        }
    }

    #[test]
    fn provider_status_confirms_an_incident_your_sessions_are_failing_against() {
        use cctop_core::provider_status::fixtures::{MAJOR, OPERATIONAL};
        let doc = provider_status_of(fixture_status(MAJOR, OPERATIONAL), vec![failing_opus()]);
        let alert = &doc["alerts"][0];
        assert_eq!(alert["page"], "anthropic");
        assert_eq!(alert["confirmed"], true);
        assert_eq!(alert["erroring"], 1);
        assert_eq!(alert["headline"], "Elevated errors on Claude Opus");
        assert_eq!(
            doc["line"]["text"],
            "⚠ Anthropic incident: Elevated errors on Claude Opus — 1 of yours failing"
        );
        assert_eq!(doc["line"]["tone"], "outage");
        let page = &doc["pages"][0];
        assert_eq!(page["site"], "https://status.claude.com");
        assert_eq!(page["state"], "ok");
        assert_eq!(page["incidents"][0]["stage"], "identified");
    }

    #[test]
    fn provider_status_says_probably_local_against_a_clean_page() {
        use cctop_core::provider_status::fixtures::OPERATIONAL;
        let doc = provider_status_of(
            fixture_status(OPERATIONAL, OPERATIONAL),
            vec![failing_opus()],
        );
        assert_eq!(doc["alerts"][0]["probably_local"], true);
        assert_eq!(doc["alerts"][0]["confirmed"], false);
        assert_eq!(doc["line"]["tone"], "warning");
    }

    #[test]
    fn provider_status_before_any_answer_is_pending_and_silent() {
        let doc = provider_status_of(Default::default(), vec![failing_opus()]);
        assert_eq!(doc["pages"][0]["state"], "pending");
        assert_eq!(doc["pages"][1]["state"], "pending");
        assert_eq!(doc["alerts"], serde_json::json!([]));
        assert_eq!(doc["line"], serde_json::Value::Null);
    }

    #[test]
    fn provider_status_honours_the_token() {
        let guarded = shared("full", "view");
        let bearer = |token: &str| format!("Authorization: Bearer {token}\r\n");
        let route = "/api/provider-status";
        assert!(status_of(&guarded, "GET", route, &bearer("full")).contains(" 200 "));
        assert!(status_of(&guarded, "GET", route, &bearer("view")).contains(" 200 "));
        assert!(status_of(&guarded, "GET", route, &bearer("wrong")).contains(" 403 "));
        assert!(status_of(&guarded, "GET", route, "").contains(" 403 "));
    }
}
