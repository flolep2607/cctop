//! Hand one session's context to a different agent.
//!
//! Every harness stores its transcript in its own shape, and none of them can
//! read another's. But cctop already parses all seven into one [`SessionData`],
//! so the material for a handoff is sitting there normalised: what the session
//! was for, which files it changed, what it ran, what plan it was working to.
//! This turns that into a markdown brief any agent can be pointed at.
//!
//! A brief is deliberately *not* a replayed transcript. Pushing a whole
//! conversation into a fresh window spends the context it is supposed to save,
//! and most of what it spends it on — tool output, file contents the new agent
//! can read itself — is the part the receiving agent should gather first-hand
//! anyway. What does not survive a restart, and so is worth carrying, is the
//! intent: the task, the decisions, the shape of the work so far.
//!
//! That intent lives in the words, though, and for a long time none of them
//! made it across. The brief listed what a session *touched* — files, commands,
//! searches — and said nothing about what it was *for*, so an agent handed one
//! knew where the work had been and not what anybody had asked for. The two
//! sections that fix it are [`Brief::to_markdown`]'s first: the user's own
//! prompts, and where the last turn left off.
//!
//! Everything else that was said goes beside the brief rather than into it, as
//! the JSONL [`write`] leaves next to the markdown — every turn, its reasoning
//! where the harness recorded any, and each tool call with its result. A brief
//! is read in full by an agent that has just started, so it stays short; the
//! record is there for the question the summary does not answer, and costs
//! nothing until something asks it. Reading a conversation is
//! [`crate::serve::chat`]'s job, which is why this calls it rather than
//! learning the transcripts again — and why it carries what that can read:
//! Claude Code and Codex. A session on any other harness still gets the brief
//! it always got.
//!
//! Between two Claudes none of that applies, because the limit it works around
//! is not there: the receiving agent reads exactly the format the sending one
//! wrote. That handoff copies the transcript instead — see [`fork`] — and the
//! brief is what every other agent gets.
//!
//! ponytail: the plan section only fills in for harnesses that record one as a
//! tool call (Claude's `TodoWrite`, Codex's `update_plan`). A session that kept
//! its plan in prose leaves it empty rather than guessing at which assistant
//! paragraph was the plan.

