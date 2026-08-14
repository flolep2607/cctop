//! Content builders for the bottom tab panels.
//!
//! Each returns `Vec<Line>` so the caller can scroll and clip uniformly; only
//! the Performance tab draws itself, since it renders charts rather than text.

use super::theme;
use crate::pricing::{Plan, Provider};
use crate::session::{Session, SessionData, Subagent, SubagentStatus, Surface};
use crate::util;
use chrono::{DateTime, Utc};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;

pub const TABS: [&str; 8] = [
    "Info",
    "Performance",
    "Processes",
    "Tool Activity",
    "Subagents",
    "Cost",
    "Config",
    "Context",
];

fn label(text: &str) -> Span<'static> {
    Span::styled(text.to_string(), theme::label())
}

fn value(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), theme::value())
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), theme::dim())
}

fn note(text: &str) -> Vec<Line<'static>> {
    vec![Line::from(dim(text.to_string()))]
}

fn is_free_model(model: &crate::session::ModelBreakdown) -> bool {
    model.total == 0.0
        && model.tokens.all_input() + model.tokens.output + model.tokens.reasoning_output > 0
}

fn displayed_cost(amount: f64, free: bool) -> String {
    if free {
        "FREE".into()
    } else {
        util::adaptive_usd(amount)
    }
}

fn cost_style(amount: f64, free: bool) -> Style {
    Style::default().fg(if free {
        theme::colors().dimmer
    } else {
        theme::cost_color(amount)
    })
}

/// `LABEL   value`, with labels padded to a shared column.
fn field(name: &str, val: impl Into<String>) -> Line<'static> {
    Line::from(vec![
        label(&format!("{name:<9}")),
        Span::raw(" "),
        value(val),
    ])
}

/// Wall time advances while a local agent is live, including pauses between
/// transcript events. Once it exits, preserve the final activity span.
fn wall_duration_ms(session: &Session, now: DateTime<Utc>) -> Option<i64> {
    let started = util::parse_ts(&session.started_at)?;
    let ended = if session.is_running() {
        now
    } else {
        util::parse_ts(&session.last_active)?
    };
    Some((ended.timestamp_millis() - started.timestamp_millis()).max(0))
}

// ---------------------------------------------------------------------------
// Info
// ---------------------------------------------------------------------------

/// A session's overlap with the other live agents, with the peers already
/// resolved to the names the table shows them under.
///
/// Resolving happens in the caller because only it holds the session list;
/// handing the panel raw keys would make it look every peer up itself and
/// print `claude:8f3a…` when it failed.
pub struct Clash {
    pub level: crate::collide::Overlap,
    pub peers: Vec<String>,
    pub files: Vec<String>,
}

/// The Info panel's account of who else is on this session's ground.
///
/// The `!` column can only say that something is wrong; this is where it says
/// what and with whom, which is what turns the warning into an action. Files
/// are listed in full rather than counted — "you and cctop-2 have both written
/// src/ui/mod.rs" is the whole message, and a number would send the reader
/// hunting for it.
fn clash_lines(clash: Option<&Clash>) -> Vec<Line<'static>> {
    /// Beyond this the list stops being read and starts being scrolled past.
    const MAX_FILES: usize = 6;

    let Some(clash) = clash else {
        return Vec::new();
    };
    let peers = clash.peers.join(", ");
    let mut lines = match clash.level {
        crate::collide::Overlap::File => vec![Line::from(vec![
            label(&format!("{:<9}", "Conflict")),
            Span::raw(" "),
            Span::styled(
                format!("⚠ also written by {peers}"),
                Style::default()
                    .fg(theme::colors().cost_high)
                    .add_modifier(Modifier::BOLD),
            ),
        ])],
        crate::collide::Overlap::Directory => vec![Line::from(vec![
            label(&format!("{:<9}", "Sharing")),
            Span::raw(" "),
            Span::styled(
                format!("· this repository with {peers}"),
                Style::default().fg(theme::colors().cost_mid),
            ),
        ])],
    };
    for path in clash.files.iter().take(MAX_FILES) {
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(11)),
            dim(util::path_tail(path, 3)),
        ]));
    }
    if clash.files.len() > MAX_FILES {
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(11)),
            dim(format!("+{} more", clash.files.len() - MAX_FILES)),
        ]));
    }
    lines
}

