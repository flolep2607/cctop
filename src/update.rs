//! Self-update against the project's GitHub releases.
//!
//! A downloaded binary has no package manager behind it, so without this it
//! stays on whatever version it was fetched at. Installs that *do* have a
//! package manager (`cargo install`, a distro package) must not be overwritten
//! behind the manager's back — that refusal is absolute and is checked before
//! anything else.
//!
//! For an install cctop does own, it updates itself as the interactive UI
//! starts, when the hourly check has *already* found a newer version. That is a
//! change of position: this module used to replace the binary only on an
//! explicit `--update`, on the grounds that a program should not swap itself out
//! from under someone. What moved the argument is where the swap happens. At
//! startup there is no pty open, no agent hosted and no pane to lose, so the new
//! binary can be exec'd in place of the old one and the session that follows is
//! simply the new version — which is not true of any later moment, and is why
//! this is a startup-only path and not a background one.
//!
//! It stays out of the way of everything else: no non-interactive mode reaches
//! it, the version it acts on is the cached one so nothing is waited for to
//! discover it, and every failure falls through to launching the version already
//! installed. `--no-auto-update` skips it for one run, and turning it off in the
//! UI's preferences skips it for good.

use crate::config;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

const RELEASES_URL: &str = "https://api.github.com/repos/flolep2607/cctop/releases/latest";
/// Every release, newest first, for the notes an update prints.
///
/// A page of thirty covers any jump anyone will make in practice — cctop has not
/// published thirty versions in the life of an install that still runs — and
/// asking for more would cost a second request to say the same thing.
const RELEASE_LIST_URL: &str = "https://api.github.com/repos/flolep2607/cctop/releases?per_page=30";
/// GitHub rejects API requests without one.
const USER_AGENT: &str = concat!("cctop/", env!("CARGO_PKG_VERSION"));
/// How long a release check stays good for.
///
/// An hour, which is far cheaper than it sounds: the check is one unauthenticated
/// call to GitHub's releases API, the cache file lives in `CACHE_DIR` and is
/// shared by every cctop on the machine, and unauthenticated GitHub allows 60
/// requests an hour per IP — so this spends about 2% of that budget. Hourly is
/// the difference between hearing about a release the day after it lands and
/// hearing about it while it is still the thing that was just fixed.
const CHECK_MAX_AGE_SECS: u64 = 60 * 60;

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// The release archive built for the running platform.
///
/// Releases are cut for a fixed set of targets, so this maps to one of those
/// rather than reporting the exact triple the binary was compiled for: a
/// `linux-gnu` build is served the static musl archive, which runs anywhere.
fn asset_target() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => return None,
    })
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    assets: Vec<Asset>,
    /// The notes GitHub holds for the release. Absent on a release published
    /// without any, which is why this is not simply a `String`.
    #[serde(default)]
    body: Option<String>,
}

impl Release {
    /// The version this release is of, without the tag's `v`.
    fn version(&self) -> &str {
        self.tag_name.trim_start_matches('v')
    }
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Serialize, Deserialize)]
struct CheckCache {
    checked_at: u64,
    latest: String,
}

fn unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .user_agent(USER_AGENT)
        .build()
        .into()
}

/// Compare dotted numeric versions. Anything unparseable sorts as zero, so a
/// malformed tag can never masquerade as an upgrade.
fn is_newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            // Drop any pre-release or build suffix before comparing.
            .split(['-', '+'])
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    }
    let (a, b) = (parts(candidate), parts(current));
    let len = a.len().max(b.len());
    for i in 0..len {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

fn fetch_latest() -> Result<Release> {
    let text = agent()
        .get(RELEASES_URL)
        .call()
        .context("could not reach GitHub")?
        .body_mut()
        .read_to_string()
        .context("could not read the release response")?;
    serde_json::from_str(&text).context("could not parse the release response")
}

/// Newest published version, refreshed at most once an hour.
///
/// Returns the cached answer without touching the network when it is fresh, so
/// this is cheap to call on every start. Failures are silent: a monitor that
/// cannot reach GitHub should still run.
pub fn cached_latest_version() -> Option<String> {
    let path = config::CACHE_DIR.join("update-check.json");
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(cache) = serde_json::from_str::<CheckCache>(&text)
        && unix_secs().saturating_sub(cache.checked_at) < CHECK_MAX_AGE_SECS
    {
        return Some(cache.latest);
    }

    let latest = fetch_latest()
        .ok()?
        .tag_name
        .trim_start_matches('v')
        .to_string();
    let _ = std::fs::create_dir_all(&*config::CACHE_DIR);
    if let Ok(text) = serde_json::to_string(&CheckCache {
        checked_at: unix_secs(),
        latest: latest.clone(),
    }) {
        let _ = std::fs::write(&path, text);
    }
    Some(latest)
}

/// The newer version available, or `None` when already current.
pub fn available_update() -> Option<String> {
    let latest = cached_latest_version()?;
    is_newer(&latest, current_version()).then_some(latest)
}

/// Pull the `cctop` executable out of a release archive.
///
/// Only the executable is taken, and only by exact file name: an archive is
/// attacker-controlled input in the general case, and honouring arbitrary paths
/// inside one is how extraction escapes its destination directory.
fn unpack(archive: &[u8], target: &str, into: &Path) -> Result<PathBuf> {
    // Both the container format and the executable's name are properties of the
    // archive's target, not of the host reading it. Deriving the name from
    // `cfg!(windows)` instead made them disagree whenever the two differ.
    let binary_name = if target.contains("windows") {
        "cctop.exe"
    } else {
        "cctop"
    };
    let out = into.join(binary_name);

    if target.contains("windows") {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
            .context("release archive is not a valid zip")?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let is_binary = Path::new(entry.name())
                .file_name()
                .is_some_and(|n| n == binary_name);
            if is_binary {
                let mut file = std::fs::File::create(&out)?;
                std::io::copy(&mut entry, &mut file)?;
                return Ok(out);
            }
        }
    } else {
        let decoder = flate2::read::GzDecoder::new(archive);
        let mut tar = tar::Archive::new(decoder);
        for entry in tar
            .entries()
            .context("release archive is not a valid tar")?
        {
            let mut entry = entry?;
            let is_binary = entry.path()?.file_name().is_some_and(|n| n == binary_name);
            if is_binary {
                let mut file = std::fs::File::create(&out)?;
                std::io::copy(&mut entry, &mut file)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755))?;
                }
                return Ok(out);
            }
        }
    }
    bail!("the release archive contains no {binary_name}")
}

