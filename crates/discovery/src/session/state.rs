//! What a session is doing now, read off the newest records of its transcript
//! (or its database, for the agents that keep no log to tail).

use super::{Discovered, devin, opencode, read_tail, vibe};
use crate::{ActivityState, Permission, Provider};

/// What the newest events say about a session: what it is doing, and how much
/// it asks before it acts.
///
/// Both come off the same tail, so they are read together — a second pass over
/// the last 64KB of every transcript, on every refresh, to answer one more
/// question would be the kind of per-tick waste `proc` was just relieved of.
///
/// The permission mode is only carried by Claude Code, on each `user` record,
/// and the newest one wins: it is a setting the user can change mid-session.
pub fn live_state(session: &Discovered) -> (ActivityState, Option<Permission>) {
    let Some(file) = session.data_file.as_ref() else {
        return (ActivityState::Working, None);
    };
    if session.provider == Provider::OpenCode {
        return (opencode::activity_state_at(file, &session.session_id), None);
    }
    // Devin's transcript is one JSON document, not a line-delimited log — the
    // per-line tail walk below would never parse a record out of it. The
    // conversation's newest node in the database says the same thing faster.
    if session.provider == Provider::Devin {
        return devin::live_state(session);
    }
    // Vibe's state is in its journal or its last message, either way a read
    // of its own rather than this walk.
    if session.provider == Provider::Vibe {
        return vibe::live_state(session);
    }
    let Some(text) = read_tail(file, 65_536) else {
        return (ActivityState::Working, None);
    };

    // Found on the way to the state, which is why the walk is not cut short the
    // moment the state is known: the newest record that names a mode may be
    // older than the newest record that names a state.
    let mut permission = None;
    let mut state = None;
    for line in text.lines().rev() {
        let Ok(item) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        if permission.is_none() {
            permission = item
                .get("permissionMode")
                .and_then(|v| v.as_str())
                .and_then(Permission::parse);
        }
        if state.is_some() {
            if permission.is_some() {
                break;
            }
            continue;
        }
        state = activity_of(session.provider, &item);
    }
    (state.unwrap_or(ActivityState::Working), permission)
}

/// What one record says about the session's state, or `None` if it says nothing
/// and the walk should carry on past it.
fn activity_of(provider: Provider, item: &serde_json::Value) -> Option<ActivityState> {
    if let Some(state) = turn_ended(item) {
        return Some(state);
    }
    if is_api_error_event(item) {
        return Some(ActivityState::ApiError);
    }
    if is_waiting_for_input_event(provider, item) {
        return Some(ActivityState::WaitingForInput);
    }
    if is_passive_event(item) {
        return None;
    }
    // The newest meaningful event was ordinary progress (a tool call, result,
    // or stream event), so do not let an older completed answer make the row
    // look like it is awaiting input.
    Some(ActivityState::Working)
}

/// Token counters and turn metadata are often appended after the event that
/// actually describes the agent's state.  Ignore them while walking backwards
/// so a finished response is still shown as waiting for the user.
fn is_passive_event(item: &serde_json::Value) -> bool {
    let kind = item
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if matches!(kind, "session_meta" | "turn_context") {
        return true;
    }
    kind == "event_msg"
        && item
            .get("payload")
            .and_then(|p| p.get("type"))
            .and_then(serde_json::Value::as_str)
            == Some("token_count")
}

/// Cursor's `{"type":"turn_ended","status":…}` line, the one transcript record
/// that says outright a turn is over — so unlike the other harnesses, a
/// finished Cursor turn is visible on disk without a hook.
///
/// Cursor files a user's own Stop under `status: "error"` too ("User aborted
/// request", "Canceled"), and on real transcripts that is 41 of every 42
/// errors. Read as an API error, nearly every interrupted turn would draw red,
/// so an error whose text says it was aborted or cancelled is the user's move
/// like any other ended turn, and only the rest is an API error.
fn turn_ended(item: &serde_json::Value) -> Option<ActivityState> {
    if item.get("type").and_then(serde_json::Value::as_str) != Some("turn_ended") {
        return None;
    }
    if item.get("status").and_then(serde_json::Value::as_str) != Some("error") {
        return Some(ActivityState::WaitingForInput);
    }
    let error = item
        .get("error")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    Some(if error.contains("abort") || error.contains("cancel") {
        ActivityState::WaitingForInput
    } else {
        ActivityState::ApiError
    })
}

