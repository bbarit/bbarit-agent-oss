#!/bin/sh
# bbarit-oss installer.  Usage:
#   curl -fsSL https://bbarit.com/agent/install.sh | sh
#
# Downloads a prebuilt `bbarit-oss` binary for this platform and installs it into
# ~/.local/bin (override with BBARIT_INSTALL_DIR).
set -eu

BASE_URL="${BBARIT_UPDATE_BASE:-https://bbarit.com/agent}"
INSTALL_DIR="${BBARIT_INSTALL_DIR:-$HOME/.local/bin}"

say()  { printf '\033[1;31mbbarit-oss\033[0m %s\n' "$1"; }
err()  { printf '\033[1;31mbbarit-oss error:\033[0m %s\n' "$1" >&2; exit 1; }

# --- detect platform --------------------------------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Darwin) case "$arch" in
            arm64|aarch64) target="macos-arm64" ;;
            x86_64)        target="macos-x64" ;;
            *) err "unsupported macOS arch: $arch" ;;
          esac ;;
  Linux)  case "$arch" in
            x86_64)        target="linux-x64" ;;
            aarch64|arm64) target="linux-arm64" ;;
            *) err "unsupported Linux arch: $arch" ;;
          esac ;;
  *) err "unsupported OS: $os (Windows: download from the GitHub releases page)" ;;
esac

command -v curl >/dev/null 2>&1 || err "curl is required"

# --- resolve version + download URL from the manifest -----------------------
manifest="$(curl -fsSL "$BASE_URL/latest.json")" || err "cannot reach $BASE_URL/latest.json"
version="$(printf '%s' "$manifest" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
[ -n "$version" ] || err "could not read version from manifest"

url="$(printf '%s' "$manifest" \
  | tr ',{}' '\n\n\n' \
  | sed -n "s/.*\"$target\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" \
  | head -n1)"
[ -n "$url" ] || url="$BASE_URL/dist/$version/bbarit-oss-$target"

say "installing v$version ($target) → $INSTALL_DIR/bbarit-oss"
mkdir -p "$INSTALL_DIR"
# Download into the install dir itself so the final mv is a same-filesystem
# atomic rename (a /tmp temp file can cross filesystems, where mv degrades to
# a non-atomic copy and can hit ETXTBSY over a running binary).
tmp="$INSTALL_DIR/.bbarit-oss.download.$$"
trap 'rm -f "$tmp"' EXIT
curl -fsSL "$url" -o "$tmp" || err "download failed: $url"
[ "$(wc -c < "$tmp")" -gt 1024 ] || err "downloaded file looks too small"
chmod +x "$tmp"
mv -f "$tmp" "$INSTALL_DIR/bbarit-oss"

say "installed. Run:  bbarit-oss --help"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) : ;;
  *) say "NOTE: add $INSTALL_DIR to your PATH:"
     printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR" ;;
esac
