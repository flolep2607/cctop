//! Self-update against the project's GitHub releases.
//!
//! A downloaded binary has no package manager behind it, so without this it
//! stays on whatever version it was fetched at. Installs that *do* have a
//! package manager (`cargo install`, a distro package) must not be overwritten
//! behind the manager's back, so replacing the executable only ever happens
//! when the user asks for it with `--update`. The passive check just reports.

use crate::config;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

const RELEASES_URL: &str = "https://api.github.com/repos/flolep2607/cctop/releases/latest";
/// GitHub rejects API requests without one.
const USER_AGENT: &str = concat!("cctop/", env!("CARGO_PKG_VERSION"));
const CHECK_MAX_AGE_SECS: u64 = 24 * 60 * 60;

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

/// Newest published version, refreshed at most once a day.
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
pub fn run(force: bool) -> Result<()> {
    let current = current_version();
    let target =
        asset_target().ok_or_else(|| anyhow!("no release is published for this platform"))?;

    println!("Current version {current}; checking for updates…");
    let release = fetch_latest()?;
    let latest = release.tag_name.trim_start_matches('v');

    if !is_newer(latest, current) && !force {
        println!("Already on the newest version ({current}).");
        return Ok(());
    }

    let asset = release
        .assets
        .iter()
        // The checksum sidecars share the archive's prefix, so match on the
        // archive extensions rather than on the target alone.
        .find(|a| {
            a.name.contains(target) && (a.name.ends_with(".tar.gz") || a.name.ends_with(".zip"))
        })
        .ok_or_else(|| anyhow!("release {latest} has no archive for {target}"))?;

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

    // Stage the new binary beside the current one: replacing an executable is a
    // rename, and a rename cannot cross a filesystem boundary.
    let exe = std::env::current_exe().context("could not locate the running executable")?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow!("the running executable has no parent directory"))?;
    let staging = tempfile::Builder::new()
        .prefix(".cctop-update-")
        .tempdir_in(dir)
        .with_context(|| {
            format!(
                "could not write to {}; if cctop was installed by a package manager, update it with that instead",
                dir.display()
            )
        })?;

    let new_binary = unpack(&body, target, staging.path())?;
    self_replace::self_replace(&new_binary).context("could not replace the running executable")?;

    println!("Updated {current} -> {latest}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
