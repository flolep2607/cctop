//! Anonymous usage telemetry: once a day, what this cctop is and which of its
//! parts ran, and where it crashed if it did.
//!
//! On by default and opt-out, but nothing is ever sent before the notice
//! ([`NOTICE`]) has been shown on an interactive run — the dashboard, or
//! `cctop serve` on a terminal — and recorded in `ui-prefs.json`. Off by
//! `[settings] telemetry = false` (which `cctop telemetry off` and the settings
//! page write), `CCTOP_TELEMETRY=0` or `DO_NOT_TRACK=1`.
//!
//! What a report holds is [`Report`], and nothing else: the version, the OS and
//! CPU type, a count per [`Feature`] and per agent kind, at most one crash, and
//! a random install id ([`ensure_id`]) so distinct installs and retention can
//! be counted. The id is 128 random bits and nothing else — derived from no
//! machine, user, hostname or MAC — made at the first report, replaced by
//! `cctop telemetry reset-id` and deleted by `cctop telemetry off`. A crash's message is [`scrub`]bed of anything that looks like a
//! path, URL, address, name or token before it is even written to disk.
//!
//! Never from `cctop hook` or `cctop yolo-hook`: both are answered and exit in
//! `main` before anything here is reached, including the panic hook.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// Where reports go: the release mirror's worker, `worker/src/index.js`.
pub const ENDPOINT: &str = "https://cctop-releases.ecorsiste.workers.dev/telemetry";

/// No more often than this.
const DAY: u64 = 24 * 60 * 60;

/// The background send gives up after this; nobody is waiting on it.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The longest crash message sent, after scrubbing.
const MESSAGE_MAX: usize = 200;

pub const NOTICE: &str = "\
cctop sends an anonymous usage report at most once a day: its version, OS and
CPU type, which of its features ran, how many agents of each kind it watched,
where it crashed if it did (file, line and a scrubbed message), and a random
ID you can reset (`cctop telemetry reset-id`). Never prompts, paths, session or
account names, hostnames or IPs. `cctop telemetry status` shows the next
report; `cctop telemetry off`, CCTOP_TELEMETRY=0 or DO_NOT_TRACK=1 turns it off.";

/// The parts of cctop a report counts runs of. The worker accepts these names
/// and no others, so a new one is added there too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// The dashboard.
    Tui,
    /// `cctop serve`, or the dashboard serving its table.
    Serve,
    /// A tab opened in the dashboard, or the web workspace page.
    Workspace,
    /// A Cloudflare tunnel in front of the server.
    Tunnel,
    /// A session seen with YOLO switched on.
    Yolo,
    /// An update installed.
    Update,
}

impl Feature {
    pub fn as_str(self) -> &'static str {
        match self {
            Feature::Tui => "tui",
            Feature::Serve => "serve",
            Feature::Workspace => "workspace",
            Feature::Tunnel => "tunnel",
            Feature::Yolo => "yolo",
            Feature::Update => "update",
        }
    }
}

/// What has been counted since the last report, kept on disk between runs.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Pending {
    last_sent: u64,
    features: BTreeMap<String, u64>,
    /// The most agents of each kind seen running at once.
    agents: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Crash {
    pub location: String,
    pub message: String,
}

/// Exactly what is sent: `cctop telemetry status` prints this.
#[derive(Debug, Serialize)]
pub struct Report {
    id: String,
    version: &'static str,
    os: &'static str,
    arch: &'static str,
    features: BTreeMap<String, u64>,
    agents: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash: Option<Crash>,
}

static PENDING_FILE: LazyLock<PathBuf> =
    LazyLock::new(|| crate::config::CACHE_DIR.join("telemetry.json"));
static CRASH_FILE: LazyLock<PathBuf> =
    LazyLock::new(|| crate::config::CACHE_DIR.join("telemetry-crash.json"));

/// Why telemetry is off, or `None` when it is on. `env` reads a variable, so a
/// test can answer without touching the process's environment.
fn off_because(
    env: impl Fn(&str) -> Option<String>,
    setting: Option<bool>,
) -> Option<&'static str> {
    let set = |name| env(name).filter(|v| !v.is_empty());
    if set("CCTOP_TELEMETRY").is_some_and(|v| matches!(v.as_str(), "0" | "false" | "off")) {
        return Some("CCTOP_TELEMETRY is set to off");
    }
    if set("DO_NOT_TRACK").is_some_and(|v| v != "0") {
        return Some("DO_NOT_TRACK is set");
    }
    if setting == Some(false) {
        return Some("turned off in config.toml (`cctop telemetry on` to undo)");
    }
    None
}

