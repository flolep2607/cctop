# cctop-tunnel

Internal to [cctop](https://github.com/flolep2607/cctop): puts local ports on
public URLs. No stable API between versions; use the `cctop` binary.

It is published only because `cargo install cctop` builds from crates.io, and a
crates.io package can depend only on others that are there too.

## What it does

- `Provider` registers a tunnel and hands back a live `Tunnel`; `Routes` say
  which local port answers which public hostname, and can change while the
  tunnel is up. Neither names a provider.
- `cloudflare::Quick` is a Cloudflare quick tunnel (`*.trycloudflare.com`): no
  account, a new hostname every start.
- `cloudflare::Named` is a named tunnel on your own Cloudflare account, from a
  tunnel token: a stable hostname on your domain, one or more hostnames per
  tunnel, and the edge's configuration push acknowledged.

Both speak the argotunnel protocol natively — QUIC to the edge, Cap'n Proto RPC
over it — so nothing needs `cloudflared` installed.

## Where it came from

This crate began as [`cloudflare-quick-tunnel`](https://github.com/lordmacu/cloudflare-quick-tunnel-rs)
0.3.1 by lordmacu, under MIT OR Apache-2.0, which it keeps. `CHANGELOG.md` says
what changed since. `THIRD_PARTY_NOTICES.md` lists the Apache-2.0 material from
Cloudflare's `cloudflared` it carries: the Cap'n Proto schemas the edge speaks
and the CA certificates that sign the edge.

## License

MIT OR Apache-2.0. See `LICENSE-MIT` and `LICENSE-APACHE`.
