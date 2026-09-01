//! The conversation itself, normalised out of a harness's transcript.
//!
//! Everything else cctop reads a transcript for is a number: tokens, costs,
//! how full the window is, which tool failed how often. None of that keeps the
//! words, and the words are what someone away from their desk actually wants —
//! what they asked for, what the agent said back, what it edited on the way.
//! [`crate::serve::report`] answers "where did the afternoon go"; this answers
//! "what is it *doing*".
//!
//! # Why this is not the extraction path
//!
//! [`SessionData`](crate::session::SessionData) is built for the table: it is
//! cached, it is loaded for every row on the machine, and it deliberately drops
//! message text — keeping it would multiply the cache by the size of every
//! conversation on disk to serve a panel that shows one. So this is a separate
//! read, on one session, on request, on the route that asked for it, and it
//! keeps nothing.
//!
//! # What bounds it
//!
//! A transcript is unbounded and a browser is not, so every axis is capped:
//! [`MAX_TURNS`] from the end, [`MAX_TEXT_CHARS`] per message,
//! [`MAX_RESULT_CHARS`] per tool result, [`MAX_DIFF_LINES`] per patch. The tail
//! rather than the head, because a conversation is read from where it got to.
//! Older turns are counted and reported as a number rather than sent, which is
//! how the page can say "312 earlier turns" instead of implying the session
//! began where the scroll does.
//!
//! # Claude Code and Codex only
//!
//! Those two write JSONL that says, per entry, who spoke and what they said.
//! The rest do not, in different ways and to different degrees: Cursor's native
//! transcripts carry no roles cctop can trust, and OpenCode and Windsurf pack
//! whole workspaces into SQLite with schemas that move between releases. Rather
//! than half-read those into a view that looks authoritative and is not, a
//! session on one of them comes back [`unsupported`](Conversation::supported)
//! with the reason attached, and the page keeps showing the tool log and the
//! diffs, which every provider does have.
//!
//! ponytail: subagent sidechains are skipped rather than nested. Claude writes
//! them interleaved into the same file, and threading them into the transcript
//! they branch from is a display problem this does not solve; the report's
//! subagent section already names them and what they cost.

use crate::pricing::Provider;
use crate::session::{Delta, Session, extract};
use crate::util;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

/// How many of the newest turns are sent.
///
/// A long session runs to thousands, and a page that renders all of them is a
/// page that locks the tab it was opened in. This is several screens of scroll
/// past what anyone reads in one sitting.
const MAX_TURNS: usize = 200;

/// The most text one message contributes.
///
/// Generous, because a pasted stack trace or a plan is exactly the message
/// someone opens this to re-read, and a message cut off at a tweet's length is
/// worse than useless — it looks like the agent said only that.
const MAX_TEXT_CHARS: usize = 6000;

/// The most of one tool result that is kept.
///
/// Shorter than a message on purpose: a result is shown to confirm what came
/// back, not to be read in full. The whole of it is in the transcript, and the
/// report's call log is where the argument that produced it lives.
const MAX_RESULT_CHARS: usize = 800;

/// The most tool calls attributed to one turn.
///
/// A turn issuing more than this is a fan-out, and the tail of it says nothing
/// the first sixty-four did not.
const MAX_TOOLS_PER_TURN: usize = 64;

/// The most diff lines carried for one edit.
const MAX_DIFF_LINES: usize = 200;

/// One session's conversation, as much of it as is sent.
#[derive(Debug, Default, Serialize)]
pub struct Conversation {
    /// Whether this harness has a reader at all. False carries a `note` saying
    /// why, and is not an error: the rest of the report is still true.
    pub supported: bool,
    /// Turns oldest-first, which is the order they are read in.
    pub turns: Vec<Turn>,
    /// Turns the transcript holds that came before the ones sent.
    pub earlier: usize,
    /// Why this is empty or short, when there is a reason worth saying.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One thing that was said, and what it caused.
#[derive(Debug, Serialize)]
pub struct Turn {
    /// `user`, `assistant`, or `system` for the harness speaking for itself.
    pub role: &'static str,
    /// `message` ordinarily; `reasoning` for a thinking summary, `compaction`
    /// for the summary a harness writes when it reclaims the window. The page
    /// styles them differently because they are read differently — a compaction
    /// is a seam in the conversation, not a thing anybody said.
    pub kind: &'static str,
    pub ts: String,
    pub text: String,
    /// Whether `text` was cut to [`MAX_TEXT_CHARS`].
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub clipped: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolUse>,
}

impl Turn {
    fn new(role: &'static str, kind: &'static str, ts: &str) -> Turn {
        Turn {
            role,
            kind,
            ts: ts.to_string(),
            text: String::new(),
            clipped: false,
            tools: Vec::new(),
        }
    }

    fn set_text(&mut self, text: &str) {
        let trimmed = text.trim();
        self.clipped = trimmed.chars().count() > MAX_TEXT_CHARS;
        self.text = match self.clipped {
            true => trimmed.chars().take(MAX_TEXT_CHARS).collect(),
            false => trimmed.to_string(),
        };
    }

