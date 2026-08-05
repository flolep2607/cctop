//! Colour palette. btop-inspired: dark ground, few saturated accents, and
//! meaning carried by hue (green = idle/cheap, amber = warming, red = hot).

use ratatui::style::{Color, Modifier, Style};

pub const BORDER: Color = Color::Indexed(60);
pub const BORDER_HI: Color = Color::Indexed(75);
pub const PANEL_TITLE: Color = Color::Indexed(179);

pub const LABEL: Color = Color::Indexed(75);
pub const VALUE: Color = Color::White;
pub const DIM: Color = Color::Indexed(245);
pub const DIMMER: Color = Color::Indexed(238);
pub const ACCENT: Color = Color::Indexed(75);

pub const COST_LOW: Color = Color::Indexed(114);
pub const COST_MID: Color = Color::Indexed(221);
pub const COST_HIGH: Color = Color::Indexed(203);

pub const CLAUDE: Color = Color::Indexed(173);
pub const OPENAI: Color = Color::Indexed(110);
pub const CURSOR: Color = Color::Indexed(141);
pub const OPENCODE: Color = Color::Indexed(117);
pub const PI: Color = Color::Indexed(150);
pub const DESKTOP_CODE: Color = Color::Indexed(141);
pub const DESKTOP_COWORK: Color = Color::Indexed(183);

pub const SELECTED_BG: Color = Color::Indexed(236);
pub const HEADER_BG: Color = Color::Indexed(236);
/// Tint for rows the user has marked for a batch action, so marks read at a
/// glance even when nothing is selected. Kept dim so it stays behind the data.
pub const MARKED_BG: Color = Color::Indexed(53);

pub fn label() -> Style {
    Style::default().fg(LABEL).add_modifier(Modifier::BOLD)
}

pub fn value() -> Style {
    Style::default().fg(VALUE).add_modifier(Modifier::BOLD)
}

pub fn dim() -> Style {
    Style::default().fg(DIM)
}

pub fn title() -> Style {
    Style::default()
        .fg(PANEL_TITLE)
        .add_modifier(Modifier::BOLD)
}

/// Green under a dollar, amber under ten, red beyond.
pub fn cost_color(value: f64) -> Color {
    if value < 1.0 {
        COST_LOW
    } else if value < 10.0 {
        COST_MID
    } else {
        COST_HIGH
    }
}

/// Hue by vendor so mixed lists stay scannable.
pub fn model_color(model: &str) -> Color {
    let m = model.to_ascii_lowercase();
    if m.starts_with("claude") {
        CLAUDE
    } else if m.starts_with("gpt") || m.starts_with('o') && m.len() > 1 {
        OPENAI
    } else {
        DIM
    }
}

/// Running sessions fade from bright to dim as they go quiet, so the eye lands
/// on genuinely active work. Log-scaled: minutes matter, weeks don't.
pub fn age_color(last_active_secs: Option<i64>, running: bool) -> Color {
    if running {
        return Color::White;
    }
    let Some(secs) = last_active_secs else {
        return DIMMER;
    };
    let hours = (secs.max(0) as f64) / 3600.0;
    let ratio = ((1.0 + hours).ln() / (1.0 + 720.0f64).ln()).clamp(0.0, 1.0);
    Color::Indexed(255 - (ratio * 17.0).round() as u8)
}

/// How a running session's dot reads: bright green when freshly active, fading
/// toward grey as it goes quiet. The shape already carries running-vs-stopped,
/// so hue may dim without the dot disappearing into the background.
pub fn running_dot_color(age_secs: Option<i64>) -> Color {
    match age_secs {
        None => Color::Indexed(82),
        Some(s) if s < 30 => Color::Indexed(82),
        Some(s) if s < 300 => COST_LOW,
        Some(s) if s < 1_800 => Color::Indexed(71),
        Some(s) if s < 7_200 => DIM,
        _ => DIMMER,
    }
}

pub fn cpu_color(cpu: f32) -> Color {
    if cpu > 80.0 {
        COST_HIGH
    } else if cpu > 40.0 {
        COST_MID
    } else if cpu > 0.0 {
        COST_LOW
    } else {
        DIM
    }
}

/// Context pressure, measured against the auto-compact threshold.
pub fn context_color(percent: f64) -> Color {
    if percent > 85.0 {
        COST_HIGH
    } else if percent > 65.0 {
        COST_MID
    } else {
        COST_LOW
    }
}

/// Gradient for CPU-like series: green through amber to red.
pub fn spark_cpu(ratio: f64) -> Color {
    match ratio {
        r if r <= 0.01 => Color::Indexed(236),
        r if r <= 0.30 => Color::Indexed(71),
        r if r <= 0.50 => Color::Indexed(114),
        r if r <= 0.70 => Color::Indexed(186),
        r if r <= 0.85 => Color::Indexed(221),
        _ => Color::Indexed(203),
    }
}

/// Gradient for spend series, same shape as CPU but tuned a shade cooler.
pub fn spark_spend(ratio: f64) -> Color {
    match ratio {
        r if r <= 0.01 => Color::Indexed(236),
        r if r <= 0.25 => Color::Indexed(71),
        r if r <= 0.50 => Color::Indexed(114),
        r if r <= 0.75 => Color::Indexed(186),
        r if r <= 0.85 => Color::Indexed(221),
        _ => Color::Indexed(203),
    }
}

/// Blue-only gradient for series where "high" isn't bad (memory, tokens).
pub fn spark_accent(ratio: f64) -> Color {
    match ratio {
        r if r <= 0.01 => Color::Indexed(238),
        r if r <= 0.25 => Color::Indexed(60),
        r if r <= 0.50 => Color::Indexed(68),
        r if r <= 0.75 => Color::Indexed(75),
        _ => Color::Indexed(117),
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
        Color::Indexed(236)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_thresholds() {
        assert_eq!(cost_color(0.5), COST_LOW);
        assert_eq!(cost_color(5.0), COST_MID);
        assert_eq!(cost_color(50.0), COST_HIGH);
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
        assert_eq!(context_color(10.0), COST_LOW);
        assert_eq!(context_color(70.0), COST_MID);
        assert_eq!(context_color(95.0), COST_HIGH);
    }
}