/// Replace the running executable with the newest release.
///
/// Integrity rests on the TLS connection to github.com. The published `.sha256`
/// sidecars are served from that same origin, so verifying against them would
/// only catch corruption that TLS already rules out — it would not defend
/// against a compromised release.
/// The releases between the version being left behind and the one arriving.
///
/// Ascending, because the reader is walking forward in time: someone four
/// versions behind wants to read what happened in order, not newest-first the
/// way the API returns it. `from` is exclusive and `to` inclusive — the version
/// you were on is not news, and the one you just got is the whole point.
fn selected<'a>(releases: &'a [Release], from: &str, to: &str) -> Vec<&'a Release> {
    let mut picked: Vec<&Release> = releases
        .iter()
        .filter(|r| is_newer(r.version(), from))
        .filter(|r| !is_newer(r.version(), to))
        .collect();
    picked.sort_by(|a, b| match is_newer(a.version(), b.version()) {
        true => std::cmp::Ordering::Greater,
        false => std::cmp::Ordering::Less,
    });
    picked
}

/// A release body reduced to lines worth printing in a terminal.
///
/// GitHub's generated notes are markdown written for a web page: a `What's
/// Changed` heading, one bullet per pull request with its author and URL
/// trailing behind the title, and a compare link at the end. The title is the
/// part that says what changed; the rest is chrome that costs a terminal three
/// lines to say nothing. Prose bodies — a release that wrote its own notes —
/// come through as their own paragraphs, since there is nothing to strip.
fn bullets(body: &str) -> Vec<String> {
    let mut items: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in body.lines().map(str::trim) {
        // A blank line ends whatever was being collected: it is the only thing
        // in markdown that reliably separates one thought from the next.
        if line.is_empty() {
            items.extend(current.take());
            continue;
        }
        // Headings and the trailing compare link: structure for a page that has
        // other things on it, and this listing is already under a version.
        if line.starts_with('#') || line.starts_with("**Full Changelog**") {
            items.extend(current.take());
            continue;
        }
        match line.starts_with(['*', '-', '•']) {
            true => {
                items.extend(current.take());
                current = Some(line.trim_start_matches(['*', '-', '•', ' ']).to_string());
            }
            // A line that is not a bullet continues the one before it. Release
            // prose is hard-wrapped in the commit it came from, so without this
            // a paragraph arrives as one bullet per line of the file it was
            // typed into.
            false => match current.as_mut() {
                Some(item) => {
                    item.push(' ');
                    item.push_str(line);
                }
                None => current = Some(line.to_string()),
            },
        }
    }
    items.extend(current);
    items
        .into_iter()
        // "…title by @someone in https://github.com/…/pull/6" — the attribution
        // is on the release page for anyone who wants it.
        .map(|item| {
            item.split(" by @")
                .next()
                .unwrap_or(&item)
                .trim()
                .to_string()
        })
        .map(|item| item.replace("**", ""))
        // "release 0.7.3: what it fixed" — the version is the heading this line
        // is printed under, so saying it again spends the width twice.
        .map(|item| strip_release_prefix(&item))
        .filter(|item| !item.is_empty())
        .collect()
}

/// `release 0.7.3: the rest`, or `0.7.0: the rest`, without the part the heading
/// above it already said.
///
/// Both shapes appear in this project's own history, because the line comes from
/// a pull request title and those were written for a list that had no version
/// beside them. Stripped only when what is left still says something: a note
/// whose whole text is the version has nothing else to give.
fn strip_release_prefix(item: &str) -> String {
    let candidate = item.strip_prefix("release ").unwrap_or(item);
    let Some((version, rest)) = candidate.split_once(": ") else {
        return item.to_string();
    };
    let numeric = !version.is_empty()
        && version
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == 'v');
    match numeric && !rest.trim().is_empty() {
        true => rest.trim().to_string(),
        false => item.to_string(),
    }
}

