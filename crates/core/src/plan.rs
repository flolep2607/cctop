//! The plan a session is working to, step by step, as its own tools recorded
//! it.
//!
//! Three harnesses keep one, in two shapes:
//!
//! - **A checklist rewritten whole.** Codex's `update_plan` and Claude's older
//!   `TodoWrite` send every step with its status on every call, so the newest
//!   call is the plan and the ones before it are history.
//! - **Tasks made and moved one at a time.** Claude's `TaskCreate` adds a step
//!   and `TaskUpdate` moves one, named by the number the harness gave it. The
//!   plan is the creations in order, with every update after them applied.
//!
//! A session that used both is read by whichever it touched last.
//!
//! ponytail: task numbers are the creations' order, which is how Claude Code
//! hands them out within a session. A session that made more tasks than the
//! extraction keeps details for ([`MAX_TOOL_DETAILS`](crate::config::MAX_TOOL_DETAILS))
//! has lost its first ones, and updates to those are dropped rather than
//! applied to the wrong step. A plan kept in prose is not a plan here.

use std::collections::HashMap;

use crate::session::ToolDetail;

/// Where one step stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Done,
    Active,
    Pending,
}

impl Status {
    fn parse(word: &str) -> Status {
        match word {
            "completed" | "done" => Status::Done,
            "in_progress" => Status::Active,
            _ => Status::Pending,
        }
    }
}

/// One step of the plan.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Step {
    pub text: String,
    pub status: Status,
}

const CHECKLISTS: &[&str] = &["update_plan", "TodoWrite", "todo_write"];

/// The session's current plan, in its own order; empty when it kept none.
pub fn latest(details: &HashMap<String, Vec<ToolDetail>>) -> Vec<Step> {
    let checklist = CHECKLISTS
        .iter()
        .filter_map(|name| details.get(*name))
        .flatten()
        .max_by(|a, b| a.ts.cmp(&b.ts));
    let creates = sorted(details, "TaskCreate");
    let updates = sorted(details, "TaskUpdate");
    let tasks_at = creates
        .iter()
        .chain(updates.iter())
        .map(|d| d.ts.as_str())
        .max();
    match (checklist, tasks_at) {
        (Some(list), Some(at)) if at > list.ts.as_str() => tasks(&creates, &updates),
        (Some(list), _) => steps_of(list),
        (None, Some(_)) => tasks(&creates, &updates),
        (None, None) => Vec::new(),
    }
}

/// How many of `plan`'s steps are done.
pub fn done(plan: &[Step]) -> usize {
    plan.iter().filter(|s| s.status == Status::Done).count()
}

fn sorted<'a>(details: &'a HashMap<String, Vec<ToolDetail>>, name: &str) -> Vec<&'a ToolDetail> {
    let mut list: Vec<&ToolDetail> = details.get(name).into_iter().flatten().collect();
    list.sort_by(|a, b| a.ts.cmp(&b.ts));
    list
}

/// A checklist call's steps, from the `[status] text` lines the extractor keeps.
fn steps_of(detail: &ToolDetail) -> Vec<Step> {
    let Some(full) = detail.full.as_deref() else {
        return Vec::new();
    };
    full.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix('[')?;
            let (status, text) = rest.split_once(']')?;
            let text = text.trim();
            (!text.is_empty()).then(|| Step {
                text: text.to_string(),
                status: Status::parse(status),
            })
        })
        .collect()
}

