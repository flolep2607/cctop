//! The settings page: every setting, every view choice and every dashboard
//! keybind, changed in place.
//!
//! Three sources meet here, and this is the only place they do. `[settings]`
//! and `[keys]` are [`cctop_core::settings`]'s — the file a user wrote, which is why
//! that module owns them. The view choices below are cctop's own: they live in
//! `ui-prefs.json`, are the running dashboard's own fields, and have no
//! `config.toml` entry to sit in. Before this page was a tab they were
//! reachable only by remembering which key pressed which one, and a setting
//! you cannot see is a setting you cannot find.
//!
//! Keeping the view table here rather than in [`cctop_core::settings`] is what stops
//! the file schema depending on the UI: the read and write halves both need
//! `App`, and a `config.toml` entry that could only be understood by the
//! dashboard would be one `doctor` and an editor could not check either.

use super::*;

/// One editable line of the page, as a place in one of the three sources.
///
/// A cursor that indexes the rendered lines cannot survive a filter changing
/// which lines exist, and a cursor that indexes a source cannot be drawn
/// without re-deriving that arithmetic at every step. Naming the line instead
/// makes the filter a list of these, and every question about the cursor — what
/// is under it, what Enter does, what Backspace does — a match on one value.
///
/// Named `Item` rather than `Row` because this crate already has a
/// [`Row`](super::Row): the session table's, and a different thing entirely.
/// Two types called `Row` in one `ui` module means every file that wants both
/// has to alias one of them, and the alias gets read as though it meant
/// something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// An entry of `[settings]`, by index into [`cctop_core::settings::SETTINGS`].
    Setting(usize),
    /// A view choice, by index into [`VIEWS`].
    View(usize),
    /// An entry of `[keys]`, by index into [`cctop_core::settings::BINDINGS`].
    Key(usize),
}

/// How a view choice is read and changed.
///
/// Every field is a function pointer or a `'static` slice, so the whole thing
/// is `Copy` — which is what lets `VIEWS` be a `const` and a row be handed
/// around by value rather than by the index it came from.
#[derive(Clone, Copy)]
pub enum View {
    /// On or off, flipped in place.
    ///
    /// Named for the effect rather than the field, because the field is an
    /// implementation detail of the dashboard and the name is what a user
    /// reading the page is looking for.
    Flag {
        value: fn(&App) -> bool,
        set: fn(&mut App, bool),
    },
    /// One of a list, cycled through in place.
    ///
    /// The list is read at draw time rather than fixed, because some of these
    /// are choices between the accounts *this machine* has and another machine
    /// has different ones. A row offering a name that does not resolve would be
    /// a row that silently launched an agent under somebody else's login.
    Choice {
        value: fn(&App) -> String,
        options: fn(&App) -> Vec<String>,
        set: fn(&mut App, &str),
    },
    /// A number in a range, typed rather than cycled.
    ///
    /// Typed because no sensible number of them is a short list: a cost floor
    /// is a dollar amount somebody has in mind, and paging towards it is
    /// slower than writing it.
    Number {
        value: fn(&App) -> f64,
        range: (f64, f64),
        set: fn(&mut App, f64),
    },
}

