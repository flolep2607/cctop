//! Colour palette. btop-inspired: dark ground, few saturated accents, and
//! meaning carried by hue (green = idle/cheap, amber = warming, red = hot).
//!
//! Three variants, chosen once at startup by [`init_from_env`]:
//!
//! * **Dark** — the original palette, unchanged. Still the default.
//! * **Light** — the same meanings at readable contrast on a pale ground,
//!   which mostly means darker, more saturated ink instead of pastels.
//! * **Mono** — `NO_COLOR`. Every colour becomes `Color::Reset` and emphasis
//!   moves to `Modifier`, so the terminal's own scheme is left alone. The
//!   shapes still carry the state: `●` versus `○` for running, `FREE`/`incl`
//!   in the cost cells, `▲`/`▼` on the sorted column.
//!
//! Everything reads the active palette through [`colors`] rather than through
//! constants, since the choice isn't known until the process has looked at its
//! environment.
//!
//! Two things about the terminal feed that choice, and both are asked once:
//!
//! * **Its ground.** `auto` asks the terminal what its background is (OSC 11)
//!   before the TUI takes the input stream, then falls back to `COLORFGBG`,
//!   then to dark. See [`query_ground`].
//! * **Its colour depth**, read from the environment by `termprofile`. Both
//!   palettes are written in the 256-colour cube; a terminal that can show
//!   only sixteen gets them folded down by [`sixteen`], and one that can show
//!   none gets Mono. See [`Depth`].

use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use termprofile::TermProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Dark,
    Light,
    /// `NO_COLOR`: shape and weight only.
    Mono,
}

/// How much colour the terminal can show, as far as cctop is concerned.
///
/// Coarser than termprofile's [`TermProfile`] on purpose: every palette here is
/// indexed, so truecolor draws exactly what 256 colours does, and the only
/// lines that change what reaches the screen are "sixteen" and "none".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    /// 256 colours or more: the palettes as written.
    Indexed,
    /// The sixteen ANSI colours, whose actual shades the terminal's scheme sets.
    Ansi16,
    /// Modifiers only, which is Mono whatever the theme says.
    Colorless,
}

impl Depth {
    fn of(profile: TermProfile) -> Depth {
        match profile {
            TermProfile::NoColor => Depth::Colorless,
            TermProfile::Ansi16 => Depth::Ansi16,
            // `NoTty` is stdout not being a terminal, which says nothing about
            // the one cctop would draw on — so it draws the way every release
            // before this one did, rather than guessing.
            TermProfile::NoTty | TermProfile::Ansi256 | TermProfile::TrueColor => Depth::Indexed,
        }
    }
}

/// What the terminal said its background is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ground {
    Light,
    Dark,
}

/// Every colour the UI can ask for, resolved for one [`Variant`].
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub variant: Variant,
    /// What the colours below were folded to. [`gray`], [`tool_color`] and the
    /// tab hues build colours after the fact and read this to fold theirs alike.
    pub depth: Depth,

    pub border: Color,
    pub border_hi: Color,
    pub panel_title: Color,

    pub label: Color,
    pub value: Color,
    pub dim: Color,
    pub dimmer: Color,
    pub accent: Color,
    /// Foreground for text printed *on* `accent` (the footer key caps).
    pub on_accent: Color,
    /// The active-search badge, which is deliberately not the accent hue.
    pub filter_badge: Color,

    pub cost_low: Color,
    pub cost_mid: Color,
    pub cost_high: Color,

    pub claude: Color,
    pub openai: Color,
    pub cursor: Color,
    pub opencode: Color,
    pub pi: Color,
    pub gemini: Color,
    pub windsurf: Color,
    /// Open-weight vendors seen through custom providers. Chosen to stay clear
    /// of each other and of the harness hues above.
    pub glm: Color,
    pub kimi: Color,
    pub deepseek: Color,
    pub qwen: Color,
    pub grok: Color,
    pub desktop_code: Color,
    pub desktop_cowork: Color,

    /// The page itself. Dark leaves this `Reset` so the terminal's own ground
    /// shows through — that is the look every previous release shipped. Light
    /// paints a pale canvas, because dark ink on a dark emulator is just a
    /// blank screen.
    pub ground: Color,
    pub selected_bg: Color,
    /// Wash behind a tool call that reported an error. Dark enough to stay
    /// behind the row's own foreground colours rather than competing with them.
    pub failed_bg: Color,
    pub header_bg: Color,
    /// Tint for rows marked for a batch action, so marks read at a glance even
    /// when nothing is selected. Kept dim so it stays behind the data.
    pub marked_bg: Color,

    /// A freshly active session's status dot, and the step just below it.
    pub dot_fresh: Color,
    pub dot_warm: Color,

    /// Named things that are neither data nor chrome: MCP servers, config keys.
    pub name_hue: Color,
    /// Extra hues for categorical breakdowns, where the only meaning is "a
    /// different slice from the one next to it".
    pub chart_hues: [Color; 2],
    /// Hues cycled by [`tool_color`], one per tool name.
    pub tool_hues: [u8; 10],

    pub spark_cpu: [Color; 6],
    pub spark_spend: [Color; 6],
    pub spark_accent: [Color; 5],
    pub spark_baseline: Color,
}

/// The palette this build started with. Byte-identical to the pre-theme
/// constants: the dark look must not move by so much as one colour index.
const DARK: Palette = Palette {
    variant: Variant::Dark,
    depth: Depth::Indexed,
    border: Color::Indexed(60),
    border_hi: Color::Indexed(75),
    panel_title: Color::Indexed(179),
    label: Color::Indexed(75),
    value: Color::White,
    dim: Color::Indexed(245),
    dimmer: Color::Indexed(238),
    accent: Color::Indexed(75),
    on_accent: Color::Black,
    filter_badge: Color::Cyan,
    cost_low: Color::Indexed(114),
    cost_mid: Color::Indexed(221),
    cost_high: Color::Indexed(203),
    claude: Color::Indexed(173),
    openai: Color::Indexed(110),
    cursor: Color::Indexed(141),
    opencode: Color::Indexed(117),
    pi: Color::Indexed(150),
    gemini: Color::Indexed(74),
    windsurf: Color::Indexed(80),
    glm: Color::Indexed(108),
    kimi: Color::Indexed(216),
    deepseek: Color::Indexed(105),
    qwen: Color::Indexed(180),
    grok: Color::Indexed(247),
    desktop_code: Color::Indexed(141),
    desktop_cowork: Color::Indexed(183),
    ground: Color::Reset,
    selected_bg: Color::Indexed(236),
    failed_bg: Color::Indexed(52),
    header_bg: Color::Indexed(236),
    marked_bg: Color::Indexed(53),
    dot_fresh: Color::Indexed(82),
    dot_warm: Color::Indexed(71),
    name_hue: Color::Indexed(180),
    chart_hues: [Color::Indexed(109), Color::Indexed(139)],
    tool_hues: [75, 114, 173, 180, 139, 109, 146, 215, 152, 167],
    spark_cpu: [
        Color::Indexed(236),
        Color::Indexed(71),
        Color::Indexed(114),
        Color::Indexed(186),
        Color::Indexed(221),
        Color::Indexed(203),
    ],
    spark_spend: [
        Color::Indexed(236),
        Color::Indexed(71),
        Color::Indexed(114),
        Color::Indexed(186),
        Color::Indexed(221),
        Color::Indexed(203),
    ],
    spark_accent: [
        Color::Indexed(238),
        Color::Indexed(60),
        Color::Indexed(68),
        Color::Indexed(75),
        Color::Indexed(117),
    ],
    spark_baseline: Color::Indexed(236),
};

