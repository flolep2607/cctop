//! The conversation itself, normalised out of a harness's transcript.
//!
//! Everything else cctop reads a transcript for is a number: tokens, costs,
//! how full the window is, which tool failed how often. None of that keeps the
//! words, and the words are what someone away from their desk actually wants —
//! what they asked for, what the agent said back, what it edited on the way.
//! [`crate::report`] answers "where did the afternoon go"; this answers
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
//! # What each harness writes
//!
//! The JSONL harnesses say, per entry, who spoke and what they said, so one
//! reader walks the line stream and matches each result to the call before it.
//! Devin writes one ATIF document instead, and OpenCode writes two tables in a
//! database its own sessions share — where a message, its reasoning, its words
//! and its calls including their results are all part of the same row, so those
//! readers take a whole entry at a time and have nothing to pair up.
//!
//! # What a harness leaves out
//!
//! A transcript is not obliged to record everything, and the gaps are read as
//! gaps rather than filled in. Cursor dates nothing and pairs nothing, so its
//! turns carry no time and its calls carry an empty result — a call that
//! finished, about an outcome the file never kept, which is a different
//! statement from one still running. Gemini and Pi keep results keyed to the
//! call, so there an absent result really is a call in flight.
//!
//! Windsurf is the one harness with no reader. It packs a whole workspace's
//! conversations into one SQLite value rewritten wholesale on every write, and
//! nothing in that value is dated, so a conversation cannot even be placed in
//! time. Rather than half-read it into a view that looks authoritative and is
//! not, a Windsurf session comes back [`unsupported`](Conversation::supported)
//! with the reason attached, and the page keeps showing the tool log and the
//! diffs, which every provider does have.
//!
//! A subagent's own turns are not part of the main conversation. Claude Code
//! writes each to its own file, `<session>/subagents/agent-<id>.jsonl`, and
//! [`build_agent`] reads one on request; the main read only ties each `Agent`
//! call, hand-back and task notification to the subagent it is about (see
//! [`AgentCall`]), so a reader can file the agent's work under the call that
//! started it instead of interleaving every agent's traffic as one channel.
//!
//! ponytail: older transcripts wrote subagent records into the main file
//! marked `isSidechain`. Those are still skipped rather than nested — threading
//! them into the transcript they branch from is a display problem this does
//! not solve, and the report's subagent section names them and what they cost.
//!
//! ponytail: Pi's transcript is a tree of `id`/`parentId` entries, so the
//! reader walks the file in order rather than resolving the tree to the leaf
//! the entries describe. An abandoned branch is therefore shown beside the one
//! that was kept. Resolving the tree first means holding every entry id in hand
//! before saying anything, which for a session file large enough to matter is
//! the whole cost of opening the page.

use crate::pricing::Provider;
use crate::session::{Delta, Session, devin, extract, gemini, opencode, pi};
use crate::util;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::path::Path;

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
///
/// `Deserialize` because the same document is the wire format between two
/// cctops: a remote row's conversation is this, read off an ssh pipe rather
/// than off a transcript — see [`crate::fleet`].
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Conversation {
    /// Whether this harness has a reader at all. False carries a `note` saying
    /// why, and is not an error: the rest of the report is still true.
    pub supported: bool,
    /// Turns oldest-first, which is the order they are read in.
    pub turns: Vec<Turn>,
    /// Turns the transcript holds that came before the ones sent.
    pub earlier: usize,
    /// Why this is empty or short, when there is a reason worth saying.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
    /// What the transcript looked like when this was read — see [`stamp_of`].
    /// A page sends it back as `?since=` so that a poll of an unchanged
    /// session costs neither side a read.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub stamp: Option<String>,
    /// The page's `since` still holds: nothing was read and nothing is sent.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub unchanged: bool,
}

/// What a polling page already holds, so the answer can be only what it lacks.
///
/// A page following a running session asked for the whole tail every five
/// seconds, and the server re-read the whole transcript to answer — on a long
/// session hundreds of kilobytes a poll to a page that then rebuilt every turn
/// it showed, to learn that one tool result had landed. With this the page says
/// what it has: the stamp of its last read, and the newest turn it holds.
#[derive(Debug, Default, Clone)]
pub struct Since {
    pub stamp: Option<String>,
    /// Send turns from this sequence number on. Inclusive, because the newest
    /// turn a page holds is the one most likely to have grown since: a tool
    /// result lands on the call made in it.
    pub after: Option<usize>,
}

/// A transcript's size and modification time, as a token a page can hold.
///
/// Transcripts are appended to, so a file whose size and time are both
/// unchanged has nothing new in it. ponytail: OpenCode keeps every session in
/// one database, so its stamp moves when *any* OpenCode session writes — a
/// poll that reads for nothing, never one that misses a change.
pub fn stamp_of(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!("{}-{modified}", meta.len()))
}

impl Conversation {
    /// The answer for a page holding `since`: nothing when its stamp still
    /// holds, and otherwise only the turns from `after` on.
    pub fn narrowed(mut self, since: &Since) -> Conversation {
        if since.stamp.is_some() && since.stamp == self.stamp {
            return Conversation {
                supported: self.supported,
                earlier: self.earlier,
                stamp: self.stamp,
                unchanged: true,
                ..Conversation::default()
            };
        }
        if let Some(after) = since.after {
            self.turns.retain(|t| t.seq >= after);
        }
        self
    }
}

/// One thing that was said, and what it caused.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    /// The turn's place in the whole transcript, oldest counting from zero.
    ///
    /// Sent because the page needs a name for a turn that survives windowing:
    /// `?before=` paging and `#chat/turn-N` links both speak in sequence
    /// numbers, and a position in the returned window is neither — empty
    /// turns are filtered out of `turns`, so position and sequence part ways.
    pub seq: usize,
    /// `user`, `assistant`, or `system` for the harness speaking for itself.
    ///
    /// A `Cow` rather than `&'static str` because a turn read back over ssh is
    /// owned; every writer below still passes a literal, so nothing here
    /// allocates.
    pub role: Cow<'static, str>,
    /// `message` ordinarily; `reasoning` for a thinking summary, `compaction`
    /// for the summary a harness writes when it reclaims the window, and
    /// [`AGENT_MESSAGE`] for what another agent sent this one. The page styles
    /// them differently because they are read differently — a compaction is a
    /// seam in the conversation, not a thing anybody said.
    pub kind: Cow<'static, str>,
    /// Who sent an [`AGENT_MESSAGE`], when the transcript names them — a
    /// subagent's type such as `general-purpose`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub from: Option<String>,
    /// The sending agent's id on an [`AGENT_MESSAGE`] (`origin.from`), which is
    /// what ties a hand-back to the subagent and the call that launched it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent: Option<String>,
    pub ts: String,
    pub text: String,
    /// Whether `text` was cut to [`MAX_TEXT_CHARS`].
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub clipped: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tools: Vec<ToolUse>,
}

impl Turn {
    fn new(role: &'static str, kind: &'static str, ts: &str) -> Turn {
        Turn {
            // Numbered by `push`, the only place that knows the count.
            seq: 0,
            role: Cow::Borrowed(role),
            kind: Cow::Borrowed(kind),
            ts: ts.to_string(),
            text: String::new(),
            clipped: false,
            tools: Vec::new(),
            from: None,
            agent: None,
        }
    }

    /// Set the turn's text, cut to `cap` characters — [`MAX_TEXT_CHARS`] for the
    /// page, unbounded for an export (see [`Limits`]).
    fn set_text(&mut self, text: &str, cap: usize) {
        let trimmed = text.trim();
        self.clipped = trimmed.chars().count() > cap;
        self.text = match self.clipped {
            true => trimmed.chars().take(cap).collect(),
            false => trimmed.to_string(),
        };
    }

    /// Add more text to a turn that already has some.
    ///
    /// A cap that was reached stays reached: a run of entries must not be able
    /// to grow one turn past its cap a block at a time.
    fn append_text(&mut self, text: &str, cap: usize) {
        let trimmed = text.trim();
        if trimmed.is_empty() || self.clipped {
            return;
        }
        if self.text.is_empty() {
            return self.set_text(trimmed, cap);
        }
        let joined = format!("{}\n\n{trimmed}", self.text);
        self.set_text(&joined, cap);
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.tools.is_empty()
    }
}

/// One tool call, with whatever came back from it.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    /// The name as the transcript spelled it, with an MCP server's prefix made
    /// readable — `mcp__linear__list_issues` is `linear: list issues` on screen
    /// and nowhere else.
    pub name: String,
    /// The one-line form: the path, the command, the pattern.
    pub detail: String,
    /// The argument in full, when it differs from `detail`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub full: Option<String>,
    /// The head of what the tool returned, or `None` while it is still running —
    /// which is what makes the last call of a live session visibly pending.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub failed: bool,
    #[serde(skip_serializing_if = "is_zero", default)]
    pub added: u32,
    #[serde(skip_serializing_if = "is_zero", default)]
    pub removed: u32,
    /// Unified-diff lines, when the harness recorded the patch it applied.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub diff: Vec<String>,
    /// The harness's id for the call (Claude's `tool_use` id), which is what a
    /// subagent's sidecar names to say which call started it.
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub id: String,
    /// For a call that started a subagent, the subagent it started.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub agent: Option<AgentCall>,
}

/// The subagent an `Agent` call started, as the session knows it.
///
/// Joined onto the call when the conversation is read, so a remote session's
/// answer carries it too — the cctop that reads the transcript is the one that
/// has the subagent list. Its point is to let a reader file each subagent's
/// work under the call that started it, rather than read every agent's traffic
/// as one channel.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AgentCall {
    /// What [`build_agent`] takes: the subagent's transcript stem, or the call
    /// id for one whose transcript is gone.
    pub id: String,
    #[serde(rename = "type")]
    pub agent_type: String,
    pub description: String,
    /// `running`, `done`, or `failed` when the call itself failed.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub last_active: Option<String>,
    #[serde(skip_serializing_if = "is_zero_i64", default)]
    pub duration_ms: i64,
    #[serde(skip_serializing_if = "is_zero_u64", default)]
    pub tool_count: u64,
    /// Replies in the agent's own transcript.
    #[serde(skip_serializing_if = "is_zero_u64", default)]
    pub turns: u64,
    /// What the agent's own requests cost, in dollars at list price. A reader
    /// on a plan that bundles the provider shows it as included instead, the
    /// way the report's subagent table does: the plan is the reader's to know,
    /// not the transcript's.
    #[serde(skip_serializing_if = "is_zero_f64", default)]
    pub cost: f64,
    /// Every token billed to the agent.
    #[serde(skip_serializing_if = "is_zero_u64", default)]
    pub tokens: u64,
    /// The transcript was purged; only what the parent recorded survives.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub ghost: bool,
    /// Launched in the background: the call's result is only the launch
    /// receipt, and the report arrives later as a hand-back.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub background: bool,
    /// The sequence number of the hand-back turn, when it is in this read.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub handback: Option<usize>,
    /// What the agent reported: the hand-back's text, or a foreground call's
    /// result — or, when `last_message` is set, what it last said.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub report: Option<String>,
    /// `report` is not a report: a background agent that never handed back —
    /// stopped, crashed, or still working — and this is its last message, which
    /// a reader must not present as the agent's conclusion.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub last_message: bool,
}

/// How a reader labels a report that is only an agent's last message.
pub const LAST_MESSAGE_LABEL: &str = "No hand-back — last message from the agent";

impl AgentCall {
    /// What the session's subagent list knows of an agent, before the
    /// conversation says how it was launched and what it reported.
    pub fn from_subagent(sa: &crate::session::Subagent) -> AgentCall {
        AgentCall {
            id: sa.agent_id.clone(),
            agent_type: sa.agent_type.clone(),
            description: sa.description.clone(),
            status: match sa.status {
                crate::session::SubagentStatus::Running => "running",
                crate::session::SubagentStatus::Done => "done",
            }
            .to_string(),
            started_at: sa.started_at.clone(),
            last_active: sa.last_active.clone(),
            duration_ms: sa.duration_ms,
            tool_count: sa.tool_count,
            turns: sa.turns,
            cost: sa.cost,
            tokens: sa.tokens,
            ghost: sa.ghost,
            ..AgentCall::default()
        }
    }

    /// `Explore — map the parser`: which agent, doing what. What names the
    /// agent anywhere it is mentioned — its call, its hand-back.
    pub fn title(&self) -> String {
        match self.description.trim() {
            "" => self.agent_type.clone(),
            what => format!("{} — {what}", self.agent_type),
        }
    }

    /// `12 turns · 3 tools · 2m10s`, the size of the work, with whatever is
    /// known.
    pub fn size(&self) -> String {
        let mut parts = Vec::new();
        if self.turns > 0 {
            let s = if self.turns == 1 { "" } else { "s" };
            parts.push(format!("{} turn{s}", self.turns));
        }
        if self.tool_count > 0 {
            let s = if self.tool_count == 1 { "" } else { "s" };
            parts.push(format!("{} tool{s}", self.tool_count));
        }
        if self.duration_ms > 0 {
            parts.push(util::compact_duration(self.duration_ms));
        }
        parts.join(" · ")
    }
}

fn is_zero_i64(n: &i64) -> bool {
    *n == 0
}

fn is_zero_u64(n: &u64) -> bool {
    *n == 0
}

