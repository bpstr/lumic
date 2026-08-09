#!/usr/bin/env sh
set -eu

REPO="bpstr/lumic"
CHANNEL="${LUMIC_CHANNEL:-stable}"
VERSION="${LUMIC_VERSION:-}"
INSTALL_DIR="${LUMIC_INSTALL_DIR:-/usr/local/bin}"
LOCAL_BINARY="${LUMIC_INSTALL_BINARY:-}"
CONFIG_DIR="${LUMIC_CONFIG_DIR:-/etc/lumic}"
STATE_DIR="${LUMIC_STATE_DIR:-/var/lib/lumic}"

fail() { printf 'lumic: %s\n' "$*" >&2; exit 1; }
info() { printf 'lumic: %s\n' "$*"; }

[ "$(id -u)" -eq 0 ] || fail "run installer as root (or with sudo)"
[ -r /etc/os-release ] || fail "cannot detect Linux distribution: /etc/os-release missing"

case "$INSTALL_DIR" in /*) ;; *) fail "LUMIC_INSTALL_DIR must be an absolute path" ;; esac
case "$CONFIG_DIR" in /*) ;; *) fail "LUMIC_CONFIG_DIR must be an absolute path" ;; esac
case "$STATE_DIR" in /*) ;; *) fail "LUMIC_STATE_DIR must be an absolute path" ;; esac
case "$CHANNEL" in stable|nightly) ;; *) fail "unknown LUMIC_CHANNEL '$CHANNEL' (expected stable or nightly)" ;; esac
case "$VERSION" in *[!0-9A-Za-z._-]*) fail "invalid LUMIC_VERSION '$VERSION'" ;; esac

# shellcheck disable=SC1091
. /etc/os-release
case "${ID:-}" in
  ubuntu|debian) ;;
  *) fail "unsupported distribution '${ID:-unknown}' (v2 currently targets Debian/Ubuntu)" ;;
esac

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) TARGET="x86_64-unknown-linux-musl" ;;
  *) fail "unsupported architecture '$ARCH' (Phase 0 release artifacts are x86_64 only)" ;;
esac

install -d -m 0755 "$INSTALL_DIR" "$CONFIG_DIR"
install -d -m 0700 "$STATE_DIR"
[ -w "$INSTALL_DIR" ] || fail "install directory is not writable: $INSTALL_DIR"
DEST="$INSTALL_DIR/lumic"
TMP="$(mktemp "$INSTALL_DIR/.lumic.XXXXXX")" || fail "cannot create temporary file in $INSTALL_DIR"
CHECKSUM_FILE="$TMP.sha256"
BACKUP="$INSTALL_DIR/.lumic.previous"
trap 'rm -f "$TMP" "$CHECKSUM_FILE"' EXIT INT TERM

if [ -n "$LOCAL_BINARY" ]; then
  [ -f "$LOCAL_BINARY" ] || fail "LUMIC_INSTALL_BINARY does not exist: $LOCAL_BINARY"
  cp "$LOCAL_BINARY" "$TMP"
else
  command -v curl >/dev/null 2>&1 || fail "curl is required for remote installation"
  if [ "$CHANNEL" = "nightly" ]; then
    [ -z "$VERSION" ] || fail "LUMIC_VERSION cannot be combined with the rolling nightly channel"
    URL="https://github.com/$REPO/releases/download/nightly/lumic-$TARGET"
  else
    [ -n "$VERSION" ] || fail "stable releases are not published yet; use LUMIC_CHANNEL=nightly or LUMIC_INSTALL_BINARY for local CI"
    URL="https://github.com/$REPO/releases/download/v$VERSION/lumic-$TARGET"
  fi
  info "downloading ${VERSION:-$CHANNEL} build for $ID/$ARCH"
  curl -fL --retry 3 "$URL" -o "$TMP" || fail "download failed from $URL; verify the channel/version and network access"
  curl -fL --retry 3 "$URL.sha256" -o "$CHECKSUM_FILE" || fail "checksum download failed from $URL.sha256"
  EXPECTED_CHECKSUM="$(awk 'NR == 1 { print $1 }' "$CHECKSUM_FILE")"
  [ "${#EXPECTED_CHECKSUM}" -eq 64 ] || fail "release checksum file is invalid"
  case "$EXPECTED_CHECKSUM" in *[!0-9a-fA-F]*) fail "release checksum file is invalid" ;; esac
  ACTUAL_CHECKSUM="$(sha256sum "$TMP" | awk '{ print $1 }')"
  [ "$EXPECTED_CHECKSUM" = "$ACTUAL_CHECKSUM" ] || fail "downloaded binary checksum does not match the release"
fi

chmod 0755 "$TMP"
INSTALLED_VERSION="$($TMP version 2>/dev/null)" || fail "downloaded binary failed 'lumic version' verification"
case "$INSTALLED_VERSION" in "lumic "*) ;; *) fail "binary returned an unexpected version string" ;; esac
if [ -n "$VERSION" ] && [ "$INSTALLED_VERSION" != "lumic $VERSION" ]; then
  fail "requested version $VERSION but binary reports '$INSTALLED_VERSION'"
fi

if [ -f "$DEST" ] && cmp -s "$TMP" "$DEST"; then
  info "$INSTALLED_VERSION is already installed at $DEST"
else
  if [ -f "$DEST" ]; then
    cp "$DEST" "$BACKUP"
    chmod 0755 "$BACKUP"
  fi
  mv -f "$TMP" "$DEST"
  chmod 0755 "$DEST"
  if ! "$DEST" version >/dev/null 2>&1; then
    if [ -f "$BACKUP" ]; then
      mv -f "$BACKUP" "$DEST"
    fi
    fail "installed binary failed post-install verification; previous binary restored"
  fi
  info "installed $INSTALLED_VERSION to $DEST"
fi

info "prepared $CONFIG_DIR (configuration) and $STATE_DIR (private state)"
if command -v systemctl >/dev/null 2>&1; then
  info "systemd detected; daemon registration will be enabled when lumicd artifacts ship"
fi
