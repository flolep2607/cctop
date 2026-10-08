//! rmux's browser terminal, served from this origin so a page can frame it.
//!
//! A session's terminal is an rmux share, and the share's frontend is a static
//! app that `share.rmux.io` serves with `frame-ancestors 'none'` — so from
//! there it can only ever be a window of its own. Everything a share needs
//! rides in its link's `#t=…` fragment, which the browser never sends to any
//! server, so the same app loaded from here, at `/term/#t=…`, opens the same
//! terminal. The page frames that, and the terminal sits beside the
//! conversation instead of floating over the desktop.
//!
//! The terminal traffic does not pass through here. The app talks to the rmux
//! daemon over its own socket, end to end encrypted, exactly as it does from
//! `share.rmux.io`; this module only hands out four static files.
//!
//! The files are a pinned copy in `assets/rmux-share/`, with their provenance
//! and hashes in `SOURCE.md` there. Pinned, not proxied, because code served
//! from this origin runs with the page's privileges — the token included — and
//! a copy only changes in a commit someone can read.
//!
//! The app asks for its assets at absolute `/_astro/…` paths, which is why
//! those are served at the root rather than under `/term/`: rewriting paths
//! inside a minified bundle would make the copy no longer the one the hashes
//! describe. None of cctop's own routes live under `/_astro/`.
//!
//! ponytail: the app's web manifest and touch icon are not served. They make
//! `share.rmux.io` installable as a phone app, which a frame inside cctop has
//! no use for, and the 404 they get costs nothing.

use super::http::{IMMUTABLE, Packed, REVALIDATE, built};
use std::sync::LazyLock;

/// A picture of nothing, for the logos the app asks for and nobody ships.
const EMPTY_SVG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>";

// Each file as the build compressed it — see [`Packed`] and `build.rs`, which
// lists the same paths. Lazy only for the ETag, a hash of the gzip copy.
static INDEX_P: LazyLock<Packed> = LazyLock::new(|| built!("src/assets/rmux-share/index.html"));
static CSS_P: LazyLock<Packed> =
    LazyLock::new(|| built!("src/assets/rmux-share/_astro/index.D2bSaP4U.css"));
static JS_P: LazyLock<Packed> = LazyLock::new(|| {
    built!("src/assets/rmux-share/_astro/index.astro_astro_type_script_index_0_lang.CdYCpIGm.js")
});
static WASM_P: LazyLock<Packed> =
    LazyLock::new(|| built!("src/assets/rmux-share/_astro/rmux_web_crypto_wasm_bg.C8R0tHIf.wasm"));
static EMPTY_P: LazyLock<Packed> = LazyLock::new(|| Packed::plain(EMPTY_SVG));

/// One file of the app, and how long a browser may keep it.
pub struct File {
    pub content_type: &'static str,
    pub body: &'static Packed,
    pub cache: &'static str,
}

/// The file at `path`, when it is part of the app.
///
/// `/term/` is the page a frame opens, with a share link's fragment appended
/// by the page that frames it — see `TerminalFrame` in `web/src/components/terminal.tsx`.
///
/// How long each may be kept is the point of the split. The `/_astro/` names
/// carry a hash of their content, so a new rmux bundle is a new name and the
/// old one can be kept for good: a workspace of eight tiles used to download
/// all three of them eight times on every load, 2.6 MB of the same bytes, and
/// now downloads them once per browser. The page that names them has no hash
/// in its own name, so it is revalidated every time, which is a `304` when it
/// has not changed — and it is the page that decides which `/_astro/` names
/// are fetched, so a stale one is never kept past a cctop upgrade.
pub fn file(path: &str) -> Option<File> {
    let (content_type, body, cache): (_, &'static Packed, _) = match path {
        "/term" | "/term/" => ("text/html; charset=utf-8", &INDEX_P, REVALIDATE),
        "/_astro/index.D2bSaP4U.css" => ("text/css; charset=utf-8", &CSS_P, IMMUTABLE),
        "/_astro/index.astro_astro_type_script_index_0_lang.CdYCpIGm.js" => {
            ("application/javascript; charset=utf-8", &JS_P, IMMUTABLE)
        }
        // `application/wasm` exactly: `WebAssembly.instantiateStreaming`
        // refuses any other type, and the app falls back to nothing.
        "/_astro/rmux_web_crypto_wasm_bg.C8R0tHIf.wasm" => ("application/wasm", &WASM_P, IMMUTABLE),
        // Logos the app asks for that nothing here would show. The root pair
        // `share.rmux.io` does not have either (it answers them with its own
        // HTML); the crab under `/term/crabs/` is the brand mark in the
        // navigation bar, which a share minted for a frame turns off
        // (`no_navbar` in `rmux::share_with`) — so the image is fetched and
        // never drawn. An empty picture rather than rmux's 36 KB crabs, so the
        // console stops reporting a 404 per frame and nothing is downloaded
        // to be hidden. Not revalidated: there is nothing in it to go stale.
        "/rose-dark.svg" | "/rose-light.svg" => ("image/svg+xml", &EMPTY_P, IMMUTABLE),
        _ if is_crab(path) => ("image/svg+xml", &EMPTY_P, IMMUTABLE),
        _ => return None,
    };
    Some(File {
        content_type,
        body,
        cache,
    })
}