    /// Add more text to a turn that already has some.
    ///
    /// A cap that was reached stays reached: a run of entries must not be able
    /// to grow one turn past [`MAX_TEXT_CHARS`] a block at a time.
    fn append_text(&mut self, text: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() || self.clipped {
            return;
        }
        if self.text.is_empty() {
            return self.set_text(trimmed);
        }
        let joined = format!("{}\n\n{trimmed}", self.text);
        self.set_text(&joined);
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.tools.is_empty()
    }
}

/// One tool call, with whatever came back from it.
#[derive(Debug, Default, Serialize)]
pub struct ToolUse {
    /// The name as the transcript spelled it, with an MCP server's prefix made
    /// readable — `mcp__linear__list_issues` is `linear: list issues` on screen
    /// and nowhere else.
    pub name: String,
    /// The one-line form: the path, the command, the pattern.
    pub detail: String,
    /// The argument in full, when it differs from `detail`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full: Option<String>,
    /// The head of what the tool returned, or `None` while it is still running —
    /// which is what makes the last call of a live session visibly pending.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
    #[serde(skip_serializing_if = "is_zero")]
    pub added: u32,
    #[serde(skip_serializing_if = "is_zero")]
    pub removed: u32,
    /// Unified-diff lines, when the harness recorded the patch it applied.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diff: Vec<String>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Read `session`'s conversation, as far as its harness allows.
pub fn build(session: &Session) -> Conversation {
    let Some(path) = session.data_file.as_ref() else {
        return unsupported("this session has no transcript file on this machine");
    };
    let mut sink = Sink::default();
    let read = match session.provider {
        Provider::Claude => extract::for_each_jsonl(path, |item| sink.claude(item)),
        Provider::Codex => extract::for_each_jsonl(path, |item| sink.codex(item)),
        _ => {
            return unsupported(&format!(
                "cctop cannot read a {} conversation yet — the tool calls, \
                 diffs and costs below come from the same transcript and are complete",
                session.surface.label(session.provider)
            ));
        }
    };
    if let Err(e) = read {
        return unsupported(&format!("could not read the transcript: {e}"));
    }
    sink.finish()
}

fn unsupported(why: &str) -> Conversation {
    Conversation {
        supported: false,
        note: Some(why.to_string()),
        ..Conversation::default()
    }
}

/// Turns as they are read, with the oldest dropped once there are too many.
///
/// A tool result arrives in a later entry than the call it belongs to, so the
/// call has to stay reachable by id. Keeping the window as a deque and the index
/// in *sequence* numbers rather than positions is what makes that survive the
/// dropping: an id whose turn has already fallen off the front resolves to
/// nothing and its result is discarded, instead of landing on whichever turn
/// happens to sit at that position now.
#[derive(Default)]
struct Sink {
    turns: VecDeque<Turn>,
    /// Sequence number of the turn at the front of `turns`.
    first: usize,
    /// Sequence number the next turn will get.
    next: usize,
    /// `tool_use` id -> (turn sequence, index within that turn's tools).
    index: HashMap<String, (usize, usize)>,
    /// Codex repeats an entry when a turn is retried; the second copy of a
    /// `call_id` is the same call, not another one.
    seen_calls: std::collections::HashSet<String>,
    /// The assistant turn still being added to, if there is one.
    ///
    /// Both harnesses write one reply as several records — the text, then each
    /// call — so a turn per record makes one answer into four boxes, three of
    /// them holding nothing but a tool name. Merging every consecutive record
    /// instead collapses a whole session into two boxes with sixty calls each.
    /// A tool result is the seam: it means the model has been asked again, and
    /// what it says next is a new turn. This is the fallback rule, used where a
    /// harness gives nothing better.
    run: Option<usize>,
    /// The API request the open turn belongs to, where the transcript says.
    ///
    /// Claude stamps every record of one response with the same `requestId`,
    /// which is the exact answer the rule above approximates: an entry carrying
    /// a request id already seen is part of that reply, however many thinking
    /// blocks and parallel tool calls it was written as.
    run_request: Option<(String, usize)>,
}

impl Sink {
    fn push(&mut self, turn: Turn) -> usize {
        // Anything pushed directly ends the run: a user turn, a compaction, a
        // block of reasoning. Only `open_assistant` reopens one.
        self.run = None;
        let seq = self.next;
        self.next += 1;
        self.turns.push_back(turn);
        while self.turns.len() > MAX_TURNS {
            self.turns.pop_front();
            self.first += 1;
        }
        seq
    }

    /// The assistant turn more of one reply belongs to, opening a new one when
    /// this record starts a different reply.
    ///
    /// `request` is the harness's own name for the response this record came
    /// from, where it has one. With it the grouping is exact; without it, the
    /// run rule in [`Sink::run`] stands in.
    fn open_assistant(&mut self, ts: &str, request: Option<&str>) -> usize {
        let roomy = |sink: &mut Sink, seq: usize| {
            sink.turn_mut(seq)
                .is_some_and(|turn| turn.tools.len() < MAX_TOOLS_PER_TURN)
        };
        if let Some(id) = request {
            if let Some((open, seq)) = self.run_request.clone()
                && open == id
                && roomy(self, seq)
            {
                self.run = Some(seq);
                return seq;
            }
        } else if let Some(seq) = self.run
            && roomy(self, seq)
        {
            return seq;
        }
        let seq = self.push(Turn::new("assistant", "message", ts));
        self.run = Some(seq);
        if let Some(id) = request {
            self.run_request = Some((id.to_string(), seq));
        }
        seq
    }

