//! An MCP server over stdio, so an agent can ask what the other agents are doing.
//!
//! cctop already knows something no single agent can find out for itself: what
//! every *other* harness on the machine is working on. The dashboard shows that
//! to a person. This shows it to an agent, which is the same data answering a
//! different question — "has someone already touched this file", "what was the
//! last plan anyone made here", "which session is burning the money".
//!
//! Deliberately read-only. Every tool here answers a question; none of them
//! start, stop, or type at anything. An agent that can drive other agents is a
//! different and much larger proposition than one that can see them, and the
//! visibility is the part with no downside.
//!
//! The protocol is JSON-RPC 2.0, one message per line, `initialize` then
//! `tools/list` then `tools/call`. No SDK: three methods and a fixed tool
//! schema is less code than a dependency to carry.
//!
//! ponytail: no `notifications/*` beyond swallowing `initialized`, and no
//! server-initiated messages. Nothing here changes without being asked.

use crate::loader::Loader;
use crate::pricing::Plan;
use crate::session::Session;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

/// The MCP revision this speaks.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// How many sessions a listing returns before it truncates.
///
/// A machine that has been running agents for months has hundreds of dead
/// sessions, and an agent reading this pays for every one. The default is the
/// recent slice, and a caller that genuinely wants the tail asks for it.
const DEFAULT_LIMIT: usize = 25;

/// Serve MCP on stdin/stdout until stdin closes.
///
/// Sessions are re-read per call rather than once at startup: an MCP server
/// lives as long as the agent that spawned it, and the whole value of the
/// answers is that they describe what is happening *now*.
pub fn serve() -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            // A malformed line has no id to answer against, so there is nothing
            // to reply to. Dropping it keeps the stream in sync.
            Err(_) => continue,
        };
        let Some(response) = handle(&request) else {
            continue;
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

/// Answer one request, or `None` for a notification, which takes no reply.
fn handle(request: &Value) -> Option<Value> {
    let method = request.get("method").and_then(Value::as_str)?;
    let id = request.get("id").cloned();
    // A notification is exactly a request with no id, and replying to one is a
    // protocol error rather than a harmless extra message.
    id.as_ref()?;
    let id = id.unwrap_or(Value::Null);

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "cctop", "version": env!("CARGO_PKG_VERSION")},
        })),
        "tools/list" => Ok(json!({"tools": tool_schemas()})),
        "tools/call" => call_tool(request.get("params")),
        // `ping` is the one method a client may send that means nothing.
        "ping" => Ok(json!({})),
        other => Err(format!("unknown method '{other}'")),
    };

    Some(match result {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(message) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": -32603, "message": message},
        }),
    })
}

/// What the server offers.
fn tool_schemas() -> Vec<Value> {
    vec![
        json!({
            "name": "list_sessions",
            "description": "List AI coding agent sessions on this machine — every harness, \
                            not just your own. Returns harness, model, working directory, \
                            git branch, token usage, estimated cost, context window \
                            occupancy, and whether the session is still running. Use this to \
                            find out what other agents are working on before you start.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "running_only": {
                        "type": "boolean",
                        "description": "Only sessions with a live process behind them.",
                    },
                    "directory": {
                        "type": "string",
                        "description": "Only sessions whose working directory is at or under \
                                        this path. Use it to ask who else is in this repo.",
                    },
                    "limit": {
                        "type": "integer",
                        "description":
                            "Maximum sessions to return, most recently active first. \
                             Defaults to 25.",
                    },
                },
            },
        }),
        json!({
            "name": "get_session_context",
            "description": "Get a context brief for one session: what it was doing, the plan \
                            it was working to, the files it changed and read, the commands it \
                            ran, and what it delegated. This is the handoff document — read it \
                            to continue another agent's work. Returns markdown.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session id, or any unique prefix of one, as returned \
                                        by list_sessions.",
                    },
                },
                "required": ["session_id"],
            },
        }),
        json!({
            "name": "check_conflicts",
            "description": "Ask whether another agent is already working where you are about \
                            to. Give it your working directory and, if you know them, the files \
                            you intend to change; it answers with the running sessions in the \
                            same repository and which of your files they have already written. \
                            Two agents writing one file is not a merge conflict — nothing warns \
                            you and one edit is simply lost — so check before a batch of edits, \
                            and take a worktree if someone is there.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "directory": {
                        "type": "string",
                        "description": "Your working directory. The repository containing it is \
                                        what gets compared, so a subdirectory is fine.",
                    },
                    "files": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Paths you are about to change, absolute or relative to \
                                        `directory`. Optional: without them the answer is just \
                                        who else is in the repository.",
                    },
                },
                "required": ["directory"],
            },
        }),
        json!({
            "name": "search_sessions",
            "description": "Search the full text of every session transcript on this machine \
                            for a string, and return the sessions that mention it with a \
                            snippet of the match. Use it to find where something was already \
                            discussed or attempted, in any harness.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Text to look for. Case-insensitive, matched literally.",
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum matching sessions to return. Defaults to 25.",
                    },
                },
                "required": ["query"],
            },
        }),
    ]
}

