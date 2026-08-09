#!/usr/bin/env sh
set -eu

: "${LUMIC_TEST_BINARY:?set LUMIC_TEST_BINARY to the mounted static lumic binary}"
export LUMIC_INSTALL_BINARY="$LUMIC_TEST_BINARY"
if [ -n "${LUMIC_TEST_DAEMON_BINARY:-}" ]; then
  export LUMIC_INSTALL_DAEMON_BINARY="$LUMIC_TEST_DAEMON_BINARY"
fi
export LUMIC_INSTALL_DIR="/usr/local/bin"

/install.sh
lumic version
if [ -n "${LUMIC_TEST_DAEMON_BINARY:-}" ]; then
  lumicd --version
  test -f /etc/systemd/system/lumicd.service
  grep -q '^ExecStart=/usr/local/bin/lumicd$' /etc/systemd/system/lumicd.service
fi
STATUS_JSON="$(lumic status --json)"
printf '%s\n' "$STATUS_JSON"
printf '%s\n' "$STATUS_JSON" | grep -q '"operating_system": "linux"'
printf '%s\n' "$STATUS_JSON" | grep -q '"distribution"'
printf '%s\n' "$STATUS_JSON" | grep -q '"hostname"'
printf '%s\n' "$STATUS_JSON" | grep -q '"cpu_count"'
printf '%s\n' "$STATUS_JSON" | grep -q '"memory"'
printf '%s\n' "$STATUS_JSON" | grep -q '"disks"'

# Installer should be idempotent for the same binary.
/install.sh
lumic version
if [ -n "${LUMIC_TEST_DAEMON_BINARY:-}" ]; then
  lumicd --version
fi