/// `text` broken into lines that fit `width`, for printing under an indent.
///
/// Release prose arrives as a paragraph once the lines it was typed on have been
/// joined back together, and a paragraph printed into a terminal that wraps it
/// itself loses the indent on every line but the first.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        // A word longer than the whole width goes on its own line rather than
        // being broken: a URL is more useful whole than tidy.
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Print what changed between `from` and `to`.
///
/// Best effort by design, and called only after the binary has already been
/// replaced: the update has succeeded by this point, and failing it over notes
/// that are a nicety would be absurd. When they cannot be had, the release page
/// is one line and one click away instead.
fn show_changes(from: &str, to: &str) -> bool {
    if !is_newer(to, from) {
        return false;
    }
    let notes = fetch_release_list().map(|list| {
        selected(&list, from, to)
            .into_iter()
            .map(|r| {
                (
                    r.version().to_string(),
                    bullets(r.body.as_deref().unwrap_or_default()),
                )
            })
            .collect::<Vec<_>>()
    });
    let Ok(notes) = notes else {
        println!("Release notes: {RELEASE_PAGE}");
        return false;
    };
    if notes.iter().all(|(_, lines)| lines.is_empty()) {
        println!("Release notes: {RELEASE_PAGE}");
        return false;
    }
    // The terminal's width less the deepest indent these lines are printed at.
    let room = crossterm::terminal::size()
        .map(|(cols, _)| usize::from(cols))
        .unwrap_or(80)
        .clamp(40, 100)
        .saturating_sub(6);
    println!();
    println!("What changed since {from}:");
    for (version, lines) in &notes {
        // A version whose release carried no notes still earns its heading:
        // silence about it would read as though the jump skipped it.
        println!();
        println!("  {version}");
        match lines.is_empty() {
            true => println!("    (no notes published)"),
            false => {
                for line in lines {
                    let mut wrapped = wrap(line, room).into_iter();
                    if let Some(first) = wrapped.next() {
                        println!("    - {first}");
                    }
                    // Hanging indent, so a wrapped item still reads as one.
                    for rest in wrapped {
                        println!("      {rest}");
                    }
                }
            }
        }
    }
    println!();
    println!("Full notes: {RELEASE_PAGE}");
    true
}

/// Where the notes live in full, for anything this cannot summarise.
const RELEASE_PAGE: &str = "https://github.com/flolep2607/cctop/releases";

fn fetch_release_list() -> Result<Vec<Release>> {
    let text = agent()
        .get(RELEASE_LIST_URL)
        .call()
        .context("could not reach GitHub")?
        .body_mut()
        .read_to_string()
        .context("could not read the release list")?;
    serde_json::from_str(&text).context("could not parse the release list")
}

pub fn run(force: bool) -> Result<()> {
    let current = current_version();
    // Before the network and before `force`, because this is not a check that a
    // newer release or a determined user can settle: the objection is to cctop
    // replacing the file at all, and it holds whatever the versions say.
    if managed_by_cargo() {
        return Err(cargo_managed());
    }
    let target =
        asset_target().ok_or_else(|| anyhow!("no release is published for this platform"))?;

    println!("Current version {current}; checking for updates…");
    let release = fetch_latest()?;
    let latest = release.tag_name.trim_start_matches('v');

    if !is_newer(latest, current) && !force {
        println!("Already on the newest version ({current}).");
        return Ok(());
    }

    install(&release, target, current).map(|_| ())
}

/// Fetch the archive for `target` and put it in place of the running binary.
///
/// Shared by `--update` and the startup path, which differ only in how they
/// decided to be here.
fn install(release: &Release, target: &str, current: &str) -> Result<bool> {
    let latest = release.version();
    let asset = release
        .assets
        .iter()
        // The checksum sidecars share the archive's prefix, so match on the
        // archive extensions rather than on the target alone.
        .find(|a| {
            a.name.contains(target) && (a.name.ends_with(".tar.gz") || a.name.ends_with(".zip"))
        })
        .ok_or_else(|| anyhow!("release {latest} has no archive for {target}"))?;

    // Claim the staging directory before downloading: whether the new binary can
    // be put in place is a permission question with an answer already available,
    // and finding out afterwards means having spent the download for nothing.
    let staging = staging_dir()?;

    println!("Downloading {}…", asset.name);
    let mut body = Vec::new();
    agent()
        .get(&asset.browser_download_url)
        .call()
        .context("could not download the release archive")?
        .body_mut()
        .as_reader()
        .read_to_end(&mut body)
        .context("could not read the release archive")?;

    let new_binary = unpack(&body, target, staging.path())?;
    self_replace::self_replace(&new_binary).context("could not replace the running executable")?;

    println!("Updated {current} -> {latest}.");
    // After the replace, so an install that worked is never held up by the
    // network call that only decorates it.
    Ok(show_changes(current, latest))
}

/// Set on the process that replaces this one, so the new binary knows it has
/// just arrived and does not go looking for another update.
///
/// The version comparison would stop it anyway — the cache names a release the
/// new binary now *is* — but a self-replacing program that re-execs itself is
/// one bad comparison away from doing it forever, and the guard costs a string.
const JUST_UPDATED: &str = "CCTOP_JUST_UPDATED";

/// Whether this process is the one that a startup update exec'd into.
pub fn just_updated() -> bool {
    std::env::var_os(JUST_UPDATED).is_some()
}

