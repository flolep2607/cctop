//! Signing in: the Cloudflare account the dashboard is served through, and
//! the Claude accounts agents run on.

use super::*;

/// Where on `screen` `link` is drawn: `(row, columns)` for the row it starts on
/// and each row after that carries a piece of it.
///
/// Columns and not only rows, because the rows are shared: the first has
/// whatever the command printed before the URL, and a hyperlink laid over that
/// would make the prompt text open the sign-in page too.
fn link_cells(screen: &vt100::Screen, link: &str) -> Vec<(u16, std::ops::Range<u16>)> {
    let (rows, width) = screen.size();
    // One char per column, so an index into the text is a column. The URL is
    // ASCII, so nothing it is matched against is lost by keeping only the
    // first char of a cell; a wide glyph's second column reads as a blank.
    let text = |row: u16| -> Vec<char> {
        (0..width)
            .map(|col| {
                screen
                    .cell(row, col)
                    .and_then(|cell| cell.contents().chars().next())
                    .unwrap_or(' ')
            })
            .collect()
    };
    // The run of non-blank columns from `start`.
    let run = |chars: &[char], start: usize| -> std::ops::Range<u16> {
        let len = chars[start..].iter().take_while(|c| **c != ' ').count();
        start as u16..(start + len) as u16
    };
    let Some((first, start)) = (0..rows).find_map(|row| {
        let chars = text(row);
        let at = chars
            .windows(8)
            .position(|w| w.iter().copied().eq("https://".chars()))?;
        Some((row, run(&chars, at)))
    }) else {
        return Vec::new();
    };
    let mut out = vec![(first, start)];
    for row in first + 1..rows {
        let chars = text(row);
        let Some(at) = chars.iter().position(|c| *c != ' ') else {
            break;
        };
        let piece = run(&chars, at);
        let shown: String = chars[piece.start as usize..piece.end as usize]
            .iter()
            .collect();
        // Blanks after the piece that are not the end of the row mean more text
        // on it, which the URL does not have.
        let rest_blank = chars[piece.end as usize..].iter().all(|c| *c == ' ');
        if !rest_blank || !link.contains(&shown) {
            break;
        }
        out.push((row, piece));
    }
    out
}

