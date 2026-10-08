#!/bin/sh
# Re-pull rmux's browser terminal from share.rmux.io. See SOURCE.md.
#
# Reads index.html for the stylesheet and script it names, follows the script
# to the WebAssembly module it loads, and prints the hashes SOURCE.md records.
# It does not edit SOURCE.md or src/serve/term.rs: a new build is something to
# read before it is something to ship.
set -eu
cd "$(dirname "$0")"
base=https://share.rmux.io
curl -sf -A cctop -o index.html "$base/"
mkdir -p _astro
for path in $(grep -o '/_astro/[^"]*' index.html | sort -u); do
    curl -sf -A cctop -o ".$path" "$base$path"
done
for path in $(grep -oh '/_astro/[A-Za-z0-9_.-]*\.wasm' _astro/*.js | sort -u); do
    curl -sf -A cctop -o ".$path" "$base$path"
done
for license in LICENSE-MIT LICENSE-APACHE; do
    curl -sf -o "$license" "https://raw.githubusercontent.com/Helvesec/rmux/main/$license"
done
sha256sum index.html _astro/*
