//! The server half: the socket a running cctop holds open, and the reader
//! for cctop's own wire format that turns each line into an [`Event`].

use super::*;

/// The socket a running cctop listens on, and the events it has collected.
///
/// Events land on a reader thread and are drained by the UI loop, which is the
/// same shape [`Attach`](crate::attach::Attach) uses — the loop already wakes on
/// a timer, so a channel would add plumbing and arrive no sooner.
pub struct Listener {
    pending: std::sync::Arc<std::sync::Mutex<Vec<Event>>>,
    /// Removed on the way out. A leftover file is survivable — the next hook to
    /// find it refuses connections unlinks it — but only the instance that bound
    /// it can know it is finished with it.
    path: PathBuf,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Listener {
    /// Start listening on an address of this instance's own.
    ///
    /// Unlike the shared address this replaced, every cctop gets one: two
    /// windows open on the same machine both see the agents report in.
    pub fn start() -> Option<Listener> {
        use std::os::unix::net::UnixListener;

        let dir = socket_dir()?;
        std::fs::create_dir_all(&dir).ok()?;
        // Hook events name the user's projects; keep the directory private even
        // when it lands in a shared cache root.
        let _ = std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700));

        // Stamped with the moment it was created as well as the pid, so a name
        // is never reused. That is what lets a hook that finds an address dead
        // delete it without racing an instance that is just starting up.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let path = dir.join(format!("{}-{stamp}.sock", std::process::id()));
        let listener = UnixListener::bind(&path).ok()?;

        let pending = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let collector = std::sync::Arc::clone(&pending);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                // One connection per event, so it is read to EOF and dropped.
                // A hook that connects and stalls must not hold up the next one.
                let collector = std::sync::Arc::clone(&collector);
                std::thread::spawn(move || {
                    let mut text = String::new();
                    if stream.take(MAX_EVENT).read_to_string(&mut text).is_err() {
                        return;
                    }
                    let events: Vec<Event> = text.lines().filter_map(parse).collect();
                    if events.is_empty() {
                        return;
                    }
                    collector
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .extend(events);
                });
            }
        });
        Some(Listener { pending, path })
    }

    /// Everything that has arrived since the last call.
    pub fn drain(&self) -> Vec<Event> {
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *pending)
    }

    /// How many *other* cctops are also listening.
    pub fn peer_count(&self) -> usize {
        socket_dir()
            .map(|dir| peers(&dir).len().saturating_sub(1))
            .unwrap_or(0)
    }
}