/// The same meanings against a pale ground. Pastels become ink: on white,
/// contrast has to come from darkness rather than from brightness.
const LIGHT: Palette = Palette {
    variant: Variant::Light,
    depth: Depth::Indexed,
    border: Color::Indexed(146),
    border_hi: Color::Indexed(25),
    panel_title: Color::Indexed(94),
    label: Color::Indexed(25),
    value: Color::Black,
    dim: Color::Indexed(243),
    // 249 on white is a ghost. One step past `dim` is still secondary, and
    // still a colour you can actually read.
    dimmer: Color::Indexed(246),
    accent: Color::Indexed(25),
    on_accent: Color::White,
    filter_badge: Color::Indexed(23),
    cost_low: Color::Indexed(28),
    cost_mid: Color::Indexed(130),
    cost_high: Color::Indexed(124),
    claude: Color::Indexed(130),
    openai: Color::Indexed(24),
    cursor: Color::Indexed(91),
    opencode: Color::Indexed(31),
    pi: Color::Indexed(64),
    gemini: Color::Indexed(26),
    windsurf: Color::Indexed(30),
    // Darkened counterparts of the dark palette's vendor hues, kept clear of
    // each other and of the harness colours above on a white background.
    glm: Color::Indexed(65),
    kimi: Color::Indexed(166),
    deepseek: Color::Indexed(55),
    qwen: Color::Indexed(100),
    grok: Color::Indexed(238),
    desktop_code: Color::Indexed(91),
    desktop_cowork: Color::Indexed(97),
    ground: Color::White,
    // 253 on white is a wash you cannot find. 250 is the same kind of grey
    // step the dark theme uses (236 against black), just mirrored.
    selected_bg: Color::Indexed(250),
    failed_bg: Color::Indexed(224),
    header_bg: Color::Indexed(250),
    marked_bg: Color::Indexed(225),
    dot_fresh: Color::Indexed(34),
    dot_warm: Color::Indexed(22),
    name_hue: Color::Indexed(94),
    tool_hues: [25, 28, 130, 94, 90, 24, 242, 166, 64, 124],
    chart_hues: [Color::Indexed(24), Color::Indexed(91)],
    spark_cpu: [
        Color::Indexed(252),
        Color::Indexed(22),
        Color::Indexed(28),
        Color::Indexed(136),
        Color::Indexed(130),
        Color::Indexed(124),
    ],
    spark_spend: [
        Color::Indexed(252),
        Color::Indexed(22),
        Color::Indexed(28),
        Color::Indexed(136),
        Color::Indexed(130),
        Color::Indexed(124),
    ],
    spark_accent: [
        Color::Indexed(252),
        Color::Indexed(146),
        Color::Indexed(67),
        Color::Indexed(25),
        Color::Indexed(24),
    ],
    spark_baseline: Color::Indexed(252),
};

/// No colour at all: the terminal's defaults, with `Modifier` doing the work.
const MONO: Palette = {
    let r = Color::Reset;
    Palette {
        variant: Variant::Mono,
        depth: Depth::Colorless,
        border: r,
        border_hi: r,
        panel_title: r,
        label: r,
        value: r,
        dim: r,
        dimmer: r,
        accent: r,
        on_accent: r,
        filter_badge: r,
        cost_low: r,
        cost_mid: r,
        cost_high: r,
        claude: r,
        openai: r,
        cursor: r,
        opencode: r,
        pi: r,
        gemini: r,
        windsurf: r,
        glm: r,
        kimi: r,
        deepseek: r,
        qwen: r,
        grok: r,
        desktop_code: r,
        desktop_cowork: r,
        ground: r,
        selected_bg: r,
        failed_bg: r,
        header_bg: r,
        marked_bg: r,
        dot_fresh: r,
        dot_warm: r,
        name_hue: r,
        chart_hues: [r; 2],
        // Unused in mono: `tool_color` returns `Reset` outright.
        tool_hues: [0; 10],
        spark_cpu: [r; 6],
        spark_spend: [r; 6],
        spark_accent: [r; 5],
        spark_baseline: r,
    }
};

impl Palette {
    /// Every colour slot passed through `f`.
    ///
    /// Spelled out field by field rather than with `..self`, so a slot added
    /// later is a compile error here instead of a 256-colour index that slips
    /// through unfolded onto a sixteen-colour screen.
    fn map(self, f: impl Fn(Color) -> Color) -> Palette {
        Palette {
            variant: self.variant,
            depth: self.depth,
            border: f(self.border),
            border_hi: f(self.border_hi),
            panel_title: f(self.panel_title),
            label: f(self.label),
            value: f(self.value),
            dim: f(self.dim),
            dimmer: f(self.dimmer),
            accent: f(self.accent),
            on_accent: f(self.on_accent),
            filter_badge: f(self.filter_badge),
            cost_low: f(self.cost_low),
            cost_mid: f(self.cost_mid),
            cost_high: f(self.cost_high),
            claude: f(self.claude),
            openai: f(self.openai),
            cursor: f(self.cursor),
            opencode: f(self.opencode),
            pi: f(self.pi),
            gemini: f(self.gemini),
            windsurf: f(self.windsurf),
            glm: f(self.glm),
            kimi: f(self.kimi),
            deepseek: f(self.deepseek),
            qwen: f(self.qwen),
            grok: f(self.grok),
            desktop_code: f(self.desktop_code),
            desktop_cowork: f(self.desktop_cowork),
            ground: f(self.ground),
            selected_bg: f(self.selected_bg),
            failed_bg: f(self.failed_bg),
            header_bg: f(self.header_bg),
            marked_bg: f(self.marked_bg),
            dot_fresh: f(self.dot_fresh),
            dot_warm: f(self.dot_warm),
            name_hue: f(self.name_hue),
            chart_hues: self.chart_hues.map(&f),
            // Indices, not colours: `tool_color` folds what it builds from them.
            tool_hues: self.tool_hues,
            spark_cpu: self.spark_cpu.map(&f),
            spark_spend: self.spark_spend.map(&f),
            spark_accent: self.spark_accent.map(&f),
            spark_baseline: f(self.spark_baseline),
        }
    }
}

/// `color` as the terminal at `depth` can show it: the nearest of the sixteen
/// by termprofile's table when that is all there is, and untouched otherwise.
/// Named colours and `Reset` are already among the sixteen and pass through.
fn fold(color: Color, depth: Depth) -> Color {
    match depth {
        Depth::Ansi16 => TermProfile::Ansi16.adapt_color(color).unwrap_or(color),
        Depth::Indexed | Depth::Colorless => color,
    }
}

/// `palette` for a terminal of sixteen colours.
///
/// Most slots take the nearest of the sixteen. The ones set by hand below are
/// where "nearest" keeps the shade and loses the meaning, because the table
/// measures distance and knows nothing about what a colour is *for*:
///
/// * The cost scale and the status dot say green, amber, red. The dark
///   palette's pale green (114) and its darker step (71) are nearest to cyan,
///   so cheap and fresh would read as neither.
/// * A wash has to stay apart from the ground it sits on. Dark's 236 is nearest
///   to black, which on a black terminal is no selection at all; light's two
///   pinks both land on bright yellow, making a marked row and a failed call
///   one colour. Blue is the selection here because the table almost never
///   produces it (it favours cyan), so no row's own ink disappears into it.
/// * A ramp's empty slot is there to be seen, and dark's lands on black.
/// * Light's accent and label were a deep blue that the table turns to cyan,
///   and white key caps on cyan are unreadable.
fn sixteen(palette: Palette) -> Palette {
    let mut p = palette.map(|c| fold(c, Depth::Ansi16));
    p.depth = Depth::Ansi16;
    match p.variant {
        Variant::Dark => {
            p.cost_low = Color::LightGreen;
            p.dot_fresh = Color::LightGreen;
            p.dot_warm = Color::Green;
            p.selected_bg = Color::Blue;
            p.header_bg = Color::Blue;
            let heat = [
                Color::DarkGray,
                Color::Green,
                Color::LightGreen,
                Color::Yellow,
                Color::LightYellow,
                Color::LightRed,
            ];
            p.spark_cpu = heat;
            p.spark_spend = heat;
            p.spark_accent = [
                Color::DarkGray,
                Color::Blue,
                Color::LightBlue,
                Color::Cyan,
                Color::LightCyan,
            ];
            p.spark_baseline = Color::DarkGray;
        }
        Variant::Light => {
            p.accent = Color::Blue;
            p.label = Color::Blue;
            p.border_hi = Color::Blue;
            p.failed_bg = Color::LightRed;
            p.marked_bg = Color::LightMagenta;
            p.spark_accent = [
                Color::Gray,
                Color::Cyan,
                Color::Cyan,
                Color::Blue,
                Color::Blue,
            ];
        }
        Variant::Mono => {}
    }
    p
}

static PALETTE: OnceLock<Palette> = OnceLock::new();

/// The active palette. Defaults to dark if nothing selected one, so tests and
/// any path that skips [`init_from_env`] behave exactly as before.
pub fn colors() -> &'static Palette {
    PALETTE.get_or_init(|| DARK)
}

pub fn variant() -> Variant {
    colors().variant
}

/// Whether colour output is suppressed, for the places that have to swap a hue
/// for a `Modifier` rather than just picking a different colour.
pub fn no_color() -> bool {
    variant() == Variant::Mono
}

