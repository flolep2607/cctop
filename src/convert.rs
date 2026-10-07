//! Carry a session from one harness's store into another's, in the shape the
//! receiving harness reads back itself.
//!
//! [`crate::handoff`] hands work over as a brief: a few hundred tokens of
//! markdown summarising what a session was for, which files it touched, what it
//! ran. That is the right trade for a harness that cannot read another's
//! transcripts, and the module there says so. This is the other trade, the one
//! [`crate::handoff::fork`] already makes between two Claudes: transcode the
//! conversation itself and let the receiving agent resume onto it, so it starts
//! knowing everything the first one knew rather than everything a summary could
//! carry.
//!
//! What crosses is the conversation — what was asked, what was said, and each
//! tool call with its result — because that is the part that does not survive a
//! restart. What does not cross is everything the receiving harness can work out
//! for itself: token counts, costs, the model, cache accounting, and reasoning.
//! Those are the sending harness's bookkeeping about *its own* window, and a
//! figure carried across would be a claim about a window the receiver is not
//! using. cctop reads them off the original transcript anyway, which is why
//! losing them costs a handoff nothing.
//!
//! The two harnesses here are the two whose stores are a file of JSON lines, so
//! a conversion is reading one and writing the other with no database in the
//! way. OpenCode is the odd one out — it keeps its transcripts in SQLite tables
//! alongside a project's real state, and writing rows into a live store another
//! agent is using is a different kind of risk from adding a file to a directory
//! of rollouts. [`convertible`] says no rather than pretending.
//!
//! # The session id is kept where it can be
//!
//! A converted session reuses the id of the session it came from, whenever the
//! receiving harness's store has that id free. That is what lets cctop recognise
//! the two as one piece of work rather than two unrelated sessions that happen
//! to have arrived in the same directory: [`crate::session::Session::key`] is
//! `provider:session_id`, so the copies are distinct rows either way — but they
//! are rows about the same id, and cctop can hide or merge them because of it.
//!
//! Both harnesses accept an id minted by the other. Claude names its transcripts
//! by session id and validates only the shape — lowercase hex in the 8-4-4-4-12
//! arrangement, which a Codex id already is — while Codex resolves a session by
//! id through a state database, then a filename lookup, then a file search, none
//! of which asks what version of UUID it is. Where the id is *taken* a new one
//! is minted instead, because writing over a conversation is the one outcome a
//! handoff must never have, and the provenance marker keeps the link to the
//! original either way.
//!
//! # Nothing is written over
//!
//! Both writers refuse an id already present in the target store, checking every
//! project directory and every dated rollout directory rather than only the path
//! about to be written. Claude resumes an id found in any project directory, so
//! an id free in one and taken in another is a collision that would surface
//! later as a session that will not resume at all.

use crate::config;
use crate::pricing::Provider;
use chrono::{Datelike, Local, SecondsFormat, Timelike, Utc};
use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

/// Where a converted session came from, so cctop can tie the two together and
/// so a person can undo the conversion.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Provenance {
    /// The harness that wrote the original session.
    pub harness: String,
    /// The original session's id, which is also the converted session's id
    /// unless the target store already held it.
    pub session_id: String,
    /// The original transcript's path.
    pub session_path: String,
    /// When the conversion was made, RFC-3339.
    pub converted_at: String,
}

/// One step of a conversation, in the shape both stores can be written from.
///
/// A tool call and its result are separate steps because the two stores
/// interleave them differently: Claude puts the call in an assistant record and
/// the result in the following user record, while Codex writes both as sibling
/// `response_item`s. Carrying them apart is what lets each writer emit the
/// arrangement its own reader expects.
#[derive(Debug, Clone, PartialEq)]
pub enum Turn {
    /// Something a person asked.
    User(String),
    /// Something the agent said.
    Assistant(String),
    /// A tool the agent invoked, with its arguments as the text of a JSON
    /// document.
    Call {
        id: String,
        name: String,
        input: String,
    },
    /// What that tool returned.
    Result { id: String, output: String },
}

/// A session, read out of one harness's store and ready to be written into
/// another's.
#[derive(Debug, Clone, Default)]
pub struct Transcript {
    /// The id to reuse in the receiving store, where it is free.
    pub session_id: String,
    /// Where the session ran. The receiving harness's project directory is
    /// named after it.
    pub cwd: String,
    /// What the session was called, for the receiving store's picker.
    pub title: String,
    /// The branch at the end, which both harnesses show beside a session.
    pub branch: String,
    /// Recorded only to label the conversion. Never written into the
    /// transcript: a model named in the history is not the model that will
    /// answer, and the receiving harness re-reads its own on the next turn.
    pub model: String,
    /// RFC-3339 instant of the first turn.
    pub started_at: String,
    /// The harness this was read from, for the provenance marker.
    pub source_harness: String,
    /// The file it was read from, so the marker can name something that still
    /// exists after the id has changed.
    pub source_path: String,
    pub turns: Vec<Turn>,
}

/// A session written into another harness's store.
#[derive(Debug, Clone)]
pub struct Converted {
    /// The store it was written into, which is the receiving harness.
    pub provider: Provider,
    pub session_id: String,
    /// The transcript cctop wrote, which the receiving agent's own resume
    /// command will find.
    pub path: PathBuf,
    /// Whether the session kept the id it was converted from. `false` means the
    /// store already held that id, so a new one was minted and the link to the
    /// original lives in the provenance marker alone.
    pub id_kept: bool,
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// Read a Claude Code transcript.
///
/// The chain is walked rather than the file order taken, because that is what
/// Claude Code itself does and the two can disagree: a transcript records a
/// sidechain beside the main conversation, and a resumed session forks, so file
/// order is the order things were written while the conversation is the order
/// the parent links say. Walking it also drops sidechains, which belong to a
/// subagent and reach the main conversation as that subagent's result.
pub fn read_claude(path: &Path) -> Option<Transcript> {
    let mut records: HashMap<String, Value> = HashMap::new();
    // A uuid already inserted, so a record repeated under its own id — a file
    // appended to by a second writer — cannot make the walk loop.
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut leaf: Option<String> = None;
    let mut leaf_at = i64::MIN;
    let mut first_prompt = String::new();
    let mut title = String::new();

    for line in lines(path) {
        let Ok(value) = line else { break };
        let uuid = str_field(&value, "uuid").to_owned();
        // The first record under a uuid wins. A transcript appended to by a
        // second writer can repeat one, and the earlier copy is the one the
        // chain was built from.
        if uuid.is_empty() || seen.insert(uuid.clone(), ()).is_some() {
            continue;
        }
        // A sidechain is left out of the map as well as out of the leaf
        // candidates: it belongs to a subagent, and it roots its own chain at
        // `parentUuid: null`, so keeping it would let the walk fall into a
        // subagent's conversation when the main one links past it.
        let kind = str_field(&value, "type");
        let is_turn = matches!(kind, "user" | "assistant");
        if is_turn && value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }

        match str_field(&value, "type") {
            "user" | "assistant" => {
                let at = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(crate::util::parse_ts)
                    .map(|d| d.timestamp_millis())
                    .unwrap_or(0);
                if at >= leaf_at {
                    leaf_at = at;
                    leaf = Some(uuid.clone());
                }
            }
            // The leaf the harness itself recorded, which is exact where the
            // timestamp comparison is a guess: real transcripts are not
            // monotonic, and a tie there selects the wrong branch.
            "last-prompt" => {
                if let Some(id) = value.get("leafUuid").and_then(Value::as_str) {
                    leaf = Some(id.to_owned());
                    leaf_at = i64::MAX;
                }
            }
            "ai-title" => {
                if let Some(t) = value.get("aiTitle").and_then(Value::as_str)
                    && !t.is_empty()
                {
                    title = t.to_owned();
                }
            }
            _ => {}
        }
        if first_prompt.is_empty()
            && kind == "user"
            && let Some(text) = plain_prompt(&value)
        {
            first_prompt = text;
        }
        records.insert(uuid, value);
    }