/// The version to update to at startup, if any.
///
/// Split out so the order of the refusals is a thing that can be tested rather
/// than a thing that can be reordered: a managed install must be refused whatever
/// the cache says, and being asked not to has to be honoured before either is
/// consulted.
fn wanted(
    enabled: bool,
    just_updated: bool,
    managed: bool,
    latest: Option<String>,
) -> Option<String> {
    if !enabled || just_updated || managed {
        return None;
    }
    latest
}

/// How the new binary is started: the same arguments this run was given, plus
/// the marker that stops it updating again.
///
/// The arguments matter more than they look. A `cctop --host box --plan max` that
/// came back as a bare `cctop` would be a different program than the one asked
/// for, and the user would have no reason to connect the difference to an update
/// they never asked about.
fn relaunch_command(
    exe: std::path::PathBuf,
    args: Vec<std::ffi::OsString>,
) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command.args(args).env(JUST_UPDATED, "1");
    command
}

/// Update before the UI opens, if the check already knows there is something to
/// update to.
///
/// Returns only when the run should continue as the version already installed —
/// on success it does not return at all, having exec'd the new binary in place
/// of this process. See the module docs for why startup is the one moment that
/// is safe to do that.
///
/// Every reason not to act is silent. This is a convenience on the way to
/// something the user actually asked for, and a paragraph about a release that
/// could not be fetched, printed before every launch, would be worse than the
/// old version they are about to keep using.
pub fn auto_at_startup(enabled: bool) {
    // The cached answer alone: this is the whole point of doing it here. The
    // hourly check has already been paid for, so nothing is waited on to learn
    // that there is a newer version — only to fetch it.
    let Some(latest) = wanted(
        enabled,
        just_updated(),
        managed_by_cargo(),
        available_update(),
    ) else {
        return;
    };
    // An install cctop cannot write to is not worth a download, and finding out
    // costs nothing: it is the same question `--update` asks before spending one.
    if staging_dir().is_err() {
        return;
    }
    let Some(target) = asset_target() else {
        return;
    };
    let current = current_version();
    println!("cctop {latest} is out; updating from {current} before starting…");
    let updated = fetch_latest().and_then(|release| {
        match is_newer(release.version(), current) {
            // The cache was stale in the direction that matters: it named a
            // release that has since been replaced by the very version running.
            false => Ok(false),
            true => install(&release, target, current),
        }
    });
    let Ok(showed_notes) = updated else {
        println!("Could not update just now; starting {current} instead.");
        return;
    };
    // The UI is about to take the alternate screen, which puts everything above
    // on the other side of a curtain until cctop exits. Notes nobody gets to
    // read are not notes, and this is the one moment they are what the user is
    // looking at — so it costs them a keypress, once per release.
    if showed_notes {
        print!("Press Enter to start cctop… ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let _ = std::io::stdin().read_line(&mut String::new());
    }
    relaunch();
}

/// Start the new binary in place of this process.
///
/// Nothing of this run is worth keeping — the UI has not opened, no pty is held,
/// no agent has been hosted — so the cleanest thing is to become the new version
/// and let it start normally. Falls through to running the old image in memory if
/// even that fails, which is the same outcome as never having tried.
fn relaunch() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut command = relaunch_command(exe, std::env::args_os().skip(1).collect());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Replaces this process outright, so there is no wrapper left holding a
        // terminal that two programs then both believe they own.
        let error = command.exec();
        println!("Could not start the new version ({error}); continuing.");
    }
    #[cfg(not(unix))]
    {
        // No exec on Windows, so the old process stays as a parent and exits
        // with whatever the new one reports.
        match command.status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(0)),
            Err(error) => println!("Could not start the new version ({error}); continuing."),
        }
    }
}

/// How to retry with the privileges this platform needs.
#[cfg(unix)]
const ELEVATE: &str = "re-run it as `sudo cctop --update`";
#[cfg(not(unix))]
const ELEVATE: &str = "re-run `cctop --update` from an elevated prompt";

/// A scratch directory beside the running binary, to stage the replacement in.
///
/// Replacing an executable is a rename and a rename cannot cross a filesystem
/// boundary, so this has to sit next to the current binary rather than in a temp
/// dir — which makes it a permission question wherever that binary lives.
fn staging_dir() -> Result<tempfile::TempDir> {
    let exe = std::env::current_exe().context("could not locate the running executable")?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow!("the running executable has no parent directory"))?;
    match raw_stage_in(dir) {
        Ok(staged) => Ok(staged),
        // The one failure the user can do something about without going back to
        // the shell, so it is worth handling rather than only reporting.
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Err(elevate(dir)),
        Err(error) => Err(anyhow::Error::new(error)
            .context(format!("could not stage an update in {}", dir.display()))),
    }
}

/// The attempt itself, with the io error kept intact: whether this failed
/// because of permissions is the question the whole path above turns on, and an
/// error already wrapped in prose can no longer answer it.
fn raw_stage_in(dir: &Path) -> std::io::Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix(".cctop-update-")
        .tempdir_in(dir)
}

/// What to say when the install directory cannot be written and cctop has no
/// way to change that from here.
///
/// The documented install puts cctop in /usr/local/bin with sudo, so a
/// user-owned process being unable to replace it is the ordinary case, not an
/// exotic one. Saying so beats reporting a bare EACCES.
fn unwritable(dir: &Path) -> anyhow::Error {
    anyhow!(
        "{} is not writable by this user, so the new binary cannot replace the old one: {ELEVATE}. \
         If a package manager installed cctop, update it with that instead.",
        dir.display()
    )
}