/// Whether the terminal has said it takes 24-bit colour, which is when an
/// eased colour can be written as it is rather than snapped to an index.
///
/// Asked of `$COLORTERM` alone, the one convention terminals and multiplexers
/// actually pass along. A terminal that takes truecolor without saying so gets
/// the snapped pulse, which is coarser and still correct; one that claimed it
/// wrongly would get escape codes it prints as garbage, so silence is read as no.
pub fn truecolor() -> bool {
    static TRUECOLOR: OnceLock<bool> = OnceLock::new();
    *TRUECOLOR.get_or_init(|| truecolor_from(std::env::var("COLORTERM").ok().as_deref()))
}

fn truecolor_from(colorterm: Option<&str>) -> bool {
    matches!(
        colorterm
            .map(str::trim)
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("truecolor" | "24bit")
    )
}

/// The terminal's ground, as a colour something can be eased from.
///
/// The dark palette leaves the ground `Reset` and the light one names `White`,
/// and neither has a value cctop can know. So this is the grey each palette is
/// drawn to sit on — near-black, and the cube's white.
///
/// ponytail: a terminal themed far from either (a navy ground, say) sees the
/// first and last steps of an unpainted tab's pulse start from grey rather
/// than from its own ground. Those steps are the faintest of the swing, and
/// asking the terminal for its real background (OSC 11) is a round trip at
/// startup that multiplexers answer unreliably.
pub fn ground_rgb() -> Color {
    match colors().ground {
        Color::Indexed(i) if i >= 16 => Color::Indexed(i),
        Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
        _ => match variant() {
            Variant::Light => Color::Indexed(231),
            _ => Color::Indexed(234),
        },
    }
}

/// Choose the palette from the environment. Call once, before the first draw;
/// later calls are ignored, which keeps the choice stable for a whole run.
///
/// `configured` is `theme` from `config.toml`; `$CCTOP_THEME` beats it, being
/// the more local of the two.
///
/// This may talk to the terminal — see [`query_ground`] — so it has to run
/// before anything else reads stdin or puts the terminal in raw mode, which in
/// practice means before `ratatui::init`.
pub fn init_from_env(configured: Option<&str>) {
    let _ = PALETTE.set(select(
        std::env::var("NO_COLOR").ok().as_deref(),
        std::env::var("CCTOP_THEME").ok().as_deref().or(configured),
        std::env::var("COLORFGBG").ok().as_deref(),
        detect_depth(&termprofile::Env, &std::io::stdout()),
        query_ground,
    ));
}

/// The terminal's colour depth, from the environment alone.
///
/// termprofile's tmux lookup is switched off: it runs `tmux info` to tell 256
/// colours from truecolor, which is a process spawned at every start to answer
/// a question whose answer cctop draws identically either way — and under rmux
/// `tmux` is a shim for a different multiplexer.
///
/// Sixteen colours only for a terminal *known* to be limited to them. termprofile
/// also reads a bare `TERM=xterm`, and `CI` being set, as sixteen — and PuTTY
/// and most emulators send `xterm` while showing 256 perfectly well, so taking
/// that at its word folded a working palette for the many to spare the few. A
/// terminal that really has sixteen names itself as one of [`SIXTEEN`].
fn detect_depth(
    source: &impl termprofile::EnvVarSource,
    out: &impl termprofile::IsTerminal,
) -> Depth {
    let settings = termprofile::DetectorSettings::new().enable_tmux_info(false);
    let vars = termprofile::TermVars::from_source(source, out, settings);
    match Depth::of(TermProfile::detect_with_vars(vars)) {
        Depth::Ansi16 => {
            let term = source.var("TERM").unwrap_or_default();
            match SIXTEEN.contains(&term.as_str()) {
                true => Depth::Ansi16,
                false => Depth::Indexed,
            }
        }
        depth => depth,
    }
}

/// The `TERM`s that mean sixteen colours and no more: the kernel console, the
/// serial terminals it descends from, and the entries that say so by name.
const SIXTEEN: &[&str] = &[
    "linux",
    "vt100",
    "vt102",
    "vt220",
    "ansi",
    "cons25",
    "xterm-16color",
    "xterm-color",
];

/// `NO_COLOR` wins over any theme choice — it is a request for no colour, not
/// for a different one — and a terminal that cannot show colour is the same
/// request made by the hardware. Otherwise `CCTOP_THEME` decides, and `auto`
/// (the default) asks the terminal: its own answer about its background first,
/// then `COLORFGBG`, then dark, because that is what most terminals and every
/// previous cctop release assumed.
///
/// The live answer outranks `COLORFGBG` because the variable is inherited, not
/// asked: it is whatever the terminal that started the shell set, which after
/// an ssh hop or a multiplexer reattach may be a different terminal altogether.
///
/// `ground` is a callback rather than a value so the question is only put to
/// the terminal when the answer would be used — an explicit theme, `NO_COLOR`
/// and a colourless terminal all skip it — and so tests can answer it.
/// Whatever the ground, the chosen palette is then folded to `depth`.
fn select(
    no_color: Option<&str>,
    theme: Option<&str>,
    colorfgbg: Option<&str>,
    depth: Depth,
    ground: impl FnOnce() -> Option<Ground>,
) -> Palette {
    // The convention is presence-with-a-non-empty-value.
    if no_color.is_some_and(|v| !v.is_empty()) || depth == Depth::Colorless {
        return MONO;
    }
    let palette = match theme.map(str::trim).unwrap_or("auto") {
        "light" => LIGHT,
        "dark" => DARK,
        "none" | "mono" => return MONO,
        // Anything unrecognised is treated as "auto" rather than refused: a
        // typo in an env var must not stop a monitoring tool from starting.
        _ => match ground() {
            Some(Ground::Light) => LIGHT,
            Some(Ground::Dark) => DARK,
            None => match detect_light(colorfgbg) {
                true => LIGHT,
                false => DARK,
            },
        },
    };
    match depth {
        Depth::Ansi16 => sixteen(palette),
        Depth::Indexed | Depth::Colorless => palette,
    }
}

/// How long [`query_ground`] waits for the terminal to finish answering.
///
/// A terminal that answers at all answers in one round trip, which locally is
/// well under a millisecond, so this is spent only on a slow link or on a
/// terminal that says nothing. Half a second covers an ssh hop across an ocean
/// and is still a pause nobody would call a hang.
const GROUND_TIMEOUT: Duration = Duration::from_millis(500);

/// Ask the terminal for its background colour (OSC 11) and say which side of
/// the middle it is on.
///
/// The question is followed by a request for the primary device attributes
/// (DA1), which every terminal answers — cctop's own shim included — and
/// answers in order. So the DA1 reply arriving means any OSC 11 reply has
/// already arrived, and a terminal that does not know OSC 11 costs one round
/// trip rather than the whole timeout. screen is one; so is an rmux pane with
/// no client attached, which answers DA1 alone, in a few milliseconds.
///
/// Skipped unless both stdin and stdout are the terminal: the question goes
/// out on one and the answer comes back on the other, and on a pipe there is
/// nobody to ask. Raw mode is held only for the exchange, so the reply is not
/// echoed onto the screen or held back waiting for a newline.
///
/// ponytail: a terminal that answers only after [`GROUND_TIMEOUT`] leaves its
/// late reply in the input stream, where the TUI will read it as keystrokes.
/// Waiting longer only moves that line, and it takes a terminal that answers
/// DA1 slower than half a second to reach it. Keys typed during the exchange
/// are consumed with the reply, the same price the clipboard read pays.
fn query_ground() -> Option<Ground> {
    use std::io::{IsTerminal, Write};
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return None;
    }
    if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
        return None;
    }
    crossterm::terminal::enable_raw_mode().ok()?;
    let asked = {
        let mut out = std::io::stdout();
        out.write_all(b"\x1b]11;?\x1b\\\x1b[c")
            .and_then(|()| out.flush())
            .is_ok()
    };
    let reply = match asked {
        true => read_until_attributes(GROUND_TIMEOUT),
        false => Vec::new(),
    };
    let _ = crossterm::terminal::disable_raw_mode();
    parse_ground(&reply)
}

