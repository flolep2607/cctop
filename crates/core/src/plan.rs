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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
