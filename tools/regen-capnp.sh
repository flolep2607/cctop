#!/usr/bin/env bash
# Regenerate cctop-tunnel's Cap'n Proto bindings, crates/tunnel/src/proto_gen/.
#
# Why they are committed rather than built: capnpc-rust is only a plugin. The
# .capnp files are parsed by the `capnp` compiler, which is C++, and capnpc
# (still, as of 0.27) shells out to it. A build.rs would make every
# `cargo install cctop` need capnproto installed; committing the output means
# only whoever changes a schema does (crates/tunnel/CHANGELOG.md, 0.2.0).
#
# Requirements:
#   - `capnp`, the Cap'n Proto schema compiler (`apt install capnproto`).
#     Any version that reads these schemas will do: it only parses them and
#     hands capnpc-rust a CodeGeneratorRequest. Point $CAPNP at one off PATH.
#   - capnpc-rust at $CAPNPC_VERSION. The generated code calls into the
#     `capnp` runtime crate and must match its minor version, which is pinned
#     in crates/tunnel/Cargo.toml — bump the two together. If it is missing it
#     is installed with `cargo install --locked` into target/, never onto PATH,
#     so a different capnpc-rust there cannot be picked up by mistake.
#
# The schemas are vendored from cloudflared (crates/tunnel/schemas/, see
# THIRD_PARTY_NOTICES.md). go.capnp holds only Go annotations; it is imported,
# not generated, so there is no go_capnp.rs.
#
# Usage:
#   tools/regen-capnp.sh           # rewrite proto_gen/
#   tools/regen-capnp.sh --check   # exit 1 if proto_gen/ differs from a fresh run
set -euo pipefail

CAPNPC_VERSION=0.20.1

cd "$(git rev-parse --show-toplevel)"
schemas=crates/tunnel/schemas
out=crates/tunnel/src/proto_gen

runtime=$(sed -n 's/^capnp = "\(.*\)"$/\1/p' crates/tunnel/Cargo.toml)
if [[ "${CAPNPC_VERSION%.*}" != "$runtime" ]]; then
    echo "capnpc $CAPNPC_VERSION does not match the capnp runtime \"$runtime\" in crates/tunnel/Cargo.toml" >&2
    exit 1
fi

capnp=${CAPNP:-capnp}
if ! command -v "$capnp" >/dev/null; then
    echo "no \`capnp\` compiler: install capnproto, or set CAPNP=/path/to/capnp" >&2
    exit 1
fi

root=target/capnpc-$CAPNPC_VERSION
plugin=$root/bin/capnpc-rust
if [[ ! -x "$plugin" ]]; then
    cargo install "capnpc@$CAPNPC_VERSION" --locked --root "$root"
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# --src-prefix keeps the `// source:` line and the output names bare
# (tunnelrpc_capnp.rs), which is what lib.rs includes.
"$capnp" compile -o"$plugin:$tmp" --src-prefix="$schemas" \
    "$schemas/tunnelrpc.capnp" "$schemas/quic_metadata_protocol.capnp"

if [[ "${1:-}" == "--check" ]]; then
    diff -r "$out" "$tmp" && echo "$out is up to date"
else
    cp "$tmp"/*.rs "$out"/
    git status --short -- "$out"
fi
