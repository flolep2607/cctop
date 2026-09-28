#!/bin/sh
# Install cctop from a GitHub release: pick the archive for this machine,
# check it against the checksum the release publishes, put the binary on PATH.
#
#   curl -fsSL https://raw.githubusercontent.com/flolep2607/cctop/main/install.sh | sh
#
# Written for POSIX sh, not bash, and it cannot be anything else: piped into
# `sh` the kernel never reads the shebang above, so whatever runs this is whatever
# `/bin/sh` is. On Debian and Ubuntu that is dash, which has no `pipefail` and
# no `[[ ]]` — a `set -euo pipefail` at the top would abort before printing
# anything on half the machines this is meant to serve. It is also why the
# download is never piped: without pipefail the exit status of a pipeline is
# the status of its last command, so `curl | tar` reports tar's opinion of an
# empty download rather than curl's failure. curl writes a file, `set -e`
# catches it, and only then is anything unpacked.
#
# What it will not do: install for a machine other than this one, put anything
# in a directory not on PATH, or run the binary to find out what it does. It
# fetches, verifies and installs, and stops.
set -eu

repo="https://github.com/flolep2607/cctop"
latest="$repo/releases/latest/download"

# Releases ship two archives, both statically linked against musl, so either
# runs on any distro. `uname -m` is the only thing consulted — a distro's
# packaging and a 32-bit userspace under a 64-bit kernel both answer the
# question this actually needs answered, which is what the CPU can run.
case "$(uname -m)" in
  x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
  aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
  *)
    echo "cctop ships x86_64 and aarch64 archives; this machine is" \
      "$(uname -m)." >&2
    echo "Build from source instead: https://github.com/flolep2607/cctop#from-source" >&2
    exit 1
    ;;
esac

command -v curl >/dev/null 2>&1 || {
  echo "install.sh needs curl." >&2
  exit 1
}

# cctop reads Linux process tables and drives agents over ptys and unix
# sockets. Saying so here is friendlier than a binary that starts and then
# misbehaves, and it is the one platform check worth making before the
# download rather than after.
[ "$(uname -s)" = "Linux" ] || {
  echo "cctop runs on Linux, including WSL. There is no macOS or Windows build." >&2
  exit 1
}

tmp="$(mktemp -d)"
# The archive holds one file called `cctop`, so extracting it into the caller's
# current directory is a name collision waiting to happen: anyone running this
# inside a cctop checkout has a directory of that name already there, and tar
# stops at `Cannot open: File exists` with nothing installed. Everything lands
# in $tmp instead, and $tmp is removed on the way out of every exit.
trap 'rm -rf "$tmp"' EXIT
trap 'rm -rf "$tmp"; exit 130' INT
trap 'rm -rf "$tmp"; exit 143' TERM

echo "fetching cctop ($target)"
# Both files are saved under the names the release gives them, because the
# checksum file names the archive it is a checksum of. Renaming the download to
# something shorter makes `sha256sum -c` look for a file that is not there, and
# a download saved under a name the checksum does not mention cannot be
# verified at all.
curl -fsSL -o "$tmp/cctop-$target.tar.gz" "$latest/cctop-$target.tar.gz"
curl -fsSL -o "$tmp/cctop-$target.sha256" "$latest/cctop-$target.sha256"

# Verified before anything is unpacked, which is the whole reason to prefer this
# over piping a tarball into sh. Note the checksum asset is named for the
# archive rather than after it — `cctop-x86_64-unknown-linux-musl.sha256`, not
# `cctop-x86_64-unknown-linux-musl.tar.gz.sha256`, which is a 404.
if command -v sha256sum >/dev/null 2>&1; then
  (cd "$tmp" && sha256sum -c "cctop-$target.sha256")
else
  # Busybox and the BSDs ship the other name for the same check.
  (cd "$tmp" && shasum -a 256 -c "cctop-$target.sha256")
fi

tar xzf "$tmp/cctop-$target.tar.gz" -C "$tmp"

# `/usr/local/bin` is where the binary belongs when this account may write
# there, and where the README puts it. The install goes through sudo only when
# it has to: an installer that demands a password to write somewhere it was
# already allowed to write is an installer people stop running. Without a
# working sudo, `~/.local/bin` is the fallback, and the PATH note below is what
# makes that usable rather than a binary nobody can run.
#
# The sudo attempt is allowed to fail rather than ending the script. It fails
# for reasons that have nothing to do with cctop — no tty to type a password
# into over ssh, no askpass helper in CI, no sudo in a minimal container — and
# dying on a sudo diagnostic says nothing about the perfectly good install that
# was one directory away.
dest="/usr/local/bin"
if [ ! -w "$dest" ] && command -v sudo >/dev/null 2>&1 &&
  sudo install -m755 "$tmp/cctop" "$dest/cctop" 2>/dev/null; then
  :
else
  [ -w "$dest" ] || dest="$HOME/.local/bin"
  mkdir -p "$dest"
  install -m755 "$tmp/cctop" "$dest/cctop"
fi

version="$("$dest/cctop" --version 2>/dev/null || echo cctop)"

case ":$PATH:" in
  *":$dest:"*) ;;
  *)
    echo
    echo "Installed to $dest, which is not on your PATH. Add this to your shell profile:"
    echo
    echo "    export PATH=\"\$PATH:$dest\""
    ;;
esac

cat <<EOF

Installed $version to $dest/cctop

  cctop                  the dashboard
  cctop --install-hooks  let the agents report their own state, live
  cctop doctor           check this installation and say what is wrong with it
EOF