pub(super) fn parse(line: &str) -> Option<Event> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let session_id = value.get("session_id")?.as_str()?.to_string();
    // A session cctop cannot name is an event it cannot apply to a row.
    if session_id.is_empty() {
        return None;
    }
    Some(Event {
        session_id,
        pids: value
            .get("pids")
            .and_then(|v| v.as_array())
            .map(|pids| {
                pids.iter()
                    .filter_map(|p| p.as_u64())
                    .map(|p| p as u32)
                    .collect()
            })
            .unwrap_or_default(),
        sandbox: value
            .get("sandbox")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        // Claude Code names it `agent_id`, and cctop stores that subagent's
        // transcript as `agent-<agent_id>.jsonl`, so the two line up directly.
        finished_agent: matches!(
            value.get("event").and_then(|v| v.as_str()),
            Some("SubagentStop" | "subagentStop")
        )
        .then(|| value.get("agent_id").and_then(|v| v.as_str()))
        .flatten()
        .filter(|id| !id.is_empty())
        .map(str::to_string),
        agent: value
            .get("agent_id")
            .and_then(|v| v.as_str())
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        reported: Reported {
            // The prompt that auto mode may answer on your behalf, and the only
            // one worth holding: an MCP elicitation and a plain notification
            // are questions for a person however long you wait.
            provisional: matches!(
                value.get("event").and_then(|v| v.as_str()),
                Some("PermissionRequest" | "permission.asked")
            ),
            signal: signal_of(
                value.get("event")?.as_str()?,
                value
                    .get("notification_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
            )?,
            cwd: value
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            permission: value
                .get("permission_mode")
                .and_then(|v| v.as_str())
                .and_then(Permission::parse),
            ask: value
                .get("ask")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            question: value.get("question").and_then(|v| v.as_bool()) == Some(true),
            call: value
                .get("call")
                .and_then(|v| serde_json::from_value(v.clone()).ok()),
            at: std::time::Instant::now(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the field: the pids the hook walked survive the wire
    /// and come back out of `parse` as the chain that was sent.
    #[test]
    fn the_process_tree_survives_the_wire() {
        let raw = br#"{"session_id":"s","hook_event_name":"Stop","cwd":"/w"}"#;
        let line = envelope("", raw, &[41, 42, 43]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap()).expect("parse");
        assert_eq!(event.pids, vec![41, 42, 43]);
    }

    /// A harness whose hook is not `cctop hook` reports no tree at all, and an
    /// empty claim must read as "no claim" rather than as an empty one.
    #[test]
    fn an_event_without_a_process_tree_claims_nothing() {
        let raw = br#"{"session_id":"s","hook_event_name":"Stop"}"#;
        let line = envelope("", raw, &[]).expect("envelope");
        assert!(
            parse(std::str::from_utf8(&line).unwrap())
                .unwrap()
                .pids
                .is_empty()
        );
        // And one that never carried the field at all.
        let event = parse(r#"{"session_id":"s","event":"Stop"}"#).expect("parse");
        assert!(event.pids.is_empty());
    }

    /// The only word cctop gets that a *background* subagent has finished. Its
    /// tool_result arrives when it launches, so without this every background
    /// subagent reads as done about three seconds into its run — and the id has
    /// to survive the envelope, which forwards a fixed set of fields.
    ///
    /// The payload is the one Claude Code 2.1 actually sends, captured from a
    /// live `SubagentStop`.
    #[test]
    fn a_subagent_stop_names_the_subagent_that_finished() {
        let raw = br#"{"session_id":"parent-1","cwd":"/x","hook_event_name":"SubagentStop","agent_id":"ab3e95cbb4558bd90","agent_type":"general-purpose","last_assistant_message":"pineapple"}"#;
        let line = envelope("", raw, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");

        assert_eq!(
            event.finished_agent.as_deref(),
            Some("ab3e95cbb4558bd90"),
            "the id must reach the UI, not stop at the envelope"
        );
        // The session named is the parent's: a subagent has no session of its
        // own, and applying the signal to the id would find no row.
        assert_eq!(event.session_id, "parent-1");
    }

    /// Every other event has no subagent to report, and must not claim one —
    /// an empty id would match nothing and a stray one would retire a subagent
    /// that is still working.
    #[test]
    fn only_a_subagent_stop_reports_a_finished_subagent() {
        for name in ["Stop", "PreToolUse", "Notification"] {
            let raw = format!(r#"{{"session_id":"s","hook_event_name":"{name}"}}"#);
            let line = envelope("", raw.as_bytes(), &[]).expect("envelope");
            let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
            assert!(event.finished_agent.is_none(), "{name} named a subagent");
        }
    }

    /// The envelope must survive the payload, whatever is in it: the fields are
    /// pulled out of somebody else's JSON and a missing one is normal.
    #[test]
    fn an_event_is_reduced_to_the_session_and_what_happened() {
        let raw = br#"{"session_id":"abc","cwd":"/x","hook_event_name":"Stop","extra":{"a":1}}"#;
        let line = envelope("", raw, &[]).expect("envelope");
        assert!(line.ends_with(b"\n"), "the wire is newline delimited");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.session_id, "abc");
        assert_eq!(event.reported.cwd, "/x");
        assert_eq!(event.reported.signal, Signal::Idle);

        // The argument wins, for a harness whose payload names events its way.
        let line = envelope("Notification", raw, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.signal, Signal::NeedsInput);

        // Junk in, nothing out — never a panic, and never a bogus event.
        assert!(envelope("Stop", b"not json", &[]).is_none());
        assert!(parse("not json").is_none());
        assert!(
            parse(r#"{"event":"Stop"}"#).is_none(),
            "no session to apply to"
        );
        assert!(
            parse(r#"{"event":"SomethingNew","session_id":"a"}"#).is_none(),
            "an unknown event must be dropped, not guessed at"
        );
    }

    /// Codex names the same three things differently and puts them in argv.
    /// Recorded from a real `codex exec` run against a probe program.
    #[test]
    fn a_codex_turn_lands_in_the_same_bin_as_a_claude_stop() {
        let raw = br#"{"type":"agent-turn-complete","thread-id":"019fda22-5315-7580-84de-033e4f6835b5","turn-id":"019fda22-6995-7c40-bf6b-aaf54b274444","cwd":"/home/flo/cctop","client":"codex_exec","input-messages":["hi"],"last-assistant-message":"ok"}"#;
        let line = envelope("", raw, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.session_id, "019fda22-5315-7580-84de-033e4f6835b5");
        assert_eq!(event.reported.cwd, "/home/flo/cctop");
        assert_eq!(
            event.reported.signal,
            Signal::Idle,
            "a finished Codex turn is the same fact as a Claude Stop"
        );
    }

    /// Gemini CLI and Cursor report the same moments as Claude Code, under
    /// their own names and in their own fields. Both payloads are the ones the
    /// agents actually send, and each has to reduce to the same [`Event`].
    #[test]
    fn every_harness_reports_a_finished_turn_the_same_way() {
        // Gemini: identical field names to Claude Code, and the end of its agent
        // loop rather than a `Stop`.
        let raw = br#"{"session_id":"a1b2c3d4-0000-4000-8000-00000000ffff","transcript_path":"/t.jsonl","cwd":"/home/flo/cctop","hook_event_name":"AfterAgent","timestamp":"2026-08-07T00:00:00Z","prompt":"hi","prompt_response":"ok","stop_hook_active":false}"#;
        let event = parse(
            std::str::from_utf8(&envelope("", raw, &[]).expect("envelope"))
                .unwrap()
                .trim(),
        )
        .expect("parse");
        assert_eq!(event.session_id, "a1b2c3d4-0000-4000-8000-00000000ffff");
        assert_eq!(event.reported.cwd, "/home/flo/cctop");
        assert_eq!(event.reported.signal, Signal::Idle);

        // Cursor: lower-cased events, and no `cwd` at all — the directory is the
        // first of its workspace roots.
        let raw = br#"{"conversation_id":"c8f2e1a0-1111-4111-8111-111111111111","session_id":"c8f2e1a0-1111-4111-8111-111111111111","hook_event_name":"stop","cursor_version":"2026.06.04","workspace_roots":["/home/flo/cctop"],"status":"completed"}"#;
        let event = parse(
            std::str::from_utf8(&envelope("", raw, &[]).expect("envelope"))
                .unwrap()
                .trim(),
        )
        .expect("parse");
        assert_eq!(event.session_id, "c8f2e1a0-1111-4111-8111-111111111111");
        assert_eq!(
            event.reported.cwd, "/home/flo/cctop",
            "a Cursor event carries its directory as a workspace root"
        );
        assert_eq!(event.reported.signal, Signal::Idle);

        // Cursor with only the conversation id, which is what its older payloads
        // carry, and a subagent finishing — the id has to survive either way.
        let raw = br#"{"conversation_id":"c8f2e1a0-1111-4111-8111-111111111111","hook_event_name":"subagentStop","subagent_id":"sub-77","workspace_roots":["/w"]}"#;
        let event = parse(
            std::str::from_utf8(&envelope("", raw, &[]).expect("envelope"))
                .unwrap()
                .trim(),
        )
        .expect("parse");
        assert_eq!(event.session_id, "c8f2e1a0-1111-4111-8111-111111111111");
        assert_eq!(event.finished_agent.as_deref(), Some("sub-77"));
        assert_eq!(event.reported.signal, Signal::Busy);

        // OpenCode, whose plugin hands the event over in argv the way Codex does.
        let raw =
            br#"{"type":"session.idle","sessionID":"ses_8a7c","directory":"/home/flo/cctop"}"#;
        let event = parse(
            std::str::from_utf8(&envelope("", raw, &[]).expect("envelope"))
                .unwrap()
                .trim(),
        )
        .expect("parse");
        assert_eq!(event.session_id, "ses_8a7c");
        assert_eq!(event.reported.cwd, "/home/flo/cctop");
        assert_eq!(event.reported.signal, Signal::Idle);
    }

    /// An answer from the page reads back as the state the agent is left in:
    /// working on the tool it was allowed, or waiting on you after a denial
    /// ended its turn.
    #[test]
    fn an_answer_from_the_page_reads_back_as_what_it_left() {
        let read = |allowed| {
            let line = String::from_utf8(answer_line("s-1", allowed)).unwrap();
            let event = parse(line.trim()).expect("a parseable event");
            assert_eq!(event.session_id, "s-1");
            event.reported.signal
        };
        assert_eq!(read(true), Signal::Busy);
        assert_eq!(read(false), Signal::Idle);
    }

    /// A permission request also carries its tool and whole command,
    /// redacted, through the socket to the reader — and keeps it through the
    /// nameless notification Claude Code sends after it. Nothing else carries
    /// one.
    #[test]
    fn a_permission_request_carries_its_call() {
        let raw = br#"{"session_id":"s-1","cwd":"/w","hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"TOKEN=dummy make\nmake install"}}"#;
        let line = envelope("PermissionRequest", raw, &[]).expect("envelope");
        let text = std::str::from_utf8(&line).unwrap();
        let sent: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
        assert!(!sent["call"].to_string().contains("dummy"), "{text}");
        let event = parse(text.trim()).expect("parse");
        let call = event.reported.call.clone().expect("a call");
        assert_eq!(call.tool, "Bash");
        assert_eq!(
            call.detail.as_deref(),
            Some("TOKEN=[redacted] make\nmake install")
        );

        let mut reports = Reports::default();
        reports.observe(&event);
        let note = br#"{"session_id":"s-1","cwd":"/w","hook_event_name":"Notification","notification_type":"permission_prompt"}"#;
        let line = envelope("Notification", note, &[]).expect("envelope");
        let note = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(note.reported.call, None);
        reports.observe(&note);
        assert_eq!(reports.report("s-1").unwrap().call, Some(call));

        let tool = br#"{"session_id":"s-1","hook_event_name":"PreToolUse","tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let line = envelope("PreToolUse", tool, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.call, None);
    }

    /// End to end, from the bytes Claude Code actually writes to the hook.
    #[test]
    fn an_idle_nudge_arrives_as_a_finished_turn() {
        let raw = br#"{"session_id":"s-1","transcript_path":"/t.jsonl","cwd":"/w","hook_event_name":"Notification","message":"Claude is waiting for your input","title":"Claude Code","notification_type":"idle_prompt"}"#;
        let line = envelope("Notification", raw, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.signal, Signal::Idle);
    }

    /// A `cctop sandbox` session says where it works on every event; the row
    /// keeps it through an event that did not say (an older hook binary), and
    /// loses it when the session ends.
    #[test]
    fn a_sandboxed_session_is_stamped_with_its_host() {
        let line = |event: &str, sandbox: &str| {
            parse(&format!(
                r#"{{"event":"{event}","session_id":"s-1","cwd":"/srv","sandbox":"{sandbox}"}}"#
            ))
            .expect("parse")
        };
        let mut reports = Reports::default();
        reports.observe(&line("PreToolUse", "box:/srv"));
        reports.observe(&line("PostToolUse", ""));
        let mut row = crate::session::Session::new(crate::pricing::Provider::Claude, "s-1".into());
        reports.stamp_sandbox(&mut row);
        assert_eq!(row.sandbox.as_deref(), Some("box:/srv"));

        reports.observe(&line("SessionEnd", ""));
        assert!(reports.sandboxes.is_empty());
        let mut other =
            crate::session::Session::new(crate::pricing::Provider::Claude, "s-2".into());
        reports.stamp_sandbox(&mut other);
        assert_eq!(other.sandbox, None);
    }

    /// The permission mode rides in on every Claude Code event, and it is the
    /// one fact here that no transcript records.
    #[test]
    fn a_session_reports_how_much_it_asks_before_it_acts() {
        let raw = br#"{"session_id":"s-1","cwd":"/w","hook_event_name":"PreToolUse","permission_mode":"bypassPermissions","tool_name":"Bash"}"#;
        let line = envelope("PreToolUse", raw, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.permission, Some(Permission::Bypass));
        assert!(Permission::Bypass.is_unrestricted());

        // Every spelling seen in the wild, across the payload and transcripts.
        for (word, mode) in [
            ("default", Permission::Ask),
            ("ask", Permission::Ask),
            ("acceptEdits", Permission::AcceptEdits),
            ("auto", Permission::AcceptEdits),
            ("plan", Permission::Plan),
            ("bypassPermissions", Permission::Bypass),
        ] {
            assert_eq!(Permission::parse(word), Some(mode), "{word}");
        }
        // The modes that do ask must never read as unrestricted — this is the
        // whole reason the column is worth drawing.
        for quiet in [Permission::Ask, Permission::AcceptEdits, Permission::Plan] {
            assert!(!quiet.is_unrestricted(), "{quiet:?}");
        }

        // An unknown mode stays unknown rather than being folded into the safe
        // end: a future mode is at least as likely to be a looser one.
        assert_eq!(Permission::parse("somethingNew"), None);
        let raw = br#"{"session_id":"s-2","cwd":"/w","hook_event_name":"Stop"}"#;
        let line = envelope("Stop", raw, &[]).expect("envelope");
        let event = parse(std::str::from_utf8(&line).unwrap().trim()).expect("parse");
        assert_eq!(event.reported.permission, None, "silence is not a mode");
    }
}