/// Read stdin until the DA1 reply has arrived or `timeout` has passed.
///
/// Straight off the fd with `poll`, as the clipboard read does: nothing else is
/// reading stdin yet, and crossterm's parser would not surface an OSC reply as
/// an event anyway.
fn read_until_attributes(timeout: Duration) -> Vec<u8> {
    let deadline = Instant::now() + timeout;
    let mut buf = Vec::new();
    while !attributes_arrived(&buf) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let mut pfd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        // Rounded up, so the last sub-millisecond of the budget is a short
        // wait rather than a zero-timeout poll spun in a loop.
        let millis = left.as_millis().max(1) as i32;
        if unsafe { libc::poll(&mut pfd, 1, millis) } <= 0 {
            break;
        }
        let mut chunk = [0u8; 256];
        let n = unsafe { libc::read(0, chunk.as_mut_ptr().cast(), chunk.len()) };
        if n <= 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n as usize]);
    }
    buf
}

/// Whether `buf` holds a complete DA1 reply, `ESC [ ? <digits and ;> c`.
fn attributes_arrived(buf: &[u8]) -> bool {
    (0..buf.len()).any(|i| {
        let Some(rest) = buf[i..].strip_prefix(b"\x1b[?") else {
            return false;
        };
        rest.iter()
            .find(|b| !(b.is_ascii_digit() || **b == b';'))
            .is_some_and(|&b| b == b'c')
    })
}

/// The ground an OSC 11 reply describes, if `buf` holds one.
///
/// The reply is `ESC ] 11 ; rgb:R/G/B` ended by BEL or ST, each channel one to
/// four hex digits scaled to its own width — `rgb:ff/ff/ff` and
/// `rgb:ffff/ffff/ffff` are the same white. Some terminals say `rgba:` and add
/// an alpha, which says nothing about lightness and is ignored.
fn parse_ground(buf: &[u8]) -> Option<Ground> {
    let start = buf.windows(5).position(|w| w == b"\x1b]11;")? + 5;
    let body = &buf[start..];
    let end = body.iter().position(|&b| b == 0x07 || b == 0x1b)?;
    let text = std::str::from_utf8(&body[..end]).ok()?;
    let (space, channels) = text.split_once(':')?;
    if space != "rgb" && space != "rgba" {
        return None;
    }
    let channel = |hex: &str| -> Option<f64> {
        if hex.is_empty() || hex.len() > 4 {
            return None;
        }
        let value = u16::from_str_radix(hex, 16).ok()?;
        Some(f64::from(value) / f64::from((1u32 << (4 * hex.len())) - 1))
    };
    let mut channels = channels.split('/').map(channel);
    let (r, g, b) = (channels.next()??, channels.next()??, channels.next()??);
    // Relative luminance, on the channels as sent. Skipping the sRGB
    // linearisation moves the midpoint, but a terminal ground is nearly always
    // far to one side of it, and this is only asked which side.
    let luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    Some(match luminance > 0.5 {
        true => Ground::Light,
        false => Ground::Dark,
    })
}

/// `COLORFGBG` is `fg;bg` (sometimes `fg;something;bg`) in ANSI colour numbers.
/// A background of 7 or 9–15 is one of the pale ones.
fn detect_light(colorfgbg: Option<&str>) -> bool {
    let Some(bg) = colorfgbg.and_then(|v| {
        v.rsplit(';')
            .next()
            .map(str::trim)
            .and_then(|b| b.parse::<u8>().ok())
    }) else {
        return false;
    };
    bg == 7 || bg >= 9
}

/// A greyscale step, named by the index the dark theme would use (232 is near
/// black, 255 near white). The light palette mirrors the ramp so "faint ink"
/// stays faint against a pale ground instead of vanishing into it.
pub fn gray(dark_index: u8) -> Color {
    let index = match variant() {
        Variant::Dark => dark_index,
        Variant::Mono => return Color::Reset,
        Variant::Light if (232..=255).contains(&dark_index) => 232 + (255 - dark_index),
        // Outside the greyscale ramp there is nothing to mirror.
        Variant::Light => dark_index,
    };
    // Sixteen colours have three greys and black and white, so a fade keeps
    // its direction in a few coarse steps rather than in twenty-four.
    fold(Color::Indexed(index), colors().depth)
}

pub fn label() -> Style {
    Style::default()
        .fg(colors().label)
        .add_modifier(Modifier::BOLD)
}

pub fn value() -> Style {
    Style::default()
        .fg(colors().value)
        .add_modifier(Modifier::BOLD)
}

/// Secondary text. Without colour there is no dimmer grey to reach for, so the
/// dim attribute stands in for it.
pub fn dim() -> Style {
    let style = Style::default().fg(colors().dim);
    match no_color() {
        true => style.add_modifier(Modifier::DIM),
        false => style,
    }
}

pub fn title() -> Style {
    Style::default()
        .fg(colors().panel_title)
        .add_modifier(Modifier::BOLD)
}

/// The page's own ground and default ink. Dark is a no-op, so a `Clear` or an
/// unstyled span still shows the terminal; light is a pale fill, so the same
/// span is black on white instead of dark-on-dark.
pub fn canvas() -> Style {
    match colors().ground {
        Color::Reset => Style::default(),
        bg => Style::default().bg(bg).fg(colors().value),
    }
}

/// The highlighted row. A background wash normally, reverse video without
/// colour — the cursor has to be findable whatever the terminal allows.
pub fn selected() -> Style {
    match no_color() {
        true => Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
        false => Style::default()
            .bg(colors().selected_bg)
            .fg(colors().value)
            .add_modifier(Modifier::BOLD),
    }
}

/// A row marked for a batch action. Underlined without colour, so it stays
/// distinguishable from the selection's reverse video.
pub fn marked() -> Style {
    match no_color() {
        true => Style::default().add_modifier(Modifier::UNDERLINED),
        false => Style::default().bg(colors().marked_bg),
    }
}

pub fn header() -> Style {
    match no_color() {
        true => Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
        false => Style::default()
            .fg(colors().value)
            .bg(colors().header_bg)
            .add_modifier(Modifier::BOLD),
    }
}

/// Wash behind a tool call that failed.
pub fn failed() -> Style {
    match no_color() {
        true => Style::default().add_modifier(Modifier::UNDERLINED),
        false => Style::default().bg(colors().failed_bg),
    }
}

/// The lit half of a blinking attention cue, on a tab that is waiting for you.
///
/// The blink is drawn by hand rather than with `Modifier::SLOW_BLINK`, so the
/// lit phase has to be visibly different by itself. Without colour that
/// difference is reverse video, which keeps the cue working when `hue` is
/// `Reset` and a background swap would show nothing at all.
pub fn attention_lit(hue: Color) -> Style {
    match no_color() {
        true => Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
        false => Style::default()
            .bg(hue)
            .fg(colors().on_accent)
            .add_modifier(Modifier::BOLD),
    }
}

/// A footer key cap: dark text on the accent, or reverse video without colour.
pub fn key_cap() -> Style {
    match no_color() {
        true => Style::default().add_modifier(Modifier::REVERSED),
        false => Style::default()
            .fg(colors().on_accent)
            .bg(colors().accent)
            .add_modifier(Modifier::BOLD),
    }
}

/// Green under a dollar, amber under ten, red beyond.
pub fn cost_color(value: f64) -> Color {
    let c = colors();
    if value < 1.0 {
        c.cost_low
    } else if value < 10.0 {
        c.cost_mid
    } else {
        c.cost_high
    }
}

/// Hue by vendor so mixed lists stay scannable.
pub fn model_color(model: &str) -> Color {
    // The route a custom provider prefixes is not part of the model's identity —
    // `canopywave/moonshotai/kimi-k2.6` is still Kimi — so only the last segment
    // decides the hue.
    let m = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();
    if m.starts_with("claude") {
        colors().claude
    } else if m.starts_with("gpt") || m.starts_with('o') && m.len() > 1 {
        colors().openai
    } else if m.starts_with("gemini") {
        colors().gemini
    } else if m.starts_with("glm") {
        colors().glm
    } else if m.starts_with("kimi") {
        colors().kimi
    } else if m.starts_with("deepseek") {
        colors().deepseek
    } else if m.starts_with("qwen") {
        colors().qwen
    } else if m.starts_with("grok") {
        colors().grok
    } else {
        colors().dim
    }
}

/// Running sessions fade from bright to dim as they go quiet, so the eye lands
/// on genuinely active work. Log-scaled: minutes matter, weeks don't.
pub fn age_color(last_active_secs: Option<i64>, running: bool) -> Color {
    if running {
        return colors().value;
    }
    let Some(secs) = last_active_secs else {
        return colors().dimmer;
    };
    let hours = (secs.max(0) as f64) / 3600.0;
    let ratio = ((1.0 + hours).ln() / (1.0 + 720.0f64).ln()).clamp(0.0, 1.0);
    gray(255 - (ratio * 17.0).round() as u8)
}

