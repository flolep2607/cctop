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
    println!(
        "cargo:rustc-env=CCTOP_CACHE_HASH={:016x}",
        digest(&read_all(&files))
    );
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