fn off_now() -> Option<&'static str> {
    off_because(
        |name| std::env::var(name).ok(),
        crate::settings::Settings::load().telemetry,
    )
}

/// Whether a report may go now: on, the notice shown, and a day since the last.
fn due(off: Option<&str>, notice_shown: bool, last_sent: u64, now: u64) -> bool {
    off.is_none() && notice_shown && now.saturating_sub(last_sent) >= DAY
}

fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn write<T: Serialize>(path: &Path, value: &T) {
    let _ = std::fs::create_dir_all(&*crate::config::CACHE_DIR);
    if let Ok(text) = serde_json::to_string(value) {
        let _ = std::fs::write(path, text);
    }
}

// ponytail: read-modify-write with no lock, so two cctops counting at the same
// instant can lose one count, and a count landing during a send is dropped.
// Fine for usage numbers; take a lock file if they ever have to be exact.
fn update_pending(change: impl FnOnce(&mut Pending)) {
    if crate::under_test() || off_now().is_some() {
        return;
    }
    let mut pending: Pending = read(&PENDING_FILE).unwrap_or_default();
    change(&mut pending);
    write(&PENDING_FILE, &pending);
}

/// Count one run of `feature` towards the next report.
pub fn record(feature: Feature) {
    update_pending(|p| *p.features.entry(feature.as_str().into()).or_default() += 1);
}

/// Running agents per kind in `sessions`, and whether any has YOLO on.
fn running(sessions: &[crate::session::Session]) -> (BTreeMap<String, u64>, bool) {
    let mut kinds = BTreeMap::new();
    for session in sessions.iter().filter(|s| s.is_running()) {
        *kinds
            .entry(session.provider.as_str().to_string())
            .or_default() += 1;
    }
    (kinds, sessions.iter().any(|s| s.yolo.is_some()))
}

/// Note the agents a face is watching. Called on every pass over the rows, so
/// it does the work at most once every ten minutes, and counts YOLO once per
/// process.
pub fn watched(sessions: &[crate::session::Session]) {
    use std::sync::Mutex;
    use std::time::Instant;
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    static YOLO_SEEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if crate::under_test() {
        return;
    }
    {
        let Ok(mut last) = LAST.lock() else { return };
        if last.is_some_and(|at| at.elapsed() < Duration::from_secs(600)) {
            return;
        }
        *last = Some(Instant::now());
    }
    let (kinds, yolo) = running(sessions);
    let first_yolo = yolo && !YOLO_SEEN.swap(true, std::sync::atomic::Ordering::Relaxed);
    update_pending(|p| {
        for (kind, n) in kinds {
            let most = p.agents.entry(kind).or_default();
            *most = (*most).max(n);
        }
        if first_yolo {
            *p.features.entry(Feature::Yolo.as_str().into()).or_default() += 1;
        }
    });
}

