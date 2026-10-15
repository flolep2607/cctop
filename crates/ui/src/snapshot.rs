//! Snapshots of whole screens, drawn from a fixture that owes nothing to the
//! machine running the test.
//!
//! The assertions elsewhere in this directory each pin one fact about a frame —
//! a hint on the footer, a figure with its unit. These pin the frame itself, so
//! a change that moves a border, drops a column or rewraps a modal shows up as
//! a diff somebody has to accept, rather than passing because no assertion
//! happened to look there.
//!
//! Determinism is the whole job of the fixture, and the frame reads the clock,
//! the disk and the environment in more places than it is worth threading a
//! parameter through. So the fixture sidesteps each rather than injecting it:
//!
//! * **Time.** Every timestamp is set relative to `Utc::now()` and placed in
//!   the middle of the bucket it renders into — 7½ minutes reads `7m` whether
//!   the draw happens one millisecond or one second later. The Overview's
//!   sparklines mark the current local hour and day, so their series are left
//!   empty, and this month's spend is zero so its per-day average does not
//!   divide by today's date.
//! * **Disk.** The fixture itself — `App::with_prefs` — reads nothing: the burn
//!   log, the hook claims and the accounts under `$HOME` are all filled in by
//!   `App::new`, which no test here goes through. Working directories live under
//!   `/nonexistent`, so the branch column finds no repository to read and
//!   `tildify` has no home directory to shorten them against. The Info tab is
//!   never drawn, because it names the account signed in on this machine.
//! * **Colour.** No test calls `theme::init_from_env`, so the palette is the
//!   dark one `theme::colors` falls back to, whatever `$TERM`, `$COLORTERM` or
//!   `$NO_COLOR` the test runs under.
//!
//! Screens that animate — the share spinner, the tab pulse, a toast fading —
//! are left out: a frame that depends on how long ago something started is not
//! one a snapshot can hold still.

use super::*;
use cctop_core::session::{ContextBreakdown, ContextUsage, CtxPoint};
use chrono::{Duration as Age, Utc};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

/// The size most terminals open at, and the one cctop is most often squeezed
/// into.
const SMALL: (u16, u16) = (80, 24);
/// Wide and tall enough for every column and the bottom panel to have room.
const LARGE: (u16, u16) = (120, 40);

/// A timestamp `ago` before now, as the transcripts spell it.
fn ago(ago: Age) -> String {
    (Utc::now() - ago).to_rfc3339()
}

fn proc(pid: u32, cpu: f32, mb: u64, args: &str, is_root: bool) -> cctop_core::proc::ProcEntry {
    cctop_core::proc::ProcEntry {
        pid,
        cpu,
        memory: mb * 1024 * 1024,
        args: args.into(),
        is_root,
        ghost: false,
    }
}

fn session(id: &str, dir: &str, model: &str, last: Age, age: Age) -> Session {
    let mut s = Session::new(Provider::Claude, id.into());
    s.label_source = format!("/nonexistent/{dir}");
    s.abbrev_label = dir.into();
    s.model = model.into();
    s.last_active = ago(last);
    s.started_at = ago(age);
    s
}

/// Four sessions that between them light up most of what a row can show: one
/// working and expensive with a nearly full window, one idle, one finished,
/// one from Codex.
fn sessions() -> Vec<Session> {
    let mut web = session(
        "11111111-aaaa-4000-8000-000000000001",
        "web",
        "claude-opus-4-1",
        Age::seconds(20),
        Age::minutes(95),
    );
    web.process = Some(cctop_core::proc::ProcInfo {
        pids: 3,
        cpu: 12.5,
        memory: 412 * 1024 * 1024,
        command: "/usr/bin/claude --model opus".into(),
        process_list: vec![
            proc(4101, 9.8, 360, "/usr/bin/claude --model opus", true),
            proc(4188, 2.4, 44, "/usr/bin/node mcp-server.js", false),
            proc(4290, 0.3, 8, "/bin/bash -c npm test", false),
        ],
    });
    web.input_tokens = 1_240_000;
    web.output_tokens = 38_400;
    web.tool_count = 212;
    web.tool_errors = 4;
    web.total_cost = Some(18.42);
    web.cost_today = 6.10;
    web.cost_hour = 1.25;
    web.context = Some(ContextUsage {
        used: 151_000,
        max: 200_000,
        compacted: false,
    });
    web.last_tool = "Edit".into();
    web.title = Some("Fix the checkout redirect loop".into());

    let mut api = session(
        "22222222-bbbb-4000-8000-000000000002",
        "api",
        "claude-sonnet-4-5",
        Age::seconds(7 * 60 + 30),
        Age::minutes(150),
    );
    api.process = Some(cctop_core::proc::ProcInfo {
        pids: 1,
        cpu: 0.4,
        memory: 180 * 1024 * 1024,
        command: "/usr/bin/claude".into(),
        process_list: vec![proc(5120, 0.4, 180, "/usr/bin/claude", true)],
    });
    api.activity_state = cctop_core::session::ActivityState::WaitingForInput;
    api.input_tokens = 310_000;
    api.output_tokens = 9_800;
    api.tool_count = 57;
    api.total_cost = Some(2.87);
    api.cost_today = 2.87;
    api.context = Some(ContextUsage {
        used: 48_000,
        max: 200_000,
        compacted: false,
    });
    api.last_tool = "Bash".into();

    let mut docs = session(
        "33333333-cccc-4000-8000-000000000003",
        "docs",
        "claude-haiku-4-5",
        Age::minutes(3 * 60 + 30),
        Age::minutes(5 * 60),
    );
    docs.input_tokens = 42_000;
    docs.output_tokens = 3_100;
    docs.tool_count = 9;
    docs.total_cost = Some(0.19);
    docs.last_tool = "Read".into();

    let mut infra = session(
        "44444444-dddd-4000-8000-000000000004",
        "infra",
        "gpt-5-codex",
        Age::hours(2 * 24 + 12),
        Age::hours(3 * 24),
    );
    infra.provider = Provider::Codex;
    infra.input_tokens = 880_000;
    infra.output_tokens = 21_000;
    infra.tool_count = 140;
    infra.total_cost = Some(7.03);
    infra.last_tool = "shell".into();

    vec![web, api, docs, infra]
}