/// How a running session's dot reads: bright green when freshly active, fading
/// toward grey as it goes quiet. The shape already carries running-vs-stopped,
/// so hue may dim without the dot disappearing into the background.
pub fn running_dot_color(age_secs: Option<i64>) -> Color {
    let c = colors();
    match age_secs {
        None => c.dot_fresh,
        Some(s) if s < 30 => c.dot_fresh,
        Some(s) if s < 300 => c.cost_low,
        Some(s) if s < 1_800 => c.dot_warm,
        Some(s) if s < 7_200 => c.dim,
        _ => c.dimmer,
    }
}

/// What a session's own hooks last said about it, coloured like the cost scale:
/// the thing blocking an agent is the loudest, a finished turn is the middle,
/// and work in progress is calm.
pub fn signal_color(signal: crate::hook::Signal) -> Color {
    use crate::hook::Signal;
    match signal {
        Signal::NeedsInput => colors().cost_high,
        Signal::Idle => colors().cost_mid,
        Signal::Busy | Signal::Acting | Signal::Started | Signal::Compacting => colors().cost_low,
        Signal::Ended => colors().dimmer,
    }
}

pub fn cpu_color(cpu: f32) -> Color {
    let c = colors();
    if cpu > 80.0 {
        c.cost_high
    } else if cpu > 40.0 {
        c.cost_mid
    } else if cpu > 0.0 {
        c.cost_low
    } else {
        c.dim
    }
}

/// Context pressure, measured against the auto-compact threshold.
pub fn context_color(percent: f64) -> Color {
    let c = colors();
    if percent > 85.0 {
        c.cost_high
    } else if percent > 65.0 {
        c.cost_mid
    } else {
        c.cost_low
    }
}

/// Gradient for CPU-like series: green through amber to red.
pub fn spark_cpu(ratio: f64) -> Color {
    let ramp = colors().spark_cpu;
    match ratio {
        r if r <= 0.01 => ramp[0],
        r if r <= 0.30 => ramp[1],
        r if r <= 0.50 => ramp[2],
        r if r <= 0.70 => ramp[3],
        r if r <= 0.85 => ramp[4],
        _ => ramp[5],
    }
}

/// Gradient for spend series, same shape as CPU but tuned a shade cooler.
pub fn spark_spend(ratio: f64) -> Color {
    let ramp = colors().spark_spend;
    match ratio {
        r if r <= 0.01 => ramp[0],
        r if r <= 0.25 => ramp[1],
        r if r <= 0.50 => ramp[2],
        r if r <= 0.75 => ramp[3],
        r if r <= 0.85 => ramp[4],
        _ => ramp[5],
    }
}

/// Blue-only gradient for series where "high" isn't bad (memory, tokens).
pub fn spark_accent(ratio: f64) -> Color {
    let ramp = colors().spark_accent;
    match ratio {
        r if r <= 0.01 => ramp[0],
        r if r <= 0.25 => ramp[1],
        r if r <= 0.50 => ramp[2],
        r if r <= 0.75 => ramp[3],
        _ => ramp[4],
    }
}

/// Stable per-tool colour so the same tool keeps its hue between refreshes.
pub fn tool_color(name: &str) -> Color {
    if no_color() {
        return Color::Reset;
    }
    let hues = colors().tool_hues;
    let hash = name
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    fold(
        Color::Indexed(hues[(hash as usize) % hues.len()]),
        colors().depth,
    )
}

/// The colours a workspace tab can be painted, in the order the picker walks
/// them.
///
/// A closed set on purpose: the choice is written onto the rmux session by
/// name — see [`crate::rmux::set_color`] — so every cctop that shows the tab
/// agrees on what the name means, and a word written by a newer cctop decodes
/// to nothing rather than to a colour nobody picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hue {
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Violet,
    Pink,
}

impl Hue {
    /// Every hue, in picker order. `None` is the picker's other stop — the tab
    /// handed back to the default ink — and lives at index zero beside it.
    pub const ALL: [Hue; 8] = [
        Hue::Red,
        Hue::Orange,
        Hue::Yellow,
        Hue::Green,
        Hue::Cyan,
        Hue::Blue,
        Hue::Violet,
        Hue::Pink,
    ];

    /// How the choice is spelled on the rmux session.
    pub fn name(self) -> &'static str {
        match self {
            Hue::Red => "red",
            Hue::Orange => "orange",
            Hue::Yellow => "yellow",
            Hue::Green => "green",
            Hue::Cyan => "cyan",
            Hue::Blue => "blue",
            Hue::Violet => "violet",
            Hue::Pink => "pink",
        }
    }

    /// The hue a session records, or `None` for a word this cctop does not
    /// know — including the empty string an unset or cleared option reads as.
    pub fn from_name(name: &str) -> Option<Hue> {
        Hue::ALL.into_iter().find(|hue| hue.name() == name)
    }

    /// The hue resolved for the active palette. Dark wants pastels, light wants
    /// ink; in mono there is no colour to give, so `Reset` leaves the choice
    /// recorded and simply undrawn.
    pub fn color(self) -> Color {
        let (dark, light) = match self {
            Hue::Red => (203, 124),
            Hue::Orange => (215, 166),
            Hue::Yellow => (221, 136),
            Hue::Green => (114, 28),
            Hue::Cyan => (80, 30),
            Hue::Blue => (110, 25),
            Hue::Violet => (141, 91),
            Hue::Pink => (213, 162),
        };
        match (variant(), colors().depth) {
            (Variant::Mono, _) => Color::Reset,
            (Variant::Dark, Depth::Ansi16) => self.ansi(),
            // ponytail: as ink on a pale ground the two bright entries are too
            // faint to read, so they give way to their darker neighbours and
            // Yellow draws as Orange, Pink as Violet. The tab *fills* keep all
            // eight apart — they carry their own ink — and so does the name
            // stored on the session.
            (Variant::Light, Depth::Ansi16) => match self.ansi() {
                Color::LightYellow => Color::Yellow,
                Color::LightMagenta => Color::Magenta,
                other => other,
            },
            (Variant::Dark, _) => Color::Indexed(dark),
            (Variant::Light, _) => Color::Indexed(light),
        }
    }

    /// The hue among the sixteen ANSI colours, chosen by hand: nearest-colour
    /// folding sends five of the eight to cyan or yellow, which would leave a
    /// sixteen-colour picker offering the same colour under several names.
    /// Orange takes the dark yellow, which most schemes draw brown or orange.
    fn ansi(self) -> Color {
        match self {
            Hue::Red => Color::Red,
            Hue::Orange => Color::Yellow,
            Hue::Yellow => Color::LightYellow,
            Hue::Green => Color::Green,
            Hue::Cyan => Color::Cyan,
            Hue::Blue => Color::Blue,
            Hue::Violet => Color::Magenta,
            Hue::Pink => Color::LightMagenta,
        }
    }

    /// The text that reads on [`Hue::ansi`] as a fill. Per hue rather than per
    /// strength, unlike [`Fill::ink`]: the sixteen are not at one lightness,
    /// and white on bright yellow is as unreadable as black on blue.
    fn ansi_ink(self) -> Color {
        match self {
            Hue::Red | Hue::Blue | Hue::Violet => Color::White,
            _ => Color::Black,
        }
    }

    /// The hue as the fill of a whole tab, at one of three strengths.
    ///
    /// [`Hue::color`] is ink — right for a label or a border, and loud as a
    /// block of background: a bar of saturated tabs outshouts everything
    /// under it. So a tab rests in a pastel, the one being watched is a step
    /// stronger, and only a tab that needs you reaches the full colour — the
    /// same hue at every step, so it is still *that* tab while it shouts.
    ///
    /// Which pastel depends on the ground, because a quiet tab is one that
    /// sits close to it. On dark it is a dark pastel: the pale row of the cube
    /// (`ffd7d7` and its neighbours) is pastel on a chart and a lit block on a
    /// black screen, and even its dusty middle (`d78787`) stood out more than a
    /// tab's colour should at rest. On light that pale row is the quiet one and
    /// the dark set is the slab, so light gets the pale row, and "stronger"
    /// there is a step deeper rather than lighter. The text that goes on each
    /// is [`Fill::ink`]'s business; [`Hue::wash`] hands over the two together.
    ///
    /// With sixteen colours there is one fill per hue, not three: see
    /// [`Hue::wash`] for how the strengths stay apart.
    pub fn fill(self, strength: Fill) -> Color {
        match (variant(), colors().depth) {
            (Variant::Mono, _) => Color::Reset,
            (_, Depth::Ansi16) => self.ansi(),
            (variant, _) => self.fill_in(strength, variant),
        }
    }

    /// [`Hue::fill`] for a palette named outright, so the light set can be
    /// checked without the process-wide choice being light.
    fn fill_in(self, strength: Fill, variant: Variant) -> Color {
        let (rest, selected, alert) = match variant {
            Variant::Mono => return Color::Reset,
            Variant::Dark => match self {
                Hue::Red => (95, 131, 203),
                Hue::Orange => (137, 173, 208),
                Hue::Yellow => (101, 143, 220),
                Hue::Green => (65, 71, 77),
                Hue::Cyan => (66, 73, 44),
                Hue::Blue => (67, 68, 39),
                Hue::Violet => (97, 134, 135),
                Hue::Pink => (132, 168, 205),
            },
            Variant::Light => match self {
                Hue::Red => (224, 217, 203),
                Hue::Orange => (223, 216, 208),
                Hue::Yellow => (230, 229, 220),
                Hue::Green => (194, 157, 120),
                Hue::Cyan => (195, 159, 87),
                Hue::Blue => (153, 117, 75),
                Hue::Violet => (183, 177, 135),
                Hue::Pink => (225, 218, 205),
            },
        };
        Color::Indexed(match strength {
            Fill::Rest => rest,
            Fill::Selected => selected,
            Fill::Alert => alert,
        })
    }

    /// A tab's fill with the text that reads on it. The pair rather than two
    /// lookups, so no caller can put one strength's ink on another's ground.
    ///
    /// Sixteen colours have no quieter and louder shade of each hue to spend,
    /// so there the strengths are said another way. Rest and Selected share a
    /// fill, which the caller already tells apart by underlining the watched
    /// tab; Alert is the fill in reverse video, the hue as ink on the ground,
    /// so the blink still flips between two looks every half-second.
    pub fn wash(self, strength: Fill) -> Style {
        match variant() != Variant::Mono && colors().depth == Depth::Ansi16 {
            true => self.ansi_wash(strength),
            false => Style::default().bg(self.fill(strength)).fg(strength.ink()),
        }
    }

    /// [`Hue::wash`] on a sixteen-colour terminal, whichever the ground.
    fn ansi_wash(self, strength: Fill) -> Style {
        let wash = Style::default().bg(self.ansi()).fg(self.ansi_ink());
        match strength {
            Fill::Alert => wash.add_modifier(Modifier::REVERSED),
            Fill::Rest | Fill::Selected => wash,
        }
    }
}

