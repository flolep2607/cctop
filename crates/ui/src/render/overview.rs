//! The overview: what is being spent, how fast, and on which models.

use super::*;

/// The longest series the Overview charts: one calendar month, one dot a day.
/// Every chart is drawn at this width or narrower and right-aligned into it, so
/// the newest sample of each row sits in the same column.
const SPARK_W: usize = 31;

/// Content rows the panel has, below its border.
const ROWS: usize = 4;

/// Scale floors, in the unit of each series.
///
/// Auto-scaling makes a chart's own peak full height, which is right for a busy
/// window and a lie for an idle one: a tenth of a cent an hour becomes a
/// mountain range, and the Overview reads as frantic while nothing is running.
/// Below these the chart scales to the floor instead, so an amount nobody would
/// call spending draws as the flat line it is.
/// Width of the right-hand machine column, and the least room the spend
/// breakdown is worth drawing in.
const MACHINE_W: usize = 42;

const MIN_GRID_W: usize = 30;

/// Target width of one project cell in the breakdown grid.
const CELL_W: usize = 27;

const RATE_FLOOR: f64 = 0.05; // $/min — $3 an hour
const HOUR_FLOOR: f64 = 1.00; // $/hour
const DAY_FLOOR: f64 = 10.00; // $/day

/// A sparkline right-aligned into `width`, scaled to its own peak or `floor`,
/// whichever is larger.
///
/// The lead is spaces rather than the baseline dots `sparkline` would pad with.
/// A dot is a reading, and the hours of today that have not happened yet have
/// not read zero — filling them made every quiet row an indistinguishable wall
/// of dots, which is the whole reason this panel was unreadable.
fn spark_spans(
    values: &[f64],
    width: usize,
    floor: f64,
    gradient: Gradient,
    now: Option<usize>,
) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let drawn = width.min(values.len().max(1));
    let peak = values.iter().cloned().fold(0.0f64, f64::max);
    let mut spans = Vec::with_capacity(drawn + 1);
    if width > drawn {
        spans.push(Span::raw(" ".repeat(width - drawn)));
    }
    spans.extend(spark::sparkline(values, drawn, peak.max(floor) * 1.1, gradient, now).spans);
    spans
}

/// `name  value` right-aligned into `width`, with the name truncated rather
/// than the amount — the amount is why the row is on the list.
fn ranked_row(name: &str, amount: f64, width: usize) -> Vec<Span<'static>> {
    let money = util::adaptive_usd(amount);
    let name_w = width.saturating_sub(money.chars().count() + 1);
    // A cut name that still looks like a whole one names the wrong project.
    let mut shown: String = match name.chars().count() > name_w && name_w > 0 {
        true => name
            .chars()
            .take(name_w - 1)
            .chain(std::iter::once('…'))
            .collect(),
        false => name.chars().take(name_w).collect(),
    };
    while shown.chars().count() < name_w {
        shown.push(' ');
    }
    vec![
        Span::styled(shown, theme::dim()),
        Span::raw(" "),
        Span::styled(money, Style::default().fg(theme::colors().cost_mid)),
    ]
}

/// Append each group to `spans` while the whole group still fits in `width`,
/// stopping at the first that does not.
///
/// The Overview's machine column is narrow on an 80-column terminal, and a
/// figure clipped at the edge reads as a different figure: `73.2K in` cut to
/// `73.2`, `96%` cut to `96`. A group is a figure with its unit, so it is shown
/// whole or not at all.
fn push_while_fits(spans: &mut Vec<Span<'static>>, groups: Vec<Vec<Span<'static>>>, width: usize) {
    let mut used: usize = spans.iter().map(Span::width).sum();
    for group in groups {
        let w: usize = group.iter().map(Span::width).sum();
        if used + w > width {
            break;
        }
        used += w;
        spans.extend(group);
    }
}

/// Models that get a hue of their own in the spend bar; the rest share one.
const CHARTED_MODELS: usize = 4;

/// The fewest cells the spend bar is drawn in. Under this a cell is more than
/// an eighth of the day, too coarse for a length to say more than the
/// percentages beside it, so the bar leaves.
const SPEND_BAR_MIN: usize = 8;

const SPEND_BAR_MAX: usize = 16;

/// The hue of the `rank`-th model in today's spend, largest first.
///
/// By rank rather than by vendor: [`theme::model_color`] paints every Claude
/// model the same, and the mix worth seeing is usually opus against haiku. The
/// slots are the palette's categorical ones, already folded to the terminal's
/// depth, so a sixteen-colour screen gets sixteen-colour segments.
fn model_hue(rank: usize) -> Color {
    let p = theme::colors();
    [p.accent, p.chart_hues[0], p.name_hue, p.chart_hues[1]]
        .get(rank)
        .copied()
        .unwrap_or(p.dim)
}

