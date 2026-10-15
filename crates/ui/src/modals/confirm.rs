//! The confirmations: a question about what is about to happen, or a note on
//! why it cannot, ending in a row of clickable chips.

use super::*;

/// Resuming a session that is already running somewhere else.
pub(crate) fn draw_resume_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let Some(session) = app.selected_session() else {
        return;
    };
    let command = session
        .resume_argv()
        .map(|argv| argv.join(" "))
        .unwrap_or_default();
    // `modal` sizes the box by line count and the paragraph wraps, so a line
    // long enough to wrap pushes the last one out of the border. Every line
    // here is kept inside the 60 columns the box has room for.
    let lines = vec![
        Line::from(Span::styled(
            format!(
                " {} is still running.",
                cctop_core::util::truncate(session.display_label(), 40)
            ),
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::default(),
        Line::from(Span::raw(
            " Resuming starts a second agent on the same transcript,",
        )),
        Line::from(Span::raw(" which neither of them will know about.")),
        Line::default(),
        Line::from(Span::styled(
            format!("   {}", cctop_core::util::truncate(&command, 56)),
            theme::value(),
        )),
        Line::default(),
    ];
    Confirm {
        title: "Resume a running session?",
        width: 62,
        // The same two chips its siblings use, rather than the sentence this
        // line used to be: they are what a pointer can be aimed at, and one
        // dialog in the family phrasing it differently taught nobody anything.
        hint: RESUME_KEYS,
        keys: &yes_no(),
    }
    .draw(frame, area, layout, lines);
}

const RESUME_KEYS: &str = " [y] resume anyway    [n / Esc] cancel";

/// The key a chip stands for: `[y]` is `y`.
fn ch(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}

/// `[n / Esc]`, and `[any key]` on the dialogs that only have something to say.
fn dismiss() -> KeyEvent {
    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
}

/// Claim a confirmation's rectangle and make the `[k]` chips on its last line
/// clickable.
///
/// Both halves matter, and both were missing. Without the rectangle a click
/// anywhere on a dialog fell straight through to the table underneath, moving
/// the selection beneath a question about the row that was selected when it was
/// asked. And the chips are written to be read as buttons — `[y] delete` is not
/// prose — so a pointer aimed at one has to land somewhere, cctop holding the
/// terminal's mouse capture.
///
/// `hint` is the text of that line and `row` its index among the lines, which
/// is how the chips are located: they are found in the string that was drawn,
/// rather than by counting columns a caller would have to keep in step with the
/// wording.
pub(super) fn confirm_chips(
    layout: &mut Layout,
    outer: Rect,
    inner: Rect,
    row: u16,
    hint: &str,
    keys: &[(&str, KeyEvent)],
) {
    layout.modal_rect = Some(outer);
    for (chip, key) in keys {
        let Some(at) = hint.find(chip) else {
            continue;
        };
        let x = inner.x + hint[..at].chars().count() as u16;
        let width = chip.chars().count() as u16;
        layout.key_hits.push((inner.y + row, x, x + width, *key));
    }
}

/// `[y]` and `[n / Esc]`: the two chips of a question that can be refused.
fn yes_no() -> [(&'static str, KeyEvent); 2] {
    [("[y]", ch('y')), ("[n / Esc]", dismiss())]
}

/// `[any key]`: the one chip of a dialog that only has something to say.
pub(super) fn any_key() -> [(&'static str, KeyEvent); 1] {
    [("[any key]", dismiss())]
}

/// A dialog that ends in a row of chips: the box, its width, the chip line and
/// what each chip presses.
///
/// Every confirmation is the same four steps — append the chip line, find the
/// row it landed on, draw the box, hand the chips to [`confirm_chips`] — and
/// they were written out a dozen times, each one a chance to put the chips a
/// row away from where they are drawn. [`Confirm::draw`] is those steps once.
pub(super) struct Confirm<'a> {
    pub(super) title: &'a str,
    pub(super) width: u16,
    /// The last line, as drawn: the chips are found in it by their text.
    pub(super) hint: &'a str,
    pub(super) keys: &'a [(&'a str, KeyEvent)],
}

