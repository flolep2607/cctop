//! The limits column: each account's subscription windows and how fast
//! they are being used.

use super::*;

/// Colour quota usage by whether it is being spent faster than an even budget
/// across its reset window. Falling back to absolute pressure keeps windows
/// useful when a provider omits either its reset time or duration.
fn quota_color(window: &cctop_core::quota::Window, now: i64) -> Color {
    if let (Some(duration), Some(reset)) = (window.duration, window.resets_at) {
        let duration_secs = duration.as_secs() as i64;
        let elapsed_secs = (now - (reset - duration_secs)).clamp(1, duration_secs);
        let pace_ratio = window.pct as f64 * duration_secs as f64 / (100.0 * elapsed_secs as f64);

        // A small overspend is worth noticing, while 50% above the sustainable
        // rate should be unmistakable. For a 7d window the sustainable rate is
        // 100 / (7 * 24), or roughly 0.6 percentage points per hour.
        if pace_ratio >= 1.5 {
            return theme::colors().cost_high;
        }
        if pace_ratio >= 1.1 {
            return theme::colors().cost_mid;
        }
        return theme::colors().cost_low;
    }

    if window.pct >= 90 {
        theme::colors().cost_high
    } else if window.pct >= 70 {
        theme::colors().cost_mid
    } else {
        theme::colors().cost_low
    }
}