/// A view choice cctop remembers for itself: the rows that are not in
/// `config.toml` because there is no file for a user to have written them in.
///
/// The default lives in `UiPrefs` rather than spelled here, which is the one
/// thing this table cannot derive the way the file schema derives its template.
/// Each of these is a deliberate act about how the whole table reads — a
/// pressed key on a mode line — so unlike the table's sort order they are worth
/// keeping, and unlike a filter they are worth being able to see at a glance.
pub const VIEWS: [(&str, &str, View); 10] = [
    (
        "tree",
        "Draw the table as repositories and their checkouts",
        View::Flag {
            value: |app| app.tree,
            set: |app, on| {
                app.tree = on;
                app.save_prefs();
            },
        },
    ),
    (
        "live_only",
        "Only sessions that are running",
        View::Flag {
            value: |app| app.live_only,
            set: |app, on| {
                app.live_only = on;
                app.save_prefs();
            },
        },
    ),
    (
        "age",
        "Hide sessions quiet for longer than",
        View::Choice {
            value: |app| {
                app.age_filter
                    .map(|a| a.key().to_string())
                    .unwrap_or_default()
            },
            options: |_| {
                std::iter::once(String::new())
                    .chain(
                        AGE_OPTIONS
                            .iter()
                            .filter_map(|o| o.as_ref())
                            .map(|a| a.key().to_string()),
                    )
                    .collect()
            },
            set: |app, key| {
                app.age_filter = AGE_OPTIONS
                    .iter()
                    .filter_map(|o| o.as_ref())
                    .find(|a| a.key() == key)
                    .copied();
                app.age_cursor = AGE_OPTIONS
                    .iter()
                    .position(|o| *o == app.age_filter)
                    .unwrap_or(AGE_OPTIONS.len() - 1);
                app.save_prefs();
            },
        },
    ),
    (
        "cost_floor",
        "Only sessions costing at least $X (0 for all)",
        View::Number {
            value: |app| app.cost_floor,
            range: (0.0, 100_000.0),
            set: |app, v| {
                app.cost_floor = v;
                app.save_prefs();
            },
        },
    ),
    (
        "bottom_panel",
        "Which panel opens under the table",
        View::Choice {
            value: |app| super::panels::TABS[app.bottom_tab.min(8)].to_string(),
            options: |_| super::panels::TABS.iter().map(|t| t.to_string()).collect(),
            set: |app, name| {
                app.bottom_tab = super::panels::TABS
                    .iter()
                    .position(|t| *t == name)
                    .unwrap_or(app.bottom_tab);
                app.save_prefs();
            },
        },
    ),
    (
        "tool_show_diff",
        "Show each edit's diff under its row",
        View::Flag {
            value: |app| app.tool_show_diff,
            set: |app, on| {
                app.tool_show_diff = on;
                app.save_prefs();
            },
        },
    ),
    (
        "tool_live_only",
        "Tool Activity shows this run only",
        View::Flag {
            value: |app| app.tool_live_only,
            set: |app, on| {
                app.tool_live_only = on;
                app.save_prefs();
            },
        },
    ),
    (
        "subagent_sort",
        "What the Subagents panel sorts by",
        View::Choice {
            value: |app| {
                let (col, asc) = app.subagent_sort;
                format!("{}{}", col.key(), if asc { " ↑" } else { " ↓" })
            },
            options: |_| {
                let cols = [
                    super::panels::SubagentSort::Last,
                    super::panels::SubagentSort::Type,
                    super::panels::SubagentSort::Model,
                    super::panels::SubagentSort::Description,
                    super::panels::SubagentSort::Cost,
                    super::panels::SubagentSort::Tools,
                    super::panels::SubagentSort::Context,
                    super::panels::SubagentSort::Duration,
                ];
                let mut out = Vec::new();
                for col in cols {
                    for asc in [false, true] {
                        out.push(format!("{}{}", col.key(), if asc { " ↑" } else { " ↓" }));
                    }
                }
                out
            },
            set: |app, spec| {
                let (key, asc) = spec.split_once(' ').unwrap_or((spec, ""));
                let asc = !asc.is_empty();
                app.subagent_sort = (super::panels::SubagentSort::parse(key), asc);
                app.save_prefs();
            },
        },
    ),
    (
        "claude_account",
        "The Claude account a new tab launches under",
        View::Choice {
            value: |app| chosen_account(app, cctop_core::pricing::Provider::Claude),
            options: |app| account_names(app, cctop_core::pricing::Provider::Claude),
            set: |app, name| {
                set_account(app, cctop_core::pricing::Provider::Claude, name);
            },
        },
    ),
    (
        "codex_account",
        "The Codex account a new tab launches under",
        View::Choice {
            value: |app| chosen_account(app, cctop_core::pricing::Provider::Codex),
            options: |app| account_names(app, cctop_core::pricing::Provider::Codex),
            set: |app, name| {
                set_account(app, cctop_core::pricing::Provider::Codex, name);
            },
        },
    ),
];

