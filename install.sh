#!/usr/bin/env sh
set -eu

REPO="bpstr/lumic"
CHANNEL="${LUMIC_CHANNEL:-stable}"
INSTALL_DIR="${LUMIC_INSTALL_DIR:-/usr/local/bin}"
LOCAL_BINARY="${LUMIC_INSTALL_BINARY:-}"

fail() { printf 'lumic: %s\n' "$*" >&2; exit 1; }
info() { printf 'lumic: %s\n' "$*"; }

[ "$(id -u)" -eq 0 ] || fail "run installer as root (or with sudo)"
[ -r /etc/os-release ] || fail "cannot detect Linux distribution: /etc/os-release missing"

# shellcheck disable=SC1091
. /etc/os-release
case "${ID:-}" in
  ubuntu|debian) ;;
  *) fail "unsupported distribution '${ID:-unknown}' (v2 currently targets Debian/Ubuntu)" ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) TARGET="x86_64-unknown-linux-musl" ;;
  aarch64|arm64) TARGET="aarch64-unknown-linux-musl" ;;
  *) fail "unsupported architecture '$ARCH'" ;;
esac

mkdir -p "$INSTALL_DIR"
DEST="$INSTALL_DIR/lumic"
TMP="${TMPDIR:-/tmp}/lumic-install.$$"
trap 'rm -f "$TMP"' EXIT INT TERM

if [ -n "$LOCAL_BINARY" ]; then
  [ -f "$LOCAL_BINARY" ] || fail "LUMIC_INSTALL_BINARY does not exist: $LOCAL_BINARY"
  cp "$LOCAL_BINARY" "$TMP"
else
  command -v curl >/dev/null 2>&1 || fail "curl is required for remote installation"
  case "$CHANNEL" in
    stable) URL="https://github.com/$REPO/releases/latest/download/lumic-$TARGET" ;;
    nightly) URL="https://github.com/$REPO/releases/download/nightly/lumic-$TARGET" ;;
    *) fail "unknown LUMIC_CHANNEL '$CHANNEL' (expected stable or nightly)" ;;
  esac
  info "downloading $CHANNEL build for $ID/$ARCH"
  curl -fL --retry 3 "$URL" -o "$TMP" || fail "download failed; releases may not exist yet"
fi

chmod 0755 "$TMP"
"$TMP" version >/dev/null 2>&1 || fail "downloaded binary failed verification"
install -m 0755 "$TMP" "$DEST"
info "installed $($DEST version) to $DEST"
info "server ready for Lumic bootstrap development"