pub(super) fn draw_limits(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    // On the panel whose columns it adds to, as a button: cctop holds the mouse,
    // so a click on it is answered as the key it names.
    const ADD: &str = " + account ";
    let add_width = ADD.chars().count() as u16;
    let block = panel_block("Limits")
        .title_top(Line::from(Span::styled(ADD, theme::title())).right_aligned());
    if area.width > add_width + 12 {
        let x = area.x + area.width - 1 - add_width;
        layout.key_hits.push((
            area.y,
            x,
            x + add_width,
            event::KeyEvent::from(event::KeyCode::Char('+')),
        ));
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // One column per account rather than a fixed two: a machine with a personal
    // and a work login has three things to report, and the panel exists to say
    // how much room is left in each.
    // The first of a harness's accounts is the one it would use unasked, and
    // naming it costs width the figures need — so only the others are
    // qualified. On the overwhelming majority of machines there are no others.
    // The harness travels alongside the label rather than being read back out
    // of it: `Codex (work)` is still Codex, and a hint chosen by comparing the
    // label told a Codex account to run `claude login`.
    let mut accounts: Vec<(String, &'static str, &cctop_core::quota::ProfileQuota)> = Vec::new();
    for (harness, qs) in [("Claude", &app.quota.claude), ("Codex", &app.quota.codex)] {
        for (i, q) in qs.iter().enumerate() {
            let name = match i {
                0 => harness.to_string(),
                _ => format!("{harness} ({})", q.profile),
            };
            accounts.push((name, harness, q));
        }
    }

    let share = 100 / accounts.len().max(1) as u16;
    let cols = RLayout::horizontal(
        accounts
            .iter()
            .map(|_| Constraint::Percentage(share))
            .collect::<Vec<_>>(),
    )
    // A gap, or a column that fills its width runs straight into the next one
    // and the two read as one sentence.
    .spacing(2)
    .split(inner);

    for (i, (name, harness, account)) in accounts.iter().enumerate() {
        let status = &account.status;
        let mut spans = vec![Span::styled(format!("{name} "), theme::label())];
        // Each failure mode gets its own message: an expired sign-in needs the
        // user to act, a rate-limit clears on its own, and "not signed in" is
        // neither. One catch-all string would hide all of that.
        match status {
            cctop_core::quota::ProviderStatus::Pending => {
                spans.push(Span::styled("checking…", theme::dim()));
            }
            cctop_core::quota::ProviderStatus::NotSignedIn => {
                spans.push(Span::styled("not signed in", theme::dim()));
            }
            cctop_core::quota::ProviderStatus::ApiBilling => {
                spans.push(Span::styled("API billing, no limits", theme::dim()));
            }
            // A token account the usage endpoint refuses is the normal case,
            // not a fault: a `claude setup-token` token can only make model
            // requests. It says so calmly, like API billing, and names no
            // command, because there is nothing to repair. `Expired` lands
            // here too for a token, though the poller already folds it: a
            // token cannot be told dead from unmeasurable by this endpoint.
            cctop_core::quota::ProviderStatus::NoLimits
            | cctop_core::quota::ProviderStatus::Expired
                if account.source == cctop_core::config::AccountSource::Token =>
            {
                spans.push(Span::styled("no limits for a token", theme::dim()));
            }
            cctop_core::quota::ProviderStatus::NoLimits => {
                spans.push(Span::styled("limits not readable", theme::dim()));
            }
            cctop_core::quota::ProviderStatus::Expired => {
                // Each harness renews its own sign-in; naming the other one's
                // command would refresh a directory this account does not use.
                let (said, cmd) = match *harness {
                    "Codex" => ("sign-in expired — ", "codex login"),
                    _ => ("sign-in expired — ", "claude login"),
                };
                // Three accounts on 120 columns is 37 a column, one short of
                // `Codex (work) sign-in expired — codex login`: the words give
                // way before the command does.
                let wanted = util::cells(name) + 1 + util::cells(said) + cmd.len();
                let said = match wanted > cols[i].width as usize {
                    true => "expired — ",
                    false => said,
                };
                spans.push(Span::styled(
                    said,
                    Style::default().fg(theme::colors().cost_mid),
                ));
                spans.push(Span::styled(cmd.to_string(), theme::value()));
            }
            cctop_core::quota::ProviderStatus::RateLimited { retry_at } => {
                spans.push(Span::styled(
                    "rate limited",
                    Style::default().fg(theme::colors().cost_mid),
                ));
                if let Some(at) = retry_at {
                    let remaining = at - chrono::Utc::now().timestamp();
                    if remaining > 0 {
                        spans.push(Span::styled(
                            format!(" — retry in {}m{:02}s", remaining / 60, remaining % 60),
                            theme::dim(),
                        ));
                    }
                }
            }
            cctop_core::quota::ProviderStatus::Unavailable(reason) => {
                spans.push(Span::styled(
                    format!("unavailable ({reason})"),
                    theme::dim(),
                ));
            }
            cctop_core::quota::ProviderStatus::Ok(q) => {
                let now = chrono::Utc::now().timestamp();
                if let Some(plan) = &q.plan {
                    spans.push(Span::styled(format!("({plan}) "), theme::dim()));
                }
                for w in &q.windows {
                    let color = quota_color(w, now);
                    spans.push(Span::styled(format!("{} ", w.label), theme::label()));
                    spans.push(Span::styled(
                        format!("{:>3}% ", w.pct),
                        Style::default().fg(color),
                    ));
                    let filled = (w.pct as usize * 8 / 100).min(8);
                    spans.push(Span::styled(
                        "\u{2501}".repeat(filled),
                        Style::default().fg(color),
                    ));
                    spans.push(Span::styled(
                        "\u{2500}".repeat(8 - filled),
                        Style::default().fg(theme::gray(244)),
                    ));
                    if let Some(reset) = w.resets_at {
                        let remaining = reset - now;
                        if remaining > 0 {
                            spans.push(Span::styled(
                                format!(" {}h{:02}m", remaining / 3600, (remaining % 3600) / 60),
                                theme::dim(),
                            ));
                        }
                    }
                    spans.push(Span::raw("  "));
                }
                if q.limit_reached {
                    spans.push(Span::styled(
                        "\u{26a0} limit",
                        Style::default().fg(theme::colors().cost_high),
                    ));
                }
                // What previous windows of this account went to waste. The
                // percentages beside it say whether there is room to keep
                // working; this says whether the plan was worth buying, which
                // is the question nobody is otherwise shown. Only the longest
                // window is offered — a five-hour window resets too often for
                // its unused share to mean anything about a subscription.
                if let Some(longest) = q
                    .windows
                    .iter()
                    .max_by_key(|w| w.duration.map(|d| d.as_secs()).unwrap_or(0))
                    // Keyed on the profile, not the label above: the label is
                    // "Claude" for the first account and "Claude (work)" for
                    // the rest, and the log knows accounts by profile.
                    && let Some(text) = cctop_core::burn::suffix(
                        &app.burn,
                        harness.to_ascii_lowercase().as_str(),
                        &account.profile,
                        longest.label,
                    )
                {
                    spans.push(Span::styled(format!("  {text}"), theme::dim()));
                }
            }
        }
        frame.render_widget(
            Paragraph::new(clip_spans(spans, cols[i].width as usize)),
            cols[i],
        );
    }
}

/// `spans` cut to `width` cells, with an ellipsis where they were cut.
///
/// A Limits column is a share of one line, and clipped by the frame alone its
/// message stopped mid-word — `sign-in expi` — with nothing saying there was
/// more; the ellipsis is what says it.
fn clip_spans(spans: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let line = Line::from(spans);
    if line.width() <= width {
        return line;
    }
    let mut room = width;
    let mut out = Vec::new();
    for span in line.spans {
        let w = span.width();
        // Strictly less: the line is being cut, so a cell stays free for the
        // ellipsis even when this span would end exactly at the edge.
        if w < room {
            room -= w;
            out.push(span);
            continue;
        }
        // The trailing `…` makes the span too long for `room` even when it
        // was exactly `room`, so truncate always marks the cut.
        let cut = util::truncate(&format!("{}…", span.content), room);
        out.push(Span::styled(cut, span.style));
        break;
    }
    Line::from(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_colour_tracks_spending_pace() {
        let duration = std::time::Duration::from_secs(7 * 24 * 60 * 60);
        let reset = 1_000_000;
        let window = cctop_core::quota::Window {
            label: "7d",
            pct: 80,
            duration: Some(duration),
            // 80% used halfway through a seven-day window is well ahead of
            // the even 100/(7*24) percentage-points-per-hour pace.
            resets_at: Some(reset + duration.as_secs() as i64 / 2),
        };
        assert_eq!(quota_color(&window, reset), theme::colors().cost_high);

        let sustainable = cctop_core::quota::Window {
            pct: 50,
            resets_at: Some(reset + duration.as_secs() as i64 / 2),
            ..window
        };
        assert_eq!(quota_color(&sustainable, reset), theme::colors().cost_low);
    }
}
