//! Reading what an agent's own screen says it is doing.
//!
//! The terminal UI reads a tab's screen through this, and so does `peek`,
//! which reads the screen of an agent cctop is not drawing. Both ask the same
//! questions of the same rows, so the table of footers lives below either.

/// How one agent's screen says what it is doing.
///
/// Each harness ends its screen with a line of key hints, and the hints change
/// with the state: a dialog holding the turn offers a way to cancel it, a turn
/// in flight offers a way to interrupt it. The same words do not mean the same
/// thing across harnesses — `esc to cancel` is a permission prompt in Claude
/// Code and a turn in flight in Gemini — which is why this is a table per
/// harness rather than one list of phrases.
///
/// A rule is a set of phrases that must *all* appear in the last `lines`
/// non-empty lines, compared without case. Asking is tried before working, so
/// a prompt raised mid-turn is the prompt.
///
/// There are no idle phrases, on purpose. Codex keeps `? for shortcuts` on its
/// bottom line while it works, and between two steps of one turn draws a frame
/// with no working line at all — so a footer that *looks* idle is a flicker
/// away from a false bell. Idle is instead the absence of both, held still;
/// see `ui::tabs::Pane::read_screen`.
///
/// Only the bottom of the screen is read because the conversation is on the
/// same screen, and a reply that quotes one of these phrases — this comment,
/// read aloud — must not answer for the footer. The narrower the footer, the
/// fewer lines: Claude Code's is the last two.
///
/// Claude Code's phrases were captured from 2.1.283 and Codex's from 0.157.1,
/// both off real screens. The rest are distilled from herdr's detection
/// manifests (Apache-2.0,
/// github.com/herdrdev/herdr, `src/detect/manifests/`, 2026-09), keeping the
/// rules that are plain phrases and leaving out the ones that need a regex or
/// the terminal title.
///
/// ponytail: phrases, not herdr's rule engine; a harness that rewords its
/// footer reads as "nothing recognised" until its row here is updated, and the
/// quiet-screen fallback in `ui::tabs::Pane::read_screen` carries it meanwhile.
struct Footer {
    harness: &'static str,
    lines: usize,
    asking: &'static [&'static [&'static str]],
    working: &'static [&'static [&'static str]],
}

const FOOTERS: &[Footer] = &[
    Footer {
        harness: "claude",
        lines: 2,
        // A permission prompt, an AskUserQuestion, the trust-this-folder question.
        asking: &[&["esc to cancel"]],
        working: &[&["esc to interrupt"]],
    },
    Footer {
        harness: "codex",
        lines: 10,
        asking: &[
            // An approval: `Would you like to run the following command?` over
            // `Press enter to confirm or esc to cancel`.
            &["press enter to confirm or esc to cancel"],
            &["would you like to run the following command?"],
            // Its startup questions: trust the hooks, update now.
            &["enter confirm · esc skip"],
            &["press enter to confirm or esc to go back"],
            &["update now", "skip until next version"],
            &["enter to submit answer"],
            &["enter to submit all"],
            &["allow command?"],
            &["do you trust the contents of this directory?"],
            &["[y/n]"],
            &["yes (y)"],
        ],
        // `• Working (12s • esc to interrupt)`, above the prompt box.
        working: &[&[" to interrupt)"]],
    },
    Footer {
        harness: "gemini",
        lines: 10,
        asking: &[
            &["apply this change"],
            &["allow execution"],
            &["waiting for user confirmation"],
            &["do you want to proceed"],
        ],
        working: &[&["esc to cancel"]],
    },
    Footer {
        harness: "opencode",
        lines: 10,
        asking: &[
            &["△ permission required"],
            &["esc dismiss", "enter confirm"],
            &["esc dismiss", "enter submit"],
        ],
        working: &[
            &["esc to interrupt"],
            &["ctrl+c to interrupt"],
            &["esc interrupt"],
            &["esc again to interrupt"],
        ],
    },
    Footer {
        harness: "cursor",
        lines: 8,
        asking: &[
            &["write to this file?", "proceed (y)"],
            &["run this command?"],
            &["skip (esc or n)"],
            &["(y) (enter)"],
            &["keep (n)"],
        ],
        working: &[&["ctrl+c to stop"]],
    },
    Footer {
        harness: "devin",
        lines: 8,
        asking: &[
            &["do you trust the authors of this directory?"],
            &["approve once", "esc cancel"],
        ],
        working: &[&["esc to interrupt"], &["guide devin while it works"]],
    },
    Footer {
        harness: "droid",
        lines: 8,
        asking: &[
            &["enter to select", "esc to cancel"],
            &["enter select", "esc cancel"],
        ],
        working: &[&["esc to stop"]],
    },
    Footer {
        harness: "pi",
        lines: 12,
        asking: &[],
        working: &[&["working..."]],
    },
];

/// Whether `harness` has a row in [`FOOTERS`] — the cheap half of the check a
/// reader makes before paying for the screen itself.
pub fn screenable(harness: &str) -> bool {
    FOOTERS.iter().any(|f| f.harness == harness)
}

/// What `harness`'s own footer says it is doing, when it says so plainly.
///
/// `None` both for a harness with no row in [`FOOTERS`] and for a screen none
/// of its rules match; `ui::tabs::Pane::read_screen` tells the two apart.
pub fn screen_state(harness: &str, rows: &[String]) -> Option<crate::hook::Signal> {
    use crate::hook::Signal;
    let footer = FOOTERS.iter().find(|f| f.harness == harness)?;
    let bottom: Vec<String> = rows
        .iter()
        .map(|row| row.trim())
        .filter(|row| !row.is_empty())
        .rev()
        .take(footer.lines)
        .map(str::to_lowercase)
        .collect();
    let text = bottom.join("\n");
    let any = |rules: &[&[&str]]| rules.iter().any(|all| all.iter().all(|p| text.contains(p)));
    if any(footer.asking) {
        Some(Signal::NeedsInput)
    } else if any(footer.working)
        || (harness == "claude" && (claude_spinner(rows) || claude_waiting(rows)))
    {
        Some(Signal::Busy)
    } else {
        None
    }
}

/// How far up from the bottom Claude Code's spinner line can sit: the prompt
/// box, the status line, the mode hint and a few rows of the subagent list
/// are drawn below it.
const SPINNER_ROWS: usize = 16;

/// Whether Claude Code's working line is on screen: `✽ Improvising… (14s · ↓
/// 1.1k tokens)`.
///
/// Its footer used to end `esc to interrupt`, which [`FOOTERS`] matches; by
/// 2.1.300 it does not, and with subagents running their list is drawn under
/// the prompt box, so the last two rows say nothing about the turn. What stays
/// is the spinner line above the box: a spinner glyph, a gerund ending in an
/// ellipsis, and the elapsed time in brackets. The finished turn's line has no
/// ellipsis — `✻ Baked for 5s` — and neither does anything else Claude Code
/// prefixes with those glyphs, so the three together are the turn running.
/// Captured off a real 2.1.300 screen (#337's tab).
fn claude_spinner(rows: &[String]) -> bool {
    const GLYPHS: &[char] = &['✻', '✽', '✶', '✳', '✢', '·', '*', '✺', '✹'];
    rows.iter()
        .map(|row| row.trim_matches(|c: char| c == '│' || c.is_whitespace()))
        .filter(|row| !row.is_empty())
        .rev()
        .take(SPINNER_ROWS)
        .any(|row| {
            let mut chars = row.chars();
            let glyph = chars.next().is_some_and(|c| GLYPHS.contains(&c));
            let rest = chars.as_str();
            glyph
                && rest.starts_with(' ')
                && rest
                    .split_once("… (")
                    .is_some_and(|(word, timer)| !word.trim().is_empty() && timer.ends_with(')'))
        })
}

/// Whether Claude Code is waiting on work it started in the background:
/// `✻ Waiting for 1 dynamic workflow to finish`, or `… for 3 background
/// agents …`.
///
/// The turn is over — the Stop hook has fired and the spinner line is gone —
/// but the session is not: Claude Code takes the next turn by itself when
/// they finish. Reading that as done put a session with a workflow still
/// running under the finished bell. The line stays up exactly as long as it
/// waits, so it is read as the session working.
fn claude_waiting(rows: &[String]) -> bool {
    const GLYPHS: &[char] = &['✻', '✽', '✶', '✳', '✢', '·', '*', '✺', '✹'];
    rows.iter()
        .map(|row| row.trim_matches(|c: char| c == '│' || c.is_whitespace()))
        .filter(|row| !row.is_empty())
        .rev()
        .take(SPINNER_ROWS)
        .any(|row| {
            let mut chars = row.chars();
            chars.next().is_some_and(|c| GLYPHS.contains(&c))
                && chars
                    .as_str()
                    .trim_start()
                    .strip_prefix("Waiting for ")
                    .is_some_and(|rest| rest.ends_with(" to finish"))
        })
}

/// Whether the prompt on `harness`'s screen is a question with choices — Claude
/// Code's AskUserQuestion — rather than a permission prompt.
///
/// Both end in `Esc to cancel`, which is all [`screen_state`] needs to know
/// something is being asked. Which kind it is matters to what may be offered:
/// Allow presses the first option and Deny presses Esc, which on a question
/// pick an answer nobody chose or throw the question away. A question draws its
/// own escape hatches — "Type something." and "Chat about this" — and its
/// footer says `Enter to select`; a permission prompt asks "Do you want …" and
/// offers `Tab to amend`. Both captured off real Claude Code screens.
///
/// ponytail: claude only, the one harness whose questions were seen.
pub fn screen_question(harness: &str, rows: &[String]) -> bool {
    if harness != "claude" {
        return false;
    }
    let text: String = rows
        .iter()
        .map(|row| row.trim())
        .filter(|row| !row.is_empty())
        .rev()
        .take(30)
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("\n");
    let question = ["chat about this", "type something.", "enter to select"]
        .iter()
        .any(|p| text.contains(p));
    let permission = ["do you want", "tab to amend"]
        .iter()
        .any(|p| text.contains(p));
    question && !permission
}

/// What a prompt on `harness`'s screen is asking for, when the prompt draws
/// the answer on it.
///
/// Read only once the screen has said it is asking — a `$ …` command line is a
/// normal thing to have on screen at a shell, and this is only called on the
/// screen that just matched a prompt. Codex puts the command it wants under
/// "Would you like to run…"; Claude Code's menu is numbered, so the question
/// and the tool's own detail are the rows above the highlighted option.
/// Whatever is found is verbatim screen text, one line and bounded: a summary
/// invented here would be shown next to an Allow button.
///
/// ponytail: claude and codex only. The other harnesses' prompts were seen,
/// not read for this — a guessed extraction beside a real answer is worse
/// than none.
pub fn screen_ask(harness: &str, rows: &[String]) -> Option<String> {
    // Wider than the footer: Claude's question box and Codex's `$` line sit a
    // few rows above the hint row the recognizer matches on.
    const ASK_ROWS: usize = 15;
    let bottom: Vec<String> = rows
        .iter()
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty())
        .rev()
        .take(ASK_ROWS)
        .collect();
    let found = match harness {
        "codex" => bottom.iter().find_map(|row| {
            row.strip_prefix('$')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        })?,
        // A question's own words: the row above its options that ends in a
        // question mark, past the `☐ Header` chip of a multi-question form.
        // Nothing else — a guess here is shown as the thing being asked.
        "claude" if screen_question(harness, rows) => {
            let top = bottom
                .iter()
                .position(|row| row.starts_with('❯') || row.starts_with('›'))?;
            bottom[top + 1..]
                .iter()
                .take(4)
                .map(|row| unbox(row))
                .find(|row| row.ends_with('?'))?
        }
        "claude" => {
            // The highlighted option (`❯ 1. Yes`) is the menu's top edge; the
            // question and the tool's box are the rows above it. The first one
            // met from the bottom is the prompt's — a `❯` in the conversation
            // above is never the nearer one.
            let top = bottom
                .iter()
                .position(|row| row.starts_with('❯') || row.starts_with('›'))?;
            let mut detail: Vec<String> = bottom[top + 1..]
                .iter()
                .map(|row| unbox(row))
                .filter(|row| !row.is_empty())
                // Further up than this is the conversation, not the prompt.
                .take(3)
                .collect();
            detail.reverse();
            match detail.is_empty() {
                true => return None,
                false => detail.join(" — "),
            }
        }
        _ => return None,
    };
    Some(crate::util::truncate(&found, crate::hook::MAX_ASK))
}

/// A row's box-drawing edges removed, for what the box says rather than draws.
fn unbox(row: &str) -> String {
    row.trim_matches(|c: char| "╭╮╰╯─│ ".contains(c))
        .trim()
        .to_string()
}

/// The harness a rmux session name or a pane label starts with, as one word.
///
/// `cctop-<harness>-…` is how every session is named at creation, whether it
/// was launched fresh or resumed; a label is the command, so its first word.
pub fn harness_of(name: &str) -> &str {
    name.strip_prefix("cctop-")
        .unwrap_or(name)
        .split(['-', ' ', '·', '@'])
        .next()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Claude Code 2.1.300 no longer ends its footer `esc to interrupt`, and
    /// with subagents running their list is drawn under the prompt box. The
    /// spinner line above the box is what says a turn is running; the finished
    /// turn's line is not. Rows off a real screen, box edges and all.
    #[test]
    fn claude_2_1_300_reads_as_working_by_its_spinner_line() {
        let screen =
            |lines: &[&str]| -> Vec<String> { lines.iter().map(|l| l.to_string()).collect() };
        let working = screen(&[
            "│✻ Waiting for 3 background agents to finish         │",
            "│● Agent \"Status audit of remaining items\" finished · 6m 20s │",
            "│✽ Improvising… (14s · ↓ 1.1k tokens)                 │",
            "│                                  ◐ medium · /effort  │",
            "│──────────────────────────────────────────────────────│",
            "│❯                                                     │",
            "│──────────────────────────────────────────────────────│",
            "│  [PONYTAIL]                                          │",
            "│  ⏵⏵ accept edits on (shift+tab to cycle) · /tasks to see subagents │",
            "│  ● main                                              │",
            "│  ◯ general-purpose  Running entity_rebuild selftest   7m 1s · ↓ 103.7k tokens  │",
        ]);
        assert_eq!(
            screen_state("claude", &working),
            Some(crate::hook::Signal::Busy)
        );

        let finished = screen(&[
            "✻ Baked for 5s",
            "────────────",
            "❯",
            "────────────",
            "  ⏵⏵ accept edits on (shift+tab to cycle)",
        ]);
        assert_eq!(screen_state("claude", &finished), None);

        // The turn is over but what it started is not: Claude Code takes the
        // next turn itself when they finish. Off a real screen, a workflow
        // running under an ended turn.
        for waiting in [
            "│✻ Waiting for 1 dynamic workflow to finish          │",
            "✻ Waiting for 3 background agents to finish",
        ] {
            let screen = screen(&[
                "✻ Baked for 5s",
                waiting,
                "│──────────────────────────────────────────────────── ultracode ─│",
                "│❯                                                     │",
                "│──────────────────────────────────────────────────────│",
                "│  [PONYTAIL]                                          │",
                "│  ⏵⏵ auto mode on (shift+tab to cycle)                │",
                "│  ◯ codeless-lines-plan  ▱▱▱▱▱▱▱▱  0/3 · 36s · ↓ 208.6k tokens │",
            ]);
            assert_eq!(
                screen_state("claude", &screen),
                Some(crate::hook::Signal::Busy),
                "{waiting}"
            );
        }

        // A prompt still wins over a spinner left above it.
        let asking = screen(&[
            "✽ Improvising… (3s · ↓ 10 tokens)",
            "Do you want to proceed?",
            "Esc to cancel · Tab to amend",
        ]);
        assert_eq!(
            screen_state("claude", &asking),
            Some(crate::hook::Signal::NeedsInput)
        );
    }
}