/// How strongly a painted tab wears its hue. See [`Hue::fill`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fill {
    /// Any tab, at rest.
    Rest,
    /// The tab being watched.
    Selected,
    /// The lit half of a blink: the tab's agent needs you.
    Alert,
}

impl Fill {
    /// The text colour that reads on a fill of this strength, on the active
    /// palette.
    ///
    /// Keyed on the strength and not the hue: at one strength every hue's fill
    /// sits at about the same lightness, which is what lets one ink serve all
    /// eight — and a tab that changed text colour with its hue would look like
    /// it was saying something by it.
    pub fn ink(self) -> Color {
        fold(self.ink_in(variant()), colors().depth)
    }

    /// [`Fill::ink`] for a palette named outright.
    fn ink_in(self, variant: Variant) -> Color {
        match (variant, self) {
            // The dark set's quiet strengths are dark enough to want light
            // text; its alert strength is the full hue, which wants dark.
            (Variant::Dark, Fill::Rest | Fill::Selected) => Color::Indexed(254),
            (Variant::Dark, Fill::Alert) => Color::Black,
            // Every step of the light set is pale, even the loud one.
            (Variant::Light, _) => Color::Black,
            (Variant::Mono, _) => Color::Reset,
        }
    }
}

/// Which gradient a series should use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gradient {
    Cpu,
    Spend,
    Accent,
}

impl Gradient {
    pub fn color(&self, ratio: f64) -> Color {
        match self {
            Gradient::Cpu => spark_cpu(ratio),
            Gradient::Spend => spark_spend(ratio),
            Gradient::Accent => spark_accent(ratio),
        }
    }

