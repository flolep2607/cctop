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

const INDEX: &[u8] = include_bytes!("assets/rmux-share/index.html");
const CSS: &[u8] = include_bytes!("assets/rmux-share/_astro/index.D2bSaP4U.css");
const JS: &[u8] = include_bytes!(
    "assets/rmux-share/_astro/index.astro_astro_type_script_index_0_lang.CdYCpIGm.js"
);
const WASM: &[u8] =
    include_bytes!("assets/rmux-share/_astro/rmux_web_crypto_wasm_bg.C8R0tHIf.wasm");

/// The file at `path`, and its content type, when it is part of the app.
///
/// `/term/` is the page a frame opens, with a share link's fragment appended
/// by the page that frames it — see `TerminalFrame` in `web/src/components/terminal.tsx`.
pub fn file(path: &str) -> Option<(&'static str, &'static [u8])> {
    match path {
        "/term" | "/term/" => Some(("text/html; charset=utf-8", INDEX)),
        "/_astro/index.D2bSaP4U.css" => Some(("text/css; charset=utf-8", CSS)),
        "/_astro/index.astro_astro_type_script_index_0_lang.CdYCpIGm.js" => {
            Some(("application/javascript; charset=utf-8", JS))
        }
        // `application/wasm` exactly: `WebAssembly.instantiateStreaming`
        // refuses any other type, and the app falls back to nothing.
        "/_astro/rmux_web_crypto_wasm_bg.C8R0tHIf.wasm" => Some(("application/wasm", WASM)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The page names its stylesheet and script by path; every one of them
    /// must be a file this module serves, or the frame draws nothing.
    #[test]
    fn every_asset_the_page_names_is_served() {
        let page = std::str::from_utf8(INDEX).expect("utf-8");
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
        let js = std::str::from_utf8(JS).expect("utf-8");
        assert!(
            js.contains("/_astro/rmux_web_crypto_wasm_bg.C8R0tHIf.wasm"),
            "the script loads a WebAssembly module this does not serve"
        );
        assert!(file("/_astro/../index.html").is_none());
    }
}