pub fn info(
    session: &Session,
    data: Option<&SessionData>,
    plan: Plan,
    clash: Option<&Clash>,
) -> Vec<Line<'static>> {
    // A remote row has no extraction behind it and never will — its transcript
    // is on the other machine. Everything below that reads `data` is optional,
    // so it is drawn from an empty one rather than being stuck on "Loading…".
    let empty = SessionData::default();
    let data = match (data, session.remote.is_some()) {
        (Some(data), _) => data,
        (None, true) => &empty,
        (None, false) => return note("Loading…"),
    };
    if let Some(err) = &data.error {
        return vec![
            Line::from(Span::styled(
                "Could not read this session:".to_string(),
                Style::default().fg(theme::colors().cost_high),
            )),
            Line::from(dim(err.clone())),
        ];
    }

    let mut lines = Vec::new();
    let provider_color = match session.surface {
        Surface::DesktopCowork => theme::colors().desktop_cowork,
        Surface::DesktopCode => theme::colors().desktop_code,
        Surface::Editor => theme::colors().cursor,
        Surface::Cli => match session.provider {
            Provider::Claude => theme::colors().claude,
            Provider::Codex => theme::colors().openai,
            Provider::Cursor => theme::colors().cursor,
            Provider::Gemini => theme::colors().gemini,
            Provider::OpenCode => theme::colors().opencode,
            Provider::Pi => theme::colors().pi,
            Provider::Windsurf => theme::colors().windsurf,
        },
    };
    let model = if data.last_model.is_empty() {
        session.model.clone()
    } else {
        data.last_model.clone()
    };

    lines.push(Line::from(vec![
        label(&format!("{:<9}", "Type")),
        Span::raw(" "),
        Span::styled(
            session.surface.label(session.provider).to_string(),
            Style::default()
                .fg(provider_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("    "),
        label("Model"),
        Span::raw("   "),
        Span::styled(
            model.clone(),
            Style::default()
                .fg(theme::model_color(&model))
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines.push(field("ID", session.session_id.clone()));
    // Above everything else about the row, because it changes what the rest of
    // this panel means: the directory is a path over there, the resume command
    // has to be run over there, and none of the keys act on it from here.
    if let Some(r) = &session.remote {
        lines.push(Line::from(vec![
            label(&format!("{:<9}", "Host")),
            Span::raw(" "),
            Span::styled(
                r.host.clone(),
                Style::default()
                    .fg(theme::colors().accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            dim("read over ssh; cctop acts only on this machine"),
        ]));
    }
    if !session.harness.is_empty() {
        lines.push(field("Harness", session.harness.clone()));
    }
    if let Some(t) = &session.title {
        lines.push(field("Title", t.clone()));
    }
    lines.push(field(
        "Dir",
        util::tildify(if session.label_source.is_empty() {
            "unknown"
        } else {
            &session.label_source
        }),
    ));
    let cmd = match session.provider {
        Provider::Claude => format!("claude --resume {}", session.session_id),
        Provider::Codex => format!("codex resume {}", session.session_id),
        Provider::Cursor => "Open from Cursor history".to_string(),
        Provider::Gemini => "gemini, then /chat resume".to_string(),
        Provider::OpenCode => format!("opencode --session {}", session.session_id),
        Provider::Pi => format!("pi --session {}", session.session_id),
        Provider::Windsurf => "Open from Windsurf history".to_string(),
    };
    lines.push(field("Cmd", cmd));
    lines.push(field("Plan", plan.as_str()));
    lines.extend(clash_lines(clash));
    if let Some(effort) = &data.reasoning_effort {
        lines.push(field("Effort", effort.clone()));
    }
    if data.tokens.reasoning_output > 0 {
        lines.push(field(
            "Reasoning",
            format!("{} tokens", util::with_commas(data.tokens.reasoning_output)),
        ));
    }

    // Whose session this is, when it is not the reader's — and the reason the
    // account below is then left off: those credentials are this user's, and
    // another user's session is signed in as somebody else.
    if let Some(owner) = &session.owner {
        lines.push(field("User", owner.clone()));
    }
    let account = match session.provider {
        _ if session.owner.is_some() => None,
        Provider::Claude => crate::quota::claude_account(),
        Provider::Codex => crate::quota::codex_account(),
        Provider::Cursor
        | Provider::Gemini
        | Provider::OpenCode
        | Provider::Pi
        | Provider::Windsurf => None,
    };
    if let Some(a) = account {
        if let Some(email) = a.email {
            lines.push(field("Account", email));
        }
        if let Some(org) = a.organization {
            lines.push(field("Org", org));
        }
    }

    if let Some(started) = util::parse_ts(&session.started_at) {
        lines.push(field(
            "Started",
            started
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        ));
    }
    // API time is what the model actually spent working; wall time includes
    // every pause while the user was reading or typing.
    if data.metrics.api_duration_ms > 0 {
        lines.push(field(
            "API",
            util::long_duration(data.metrics.api_duration_ms as i64),
        ));
    }
    if let Some(wall_ms) = wall_duration_ms(session, Utc::now()) {
        lines.push(field("Wall", util::long_duration(wall_ms)));
    }

    let m = &data.metrics;
    // Only when there is something to say. The `ERR%` column carries the rate;
    // this carries the two numbers behind it, because "8%" of what is the first
    // thing anyone asks of a rate.
    if m.tool_errors > 0 {
        let rate = session.error_rate().unwrap_or(0.0);
        lines.push(Line::from(vec![
            label(&format!("{:<9}", "Failed")),
            Span::raw(" "),
            Span::styled(
                format!("{} of {} calls", m.tool_errors, m.tool_count),
                Style::default().fg(match rate {
                    r if r >= 0.25 => theme::colors().cost_high,
                    r if r >= 0.10 => theme::colors().cost_mid,
                    _ => theme::colors().dim,
                }),
            ),
            Span::raw("  "),
            dim(format!("{}%", (rate * 100.0).round() as i64)),
        ]));
    }
    if m.lines_added > 0 || m.lines_removed > 0 {
        lines.push(Line::from(vec![
            label(&format!("{:<9}", "Lines")),
            Span::raw(" "),
            Span::styled(
                format!("+{}", util::with_commas(m.lines_added)),
                Style::default().fg(theme::colors().cost_low),
            ),
            Span::raw("  "),
            Span::styled(
                format!("-{}", util::with_commas(m.lines_removed)),
                Style::default().fg(theme::colors().cost_high),
            ),
        ]));
    }

    if let Some(ctx) = &session.context {
        lines.push(Line::default());
        if session.is_compacting() {
            lines.push(Line::from(vec![
                label("Compaction"),
                Span::raw(" "),
                Span::styled(
                    "compacting…".to_string(),
                    Style::default()
                        .fg(theme::colors().cost_high)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else if ctx.compacted {
            // The percentage below would be of a window this session compacted
            // away, and nothing has measured what replaced it.
            lines.push(Line::from(vec![
                label("Compaction"),
                Span::raw(" "),
                dim("compacted; no request since"),
            ]));
        } else {
            let compact_at = (ctx.max as f64 * *crate::config::COMPACT_THRESHOLD).round() as u64;
            let pct = ctx.percent_to_compact();
            let color = theme::context_color(pct);
            lines.push(Line::from(vec![
                label("Compaction"),
                Span::raw(" "),
                Span::styled(
                    format!("{:>3}%", pct.round() as i64),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                label("used"),
                Span::raw(" "),
                value(util::compact_tokens(ctx.used)),
                Span::raw(" "),
                label("of"),
                Span::raw(" "),
                value(util::compact_tokens(compact_at)),
                Span::raw(" "),
                label("tokens"),
            ]));
            lines.push(Line::from(vec![
                Span::raw("           "),
                bar(pct / 100.0, 40, color),
            ]));
        }
    }

    lines
}

/// A horizontal meter: filled portion in `color`, remainder dimmed.
fn bar(ratio: f64, width: usize, color: ratatui::style::Color) -> Span<'static> {
    let filled = ((ratio.clamp(0.0, 1.0)) * width as f64).round() as usize;
    Span::styled(
        format!("{}{}", "━".repeat(filled), "─".repeat(width - filled)),
        Style::default().fg(color),
    )
}

// ---------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------

pub fn processes(session: &Session, width: usize) -> Vec<Line<'static>> {
    if session.surface == Surface::DesktopCowork {
        return note("Cowork sessions run in a cloud VM — no local process tree.");
    }
    if session.surface == Surface::Editor && session.provider == Provider::Cursor {
        return note("Cursor uses a shared editor process — no per-session process tree.");
    }
    let Some(pm) = &session.process else {
        return note("Process data is only available for running sessions.");
    };
    if pm.process_list.is_empty() {
        return note("No process data available.");
    }

    let mut procs = pm.process_list.clone();
    procs.sort_by(|a, b| {
        b.cpu
            .partial_cmp(&a.cpu)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let cmd_w = width.saturating_sub(7 + 2 + 6 + 2 + 8 + 2).max(10);
    let mut lines = Vec::new();
    if !pm.command.is_empty() {
        lines.push(Line::from(vec![
            label("Root"),
            Span::raw(" "),
            dim(util::truncate(
                &util::tildify(&pm.command),
                width.saturating_sub(6),
            )),
        ]));
    }
    lines.push(Line::from(Span::styled(
        format!("{:>7}  {:>6}  {:>8}  {}", "PID", "CPU%", "MEM", "COMMAND"),
        theme::header(),
    )));

    for p in procs {
        // A recently-exited child is shown greyed rather than vanishing, so a
        // burst of short-lived tool subprocesses doesn't make the list flicker.
        let base = if p.ghost {
            Style::default().fg(theme::colors().dim)
        } else if p.is_root {
            theme::value()
        } else {
            Style::default().fg(theme::gray(250))
        };
        let cpu_style = if p.ghost {
            base
        } else {
            Style::default().fg(theme::cpu_color(p.cpu))
        };

        let argv0 = p.args.split(' ').next().unwrap_or("");
        let name = argv0.rsplit('/').next().unwrap_or(argv0);
        let rest = p.args[argv0.len()..].trim_start();
        let cmd = if rest.is_empty() {
            name.to_string()
        } else {
            format!("{name} {rest}")
        };

        lines.push(Line::from(vec![
            Span::styled(format!("{:>7}", p.pid), base),
            Span::raw("  "),
            Span::styled(format!("{:>5.1}%", p.cpu), cpu_style),
            Span::raw("  "),
            Span::styled(format!("{:>8}", util::compact_bytes(p.memory)), base),
            Span::raw("  "),
            Span::styled(util::truncate(&cmd, cmd_w), base),
        ]));
    }
    lines
}

// ---------------------------------------------------------------------------
// Tool activity
// ---------------------------------------------------------------------------

/// Tool names for the sidebar, most-used first, with an "All" entry at index 0.
pub fn tool_tabs(data: &SessionData) -> Vec<(String, u64)> {
    let mut tools: Vec<(String, u64)> = data
        .metrics
        .tools
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    tools.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut out = vec![("All".to_string(), data.metrics.tool_count)];
    out.extend(tools);
    out
}

/// Invocation rows for the selected tool tab, oldest first.
/// Stable identity for one invocation, so an expanded row survives the log
/// growing beneath it. Row indices shift as new entries arrive; ids don't.
pub fn detail_key(d: &crate::session::ToolDetail) -> String {
    d.id.clone().unwrap_or_else(|| format!("{}|{}", d.ts, d.d))
}

/// Wrap text to `width`, breaking on the last space that fits.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut line = raw;
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        while line.chars().count() > width {
            // Prefer the last space that still fits; fall back to a hard cut
            // when a single token is longer than the whole line.
            let mut cut = None;
            for (i, c) in line.char_indices().take(width + 1) {
                if c == ' ' {
                    cut = Some(i);
                }
            }
            let cut = cut.unwrap_or_else(|| {
                line.char_indices()
                    .nth(width)
                    .map(|(i, _)| i)
                    .unwrap_or(line.len())
            });
            let cut = if cut == 0 { line.len().min(width) } else { cut };
            out.push(line[..cut].to_string());
            line = line[cut..].trim_start();
        }
        out.push(line.to_string());
    }
    out
}

/// Rendered rows plus, for each rendered line, the invocation it belongs to.
///
/// The caller needs that mapping to turn a mouse click on a screen row back into
/// the entry under it, since wrapped and diff lines make the relationship
/// non-uniform.
pub fn tool_activity(
    data: &SessionData,
    tab: usize,
    live_since: Option<&str>,
    show_diff: bool,
    expanded: Option<&str>,
    width: usize,
) -> (Vec<Line<'static>>, Vec<Option<String>>) {
    let tabs = tool_tabs(data);
    let bare = |lines: Vec<Line<'static>>| {
        let owners = vec![None; lines.len()];
        (lines, owners)
    };
    if data.metrics.tool_count == 0 {
        return bare(note("No tool invocations."));
    }
    let Some((name, _)) = tabs.get(tab) else {
        return bare(note("No tool selected."));
    };
    let all = tab == 0;

    let mut rows: Vec<(String, &crate::session::ToolDetail)> = Vec::new();
    for (tool, details) in &data.metrics.tool_details {
        if !all && tool != name {
            continue;
        }
        rows.extend(details.iter().map(|d| (tool.clone(), d)));
    }
    if let Some(since) = live_since {
        rows.retain(|(_, d)| d.ts.as_str() >= since);
    }
    rows.sort_by(|a, b| a.1.ts.cmp(&b.1.ts));

    if rows.is_empty() {
        return bare(note(if live_since.is_some() {
            "No invocations since cctop started."
        } else {
            "No invocations recorded."
        }));
    }

    let mut out = Vec::with_capacity(rows.len());
    let mut owners: Vec<Option<String>> = Vec::with_capacity(rows.len());
    for (tool, d) in rows {
        let key = detail_key(d);
        let is_open = expanded == Some(key.as_str());
        let ts = util::parse_ts(&d.ts)
            .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
            .unwrap_or_else(|| "     ".into());
        // The gap after the timestamp doubles as a failure marker, so a failed
        // call is legible without relying on the background colour alone — and
        // the row's width is unchanged either way.
        let mut spans = vec![
            dim(ts),
            if d.failed {
                Span::styled("✗", Style::default().fg(theme::colors().cost_high))
            } else {
                Span::raw(" ")
            },
        ];
        let mut used = 6;

        // Who made the call: the main session, or one of its subagents. Subagent
        // activity is interleaved into the same log, so without a marker there
        // is no way to tell an agent's edits from the parent's.
        let origin_tag = match &d.origin {
            None => "main".to_string(),
            Some(agent) => {
                let short = agent.strip_prefix("agent-").unwrap_or(agent);
                format!("↳{}", &short[..short.len().min(6)])
            }
        };
        let origin_style = if d.origin.is_some() {
            Style::default().fg(theme::colors().desktop_code)
        } else {
            Style::default().fg(theme::colors().dimmer)
        };
        used += 8;
        spans.push(Span::styled(format!("{origin_tag:<7} "), origin_style));

        if all {
            let pretty = util::pretty_mcp_name(&tool);
            used += pretty.chars().count() + 1;
            spans.push(Span::styled(
                format!("{pretty} "),
                Style::default().fg(theme::tool_color(&tool)),
            ));
        }

        // Right-hand metrics are built first so the detail column can claim
        // exactly the width they leave behind.
        let mut trailing: Vec<Span<'static>> = Vec::new();
        if let Some(delta) = &d.delta {
            trailing.push(Span::styled(
                format!(" +{}", delta.added),
                Style::default().fg(theme::colors().cost_low),
            ));
            trailing.push(Span::styled(
                format!(" -{}", delta.removed),
                Style::default().fg(theme::colors().cost_high),
            ));
        }
        if let Some(ms) = d.dur_ms {
            trailing.push(Span::styled(
                format!(" {:>7}", fmt_millis(ms)),
                theme::dim(),
            ));
        }
        if d.tokens_in > 0 || d.tokens_out > 0 {
            // `shared` marks a turn that issued several calls: the counts below
            // belong to the whole turn, not to this call alone.
            let mark = if d.shared > 1 { "*" } else { " " };
            trailing.push(Span::styled(
                format!(
                    "{mark}↓{:>6} ↑{:>5}",
                    util::compact_tokens(d.tokens_in),
                    util::compact_tokens(d.tokens_out)
                ),
                Style::default().fg(theme::colors().dim),
            ));
        }
        let trailing_w: usize = trailing.iter().map(|sp| sp.content.chars().count()).sum();

        let text = util::tildify(&d.d);
        let detail_w = width.saturating_sub(used + trailing_w).max(8);
        spans.push(value(format!(
            "{:<detail_w$}",
            util::truncate(&text, detail_w)
        )));
        spans.extend(trailing);
        let row_style = if d.failed {
            theme::failed()
        } else {
            Style::default()
        };
        out.push(Line::from(spans).style(row_style));
        owners.push(Some(key.clone()));

        // Expanded: show the untruncated argument, wrapped.
        if is_open {
            let full = d.full.as_deref().unwrap_or(&d.d);
            for line in wrap(full, width.saturating_sub(10)) {
                out.push(
                    Line::from(vec![
                        Span::raw("        "),
                        Span::styled(line, Style::default().fg(theme::gray(252))),
                    ])
                    .style(row_style),
                );
                owners.push(Some(key.clone()));
            }
        }

        if show_diff && let Some(delta) = &d.delta {
            for hunk in &delta.hunks {
                let style = match hunk.chars().next() {
                    Some('+') => Style::default().fg(theme::colors().cost_low),
                    Some('-') => Style::default().fg(theme::colors().cost_high),
                    _ => Style::default().fg(theme::colors().dimmer),
                };
                out.push(Line::from(vec![
                    Span::raw("        "),
                    Span::styled(util::truncate(hunk, width.saturating_sub(9)), style),
                ]));
                owners.push(Some(key.clone()));
            }
        }
    }
    (out, owners)
}

/// Sub-second durations read better in milliseconds than as `0s`.
fn fmt_millis(ms: i64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        util::compact_duration(ms)
    }
}

// ---------------------------------------------------------------------------
// Subagents
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentSort {
    Last,
    Type,
    Model,
    Description,
    Cost,
    Tools,
    Context,
    Duration,
}

impl SubagentSort {
    pub fn key(&self) -> &'static str {
        match self {
            SubagentSort::Last => "last",
            SubagentSort::Type => "type",
            SubagentSort::Model => "model",
            SubagentSort::Description => "desc",
            SubagentSort::Cost => "cost",
            SubagentSort::Tools => "tools",
            SubagentSort::Context => "ctx",
            SubagentSort::Duration => "dur",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "type" => SubagentSort::Type,
            "model" => SubagentSort::Model,
            "desc" => SubagentSort::Description,
            "cost" => SubagentSort::Cost,
            "tools" => SubagentSort::Tools,
            "ctx" => SubagentSort::Context,
            "dur" => SubagentSort::Duration,
            _ => SubagentSort::Last,
        }
    }
}

pub fn sort_subagents(list: &mut [Subagent], sort: SubagentSort, asc: bool) {
    list.sort_by(|a, b| {
        let ord = match sort {
            SubagentSort::Last => a
                .last_active
                .as_deref()
                .unwrap_or_default()
                .cmp(b.last_active.as_deref().unwrap_or_default()),
            SubagentSort::Type => a.agent_type.cmp(&b.agent_type),
            SubagentSort::Model => a.model.cmp(&b.model),
            SubagentSort::Description => a.description.cmp(&b.description),
            SubagentSort::Cost => a
                .cost
                .partial_cmp(&b.cost)
                .unwrap_or(std::cmp::Ordering::Equal),
            SubagentSort::Tools => a.tool_count.cmp(&b.tool_count),
            SubagentSort::Context => {
                let r = |s: &Subagent| s.context.map(|c| c.percent_to_compact()).unwrap_or(-1.0);
                r(a).partial_cmp(&r(b)).unwrap_or(std::cmp::Ordering::Equal)
            }
            SubagentSort::Duration => a.duration_ms.cmp(&b.duration_ms),
        }
        .then_with(|| a.agent_id.cmp(&b.agent_id));
        if asc { ord } else { ord.reverse() }
    });
}

pub fn subagents(
    data: Option<&SessionData>,
    sort: SubagentSort,
    asc: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let Some(data) = data else {
        return note("Loading…");
    };
    if data.subagents.is_empty() {
        return note("No subagents.");
    }
    let mut list = data.subagents.clone();
    sort_subagents(&mut list, sort, asc);

    let fixed = 6 + 2 + 2 + 12 + 1 + 12 + 1 + 8 + 1 + 6 + 1 + 5 + 1 + 7;
    let desc_w = width.saturating_sub(fixed).max(10);

    let mut lines = vec![Line::from(Span::styled(
        format!(
            "{:<6}   {:<12} {:<12} {:<desc_w$} {:>8} {:>6} {:>5} {:>7}",
            "LAST", "TYPE", "MODEL", "DESC", "COST", "TOOLS", "CTX", "TIME"
        ),
        theme::header(),
    ))];

    let now = chrono::Utc::now();
    for sa in list {
        let running = sa.status == SubagentStatus::Running;
        // A ghost's transcript was purged, so its per-agent metrics are gone;
        // showing zeros would read as "did nothing" rather than "unknown".
        let (icon, icon_color) = if sa.ghost {
            ("◌", theme::colors().dim)
        } else if running {
            ("●", theme::colors().cost_low)
        } else {
            ("○", theme::colors().dim)
        };
        let row_style = if sa.ghost {
            Style::default().fg(theme::colors().dim)
        } else if running {
            theme::value()
        } else {
            Style::default().fg(theme::gray(250))
        };

        let last = sa
            .last_active
            .as_deref()
            .or(sa.started_at.as_deref())
            .map(|t| util::relative_age(t, &now))
            .unwrap_or_else(|| "—".into());
        let unknown = |s: String| if sa.ghost { "—".to_string() } else { s };
        let cost = unknown(util::compact_usd(sa.cost));
        let tools = unknown(sa.tool_count.to_string());
        let time = unknown(util::compact_duration(sa.duration_ms));
        let ctx = match (sa.ghost, sa.context) {
            (false, Some(c)) => format!("{}%", c.percent_to_compact().round() as i64),
            _ => "—".into(),
        };

        lines.push(Line::from(vec![
            Span::styled(format!("{last:<6}"), row_style),
            Span::raw(" "),
            Span::styled(icon.to_string(), Style::default().fg(icon_color)),
            Span::raw(" "),
            Span::styled(
                format!("{:<12}", util::truncate(&sa.agent_type, 12)),
                row_style,
            ),
            Span::raw(" "),
            Span::styled(
                format!("{:<12}", util::truncate(&util::short_model(&sa.model), 12)),
                if sa.ghost {
                    row_style
                } else {
                    Style::default().fg(theme::model_color(&sa.model))
                },
            ),
            Span::raw(" "),
            Span::styled(
                format!("{:<desc_w$}", util::truncate(&sa.description, desc_w)),
                row_style,
            ),
            Span::raw(" "),
            Span::styled(
                format!("{cost:>8}"),
                if sa.ghost {
                    row_style
                } else {
                    Style::default().fg(theme::cost_color(sa.cost))
                },
            ),
            Span::raw(" "),
            Span::styled(format!("{tools:>6}"), row_style),
            Span::raw(" "),
            Span::styled(format!("{ctx:>5}"), row_style),
            Span::raw(" "),
            Span::styled(format!("{time:>7}"), row_style),
        ]));
    }
    lines
}

// ---------------------------------------------------------------------------
// Cost
// ---------------------------------------------------------------------------

pub fn cost(session: &Session, data: Option<&SessionData>, plan: Plan) -> Vec<Line<'static>> {
    let Some(data) = data else {
        return note("Loading…");
    };
    let included = plan.includes(session.provider);
    let mut lines = Vec::new();

    if !session.cost_available {
        return note("Cost and token usage are not present in Cursor transcripts.");
    }

    if included && !session.cost_is_free {
        lines.push(Line::from(vec![
            label("Total cost"),
            Span::raw("  "),
            dim("included in plan"),
        ]));
        lines.push(Line::default());
        lines.push(Line::from(dim(format!(
            "Retail-equivalent: {}",
            util::compact_usd(data.costs.total)
        ))));
        return lines;
    }

    lines.push(Line::from(vec![
        label("Total cost"),
        Span::raw("  "),
        Span::styled(
            displayed_cost(data.costs.total, session.cost_is_free),
            cost_style(data.costs.total, session.cost_is_free).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(dim(
        "estimate: tokens × published per-token rates",
    )));
    lines.push(Line::default());

    // Calendar-bucket spend windows.
    let now = chrono::Utc::now();
    let midnight = util::local_midnight_today();
    let today = util::local_date_key(&midnight);
    let week = util::local_date_key(&(midnight - chrono::Duration::days(6)));
    let month = util::local_date_key(&(midnight - chrono::Duration::days(29)));
    let hour = util::local_hour_key(&now);

    let sum_day = |from: &str| -> f64 {
        data.costs_by_day
            .iter()
            .filter(|(d, _)| d.as_str() >= from)
            .map(|(_, m)| m.values().sum::<f64>())
            .sum()
    };
    let hour_cost: f64 = data
        .costs_by_hour
        .get(&hour)
        .map(|m| m.values().sum())
        .unwrap_or(0.0);

    for (name, amount) in [
        ("this hour", hour_cost),
        ("today", sum_day(&today)),
        ("7 days", sum_day(&week)),
        ("30 days", sum_day(&month)),
    ] {
        lines.push(Line::from(vec![
            label(&format!("  {name:<10}")),
            Span::styled(
                format!("{:>10}", displayed_cost(amount, session.cost_is_free)),
                cost_style(amount, session.cost_is_free),
            ),
        ]));
    }

    // Per-model breakdown.
    for mb in &data.model_breakdown {
        let free_model = is_free_model(mb);
        lines.push(Line::default());
        lines.push(Line::from(vec![
            value(mb.model.clone()),
            Span::raw("  "),
            Span::styled(
                displayed_cost(mb.total, free_model),
                cost_style(mb.total, free_model),
            ),
        ]));
        let rows: [(&str, u64, f64); 5] = [
            ("in", mb.tokens.input, mb.costs.input),
            ("out", mb.tokens.output, mb.costs.output),
            (
                "cache↓",
                mb.tokens.cache_read + mb.tokens.cached_input,
                mb.costs.cache_read + mb.costs.cached_input,
            ),
            (
                "cache↑",
                mb.tokens.cache_write_5m + mb.tokens.cache_write_1h,
                mb.costs.cache_write_5m + mb.costs.cache_write_1h,
            ),
            ("reasoning", mb.tokens.reasoning_output, 0.0),
        ];
        for (name, tokens, amount) in rows {
            if tokens == 0 {
                continue;
            }
            lines.push(Line::from(vec![
                label(&format!("  {name:<9}")),
                value(format!("{:>8}", util::compact_tokens(tokens))),
                Span::raw("  "),
                Span::styled(
                    format!("{:>10}", displayed_cost(amount, free_model)),
                    cost_style(amount, free_model),
                ),
            ]));
        }
    }

    if let Some(r) = &data.rates {
        lines.push(Line::default());
        lines.push(Line::from(dim(format!(
            "rates per 1M tokens — in ${:.2}  cached ${:.3}  out ${:.2}",
            r.input, r.cached_input, r.output
        ))));
    }

    lines
}

// ---------------------------------------------------------------------------
// Context breakdown
// ---------------------------------------------------------------------------

/// One category of what the window holds.
struct Slice {
    name: &'static str,
    tokens: u64,
    color: ratatui::style::Color,
    /// What the bar and the legend swatch are drawn with. Solid for everything
    /// the window holds; the free remainder is shaded so that "nothing is here
    /// yet" does not read as one more category.
    fill: char,
}

impl Slice {
    fn held(name: &'static str, tokens: u64, color: ratatui::style::Color) -> Self {
        Slice {
            name,
            tokens,
            color,
            fill: '█',
        }
    }
}

/// What the context window is filled with, largest share first.
///
/// The panel's job is to be believed, so it never pretends the parts add up.
/// `Startup` is measured and the other categories are estimated from transcript
/// characters, and whatever the two together fail to reach gets its own share
/// instead of being spread over the categories that happen to be measurable.
///
/// The shares are drawn as one stacked bar rather than as a bar apiece: the
/// question the panel answers is what proportion of a *single* window each
/// category holds, and separate bars make that a comparison between rows instead
/// of something the eye reads off at once. It also leaves room for the window's
/// unused remainder, which is the part a bar-per-row cannot show at all.
pub fn context(session: &Session, data: Option<&SessionData>, width: usize) -> Vec<Line<'static>> {
    let Some(data) = data else {
        return note("Loading…");
    };
    let Some(b) = data.context_breakdown else {
        return note("This transcript reports no per-request usage — nothing to break down.");
    };

    let palette = theme::colors();
    let mut slices = vec![
        // Named for what it holds rather than "system prompt", because after a
        // compaction the summary is folded into the same number.
        Slice::held("Startup", b.startup, palette.panel_title),
        Slice::held("Tool output", b.tool_output, palette.accent),
        Slice::held("Tool input", b.tool_input, palette.chart_hues[0]),
        Slice::held("Attachments", b.attachments, palette.name_hue),
        Slice::held("Your messages", b.user_text, palette.cost_low),
        Slice::held("Assistant text", b.assistant_text, palette.chart_hues[1]),
    ];
    slices.retain(|s| s.tokens > 0);
    slices.sort_by_key(|s| std::cmp::Reverse(s.tokens));
    // Pinned last wherever it lands by size: it is the leftover, and it belongs
    // against the free remainder rather than in the middle of the measured
    // categories.
    let unaccounted = b.unaccounted();
    if unaccounted > 0 {
        slices.push(Slice::held(
            "Unaccounted",
            unaccounted as u64,
            theme::colors().dim,
        ));
    }
    // What is still free, so the bar is the whole window rather than only the
    // part already spent — which is what makes the used portion's length mean
    // something at a glance.
    let free = session
        .context
        .map(|ctx| ctx.max.saturating_sub(b.total))
        .unwrap_or(0);
    if free > 0 {
        slices.push(Slice {
            name: "Free",
            tokens: free,
            color: theme::colors().dimmer,
            fill: '░',
        });
    }

    let compaction = compaction_cell(session, &slices, b.superseded, width);
    let mut lines = vec![context_header(session, &b)];
    lines.push(Line::default());
    lines.push(stacked_bar(&slices, compaction, width));
    lines.push(Line::default());
    lines.extend(legend(&slices, width));
    lines.push(Line::default());
    lines.extend(context_footnotes(
        &b,
        unaccounted,
        compaction.is_some(),
        width,
    ));
    lines.extend(context_timeline(session, data, width));
    lines
}

/// The window across every request the session made, under the bar that shows
/// what it currently holds.
///
/// The bar answers "what is in there"; this answers "how did it get that full",
/// which is the question that changes what anyone does next. A window that
/// climbed evenly is a conversation that grew and will keep growing. One that
/// stepped is a handful of large tool results, and the same call will do it
/// again. A sawtooth is a session living on compactions, paying to rebuild its
/// context over and over.
fn context_timeline(session: &Session, data: &SessionData, width: usize) -> Vec<Line<'static>> {
    // Two points is a line between two measurements, which says nothing a
    // reader could not get from the header. Below that there is no shape.
    if data.context_series.len() < 3 || width < 24 {
        return Vec::new();
    }
    let values: Vec<f64> = data
        .context_series
        .iter()
        .map(|p| p.window as f64)
        .collect();
    // Scaled to the window rather than to the tallest point, so the chart's
    // height means the same thing as the bar above it: how full, not how much
    // taller than the rest of this session.
    let max = session
        .context
        .map(|c| c.max as f64)
        .filter(|m| *m > 0.0)
        .unwrap_or_else(|| values.iter().cloned().fold(1.0, f64::max));

    // The counted total, not the markers in the series: that is decimated once
    // it passes its cap, and a chart that lost a marker would under-report the
    // one thing it is being read for.
    let compactions = data.compactions as usize;
    let peak = values.iter().cloned().fold(0.0, f64::max);

    let mut lines = vec![
        Line::default(),
        Line::from(vec![
            label("How it filled  "),
            dim(format!("{} requests", data.context_series.len())),
            dim("   peak "),
            value(crate::util::compact_tokens(peak as u64)),
            match compactions {
                0 => dim(String::new()),
                // Named on the chart because they are the only drops in it: a
                // fall with no compaction behind it would be a measurement
                // error, and telling the two apart matters.
                n => dim(format!(
                    "   {n} compaction{}",
                    if n == 1 { "" } else { "s" }
                )),
            },
        ]),
    ];
    lines.extend(crate::ui::spark::line_chart(
        &values,
        width,
        5,
        max,
        theme::Gradient::Accent,
        None,
    ));
    lines.extend(thrash_note(session, compactions));
    lines
}

/// What a session living on compactions is paying for, spelled out.
///
/// The sawtooth is visible in the chart above, but only to someone who already
/// knows what it means. A cadence is not: three compactions over two days is a
/// long conversation, three in twenty minutes is a session that will spend the
/// rest of the day rebuilding a window it keeps refilling, and the chart draws
/// those identically.
fn thrash_note(session: &Session, compactions: usize) -> Vec<Line<'static>> {
    /// Below this there is no cadence yet, only a long conversation.
    const MIN_COMPACTIONS: usize = 3;

    if compactions < MIN_COMPACTIONS {
        return Vec::new();
    }
    let (Some(start), Some(end)) = (
        util::parse_ts(&session.started_at),
        util::parse_ts(&session.last_active),
    ) else {
        return Vec::new();
    };
    let span_ms = (end.timestamp_millis() - start.timestamp_millis()).max(0);
    let every = span_ms / compactions as i64;
    if every <= 0 {
        return Vec::new();
    }
    vec![Line::from(vec![
        Span::styled(
            format!(
                "↺ one compaction every {}",
                crate::util::compact_duration(every)
            ),
            Style::default().fg(theme::colors().cost_mid),
        ),
        dim(" — each one re-sends the conversation as a summary the model has to read again"),
    ])]
}

/// Which bar cell the auto-compact threshold falls on, when it is still ahead.
///
/// Marking it turns the free remainder into two readable parts: the room that is
/// genuinely usable, and the tail past the threshold that the harness will
/// reclaim before it is ever reached. `None` once the threshold is behind — the
/// header already says so in red, and a marker there would erase a category.
fn compaction_cell(
    session: &Session,
    slices: &[Slice],
    superseded: bool,
    width: usize,
) -> Option<usize> {
    let ctx = session.context?;
    let scale = slices.iter().map(|s| s.tokens).sum::<u64>();
    // Nothing to point at once the compaction has happened: the bar is a window
    // that has already been reclaimed, and a threshold ahead of it is a claim
    // about a window nobody has measured.
    if scale == 0 || superseded {
        return None;
    }
    let compact_at = ctx.max as f64 * *crate::config::COMPACT_THRESHOLD;
    let cell = (compact_at / scale as f64 * width as f64).round() as usize;
    // Only inside the free tail: elsewhere it would overwrite something held.
    let held: u64 = slices
        .iter()
        .filter(|s| s.name != "Free")
        .map(|s| s.tokens)
        .sum();
    let free_starts = (held as f64 / scale as f64 * width as f64).round() as usize;
    (cell > free_starts && cell < width).then_some(cell)
}

/// Window size, how full it is, and how much is left before auto-compaction.
///
/// Headroom in tokens rather than only a percentage: "how much can I still say"
/// is the decision this panel is consulted for, and a share of a window whose
/// size varies by model does not answer it. The gauge rides on the same line
/// because fullness is the one thing here worth seeing without reading.
fn context_header(session: &Session, b: &crate::session::ContextBreakdown) -> Line<'static> {
    let mut spans = vec![
        label("Window"),
        Span::raw("  "),
        value(util::compact_tokens(b.total)),
    ];
    let Some(ctx) = session.context else {
        spans.push(Span::raw(" "));
        spans.push(dim("in the live conversation"));
        return Line::from(spans);
    };

    spans.push(dim(format!(" of {}", util::compact_tokens(ctx.max))));
    // Headroom is the one thing that cannot be stated across a compaction: the
    // window below is the one that was compacted away, so how much is "left" in
    // the one replacing it is not known until its first request lands.
    if b.superseded {
        spans.push(Span::raw("   "));
        spans.push(if session.is_compacting() {
            Span::styled(
                "compacting…",
                Style::default()
                    .fg(theme::colors().cost_high)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            dim("as it stood before the last compaction")
        });
        return Line::from(spans);
    }

    let pct = ctx.percent_to_compact();
    let color = theme::context_color(pct);
    let compact_at = (ctx.max as f64 * *crate::config::COMPACT_THRESHOLD).round() as u64;
    // No gauge here: the bar below already shows how full the window is, and a
    // second meter measuring a *different* denominator — share of the threshold
    // rather than of the window — is two answers to one question.
    spans.push(Span::raw("   "));
    spans.push(Span::styled(
        format!("{}%", pct.round() as i64),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ));
    spans.push(dim(" to compaction"));
    spans.push(Span::raw("   "));
    spans.push(value(util::compact_tokens(
        compact_at.saturating_sub(b.total),
    )));
    spans.push(dim(" left"));
    Line::from(spans)
}

/// Every category in one bar, in the legend's order, spanning the panel.
fn stacked_bar(slices: &[Slice], compaction: Option<usize>, width: usize) -> Line<'static> {
    let cells = width.max(1);
    let weights: Vec<u64> = slices.iter().map(|s| s.tokens).collect();

    // Laid out cell by cell so the threshold marker can replace one of them: a
    // marker appended as its own span would push the bar a cell past the panel.
    let mut cell_styles: Vec<(char, ratatui::style::Color)> = slices
        .iter()
        .zip(apportion(&weights, cells))
        .flat_map(|(slice, w)| std::iter::repeat_n((slice.fill, slice.color), w))
        .collect();
    if let Some(at) = compaction
        && let Some(cell) = cell_styles.get_mut(at)
    {
        *cell = ('┊', theme::colors().dim);
    }

    // Runs of one style become one span; a span per cell would allocate a String
    // per column of the panel, on every frame.
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (glyph, color) in cell_styles {
        match spans.last_mut() {
            Some(last) if last.style.fg == Some(color) => last.content.to_mut().push(glyph),
            _ => spans.push(Span::styled(glyph.to_string(), Style::default().fg(color))),
        }
    }
    Line::from(spans)
}

/// Split `cells` across `weights` in proportion, summing to exactly `cells`.
///
/// Largest remainder rather than a rounded share apiece: independent rounding
/// leaves the bar a cell or two short of the panel width, and in a stacked bar
/// that error lands on the boundary between two colours, which is exactly where
/// the eye is already looking.
fn apportion(weights: &[u64], cells: usize) -> Vec<usize> {
    let scale: u64 = weights.iter().sum::<u64>().max(1);
    let exact: Vec<f64> = weights
        .iter()
        .map(|w| *w as f64 / scale as f64 * cells as f64)
        .collect();
    let mut out: Vec<usize> = exact.iter().map(|e| e.floor() as usize).collect();

    let mut spare = cells.saturating_sub(out.iter().sum::<usize>());
    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|a, b| {
        let frac = |i: usize| exact[i] - exact[i].floor();
        frac(*b).total_cmp(&frac(*a))
    });
    for i in order {
        if spare == 0 {
            break;
        }
        out[i] += 1;
        spare -= 1;
    }
    out
}

/// Swatch, name, tokens and share per category, two to a line where it fits.
///
/// Shares are of the whole window, free space included, so a legend entry and
/// its segment in the bar above are always the same length of the same thing.
fn legend(slices: &[Slice], width: usize) -> Vec<Line<'static>> {
    let scale = slices.iter().map(|s| s.tokens).sum::<u64>().max(1);
    // Two columns halve the panel's height for free, but need the room; below
    // that the entries stack rather than truncate.
    let columns = if width >= 64 { 2 } else { 1 };

    let mut lines = Vec::new();
    for row in slices.chunks(columns) {
        let mut spans = Vec::new();
        for (i, slice) in row.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw("   "));
            }
            let share = slice.tokens as f64 / scale as f64 * 100.0;
            spans.push(Span::styled(
                format!("{} ", slice.fill),
                Style::default().fg(slice.color),
            ));
            spans.push(value(format!("{:<14}", slice.name)));
            spans.push(Span::styled(
                format!("{:>7}", util::compact_tokens(slice.tokens)),
                Style::default().fg(slice.color),
            ));
            spans.push(dim(format!(" {:>3}%", share.round() as i64)));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// Which numbers were measured and which were guessed.
///
/// The distinction is the point of the panel, so it stays on screen — but as two
/// dim lines under the bar rather than as the three paragraphs it takes to say
/// the same thing in prose.
fn context_footnotes(
    b: &crate::session::ContextBreakdown,
    unaccounted: i64,
    marked: bool,
    width: usize,
) -> Vec<Line<'static>> {
    let marker = if marked {
        " ┊ on the bar is where auto-compaction triggers."
    } else {
        ""
    };
    let startup = if b.after_compaction {
        "Measured: Window, and Startup — this segment's first request (system prompt, tool schemas, CLAUDE.md, skills index, compaction summary), which the transcript cannot split further."
    } else {
        "Measured: Window, and Startup — the first request (system prompt, tool schemas, CLAUDE.md, skills index), which the transcript cannot split further."
    };
    let rest = if unaccounted >= 0 {
        "Estimated from transcript characters: everything else — read as proportions. Unaccounted is thinking (stored stripped), the harness's per-turn reminders, and estimation error."
    } else {
        "Estimated from transcript characters: everything else — read as proportions. Here they overshoot the window, which means the harness has dropped context the transcript still holds."
    };
    // Said outright rather than left to the header: every number in the panel
    // describes a window that no longer exists, and that is not something to
    // infer from a missing threshold marker.
    let superseded = if b.superseded {
        " A compaction has since replaced this window; nothing has measured what took its place."
    } else {
        ""
    };
    [startup.to_string(), format!("{rest}{marker}{superseded}")]
        .iter()
        .flat_map(|note| wrap(note, width.max(20)))
        .map(|l| Line::from(dim(l)))
        .collect()
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Read a file's first `max_lines` lines, prefixed by a header.
fn file_section(path: &Path, display: &str, max_lines: usize) -> Option<Vec<Line<'static>>> {
    let content = util::read_head(path, 32 * 1024)?;
    let mut out = vec![Line::from(Span::styled(
        display.to_string(),
        Style::default()
            .fg(theme::colors().border_hi)
            .add_modifier(Modifier::BOLD),
    ))];
    let src: Vec<&str> = content.lines().collect();
    for line in src.iter().take(max_lines) {
        out.push(Line::from(Span::raw(format!(
            "  {}",
            line.replace('\t', "    ")
        ))));
    }
    if src.len() > max_lines {
        out.push(Line::from(dim("  … (truncated)")));
    }
    out.push(Line::default());
    Some(out)
}

fn missing(text: String) -> Line<'static> {
    Line::from(Span::styled(
        text,
        Style::default().fg(theme::colors().dimmer),
    ))
}

/// Instructions, memory, skills, and MCP servers backing this session.
pub fn config(session: &Session) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let cwd = Path::new(&session.label_source);

    match session.provider {
        Provider::Claude => {
            let root = match (session.surface.is_desktop(), &session.mac_meta) {
                (true, Some(m)) => m.session_dir.join(".claude"),
                _ => crate::config::CLAUDE_CONFIG_DIR.clone(),
            };

            lines.push(Line::from(Span::styled(
                "── Instructions ──".to_string(),
                theme::title(),
            )));
            let global = root.join("CLAUDE.md");
            match file_section(&global, &util::tildify(&global.to_string_lossy()), 30) {
                Some(block) => lines.extend(block),
                None => lines.push(missing(format!("{} not found", global.display()))),
            }
            if !session.label_source.is_empty() {
                match file_section(&cwd.join("CLAUDE.md"), "./CLAUDE.md", 40) {
                    Some(block) => lines.extend(block),
                    None => lines.push(missing("./CLAUDE.md not found".into())),
                }
            }

            lines.push(Line::from(Span::styled(
                "── Skills ──".to_string(),
                theme::title(),
            )));
            lines.extend(skill_list(&root.join("skills")));

            lines.push(Line::from(Span::styled(
                "── MCP ──".to_string(),
                theme::title(),
            )));
            lines.extend(mcp_from_json(&root.join("settings.json"), "global"));
            if !session.label_source.is_empty() {
                lines.extend(mcp_from_json(&cwd.join(".mcp.json"), "project"));
            }
        }
        Provider::Codex => {
            lines.push(Line::from(Span::styled(
                "── Instructions ──".to_string(),
                theme::title(),
            )));
            let global = crate::config::CODEX_HOME.join("AGENTS.md");
            match file_section(&global, "~/.codex/AGENTS.md", 30) {
                Some(block) => lines.extend(block),
                None => lines.push(missing("~/.codex/AGENTS.md not found".into())),
            }
            if !session.label_source.is_empty() {
                match file_section(&cwd.join("AGENTS.md"), "./AGENTS.md", 40) {
                    Some(block) => lines.extend(block),
                    None => lines.push(missing("./AGENTS.md not found".into())),
                }
            }

            lines.push(Line::from(Span::styled(
                "── Config ──".to_string(),
                theme::title(),
            )));
            let toml = crate::config::CODEX_HOME.join("config.toml");
            match file_section(&toml, "~/.codex/config.toml", 30) {
                Some(block) => lines.extend(block),
                None => lines.push(missing("~/.codex/config.toml not found".into())),
            }

            lines.push(Line::from(Span::styled(
                "── Skills ──".to_string(),
                theme::title(),
            )));
            lines.extend(skill_list(&crate::config::CODEX_HOME.join("skills")));

            lines.push(Line::from(Span::styled(
                "── MCP ──".to_string(),
                theme::title(),
            )));
            lines.extend(mcp_from_toml(&toml));
        }
        Provider::Cursor => {
            lines.push(Line::from(Span::styled(
                "── Cursor ──".to_string(),
                theme::title(),
            )));
            lines.push(Line::from(dim(
                "Native agent transcripts do not expose model, token, context, or cost data.",
            )));
            lines.push(Line::from(dim(format!(
                "Transcript: {}",
                session
                    .data_file
                    .as_ref()
                    .map(|p| util::tildify(&p.to_string_lossy()))
                    .unwrap_or_else(|| "unknown".into())
            ))));
        }
        Provider::OpenCode => {
            lines.push(Line::from(Span::styled(
                "── Instructions ──".to_string(),
                theme::title(),
            )));
            if !session.label_source.is_empty() {
                match file_section(&cwd.join("AGENTS.md"), "./AGENTS.md", 40) {
                    Some(block) => lines.extend(block),
                    None => lines.push(missing("./AGENTS.md not found".into())),
                }
            }
            lines.push(Line::from(Span::styled(
                "── Config ──".to_string(),
                theme::title(),
            )));
            let config = dirs::config_dir()
                .unwrap_or_else(|| crate::config::HOME.join(".config"))
                .join("opencode")
                .join("opencode.json");
            match file_section(&config, &util::tildify(&config.to_string_lossy()), 30) {
                Some(block) => lines.extend(block),
                None => lines.push(missing(format!("{} not found", config.display()))),
            }
        }
        Provider::Pi => {
            lines.push(Line::from(Span::styled(
                "── Instructions ──".to_string(),
                theme::title(),
            )));
            let global = crate::config::PI_AGENT_DIR.join("AGENTS.md");
            match file_section(&global, &util::tildify(&global.to_string_lossy()), 30) {
                Some(block) => lines.extend(block),
                None => lines.push(missing(format!("{} not found", global.display()))),
            }
            if !session.label_source.is_empty() {
                match file_section(&cwd.join("AGENTS.md"), "./AGENTS.md", 40) {
                    Some(block) => lines.extend(block),
                    None => lines.push(missing("./AGENTS.md not found".into())),
                }
            }
            lines.push(Line::from(Span::styled(
                "── Config ──".to_string(),
                theme::title(),
            )));
            let settings = crate::config::PI_AGENT_DIR.join("settings.json");
            match file_section(&settings, &util::tildify(&settings.to_string_lossy()), 30) {
                Some(block) => lines.extend(block),
                None => lines.push(missing(format!("{} not found", settings.display()))),
            }
            lines.push(Line::from(Span::styled(
                "── Skills ──".to_string(),
                theme::title(),
            )));
            lines.extend(skill_list(&crate::config::PI_AGENT_DIR.join("skills")));
        }
        Provider::Gemini => {
            lines.push(Line::from(Span::styled(
                "── Instructions ──".to_string(),
                theme::title(),
            )));
            let global = crate::config::GEMINI_HOME.join("GEMINI.md");
            match file_section(&global, &util::tildify(&global.to_string_lossy()), 30) {
                Some(block) => lines.extend(block),
                None => lines.push(missing(format!("{} not found", global.display()))),
            }
            if !session.label_source.is_empty() {
                match file_section(&cwd.join("GEMINI.md"), "./GEMINI.md", 40) {
                    Some(block) => lines.extend(block),
                    None => lines.push(missing("./GEMINI.md not found".into())),
                }
            }
            lines.push(Line::from(Span::styled(
                "── Config ──".to_string(),
                theme::title(),
            )));
            let settings = crate::config::GEMINI_HOME.join("settings.json");
            match file_section(&settings, &util::tildify(&settings.to_string_lossy()), 30) {
                Some(block) => lines.extend(block),
                None => lines.push(missing(format!("{} not found", settings.display()))),
            }
            lines.push(Line::from(Span::styled(
                "── Skills ──".to_string(),
                theme::title(),
            )));
            lines.extend(skill_list(&crate::config::GEMINI_HOME.join("skills")));
        }
        Provider::Windsurf => {
            // Windsurf's global rules live in the editor's own settings UI, not
            // in a file cctop can point at; only the workspace rules are on disk.
            lines.push(Line::from(Span::styled(
                "── Instructions ──".to_string(),
                theme::title(),
            )));
            match file_section(&cwd.join(".windsurfrules"), "./.windsurfrules", 40) {
                Some(block) => lines.extend(block),
                None => lines.push(missing("./.windsurfrules not found".into())),
            }
        }
    }
    lines
}

/// Skill names and descriptions read from each `SKILL.md` front matter.
fn skill_list(dir: &Path) -> Vec<Line<'static>> {
    if !dir.is_dir() {
        return vec![missing(format!("No skills installed ({})", dir.display()))];
    }
    let mut out = Vec::new();
    for entry in crate::config::list_dir(dir) {
        let skill_md = dir.join(&entry).join("SKILL.md");
        let (mut name, mut desc) = (entry.clone(), String::new());
        if let Some(text) = util::read_head(&skill_md, 4096) {
            for line in text.lines().take(20) {
                if let Some(v) = line.strip_prefix("name:") {
                    name = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("description:") {
                    desc = v.trim().to_string();
                }
            }
        }
        let mut spans = vec![Span::styled(
            name,
            Style::default().fg(theme::colors().cost_low),
        )];
        if !desc.is_empty() {
            spans.push(Span::raw("  "));
            spans.push(dim(util::truncate(&desc, 80)));
        }
        out.push(Line::from(spans));
    }
    if out.is_empty() {
        out.push(missing("No skills installed".into()));
    }
    out
}

fn mcp_from_json(path: &Path, scope: &str) -> Vec<Line<'static>> {
    let Some(text) = util::read_head(path, 64 * 1024) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    // Project `.mcp.json` may hold the servers at the top level.
    let servers = v.get("mcpServers").unwrap_or(&v);
    let Some(map) = servers.as_object() else {
        return Vec::new();
    };
    map.iter()
        .filter(|(_, cfg)| cfg.is_object())
        .map(|(name, cfg)| {
            let mut spans = vec![
                Span::styled(name.clone(), Style::default().fg(theme::colors().name_hue)),
                Span::raw("  "),
                dim(format!("({scope})")),
            ];
            if let Some(cmd) = cfg.get("command").and_then(|c| c.as_str()) {
                spans.push(Span::raw("  "));
                spans.push(dim(cmd.to_string()));
            }
            Line::from(spans)
        })
        .collect()
}