use crate::serve::chat;
use crate::session::{EDIT_TOOLS, Session, SessionData, ToolDetail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many entries each list in the brief is allowed to carry.
///
/// A brief is read by an agent with a budget, so every section is bounded. The
/// caps differ because the sections do: a file list is the spine of the handoff
/// and worth its length, while the hundredth `Read` is noise.
const MAX_FILES: usize = 40;
const MAX_COMMANDS: usize = 25;
const MAX_READS: usize = 25;
const MAX_SEARCHES: usize = 15;
const MAX_SUBAGENTS: usize = 15;

/// How much of the conversation the brief itself quotes.
///
/// Deliberately mean next to the JSONL beside it, which is bounded only by what
/// [`crate::serve::chat`] will read. These are the two questions a receiving
/// agent has before it can do anything — what was asked, and where it got to —
/// and answering them costs a few hundred tokens rather than a window.
///
/// Prompts are quoted at length because a prompt is the one thing in a brief
/// nothing else can restate: it is what the user actually said, and half of it
/// is a different instruction. The closing turns are cut harder — they are
/// context for what to do next, not the instruction itself.
const MAX_PROMPTS: usize = 20;
const MAX_PROMPT_CHARS: usize = 1200;
const MAX_CLOSING: usize = 2;
const MAX_CLOSING_CHARS: usize = 1500;

/// A file the session changed, and by how much.
#[derive(Debug, Clone)]
pub struct FileTouch {
    pub path: String,
    pub edits: u64,
    pub added: u64,
    pub removed: u64,
}

/// The portable form of a session: everything a different agent would need to
/// pick the work up, and nothing it can read off disk itself.
#[derive(Debug, Clone, Default)]
pub struct Brief {
    pub source: String,
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub model: String,
    pub started_at: String,
    pub last_active: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: Option<f64>,
    /// The most recent plan the session recorded, one entry per step.
    pub plan: Vec<String>,
    pub files: Vec<FileTouch>,
    pub reads: Vec<String>,
    pub commands: Vec<String>,
    pub searches: Vec<String>,
    /// Delegated work: agent type and what it was asked to do.
    pub subagents: Vec<(String, String)>,
    /// Window occupancy at the last measured request, when the harness reports it.
    pub context: Option<(u64, u64)>,
    /// What was said, where the harness's transcript can be read.
    ///
    /// Carried whole rather than reduced to the few lines the markdown quotes,
    /// because [`write`] spends it twice: the brief takes the prompts and the
    /// last turn, and the JSONL beside it takes everything.
    pub chat: Option<chat::Conversation>,
}

const READ_TOOLS: &[&str] = &["Read", "read", "view"];
const SHELL_TOOLS: &[&str] = &["Bash", "bash", "shell", "run_terminal_cmd"];
const PLAN_TOOLS: &[&str] = &["TodoWrite", "update_plan", "ExitPlanMode", "todo_write"];

/// Build the brief for `session`, using `data` when extraction has finished.
///
/// Works from a `None` `data` too: the header alone — task, directory, branch,
/// harness — is already a usable handoff for a session whose transcript has not
/// been read yet, and refusing to produce one would make the feature depend on
/// cache warmth.
pub fn build(session: &Session, data: Option<&SessionData>) -> Brief {
    let mut brief = Brief {
        source: harness_label(session),
        session_id: session.session_id.clone(),
        title: session
            .title
            .clone()
            .or_else(|| data.and_then(|d| d.title.clone()))
            .unwrap_or_else(|| "(untitled session)".into()),
        cwd: session.label_source.clone(),
        branch: crate::ui::columns::branch_of(session),
        model: match session.model.is_empty() {
            true => data.map(|d| d.last_model.clone()).unwrap_or_default(),
            false => session.model.clone(),
        },
        started_at: session.started_at.clone(),
        last_active: session.last_active.clone(),
        input_tokens: session.input_tokens,
        output_tokens: session.output_tokens,
        cost: session.total_cost,
        context: session.context.map(|c| (c.used, c.max)),
        ..Brief::default()
    };

    // Read before the early return: what was said comes off the transcript
    // directly, so it is there for a session whose extraction has not finished
    // — which is the case that most needs it, a brief taken of a session that
    // is still running.
    brief.chat = conversation(session);

    let Some(data) = data else { return brief };
    let details = &data.metrics.tool_details;

    brief.plan = latest_plan(details);
    brief.files = touched_files(details);
    brief.reads = recent(details, READ_TOOLS, MAX_READS);
    brief.commands = recent(details, SHELL_TOOLS, MAX_COMMANDS);

    // The receiving agent is started in this same directory, so an absolute
    // path spends characters restating where it already is — and a repo-relative
    // path is what it will be typing back at its own tools anyway.
    let root = brief.cwd.trim_end_matches('/').to_string();
    if !root.is_empty() {
        for f in brief.files.iter_mut() {
            f.path = relative_to(&f.path, &root);
        }
        for r in brief.reads.iter_mut() {
            *r = relative_to(r, &root);
        }
    }

    brief.searches = data
        .metrics
        .web_searches
        .iter()
        .chain(data.metrics.web_fetches.iter())
        .take(MAX_SEARCHES)
        .cloned()
        .collect();

    brief.subagents = data
        .subagents
        .iter()
        .rev()
        .take(MAX_SUBAGENTS)
        .map(|s| (s.agent_type.clone(), s.description.clone()))
        .collect();

    brief
}

/// `path` with `root` stripped, when it is under it.
///
/// Left alone otherwise: a file edited outside the session's own directory is
/// genuinely elsewhere, and a `../../` chain would say that less clearly than
/// the absolute path does.
fn relative_to(path: &str, root: &str) -> String {
    match path
        .strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
    {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => path.to_string(),
    }
}

/// What to call the session's harness in the brief.
///
/// The receiving agent has no way to look this up, and "the last plan Codex
/// made" reads differently from "the last plan Claude made" — so the label is
/// the host application when one was inferred, and the provider otherwise.
fn harness_label(session: &Session) -> String {
    match session.harness.is_empty() || session.harness == "─" {
        true => session.provider.as_str().to_string(),
        false => session.harness.clone(),
    }
}

/// The most recent recorded plan, as one string per step.
///
/// Plans are rewritten in place — every `TodoWrite` supersedes the last — so
/// only the newest call carries the live plan, and the ones before it are
/// history the receiving agent does not need.
fn latest_plan(details: &ToolDetails) -> Vec<String> {
    let mut newest: Option<&ToolDetail> = None;
    for name in PLAN_TOOLS {
        let Some(list) = details.get(*name) else {
            continue;
        };
        for d in list {
            if newest.is_none_or(|best| best.ts < d.ts) {
                newest = Some(d);
            }
        }
    }
    let d = match newest {
        Some(d) => d,
        None => return Vec::new(),
    };
    // `full` holds every step; `d` is the one-line summary the panel shows, and
    // is only worth falling back to when there was nothing longer to keep.
    d.full
        .as_deref()
        .unwrap_or(&d.d)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// Alias so the signatures below read as what they are rather than as the
/// underlying map type spelled out four times.
type ToolDetails = std::collections::HashMap<String, Vec<ToolDetail>>;

/// Files the session modified, most-edited first.
///
/// Edit counts come from the tool details, which name their target file; the
/// line counts are the session's totals and cannot be split per file, so they
/// are reported once on the section instead of invented per row.
fn touched_files(details: &ToolDetails) -> Vec<FileTouch> {
    let mut counts: BTreeMap<String, FileTouch> = BTreeMap::new();
    for name in EDIT_TOOLS {
        let Some(list) = details.get(*name) else {
            continue;
        };
        for d in list {
            let path = d.d.trim();
            if path.is_empty() {
                continue;
            }
            let entry = counts.entry(path.to_string()).or_insert_with(|| FileTouch {
                path: path.to_string(),
                edits: 0,
                added: 0,
                removed: 0,
            });
            entry.edits += 1;
            if let Some(delta) = &d.delta {
                entry.added += u64::from(delta.added);
                entry.removed += u64::from(delta.removed);
            }
        }
    }
    let mut files: Vec<FileTouch> = counts.into_values().collect();
    files.sort_by(|a, b| b.edits.cmp(&a.edits).then_with(|| a.path.cmp(&b.path)));
    files.truncate(MAX_FILES);
    files
}

/// The newest `limit` details across `tools`, deduplicated, newest first.
fn recent(details: &ToolDetails, tools: &[&str], limit: usize) -> Vec<String> {
    let mut all: Vec<&ToolDetail> = tools
        .iter()
        .filter_map(|name| details.get(*name))
        .flatten()
        .collect();
    all.sort_by(|a, b| b.ts.cmp(&a.ts));

    let mut seen = std::collections::HashSet::new();
    all.into_iter()
        .map(|d| d.d.trim())
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.to_string()))
        .take(limit)
        .map(str::to_string)
        .collect()
}

