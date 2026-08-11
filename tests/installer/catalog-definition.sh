#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

owns_test_root=0
if [[ -z "${INSTALLER_TEST_ROOT:-}" ]]; then
  INSTALLER_TEST_ROOT="$(mktemp -d)"
  owns_test_root=1
fi
: "${INSTALLER_RESULTS_DIR:=$INSTALLER_TEST_ROOT/results}"
export INSTALLER_TEST_ROOT INSTALLER_RESULTS_DIR
cleanup() {
  if [[ "$owns_test_root" == 1 ]]; then
    rm -rf "$INSTALLER_TEST_ROOT"
  fi
}
trap cleanup EXIT INT TERM

catalog="$INSTALLER_TEST_ROOT/catalog.json"
"$LUMIC_BIN" managed-service catalog >"$catalog"
assert_json 'type == "array" and length > 0' "$catalog"

while IFS= read -r definition; do
  schema="$INSTALLER_TEST_ROOT/schema-$definition.json"
  "$LUMIC_BIN" managed-service schema "$definition" >"$schema"
  jq -e --arg id "$definition" '.id == $id and .schema_version == 1 and .definition_version == 1' "$schema" >/dev/null
done < <(jq -r '.[].id' "$catalog")

write_installer_result catalog definition passed