/// Tasks as made, numbered from one, with each update applied in turn.
fn tasks(creates: &[&ToolDetail], updates: &[&ToolDetail]) -> Vec<Step> {
    let mut steps: Vec<Option<Step>> = creates
        .iter()
        .map(|d| {
            Some(Step {
                text: d.d.trim().to_string(),
                status: Status::Pending,
            })
        })
        .collect();
    for update in updates {
        // `#3 completed`, as the extractor writes a `TaskUpdate`.
        let mut words = update.d.split_whitespace();
        let Some(id) = words
            .next()
            .and_then(|w| w.strip_prefix('#'))
            .and_then(|n| n.parse::<usize>().ok())
        else {
            continue;
        };
        let Some(slot) = id.checked_sub(1).and_then(|i| steps.get_mut(i)) else {
            continue;
        };
        match words.next() {
            Some("deleted") => *slot = None,
            Some(word) => {
                if let Some(step) = slot {
                    step.status = Status::parse(word);
                }
            }
            None => {}
        }
    }
    steps
        .into_iter()
        .flatten()
        .filter(|s| !s.text.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Plans an agent published itself
// ---------------------------------------------------------------------------

/// The most steps a published plan keeps; past it the rest are dropped.
pub const MAX_STEPS: usize = 50;
/// The longest step a published plan keeps, in characters; longer is cut.
pub const MAX_TEXT: usize = 200;

/// A plan an agent sent through cctop's `set_plan` MCP tool, checked.
///
/// For the harnesses with no to-do tool of their own — OpenCode, Devin and
/// the rest — which is the only way their plan reaches the sidebar. The
/// status words are the ones the harnesses' own tools use, so an agent that
/// knows one knows these; anything else is refused rather than guessed at,
/// because a step shown done that is not is worse than no plan.
pub fn checked(steps: &[(String, String)]) -> Result<Vec<Step>, String> {
    if steps.is_empty() {
        return Err("a plan needs at least one step".into());
    }
    steps
        .iter()
        .take(MAX_STEPS)
        .map(|(text, status)| {
            let status = match status.as_str() {
                "pending" => Status::Pending,
                "in_progress" => Status::Active,
                "completed" => Status::Done,
                other => {
                    return Err(format!(
                        "{other:?} is not a status: use pending, in_progress or completed"
                    ));
                }
            };
            let text: String = text
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_TEXT)
                .collect();
            match text.trim().is_empty() {
                true => Err("a step needs some text".into()),
                false => Ok(Step {
                    text: text.trim().to_string(),
                    status,
                }),
            }
        })
        .collect()
}

/// Where published plans are kept: one small file per session, beside the
/// notes, so a plan outlives the agent's MCP server and the cctop showing it.
fn published_dir() -> std::path::PathBuf {
    crate::config::data_base().join("cctop").join("plans")
}

/// The file for one session's plan, or `None` for an id that could name a
/// path rather than a session.
fn published_file(dir: &std::path::Path, provider: &str, id: &str) -> Option<std::path::PathBuf> {
    let safe = |word: &str| crate::remote_open::id_ok(word);
    (safe(provider) && safe(id)).then(|| dir.join(format!("{provider}-{id}.json")))
}

/// Keep `steps` as the session's published plan, replacing the last one.
pub fn publish(provider: &str, id: &str, steps: &[Step]) -> std::io::Result<()> {
    publish_in(&published_dir(), provider, id, steps)
}

fn publish_in(
    dir: &std::path::Path,
    provider: &str,
    id: &str,
    steps: &[Step],
) -> std::io::Result<()> {
    let path = published_file(dir, provider, id)
        .ok_or_else(|| std::io::Error::other("not a session id"))?;
    let list: Vec<serde_json::Value> = steps
        .iter()
        .map(|step| {
            let status = match step.status {
                Status::Done => "completed",
                Status::Active => "in_progress",
                Status::Pending => "pending",
            };
            serde_json::json!({ "text": step.text, "status": status })
        })
        .collect();
    let bytes = serde_json::to_vec(&list).map_err(std::io::Error::other)?;
    crate::util::write_atomic(&path, &bytes, true)
}

/// The plan the session published, or nothing.
///
/// The sidebar asks for it only when the transcript holds no plan of its own:
/// a harness's own to-do tool is the better witness, since the agent uses it
/// as it works rather than when told to report.
pub fn published(provider: &str, id: &str) -> Vec<Step> {
    if crate::under_test() {
        return Vec::new();
    }
    published_in(&published_dir(), provider, id)
}

fn published_in(dir: &std::path::Path, provider: &str, id: &str) -> Vec<Step> {
    let Some(path) = published_file(dir, provider, id) else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Array(list)) = serde_json::from_str(&text) else {
        return Vec::new();
    };
    list.iter()
        .take(MAX_STEPS)
        .filter_map(|step| {
            Some(Step {
                text: step.get("text")?.as_str()?.chars().take(MAX_TEXT).collect(),
                status: Status::parse(step.get("status")?.as_str()?),
            })
        })
        .collect()
}