fn is_zero_f64(n: &f64) -> bool {
    *n == 0.0
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// How much of a transcript one read keeps.
///
/// Two readers want different answers from the same parse. The page wants a
/// window it can render without locking the tab, so it gets the tail and every
/// message cut to size. An export is the whole conversation handed to someone
/// — another agent, an issue, a doc — and a transcript that silently starts at
/// turn 312, or stops a pasted plan mid-sentence, is a wrong one. Tool results
/// stay at [`MAX_RESULT_CHARS`] in both: an export that includes them wants
/// the head of each, not the megabytes.
#[derive(Debug, Clone, Copy)]
struct Limits {
    turns: usize,
    text: usize,
    tools: usize,
}

impl Limits {
    const PAGE: Limits = Limits {
        turns: MAX_TURNS,
        text: MAX_TEXT_CHARS,
        tools: MAX_TOOLS_PER_TURN,
    };
    const WHOLE: Limits = Limits {
        turns: usize::MAX,
        text: usize::MAX,
        tools: usize::MAX,
    };
}

impl Default for Limits {
    fn default() -> Limits {
        Limits::PAGE
    }
}

/// Read `session`'s conversation, as far as its harness allows.
///
/// `before` pages backwards through the turns: when it is `Some(seq)`, the
/// conversation returned ends just before that turn's sequence number instead
/// of at the latest one — same [`MAX_TURNS`] window, reached from the other
/// side. The parse still reads the whole transcript either way, because a tool
/// result near the end of the file can belong to a call inside the window.
pub fn build(session: &Session, before: Option<usize>) -> Conversation {
    read(session, before, Limits::PAGE)
}

/// Read `session`'s whole conversation: every turn, every message in full.
///
/// What [`crate::export`] renders. One session, on request, so holding
/// all of it is the cost of the answer rather than of a page that polls.
pub fn whole(session: &Session) -> Conversation {
    read(session, None, Limits::WHOLE)
}

fn read(session: &Session, before: Option<usize>, limits: Limits) -> Conversation {
    let mut conversation = read_main(session, before, limits);
    if session.provider == Provider::Claude {
        join_agents(&mut conversation, &session.subagents);
    }
    conversation
}

fn read_main(session: &Session, before: Option<usize>, limits: Limits) -> Conversation {
    let Some(path) = session.data_file.as_ref() else {
        return unsupported("this session has no transcript file on this machine");
    };
    let mut sink = Sink {
        before,
        limits,
        ..Sink::default()
    };
    let read = match session.provider {
        Provider::Claude => extract::for_each_jsonl(path, |item| sink.claude(item)),
        Provider::Codex => extract::for_each_jsonl(path, |item| sink.codex(item)),
        Provider::Devin => read_devin(path, &mut sink),
        Provider::OpenCode => read_opencode(path, &session.session_id, &mut sink),
        Provider::Cursor => extract::for_each_jsonl(path, |item| sink.cursor(item)),
        Provider::Gemini => gemini::for_each_record(path, |item| sink.gemini(item)),
        Provider::Pi => extract::for_each_jsonl(path, |item| sink.pi(item)),
        _ => {
            return unsupported(&format!(
                "cctop cannot read {} {} conversation yet — it keeps the whole \
                 workspace's conversations in one settings value, rewritten \
                 wholesale on every write, and the tool log and diffs below come \
                 from the same value and are complete",
                article(session.surface.label(session.provider)),
                session.surface.label(session.provider)
            ));
        }
    };
    if let Err(e) = read {
        return unsupported(&format!("could not read the transcript: {e}"));
    }
    Conversation {
        stamp: stamp_of(path),
        ..sink.finish()
    }
}

/// One subagent's own conversation, from its own transcript.
///
/// `None` when `agent` is not one of the session's subagents — the id comes
/// from a query string, so it is only ever looked up in the session's own
/// list, never turned into a path. The turns' sequence numbers are that file's,
/// not the main conversation's.
pub fn build_agent(session: &Session, agent: &str, before: Option<usize>) -> Option<Conversation> {
    if agent.is_empty() || agent.contains('/') || agent.contains("..") {
        return None;
    }
    let subagent = session.subagents.iter().find(|s| s.agent_id == agent)?;
    if subagent.ghost {
        return Some(unsupported(
            "this agent's transcript is gone — Claude Code purged it, and only what the main conversation recorded is left",
        ));
    }
    let Some(main) = session.data_file.as_ref() else {
        return Some(unsupported(
            "this session has no transcript file on this machine",
        ));
    };
    let Some(path) = crate::session::transcript_files(main)
        .into_iter()
        .skip(1)
        .find(|f| f.file_stem().is_some_and(|stem| stem == agent))
    else {
        return Some(unsupported("this agent's transcript is not on disk"));
    };
    let mut sink = Sink {
        before,
        limits: Limits::PAGE,
        sidechains: true,
        ..Sink::default()
    };
    if let Err(e) = extract::for_each_jsonl(&path, |item| sink.claude(item)) {
        return Some(unsupported(&format!("could not read the transcript: {e}")));
    }
    Some(Conversation {
        stamp: stamp_of(&path),
        ..sink.finish()
    })
}

/// Tie each `Agent` call, hand-back and task notification to the subagent it is
/// about.
///
/// The transcript says it three ways: the subagent's sidecar names the call
/// (`toolUseId`), a hand-back names the sender (`origin.from`, the stem without
/// its `agent-` prefix), and a task notification its task id. Each is turned
/// into the subagent's [`AgentCall::id`], so a reader matches on one key; an
/// id that names no subagent is dropped rather than left to match nothing.
fn join_agents(conversation: &mut Conversation, subagents: &[crate::session::Subagent]) {
    let known = |raw: &str| {
        subagents
            .iter()
            .find(|s| s.agent_id == raw || s.agent_id.strip_prefix("agent-") == Some(raw))
            .map(|s| s.agent_id.clone())
    };
    let mut reports: HashMap<String, (usize, String)> = HashMap::new();
    for turn in conversation.turns.iter_mut() {
        turn.agent = turn.agent.as_deref().and_then(known);
        if turn.kind == AGENT_MESSAGE
            && let Some(id) = &turn.agent
        {
            // The last hand-back is the report: an agent asked to carry on
            // reports again, and the newer one supersedes.
            reports.insert(id.clone(), (turn.seq, turn.text.clone()));
        }
    }
    for tool in conversation
        .turns
        .iter_mut()
        .flat_map(|t| t.tools.iter_mut())
    {
        if !matches!(tool.name.as_str(), "Agent" | "Task") || tool.id.is_empty() {
            continue;
        }
        let Some(sa) = subagents
            .iter()
            .find(|s| s.tool_use_id.as_deref() == Some(tool.id.as_str()))
        else {
            continue;
        };
        let background = tool
            .result
            .as_deref()
            .is_some_and(|r| r.trim_start().starts_with("Async agent launched"));
        let handback = reports.get(&sa.agent_id);
        let report = match background {
            true => handback.map(|(_, text)| text.clone()),
            false => tool.result.clone().filter(|r| !r.trim().is_empty()),
        };
        // A background agent that never handed back still said something, and
        // the parser kept the last of it; a foreground agent's result is its
        // report even when empty, since the call is what it answered.
        let last_message = background && report.is_none() && sa.last_text.is_some();
        let report = match last_message {
            true => sa.last_text.clone(),
            false => report,
        };
        let mut call = AgentCall {
            background,
            handback: handback.map(|(seq, _)| *seq),
            report,
            last_message,
            ..AgentCall::from_subagent(sa)
        };
        if tool.failed {
            call.status = "failed".into();
        }
        tool.agent = Some(call);
    }
}

/// One OpenCode conversation, out of the database every one of its sessions
/// shares.
///
/// Nothing is asked for by file: the transcript is two tables in that
/// database, and a session is a `session_id` rather than a path. A message and
/// its parts come back together, which is what lets a tool call show as
/// finished the moment it is read — the call and its result are one row there,
/// rather than two entries a reader has to pair up.
fn read_opencode(path: &Path, session_id: &str, sink: &mut Sink) -> std::io::Result<()> {
    opencode::for_each_message(path, session_id, |message, created, parts| {
        sink.opencode(message, &opencode::message_time(message, created), parts);
    });
    Ok(())
}

/// Devin's transcript is one ATIF document rather than a line stream, so it
/// cannot go through [`extract::for_each_jsonl`]: read it whole and feed each
/// step through the same sink the other readers use.
fn read_devin(path: &Path, sink: &mut Sink) -> std::io::Result<()> {
    let content = std::fs::read_to_string(path)?;
    let doc: Value = serde_json::from_str(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    // A call's outcome is not in the transcript — the step records the call,
    // the database records how it ended.
    let statuses = devin::tool_statuses(
        doc.get("session_id").and_then(Value::as_str),
        &devin::db_for(Some(path)),
    );
    if let Some(steps) = doc.get("steps").and_then(Value::as_array) {
        for step in steps {
            sink.devin(step, &statuses);
        }
    }
    Ok(())
}

/// `a` or `an`, whichever a harness label takes — "an OpenCode conversation",
/// not "a OpenCode conversation".
fn article(label: &str) -> &'static str {
    match label.chars().next() {
        Some('A' | 'E' | 'I' | 'O' | 'U') => "an",
        _ => "a",
    }
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
    /// When set, the window stops at this sequence number: turns at or past it
    /// still count (`next` keeps moving, so a later window's `earlier` is
    /// unchanged) but are not kept. Their records are still read — a tool
    /// result can arrive entries after the boundary and still needs to land on
    /// a call inside the window.
    before: Option<usize>,
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
    limits: Limits,
    /// Read sidechain records rather than skip them: set when the file being
    /// read is a subagent's own, where every record is one.
    sidechains: bool,
}

impl Sink {
    fn push(&mut self, turn: Turn) -> usize {
        // Anything pushed directly ends the run: a user turn, a compaction, a
        // block of reasoning. Only `open_assistant` reopens one.
        self.run = None;
        let seq = self.next;
        self.next += 1;
        // Past the `before` boundary the turn is numbered but not kept: its
        // sequence has to exist so `first` still counts it, but the window a
        // paged-back request is answering ends before it.
        if self.before.is_none_or(|before| seq < before) {
            let mut turn = turn;
            turn.seq = seq;
            self.turns.push_back(turn);
            while self.turns.len() > self.limits.turns {
                self.turns.pop_front();
                self.first += 1;
            }
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
            let cap = sink.limits.tools;
            sink.turn_mut(seq)
                .is_some_and(|turn| turn.tools.len() < cap)
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
        let cap = self.limits.tools;
        let Some(turn) = self.turn_mut(seq) else {
            return;
        };
        if turn.tools.len() >= cap {
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
            stamp: None,
            unchanged: false,
        }
    }

    // --- Claude Code ---

    fn claude(&mut self, item: &Value) {
        // A subagent's turns are a different conversation that happens to share
        // a file. See the module docs.
        if !self.sidechains && item.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            return;
        }
        let ts = item.get("timestamp").and_then(Value::as_str).unwrap_or("");
        match item.get("type").and_then(Value::as_str) {
            Some("user") => self.claude_user(item, ts),
            Some("assistant") => self.claude_assistant(item, ts),
            Some("attachment") => self.claude_queued(item, ts),
            _ => {}
        }
    }

    /// Something that arrived while the agent was busy.
    ///
    /// Claude Code writes a message that lands mid-turn as a `queued_command`
    /// attachment rather than a `user` entry — whoever wrote it. A person
    /// typing while the agent works is recorded only this way, so ignoring
    /// attachments lost every word said mid-turn; and a subagent's report or
    /// a task notification arrives the same way, so the record says nothing
    /// about who spoke until its `origin` is read.
    fn claude_queued(&mut self, item: &Value, ts: &str) {
        let Some(queued) = item
            .get("attachment")
            .filter(|a| a.get("type").and_then(Value::as_str) == Some("queued_command"))
        else {
            return;
        };
        let text = prompt_text(queued.get("prompt"));
        if text.trim().is_empty() {
            return;
        }
        let mode = queued.get("commandMode").and_then(Value::as_str);
        let by = author(queued.get("origin"), mode, &text);
        self.claude_said(by, false, &text, ts);
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
        // what is left is a person talking — unless the entry is the harness's
        // own (`isMeta`), as a hook's output is.
        let compaction = item.get("isCompactSummary").and_then(Value::as_bool) == Some(true);
        let by = match had_result && item_is_meta(item) {
            true => Author::Harness,
            false => author(item.get("origin"), None, &text),
        };
        self.claude_said(by, compaction, &text, ts);
    }

    /// A message filed where a prompt goes, as the turn its author makes it.
    fn claude_said(&mut self, by: Author, compaction: bool, text: &str, ts: &str) {
        let text_in = text;
        let mut sender = (None, None);
        let (role, kind, text) = match (compaction, by) {
            (true, _) => ("system", "compaction", text.to_string()),
            (false, Author::Person) => ("user", "message", text.to_string()),
            // The harness writes for a parser, not for a reader. What it says
            // is worth keeping; the tags around it are not, and a turn that is
            // only tags says nothing at all.
            (false, Author::Harness) => ("system", "message", tidy_harness_text(text)),
            (false, Author::Agent { name, id }) => {
                sender = (name, id.or_else(|| agent_message_sender(text)));
                ("system", AGENT_MESSAGE, tidy_agent_message(text))
            }
        };
        if text.trim().is_empty() {
            return;
        }
        if kind == "message" && role == "system" {
            // A task notification about a subagent names it by its task id; kept
            // so the join can tie the notice to the agent, and dropped there if
            // it names no agent.
            sender.1 = tagged(text_in, "task-id").map(|id| id.trim().to_string());
        }
        let mut turn = Turn::new(role, kind, ts);
        (turn.from, turn.agent) = sender;
        turn.set_text(&text, self.limits.text);
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
                            id: block
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
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
            turn.set_text(&thinking, self.limits.text);
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
        let cap = self.limits.text;
        if let Some(turn) = self.turn_mut(seq) {
            turn.append_text(&text, cap);
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
                    turn.set_text(&text, self.limits.text);
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
        turn.set_text(&text, self.limits.text);
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

    // --- OpenCode ---

    /// One OpenCode message: what it said, and what it did while saying it.
    ///
    /// A message is already a whole reply. Its reasoning, its words and every
    /// call it made are parts of this one row, so there is no run to carry
    /// across entries as there is for the harnesses that write a reply in
    /// pieces, and no result waiting on a later one — which is also why two
    /// messages in a row are two replies rather than one spread over two.
    fn opencode(&mut self, message: &Value, ts: &str, parts: &[Value]) {
        let mut text = String::new();
        let mut thinking = String::new();
        let mut calls: Vec<ToolUse> = Vec::new();
        let mut compacted = false;

        for part in parts {
            match part.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if opencode_is_played_back(part) {
                        continue;
                    }
                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                        push_text(&mut text, t);
                    }
                }
                Some("reasoning") => {
                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                        push_text(&mut thinking, t);
                    }
                }
                Some("tool") => calls.push(opencode_call(part)),
                Some("compaction") => compacted = true,
                // `step-start` and `step-finish` bracket a model call and carry
                // its cost; `patch` is the file state a call left behind, which
                // the call itself already reports; `file` is an attachment.
                _ => {}
            }
        }

        if compacted {
            // The messages before a compaction are deleted rather than scrolled
            // past, so the transcript jumps with nothing to mark it. This is the
            // only thing in it that says the conversation went on.
            let mut turn = Turn::new("system", "compaction", ts);
            turn.set_text(
                "the context was compacted here — every turn before it \
                           is no longer in this session's transcript",
                self.limits.text,
            );
            self.push(turn);
        }
        if !thinking.trim().is_empty() {
            let mut turn = Turn::new("assistant", "reasoning", ts);
            turn.set_text(&thinking, self.limits.text);
            self.push(turn);
        }
        if text.trim().is_empty() && calls.is_empty() {
            return;
        }
        let role = match message.get("role").and_then(Value::as_str) {
            Some("assistant") => "assistant",
            Some("user") => "user",
            _ => "system",
        };
        let mut turn = Turn::new(role, "message", ts);
        turn.set_text(&text, self.limits.text);
        let seq = self.push(turn);
        // No call id: the result is on the same part as the call, so nothing
        // later can resolve it and an index entry would only be a leak.
        for call in calls {
            self.add_tool(seq, None, call);
        }
    }

    // --- Devin ---

    /// One ATIF step: the source says who is speaking, and an agent step is a
    /// whole model response — reasoning, reply text, tool calls, and the
    /// results they produced, which Devin records on the same step's
    /// `observation` rather than as the later entry Claude and Codex use.
    fn devin(&mut self, step: &Value, statuses: &HashMap<String, String>) {
        let ts = step.get("timestamp").and_then(Value::as_str).unwrap_or("");
        match step.get("source").and_then(Value::as_str) {
            Some("user") => {
                let Some(text) = step.get("message").and_then(Value::as_str) else {
                    return;
                };
                if text.trim().is_empty() {
                    return;
                }
                let mut turn = Turn::new("user", "message", ts);
                turn.set_text(text, self.limits.text);
                self.push(turn);
            }
            Some("agent") => self.devin_agent(step, ts, statuses),
            Some("system") => {
                // Most `system` steps are the prompt being assembled — the
                // `sysprompt` and `rules` telemetry sources say so directly, and
                // the rest are context blocks re-injected at each turn:
                // `<available_skills>`, `<system_info>`, loose chunks of the
                // prompt. What is worth a turn is an *event* — a tagged block
                // reporting that something happened, like a backgrounded
                // subagent finishing or the user editing a file mid-run.
                match step
                    .pointer("/extra/telemetry/source")
                    .and_then(Value::as_str)
                {
                    Some("sysprompt") | Some("rules") => return,
                    _ => {}
                }
                let Some(text) = step.get("message").and_then(Value::as_str) else {
                    return;
                };
                match text
                    .trim_start()
                    .strip_prefix('<')
                    .and_then(|r| r.split(['>', ' ', '\n', '\t', '/']).next())
                {
                    Some(tag)
                        if !tag.is_empty()
                            && tag
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                            && !matches!(tag, "available_skills" | "system_info") => {}
                    _ => return,
                }
                let mut turn = Turn::new("system", "message", ts);
                turn.set_text(&tidy_devin_event(text), self.limits.text);
                self.push(turn);
            }
            _ => {}
        }
    }

    fn devin_agent(&mut self, step: &Value, ts: &str, statuses: &HashMap<String, String>) {
        if let Some(thinking) = step.get("reasoning_content").and_then(Value::as_str)
            && !thinking.trim().is_empty()
        {
            let mut turn = Turn::new("assistant", "reasoning", ts);
            turn.set_text(thinking, self.limits.text);
            self.push(turn);
        }

        let text = step.get("message").and_then(Value::as_str).unwrap_or("");
        let empty: Vec<Value> = Vec::new();
        let calls = step
            .get("tool_calls")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        if text.trim().is_empty() && calls.is_empty() {
            return;
        }
        // `step_id` names the response: every step gets a fresh id, so one
        // step is one turn and nothing is ever folded into the reply before.
        // It is a number in ATIF, not a string.
        let request = step.get("step_id").map(|id| match id {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
        let seq = self.open_assistant(ts, request.as_deref());
        let cap = self.limits.text;
        if let Some(turn) = self.turn_mut(seq) {
            turn.append_text(text, cap);
        }
        for call in calls {
            let name = call
                .get("function_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let args = call.get("arguments").cloned().unwrap_or(Value::Null);
            let (short, full) = extract::tool_detail(name, &args);
            let mut tool = ToolUse {
                name: util::pretty_mcp_name(name),
                detail: short,
                full,
                ..ToolUse::default()
            };
            if matches!(name, "edit" | "write")
                && let Some(delta) = extract::edit_delta(&args)
            {
                tool.added = delta.added;
                tool.removed = delta.removed;
                tool.diff = delta.hunks.into_iter().take(MAX_DIFF_LINES).collect();
            }
            let id = call.get("tool_call_id").and_then(Value::as_str);
            self.add_tool(seq, id, tool);
        }
        for result in step
            .get("observation")
            .and_then(|o| o.get("results"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = result.get("source_call_id").and_then(Value::as_str) else {
                continue;
            };
            let body = flatten_content(result.get("content"));
            let failed = statuses.get(id).is_some_and(|s| s != "completed");
            self.resolve(id, Some(body), failed, None);
        }
    }

    // --- Cursor ---

    /// One Cursor entry: the role says who is speaking, and the blocks beneath
    /// it are what they said and what they asked the agent to do.
    ///
    /// A Cursor transcript dates nothing and pairs nothing, so a turn here has
    /// no time and a call has nothing to resolve — which is why each call is
    /// given an empty result rather than none. A missing one would say the call
    /// is still running, and for a harness that keeps no outcomes at all that
    /// would be a claim the transcript cannot support.
    fn cursor(&mut self, item: &Value) {
        let Some(role) = item.get("role").and_then(Value::as_str) else {
            return;
        };
        let Some(blocks) = item
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        else {
            return;
        };
        let mut text = String::new();
        let mut calls = Vec::new();
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = block.get("text").and_then(Value::as_str) {
                        push_text(&mut text, t);
                    }
                }
                Some("tool_use") => {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                    let args = block.get("input").cloned().unwrap_or(Value::Null);
                    calls.push(call_tool(name, &args, Outcome::NotRecorded));
                }
                _ => {}
            }
        }
        // Cursor wraps what a person typed in a `<user_query>` block, and hangs
        // attachments off the same message as their own tag.
        let tidy = tidy_harness_text(&text);
        if tidy.trim().is_empty() && calls.is_empty() {
            return;
        }
        let mut turn = Turn::new(
            match role {
                "assistant" => "assistant",
                "user" => "user",
                _ => "system",
            },
            "message",
            "",
        );
        turn.set_text(&tidy, self.limits.text);
        let seq = self.push(turn);
        for call in calls {
            self.add_tool(seq, None, call);
        }
    }

    // --- Gemini ---

    /// One Gemini record: the header and the `$set` patches that revise it are
    /// not conversation, so a record without one of the three types a person
    /// reads as nothing.
    fn gemini(&mut self, item: &Value) {
        let ts = item
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        match item.get("type").and_then(Value::as_str) {
            Some("user") => {
                let mut turn = Turn::new("user", "message", &ts);
                turn.set_text(&gemini_text(item.get("content")), self.limits.text);
                self.push(turn);
            }
            Some("info") => {
                // "Switched to Plan Mode." and the like: the harness speaking
                // for itself, which is what a `system` turn is.
                let text = item.get("content").and_then(Value::as_str).unwrap_or("");
                if text.trim().is_empty() {
                    return;
                }
                let mut turn = Turn::new("system", "message", &ts);
                turn.set_text(&tidy_harness_text(text), self.limits.text);
                self.push(turn);
            }
            Some("gemini") => self.gemini_reply(item, &ts),
            _ => {}
        }
    }

    /// One Gemini reply: what it thought, what it said, and every call it made
    /// in between.
    ///
    /// A `gemini` record is a whole response, and it carries each call's
    /// outcome on the call itself — the same one-record-one-reply shape
    /// OpenCode uses, and for the same reason there is no run to carry.
    fn gemini_reply(&mut self, item: &Value, ts: &str) {
        let mut thinking = String::new();
        for thought in item
            .get("thoughts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let body = thought
                .get("description")
                .and_then(Value::as_str)
                .or_else(|| thought.get("subject").and_then(Value::as_str))
                .unwrap_or("");
            push_text(&mut thinking, body);
        }
        if !thinking.trim().is_empty() {
            let mut turn = Turn::new("assistant", "reasoning", ts);
            turn.set_text(&thinking, self.limits.text);
            self.push(turn);
        }

        let text = item.get("content").and_then(Value::as_str).unwrap_or("");
        let empty: Vec<Value> = Vec::new();
        let calls = item
            .get("toolCalls")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        if text.trim().is_empty() && calls.is_empty() {
            return;
        }
        let mut turn = Turn::new("assistant", "message", ts);
        turn.set_text(&tidy_harness_text(text), self.limits.text);
        let seq = self.push(turn);
        for call in calls {
            let name = call.get("name").and_then(Value::as_str).unwrap_or("tool");
            let args = call.get("args").cloned().unwrap_or(Value::Null);
            // A Gemini call records what came back on the call itself, so a
            // missing result is a call still running rather than one whose
            // outcome the harness threw away.
            let outcome = match call.get("result") {
                Some(result) => Outcome::Recorded(
                    gemini_result(result),
                    call.get("status").and_then(Value::as_str) == Some("error"),
                ),
                None => Outcome::Running,
            };
            self.add_tool(seq, None, call_tool(name, &args, outcome));
        }
    }

    // --- Pi ---

    /// One Pi entry, of which only a `message` is conversation: the rest of the
    /// file is the harness narrating itself — labels, model changes, branch
    /// summaries, compactions.
    fn pi(&mut self, item: &Value) {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            return;
        }
        let Some(message) = item.get("message") else {
            return;
        };
        let ts = pi::message_ts(item, message);
        match message.get("role").and_then(Value::as_str) {
            Some("toolResult") => {
                let Some(id) = message.get("toolCallId").and_then(Value::as_str) else {
                    return;
                };
                let body = flatten_content(message.get("content"));
                let failed = message.get("isError").and_then(Value::as_bool) == Some(true);
                self.resolve(id, Some(body), failed, None);
            }
            Some(role @ ("user" | "assistant")) => {
                let mut text = String::new();
                let mut thinking = String::new();
                let mut calls = Vec::new();
                match message.get("content") {
                    // A person's turn is the string; an agent's is blocks.
                    Some(Value::String(s)) => push_text(&mut text, s),
                    Some(Value::Array(blocks)) => {
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
                                Some("toolCall") => {
                                    let name =
                                        block.get("name").and_then(Value::as_str).unwrap_or("tool");
                                    let args =
                                        block.get("arguments").cloned().unwrap_or(Value::Null);
                                    // The result is a later entry keyed by
                                    // `toolCallId`, so the call starts in flight
                                    // and `resolve` is what fills it in.
                                    let call = call_tool(name, &args, Outcome::Running);
                                    calls.push((id_of(block), call));
                                }
                                // An image is what a person pasted; the page
                                // shows the words, not the bytes.
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                if !thinking.trim().is_empty() {
                    let mut turn = Turn::new("assistant", "reasoning", &ts);
                    turn.set_text(&thinking, self.limits.text);
                    self.push(turn);
                }
                if text.trim().is_empty() && calls.is_empty() {
                    return;
                }
                let mut turn = Turn::new(
                    if role == "user" { "user" } else { "assistant" },
                    "message",
                    &ts,
                );
                turn.set_text(&tidy_harness_text(&text), self.limits.text);
                let seq = self.push(turn);
                for (id, call) in calls {
                    self.add_tool(seq, id.as_deref(), call);
                }
            }
            _ => {}
        }
    }
}

/// The id a Pi tool call is answered by, if it gave one.
///
/// Pi pairs a result to its call by id, so an idless call is one no later entry
/// can resolve and indexing it would be a key nothing ever looks up.
fn id_of(block: &Value) -> Option<String> {
    block.get("id").and_then(Value::as_str).map(str::to_string)
}

/// What a transcript says about how a call ended.
enum Outcome {
    /// The outcome is here, and this is whether it was a failure.
    Recorded(String, bool),
    /// The call is in flight. This harness records results, and there is not
    /// one yet — which is a different statement from there being no kind.
    Running,
    /// This harness records no outcomes at all, so a call in the transcript
    /// has finished and whatever came back was never kept.
    NotRecorded,
}

/// One call as a transcript records it.
///
/// [`Outcome::NotRecorded`] is the case worth the type: a missing result on a
/// harness that keeps some is a call still running, and the page draws those
/// two differently. Reading one as the other is the difference between a live
/// conversation and a transcript claiming every call in it is in flight.
fn call_tool(name: &str, args: &Value, outcome: Outcome) -> ToolUse {
    let (detail, full) = extract::tool_detail(name, args);
    let (result, failed) = match outcome {
        Outcome::Recorded(result, failed) => (Some(result), failed),
        Outcome::Running => (None, false),
        Outcome::NotRecorded => (Some(String::new()), false),
    };
    let delta = edits_file(name)
        .then(|| extract::edit_delta(args))
        .flatten();
    ToolUse {
        name: util::pretty_mcp_name(name),
        detail,
        full,
        result,
        failed,
        added: delta.as_ref().map_or(0, |d| d.added),
        removed: delta.as_ref().map_or(0, |d| d.removed),
        diff: delta
            .map(|d| d.hunks.into_iter().take(MAX_DIFF_LINES).collect())
            .unwrap_or_default(),
        ..ToolUse::default()
    }
}

/// Whether a tool is one of the harnesses' own file editors, which are the
/// calls whose arguments are the patch.
///
/// The names differ by capitalisation and by synonym between harnesses, and
/// none of them is the same word, so this is the list rather than a rule about
/// the arguments — a call's `content` key can hold anything, and treating that
/// as a diff would invent one.
fn edits_file(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "edit" | "write" | "replace" | "multiedit" | "str_replace" | "str_replace_editor"
    )
}