/// Run one tool and wrap its output the way MCP expects.
fn call_tool(params: Option<&Value>) -> Result<Value, String> {
    let params = params.ok_or("tools/call needs params")?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("tools/call needs a tool name")?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let mut loader = Loader::new();
    let sessions = loader.load(Plan::Retail);

    let text = match name {
        "list_sessions" => list_sessions(&sessions, &args)?,
        "get_session_context" => get_session_context(&sessions, &loader, &args)?,
        "check_conflicts" => check_conflicts(&sessions, &args)?,
        "search_sessions" => search_sessions(&sessions, &args)?,
        other => return Err(format!("unknown tool '{other}'")),
    };
    // Persisting is worth the write even on a read-only call: an MCP server is
    // spawned per agent, and each one that reparsed every transcript from cold
    // would undo the cache the dashboard depends on.
    loader.store().save();

    Ok(json!({"content": [{"type": "text", "text": text}]}))
}

fn list_sessions(sessions: &[Session], args: &Value) -> Result<String, String> {
    let running_only = args
        .get("running_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let directory = args.get("directory").and_then(Value::as_str);
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_LIMIT);

    let mut matched: Vec<&Session> = sessions
        .iter()
        .filter(|s| !running_only || s.is_running())
        .filter(|s| match directory {
            Some(dir) => s.label_source.starts_with(dir.trim_end_matches('/')),
            None => true,
        })
        .collect();
    // Most recent first: an agent reading a truncated list should get the
    // sessions that are still relevant, not whichever provider sorted first.
    matched.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    let total = matched.len();
    matched.truncate(limit);

    let rows: Vec<Value> = matched
        .iter()
        .map(|s| {
            json!({
                "session_id": s.session_id,
                "harness": s.harness,
                "provider": s.provider.as_str(),
                "model": s.model,
                "title": s.title,
                // Absent for the caller's own sessions, which is all of them
                // unless cctop is reading every user's homes.
                "user": s.owner,
                "directory": s.label_source,
                "branch": crate::ui::columns::branch_of(s),
                "branch_note": "the branch checked out now, not necessarily the one it worked on",
                "running": s.is_running(),
                "started_at": s.started_at,
                "last_active": s.last_active,
                "input_tokens": s.input_tokens,
                "output_tokens": s.output_tokens,
                "estimated_cost_usd": s.total_cost,
                "context_used": s.context.map(|c| c.used),
                "context_max": s.context.map(|c| c.max),
            })
        })
        .collect();

    let payload = json!({
        "sessions": rows,
        "returned": rows.len(),
        "total_matching": total,
        // Said explicitly, because an agent handed a list of 25 out of 300 will
        // otherwise reason as though it has seen everything.
        "truncated": total > rows.len(),
        "cost_note": "Costs are estimates from published per-token rates. Flat-rate plans \
                      bill differently.",
    });
    Ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

/// Who else is running in this repository, and which of the caller's files they
/// have already written.
///
/// The caller is very likely one of the sessions in the list — an agent asking
/// about its own repository is a session cctop can see. It is left in rather
/// than guessed out: cctop cannot tell which row is the caller (an MCP server
/// is a child process, not a session), and a wrong guess would drop the one
/// peer that mattered. `own_row_note` says so instead.
fn check_conflicts(sessions: &[Session], args: &Value) -> Result<String, String> {
    let directory = args
        .get("directory")
        .and_then(Value::as_str)
        .ok_or("check_conflicts needs a directory")?;
    let files: Vec<String> = args
        .get("files")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let peers = crate::collide::peers_of(sessions, directory, &files);
    let contested: Vec<&str> = peers
        .iter()
        .filter(|(_, shared)| !shared.is_empty())
        .flat_map(|(_, shared)| shared.iter().map(String::as_str))
        .collect();

    let rows: Vec<Value> = peers
        .iter()
        .map(|(s, shared)| {
            json!({
                "session_id": s.session_id,
                "harness": s.harness,
                "directory": s.label_source,
                "last_active": s.last_active,
                "your_files_it_has_written": shared,
                "recently_written": s.recent_writes,
            })
        })
        .collect();

    let payload = json!({
        "directory": directory,
        "agents_here": rows,
        "contested_files": contested,
        "clear": contested.is_empty(),
        "own_row_note": "One of these is probably you — an agent calling this has a session of \
                         its own. Match on session_id or working directory.",
        "advice": match contested.is_empty() {
            true if rows.len() > 1 =>
                "Another agent is in this repository but has not written your files. Nothing is \
                 lost yet; check again if you start editing widely.",
            true => "Nobody else is running here.",
            false =>
                "Another running agent has already written these files. Whichever of you saves \
                 last wins and the other edit is gone, with no warning from git. Take a worktree, \
                 or agree who finishes first.",
        },
        "limits": "Only running sessions are compared, only the files each has written recently, \
                   and a linked git worktree counts as a separate repository — which is the point \
                   of one.",
    });
    Ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn get_session_context(
    sessions: &[Session],
    loader: &Loader,
    args: &Value,
) -> Result<String, String> {
    let wanted = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or("get_session_context needs a session_id")?;

    let matched: Vec<&Session> = sessions
        .iter()
        .filter(|s| s.session_id.starts_with(wanted))
        .collect();

    let session = match matched.as_slice() {
        [only] => *only,
        [] => return Err(format!("no session id starts with '{wanted}'")),
        many => {
            return Err(format!(
                "'{wanted}' matches {} sessions: {}",
                many.len(),
                many.iter()
                    .map(|s| s.session_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    };

    let data = loader.store().session_data(session);
    Ok(crate::handoff::rendered(&crate::handoff::build(
        session,
        Some(&data),
    )))
}

fn search_sessions(sessions: &[Session], args: &Value) -> Result<String, String> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .ok_or("search_sessions needs a query")?;
    if query.trim().is_empty() {
        return Err("search_sessions needs a non-empty query".into());
    }
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_LIMIT);

    let needle = query.to_lowercase();
    let mut ordered: Vec<&Session> = sessions.iter().collect();
    ordered.sort_by(|a, b| b.last_active.cmp(&a.last_active));

    let mut hits = Vec::new();
    for session in ordered {
        if hits.len() >= limit {
            break;
        }
        let target = crate::session::search::Target::of(session);
        let Some(hit) = crate::session::search::find(&target, &needle) else {
            continue;
        };
        hits.push(json!({
            "session_id": session.session_id,
            "harness": session.harness,
            "provider": session.provider.as_str(),
            "directory": session.label_source,
            "title": session.title,
            "last_active": session.last_active,
            "snippet": hit.snippet,
        }));
    }

    let payload = json!({
        "query": query,
        "matches": hits,
        // Scanning stops at the limit rather than counting every match first,
        // so there is no honest total to report — say that instead of implying
        // the number returned is the number that exist.
        "note": format!(
            "Searched newest-first and stopped at {limit} matches; there may be older ones."
        ),
    });
    Ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A notification carries no id, and answering one is a protocol error —
    /// the client is not waiting for a reply and will not match it to anything.
    #[test]
    fn notifications_get_no_reply() {
        let notification = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle(&notification).is_none());
    }

    /// The handshake has to name a protocol version the client recognises, or
    /// the connection is dropped before any tool is ever listed.
    #[test]
    fn initialize_answers_with_the_protocol_version() {
        let request = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"});
        let response = handle(&request).expect("a reply");
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(response["id"], 1);
    }

    /// Every advertised tool needs a name, a description an agent can choose
    /// from, and a schema — a tool missing any of them is one no client will
    /// call correctly.
    #[test]
    fn every_tool_is_fully_described() {
        for tool in tool_schemas() {
            let name = tool["name"].as_str().expect("a name");
            assert!(
                tool["description"].as_str().is_some_and(|d| d.len() > 40),
                "{name} needs a description an agent can choose from"
            );
            assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
        }
    }

    /// An unknown method is answered, not ignored: a client that sent an id is
    /// blocked waiting for something.
    #[test]
    fn an_unknown_method_still_gets_an_answer() {
        let request = json!({"jsonrpc": "2.0", "id": 7, "method": "resources/list"});
        let response = handle(&request).expect("a reply");
        assert_eq!(response["id"], 7);
        assert!(response["error"]["message"].as_str().is_some());
    }
}