/// Today's spend per model as one stacked bar, in whatever of `width` the
/// names beside it leave.
///
/// A bar rather than more percentages because the row's question is the mix,
/// and a mix is a proportion the eye reads off a length faster than it adds up
/// numbers. Models past [`CHARTED_MODELS`] fold into one dim tail rather than
/// being dropped, so the bar is always the whole of today.
///
/// The names come first and the bar takes what they leave: the percentages are
/// what the row said before it had a bar, and a bar that pushed the second
/// model's name off the row would have traded a number for a colour. Empty when
/// that leaves fewer than [`SPEND_BAR_MIN`] cells — a squeezed bar is a smudge,
/// and the names alone still answer the question.
fn spend_bar(models: &[(String, f64)], width: usize) -> Vec<Span<'static>> {
    let today: f64 = models.iter().map(|m| m.1).sum();
    if models.is_empty() || today <= 0.0 {
        return Vec::new();
    }
    // Each entry is drawn as ` name NN%` beside the bar; the two leading
    // models must fit, the rest may fall off the end.
    let entry = |(name, cost): &(String, f64)| {
        name.chars().count() + format!(" {:.0}%", cost / today * 100.0).len() + 1
    };
    let names: usize = models.iter().take(2).map(entry).sum();
    let bar_w = width.saturating_sub(names).min(SPEND_BAR_MAX);
    if bar_w < SPEND_BAR_MIN {
        return Vec::new();
    }
    let mut weights: Vec<u64> = models
        .iter()
        .take(CHARTED_MODELS)
        .map(|(_, cost)| (cost.max(0.0) * 10_000.0).round() as u64)
        .collect();
    let rest: f64 = models.iter().skip(CHARTED_MODELS).map(|m| m.1).sum();
    if rest > 0.0 {
        weights.push((rest * 10_000.0).round() as u64);
    }
    panels::apportion(&weights, bar_w)
        .into_iter()
        .enumerate()
        .filter(|(_, n)| *n > 0)
        .map(|(rank, n)| Span::styled("█".repeat(n), Style::default().fg(model_hue(rank))))
        .collect()
}

