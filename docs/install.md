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

cctop checks for a new release once an hour in the background and, when one
exists, says so in the footer. It never updates itself: the check only reports,
and replacing the binary always takes an explicit `--update`. If you installed
with `cargo install` or a package manager, update it the same way you installed
it — `--update` will refuse rather than overwrite a managed install it cannot
write to.

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
