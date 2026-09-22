//! What a user can tune, read from `[settings]` and `[keys]` in cctop's
//! `config.toml`.
//!
//! One file rather than a second one beside it: `config.toml` already exists
//! for the account tokens, it is the file a user goes looking for, and it lives
//! outside `CACHE_DIR` so `--clear-cache` cannot take a hand-written setting
//! with it. `ui-prefs.json` stays what it was — state cctop remembers for
//! itself — and where the two name the same thing, the file the user wrote
//! wins.
//!
//! [`SETTINGS`] and [`BINDINGS`] are the whole schema. The template written
//! into a fresh file, the settings panel and the key remapping are all derived
//! from them, so a knob cannot be documented in one place and missing from
//! another.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::Path;

/// Every `[settings]` key: its name, its default as the file would spell it,
/// and what it does.
pub const SETTINGS: [(&str, &str, &str); 5] = [
    (
        "theme",
        "\"auto\"",
        "auto/light/dark/mono; $CCTOP_THEME wins. On restart",
    ),
    (
        "notify",
        "false",
        "Bell + desktop alert when a session needs you",
    ),
    (
        "auto_update",
        "true",
        "Offer to update at startup when a newer release is out",
    ),
    (
        "compact_threshold",
        "83.5",
        "Context % at which the agent compacts. On restart",
    ),
    (
        "hide_columns",
        "\"\"",
        "Columns to hide, e.g. \"tok_rate,mem\". On restart",
    ),
];

/// Every dashboard action a `[keys]` entry can rebind: its name, the key it is
/// on by default, and what it does.
///
/// Only the session table's single keys. Modals, panes and the Alt- workspace
/// keys keep theirs: a modal's keys are the letters on its own buttons, and a
/// pane's belong to the agent.
///
// ponytail: dashboard keys only; extend to modals if someone asks to rebind one.
pub const BINDINGS: [(&str, &str, &str); 46] = [
    ("quit", "q", "Quit"),
    ("help", "?", "Help"),
    ("settings", ",", "This settings panel"),
    ("up", "k", "Move up (the arrow keeps working)"),
    ("panel_up", "shift+up", "Scroll the bottom panel up"),
    ("panel_down", "shift+down", "Scroll the bottom panel down"),
    ("panel_top", "shift+home", "Top of the bottom panel"),
    ("panel_bottom", "shift+end", "Bottom of the bottom panel"),
    ("down", "j", "Move down (the arrow keeps working)"),
    ("top", "g", "Jump to the first session"),
    ("bottom", "G", "Jump to the last session"),
    ("next_match", "n", "Next search match"),
    ("prev_match", "N", "Previous search match"),
    ("bell", "b", "Jump to the session that rang last"),
    ("follow", "f", "Follow mode"),
    ("attach", "a", "Open its terminal in a tab"),
    ("resume", "R", "Resume it in a tab of its own"),
    ("send", "s", "Type a line into its terminal"),
    ("handoff", "O", "Hand its context to a different agent"),
    ("conversation", "i", "Read its conversation"),
    ("copy", "y", "Copy resume command or transcript path"),
    ("expand", "e", "Show its subagents"),
    ("expand_all", "E", "Show all subagents"),
    ("delete", "d", "Delete it"),
    ("terminate", "ctrl+k", "Terminate it"),
    ("mark", "space", "Mark / unmark it"),
    ("unmark_all", "U", "Clear all marks"),
    ("delete_marked", "D", "Delete all marked sessions"),
    ("kill_marked", "K", "Terminate all marked sessions"),
    ("search", "/", "Filter"),
    ("sort", "S", "Sort by a column"),
    ("cost_floor", "#", "Only sessions costing at least $X"),
    ("running_only", "`", "Show only running sessions"),
    ("new_tab", "t", "New tab"),
    ("open_hosted", "A", "Open the agent this cctop launched"),
    ("notify", "w", "Toggle alerts"),
    ("share", "W", "Share the agent's terminal to a browser"),
    ("serve", "B", "Serve this table to a browser"),
    ("optimize", "o", "What was spent and not got back"),
    ("compare", "c", "How each model did on the work you gave it"),
    ("hooks", "h", "Agent integration panel"),
    ("refresh", "r", "Refresh now"),
    ("tool_filter_prev", "[", "Previous Tool Activity filter"),
    ("tool_filter_next", "]", "Next Tool Activity filter"),
    ("live_filter", "L", "Toggle the Tool Activity live filter"),
    ("diffs", "v", "Toggle inline diffs"),
];