/// What the worker would have extracted for the `web` session: enough for the
/// Context tab to draw its bar, legend and timeline.
fn web_data() -> SessionData {
    let window = [
        (90, 22_000, false),
        (75, 61_000, false),
        (60, 98_000, false),
        (45, 126_000, false),
        (30, 139_000, false),
        (1, 151_000, false),
    ];
    SessionData {
        last_model: "claude-opus-4-1".into(),
        context_breakdown: Some(ContextBreakdown {
            total: 151_000,
            startup: 24_000,
            tool_output: 71_000,
            tool_input: 12_000,
            attachments: 4_000,
            user_text: 6_000,
            agent_text: 0,
            assistant_text: 19_000,
            after_compaction: false,
            superseded: false,
        }),
        context_series: window
            .iter()
            .map(|&(mins, window, after_compaction)| CtxPoint {
                ts: ago(Age::minutes(mins)),
                window,
                after_compaction,
            })
            .collect(),
        ..Default::default()
    }
}

/// The dashboard with the fixture loaded and the first row selected.
fn fixture() -> App {
    let mut app = tests::test_app();
    app.hidden_columns = Vec::new();
    app.launch_root = Some("/nonexistent".into());
    // `Quota::default` lists one pending entry per profile found under
    // `$HOME`, so a developer with a second Claude login drew an extra column.
    let pending = || {
        vec![cctop_core::quota::ProfileQuota {
            profile: "default".into(),
            status: cctop_core::quota::ProviderStatus::Pending,
            source: cctop_core::config::AccountSource::Directory,
        }]
    };
    app.quota = cctop_core::quota::Quota {
        fetched: false,
        claude: pending(),
        codex: pending(),
    };

    app.sessions = sessions();
    app.loaded = true;
    app.stats = Stats {
        total: 4,
        total_claude: 3,
        total_codex: 1,
        active_1h: 2,
        active_24h: 3,
        active_7d: 4,
        running: 2,
        total_input: 2_472_000,
        total_output: 72_300,
        total_cpu: 3.2,
        total_memory: 592 * 1024 * 1024,
        total_tools: 418,
        spend_total: 28.51,
        spend_claude: 21.48,
        spend_codex: 7.03,
        spend_hour: 1.25,
        spend_today: 8.97,
        spend_week: 28.51,
        spend_month: 28.51,
        spend_per_min: 0.04,
        top_today: vec![("web".into(), 6.10), ("api".into(), 2.87)],
        models_today: vec![
            ("claude-opus-4-1".into(), 6.10),
            ("claude-sonnet-4-5".into(), 2.87),
        ],
        ..Default::default()
    };
    app.refilter();
    app.selected = 0;
    // Processes rather than Info, the tab a fresh start opens on: Info names
    // the account signed in on this machine (read from `~/.claude.json`) and
    // prints the start time in the local timezone, and neither belongs in a
    // file every machine has to reproduce.
    app.bottom_tab = tab("Processes");
    app.panel_key = app.sessions[0].key();
    app.panel_data = Some(web_data());
    app
}

fn tab(name: &str) -> usize {
    panels::TABS
        .iter()
        .position(|t| *t == name)
        .unwrap_or_else(|| panic!("no {name} tab"))
}

fn draw(app: &mut App, (cols, rows): (u16, u16)) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(cols, rows)).expect("backend");
    terminal
        .draw(|frame| {
            render::draw(frame, app);
        })
        .expect("draw");
    terminal.backend().buffer().clone()
}