    fn turn_mut(&mut self, seq: usize) -> Option<&mut Turn> {
        let position = seq.checked_sub(self.first)?;
        self.turns.get_mut(position)
    }

    fn add_tool(&mut self, seq: usize, id: Option<&str>, tool: ToolUse) {
        let Some(turn) = self.turn_mut(seq) else {
            return;
        };
        if turn.tools.len() >= MAX_TOOLS_PER_TURN {
            return;
        }
        let at = turn.tools.len();
        turn.tools.push(tool);
        if let Some(id) = id {
            self.index.insert(id.to_string(), (seq, at));
        }
    }

    /// Attach a result to the call it came back from, if that call is still in
    /// the window.
    fn resolve(&mut self, id: &str, result: Option<String>, failed: bool, delta: Option<Delta>) {
        // Whatever the model says after this is a new reply, whether or not the
        // call it answers is still in the window.
        self.run = None;
        let Some((seq, at)) = self.index.remove(id) else {
            return;
        };
        let Some(tool) = self.turn_mut(seq).and_then(|t| t.tools.get_mut(at)) else {
            return;
        };
        // A result is recorded even when it is empty, because the presence of
        // one is what distinguishes a finished call from a running one.
        tool.result = Some(result.unwrap_or_default());
        tool.failed = failed;
        if let Some(delta) = delta {
            tool.added = delta.added;
            tool.removed = delta.removed;
            tool.diff = delta.hunks.into_iter().take(MAX_DIFF_LINES).collect();
        }
    }

    fn finish(self) -> Conversation {
        Conversation {
            supported: true,
            earlier: self.first,
            turns: self
                .turns
                .into_iter()
                .filter(|turn| !turn.is_empty())
                .collect(),
            note: None,
        }
    }

    // --- Claude Code ---

    fn claude(&mut self, item: &Value) {
        // A subagent's turns are a different conversation that happens to share
        // a file. See the module docs.
        if item.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return;
        }
        let ts = item.get("timestamp").and_then(Value::as_str).unwrap_or("");
        match item.get("type").and_then(Value::as_str) {
            Some("user") => self.claude_user(item, ts),
            Some("assistant") => self.claude_assistant(item, ts),
            _ => {}
        }
    }

    fn claude_user(&mut self, item: &Value, ts: &str) {
        let content = item.get("message").and_then(|m| m.get("content"));
        // The patch an edit applied is recorded on the entry carrying its
        // result, not on the call, so it is read once here and handed to
        // whichever `tool_result` block claims it.
        let mut delta = claude_delta(item);
        let mut text = String::new();
        let mut had_result = false;

        match content {
            Some(Value::String(s)) => text.push_str(s),
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    if let Value::String(s) = block {
                        push_text(&mut text, s);
                        continue;
                    }
                    match block.get("type").and_then(Value::as_str) {
                        Some("tool_result") => {
                            had_result = true;
                            let Some(id) = block.get("tool_use_id").and_then(Value::as_str) else {
                                continue;
                            };
                            let failed =
                                block.get("is_error").and_then(Value::as_bool) == Some(true);
                            let body = flatten_content(block.get("content"));
                            self.resolve(id, Some(body), failed, delta.take());
                        }
                        _ => {
                            if let Some(t) = block.get("text").and_then(Value::as_str) {
                                push_text(&mut text, t);
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        if text.trim().is_empty() {
            return;
        }
        // A tool result and a typed message can share one entry when someone
        // types while a tool is running. The result has already been attached;
        // what is left is a person talking.
        let compaction = item.get("isCompactSummary").and_then(Value::as_bool) == Some(true);
        let (role, kind) = match compaction {
            true => ("system", "compaction"),
            // A `<command-name>` block or a hook's output is the harness
            // speaking through the user's turn, and reads wrong attributed to
            // the person.
            false if is_harness_text(&text) || (had_result && item_is_meta(item)) => {
                ("system", "message")
            }
            false => ("user", "message"),
        };
        // The harness writes for a parser, not for a reader. What it says is
        // worth keeping; the tags around it are not, and a turn that is only
        // tags says nothing at all.
        let text = match role {
            "system" if kind == "message" => tidy_harness_text(&text),
            _ => text,
        };
        if text.trim().is_empty() {
            return;
        }
        let mut turn = Turn::new(role, kind, ts);
        turn.set_text(&text);
        self.push(turn);
    }

    fn claude_assistant(&mut self, item: &Value, ts: &str) {
        let request = item.get("requestId").and_then(Value::as_str);
        let Some(blocks) = item
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        else {
            return;
        };

        let mut text = String::new();
        let mut thinking = String::new();
        let mut calls: Vec<(Option<String>, ToolUse)> = Vec::new();
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = block.get("text").and_then(Value::as_str) {
                        push_text(&mut text, t);
                    }
                }
                Some("thinking") => {
                    if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                        push_text(&mut thinking, t);
                    }
                }
                Some("tool_use") => {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                    let (short, full) = extract::tool_detail(name, &input);
                    calls.push((
                        block.get("id").and_then(Value::as_str).map(str::to_string),
                        ToolUse {
                            name: util::pretty_mcp_name(name),
                            detail: short,
                            full,
                            ..ToolUse::default()
                        },
                    ));
                }
                _ => {}
            }
        }

        // Thinking is its own turn rather than a prefix of the reply: it is
        // shown differently, and folding it into the text would mean either
        // hiding what the agent said or leading with all of its reasoning.
        //
        // Pushing it does not end the reply it belongs to — `run_request` is
        // what reopens that — which matters because a harness with extended
        // thinking writes a thinking block into most records, and closing the
        // reply on each one puts every tool call in a box of its own.
        if !thinking.trim().is_empty() {
            let mut turn = Turn::new("assistant", "reasoning", ts);
            turn.set_text(&thinking);
            self.push(turn);
        }

        if text.trim().is_empty() && calls.is_empty() {
            return;
        }
        // One reply, however many entries it took. Claude writes a turn's text
        // and each of its tool calls as separate records, so a turn shown per
        // record is one answer split across four boxes with three of them
        // holding nothing but a tool name. Everything between two user turns is
        // one thing the agent said, which is how its own interface reads it.
        let seq = self.open_assistant(ts, request);
        if let Some(turn) = self.turn_mut(seq) {
            turn.append_text(&text);
        }
        for (id, tool) in calls {
            self.add_tool(seq, id.as_deref(), tool);
        }
    }