/// What to say when even root cannot write there.
///
/// Root and still refused is not a permission problem anyone can grant their way
/// out of, so this must not mention sudo: pointing at it would only send the user
/// round the same loop a second time.
fn read_only(dir: &Path) -> anyhow::Error {
    anyhow!(
        "{} is not writable even as root, so the new binary cannot replace the old one — \
         the filesystem is mounted read-only, or the binary is immutable. \
         If a package manager installed cctop, update it with that instead.",
        dir.display()
    )
}

/// Where cargo puts the binaries it installs.
///
/// `CARGO_HOME` when it is set, because a user who moved it did so precisely so
/// that this is not `~/.cargo`, and the default otherwise.
fn cargo_bin() -> Option<PathBuf> {
    let home = match std::env::var_os("CARGO_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => dirs::home_dir()?.join(".cargo"),
    };
    Some(home.join("bin"))
}

/// Whether `exe` is a file cargo installed into `bin`.
///
/// Both sides are resolved before they are compared. `~/.cargo/bin` is on `PATH`
/// through a symlink often enough — a home directory that is one, a toolchain
/// managed somewhere else and linked back — that comparing the paths as written
/// would answer "no" for an install that cargo plainly owns. Resolution failing
/// is itself an answer: a path that cannot be resolved is not one cargo is
/// managing, and guessing "yes" would refuse an update nobody could then perform.
fn under(exe: &Path, bin: &Path) -> bool {
    let (Ok(exe), Ok(bin)) = (exe.canonicalize(), bin.canonicalize()) else {
        return false;
    };
    exe.parent() == Some(bin.as_path())
}

/// Whether cargo, rather than a download, owns the running executable.
fn managed_by_cargo() -> bool {
    let (Ok(exe), Some(bin)) = (std::env::current_exe(), cargo_bin()) else {
        return false;
    };
    under(&exe, &bin)
}

/// What to say to a user whose cctop came from `cargo install`.
///
/// This is the case the permission check cannot catch, and the reason it needs
/// catching separately: `~/.cargo/bin` *is* writable, so nothing would refuse and
/// the replacement would simply happen. What breaks is not the binary but the
/// bookkeeping — cargo keeps its own record of what it installed, and a file
/// swapped underneath it leaves that record describing a version that is no
/// longer there.
fn cargo_managed() -> anyhow::Error {
    anyhow!(
        "cctop was installed by cargo, so replacing the binary here would put it out of step \
         with what cargo has recorded: `cargo install --list` would go on reporting {}, and the \
         next `cargo install-update` would undo the update. Run `cargo install cctop --force` \
         instead.",
        current_version()
    )
}

/// What cctop can offer a user whose install directory it cannot write to.
///
/// Kept as a decision separate from acting on it, because the interesting part
/// is the table of cases and none of it is testable once it has re-executed the
/// process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Recourse {
    /// Ask on the terminal, and re-run under sudo if the user agrees.
    Ask,
    /// Nothing to offer: report the failure and the manual fix.
    Explain,
    /// Already privileged, so the permission error is about the filesystem
    /// rather than about who is asking.
    Privileged,
}

/// Which of those applies, from facts the caller has already gathered.
///
/// `elevated` is the recursion guard, and it is deliberately wider than "am I
/// root": the child cctop re-runs under sudo must never be able to prompt and
/// elevate again, and a `sudo -u someone-else` that is not root would otherwise
/// slip past the root check and start the loop.
///
/// `interactive` covers CI, pipelines and hooks. Elevating unattended is not a
/// thing cctop may do — an unanswerable prompt on a detached stderr is a hang at
/// best, and a silent privilege escalation at worst — so those keep exactly the
/// behaviour they had before any of this existed.
fn recourse(root: bool, elevated: bool, sudo: bool, interactive: bool) -> Recourse {
    match (root, elevated, sudo && interactive) {
        (true, _, _) => Recourse::Privileged,
        (false, false, true) => Recourse::Ask,
        _ => Recourse::Explain,
    }
}

