# Preact and htm, pinned

The UMD builds of [Preact](https://preactjs.com) and its hooks, and of
[htm](https://github.com/developit/htm), copied unmodified from the npm
registry on 2026-10-05. Preact is MIT (`LICENSE-preact`); htm is Apache-2.0
(`LICENSE-htm`).

## Why it is here

The workspace page holds state that has to survive its own live updates: a
dozen terminal frames, each with a socket to an agent, whose headers change
every few seconds while the frames themselves must not be touched. The other
pages rebuild their DOM on every refresh and work around what that destroys
(the report keeps its find box outside the area it rebuilds for exactly this
reason). A keyed, diffing renderer is the tool for that, and this is the
smallest one that needs no build step: about 16 KB, inlined into the one page
that uses it, under the same content policy as every other page.

htm is what makes "no build step" true. It turns tagged template literals into
Preact calls at runtime — `` html`<div class=${c}>…</div>` `` — so the page is
plain JavaScript that runs as written, with no JSX compiler and no Node in the
toolchain. It does this by parsing, not by `eval` or `new Function`, so the
policy needs no `unsafe-eval`.

## Why 10.x and not 11

Preact 11 ships ES modules only. A module can be inlined, but it cannot import
another one: the page's policy loads nothing from any URL, its own origin
included. The 10.x line still ships these self-contained UMD builds, which
define `preact`, `preactHooks` and `htm` as globals and need nothing else.

## What it is

| file | from | sha256 |
|---|---|---|
| `preact.umd.js` | `preact@10.29.8` `dist/preact.umd.js` | `134b77bc803fa38661dc1b1e44e96eb0bb6a1a00edbb96d34dcde421b2e80b06` |
| `preact-hooks.umd.js` | `preact@10.29.8` `hooks/dist/hooks.umd.js` | `5c29238e5dc99df306d7f7fff038591a397cfcfabb59f81fbdef43d670aa0566` |
| `htm.umd.js` | `htm@3.1.1` `dist/htm.umd.js` | `7a31776e04bd4afde0d4308177d26f377716fcf7e4bd70be590746d6aa594f08` |

Each still ends in a `sourceMappingURL` comment; the map is not served, and the
only cost is a 404 when someone opens devtools. Left in so the files stay
byte-identical to the published ones and the hashes above can be checked
against the registry.

## Updating

```bash
cd "$(mktemp -d)"
curl -sSL https://registry.npmjs.org/preact/-/preact-10.X.Y.tgz | tar xz && mv package preact
curl -sSL https://registry.npmjs.org/htm/-/htm-3.X.Y.tgz | tar xz && mv package htm
V=~/cctop/src/serve/assets/vendor
cp preact/dist/preact.umd.js "$V/preact.umd.js"
cp preact/hooks/dist/hooks.umd.js "$V/preact-hooks.umd.js"
cp htm/dist/htm.umd.js "$V/htm.umd.js"
sha256sum "$V"/*.js   # and update the table
```
