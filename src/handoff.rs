//! Hand one session's context to a different agent.
//!
//! Every harness stores its transcript in its own shape, and none of them can
//! read another's. But cctop already parses all seven into one [`SessionData`],
//! so the material for a handoff is sitting there normalised: what the session
//! was for, which files it changed, what it ran, what plan it was working to.
//! This turns that into a markdown brief any agent can be pointed at.
//!
//! A brief is deliberately *not* a transcript. Replaying a conversation into a
//! fresh window spends the context it is supposed to save, and most of what it
//! spends it on — tool output, file contents the new agent can read itself — is
//! the part the receiving agent should gather first-hand anyway. What does not
//! survive a restart, and so is worth carrying, is the intent: the task, the
//! decisions, the shape of the work so far.
//!
//! ponytail: the plan section only fills in for harnesses that record one as a
//! tool call (Claude's `TodoWrite`, Codex's `update_plan`). A session that kept
//! its plan in prose leaves it empty rather than guessing at which assistant
//! paragraph was the plan.

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

impl Brief {
    /// Render the brief as the markdown a receiving agent is handed.
    ///
    /// Addressed to that agent rather than to a human reader: it opens by
    /// saying what the document is and where it came from, because an agent
    /// that finds an unexplained file of notes in its context will reasonably
    /// treat them as instructions from the user.
    pub fn to_markdown(&self) -> String {
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

        push(
            p,
            "\n---\n\nWritten by cctop. The lists above are bounded — a long session \
             carries its most recent and most-touched entries, not all of them.\n",
        );
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
pub fn write(brief: &Brief) -> std::io::Result<PathBuf> {
    let dir = dir();
    std::fs::create_dir_all(&dir)?;
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
    std::fs::write(&path, brief.to_markdown())?;
    Ok(path)
}

/// The line typed at the receiving agent so it goes and reads the brief.
///
/// A path rather than the brief's text: the text is thousands of tokens, and
/// pasting it through a pty is both slow and at the mercy of the terminal's
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

    /// The brief tells the receiving agent what it is reading. Without that an
    /// agent treats a file of notes as instructions from its user.
    #[test]
    fn the_brief_says_what_it_is() {
        let brief = build(&Session::new(Provider::Claude, "x".into()), None);
        let md = brief.to_markdown();
        assert!(md.contains("background, not instructions"));
    }
}
