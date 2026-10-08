// Compresses the web UI once, when cctop is built, rather than once per run.
//
// The app page and rmux's terminal bundle are the same bytes for every run of
// a given build, so the build is the place to compress them. The binary then
// carries a brotli and a gzip copy of each, and no brotli encoder: the encoder
// was about a megabyte of the binary, and it only ever compressed bodies that
// could have been compressed here. See `Packed` in `src/http.rs` for how the
// copies are served, and what a client that takes neither is sent.
//
// Every output is a pure function of its input — brotli is deterministic, and
// the gzip header carries no name and a zero mtime — so the same tree builds
// the same binary.

use std::io::Write;
use std::path::Path;

/// Each file compressed, relative to this crate. `src/http.rs` names the same
/// paths through its `built!` macro, which is what puts the outputs in the
/// binary; a path missing here fails that `include_bytes!` at compile time.
const ASSETS: &[&str] = &[
    "src/assets/app/index.html",
    "src/assets/rmux-share/index.html",
    "src/assets/rmux-share/_astro/index.D2bSaP4U.css",
    "src/assets/rmux-share/_astro/index.astro_astro_type_script_index_0_lang.CdYCpIGm.js",
    "src/assets/rmux-share/_astro/rmux_web_crypto_wasm_bg.C8R0tHIf.wasm",
];

fn main() {
    let out = std::env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    // Only the inputs: with any `rerun-if-changed` line, cargo stops rerunning
    // this on every edit to the crate, so an incremental build of a change to
    // the server's Rust skips the compression entirely.
    for asset in ASSETS {
        println!("cargo:rerun-if-changed={asset}");
    }
    // One thread per file, since the page and the script are most of the
    // time and neither waits on the other.
    std::thread::scope(|scope| {
        for asset in ASSETS {
            let out = &out;
            scope.spawn(move || {
                let body = std::fs::read(asset).unwrap_or_else(|e| panic!("{asset}: {e}"));
                let dest = Path::new(out).join(asset);
                std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
                write(&dest, "br", &brotli(&body));
                write(&dest, "gz", &gzip(&body));
            });
        }
    });
}

fn write(dest: &Path, ext: &str, bytes: &[u8]) {
    let mut name = dest.as_os_str().to_owned();
    name.push(".");
    name.push(ext);
    std::fs::write(&name, bytes).unwrap_or_else(|e| panic!("{}: {e}", dest.display()));
}

/// Brotli at its highest quality, with the largest window browsers decode.
/// Bought once per build and paid back on every first load: against quality
/// 9, it is 33 KB less of the 1.7 MB a terminal and the page cost to fetch.
///
/// Through the C-shaped entry point rather than `BrotliCompress`, for build
/// time and nothing else. A build script is compiled unoptimised, and the
/// generic entry points are instantiated in the crate that calls them — here —
/// so the encoder ran unoptimised too: 25 s for these files. This one is a
/// plain function compiled inside `brotli`, which the workspace's profiles
/// optimise on its own, and the same files take about 2 s.
fn brotli(body: &[u8]) -> Vec<u8> {
    use brotli::ffi::compressor::{
        BrotliEncoderCompress, BrotliEncoderMaxCompressedSize, BrotliEncoderMode,
    };
    let mut out = vec![0u8; BrotliEncoderMaxCompressedSize(body.len())];
    let mut size = out.len();
    // SAFETY: both pointers come from live slices whose lengths are the ones
    // passed beside them, and `size` is `out`'s length, which the encoder
    // writes within and then sets to what it wrote.
    let ok = unsafe {
        BrotliEncoderCompress(
            11,
            24,
            BrotliEncoderMode::BROTLI_MODE_GENERIC,
            body.len(),
            body.as_ptr(),
            &mut size,
            out.as_mut_ptr(),
        )
    };
    assert_eq!(ok, 1, "brotli refused to compress");
    out.truncate(size);
    out
}

/// Gzip at its best level. `GzBuilder` with nothing set writes no file name
/// and a zero mtime, so the header is the same on every build.
fn gzip(body: &[u8]) -> Vec<u8> {
    let mut gz = flate2::GzBuilder::new().write(
        Vec::with_capacity(body.len() / 3),
        flate2::Compression::best(),
    );
    gz.write_all(body).expect("gzip into a Vec");
    gz.finish().expect("gzip into a Vec")
}
