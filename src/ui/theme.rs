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

use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Dark,
    Light,
    /// `NO_COLOR`: shape and weight only.
    Mono,
}

/// Every colour the UI can ask for, resolved for one [`Variant`].
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub variant: Variant,

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
    border: Color::Indexed(146),
    border_hi: Color::Indexed(25),
    panel_title: Color::Indexed(94),
    label: Color::Indexed(25),
    value: Color::Black,
    dim: Color::Indexed(243),
    dimmer: Color::Indexed(249),
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
    selected_bg: Color::Indexed(253),
    failed_bg: Color::Indexed(224),
    header_bg: Color::Indexed(253),
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

/// Choose the palette from the environment. Call once, before the first draw;
/// later calls are ignored, which keeps the choice stable for a whole run.
pub fn init_from_env() {
    let _ = PALETTE.set(select(
        std::env::var("NO_COLOR").ok().as_deref(),
        std::env::var("CCTOP_THEME").ok().as_deref(),
        std::env::var("COLORFGBG").ok().as_deref(),
    ));
}

/// `NO_COLOR` wins over any theme choice — it is a request for no colour, not
/// for a different one. Otherwise `CCTOP_THEME` decides, and `auto` (the
/// default) asks the terminal via `COLORFGBG`, falling back to dark because
/// that is what most terminals and every previous cctop release assumed.
fn select(no_color: Option<&str>, theme: Option<&str>, colorfgbg: Option<&str>) -> Palette {
    // The convention is presence-with-a-non-empty-value.
    if no_color.is_some_and(|v| !v.is_empty()) {
        return MONO;
    }
    match theme.map(str::trim).unwrap_or("auto") {
        "light" => LIGHT,
        "dark" => DARK,
        "none" | "mono" => MONO,
        // Anything unrecognised is treated as "auto" rather than refused: a
        // typo in an env var must not stop a monitoring tool from starting.
        _ => match detect_light(colorfgbg) {
            true => LIGHT,
            false => DARK,
        },
    }
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
    match variant() {
        Variant::Dark => Color::Indexed(dark_index),
        Variant::Mono => Color::Reset,
        Variant::Light if (232..=255).contains(&dark_index) => {
            Color::Indexed(232 + (255 - dark_index))
        }
        // Outside the greyscale ramp there is nothing to mirror.
        Variant::Light => Color::Indexed(dark_index),
    }
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
    Color::Indexed(hues[(hash as usize) % hues.len()])
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
        assert_eq!(select(None, None, None).variant, Variant::Dark);
        assert_eq!(select(None, Some("auto"), None).variant, Variant::Dark);
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

    #[test]
    fn no_color_beats_an_explicit_theme() {
        assert_eq!(
            select(Some("1"), Some("light"), None).variant,
            Variant::Mono
        );
        // Set but empty is not set, per the NO_COLOR convention.
        assert_eq!(
            select(Some(""), Some("light"), None).variant,
            Variant::Light
        );
    }

    #[test]
    fn theme_env_selects_and_tolerates_nonsense() {
        assert_eq!(select(None, Some("light"), None).variant, Variant::Light);
        assert_eq!(select(None, Some(" dark "), None).variant, Variant::Dark);
        // A typo falls back to auto-detection rather than failing.
        assert_eq!(select(None, Some("lihgt"), None).variant, Variant::Dark);
        assert_eq!(
            select(None, Some("lihgt"), Some("0;15")).variant,
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
}
