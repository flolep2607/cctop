//! What cctop is serving and sharing: the served links, a shared terminal's
//! code, and the address either lives at.

use super::*;

/// Whether this cctop is serving its table to a browser, and on what.
///
/// The links are drawn as their origin and made clickable — an OSC 8 hyperlink
/// to the whole URL, token and all, laid over the origin — rather than printed
/// in full. A served link carries the token that opens it, so the full text is
/// something to hand over deliberately — `y` puts it on the clipboard — and not
/// something to leave on screen. It also does not fit: a tunnel hostname plus a
/// token is most of a hundred columns.
///
/// The tunnel link can also be drawn as a QR code, with `c`, for a phone to
/// open it. Only the tunnel's: the loopback link is the one thing a phone
/// cannot reach, and it is the only other link this panel has — the dashboard
/// binds nothing but `127.0.0.1`, so there is no LAN address to offer either.
/// The code encodes what `y` copies, token and all, which is why it waits to be
/// asked for rather than appearing with the link; see [`qr`].
pub(crate) fn draw_serve(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let mut lines: Vec<Line> = Vec::new();
    // The internet origin's line, when its address can be chosen: what a
    // right-click on it renames.
    let mut address_at = None;
    // Where the code goes if there is one to draw: under the warning that says
    // what holding it grants, so the two are read together.
    let mut qr_at = None;
    // `(what is drawn, where it goes)`, top to bottom, for the hyperlinks laid
    // over the drawn origins once the paragraph has placed them.
    let mut links: Vec<(String, String)> = Vec::new();

    match &app.serving {
        None => {
            lines.push(Line::from(Span::raw(
                " Nothing is being served. The table can be a page,",
            )));
            lines.push(Line::from(Span::raw(" with cctop still running here.")));
        }
        Some(serving) => {
            // A free function rather than a closure: it appends to `lines`,
            // and a closure that captures it mutably shuts out every other push
            // in this arm.
            fn show(
                lines: &mut Vec<Line<'static>>,
                links: &mut Vec<(String, String)>,
                what: &str,
                url: &str,
            ) {
                lines.push(Line::from(Span::styled(format!(" {what}"), theme::dim())));
                lines.push(Line::from(Span::styled(
                    format!("  {}", origin_of(url)),
                    Style::default().fg(theme::colors().accent),
                )));
                links.push((origin_of(url), url.to_string()));
            }
            show(&mut lines, &mut links, "This machine", &serving.local);
            match &serving.public {
                None => {
                    lines.push(Line::default());
                    lines.push(Line::from(Span::styled(
                        " No tunnel — nothing off this machine can reach it.",
                        theme::dim(),
                    )));
                }
                Some(public) => {
                    lines.push(Line::default());
                    show(&mut lines, &mut links, "The internet", public);
                    match app.dashboard_rename() {
                        Ok(_) => {
                            address_at = Some(lines.len() - 1);
                            lines.push(Line::from(Span::styled(
                                "  e or right-click to choose this address",
                                theme::dim(),
                            )));
                        }
                        // On the account's domain and still not choosable:
                        // a tunnel token, which writes no DNS. Said, since
                        // the address is right there to want to change.
                        Err(why) if app.serving_on_account() => {
                            lines.push(Line::from(Span::styled(format!("  {why}"), theme::dim())))
                        }
                        Err(_) => {}
                    }
                    lines.push(Line::default());
                    // The one thing to understand before sending this to
                    // anybody, said where the link is being looked at.
                    lines.push(Line::from(Span::styled(
                        " Anyone holding it reads every session here,",
                        Style::default().fg(theme::colors().cost_mid),
                    )));
                    lines.push(Line::from(Span::styled(
                        match serving.actions {
                            true => " and can type at your agents. Cloudflare carries it.",
                            false => " but cannot act. Cloudflare carries it.",
                        },
                        Style::default().fg(theme::colors().cost_mid),
                    )));
                    if app.serve_qr {
                        lines.push(Line::default());
                        qr_at = Some((lines.len(), qr::encode(public)));
                    }
                }
            }
            // The same origin as whichever link is handed out, but a different
            // credential behind it — the label has to say that, because the
            // drawn origin cannot.
            if !serving.readonly.is_empty() {
                lines.push(Line::default());
                show(
                    &mut lines,
                    &mut links,
                    "Read-only — watches, never acts",
                    &serving.readonly,
                );
            }
        }
    }
    // Said in the panel as well as in the corner: `t` is pressed here, and a
    // panel that answered a keypress with nothing would read as a dead key.
    if let Some(opening) = &app.share_opening {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(" {} Opening a tunnel…", opening.frame()),
            Style::default().fg(theme::colors().cost_mid),
        )));
    }
    if let Some(error) = &app.serve_error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(" {}", cctop_core::util::truncate(error, 58)),
            Style::default().fg(theme::colors().cost_high),
        )));
    }
    let tunnelled = app.serving.as_ref().is_some_and(|s| s.public.is_some());
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        match (app.serving.is_some(), tunnelled, app.serve_qr) {
            (true, true, false) => " o open · y copy · c QR code · l local · t + tunnel · x stop",
            (true, true, true) => " o open · y copy · c hide QR · l local · t + tunnel · x stop",
            (true, false, _) => " o open · y copy · l local · t + tunnel · x stop",
            (false, ..) => " l this machine only · t also a public tunnel",
        },
        theme::dim(),
    )));
    // Next to `t`, because it is what makes `t`'s link one worth keeping:
    // offered while no account is connected, and the account named once one
    // is, since that is where `t` now goes.
    match &app.connected {
        None => lines.push(Line::from(Span::styled(
            " a connect your Cloudflare account: a link that stays",
            theme::dim(),
        ))),
        Some(connected) if app.serving.is_none() => lines.push(Line::from(Span::styled(
            match &connected.hostname {
                Some(host) => format!(" t goes to {host} · a account"),
                None => " t goes to your Cloudflare tunnel · a account".to_string(),
            },
            theme::dim(),
        ))),
        // Its links outlive a stop, so the way to revoke them is said here.
        Some(_) if app.serving_on_account() => lines.push(Line::from(Span::styled(
            " r new links, revoking these · a account",
            theme::dim(),
        ))),
        Some(_) => {}
    }

    // Sized to the longest line, so a tunnel hostname is one line and not two,
    // and capped to the screen, where it wraps instead — into a box now tall
    // enough for it. A code asked for widens it to the code, when the screen
    // has the columns. `max` then `min` and not `clamp`: on a screen narrower
    // than the floor the cap is below it, and `clamp` panics on that.
    let widest = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let code_width = match &qr_at {
        Some((_, Some(qr))) => qr.width + 4,
        _ => 0,
    };
    let width = widest
        .saturating_add(3)
        .max(code_width)
        .max(56)
        .min(area.width.saturating_sub(4).max(24));
    let room = match &qr_at {
        Some((at, Some(qr))) => make_room_for_qr(area, width, &mut lines, *at, qr),
        _ => None,
    };
    // `c` was pressed and there is no code to show for it. Said, because a key
    // that visibly does nothing reads as a dead one; and said as the thing to
    // do about it, which is nearly always a taller terminal.
    if let (Some((at, _)), None) = (&qr_at, room) {
        lines.insert(
            *at,
            Line::from(Span::styled(
                " No room here for a QR code — try a taller terminal.",
                theme::dim(),
            )),
        );
    }
    let address_row = address_at.map(|at| wrapped_rows(&lines[..at], width.saturating_sub(2)));
    let (_, inner) = modal(frame, area, "Serve this table to a browser", lines, width);
    if let Some(row) = address_row {
        layout.address_row = Some((inner.y + row, inner.x, inner.x + inner.width));
    }
    if let (Some((_, Some(qr))), Some(row)) = (&qr_at, room) {
        draw_qr(frame, inner, row, qr);
    }
    // Searched for in order, each below the last: the read-only link has the
    // same origin as the local one, and only its place says which token it is.
    let mut from = inner.y;
    for (shown, url) in &links {
        if let Some(row) = hyperlink::link_shown(frame.buffer_mut(), inner, from, shown, url) {
            from = row + 1;
        }
    }
}