/// The Cloudflare popup: the ways in, then a domain, a name, and what came of
/// it — or, with an account already connected, what it is and Disconnect.
///
/// Laid out step by step in the add-account popup's manner, each step a short
/// paragraph and a line of keys that are also buttons. The first step is
/// headed by a row of the ways in ([`connect::Method`]), the chosen one lit,
/// and the paragraph under it is that way's.
pub(crate) fn draw_connect(frame: &mut Frame, area: Rect, app: &mut App, layout: &mut Layout) {
    use super::connect::Step;
    use cctop_core::cloudflare;
    let Some(flow) = app.connect.as_ref() else {
        return;
    };
    let esc = KeyEvent::from(KeyCode::Esc);
    let enter = KeyEvent::from(KeyCode::Enter);
    let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    let warn = Style::default().fg(theme::colors().cost_mid);
    let good = Style::default().fg(theme::colors().cost_low);
    let accent = Style::default().fg(theme::colors().accent);
    let text = |s: &str| Line::from(Span::raw(format!(" {s}")));
    let dim = |s: &str| Line::from(Span::styled(format!(" {s}"), theme::dim()));
    // Decided before the lines, because a sentence from Cloudflare is wrapped
    // here to it: the box is sized by counting rows, and a paragraph's own
    // word wrap of a long URL takes more of them than the count expects.
    let width = 70.min(area.width.saturating_sub(4).max(24));
    let wrap = |s: &str, indent: usize, style: Style| -> Vec<Line<'static>> {
        let room = (width as usize).saturating_sub(3 + indent).max(8);
        wrap_words(s, room)
            .into_iter()
            .map(|row| Line::from(Span::styled(format!(" {}{row}", " ".repeat(indent)), style)))
            .collect()
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    // `(what is drawn, where it goes)`, laid over the text once it is placed.
    let mut links: Vec<(String, String)> = Vec::new();
    let mut qr_at = None;
    let (hint, keys): (String, Vec<(&str, KeyEvent)>) = match &flow.step {
        Step::Start { method, field } => {
            use super::connect::{METHODS, Method};
            lines.push(text("One address of your own for t and W, so a bookmark"));
            lines.push(text("or a phone shortcut keeps working."));
            lines.push(Line::default());
            let mut row = vec![Span::raw(" ")];
            for (i, way) in METHODS.iter().enumerate() {
                if i > 0 {
                    row.push(Span::styled(" │ ", theme::dim()));
                }
                row.push(match way == method {
                    true => Span::styled(format!(" {} ", way.label()), theme::selected()),
                    false => Span::styled(format!(" {} ", way.label()), theme::dim()),
                });
            }
            row.push(Span::styled("  Tab ⇄", theme::dim()));
            lines.push(Line::from(row));
            lines.push(Line::default());
            match method {
                Method::Browser => {
                    lines.push(text("Log in to Cloudflare and pick the domain there:"));
                    lines.push(text("cctop creates the tunnel and its DNS records."));
                    lines.push(dim("Nothing to make or paste. Over ssh, the address is"));
                    lines.push(dim("shown here, to open wherever your browser is."));
                    (
                        " [Enter] log in  [Tab] paste a token instead  [Esc] cancel".to_string(),
                        vec![
                            ("[Enter]", enter),
                            ("[Tab]", KeyEvent::from(KeyCode::Tab)),
                            ("[Esc]", esc),
                        ],
                    )
                }
                Method::Paste => {
                    lines.push(text("1. Make an API token; the link fills it in:"));
                    let shown = "dash.cloudflare.com/profile/api-tokens";
                    lines.push(Line::from(Span::styled(format!("    {shown}"), accent)));
                    links.push((shown.to_string(), cloudflare::token_link()));
                    lines.push(dim("   Tunnel · Edit, DNS · Edit, Zone · Read"));
                    if flow.qr {
                        qr_at = Some((lines.len(), qr::encode(&cloudflare::token_link())));
                    }
                    lines.push(text("2. Paste it here. A tunnel token works too."));
                    lines.push(masked_line(field.chars().count()));
                    (
                        " [Enter] connect  [Ctrl+O] copy link  [Ctrl+Q] QR  [Esc] cancel"
                            .to_string(),
                        vec![
                            ("[Enter]", enter),
                            ("[Ctrl+O]", ctrl('o')),
                            ("[Ctrl+Q]", ctrl('q')),
                            ("[Esc]", esc),
                        ],
                    )
                }
            }
        }
        Step::LoggingIn { url, opened } => {
            match opened {
                true => {
                    lines.push(text("Your browser should have opened Cloudflare's login."));
                    lines.push(text("Log in there and pick the domain for the tunnel."));
                    lines.push(dim("If it did not open, this is the address:"));
                }
                false => {
                    lines.push(text("Open this address in a browser, log in to"));
                    lines.push(text("Cloudflare and pick the domain for the tunnel:"));
                }
            }
            // The whole address is a hundred and fifty characters; the
            // terminal's link, the copy and the code all carry it whole.
            let shown = "dash.cloudflare.com/argotunnel";
            lines.push(Line::from(Span::styled(format!("    {shown}"), accent)));
            links.push((shown.to_string(), url.clone()));
            if flow.qr {
                qr_at = Some((lines.len(), qr::encode(url)));
            }
            lines.push(dim("cctop waits here for up to ten minutes."));
            (
                " [Ctrl+O] copy link  [Ctrl+Q] QR  [Esc] cancel".to_string(),
                vec![
                    ("[Ctrl+O]", ctrl('o')),
                    ("[Ctrl+Q]", ctrl('q')),
                    ("[Esc]", esc),
                ],
            )
        }
        Step::Zones { zones, cursor, .. } => {
            lines.push(text("The token works. Which domain is the tunnel on?"));
            lines.push(Line::default());
            for (i, zone) in zones.iter().enumerate() {
                let here = i == *cursor;
                let line = Line::from(Span::raw(format!(
                    " {} {}",
                    if here { "›" } else { " " },
                    zone.name
                )));
                lines.push(match here {
                    true => line.style(theme::selected()),
                    false => line,
                });
            }
            (
                " [↑↓] pick  [Enter] next  [Esc] cancel".to_string(),
                vec![("[Enter]", enter), ("[Esc]", esc)],
            )
        }
        Step::Hostname { zone, field, .. } => {
            match zone.name.is_empty() {
                // A login's domain whose name could not be read.
                true => {
                    lines.push(text("Logged in. Its address, on the domain you picked:"));
                    lines.push(text("one name under it, like cctop.example.com."));
                }
                false => {
                    lines.push(text(&format!("Its address: one name under {},", zone.name)));
                    lines.push(text("which Cloudflare's free certificate covers."));
                }
            }
            lines.push(input_line(field));
            if !field.is_empty() {
                lines.push(dim(&format!(
                    "  W shares go on {} beside it.",
                    cloudflare::share_hostname(field)
                )));
            }
            (
                " [Enter] create  [Esc] cancel".to_string(),
                vec![("[Enter]", enter), ("[Esc]", esc)],
            )
        }
        Step::TunnelHostname { field, .. } => {
            lines.push(text("A tunnel token: there is nothing to create."));
            lines.push(text("Which hostname does it serve? Left empty, it is"));
            lines.push(text("learned when the tunnel first connects."));
            lines.push(input_line(field));
            (
                " [Enter] save  [Esc] cancel".to_string(),
                vec![("[Enter]", enter), ("[Esc]", esc)],
            )
        }
        Step::Done { hostname, shares } => {
            match hostname {
                Some(host) => {
                    let url = format!("https://{host}");
                    lines.push(Line::from(vec![
                        Span::styled(" ✓ Connected: ", good),
                        Span::styled(url.clone(), accent),
                    ]));
                    links.push((url.clone(), url));
                }
                None => lines.push(Line::from(Span::styled(
                    " ✓ Connected. Its address is learned when it first connects.",
                    good,
                ))),
            }
            lines.push(dim(match shares {
                true => "t in the serve panel now uses it, and W shares too.",
                false => "t in the serve panel now uses it.",
            }));
            (
                " [s] serve on it now  [Enter] done".to_string(),
                vec![
                    ("[s]", KeyEvent::from(KeyCode::Char('s'))),
                    ("[Enter]", enter),
                ],
            )
        }
        Step::Failed { message, .. } => {
            lines.extend(wrap(message, 0, warn));
            lines.push(Line::default());
            lines.push(dim("Quick tunnels still work: t in the serve panel."));
            (
                " [Enter] try again  [Esc] close".to_string(),
                vec![("[Enter]", enter), ("[Esc]", esc)],
            )
        }
        Step::Connected { account, confirm } => {
            let url = account.hostname.as_ref().map(|h| format!("https://{h}"));
            lines.push(Line::from(vec![
                Span::raw(" Page    "),
                match &url {
                    Some(url) => Span::styled(url.clone(), accent),
                    None => Span::styled("learned when it connects", theme::dim()),
                },
            ]));
            if let Some(url) = url {
                links.push((url.clone(), url));
            }
            if let Some(share) = &account.share_hostname {
                lines.push(Line::from(vec![
                    Span::raw(" Shares  "),
                    Span::styled(format!("https://{share}"), theme::dim()),
                ]));
            }
            if account.from_env {
                lines.push(dim("From CCTOP_TUNNEL_TOKEN, not from setup."));
            }
            lines.push(Line::from(vec![
                Span::raw(" Access  "),
                match &account.access {
                    Some(a) => Span::styled(format!("on — {} logs in", a.owner), good),
                    None => Span::styled("off — opened with its token link", theme::dim()),
                },
            ]));
            match confirm {
                false => (
                    " [a] who may log in  [d] disconnect  [Esc] close".to_string(),
                    vec![
                        ("[a]", KeyEvent::from(KeyCode::Char('a'))),
                        ("[d]", KeyEvent::from(KeyCode::Char('d'))),
                        ("[Esc]", esc),
                    ],
                ),
                true => {
                    lines.push(Line::default());
                    match account.api_token.is_some() {
                        true => {
                            lines.push(Line::from(Span::styled(
                                " Delete the tunnel and its DNS records from",
                                warn,
                            )));
                            lines.push(Line::from(Span::styled(
                                " Cloudflare, and forget it here?",
                                warn,
                            )));
                        }
                        false => {
                            lines.push(Line::from(Span::styled(" Forget it here?", warn)));
                            lines.push(dim("It was made in the Cloudflare dashboard, and stays."));
                        }
                    }
                    if app.serving_on_account() {
                        lines.push(dim("Serving here over it stops first."));
                    }
                    (
                        " [y] disconnect  [n] keep".to_string(),
                        vec![
                            ("[y]", KeyEvent::from(KeyCode::Char('y'))),
                            ("[n]", KeyEvent::from(KeyCode::Char('n'))),
                        ],
                    )
                }
            }
        }
        Step::Access {
            account,
            cursor,
            field,
            confirm,
            said,
        } => {
            use super::connect::Typing;
            let key = |c| KeyEvent::from(KeyCode::Char(c));
            let typing = field.as_ref().map(|(t, _)| *t);
            let keys = match &account.access {
                None => {
                    lines.push(text("Cloudflare Access: off. The page opens with its"));
                    lines.push(text("token link, and anyone holding it is in."));
                    lines.push(dim("On, it asks for a login: a code emailed to you or"));
                    lines.push(dim(
                        "whoever you invite. Nobody needs a Cloudflare account.",
                    ));
                    match field {
                        Some((Typing::Token, edit)) => {
                            lines.push(Line::default());
                            lines.push(text("A browser login cannot manage Access; an API"));
                            lines.push(text("token can, and keeps this tunnel as it is:"));
                            let shown = "dash.cloudflare.com/profile/api-tokens";
                            lines.push(Line::from(Span::styled(format!("    {shown}"), accent)));
                            links.push((shown.to_string(), cloudflare::token_link()));
                            lines.push(text("Paste it here:"));
                            lines.push(masked_line(edit.chars().count()));
                        }
                        Some((_, edit)) => {
                            lines.push(Line::default());
                            lines.push(text("Your email — the owner, always full:"));
                            lines.push(input_line(edit));
                        }
                        None => {}
                    }
                    match typing {
                        Some(Typing::Token) => (
                            " [Enter] use it  [Ctrl+O] copy link  [Esc] cancel".to_string(),
                            vec![("[Enter]", enter), ("[Ctrl+O]", ctrl('o')), ("[Esc]", esc)],
                        ),
                        Some(_) => (
                            " [Enter] turn on  [Esc] cancel".to_string(),
                            vec![("[Enter]", enter), ("[Esc]", esc)],
                        ),
                        None => (
                            " [o] turn on  [Esc] back".to_string(),
                            vec![("[o]", key('o')), ("[Esc]", esc)],
                        ),
                    }
                }
                Some(settings) => {
                    lines.push(Line::from(vec![
                        Span::raw(" Access  "),
                        Span::styled(format!("on — {} owns it", settings.owner), good),
                    ]));
                    match (settings.public_links, &settings.link_hostname) {
                        (true, Some(link)) => {
                            let url = format!("https://{link}");
                            lines.push(Line::from(vec![
                                Span::raw(" Links   "),
                                Span::styled(url.clone(), accent),
                            ]));
                            links.push((url.clone(), url));
                            lines.push(dim("        token links, for people who cannot log in"));
                        }
                        (true, None) => {
                            lines.push(Line::from(vec![Span::raw(" Links   "), Span::raw("on")]))
                        }
                        (false, _) => lines.push(Line::from(vec![
                            Span::raw(" Links   "),
                            Span::styled("off — from outside, only a login gets in", warn),
                        ])),
                    }
                    lines.push(Line::default());
                    match settings.invites.is_empty() {
                        true => lines.push(dim("No one else is invited yet.")),
                        false => {
                            let wide = settings
                                .invites
                                .iter()
                                .map(|i| i.who.chars().count())
                                .max()
                                .unwrap_or(0)
                                .min(40);
                            for (i, invite) in settings.invites.iter().enumerate() {
                                let here = i == *cursor && typing.is_none();
                                let can = match invite.level {
                                    cloudflare::access::Level::Read => "read-only",
                                    cloudflare::access::Level::Full => "full",
                                };
                                let line = Line::from(Span::raw(format!(
                                    " {} {:wide$}  {can}",
                                    if here { "›" } else { " " },
                                    invite.who,
                                )));
                                lines.push(match here {
                                    true => line.style(theme::selected()),
                                    false => line,
                                });
                            }
                        }
                    }
                    if let Some((Typing::Invite, edit)) = field {
                        lines.push(Line::default());
                        lines.push(text("Invite an email, or everyone at @company.com:"));
                        lines.push(input_line(edit));
                        lines.push(dim("  read-only; f on it afterwards makes it full"));
                    }
                    if *confirm {
                        lines.push(Line::default());
                        lines.push(Line::from(Span::styled(
                            " Take the page out of Access? Its token link opens it",
                            warn,
                        )));
                        lines.push(Line::from(Span::styled(
                            " again, and the invites are forgotten.",
                            warn,
                        )));
                    }
                    match (typing, *confirm) {
                        (Some(_), _) => (
                            " [Enter] invite  [Esc] cancel".to_string(),
                            vec![("[Enter]", enter), ("[Esc]", esc)],
                        ),
                        (None, true) => (
                            " [y] turn off  [n] keep".to_string(),
                            vec![("[y]", key('y')), ("[n]", key('n'))],
                        ),
                        (None, false) => (
                            " [i] invite [x] remove [f] full/read [l] links [o] off [Esc]"
                                .to_string(),
                            vec![
                                ("[i]", key('i')),
                                ("[x]", key('x')),
                                ("[f]", key('f')),
                                ("[l]", key('l')),
                                ("[o]", key('o')),
                                ("[Esc]", esc),
                            ],
                        ),
                    }
                }
            };
            for line in said {
                lines.extend(wrap(line, 0, theme::dim()));
            }
            keys
        }
        Step::Disconnected { left, made_here } => {
            match (made_here, left.is_empty()) {
                (true, true) => lines.push(Line::from(Span::styled(
                    " ✓ Disconnected: the tunnel and its DNS records are gone.",
                    good,
                ))),
                (false, _) => {
                    lines.push(Line::from(Span::styled(" ✓ Forgotten.", good)));
                    lines.push(dim("The tunnel was made in the Cloudflare dashboard;"));
                    lines.push(dim("delete it there if you no longer want it."));
                }
                (true, false) => {
                    lines.push(Line::from(Span::styled(
                        " Forgotten, but left on Cloudflare to delete by hand:",
                        warn,
                    )));
                    for item in left {
                        lines.extend(wrap(&format!("- {item}"), 0, theme::dim()));
                    }
                }
            }
            lines.push(dim("t goes back to quick tunnels."));
            (" [Enter] done".to_string(), vec![("[Enter]", enter)])
        }
    };
    if let Some(problem) = &flow.problem {
        lines.extend(wrap(
            problem,
            2,
            Style::default().fg(theme::colors().cost_high),
        ));
    }
    lines.push(Line::default());
    // A call in flight replaces the keys: there is nothing to press but Esc,
    // and the spinner is what says cctop has not hung.
    let (hint, keys) = match &flow.working {
        // Waiting on the user, who may want the link again or its code.
        Some(_) if matches!(flow.step, Step::LoggingIn { .. }) => (
            format!(
                " {} Waiting for the login  [Ctrl+O] copy  [Ctrl+Q] QR  [Esc] cancel",
                share::spinner_frame()
            ),
            vec![
                ("[Ctrl+O]", ctrl('o')),
                ("[Ctrl+Q]", ctrl('q')),
                ("[Esc]", esc),
            ],
        ),
        Some(working) => (
            format!(" {} {}  [Esc] close", share::spinner_frame(), working.what),
            vec![("[Esc]", esc)],
        ),
        None => (hint, keys),
    };
    lines.push(Line::from(Span::styled(hint.clone(), theme::dim())));

    let room = match &qr_at {
        Some((at, Some(qr))) => make_room_for_qr(area, width, &mut lines, *at, qr),
        _ => None,
    };
    if let (Some((at, _)), None) = (&qr_at, room) {
        lines.insert(
            *at,
            dim("  No room here for a QR code — try a taller terminal."),
        );
    }
    let row = wrapped_rows(&lines[..lines.len() - 1], width.saturating_sub(2));
    let title = match flow.step {
        Step::Connected { .. } | Step::Disconnected { .. } => "Your Cloudflare account",
        Step::Access { .. } => "Who may log in",
        _ => "Connect your Cloudflare account",
    };
    let (outer, inner) = modal(frame, area, title, lines, width);
    if let (Some((_, Some(qr))), Some(at)) = (&qr_at, room) {
        draw_qr(frame, inner, at, qr);
    }
    let mut from = inner.y;
    for (shown, url) in &links {
        if let Some(row) = hyperlink::link_shown(frame.buffer_mut(), inner, from, shown, url) {
            from = row + 1;
        }
    }
    confirm_chips(layout, outer, inner, row, &hint, &keys);
}