/// Whether a text part is something a person said rather than something
/// OpenCode or a plugin put in their mouth.
///
/// Two flags mark it, and both land on `user` messages: `ignored` is a plugin's
/// own progress output (a compression notice with a bar of block characters
/// under it), and `synthetic` is a replay of a tool call the reader has already
/// been shown. Neither is in the conversation, and a transcript showing either
/// is one nobody recognises.
fn opencode_is_played_back(part: &Value) -> bool {
    part.get("ignored").and_then(Value::as_bool) == Some(true)
        || part.get("synthetic").and_then(Value::as_bool) == Some(true)
}

/// One OpenCode tool call, with whatever came back from it.
///
/// The call and its outcome are one record, so there is nothing to wait for: a
/// `state` with no `output` yet is a call still running, and that absence is
/// what the page already draws for a live call from any other harness.
fn opencode_call(part: &Value) -> ToolUse {
    let state = part.get("state");
    let name = part.get("tool").and_then(Value::as_str).unwrap_or("tool");
    let input = state
        .and_then(|s| s.get("input"))
        .cloned()
        .unwrap_or(Value::Null);
    let (mut detail, full) = extract::tool_detail(name, &input);
    // OpenCode writes the one line it shows for the call, which is the only
    // thing that can describe a tool cctop has no rule for — a plugin's, or an
    // MCP server's, where `tool_detail` has nothing to go on.
    if detail.is_empty()
        && let Some(title) = state.and_then(|s| s.get("title")).and_then(Value::as_str)
    {
        detail = title.to_string();
    }
    let output = state.and_then(|s| s.get("output"));
    let why = state
        .and_then(|s| s.get("error"))
        .and_then(Value::as_str)
        .filter(|e| !e.trim().is_empty());
    let mut tool = ToolUse {
        name: util::pretty_mcp_name(name),
        detail,
        full,
        result: output
            .map(|out| flatten_content(Some(out)))
            .or_else(|| why.map(str::to_string)),
        failed: state.and_then(|s| s.get("status")).and_then(Value::as_str) == Some("error"),
        ..ToolUse::default()
    };
    if let Some(delta) = state.and_then(|s| opencode::tool_delta(name, s)) {
        tool.added = delta.added;
        tool.removed = delta.removed;
        tool.diff = delta.hunks.into_iter().take(MAX_DIFF_LINES).collect();
    }
    tool
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

/// What a Gemini record said, whichever of its two shapes holds it.
///
/// A person's turn is a list of text blocks; an agent's is the string itself.
/// Both appear on the same field, so the string case is the one that has to be
/// told apart rather than assumed.
fn gemini_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(blocks) => {
            let mut out = String::new();
            for block in blocks.as_array().into_iter().flatten() {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    push_text(&mut out, t);
                }
            }
            out
        }
        None => String::new(),
    }
}

