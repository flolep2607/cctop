# rmux's browser terminal, pinned

The static frontend of `rmux web-share`, copied from <https://share.rmux.io/> on
2026-10-03 by `pull.sh`, unmodified. rmux is © its authors and dual-licensed MIT
or Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, from
<https://github.com/Helvesec/rmux>).

## Why it is here

So a session's terminal can be drawn *inside* cctop's page. `share.rmux.io`
answers with `frame-ancestors 'none'`, so no page can frame it there; served
from cctop's own origin under `/term/`, it can be. The share link's `#t=…`
fragment carries everything the frontend needs, and terminal traffic still runs
end to end between this code in the browser and the rmux daemon — cctop serves
the page, never the terminal.

It is also what a connected account's share hostname serves at its root, so a
`W` link reads `https://<share host>/#…` (`share_host.rs`). There it is the only
page on a listener with no other route, and no token of cctop's ever reaches it.

## Why a pinned copy, not a proxy

Served from cctop's origin, this code runs with the same privileges as cctop's
own page — including the token that can act on agents. A live proxy of
`share.rmux.io` would run whatever that site served that day with those
privileges. A copy in the repository changes only in a commit, where its diff
and these hashes can be read.

## What it is

| file | sha256 |
|---|---|
| `index.html` | `1024aeaadd7030b96ca7623a13cf391f71bc95ee6908329acb787e4228cd06d4` |
| `_astro/index.D2bSaP4U.css` | `11da0b5a68c30d1244c4d63a19d1ef200253049a536dc8f8b0f6e89c9bbf5452` |
| `_astro/index.astro_astro_type_script_index_0_lang.CdYCpIGm.js` | `0afcd35385ef2a0c3e2368361bbd1d4e6a9a0c375cb02c7471a8e8893c006d0e` |
| `_astro/rmux_web_crypto_wasm_bg.C8R0tHIf.wasm` | `da693d5b95e361da9870689457c6892d11770508e2f5c2a5079bd9000b131bba` |

`index.html` also pins the stylesheet and the script with Subresource
Integrity, so a browser refuses either if it does not match.

The page's manifest and touch icon are not copied: neither is needed to run a
terminal inside a frame.

## Updating

Run `pull.sh` from this directory, read the diff, and update the table. The
script's file names carry content hashes, so a new rmux release usually
renames them; `crates/serve/src/term.rs` names them too.