/// The session's conversation, or `None` where there is nothing usable.
///
/// An unsupported harness and an empty transcript are the same answer here:
/// the brief has no conversation to show and says nothing rather than
/// explaining an absence the receiving agent can do nothing about. The note
/// [`chat::build`] attaches is written for the report's reader, who is looking
/// at the session; an agent picking work up is not.
fn conversation(session: &Session) -> Option<chat::Conversation> {
    let chat = chat::build(session);
    (chat.supported && !chat.turns.is_empty()).then_some(chat)
}

/// The user's own prompts, oldest first.
///
/// `message` only: a `compaction` is the harness summarising itself and a
/// `reasoning` turn is never the user's, and neither is something anybody
/// asked for.
fn prompts(chat: &chat::Conversation) -> Vec<String> {
    let all: Vec<&chat::Turn> = chat
        .turns
        .iter()
        .filter(|t| t.role == "user" && t.kind == "message")
        .filter(|t| !t.text.trim().is_empty())
        .collect();
    // The newest kept, but shown in the order they were said: a brief that
    // dropped the *recent* prompts to keep the opening one would carry the task
    // as it was first described rather than as it currently stands.
    all.iter()
        .skip(all.len().saturating_sub(MAX_PROMPTS))
        .map(|t| clip(&t.text, MAX_PROMPT_CHARS))
        .collect()
}

/// The last thing the agent said, which is where the work stands.
fn closing(chat: &chat::Conversation) -> Vec<String> {
    let all: Vec<&chat::Turn> = chat
        .turns
        .iter()
        .filter(|t| t.role == "assistant" && t.kind == "message")
        .filter(|t| !t.text.trim().is_empty())
        .collect();
    all.iter()
        .skip(all.len().saturating_sub(MAX_CLOSING))
        .map(|t| clip(&t.text, MAX_CLOSING_CHARS))
        .collect()
}

/// Cut to `max` characters, never mid-character, marking that it was cut.
///
/// The ellipsis is load-bearing in a document an agent reads as fact: a
/// half-quoted instruction that ends cleanly reads as the whole instruction.
fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let end = text
        .char_indices()
        .nth(max)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    format!("{}…", &text[..end].trim_end())
}

