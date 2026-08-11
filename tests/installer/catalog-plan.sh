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
while IFS= read -r definition; do
  service="plan-$definition"
  plan="$INSTALLER_TEST_ROOT/$definition-plan.json"
  dry_run="$INSTALLER_TEST_ROOT/$definition-dry-run.json"
  "$LUMIC_BIN" managed-service plan-install "$service" "$definition" >"$plan"
  assert_json '.changes | any(.capability == "managed_service.install")' "$plan"
  "$LUMIC_BIN" managed-service install "$service" "$definition" --dry-run >"$dry_run"
  assert_json '.changed == false and (.message | contains("dry run"))' "$dry_run"
done < <(jq -r '.[].id' "$catalog")

write_installer_result catalog plan passed