/// What a Gemini call returned, out of the envelope it arrives in.
///
/// The outcome sits under `functionResponse.response`, whose shape is the
/// called tool's own, so the first response that carries any text is the one
/// shown and the whole envelope is the fallback — an empty box on a call that
/// demonstrably did something is worse than an ugly one.
fn gemini_result(result: &Value) -> String {
    for response in result
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("functionResponse"))
    {
        let body = response
            .get("response")
            .map(|inner| flatten_content(Some(inner)))
            .unwrap_or_default();
        if !body.trim().is_empty() {
            return body;
        }
    }
    flatten_content(Some(result))
}

/// Whether a user turn is really the harness talking.
///
/// Claude Code writes several of its own things into `user` entries — the
/// expansion of a slash command, a hook's output, the reminder blocks it
/// injects — and showing those as something a person typed is the difference
/// between a transcript someone recognises and one they do not.
///
/// The named prefixes stay because those blocks are also written on one line,
/// where the shape below cannot see them. The shape is what catches the rest:
/// the list of names was a list that had to be kept up to date and was not, and
/// `<task-notification>` reached a handoff brief quoted as the user's own words
/// because nothing had added it. A person opening a message with a bare tag and
/// nothing else on the line is the rarer mistake to make.
fn is_harness_text(text: &str) -> bool {
    let head = text.trim_start();
    head.starts_with("<command-name>")
        || head.starts_with("<local-command")
        || head.starts_with("<system-reminder>")
        || head.starts_with("<user-prompt-submit-hook>")
        || head.starts_with("Caveat:")
        || opens_with_bare_tag(head)
}

/// Whether the first line is `<name>` and nothing else — the shape every
/// injected block shares, and the shape prose does not.
fn opens_with_bare_tag(text: &str) -> bool {
    let Some(line) = text.lines().next().map(str::trim) else {
        return false;
    };
    let Some(name) = line.strip_prefix('<').and_then(|l| l.strip_suffix('>')) else {
        return false;
    };
    // No attributes and no closing tag on the same line: `<p>hi</p>` is
    // somebody pasting markup, and `<b>` alone on a line is not a sentence.
    !name.is_empty()
        && !name.starts_with('/')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
    if text.trim_start().starts_with("<task-notification>") {
        return tidy_task_notification(text);
    }
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

/// A `<task-notification>` as news, not markup.
///
/// The notice that a background task or a monitor spoke arrives as a `user`
/// entry full of the fields a dispatcher needs — `task-id`, `tool-use-id`,
/// `output-file`, `usage`, `worktree` — around the ones a reader does:
/// `summary`, `event` when a monitor is reporting, and `result` when a
/// finished agent left a note. Every field sits on its own `<field>value
/// </field>` line, so the generic tidy strips them all, finds the body
/// empty, and strips only the outer tag — which is how the whole field list
/// reached the transcript as the turn's text. Read the fields that carry
/// the news; a notification holding none of them says nothing worth a turn.
fn tidy_task_notification(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for field in ["summary", "event", "result"] {
        let Some(body) = tagged(text, field).map(str::trim) else {
            continue;
        };
        if body.is_empty() || lines.iter().any(|l| l == body) {
            continue;
        }
        lines.push(unescape_entities(body));
    }
    // Anything written after the closing tag is the entry's real text — the
    // notice is only the part inside it.
    if let Some(end) = text.find("</task-notification>") {
        let tail = text[end + "</task-notification>".len()..].trim();
        if !tail.is_empty() {
            lines.push(tail.to_string());
        }
    }
    lines.join("\n")
}

/// The entities the transcript writer escapes inside these fields. `&amp;`
/// goes last, or `&amp;gt;` decodes twice.
fn unescape_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// A Devin system event, readable.
///
/// The block arrives dressed for the model, not for a reader: an
/// `additional_metadata` opens with instructions about when to mention it,
/// then a `user_actions` carrying one `[diff_block]` per file the user
/// touched — dozens of lines of pseudo-diff where the event was "you edited
/// these files". The block and its footnote go; the file names and any other
/// action prose stay. Anything else — a subagent's completion report — loses
/// only its envelope, same as every other harness block.
fn tidy_devin_event(text: &str) -> String {
    let Some(actions) = tagged(text, "user_actions") else {
        return tidy_harness_text(text);
    };
    let mut files = Vec::new();
    let mut prose = Vec::new();
    let mut in_diff = false;
    for line in actions.lines().map(str::trim) {
        match line {
            "[diff_block_start]" => in_diff = true,
            "[diff_block_end]" => in_diff = false,
            _ if in_diff || line.is_empty() => {}
            l if l.starts_with("Please note that") => {}
            l => match l.strip_prefix("The following changes were made by the USER to: ") {
                Some(file) => files.push(file.trim_end_matches('.').to_string()),
                None => prose.push(l.to_string()),
            },
        }
    }
    if !files.is_empty() {
        prose.insert(0, format!("the user edited {}", files.join(", ")));
    }
    match prose.is_empty() {
        true => tidy_harness_text(text),
        false => prose.join("\n"),
    }
}

/// The [`Turn::kind`] of a message another agent sent this one: a subagent's
/// hand-back, another session writing in, or a coordinator briefing a worker.
pub const AGENT_MESSAGE: &str = "agent-message";

/// The line Claude Code opens another session's message with.
const AGENT_PREAMBLE: &str = "Another Claude session sent a message";

/// Who wrote a message Claude Code filed where a person's prompt goes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Author {
    /// The person at the keyboard.
    Person,
    /// Another agent: the name the transcript gives it and its id, if any.
    Agent {
        name: Option<String>,
        id: Option<String>,
    },
    /// The harness: a slash command, a hook, a task notification.
    Harness,
}

/// Who wrote a prompt-shaped message — the one decision both a `user` entry
/// and a `queued_command` attachment go through.
///
/// `origin.kind` is the record's own answer and is read first. Nothing else on
/// the record separates the cases: `renderedRole` is `system` on everything
/// queued, including what the person typed, and neither `isMeta` nor
/// `commandMode` is written on every kind. Only `human` is the person; a kind
/// this does not know is someone else, because showing a machine's words as
/// the person's is the mistake this exists to stop, and the opposite one only
/// dims a line. Transcripts older than `origin` fall back to the shape of the
/// text.
fn author(origin: Option<&Value>, command_mode: Option<&str>, text: &str) -> Author {
    let kind = origin.and_then(|o| o.get("kind")).and_then(Value::as_str);
    let field = |key: &str| {
        origin
            .and_then(|o| o.get(key))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
    };
    let agent = || Author::Agent {
        name: field("name"),
        id: field("from"),
    };
    match kind {
        // A slash command the person typed is still recorded as the harness's
        // expansion of it.
        Some("human") if is_harness_text(text) => Author::Harness,
        Some("human") => Author::Person,
        Some("peer" | "coordinator") => agent(),
        Some(_) if is_agent_text(text) => agent(),
        Some(_) => Author::Harness,
        None if command_mode == Some("task-notification") => Author::Harness,
        None if is_agent_text(text) => Author::Agent {
            name: None,
            id: agent_message_sender(text),
        },
        None if is_harness_text(text) => Author::Harness,
        None => Author::Person,
    }
}

/// Who wrote a prompt-shaped message, without the sender's name: what the
/// context meter needs to file its characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Speaker {
    Person,
    Agent,
    Harness,
}

/// [`author`], for the parser: the same decision the conversation view makes,
/// so the context meter can never file a message under someone the reader
/// does not show as its writer.
pub(crate) fn speaker(origin: Option<&Value>, command_mode: Option<&str>, text: &str) -> Speaker {
    match author(origin, command_mode, text) {
        Author::Person => Speaker::Person,
        Author::Agent { .. } => Speaker::Agent,
        Author::Harness => Speaker::Harness,
    }
}

/// Whether a record's `origin` says someone other than the person wrote it.
///
/// For the readers that only need to know whether to trust a record as typed —
/// the effort switch — rather than how to show it. A record with no `origin`
/// is not ruled out here; it predates the field.
pub(crate) fn from_someone_else(item: &Value) -> bool {
    item.get("origin")
        .and_then(|o| o.get("kind"))
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "human")
}

/// Whether text has the shape of another agent's message, for records that do
/// not say who wrote them.
fn is_agent_text(text: &str) -> bool {
    let head = text.trim_start();
    head.starts_with(AGENT_PREAMBLE) || head.starts_with("<agent-message")
}

/// The `from` attribute of an `<agent-message>` tag: the sender's id, for a
/// record whose `origin` does not carry it.
fn agent_message_sender(text: &str) -> Option<String> {
    let open = &text[text.find("<agent-message")?..];
    let tag = &open[..open.find('>')?];
    let rest = &tag[tag.find("from=\"")? + "from=\"".len()..];
    let id = &rest[..rest.find('"')?];
    (!id.is_empty()).then(|| id.to_string())
}

/// Another agent's message as the report it carries.
///
/// The harness wraps it three times over for the model's benefit: a preamble
/// line, `<agent-message from="…">` tags, and a trailing paragraph explaining
/// what the other session is. Inside, a subagent's hand-back opens with a
/// frame ending `The report follows:` and indents the report beneath it. A
/// reader wants the report, so all of that comes off and the report is
/// de-indented; text that does not have a given layer keeps what it has.
fn tidy_agent_message(text: &str) -> String {
    let body = match text.find("<agent-message") {
        Some(open) => {
            let rest = &text[open..];
            let rest = rest.find('>').map_or("", |end| &rest[end + 1..]);
            rest.find("</agent-message>")
                .map_or(rest, |close| &rest[..close])
        }
        None => match text.trim_start().strip_prefix(AGENT_PREAMBLE) {
            Some(rest) => rest.split_once('\n').map_or("", |(_, after)| after),
            None => text,
        },
    };
    const FRAME_END: &str = "The report follows:";
    let body = match body.trim_start().starts_with("[Subagent hand-back]") {
        true => body
            .find(FRAME_END)
            .map_or(body, |at| &body[at + FRAME_END.len()..]),
        false => body,
    };
    dedent(body).trim().to_string()
}