/// Whether cctop is already running as root.
#[cfg(unix)]
fn is_root() -> bool {
    // Safe: geteuid reads a process property and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(unix))]
fn is_root() -> bool {
    false
}

/// Whether this process was itself started through sudo.
///
/// sudo puts `SUDO_USER` in the environment it hands the command, and unlike the
/// euid it survives a target user who is not root. Together with [`is_root`] it
/// is what stops the elevated child from offering to elevate again.
fn already_elevated() -> bool {
    std::env::var_os("SUDO_USER").is_some()
}

/// The command that re-runs this exact binary as root.
///
/// The resolved path from `current_exe`, never argv[0]: sudo resets `PATH` to
/// its own `secure_path`, so a bare `cctop` would be looked up somewhere else or
/// nowhere at all — and the binary to replace is the one at *this* path, which is
/// the same one `self_replace` will go for on the other side. `--` so a path that
/// begins with a dash can never be read as an option of sudo's.
fn sudo_argv(exe: &Path) -> Vec<String> {
    vec![
        "sudo".to_string(),
        "--".to_string(),
        exe.to_string_lossy().into_owned(),
        "--update".to_string(),
    ]
}

/// Handle an install directory this process cannot write into, elevating if the
/// user asks for it.
///
/// Returns the error to fail with. On the one path where it does not fail it
/// does not return at all: cctop hands the terminal to sudo, waits for the
/// privileged run to finish, and exits with whatever that run made of it — there
/// is nothing sensible left for this process to do afterwards, since the update
/// it was asked for has either already happened or already been reported.
fn elevate(dir: &Path) -> anyhow::Error {
    match recourse(
        is_root(),
        already_elevated(),
        crate::shim::is_command("sudo"),
        interactive(),
    ) {
        Recourse::Privileged => return read_only(dir),
        Recourse::Explain => return unwritable(dir),
        Recourse::Ask => {}
    }

    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(_) => return unwritable(dir),
    };
    if !confirm(dir, &exe) {
        return anyhow!(
            "Not updated: {} is not writable by this user. \
             If a package manager installed cctop, update it with that instead.",
            dir.display()
        );
    }

    let argv = sudo_argv(&exe);
    // Inherited stdio, which is the whole reason this is a child process and not
    // an exec of something quieter: sudo asks for a password on the terminal, and
    // a captured stderr is a prompt the user never sees.
    match std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .status()
    {
        // The privileged run has already printed everything there is to say,
        // including its own failures, so this adds nothing and only forwards how
        // it went.
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(error) => anyhow!("could not run sudo ({error}): {ELEVATE}."),
    }
}

