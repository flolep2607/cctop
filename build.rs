// Derives the session cache's version from the sources that define it.
//
// The cache stores parser *output*, so any change to the extracted shape or to
// the semantics producing it makes every stored entry a potential lie. That
// used to be a hand-bumped `CACHE_VERSION`, which is exactly the kind of manual
// step that gets forgotten in the commit that needed it — and a forgotten bump
// is invisible: `#[serde(default)]` fills the new field with `None` and the
// panel renders blank forever, because a finished transcript never changes
// again to force a re-parse.
//
// So hash the sources instead. Emits `CCTOP_CACHE_HASH`, which `src/cache.rs`
// uses as the cache version, and which its tests re-derive through the
// functions below.
//
// Also emits `CCTOP_COMMIT`, the commit being built, which the help page shows
// beside the version — so a report of "the help says X" can be told apart
// from a build of the same version with a different tree under it.
//
// No dependencies here on purpose: a build script is compiled for the host
// before anything else, and a 20-line FNV-1a is cheaper than making every
// build of cctop wait on a hashing crate it does not otherwise need.
//
// (`//` and not `//!`: cache.rs `include!`s this file into its test module, and
// an inner doc comment cannot appear in macro-expanded code.)

use std::path::{Path, PathBuf};

/// Sources whose content decides what a cache entry means. Missing entries are
/// skipped, so `src/cache.rs` and `src/cache/` can both be listed.
pub const ROOTS: &[&str] = &[
    "src/session",
    "src/cache.rs",
    "src/cache",
    "src/pricing.rs",
    "src/config.rs",
];

fn main() {
    let (files, dirs) = sources();
    for path in files.iter().chain(&dirs) {
        // Directories are listed too, so a *new* parser — one no hashed file
        // mentions yet — still re-runs this script.
        println!("cargo:rerun-if-changed={}", slash_path(path));
    }
    // Which checkout is being built. Cargo names this script's output after
    // the package and not after where it lives, and judges the lines above by
    // mtime, relative to whichever package root is building — so two
    // checkouts sharing a target dir share one output, and a checkout whose
    // files are older than the other's last run takes that run's digest as
    // fresh. (And the whole binary with it: `CARGO_MANIFEST_DIR` is not
    // fingerprinted either.) The value is set by `.cargo/config.toml` to the
    // checkout's own path, so switching checkouts changes a watched variable,
    // which reruns this and so recompiles the crate once. The same checkout
    // builds as incrementally as before.
    //
    // ponytail: only cargo run from inside the checkout reads that config, so
    // one built with `--manifest-path` from elsewhere is not covered.
    println!("cargo:rerun-if-env-changed=CCTOP_CHECKOUT");
    println!(
        "cargo:rustc-env=CCTOP_CACHE_HASH={:016x}",
        digest(&read_all(&files))
    );
    println!(
        "cargo:rustc-env=CCTOP_COMMIT={}",
        commit().unwrap_or_default()
    );
}

/// The commit being built, abbreviated, or `None` when nothing says.
///
/// From git when this is a checkout, which also tells cargo to look again when
/// HEAD moves — a commit, a checkout, a rebase — and otherwise from the
/// `.cargo_vcs_info.json` that `cargo package` writes into a crate, which is
/// the only record of the commit a `cargo install cctop` builds from.
///
/// ponytail: a tree with uncommitted changes shows the commit it started
/// from, unmarked. Knowing it was dirty would need this script to rerun on
/// every edit to every file, which is a recompile of the crate on each build.
pub fn commit() -> Option<String> {
    let git = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("git").args(args).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !text.is_empty()).then_some(text)
    };
    if let Some(hash) = git(&["rev-parse", "--short=9", "HEAD"]) {
        // HEAD, the branch it names, and the packed refs a branch can live in
        // instead. Only those that exist: cargo reruns a script whose watched
        // path is missing on every build.
        let mut watched = vec!["HEAD".to_string(), "packed-refs".to_string()];
        watched.extend(git(&["symbolic-ref", "-q", "HEAD"]));
        for name in watched {
            if let Some(path) = git(&["rev-parse", "--git-path", &name])
                && Path::new(&path).exists()
            {
                println!("cargo:rerun-if-changed={path}");
            }
        }
        return Some(hash);
    }
    let info = std::fs::read_to_string(".cargo_vcs_info.json").ok()?;
    let at = info.find("\"sha1\"")?;
    let hash: String = info[at + 6..]
        .chars()
        .skip_while(|c| !c.is_ascii_hexdigit())
        .take_while(char::is_ascii_hexdigit)
        .take(9)
        .collect();
    (hash.len() == 9).then_some(hash)
}

/// Every hashed file and every directory walked to find them, relative to the
/// package root, sorted so the result never depends on readdir order.
pub fn sources() -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut files, mut dirs) = (Vec::new(), Vec::new());
    for root in ROOTS {
        collect(Path::new(root), &mut files, &mut dirs);
    }
    files.sort();
    dirs.sort();
    (files, dirs)
}

pub fn collect(path: &Path, files: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>) {
    if path.is_dir() {
        dirs.push(path.to_path_buf());
        let Ok(rd) = std::fs::read_dir(path) else {
            return;
        };
        for entry in rd.flatten() {
            collect(&entry.path(), files, dirs);
        }
    } else if path.is_file() {
        files.push(path.to_path_buf());
    }
}

/// Pair each file with its bytes. An unreadable file is a distinct state, not
/// an empty one, so it hashes differently rather than silently matching.
pub fn read_all(files: &[PathBuf]) -> Vec<(String, Vec<u8>)> {
    files
        .iter()
        .map(|f| {
            let bytes = std::fs::read(f).unwrap_or_else(|_| b"<unreadable>".to_vec());
            (slash_path(f), bytes)
        })
        .collect()
}

/// FNV-1a over each `(path, bytes)` pair.
///
/// The path is hashed alongside the content because moving a parser between
/// files changes what runs, even when the bytes are merely relocated.
pub fn digest(entries: &[(String, Vec<u8>)]) -> u64 {
    let mut hash = FNV_OFFSET;
    for (path, bytes) in entries {
        write(&mut hash, path.as_bytes());
        write(&mut hash, bytes);
    }
    hash
}

/// Relative path with `/` separators, so a Windows build hashes the same as a
/// Unix one.
pub fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn write(hash: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *hash ^= u64::from(*b);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
    // Length-delimit, so ("ab", "c") and ("a", "bc") do not collide.
    *hash ^= bytes.len() as u64;
    *hash = hash.wrapping_mul(FNV_PRIME);
}
