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
use crate::session::claude_line::{self, Block, Line, Origin};
use crate::session::codex_line;
use crate::session::lenient::{Raw, Shape, Str};
use crate::session::{Delta, Session, devin, extract, gemini, opencode, pi};
use crate::util;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::path::Path;

mod sink;
mod tidy;

use sink::*;
pub use tidy::*;

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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
    /// `text` rendered from markdown to HTML, safe to set as markup: filled
    /// by the web server for the page it answers, and absent everywhere else —
    /// the TUI, an export and a peer's `--chat` all read `text`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub html: Option<String>,
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
            html: None,
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
    /// `report` rendered to HTML, as [`Turn::html`] is.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub report_html: Option<String>,
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
        Provider::Claude => claude_line::for_each(path, |item| sink.claude(item)),
        Provider::Codex => codex_line::for_each(path, |item| sink.codex(item)),
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
    if let Err(e) = claude_line::for_each(&path, |item| sink.claude(item)) {
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

#[cfg(test)]
fn sink_claude(lines: &[&str]) -> Conversation {
    let mut sink = Sink::default();
    for line in lines {
        sink.claude(&Line::parse(line.as_bytes()).unwrap());
    }
    sink.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_provider_with_no_reader_says_so_instead_of_looking_empty() {
        let mut session = Session::new(Provider::Windsurf, "s1".into());
        session.data_file = Some(std::path::PathBuf::from("/nonexistent"));
        let chat = build(&session, None);
        assert!(!chat.supported);
        assert!(chat.note.is_some_and(|n| n.contains("Windsurf")));
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
}
