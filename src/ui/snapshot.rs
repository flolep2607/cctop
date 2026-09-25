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
//! * **Disk.** `App::with_prefs` loads the burn log and the hook claims from
//!   `$HOME`; both are replaced with empty ones. Working directories live under
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
use crate::session::{ContextBreakdown, ContextUsage, CtxPoint};
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

fn proc(pid: u32, cpu: f32, mb: u64, args: &str, is_root: bool) -> crate::proc::ProcEntry {
    crate::proc::ProcEntry {
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
    web.process = Some(crate::proc::ProcInfo {
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
    api.process = Some(crate::proc::ProcInfo {
        pids: 1,
        cpu: 0.4,
        memory: 180 * 1024 * 1024,
        command: "/usr/bin/claude".into(),
        process_list: vec![proc(5120, 0.4, 180, "/usr/bin/claude", true)],
    });
    api.activity_state = crate::session::ActivityState::WaitingForInput;
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
    // Loaded from `$HOME` by `with_prefs`; a developer's history must not
    // reach the frame.
    app.burn = crate::burn::Log::default();
    app.hook_pids = Default::default();
    app.hidden_columns = Vec::new();
    app.launch_root = Some("/nonexistent".into());

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
fn text(buffer: &Buffer) -> String {
    let area = buffer.area;
    (area.top()..area.bottom())
        .map(|y| {
            let line: String = (area.left()..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            line.trim_end().to_string()
        })
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

#[test]
fn help_modal() {
    let mut app = fixture();
    app.mode = Mode::Help;
    snap("help", &mut app);
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
