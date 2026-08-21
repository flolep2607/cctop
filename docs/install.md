# Installing cctop

[← back to the README](../README.md)

cctop is a single binary. It links no system libraries and needs no runtime, so
"installing" it means putting one file on your `PATH`.

## Download a binary

Grab the archive for your platform from the
[latest release](https://github.com/flolep2607/cctop/releases/latest) and put
`cctop` somewhere on your `PATH`:

```bash
# Linux x86_64 (static — works on any distro)
curl -fsSL https://github.com/flolep2607/cctop/releases/latest/download/cctop-x86_64-unknown-linux-musl.tar.gz | tar xz
sudo install -m755 cctop /usr/local/bin/cctop
```

```bash
# macOS (Apple silicon; use x86_64-apple-darwin on Intel)
curl -fsSL https://github.com/flolep2607/cctop/releases/latest/download/cctop-aarch64-apple-darwin.tar.gz | tar xz
sudo install -m755 cctop /usr/local/bin/cctop
```

On Windows, download `cctop-x86_64-pc-windows-msvc.zip` and extract `cctop.exe`.

Every archive ships with a `.sha256` file next to it:

```bash
curl -fsSLO https://github.com/flolep2607/cctop/releases/latest/download/cctop-x86_64-unknown-linux-musl.tar.gz.sha256
sha256sum -c cctop-x86_64-unknown-linux-musl.tar.gz.sha256
```

macOS will quarantine an unsigned download. If Gatekeeper blocks it:

```bash
xattr -d com.apple.quarantine /usr/local/bin/cctop
```

## Staying up to date

A downloaded binary has no package manager behind it, so `cctop --update`
fetches the newest release for your platform and replaces the running
executable in place:

```bash
cctop --update
```

Replacing the binary needs write access to the directory it lives in. The install
above puts it in `/usr/local/bin` with `sudo`, so updating it needs `sudo` too:

```bash
sudo cctop --update
```

An update that lands prints what it brought, one heading per version crossed,
so a machine three versions behind sees all three rather than only the newest:

```
Updated 0.7.0 -> 0.7.3.

What changed since 0.7.0:

  0.7.1
    - Codex accounts per subscription, and a tab you can close without losing
      your place

  0.7.2
    - Fix the login hint for a named account, and add a run skill that drives
      the TUI

  0.7.3
    - the signals between cctop and its agents stop getting lost

Full notes: https://github.com/flolep2607/cctop/releases
```

The notes come from the GitHub releases themselves, fetched after the binary is
already in place — so a network that fails at that moment costs you the summary
and not the update, and you get the link instead.

cctop checks for a new release once an hour in the background and, when one
exists, says so in the footer. The next time you start it, it installs that
release before opening: the version was already known, so nothing is waited on
to discover it, and the download is the only pause. It then prints what changed,
waits for you to press Enter, and starts as the new version.

Startup is the only moment it will do this. Nothing is open yet — no terminal
held, no agent hosted, no pane to lose — so the new binary can simply take the
place of the old one. That is not true of any later moment, which is why an
update that becomes available while cctop is running waits for the next start
rather than interrupting the one you are in.

To start on the version you already have, pass `--no-auto-update`. To stop it
happening at all, set `"auto_update": false` in `ui-prefs.json` under your cache
directory (`~/.cache/cctop` on Linux, `~/Library/Caches/cctop` on macOS);
`cctop --update` still works whenever you want it. `cctop claude` and the other
agent aliases skip it too — you are waiting on an agent, and a download between
the command and the agent starting is not what you asked for.

If you installed with `cargo install` or a package manager, none of this
applies: cctop will not touch a binary something else is responsible for, and
`--update` refuses for the same reason. Update it the way you installed it.

## With cargo

```bash
cargo install cctop
```

Or straight from the repository, without waiting for a release:

```bash
cargo install --git https://github.com/flolep2607/cctop
```

## From source

```bash
git clone https://github.com/flolep2607/cctop
cd cctop
cargo build --release
```

The binary lands at `target/release/cctop`. It links no system libraries and
needs no runtime — a single file you can copy anywhere.

Building requires Rust 1.88 or newer (the code uses let-chains).