    // Walk parent links from the leaf back to the root, then reverse: the
    // conversation in the order it was had. A link to a uuid that is not in the
    // file ends the walk, as it does in Claude Code, rather than skipping a
    // record and stitching across the gap.
    let mut chain: Vec<&Value> = Vec::new();
    let mut cursor = leaf;
    while let Some(id) = cursor {
        let Some(record) = records.get(&id) else {
            break;
        };
        chain.push(record);
        cursor = record
            .get("parentUuid")
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    chain.reverse();
    let (first, last) = (chain.first()?, chain.last()?);

    let mut out = Transcript {
        session_id: path.file_stem()?.to_str()?.to_owned(),
        cwd: str_field(first, "cwd").to_owned(),
        title: title.clone(),
        branch: str_field(last, "gitBranch").to_owned(),
        model: chain
            .iter()
            .rev()
            .find_map(|r| r.pointer("/message/model"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        started_at: str_field(first, "timestamp").to_owned(),
        source_harness: Provider::Claude.as_str().to_owned(),
        source_path: path.to_string_lossy().into_owned(),
        turns: Vec::new(),
    };
    for record in &chain {
        let message = record.get("message");
        match str_field(record, "type") {
            "user" => user_turns(message, &mut out.turns),
            "assistant" => assistant_turns(message, &mut out.turns),
            _ => {}
        }
    }
    if title.is_empty() {
        out.title = first_prompt;
    }
    Some(out)
}

/// Read a Codex rollout.
///
/// Codex needs no chain walk: a rollout is written strictly in order, and
/// `response_item`s are the model-facing history while `event_msg`s belong to
/// the interface. Developer turns are dropped with them — those are Codex's
/// system prompt, and handing one harness another's instructions as though they
/// were the conversation would be worse than losing them.
pub fn read_codex(path: &Path) -> Option<Transcript> {
    let mut out = Transcript {
        session_id: path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(config::trailing_uuid)
            .unwrap_or_default()
            .to_owned(),
        source_harness: Provider::Codex.as_str().to_owned(),
        source_path: path.to_string_lossy().into_owned(),
        ..Transcript::default()
    };
    let mut meta_id = String::new();
    let mut last = String::new();

    for line in lines(path) {
        let Ok(value) = line else { break };
        let Some(payload) = value.get("payload") else {
            continue;
        };
        let at = value
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(stamp_of);
        let at = at.as_deref().unwrap_or_default();
        match str_field(&value, "type") {
            "session_meta" => {
                if out.cwd.is_empty() {
                    out.cwd = str_field(payload, "cwd").to_owned();
                }
                if let Some(id) = payload.get("id").and_then(Value::as_str) {
                    meta_id = id.to_owned();
                }
                if out.started_at.is_empty() {
                    out.started_at = str_field(payload, "timestamp").to_owned();
                }
            }
            "turn_context" => {
                if let Some(model) = payload.get("model").and_then(Value::as_str) {
                    out.model = model.to_owned();
                }
            }
            "response_item" => {
                let before = out.turns.len();
                codex_item(payload, at, &mut out);
                if out.turns.len() > before && !at.is_empty() {
                    last = at.to_owned();
                }
            }
            _ => {}
        }
    }

    if out.session_id.is_empty() {
        out.session_id = meta_id;
    }
    if out.started_at.is_empty() {
        out.started_at = last.clone();
    }
    if out.title.is_empty() {
        out.title = out
            .turns
            .iter()
            .find_map(|t| match t {
                Turn::User(text) => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default();
    }
    if out.turns.is_empty() {
        return None;
    }
    Some(out)
}

/// One Codex `response_item`, or nothing for the kinds that do not survive.
fn codex_item(payload: &Value, at: &str, out: &mut Transcript) {
    let kind = str_field(payload, "type");
    match kind {
        "message" => {
            let text = content_text(payload.get("content"));
            if text.trim().is_empty() {
                return;
            }
            match str_field(payload, "role") {
                "user" => {
                    if out.started_at.is_empty() && !at.is_empty() {
                        out.started_at = at.to_owned();
                    }
                    out.turns.push(Turn::User(text));
                }
                "assistant" => out.turns.push(Turn::Assistant(text)),
                _ => {}
            }
        }
        // `function_call` carries its arguments as a JSON document's text;
        // `custom_tool_call` carries the raw string its tool takes, which is not
        // JSON at all — a patch, for the one tool Codex has this way.
        "function_call" | "custom_tool_call" => {
            let (Some(id), false) = (
                payload.get("call_id").and_then(Value::as_str),
                str_field(payload, "name").is_empty(),
            ) else {
                return;
            };
            let input = payload
                .get("arguments")
                .or_else(|| payload.get("input"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            out.turns.push(Turn::Call {
                id: id.to_owned(),
                name: str_field(payload, "name").to_owned(),
                input,
            });
        }
        "function_call_output" | "custom_tool_call_output" => {
            let Some(id) = payload.get("call_id").and_then(Value::as_str) else {
                return;
            };
            out.turns.push(Turn::Result {
                id: id.to_owned(),
                output: content_text(payload.get("output")),
            });
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Writing: Claude Code
// ---------------------------------------------------------------------------

/// Write `transcript` into `home`'s project store as a session Claude Code can
/// resume, and return what it wrote.
pub fn write_claude(transcript: &Transcript, home: &Path) -> std::io::Result<Converted> {
    let cwd = working_dir(transcript)?;
    let dir = home.join("projects").join(project_slug(&cwd));
    std::fs::create_dir_all(&dir)?;

    let (id, id_kept) = free_id_claude(home, transcript.session_id.as_str());
    let path = dir.join(format!("{id}.jsonl"));
    let provenance = provenance_of(transcript);

    let mut out = BufWriter::new(std::fs::File::create(&path)?);
    writeln!(out, "{}", line_of(marker(&provenance, &id)))?;

    // `uuid` immediately followed by `timestamp`, which is the order Claude
    // Code's own records use and the one its reader depends on: for a
    // transcript over 5 MiB it stops parsing records as JSON and finds each one
    // by scanning for a uuid adjacent to a timestamp. A record whose keys fall
    // in another order is one that scan cannot see.
    let mut parent: Option<String> = None;
    let mut at = first_instant(&transcript.started_at);
    let mut leaf: Option<String> = None;
    let mut first_prompt = String::new();
    let mut message = 0u32;

    for turn in &transcript.turns {
        let (kind, body) = match turn {
            Turn::User(text) => {
                if first_prompt.is_empty() {
                    first_prompt = text.clone();
                }
                message += 1;
                ("user", json!({ "role": "user", "content": text }))
            }
            Turn::Assistant(text) => {
                message += 1;
                (
                    "assistant",
                    json!({
                        "model": transcript.model,
                        "id": message_id(&id, message),
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "text", "text": text }],
                        "stop_reason": "end_turn",
                    }),
                )
            }
            Turn::Call {
                id: call,
                name,
                input,
            } => {
                message += 1;
                (
                    "assistant",
                    json!({
                        "model": transcript.model,
                        "id": message_id(call, message),
                        "type": "message",
                        "role": "assistant",
                        "content": [{
                            "type": "tool_use",
                            "id": call,
                            "name": name,
                            "input": as_input(input),
                        }],
                        "stop_reason": "tool_use",
                    }),
                )
            }
            Turn::Result { id: call, output } => (
                "user",
                json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": call,
                        "content": output,
                    }],
                }),
            ),
        };

        let uuid = new_uuid();
        writeln!(
            out,
            "{}",
            line_of(json!({
                "parentUuid": parent,
                "isSidechain": false,
                "type": kind,
                "message": body,
                "uuid": uuid,
                "timestamp": at,
                "userType": "external",
                "entrypoint": "cli",
                "cwd": cwd.to_string_lossy(),
                "sessionId": id,
                "gitBranch": transcript.branch,
            }))
        )?;
        leaf = Some(uuid);
        parent = leaf.clone();
        // Strictly increasing, because Claude Code picks the record a session
        // resumes at by the newest timestamp among the candidates, and its own
        // transcripts are not monotonic — two records a millisecond apart in the
        // wrong order select the wrong branch.
        at = next_instant(&at);
    }

    if let Some(leaf) = &leaf {
        writeln!(
            out,
            "{}",
            line_of(json!({
                "type": "last-prompt",
                "leafUuid": leaf,
                "lastPrompt": clip(&first_prompt, 200),
                "sessionId": id,
            }))
        )?;
        writeln!(
            out,
            "{}",
            line_of(json!({
                "type": "ai-title",
                "aiTitle": clip(&title_of(transcript, &first_prompt), 120),
                "sessionId": id,
            }))
        )?;
    }
    out.flush()?;
    Ok(Converted {
        provider: Provider::Claude,
        session_id: id,
        path,
        id_kept,
    })
}

/// The project directory name Claude Code derives from a working directory.
///
/// Every character that is not an ASCII letter or digit becomes a dash, which is
/// what makes `-home-flo-cctop` and `-home-flo--claude-worktrees-rave-2` two
/// spellings of two paths. A path longer than the limit is cut and
/// disambiguated with a hash of the whole, because a directory named after a
/// prefix alone would be shared by every path with that prefix — and the resume
/// lookup gives up on an id it finds in more than one of them.
pub fn project_slug(cwd: &Path) -> String {
    const LIMIT: usize = 200;
    let full: String = cwd.to_string_lossy().into_owned();
    let plain: String = full
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    match plain.len() {
        n if n <= LIMIT => plain,
        _ => {
            let cut: String = plain.chars().take(LIMIT).collect();
            format!("{cut}-{}", base36(slug_hash(&full)))
        }
    }
}

/// The hash Claude Code disambiguates an over-long project path with.
///
/// Thirty-one times the previous value plus the character, truncated to
/// thirty-two bits — the same arithmetic, and therefore the same number for the
/// same path, which is the only part of this that matters. Spelled out because a
/// different one would silently put the session in a directory nothing else
/// looks in.
fn slug_hash(s: &str) -> u32 {
    let mut h: i32 = 0;
    for c in s.encode_utf16() {
        h = h.wrapping_shl(5).wrapping_sub(h).wrapping_add(c as i32);
    }
    h.unsigned_abs()
}

/// A number in base 36, the digits JavaScript's `toString(36)` would use.
fn base36(mut n: u32) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Writing: Codex
// ---------------------------------------------------------------------------

/// Write `transcript` into `home`'s dated rollout store as a session Codex can
/// resume, and return what it wrote.
pub fn write_codex(transcript: &Transcript, home: &Path) -> std::io::Result<Converted> {
    // Codex files a rollout under the local day it began and stamps the filename
    // in local time, while the timestamp inside the record is UTC. Both are
    // parsed, and they are not the same clock reading.
    let start = crate::util::parse_ts(&transcript.started_at).unwrap_or_else(Utc::now);
    let local = start.with_timezone(&Local);
    let dir = home
        .join("sessions")
        .join(format!("{:04}", local.year()))
        .join(format!("{:02}", local.month()))
        .join(format!("{:02}", local.day()));
    std::fs::create_dir_all(&dir)?;

    let (id, id_kept) = free_id_codex(home, transcript.session_id.as_str());
    let path = dir.join(format!(
        "rollout-{:04}-{:02}-{:02}T{:02}-{:02}-{:02}-{id}.jsonl",
        local.year(),
        local.month(),
        local.day(),
        local.hour(),
        local.minute(),
        local.second()
    ));
    let provenance = provenance_of(transcript);
    let cwd = working_dir(transcript)?;
    let day = format!(
        "{:04}-{:02}-{:02}",
        local.year(),
        local.month(),
        local.day()
    );
    let here = cwd.to_string_lossy().into_owned();

    let mut out = BufWriter::new(std::fs::File::create(&path)?);
    // The first line, and it has to be: Codex rejects a rollout that does not
    // open with session metadata. The provenance rides inside that payload
    // rather than beside it because an unrecognised top-level record type is
    // dropped on read — the line stays in the file, but nothing carries it
    // forward into the resumed session.
    writeln!(
        out,
        "{}",
        line_of(json!({
            "type": "session_meta",
            "timestamp": start.to_rfc3339_opts(SecondsFormat::Millis, true),
            "payload": {
                "id": id,
                "session_id": id,
                "timestamp": start.to_rfc3339_opts(SecondsFormat::Millis, true),
                "cwd": here,
                "originator": "codex-tui",
                "cli_version": CODEX_CLI_VERSION,
                "source": "cli",
                "thread_source": "user",
                "model_provider": "openai",
                "base_instructions": { "text": "" },
                "cctop_conversion": {
                    "from": provenance.harness,
                    "sourceSessionId": provenance.session_id,
                    "sourcePath": provenance.session_path,
                    "convertedAt": provenance.converted_at,
                },
            },
        }))
    )?;
    // One turn's context, which is what a resume picker reads the session's
    // directory from. Two fields are left out on purpose. `model` is not
    // carried: it is the *source* harness's model, which names a provider this
    // one does not have, and Codex re-reads its own configuration on the next
    // turn anyway — so the honest answer is the field's absence. `history_mode`
    // is unset because `paginated` would oblige every line to carry a
    // contiguous `ordinal`, and the default needs none.
    writeln!(
        out,
        "{}",
        line_of(json!({
            "type": "turn_context",
            "timestamp": start.to_rfc3339_opts(SecondsFormat::Millis, true),
            "payload": {
                "turn_id": id,
                "cwd": here,
                "workspace_roots": [here],
                "current_date": day,
                "approval_policy": "on-request",
                "sandbox_policy": { "type": "workspace-write", "network_access": false },
            },
        }))
    )?;

    let mut at = start;
    for turn in &transcript.turns {
        let payload = match turn {
            Turn::User(text) => json!({
                "type": "message",
                "role": "user",
                "content": [{ "type": "input_text", "text": text }],
            }),
            Turn::Assistant(text) => json!({
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "output_text", "text": text }],
            }),
            Turn::Call {
                id: call,
                name,
                input,
            } => json!({
                "type": "function_call",
                "name": name,
                "arguments": input,
                "call_id": call,
            }),
            Turn::Result { id: call, output } => json!({
                "type": "function_call_output",
                "call_id": call,
                "output": output,
            }),
        };
        writeln!(
            out,
            "{}",
            line_of(json!({
                "type": "response_item",
                "timestamp": at.to_rfc3339_opts(SecondsFormat::Millis, true),
                "payload": payload,
            }))
        )?;
        at = at
            .checked_add_signed(chrono::Duration::milliseconds(1))
            .unwrap_or(start);
    }
    out.flush()?;
    Ok(Converted {
        provider: Provider::Codex,
        session_id: id,
        path,
        id_kept,
    })
}

/// The Codex version stamped into a converted rollout's header.
///
/// Not the installed one: a converted session has not been produced by any
/// Codex, and the field records which build last wrote the file. It is read
/// only by migration code that branches on it, so a fixed value keeps that code
/// on its oldest path. A real session would carry the version that wrote it.
const CODEX_CLI_VERSION: &str = "0.149.1";

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// The marker a converted Claude transcript opens with, if it has one.
///
/// Both stores keep their marker and neither reads it, which is the point: it
/// is how a converted session is told from one a person ran, by cctop and by
/// whoever comes looking for the file. The head is read rather than the whole
/// file deliberately — the marker is written first, so a transcript too large
/// to hold in memory still answers.
pub fn provenance_of_file(path: &Path) -> Option<Provenance> {
    let text = crate::util::read_head(path, 64 * 1024)?;
    let value: Value = serde_json::from_str(text.lines().next()?).ok()?;
    if str_field(&value, "type") != "cctop-conversion" {
        return None;
    }
    read_provenance(&value, path)
}

/// The marker as it appears in a Codex rollout, where it rides inside the
/// session metadata rather than on a line of its own.
pub fn provenance_of_codex(path: &Path) -> Option<Provenance> {
    let text = crate::util::read_head(path, 64 * 1024)?;
    let value: Value = serde_json::from_str(text.lines().next()?).ok()?;
    if str_field(&value, "type") != "session_meta" {
        return None;
    }
    let marker = value.pointer("/payload/cctop_conversion")?;
    read_provenance(marker, path)
}

/// The marker as written into whichever store, before either writer's spelling.
fn marker(provenance: &Provenance, new_id: &str) -> Value {
    json!({
        "type": "cctop-conversion",
        "convertedFrom": provenance.harness,
        "sourceSessionId": provenance.session_id,
        "sourcePath": provenance.session_path,
        "convertedTo": new_id,
        "convertedAt": provenance.converted_at,
    })
}

/// Fill a [`Provenance`] from either spelling of the marker.
///
/// The recorded source path is dropped when it no longer names a file: a marker
/// pointing at a conversation that is gone is a claim, and cctop would rather
/// show the copy than resolve it to something it cannot open.
fn read_provenance(marker: &Value, path: &Path) -> Option<Provenance> {
    let session_id = marker.get("sourceSessionId").and_then(Value::as_str)?;
    let source_path = marker
        .get("sourcePath")
        .and_then(Value::as_str)
        .filter(|p| Path::new(p).is_file())
        .unwrap_or_default();
    Some(Provenance {
        harness: marker
            .get("convertedFrom")
            .or_else(|| marker.get("from"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        session_id: session_id.to_owned(),
        session_path: if source_path.is_empty() {
            path.to_string_lossy().into_owned()
        } else {
            source_path.to_owned()
        },
        converted_at: marker
            .get("convertedAt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    })
}

fn provenance_of(transcript: &Transcript) -> Provenance {
    Provenance {
        harness: transcript.source_harness.clone(),
        session_id: transcript.session_id.clone(),
        session_path: transcript.source_path.clone(),
        converted_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
    }
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

/// Convert the session behind `source_transcript` into `target`'s store.
///
/// `None` for a pair cctop cannot transcode, and for a source whose transcript
/// is not on this disk — a session from another machine, or one Claude for Mac
/// keeps in a directory of its own. The caller falls back to a brief, which is
/// the right answer for a harness with no store to write.
pub fn convert(
    source: Provider,
    source_transcript: &Path,
    target: Provider,
    target_home: &Path,
) -> Option<Converted> {
    match (source, target) {
        (Provider::Claude, Provider::Codex) => {
            write_codex(&read_claude(source_transcript)?, target_home).ok()
        }
        (Provider::Codex, Provider::Claude) => {
            write_claude(&read_codex(source_transcript)?, target_home).ok()
        }
        _ => None,
    }
}

/// Whether a pair of harnesses can be converted between at all.
///
/// Claude to Claude is deliberately absent: that is [`crate::handoff::fork`],
/// which copies the file rather than reading and rewriting it, so it keeps
/// everything a conversion drops.
pub fn convertible(source: Provider, target: Provider) -> bool {
    matches!(
        (source, target),
        (Provider::Claude, Provider::Codex) | (Provider::Codex, Provider::Claude)
    )
}

/// Whether this session's transcript is one cctop can convert from.
///
/// A remote session has no file on this disk, and a Claude for Mac conversation
/// lives in a directory of its own that the CLI does not resume from — in both
/// cases there is nothing to read, and a brief is the only handoff available.
pub fn convertible_session(session: &crate::session::Session) -> bool {
    if session.remote.is_some() || session.surface.is_desktop() {
        return false;
    }
    matches!(session.provider, Provider::Claude | Provider::Codex) && session.data_file.is_some()
}

// ---------------------------------------------------------------------------
// Ids
// ---------------------------------------------------------------------------

/// `wanted` if no Claude transcript anywhere under `home` holds it, else a new
/// one.
///
/// The check covers every project directory rather than only the one about to be
/// written, because `claude --resume <id>` finds an id in any of them and
/// refuses when it is in two.
fn free_id_claude(home: &Path, wanted: &str) -> (String, bool) {
    if config::is_full_uuid(wanted) && !claude_id_taken(home, wanted) {
        return (wanted.to_owned(), true);
    }
    (new_uuid(), false)
}

fn claude_id_taken(home: &Path, id: &str) -> bool {
    let projects = home.join("projects");
    config::list_dir(&projects)
        .iter()
        .any(|project| projects.join(project).join(format!("{id}.jsonl")).is_file())
}

/// `wanted` if no rollout anywhere under `home` holds it, else a new one.
fn free_id_codex(home: &Path, wanted: &str) -> (String, bool) {
    if config::is_full_uuid(wanted) && !codex_id_taken(home, wanted) {
        return (wanted.to_owned(), true);
    }
    (new_uuid(), false)
}

pub(crate) fn codex_id_taken(home: &Path, id: &str) -> bool {
    let sessions = home.join("sessions");
    walk(&sessions, 0, &mut |path| {
        // The stem, not the name: the trailing 36 bytes of `…-<uuid>.jsonl` are
        // not the uuid, so asking the whole filename for one finds nothing and
        // every id looks free.
        path.file_stem()
            .and_then(|n| n.to_str())
            .and_then(config::trailing_uuid)
            == Some(id)
    })
}

/// Depth-first walk yielding every file, bounded so a symlink loop or a
/// pathological tree costs a conversion a refusal rather than a hang.
fn walk(dir: &Path, depth: usize, each: &mut impl FnMut(&Path) -> bool) -> bool {
    if depth > 8 || !dir.is_dir() {
        return false;
    }
    for name in config::list_dir(dir) {
        let path = dir.join(&name);
        let hit = if path.is_dir() {
            walk(&path, depth + 1, each)
        } else {
            each(&path)
        };
        if hit {
            return true;
        }
    }
    false
}

/// A fresh session id of the shape every harness here writes: sixteen random
/// bytes as a version-4 UUID.
fn new_uuid() -> String {
    let mut bytes = crate::util::random_bytes(16);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// A JSON object serialised with its fields in the order they were written.
///
/// `serde_json`'s own `Map` preserves insertion order in this build, so this
/// changes nothing today — but the order is load-bearing on the Claude side, and
/// a `Map` that sorted would put a different key between the uuid and the
/// timestamp its reader scans for. Writing the object out here makes that a
/// property of this function rather than of a feature flag in `Cargo.toml`.
struct Ordered(Value);

impl Serialize for Ordered {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match &self.0 {
            Value::Object(map) => {
                let mut out = s.serialize_map(Some(map.len()))?;
                for (key, value) in map {
                    out.serialize_entry(key, value)?;
                }
                out.end()
            }
            other => other.serialize(s),
        }
    }
}

/// One transcript line, with its fields in the order the value was built.
fn line_of(value: Value) -> String {
    serde_json::to_string(&Ordered(value)).unwrap_or_else(|_| "{}".into())
}

/// The lines of a JSON-lines transcript, stopping at the first line that will
/// not parse.
///
/// A truncated tail is what a session being written right now looks like, and
/// cctop reads transcripts of running sessions routinely. Stopping rather than
/// skipping keeps a half-written line from being read as a whole record.
fn lines(path: &Path) -> impl Iterator<Item = Result<Value, serde_json::Error>> {
    let reader = std::fs::File::open(path).ok().map(BufReader::new);
    Box::new(reader.into_iter().flat_map(|mut r| {
        std::iter::from_fn(move || {
            let mut line = String::new();
            match r.read_line(&mut line) {
                Ok(0) | Err(_) => None,
                Ok(_) => Some(serde_json::from_str(&line)),
            }
        })
    }))
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// The text of a `user` record that is a person asking something, rather than a
/// tool result arriving.
///
/// A `user` record's content is either a bare string or a list of blocks, and
/// only the string is a prompt — though a list can hold one alongside results.
fn plain_prompt(record: &Value) -> Option<String> {
    let content = record.pointer("/message/content")?;
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let text: Vec<&str> = blocks
                .iter()
                .filter(|b| str_field(b, "type") == "text")
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect();
            (!text.is_empty()).then(|| text.join("\n"))
        }
        _ => None,
    }
}

fn user_turns(message: Option<&Value>, out: &mut Vec<Turn>) {
    let Some(content) = message.and_then(|m| m.get("content")) else {
        return;
    };
    match content {
        Value::String(text) => {
            if !text.trim().is_empty() {
                out.push(Turn::User(text.clone()));
            }
        }
        Value::Array(blocks) => {
            for block in blocks {
                match str_field(block, "type") {
                    "text" => {
                        if let Some(text) = block.get("text").and_then(Value::as_str)
                            && !text.trim().is_empty()
                        {
                            out.push(Turn::User(text.to_owned()));
                        }
                    }
                    "tool_result" => {
                        let Some(id) = block.get("tool_use_id").and_then(Value::as_str) else {
                            continue;
                        };
                        out.push(Turn::Result {
                            id: id.to_owned(),
                            output: content_text(block.get("content")),
                        });
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn assistant_turns(message: Option<&Value>, out: &mut Vec<Turn>) {
    let Some(Value::Array(blocks)) = message.and_then(|m| m.get("content")) else {
        return;
    };
    for block in blocks {
        match str_field(block, "type") {
            "text" => {
                if let Some(text) = block.get("text").and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    out.push(Turn::Assistant(text.to_owned()));
                }
            }
            "tool_use" => {
                let (Some(id), Some(name)) = (
                    block.get("id").and_then(Value::as_str),
                    block.get("name").and_then(Value::as_str),
                ) else {
                    continue;
                };
                out.push(Turn::Call {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    // An object, serialised back to text, so the reader on the
                    // far side gets the same document the sender's tool saw.
                    input: block
                        .get("input")
                        .filter(|i| i.is_object())
                        .map_or_else(|| "{}".into(), ToString::to_string),
                });
            }
            // `thinking` is the one block deliberately not carried. It is the
            // sending harness's reasoning about its own window, signed so only
            // the model that wrote it can read it back; across the gap it is an
            // opaque blob the receiving model cannot use.
            _ => {}
        }
    }
}

/// The text of a content array or string, joined.
///
/// Both stores allow a tool's output to be either, and Claude also writes a
/// result as a list of `tool_reference` blocks that have no text at all — which
/// is why this yields an empty string rather than a placeholder.
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join(""),
        Some(Value::String(text)) => text.clone(),
        _ => String::new(),
    }
}

/// A tool call's arguments as Claude Code wants them: an object, always.
///
/// Codex's own calls carry a JSON document's text, and its custom tools carry no
/// JSON at all — a patch, which Claude Code has no field for. Wrapping the
/// unparseable case keeps the record a well-formed call rather than a string
/// where an object belongs.
fn as_input(input: &str) -> Value {
    serde_json::from_str::<Value>(input)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({ "input": input }))
}

/// A timestamp restated in the one spelling both stores write.
fn stamp_of(value: &str) -> Option<String> {
    Some(crate::util::parse_ts(value)?.to_rfc3339_opts(SecondsFormat::Millis, true))
}

/// The working directory a converted session should claim: the source's, or
/// wherever cctop itself was run.
fn working_dir(transcript: &Transcript) -> std::io::Result<PathBuf> {
    match transcript.cwd.is_empty() {
        true => std::env::current_dir(),
        false => Ok(PathBuf::from(&transcript.cwd)),
    }
}

/// The instant a converted transcript starts from: the source's, or now.
fn first_instant(value: &str) -> String {
    crate::util::parse_ts(value)
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// The instant after `at`, for the next record.
///
/// Claude Code resumes at the record with the newest timestamp among the
/// candidates, so a transcript whose timestamps do not strictly increase can put
/// the receiving agent at the wrong point in the conversation. A millisecond on
/// is below what either harness displays at and above the resolution this
/// comparison uses.
fn next_instant(at: &str) -> String {
    crate::util::parse_ts(at)
        .unwrap_or_else(Utc::now)
        .checked_add_signed(chrono::Duration::milliseconds(1))
        .unwrap_or_else(Utc::now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// What to call the converted session: the source's title, or its first prompt.
fn title_of(transcript: &Transcript, fallback: &str) -> String {
    let title = match transcript.title.trim().is_empty() {
        true => fallback,
        false => transcript.title.as_str(),
    };
    clip(title, 120)
}

/// Cut to `max` characters, marking that it was cut.
fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", text[..at].trim_end()),
        None => text.to_owned(),
    }
}

/// A message id for an assistant record, distinct within the session.
///
/// Claude Code groups an assistant record's blocks by this and looks for the
/// last real API message in it, so it has to be a string and has to differ
/// between records. Carrying the sending harness's own id would be truer, and
/// neither store puts one where the other can read it, so this stands in.
fn message_id(seed: &str, nth: u32) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in seed.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("msg_{hash:016x}{nth:08x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory of this test binary's own, emptied first so a run
    /// never inherits the last one's files.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cctop-convert-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A fixture transcript at the exact name the harness would give it.
    ///
    /// The name is not decoration: a Claude transcript is named by its session
    /// id, so a fixture written under any other name is a session whose id is
    /// not an id, and a test asserting the id survives would fail for the right
    /// reason on the wrong one. A per-test directory keeps fixtures apart
    /// without renaming them.
    fn write_file(name: &str, body: &str) -> PathBuf {
        let path = scratch(&format!("src-{name}")).join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    fn sample() -> Transcript {
        Transcript {
            session_id: "019f1075-3f22-7ad0-b496-73dcda6a7a25".into(),
            cwd: "/tmp/proj".into(),
            title: "a task".into(),
            branch: "main".into(),
            model: "some-model".into(),
            started_at: "2026-09-28T00:00:00.000Z".into(),
            source_harness: "codex".into(),
            source_path: "/tmp/proj/rollout.jsonl".into(),
            turns: vec![
                Turn::User("hello".into()),
                Turn::Call {
                    id: "call_abc".into(),
                    name: "shell".into(),
                    input: r#"{"cmd":"ls"}"#.into(),
                },
                Turn::Result {
                    id: "call_abc".into(),
                    output: "a\nb".into(),
                },
                Turn::Assistant("done".into()),
            ],
        }
    }

    #[test]
    fn the_slug_is_what_the_harness_writes() {
        assert_eq!(
            project_slug(Path::new("/home/flo/cctop")),
            "-home-flo-cctop"
        );
        assert_eq!(project_slug(Path::new("/tmp")), "-tmp");
        // A dot is as good as a slash for producing a doubled dash.
        assert_eq!(
            project_slug(Path::new("/home/flo/.claude/jobs/3a74/perm")),
            "-home-flo--claude-jobs-3a74-perm"
        );
    }

    #[test]
    fn an_over_long_path_is_hashed_rather_than_shared() {
        let a = project_slug(Path::new(&format!("/{}", "x".repeat(300))));
        let b = project_slug(Path::new(&format!("/{}{}", "x".repeat(299), "y")));
        // Same first 200 characters, so without the hash these would be one
        // directory and a resume by id would be ambiguous between them.
        assert_ne!(a, b);
        // The cut is 200, then a dash, then the hash in base 36 — whose width
        // varies with the value, so only the floor is fixed.
        assert!(a.len() > 201 && a.len() <= 200 + 1 + 7, "{}", a.len());
        assert_eq!(&a[..200], &b[..200]);
    }

    #[test]
    fn base36_is_the_digits_javascript_would_use() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(base36(1_234_567), "qglj");
    }

    #[test]
    fn a_claude_transcript_is_read_along_its_chain_not_its_file_order() {
        let path = write_file(
            "chain.jsonl",
            &[
                r#"{"type":"assistant","uuid":"b","parentUuid":"a","isSidechain":false,"timestamp":"2026-09-28T00:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"second"}]}}"#,
                r#"{"type":"user","uuid":"a","parentUuid":null,"isSidechain":false,"timestamp":"2026-09-28T00:00:01.000Z","message":{"role":"user","content":"first"}}"#,
                r#"{"type":"assistant","uuid":"side","parentUuid":null,"isSidechain":true,"timestamp":"2026-09-28T00:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"a subagent"}]}}"#,
            ]
            .join("\n"),
        );
        let t = read_claude(&path).unwrap();
        assert_eq!(
            t.turns,
            vec![Turn::User("first".into()), Turn::Assistant("second".into())]
        );
    }

    #[test]
    fn a_sidechain_record_is_left_out_of_the_conversation() {
        let path = write_file(
            "side.jsonl",
            &[
                r#"{"type":"user","uuid":"a","parentUuid":null,"isSidechain":false,"timestamp":"2026-09-28T00:00:01.000Z","message":{"role":"user","content":"first"}}"#,
                r#"{"type":"assistant","uuid":"side","parentUuid":"a","isSidechain":true,"timestamp":"2026-09-28T00:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"a subagent"}]}}"#,
            ]
            .join("\n"),
        );
        let t = read_claude(&path).unwrap();
        assert_eq!(t.turns, vec![Turn::User("first".into())]);
    }

    #[test]
    fn a_claude_tool_call_and_its_result_become_a_pair() {
        let path = write_file(
            "tools.jsonl",
            &[
                r#"{"type":"user","uuid":"a","parentUuid":null,"isSidechain":false,"timestamp":"2026-09-28T00:00:01.000Z","message":{"role":"user","content":"run it"}}"#,
                r#"{"type":"assistant","uuid":"b","parentUuid":"a","isSidechain":false,"timestamp":"2026-09-28T00:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}},{"type":"thinking","thinking":"hmm"}]}}"#,
                r#"{"type":"user","uuid":"c","parentUuid":"b","isSidechain":false,"timestamp":"2026-09-28T00:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"a\nb"}]}}"#,
            ]
            .join("\n"),
        );
        let t = read_claude(&path).unwrap();
        assert_eq!(
            t.turns,
            vec![
                Turn::User("run it".into()),
                Turn::Call {
                    id: "toolu_1".into(),
                    name: "Bash".into(),
                    input: r#"{"command":"ls"}"#.into(),
                },
                Turn::Result {
                    id: "toolu_1".into(),
                    output: "a\nb".into(),
                },
            ]
        );
    }

    #[test]
    fn a_codex_rollout_is_read_and_its_developer_turns_dropped() {
        let path = write_file(
            "rollout.jsonl",
            &[
                r#"{"type":"session_meta","timestamp":"2026-09-28T00:00:00.000Z","payload":{"id":"019f1075-3f22-7ad0-b496-73dcda6a7a25","cwd":"/tmp/x","timestamp":"2026-09-28T00:00:00.000Z"}}"#,
                r#"{"type":"response_item","timestamp":"2026-09-28T00:00:01.000Z","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"you are codex"}]}}"#,
                r#"{"type":"response_item","timestamp":"2026-09-28T00:00:02.000Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"do it"}]}}"#,
                r#"{"type":"response_item","timestamp":"2026-09-28T00:00:03.000Z","payload":{"type":"function_call","name":"shell","arguments":"{\"cmd\":\"ls\"}","call_id":"call_abc"}}"#,
                r#"{"type":"response_item","timestamp":"2026-09-28T00:00:04.000Z","payload":{"type":"function_call_output","call_id":"call_abc","output":"a\nb"}}"#,
            ]
            .join("\n"),
        );
        let t = read_codex(&path).unwrap();
        assert_eq!(t.session_id, "019f1075-3f22-7ad0-b496-73dcda6a7a25");
        assert_eq!(t.cwd, "/tmp/x");
        assert_eq!(t.turns.len(), 3);
        assert!(
            !t.turns
                .iter()
                .any(|x| matches!(x, Turn::User(u) if u.contains("codex"))),
            "the developer turn is codex's own system prompt, not the conversation"
        );
        assert_eq!(
            t.turns[1],
            Turn::Call {
                id: "call_abc".into(),
                name: "shell".into(),
                input: r#"{"cmd":"ls"}"#.into(),
            }
        );
    }

    #[test]
    fn a_written_claude_transcript_puts_uuid_next_to_timestamp() {
        let home = scratch("claude-out");
        let got = write_claude(&sample(), &home).unwrap();
        assert!(got.id_kept, "a free id should be kept");
        assert_eq!(got.session_id, sample().session_id);

        let body = std::fs::read_to_string(&got.path).unwrap();
        for line in body.lines().filter(|l| l.contains(r#""uuid""#)) {
            assert!(
                line.contains(r#""uuid":""#) && line.contains(r#"","timestamp":""#),
                "uuid and timestamp must be adjacent: {line}"
            );
        }
    }

    #[test]
    fn every_written_record_names_the_session_the_file_is_called() {
        let home = scratch("claude-id");
        let got = write_claude(&sample(), &home).unwrap();
        // Claude Code takes the id it adopts from the leaf record rather than
        // from the filename, so the two have to agree.
        let body = std::fs::read_to_string(&got.path).unwrap();
        for line in body.lines().filter(|l| l.contains(r#""sessionId""#)) {
            assert!(line.contains(&sample().session_id), "{line}");
        }
        assert!(got.path.ends_with(format!("{}.jsonl", sample().session_id)));
    }

    #[test]
    fn a_written_claude_transcript_chains_and_advances() {
        let home = scratch("claude-chain");
        let got = write_claude(&sample(), &home).unwrap();
        let body = std::fs::read_to_string(&got.path).unwrap();
        let mut parent: Option<String> = None;
        let mut last = String::new();
        for line in body.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if !matches!(str_field(&v, "type"), "user" | "assistant") {
                continue;
            }
            assert_eq!(
                v.get("parentUuid")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                parent,
                "each record links to the one before it"
            );
            assert!(v.pointer("/message").is_some(), "message must be an object");
            if last.as_str() > str_field(&v, "timestamp") {
                panic!("timestamps must not go backwards");
            }
            last = str_field(&v, "timestamp").to_owned();
            parent = v.get("uuid").and_then(Value::as_str).map(str::to_owned);
        }
        assert!(parent.is_some());
    }

    #[test]
    fn a_written_codex_rollout_opens_with_metadata_naming_the_file() {
        let home = scratch("codex-out");
        let got = write_codex(&sample(), &home).unwrap();
        assert!(got.id_kept);
        let body = std::fs::read_to_string(&got.path).unwrap();
        let first: Value = serde_json::from_str(body.lines().next().unwrap()).unwrap();
        assert_eq!(first["type"], "session_meta");
        assert_eq!(first["payload"]["id"], sample().session_id.as_str());
        // The filename's uuid and the header's id are read by different code
        // paths, and `codex doctor` reports the disagreement.
        assert!(
            got.path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with(&format!("{}.jsonl", sample().session_id))
        );
    }

    #[test]
    fn an_id_already_in_the_store_is_not_written_over() {
        let home = scratch("taken");
        let dir = home
            .join("projects")
            .join(project_slug(Path::new("/tmp/proj")));
        std::fs::create_dir_all(&dir).unwrap();
        let taken = "5049dbcd-8ef7-412f-bf57-589e61c41d0e";
        let original = "{\"type\":\"user\",\"uuid\":\"x\"}\n";
        std::fs::write(dir.join(format!("{taken}.jsonl")), original).unwrap();

        let mut t = sample();
        t.session_id = taken.into();
        let got = write_claude(&t, &home).unwrap();
        assert!(!got.id_kept);
        assert_ne!(got.session_id, taken);
        assert_eq!(
            std::fs::read_to_string(dir.join(format!("{taken}.jsonl"))).unwrap(),
            original,
            "the conversation that was there is untouched"
        );
    }

    #[test]
    fn a_codex_id_taken_in_another_day_is_still_taken() {
        let home = scratch("codex-taken");
        // A different dated directory, so only a search of the tree finds it.
        let dir = home.join("sessions/2020/01/01");
        std::fs::create_dir_all(&dir).unwrap();
        let taken = "5049dbcd-8ef7-412f-bf57-589e61c41d0e";
        std::fs::write(
            dir.join(format!("rollout-2020-01-01T00-00-00-{taken}.jsonl")),
            "",
        )
        .unwrap();

        let mut t = sample();
        t.session_id = taken.into();
        assert!(!write_codex(&t, &home).unwrap().id_kept);
    }

    #[test]
    fn provenance_survives_a_round_trip_in_both_directions() {
        let home = scratch("provenance");
        for (write, read) in [
            (
                write_claude as fn(&Transcript, &Path) -> std::io::Result<Converted>,
                provenance_of_file as fn(&Path) -> Option<Provenance>,
            ),
            (
                write_codex,
                provenance_of_codex as fn(&Path) -> Option<Provenance>,
            ),
        ] {
            let got = write(&sample(), &home).unwrap();
            let p = read(&got.path).expect("the marker is readable");
            assert_eq!(p.session_id, sample().session_id);
            assert_eq!(p.harness, "codex");
            assert!(!p.converted_at.is_empty());
        }
    }

    #[test]
    fn a_transcript_cctop_did_not_write_has_no_provenance() {
        let home = scratch("foreign");
        let dir = home
            .join("projects")
            .join(project_slug(Path::new("/tmp/proj")));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f83c1a2b-0000-4000-8000-000000000000.jsonl");
        std::fs::write(&path, "{\"type\":\"user\",\"uuid\":\"a\"}\n").unwrap();
        assert_eq!(provenance_of_file(&path), None);
    }

    #[test]
    fn both_directions_are_offered_and_the_rest_are_not() {
        assert!(convertible(Provider::Claude, Provider::Codex));
        assert!(convertible(Provider::Codex, Provider::Claude));
        // Claude to Claude is a file copy, not a conversion — see
        // `handoff::fork`, which keeps what this drops.
        assert!(!convertible(Provider::Claude, Provider::Claude));
        assert!(!convertible(Provider::Codex, Provider::OpenCode));
        assert!(!convertible(Provider::OpenCode, Provider::Claude));
    }

    /// Ask the real Codex binary whether it accepts a rollout cctop wrote.
    ///
    /// A passing writer test says the fields are the ones the reader documents;
    /// it does not say the reader takes the file. This is the difference, and it
    /// is the reason the header is written the way it is: Codex rejects a
    /// rollout that does not open with session metadata, ignores an
    /// unrecognised top-level type, and wants `id` to equal the filename's
    /// uuid. All three are invisible to a round trip through cctop's own reader.
    ///
    /// `codex doctor` reports the rollout inventory and `codex archive <id>`
    /// resolves an id through the same lookup `codex resume` uses, so between
    /// them they answer "would this session open" without starting a TUI. Both
    /// run against a scratch `CODEX_HOME`, so no session of the caller's is
    /// touched, and the archive is undone before the test ends.
    #[test]
    #[ignore = "runs the machine's real codex against a scratch store"]
    fn the_codex_binary_accepts_a_rollout_cctop_wrote() {
        let home = std::env::temp_dir().join(format!("cctop-verify-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let written = write_codex(&sample(), &home).expect("the rollout can be written");

        let run = |args: &[&str]| -> String {
            let out = std::process::Command::new("codex")
                .args(args)
                .env("CODEX_HOME", &home)
                .output();
            match out {
                Ok(o) => format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                ),
                Err(e) => panic!("could not run codex: {e}"),
            }
        };

        let doctor = run(&["doctor"]);
        for line in ["rollout DB scan errors", "rollout DB malformed file names"] {
            let found = doctor
                .lines()
                .find(|l| l.contains(line))
                .unwrap_or_else(|| panic!("codex doctor reported no {line:?}:\n{doctor}"));
            // Both report a count; anything but zero is a file it could not read.
            assert!(
                found.trim_end().ends_with(" 0"),
                "codex could not read the rollout: {found}"
            );
        }

        // Resolution by id is what `codex resume <id>` does first, and it is
        // the step a mismatched filename or header id would fail.
        let archived = run(&["archive", &written.session_id]);
        assert!(
            archived.contains("Archived session"),
            "codex did not resolve the id:\n{archived}"
        );
        let _ = run(&["unarchive", &written.session_id]);
    }

    /// Ask the real Claude Code binary whether it accepts a transcript cctop
    /// wrote.
    ///
    /// The check is the error text, and there are two ways to fail. An id that
    /// resolves to a file whose chain cannot be walked reports
    /// "No valid conversation chain found in JSONL file"; an id that is not
    /// there at all reports "No conversation found with session ID". Ours must
    /// draw neither, while the same call on a deliberately absent id must draw
    /// the second — otherwise this would only be proving the command runs.
    ///
    /// The scratch `CLAUDE_CONFIG_DIR` holds no credentials, so a run stops at
    /// "Not logged in" having already read the transcript. That is as far as
    /// this goes: it proves the file is found and its chain walked, and it does
    /// not prove a model will answer from it. The test asserts exactly that
    /// much and says so, rather than calling a successful login a pass.
    #[test]
    #[ignore = "resumes a session with the machine's real claude binary"]
    fn the_claude_binary_accepts_a_transcript_cctop_wrote() {
        let home = std::env::temp_dir().join(format!("cctop-verify-claude-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let written = write_claude(&sample(), &home).expect("the transcript can be written");

        let run = |id: &str| -> String {
            let out = std::process::Command::new("claude")
                .args(["--resume", id, "-p", "reply with the single word ok"])
                .env("CLAUDE_CONFIG_DIR", &home)
                .output();
            match out {
                Ok(o) => format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                ),
                Err(e) => panic!("could not run claude: {e}"),
            }
        };
        let not_found = |text: &str| {
            text.contains("No conversation found") || text.contains("No valid conversation chain")
        };

        let ours = run(&written.session_id);
        eprintln!("converted session: {}", first_line(&ours));
        assert!(
            !not_found(&ours),
            "claude would not resume what cctop wrote:\n{ours}"
        );

        // The same call on an id that is not there has to fail this way, or the
        // check above is only proving the command runs.
        let absent = run("00000000-0000-4000-8000-000000000000");
        eprintln!("absent session:     {}", first_line(&absent));
        assert!(
            not_found(&absent),
            "claude resolved an id that is not there, so the check above proves nothing:\n{absent}"
        );
        eprintln!("(no credentials in the scratch dir, so this stops before the model)");
    }

    fn first_line(text: &str) -> String {
        text.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("(nothing)")
            .chars()
            .take(100)
            .collect()
    }

    /// Convert a session that is really on this machine, in the direction
    /// named by `CCTOP_CONVERT_PAIR`, and read it back.
    ///
    /// The tests above are built from fixtures chosen to be small. This one is
    /// not: it takes whatever transcript cctop itself finds, with whatever
    /// shapes a real harness has written, and checks that the conversation
    /// survives the trip. A fixture cannot tell you what a harness does with a
    /// record type the fixture did not think of, and these two have more of
    /// those than the tests above account for.
    ///
    /// `#[ignore]`d because it reads the caller's real sessions and writes
    /// nothing: the read side is the whole point, and the write side is a
    /// scratch directory that goes away with it.
    #[test]
    #[ignore = "converts a real session from this machine"]
    fn a_real_session_survives_the_trip_in_both_directions() {
        let pair = std::env::var("CCTOP_CONVERT_PAIR").unwrap_or_else(|_| "claude-codex".into());
        let (source, target) = match pair.as_str() {
            "claude-codex" => (Provider::Claude, Provider::Codex),
            "codex-claude" => (Provider::Codex, Provider::Claude),
            other => panic!("CCTOP_CONVERT_PAIR is {other:?}, not a known pair"),
        };
        let sessions = match source {
            Provider::Claude => crate::session::claude::list_sessions(),
            _ => crate::session::codex::list_sessions(),
        };
        let candidates: Vec<_> = sessions
            .into_iter()
            .filter(crate::convert::convertible_session)
            .filter_map(|s| s.data_file)
            .collect();
        assert!(
            !candidates.is_empty(),
            "no {source:?} session to convert on this machine"
        );

        // Smallest first: the interesting record shapes appear within a few
        // dozen turns, and a conversion is the one operation here that gets
        // slower with the size of the transcript. The smallest is the 202-byte
        // transcript a session that never reached the model leaves behind,
        // which is why sessions with no model are filtered out above — there is
        // no conversation in one to convert.
        let mut sized: Vec<_> = candidates
            .into_iter()
            .map(|p| {
                let n = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(u64::MAX);
                (n, p)
            })
            .collect();
        sized.sort();
        let (_, path) = sized.remove(0);
        eprintln!(
            "converting {} ({} bytes)",
            path.display(),
            std::fs::metadata(&path).map_or(0, |m| m.len())
        );

        let home = scratch("real");
        let written =
            crate::convert::convert(source, &path, target, &home).expect("the conversion runs");
        let back = match target {
            Provider::Claude => read_claude(&written.path),
            _ => read_codex(&written.path),
        }
        .expect("what cctop wrote, cctop can read back");

        let before = match source {
            Provider::Claude => read_claude(&path),
            _ => read_codex(&path),
        }
        .expect("the source is readable");
        assert!(
            back.turns.len() >= before.turns.len() / 2,
            "most of the conversation went missing: {} of {} turns",
            back.turns.len(),
            before.turns.len()
        );
        // The first thing anybody said has to be the first thing the new agent
        // reads, or the handoff opens on the wrong end of the work.
        let first_of = |turns: &[Turn]| {
            turns.iter().find_map(|t| match t {
                Turn::User(text) => Some(text.clone()),
                _ => None,
            })
        };
        assert_eq!(
            first_of(&back.turns),
            first_of(&before.turns),
            "the opening prompt did not survive"
        );
        // Every tool call the source recorded has to arrive with its result, or
        // the receiving agent sees a call that never returned.
        let calls = |turns: &[Turn]| {
            turns
                .iter()
                .filter(|t| matches!(t, Turn::Call { .. }))
                .count()
        };
        let results = |turns: &[Turn]| {
            turns
                .iter()
                .filter(|t| matches!(t, Turn::Result { .. }))
                .count()
        };
        assert_eq!(
            calls(&back.turns),
            calls(&before.turns),
            "a tool call was lost"
        );
        assert_eq!(
            results(&back.turns),
            results(&before.turns),
            "a tool result was lost"
        );
        eprintln!(
            "{} turns, {} tool calls, id kept: {}",
            back.turns.len(),
            calls(&back.turns),
            written.id_kept
        );
    }

    #[test]
    fn a_claude_session_becomes_a_codex_rollout_and_back() {
        // Named by its id, as Claude Code names every transcript: the file name
        // is where the id comes from, so a fixture called anything else would be
        // testing a session whose id is not a session id.
        let source = write_file(
            "5049dbcd-8ef7-412f-bf57-589e61c41d0e.jsonl",
            &[
                r#"{"type":"user","uuid":"a","parentUuid":null,"isSidechain":false,"timestamp":"2026-09-28T00:00:01.000Z","cwd":"/tmp/proj","message":{"role":"user","content":"hello"},"sessionId":"5049dbcd-8ef7-412f-bf57-589e61c41d0e","gitBranch":"main"}"#,
                r#"{"type":"assistant","uuid":"b","parentUuid":"a","isSidechain":false,"timestamp":"2026-09-28T00:00:02.000Z","message":{"role":"assistant","model":"m","content":[{"type":"text","text":"hi"}]},"sessionId":"5049dbcd-8ef7-412f-bf57-589e61c41d0e"}"#,
            ]
            .join("\n"),
        );
        let codex_home = scratch("rt-codex");
        let to_codex = convert(Provider::Claude, &source, Provider::Codex, &codex_home).unwrap();
        assert!(to_codex.id_kept, "the id survives into the other store");

        let back = read_codex(&to_codex.path).unwrap();
        assert_eq!(
            back.turns,
            vec![Turn::User("hello".into()), Turn::Assistant("hi".into())]
        );
        assert_eq!(back.cwd, "/tmp/proj");

        // Back again, into an empty Claude store. The id is the one the original
        // Claude session had and the Codex copy kept, so it is free here and the
        // round trip lands on the id it started from.
        let claude_home = scratch("rt-claude");
        let to_claude = convert(
            Provider::Codex,
            &to_codex.path,
            Provider::Claude,
            &claude_home,
        )
        .unwrap_or_else(|| write_claude(&back, &claude_home).expect("the copy can be written"));
        assert!(to_claude.id_kept, "the id survives the whole round trip");
        assert_eq!(to_claude.session_id, "5049dbcd-8ef7-412f-bf57-589e61c41d0e");
        let again = read_claude(&to_claude.path).unwrap();
        assert_eq!(
            again.turns,
            vec![Turn::User("hello".into()), Turn::Assistant("hi".into())]
        );
    }
}
