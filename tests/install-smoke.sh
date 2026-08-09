#!/usr/bin/env sh
set -eu

: "${LUMIC_TEST_BINARY:?set LUMIC_TEST_BINARY to the mounted static lumic binary}"
export LUMIC_INSTALL_BINARY="$LUMIC_TEST_BINARY"
export LUMIC_INSTALL_DIR="/usr/local/bin"

/install.sh
lumic version
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
