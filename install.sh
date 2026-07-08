#!/bin/sh
# mail-cli installer — downloads the latest prebuilt release binary.
#
#   curl -fsSL https://raw.githubusercontent.com/patch-notes/mail-cli/master/install.sh | sh
#
# Options (env vars):
#   BINDIR=/path        install directory (default: /usr/local/bin, or
#                       ~/.local/bin when /usr/local/bin is not writable)
#   INSTALL_SKILL=0     skip running `mail-cli install-skill` afterwards
set -eu

REPO="patch-notes/mail-cli"
API="https://api.github.com/repos/${REPO}/releases/latest"

err() { printf 'error: %s\n' "$1" >&2; exit 1; }
info() { printf '%s\n' "$1" >&2; }

# --- detect OS/arch and map to the release asset suffix -------------------
os=$(uname -s)
arch=$(uname -m)
case "$os" in
  Linux)  os_tag=linux ;;
  Darwin) os_tag=macos ;;
  *) err "unsupported OS '$os' (grab a binary from https://github.com/${REPO}/releases/latest)" ;;
esac
case "$arch" in
  x86_64|amd64)  arch_tag=amd64 ;;
  arm64|aarch64) arch_tag=arm64 ;;
  *) err "unsupported arch '$arch'" ;;
esac
# Linux ships amd64 only.
if [ "$os_tag" = linux ] && [ "$arch_tag" != amd64 ]; then
  err "no linux-$arch_tag build published (only linux-amd64)"
fi
suffix="${os_tag}-${arch_tag}"

# --- resolve the download URL for the latest release ----------------------
command -v curl >/dev/null 2>&1 || err "curl is required"
info "resolving latest mail-cli release for ${suffix}..."
url=$(curl -fsSL "$API" | grep -o "https://[^\"]*${suffix}\(\.exe\)\?" | head -n1)
[ -n "$url" ] || err "could not find a '${suffix}' asset in the latest release"

# --- download -------------------------------------------------------------
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
info "downloading $url"
curl -fSL "$url" -o "$tmp"
chmod +x "$tmp"

# --- install --------------------------------------------------------------
bindir=${BINDIR:-/usr/local/bin}
if [ ! -d "$bindir" ] || [ ! -w "$bindir" ]; then
  if [ -z "${BINDIR:-}" ]; then
    bindir="$HOME/.local/bin"   # fall back to a user dir when no write access
  fi
fi
mkdir -p "$bindir"

dest="$bindir/mail-cli"
if [ -w "$bindir" ]; then
  mv "$tmp" "$dest"
elif command -v sudo >/dev/null 2>&1; then
  info "installing to $dest (needs sudo)"
  sudo mv "$tmp" "$dest"
else
  err "cannot write to $bindir and sudo is unavailable; set BINDIR to a writable path"
fi
trap - EXIT
info "installed mail-cli -> $dest"

case ":$PATH:" in
  *":$bindir:"*) : ;;
  *) info "note: $bindir is not on your PATH — add it to use 'mail-cli' directly" ;;
esac

# --- bundled agent skill --------------------------------------------------
if [ "${INSTALL_SKILL:-1}" != 0 ]; then
  info "installing bundled agent skill..."
  "$dest" install-skill || info "skipped skill install (run 'mail-cli install-skill' later)"
fi

info "done. Run 'mail-cli --help' to get started."