    /// Colour for an empty slot, so the baseline stays visible.
    pub fn baseline(&self) -> Color {
        colors().spark_baseline
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// [`select`] on a 256-colour terminal that did not answer the background
    /// question — the situation every release before the question existed.
    fn plain(no_color: Option<&str>, theme: Option<&str>, colorfgbg: Option<&str>) -> Palette {
        select(no_color, theme, colorfgbg, Depth::Indexed, || None)
    }

    /// A `ground` callback that fails the test if it is called: the question
    /// goes to a real terminal, so asking it when the answer is unused would
    /// be a pause and a raw-mode flip for nothing.
    fn unasked() -> Option<Ground> {
        panic!("the terminal was asked for its ground when the answer was not needed")
    }

    #[test]
    fn cost_thresholds() {
        assert_eq!(cost_color(0.5), colors().cost_low);
        assert_eq!(cost_color(5.0), colors().cost_mid);
        assert_eq!(cost_color(50.0), colors().cost_high);
    }

    #[test]
    fn age_fades_monotonically() {
        let fresh = age_color(Some(0), false);
        let old = age_color(Some(30 * 86_400), false);
        let (Color::Indexed(a), Color::Indexed(b)) = (fresh, old) else {
            panic!("expected indexed colors");
        };
        assert!(a > b, "older sessions must render dimmer ({a} vs {b})");
        assert_eq!(age_color(Some(99_999), true), Color::White);
    }

    #[test]
    fn context_color_escalates() {
        assert_eq!(context_color(10.0), colors().cost_low);
        assert_eq!(context_color(70.0), colors().cost_mid);
        assert_eq!(context_color(95.0), colors().cost_high);
    }

    /// The default has to stay exactly what it was, env or no env.
    #[test]
    fn dark_is_the_default() {
        assert_eq!(colors().variant, Variant::Dark);
        assert_eq!(plain(None, None, None).variant, Variant::Dark);
        assert_eq!(plain(None, Some("auto"), None).variant, Variant::Dark);
    }

    /// Every colour these constants used to hold, written out again. This is a
    /// transcription of the pre-palette module: if a value here has to change
    /// to make a test pass, the dark theme has silently moved.
    #[test]
    fn the_dark_palette_still_matches_the_constants_it_replaced() {
        let c = DARK;
        for (got, want) in [
            (c.border, Color::Indexed(60)),
            (c.border_hi, Color::Indexed(75)),
            (c.panel_title, Color::Indexed(179)),
            (c.label, Color::Indexed(75)),
            (c.value, Color::White),
            (c.dim, Color::Indexed(245)),
            (c.dimmer, Color::Indexed(238)),
            (c.accent, Color::Indexed(75)),
            (c.cost_low, Color::Indexed(114)),
            (c.cost_mid, Color::Indexed(221)),
            (c.cost_high, Color::Indexed(203)),
            (c.claude, Color::Indexed(173)),
            (c.openai, Color::Indexed(110)),
            (c.cursor, Color::Indexed(141)),
            (c.opencode, Color::Indexed(117)),
            (c.pi, Color::Indexed(150)),
            (c.gemini, Color::Indexed(74)),
            (c.windsurf, Color::Indexed(80)),
            (c.desktop_code, Color::Indexed(141)),
            (c.desktop_cowork, Color::Indexed(183)),
            (c.selected_bg, Color::Indexed(236)),
            (c.failed_bg, Color::Indexed(52)),
            (c.header_bg, Color::Indexed(236)),
            (c.marked_bg, Color::Indexed(53)),
            (c.spark_baseline, Color::Indexed(236)),
        ] {
            assert_eq!(got, want, "a dark colour moved");
        }
        // The ramps, whose old bodies were literal match arms.
        assert_eq!(spark_cpu(0.0), Color::Indexed(236));
        assert_eq!(spark_cpu(0.4), Color::Indexed(114));
        assert_eq!(spark_cpu(1.0), Color::Indexed(203));
        assert_eq!(spark_accent(0.6), Color::Indexed(75));
        assert_eq!(running_dot_color(Some(0)), Color::Indexed(82));
        assert_eq!(running_dot_color(Some(1_000)), Color::Indexed(71));
        // Same hash over the same ten hues, so a tool keeps the colour it had.
        assert_eq!(tool_color("Bash"), Color::Indexed(173));
    }

    /// Only a terminal that says so gets RGB; anything else — including the
    /// `256color` a `TERM` might put there — is snapped to an index.
    #[test]
    fn truecolor_is_only_what_colorterm_claims() {
        assert!(truecolor_from(Some("truecolor")));
        assert!(truecolor_from(Some("24bit")));
        assert!(truecolor_from(Some(" TrueColor ")));
        assert!(!truecolor_from(None));
        assert!(!truecolor_from(Some("")));
        assert!(!truecolor_from(Some("256color")));
    }

    #[test]
    fn no_color_beats_an_explicit_theme() {
        assert_eq!(plain(Some("1"), Some("light"), None).variant, Variant::Mono);
        // Set but empty is not set, per the NO_COLOR convention.
        assert_eq!(plain(Some(""), Some("light"), None).variant, Variant::Light);
    }

    #[test]
    fn theme_env_selects_and_tolerates_nonsense() {
        assert_eq!(plain(None, Some("light"), None).variant, Variant::Light);
        assert_eq!(plain(None, Some(" dark "), None).variant, Variant::Dark);
        // A typo falls back to auto-detection rather than failing.
        assert_eq!(plain(None, Some("lihgt"), None).variant, Variant::Dark);
        assert_eq!(
            plain(None, Some("lihgt"), Some("0;15")).variant,
            Variant::Light
        );
    }

    #[test]
    fn colorfgbg_reports_the_background() {
        assert!(detect_light(Some("0;15")));
        assert!(detect_light(Some("0;default;7")));
        assert!(!detect_light(Some("15;0")));
        assert!(!detect_light(Some("15;8")));
        assert!(!detect_light(Some("nonsense")));
        assert!(!detect_light(None));
    }

    /// Every palette must define every slot; a `Color::Reset` outside Mono
    /// would be an unstyled hole on screen.
    #[test]
    fn only_mono_uses_reset() {
        for p in [DARK, LIGHT] {
            for c in [
                p.border,
                p.value,
                p.dim,
                p.accent,
                p.cost_low,
                p.cost_high,
                p.selected_bg,
                p.header_bg,
                p.spark_baseline,
            ] {
                assert_ne!(c, Color::Reset, "{:?} has an unset slot", p.variant);
            }
        }
        assert_eq!(MONO.value, Color::Reset);
    }

    /// The light theme mirrors the greyscale ramp instead of reusing it.
    #[test]
    fn gray_passes_through_on_dark() {
        assert_eq!(gray(240), Color::Indexed(240));
        assert_eq!(gray(120), Color::Indexed(120));
    }

    /// White ink on a pale wash is how the cursor used to vanish. The selected
    /// style has to use the palette's ink, and the wash has to sit apart from
    /// both that ink and the page.
    #[test]
    fn light_selection_is_not_white_on_white() {
        assert_eq!(LIGHT.value, Color::Black);
        assert_eq!(LIGHT.ground, Color::White);
        assert_ne!(LIGHT.selected_bg, Color::White);
        assert_ne!(LIGHT.selected_bg, LIGHT.ground);
        assert_ne!(LIGHT.selected_bg, LIGHT.value);
        assert_eq!(LIGHT.header_bg, LIGHT.selected_bg);
    }

    #[test]
    fn selected_style_uses_palette_ink() {
        let s = selected();
        assert_eq!(s.fg, Some(colors().value));
        assert_eq!(s.bg, Some(colors().selected_bg));
    }

    /// Dark must not start painting a ground. A Reset here is the whole point:
    /// the original look is the terminal's, not ours.
    #[test]
    fn dark_leaves_the_terminal_ground_alone() {
        assert_eq!(DARK.ground, Color::Reset);
        assert_eq!(canvas().bg, None);
    }

    /// The names are the wire format — a hue is written onto the rmux session
    /// by name and read back off it — so each one has to survive the round
    /// trip, and nothing else may decode: a word a newer cctop wrote is no
    /// colour here, never a guess.
    #[test]
    fn hue_names_survive_the_round_trip_through_a_session() {
        for hue in Hue::ALL {
            assert_eq!(Hue::from_name(hue.name()), Some(hue));
        }
        assert_eq!(Hue::from_name(""), None, "an unset option is no colour");
        assert_eq!(
            Hue::from_name("puce"),
            None,
            "a word this cctop does not know was guessed at"
        );
    }

    /// How light an xterm-256 colour is, for the six-level cube the fills are
    /// drawn from — enough to say which of two fills sits nearer the ground.
    fn lightness(color: Color) -> f64 {
        const LEVEL: [f64; 6] = [0.0, 95.0, 135.0, 175.0, 215.0, 255.0];
        let Color::Indexed(i @ 16..=231) = color else {
            panic!("{color:?} is not a cube colour");
        };
        let i = (i - 16) as usize;
        let (r, g, b) = (LEVEL[i / 36], LEVEL[(i / 6) % 6], LEVEL[i % 6]);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// "Stronger" means further from the ground, which is lighter on dark and
    /// deeper on light. The watched tab has to be that on both, or on one of
    /// them it is the tab you are *not* looking at that stands out. And each
    /// palette's text has to be the one that reads on it: light on the dark
    /// set's quiet steps, dark on every step of the pale set.
    #[test]
    fn the_watched_tab_stands_further_from_the_ground_on_either_palette() {
        for hue in Hue::ALL {
            let at = |fill, variant| hue.fill_in(fill, variant);
            assert!(
                lightness(at(Fill::Selected, Variant::Dark))
                    > lightness(at(Fill::Rest, Variant::Dark)),
                "{hue:?}: the watched tab is no lighter than the rest on dark"
            );
            assert!(
                lightness(at(Fill::Selected, Variant::Light))
                    < lightness(at(Fill::Rest, Variant::Light)),
                "{hue:?}: the watched tab is no deeper than the rest on light"
            );
            for variant in [Variant::Dark, Variant::Light] {
                assert_ne!(at(Fill::Alert, variant), at(Fill::Selected, variant));
            }
            assert_eq!(at(Fill::Rest, Variant::Mono), Color::Reset);
        }
        for fill in [Fill::Rest, Fill::Selected, Fill::Alert] {
            assert_eq!(fill.ink_in(Variant::Light), Color::Black);
            assert_eq!(fill.ink_in(Variant::Mono), Color::Reset);
        }
        assert_eq!(Fill::Rest.ink_in(Variant::Dark), Color::Indexed(254));
        assert_eq!(Fill::Selected.ink_in(Variant::Dark), Color::Indexed(254));
        assert_eq!(Fill::Alert.ink_in(Variant::Dark), Color::Black);
    }

    /// On the default palette every hue is a real, distinct colour: a `Reset`
    /// would paint the tab in nothing, and two hues sharing an index would be
    /// two names for the same mark.
    #[test]
    fn every_hue_is_its_own_colour() {
        let mut seen = std::collections::HashSet::new();
        for hue in Hue::ALL {
            let color = hue.color();
            assert!(
                matches!(color, Color::Indexed(_)),
                "{hue:?} resolved to no colour"
            );
            assert!(
                seen.insert(color),
                "{hue:?} shares a colour with another hue"
            );
        }
    }

    /// An explicit choice, `NO_COLOR` and a colourless terminal each settle
    /// the palette on their own, so none of them may put the question to the
    /// terminal.
    #[test]
    fn nothing_asks_the_terminal_when_the_answer_is_already_known() {
        let pick = |no_color, theme, depth| select(no_color, theme, None, depth, unasked).variant;
        assert_eq!(pick(Some("1"), None, Depth::Indexed), Variant::Mono);
        assert_eq!(
            pick(Some("1"), Some("light"), Depth::Indexed),
            Variant::Mono
        );
        assert_eq!(pick(None, Some("light"), Depth::Indexed), Variant::Light);
        assert_eq!(pick(None, Some("dark"), Depth::Indexed), Variant::Dark);
        assert_eq!(pick(None, Some("mono"), Depth::Indexed), Variant::Mono);
        assert_eq!(pick(None, Some("light"), Depth::Ansi16), Variant::Light);
        // A terminal that cannot show colour outranks a theme the way NO_COLOR
        // does: a light palette it cannot draw is not a light palette.
        assert_eq!(pick(None, None, Depth::Colorless), Variant::Mono);
        assert_eq!(pick(None, Some("light"), Depth::Colorless), Variant::Mono);
    }

    /// `auto` takes the terminal's own answer first, `COLORFGBG` only when
    /// there was none, and dark when there was neither.
    #[test]
    fn auto_follows_the_terminal_then_colorfgbg_then_dark() {
        let pick = |theme, colorfgbg, ground: Option<Ground>| {
            select(None, theme, colorfgbg, Depth::Indexed, || ground).variant
        };
        assert_eq!(pick(None, None, Some(Ground::Light)), Variant::Light);
        assert_eq!(pick(Some("auto"), None, Some(Ground::Dark)), Variant::Dark);
        // The live answer beats an inherited variable that says otherwise —
        // both ways round.
        assert_eq!(pick(None, Some("0;15"), Some(Ground::Dark)), Variant::Dark);
        assert_eq!(
            pick(None, Some("15;0"), Some(Ground::Light)),
            Variant::Light
        );
        assert_eq!(pick(None, Some("0;15"), None), Variant::Light);
        assert_eq!(pick(None, Some("15;0"), None), Variant::Dark);
        assert_eq!(pick(None, None, None), Variant::Dark);
        // A typo is auto, and auto asks.
        assert_eq!(
            pick(Some("lihgt"), None, Some(Ground::Light)),
            Variant::Light
        );
    }

    /// A 256-colour terminal gets the palettes exactly as written: detection
    /// must not move the default look by one index.
    #[test]
    fn a_256_colour_terminal_gets_the_palette_unfolded() {
        let dark = select(None, None, None, Depth::Indexed, || None);
        assert_eq!(dark.depth, Depth::Indexed);
        assert_eq!(dark.cost_low, Color::Indexed(114));
        assert_eq!(dark.selected_bg, Color::Indexed(236));
        let light = select(None, Some("light"), None, Depth::Indexed, unasked);
        assert_eq!(light.marked_bg, LIGHT.marked_bg);
    }

    /// Every slot of a sixteen-colour palette is one of the sixteen, and the
    /// hand-set ones still mean what they meant: green/amber/red in that order,
    /// a selection that is not the ground, marks and failures kept apart.
    #[test]
    fn sixteen_colours_fold_every_slot_and_keep_the_meanings() {
        for theme in ["dark", "light"] {
            let p = select(None, Some(theme), None, Depth::Ansi16, unasked);
            assert_eq!(p.depth, Depth::Ansi16);
            p.map(|c| {
                assert!(
                    !matches!(c, Color::Indexed(_) | Color::Rgb(..)),
                    "{theme}: {c:?} is outside the sixteen"
                );
                c
            });
            let scale = [p.cost_low, p.cost_mid, p.cost_high];
            assert!(
                scale[0] != scale[1] && scale[1] != scale[2] && scale[0] != scale[2],
                "{theme}: the cost scale collapsed: {scale:?}"
            );
            assert!(matches!(p.cost_low, Color::Green | Color::LightGreen));
            assert!(matches!(p.cost_high, Color::Red | Color::LightRed));
            assert_ne!(p.marked_bg, p.failed_bg, "{theme}");
            assert_ne!(p.spark_baseline, p.ground, "{theme}");
        }
        let dark = select(None, None, None, Depth::Ansi16, || None);
        assert_eq!(dark.variant, Variant::Dark);
        assert!(!matches!(dark.selected_bg, Color::Black | Color::Reset));
        assert_ne!(dark.spark_baseline, Color::Black);
        let light = select(None, None, None, Depth::Ansi16, || Some(Ground::Light));
        assert_eq!(light.variant, Variant::Light);
        assert_ne!(light.selected_bg, light.ground);
        assert_ne!(light.accent, light.on_accent);
    }

    /// With one fill per hue, the eight tabs still have to be eight colours,
    /// and the blink still has to flip between two looks.
    #[test]
    fn sixteen_colour_tabs_stay_apart_and_still_blink() {
        let mut seen = std::collections::HashSet::new();
        for hue in Hue::ALL {
            assert!(seen.insert(hue.ansi()), "{hue:?} shares a fill");
            let rest = hue.ansi_wash(Fill::Rest);
            assert_ne!(rest, hue.ansi_wash(Fill::Alert), "{hue:?} does not blink");
            assert_ne!(rest.bg, rest.fg, "{hue:?} is unreadable");
        }
    }

    /// A stand-in for stdout whose terminal-ness the test decides.
    struct Out(bool);

    impl termprofile::IsTerminal for Out {
        fn is_terminal(&self) -> bool {
            self.0
        }
    }

    /// termprofile's reading of the environment, through [`Depth::of`]. The
    /// variables come from a map, so the test's own terminal and CI's
    /// variables do not leak in.
    #[test]
    fn depth_is_read_from_the_environment() {
        let depth = |vars: &[(&str, &str)], tty: bool| {
            detect_depth(&vars.iter().copied().collect::<HashMap<_, _>>(), &Out(tty))
        };
        assert_eq!(depth(&[("TERM", "xterm-256color")], true), Depth::Indexed);
        assert_eq!(
            depth(&[("TERM", "xterm"), ("COLORTERM", "truecolor")], true),
            Depth::Indexed
        );
        // Inside rmux or tmux, with the lookup that would spawn `tmux info`
        // switched off.
        assert_eq!(
            depth(
                &[
                    ("TERM", "tmux-256color"),
                    ("TMUX", "/tmp/rmux-1000/default,1,0")
                ],
                true
            ),
            Depth::Indexed
        );
        assert_eq!(depth(&[("TERM", "linux")], true), Depth::Ansi16);
        // What PuTTY and most emulators send, while showing 256.
        assert_eq!(depth(&[("TERM", "xterm")], true), Depth::Indexed);
        assert_eq!(
            depth(&[("TERM", "xterm-256color"), ("CI", "true")], true),
            Depth::Indexed
        );
        assert_eq!(
            depth(&[("TERM", "xterm"), ("FORCE_COLOR", "ansi256")], true),
            Depth::Indexed
        );
        assert_eq!(
            depth(&[("TERM", "xterm-256color"), ("NO_COLOR", "1")], true),
            Depth::Colorless
        );
        // Not a terminal: no signal, so the palette is drawn as it always was.
        assert_eq!(depth(&[("TERM", "linux")], false), Depth::Indexed);
    }

    #[test]
    fn an_osc_11_reply_says_which_side_of_the_middle_the_ground_is() {
        let ground = |reply: &[u8]| parse_ground(reply);
        assert_eq!(
            ground(b"\x1b]11;rgb:ffff/ffff/ffff\x1b\\"),
            Some(Ground::Light)
        );
        assert_eq!(
            ground(b"\x1b]11;rgb:1e1e/1e1e/2e2e\x07"),
            Some(Ground::Dark)
        );
        // Two digits a channel — Solarized Light's base3 — scale the same way.
        assert_eq!(ground(b"\x1b]11;rgb:fd/f6/e3\x07"), Some(Ground::Light));
        assert_eq!(
            ground(b"\x1b]11;rgba:0000/0000/0000/ffff\x1b\\"),
            Some(Ground::Dark)
        );
        // A keystroke ahead of it and the DA1 reply behind it, as it arrives.
        assert_eq!(
            ground(b"j\x1b]11;rgb:f0f0/f0f0/f0f0\x1b\\\x1b[?62;22c"),
            Some(Ground::Light)
        );
        // A terminal that answered only DA1, a reply cut off by the timeout,
        // and one in a colour space this does not read.
        assert_eq!(ground(b"\x1b[?62;22c"), None);
        assert_eq!(ground(b"\x1b]11;rgb:ffff/ff"), None);
        assert_eq!(ground(b"\x1b]11;#ffffff\x07"), None);
        assert_eq!(ground(b"\x1b]11;rgb:fffff/0/0\x07"), None);
        assert_eq!(ground(b""), None);
    }

    /// The DA1 reply is the end of the exchange: it has to be recognised whole,
    /// and not mistaken for a prefix of itself.
    #[test]
    fn the_device_attributes_reply_ends_the_exchange() {
        assert!(attributes_arrived(b"\x1b[?62;22c"));
        assert!(attributes_arrived(b"\x1b]11;rgb:0/0/0\x1b\\\x1b[?1;2c"));
        assert!(!attributes_arrived(b"\x1b[?62;22"));
        assert!(!attributes_arrived(b"\x1b]11;rgb:0/0/0\x1b\\"));
        assert!(!attributes_arrived(b"\x1b[?2004h"));
        assert!(!attributes_arrived(b""));
    }
}
