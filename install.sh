#!/usr/bin/env sh
set -eu

REPO="bpstr/lumic"
CHANNEL="${LUMIC_CHANNEL:-stable}"
REQUESTED_VERSION="${LUMIC_VERSION:-}"
INSTALL_DIR="${LUMIC_INSTALL_DIR:-/usr/local/bin}"
LOCAL_BINARY="${LUMIC_INSTALL_BINARY:-}"
LOCAL_DAEMON_BINARY="${LUMIC_INSTALL_DAEMON_BINARY:-}"
CONFIG_DIR="${LUMIC_CONFIG_DIR:-/etc/lumic}"
STATE_DIR="${LUMIC_STATE_DIR:-/var/lib/lumic}"

fail() { printf 'lumic: %s\n' "$*" >&2; exit 1; }
info() { printf 'lumic: %s\n' "$*"; }

[ "$(id -u)" -eq 0 ] || fail "run installer as root (or with sudo)"
[ -r /etc/os-release ] || fail "cannot detect Linux distribution: /etc/os-release missing"

case "$INSTALL_DIR" in /*) ;; *) fail "LUMIC_INSTALL_DIR must be an absolute path" ;; esac
case "$CONFIG_DIR" in /*) ;; *) fail "LUMIC_CONFIG_DIR must be an absolute path" ;; esac
case "$STATE_DIR" in /*) ;; *) fail "LUMIC_STATE_DIR must be an absolute path" ;; esac
case "$INSTALL_DIR" in *[!A-Za-z0-9_./-]*) fail "LUMIC_INSTALL_DIR contains unsupported characters" ;; esac
case "$CONFIG_DIR" in *[!A-Za-z0-9_./-]*) fail "LUMIC_CONFIG_DIR contains unsupported characters" ;; esac
case "$STATE_DIR" in *[!A-Za-z0-9_./-]*) fail "LUMIC_STATE_DIR contains unsupported characters" ;; esac
case "$CHANNEL" in stable|nightly) ;; *) fail "unknown LUMIC_CHANNEL '$CHANNEL' (expected stable or nightly)" ;; esac
if [ -n "$REQUESTED_VERSION" ] && ! printf '%s\n' "$REQUESTED_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  fail "invalid LUMIC_VERSION '$REQUESTED_VERSION' (expected MAJOR.MINOR.PATCH without a v prefix)"
fi

# shellcheck disable=SC1091
. /etc/os-release
case "${ID:-}" in
  ubuntu|debian) ;;
  *) fail "unsupported distribution '${ID:-unknown}' (Lumic currently targets Debian/Ubuntu)" ;;
esac

ARCH="$(uname -m)"
TARGET=""
if [ -z "$LOCAL_BINARY" ]; then
  case "$ARCH" in
    x86_64|amd64) TARGET="x86_64-unknown-linux-musl" ;;
    *) fail "unsupported architecture '$ARCH' (Phase 0 release artifacts are x86_64 only)" ;;
  esac
fi

install -d -m 0755 "$INSTALL_DIR" "$CONFIG_DIR"
install -d -m 0700 "$STATE_DIR"
[ -w "$INSTALL_DIR" ] || fail "install directory is not writable: $INSTALL_DIR"
DEST="$INSTALL_DIR/lumic"
DAEMON_DEST="$INSTALL_DIR/lumicd"
TMP="$(mktemp "$INSTALL_DIR/.lumic.XXXXXX")" || fail "cannot create temporary file in $INSTALL_DIR"
CHECKSUM_FILE="$TMP.sha256"
DAEMON_TMP="$(mktemp "$INSTALL_DIR/.lumicd.XXXXXX")" || fail "cannot create daemon temporary file in $INSTALL_DIR"
DAEMON_CHECKSUM_FILE="$DAEMON_TMP.sha256"
BACKUP="$INSTALL_DIR/.lumic.previous"
DAEMON_BACKUP="$INSTALL_DIR/.lumicd.previous"
trap 'rm -f "$TMP" "$CHECKSUM_FILE" "$DAEMON_TMP" "$DAEMON_CHECKSUM_FILE"' EXIT INT TERM

if [ -n "$LOCAL_BINARY" ]; then
  [ -f "$LOCAL_BINARY" ] || fail "LUMIC_INSTALL_BINARY does not exist: $LOCAL_BINARY"
  cp "$LOCAL_BINARY" "$TMP"
  if [ -n "$LOCAL_DAEMON_BINARY" ]; then
    [ -f "$LOCAL_DAEMON_BINARY" ] || fail "LUMIC_INSTALL_DAEMON_BINARY does not exist: $LOCAL_DAEMON_BINARY"
    cp "$LOCAL_DAEMON_BINARY" "$DAEMON_TMP"
  fi
else
  command -v curl >/dev/null 2>&1 || fail "curl is required for remote installation"
  if [ "$CHANNEL" = "nightly" ]; then
    [ -z "$REQUESTED_VERSION" ] || fail "LUMIC_VERSION cannot be combined with the rolling nightly channel"
    URL="https://github.com/$REPO/releases/download/nightly/lumic-$TARGET"
  else
    if [ -n "$REQUESTED_VERSION" ]; then
      URL="https://github.com/$REPO/releases/download/$REQUESTED_VERSION/lumic-$TARGET"
    else
      URL="https://github.com/$REPO/releases/latest/download/lumic-$TARGET"
    fi
  fi
  info "downloading ${REQUESTED_VERSION:-$CHANNEL} build for $ID/$ARCH"
  curl -fL --retry 3 "$URL" -o "$TMP" || fail "download failed from $URL; verify the channel/version and network access"
  curl -fL --retry 3 "$URL.sha256" -o "$CHECKSUM_FILE" || fail "checksum download failed from $URL.sha256"
  EXPECTED_CHECKSUM="$(awk 'NR == 1 { print $1 }' "$CHECKSUM_FILE")"
  [ "${#EXPECTED_CHECKSUM}" -eq 64 ] || fail "release checksum file is invalid"
  case "$EXPECTED_CHECKSUM" in *[!0-9a-fA-F]*) fail "release checksum file is invalid" ;; esac
  ACTUAL_CHECKSUM="$(sha256sum "$TMP" | awk '{ print $1 }')"
  [ "$EXPECTED_CHECKSUM" = "$ACTUAL_CHECKSUM" ] || fail "downloaded binary checksum does not match the release"

  DAEMON_URL="$(dirname "$URL")/lumicd-$TARGET"
  curl -fL --retry 3 "$DAEMON_URL" -o "$DAEMON_TMP" || fail "daemon download failed from $DAEMON_URL"
  curl -fL --retry 3 "$DAEMON_URL.sha256" -o "$DAEMON_CHECKSUM_FILE" || fail "daemon checksum download failed from $DAEMON_URL.sha256"
  EXPECTED_DAEMON_CHECKSUM="$(awk 'NR == 1 { print $1 }' "$DAEMON_CHECKSUM_FILE")"
  [ "${#EXPECTED_DAEMON_CHECKSUM}" -eq 64 ] || fail "daemon release checksum file is invalid"
  case "$EXPECTED_DAEMON_CHECKSUM" in *[!0-9a-fA-F]*) fail "daemon release checksum file is invalid" ;; esac
  ACTUAL_DAEMON_CHECKSUM="$(sha256sum "$DAEMON_TMP" | awk '{ print $1 }')"
  [ "$EXPECTED_DAEMON_CHECKSUM" = "$ACTUAL_DAEMON_CHECKSUM" ] || fail "downloaded daemon checksum does not match the release"
  LOCAL_DAEMON_BINARY="$DAEMON_TMP"
fi

chmod 0755 "$TMP"
INSTALLED_VERSION="$($TMP version 2>/dev/null)" || fail "downloaded binary failed 'lumic version' verification"
case "$INSTALLED_VERSION" in "lumic "*) ;; *) fail "binary returned an unexpected version string" ;; esac
if [ -n "$REQUESTED_VERSION" ] && [ "$INSTALLED_VERSION" != "lumic $REQUESTED_VERSION" ]; then
  fail "requested version $REQUESTED_VERSION but binary reports '$INSTALLED_VERSION'"
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
CHANNEL_TMP="$(mktemp "$CONFIG_DIR/.channel.XXXXXX")" || fail "cannot create channel file in $CONFIG_DIR"
printf '%s\n' "$CHANNEL" > "$CHANNEL_TMP"
chmod 0644 "$CHANNEL_TMP"
mv -f "$CHANNEL_TMP" "$CONFIG_DIR/channel"
if [ -n "$LOCAL_DAEMON_BINARY" ]; then
  chmod 0755 "$DAEMON_TMP"
  DAEMON_VERSION="$($DAEMON_TMP --version 2>/dev/null)" || fail "daemon binary failed 'lumicd --version' verification"
  case "$DAEMON_VERSION" in "lumicd "*) ;; *) fail "daemon returned an unexpected version string" ;; esac
  if [ -n "$REQUESTED_VERSION" ] && [ "$DAEMON_VERSION" != "lumicd $REQUESTED_VERSION" ]; then
    fail "requested version $REQUESTED_VERSION but daemon reports '$DAEMON_VERSION'"
  fi
  if [ -f "$DAEMON_DEST" ] && cmp -s "$DAEMON_TMP" "$DAEMON_DEST"; then
    info "$DAEMON_VERSION is already installed at $DAEMON_DEST"
  else
    if [ -f "$DAEMON_DEST" ]; then
      cp "$DAEMON_DEST" "$DAEMON_BACKUP"
      chmod 0755 "$DAEMON_BACKUP"
    fi
    mv -f "$DAEMON_TMP" "$DAEMON_DEST"
    chmod 0755 "$DAEMON_DEST"
    if ! "$DAEMON_DEST" --version >/dev/null 2>&1; then
      if [ -f "$DAEMON_BACKUP" ]; then
        mv -f "$DAEMON_BACKUP" "$DAEMON_DEST"
      fi
      fail "installed daemon failed post-install verification; previous daemon restored"
    fi
    info "installed $DAEMON_VERSION to $DAEMON_DEST"
  fi

  UNIT_PATH="/etc/systemd/system/lumicd.service"
  install -d -m 0755 /etc/systemd/system
  UNIT_TMP="$(mktemp /etc/systemd/system/.lumicd.XXXXXX)" || fail "cannot create temporary systemd unit"
  trap 'rm -f "$TMP" "$CHECKSUM_FILE" "$DAEMON_TMP" "$DAEMON_CHECKSUM_FILE" "$UNIT_TMP"' EXIT INT TERM
  sed \
    -e "s|@DAEMON_DEST@|$DAEMON_DEST|g" \
    -e "s|@STATE_DIR@|$STATE_DIR|g" > "$UNIT_TMP" <<'EOF'
[Unit]
Description=Lumic host operating layer
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=@DAEMON_DEST@
Environment=LUMIC_STATE_DIR=@STATE_DIR@
Restart=on-failure
RestartSec=3
NoNewPrivileges=yes

[Install]
WantedBy=multi-user.target
EOF
  chmod 0644 "$UNIT_TMP"
  if [ ! -f "$UNIT_PATH" ] || ! cmp -s "$UNIT_TMP" "$UNIT_PATH"; then
    mv -f "$UNIT_TMP" "$UNIT_PATH"
    info "installed systemd unit at $UNIT_PATH"
  fi
  if [ -d /run/systemd/system ]; then
    systemctl daemon-reload
    systemctl enable --now lumicd.service
    info "enabled and started lumicd.service"
  else
    info "systemd is not running; lumicd.service will start on the next boot"
  fi
else
  info "no local daemon artifact supplied; installed CLI without lumicd"
fi