/// The accounts `provider` can launch under, as the page offers them.
///
/// The default is the empty string and comes first, because it is both the
/// first thing anybody wants and the thing to come back to — see
/// [`chosen_account`], which spells the same choice the same way.
///
/// `app` goes unused and is named only to match the
/// [`View::Choice::options`] signature every row in the table shares. The
/// accounts are read off the filesystem rather than off the dashboard, so a
/// row that did need the app would be a row reading something else.
fn account_names(_app: &App, provider: cctop_core::pricing::Provider) -> Vec<String> {
    let mut names = vec![String::new()];
    for profile in cctop_core::config::launchable_for(provider) {
        // The default account is `default` by name, and the empty choice above
        // is that account; offering it twice would make the row cycle through
        // two entries that mean one thing.
        if profile.name != "default" {
            names.push(profile.name.clone());
        }
    }
    names
}

/// Which account a new tab under `provider` will use, as the page spells it.
///
/// Spelled the way [`account_names`] spells the choice it corresponds to, and
/// that means the default account is the empty string rather than the name it
/// happens to go by. A row that read `default` against an option list whose
/// first entry is the empty string would show a marker on every account
/// selection nobody had ever made.
fn chosen_account(app: &App, provider: cctop_core::pricing::Provider) -> String {
    match app.chosen_profile(provider) {
        // The conventional directory is what "no choice" means here, and it is
        // named `default` on disk; the page does not need to say so.
        Some(p) if p.name != "default" => p.name.clone(),
        _ => String::new(),
    }
}

/// Point new tabs at a named account, refusing one that has gone.
///
/// Resolved against the live list rather than trusted: a profile removed from
/// the machine between the row being drawn and Enter being pressed would
/// otherwise leave `launch_profile` pointing past the end of a shorter list,
/// and a new agent would start under whichever account happened to sit there.
fn set_account(app: &mut App, provider: cctop_core::pricing::Provider, name: &str) {
    let Some(at) = cctop_core::config::launchable_for(provider)
        .iter()
        .position(|p| p.name == name)
    else {
        return;
    };
    app.launch_profile.insert(provider, at);
    app.save_prefs();
}

impl App {
    /// Re-read `config.toml` when it has changed since the last read, so a
    /// rebinding saved in an editor works at the next key.
    ///
    /// The settings that shape the first frame — the theme — are not re-applied
    /// here; the row says it wants a restart. Hidden columns are, because the
    /// table reads the list every frame and nothing about a palette is involved.
    pub(super) fn reload_settings(&mut self) {
        let Some(path) = &self.settings_file else {
            return;
        };
        let now = cctop_core::config::file_mtime_ms(path);
        if now == self.settings_stamp && now != 0 {
            return;
        }
        self.settings_stamp = now;
        let mut settings = cctop_core::settings::Settings::load_from(path);
        let (keymap, problems) = cctop_core::settings::Keymap::build(&settings);
        settings.problems.extend(problems);
        self.settings = settings;
        self.keymap = keymap;
        self.apply_columns();
        self.apply_theme();
        self.apply_footer();
    }

    /// Repaint if the file now names a different theme.
    ///
    /// `reload_settings` is the one path every source of a change goes through —
    /// the settings page's own writes, and an edit made in an editor while the
    /// page is open with `e`. The second is why this is here rather than only in
    /// the row's action: without it, changing the theme by hand and saving would
    /// reload the keymap and the columns and quietly leave the screen in the old
    /// colours, which is the exact thing the row on the page promises does not
    /// happen.
    pub(super) fn apply_theme(&mut self) {
        super::theme::set_theme(self.settings.theme.as_deref());
        // And to agents opened from here on, which draw inside this palette.
        super::theme::tell_hosted_agents();
    }

    /// Re-read the hidden columns out of the settings file.
    ///
    /// Applied on every reload rather than once at startup, so hiding a column
    /// takes effect on the keypress that saved it. The table asks for the list
    /// on every frame — see [`columns::visible_columns_among`] — so this is a re-parse
    /// and nothing more. The environment variable still wins, as an override
    /// should.
    pub(super) fn apply_columns(&mut self) {
        if std::env::var_os("CCTOP_COLUMNS_HIDE").is_some() {
            return;
        }
        self.hidden_columns = match self.settings.hide_columns.as_deref() {
            Some(list) => super::columns::parse_hidden(list),
            None => Vec::new(),
        };
    }