/// 128 random bits as 32 lowercase hex characters, from the system's random
/// source and nothing else about the machine.
fn new_id() -> String {
    let mut bytes = [0u8; 16];
    // ring's SystemRandom is getrandom(2); it does not fail on Linux.
    let _ = ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The install id kept in `prefs`, made the first time one is needed.
fn ensure_id(prefs: &mut crate::cache::UiPrefs) -> String {
    prefs.telemetry_id.get_or_insert_with(new_id).clone()
}

/// Replace the install id with a fresh one, and return it.
fn reset_id(prefs: &mut crate::cache::UiPrefs) -> String {
    prefs.telemetry_id = None;
    ensure_id(prefs)
}

fn report(pending: &Pending, crash: Option<Crash>, id: String) -> Report {
    Report {
        id,
        version: crate::update::current_version(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        features: pending.features.clone(),
        agents: pending.agents.clone(),
        crash,
    }
}

/// Show [`NOTICE`] once, on stderr, and remember that it was. For an
/// interactive run only; telemetry that is off has nothing to give notice of.
pub fn notice_once() {
    if crate::under_test() || off_now().is_some() {
        return;
    }
    let mut prefs = crate::cache::UiPrefs::load();
    if prefs.telemetry_notice_shown {
        return;
    }
    eprintln!("\n{NOTICE}\n");
    prefs.telemetry_notice_shown = true;
    prefs.save();
}

/// Send the day's report in the background, if one is due. Every failure is
/// silent: telemetry must never be why cctop misbehaves.
pub fn start() {
    if crate::under_test() {
        return;
    }
    std::thread::spawn(|| {
        let mut prefs = crate::cache::UiPrefs::load();
        let mut pending: Pending = read(&PENDING_FILE).unwrap_or_default();
        let now = crate::util::now_secs();
        if !due(
            off_now(),
            prefs.telemetry_notice_shown,
            pending.last_sent,
            now,
        ) {
            return;
        }
        // Saved only when new: this thread's copy of the prefs is the one the
        // dashboard is about to start changing.
        let fresh = prefs.telemetry_id.is_none();
        let id = ensure_id(&mut prefs);
        if fresh {
            prefs.save();
        }
        let crash = read(&CRASH_FILE);
        let body = report(&pending, crash, id);
        // The day is claimed and the counts cleared before sending, so a slow
        // or failed send is not retried until tomorrow and never sent twice.
        let _ = std::fs::remove_file(&*CRASH_FILE);
        pending = Pending {
            last_sent: now,
            ..Pending::default()
        };
        write(&PENDING_FILE, &pending);
        let _ = crate::update::agent()
            .post(ENDPOINT)
            .config()
            .timeout_global(Some(TIMEOUT))
            .build()
            .header("content-type", "application/json")
            .send(serde_json::to_string(&body).unwrap_or_default());
    });
}

/// Record a panic for the next report, then hand it to the hook that was there
/// before. No network here: a panicking process is no place to wait on one.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if !crate::under_test() && off_now().is_none() {
            let payload = info.payload();
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("");
            let location = info
                .location()
                .and_then(|l| own_location(l.file(), l.line()))
                .unwrap_or_else(|| "elsewhere".into());
            // ponytail: one crash per report, the latest; a count per location
            // if crashes ever turn out to be common enough to rank.
            write(
                &CRASH_FILE,
                &Crash {
                    location,
                    message: scrub(message),
                },
            );
        }
        previous(info);
    }));
}

/// A panic's file as a path inside cctop's own source, or `None` for anywhere
/// else. A workspace build names it relative to the workspace (`src/main.rs`,
/// `crates/ui/src/app.rs`); `cargo install` builds from the registry, which
/// names it absolutely, under a home directory, so it is cut to the crate.
fn own_location(file: &str, line: u32) -> Option<String> {
    let own = match file.starts_with("src/") || file.starts_with("crates/") {
        true => file.to_string(),
        false => {
            // `…/cctop-ui-0.28.0/src/app.rs` → `cctop-ui/src/app.rs`
            let rest = &file[file.rfind("/cctop-")? + 1..];
            let (dir, path) = rest.split_once('/')?;
            format!("{}/{path}", dir.rsplit_once('-')?.0)
        }
    };
    let plain = own
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'));
    (plain && !own.contains("..")).then(|| format!("{own}:{line}"))
}

