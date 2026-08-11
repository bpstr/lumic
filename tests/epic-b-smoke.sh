#!/usr/bin/env bash
set -euo pipefail

: "${LUMIC_TEST_BINARY:?set LUMIC_TEST_BINARY to the built lumic CLI}"
case "$LUMIC_TEST_BINARY" in
  /*) LUMIC_BIN="$LUMIC_TEST_BINARY" ;;
  *) LUMIC_BIN="$(pwd)/$LUMIC_TEST_BINARY" ;;
esac

TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT INT TERM
export LUMIC_STATE_DIR="$TEST_ROOT/state"
export LUMIC_APPS_ROOT="$TEST_ROOT/apps"
export INSTALLER_TEST_ROOT="$TEST_ROOT"
export INSTALLER_RESULTS_DIR="${INSTALLER_RESULTS_DIR:-$TEST_ROOT/results}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$SCRIPT_DIR/installer/catalog-definition.sh"
"$SCRIPT_DIR/installer/catalog-plan.sh"

TOKEN_OUTPUT="$("$LUMIC_BIN" ui token rotate)"
printf '%s\n' "$TOKEN_OUTPUT" | grep -q 'shown once'
test -s "$LUMIC_STATE_DIR/ui-admin-token.sha256"
if printf '%s\n' "$TOKEN_OUTPUT" | grep -F -f "$LUMIC_STATE_DIR/ui-admin-token.sha256" >/dev/null 2>&1; then
  echo 'stored UI credential must not be the printed token' >&2
  exit 1
fi

if [[ "${LUMIC_MANAGED_SERVICE_LIVE:-0}" != 1 ]]; then
  exit 0
fi
"$SCRIPT_DIR/installer/services-live.sh"