/// Whether `path` is one of the app's brand crabs: `/term/crabs/<colour>-dark.svg`
/// or `-light.svg`, the colour one lowercase word. By shape rather than by the
/// app's list of ten colours, so a release that adds an eleventh does not
/// bring the 404s back.
fn is_crab(path: &str) -> bool {
    path.strip_prefix("/term/crabs/")
        .and_then(|name| {
            name.strip_suffix("-dark.svg")
                .or_else(|| name.strip_suffix("-light.svg"))
        })
        .is_some_and(|colour| !colour.is_empty() && colour.bytes().all(|b| b.is_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page names its stylesheet and script by path; every one of them
    /// must be a file this module serves, or the frame draws nothing.
    #[test]
    fn every_asset_the_page_names_is_served() {
        let page = std::str::from_utf8(INDEX_P.decoded()).expect("utf-8");
        let mut named = 0;
        for piece in page.split('"') {
            if piece.starts_with("/_astro/") {
                assert!(file(piece).is_some(), "{piece} is named but not served");
                named += 1;
            }
        }
        assert_eq!(
            named, 2,
            "the page should name one stylesheet and one script"
        );
        let js = std::str::from_utf8(JS_P.decoded()).expect("utf-8");
        assert!(
            js.contains("/_astro/rmux_web_crypto_wasm_bg.C8R0tHIf.wasm"),
            "the script loads a WebAssembly module this does not serve"
        );
        assert!(file("/_astro/../index.html").is_none());
    }

    /// The logo a frame asks for in every colour and theme is answered, and
    /// nothing else under `/term/` is mistaken for one.
    #[test]
    fn every_crab_is_answered_and_nothing_else_is() {
        let js = std::str::from_utf8(JS_P.decoded()).expect("utf-8");
        assert!(
            js.contains("crabs/${e}-dark.svg"),
            "the app no longer asks for its crabs where this answers them"
        );
        for colour in ["lime", "indigo", "orange", "green", "blue", "rose"] {
            for theme in ["dark", "light"] {
                let path = format!("/term/crabs/{colour}-{theme}.svg");
                assert!(file(&path).is_some(), "{path} is not answered");
            }
        }
        for not in [
            "/term/crabs/-dark.svg",
            "/term/crabs/../x-dark.svg",
            "/term/crabs/lime-dark.png",
            "/term/crabs/Lime-dark.svg",
            "/crabs/lime-dark.svg",
        ] {
            assert!(file(not).is_none(), "{not} should not be answered");
        }
    }

    /// The hashed names are kept for good and the page that names them is
    /// asked about every time — the other way round, an upgrade would leave a
    /// browser on the old bundle.
    #[test]
    fn only_the_hashed_files_are_kept_without_asking() {
        assert_eq!(file("/term/").unwrap().cache, REVALIDATE);
        for piece in std::str::from_utf8(INDEX_P.decoded()).unwrap().split('"') {
            if piece.starts_with("/_astro/") {
                assert_eq!(file(piece).unwrap().cache, IMMUTABLE, "{piece}");
            }
        }
    }
}