fn mcp_from_toml(path: &Path) -> Vec<Line<'static>> {
    let Some(text) = util::read_head(path, 64 * 1024) else {
        return vec![missing("No MCP servers configured".into())];
    };
    let out: Vec<Line<'static>> = text
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("[mcp_servers.")
                .and_then(|r| r.strip_suffix(']'))
                .map(|name| {
                    Line::from(Span::styled(
                        name.to_string(),
                        Style::default().fg(theme::colors().name_hue),
                    ))
                })
        })
        .collect();
    if out.is_empty() {
        vec![missing("No MCP servers in config.toml".into())]
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{ContextUsage, SubagentStatus};

    fn series(windows: &[u64]) -> SessionData {
        SessionData {
            context_series: windows
                .iter()
                .enumerate()
                .map(|(i, w)| crate::session::CtxPoint {
                    ts: format!("2026-08-05T10:{i:02}:00+00:00"),
                    window: *w,
                    after_compaction: false,
                })
                .collect(),
            ..SessionData::default()
        }
    }

    /// The chart says how the window filled, which needs a shape to show. Two
    /// points are a straight line between two numbers the header already
    /// prints, so the section stays off rather than drawing a truism.
    #[test]
    fn the_context_chart_needs_more_than_a_pair_of_points() {
        let session = Session::new(crate::pricing::Provider::Claude, "x".into());
        assert!(context_timeline(&session, &series(&[10, 20]), 80).is_empty());
        assert!(!context_timeline(&session, &series(&[10, 20, 30]), 80).is_empty());
    }

    /// A narrow panel has no room for a chart, and drawing one anyway would push
    /// the bar and legend — which do fit — off the top.
    #[test]
    fn the_context_chart_stays_off_a_narrow_panel() {
        let session = Session::new(crate::pricing::Provider::Claude, "x".into());
        assert!(context_timeline(&session, &series(&[10, 20, 30, 40]), 12).is_empty());
    }

    /// Compactions are the only drops in the chart, so the header counts them:
    /// a fall with nothing behind it would otherwise read as a measurement bug.
    ///
    /// The count is the session's own, not the markers left in the series: that
    /// series is decimated once it outgrows its cap, and a chart that lost a
    /// marker would under-report the one number it is being read for.
    #[test]
    fn the_context_chart_counts_the_compactions_it_drew() {
        let session = Session::new(crate::pricing::Provider::Claude, "x".into());
        let mut data = series(&[10, 90, 20, 40]);
        data.context_series[2].after_compaction = true;
        data.compactions = 1;
        let text: String = context_timeline(&session, &data, 80)
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(text.contains("1 compaction"), "got {text:?}");
        assert!(text.contains("4 requests"), "got {text:?}");

        // A marker the decimation dropped must not cost the header a count.
        data.compactions = 3;
        data.context_series[2].after_compaction = false;
        let text: String = context_timeline(&session, &data, 80)
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(text.contains("3 compactions"), "got {text:?}");
    }

    /// Three compactions over two days is a long conversation; three in twenty
    /// minutes is a session that will spend the day rebuilding a window it
    /// keeps refilling. The chart draws those identically, so the cadence is
    /// spelled out under it — and only once there is a cadence to spell.
    #[test]
    fn a_thrashing_session_is_told_how_often_it_is_rebuilding() {
        let mut session = Session::new(crate::pricing::Provider::Claude, "x".into());
        session.started_at = "2026-08-11T10:00:00Z".into();
        session.last_active = "2026-08-11T11:00:00Z".into();

        assert!(thrash_note(&session, 2).is_empty(), "two is not a cadence");
        let text: String = thrash_note(&session, 4)
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(text.contains("every 15m"), "got {text:?}");

        // A session with no clock behind it says nothing rather than dividing
        // by a span it does not have.
        session.last_active = session.started_at.clone();
        assert!(thrash_note(&session, 4).is_empty());
    }

    /// A failed call is marked two ways on purpose: the red wash, and a glyph in
    /// the gap after the timestamp so the row still reads on a terminal that
    /// drops background colour.
    #[test]
    fn failed_tool_calls_are_marked_in_the_activity_rows() {
        let mut data = SessionData::default();
        for (command, failed) in [("true", false), ("exit 1", true)] {
            data.metrics
                .tool_details
                .entry("Bash".to_string())
                .or_default()
                .push(crate::session::ToolDetail {
                    d: command.to_string(),
                    ts: "2026-08-05T10:00:00+00:00".to_string(),
                    failed,
                    ..Default::default()
                });
        }
        data.metrics.tool_count = 2;

        let (lines, _) = tool_activity(&data, 0, None, false, None, 120);
        let row_of = |needle: &str| {
            lines
                .iter()
                .find(|l| l.spans.iter().any(|s| s.content.contains(needle)))
                .unwrap_or_else(|| panic!("no row for {needle}"))
        };

        let ok = row_of("true");
        assert_eq!(ok.style.bg, None, "a successful call keeps the normal row");
        assert!(!ok.spans.iter().any(|s| s.content.contains('✗')));

        let bad = row_of("exit 1");
        assert_eq!(bad.style.bg, Some(theme::colors().failed_bg));
        assert!(
            bad.spans.iter().any(|s| s.content.contains('✗')),
            "the failure must not be conveyed by colour alone"
        );
    }

    fn subagent(id: &str, cost: f64, ghost: bool) -> Subagent {
        Subagent {
            agent_id: id.into(),
            agent_type: "Explore".into(),
            description: "look around".into(),
            model: "claude-haiku-4-5-20251001".into(),
            started_at: Some("2026-01-01T00:00:00Z".into()),
            last_active: Some("2026-01-01T00:01:00Z".into()),
            duration_ms: 60_000,
            status: SubagentStatus::Done,
            cost,
            tool_count: 3,
            tool_use_id: None,
            context: Some(ContextUsage {
                used: 1000,
                max: 200_000,
                compacted: false,
            }),
            ghost,
        }
    }

    #[test]
    fn subagent_sort_respects_direction() {
        let mut list = vec![subagent("a", 1.0, false), subagent("b", 5.0, false)];
        sort_subagents(&mut list, SubagentSort::Cost, true);
        assert_eq!(list[0].agent_id, "a");
        sort_subagents(&mut list, SubagentSort::Cost, false);
        assert_eq!(list[0].agent_id, "b");
    }

    #[test]
    fn ghost_subagents_show_unknown_not_zero() {
        let data = SessionData {
            subagents: vec![subagent("g", 0.0, true)],
            ..Default::default()
        };
        let lines = subagents(Some(&data), SubagentSort::Last, false, 120);
        let row: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(row.contains('◌'), "ghost needs its own marker: {row}");
        assert!(
            !row.contains("$0.00"),
            "purged transcript must not report $0.00 as if measured: {row}"
        );
        assert!(row.contains('—'));
    }

    #[test]
    fn tool_tabs_are_ordered_by_use_with_all_first() {
        let mut data = SessionData::default();
        data.metrics.tools.insert("Read".into(), 3);
        data.metrics.tools.insert("Bash".into(), 9);
        data.metrics.tool_count = 12;
        let tabs = tool_tabs(&data);
        assert_eq!(tabs[0].0, "All");
        assert_eq!(tabs[0].1, 12);
        assert_eq!(tabs[1].0, "Bash");
        assert_eq!(tabs[2].0, "Read");
    }

    #[test]
    fn tool_activity_live_filter_excludes_older_entries() {
        let mut data = SessionData::default();
        data.metrics.tool_count = 2;
        data.metrics.tools.insert("Bash".into(), 2);
        data.metrics.tool_details.insert(
            "Bash".into(),
            vec![
                crate::session::ToolDetail {
                    d: "old".into(),
                    ts: "2026-01-01T00:00:00Z".into(),
                    ..Default::default()
                },
                crate::session::ToolDetail {
                    d: "new".into(),
                    ts: "2026-06-01T00:00:00Z".into(),
                    ..Default::default()
                },
            ],
        );
        let (lines, _) = tool_activity(&data, 0, Some("2026-03-01T00:00:00Z"), false, None, 80);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(text.contains("new"));
        assert!(!text.contains("old"));
    }

    #[test]
    fn info_reports_extraction_errors_instead_of_blank() {
        let s = Session::new(Provider::Claude, "x".into());
        let data = SessionData {
            error: Some("boom".into()),
            ..Default::default()
        };
        let lines = info(&s, Some(&data), Plan::Retail, None);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(text.contains("boom"));
    }

    #[test]
    fn wall_time_advances_for_live_sessions_and_freezes_when_stopped() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:02:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut session = Session::new(Provider::Claude, "x".into());
        session.started_at = "2026-01-01T00:00:00Z".into();
        session.last_active = "2026-01-01T00:00:49Z".into();

        assert_eq!(wall_duration_ms(&session, now), Some(49_000));

        session.process = Some(crate::proc::ProcInfo::default());
        assert_eq!(wall_duration_ms(&session, now), Some(120_000));
    }

    #[test]
    fn bundled_plan_cost_panel_still_shows_retail_equivalent() {
        let s = Session::new(Provider::Claude, "x".into());
        let data = SessionData {
            costs: crate::session::Costs {
                total: 4.25,
                ..Default::default()
            },
            ..Default::default()
        };
        let lines = cost(&s, Some(&data), Plan::Max);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(text.contains("included in plan"));
        assert!(text.contains("$4.25"));
    }

    #[test]
    fn cost_panel_labels_free_model_usage() {
        let mut s = Session::new(Provider::OpenCode, "x".into());
        s.cost_is_free = true;
        let data = SessionData {
            model_breakdown: vec![crate::session::ModelBreakdown {
                model: "deepseek-v4-flash-free".into(),
                tokens: crate::session::Tokens {
                    input: 100,
                    output: 20,
                    cache_read: 50,
                    reasoning_output: 10,
                    total: 180,
                    ..Default::default()
                },
                costs: crate::session::Costs::default(),
                total: 0.0,
            }],
            ..Default::default()
        };
        let lines = cost(&s, Some(&data), Plan::Retail);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(text.contains("deepseek-v4-flash-free  FREE"));
        assert!(!text.contains("$0.00"));
        assert!(text.matches("FREE").count() >= 8, "{text}");
    }

    /// The panel exists to say what is in the window, so the part it cannot
    /// explain has to be as visible as the parts it can. Folding the shortfall
    /// into the measurable categories would make every bar a lie.
    #[test]
    fn context_panel_shows_the_gap_rather_than_hiding_it() {
        let s = Session::new(Provider::Claude, "x".into());
        let data = SessionData {
            context_breakdown: Some(crate::session::ContextBreakdown {
                total: 100_000,
                startup: 20_000,
                tool_output: 30_000,
                tool_input: 5_000,
                ..Default::default()
            }),
            ..Default::default()
        };
        let lines = rendered(&s, &data, 100);
        let text = lines.join("\n");
        let entries = legend_entries(&lines);

        // 100k window, 55k attributed: the gap is 45%, the biggest single share.
        // It still reads last, because it is the leftover and belongs at the tail
        // of the bar rather than in the middle of the measured categories.
        let gap = entries.last().expect("the legend must have entries");
        assert!(gap.starts_with("Unaccounted"), "{entries:?}");
        assert!(gap.contains("45%"), "{gap}");
        // Shares are of what the window holds, so they account for all of it.
        let shares: i64 = entries.iter().filter_map(|e| share_of(e)).sum();
        assert!((99..=101).contains(&shares), "shares summed to {shares}");
        assert!(
            text.contains("Estimated"),
            "the estimate must say so: {text}"
        );
    }

    /// When the estimate overshoots, saying so beats drawing bars that run off
    /// the panel — the overshoot means the harness dropped context the
    /// transcript still holds, which is worth knowing.
    #[test]
    fn context_panel_admits_when_the_estimate_exceeds_the_window() {
        let s = Session::new(Provider::Claude, "x".into());
        let data = SessionData {
            context_breakdown: Some(crate::session::ContextBreakdown {
                total: 50_000,
                startup: 20_000,
                tool_output: 60_000,
                ..Default::default()
            }),
            ..Default::default()
        };
        let text = rendered(&s, &data, 100).join("\n");
        assert!(!text.contains("Unaccounted"), "there is no gap to report");
        assert!(text.contains("overshoot"), "{text}");
        // Shares are measured against the larger of the two, so the bar cannot
        // run past the panel.
        assert!(text.contains("75%"), "60k of 80k: {text}");
    }

    /// Once a compaction has replaced the window, headroom in it is not a thing
    /// anyone has measured. Quoting a figure anyway — in the header or as the
    /// threshold marker on the bar — would invite the reader to plan the next
    /// turn around a window that no longer exists.
    #[test]
    fn a_superseded_window_is_shown_as_past_rather_than_as_headroom() {
        let mut s = Session::new(Provider::Claude, "x".into());
        s.context = Some(ContextUsage {
            used: 100_000,
            max: 200_000,
            compacted: true,
        });
        let data = SessionData {
            context_breakdown: Some(crate::session::ContextBreakdown {
                total: 100_000,
                startup: 20_000,
                tool_output: 30_000,
                superseded: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        let text = rendered(&s, &data, 100).join("\n");
        assert!(text.contains("before the last compaction"), "{text}");
        assert!(!text.contains("left"), "no headroom claim: {text}");
        assert!(!text.contains('┊'), "no threshold marker: {text}");

        // Still running, so the same transcript reads as a compaction in flight —
        // which is what the CTX% column says for it too.
        s.inferred_running = true;
        let text = rendered(&s, &data, 100).join("\n");
        assert!(text.contains("compacting…"), "{text}");
    }

    #[test]
    fn processes_panel_explains_cowork_absence() {
        let mut s = Session::new(Provider::Claude, "x".into());
        s.surface = Surface::DesktopCowork;
        let text: String = processes(&s, 80)[0]
            .spans
            .iter()
            .map(|sp| sp.content.to_string())
            .collect();
        assert!(text.contains("cloud VM"));
    }

    /// A stacked bar has to land exactly on the panel width whatever the shares
    /// round to, or its right edge wanders against everything drawn beside it.
    #[test]
    fn apportioning_a_bar_always_spends_every_cell() {
        for weights in [
            vec![1u64, 1, 1],        // thirds, which never divide evenly
            vec![999_999, 1],        // a share too small for one cell
            vec![7, 11, 13, 17, 19], // primes, so no share is exact
            vec![0, 0, 5],           // categories that contributed nothing
        ] {
            for cells in [10usize, 37, 96] {
                let parts = apportion(&weights, cells);
                assert_eq!(
                    parts.iter().sum::<usize>(),
                    cells,
                    "{weights:?} over {cells} cells"
                );
            }
        }
    }

    fn breakdown() -> crate::session::ContextBreakdown {
        crate::session::ContextBreakdown {
            total: 118_200,
            startup: 45_200,
            tool_output: 32_100,
            tool_input: 18_000,
            attachments: 2_100,
            user_text: 8_100,
            assistant_text: 6_900,
            after_compaction: false,
            superseded: false,
        }
    }

    fn rendered(session: &Session, data: &SessionData, width: usize) -> Vec<String> {
        context(session, Some(data), width)
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.to_string()).collect())
            .collect()
    }

    /// The legend's entries in reading order, however many share a line.
    ///
    /// A legend line is a swatch followed by a space, which is what tells it
    /// apart from the solid bar above it.
    fn legend_entries(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .filter(|l| l.starts_with("█ ") || l.starts_with("░ "))
            .flat_map(|l| l.split(['█', '░']))
            .map(|e| e.trim().to_string())
            .filter(|e| e.ends_with('%'))
            .collect()
    }

    /// The trailing `NN%` of a legend entry.
    fn share_of(entry: &str) -> Option<i64> {
        entry.trim_end_matches('%').rsplit(' ').next()?.parse().ok()
    }

    /// The header carries the two numbers a running session is consulted for —
    /// how full the window is and how much room is left — and the bar underneath
    /// spans the panel exactly.
    #[test]
    fn the_context_panel_leads_with_headroom_and_a_full_width_bar() {
        let mut s = Session::new(Provider::Claude, "x".into());
        s.context = Some(ContextUsage {
            used: 118_200,
            max: 200_000,
            compacted: false,
        });
        let data = SessionData {
            context_breakdown: Some(breakdown()),
            ..Default::default()
        };

        let lines = rendered(&s, &data, 100);
        assert!(lines[0].contains("118.2K of 200.0K"), "{}", lines[0]);
        assert!(lines[0].contains("to compaction"), "{}", lines[0]);
        assert!(lines[0].contains("left"), "{}", lines[0]);
        assert_eq!(
            lines[2].chars().count(),
            100,
            "the stacked bar must fill the panel: {}",
            lines[2]
        );
        // 118.2K of a 200K window: held cells and free cells in that proportion.
        assert_eq!(lines[2].chars().filter(|c| *c == '█').count(), 59);
        assert!(
            lines[2].ends_with('░'),
            "free space must trail the bar: {}",
            lines[2]
        );
        // The marker lands on the auto-compaction threshold, wherever the
        // harness (or its env override) puts it.
        let expected = (*crate::config::COMPACT_THRESHOLD * 100.0).round() as usize;
        assert_eq!(
            lines[2].find('┊').map(|i| lines[2][..i].chars().count()),
            Some(expected),
            "{}",
            lines[2]
        );

        let entries = legend_entries(&lines);
        let free = entries.last().expect("the legend must have entries");
        assert!(free.starts_with("Free"), "{entries:?}");
        assert!(free.contains("81.8K"), "{free}");
    }
}