/// `text` in rows of at most `width` cells, broken between words, and inside
/// a word only where one is longer than a row — a URL, usually.
///
/// [`util::wrap`] with the whitespace collapsed first, because what arrives
/// here is a sentence from a server whose own line breaks mean nothing in a
/// box this size; and always one row, so an empty message still has its line.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let rows = util::wrap(&words, width);
    if rows.is_empty() {
        vec![String::new()]
    } else {
        rows
    }
}

/// The paste field, masked: as many dots as characters, up to a line's worth,
/// and the count — enough to see a paste landed whole, nothing to read off.
fn masked_line(chars: usize) -> Line<'static> {
    let (text, cursor) = input_styles();
    let mut spans = vec![
        Span::raw(" > "),
        Span::styled("•".repeat(chars.min(24)), text),
        Span::styled("█", cursor),
    ];
    if chars > 0 {
        spans.push(Span::styled(format!("  {chars} characters"), theme::dim()));
    }
    Line::from(spans)
}

/// The add-account popup: a name field, which kind of account, then the command
/// that makes it running in a terminal inside the popup, then what came of it.
pub(crate) fn draw_add_account(frame: &mut Frame, area: Rect, app: &mut App, layout: &mut Layout) {
    let esc = KeyEvent::from(KeyCode::Esc);
    let flow = &mut app.add_account;

    if let Some(outcome) = &flow.outcome
        && flow.pane.is_none()
    {
        let mut lines = match outcome {
            Ok(name) => vec![
                Line::from(vec![
                    Span::styled(" ✓ Saved ", Style::default().fg(theme::colors().cost_low)),
                    Span::styled(name.clone(), theme::value()),
                    Span::styled(". Its limits appear in the panel below.", theme::dim()),
                ]),
                Line::from(Span::styled(
                    " Start an agent on it with p in the launcher, or from a shell:",
                    theme::dim(),
                )),
                Line::from(Span::styled(
                    format!("   cctop as {name} claude"),
                    theme::value(),
                )),
            ],
            Err(why) => vec![Line::from(Span::styled(
                format!(" {why}"),
                Style::default().fg(theme::colors().cost_mid),
            ))],
        };
        lines.push(Line::default());
        Confirm {
            title: "Add a Claude account",
            width: 72,
            hint: " [Enter] done",
            keys: &[("[Enter]", KeyEvent::from(KeyCode::Enter))],
        }
        .draw(frame, area, layout, lines);
        return;
    }

    if flow.named && flow.pane.is_none() {
        let key = |label: &'static str| Span::styled(label, theme::value());
        let lines = vec![
            Line::from(vec![
                key(" [f] full login  "),
                Span::styled(
                    format!("its own ~/.claude-{}, everything works", flow.name),
                    theme::dim(),
                ),
            ]),
            Line::from(vec![
                key(" [t] token       "),
                Span::styled("shares ~/.claude history, but no Remote", theme::dim()),
            ]),
            Line::from(Span::styled(
                "                  Control or claude.ai connectors",
                theme::dim(),
            )),
            Line::default(),
        ];
        Confirm {
            title: &format!("Add Claude account {}", flow.name),
            width: 64,
            hint: " [f] full login   [t] token   [Esc] cancel",
            keys: &[
                ("[f]", KeyEvent::from(KeyCode::Char('f'))),
                ("[t]", KeyEvent::from(KeyCode::Char('t'))),
                ("[Esc]", esc),
            ],
        }
        .draw(frame, area, layout, lines);
        return;
    }

    let Some(pane) = flow.pane.as_mut() else {
        let lines = vec![
            Line::from(Span::styled(" What is it called?", theme::dim())),
            input_line(&flow.name),
            Line::default(),
        ];
        Confirm {
            title: "Add a Claude account",
            width: 52,
            hint: " [Enter] next   [Esc] cancel",
            keys: &[("[Enter]", KeyEvent::from(KeyCode::Enter)), ("[Esc]", esc)],
        }
        .draw(frame, area, layout, lines);
        return;
    };

    // Wide enough for the token on one line where the screen allows: it is
    // read back off this terminal, and unwrapped is the easy case.
    let rect = centered(area, 116, 30);
    frame.render_widget(Clear, rect);
    let hint = match flow.link {
        Some(_) => " [Ctrl+O] or click the link: copy it   [Esc] cancel ",
        None => " [Esc] cancel ",
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .style(theme::canvas())
        .title(Span::styled(
            format!(
                " Add Claude account {} — {} ",
                flow.name,
                match flow.kind {
                    Some(AccountKind::Login) => "claude auth login",
                    _ => "claude setup-token",
                }
            ),
            theme::title(),
        ))
        .title_bottom(Span::styled(hint, theme::title()));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let [note, term] = ratatui::layout::Layout::vertical([
        ratatui::layout::Constraint::Length(2),
        ratatui::layout::Constraint::Min(1),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    " Approve it in the browser signed in as {}. {}",
                    flow.name,
                    match flow.kind {
                        Some(AccountKind::Login) => "The account is ready",
                        _ => "The token is saved",
                    }
                ),
                theme::dim(),
            )),
            Line::from(Span::styled(
                match flow.kind {
                    Some(AccountKind::Login) => " the moment the login finishes.",
                    _ => " the moment it is printed — nothing to copy.",
                },
                theme::dim(),
            )),
        ]),
        note,
    );
    pane.view.resize(term.width, term.height);
    let (cols, rows) = pane.view.size;
    let screen = Rect {
        width: cols.min(term.width),
        height: rows.min(term.height),
        ..term
    };
    frame.render_widget(
        tui_term::widget::PseudoTerminal::new(pane.view.parser.screen()),
        screen,
    );
    // Every row the link is wrapped over is one target. The terminal's own
    // link detection sees only the row under the pointer, so a click there
    // opened the first line of the URL — a sign-in page that cannot work.
    //
    // Two targets, in fact, for two kinds of click. cctop holds the mouse, so a
    // plain click is cctop's and copies the link. The cells are also an OSC 8
    // hyperlink to the whole URL, for the click a terminal keeps for itself —
    // Ctrl or Shift and a click, depending on the terminal — which now opens
    // the page it means from any row of it.
    if let Some(link) = &flow.link {
        for (y, columns) in link_cells(pane.view.parser.screen(), link) {
            if y < screen.height {
                hyperlink::link(
                    frame.buffer_mut(),
                    screen.y + y,
                    screen.x + columns.start..screen.x + columns.end.min(screen.width),
                    link,
                );
                layout.key_hits.push((
                    screen.y + y,
                    screen.x,
                    screen.x + screen.width,
                    KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
                ));
            }
        }
    }
    // The chips are on the bottom border, one row below the terminal.
    layout.modal_rect = Some(rect);
    let bottom = rect.y + rect.height - 1;
    for (chip, key) in [
        (
            "[Ctrl+O] or click the link: copy it",
            KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
        ),
        ("[Esc] cancel", esc),
    ] {
        if let Some(at) = hint.find(chip) {
            let x = rect.x + 1 + hint[..at].chars().count() as u16;
            layout
                .key_hits
                .push((bottom, x, x + chip.chars().count() as u16, key));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sign-in link wrapped over three rows is three click targets, and the
    /// text after it is none — a click there is not a click on the link.
    #[test]
    fn every_row_of_a_wrapped_link_is_clickable() {
        let mut parser = vt100::Parser::new(6, 20, 0);
        let link = "https://claude.com/cai/oauth/authorize?code=true";
        parser.process(format!("Sign in:\r\n{link}\r\n\r\nPaste code:").as_bytes());
        assert_eq!(
            link_cells(parser.screen(), link),
            vec![(1, 0..20), (2, 0..20), (3, 0..8)]
        );
    }

    /// The prompt in front of the URL is not part of the link, and neither is
    /// a row that only starts like a piece of it.
    #[test]
    fn the_link_is_its_own_columns_and_not_the_row() {
        let mut parser = vt100::Parser::new(6, 20, 0);
        let link = "https://a.io/abc";
        // "abc later" starts with a piece of it, but carries on past one.
        parser.process(format!("Go: {link}\r\nabc later").as_bytes());
        assert_eq!(link_cells(parser.screen(), link), vec![(0, 4..20)]);
    }

    /// The popup's sign-in link, wrapped over rows by the terminal that printed
    /// it, is one hyperlink to the whole URL on every one of them — and the
    /// screen reads, cell for cell, as it did before the link was laid over it.
    #[test]
    fn a_wrapped_sign_in_link_opens_the_whole_url_from_every_row() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let link = format!(
            "https://claude.com/cai/oauth/authorize?code=true&client_id={}&state={}",
            "c".repeat(60),
            "s".repeat(70)
        );
        let draw = |with_link: bool| {
            let mut app = crate::tests::test_app();
            let mut pane = crate::tabs::Pane::for_test("claude");
            pane.view.parser.process(
                format!(
                    "Browser didn't open? Use the url below:\r\n\r\n{link}\r\n\r\nPaste code > "
                )
                .as_bytes(),
            );
            app.add_account = crate::AddAccount {
                name: "work".into(),
                kind: Some(AccountKind::Token),
                named: true,
                pane: Some(pane),
                link: with_link.then(|| link.clone()),
                outcome: None,
            };
            let mut layout = Layout::default();
            let mut terminal = Terminal::new(TestBackend::new(140, 40)).expect("backend");
            terminal
                .draw(|frame| draw_add_account(frame, frame.area(), &mut app, &mut layout))
                .expect("draw");
            (terminal.backend().buffer().clone(), layout)
        };
        let (linked, layout) = draw(true);
        let (plain, _) = draw(false);

        // Every cell that opens a link opens this one, and read in order the
        // linked cells spell it exactly: nothing of it left plain, nothing
        // around it swept in.
        let mut rows = Vec::new();
        let mut spelled = String::new();
        let mut skip = 0u16;
        for (i, cell) in linked.content().iter().enumerate() {
            let (x, y) = linked.pos_of(i);
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if let Some(target) = hyperlink::target_of(cell.symbol()) {
                assert_eq!(target, link, "a cell links somewhere else");
                let ratatui::buffer::CellDiffOption::ForcedWidth(width) = cell.diff_option else {
                    panic!("a link cell at ({x}, {y}) with no forced width");
                };
                let label = cell
                    .symbol()
                    .split_once("\x1b\\")
                    .and_then(|(_, rest)| rest.strip_suffix("\x1b]8;;\x1b\\"))
                    .expect("a closed link");
                assert_eq!(label.chars().count(), width.get() as usize);
                spelled.push_str(label);
                skip = width.get() - 1;
                if rows.last() != Some(&y) {
                    rows.push(y);
                }
            }
        }
        assert_eq!(spelled, link);
        assert!(rows.len() >= 3, "the URL was meant to wrap: {rows:?}");

        // What is on screen has not moved a column: the text is the same, row
        // for row, as the frame drawn without the link — the bottom border
        // aside, whose hint only offers the copy when there is a link.
        let bottom = rows[rows.len() - 1] + 4;
        let text = |buf: &ratatui::buffer::Buffer| -> Vec<String> {
            hyperlink::visible(buf)
                .lines()
                .take(bottom as usize)
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(text(&linked), text(&plain));

        // And a plain click on any of those rows is still cctop's, and copies.
        let copy = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
        for y in rows {
            assert!(
                layout
                    .key_hits
                    .iter()
                    .any(|(row, _, _, key)| *row == y && *key == copy),
                "row {y} is not a click target"
            );
        }
    }

    #[test]
    fn a_long_sentence_wraps_between_words_and_a_url_is_cut() {
        let rows = wrap_words(
            "Add one at https://dash.example.test/a/very/long/path then",
            16,
        );
        assert_eq!(
            rows,
            [
                "Add one at",
                "https://dash.exa",
                "mple.test/a/very",
                "/long/path then"
            ]
        );
        assert!(rows.iter().all(|r| util::cells(r) <= 16));
        assert_eq!(wrap_words("", 10), [""]);
        // Cells, not chars: eight CJK characters are sixteen columns.
        assert_eq!(wrap_words("日本語の テキスト", 8), ["日本語の", "テキスト"]);
    }
}