/// What `config.toml` says, with anything it does not say left `None`.
#[derive(Debug, Default, Clone)]
pub struct Settings {
    pub theme: Option<String>,
    pub notify: Option<bool>,
    pub auto_update: Option<bool>,
    /// A fraction, though the file spells it as a percentage — the same as
    /// `$CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, which is what a user will compare it
    /// to.
    pub compact_threshold: Option<f64>,
    pub hide_columns: Option<String>,
    /// `(action, key)` as written, in file order.
    pub keys: Vec<(String, String)>,
    /// Everything that was written and could not be used, said in a sentence.
    /// Shown in the panel rather than refused: a typo in a config file must not
    /// stop a monitoring tool from starting.
    pub problems: Vec<String>,
}

impl Settings {
    pub fn load() -> Settings {
        Settings::load_from(&crate::config::CONFIG_FILE)
    }

    pub fn load_from(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .map(|text| Settings::parse(&text))
            .unwrap_or_default()
    }

    pub fn parse(text: &str) -> Settings {
        let mut out = Settings::default();
        let doc = match text.parse::<toml_edit::DocumentMut>() {
            Ok(doc) => doc,
            Err(e) => {
                out.problems.push(format!("not valid TOML: {e}"));
                return out;
            }
        };
        if let Some(table) = doc.get("settings").and_then(|t| t.as_table_like()) {
            for (name, item) in table.iter() {
                let bad = match name {
                    "theme" => item.as_str().map(|v| out.theme = Some(v.into())).is_none(),
                    "notify" => item.as_bool().map(|v| out.notify = Some(v)).is_none(),
                    "auto_update" => item.as_bool().map(|v| out.auto_update = Some(v)).is_none(),
                    "compact_threshold" => item
                        .as_float()
                        .or_else(|| item.as_integer().map(|i| i as f64))
                        .filter(|p| (1.0..=100.0).contains(p))
                        .map(|p| out.compact_threshold = Some(p / 100.0))
                        .is_none(),
                    "hide_columns" => item
                        .as_str()
                        .map(|v| out.hide_columns = Some(v.into()))
                        .is_none(),
                    _ => {
                        out.problems
                            .push(format!("[settings] {name}: no such setting"));
                        false
                    }
                };
                if bad {
                    out.problems
                        .push(format!("[settings] {name}: wrong kind of value"));
                }
            }
        }
        if let Some(table) = doc.get("keys").and_then(|t| t.as_table_like()) {
            for (action, item) in table.iter() {
                match item.as_str() {
                    Some(key) => out.keys.push((action.to_string(), key.to_string())),
                    None => out.problems.push(format!("[keys] {action}: not a string")),
                }
            }
        }
        out
    }

    /// The key `action` is on now: the file's, else the default.
    pub fn key_for(&self, action: &str) -> &str {
        self.keys
            .iter()
            .rev()
            .find(|(a, _)| a == action)
            .map(|(_, k)| k.as_str())
            .or_else(|| BINDINGS.iter().find(|b| b.0 == action).map(|b| b.1))
            .unwrap_or("")
    }

    /// The value a setting has now, as the file would spell it, and whether the
    /// file is what set it.
    pub fn value_of(&self, name: &str) -> (String, bool) {
        let set = match name {
            "theme" => self.theme.as_ref().map(|v| format!("{v:?}")),
            "notify" => self.notify.map(|v| v.to_string()),
            "auto_update" => self.auto_update.map(|v| v.to_string()),
            "compact_threshold" => self.compact_threshold.map(|v| format!("{}", v * 100.0)),
            "hide_columns" => self.hide_columns.as_ref().map(|v| format!("{v:?}")),
            _ => None,
        };
        match set {
            Some(v) => (v, true),
            None => {
                let default = SETTINGS.iter().find(|s| s.0 == name).map_or("", |s| s.1);
                (default.to_string(), false)
            }
        }
    }
}