    // --- Codex ---

    fn codex(&mut self, item: &Value) {
        let ts = item.get("timestamp").and_then(Value::as_str).unwrap_or("");
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        let Some(payload) = item.get("payload") else {
            return;
        };
        // A rollout writes the same shapes either at the top level or wrapped
        // in a `response_item`, exactly as the extraction path finds them.
        let effective = match kind {
            "response_item" => payload
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            other => other,
        };

        match effective {
            "message" => self.codex_message(payload, ts),
            "reasoning" => {
                let text = codex_summary(payload);
                if !text.trim().is_empty() {
                    let mut turn = Turn::new("assistant", "reasoning", ts);
                    turn.set_text(&text);
                    self.push(turn);
                }
            }
            "function_call" | "custom_tool_call" => self.codex_call(payload, ts),
            "function_call_output" | "custom_tool_call_output" => {
                let Some(id) = payload.get("call_id").and_then(Value::as_str) else {
                    return;
                };
                let output = payload.get("output");
                let failed = codex_output_failed(output);
                self.resolve(id, Some(flatten_content(output)), failed, None);
            }
            _ => {}
        }
    }

    fn codex_message(&mut self, payload: &Value, ts: &str) {
        let role = match payload.get("role").and_then(Value::as_str) {
            Some("user") => "user",
            Some("assistant") => "assistant",
            // `system` and `developer` are both the harness talking: the
            // instructions, the environment block, the wrapper around a slash
            // command.
            _ => "system",
        };
        let text = codex_text(payload);
        if text.trim().is_empty() {
            return;
        }
        let mut turn = Turn::new(role, "message", ts);
        turn.set_text(&text);
        let seq = self.push(turn);
        // The calls this reply makes are written as their own entries after it,
        // so the reply stays open for them until a result comes back.
        if role == "assistant" {
            self.run = Some(seq);
        }
    }