/// `message` with anything that could identify a person or a machine taken
/// out, cut to [`MESSAGE_MAX`] characters.
///
// ponytail: patterns, not proof. A prompt quoted in a panic message keeps its
// plain words; this removes the shapes that identify, not everything private.
pub fn scrub(message: &str) -> String {
    use regex::Regex;
    static RULES: LazyLock<Vec<(Regex, &str)>> = LazyLock::new(|| {
        [
            (r"[A-Za-z][A-Za-z0-9+.-]*://\S+", "<url>"),
            (r"[\w.+-]+@[\w-]+(\.[\w-]+)+", "<email>"),
            // Any word with a slash in it: a path, a home dir, a ref.
            (
                r#"[^\s"'`()\[\]{}<>,;]*[/\\][^\s"'`()\[\]{}<>,;]*"#,
                "<path>",
            ),
            (r"~[\w.-]*", "<path>"),
            (r"\b\d{1,3}(\.\d{1,3}){3}\b", "<ip>"),
            (
                r"\b(?:[0-9A-Fa-f]{1,4})?(?:::?[0-9A-Fa-f]{1,4}){2,7}\b",
                "<ip>",
            ),
            // A name with two dots and a word at the end: a hostname.
            (
                r"\b[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+\.[A-Za-z]{2,}\b",
                "<host>",
            ),
            (r"\b[0-9A-Fa-f]{12,}\b", "<token>"),
        ]
        .into_iter()
        .map(|(pattern, with)| (Regex::new(pattern).expect("valid pattern"), with))
        .collect()
    });
    static TOKEN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[A-Za-z0-9_+=-]{20,}").expect("valid pattern"));
    let mut out = message.to_string();
    // This machine's own names first, wherever they turn up: the home dir as
    // written, the user and host names as whole words.
    if let Some(home) = dirs::home_dir().map(|h| h.to_string_lossy().into_owned())
        && home.len() > 1
    {
        out = out.replace(&home, "<path>");
    }
    let names = [
        std::env::var("USER").ok(),
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .ok()
            .map(|h| h.trim().to_string()),
    ];
    for name in names.into_iter().flatten().filter(|n| n.len() >= 3) {
        if let Ok(word) = Regex::new(&format!(r"\b{}\b", regex::escape(&name))) {
            out = word.replace_all(&out, "<name>").into_owned();
        }
    }
    for (rule, with) in RULES.iter() {
        out = rule.replace_all(&out, *with).into_owned();
    }
    // A long run with a digit in it is a key, an id or base64; a long
    // identifier without one is left alone.
    out = TOKEN
        .replace_all(&out, |c: &regex::Captures| {
            match c[0].bytes().any(|b| b.is_ascii_digit()) {
                true => "<token>".to_string(),
                false => c[0].to_string(),
            }
        })
        .into_owned();
    out.chars().take(MESSAGE_MAX).collect()
}