/// Whether there is a user at the other end to answer a question.
///
/// stdin because the answer has to come from somewhere, and stderr because that
/// is where the question goes — stdout is left alone so `--update` stays usable
/// in a pipeline, which is also a place this must never prompt.
fn interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Ask before running anything as root.
///
/// Explicit consent, defaulting to no: this re-runs a binary with full
/// privileges, and a user who typed `--update` asked to be updated, not to hand
/// root to whatever cctop decides to do next. Anything but a plain yes is a no,
/// including a closed stdin.
fn confirm(dir: &Path, exe: &Path) -> bool {
    use std::io::Write;

    let mut err = std::io::stderr();
    let _ = write!(
        err,
        "{} is not writable by this user.\nRe-run as root to replace {}? [y/N] ",
        dir.display(),
        exe.display()
    );
    let _ = err.flush();

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_err() {
        return false;
    }
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The refusals, in the order that matters. A managed install is the one
    /// that has to hold whatever else is true: `cargo install --list` would go
    /// on naming a version that is no longer on disk, and nothing about a newer
    /// release makes that acceptable.
    #[test]
    fn a_startup_update_gives_way_to_every_reason_not_to() {
        let latest = || Some("0.9.0".to_string());
        assert_eq!(wanted(true, false, false, latest()), latest());

        // Asked not to, for this run or for good.
        assert_eq!(wanted(false, false, false, latest()), None);
        // Already the product of one update: never twice in a chain.
        assert_eq!(wanted(true, true, false, latest()), None);
        // Cargo owns this binary.
        assert_eq!(wanted(true, false, true, latest()), None);
        // Nothing to update to, which is the ordinary case.
        assert_eq!(wanted(true, false, false, None), None);
    }

    /// The relaunch has to be the same program, or an update silently changes
    /// what the user ran.
    #[test]
    fn the_new_binary_is_started_the_way_this_one_was() {
        let args = ["--host", "box", "--plan", "max"]
            .iter()
            .map(std::ffi::OsString::from)
            .collect();
        let command = relaunch_command("/usr/local/bin/cctop".into(), args);
        let passed: Vec<_> = command.get_args().collect();
        assert_eq!(passed, ["--host", "box", "--plan", "max"]);
        // And the marker, so the version that arrives does not go looking again.
        let marked = command
            .get_envs()
            .any(|(k, v)| k == JUST_UPDATED && v == Some(std::ffi::OsStr::new("1")));
        assert!(marked, "the new process could update itself again");
    }

    /// Nothing to report when nothing moved, which is what `--update --force`
    /// on the newest version does.
    #[test]
    fn there_are_no_notes_for_a_version_that_did_not_change() {
        assert!(!show_changes("0.7.5", "0.7.5"));
        assert!(!show_changes("0.7.5", "0.7.4"));
    }

    fn release(tag: &str, body: &str) -> Release {
        Release {
            tag_name: tag.to_string(),
            assets: Vec::new(),
            body: Some(body.to_string()),
        }
    }

    /// What an update prints is the versions strictly after the one being left
    /// and up to the one arriving. The version you were already running is not
    /// news, and a release newer than what you got has not arrived yet — it can
    /// be in the list, since the check and the download are separate requests
    /// and a release can land between them.
    #[test]
    fn the_notes_cover_the_versions_actually_crossed() {
        let list = [
            release("v0.7.4", "later"),
            release("v0.7.3", "third"),
            release("v0.7.2", "second"),
            release("v0.7.1", "first"),
            release("v0.7.0", "already had this"),
        ];
        let picked: Vec<&str> = selected(&list, "0.7.0", "0.7.3")
            .iter()
            .map(|r| r.version())
            .collect();
        // Ascending: someone three versions behind reads forward in time.
        assert_eq!(picked, ["0.7.1", "0.7.2", "0.7.3"]);

        // One step is the ordinary case, and the boundaries hold there too.
        let picked: Vec<&str> = selected(&list, "0.7.2", "0.7.3")
            .iter()
            .map(|r| r.version())
            .collect();
        assert_eq!(picked, ["0.7.3"]);
    }

    /// A line that opens by naming its own version says it twice: the version is
    /// the heading it is printed under. Both spellings appear in this project's
    /// own releases.
    #[test]
    fn a_bullet_does_not_repeat_the_version_above_it() {
        assert_eq!(
            bullets("* release 0.7.3: the signals stop getting lost"),
            ["the signals stop getting lost"]
        );
        assert_eq!(
            bullets("* 0.7.0: shared tabs, and a report"),
            ["shared tabs, and a report"]
        );
        // Nothing left over means nothing to strip.
        assert_eq!(bullets("* release 0.7.3"), ["release 0.7.3"]);
        // And a colon after something that is not a version is just a colon.
        assert_eq!(
            bullets("* fix: the login hint for a named account"),
            ["fix: the login hint for a named account"]
        );
    }

    /// The real body of a real release, which is markdown written for a web
    /// page. What survives is the sentence that says what changed.
    #[test]
    fn a_generated_release_body_becomes_one_line() {
        let body = "## What's Changed\n\
             * Fix the login hint for a named account, and add a run skill that \
             drives the TUI by @flolep2607 in https://github.com/flolep2607/cctop/pull/5\n\
             \n\n**Full Changelog**: https://github.com/flolep2607/cctop/compare/v0.7.1...v0.7.2";
        assert_eq!(
            bullets(body),
            ["Fix the login hint for a named account, and add a run skill that drives the TUI"]
        );
    }

    /// A release that wrote its own notes keeps them: there is no chrome to
    /// strip, and throwing away prose because it is not a bullet would lose the
    /// only bodies worth reading.
    #[test]
    fn a_hand_written_release_body_survives_intact() {
        let body = "A patch, because every line of it fixes something.\n\n                    - **Handoff** briefs travel in the argv now\n                    - Bells reach the tab bar";
        assert_eq!(
            bullets(body),
            [
                "A patch, because every line of it fixes something.",
                "Handoff briefs travel in the argv now",
                "Bells reach the tab bar",
            ]
        );
    }

    /// A release with no notes at all must not be silently skipped — a version
    /// missing from the list reads as though the jump went around it.
    #[test]
    fn a_release_with_no_notes_is_still_a_release() {
        let empty = Release {
            tag_name: "v0.7.3".into(),
            assets: Vec::new(),
            body: None,
        };
        assert!(bullets(empty.body.as_deref().unwrap_or_default()).is_empty());
        assert_eq!(selected(&[empty], "0.7.2", "0.7.3").len(), 1);
    }

    /// A cargo install is the case the permission check cannot see, because the
    /// directory it would test is writable. Only the executable's location
    /// separates it from a downloaded binary.
    #[test]
    fn cargo_owns_only_what_sits_directly_in_its_bin() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let exe = bin.join("cctop");
        std::fs::write(&exe, b"").unwrap();
        assert!(under(&exe, &bin));

        // A directory below it is not cargo's: nothing cargo installs lands
        // there, so a binary there is somebody else's to replace.
        let nested = bin.join("vendor");
        std::fs::create_dir(&nested).unwrap();
        let deep = nested.join("cctop");
        std::fs::write(&deep, b"").unwrap();
        assert!(!under(&deep, &bin));

        // The ordinary install, which must stay updatable.
        let elsewhere = home.path().join("cctop");
        std::fs::write(&elsewhere, b"").unwrap();
        assert!(!under(&elsewhere, &bin));
    }

    /// Regression: `~/.cargo/bin` reaches `PATH` through a symlink often enough
    /// that comparing the paths as written would call a cargo install a download
    /// and overwrite it.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_cargo_bin_is_still_cargo() {
        let home = tempfile::tempdir().unwrap();
        let real = home.path().join("real-bin");
        std::fs::create_dir(&real).unwrap();
        let exe = real.join("cctop");
        std::fs::write(&exe, b"").unwrap();

        let linked = home.path().join("bin");
        std::os::unix::fs::symlink(&real, &linked).unwrap();
        assert!(under(&exe, &linked), "the link and its target disagreed");
    }

    /// A path that cannot be resolved is not one cargo is managing. Answering
    /// "yes" here would refuse an update that nothing could then perform.
    #[test]
    fn an_unresolvable_path_is_not_a_cargo_install() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("bin");
        assert!(!under(&bin.join("cctop"), &bin));
    }

    /// The message has to name the command that does work, not only refuse.
    #[test]
    fn the_cargo_message_names_the_command_that_replaces_it() {
        let error = cargo_managed().to_string();
        assert!(
            error.contains("cargo install cctop --force"),
            "got: {error}"
        );
        assert!(error.contains(current_version()), "got: {error}");
    }

    /// The failure every user of the documented install hits, and the one place
    /// the message has to name `sudo` rather than report a bare EACCES.
    ///
    /// Two halves, because the live path between them re-executes the process:
    /// an install directory the user cannot write to really does fail with
    /// `PermissionDenied` — which is what routes it to [`elevate`] — and the
    /// message [`elevate`] falls back to when it has nothing to offer names both
    /// the directory and the command that would work.
    #[cfg(unix)]
    #[test]
    fn an_unwritable_install_directory_names_the_fix() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        let kind = match raw_stage_in(dir.path()) {
            Err(error) => error.kind(),
            // Running as root, where the mode bits don't apply.
            Ok(_) => return,
        };
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(kind, std::io::ErrorKind::PermissionDenied);

        let error = format!("{:#}", unwritable(dir.path()));
        assert!(error.contains("sudo cctop --update"), "got: {error}");
        assert!(error.contains("package manager"), "got: {error}");
        assert!(
            error.contains(&dir.path().display().to_string()),
            "got: {error}"
        );
    }

    /// Elevating is something the user asks for, never something that happens
    /// to them. Every case here is a case where cctop must *not* run sudo.
    #[test]
    fn sudo_is_only_ever_offered_to_someone_who_can_answer() {
        // The offer, and the only combination that produces it.
        assert_eq!(recourse(false, false, true, true), Recourse::Ask);

        // No sudo to run, and no terminal to ask on: CI, a pipeline, a hook.
        // Both keep the message the user has always got.
        assert_eq!(recourse(false, false, false, true), Recourse::Explain);
        assert_eq!(recourse(false, false, true, false), Recourse::Explain);
        assert_eq!(recourse(false, false, false, false), Recourse::Explain);

        // The recursion guard: the child cctop started under sudo finds the same
        // unwritable directory, and must not offer to elevate a second time —
        // whether or not sudo made it root.
        assert_eq!(recourse(false, true, true, true), Recourse::Explain);
        assert_eq!(recourse(true, true, true, true), Recourse::Privileged);

        // Root already, so the refusal is the filesystem's and not a question of
        // who is asking.
        assert_eq!(recourse(true, false, true, true), Recourse::Privileged);
    }

    /// What gets run as root has to be this binary at this path, since that is
    /// the file being replaced — and sudo's `secure_path` means a bare `cctop`
    /// is not a way to name it.
    #[test]
    fn the_elevated_command_names_the_running_binary_by_path() {
        let argv = sudo_argv(Path::new("/usr/local/bin/cctop"));
        assert_eq!(argv, ["sudo", "--", "/usr/local/bin/cctop", "--update"]);
    }

    /// Root has no fix to suggest, so it must not send the user back to sudo —
    /// and it still has to name the one thing that might explain it.
    #[test]
    fn a_root_failure_does_not_point_at_sudo() {
        let error = format!("{:#}", read_only(Path::new("/usr/local/bin")));
        assert!(!error.contains("sudo"), "got: {error}");
        assert!(error.contains("/usr/local/bin"), "got: {error}");
        assert!(error.contains("read-only"), "got: {error}");
        assert!(error.contains("package manager"), "got: {error}");
    }

    #[test]
    fn version_ordering_only_moves_forward() {
        assert!(is_newer("0.1.8", "0.1.7"));
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.7", "0.1.7"));
        assert!(!is_newer("0.1.6", "0.1.7"));
        // Differing lengths compare on the shared prefix, then on the extra parts.
        assert!(is_newer("0.1.7.1", "0.1.7"));
        assert!(!is_newer("0.1.7", "0.1.7.1"));
        // A pre-release of the current version is not an upgrade.
        assert!(!is_newer("0.1.7-rc1", "0.1.7"));
        // Garbage must never read as newer.
        assert!(!is_newer("not-a-version", "0.1.7"));
        assert!(!is_newer("", "0.1.7"));
    }

    #[test]
    fn every_released_target_is_reachable() {
        // The running platform must map to a published asset, or `--update`
        // could never work on the machines CI builds for.
        if matches!(std::env::consts::ARCH, "x86_64" | "aarch64") {
            assert!(asset_target().is_some(), "no asset for this platform");
        }
    }

    #[test]
    fn unpack_takes_only_the_executable_from_a_tarball() {
        let mut tar = tar::Builder::new(Vec::new());
        let payload = b"#!/bin/sh\necho hi\n";
        // A decoy that must be ignored, and the binary under a nested path: the
        // destination comes from the staging directory, never from the archive.
        for name in ["README.md", "dist/nested/cctop"] {
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            tar.append_data(&mut header.clone(), name, &payload[..])
                .unwrap();
        }
        let raw = tar.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &raw).unwrap();
        let archive = gz.finish().unwrap();

        let dir = tempfile::tempdir().unwrap();
        let out = unpack(&archive, "x86_64-unknown-linux-musl", dir.path()).unwrap();

        // Flat in the staging directory, ignoring the archive's own path.
        assert_eq!(out, dir.path().join("cctop"));
        assert_eq!(std::fs::read(&out).unwrap(), payload);
        assert!(!dir.path().join("dist").exists());
        assert!(!dir.path().join("README.md").exists());
    }

    #[test]
    fn unpack_reports_an_archive_without_the_binary() {
        let mut tar = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(3);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "README.md", &b"hi\n"[..])
            .unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &tar.into_inner().unwrap()).unwrap();
        let archive = gz.finish().unwrap();

        let dir = tempfile::tempdir().unwrap();
        assert!(unpack(&archive, "x86_64-unknown-linux-musl", dir.path()).is_err());
    }
}