fn is_api_error_event(item: &serde_json::Value) -> bool {
    let kind = item
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let subtype = item
        .get("subtype")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if matches!(kind, "error" | "api_error") || matches!(subtype, "api_error" | "error") {
        return true;
    }
    let payload = item.get("payload").unwrap_or(item);
    matches!(
        payload.get("type").and_then(serde_json::Value::as_str),
        Some("error" | "api_error" | "stream_error" | "turn_aborted")
    )
}

fn is_waiting_for_input_event(provider: Provider, item: &serde_json::Value) -> bool {
    match provider {
        Provider::Claude | Provider::Cursor => {
            // The same message shape, except that Claude Code says whose turn
            // it is in `type` and Cursor in `role`.
            let speaker = if provider == Provider::Cursor {
                "role"
            } else {
                "type"
            };
            if item.get(speaker).and_then(serde_json::Value::as_str) != Some("assistant") {
                return false;
            }
            let Some(blocks) = item
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(serde_json::Value::as_array)
            else {
                return false;
            };
            blocks.iter().any(|b| {
                matches!(
                    b.get("type").and_then(serde_json::Value::as_str),
                    Some("tool_use" | "toolCall")
                ) && b
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(is_input_request_tool)
            })
        }
        Provider::Codex => {
            let payload = item.get("payload").unwrap_or(item);
            matches!(
                item.get("type").and_then(serde_json::Value::as_str),
                Some("function_call" | "custom_tool_call" | "response_item")
            ) && payload
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(is_input_request_tool)
        }
        Provider::Pi => {
            item.get("type").and_then(serde_json::Value::as_str) == Some("message")
                && item
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|blocks| {
                        blocks.iter().any(|b| {
                            b.get("type").and_then(serde_json::Value::as_str) == Some("toolCall")
                                && b.get("name")
                                    .and_then(serde_json::Value::as_str)
                                    .is_some_and(is_input_request_tool)
                        })
                    })
        }
        Provider::Gemini => {
            item.get("type").and_then(serde_json::Value::as_str) == Some("gemini")
                && item
                    .get("toolCalls")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|calls| {
                        calls.iter().any(|call| {
                            call.get("name")
                                .and_then(serde_json::Value::as_str)
                                .is_some_and(is_input_request_tool)
                        })
                    })
        }
        // OpenCode keeps no transcript to tail, and Windsurf's conversation blob
        // is one SQLite value rewritten wholesale rather than a growing log, so
        // neither has a "newest event" this walk could read. Devin never gets
        // here — live_state hands it to its own database read first, as it
        // does Vibe to its journal.
        // ponytail: no `agy` tool asking the person a question has been seen
        // in a transcript yet, so there is nothing to match.
        Provider::OpenCode
        | Provider::Devin
        | Provider::Vibe
        | Provider::Windsurf
        | Provider::Antigravity => false,
    }
}

pub fn is_input_request_tool(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "askuserquestion"
            | "ask_user_question"
            | "ask_user"
            | "askuser"
            | "askquestion"
            | "question"
            | "request_user_input"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_completed_assistant_responses_as_waiting() {
        let claude = json!({
            "type": "assistant",
            "message": {"content": [{"type": "tool_use", "name": "AskUserQuestion"}]}
        });
        assert!(is_waiting_for_input_event(Provider::Claude, &claude));

        let codex = json!({
            "type": "function_call",
            "payload": {"name": "request_user_input"}
        });
        assert!(is_waiting_for_input_event(Provider::Codex, &codex));
    }

    #[test]
    fn keeps_tool_turns_and_api_errors_distinct() {
        let tool_turn = json!({
            "type": "assistant",
            "message": {"content": [{"type": "tool_use", "name": "Read"}]}
        });
        assert!(!is_waiting_for_input_event(Provider::Claude, &tool_turn));

        let error = json!({"type": "system", "subtype": "api_error"});
        assert!(is_api_error_event(&error));
    }

    #[test]
    fn ignores_codex_token_bookkeeping_when_finding_last_state() {
        let item = json!({
            "type": "event_msg",
            "payload": {"type": "token_count"}
        });
        assert!(is_passive_event(&item));
    }
}