/// The frame as the characters on it, one line per row.
///
/// Trailing blanks are trimmed, because an empty cell and a space are the same
/// on screen and a diff of invisible whitespace helps nobody. The continuation
/// cell behind a wide character has an empty symbol, so joining symbols keeps
/// every row its true width.
///
/// A hyperlink is read as its label, as a screen shows it: the escape
/// sequence and the URL behind it are the terminal's business, and a
/// snapshot that held them would be one long line of percent-encoding.
fn text(buffer: &Buffer) -> String {
    super::hyperlink::visible(buffer)
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

/// The frame's styles as a grid of letters, with a legend.
///
/// Each distinct (fg, bg, modifier) gets a letter in order of first
/// appearance, and the default style a `.`, so the grid lines up with the
/// text snapshot cell for cell. A change of colour then reads as a changed
/// letter at the place it changed, rather than as a wall of escape codes.
fn styles(buffer: &Buffer) -> String {
    const MARKS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let area = buffer.area;
    let mut seen: Vec<(Color, Color, Modifier)> = Vec::new();
    let mut grid = Vec::new();
    for y in area.top()..area.bottom() {
        let mut line = String::new();
        for x in area.left()..area.right() {
            let cell = &buffer[(x, y)];
            let key = (cell.fg, cell.bg, cell.modifier);
            let plain = key == (Color::Reset, Color::Reset, Modifier::empty());
            let mark = match plain {
                true => '.',
                false => {
                    let at = seen.iter().position(|k| *k == key).unwrap_or_else(|| {
                        seen.push(key);
                        seen.len() - 1
                    });
                    MARKS.chars().nth(at).unwrap_or('?')
                }
            };
            line.push(mark);
        }
        grid.push(line.trim_end_matches('.').to_string());
    }
    let legend = seen
        .iter()
        .enumerate()
        .map(|(i, (fg, bg, modifier))| {
            format!(
                "{} fg={fg:?} bg={bg:?} mod={modifier:?}",
                MARKS.chars().nth(i).unwrap_or('?')
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}\n\n{legend}", grid.join("\n"))
}

/// Snapshot `app` at both sizes, named `<name>_<cols>x<rows>`.
fn snap(name: &str, app: &mut App) {
    for size in [LARGE, SMALL] {
        // The build on the help's border changes with every commit, which
        // no snapshot can hold; the same width of placeholder keeps the frame.
        let label = super::modals::build_label();
        let screen = text(&draw(app, size)).replace(
            &label,
            &format!("{:<w$}", "cctop VERSION", w = label.chars().count()),
        );
        // Caught here rather than in review: a frame that read this machine's
        // account passes locally, is committed, and fails on every other one.
        assert!(
            !screen.contains("Account "),
            "{name} drew the signed-in account:\n{screen}"
        );
        insta::assert_snapshot!(format!("{name}_{}x{}", size.0, size.1), screen);
    }
}

#[test]
fn dashboard() {
    let mut app = fixture();
    snap("dashboard", &mut app);
}

/// Colour is most of what the dashboard says — age, cost, context pressure —
/// so it gets a style snapshot as well. Once, at the large size: the grid is
/// long, and the small one would repeat it.
#[test]
fn dashboard_styles() {
    let mut app = fixture();
    let buffer = draw(&mut app, LARGE);
    insta::assert_snapshot!(styles(&buffer));
}

/// The Overview's sparklines and the Performance tab's line charts drawn in
/// octants, the set that needs a font with the Unicode 16 blocks. Braille is
/// every other frame here, so this is the one that shows what the choice
/// changes.
///
/// A triangle wave rather than anything with `sin` in it: a libm that rounds
/// one sample differently would move a dot, and the frame must be the same on
/// every machine.
#[test]
fn octant_charts() {
    let mut app = fixture();
    let wave = |n: usize, period: usize, top: f64| -> Vec<f64> {
        (0..n)
            .map(|i| {
                let t = i % period;
                let up = t.min(period - t) as f64;
                top * up / (period / 2) as f64
            })
            .collect()
    };
    for v in wave(120, 24, 80.0) {
        app.global_cpu.push(v);
    }
    for v in wave(60, 10, 0.2) {
        app.global_spend.push(v);
    }
    let key = app.sessions[0].key();
    let mut cpu = spark::History::default();
    let mut mem = spark::History::default();
    for (c, m) in wave(200, 40, 90.0).into_iter().zip(wave(200, 64, 400.0)) {
        cpu.push(c);
        mem.push(m + 100.0);
    }
    app.cpu_history.insert(key.clone(), cpu);
    app.mem_history.insert(key, mem);
    app.bottom_tab = tab("Performance");
    spark::set_glyphs(spark::Glyphs::Octant);
    snap("octant_charts", &mut app);
}

#[test]
fn help_modal() {
    let mut app = fixture();
    app.mode = Mode::Help;
    snap("help", &mut app);
}

/// The settings page on its own tab, and the same page with a filter over it.
///
/// Both, because the two things that make the page work are invisible in a
/// single frame: that it is a tab with the whole frame rather than a box over
/// the table, and that sixty-odd rows are navigable because they can be
/// filtered.
#[test]
fn settings_page() {
    use super::connect::{Connected, Row, Setup, Typing};
    use super::settings::Item;
    use cctop_core::cloudflare::Zone;
    use cctop_core::cloudflare::access::{Invite, Level, Settings};
    use cctop_core::tunnel::Account;
    let mut app = fixture();
    app.settings_open = true;
    snap("settings", &mut app);

    // A filter that matches only the alerts, so the cursor staying inside the
    // narrowed page is held in a frame as well as in a test.
    let mut app = fixture();
    app.settings_open = true;
    app.settings_filter = "alert when".into();
    snap("settings_filtered", &mut app);

    // The tunnel section with nothing connected: the two ways in.
    let mut app = fixture();
    app.settings_open = true;
    app.settings_filter = "tunnel".into();
    snap("settings_tunnel", &mut app);

    // Connected, with Access on and two invites, the cursor on remove and
    // remove asking. Every address is made up, and the token never shows.
    let mut app = fixture();
    app.settings_open = true;
    app.settings_filter = "tunnel".into();
    app.connected = Some(Connected::from(&Account {
        token: "eyJhIjoi-made-up".into(),
        hostname: Some("cctop.example.com".into()),
        share_hostname: Some("cctop-share.example.com".into()),
        account_id: Some("acct1".into()),
        zone_id: Some("zone1".into()),
        tunnel_id: Some("tunnel1".into()),
        api_token: Some("made-up-api-token".into()),
        access: Some(Box::new(Settings {
            owner: "owner@example.com".into(),
            invites: vec![
                Invite {
                    who: "@example.com".into(),
                    level: Level::Read,
                },
                Invite {
                    who: "boss@example.org".into(),
                    level: Level::Full,
                },
            ],
            link_hostname: Some("cctop-link.example.com".into()),
            ..Settings::default()
        })),
        ..Account::default()
    }));
    app.settings_cursor = app
        .settings_shown()
        .iter()
        .position(|i| *i == Item::Tunnel(Row::Remove))
        .expect("a remove row");
    app.cf.confirm = Some(Row::Remove);
    snap("settings_tunnel_connected", &mut app);

    // The same account served over it, its link drawn as a QR code over the
    // Overview and the pane: whole where they have the rows, said not to fit
    // where they have not. The token in the link is made up.
    app.cf.confirm = None;
    app.serving = Some(super::share::Served {
        local: "http://127.0.0.1:7777/?t=f3a9c2e17b4d0a6658e1c3b7d92f4e05".into(),
        public: Some("https://cctop.example.com/?t=f3a9c2e17b4d0a6658e1c3b7d92f4e05".into()),
        tunnel_fallback: None,
        readonly: String::new(),
        actions: true,
        publish: Box::new(|_, _, _| {}),
    });
    app.cf.qr = Some(super::connect::QrOf::Dashboard);
    snap("settings_tunnel_qr", &mut app);

    // A login being waited for, drawn with `q`. Its link is the real shape —
    // the store and a 32-byte key, escaped — because that length, not the
    // dashboard's, is what decides whether a terminal can show it. The key is
    // made up.
    let mut app = fixture();
    app.settings_open = true;
    app.settings_filter = "tunnel".into();
    app.cf.setup = Some(Setup::LoggingIn {
        url: "https://dash.cloudflare.com/argotunnel?aud=&callback=\
              https%3A%2F%2Flogin.cloudflareaccess.org%2F\
              q7Lx0bVd2mKp9RfT3wYc8NhJ5sGa1ZeU4oIi6EyQnWk%3D"
            .into(),
    });
    app.settings_cursor = app
        .settings_shown()
        .iter()
        .position(|i| *i == Item::Tunnel(Row::Login))
        .expect("a login row");
    app.cf.qr = Some(super::connect::QrOf::Login);
    snap("settings_tunnel_login_qr", &mut app);

    // A setup half done: two domains to cycle, and the name open to create.
    let mut app = fixture();
    app.settings_open = true;
    app.settings_filter = "tunnel".into();
    let zone = |name: &str| Zone {
        id: format!("id-{name}"),
        name: name.to_string(),
        account_id: "acct".to_string(),
        active: true,
    };
    app.cf.setup = Some(Setup::Picked {
        auth: "made-up-api-token".into(),
        zones: vec![zone("example.com"), zone("example.org")],
        at: 0,
        hostname: "cctop-box.example.com".into(),
    });
    app.settings_cursor = app
        .settings_shown()
        .iter()
        .position(|i| *i == Item::Tunnel(Row::SetupHostname))
        .expect("a hostname row");
    app.cf.typing = Some(Typing::SetupHostname);
    app.settings_input = Some("cctop-box.example.com".into());
    snap("settings_tunnel_setup", &mut app);
}

/// A binding is drawn as a cap, which no text frame can show — and the footer
/// at the bottom of the very same frame has drawn its keys as caps all along,
/// so this is the one place the page and the footer could have disagreed
/// without anybody noticing. Filtered to the bindings, because on an unfiltered
/// page the `[keys]` section is below the fold and a style snapshot of rows
/// nobody can see proves nothing.
#[test]
fn settings_keys_styles() {
    let mut app = fixture();
    app.settings_open = true;
    app.settings_filter = "quit".into();
    let buffer = draw(&mut app, LARGE);
    insta::assert_snapshot!(styles(&buffer));
}

/// And what the filter's own mark looks like, which a text frame cannot show
/// at all. Narrowed to one row, so the grid is short enough to read.
#[test]
fn settings_filtered_styles() {
    let mut app = fixture();
    app.settings_open = true;
    app.settings_filter = "alert_cost".into();
    let buffer = draw(&mut app, LARGE);
    insta::assert_snapshot!(styles(&buffer));
}

/// The settings page's colours, which the text snapshots cannot carry: each
/// value type in its own ink, a bool green or red, and the wash a set value
/// gets behind it.
///
/// Two values are set on the fixture for the wash, because the page draws only
/// defaults otherwise and a highlight nobody can see is one the snapshot would
/// happily let regress. A number and a bool, so both readings are pinned: the
/// wash over a type colour, and a true toggle as `[x]` plus its green.
#[test]
fn settings_styles() {
    let mut app = fixture();
    app.settings_open = true;
    app.settings.notify = Some(true);
    app.settings.idle_after = Some(3.0);
    let buffer = draw(&mut app, LARGE);
    insta::assert_snapshot!(styles(&buffer));
}

#[test]
fn context_panel() {
    let mut app = fixture();
    app.bottom_tab = tab("Context");
    snap("context", &mut app);
}

/// A filter that is on narrows the table and brings `Esc` onto the footer.
#[test]
fn filter_active() {
    let mut app = fixture();
    app.search = "web".into();
    app.refilter();
    snap("filter_active", &mut app);
}

/// The search box open mid-typing, over the table it is narrowing.
#[test]
fn search_typing() {
    let mut app = fixture();
    app.mode = Mode::Search;
    app.search = "ap".into();
    app.refilter();
    snap("search_typing", &mut app);
}

#[test]
fn sort_modal() {
    let mut app = fixture();
    app.mode = Mode::SortBy;
    snap("sort", &mut app);
}

#[test]
fn row_menu() {
    let mut app = fixture();
    app.mode = Mode::RowMenu;
    snap("row_menu", &mut app);
}

/// The handoff picker for a Claude session on its `work` account: every
/// (agent, account) pair but that one, so `claude · default` is offered and
/// `claude · work` is not, and the column beside each agent says whose
/// subscription it would spend.
#[test]
fn handoff_picker() {
    use cctop_core::handoff::Target;
    let mut app = fixture();
    let to = |agent: &str, account: Option<&str>| Target {
        agent: agent.into(),
        account: account.map(str::to_string),
    };
    app.pending_brief = Some("/home/me/.cache/cctop/handoff/web.md".into());
    app.open_handoff(vec![
        to("claude", Some("default")),
        to("codex", Some("default")),
        to("codex", Some("spare")),
        to("opencode", None),
    ]);
    app.launch_cwd = Some("/home/me/src/web".into());
    app.launch_cursor = 2;
    snap("handoff_picker", &mut app);
}

/// The new-tab launcher pointed at a directory on an ssh host: the line says
/// `in host:path`, codex is greyed with why it cannot go there, and the reason
/// is spelled out under the list while it is the one picked.
#[test]
fn new_tab_remote() {
    use super::location::RemoteTarget;
    let mut app = fixture();
    app.launch_offer = vec![
        tabs::Choice::Start(vec!["claude".into()]),
        tabs::Choice::Start(vec!["codex".into()]),
        tabs::Choice::Start(vec!["opencode".into()]),
    ];
    app.launch_into = LaunchInto::Tab;
    app.launch_remote = Some(RemoteTarget {
        host: "procdb".into(),
        path: "~/proj".into(),
        resolved: Some("/home/me/proj".into()),
        facts: None,
        problem: None,
        unchecked: None,
        peer: false,
    });
    app.launch_cursor = 1;
    app.mode = Mode::Launch;
    snap("new_tab_remote", &mut app);
}

/// The new-tab launcher pointed at a project on a sibling cctop: the line
/// says it goes through the tunnel, the agents are offered with no sandbox
/// to need, and the shell is offered as the sibling's own, named `shell`
/// rather than after this machine's.
#[test]
fn new_tab_peer() {
    use super::location::RemoteTarget;
    let mut app = fixture();
    app.launch_offer = vec![
        tabs::Choice::Start(vec!["claude".into()]),
        tabs::Choice::Start(vec!["codex".into()]),
        tabs::Choice::Start(vec!["/bin/sh".into()]),
    ];
    app.launch_into = LaunchInto::Tab;
    app.launch_remote = Some(RemoteTarget {
        host: "computer36".into(),
        path: "/srv/api".into(),
        resolved: None,
        facts: None,
        problem: None,
        unchecked: Some("computer36 checks it".into()),
        peer: true,
    });
    app.launch_cursor = 2;
    app.mode = Mode::Launch;
    snap("new_tab_peer", &mut app);
}

/// The directory field with a sibling on the Cloudflare account offered
/// beside the ssh hosts, then that sibling in the field, offering the
/// projects its rows report.
#[test]
fn launch_cwd_peers() {
    use super::location::{Hit, PeerHost};
    let mut app = fixture();
    app.launch_offer = vec![tabs::Choice::Start(vec!["/bin/sh".into()])];
    app.launch_cursor = 0;
    app.launch_cwd_input.clear();
    app.peer_hosts = vec![PeerHost {
        name: "computer36".into(),
        offline: None,
    }];
    app.launch_cwd_hits = vec![
        Hit::Dir("/home/me/src/web".into()),
        Hit::Peer {
            name: "computer36".into(),
            offline: None,
        },
        Hit::Host {
            name: "devbox".into(),
            aliases: vec![],
        },
    ];
    app.launch_cwd_pick = Some(1);
    app.mode = Mode::LaunchCwd;
    snap("launch_cwd_peers", &mut app);

    let remote = |path: &str| Hit::Remote {
        host: "computer36".into(),
        path: path.into(),
        problem: None,
    };
    app.launch_cwd_input.set("computer36:".to_string());
    app.launch_cwd_hits = vec![remote("/srv/api"), remote("/home/me/notes")];
    app.launch_cwd_pick = None;
    snap("launch_cwd_peer_projects", &mut app);
}

/// A harness's options: its model, its permission setting — YOLO in the
/// colour of a warning — and a worktree, which is greyed with why where the
/// launch is not into a repository; under them, where it starts and the
/// command Enter runs. Codex, whose account line shows only on a machine
/// with two accounts, which this snapshot cannot depend on.
#[test]
fn launch_options() {
    use cctop_core::launch_opts::{Options, Permissions};
    // The account line is the machine's: drawn on one with two Codex
    // logins, and a snapshot cannot depend on which machine it runs on.
    if cctop_core::config::launchable_for(Provider::Codex).len() > 1 {
        return;
    }
    let mut app = fixture();
    app.launch_offer = vec![
        tabs::Choice::Start(vec!["claude".into()]),
        tabs::Choice::Start(vec!["codex".into()]),
    ];
    app.launch_cursor = 1;
    app.launch_root = None;
    app.launch_cwd = Some("/nonexistent/web".into());
    app.launch_options.insert(
        "codex".into(),
        super::harness_opts::HarnessOptions {
            options: Options {
                model: Some("gpt-5-codex".into()),
                permissions: Permissions::Bypass,
            },
            branch: None,
        },
    );
    app.mode = Mode::LaunchOptions;
    app.launch_opt_cursor = 1;
    snap("launch_options", &mut app);

    // Back in the list, the row says what is set.
    app.mode = Mode::Launch;
    snap("launch_options_summary", &mut app);
}

/// A harness's options for an agent started on a sibling: the account line
/// offers the sibling's accounts, as it named them, never this machine's —
/// and the list under it says which one, the same way.
#[test]
fn launch_options_peer_account() {
    use super::location::RemoteTarget;
    let mut app = fixture();
    app.launch_offer = vec![
        tabs::Choice::Start(vec!["claude".into()]),
        tabs::Choice::Start(vec!["codex".into()]),
    ];
    app.launch_cursor = 0;
    app.launch_remote = Some(RemoteTarget {
        host: "computer40".into(),
        path: "/srv/api".into(),
        resolved: None,
        facts: None,
        problem: None,
        unchecked: Some("computer40 checks it".into()),
        peer: true,
    });
    cctop_core::peer::set_accounts("computer40", "claude", &["default", "work"]);
    app.launch_peer_picked
        .insert(("computer40".into(), Provider::Claude), "work".into());
    app.mode = Mode::LaunchOptions;
    app.launch_opt_cursor = 1;
    snap("launch_options_peer_account", &mut app);

    app.mode = Mode::Launch;
    snap("launch_options_peer_account_list", &mut app);
}

/// The directory field before anything is typed: the projects, then the ssh
/// hosts as `name:`, with their other names beside them.
#[test]
fn launch_cwd_hosts() {
    use super::location::Hit;
    let mut app = fixture();
    // A plain shell, which has no accounts: a `claude` row would add an
    // `as <account>` line only on a machine that has some.
    app.launch_offer = vec![tabs::Choice::Start(vec!["/bin/sh".into()])];
    app.launch_cursor = 0;
    app.launch_cwd_input.clear();
    app.launch_cwd_hits = vec![
        Hit::Dir("/home/me/src/web".into()),
        Hit::Dir("/home/me/src/api".into()),
        Hit::Host {
            name: "nz-b-procurementdb1".into(),
            aliases: vec!["procdb".into()],
        },
        Hit::Host {
            name: "devbox".into(),
            aliases: vec![],
        },
    ];
    app.launch_cwd_pick = Some(2);
    app.mode = Mode::LaunchCwd;
    snap("launch_cwd_hosts", &mut app);
}

/// A host in the field: its directories completed like local ones, one marked
/// with why the agent could not work in it — and, for a host that would need a
/// prompt, the line that says so instead of a list.
#[test]
fn launch_cwd_remote() {
    use super::location::{Conn, Dir, Hit, HostState};
    let mut app = fixture();
    // A plain shell, which has no accounts: a `claude` row would add an
    // `as <account>` line only on a machine that has some.
    app.launch_offer = vec![tabs::Choice::Start(vec!["/bin/sh".into()])];
    app.launch_cursor = 0;
    let listed = HostState {
        conn: Conn::Ready {
            home: "/home/me".into(),
        },
        repos: Some(Vec::new()),
        dirs: [(
            "~/src".to_string(),
            Dir::Listed(cctop_core::remote_fs::Listing::default()),
        )]
        .into_iter()
        .collect(),
    };
    app.ssh_states.insert("procdb".into(), listed);
    app.launch_cwd_input.set("procdb:~/src/".to_string());
    let remote = |path: &str, problem: Option<&str>| Hit::Remote {
        host: "procdb".into(),
        path: path.into(),
        problem: problem.map(str::to_string),
    };
    app.launch_cwd_hits = vec![
        remote("~/src/api", None),
        remote("~/src/cctop", Some("read-only on the host")),
        remote("~/src/web", None),
    ];
    app.mode = Mode::LaunchCwd;
    snap("launch_cwd_remote", &mut app);

    app.ssh_states.insert(
        "devbox".into(),
        HostState {
            conn: Conn::Offline("needs a password or key prompt".into()),
            repos: None,
            dirs: Default::default(),
        },
    );
    app.launch_cwd_input.set("devbox:~/src".to_string());
    app.launch_cwd_hits.clear();
    snap("launch_cwd_offline", &mut app);
}

#[test]
fn new_worktree() {
    let mut app = fixture();
    app.worktree_base = "/home/me/src/cctop/.claude/worktrees/side".into();
    app.worktree_repo = "/home/me/src/cctop".into();
    app.worktree_input = "try-fork".into();
    app.mode = Mode::NewWorktree;
    snap("new_worktree", &mut app);
}

#[test]
fn delete_confirm() {
    let mut app = fixture();
    // A finished row, since a running one gets the "stop it first" modal
    // instead. Processes is a live-only tab and would fall back to Info, which
    // the fixture never draws, so the panel is the Context tab — and the
    // extraction behind it is this row's own, not `web`'s.
    app.selected = 2;
    app.panel_key = app.sessions[2].key();
    app.panel_data = Some(SessionData::default());
    app.bottom_tab = tab("Context");
    app.mode = Mode::DeleteConfirm;
    snap("delete_confirm", &mut app);
}

/// The idle view, with one session to stop and one it will not: `web` has been
/// quiet for hours but its tree is still busy, `api` has been sitting for a
/// day. The title carries the total, and `K` names both.
#[test]
fn idle_view_and_reclaim() {
    let mut app = fixture();
    app.sessions[0].last_active = ago(Age::hours(9));
    app.sessions[1].last_active = ago(Age::hours(26));
    app.toggle_idle_view();
    // The toast opening it would otherwise cover the corner being pinned.
    app.toasts = Default::default();
    snap("idle_view", &mut app);
    app.batch(super::BatchKind::Reclaim);
    assert_eq!(app.mode, Mode::BatchConfirm);
    snap("idle_reclaim", &mut app);
}

/// A tab whose agent died at once: its last screen, frozen, and the line
/// across the bottom saying how it ended and that any key closes it.
#[test]
fn dead_pane() {
    let mut app = fixture();
    let mut pane = tabs::Pane::for_test("claude");
    pane.view
        .parser
        .process(b"Starting claude...\r\nerror: not logged in - run `claude /login`\r\n");
    pane.dead = Some(tabs::Death {
        code: Some(1),
        signal: None,
        lived: Some(std::time::Duration::from_millis(400)),
        last_line: Some("error: not logged in - run `claude /login`".into()),
    });
    app.tabs.push(tabs::Tab::new(pane));
    app.tab = 1;
    snap("dead_pane", &mut app);
}

/// Tabs on other machines: a session opened on a sibling through its
/// Cloudflare tunnel, and an agent launched on an ssh host, each marked before
/// its label beside a local one.
#[test]
fn remote_tabs() {
    use cctop_core::reach::Transport;
    let mut app = fixture();
    let remote = |label: &str, transport| {
        let mut pane = tabs::Pane::for_test(label);
        pane.transport = Some(transport);
        tabs::Tab::new(pane)
    };
    app.tabs
        .push(tabs::Tab::new(tabs::Pane::for_test("claude")));
    app.tabs
        .push(remote("desk · Fix the login hint", Transport::Peer));
    app.tabs.push(remote("codex@procdb", Transport::Ssh));
    snap("remote_tabs", &mut app);
}

/// The bar on each other side `tab_bar` puts it, inside a tab: a title wide in
/// cells, one with an emoji sequence, a remote tab's mark and a zoomed split's,
/// so every way a label is measured and cut is on screen at once.
#[test]
fn tab_bar_sides() {
    use super::render::{TabBar, set_tab_bar};
    use cctop_core::reach::Transport;
    let mut app = fixture();
    let mut split = tabs::Tab::new(tabs::Pane::for_test("claude"));
    split.panes.push(tabs::Pane::for_test("zsh"));
    split.toggle_zoom();
    app.tabs.push(split);
    app.tabs.push(tabs::Tab::new(tabs::Pane::for_test(
        "日本語プロジェクトの長いタイトル",
    )));
    let mut remote = tabs::Pane::for_test("desk · Fix the login hint");
    remote.transport = Some(Transport::Peer);
    app.tabs.push(tabs::Tab::new(remote));
    app.tabs.push(tabs::Tab::new(tabs::Pane::for_test(
        "⚠\u{FE0F} 🧑\u{200D}🌾 hotfix",
    )));
    app.tab = 2;
    for (side, name) in [
        (TabBar::Bottom, "bottom"),
        (TabBar::Left, "left"),
        (TabBar::Right, "right"),
    ] {
        set_tab_bar(side);
        let screen = text(&draw(&mut app, LARGE));
        insta::assert_snapshot!(format!("tab_bar_{name}_120x40"), screen);
    }
    set_tab_bar(TabBar::Top);
}

/// Inside a sibling's tab, its link in the bottom-right corner: the round trip,
/// the reconnects so far and how well the output compresses, beside the share
/// button.
#[test]
fn remote_tab_link() {
    use cctop_core::link_health::{State, Stats};
    use cctop_core::reach::Transport;
    let mut app = fixture();
    let mut pane = tabs::Pane::for_test("desk · Fix the login hint");
    pane.transport = Some(Transport::Peer);
    app.tabs.push(tabs::Tab::new(pane));
    app.tab = 1;
    app.link = Some(super::link::Seen::fixed(
        "cctop-claude-on-desk-a",
        Transport::Peer,
        Some(Stats {
            state: State::Connected,
            rtt_ms: Some(118),
            reconnects: 2,
            raw_in: 41_000,
            wire_in: 10_000,
            raw_out: 0,
            wire_out: 0,
            since: 0,
            at: 0,
        }),
    ));
    snap("remote_tab_link", &mut app);
}

/// Three tabs on siblings, as the bar shows them when one machine is being
/// dialled again and another has stopped answering: `↻` and `✕` after the
/// label, on top of the cloud's amber and red, so a dead machine's tab is
/// seen without opening it.
#[test]
fn remote_tabs_down() {
    use cctop_core::link_health::State;
    use cctop_core::reach::Transport;
    let mut app = fixture();
    for (title, session, state) in [
        ("desk · web", "cctop-claude-on-desk-a", None),
        (
            "desk · issue 325",
            "cctop-claude-on-desk-b",
            Some(State::Reconnecting),
        ),
        (
            "laptop · backfill",
            "cctop-claude-on-laptop-c",
            Some(State::Lost),
        ),
    ] {
        let mut pane = tabs::Pane::for_test(title);
        pane.transport = Some(Transport::Peer);
        pane.rmux = Some(session.to_string());
        app.tabs.push(tabs::Tab::new(pane));
        if let Some(state) = state {
            app.down.states.insert(session.to_string(), state);
        }
    }
    let screen = text(&draw(&mut app, LARGE));
    let bar = screen.lines().next().unwrap_or_default();
    assert!(bar.contains("desk · issue 325 ↻"), "{bar}");
    assert!(bar.contains("laptop · backfill ✕"), "{bar}");
    assert!(
        !bar.contains("desk · web ↻") && !bar.contains("desk · web ✕"),
        "{bar}"
    );
    insta::assert_snapshot!("remote_tabs_down_120x40", screen);
}

/// Choosing an address: the share panel with its `Address` line, the field
/// `e` opens on it, and the dashboard's field with its warning. Every token
/// and link here is made up, and only hostnames reach the screen.
#[test]
fn address_rename() {
    use super::address::{AddressEdit, Target};
    use super::share::{Naming, ShareQr};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = fixture();
    app.share_qr = Some(ShareQr {
        label: "web".to_string(),
        link: format!(
            "https://fix-tests.example.com/#e=wss://fix-tests.example.com/share&t={}",
            "0123456789abcdef".repeat(2)
        ),
        pin: Some("482913".to_string()),
        session_id: "11111111-aaaa-4000-8000-000000000001".to_string(),
        name: "cctop-claude-11111111".to_string(),
        host: Some("fix-tests.example.com".to_string()),
        rename: Ok(Naming {
            zone: "example.com".to_string(),
            default: Some("cctop-share.example.com".to_string()),
        }),
    });
    app.mode = Mode::ShareQr;
    snap("share_address", &mut app);
    app.on_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::RenameAddress);
    snap("share_rename", &mut app);

    let mut app = fixture();
    let mut field = super::line_edit::LineEdit::default();
    field.set("home");
    app.address = Some(AddressEdit::for_test(
        Target::Dashboard,
        "cctop.example.com",
        "example.com",
        field,
    ));
    app.mode = Mode::RenameAddress;
    snap("dashboard_rename", &mut app);
}