    fn codex_call(&mut self, payload: &Value, ts: &str) {
        if let Some(id) = payload.get("call_id").and_then(Value::as_str)
            && !self.seen_calls.insert(id.to_string())
        {
            return;
        }
        let name = payload
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        // `arguments` is a JSON-encoded string on a `function_call` and `input`
        // on a `custom_tool_call`, and `apply_patch` sends a raw patch through
        // either — so the argument is parsed if it parses and shown verbatim if
        // it does not, which is what the extraction path does with the same
        // entries.
        let raw_field = payload.get("arguments").or_else(|| payload.get("input"));
        let raw = raw_field.and_then(Value::as_str);
        let args: Value = match raw_field {
            Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
            Some(other) => other.clone(),
            None => Value::Null,
        };

        let mut tool = ToolUse {
            name: util::pretty_mcp_name(name),
            ..ToolUse::default()
        };
        if let Some(patch) = raw.filter(|_| name == "apply_patch" || args.is_null()) {
            match name {
                "apply_patch" => {
                    let (summary, delta) = extract::parse_apply_patch(patch);
                    tool.detail = summary;
                    tool.full = Some(patch.to_string());
                    tool.added = delta.added;
                    tool.removed = delta.removed;
                    tool.diff = delta.hunks.into_iter().take(MAX_DIFF_LINES).collect();
                }
                _ => {
                    tool.detail = extract::flatten_public(patch, 300);
                    tool.full = Some(patch.to_string());
                }
            }
        } else {
            let (short, full) = extract::tool_detail(name, &args);
            tool.detail = short;
            tool.full = full;
        }

        let seq = self.open_assistant(ts, None);
        let id = payload.get("call_id").and_then(Value::as_str);
        self.add_tool(seq, id, tool);
    }
}

/// The diff a Claude edit reported, from the entry carrying its result.
fn claude_delta(item: &Value) -> Option<Delta> {
    let hunks = item
        .get("toolUseResult")
        .and_then(|r| r.get("structuredPatch"))
        .and_then(Value::as_array)?;
    let mut delta = Delta::default();
    for hunk in hunks {
        let Some(lines) = hunk.get("lines").and_then(Value::as_array) else {
            continue;
        };
        for line in lines.iter().filter_map(Value::as_str) {
            if line.starts_with('+') {
                delta.added += 1;
            } else if line.starts_with('-') {
                delta.removed += 1;
            }
            if delta.hunks.len() < MAX_DIFF_LINES {
                delta.hunks.push(line.to_string());
            }
        }
    }
    (delta.added > 0 || delta.removed > 0).then_some(delta)
}

/// Text out of a Codex message payload's content blocks.
fn codex_text(payload: &Value) -> String {
    let mut out = String::new();
    match payload.get("content") {
        Some(Value::String(s)) => out.push_str(s),
        Some(Value::Array(blocks)) => {
            for block in blocks {
                match block {
                    Value::String(s) => push_text(&mut out, s),
                    _ => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            push_text(&mut out, t);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// The reasoning summary Codex records, which is a list of its own blocks.
fn codex_summary(payload: &Value) -> String {
    let mut out = String::new();
    for block in payload
        .get("summary")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(t) = block.get("text").and_then(Value::as_str) {
            push_text(&mut out, t);
        }
    }
    out
}

/// Whether a Codex tool output says the call failed.
///
/// The field is not always there and not always a bool: a shell call reports an
/// exit status inside its output text instead, so both are checked and neither
/// is required.
fn codex_output_failed(output: Option<&Value>) -> bool {
    let Some(output) = output else {
        return false;
    };
    if output.get("success").and_then(Value::as_bool) == Some(false) {
        return true;
    }
    let text = match output {
        Value::String(s) => s.clone(),
        other => other
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    };
    let head: String = text.chars().take(400).collect();
    head.contains("exit code 1")
        || head.contains("Error:")
        || head.contains("command not found")
        || head.contains("No such file or directory")
}

/// A tool result's content, whatever shape it arrived in, cut to size.
fn flatten_content(content: Option<&Value>) -> String {
    let mut out = String::new();
    match content {
        Some(Value::String(s)) => out.push_str(s),
        Some(Value::Array(blocks)) => {
            for block in blocks {
                match block {
                    Value::String(s) => push_text(&mut out, s),
                    _ => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            push_text(&mut out, t);
                        } else if block.get("type").and_then(Value::as_str) == Some("image") {
                            // The bytes are megabytes of base64 and the page has
                            // nothing to do with them, but a result that was an
                            // image should not read as an empty one.
                            push_text(&mut out, "[image]");
                        }
                    }
                }
            }
        }
        Some(Value::Object(map)) => {
            // Codex's `output` is an object with the text under one of a few
            // keys depending on the tool.
            for key in ["content", "output", "stdout", "text"] {
                if let Some(t) = map.get(key).and_then(Value::as_str) {
                    push_text(&mut out, t);
                }
            }
            if out.is_empty() {
                out = content.map(|c| c.to_string()).unwrap_or_default();
            }
        }
        Some(other) => out = other.to_string(),
        None => {}
    }
    let trimmed = out.trim();
    match trimmed.chars().count() > MAX_RESULT_CHARS {
        true => trimmed.chars().take(MAX_RESULT_CHARS).collect::<String>() + "…",
        false => trimmed.to_string(),
    }
}

/// Append with a blank line between blocks, so two text blocks do not run into
/// one word.
fn push_text(out: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(text);
}

/// Whether a user turn is really the harness talking.
///
/// Claude Code writes several of its own things into `user` entries — the
/// expansion of a slash command, a hook's output, the reminder blocks it
/// injects — and showing those as something a person typed is the difference
/// between a transcript someone recognises and one they do not.
fn is_harness_text(text: &str) -> bool {
    let head = text.trim_start();
    head.starts_with("<command-name>")
        || head.starts_with("<local-command")
        || head.starts_with("<system-reminder>")
        || head.starts_with("Caveat:")
        || head.starts_with("<user-prompt-submit-hook>")
}

/// The text inside `<tag>…</tag>`, the first time it appears.
fn tagged<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let rest = &text[text.find(&open)? + open.len()..];
    Some(&rest[..rest.find(&format!("</{tag}>"))?])
}

/// A harness turn as something to read.
///
/// Claude Code records a slash command as the three tags it parsed it into —
/// `<command-name>`, `<command-message>`, `<command-args>` — and the output as
/// a fourth. Shown raw, a `/clear` fills four lines with markup and buries the
/// one token that matters. So it comes back out as the command someone typed,
/// with whatever it printed beneath it.
///
/// Anything else the harness writes keeps its text and loses its wrapper: a
/// reminder still reads as a reminder without the tag announcing it as one.
fn tidy_harness_text(text: &str) -> String {
    let out = tagged(text, "local-command-stdout").unwrap_or("").trim();
    if let Some(name) = tagged(text, "command-name") {
        let args = tagged(text, "command-args").unwrap_or("").trim();
        let said = format!("{} {args}", name.trim());
        return match out.is_empty() {
            true => said.trim().to_string(),
            false => format!("{}\n\n{out}", said.trim()),
        };
    }
    if text.trim_start().starts_with("<local-command") {
        return out.to_string();
    }
    // Every other block is a wrapper around prose. Dropping the tag lines is
    // enough — the text between them was written to be read.
    let stripped: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !(line.trim_start().starts_with('<') && line.trim_end().ends_with('>')))
        .collect();
    let stripped = stripped.join("\n");
    let stripped = stripped.trim();
    match stripped.is_empty() {
        // A block written entirely on one line has no line to keep, so the
        // tags come off it directly rather than leaving the turn empty.
        true => strip_outer_tags(text.trim()).trim().to_string(),
        false => stripped.to_string(),
    }
}

/// `<tag>body</tag>` on a single line, reduced to `body`.
fn strip_outer_tags(text: &str) -> &str {
    let Some(open_end) = text.find('>') else {
        return text;
    };
    if !text.starts_with('<') || !text.ends_with('>') {
        return text;
    }
    let body = &text[open_end + 1..];
    match body.rfind("</") {
        Some(close) => &body[..close],
        None => body,
    }
}

fn item_is_meta(item: &Value) -> bool {
    item.get("isMeta").and_then(Value::as_bool) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink_claude(lines: &[&str]) -> Conversation {
        let mut sink = Sink::default();
        for line in lines {
            sink.claude(&serde_json::from_str(line).unwrap());
        }
        sink.finish()
    }

    fn sink_codex(lines: &[&str]) -> Conversation {
        let mut sink = Sink::default();
        for line in lines {
            sink.codex(&serde_json::from_str(line).unwrap());
        }
        sink.finish()
    }

    #[test]
    fn a_claude_exchange_becomes_a_user_turn_and_an_assistant_turn() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"role":"user","content":"fix the parser"}}"#,
            r#"{"type":"assistant","timestamp":"t2","message":{"role":"assistant","content":[{"type":"text","text":"on it"}]}}"#,
        ]);
        assert!(chat.supported);
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[0].text, "fix the parser");
        assert_eq!(chat.turns[1].role, "assistant");
        assert_eq!(chat.turns[1].text, "on it");
    }

    /// The call and its result are two entries a long way apart in the file, and
    /// the whole point of the id index is that they come back as one thing.
    #[test]
    fn a_tool_result_lands_on_the_call_it_answers() {
        let chat = sink_claude(&[
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a/b.rs"}}]}}"#,
            r#"{"type":"assistant","timestamp":"t2","message":{"content":[{"type":"text","text":"meanwhile"}]}}"#,
            r#"{"type":"user","timestamp":"t3","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"fn main() {}"}]}}"#,
        ]);
        let tool = &chat.turns[0].tools[0];
        assert_eq!(tool.name, "Read");
        assert_eq!(tool.detail, "/a/b.rs");
        assert_eq!(tool.result.as_deref(), Some("fn main() {}"));
        assert!(!tool.failed);
        // The result entry carried no text of its own, so it is not a turn —
        // and the two assistant entries are one reply, since nothing came back
        // in between.
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "meanwhile");
    }

    /// Claude writes the text of a reply and each of its tool calls as separate
    /// records. Rendered one box per record, a single answer becomes four, three
    /// of them empty but for a tool name.
    #[test]
    fn a_run_of_assistant_entries_is_one_reply() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t0","message":{"content":"go"}}"#,
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"text","text":"first"}]}}"#,
            r#"{"type":"assistant","timestamp":"t2","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a"}}]}}"#,
            r#"{"type":"assistant","timestamp":"t3","message":{"content":[{"type":"text","text":"second"}]}}"#,
            r#"{"type":"user","timestamp":"t4","message":{"content":"again"}}"#,
            r#"{"type":"assistant","timestamp":"t5","message":{"content":[{"type":"text","text":"a new reply"}]}}"#,
        ]);
        let roles: Vec<&str> = chat.turns.iter().map(|t| t.role).collect();
        assert_eq!(roles, vec!["user", "assistant", "user", "assistant"]);
        assert_eq!(chat.turns[1].text, "first\n\nsecond");
        assert_eq!(chat.turns[1].tools.len(), 1);
        // A user turn between them ends the run, so the next reply is its own.
        assert_eq!(chat.turns[3].text, "a new reply");
    }

    #[test]
    fn a_failed_result_is_marked_as_one() {
        let chat = sink_claude(&[
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
            r#"{"type":"user","timestamp":"t2","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":"no such command"}]}}"#,
        ]);
        assert!(chat.turns[0].tools[0].failed);
    }

    /// A call with no result yet is what a live session looks like, and the page
    /// shows it as running — so the absence has to survive to the JSON.
    #[test]
    fn a_call_still_running_has_no_result() {
        let chat = sink_claude(&[
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"sleep 60"}}]}}"#,
        ]);
        assert!(chat.turns[0].tools[0].result.is_none());
    }

    #[test]
    fn an_edits_patch_is_carried_with_its_tool_call() {
        let chat = sink_claude(&[
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Edit","input":{"file_path":"/a/b.rs"}}]}}"#,
            r#"{"type":"user","timestamp":"t2","toolUseResult":{"structuredPatch":[{"lines":["-old","+new","+also"]}]},"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}]}}"#,
        ]);
        let tool = &chat.turns[0].tools[0];
        assert_eq!((tool.added, tool.removed), (2, 1));
        assert_eq!(tool.diff, vec!["-old", "+new", "+also"]);
    }

    /// A subagent writes into the same file, and its turns are another
    /// conversation. Including them interleaves two agents into one thread.
    #[test]
    fn sidechain_turns_are_left_out() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"the real ask"}}"#,
            r#"{"type":"user","timestamp":"t2","isSidechain":true,"message":{"content":"a subagent's brief"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "the real ask");
    }

    #[test]
    fn a_compaction_summary_is_a_seam_not_a_message() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","isCompactSummary":true,"message":{"content":"everything so far"}}"#,
        ]);
        assert_eq!(chat.turns[0].kind, "compaction");
        assert_eq!(chat.turns[0].role, "system");
    }

    #[test]
    fn a_slash_command_expansion_is_attributed_to_the_harness() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<command-name>/clear</command-name>"}}"#,
        ]);
        assert_eq!(chat.turns[0].role, "system");
    }

    /// What the harness records for one `/loop 5m` is four tags. What it did
    /// is one line, and that is what the page has room for.
    #[test]
    fn a_slash_command_reads_as_the_command_that_was_typed() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<command-name>/loop</command-name>\n<command-message>loop</command-message>\n<command-args>5m</command-args>\n<local-command-stdout>started</local-command-stdout>"}}"#,
        ]);
        assert_eq!(chat.turns[0].text, "/loop 5m\n\nstarted");
    }

    /// `/clear` prints nothing, so its turn is the command alone rather than
    /// the command and an empty line where the output would have been.
    #[test]
    fn a_command_that_printed_nothing_is_just_the_command() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<command-name>/clear</command-name>\n<local-command-stdout></local-command-stdout>"}}"#,
        ]);
        assert_eq!(chat.turns[0].text, "/clear");
    }

    /// A reminder is prose in a wrapper. The prose survives; the wrapper does
    /// not, and neither does a turn that was nothing but wrapper.
    #[test]
    fn a_reminder_keeps_its_words_and_loses_its_tags() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<system-reminder>\nthe file changed on disk\n</system-reminder>"}}"#,
            r#"{"type":"user","timestamp":"t2","message":{"content":"<local-command-stdout></local-command-stdout>"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "the file changed on disk");
    }

    /// The same block written on one line has no line the filter can keep, and
    /// dropping it would lose the only thing it said.
    #[test]
    fn a_one_line_reminder_survives_the_same_way() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<system-reminder>read the file first</system-reminder>"}}"#,
        ]);
        assert_eq!(chat.turns[0].text, "read the file first");
    }

    /// Everything after the cap is dropped from the *front*, because a
    /// conversation is read from where it got to — and the count of what was
    /// dropped is what stops the page implying the session started there.
    #[test]
    fn only_the_newest_turns_survive_and_the_rest_are_counted() {
        let lines: Vec<String> = (0..MAX_TURNS + 10)
            .map(|i| {
                format!(
                    r#"{{"type":"user","timestamp":"t","message":{{"content":"message {i}"}}}}"#
                )
            })
            .collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let chat = sink_claude(&refs);
        assert_eq!(chat.turns.len(), MAX_TURNS);
        assert_eq!(chat.earlier, 10);
        assert_eq!(chat.turns[0].text, "message 10");
    }

    /// A result whose call has already fallen off the front must not land on
    /// whichever turn now occupies that slot. This is the bug the sequence
    /// numbering exists to prevent.
    #[test]
    fn a_result_for_a_dropped_call_is_discarded() {
        let mut lines = vec![
            r#"{"type":"assistant","timestamp":"t0","message":{"content":[{"type":"tool_use","id":"toolu_old","name":"Read","input":{"file_path":"/gone.rs"}}]}}"#.to_string(),
        ];
        for i in 0..MAX_TURNS + 5 {
            lines.push(format!(
                r#"{{"type":"user","timestamp":"t","message":{{"content":"filler {i}"}}}}"#
            ));
        }
        lines.push(
            r#"{"type":"user","timestamp":"tz","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_old","content":"late"}]}}"#
                .to_string(),
        );
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let chat = sink_claude(&refs);
        assert!(
            chat.turns
                .iter()
                .all(|t| t.tools.iter().all(|tool| tool.result.is_none())),
            "a result was attached to a turn that did not make the call"
        );
    }

    #[test]
    fn text_past_the_cap_is_cut_and_says_so() {
        let long = "x".repeat(MAX_TEXT_CHARS + 100);
        let chat = sink_claude(&[&format!(
            r#"{{"type":"user","timestamp":"t","message":{{"content":"{long}"}}}}"#
        )]);
        assert!(chat.turns[0].clipped);
        assert_eq!(chat.turns[0].text.chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn thinking_is_its_own_turn_ahead_of_the_reply() {
        let chat = sink_claude(&[
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"thinking","thinking":"weighing it up"},{"type":"text","text":"here goes"}]}}"#,
        ]);
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].kind, "reasoning");
        assert_eq!(chat.turns[1].text, "here goes");
    }

    #[test]
    fn a_codex_exchange_reads_the_same_way() {
        let chat = sink_codex(&[
            r#"{"type":"response_item","timestamp":"t1","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"ship it"}]}}"#,
            r#"{"type":"response_item","timestamp":"t2","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"shipping"}]}}"#,
        ]);
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[1].text, "shipping");
    }

    /// Codex writes a call as its own entry with nothing linking it to the
    /// message that issued it, so it has to attach to the turn in progress.
    #[test]
    fn a_codex_call_attaches_to_the_assistant_turn_in_progress() {
        let chat = sink_codex(&[
            r#"{"type":"response_item","timestamp":"t1","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"looking"}]}}"#,
            r#"{"type":"response_item","timestamp":"t2","payload":{"type":"function_call","call_id":"c1","name":"shell","arguments":"{\"command\":\"ls\"}"}}"#,
            r#"{"type":"response_item","timestamp":"t3","payload":{"type":"function_call_output","call_id":"c1","output":"a.rs\nb.rs"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].tools.len(), 1);
        assert_eq!(chat.turns[0].tools[0].result.as_deref(), Some("a.rs\nb.rs"));
    }

    #[test]
    fn a_repeated_codex_call_id_is_one_call() {
        let entry = r#"{"type":"response_item","timestamp":"t","payload":{"type":"function_call","call_id":"c1","name":"shell","arguments":"{\"command\":\"ls\"}"}}"#;
        let chat = sink_codex(&[entry, entry]);
        assert_eq!(chat.turns[0].tools.len(), 1);
    }

    #[test]
    fn a_codex_apply_patch_carries_its_diff() {
        let chat = sink_codex(&[
            r#"{"type":"response_item","timestamp":"t","payload":{"type":"custom_tool_call","call_id":"c1","name":"apply_patch","input":"*** Begin Patch\n*** Update File: src/a.rs\n-old\n+new\n*** End Patch"}}"#,
        ]);
        let tool = &chat.turns[0].tools[0];
        assert!(tool.added >= 1 && tool.removed >= 1, "{tool:?}");
        assert!(!tool.diff.is_empty());
    }

    /// The other half of the merge: everything between two user turns is *not*
    /// one reply, because a tool result means the model was asked again. Without
    /// this bound an afternoon's session comes back as two boxes holding sixty
    /// tool calls each — which is what the first cut of this did.
    /// A reply with extended thinking writes a thinking block into every record
    /// of itself, and its parallel tool calls arrive as separate records too.
    /// The request id is what says they are all one answer.
    #[test]
    fn records_sharing_a_request_id_are_one_reply() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t0","message":{"content":"go"}}"#,
            r#"{"type":"assistant","timestamp":"t1","requestId":"req_1","message":{"content":[{"type":"thinking","thinking":"weighing"},{"type":"text","text":"two at once"}]}}"#,
            r#"{"type":"assistant","timestamp":"t2","requestId":"req_1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a"}}]}}"#,
            r#"{"type":"assistant","timestamp":"t3","requestId":"req_1","message":{"content":[{"type":"tool_use","id":"toolu_2","name":"Read","input":{"file_path":"/b"}}]}}"#,
        ]);
        let kinds: Vec<&str> = chat.turns.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec!["message", "reasoning", "message"]);
        assert_eq!(chat.turns[2].text, "two at once");
        assert_eq!(chat.turns[2].tools.len(), 2, "{:?}", chat.turns[2]);
    }

    #[test]
    fn a_tool_result_ends_the_reply_it_came_back_to() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t0","message":{"content":"go"}}"#,
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"text","text":"looking"},{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a"}}]}}"#,
            r#"{"type":"user","timestamp":"t2","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"contents"}]}}"#,
            r#"{"type":"assistant","timestamp":"t3","message":{"content":[{"type":"text","text":"now I know"}]}}"#,
        ]);
        let roles: Vec<&str> = chat.turns.iter().map(|t| t.role).collect();
        assert_eq!(roles, vec!["user", "assistant", "assistant"]);
        assert_eq!(chat.turns[1].text, "looking");
        assert_eq!(chat.turns[1].tools.len(), 1);
        assert_eq!(chat.turns[2].text, "now I know");
        assert!(chat.turns[2].tools.is_empty());
    }

    #[test]
    fn a_provider_with_no_reader_says_so_instead_of_looking_empty() {
        let mut session = Session::new(Provider::Windsurf, "s1".into());
        session.data_file = Some(std::path::PathBuf::from("/nonexistent"));
        let chat = build(&session);
        assert!(!chat.supported);
        assert!(chat.note.is_some_and(|n| n.contains("Windsurf")));
    }
}