/// The session whose agent started the process with ancestry `ancestors`.
///
/// cctop's MCP server is spawned by the agent it serves, so the agent is
/// among its parents; nothing else says which session called a tool. Nearest
/// first, and an agent root before any process merely in a session's list —
/// a child an agent spawned is in that agent's list too, and naming it would
/// pick the same session the long way round. Local sessions only: a pid from
/// another machine names nothing here.
pub fn caller<'a>(
    ancestors: &[u32],
    sessions: &'a [crate::session::Session],
) -> Option<&'a crate::session::Session> {
    let local = || sessions.iter().filter(|s| s.remote.is_none());
    ancestors
        .iter()
        .find_map(|pid| local().find(|s| s.root_pid() == Some(*pid)))
        .or_else(|| {
            ancestors.iter().find_map(|pid| {
                local().find(|s| {
                    s.process.as_ref().is_some_and(|info| {
                        info.process_list.iter().any(|p| p.pid == *pid && !p.ghost)
                    })
                })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(text: &str, status: &str) -> (String, String) {
        (text.to_string(), status.to_string())
    }

    /// A published plan is kept to its caps, its controls stripped, and an
    /// unknown status or an empty step refused rather than guessed at.
    #[test]
    fn a_published_plan_is_checked_and_capped() {
        let plan = checked(&[
            step("read it", "completed"),
            step("fix\x1bit", "in_progress"),
        ])
        .unwrap();
        assert_eq!(plan[0].status, Status::Done);
        assert_eq!(plan[1].text, "fixit");
        assert_eq!(plan[1].status, Status::Active);
        assert!(checked(&[step("x", "doing")]).is_err());
        assert!(checked(&[step("  ", "pending")]).is_err());
        assert!(checked(&[]).is_err());
        let long: Vec<_> = (0..80)
            .map(|i| step(&"y".repeat(300 + i), "pending"))
            .collect();
        let capped = checked(&long).unwrap();
        assert_eq!(capped.len(), MAX_STEPS);
        assert!(capped.iter().all(|s| s.text.chars().count() == MAX_TEXT));
    }

    /// What is published is what is read back, per session, and an id that
    /// could name a path is neither written nor read.
    #[test]
    fn a_published_plan_is_read_back_for_its_session_only() {
        let dir = tempfile::tempdir().unwrap();
        let plan = checked(&[step("one", "completed"), step("two", "pending")]).unwrap();
        publish_in(dir.path(), "opencode", "ses_abc", &plan).unwrap();
        assert_eq!(published_in(dir.path(), "opencode", "ses_abc"), plan);
        assert!(published_in(dir.path(), "opencode", "ses_other").is_empty());
        assert!(publish_in(dir.path(), "opencode", "../escape", &plan).is_err());
        assert!(published_in(dir.path(), "opencode", "../escape").is_empty());
    }

    /// The caller is the nearest ancestor that is a session's agent root, and
    /// a process merely in a session's list counts only when no root matches.
    #[test]
    fn the_caller_is_the_agent_that_started_the_server() {
        use crate::proc::{ProcEntry, ProcInfo};
        use crate::session::Session;
        let session = |id: &str, root: u32, child: u32| {
            let mut s = Session::new(crate::pricing::Provider::OpenCode, id.into());
            let entry = |pid, is_root| ProcEntry {
                pid,
                cpu: 0.0,
                memory: 0,
                args: String::new(),
                is_root,
                ghost: false,
            };
            s.process = Some(ProcInfo {
                process_list: vec![entry(root, true), entry(child, false)],
                ..Default::default()
            });
            s
        };
        let sessions = vec![session("a", 100, 101), session("b", 200, 201)];
        // The MCP server's parents: its agent (200), then a shell, then init.
        assert_eq!(
            caller(&[200, 50, 1], &sessions).map(|s| s.session_id.as_str()),
            Some("b")
        );
        // Spawned by the agent's child rather than the agent itself.
        assert_eq!(
            caller(&[101, 100, 1], &sessions).map(|s| s.session_id.as_str()),
            Some("a")
        );
        assert!(caller(&[999, 1], &sessions).is_none());
    }

    fn detail(d: &str, ts: &str, full: Option<&str>) -> ToolDetail {
        ToolDetail {
            d: d.into(),
            ts: ts.into(),
            full: full.map(str::to_string),
            ..ToolDetail::default()
        }
    }

    #[test]
    fn the_newest_checklist_is_the_plan() {
        let mut details = HashMap::new();
        details.insert(
            "TodoWrite".to_string(),
            vec![
                detail("", "1", Some("[pending] old")),
                detail(
                    "",
                    "2",
                    Some("[completed] read\n[in_progress] write\n[pending] ship"),
                ),
            ],
        );
        let plan = latest(&details);
        assert_eq!(
            plan.iter()
                .map(|s| (s.text.as_str(), s.status))
                .collect::<Vec<_>>(),
            [
                ("read", Status::Done),
                ("write", Status::Active),
                ("ship", Status::Pending)
            ]
        );
        assert_eq!(done(&plan), 1);
    }

    #[test]
    fn tasks_are_numbered_by_creation_and_moved_by_updates() {
        let mut details = HashMap::new();
        details.insert(
            "TaskCreate".to_string(),
            vec![
                detail("second", "2", None),
                detail("first", "1", None),
                detail("dropped", "3", None),
            ],
        );
        details.insert(
            "TaskUpdate".to_string(),
            vec![
                detail("#1 completed", "4", None),
                detail("#2 in_progress", "5", None),
                detail("#3 deleted", "6", None),
                detail("#9 completed", "7", None),
            ],
        );
        // An older checklist loses to tasks touched since.
        details.insert(
            "TodoWrite".to_string(),
            vec![detail("", "0", Some("[pending] stale"))],
        );
        let plan = latest(&details);
        assert_eq!(
            plan.iter()
                .map(|s| (s.text.as_str(), s.status))
                .collect::<Vec<_>>(),
            [("first", Status::Done), ("second", Status::Active)]
        );
    }

    #[test]
    fn no_plan_is_an_empty_one() {
        assert!(latest(&HashMap::new()).is_empty());
    }
}
