//! `/api/events`: the server-sent stream behind the live table and a session
//! page's "the conversation moved" mark.
//!
//! The table goes out whole once and then as deltas, which is why the diffing
//! ([`table_delta`], [`table_patch`]) lives beside the stream that sends it.

use crate::api::find;
use crate::http;
use crate::{SSE_KEEPALIVE, Shared, Snapshot};
use cctop_core::chat;
use http::{EventStream, Request};
use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Hold an SSE stream open, sending each new snapshot as it lands.
pub(crate) fn events(shared: &Shared, stream: &mut TcpStream, request: &Request) {
    events_every(shared, stream, request, SSE_KEEPALIVE);
}

/// [`events`], with the ping interval as an argument so a test can wait for
/// one without waiting fifteen seconds.
///
/// Two shapes of stream. A session page asks for its own row and is sent it,
/// whole, only when it moved. The table is sent whole once, as `sessions`, and
/// after that as `rows`: only the rows that changed since this stream last
/// wrote, and the order of ids when that moved — see [`table_delta`].
pub(crate) fn events_every(
    shared: &Shared,
    stream: &mut TcpStream,
    request: &Request,
    keepalive: Duration,
) {
    // A session page asks for its own row only. It used to be sent the whole
    // table on every refresh — every session on the machine and on every
    // `--host`, re-parsed in the browser several times a minute — to read one
    // row's state out of it.
    let only = request
        .query
        .get("session")
        .cloned()
        .filter(|s| !s.is_empty());
    // A page that knows the field-level shape says so; one loaded before this
    // server was upgraded does not, and keeps the shape it can apply rather
    // than silently dropping every patch it was sent.
    let fine = request.query.get("rows").is_some_and(|v| v == "patch");
    let mut last_row: Option<String> = None;
    // The last `chat` mark this stream sent its session page — see [`chat_mark`].
    let mut last_chat: Option<String> = None;
    // The table this stream last wrote, which is what the next delta is taken
    // against. Per stream rather than per snapshot: a stream that fell behind
    // skips snapshots, and its delta has to cover everything it skipped.
    let mut last_table: Option<Arc<Snapshot>> = None;
    let Ok(mut sse) = EventStream::open(stream, request) else {
        return;
    };
    cctop_core::elog::event("sse", "open", serde_json::json!({}));
    // A ping before anything else, so the page hears from a stream that
    // works within a round trip — even a session page whose row is not in
    // the table, which would otherwise be sent nothing until the first
    // keepalive. That is how the page tells a stream that delivers from one a
    // tunnel is holding, and falls back to polling `/api/sessions` within
    // seconds rather than sitting on a table that never moves.
    if sse.ping().is_err() {
        return;
    }

    let mut sent = 0u64;
    let mut wrote = Instant::now();
    // Which write lost the client is the difference between "browser closed"
    // and "network went" — the reason is kept rather than collapsed.
    let by = loop {
        // The wait is what makes an idle stream free: no polling, and one
        // wakeup per refresh rather than one per connection per tick. It is
        // bounded by the ping that is next due, not by a fixed window: a
        // refresh lands every couple of seconds, and a stream whose own rows
        // never move would otherwise wake on every one, write nothing, and
        // never come round to pinging at all.
        let snapshot = {
            let Ok(latest) = shared.latest.lock() else {
                break "lock";
            };
            let due = keepalive.saturating_sub(wrote.elapsed());
            let (latest, _) = match shared
                .updated
                .wait_timeout_while(latest, due, |s| s.version <= sent)
            {
                Ok(pair) => pair,
                Err(_) => break "wait",
            };
            Arc::clone(&latest)
        };

        if snapshot.version > sent {
            let frame = match &only {
                None => match &last_table {
                    None => Some(("sessions", snapshot.built(&shared.store).json.clone())),
                    Some(before) => match fine {
                        true => table_patch(
                            &before.built(&shared.store).rows,
                            &snapshot.built(&shared.store).rows,
                        ),
                        false => table_delta(
                            &before.built(&shared.store).rows,
                            &snapshot.built(&shared.store).rows,
                        ),
                    }
                    .map(|delta| ("rows", delta)),
                },
                Some(id) => {
                    let rows = &snapshot.built(&shared.store).rows;
                    let row = rows
                        .iter()
                        .find(|(sid, _)| sid == id)
                        .or_else(|| rows.iter().find(|(sid, _)| sid.starts_with(id.as_str())))
                        .map(|(_, r)| r.clone());
                    // Only when it moved: a refresh that changed some other
                    // row is nothing to this page.
                    match row {
                        Some(row) if last_row.as_ref() != Some(&row) => {
                            last_row = Some(row.clone());
                            Some(("sessions", format!("[{row}]")))
                        }
                        _ => None,
                    }
                }
            };
            if let Some((event, body)) = frame {
                if sse.send(event, &body).is_err() {
                    break "send";
                }
                wrote = Instant::now();
            }
            // The conversation moved: the page refetches it on this rather than
            // polling `/api/chat` on a timer of its own.
            if let Some(id) = &only {
                let mark = chat_mark(&snapshot, &shared.store, id);
                if mark.is_some() && mark != last_chat {
                    let body = serde_json::to_string(&mark).unwrap_or_default();
                    if sse.send("chat", &body).is_err() {
                        break "send";
                    }
                    wrote = Instant::now();
                    last_chat = mark;
                }
            }
            if only.is_none() {
                last_table = Some(Arc::clone(&snapshot));
            }
            sent = snapshot.version;
        }

        // Nothing written for a whole interval — no new snapshot, or none that
        // moved anything this stream shows — so say the stream is alive, which
        // is also what surfaces a client that quietly went away.
        if wrote.elapsed() >= keepalive {
            if sse.ping().is_err() {
                break "ping";
            }
            wrote = Instant::now();
        }
    };
    cctop_core::elog::event(
        "sse",
        "close",
        serde_json::json!({ "by": by, "sent": sent }),
    );
}