    /// Re-read the footer's hiding list and live note out of the settings file.
    ///
    /// Applied on every reload rather than once at startup, because the
    /// footer reads the list on every frame — same reasoning as
    /// [`App::apply_columns`].
    pub(super) fn apply_footer(&mut self) {
        self.hidden_footer = self
            .settings
            .footer_hide
            .as_deref()
            .map(|list| {
                list.split(',')
                    .map(|name| name.trim().to_lowercase())
                    .filter(|name| !name.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        self.footer_extra.apply(
            self.settings.footer_note.as_deref(),
            self.settings.footer_command.as_deref(),
        );
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
        match cctop_core::settings::write(&path, table, name, value) {
            Ok(()) => self.set_status(format!("Saved {said}")),
            Err(e) => self.set_status(format!("Could not save: {e}")),
        }
        // Forced: two writes inside one millisecond share an mtime.
        self.settings_stamp = 0;
        self.reload_settings();
    }

    /// Every line of the page, in the order they are shown.
    ///
    /// The three sources in one list, because the page is one list: a filter
    /// that could only see one of them would be worse than no filter, and the
    /// cursor needs a single numbering to walk.
    pub(super) fn settings_items(&self) -> Vec<Item> {
        (0..cctop_core::settings::SETTINGS.len())
            .map(Item::Setting)
            .chain((0..VIEWS.len()).map(Item::View))
            .chain((0..cctop_core::settings::BINDINGS.len()).map(Item::Key))
            .collect()
    }

    /// The lines the filter leaves standing.
    ///
    /// Matched against the name, what it does and the value, because the three
    /// are what somebody knows: "which key does search", "alerts", "83.5". A
    /// filter that only read names could not find a setting by what it does,
    /// which is most of the time.
    pub(super) fn settings_shown(&self) -> Vec<Item> {
        let needle = self.settings_filter.to_string();
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return self.settings_items();
        }
        self.settings_items()
            .into_iter()
            .filter(|item| {
                let hay = match item {
                    Item::Setting(i) => {
                        let (name, default, what) = cctop_core::settings::SETTINGS[*i];
                        let (now, _) = self.settings.value_of(name);
                        format!("{name} {default} {what} {now}")
                    }
                    Item::View(i) => {
                        let (name, what, view) = &VIEWS[*i];
                        format!("{name} {what} {}", self.view_value(view))
                    }
                    Item::Key(i) => {
                        let (action, default, what) = cctop_core::settings::BINDINGS[*i];
                        format!(
                            "{action} {default} {what} {}",
                            self.settings.key_for(action)
                        )
                    }
                };
                hay.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// The line under the cursor, once the filter has had its say.
    pub(super) fn settings_item(&self) -> Option<Item> {
        self.settings_shown().get(self.settings_cursor).copied()
    }

    /// What a view choice is set to, as the page shows it.
    pub(super) fn view_value(&self, view: &View) -> String {
        match view {
            View::Flag { value, .. } => if value(self) { "on" } else { "off" }.to_string(),
            View::Choice { value, .. } => {
                let now = value(self);
                // Every choice list spells its first option as the empty
                // string, so that cycling round to the default is one step and
                // so the default is findable by name. That is a representation
                // detail and not something to print: a row with a blank value
                // reads as unset rather than as the thing it is set to.
                match now.is_empty() {
                    true => "default".to_string(),
                    false => now,
                }
            }
            View::Number { value, .. } => {
                let v = value(self);
                // Whole dollars, because a cost floor is money and `$0.50` is a
                // number nobody sets.
                if v.fract() == 0.0 && v.abs() < 1e9 {
                    format!("${v:.0}")
                } else {
                    format!("${v:.2}")
                }
            }
        }
    }

    /// Whether a view choice is off its default.
    ///
    /// Asked of the view rather than of the rendered text, because the two
    /// answers are not the same: a choice's default prints as the word
    /// `default`, and a row that is on its default should not be marked as
    /// though somebody had chosen it.
    pub(super) fn view_is_set(&self, view: &View) -> bool {
        match view {
            View::Flag { value, .. } => value(self),
            View::Number { value, .. } => value(self) != 0.0,
            View::Choice { value, options, .. } => {
                let first = options(self).first().cloned().unwrap_or_default();
                value(self) != first
            }
        }
    }

    /// Enter on the row: a toggle flips, a choice turns to the next one, a
    /// keybind waits for its new key, and anything else opens a field.
    pub(super) fn settings_activate(&mut self) {
        let Some(item) = self.settings_item() else {
            return;
        };
        match item {
            Item::Key(_) => self.settings_capture = true,
            Item::View(i) => self.view_activate(i),
            Item::Setting(i) => self.setting_activate(cctop_core::settings::SETTINGS[i].0),
        }
    }

    /// Turn a view choice to its next value, or open a field for a number.
    fn view_activate(&mut self, i: usize) {
        let (name, _, view) = &VIEWS[i];
        match view {
            View::Flag { value, set } => {
                let on = !value(self);
                set(self, on);
                self.set_status(format!("{name} {}", if on { "on" } else { "off" }));
            }
            View::Choice {
                value,
                options,
                set,
            } => {
                let options = options(self);
                if options.is_empty() {
                    return;
                }
                let now = value(self);
                let at = options.iter().position(|o| *o == now).unwrap_or(0);
                let next = options[(at + 1) % options.len()].clone();
                set(self, &next);
                // The name of what it became, not the field it was stored in: a
                // choice between accounts is read by the account, and one
                // between sort orders by the column.
                self.set_status(format!("{name} = {}", said(&next)));
            }
            View::Number { value, .. } => {
                self.settings_input = Some(format!("{}", value(self)).into());
            }
        }
    }

    /// Change one `[settings]` entry: a boolean flips, the theme turns to the
    /// next one, and anything else opens a field.
    fn setting_activate(&mut self, name: &str) {
        let (current, _) = self.settings.value_of(name);
        match name {
            _ if cctop_core::settings::is_toggle(name) => {
                let on = current != "true";
                self.write_setting("settings", name, Some(on.into()));
                // The one setting with a live switch of its own, so the file
                // and the running cctop agree without a restart.
                if name == "notify" && self.notify.enabled != on {
                    self.notify.enabled = on;
                    self.save_prefs();
                }
            }
            // Reached only because a theme is a choice and not a boolean, so
            // the guard above does not take it.
            "theme" => {
                const THEMES: [&str; 4] = ["auto", "light", "dark", "mono"];
                let at = THEMES.iter().position(|t| current.trim_matches('"') == *t);
                let next = THEMES[at.map_or(0, |i| (i + 1) % THEMES.len())];
                self.write_setting("settings", name, Some(next.into()));
                // Repainted by `write_setting` → `reload_settings` → the
                // reload's own `apply_theme`, so this is a no-op and is left
                // out rather than written twice. See [`App::apply_theme`] for
                // why the repaint lives there and not only here.
            }
            _ => self.settings_input = Some(current.trim_matches('"').to_string().into()),
        }
    }

    /// Backspace on the row: take a file entry out of the file, or put a view
    /// choice back to its default.
    ///
    /// A view choice's default is whatever `UiPrefs::default` says, which the
    /// table does not carry — so the reset writes the default value rather than
    /// removing anything. There is no file entry to take out, and leaving "off"
    /// spelled as "true" because it was once changed is the state the whole
    /// page exists to stop having.
    pub(super) fn settings_reset(&mut self) {
        let Some(item) = self.settings_item() else {
            return;
        };
        match item {
            Item::Setting(i) => {
                self.write_setting("settings", cctop_core::settings::SETTINGS[i].0, None)
            }
            Item::Key(i) => self.write_setting("keys", cctop_core::settings::BINDINGS[i].0, None),
            Item::View(i) => self.view_reset(i),
        }
    }

    fn view_reset(&mut self, i: usize) {
        let (name, _, view) = &VIEWS[i];
        match view {
            View::Flag { set, .. } => set(self, false),
            View::Choice { options, set, .. } => {
                // The first option is the default by construction: every
                // choice in the table puts the default first.
                let default = options(self).first().cloned().unwrap_or_default();
                set(self, &default);
            }
            View::Number { set, .. } => set(self, 0.0),
        }
        // What it became, not that it was reset: a value is what the row shows
        // and what somebody who pressed Backspace is looking for, and "back to
        // its default" is a sentence about the row rather than about the
        // setting.
        self.set_status(format!("{name} = {}", said(&self.view_value(view))));
    }

    /// The key pressed while the page was waiting for one, bound to the
    /// cursor's action.
    ///
    /// Esc cancels rather than binding, since it is the only way out of the
    /// wait. A key another action is on is still taken — the user pressed it
    /// on purpose — but the status line says which action just lost it.
    pub(super) fn settings_capture_key(&mut self, key: ratatui::crossterm::event::KeyEvent) {
        use cctop_core::settings::BINDINGS;
        self.settings_capture = false;
        if key.code == ratatui::crossterm::event::KeyCode::Esc {
            return;
        }
        let Some(Item::Key(i)) = self.settings_item() else {
            return;
        };
        let Some((action, default, _)) = BINDINGS.get(i) else {
            return;
        };
        let Some(spec) = cctop_core::settings::key_name(key) else {
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

    /// Enter in a field: an empty one resets the entry, anything else is
    /// checked the way the file's reader would check it.
    pub(super) fn settings_commit_input(&mut self) {
        use cctop_core::settings::SETTINGS;
        let Some(text) = self.settings_input.take() else {
            return;
        };
        let Some(item) = self.settings_item() else {
            return;
        };
        let text = text.trim();
        let Item::Setting(i) = item else {
            return self.view_commit(item, text);
        };
        let Some((name, _, _)) = SETTINGS.get(i) else {
            return;
        };
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
            "alert_error_calls" => match text.parse::<i64>() {
                Ok(n) if n >= 1 => self.write_setting("settings", name, Some(n.into())),
                _ => self.set_status("alert_error_calls is a whole number, 1 or more"),
            },
            "paste_debounce_ms" => match text.parse::<u64>() {
                Ok(n) if i64::try_from(n).is_ok() => {
                    self.write_setting("settings", name, Some((n as i64).into()))
                }
                _ => self.set_status("paste_debounce_ms is a whole number, 0 to turn it off"),
            },
            "idle_after" => match text.parse::<f64>() {
                Ok(h) if h.is_finite() && h > 0.0 => {
                    let value = match h.fract() == 0.0 && h < i64::MAX as f64 {
                        true => toml_edit::Value::from(h as i64),
                        false => toml_edit::Value::from(h),
                    };
                    self.write_setting("settings", name, Some(value))
                }
                _ => self.set_status("idle_after is a number of hours, more than 0"),
            },
            alert if alert.starts_with("alert_") => match text.parse::<f64>() {
                Ok(v) if v.is_finite() && v >= 0.0 && (alert != "alert_errors" || v <= 100.0) => {
                    // A whole number is written as one, so the file reads
                    // `alert_cost = 20` rather than `20.0`.
                    let value = match v.fract() == 0.0 && v < i64::MAX as f64 {
                        true => toml_edit::Value::from(v as i64),
                        false => toml_edit::Value::from(v),
                    };
                    self.write_setting("settings", name, Some(value))
                }
                _ => self.set_status(format!("{alert} is a number, 0 to turn it off")),
            },
            _ => self.write_setting("settings", name, Some(text.into())),
        }
    }

    /// Enter in a view choice's field, which is only ever a number.
    ///
    /// The bounds are the row's own and the message names them, because a
    /// figure quietly clamped to the range would leave somebody believing a
    /// cost floor of ten thousand was something other than the four they typed.
    fn view_commit(&mut self, item: Item, text: &str) {
        let Item::View(i) = item else {
            return;
        };
        let (name, _, view) = &VIEWS[i];
        let View::Number { range, set, .. } = view else {
            return;
        };
        if text.is_empty() {
            set(self, 0.0);
            return self.set_status(format!("{name} back to its default"));
        }
        // A dollar sign is what the row shows, so it is what gets typed back.
        let digits = text.trim_start_matches(['$', ' ']);
        match digits.parse::<f64>() {
            Ok(v) if v.is_finite() && (range.0..=range.1).contains(&v) => {
                set(self, v);
                self.set_status(format!("{name} = {v}"));
            }
            _ => self.set_status(format!("{name} is a number, {} to {}", range.0, range.1)),
        }
    }
}

/// How a value reads in a status line, where the empty string is the default
/// rather than nothing.
///
/// A choice list spells its default as `""` so that cycling lands on it, and
/// that is not a thing to say out loud.
fn said(value: &str) -> String {
    if value.is_empty() {
        "the default".to_string()
    } else {
        value.to_string()
    }
}