/// Quote `text` as markdown, so a prompt that was itself markdown — a list, a
/// fenced block — cannot be read as part of the brief's own structure.
fn quote(text: &str) -> String {
    text.lines()
        .map(|line| match line.trim().is_empty() {
            true => ">".to_string(),
            false => format!("> {line}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl Brief {
    /// Render the brief as the markdown a receiving agent is handed.
    ///
    /// Addressed to that agent rather than to a human reader: it opens by
    /// saying what the document is and where it came from, because an agent
    /// that finds an unexplained file of notes in its context will reasonably
    /// treat them as instructions from the user.
    pub fn to_markdown(&self) -> String {
        self.to_markdown_with(None)
    }

    /// The brief, optionally naming the JSONL record [`write`] left beside it.
    ///
    /// Separate from [`Brief::to_markdown`] because the record is a file, and a
    /// caller that only wants the text — [`crate::mcp`] answers a tool call
    /// with it — has no file to point at. Pointing at one that is not there
    /// would send the receiving agent looking for it.
    pub fn to_markdown_with(&self, record: Option<&Path>) -> String {
        let mut out = String::new();
        let p = &mut out;

        push(p, &format!("# Handoff — {}\n", self.title));
        push(
            p,
            "\nThis is a context brief written by cctop from another agent's session. \
             It is background, not instructions: read it to pick the work up, then \
             verify anything you rely on against the current state of the repository.\n",
        );

        push(p, "\n## Where this came from\n\n");
        field(p, "Harness", &self.source);
        field(p, "Model", &self.model);
        field(p, "Session", &self.session_id);
        field(p, "Directory", &crate::util::tildify(&self.cwd));
        if let Some(branch) = &self.branch {
            field(p, "Branch", branch);
        }
        if !self.started_at.is_empty() {
            field(p, "Started", &self.started_at);
        }
        if !self.last_active.is_empty() {
            field(p, "Last active", &self.last_active);
        }
        field(
            p,
            "Tokens",
            &format!(
                "{} in / {} out",
                crate::util::compact_tokens(self.input_tokens),
                crate::util::compact_tokens(self.output_tokens)
            ),
        );
        if let Some((used, max)) = self.context
            && max > 0
        {
            field(
                p,
                "Context at handoff",
                &format!(
                    "{} of {} ({}%)",
                    crate::util::compact_tokens(used),
                    crate::util::compact_tokens(max),
                    used * 100 / max
                ),
            );
        }
        if let Some(cost) = self.cost {
            field(p, "Estimated cost", &format!("${cost:.2}"));
        }

        let chat = self.chat.as_ref();
        let asked = chat.map(prompts).unwrap_or_default();
        if !asked.is_empty() {
            push(p, "\n## What it was asked\n\n");
            // Said outright, because a quoted block in a document an agent is
            // reading as background is exactly the shape of a thing it might
            // decide to act on.
            push(
                p,
                "The user's own words to the previous agent, oldest first. They are \
                 quoted as history, not addressed to you.\n\n",
            );
            for (i, ask) in asked.iter().enumerate() {
                if i > 0 {
                    push(p, "\n");
                }
                push(p, &format!("{}\n", quote(ask)));
            }
            if let Some(earlier) = chat.map(|c| c.earlier).filter(|n| *n > 0) {
                push(
                    p,
                    &format!("\n{earlier} earlier turns came before these and are not carried.\n"),
                );
            }
        }

        let left_off = chat.map(closing).unwrap_or_default();
        if !left_off.is_empty() {
            push(p, "\n## Where it left off\n\n");
            push(p, "The last thing the previous agent said:\n\n");
            for (i, said) in left_off.iter().enumerate() {
                if i > 0 {
                    push(p, "\n");
                }
                push(p, &format!("{}\n", quote(said)));
            }
        }

        if !self.plan.is_empty() {
            push(p, "\n## The plan it was working to\n\n");
            for step in &self.plan {
                push(p, &format!("- {step}\n"));
            }
        }

        if !self.files.is_empty() {
            push(p, "\n## Files it changed\n\n");
            for f in &self.files {
                let churn = match (f.added, f.removed) {
                    (0, 0) => String::new(),
                    (a, r) => format!(" (+{a}/-{r})"),
                };
                let times = match f.edits {
                    1 => String::new(),
                    n => format!(" ×{n}"),
                };
                push(p, &format!("- `{}`{times}{churn}\n", f.path));
            }
        }

        if !self.reads.is_empty() {
            push(p, "\n## Files it read\n\n");
            for r in &self.reads {
                push(p, &format!("- `{r}`\n"));
            }
        }

        if !self.commands.is_empty() {
            push(p, "\n## Commands it ran\n\n```\n");
            for c in &self.commands {
                push(p, &format!("{c}\n"));
            }
            push(p, "```\n");
        }

        if !self.subagents.is_empty() {
            push(p, "\n## Work it delegated\n\n");
            for (kind, desc) in &self.subagents {
                push(p, &format!("- **{kind}** — {desc}\n"));
            }
        }

        if !self.searches.is_empty() {
            push(p, "\n## What it looked up\n\n");
            for s in &self.searches {
                push(p, &format!("- {s}\n"));
            }
        }

        if let Some(record) = record {
            push(p, "\n## The conversation in full\n\n");
            push(
                p,
                &format!(
                    "`{}` — one JSON object per line, oldest first: the role and kind \
                     of each turn (`message`, `reasoning` where the harness recorded \
                     its thinking, `compaction` where it reclaimed its window), what \
                     was said, and every tool call with its argument, its result and \
                     its diff. Read it when the summary above leaves a decision \
                     unexplained; it is long, so read it for a question rather than \
                     from the top.\n",
                    record.display()
                ),
            );
        }

        push(
            p,
            "\n---\n\nWritten by cctop. The lists above are bounded — a long session \
             carries its most recent and most-touched entries, not all of them.\n",
        );
        out
    }

    /// The conversation as JSONL: a header line, then one line per turn.
    ///
    /// The header comes first so the file identifies itself to whatever opens
    /// it — a record of a conversation, with no provenance, found in a cache
    /// directory is a puzzle. Turns are [`chat::Turn`] serialised as they
    /// stand, which is the same shape the web view is served, rather than a
    /// second format to keep in step with it.
    pub fn to_jsonl(&self) -> String {
        let Some(chat) = self.chat.as_ref() else {
            return String::new();
        };
        let header = serde_json::json!({
            "type": "cctop-handoff-record",
            "session_id": self.session_id,
            "harness": self.source,
            "model": self.model,
            "title": self.title,
            "cwd": self.cwd,
            "branch": self.branch,
            "turns": chat.turns.len(),
            "earlier_turns": chat.earlier,
        });
        let mut out = String::new();
        // `flatten` drops a turn that will not serialise rather than aborting
        // the file: the rest of the conversation is still worth handing over.
        for line in std::iter::once(serde_json::to_string(&header))
            .chain(chat.turns.iter().map(serde_json::to_string))
            .flatten()
        {
            out.push_str(&line);
            out.push('\n');
        }
        out
    }

    /// A one-line summary for the status bar.
    pub fn summary(&self) -> String {
        format!(
            "{} · {} files · {} commands",
            self.source,
            self.files.len(),
            self.commands.len()
        )
    }
}

fn push(out: &mut String, text: &str) {
    out.push_str(text);
}

fn field(out: &mut String, name: &str, value: &str) {
    if !value.is_empty() {
        out.push_str(&format!("- **{name}:** {value}\n"));
    }
}

/// Where briefs are written.
///
/// Under the cache directory rather than beside the transcripts: a brief is a
/// derived artefact with no value once it has been read, and putting it in a
/// project directory would mean handing an agent a file its own repository is
/// about to want to gitignore.
pub fn dir() -> PathBuf {
    crate::config::CACHE_DIR.join("handoff")
}

/// Write the brief out and return the path an agent can be pointed at.
///
/// Named for the session rather than uniquely per call: writing a second brief
/// for the same session replaces the first, which is what re-handing-off a
/// session that has moved on should do.
///
/// Two files, where there is a conversation to record: `<id>.md` is the brief
/// and `<id>.jsonl` beside it is everything that was said. Only the markdown's
/// path is returned, because it is the only one the receiving agent is pointed
/// at — the brief names the record, and an agent that never needs it never
/// spends a token on it. A stale record from an earlier handoff is removed
/// rather than left, so the brief cannot name a file describing a conversation
/// that has since moved on.
pub fn write(brief: &Brief) -> std::io::Result<PathBuf> {
    write_in(&dir(), brief)
}

/// [`write`], against a directory named by the caller.
///
/// Split out so the pair of files it leaves — and the stale record it clears —
/// can be tested somewhere that is not the user's own cache.
fn write_in(dir: &Path, brief: &Brief) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let safe: String = brief
        .session_id
        .chars()
        .map(
            |c| match c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                true => c,
                false => '-',
            },
        )
        .collect();
    let path = dir.join(format!("{safe}.md"));

    let record = dir.join(format!("{safe}.jsonl"));
    let lines = brief.to_jsonl();
    let record = match lines.is_empty() {
        false => {
            std::fs::write(&record, lines)?;
            Some(record)
        }
        true => {
            let _ = std::fs::remove_file(&record);
            None
        }
    };

    std::fs::write(&path, brief.to_markdown_with(record.as_deref()))?;
    Ok(path)
}

/// The transcript a Claude session could be handed over whole, rather than as a
/// brief.
///
/// A brief exists because no harness can read another's transcripts. Claude to
/// Claude that limit is not there: the receiving agent reads exactly the format
/// the sending one wrote, so the conversation itself can be handed over — see
/// [`fork`] — and the summary is only worth writing when it cannot be.
///
/// `None` for everything that would not survive the copy: another harness,
/// another machine, and Claude for Mac, which keeps its conversations in a
/// directory of its own and resumes them by title rather than by id.
pub fn forkable(session: &Session) -> Option<&Path> {
    if session.provider != crate::pricing::Provider::Claude
        || session.surface.is_desktop()
        || session.remote.is_some()
    {
        return None;
    }
    let file = session.data_file.as_deref()?;
    // The parent is the project directory whose name Claude derives from the
    // working directory, and the copy has to land in one of those or the
    // resume will not find it.
    let named = file.extension().is_some_and(|e| e == "jsonl") && file.parent().is_some();
    named.then_some(file)
}

/// Copy `transcript` into `config_dir` under a new session id, and return it.
///
/// This is the Claude-to-Claude handoff: the receiving agent is resumed onto a
/// copy of the conversation, so it starts knowing everything the first one knew
/// rather than everything a summary could carry. The copy is what makes it a
/// handoff rather than a second agent on one transcript — the two then diverge,
/// and the session that was handed over is left exactly as it was found, still
/// resumable itself.
///
/// It costs what the brief was written to avoid: the whole window, tool output
/// and all, replayed into a fresh one. That is the trade being made here on
/// purpose — everything is carried because everything can be.
///
/// `config_dir` is the receiving account's, which is not always the sending
/// one's: handing a personal session to a work login has to put the copy where
/// that login will look.
///
/// ponytail: subagent sidechains are not copied. They live in a directory
/// beside the transcript named after it, the resumed session does not read them
/// back, and the parent transcript already holds each subagent's report.
pub fn fork(transcript: &Path, config_dir: &Path) -> std::io::Result<String> {
    use std::io::{BufRead, BufWriter, Write};

    let missing = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, what);
    let project = transcript
        .parent()
        .and_then(Path::file_name)
        .ok_or_else(|| missing("that transcript is not in a project directory"))?;
    let old = transcript
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| missing("that transcript has no session id"))?;

    let dir = config_dir.join("projects").join(project);
    std::fs::create_dir_all(&dir)?;
    // Retried rather than trusted: an id that already exists would mean writing
    // over somebody's conversation, which is the one outcome a fork must not
    // have. Three tries because the second one failing is already impossible.
    let (id, path) = (0..3)
        .map(|_| new_session_id())
        .map(|id| {
            let path = dir.join(format!("{id}.jsonl"));
            (id, path)
        })
        .find(|(_, path)| !path.exists())
        .ok_or_else(|| missing("could not find an unused session id"))?;

    let source = std::io::BufReader::new(std::fs::File::open(transcript)?);
    let mut out = BufWriter::new(std::fs::File::create(&path)?);
    for line in source.lines() {
        let line = line?;
        // Every record carries the id, and a transcript still claiming the old
        // one is a session pointing at a conversation it is not: `/resume`
        // lists the copy under the original's name, and the hooks cctop reads
        // report both agents as the same session.
        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(mut value) if value.get("sessionId").and_then(|v| v.as_str()) == Some(old) => {
                value["sessionId"] = serde_json::Value::String(id.clone());
                writeln!(out, "{value}")?;
            }
            // Anything else is passed through byte for byte. A line this does
            // not understand is still one the harness wrote and will read.
            _ => writeln!(out, "{line}")?,
        }
    }
    out.flush()?;
    Ok(id)
}