pub(super) fn draw_overview(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel_block("Overview");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let label_w = 11usize;
    let value_w = 12usize;
    let gutter = 2usize;

    // The charts shrink before anything else, and the two right-hand columns
    // drop off entirely rather than being squeezed into a width where a project
    // name is three letters. A narrow terminal gets the spend rows, which is
    // what the panel is for.
    let spark_w = SPARK_W.min(
        (inner.width as usize)
            .saturating_sub(label_w + value_w + 1 + gutter)
            .min(SPARK_W),
    );
    let left_w = (label_w + value_w + 1 + spark_w + gutter) as u16;
    let rest = inner.width.saturating_sub(left_w) as usize;
    // The machine's figures are a fixed width — they say the same thing on a
    // 100-column terminal as on a 300-column one. Every column beyond that goes
    // to the spend breakdown, which has more to say the more room it is given.
    let (mid_w, right_w) = match rest {
        r if r >= MACHINE_W + MIN_GRID_W => (r - MACHINE_W, MACHINE_W),
        r if r >= 20 => (0, r),
        _ => (0, 0),
    };

    let cols = RLayout::horizontal([
        Constraint::Length(left_w),
        Constraint::Length(mid_w as u16),
        Constraint::Length(right_w as u16),
    ])
    .split(inner);

    let now = chrono::Local::now();
    let hour_idx = Some(chrono::Timelike::hour(&now) as usize);
    let day_of_month = chrono::Datelike::day(&now) as usize;
    let day_idx = Some(day_of_month - 1);
    let rt_idx = app.global_spend.values().len().checked_sub(1);

    // --- Spend -------------------------------------------------------------
    let row = |name: &str, amount: f64, series: &[f64], floor: f64, now_idx: Option<usize>| {
        let mut spans = vec![
            Span::styled(format!("{name:<label_w$}"), theme::label()),
            Span::styled(
                format!("{:>value_w$} ", util::adaptive_usd(amount)),
                Style::default()
                    .fg(theme::colors().cost_mid)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        spans.extend(spark_spans(
            series,
            spark_w,
            floor,
            Gradient::Spend,
            now_idx,
        ));
        Line::from(spans)
    };

    // A month-to-date average is the one number that says whether today was
    // ordinary, and it is a division of two figures already on the panel.
    let per_day = app.stats.spend_calendar_month / day_of_month as f64;

    let left_lines = vec![
        row(
            "Live rate",
            app.stats.spend_per_min,
            app.global_spend.values(),
            RATE_FLOOR,
            rt_idx,
        ),
        row(
            "Today",
            app.stats.spend_today,
            &app.stats.daily_hourly,
            HOUR_FLOOR,
            hour_idx,
        ),
        row(
            "This month",
            app.stats.spend_calendar_month,
            &app.stats.monthly_daily,
            DAY_FLOOR,
            day_idx,
        ),
        // Every session ever recorded, across every provider. Deliberately without
        // a sparkline: the others chart a window that scrolls, and a running total
        // only ever climbs, so a chart of it says nothing the number doesn't. The
        // space goes to the month's daily average instead, which does.
        Line::from(vec![
            Span::styled(format!("{:<label_w$}", "All time"), theme::label()),
            Span::styled(
                format!("{:>value_w$} ", util::adaptive_usd(app.stats.spend_total)),
                Style::default()
                    .fg(theme::colors().cost_mid)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}/day this month", util::adaptive_usd(per_day)),
                theme::dim(),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(left_lines), cols[0]);

    // --- Where today's money went ------------------------------------------
    //
    // The width that used to be filler is a ranking instead: today's spend by
    // project, read down each sub-column and then across. Four rows is what the
    // panel has, so a wider terminal buys more projects rather than more air.
    if mid_w > 0 {
        let head_w = 10usize;
        let grid_w = mid_w.saturating_sub(head_w);
        // Sub-columns sized near CELL_W rather than as many as will fit: two
        // columns of forty put the project at one end of the row and its amount
        // at the other, which is harder to read than three tighter ones.
        let sub_cols = ((grid_w + CELL_W / 2) / CELL_W).max(1);
        let cell_w = grid_w / sub_cols;
        let capacity = sub_cols * ROWS;

        let mut mid_lines: Vec<Line<'static>> = Vec::with_capacity(ROWS);
        let top = &app.stats.top_today;

        if top.is_empty() {
            mid_lines.push(Line::from(vec![
                Span::styled(format!("{:<head_w$}", "Top today"), theme::label()),
                Span::styled("nothing spent yet today", theme::dim()),
            ]));
        } else {
            let shown = capacity.min(top.len());
            // The tail is one row saying how much was left out, never a silent
            // truncation: a ranking you cannot tell is partial is a wrong total.
            let hidden = top.len() - shown;
            for r in 0..ROWS {
                let head = match r {
                    0 => format!("{:<head_w$}", "Top today"),
                    _ => " ".repeat(head_w),
                };
                let mut spans = vec![Span::styled(head, theme::label())];
                for c in 0..sub_cols {
                    let i = c * ROWS + r;
                    let last = i + 1 == shown;
                    if i >= shown {
                        break;
                    }
                    if hidden > 0 && last {
                        let rest: f64 = top[i..].iter().map(|e| e.1).sum();
                        spans.extend(ranked_row(
                            &format!("+{} more", hidden + 1),
                            rest,
                            cell_w.saturating_sub(2),
                        ));
                    } else {
                        spans.extend(ranked_row(&top[i].0, top[i].1, cell_w.saturating_sub(2)));
                    }
                    spans.push(Span::raw("  "));
                }
                mid_lines.push(Line::from(spans));
            }
        }
        frame.render_widget(Paragraph::new(mid_lines), cols[1]);
    }

    // --- The machine --------------------------------------------------------
    if right_w > 0 {
        let r_label_w = 10usize;
        let body_w = right_w.saturating_sub(r_label_w);
        let stats = &app.stats;

        let mut model_spans = vec![Span::styled(
            format!("{:<r_label_w$}", "Models"),
            theme::label(),
        )];
        let today: f64 = stats.models_today.iter().map(|m| m.1).sum();
        if today <= 0.0 {
            model_spans.push(Span::styled("idle today", theme::dim()));
        } else {
            // Percentages of today's spend, not a count of sessions ever seen:
            // the mix that is costing money is the one worth naming.
            let bar = spend_bar(&stats.models_today, body_w);
            let charted = !bar.is_empty();
            model_spans.extend(bar);
            let mut groups = Vec::new();
            for (i, (name, cost)) in stats
                .models_today
                .iter()
                .take(if charted { CHARTED_MODELS } else { 3 })
                .enumerate()
            {
                // Beside the bar each name wears its segment's hue, which is
                // the legend; without one the plain dotted list is clearer.
                let (sep, style) = match charted {
                    true => (" ", Style::default().fg(model_hue(i))),
                    false if i > 0 => (" · ", theme::value()),
                    false => ("", theme::value()),
                };
                groups.push(vec![
                    Span::styled(sep, theme::dim()),
                    Span::styled(name.clone(), style),
                ]);
                groups.push(vec![Span::styled(
                    format!(" {:.0}%", cost / today * 100.0),
                    theme::dim(),
                )]);
            }
            push_while_fits(&mut model_spans, groups, right_w);
        }

        let mem_mb = stats.total_memory as f64 / (1024.0 * 1024.0);
        let cpu_text = format!("{:.1}%", stats.total_cpu);
        let cpu_spark_w = body_w.saturating_sub(cpu_text.chars().count() + 2).min(20);

        let mut cpu_spans = vec![
            Span::styled(format!("{:<r_label_w$}", "Agent CPU"), theme::label()),
            Span::styled(format!("{cpu_text:>6} "), theme::value()),
        ];
        cpu_spans.extend(spark_spans(
            app.global_cpu.values(),
            cpu_spark_w,
            100.0,
            Gradient::Cpu,
            app.global_cpu.values().len().checked_sub(1),
        ));

        let mut session_spans = vec![
            Span::styled(format!("{:<r_label_w$}", "Sessions"), theme::label()),
            Span::styled(stats.total.to_string(), theme::value()),
        ];
        push_while_fits(
            &mut session_spans,
            vec![
                vec![
                    Span::styled(" · ", theme::dim()),
                    Span::styled(
                        format!("{} live", stats.running),
                        Style::default().fg(theme::colors().cost_low),
                    ),
                ],
                vec![Span::styled(
                    format!(" · {} in 24h", stats.active_24h),
                    theme::dim(),
                )],
            ],
            right_w,
        );
        let mut mem_spans = vec![
            Span::styled(format!("{:<r_label_w$}", "Agent mem"), theme::label()),
            Span::styled(format!("{mem_mb:.0} MB"), theme::value()),
        ];
        // Beside the total it is a share of, and only past a gigabyte: under
        // that it would be on screen most of the time and worth little when
        // it was. First in the queue, so a narrow panel keeps it over tokens.
        let (idle_n, idle_bytes) = app.reclaimable();
        let mut mem_groups = Vec::new();
        if idle_n > 0 && idle_bytes >= super::idle::HINT_BYTES {
            mem_groups.push(vec![
                Span::styled(" · ", theme::dim()),
                Span::styled(
                    format!("{} idle", util::compact_bytes(idle_bytes)),
                    Style::default().fg(theme::colors().cost_mid),
                ),
                Span::styled(" (I)", theme::dim()),
            ]);
        }
        mem_groups.push(vec![Span::styled(
            format!(
                " · {} in / {} out",
                util::compact_tokens(stats.total_input),
                util::compact_tokens(stats.total_output)
            ),
            theme::dim(),
        )]);
        push_while_fits(&mut mem_spans, mem_groups, right_w);
        let right_lines = vec![
            Line::from(model_spans),
            Line::from(session_spans),
            Line::from(cpu_spans),
            Line::from(mem_spans),
        ];
        frame.render_widget(Paragraph::new(right_lines), cols[2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spend bar fills the room the two leading names leave, up to a cap,
    /// and every model spent on — the folded tail included — has a cell.
    #[test]
    fn the_spend_bar_takes_what_the_names_leave_and_hides_when_squeezed() {
        let cells =
            |spans: &[Span]| -> usize { spans.iter().map(|s| s.content.chars().count()).sum() };
        let models: Vec<(String, f64)> = vec![
            ("opus-5".into(), 90.0),
            ("haiku-4-5".into(), 8.0),
            ("a".into(), 1.0),
            ("b".into(), 0.5),
            ("c".into(), 0.3),
            ("d".into(), 0.2),
        ];
        // ` opus-5 90%` and ` haiku-4-5 8%` take 24 columns.
        let bar = spend_bar(&models, 34);
        assert_eq!(cells(&bar), 10);
        // Four hues and the folded tail, each at least a cell.
        assert_eq!(bar.len(), CHARTED_MODELS + 1, "{bar:?}");
        assert_eq!(cells(&spend_bar(&models, 200)), SPEND_BAR_MAX);
        assert!(spend_bar(&models, 24 + SPEND_BAR_MIN - 1).is_empty());
        assert!(spend_bar(&[], 200).is_empty());
        // One model is still a mix worth drawing: all of today.
        assert_eq!(spend_bar(&models[..1], 40).len(), 1);
    }
}