/// A token that changes when session `id`'s conversation may have, sent to its
/// page as a `chat` event so the page refetches `/api/chat` when there is
/// something to fetch rather than every few seconds in case there is.
///
/// For a session read here, the stamps ([`chat::stamp_of`]) of its transcript
/// and of every subagent's beside it — a subagent's page follows its own file,
/// which the main transcript does not move while the agent works. A stat per
/// file per refresh, for a stream a session page holds open. For a remote row,
/// whose files are on another machine, the row itself: its activity time and
/// counts move when its transcript does.
pub(crate) fn chat_mark(
    snapshot: &Snapshot,
    store: &cctop_core::cache::Store,
    id: &str,
) -> Option<String> {
    let session = find(&snapshot.sessions, id)?;
    match (&session.remote, &session.data_file) {
        (None, Some(main)) => Some(
            cctop_core::session::transcript_files(main)
                .iter()
                .map(|f| chat::stamp_of(f).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(","),
        ),
        _ => {
            use std::hash::{Hash, Hasher};
            let row = snapshot
                .built(store)
                .rows
                .iter()
                .find(|(sid, _)| *sid == session.session_id)?;
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            row.1.hash(&mut hasher);
            Some(format!("r{:016x}", hasher.finish()))
        }
    }
}

/// What a stream that last sent the table `before` needs to be told to be
/// showing `after`, as the body of a `rows` event — `None` when nothing moved.
///
/// The body is `{"set": [row, …]}`, the rows that are new or changed, plus
/// `"order": [id, …]` when the sequence of ids is not what it was — which is
/// also how a row that went away is said, by its id no longer being in it.
///
/// This exists because the whole table on every refresh is more than some
/// links carry. On a shared box with eight hundred sessions it is 3.6 MB every
/// two seconds; through an ssh forward or a trycloudflare tunnel that queued
/// behind itself, and the page fell tens of seconds behind the server — still
/// offering Allow on a prompt that had been answered, which the server then
/// refused, because *it* was current. A refresh moves a handful of rows, so
/// the stream now carries a handful of rows.
///
/// Rows are compared as the strings already rendered for the snapshot, so a
/// row counts as changed exactly when its JSON did — no second notion of
/// equality to drift from what the page would have seen.
///
/// This is the shape a page from before [`table_patch`] understands, and what
/// a stream that did not ask for the finer one is still sent.
///
/// ponytail: rows are told apart by session id, as the page already does for
/// its keys and its routes. Two rows sharing one — the same session reported
/// by this machine and by a `--host` — are one row to the delta, and the page
/// shows whichever it was sent last.
pub(crate) fn table_delta(
    before: &[(String, String)],
    after: &[(String, String)],
) -> Option<String> {
    let had: HashMap<&str, &str> = before
        .iter()
        .map(|(id, row)| (id.as_str(), row.as_str()))
        .collect();
    let set: Vec<&str> = after
        .iter()
        .filter(|(id, row)| had.get(id.as_str()) != Some(&row.as_str()))
        .map(|(_, row)| row.as_str())
        .collect();
    let reordered =
        before.len() != after.len() || before.iter().zip(after).any(|((a, _), (b, _))| a != b);
    if set.is_empty() && !reordered {
        return None;
    }
    let mut body = format!("{{\"set\":[{}]", set.join(","));
    if reordered {
        let ids: Vec<&str> = after.iter().map(|(id, _)| id.as_str()).collect();
        body.push_str(",\"order\":");
        body.push_str(&serde_json::to_string(&ids).unwrap_or_else(|_| "[]".into()));
    }
    body.push('}');
    Some(body)
}

/// [`table_delta`], down to the field: what a page that asked for it is sent.
///
/// [`table_delta`] still sent too much. A session row is four kilobytes —
/// costs per model, subagents, an activity histogram — and what moves between
/// two refreshes is usually one field of it: a running agent's CPU figure,
/// which changes on every tick for every live row. And the order of eight
/// hundred ids was sent whole whenever one session became the newest, which is
/// most refreshes on a busy machine. Measured on one, both together were most
/// of 5 MB a minute. So:
///
/// - A row that existed before goes as `"patch": [{"id", "to": {field: value},
///   "drop": [field]}]` — the fields whose JSON changed, and the ones that are
///   gone — unless that would be no shorter than the row, which then goes whole
///   in `"set"` as before. New rows always go whole.
/// - A changed order goes as `"head": [id, …]` and `"gone": [id, …]`: the new
///   order is `head`, then every row the page had that is in neither list, in
///   the order the page had them. A refresh moves a few rows to the top and
///   leaves the rest as they were, which is a head of a few ids. Any other
///   reshuffle that would make `head` as long as the table goes as `"order"`.
///
/// Fields are compared as parsed JSON, and only for rows whose string already
/// differed, so a quiet refresh parses nothing.
pub(crate) fn table_patch(
    before: &[(String, String)],
    after: &[(String, String)],
) -> Option<String> {
    use serde_json::{Map, Value};
    let had: HashMap<&str, &str> = before
        .iter()
        .map(|(id, row)| (id.as_str(), row.as_str()))
        .collect();
    let mut set: Vec<&str> = Vec::new();
    let mut patch: Vec<Value> = Vec::new();
    for (id, row) in after {
        let Some(old) = had.get(id.as_str()) else {
            set.push(row);
            continue;
        };
        if old == row {
            continue;
        }
        let parsed = (
            serde_json::from_str::<Map<String, Value>>(old),
            serde_json::from_str::<Map<String, Value>>(row),
        );
        let (Ok(old), Ok(new)) = parsed else {
            set.push(row);
            continue;
        };
        let to: Map<String, Value> = new
            .iter()
            .filter(|(k, v)| old.get(*k) != Some(*v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let drop: Vec<&String> = old.keys().filter(|k| !new.contains_key(*k)).collect();
        let mut one = serde_json::json!({ "id": id, "to": to });
        if !drop.is_empty() {
            one["drop"] = serde_json::json!(drop);
        }
        match one.to_string().len() < row.len() {
            true => patch.push(one),
            false => set.push(row),
        }
    }

    let reordered =
        before.len() != after.len() || before.iter().zip(after).any(|((a, _), (b, _))| a != b);
    if set.is_empty() && patch.is_empty() && !reordered {
        return None;
    }
    let mut body = format!("{{\"set\":[{}]", set.join(","));
    if !patch.is_empty() {
        body.push_str(",\"patch\":");
        body.push_str(&Value::from(patch).to_string());
    }
    if reordered {
        let ids: Vec<&str> = after.iter().map(|(id, _)| id.as_str()).collect();
        let list = |ids: &[&str]| serde_json::to_string(ids).unwrap_or_else(|_| "[]".into());
        match order_edit(before, &ids) {
            Some((head, gone)) => {
                body.push_str(&format!(
                    ",\"head\":{},\"gone\":{}",
                    list(&head),
                    list(&gone)
                ));
            }
            None => body.push_str(&format!(",\"order\":{}", list(&ids))),
        }
    }
    body.push('}');
    Some(body)
}

/// The order `after` as an edit of `before`'s: the ids to put first, and the
/// ids that are gone — with every other row keeping its place relative to the
/// others. `None` when that is no shorter than the whole order, or when an id
/// appears twice (see [`table_delta`]'s ponytail), where "its place" means
/// nothing.
///
/// `head` is the shortest prefix of `after` whose removal leaves the rest of
/// `after` in the order `before` had it. Found from the back: the longest tail
/// of `after` that `before` contains in sequence, matched greedily from the
/// end, which is what makes the head left over the shortest. A row new to the
/// table cannot be in that tail — `before` does not have it — so it is always
/// in `head`, where the page finds it next to its row in `set`.
pub(crate) fn order_edit<'a>(
    before: &'a [(String, String)],
    after: &[&'a str],
) -> Option<(Vec<&'a str>, Vec<&'a str>)> {
    use std::collections::HashSet;
    let now: HashSet<&str> = after.iter().copied().collect();
    let was: HashSet<&str> = before.iter().map(|(id, _)| id.as_str()).collect();
    if now.len() != after.len() || was.len() != before.len() {
        return None;
    }
    let gone: Vec<&str> = before
        .iter()
        .map(|(id, _)| id.as_str())
        .filter(|id| !now.contains(id))
        .collect();
    let kept: Vec<&str> = before
        .iter()
        .map(|(id, _)| id.as_str())
        .filter(|id| now.contains(id))
        .collect();
    let mut j = kept.len();
    let mut tail = 0;
    for id in after.iter().rev() {
        match kept[..j].iter().rposition(|k| k == id) {
            Some(at) => {
                j = at;
                tail += 1;
            }
            None => break,
        }
    }
    let head: Vec<&str> = after[..after.len() - tail].to_vec();
    (head.len() + gone.len() < after.len()).then_some((head, gone))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tests::{session, shared};
    use std::io::Write;
    use std::net::TcpListener;

    pub(crate) fn rows(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(id, row)| (id.to_string(), row.to_string()))
            .collect()
    }

    #[test]
    fn a_delta_carries_only_the_rows_that_moved() {
        let before = rows(&[("a", r#"{"id":"a","n":1}"#), ("b", r#"{"id":"b","n":1}"#)]);

        // The same table again is nothing to send.
        assert_eq!(table_delta(&before, &before.clone()), None);

        // One row changed in place: that row, and no order, since none moved.
        let changed = rows(&[("a", r#"{"id":"a","n":2}"#), ("b", r#"{"id":"b","n":1}"#)]);
        assert_eq!(
            table_delta(&before, &changed).as_deref(),
            Some(r#"{"set":[{"id":"a","n":2}]}"#)
        );

        // A row that appeared is sent, and the order says where it goes.
        let grown = rows(&[
            ("c", r#"{"id":"c"}"#),
            ("a", r#"{"id":"a","n":1}"#),
            ("b", r#"{"id":"b","n":1}"#),
        ]);
        assert_eq!(
            table_delta(&before, &grown).as_deref(),
            Some(r#"{"set":[{"id":"c"}],"order":["c","a","b"]}"#)
        );

        // A row that went away is said by the order alone.
        let shrunk = rows(&[("b", r#"{"id":"b","n":1}"#)]);
        assert_eq!(
            table_delta(&before, &shrunk).as_deref(),
            Some(r#"{"set":[],"order":["b"]}"#)
        );

        // And rows that only swapped places are an order and nothing else.
        let swapped = rows(&[("b", r#"{"id":"b","n":1}"#), ("a", r#"{"id":"a","n":1}"#)]);
        assert_eq!(
            table_delta(&before, &swapped).as_deref(),
            Some(r#"{"set":[],"order":["b","a"]}"#)
        );
    }

    /// Put a table in front of every stream, the way `publish` does.
    pub(crate) fn put(shared: &Shared, version: u64, table: &[(&str, &str)]) {
        let rows = rows(table);
        let json = format!(
            "[{}]",
            rows.iter()
                .map(|(_, r)| r.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );
        *shared.latest.lock().unwrap() = Arc::new(Snapshot {
            version,
            doc: crate::Doc::ready(json, rows),
            sessions: Vec::new(),
            host_errors: Vec::new(),
        });
        shared.updated.notify_all();
    }

    /// Open `/api/events<query>` against `shared` on a thread of its own, and
    /// hand back a reader of its frames as `(event, data)`.
    fn open_stream(
        shared: &Arc<Shared>,
        query: &str,
        keepalive: Duration,
    ) -> impl FnMut() -> (String, String) {
        open_stream_with(shared, query, "", keepalive).1
    }

    /// [`open_stream`] with request headers, handing back the response head
    /// too. A gzip stream is decoded as it arrives, the way a browser does.
    fn open_stream_with(
        shared: &Arc<Shared>,
        query: &str,
        headers: &str,
        keepalive: Duration,
    ) -> (String, impl FnMut() -> (String, String)) {
        use std::io::{BufRead, BufReader};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (mut server, _) = listener.accept().unwrap();
        client
            .write_all(format!("GET /api/events{query} HTTP/1.1\r\n{headers}\r\n").as_bytes())
            .unwrap();
        let request = Request::parse(&server).unwrap();
        let shared = Arc::clone(shared);
        std::thread::spawn(move || events_every(&shared, &mut server, &request, keepalive));
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut reader = BufReader::new(client);
        // The response head, which ends at its blank line.
        let mut head = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("the response head");
            if line == "\r\n" {
                break;
            }
            head.push_str(&line);
        }
        let mut reader: Box<dyn BufRead> = match head.contains("Content-Encoding: gzip") {
            true => Box::new(BufReader::new(flate2::read::GzDecoder::new(reader))),
            false => Box::new(reader),
        };
        let mut next = move || {
            let (mut event, mut data) = (String::new(), String::new());
            loop {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .expect("a frame before the timeout");
                let line = line.trim_end_matches('\n');
                if line.is_empty() {
                    return (event, data);
                }
                if let Some(e) = line.strip_prefix("event: ") {
                    event = e.to_string();
                } else if let Some(d) = line.strip_prefix("data: ") {
                    data = d.to_string();
                }
            }
        };
        // Every stream opens with a ping, before what it has to say.
        assert_eq!(next().0, "ping", "a stream opens with a ping");
        (head, next)
    }

    /// The table goes out whole once, then as the rows that moved — and a
    /// refresh that moved nothing writes nothing.
    #[test]
    fn the_table_stream_sends_the_table_once_and_then_what_changed() {
        let shared = Arc::new(shared("", ""));
        put(&shared, 1, &[("a", r#"{"a":1}"#), ("b", r#"{"b":1}"#)]);
        let mut next = open_stream(&shared, "", Duration::from_secs(30));

        assert_eq!(next(), ("sessions".into(), r#"[{"a":1},{"b":1}]"#.into()));
        // Unchanged, then changed: the first is never written, so the next
        // frame is the second's one row.
        put(&shared, 2, &[("a", r#"{"a":1}"#), ("b", r#"{"b":1}"#)]);
        put(&shared, 3, &[("a", r#"{"a":1}"#), ("b", r#"{"b":2}"#)]);
        assert_eq!(next(), ("rows".into(), r#"{"set":[{"b":2}]}"#.into()));
    }

    /// The bug a comment keepalive hid: refreshes land every two seconds, and
    /// a stream whose own row never moved woke on each one, wrote nothing,
    /// and so never reached its keepalive at all — which the page could not
    /// tell from a dead connection. A ping is due by the clock of the last
    /// write, however many snapshots arrive in between.
    #[test]
    fn a_quiet_stream_pings_even_while_refreshes_keep_landing() {
        let shared = Arc::new(shared("", ""));
        put(&shared, 1, &[("abc", r#"{"s":"asking"}"#)]);
        let mut next = open_stream(&shared, "?session=abc", Duration::from_millis(300));
        assert_eq!(next(), ("sessions".into(), r#"[{"s":"asking"}]"#.into()));

        // Refreshes keep landing, faster than the keepalive, until the ping
        // has come through them; stopped then rather than run for a fixed
        // count, which was seconds of churn after the answer was in.
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let churn = {
            let (shared, stop) = (Arc::clone(&shared), Arc::clone(&stop));
            std::thread::spawn(move || {
                let mut version = 2;
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    put(&shared, version, &[("abc", r#"{"s":"asking"}"#)]);
                    version += 1;
                    // The cadence of the churn, not a wait for anything.
                    std::thread::sleep(Duration::from_millis(50));
                }
            })
        };
        let pinged = next();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        churn.join().unwrap();
        assert_eq!(pinged, ("ping".into(), "1".into()));
    }

    /// A session page is told when its conversation moved, as a `chat` event,
    /// and is not told again for a refresh that left the transcript alone —
    /// which is what lets the page stop polling `/api/chat`.
    #[test]
    fn a_session_stream_says_when_the_conversation_moved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc.jsonl");
        std::fs::write(&path, "{}\n").unwrap();
        let mut s = session("abc");
        s.data_file = Some(path.clone());
        let shared = Arc::new(shared("", ""));
        let publish = |version: u64| {
            *shared.latest.lock().unwrap() = Arc::new(Snapshot {
                version,
                doc: crate::Doc::ready("[{}]".into(), vec![("abc".into(), "{}".into())]),
                sessions: vec![s.clone()],
                host_errors: Vec::new(),
            });
            shared.updated.notify_all();
        };
        publish(1);
        let mut next = open_stream(&shared, "?session=abc", Duration::from_secs(30));
        assert_eq!(next().0, "sessions");
        let (event, first) = next();
        assert_eq!(event, "chat");

        // Unchanged, then grown: only the second is news.
        publish(2);
        std::fs::write(&path, "{}\n{}\n").unwrap();
        publish(3);
        let (event, second) = next();
        assert_eq!(event, "chat");
        assert_ne!(first, second);
    }

    #[test]
    fn a_patch_carries_only_the_fields_that_moved() {
        let row = |id: &str, cpu: u32, extra: &str| {
            format!(
                r#"{{"session_id":"{id}","title":"a long title that is the bulk of the row","cpu":{cpu}{extra}}}"#
            )
        };
        let before = vec![
            ("a".to_string(), row("a", 1, r#","permission":"edits""#)),
            ("b".to_string(), row("b", 1, "")),
        ];
        assert_eq!(table_patch(&before, &before.clone()), None);

        // One field moved and one went: the row is not resent, only those.
        let after = vec![
            ("a".to_string(), row("a", 2, "")),
            ("b".to_string(), row("b", 1, "")),
        ];
        assert_eq!(
            table_patch(&before, &after).as_deref(),
            Some(r#"{"set":[],"patch":[{"id":"a","to":{"cpu":2},"drop":["permission"]}]}"#)
        );

        // A new row goes whole, and the order as the ids to put first.
        let grown = vec![
            ("c".to_string(), row("c", 1, "")),
            ("a".to_string(), before[0].1.clone()),
            ("b".to_string(), before[1].1.clone()),
        ];
        let delta = table_patch(&before, &grown).unwrap();
        assert!(
            delta.starts_with(&format!(r#"{{"set":[{}]"#, row("c", 1, ""))),
            "{delta}"
        );
        assert!(delta.ends_with(r#""head":["c"],"gone":[]}"#), "{delta}");
    }

    /// The order edit is checked by doing what the page does with it — `head`,
    /// then every row it had that is neither in `head` nor gone — and
    /// comparing with the order wanted.
    #[test]
    fn an_order_edit_rebuilds_the_order_it_describes() {
        let ids = |s: &str| -> Vec<(String, String)> {
            s.chars().map(|c| (c.to_string(), String::new())).collect()
        };
        let apply = |before: &str, head: &[&str], gone: &[&str]| -> String {
            let mut out: String = head.concat();
            out.extend(
                before
                    .chars()
                    .filter(|c| !head.contains(&c.to_string().as_str()))
                    .filter(|c| !gone.contains(&c.to_string().as_str())),
            );
            out
        };
        for (before, after) in [
            ("abcdefgh", "dabcefgh"),  // one row became the newest
            ("abcdefgh", "xabcdefgh"), // a new row arrived on top
            ("abcdefgh", "abcdefg"),   // the oldest went away
            ("abcdefgh", "abdefgh"),   // one in the middle went away
            ("abcdefgh", "hgabcdef"),  // two moved up together
            ("abcdefgh", "fabcdegh"),  // one moved up, one went
        ] {
            let after_ids: Vec<String> = after.chars().map(String::from).collect();
            let after_refs: Vec<&str> = after_ids.iter().map(String::as_str).collect();
            let before_ids = ids(before);
            let (head, gone) = order_edit(&before_ids, &after_refs)
                .unwrap_or_else(|| panic!("{before} -> {after} should be an edit"));
            assert_eq!(apply(before, &head, &gone), after, "{before} -> {after}");
            assert!(head.len() <= 2, "{before} -> {after}: head {head:?}");
        }
        // Turned around entirely, nearly every id has to be named, and the
        // whole order is no longer than that plus the departures.
        let after_ids: Vec<String> = "hgf".chars().map(String::from).collect();
        let after_refs: Vec<&str> = after_ids.iter().map(String::as_str).collect();
        assert_eq!(order_edit(&ids("abcdefgh"), &after_refs), None);
        // And a repeated id has no place to keep.
        assert_eq!(order_edit(&ids("aab"), &["b", "a", "a"]), None);
    }

    /// A page that did not ask for patches is sent whole rows, which is all a
    /// page loaded before the patch shape existed knows how to apply.
    #[test]
    fn only_a_stream_that_asks_is_sent_patches() {
        let shared = Arc::new(shared("", ""));
        let row = |n: u32| {
            format!(
                r#"{{"session_id":"a","title":"{}","n":{n}}}"#,
                "x".repeat(200)
            )
        };
        let first = row(1);
        put(&shared, 1, &[("a", &first)]);
        let mut old = open_stream(&shared, "", Duration::from_secs(30));
        let mut new = open_stream(&shared, "?rows=patch", Duration::from_secs(30));
        assert_eq!(old().0, "sessions");
        assert_eq!(new().0, "sessions");
        let second = row(2);
        put(&shared, 2, &[("a", &second)]);
        assert_eq!(old(), ("rows".into(), format!(r#"{{"set":[{second}]}}"#)));
        assert_eq!(
            new(),
            (
                "rows".into(),
                r#"{"set":[],"patch":[{"id":"a","to":{"n":2}}]}"#.into()
            )
        );
    }

    /// Gzip to a client that offers it, and each event decodable the moment
    /// it is written: a stream that only came out of the compressor when the
    /// next event pushed it would hold every update back by one refresh.
    #[test]
    fn the_event_stream_is_gzip_and_flushed_per_event() {
        let shared = Arc::new(shared("", ""));
        put(&shared, 1, &[("a", r#"{"a":1}"#)]);
        let (head, mut next) = open_stream_with(
            &shared,
            "",
            "Accept-Encoding: gzip, deflate, br\r\n",
            Duration::from_millis(300),
        );
        assert!(head.contains("Content-Encoding: gzip"), "{head}");
        assert_eq!(next(), ("sessions".into(), r#"[{"a":1}]"#.into()));
        put(&shared, 2, &[("a", r#"{"a":2}"#)]);
        assert_eq!(next(), ("rows".into(), r#"{"set":[{"a":2}]}"#.into()));
        // The ping goes through the same encoder.
        assert_eq!(next(), ("ping".into(), "1".into()));

        let (plain, _) = open_stream_with(&shared, "", "", Duration::from_secs(30));
        assert!(!plain.contains("Content-Encoding"), "{plain}");
    }
}
