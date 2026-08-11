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
  plan_error="$INSTALLER_TEST_ROOT/$definition-plan.error"
  dry_run="$INSTALLER_TEST_ROOT/$definition-dry-run.json"
  dry_run_error="$INSTALLER_TEST_ROOT/$definition-dry-run.error"
  if ! "$LUMIC_BIN" managed-service plan-install "$service" "$definition" >"$plan" 2>"$plan_error"; then
    assert_file_contains "$plan_error" 'has no managed-service driver'
    continue
  fi
  assert_json '.changes | any(.capability == "managed_service.install")' "$plan"
  if "$LUMIC_BIN" managed-service install "$service" "$definition" --dry-run >"$dry_run" 2>"$dry_run_error"; then
    assert_json '.changed == false and (.message | contains("dry run"))' "$dry_run"
  elif grep -Fq 'no install candidate for' "$dry_run_error"; then
    assert_file_contains "$dry_run_error" 'configure a trusted apt source before setup'
  else
    assert_file_contains "$dry_run_error" 'has no managed-service driver'
  fi
done < <(jq -r '.[].id' "$catalog")

write_installer_result catalog plan passed