/// A session id of the shape the harness writes: a version-4 UUID.
fn new_session_id() -> String {
    let mut bytes = crate::util::random_bytes(16);
    // The version and variant bits. Nothing checks them, but a file whose name
    // is not a UUID is one a person reading the directory cannot place.
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

/// The line the receiving agent is given so it goes and reads the brief.
///
/// A path rather than the brief's text: the text is thousands of tokens, and
/// pushing it into an agent is both slow and at the mercy of the terminal's
/// bracketed-paste handling. A one-line instruction the agent acts on itself is
/// the same information with none of that.
pub fn prompt_for(path: &Path) -> String {
    format!(
        "Read {} — it is a cctop handoff brief describing work another agent was \
         doing in this directory. Use it as background, verify what it claims \
         against the repository, then continue from where it leaves off.",
        path.display()
    )
}

/// `argv` with `line` added as the agent's opening prompt, for the harnesses
/// that take one on their command line.
///
/// This is how a brief should reach an agent, and typing it is the fallback.
/// Every one of these CLIs starts by asking the terminal what it can do — the
/// keyboard-enhancement query, and whatever else — and reads its stdin looking
/// for the answer, discarding what does not match. A brief that arrives during
/// that window loses however much of itself was in the input queue at the time,
/// which is not a partial failure the user can see: what landed still looks like
/// a sentence. Handing off a Claude session to Codex produced
///
/// ```text
/// bc09dce-4d0a-4568-b4b7-870bcec98aa3.md — it is a cctop handoff brief …
/// ```
///
/// from a file called `3bc09dce-…`, so Codex went looking for a path that had
/// never existed, found nothing, and asked for the brief to be pasted. No settle
/// delay fixes that: the query happens when the agent starts, which is exactly
/// when there is a brief waiting for it. An argument is read after the agent is
/// running, from somewhere nothing else is competing for.
///
/// `None` for anything not listed — a login shell, or a harness whose flag isn't
/// known here — which leaves that agent on the typed path it used before.
pub fn opening_argv(argv: &[String], line: &str) -> Option<Vec<String>> {
    // `None` is a positional prompt, which is what most of them take.
    let flag = match command_of(argv)? {
        "claude" | "codex" | "cursor-agent" => None,
        "opencode" => Some("--prompt"),
        _ => return None,
    };
    let mut out = argv.to_vec();
    out.extend(flag.map(str::to_string));
    out.push(line.to_string());
    Some(out)
}

/// The command an argv runs, bare of any path and of the `env VAR=value` prefix
/// a profile launch carries.
pub fn command_of(argv: &[String]) -> Option<&str> {
    let mut rest = argv;
    if rest.first().map(String::as_str) == Some("env") {
        rest = &rest[1..];
        while rest
            .first()
            .is_some_and(|a| a.contains('=') && !a.starts_with('-'))
        {
            rest = &rest[1..];
        }
    }
    let first = rest.first()?.as_str();
    Some(first.rsplit('/').next().unwrap_or(first))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pricing::Provider;
    use crate::session::Delta;

    fn detail(d: &str, ts: &str) -> ToolDetail {
        ToolDetail {
            d: d.to_string(),
            ts: ts.to_string(),
            full: None,
            id: None,
            dur_ms: None,
            tokens_in: 0,
            tokens_out: 0,
            shared: 0,
            window_growth: None,
            delta: None,
            failed: false,
            origin: None,
        }
    }

    fn turn(role: &'static str, kind: &'static str, text: &str) -> chat::Turn {
        chat::Turn {
            role,
            kind,
            ts: "2026-08-05T10:00:00Z".into(),
            text: text.to_string(),
            clipped: false,
            tools: Vec::new(),
        }
    }

    fn chat_of(turns: Vec<chat::Turn>) -> chat::Conversation {
        chat::Conversation {
            supported: true,
            turns,
            earlier: 0,
            note: None,
        }
    }

    fn briefed(turns: Vec<chat::Turn>) -> Brief {
        Brief {
            chat: Some(chat_of(turns)),
            ..Brief::default()
        }
    }

    fn data_with(details: Vec<(&str, Vec<ToolDetail>)>) -> SessionData {
        let mut data = SessionData::default();
        for (name, list) in details {
            data.metrics.tool_details.insert(name.to_string(), list);
        }
        data
    }

    /// A brief must be produced for a session whose transcript has not been
    /// read yet — the header alone is a usable handoff.
    #[test]
    fn a_session_with_no_extracted_data_still_briefs() {
        let mut session = Session::new(Provider::Claude, "abc".into());
        session.title = Some("Fix the parser".into());
        session.label_source = "/tmp/repo".into();
        let brief = build(&session, None);
        assert_eq!(brief.title, "Fix the parser");
        assert!(brief.to_markdown().contains("Fix the parser"));
        assert!(brief.files.is_empty());
    }

    /// The brief listed what a session touched and never what it was for. The
    /// prompts are the intent, and they are what a receiving agent cannot
    /// reconstruct from the repository.
    #[test]
    fn the_users_prompts_are_carried_oldest_first() {
        let brief = briefed(vec![
            turn("user", "message", "add a --json flag"),
            turn("assistant", "message", "Added it."),
            turn("user", "message", "keep the human output default"),
        ]);
        let md = brief.to_markdown();
        let first = md.find("> add a --json flag").expect("first prompt");
        let second = md.find("> keep the human output").expect("second prompt");
        assert!(
            first < second,
            "prompts are read in the order they were said"
        );
    }

    /// A prompt is quoted so that one written in markdown cannot be read as the
    /// brief's own headings — or as an instruction to the agent reading it.
    #[test]
    fn a_prompt_that_is_markdown_cannot_become_the_briefs_structure() {
        let brief = briefed(vec![turn("user", "message", "## Do this\n- step one")]);
        let md = brief.to_markdown();
        assert!(md.contains("> ## Do this"));
        assert!(md.contains("> - step one"));
        assert!(!md.contains("\n## Do this"));
    }

    /// Only the newest prompts survive the cap. A brief that kept the opening
    /// one instead would describe the task as first stated, which is exactly
    /// the version a long session has moved on from.
    #[test]
    fn the_cap_on_prompts_drops_the_oldest() {
        let many: Vec<chat::Turn> = (0..MAX_PROMPTS + 5)
            .map(|i| turn("user", "message", &format!("prompt {i}")))
            .collect();
        let asked = prompts(&chat_of(many));
        assert_eq!(asked.len(), MAX_PROMPTS);
        assert_eq!(asked[0], "prompt 5");
        assert_eq!(
            asked[MAX_PROMPTS - 1],
            format!("prompt {}", MAX_PROMPTS + 4)
        );
    }

    /// A quoted instruction that ends cleanly reads as the whole instruction,
    /// which in a document an agent treats as fact is a wrong instruction.
    #[test]
    fn a_clipped_prompt_says_that_it_was_clipped() {
        let long = "x".repeat(MAX_PROMPT_CHARS + 100);
        let asked = prompts(&chat_of(vec![turn("user", "message", &long)]));
        assert!(asked[0].ends_with('…'));
        assert_eq!(asked[0].chars().count(), MAX_PROMPT_CHARS + 1);
    }

    /// Reasoning and compaction turns are the harness talking, not the user.
    #[test]
    fn only_what_was_said_counts_as_a_prompt_or_a_closing_turn() {
        let chat = chat_of(vec![
            turn("user", "message", "the ask"),
            turn("assistant", "reasoning", "thinking out loud"),
            turn("system", "compaction", "summary of earlier turns"),
            turn("assistant", "message", "the answer"),
        ]);
        assert_eq!(prompts(&chat), vec!["the ask"]);
        assert_eq!(closing(&chat), vec!["the answer"]);
    }

    /// The record is a file, and a brief rendered without one — the MCP tool
    /// answers with text alone — must not send an agent looking for it.
    #[test]
    fn the_record_is_named_only_when_one_was_written() {
        let brief = briefed(vec![turn("user", "message", "the ask")]);
        assert!(!brief.to_markdown().contains("The conversation in full"));
        let named = brief.to_markdown_with(Some(Path::new("/tmp/x.jsonl")));
        assert!(named.contains("The conversation in full"));
        assert!(named.contains("/tmp/x.jsonl"));
    }

    /// The record carries what the brief leaves out: the thinking, and every
    /// tool call rather than a bounded list of the paths they named.
    #[test]
    fn the_record_carries_the_reasoning_and_the_tool_calls() {
        let mut acting = turn("assistant", "message", "editing now");
        acting.tools = vec![chat::ToolUse {
            name: "Edit".into(),
            detail: "src/main.rs".into(),
            result: Some("ok".into()),
            ..chat::ToolUse::default()
        }];
        let brief = Brief {
            session_id: "abc".into(),
            chat: Some(chat_of(vec![
                turn("assistant", "reasoning", "weighing two designs"),
                acting,
            ])),
            ..Brief::default()
        };

        let record = brief.to_jsonl();
        let lines: Vec<&str> = record.lines().collect();
        assert_eq!(lines.len(), 3, "a header line, then one line per turn");
        assert!(lines[0].contains("cctop-handoff-record"));
        assert!(lines[0].contains("\"session_id\":\"abc\""));
        assert!(lines[1].contains("reasoning"));
        assert!(lines[1].contains("weighing two designs"));
        assert!(lines[2].contains("src/main.rs"));
        for line in lines {
            serde_json::from_str::<serde_json::Value>(line).expect("every line is one object");
        }
    }

    /// A harness cctop cannot read a conversation for keeps the brief it always
    /// had, and gets no empty record beside it.
    #[test]
    fn a_session_with_no_readable_conversation_writes_no_record() {
        let brief = Brief {
            title: "Fix the parser".into(),
            ..Brief::default()
        };
        assert!(brief.to_jsonl().is_empty());
        assert!(brief.to_markdown().contains("Fix the parser"));
        assert!(!brief.to_markdown().contains("What it was asked"));
    }

    /// The record is written beside the brief, and the brief names it.
    #[test]
    fn a_brief_with_a_conversation_leaves_a_record_beside_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let brief = Brief {
            session_id: "abc-123".into(),
            chat: Some(chat_of(vec![turn("user", "message", "the ask")])),
            ..Brief::default()
        };

        let path = write_in(dir.path(), &brief).expect("write");
        let record = dir.path().join("abc-123.jsonl");
        assert!(record.exists());
        let md = std::fs::read_to_string(&path).expect("read brief");
        assert!(md.contains(&record.display().to_string()));
    }

    /// Handing off again, once the conversation can no longer be read, must not
    /// leave the new brief naming the old conversation.
    #[test]
    fn a_stale_record_does_not_outlive_the_brief_that_named_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut brief = Brief {
            session_id: "abc-123".into(),
            chat: Some(chat_of(vec![turn("user", "message", "the ask")])),
            ..Brief::default()
        };
        write_in(dir.path(), &brief).expect("first write");

        brief.chat = None;
        let path = write_in(dir.path(), &brief).expect("second write");
        assert!(!dir.path().join("abc-123.jsonl").exists());
        let md = std::fs::read_to_string(&path).expect("read brief");
        assert!(!md.contains("The conversation in full"));
    }

    /// Only the newest plan survives: earlier ones were superseded in place and
    /// describe work that has already moved on.
    #[test]
    fn only_the_last_recorded_plan_is_carried() {
        let mut old = detail("1/3 → old step", "2026-08-05T10:00:00Z");
        old.full = Some("[completed] old step".into());
        let mut new = detail("2/3 → new step", "2026-08-05T11:00:00Z");
        new.full = Some("[completed] first\n[in_progress] new step".into());
        let data = data_with(vec![("update_plan", vec![old, new])]);

        let brief = build(&Session::new(Provider::Codex, "x".into()), Some(&data));
        assert_eq!(
            brief.plan,
            vec!["[completed] first", "[in_progress] new step"]
        );
    }

    /// Edits to one file across several calls collapse into a single row that
    /// counts them, rather than repeating the path.
    #[test]
    fn repeated_edits_to_a_file_collapse_into_one_row() {
        let mut second = detail("src/main.rs", "2026-08-05T10:01:00Z");
        second.delta = Some(Delta {
            added: 3,
            removed: 1,
            hunks: Vec::new(),
        });
        let data = data_with(vec![(
            "Edit",
            vec![detail("src/main.rs", "2026-08-05T10:00:00Z"), second],
        )]);

        let brief = build(&Session::new(Provider::Claude, "x".into()), Some(&data));
        assert_eq!(brief.files.len(), 1);
        assert_eq!(brief.files[0].edits, 2);
        assert_eq!(brief.files[0].added, 3);
        assert!(brief.to_markdown().contains("`src/main.rs` ×2 (+3/-1)"));
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    /// The whole point: the brief reaches the agent as an argument, so nothing
    /// it does to the terminal on startup can eat the first half of the line.
    #[test]
    fn a_harness_that_takes_a_prompt_is_given_one_in_its_argv() {
        assert_eq!(
            opening_argv(&argv(&["codex"]), "Read /tmp/b.md and continue"),
            Some(argv(&["codex", "Read /tmp/b.md and continue"]))
        );
        // The prompt stays one argument, whatever is in it — nothing between
        // here and the exec re-splits it.
        let flagged = opening_argv(&argv(&["opencode"]), "Read /tmp/b.md").unwrap();
        assert_eq!(flagged, argv(&["opencode", "--prompt", "Read /tmp/b.md"]));
    }

    /// A profile launch is `env CODEX_HOME=… codex`, and the prompt belongs to
    /// the agent at the end of it rather than to `env`.
    #[test]
    fn a_profile_prefix_does_not_hide_the_harness() {
        assert_eq!(
            opening_argv(
                &argv(&["env", "CODEX_HOME=/home/x/.codex-work", "codex"]),
                "go"
            ),
            Some(argv(&[
                "env",
                "CODEX_HOME=/home/x/.codex-work",
                "codex",
                "go"
            ]))
        );
        // An absolute path is the same harness.
        assert!(opening_argv(&argv(&["/usr/bin/claude"]), "go").is_some());
    }

    /// A shell has no prompt to be given, and inventing an argument for it would
    /// mean launching `zsh "Read …"` — so these fall back to being typed at.
    #[test]
    fn a_command_with_no_known_prompt_argument_is_left_alone() {
        assert_eq!(opening_argv(&argv(&["/bin/zsh"]), "go"), None);
        assert_eq!(opening_argv(&[], "go"), None);
    }

    /// The brief tells the receiving agent what it is reading. Without that an
    /// agent treats a file of notes as instructions from its user.
    #[test]
    fn the_brief_says_what_it_is() {
        let brief = build(&Session::new(Provider::Claude, "x".into()), None);
        let md = brief.to_markdown();
        assert!(md.contains("background, not instructions"));
    }

    /// The Claude-to-Claude handoff: the copy is a session in its own right,
    /// claiming its own id, and the transcript it was taken from is left exactly
    /// as it was found — both halves matter, since the session being handed over
    /// is still resumable and must not end up with two agents appending to it.
    #[test]
    fn a_fork_is_a_second_session_on_the_same_conversation() {
        let home = tempfile::tempdir().unwrap();
        let old = "2714fc38-60cd-49bd-b00c-c6577c31c720";
        let project = home.path().join("projects").join("-home-x-work");
        std::fs::create_dir_all(&project).unwrap();
        let transcript = project.join(format!("{old}.jsonl"));
        let original = format!(
            "{{\"type\":\"mode\",\"sessionId\":\"{old}\"}}\n\
             {{\"type\":\"user\",\"sessionId\":\"{old}\",\"text\":\"about {old}\"}}\n\
             not json at all\n"
        );
        std::fs::write(&transcript, &original).unwrap();

        let into = tempfile::tempdir().unwrap();
        let id = fork(&transcript, into.path()).unwrap();
        assert_ne!(id, old);
        // Landed in the same project directory, under the receiving account.
        let copy = into
            .path()
            .join("projects/-home-x-work")
            .join(format!("{id}.jsonl"));
        let text = std::fs::read_to_string(&copy).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(
            !text.contains(&format!("\"sessionId\":\"{old}\"")),
            "{text}"
        );
        assert_eq!(text.matches(&format!("\"sessionId\":\"{id}\"")).count(), 2);
        // Only the field is rewritten: the old id inside a message is part of
        // what was said, and a session that talked about a uuid still did.
        assert!(text.contains(&format!("about {old}")), "{text}");
        // A line the harness wrote and this does not parse is still carried.
        assert!(text.contains("not json at all"), "{text}");
        // And the session handed over is untouched.
        assert_eq!(std::fs::read_to_string(&transcript).unwrap(), original);
    }

    /// Two forks of one session are two sessions.
    #[test]
    fn a_fork_never_writes_over_a_conversation() {
        let home = tempfile::tempdir().unwrap();
        let project = home.path().join("projects").join("-p");
        std::fs::create_dir_all(&project).unwrap();
        let transcript = project.join("aaaa.jsonl");
        std::fs::write(&transcript, "{}\n").unwrap();
        let first = fork(&transcript, home.path()).unwrap();
        let second = fork(&transcript, home.path()).unwrap();
        assert_ne!(first, second);
        assert_eq!(new_session_id().len(), 36);
    }

    /// What can be handed over whole, and what has to go as a summary.
    #[test]
    fn only_a_local_claude_cli_transcript_can_be_forked() {
        let mut session = Session::new(Provider::Claude, "abc".into());
        assert_eq!(forkable(&session), None, "no transcript to copy");
        session.data_file = Some(PathBuf::from("/home/x/.claude/projects/-p/abc.jsonl"));
        assert!(forkable(&session).is_some());

        // Claude for Mac keeps its conversations elsewhere and resumes them by
        // title, so a copy under a new id is not a session it would ever find.
        let mut desktop = session.clone();
        desktop.surface = crate::session::Surface::DesktopCode;
        assert_eq!(forkable(&desktop), None);

        // Another harness cannot read this format at all — that is the whole
        // reason the brief exists.
        let mut codex = session.clone();
        codex.provider = Provider::Codex;
        assert_eq!(forkable(&codex), None);
    }
}