/// The commented reference appended to `config.toml`, every line at its default.
pub fn template() -> String {
    let mut out = String::from(
        "\n# cctop settings. Uncomment a line to change it; `,` in cctop shows what\n\
         # is in effect. Keys are a character (\"x\", \"X\"), a name (space, enter,\n\
         # esc, tab, up, down, left, right, pageup, pagedown, home, end, f1–f12),\n\
         # optionally prefixed with ctrl+, alt+ or shift+.\n\n[settings]\n",
    );
    for (name, default, what) in SETTINGS {
        out.push_str(&format!(
            "# {:<32} # {what}\n",
            format!("{name} = {default}")
        ));
    }
    out.push_str("\n[keys]\n");
    for (action, key, what) in BINDINGS {
        out.push_str(&format!(
            "# {:<32} # {what}\n",
            format!("{action} = {key:?}")
        ));
    }
    out
}

/// Make sure `config.toml` exists and carries the reference, so opening it to
/// change something shows what there is to change.
///
/// Appended rather than rewritten, and only to a file that parses and has
/// neither table yet: the file may hold account tokens, and a second
/// `[settings]` header would make the whole of it invalid.
pub fn ensure_template(path: &Path) -> std::io::Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return Ok(());
    };
    if doc.contains_key("settings") || doc.contains_key("keys") {
        return Ok(());
    }
    save(path, &(text + &template()))
}

/// Replace the file with `text` the way `quota::add_account` does: through a
/// temporary file, so an interrupted write cannot truncate a file holding
/// account tokens, and owner-only from the start, because it may hold them.
fn save(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.cctop-tmp");
    std::fs::write(&tmp, text)?;
    crate::quota::restrict(&tmp)?;
    std::fs::rename(&tmp, path)
}

/// Set `name` in `[table]` of the file at `path` to `value`, or take it out
/// when `value` is `None` — which is how a setting goes back to its default.
///
/// Through `toml_edit`, like `quota::add_account`, so the comments, the layout
/// and the account tokens in the file come through untouched; and refused on a
/// file that does not parse, which cctop must not rewrite. A file that does not
/// exist yet gets the reference first, so opening it later still shows what
/// else there is.
pub fn write(
    path: &Path,
    table: &str,
    name: &str,
    value: Option<toml_edit::Value>,
) -> anyhow::Result<()> {
    ensure_template(path)?;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| anyhow::anyhow!("{} is not valid TOML ({e}); fix it first", path.display()))?;
    if !doc.contains_key(table) {
        doc.insert(table, toml_edit::Item::Table(toml_edit::Table::new()));
    }
    let entries = doc[table]
        .as_table_like_mut()
        .ok_or_else(|| anyhow::anyhow!("`{table}` in {} is not a table", path.display()))?;
    match value {
        Some(value) => {
            entries.insert(name, toml_edit::Item::Value(value));
        }
        None => {
            entries.remove(name);
        }
    }
    save(path, &doc.to_string())?;
    Ok(())
}

type Key = (KeyCode, KeyModifiers);

/// A pressed key as `[keys]` spells it — the inverse of [`parse_key`] — or
/// `None` for one the file has no way to say, such as Shift on an arrow.
pub fn key_name(key: KeyEvent) -> Option<String> {
    let (code, mods) = normal(key.code, key.modifiers);
    let mut out = String::new();
    if mods.contains(KeyModifiers::CONTROL) {
        out.push_str("ctrl+");
    }
    if mods.contains(KeyModifiers::ALT) {
        out.push_str("alt+");
    }
    if mods.contains(KeyModifiers::SHIFT) {
        out.push_str("shift+");
    }
    let name = match code {
        KeyCode::BackTab => "shift+tab".into(),
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::F(n @ 1..=12) => format!("f{n}"),
        _ => return None,
    };
    out.push_str(&name);
    Some(out)
}

/// `"ctrl+k"`, `"G"`, `"f5"`, `"space"` as the key it names.
pub fn parse_key(spec: &str) -> Option<Key> {
    let mut mods = KeyModifiers::NONE;
    let mut rest = spec;
    // A lone `+` is the plus key, not an empty modifier.
    while let Some((head, tail)) = rest.split_once('+').filter(|(_, t)| !t.is_empty()) {
        mods |= match head.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => KeyModifiers::CONTROL,
            "alt" => KeyModifiers::ALT,
            "shift" => KeyModifiers::SHIFT,
            _ => return None,
        };
        rest = tail;
    }
    let mut chars = rest.chars();
    let code = match (chars.next(), chars.next()) {
        (Some(c), None) => KeyCode::Char(c),
        _ => match rest.to_ascii_lowercase().as_str() {
            "space" => KeyCode::Char(' '),
            "enter" => KeyCode::Enter,
            "esc" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            f => KeyCode::F(
                f.strip_prefix('f')?
                    .parse()
                    .ok()
                    .filter(|n| (1..=12).contains(n))?,
            ),
        },
    };
    // Shift on a character is the capital it types, and Shift+Tab is the key
    // terminals send as BackTab; `normal` then compares them as they arrive.
    Some(match code {
        KeyCode::Char(c) if mods.contains(KeyModifiers::SHIFT) => (
            KeyCode::Char(c.to_ascii_uppercase()),
            mods - KeyModifiers::SHIFT,
        ),
        KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => {
            (KeyCode::BackTab, mods - KeyModifiers::SHIFT)
        }
        _ => (code, mods),
    })
}