/// `cctop telemetry off|on|status|reset-id`.
pub fn run(argv: &[String]) -> i32 {
    let set = |on: bool| {
        crate::settings::write(
            &crate::config::CONFIG_FILE,
            "settings",
            "telemetry",
            Some(on.into()),
        )
    };
    match argv.first().map(String::as_str) {
        Some("off") => match set(false) {
            Ok(()) => {
                // What was counted goes too: off means nothing leaves later.
                let _ = std::fs::remove_file(&*PENDING_FILE);
                let _ = std::fs::remove_file(&*CRASH_FILE);
                // And the id, so turning it back on starts a stranger.
                let mut prefs = crate::cache::UiPrefs::load();
                prefs.telemetry_id = None;
                prefs.save();
                println!("Telemetry is off. Nothing will be sent, and the install id is deleted.");
                0
            }
            Err(e) => {
                eprintln!("cctop: {e:#}");
                1
            }
        },
        Some("on") => match set(true) {
            Ok(()) => {
                println!("Telemetry is on.\n\n{NOTICE}");
                let mut prefs = crate::cache::UiPrefs::load();
                prefs.telemetry_notice_shown = true;
                prefs.save();
                0
            }
            Err(e) => {
                eprintln!("cctop: {e:#}");
                1
            }
        },
        Some("status") | None => {
            match off_now() {
                Some(why) => println!("Telemetry is off: {why}."),
                None if !crate::cache::UiPrefs::load().telemetry_notice_shown => println!(
                    "Telemetry is on, but nothing is sent until its notice has been shown \
                     on an interactive run."
                ),
                None => println!("Telemetry is on."),
            }
            let pending: Pending = read(&PENDING_FILE).unwrap_or_default();
            let id = crate::cache::UiPrefs::load()
                .telemetry_id
                .unwrap_or_else(|| "(a new random id, made when the first report is sent)".into());
            let body = report(&pending, read(&CRASH_FILE), id);
            println!(
                "\nThe next report, at most once a day, to {ENDPOINT}:\n{}",
                serde_json::to_string_pretty(&body).unwrap_or_default()
            );
            0
        }
        Some("reset-id") => {
            let mut prefs = crate::cache::UiPrefs::load();
            let id = reset_id(&mut prefs);
            prefs.save();
            println!("The install id is now {id}.");
            0
        }
        Some(other) => {
            eprintln!("cctop telemetry: unknown '{other}'; use off, on, status or reset-id");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn scrubs_paths_urls_names_and_tokens() {
        let home = dirs::home_dir().unwrap().to_string_lossy().into_owned();
        let said = format!(
            "could not open {home}/work/secret.rs: see https://example.com/x?a=1, \
             mail bob@corp.io, key sk-ant-abc123def456ghi789jkl, \
             id 0123456789abcdef0123, host build.corp.example.com at 10.0.0.12, \
             also ~/notes and ./rel/file.txt"
        );
        let clean = scrub(&said);
        for leak in [
            home.as_str(),
            "secret.rs",
            "example.com",
            "bob@",
            "sk-ant",
            "0123456789abcdef",
            "corp.example",
            "10.0.0.12",
            "notes",
            "rel/file",
        ] {
            assert!(!clean.contains(leak), "{leak} left in {clean}");
        }
        assert!(clean.starts_with("could not open <"), "{clean}");
        assert!(
            clean.contains("<url>") && clean.contains("<token>"),
            "{clean}"
        );
    }

    #[test]
    fn scrub_keeps_plain_words_and_truncates() {
        assert_eq!(
            scrub("called `Option::unwrap()` on a `None` value"),
            "called `Option::unwrap()` on a `None` value"
        );
        assert_eq!(scrub(&"word ".repeat(100)).chars().count(), MESSAGE_MAX);
    }

    #[test]
    fn a_location_is_only_ever_inside_cctop() {
        assert_eq!(
            own_location("crates/ui/src/app.rs", 12).as_deref(),
            Some("crates/ui/src/app.rs:12")
        );
        assert_eq!(
            own_location(
                "/home/someone/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cctop-ui-0.28.0/src/app.rs",
                3
            )
            .as_deref(),
            Some("cctop-ui/src/app.rs:3")
        );
        assert_eq!(
            own_location(
                "/home/someone/.cargo/registry/src/x/serde-1.0.0/src/de.rs",
                1
            ),
            None
        );
        assert_eq!(
            own_location("/rustc/abc/library/core/src/option.rs", 1),
            None
        );
    }

    #[test]
    fn nothing_is_sent_before_the_notice() {
        assert!(!due(None, false, 0, 10 * DAY));
        assert!(due(None, true, 0, 10 * DAY));
    }

    #[test]
    fn every_off_switch_stops_it() {
        assert_eq!(off_because(env(&[]), None), None);
        assert_eq!(off_because(env(&[]), Some(true)), None);
        assert!(off_because(env(&[]), Some(false)).is_some());
        assert!(off_because(env(&[("CCTOP_TELEMETRY", "0")]), None).is_some());
        assert!(off_because(env(&[("DO_NOT_TRACK", "1")]), Some(true)).is_some());
        assert_eq!(off_because(env(&[("DO_NOT_TRACK", "0")]), None), None);
        let off = off_because(env(&[("DO_NOT_TRACK", "1")]), None);
        assert!(!due(off, true, 0, 10 * DAY));
    }

    #[test]
    fn at_most_once_a_day() {
        let sent = 100 * DAY;
        assert!(!due(None, true, sent, sent + DAY - 1));
        assert!(due(None, true, sent, sent + DAY));
    }

    #[test]
    fn under_test_nothing_is_recorded_or_started() {
        record(Feature::Tui);
        start();
        assert!(!PENDING_FILE.exists());
    }

    #[test]
    fn a_report_carries_only_its_fields() {
        let mut pending = Pending::default();
        pending.features.insert("tui".into(), 2);
        pending.agents.insert("claude".into(), 3);
        let json = serde_json::to_value(report(&pending, None, new_id())).unwrap();
        let keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["id", "version", "os", "arch", "features", "agents"]);
        assert_eq!(json["features"]["tui"], 2);
    }

    #[test]
    fn the_id_is_stable_until_reset() {
        let mut prefs = crate::cache::UiPrefs::default();
        let first = ensure_id(&mut prefs);
        assert_eq!(first.len(), 32);
        assert!(
            first
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        let report_id = |prefs: &mut crate::cache::UiPrefs| {
            report(&Pending::default(), None, ensure_id(prefs)).id
        };
        assert_eq!(report_id(&mut prefs), first);
        assert_eq!(report_id(&mut prefs), first);
        let reset = reset_id(&mut prefs);
        assert_ne!(reset, first);
        assert_eq!(report_id(&mut prefs), reset);
    }
}
