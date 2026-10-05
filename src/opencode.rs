//! Which OpenCode is installed, and what that changes for cctop.
//!
//! OpenCode 2 is not OpenCode 1 with more features. Its plugin API was replaced
//! outright, and its terminal client grew a tab strip of its own. The plugin is
//! written to work on both — see [`crate::hook`] — so what is left here is the
//! two things that genuinely differ: the tab strip cctop turns off, and telling
//! a plugin file written for one version from one written for the other.
//!
//! The answer comes from the binary rather than from configuration, because
//! nothing in a config file records it. One `opencode` on `PATH` serves every
//! project on the machine, and the file cctop writes has to match the one that
//! will read it — including after an upgrade, which is exactly when a plugin
//! written for the old one stops being read.

use std::path::Path;
use std::sync::LazyLock;

/// Which OpenCode is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    /// OpenCode 1.x. A plugin is a function that returns hooks, an event's
    /// payload is under `properties`, and the terminal client draws no tabs of
    /// its own for cctop to argue with.
    V1,
    /// OpenCode 2.x. A plugin default-exports a definition with an `id` and a
    /// `setup`, an event's payload is under `data`, and the client keeps a
    /// session tab strip.
    V2,
}

/// The binary names OpenCode ships under. The npm distribution installs as
/// `opencode-cli` and the installer puts `opencode` on `PATH`, and cctop has
/// always known both spellings — see [`crate::proc`].
const NAMES: [&str; 2] = ["opencode", "opencode-cli"];

/// The API of the OpenCode this machine has, asked once per process.
///
/// Once because it costs a process spawn and cannot change under a running
/// cctop: an upgrade between two reads of the same installation is not a case
/// worth handling, and a status panel that re-ran it per line would be paying
/// for it on every frame.
static API: LazyLock<Api> = LazyLock::new(detect);

/// The API of the installed OpenCode.
pub fn api() -> Api {
    *API
}

/// Ask the binary. Anything that goes wrong answers [`Api::V1`], which is the
/// plugin cctop wrote before it knew the question existed — a conservative
/// answer for the case where there is no OpenCode to ask, where nothing loads
/// the file either way.
fn detect() -> Api {
    for name in NAMES {
        let Ok(out) = std::process::Command::new(name).arg("--version").output() else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(major) = major(&text) {
            return if major >= 2 { Api::V2 } else { Api::V1 };
        }
    }
    Api::V1
}

/// The major version in `text`, or `None` if it holds no version at all.
///
/// The first number that starts a dotted run, so it does not matter whether the
/// version is printed bare (`1.14.47`), prefixed (`opencode v2.0.20`), or
/// tagged (`opencode/2.0.20-beta.3`) — the tag is split off before the version
/// is read rather than being a case of its own.
fn major(text: &str) -> Option<u32> {
    text.split(|c: char| !c.is_ascii_digit() && c != '.')
        .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))
        .and_then(|word| word.split('.').next())
        .and_then(|part| part.parse().ok())
}

/// OpenCode 2's inline CLI settings, merged over the global `cli.json`. The
/// documented way to change a setting for one run rather than for the user, and
/// the only one that leaves `cli.json` — which belongs to them — alone.
const CLI_CONFIG_ENV: &str = "OPENCODE_CLI_CONFIG_CONTENT";

/// The variables an agent cctop launches needs that it would not otherwise
/// inherit, for the OpenCode that is installed.
///
/// Empty for every agent but OpenCode 2, and for OpenCode 1, whose client has no
/// tab strip.
pub fn launch_env(argv: &[String]) -> Vec<(String, String)> {
    launch_env_for(api(), argv)
}

/// [`launch_env`] for a known [`Api`], so the tests need not have an OpenCode
/// of a particular vintage installed to ask what a given one would be given.
fn launch_env_for(api: Api, argv: &[String]) -> Vec<(String, String)> {
    if api != Api::V2 || !argv.first().is_some_and(|word| is_opencode(word)) {
        return Vec::new();
    }
    vec![(CLI_CONFIG_ENV.to_string(), tabs_off())]
}

/// Whether `word` is a command that runs OpenCode, however it was spelled on the
/// way here: a bare name, an absolute path from an install script, or the npm
/// distribution's `opencode-cli`.
fn is_opencode(word: &str) -> bool {
    let name = Path::new(word)
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    NAMES.contains(&name)
}

/// cctop's tabs, turned off in the tabs OpenCode keeps for itself.
///
/// The strip is OpenCode's own and cctop's are the real ones: a tab in a tab
/// bar is two ways to say which session you meant, and the one inside the pane
/// is the one that cannot be shared or reordered from the dashboard. Only this
/// run is changed — somebody who wants their tabs back has them back by typing
/// `opencode` themselves.
fn tabs_off() -> String {
    tabs_off_from(&|key| std::env::var(key).ok())
}