/// A key as the remapping compares it: Shift is dropped from a character and
/// from BackTab, because the key already says it — terminals disagree about
/// whether `G` arrives with the modifier or without. On anything else Shift is
/// the difference between two keys, and is kept.
fn normal(code: KeyCode, mods: KeyModifiers) -> Key {
    match code {
        KeyCode::Char(_) | KeyCode::BackTab => (code, mods - KeyModifiers::SHIFT),
        _ => (code, mods),
    }
}

/// The user's `[keys]`, as a translation in front of the dashboard's own
/// handler.
///
/// A translation rather than a table the handler looks actions up in: the
/// handler is a long `match` whose arms carry guards and reasons of their own,
/// and turning each into a lookup would be a rewrite of every one for a feature
/// that only ever moves a key. So a rebound key is rewritten into the default
/// key of its action, and the default key of an action that has moved is
/// swallowed — unless something else was rebound onto it, which is how two
/// actions swap.
#[derive(Debug, Default, Clone)]
pub struct Keymap {
    remap: Vec<(Key, Key)>,
    freed: Vec<Key>,
}

impl Keymap {
    /// The keymap `settings` describes, and a sentence for each entry that
    /// could not be used.
    pub fn build(settings: &Settings) -> (Keymap, Vec<String>) {
        let mut map = Keymap::default();
        let mut problems = Vec::new();
        for (action, spec) in &settings.keys {
            let Some((_, default, _)) = BINDINGS.iter().find(|b| b.0 == action) else {
                problems.push(format!("[keys] {action}: no such action"));
                continue;
            };
            let Some((code, mods)) = parse_key(spec) else {
                problems.push(format!("[keys] {action}: cannot read the key {spec:?}"));
                continue;
            };
            let from = normal(code, mods);
            let (dc, dm) = parse_key(default).expect("every default key parses");
            let to = normal(dc, dm);
            if from == to {
                continue;
            }
            if map.remap.iter().any(|(f, _)| *f == from) {
                problems.push(format!("[keys] {action}: {spec:?} is already bound above"));
                continue;
            }
            map.remap.push((from, to));
            map.freed.push(to);
        }
        (map, problems)
    }