impl Confirm<'_> {
    /// Draw `lines` with the hint below them, claim the box and make its chips
    /// clickable.
    ///
    /// The chip row is counted in wrapped rows, as [`modal`] sizes the box, and
    /// not in lines: a session label long enough to wrap above it would
    /// otherwise leave every chip a row above the text it stands for.
    pub(super) fn draw(
        self,
        frame: &mut Frame,
        area: Rect,
        layout: &mut Layout,
        mut lines: Vec<Line<'static>>,
    ) {
        let row = wrapped_rows(&lines, self.width.saturating_sub(2));
        lines.push(Line::from(Span::styled(
            self.hint.to_string(),
            theme::dim(),
        )));
        let (outer, inner) = modal(frame, area, self.title, lines, self.width);
        confirm_chips(layout, outer, inner, row, self.hint, self.keys);
    }
}

/// The session a single-session dialog is about: its label, and its id when
/// the dialog is about to act on it, so the one being deleted or stopped can
/// be told from another with the same name.
fn session_header(s: &cctop_core::session::Session, with_id: bool) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!("  {}", s.display_label()),
        theme::value(),
    ))];
    if with_id {
        lines.push(Line::from(Span::styled(
            format!("  {}", s.session_id),
            theme::dim(),
        )));
    }
    lines
}