/// [`tabs_off`] with the environment handed in rather than read.
///
/// A test that wants somebody else's inline settings must not put them in the
/// process environment to get them: `setenv` beside another thread's `getenv`
/// is undefined behaviour, and the runner has a thread per test.
fn tabs_off_from(env: &dyn Fn(&str) -> Option<String>) -> String {
    let mut root = env(CLI_CONFIG_ENV)
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
    if let Some(object) = root.as_object_mut() {
        // An inline `tabs` of the user's own is kept apart from the one setting
        // being changed, and the pre-2.0 spelling goes rather than competing
        // with `mode` for which one wins.
        let mut tabs = object
            .remove("tabs")
            .filter(|tabs| tabs.is_object())
            .and_then(|tabs| tabs.as_object().cloned())
            .unwrap_or_default();
        tabs.remove("enabled");
        tabs.insert("mode".into(), "off".into());
        object.insert("tabs".into(), serde_json::Value::Object(tabs));
    }
    serde_json::to_string(&root).unwrap_or_else(|_| TABS_OFF.into())
}

/// What [`tabs_off`] writes when there is nothing of the user's to merge with.
const TABS_OFF: &str = r#"{"tabs":{"mode":"off"}}"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_is_read_out_of_whatever_shape_it_is_printed_in() {
        assert_eq!(major("opencode v2.0.20"), Some(2));
        assert_eq!(major("1.14.47"), Some(1));
        assert_eq!(major("opencode/2.0.20-beta.3"), Some(2));
        assert_eq!(major("2.1"), Some(2));
        // A word with digits in it that is not a version: the dotted run has to
        // start one for this to read anything at all.
        assert_eq!(major("command not found"), None);
        assert_eq!(major(""), None);
    }

    #[test]
    fn opencode_is_recognised_however_it_was_spelled() {
        assert!(is_opencode("opencode"));
        assert!(is_opencode("opencode-cli"));
        assert!(is_opencode("/home/flo/.opencode/bin/opencode"));
        assert!(!is_opencode("claude"));
        assert!(!is_opencode("/usr/bin/opencode-helper"));
        assert!(!is_opencode(""));
    }

    /// The point of the whole exercise: cctop's tabs are already the ones that
    /// matter, and OpenCode 2's are turned off for the run cctop launches —
    /// without touching `cli.json`, which is the user's.
    #[test]
    fn opencode_two_is_launched_with_its_own_tabs_off() {
        let env = launch_env_for(
            Api::V2,
            &[
                "opencode".to_string(),
                "--session".to_string(),
                "ses_1".to_string(),
            ],
        );
        assert_eq!(
            env,
            vec![(CLI_CONFIG_ENV.to_string(), TABS_OFF.to_string())]
        );
    }

    #[test]
    fn no_other_agent_and_no_opencode_one_is_dressed() {
        assert!(launch_env_for(Api::V2, &["claude".to_string()]).is_empty());
        assert!(launch_env_for(Api::V1, &["opencode".to_string()]).is_empty());
        assert!(launch_env_for(Api::V2, &[]).is_empty());
    }

    /// Somebody who launches their own OpenCode with settings in the
    /// environment keeps them: inline settings are merged, and only the tabs are
    /// touched.
    #[test]
    fn an_inline_tab_setting_of_the_users_own_survives() {
        let existing = r#"{"session":{"scrollbar":false},"tabs":{"layout":"vertical"}}"#;
        let merged: serde_json::Value =
            serde_json::from_str(&tabs_off_from(&|key| (key == CLI_CONFIG_ENV).then(|| existing.to_string())))
                .unwrap();
        assert_eq!(merged["tabs"]["mode"], "off");
        assert_eq!(merged["tabs"]["layout"], "vertical");
        assert_eq!(merged["session"]["scrollbar"], false);
    }

    /// The pre-2.0 spelling is read as the same setting, so leaving it beside
    /// `mode` would leave the file saying both on and off.
    #[test]
    fn the_legacy_boolean_gives_way_to_the_mode_it_became() {
        let existing = r#"{"tabs":{"enabled":true}}"#;
        let merged: serde_json::Value =
            serde_json::from_str(&tabs_off_from(&|key| (key == CLI_CONFIG_ENV).then(|| existing.to_string())))
                .unwrap();
        assert_eq!(merged["tabs"]["mode"], "off");
        assert!(merged["tabs"].get("enabled").is_none());
    }

    /// An inline value that is not JSON is not something to fail a launch over.
    #[test]
    fn unreadable_inline_settings_are_replaced_rather_than_kept() {
        let written = tabs_off_from(&|key| (key == CLI_CONFIG_ENV).then(|| "{not json".to_string()));
        assert_eq!(written, TABS_OFF);
    }
}