    /// The key the dashboard should see for `key`, or `None` when it has been
    /// moved away and nothing took its place.
    pub fn apply(&self, key: KeyEvent) -> Option<KeyEvent> {
        let k = normal(key.code, key.modifiers);
        if let Some((_, (code, mods))) = self.remap.iter().find(|(from, _)| *from == k) {
            return Some(KeyEvent::new(*code, *mods));
        }
        if self.freed.contains(&k) {
            return None;
        }
        Some(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn the_template_parses_to_nothing_set() {
        let s = Settings::parse(&template());
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        assert!(s.keys.is_empty() && s.theme.is_none());
        // And uncommenting every line gives back the defaults without complaint,
        // so no line in it is a lie about its own syntax.
        let all: String = template()
            .lines()
            .map(|l| match l.strip_prefix("# ") {
                Some(entry) if entry.contains(" = ") => format!("{entry}\n"),
                _ => format!("{l}\n"),
            })
            .collect();
        let s = Settings::parse(&all);
        assert!(s.problems.is_empty(), "{:?}", s.problems);
        assert_eq!(s.keys.len(), BINDINGS.len());
        let (map, problems) = Keymap::build(&s);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(
            map.remap.is_empty(),
            "a default rebound onto itself is no change"
        );
    }

    #[test]
    fn a_rebound_key_moves_and_frees_its_old_one() {
        let s = Settings::parse("[keys]\nquit = \"x\"\n");
        let (map, _) = Keymap::build(&s);
        assert_eq!(
            map.apply(press('x')).map(|k| k.code),
            Some(KeyCode::Char('q'))
        );
        assert_eq!(map.apply(press('q')), None);
        assert_eq!(
            map.apply(press('j')).map(|k| k.code),
            Some(KeyCode::Char('j'))
        );
        assert_eq!(s.key_for("quit"), "x");
        assert_eq!(s.key_for("help"), "?");
    }

    #[test]
    fn two_actions_can_swap() {
        let s = Settings::parse("[keys]\nup = \"j\"\ndown = \"k\"\n");
        let (map, _) = Keymap::build(&s);
        assert_eq!(
            map.apply(press('j')).map(|k| k.code),
            Some(KeyCode::Char('k'))
        );
        assert_eq!(
            map.apply(press('k')).map(|k| k.code),
            Some(KeyCode::Char('j'))
        );
    }

    #[test]
    fn keys_parse_with_modifiers_and_names() {
        assert_eq!(
            parse_key("ctrl+k"),
            Some((KeyCode::Char('k'), KeyModifiers::CONTROL))
        );
        assert_eq!(parse_key("f5"), Some((KeyCode::F(5), KeyModifiers::NONE)));
        assert_eq!(
            parse_key("+"),
            Some((KeyCode::Char('+'), KeyModifiers::NONE))
        );
        assert_eq!(
            parse_key("space"),
            Some((KeyCode::Char(' '), KeyModifiers::NONE))
        );
        assert_eq!(parse_key("hyper+x"), None);
        assert_eq!(parse_key("f13"), None);
        // Shift arrives on a capital from some terminals and not others.
        let s = Settings::parse("[keys]\nbottom = \"x\"\n");
        let (map, _) = Keymap::build(&s);
        assert_eq!(
            map.apply(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT)),
            None
        );
    }

    #[test]
    fn every_default_key_spells_back_the_same() {
        for (action, spec, _) in BINDINGS {
            let (code, mods) = parse_key(spec).unwrap();
            assert_eq!(
                key_name(KeyEvent::new(code, mods)).as_deref(),
                Some(spec),
                "{action}"
            );
        }
        let shifted = KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT);
        assert_eq!(key_name(shifted).as_deref(), Some("shift+up"));
        let back = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(key_name(back).as_deref(), Some("shift+tab"));
        assert_eq!(
            parse_key("shift+tab"),
            Some((KeyCode::BackTab, KeyModifiers::NONE))
        );
        assert_eq!(parse_key("shift+a"), parse_key("A"));
        // Shift on an arrow is its own key: binding it leaves the plain arrow be.
        let s = Settings::parse("[keys]\nbottom = \"shift+down\"\n");
        let (map, _) = Keymap::build(&s);
        let got = map.apply(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
        assert_eq!(got.map(|k| k.code), Some(KeyCode::Char('G')));
        let plain = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(map.apply(plain), Some(plain));
    }

    #[test]
    fn writing_keeps_the_rest_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "# mine\n[accounts.work]\ntoken = \"t\"\n").unwrap();
        write(&path, "keys", "quit", Some("x".into())).unwrap();
        write(&path, "settings", "notify", Some(true.into())).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "a file that may hold tokens is readable by others"
        );
        assert!(
            text.contains("# mine") && text.contains("token = \"t\""),
            "{text}"
        );
        let s = Settings::parse(&text);
        assert_eq!((s.key_for("quit"), s.notify), ("x", Some(true)));
        // Taking it out is going back to the default.
        write(&path, "keys", "quit", None).unwrap();
        assert_eq!(Settings::load_from(&path).key_for("quit"), "q");
        // And a file that does not parse is left alone.
        std::fs::write(&path, "[broken").unwrap();
        assert!(write(&path, "keys", "quit", Some("x".into())).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[broken");
    }

    #[test]
    fn mistakes_are_reported_not_fatal() {
        let s = Settings::parse(
            "[settings]\nnotify = \"yes\"\nshiny = 1\ncompact_threshold = 90\n[keys]\nfly = \"x\"\nquit = \"hyper+q\"\n",
        );
        assert_eq!(s.compact_threshold, Some(0.9));
        assert_eq!(s.problems.len(), 2, "{:?}", s.problems);
        let (_, problems) = Keymap::build(&s);
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert_eq!(Settings::parse("not toml [").problems.len(), 1);
    }
}