/// Text with the indentation every non-blank line shares taken off.
fn dedent(text: &str) -> String {
    // Whatever shares the first line with the frame is not indented with the
    // rest, so it is measured separately and only trimmed.
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let indent = rest
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out = first.trim().to_string();
    for line in rest.lines() {
        out.push('\n');
        // Only whitespace is cut: a blank line shorter than the indent is
        // cut to nothing rather than into its neighbour's text.
        let cut = line.len() - line.trim_start().len();
        out.push_str(&line[cut.min(indent)..]);
    }
    out
}

/// A queued message's `prompt`: a string ordinarily, a list of blocks when an
/// image came with it. Only the words are kept, as for a `user` entry.
pub(crate) fn prompt_text(prompt: Option<&Value>) -> String {
    let mut text = String::new();
    match prompt {
        Some(Value::String(s)) => text.push_str(s),
        Some(Value::Array(blocks)) => {
            for block in blocks {
                match block {
                    Value::String(s) => push_text(&mut text, s),
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
    text
}

fn item_is_meta(item: &Value) -> bool {
    item.get("isMeta").and_then(Value::as_bool) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The named prefixes were a list that had to be kept up to date, and was
    /// not: a `<task-notification>` reached a handoff brief quoted as the
    /// user's own words. Every injected block shares a shape.
    #[test]
    fn a_block_the_harness_injected_is_not_something_a_person_said() {
        assert!(is_harness_text(
            "<system-reminder>\nbe careful\n</system-reminder>"
        ));
        assert!(is_harness_text(
            "<task-notification>\n<task-id>abc</task-id>"
        ));
        assert!(is_harness_text("<user-prompt-submit-hook>\nhook said this"));
        assert!(is_harness_text("<command-name>/clear</command-name>"));
        assert!(is_harness_text("Caveat: the messages below were generated"));
    }

    /// Matching on shape must not swallow a person who happens to write markup.
    #[test]
    fn a_person_who_pastes_markup_is_still_a_person() {
        assert!(!is_harness_text("<p>hello</p>"));
        assert!(!is_harness_text("why does <div> break here?"));
        assert!(!is_harness_text("<img src=\"x\">"));
        assert!(!is_harness_text("</closing>"));
        assert!(!is_harness_text("fix the parser"));
    }

    /// A page that already holds the conversation gets nothing for an
    /// unchanged transcript, and only its newest turn onward for a changed
    /// one — the newest included, because a tool result lands on it.
    #[test]
    fn a_poll_gets_only_what_the_page_lacks() {
        let turn = |seq| Turn {
            seq,
            ..sink_claude(&[r#"{"type":"user","message":{"content":"hi"}}"#]).turns[0].clone()
        };
        let whole = Conversation {
            supported: true,
            turns: (0..5).map(turn).collect(),
            earlier: 10,
            stamp: Some("100-1".into()),
            ..Default::default()
        };

        let same = whole.clone().narrowed(&Since {
            stamp: Some("100-1".into()),
            after: Some(4),
        });
        assert!(same.unchanged);
        assert!(same.turns.is_empty());
        assert_eq!(same.earlier, 10, "the count of earlier turns still holds");

        let moved = whole.clone().narrowed(&Since {
            stamp: Some("90-0".into()),
            after: Some(3),
        });
        assert!(!moved.unchanged);
        assert_eq!(
            moved.turns.iter().map(|t| t.seq).collect::<Vec<_>>(),
            [3, 4]
        );

        let first = whole.clone().narrowed(&Since::default());
        assert_eq!(first.turns.len(), 5, "a first read is the whole window");
    }

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

    fn sink_cursor(lines: &[&str]) -> Conversation {
        let mut sink = Sink::default();
        for line in lines {
            sink.cursor(&serde_json::from_str(line).unwrap());
        }
        sink.finish()
    }

    fn sink_gemini(lines: &[&str]) -> Conversation {
        let mut sink = Sink::default();
        for line in lines {
            sink.gemini(&serde_json::from_str(line).unwrap());
        }
        sink.finish()
    }

    fn sink_pi(lines: &[&str]) -> Conversation {
        let mut sink = Sink::default();
        for line in lines {
            sink.pi(&serde_json::from_str(line).unwrap());
        }
        sink.finish()
    }

    /// A database shaped the way OpenCode writes one, holding the rows of
    /// `ses_f1d129d6dffeGj3ft9REKBbzIY` verbatim.
    ///
    /// The envelope of each message and the JSON of each part are exactly what
    /// the real database held, so a reader that gets this right is right about
    /// the format rather than about a shape invented beside it.
    fn opencode_db(
        messages: &[(&str, i64, &str, &str, &[&str])],
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode.db");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE message (id text PRIMARY KEY, session_id text NOT NULL, \
             time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL); \
             CREATE TABLE part (id text PRIMARY KEY, message_id text NOT NULL, \
             session_id text NOT NULL, time_created integer NOT NULL, \
             time_updated integer NOT NULL, data text NOT NULL);",
        )
        .unwrap();
        for (index, (id, created, data, session, parts)) in messages.iter().enumerate() {
            db.execute(
                "INSERT INTO message (id, session_id, time_created, time_updated, data) \
                 VALUES (?1, ?4, ?2, ?2, ?3)",
                rusqlite::params![id, created, data, session],
            )
            .unwrap();
            for (part, data) in parts.iter().enumerate() {
                db.execute(
                    "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) \
                     VALUES (?1, ?2, ?4, ?3, ?3, ?5)",
                    rusqlite::params![
                        format!("prt_{index}_{part:02}"),
                        id,
                        created,
                        session,
                        *data
                    ],
                )
                .unwrap();
            }
        }
        (dir, path)
    }

    /// What a person asked and what the agent did about it, read out of two
    /// tables rather than a line of JSON: the text is a part of a message, and
    /// so is the call the reply made.
    #[test]
    fn an_opencode_exchange_reads_the_same_way() {
        let (_dir, path) = opencode_db(&[
            (
                "msg_user",
                1_790_513_603_273,
                r#"{"role":"user","time":{"created":1790513603273},"agent":"build","model":{"providerID":"opencode-go","modelID":"deepseek-v4.1-flash"},"summary":{"diffs":[]}}"#,
                "ses_1",
                &[r#"{"type":"text","text":"fix that"}"#],
            ),
            (
                "msg_assistant",
                1_790_513_603_841,
                r#"{"parentID":"msg_user","role":"assistant","mode":"build","agent":"build","cost":0.005038026,"tokens":{"total":35130,"input":33267,"output":43,"reasoning":28,"cache":{"write":0,"read":1792}},"modelID":"deepseek-v4.1-flash","providerID":"opencode-go","time":{"created":1790513603841,"completed":1790513623910},"finish":"tool-calls"}"#,
                "ses_1",
                &[
                    r#"{"type":"step-start","snapshot":"b5c54e70bee180a4dfda72a57242f7cf0cf790c0"}"#,
                    r#"{"type":"reasoning","text":"the note reads wrong","time":{"start":1790513603841,"end":1790513604000}}"#,
                    r#"{"type":"tool","tool":"grep","callID":"call_00_2oelhco6qfl7o8wyhsmg9e2s","state":{"status":"completed","input":{"pattern":"cannot read a OpenCode conversation"},"output":"No files found","metadata":{"matches":0,"truncated":false},"title":"cannot read a OpenCode conversation","time":{"start":1790513604100,"end":1790513604110}}}"#,
                    r#"{"type":"step-finish","reason":"tool-calls","snapshot":"b5c54e70bee180a4dfda72a57242f7cf0cf790c0","cost":0.005038026}"#,
                ],
            ),
        ]);
        let mut session = Session::new(Provider::OpenCode, "ses_1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        assert!(chat.supported, "{:?}", chat.note);
        // The thinking part and the two bookkeeping parts are not turns, but
        // the thinking is a turn of its own: it reads differently on purpose.
        assert_eq!(chat.turns.len(), 3);
        assert_eq!(
            (chat.turns[0].role.as_ref(), chat.turns[0].kind.as_ref()),
            ("user", "message")
        );
        assert_eq!(chat.turns[0].text, "fix that");
        assert_eq!(
            (chat.turns[1].role.as_ref(), chat.turns[1].kind.as_ref()),
            ("assistant", "reasoning")
        );
        assert_eq!(chat.turns[1].text, "the note reads wrong");
        // A message is already a whole reply, so the turn it becomes is its own
        // rather than an extension of the reasoning above it.
        assert_eq!(chat.turns[2].seq, 2);
        assert_eq!(chat.turns[2].role, "assistant");
        let call = &chat.turns[2].tools[0];
        assert_eq!(call.name, "grep");
        assert_eq!(call.detail, "cannot read a OpenCode conversation");
        assert_eq!(call.result.as_deref(), Some("No files found"));
        assert!(!call.failed);
        // The millisecond the envelope records is the turn's time, in the form
        // the page hands to its clock.
        assert_eq!(chat.turns[0].ts, util::ms_to_rfc3339(1_790_513_603_273));
    }

    /// A call that has not come back yet, and one that came back wrong, are
    /// both things the live view exists to show: the first has no result to
    /// show, the second has a message to show instead of output.
    #[test]
    fn an_opencode_call_says_whether_it_is_still_running() {
        let (_dir, path) = opencode_db(&[(
            "msg_assistant",
            1_790_513_603_841,
            r#"{"role":"assistant","time":{"created":1790513603841}}"#,
            "ses_1",
            &[
                r#"{"type":"tool","tool":"bash","state":{"status":"running","input":{"command":"cargo test"},"title":"cargo test"}}"#,
                r#"{"type":"tool","tool":"read","state":{"status":"error","input":{"filePath":"/tmp/backend_test.log"},"error":"Cannot read binary file: /tmp/backend_test.log","title":"/tmp/backend_test.log"}}"#,
            ],
        )]);
        let mut session = Session::new(Provider::OpenCode, "ses_1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        let tools = &chat.turns[0].tools;
        assert_eq!(tools.len(), 2);
        assert!(tools[0].result.is_none(), "a running call has no result");
        assert!(!tools[0].failed);
        assert_eq!(
            tools[1].result.as_deref(),
            Some("Cannot read binary file: /tmp/backend_test.log")
        );
        assert!(tools[1].failed);
    }

    /// The lines an edit added and removed come out of the input OpenCode
    /// recorded, and the table already counts its `+` and `-` from there, so a
    /// call in the conversation view and its row in the table say one number.
    #[test]
    fn an_opencode_edit_carries_the_same_lines_the_table_counted() {
        let (_dir, path) = opencode_db(&[(
            "msg_assistant",
            1_790_513_603_841,
            r#"{"role":"assistant","time":{"created":1790513603841}}"#,
            "ses_1",
            &[
                r#"{"type":"tool","tool":"edit","state":{"status":"completed","input":{"filePath":"/a/b.rs","oldString":"let a = 1;\nlet b = 2;\n","newString":"let a = 1;\nlet b = 3;\nlet c = 4;\n"},"output":"ok","title":"/a/b.rs"}}"#,
            ],
        )]);
        let mut session = Session::new(Provider::OpenCode, "ses_1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        let call = &chat.turns[0].tools[0];
        assert_eq!(call.name, "edit");
        assert_eq!((call.added, call.removed), (3, 2));
        assert!(call.diff.iter().any(|l| l.contains("let b = 3;")));
        assert!(call.diff.iter().any(|l| l.contains("let b = 2;")));
    }

    /// A plugin writing its progress and a tool call replayed into the prompt
    /// both land on a user message. Shown, they are a person being shown their
    /// own tool output a second time, as though they had asked for it.
    #[test]
    fn text_the_harness_played_back_is_not_something_a_person_said() {
        let (_dir, path) = opencode_db(&[(
            "msg_user",
            1_790_513_603_273,
            r#"{"role":"user","time":{"created":1790513603273},"summary":{"diffs":[]}}"#,
            "ses_1",
            &[
                r#"{"type":"text","text":"the real question"}"#,
                r#"{"type":"text","text":"▣ DCP | -31.1K removed, +4.7K summary","ignored":true}"#,
                r#"{"type":"text","text":"Called the Read tool with the following input: {\"filePath\":\"/a/b.rs\"}","synthetic":true}"#,
            ],
        )]);
        let mut session = Session::new(Provider::OpenCode, "ses_1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "the real question");
    }

    /// OpenCode drops the messages before a compaction and records the seam. A
    /// transcript that opens mid-conversation says so, rather than implying the
    /// session began where the scroll does.
    #[test]
    fn an_opencode_compaction_is_a_seam_not_a_message() {
        let (_dir, path) = opencode_db(&[(
            "msg_compaction",
            1_790_513_603_273,
            r#"{"role":"user","model":{"providerID":"opencode-go"},"agent":"explore","time":{"created":1790513603273},"summary":{"diffs":[]}}"#,
            "ses_1",
            &[r#"{"type":"compaction","auto":true,"overflow":false,"tail_start_id":"msg_tail"}"#],
        )]);
        let mut session = Session::new(Provider::OpenCode, "ses_1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(
            (chat.turns[0].role.as_ref(), chat.turns[0].kind.as_ref()),
            ("system", "compaction")
        );
        assert!(chat.turns[0].text.contains("compacted"));
    }

    /// Four hundred and ninety of the seven hundred sessions here are a
    /// subagent, whose messages are their own. A parent that read its child's
    /// would show work twice, on two rows of the table at once.
    #[test]
    fn an_opencode_subagent_does_not_leak_into_its_parent() {
        let (_dir, path) = opencode_db(&[
            (
                "msg_parent",
                1_790_513_603_273,
                r#"{"role":"user","time":{"created":1790513603273}}"#,
                "ses_parent",
                &[r#"{"type":"text","text":"the parent's question"}"#],
            ),
            (
                "msg_child",
                1_790_513_603_274,
                r#"{"role":"user","time":{"created":1790513603274}}"#,
                "ses_child",
                &[r#"{"type":"text","text":"the subagent's brief"}"#],
            ),
        ]);
        let mut session = Session::new(Provider::OpenCode, "ses_parent".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "the parent's question");
    }

    /// A Cursor exchange reads the way every other does: the role says who is
    /// speaking, the blocks are the words and the calls, and what the harness
    /// does not keep is left as a gap rather than filled in.
    ///
    /// These are the shapes Cursor actually writes, taken from transcripts in
    /// `~/.cursor/projects`: no `type` on a message entry at all, `turn_ended`
    /// entries beside them, and user text wrapped in `<user_query>`.
    #[test]
    fn a_cursor_exchange_reads_the_same_way() {
        let chat = sink_cursor(&[
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>\nextract doesn't extract the lorebook\n</user_query>"}]}}"#,
            r#"{"role":"assistant","message":{"content":[{"type":"text","text":"I'll trace how `extract` works."},{"type":"tool_use","name":"Grep","input":{"pattern":"lorebook","path":"/home/flo/JAR/jar_cli"}}]}}"#,
            r#"{"type":"turn_ended","status":"success"}"#,
            r#"{"type":"turn_ended","status":"error","error":"User aborted request"}"#,
        ]);
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].role, "user");
        // The wrapper is Cursor's, not the person's.
        assert_eq!(chat.turns[0].text, "extract doesn't extract the lorebook");
        assert_eq!(chat.turns[1].role, "assistant");
        assert_eq!(chat.turns[1].text, "I'll trace how `extract` works.");
        assert_eq!(chat.turns[1].tools.len(), 1);
        assert_eq!(chat.turns[1].tools[0].name, "Grep");
        assert!(chat.turns[1].tools[0].detail.contains("lorebook"));
        // Cursor dates nothing, so there is no time to show.
        assert_eq!(chat.turns[1].ts, "");
    }

    /// A Cursor call carries no outcome, and saying so must not read as a call
    /// still running: the page shows a missing result as a live one.
    #[test]
    fn a_cursor_call_is_not_forever_running() {
        let chat = sink_cursor(&[
            r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"path":"src/main.rs"}}]}}"#,
        ]);
        let tool = &chat.turns[0].tools[0];
        assert_eq!(tool.result.as_deref(), Some(""));
        assert!(!tool.failed);
    }

    /// A Gemini reply is one record holding its thoughts, its words and every
    /// call it made, with each result on the call itself.
    #[test]
    fn a_gemini_reply_reads_the_same_way() {
        let chat = sink_gemini(&[
            r#"{"sessionId":"79709c93","startTime":"2026-05-14T17:34:01.387Z","kind":"main"}"#,
            r#"{"$set":{"summary":"Improving script latency"}}"#,
            r#"{"type":"user","timestamp":"2026-05-14T17:34:06.028Z","content":[{"text":"go"}]}"#,
            r#"{"type":"gemini","timestamp":"2026-05-14T17:34:13.475Z","content":"on it","thoughts":[{"subject":"Where it blocks","description":"the parser re-reads the file per record"}],"toolCalls":[{"id":"read_file_1","name":"read_file","args":{"file_path":"README.md"},"status":"success","result":[{"functionResponse":{"id":"read_file_1","name":"read_file","response":{"output":"the readme"}}}]}]}"#,
            r#"{"type":"info","timestamp":"2026-05-14T17:34:20.000Z","content":"Switched to Plan Mode."}"#,
        ]);
        // The header and the `$set` patch are not conversation.
        assert_eq!(chat.turns.len(), 4);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[0].text, "go");
        assert_eq!(chat.turns[0].ts, "2026-05-14T17:34:06.028Z");
        assert_eq!(chat.turns[1].kind, "reasoning");
        assert_eq!(
            chat.turns[1].text,
            "the parser re-reads the file per record"
        );
        assert_eq!(chat.turns[2].text, "on it");
        let tool = &chat.turns[2].tools[0];
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.result.as_deref(), Some("the readme"));
        assert!(!tool.failed);
        // The harness speaking for itself.
        assert_eq!(chat.turns[3].role, "system");
        assert_eq!(chat.turns[3].text, "Switched to Plan Mode.");
    }

    /// A Gemini call that failed says so, and one with no result yet is a call
    /// in flight — this harness records outcomes, so the two are tellable apart.
    #[test]
    fn a_gemini_call_says_whether_it_still_is_running() {
        let chat = sink_gemini(&[
            r#"{"type":"gemini","timestamp":"t","content":"","toolCalls":[{"id":"a","name":"replace","args":{"file_path":"src/main.rs"},"status":"error","result":[{"functionResponse":{"response":{"output":"no such file"}}}]}]}"#,
            r#"{"type":"gemini","timestamp":"t2","content":"","toolCalls":[{"id":"b","name":"read_file","args":{}}]}"#,
        ]);
        assert!(chat.turns[0].tools[0].failed);
        assert_eq!(
            chat.turns[0].tools[0].result.as_deref(),
            Some("no such file")
        );
        // No result recorded: Gemini keeps them, so this one is still running.
        assert!(chat.turns[1].tools[0].result.is_none());
    }

    /// A Pi exchange: a person's turn is a bare string, an agent's is blocks,
    /// and a result is an entry of its own keyed to the call it answers.
    #[test]
    fn a_pi_exchange_reads_the_same_way() {
        let chat = sink_pi(&[
            r#"{"type":"session","version":3,"id":"pi-1","timestamp":"2024-12-03T14:00:00.000Z","cwd":"/work"}"#,
            r#"{"type":"label","label":"a name for this session"}"#,
            r#"{"type":"message","id":"a1","parentId":null,"timestamp":"2024-12-03T14:00:01.000Z","message":{"role":"user","content":"Hello"}}"#,
            r#"{"type":"message","id":"b2","parentId":"a1","timestamp":"2024-12-03T14:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"they said hello"},{"type":"text","text":"Hi!"},{"type":"toolCall","id":"call_123","name":"bash","arguments":{"command":"ls"}}]}}"#,
            r#"{"type":"message","id":"c3","parentId":"b2","timestamp":"2024-12-03T14:00:03.000Z","message":{"role":"toolResult","toolCallId":"call_123","toolName":"bash","content":[{"type":"text","text":"README.md"}],"isError":false}}"#,
        ]);
        // The session header and the label are the harness, not the conversation.
        assert_eq!(chat.turns.len(), 3);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[0].text, "Hello");
        assert_eq!(chat.turns[1].kind, "reasoning");
        assert_eq!(chat.turns[1].text, "they said hello");
        assert_eq!(chat.turns[2].text, "Hi!");
        let tool = &chat.turns[2].tools[0];
        assert_eq!(tool.name, "bash");
        // The result arrived as a later entry and found its call.
        assert_eq!(tool.result.as_deref(), Some("README.md"));
        assert!(!tool.failed);
    }

    /// A Pi result that failed is marked as one, and a call with no result
    /// entry behind it is a call in flight.
    #[test]
    fn a_pi_call_says_whether_it_still_is_running() {
        let chat = sink_pi(&[
            r#"{"type":"message","timestamp":"t","message":{"role":"assistant","content":[{"type":"toolCall","id":"c1","name":"bash","arguments":{}}]}}"#,
            r#"{"type":"message","timestamp":"t2","message":{"role":"toolResult","toolCallId":"c1","toolName":"bash","content":[{"type":"text","text":"boom"}],"isError":true}}"#,
        ]);
        assert!(chat.turns[0].tools[0].failed);
        assert_eq!(chat.turns[0].tools[0].result.as_deref(), Some("boom"));
    }

    /// An editor's arguments are the patch, whichever harness spelled the tool
    /// its own way — this is what the report's per-file diffs read.
    #[test]
    fn an_edit_carries_the_patch_whatever_the_tool_is_called() {
        let chat = sink_cursor(&[
            r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"path":"a.rs","oldString":"let x = 1;","newString":"let x = 2;"}}]}}"#,
        ]);
        let tool = &chat.turns[0].tools[0];
        assert_eq!((tool.added, tool.removed), (1, 1));
        assert_eq!(tool.diff, vec!["-let x = 1;", "+let x = 2;"]);
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
        let roles: Vec<&str> = chat.turns.iter().map(|t| t.role.as_ref()).collect();
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

    /// A task notification is fields for a dispatcher around one line of
    /// news. The generic tidy used to strip every field line, find nothing
    /// left, and keep the whole list as the turn's text.
    #[test]
    fn a_task_notification_reads_as_its_summary() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<task-notification>\n<task-id>bgbdzpbxb</task-id>\n<tool-use-id>toolu_01EK11bfRvKo9QaZMQWsMBGf</tool-use-id>\n<output-file>/tmp/tasks/bgbdzpbxb.output</output-file>\n<status>completed</status>\n<summary>Background command &quot;cargo build 2&gt;&amp;1 | tail -3&quot; completed (exit code 0)</summary>\n</task-notification>"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].role, "system");
        assert_eq!(
            chat.turns[0].text,
            "Background command \"cargo build 2>&1 | tail -3\" completed (exit code 0)"
        );
    }

    /// A monitor's report names its watch in `summary` and carries the news
    /// in `event`; both belong on the turn.
    #[test]
    fn a_monitor_event_keeps_the_event_it_reports() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<task-notification>\n<task-id>br7crykx8</task-id>\n<summary>Monitor event: &quot;rebuild chain&quot;</summary>\n<event>[Monitor expired after 30m with 11 events delivered.]</event>\n</task-notification>"}}"#,
        ]);
        assert_eq!(
            chat.turns[0].text,
            "Monitor event: \"rebuild chain\"\n[Monitor expired after 30m with 11 events delivered.]"
        );
    }

    /// A finished agent can leave a `result` note under its summary.
    #[test]
    fn a_task_notification_keeps_an_agents_parting_note() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<task-notification>\n<task-id>ad13e524</task-id>\n<status>completed</status>\n<summary>Agent &quot;bench linkage&quot; finished</summary>\n<result>the build query is running in the background</result>\n<usage><subagent_tokens>83901</subagent_tokens></usage>\n</task-notification>"}}"#,
        ]);
        assert_eq!(
            chat.turns[0].text,
            "Agent \"bench linkage\" finished\nthe build query is running in the background"
        );
    }

    /// The shapes Claude Code writes for a message that is not the person's,
    /// with dummy text. A peer `user` entry: a subagent handing back.
    const PEER_USER: &str = r#"{"type":"user","timestamp":"t1","isMeta":true,"promptSource":"system","turnOrigin":"peer","origin":{"kind":"peer","from":"a0000000000000001","name":"general-purpose","handback":true,"body":"x"},"message":{"role":"user","content":"Another Claude session sent a message:\n<agent-message from=\"a0000000000000001\">\n[Subagent hand-back] A subagent you launched has finished. The report follows:\n  dummy report\n    with an indented line\n</agent-message>\n\nThat \"other Claude session\" is a dummy trailer."}}"#;

    /// A subagent's report arriving as a `user` entry is not the person, and
    /// reads as the report: no preamble, no tags, no frame, no trailer.
    #[test]
    fn a_peer_message_is_an_agent_message_without_its_envelope() {
        let chat = sink_claude(&[PEER_USER]);
        assert_eq!(chat.turns.len(), 1);
        let turn = &chat.turns[0];
        assert_eq!(
            (turn.role.as_ref(), turn.kind.as_ref()),
            ("system", AGENT_MESSAGE)
        );
        assert_eq!(turn.text, "dummy report\n  with an indented line");
        assert_eq!(turn.from.as_deref(), Some("general-purpose"));
        assert_eq!(turn.agent.as_deref(), Some("a0000000000000001"));
    }

    /// A transcript older than `origin` is told by its shape, and the sender's
    /// id comes off the tag.
    #[test]
    fn an_agent_message_without_an_origin_is_told_by_its_shape() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"Another Claude session sent a message:\n<agent-message from=\"a0000000000000003\">\nplain dummy note\n</agent-message>"}}"#,
        ]);
        assert_eq!(chat.turns[0].kind, AGENT_MESSAGE);
        assert_eq!(chat.turns[0].text, "plain dummy note");
        assert_eq!(chat.turns[0].agent.as_deref(), Some("a0000000000000003"));
        assert_eq!(chat.turns[0].from, None);
    }

    /// What the person types while the agent works is recorded only as a
    /// queued attachment. It was dropped, so the conversation lost it.
    #[test]
    fn a_message_typed_mid_turn_is_the_persons() {
        let chat = sink_claude(&[
            r#"{"type":"attachment","timestamp":"t1","renderedRole":"system","attachment":{"type":"queued_command","commandMode":"prompt","humanTurn":true,"origin":{"kind":"human"},"prompt":"dummy typed text"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[0].kind, "message");
        assert_eq!(chat.turns[0].ts, "t1");
        assert_eq!(chat.turns[0].text, "dummy typed text");
    }

    /// An image typed with the message makes `prompt` a list of blocks; the
    /// words are kept as for a `user` entry.
    #[test]
    fn a_queued_message_with_blocks_keeps_its_words() {
        let chat = sink_claude(&[
            r#"{"type":"attachment","timestamp":"t1","attachment":{"type":"queued_command","origin":{"kind":"human"},"prompt":[{"type":"text","text":"look at this"},{"type":"image","source":{"type":"base64","data":"AAAA"}},{"type":"text","text":"and this"}]}}"#,
        ]);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[0].text, "look at this\n\nand this");
    }

    /// A peer or a coordinator writing in mid-turn is another agent, however
    /// the attachment's other fields read.
    #[test]
    fn a_queued_peer_or_coordinator_message_is_an_agent_message() {
        let chat = sink_claude(&[
            r#"{"type":"attachment","timestamp":"t1","renderedRole":"system","attachment":{"type":"queued_command","commandMode":"prompt","isMeta":true,"origin":{"kind":"peer","from":"a0000000000000002","name":"Plan","handback":true,"body":"x"},"prompt":"<agent-message from=\"a0000000000000002\">\n[Subagent hand-back] frame. The report follows:\n  dummy plan\n</agent-message>"}}"#,
            r#"{"type":"attachment","timestamp":"t2","attachment":{"type":"queued_command","isMeta":true,"origin":{"kind":"coordinator"},"prompt":"dummy instruction from the orchestrator"}}"#,
        ]);
        let shown: Vec<(&str, &str, Option<&str>)> = chat
            .turns
            .iter()
            .map(|t| (t.kind.as_ref(), t.text.as_str(), t.from.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                (AGENT_MESSAGE, "dummy plan", Some("Plan")),
                (
                    AGENT_MESSAGE,
                    "dummy instruction from the orchestrator",
                    None
                ),
            ]
        );
        assert!(chat.turns.iter().all(|t| t.role == "system"));
    }

    /// A queued task notification is the harness's, with an `origin` or
    /// without, and in the plain-text form as well as the tagged one.
    #[test]
    fn a_queued_task_notification_is_the_harness() {
        let chat = sink_claude(&[
            r#"{"type":"attachment","timestamp":"t1","attachment":{"type":"queued_command","commandMode":"task-notification","origin":{"kind":"task-notification","producer":"session-task"},"prompt":"<task-notification>\n<task-id>x</task-id>\n<summary>dummy finished</summary>\n</task-notification>"}}"#,
            r#"{"type":"attachment","timestamp":"t2","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"<task-notification>\n<task-id>y</task-id>\n<summary>dummy older</summary>\n</task-notification>"}}"#,
            r#"{"type":"attachment","timestamp":"t3","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"Background agent \"dummy\" was stopped by the user."}}"#,
            r#"{"type":"user","timestamp":"t4","promptSource":"system","turnOrigin":"task_notification","origin":{"kind":"task-notification"},"message":{"role":"user","content":"Background agent \"dummy\" was stopped by the user."}}"#,
        ]);
        let shown: Vec<(&str, &str)> = chat
            .turns
            .iter()
            .map(|t| (t.role.as_ref(), t.text.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("system", "dummy finished"),
                ("system", "dummy older"),
                (
                    "system",
                    "Background agent \"dummy\" was stopped by the user."
                ),
                (
                    "system",
                    "Background agent \"dummy\" was stopped by the user."
                ),
            ]
        );
        assert!(chat.turns.iter().all(|t| t.kind == "message"));
    }

    /// An `origin` this does not know is not the person: the safe mistake is
    /// dimming a line, not putting words in someone's mouth.
    #[test]
    fn an_unknown_origin_is_never_the_person() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","origin":{"kind":"something-new"},"message":{"content":"dummy machine text"}}"#,
        ]);
        assert_eq!(chat.turns[0].role, "system");
    }

    /// A queued message on a sidechain is a subagent's, like every sidechain
    /// record, and other attachments are not messages at all.
    #[test]
    fn sidechain_and_other_attachments_are_skipped() {
        let chat = sink_claude(&[
            r#"{"type":"attachment","timestamp":"t1","isSidechain":true,"attachment":{"type":"queued_command","origin":{"kind":"human"},"prompt":"a subagent's queue"}}"#,
            r#"{"type":"attachment","timestamp":"t2","attachment":{"type":"file","content":"dummy file"}}"#,
        ]);
        assert!(chat.turns.is_empty(), "{:?}", chat.turns);
    }

    /// A session with two background subagents and one foreground one, in the
    /// layout Claude Code writes: the main transcript, and each agent's own
    /// file beside a sidecar naming the call that started it. B finishes
    /// first; A's hand-back arrives mid-turn. Dummy text throughout.
    fn two_agents() -> (tempfile::TempDir, Session) {
        let dir = tempfile::tempdir().expect("tempdir");
        let main = dir.path().join("s.jsonl");
        let sub = dir.path().join("s").join("subagents");
        std::fs::create_dir_all(&sub).expect("subagents dir");
        let assistant = |req: &str, ts: &str, content: &str| {
            format!(
                r#"{{"type":"assistant","timestamp":"2026-10-08T10:{ts}.000Z","requestId":"{req}","message":{{"id":"m_{req}","role":"assistant","model":"claude-opus-5","content":[{content}],"usage":{{"input_tokens":100,"output_tokens":5}}}}}}"#
            )
        };
        let call = |id: &str, kind: &str, what: &str, background: bool| {
            format!(
                r#"{{"type":"tool_use","id":"{id}","name":"Agent","input":{{"description":"{what}","subagent_type":"{kind}","prompt":"dummy brief for {what}","run_in_background":{background}}}}}"#
            )
        };
        let result = |ts: &str, id: &str, text: &str| {
            format!(
                r#"{{"type":"user","timestamp":"2026-10-08T10:{ts}.000Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"{text}"}}]}}}}"#
            )
        };
        let handback = |from: &str, text: &str| {
            format!(
                r#"Another Claude session sent a message:\n<agent-message from=\"{from}\">\n[Subagent hand-back] frame. The report follows:\n  {text}\n</agent-message>\n\nDummy trailer."#
            )
        };
        let lines = [
            r#"{"type":"user","timestamp":"2026-10-08T10:00:00.000Z","origin":{"kind":"human"},"message":{"role":"user","content":"dummy ask"}}"#.to_string(),
            assistant(
                "r1",
                "01:00",
                &format!(
                    "{},{}",
                    call("toolu_a", "Explore", "map the parser", true),
                    call("toolu_b", "general-purpose", "check the tests", true)
                ),
            ),
            result("01:01", "toolu_a", "Async agent launched successfully.\\nagentId: aaaa0001"),
            result("01:02", "toolu_b", "Async agent launched successfully.\\nagentId: bbbb0002"),
            assistant("r2", "02:00", r#"{"type":"text","text":"both are running"}"#),
            format!(
                r#"{{"type":"user","timestamp":"2026-10-08T10:05:00.000Z","isMeta":true,"origin":{{"kind":"peer","from":"bbbb0002","name":"general-purpose"}},"message":{{"role":"user","content":"{}"}}}}"#,
                handback("bbbb0002", "dummy report from B")
            ),
            assistant("r3", "06:00", r#"{"type":"text","text":"B is done"}"#),
            format!(
                r#"{{"type":"attachment","timestamp":"2026-10-08T10:07:00.000Z","attachment":{{"type":"queued_command","isMeta":true,"origin":{{"kind":"peer","from":"aaaa0001","name":"Explore"}},"prompt":"{}"}}}}"#,
                handback("aaaa0001", "dummy report from A")
            ),
            r#"{"type":"attachment","timestamp":"2026-10-08T10:07:30.000Z","attachment":{"type":"queued_command","commandMode":"task-notification","origin":{"kind":"task-notification"},"prompt":"<task-notification>\n<task-id>aaaa0001</task-id>\n<summary>Agent finished</summary>\n</task-notification>"}}"#.to_string(),
            assistant("r4", "08:00", &call("toolu_c", "Plan", "plan it", false)),
            result("09:00", "toolu_c", "dummy foreground report"),
            r#"{"type":"user","timestamp":"2026-10-08T10:09:30.000Z","isSidechain":true,"message":{"role":"user","content":"a legacy interleaved subagent line"}}"#.to_string(),
        ];
        std::fs::write(&main, lines.join("\n") + "\n").expect("main transcript");
        for (id, tool, kind, what, ts) in [
            (
                "aaaa0001",
                "toolu_a",
                "Explore",
                "map the parser",
                ["01:10", "03:00"],
            ),
            (
                "bbbb0002",
                "toolu_b",
                "general-purpose",
                "check the tests",
                ["01:20", "02:30"],
            ),
            ("cccc0003", "toolu_c", "Plan", "plan it", ["08:10", "08:50"]),
        ] {
            let body = [
                format!(
                    r#"{{"type":"user","isSidechain":true,"timestamp":"2026-10-08T10:{}.000Z","message":{{"role":"user","content":"dummy brief for {what}"}}}}"#,
                    ts[0]
                ),
                assistant(
                    &format!("{id}-r"),
                    ts[1],
                    &format!(r#"{{"type":"text","text":"{id} working"}}"#),
                )
                .replacen(
                    r#"{"type":"assistant","#,
                    r#"{"type":"assistant","isSidechain":true,"#,
                    1,
                ),
            ];
            std::fs::write(
                sub.join(format!("agent-{id}.jsonl")),
                body.join("\n") + "\n",
            )
            .expect("agent transcript");
            std::fs::write(
                sub.join(format!("agent-{id}.meta.json")),
                format!(r#"{{"agentType":"{kind}","description":"{what}","toolUseId":"{tool}"}}"#),
            )
            .expect("sidecar");
        }
        let mut session = Session::new(Provider::Claude, "s".into());
        session.subagents = crate::session::claude::extract(&main).subagents;
        session.data_file = Some(main);
        (dir, session)
    }

    /// Each `Agent` call carries its own subagent, A and B are not swapped,
    /// and a background call's report is its hand-back while a foreground
    /// call's is its result.
    #[test]
    fn each_agent_call_is_joined_to_the_subagent_it_started() {
        let (_dir, session) = two_agents();
        let chat = build(&session, None);
        let calls: Vec<&ToolUse> = chat.turns.iter().flat_map(|t| &t.tools).collect();
        let joined: Vec<(&str, &str, &str, bool, Option<&str>)> = calls
            .iter()
            .map(|t| {
                let a = t.agent.as_ref().expect("every call here started an agent");
                (
                    t.id.as_str(),
                    a.id.as_str(),
                    a.agent_type.as_str(),
                    a.background,
                    a.report.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            joined,
            [
                (
                    "toolu_a",
                    "agent-aaaa0001",
                    "Explore",
                    true,
                    Some("dummy report from A")
                ),
                (
                    "toolu_b",
                    "agent-bbbb0002",
                    "general-purpose",
                    true,
                    Some("dummy report from B")
                ),
                (
                    "toolu_c",
                    "agent-cccc0003",
                    "Plan",
                    false,
                    Some("dummy foreground report")
                ),
            ]
        );

        // The hand-backs and the notification name their agent by the same id,
        // and the call points at its hand-back.
        let about: Vec<(&str, &str)> = chat
            .turns
            .iter()
            .filter_map(|t| Some((t.kind.as_ref(), t.agent.as_deref()?)))
            .collect();
        assert_eq!(
            about,
            [
                (AGENT_MESSAGE, "agent-bbbb0002"),
                (AGENT_MESSAGE, "agent-aaaa0001"),
                ("message", "agent-aaaa0001"),
            ]
        );
        let a = calls[0].agent.as_ref().unwrap();
        let handback = chat
            .turns
            .iter()
            .find(|t| Some(t.seq) == a.handback)
            .unwrap();
        assert_eq!(handback.text, "dummy report from A");

        // The legacy interleaved layout is still kept out.
        assert!(
            !chat
                .turns
                .iter()
                .any(|t| t.text.contains("legacy interleaved")),
            "{:?}",
            chat.turns
        );
    }

    /// A background agent that never handed back is shown with what it last
    /// said, marked as that; one that did keeps its hand-back. Every call
    /// carries its agent's turns, tokens and cost, and the turn count is the
    /// replies its own conversation draws.
    #[test]
    fn an_agent_with_no_handback_falls_back_to_its_last_message() {
        let (_dir, session) = two_agents();
        let main = session.data_file.clone().expect("main transcript");
        let kept: Vec<String> = std::fs::read_to_string(&main)
            .expect("read")
            .lines()
            .filter(|l| !l.contains("dummy report from A"))
            .map(str::to_string)
            .collect();
        std::fs::write(&main, kept.join("\n") + "\n").expect("rewrite");

        let chat = build(&session, None);
        let agents: Vec<&AgentCall> = chat
            .turns
            .iter()
            .flat_map(|t| &t.tools)
            .filter_map(|t| t.agent.as_ref())
            .collect();
        let shown: Vec<(&str, bool)> = agents
            .iter()
            .map(|a| (a.report.as_deref().unwrap_or(""), a.last_message))
            .collect();
        assert_eq!(
            shown,
            [
                ("aaaa0001 working", true),
                ("dummy report from B", false),
                ("dummy foreground report", false),
            ]
        );
        for a in &agents {
            let own = build_agent(&session, &a.id, None).expect("listed");
            let replies = own
                .turns
                .iter()
                .filter(|t| t.role == "assistant" && t.kind == "message")
                .count() as u64;
            assert_eq!(a.turns, replies, "{}", a.id);
            assert_eq!(a.tokens, 105, "{}", a.id);
            assert!(a.cost > 0.0, "{}", a.id);
        }
        assert!(
            agents[0].size().starts_with("1 turn"),
            "{}",
            agents[0].size()
        );
    }

    /// A subagent's own conversation is its file and nothing else, numbered
    /// from its own start; an id the session does not list is refused.
    #[test]
    fn an_agents_own_turns_are_read_from_its_own_file() {
        let (_dir, session) = two_agents();
        let a = build_agent(&session, "agent-aaaa0001", None).expect("A is listed");
        let texts: Vec<&str> = a.turns.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(
            texts,
            ["dummy brief for map the parser", "aaaa0001 working"]
        );
        assert_eq!(a.turns[0].seq, 0);
        assert!(a.stamp.is_some());

        for refused in [
            "agent-unknown",
            "",
            "../s",
            "subagents/agent-aaaa0001",
            "aaaa0001",
        ] {
            assert!(build_agent(&session, refused, None).is_none(), "{refused}");
        }
    }

    /// A subagent whose transcript was purged still answers, with a note.
    #[test]
    fn a_ghost_agent_answers_with_a_note() {
        let (_dir, mut session) = two_agents();
        session.subagents[0].ghost = true;
        let id = session.subagents[0].agent_id.clone();
        let chat = build_agent(&session, &id, None).expect("listed");
        assert!(!chat.supported);
        assert!(chat.note.unwrap().contains("gone"));
    }

    /// A notification carrying none of the fields a reader needs is not a
    /// turn at all — the field list itself was the bug, not the content.
    #[test]
    fn a_notification_with_no_news_is_no_turn() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<task-notification>\n<task-id>x</task-id>\n<output-file>/tmp/x.output</output-file>\n<status>completed</status>\n</task-notification>"}}"#,
            r#"{"type":"user","timestamp":"t2","message":{"content":"a real message"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "a real message");
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
        let kinds: Vec<&str> = chat.turns.iter().map(|t| t.kind.as_ref()).collect();
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
        let roles: Vec<&str> = chat.turns.iter().map(|t| t.role.as_ref()).collect();
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
        let chat = build(&session, None);
        assert!(!chat.supported);
        assert!(chat.note.is_some_and(|n| n.contains("Windsurf")));
    }

    fn sink_devin(steps: &[&str], statuses: &HashMap<String, String>) -> Conversation {
        let mut sink = Sink::default();
        for step in steps {
            sink.devin(&serde_json::from_str(step).unwrap(), statuses);
        }
        sink.finish()
    }

    /// One ATIF step is one model response: the message is the reply text,
    /// `reasoning_content` is the thinking that preceded it.
    #[test]
    fn a_devin_exchange_reads_the_same_way() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"user","timestamp":"t1","message":"fix the parser"}"#,
                r#"{"step_id":2,"source":"agent","timestamp":"t2","reasoning_content":"weighing it up","message":"on it"}"#,
            ],
            &HashMap::new(),
        );
        assert!(chat.supported);
        let roles: Vec<(&str, &str)> = chat
            .turns
            .iter()
            .map(|t| (t.role.as_ref(), t.kind.as_ref()))
            .collect();
        assert_eq!(
            roles,
            vec![
                ("user", "message"),
                ("assistant", "reasoning"),
                ("assistant", "message")
            ]
        );
        assert_eq!(chat.turns[0].text, "fix the parser");
        assert_eq!(chat.turns[1].text, "weighing it up");
        assert_eq!(chat.turns[2].text, "on it");
    }

    /// Devin records a call's result on the same step that made it, under
    /// `observation.results` keyed by `source_call_id` — the call and its
    /// outcome still come back as one thing.
    #[test]
    fn a_devin_result_lands_on_the_call_it_made() {
        let chat = sink_devin(
            &[
                r#"{"step_id":3,"source":"agent","timestamp":"t","message":"reading","tool_calls":[{"tool_call_id":"read_1#abc","function_name":"read","arguments":{"file_path":"/a/b.rs"}}],"observation":{"results":[{"source_call_id":"read_1#abc","content":"fn main() {}"}]}}"#,
            ],
            &HashMap::new(),
        );
        assert_eq!(chat.turns.len(), 1);
        let tool = &chat.turns[0].tools[0];
        assert_eq!(tool.name, "read");
        assert_eq!(tool.detail, "/a/b.rs");
        assert_eq!(tool.result.as_deref(), Some("fn main() {}"));
        assert!(!tool.failed);
    }

    /// Two steps are two replies even when neither made a call — the step id
    /// is what stops them being folded into one.
    #[test]
    fn consecutive_devin_steps_stay_separate_replies() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"agent","timestamp":"t1","message":"first"}"#,
                r#"{"step_id":2,"source":"agent","timestamp":"t2","message":"second"}"#,
            ],
            &HashMap::new(),
        );
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].text, "first");
        assert_eq!(chat.turns[1].text, "second");
    }

    /// The transcript does not say how a call ended; the database's status
    /// map does. A call whose last reported status is not `completed` failed.
    #[test]
    fn a_devin_call_the_database_marked_failed_is_marked() {
        let statuses = HashMap::from([("exec_1#abc".to_string(), "failed".to_string())]);
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"agent","timestamp":"t","tool_calls":[{"tool_call_id":"exec_1#abc","function_name":"exec","arguments":{"command":"cargo test"}}],"observation":{"results":[{"source_call_id":"exec_1#abc","content":"error: no such target"}]}}"#,
            ],
            &statuses,
        );
        let tool = &chat.turns[0].tools[0];
        assert!(tool.failed);
        assert_eq!(tool.result.as_deref(), Some("error: no such target"));
    }

    /// A call still running has an observation pending, so no result at all —
    /// which is what the page shows as in-flight.
    #[test]
    fn a_devin_call_with_no_observation_is_still_running() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"agent","timestamp":"t","tool_calls":[{"tool_call_id":"exec_1#abc","function_name":"exec","arguments":{"command":"sleep 60"}}]}"#,
            ],
            &HashMap::new(),
        );
        assert!(chat.turns[0].tools[0].result.is_none());
    }

    /// ATIF has no `structuredPatch`; the patch an `edit` applied is its own
    /// `old_string`/`new_string` arguments.
    #[test]
    fn a_devin_edit_carries_its_diff() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"agent","timestamp":"t","tool_calls":[{"tool_call_id":"edit_1#abc","function_name":"edit","arguments":{"file_path":"/a/b.rs","old_string":"old\nlines","new_string":"new\nalso"}}],"observation":{"results":[{"source_call_id":"edit_1#abc","content":"updated"}]}}"#,
            ],
            &HashMap::new(),
        );
        let tool = &chat.turns[0].tools[0];
        assert_eq!((tool.added, tool.removed), (2, 2));
        assert_eq!(tool.diff, vec!["-old", "-lines", "+new", "+also"]);
    }

    /// The system prompt, the rule files and the context blocks re-injected at
    /// every turn are all steps too, but they are the prompt being assembled,
    /// not the conversation. A step the harness injected mid-run — a
    /// backgrounded subagent finishing, the user editing a file — is one.
    #[test]
    fn devin_prompt_assembly_is_not_a_turn() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"system","timestamp":"t1","message":"You are Devin…","extra":{"telemetry":{"source":"sysprompt"}}}"#,
                r#"{"step_id":2,"source":"system","timestamp":"t2","message":"<rules>…</rules>","extra":{"telemetry":{"source":"rules"}}}"#,
                r#"{"step_id":3,"source":"system","timestamp":"t3","message":"<available_skills>…</available_skills>","extra":{"telemetry":{"source":"system"}}}"#,
                r#"{"step_id":4,"source":"system","timestamp":"t4","message":"You are powered by SWE-2 High.","extra":{"telemetry":{"source":"system"}}}"#,
                r#"{"step_id":5,"source":"system","timestamp":"t5","message":"<subagent_completion_notification>done</subagent_completion_notification>","extra":{"telemetry":{"source":"system"}}}"#,
            ],
            &HashMap::new(),
        );
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].role, "system");
        // The envelope comes off an event the same as any other harness block.
        assert_eq!(chat.turns[0].text, "done");
    }

    /// The user-edited-a-file block is aimed at the model: a preamble about
    /// when to mention it and a pseudo-diff per file. The turn is the event.
    #[test]
    fn a_devin_user_action_reads_as_the_files_touched() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"system","timestamp":"t1","message":"<additional_metadata>\nThe user took the following actions after the last message. ONLY talk about this if it is directly relevant to the user's next request.\n\n<user_actions>\nThe following changes were made by the USER to: /home/flo/src/a.rs.\n[diff_block_start]\n@@ -1 +1 @@\n-old\n+new\n[diff_block_end]\nPlease note that the above snippet only shows the MODIFIED lines.\nThe following changes were made by the USER to: /home/flo/src/b.rs.\n[diff_block_start]\n@@ -2 +2 @@\n-x\n+y\n[diff_block_end]\n</user_actions>\n</additional_metadata>","extra":{"telemetry":{"source":"system"}}}"#,
            ],
            &HashMap::new(),
        );
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(
            chat.turns[0].text,
            "the user edited /home/flo/src/a.rs, /home/flo/src/b.rs"
        );
    }

    /// A transcript that is not the document ATIF says it is reports as
    /// unreadable rather than panicking or looking empty.
    #[test]
    fn a_devin_transcript_that_is_not_json_is_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s1.json");
        std::fs::write(&path, b"not json").unwrap();
        let mut session = Session::new(Provider::Devin, "s1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        assert!(!chat.supported);
        assert!(chat.note.is_some_and(|n| n.contains("could not read")));
    }

    /// The page's window and the export read the same file to different
    /// limits: the page gets the tail with each message cut, the export every
    /// turn whole — a transcript handed to someone else that began at turn 10
    /// or stopped a message mid-sentence would be a wrong one.
    #[test]
    fn the_whole_read_keeps_what_the_page_window_drops() {
        let long = "y".repeat(MAX_TEXT_CHARS + 50);
        let mut lines: Vec<String> = (0..MAX_TURNS + 10)
            .map(|i| {
                format!(
                    r#"{{"type":"user","timestamp":"t","message":{{"content":"message {i}"}}}}"#
                )
            })
            .collect();
        lines.push(format!(
            r#"{{"type":"user","timestamp":"t","message":{{"content":"{long}"}}}}"#
        ));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s1.jsonl");
        std::fs::write(&path, lines.join("\n")).unwrap();
        let mut session = Session::new(Provider::Claude, "s1".into());
        session.data_file = Some(path);

        let page = build(&session, None);
        assert_eq!(page.turns.len(), MAX_TURNS);
        assert!(page.turns.last().unwrap().clipped);

        let all = whole(&session);
        assert_eq!(all.earlier, 0);
        assert_eq!(all.turns.len(), MAX_TURNS + 11);
        assert_eq!(all.turns[0].text, "message 0");
        assert_eq!(all.turns.last().unwrap().text, long);
        assert!(!all.turns.last().unwrap().clipped);
    }

    /// A harness name is the one place a label is read inside a sentence, and
    /// "a OpenCode conversation" would be the sentence that carried it.
    #[test]
    fn an_article_follows_the_first_letter_of_the_harness() {
        assert_eq!(article("OpenCode"), "an");
        assert_eq!(article("Claude Code"), "a");
        assert_eq!(article("Codex"), "a");
    }

    /// A provider with no reader says which one it is, rather than coming back
    /// empty and reading like a session that said nothing.
    #[test]
    fn a_provider_with_no_reader_names_itself() {
        let mut session = Session::new(Provider::Windsurf, "s1".into());
        session.data_file = Some(std::path::PathBuf::from("/nonexistent"));
        let chat = build(&session, None);
        assert!(!chat.supported);
        let note = chat.note.expect("a reason is attached");
        assert!(
            note.starts_with("cctop cannot read a Windsurf conversation yet"),
            "{note}"
        );
    }

    /// A turn's `seq` is its place in the whole transcript, not its position
    /// in the window — what `?before=` and the page's turn links count in.
    #[test]
    fn turns_kept_off_the_front_keep_their_sequence_numbers() {
        let mut sink = Sink::default();
        for i in 0..MAX_TURNS + 3 {
            let mut turn = Turn::new("user", "message", "t");
            turn.text = format!("turn {i}");
            sink.push(turn);
        }
        let chat = sink.finish();
        assert_eq!(chat.turns.len(), MAX_TURNS);
        assert_eq!(chat.earlier, 3);
        assert_eq!(chat.turns[0].seq, 3);
        assert_eq!(chat.turns[0].text, "turn 3");
    }

    /// `?before=` ends the window at the boundary rather than the newest
    /// turn, same size and same `earlier` count as the unwindowed one.
    #[test]
    fn a_before_window_ends_where_it_was_asked_to() {
        let mut sink = Sink {
            before: Some(MAX_TURNS + 2),
            ..Sink::default()
        };
        for i in 0..MAX_TURNS + 5 {
            let mut turn = Turn::new("user", "message", "t");
            turn.text = format!("turn {i}");
            sink.push(turn);
        }
        let chat = sink.finish();
        // The three turns past the boundary were numbered — `earlier` and the
        // next window's seqs depend on it — but never kept.
        assert_eq!(chat.turns.len(), MAX_TURNS);
        assert_eq!(chat.earlier, 2);
        assert_eq!(chat.turns[0].seq, 2);
        assert_eq!(chat.turns.last().unwrap().seq, MAX_TURNS + 1);
    }

    /// A result can be written entries after the window's boundary; the call
    /// it answers still has to show as finished.
    #[test]
    fn a_result_past_the_boundary_still_lands_on_its_call() {
        let mut sink = Sink {
            before: Some(1),
            ..Sink::default()
        };
        for line in [
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a"}}]}}"#,
            // Turn 1 — past the boundary, numbered but not kept.
            r#"{"type":"user","timestamp":"t2","message":{"content":"next question"}}"#,
            // The call's result, recorded last of all.
            r#"{"type":"user","timestamp":"t3","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"done"}]}}"#,
        ] {
            sink.claude(&serde_json::from_str(line).unwrap());
        }
        let chat = sink.finish();
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].seq, 0);
        assert_eq!(chat.turns[0].tools[0].result.as_deref(), Some("done"));
    }
}