/// A terminal `W` has just shared off this machine, as a code a phone can scan.
///
/// The operator link is a credential for a live agent — whoever opens it types
/// into it — and it is otherwise kept off the screen entirely; see
/// [`App::share_selected`](super::App) for the one reason it is drawn here.
/// The warning sits directly under the code for that reason, and the PIN beside
/// it because the browser asks for it once the link has opened.
///
/// Without the rows or columns for the code the panel still opens, saying so:
/// `W` was pressed to hand the terminal over, and the clipboard has done that
/// whether or not the code could be drawn.
pub(crate) fn draw_share_qr(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    let Some(share) = &app.share_qr else {
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            format!(" {}", cctop_core::util::truncate(&share.label, 50)),
            theme::value(),
        )),
        Line::default(),
    ];
    let at = lines.len();
    lines.push(Line::default());
    if let Some(pin) = &share.pin {
        lines.push(Line::from(vec![
            Span::raw(" Pairing code "),
            Span::styled(pin.clone(), theme::value()),
            Span::styled(" — asked for once the link opens.", theme::dim()),
        ]));
    }
    lines.push(Line::from(Span::styled(
        " Whoever holds this link can type at the agent.",
        Style::default().fg(theme::colors().cost_mid),
    )));
    lines.push(Line::from(Span::styled(
        " It is on your clipboard as well.",
        theme::dim(),
    )));
    // Where the link goes, never the link: the hostname opens nothing without
    // the token. The line a right-click renames, when it can be renamed.
    let mut address_at = None;
    if let Some(host) = &share.host {
        lines.push(Line::default());
        address_at = Some(lines.len());
        lines.push(Line::from(vec![
            Span::styled(" Address  ", theme::dim()),
            Span::styled(host.clone(), Style::default().fg(theme::colors().accent)),
        ]));
        lines.push(Line::from(Span::styled(
            match &share.rename {
                Ok(_) => " e or right-click to choose another".to_string(),
                Err(why) => format!(" {why}"),
            },
            theme::dim(),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(DISMISS_KEYS, theme::dim())));

    let code = qr::encode(&share.link);
    // A hostname is one line where the screen has the columns: broken in two,
    // it reads as two names.
    let address_width = address_at.map_or(0, |at| lines[at].width() as u16 + 3);
    let width = code
        .as_ref()
        .map_or(0, |qr| qr.width + 4)
        .max(56)
        .max(address_width)
        .min(area.width.saturating_sub(4).max(24));
    let room = code
        .as_ref()
        .and_then(|qr| make_room_for_qr(area, width, &mut lines, at, qr));
    if room.is_none() {
        lines[at] = Line::from(Span::styled(
            " No room here for a QR code — try a taller terminal.",
            theme::dim(),
        ));
    }
    let last = lines.len() as u16 - 1;
    // Placed after the code has made its room, which moves every line below
    // it; counted in wrapped rows, since a long line above wraps.
    let address_row = address_at.filter(|_| share.rename.is_ok()).map(|_| {
        let at = lines.len() - 4;
        wrapped_rows(&lines[..at], width.saturating_sub(2))
    });
    let last = wrapped_rows(&lines[..last as usize], width.saturating_sub(2));
    let (outer, inner) = modal(frame, area, "Open this terminal elsewhere", lines, width);
    if let Some(row) = address_row {
        layout.address_row = Some((inner.y + row, inner.x, inner.x + inner.width));
    }
    if let (Some(qr), Some(row)) = (&code, room) {
        draw_qr(frame, inner, row, qr);
    }
    confirm_chips(layout, outer, inner, last, DISMISS_KEYS, &any_key());
}

/// `http://127.0.0.1:7778/?t=abc` → `http://127.0.0.1:7778`.
///
/// What is worth reading on screen: where the link goes, without the token that
/// opens it.
fn origin_of(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let host = rest.split(['/', '?']).next().unwrap_or(rest);
    match scheme.is_empty() {
        true => host.to_string(),
        false => format!("{scheme}://{host}"),
    }
}

/// The address field: an agent's share hostname or the dashboard's, as one
/// label under the account's domain.
///
/// What Enter will do is said before it is pressed. For the dashboard that is
/// the whole of the warning the issue asked for — every link to the old
/// hostname stops working — and it names the new one as it is typed, so the
/// sentence is about this rename and not renames in general.
pub(crate) fn draw_address(frame: &mut Frame, area: Rect, app: &App, layout: &mut Layout) {
    use super::address::Target;
    let Some(edit) = &app.address else {
        return;
    };
    let warn = Style::default().fg(theme::colors().cost_mid);
    let mut lines = vec![Line::from(vec![
        Span::styled(" Now at ", theme::dim()),
        Span::styled(edit.was.clone(), theme::value()),
    ])];
    let title = match &edit.target {
        Target::Share { label, .. } => {
            lines.push(Line::from(Span::styled(
                format!(
                    " Where {} goes when shared with W.",
                    util::truncate(label, 36)
                ),
                theme::dim(),
            )));
            if let Some(default) = &edit.default {
                lines.push(Line::from(Span::styled(
                    format!(" Empty sends it back to {default}."),
                    theme::dim(),
                )));
            }
            lines.push(Line::from(Span::styled(
                " Links on the old address stop working.",
                warn,
            )));
            "Choose this agent's address"
        }
        Target::Dashboard => {
            let to = edit
                .typed_host()
                .filter(|host| *host != edit.was)
                .unwrap_or_else(|| format!("<name>.{}", edit.zone));
            lines.push(Line::from(Span::styled(
                format!(" Links to {} stop working;", edit.was),
                warn,
            )));
            lines.push(Line::from(Span::styled(
                format!(" the page moves to {to}. Its token stays the same."),
                warn,
            )));
            "Move the dashboard"
        }
    };
    lines.push(Line::default());
    let mut field = input_line(&edit.field);
    field
        .spans
        .push(Span::styled(format!(".{}", edit.zone), theme::dim()));
    lines.push(field);
    lines.push(Line::default());
    match (edit.frame(), &edit.problem) {
        (Some(frame_char), _) => lines.push(Line::from(Span::styled(
            format!(" {frame_char} Asking Cloudflare…"),
            warn,
        ))),
        (None, Some(problem)) => lines.push(Line::from(Span::styled(
            format!(" {problem}"),
            Style::default().fg(theme::colors().cost_high),
        ))),
        (None, None) => lines.push(Line::from(Span::styled(
            " Letters, digits and -.",
            theme::dim(),
        ))),
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " Enter apply   Esc cancel",
        theme::dim(),
    )));
    let width = 64.min(area.width.saturating_sub(4).max(24));
    let (outer, _) = modal(frame, area, title, lines, width);
    layout.modal_rect = Some(outer);
}