pub(crate) fn draw_delete_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let Some(s) = app.selected_session() else {
        return;
    };
    let mut lines = session_header(s, true);
    lines.extend([
        Line::default(),
        Line::from(Span::styled(
            "  This permanently removes the transcript from disk.",
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::default(),
    ]);
    Confirm {
        title: "Delete session?",
        width: 62,
        hint: DELETE_KEYS,
        keys: &yes_no(),
    }
    .draw(frame, area, layout, lines);
}

const DELETE_KEYS: &str = "  [y] delete    [n / Esc] cancel";

pub(crate) fn draw_delete_blocked(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let Some(s) = app.selected_session() else {
        return;
    };
    let mut lines = session_header(s, false);
    lines.extend([
        Line::default(),
        Line::from(Span::raw("  This session is still running.")),
        Line::from(Span::raw("  Stop the agent first, then delete it.")),
        Line::default(),
    ]);
    Confirm {
        title: "Cannot delete",
        width: 62,
        hint: DISMISS_KEYS,
        keys: &any_key(),
    }
    .draw(frame, area, layout, lines);
}

/// The one chip on a dialog that has nothing to decide, only something to say.
pub(super) const DISMISS_KEYS: &str = "  [any key] dismiss";

pub(crate) fn draw_kill_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let Some(s) = app.selected_session() else {
        return;
    };
    let mut lines = session_header(s, true);
    lines.extend([
        Line::default(),
        Line::from(Span::styled(
            "  Send a termination signal to this agent process?",
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::from(Span::raw("  Unsaved work in the agent may be interrupted.")),
        Line::default(),
    ]);
    Confirm {
        title: "Terminate session?",
        width: 62,
        hint: KILL_KEYS,
        keys: &yes_no(),
    }
    .draw(frame, area, layout, lines);
}

const KILL_KEYS: &str = "  [y] terminate    [n / Esc] cancel";

pub(crate) fn draw_quit_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let label = match &app.hosted {
        Some((_, label)) => label.clone(),
        None => return,
    };
    let lines = vec![
        Line::from(Span::styled(format!("  {label}"), theme::value())),
        Line::default(),
        Line::from(Span::styled(
            "  This agent is running on a terminal cctop owns,",
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::from(Span::styled(
            "  and quitting ends it.",
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::from(Span::raw("  Exit the agent itself to leave it cleanly.")),
        Line::default(),
    ];
    Confirm {
        title: "Quit and stop the agent?",
        width: 62,
        hint: QUIT_KEYS,
        keys: &[("[y]", ch('y')), ("[n / Esc]", dismiss()), ("[A]", ch('A'))],
    }
    .draw(frame, area, layout, lines);
}

const QUIT_KEYS: &str = "  [y] quit anyway    [n / Esc] stay    [A] back to the agent";

/// Ask before running `cctop --update` on another machine.
///
/// The command is shown as it will run, because this is the one key in cctop
/// that changes something that is not on this computer: someone saying yes to
/// it should be able to read, in full, what they are saying yes to.
pub(crate) fn draw_remote_update_confirm(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    layout: &mut Layout,
) {
    let Some(host) = app
        .remote_update
        .as_deref()
        .and_then(|t| app.remote_host(t))
    else {
        return;
    };
    let theirs = match app.remote_versions.get(&host.target) {
        Some(cctop_core::fleet::Probe::Version(v)) => v.clone(),
        _ => "an unknown version".to_string(),
    };
    let lines = vec![
        Line::from(Span::styled(format!("  {}", host.target), theme::value())),
        Line::from(Span::styled(
            format!(
                "  runs cctop {theirs}; this one is {}",
                cctop_core::update::current_version()
            ),
            theme::dim(),
        )),
        Line::default(),
        Line::from(Span::raw("  Run this, to fetch the newest release there:")),
        Line::from(Span::styled(
            format!("    {}", host.shown_command(&["--update"])),
            Style::default().fg(theme::colors().cost_mid),
        )),
        Line::default(),
        Line::from(Span::raw(
            "  It replaces the binary on that machine. Nothing is asked there: if it",
        )),
        Line::from(Span::raw(
            "  needs root, cctop says so and shows the sudo command to run yourself.",
        )),
        Line::default(),
    ];
    Confirm {
        title: "Update cctop over there?",
        width: 78,
        hint: REMOTE_UPDATE_KEYS,
        keys: &yes_no(),
    }
    .draw(frame, area, layout, lines);
}

const REMOTE_UPDATE_KEYS: &str = "  [y] update    [n / Esc] cancel";

pub(crate) fn draw_kill_blocked(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let Some(s) = app.selected_session() else {
        return;
    };
    let mut lines = session_header(s, false);
    lines.extend([
        Line::default(),
        Line::from(Span::raw(
            "  This session has no locally controllable process.",
        )),
        Line::from(Span::raw("  It may be running in a remote or shared host.")),
        Line::default(),
    ]);
    Confirm {
        title: "Cannot terminate",
        width: 62,
        hint: DISMISS_KEYS,
        keys: &any_key(),
    }
    .draw(frame, area, layout, lines);
}

pub(crate) fn draw_batch_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let (verb, noun) = match app.batch {
        BatchKind::Delete => ("delete", "sessions"),
        BatchKind::Kill => ("terminate", "live sessions"),
        BatchKind::Reclaim => return draw_reclaim_confirm(frame, area, app, layout),
    };
    let ms = app.marked_sessions();
    let mut lines = vec![
        Line::from(Span::styled(
            format!("  {} marked {noun}", ms.len()),
            theme::value(),
        )),
        Line::default(),
    ];
    for s in ms.iter().take(8) {
        lines.push(Line::from(Span::styled(
            format!("    · {}", s.display_label()),
            theme::dim(),
        )));
    }
    if ms.len() > 8 {
        lines.push(Line::from(Span::styled(
            format!("    … and {} more", ms.len() - 8),
            theme::dim(),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        match app.batch {
            BatchKind::Delete => "  This permanently removes their transcripts from disk.",
            BatchKind::Kill | BatchKind::Reclaim => {
                "  Unsaved work in the agents may be interrupted."
            }
        },
        Style::default().fg(theme::colors().cost_mid),
    )));
    lines.push(Line::default());
    let hint = format!("  [y] {verb} all    [n / Esc] cancel");
    Confirm {
        title: &format!("{verb} all?"),
        width: 62,
        hint: &hint,
        keys: &yes_no(),
    }
    .draw(frame, area, layout, lines);
}

/// The idle view's `K`: what will be stopped, how much memory that gives
/// back, and — named, not counted — what is being left running and why.
///
/// The skipped ones are listed because the count alone would read as cctop
/// having failed at them; a reason per row says it chose not to.
fn draw_reclaim_confirm(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    const SHOWN: usize = 8;
    const KEPT_SHOWN: usize = 4;
    let plan = app.reclaim_plan();
    let now = chrono::Utc::now();
    let n = plan.stop.len();
    let mut lines = vec![
        Line::from(Span::styled(
            format!(
                "  Stop {}, freeing {}",
                util::plural(n, "idle session"),
                util::compact_bytes(plan.bytes)
            ),
            theme::value(),
        )),
        Line::default(),
    ];
    // Label, then how long it has been quiet and what it holds, in the table's
    // own spellings so the two can be read against each other.
    for s in plan.stop.iter().take(SHOWN) {
        lines.push(Line::from(vec![
            Span::styled(
                format!("    · {}", util::pad(s.display_label(), 36, false)),
                theme::dim(),
            ),
            Span::styled(
                format!(
                    "{:>5}",
                    util::relative_age(super::idle::quiet_since(s), &now)
                ),
                theme::dim(),
            ),
            Span::styled(
                format!("{:>8}", util::compact_bytes(super::idle::tree_memory(s))),
                theme::value(),
            ),
        ]));
    }
    if n > SHOWN {
        let rest: u64 = plan.stop[SHOWN..]
            .iter()
            .map(|s| super::idle::tree_memory(s))
            .sum();
        lines.push(Line::from(Span::styled(
            format!(
                "    … and {} more, {}",
                n - SHOWN,
                util::compact_bytes(rest)
            ),
            theme::dim(),
        )));
    }
    if !plan.keep.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::raw(format!(
            "  Left running ({}):",
            plan.keep.len()
        ))));
        for (s, why) in plan.keep.iter().take(KEPT_SHOWN) {
            lines.push(Line::from(Span::styled(
                format!(
                    "    · {} — {}",
                    util::truncate(s.display_label(), 30),
                    why.reason()
                ),
                theme::dim(),
            )));
        }
        if plan.keep.len() > KEPT_SHOWN {
            lines.push(Line::from(Span::styled(
                format!("    … and {} more", plan.keep.len() - KEPT_SHOWN),
                theme::dim(),
            )));
        }
    }
    lines.push(Line::default());
    let warn = Style::default().fg(theme::colors().cost_mid);
    lines.push(Line::from(Span::styled(
        "  Each is sent SIGTERM and exits cleanly; R resumes it.",
        warn,
    )));
    lines.push(Line::from(Span::styled(
        "  A prompt typed into one and not sent is lost.",
        warn,
    )));
    lines.push(Line::default());
    let hint = format!("  [y] stop {n}    [n / Esc] cancel");
    Confirm {
        title: "Stop idle sessions?",
        width: 62,
        hint: &hint,
        keys: &yes_no(),
    }
    .draw(frame, area, layout, lines);
}

pub(crate) fn draw_batch_blocked(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    deleting: bool,
    layout: &mut Layout,
) {
    let ms = app.marked_sessions();
    // First one that can't be processed, for the explanation.
    let (explain, name) = match app.batch {
        BatchKind::Delete => (
            "running — stop the agent first",
            ms.iter().find(|s| s.is_running()),
        ),
        BatchKind::Kill | BatchKind::Reclaim => (
            "has no locally controllable process",
            ms.iter().find(|s| s.root_pid().is_none()),
        ),
    };
    let lines = vec![
        Line::from(Span::styled(
            format!("  {} marked sessions", ms.len()),
            theme::value(),
        )),
        Line::default(),
        Line::from(Span::raw(format!(
            "  Not all can be {}.",
            if deleting { "deleted" } else { "killed" }
        ))),
        match name {
            Some(s) => Line::from(Span::styled(
                format!("  · {} {}", s.display_label(), explain),
                theme::dim(),
            )),
            None => Line::from(Span::raw("  At least one is not ready.")),
        },
        Line::default(),
        Line::from(Span::styled(
            "  e.g. unmark the running / remote sessions first.",
            theme::dim(),
        )),
        Line::default(),
    ];
    Confirm {
        title: if deleting {
            "Cannot delete all"
        } else {
            "Cannot terminate all"
        },
        width: 62,
        hint: DISMISS_KEYS,
        keys: &any_key(),
    }
    .draw(frame, area, layout, lines);
}
