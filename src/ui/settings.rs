//! The settings panel: every setting and dashboard keybind in `config.toml`,
//! changed in place.
//!
//! The schema and the file handling are [`crate::settings`]'s; this is the
//! half that belongs to the running dashboard — reading the file back when it
//! changes, and turning a row and a keypress into one entry written.

use super::*;

impl App {
    /// Re-read `config.toml` when it has changed since the last read, so a
    /// rebinding saved in the editor tab works at the next key.
    ///
    /// The settings that shape the first frame — theme, hidden columns — are
    /// not re-applied here; the panel says they want a restart.
    pub(super) fn reload_settings(&mut self) {
        let Some(path) = &self.settings_file else {
            return;
        };
        let now = crate::config::file_mtime_ms(path);
        if now == self.settings_stamp && now != 0 {
            return;
        }
        self.settings_stamp = now;
        let mut settings = crate::settings::Settings::load_from(path);
        let (keymap, problems) = crate::settings::Keymap::build(&settings);
        settings.problems.extend(problems);
        self.settings = settings;
        self.keymap = keymap;
    }

    /// Write one entry of the settings file and read it straight back, saying
    /// in the status line what changed or why it could not.
    fn write_setting(&mut self, table: &str, name: &str, value: Option<toml_edit::Value>) {
        let Some(path) = self.settings_file.clone() else {
            return;
        };
        let said = match &value {
            Some(v) => format!("{name} = {}", v.to_string().trim()),
            None => format!("{name} back to its default"),
        };
        match crate::settings::write(&path, table, name, value) {
            Ok(()) => self.set_status(format!("Saved {said}")),
            Err(e) => self.set_status(format!("Could not save: {e}")),
        }
        // Forced: two writes inside one millisecond share an mtime.
        self.settings_stamp = 0;
        self.reload_settings();
    }

    /// Enter on the panel's row: a toggle flips, the theme turns to the next
    /// one, a keybind waits for its new key, and anything else opens a field.
    pub(super) fn settings_activate(&mut self) {
        use crate::settings::{BINDINGS, SETTINGS};
        let Some((name, _, _)) = SETTINGS.get(self.settings_cursor) else {
            if self.settings_cursor < SETTINGS.len() + BINDINGS.len() {
                self.settings_capture = true;
            }
            return;
        };
        let (current, _) = self.settings.value_of(name);
        match *name {
            "notify" | "auto_update" => {
                let on = current != "true";
                self.write_setting("settings", name, Some(on.into()));
                // The one setting with a live switch of its own, so the file
                // and the running cctop agree without a restart.
                if *name == "notify" && self.notify.enabled != on {
                    self.notify.enabled = on;
                    self.save_prefs();
                }
            }
            "theme" => {
                const THEMES: [&str; 4] = ["auto", "light", "dark", "mono"];
                let at = THEMES.iter().position(|t| current.trim_matches('"') == *t);
                let next = THEMES[at.map_or(0, |i| (i + 1) % THEMES.len())];
                self.write_setting("settings", name, Some(next.into()));
            }
            _ => self.settings_input = Some(current.trim_matches('"').to_string()),
        }
    }

    /// Backspace on the panel's row: take its entry out of the file.
    pub(super) fn settings_reset(&mut self) {
        use crate::settings::{BINDINGS, SETTINGS};
        match self.settings_cursor.checked_sub(SETTINGS.len()) {
            None => self.write_setting("settings", SETTINGS[self.settings_cursor].0, None),
            Some(i) if i < BINDINGS.len() => self.write_setting("keys", BINDINGS[i].0, None),
            Some(_) => {}
        }
    }

    /// The key pressed while the panel was waiting for one, bound to the
    /// cursor's action.
    ///
    /// Esc cancels rather than binding, since it is the only way out of the
    /// wait. A key another action is on is still taken — the user pressed it
    /// on purpose — but the status line says which action just lost it.
    pub(super) fn settings_capture_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        use crate::settings::{BINDINGS, SETTINGS};
        self.settings_capture = false;
        if key.code == ratatui::crossterm::event::KeyCode::Esc {
            return;
        }
        let Some((action, default, _)) = self
            .settings_cursor
            .checked_sub(SETTINGS.len())
            .and_then(|i| BINDINGS.get(i))
        else {
            return;
        };
        let Some(spec) = crate::settings::key_name(key) else {
            self.set_status("That key cannot be written in the config file");
            return;
        };
        let taken = BINDINGS
            .iter()
            .find(|b| b.0 != *action && self.settings.key_for(b.0) == spec)
            .map(|b| b.0);
        // The default is written as no entry at all, so the file only ever
        // holds what someone changed.
        let value = (spec != *default).then(|| spec.as_str().into());
        self.write_setting("keys", action, value);
        if let Some(other) = taken {
            self.set_status(format!(
                "{action} = {spec} — that was {other}'s key, so rebind {other} too"
            ));
        }
    }

    /// Enter in a setting's field: an empty one resets it, anything else is
    /// checked the way the file's reader would check it.
    pub(super) fn settings_commit_input(&mut self) {
        use crate::settings::SETTINGS;
        let Some(text) = self.settings_input.take() else {
            return;
        };
        let Some((name, _, _)) = SETTINGS.get(self.settings_cursor) else {
            return;
        };
        let text = text.trim();
        if text.is_empty() {
            return self.write_setting("settings", name, None);
        }
        match *name {
            "compact_threshold" => match text.parse::<f64>() {
                Ok(p) if (1.0..=100.0).contains(&p) => {
                    self.write_setting("settings", name, Some(p.into()))
                }
                _ => self.set_status("compact_threshold is a percentage, 1 to 100"),
            },
            _ => self.write_setting("settings", name, Some(text.into())),
        }
    }
}
