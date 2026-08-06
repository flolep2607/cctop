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
        theme::DIMMER
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

pub fn info(session: &Session, data: Option<&SessionData>, plan: Plan) -> Vec<Line<'static>> {
    let Some(data) = data else {
        return note("Loading…");
    };
    if let Some(err) = &data.error {
        return vec![
            Line::from(Span::styled(
                "Could not read this session:".to_string(),
                Style::default().fg(theme::COST_HIGH),
            )),
            Line::from(dim(err.clone())),
        ];
    }

    let mut lines = Vec::new();
    let provider_color = match session.surface {
        Surface::DesktopCowork => theme::DESKTOP_COWORK,
        Surface::DesktopCode => theme::DESKTOP_CODE,
        Surface::Editor => theme::CURSOR,
        Surface::Cli => match session.provider {
            Provider::Claude => theme::CLAUDE,
            Provider::Codex => theme::OPENAI,
            Provider::Cursor => theme::CURSOR,
            Provider::OpenCode => theme::OPENCODE,
            Provider::Pi => theme::PI,
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
        Provider::OpenCode => format!("opencode --session {}", session.session_id),
        Provider::Pi => format!("pi --session {}", session.session_id),
    };
    lines.push(field("Cmd", cmd));
    lines.push(field("Plan", plan.as_str()));
    if let Some(effort) = &data.reasoning_effort {
        lines.push(field("Effort", effort.clone()));
    }
    if data.tokens.reasoning_output > 0 {
        lines.push(field(
            "Reasoning",
            format!("{} tokens", util::with_commas(data.tokens.reasoning_output)),
        ));
    }

    let account = match session.provider {
        Provider::Claude => crate::quota::claude_account(),
        Provider::Codex => crate::quota::codex_account(),
        Provider::Cursor | Provider::OpenCode | Provider::Pi => None,
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
    if m.lines_added > 0 || m.lines_removed > 0 {
        lines.push(Line::from(vec![
            label(&format!("{:<9}", "Lines")),
            Span::raw(" "),
            Span::styled(
                format!("+{}", util::with_commas(m.lines_added)),
                Style::default().fg(theme::COST_LOW),
            ),
            Span::raw("  "),
            Span::styled(
                format!("-{}", util::with_commas(m.lines_removed)),
                Style::default().fg(theme::COST_HIGH),
            ),
        ]));
    }

    if let Some(ctx) = &session.context {
        lines.push(Line::default());
        if ctx.compacting {
            lines.push(Line::from(vec![
                label("Compaction"),
                Span::raw(" "),
                Span::styled(
                    "compacting…".to_string(),
                    Style::default()
                        .fg(theme::COST_HIGH)
                        .add_modifier(Modifier::BOLD),
                ),
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
        Style::default()
            .fg(ratatui::style::Color::White)
            .bg(theme::HEADER_BG)
            .add_modifier(Modifier::BOLD),
    )));

    for p in procs {
        // A recently-exited child is shown greyed rather than vanishing, so a
        // burst of short-lived tool subprocesses doesn't make the list flicker.
        let base = if p.ghost {
            Style::default().fg(theme::DIM)
        } else if p.is_root {
            theme::value()
        } else {
            Style::default().fg(ratatui::style::Color::Indexed(250))
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
                Span::styled("✗", Style::default().fg(theme::COST_HIGH))
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
            Style::default().fg(theme::DESKTOP_CODE)
        } else {
            Style::default().fg(theme::DIMMER)
        };
        used += 8;
        spans.push(Span::styled(format!("{origin_tag:<7} "), origin_style));

        if all {
            let pretty = util::pretty_mcp_name(&tool);
            used += pretty.chars().count() + 1;
            spans.push(Span::styled(
                format!("{pretty} "),
                Style::default().fg(tool_color(&tool)),
            ));
        }

        // Right-hand metrics are built first so the detail column can claim
        // exactly the width they leave behind.
        let mut trailing: Vec<Span<'static>> = Vec::new();
        if let Some(delta) = &d.delta {
            trailing.push(Span::styled(
                format!(" +{}", delta.added),
                Style::default().fg(theme::COST_LOW),
            ));
            trailing.push(Span::styled(
                format!(" -{}", delta.removed),
                Style::default().fg(theme::COST_HIGH),
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
                Style::default().fg(theme::DIM),
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
            Style::default().bg(theme::FAILED_BG)
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
                        Span::styled(
                            line,
                            Style::default().fg(ratatui::style::Color::Indexed(252)),
                        ),
                    ])
                    .style(row_style),
                );
                owners.push(Some(key.clone()));
            }
        }

        if show_diff && let Some(delta) = &d.delta {
            for hunk in &delta.hunks {
                let style = match hunk.chars().next() {
                    Some('+') => Style::default().fg(theme::COST_LOW),
                    Some('-') => Style::default().fg(theme::COST_HIGH),
                    _ => Style::default().fg(theme::DIMMER),
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

/// Stable per-tool colour so the same tool keeps its hue between refreshes.
fn tool_color(name: &str) -> ratatui::style::Color {
    const PALETTE: [u8; 10] = [75, 114, 173, 180, 139, 109, 146, 215, 152, 167];
    let hash = name
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    ratatui::style::Color::Indexed(PALETTE[(hash as usize) % PALETTE.len()])
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
        Style::default()
            .fg(ratatui::style::Color::White)
            .bg(theme::HEADER_BG)
            .add_modifier(Modifier::BOLD),
    ))];

    let now = chrono::Utc::now();
    for sa in list {
        let running = sa.status == SubagentStatus::Running;
        // A ghost's transcript was purged, so its per-agent metrics are gone;
        // showing zeros would read as "did nothing" rather than "unknown".
        let (icon, icon_color) = if sa.ghost {
            ("◌", theme::DIM)
        } else if running {
            ("●", theme::COST_LOW)
        } else {
            ("○", theme::DIM)
        };
        let row_style = if sa.ghost {
            Style::default().fg(theme::DIM)
        } else if running {
            theme::value()
        } else {
            Style::default().fg(ratatui::style::Color::Indexed(250))
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

/// What the context window is filled with, largest share first.
///
/// The panel's job is to be believed, so it never pretends the parts add up.
/// `Startup` is measured and the other categories are estimated from transcript
/// characters, and whatever the two together fail to reach gets its own bar
/// instead of being spread over the categories that happen to be measurable.
pub fn context(session: &Session, data: Option<&SessionData>, width: usize) -> Vec<Line<'static>> {
    let Some(data) = data else {
        return note("Loading…");
    };
    let Some(b) = data.context_breakdown else {
        return note("This transcript reports no per-request usage — nothing to break down.");
    };

    let mut rows: Vec<(&str, u64, ratatui::style::Color)> = vec![
        // Named for what it holds rather than "system prompt", because after a
        // compaction the summary is folded into the same number.
        ("Startup", b.startup, theme::PANEL_TITLE),
        (
            "Tool output",
            b.tool_output,
            ratatui::style::Color::Indexed(75),
        ),
        (
            "Tool input",
            b.tool_input,
            ratatui::style::Color::Indexed(109),
        ),
        (
            "Attachments",
            b.attachments,
            ratatui::style::Color::Indexed(180),
        ),
        ("Your messages", b.user_text, theme::COST_LOW),
        (
            "Assistant text",
            b.assistant_text,
            ratatui::style::Color::Indexed(139),
        ),
    ];
    let unaccounted = b.unaccounted();
    if unaccounted > 0 {
        rows.push(("Unaccounted", unaccounted as u64, theme::DIM));
    }
    rows.retain(|(_, tokens, _)| *tokens > 0);
    rows.sort_by_key(|(_, tokens, _)| std::cmp::Reverse(*tokens));

    // When the estimate overshoots there is no gap to draw, and scaling the bars
    // to the window would push them past the panel edge. Measure them against
    // whichever is larger and let the footer explain the discrepancy.
    let scale = b.total.max(b.startup + b.estimated()).max(1);

    let bar_w = width.saturating_sub(28).clamp(10, 48);
    let mut lines = vec![Line::from(vec![
        label("Window"),
        Span::raw("    "),
        value(util::compact_tokens(b.total)),
        Span::raw(" "),
        dim(match session.context {
            Some(ctx) => format!("of {}", util::compact_tokens(ctx.max)),
            None => "in the live conversation".to_string(),
        }),
    ])];
    lines.push(Line::default());

    for (name, tokens, color) in rows {
        let ratio = tokens as f64 / scale as f64;
        lines.push(Line::from(vec![
            Span::styled(format!("{name:<14} "), theme::value()),
            bar(ratio, bar_w, color),
            Span::styled(
                format!(" {:>7}", util::compact_tokens(tokens)),
                Style::default().fg(color),
            ),
            dim(format!(" {:>3}%", (ratio * 100.0).round() as i64)),
        ]));
    }

    // Which numbers were measured and which were guessed is the point of the
    // panel, so the caveats belong next to the bars rather than in the README.
    lines.push(Line::default());
    let footnotes = [
        if b.after_compaction {
            "Startup is measured: this segment's first request — system prompt, tool schemas, CLAUDE.md, the skills index, and the compaction summary. The transcript never records its parts, so it cannot be split further."
        } else {
            "Startup is measured: the first request — system prompt, tool schemas, CLAUDE.md, the skills index. The transcript never records its parts, so it cannot be split further."
        },
        "Every other bar is estimated from how many characters the transcript holds, so read them as proportions rather than as counts.",
        if unaccounted >= 0 {
            "Unaccounted is what neither reaches: thinking, which is stored with its text stripped; the reminders the harness injects each turn; and estimation error."
        } else {
            "The estimate overshoots the window, which means the harness has dropped context that the transcript still holds."
        },
    ];
    for note in footnotes {
        lines.extend(
            wrap(note, width.max(20))
                .into_iter()
                .map(|l| Line::from(dim(l))),
        );
    }
    lines
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
            .fg(theme::BORDER_HI)
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
    Line::from(Span::styled(text, Style::default().fg(theme::DIMMER)))
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
        let mut spans = vec![Span::styled(name, Style::default().fg(theme::COST_LOW))];
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
                Span::styled(
                    name.clone(),
                    Style::default().fg(ratatui::style::Color::Indexed(180)),
                ),
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
                        Style::default().fg(ratatui::style::Color::Indexed(180)),
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
        assert_eq!(bad.style.bg, Some(theme::FAILED_BG));
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
                compacting: false,
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
        let lines = info(&s, Some(&data), Plan::Retail);
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
        let lines = context(&s, Some(&data), 100);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();

        assert!(text.contains("Unaccounted"), "{text}");
        // 100k window, 55k attributed: the gap is 45%, and it is the biggest
        // single share, so it must sort to the top.
        let first_bar = lines[2]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect::<String>();
        assert!(first_bar.starts_with("Unaccounted"), "{first_bar}");
        assert!(first_bar.contains("45%"), "{first_bar}");
        assert!(
            text.contains("estimated"),
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
        let lines = context(&s, Some(&data), 100);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(!text.contains("Unaccounted"), "there is no gap to report");
        assert!(text.contains("overshoots"), "{text}");
        // Bars are measured against the larger of the two, so none can exceed
        // the panel width.
        assert!(text.contains("75%"), "60k of 80k: {text}");
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
}
