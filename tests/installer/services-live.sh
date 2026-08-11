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
  "$LUMIC_BIN" app delete service-demo >/dev/null 2>&1 || true
  redis-cli --raw DEL lumic:installer:probe >/dev/null 2>&1 || true
  mysql --protocol=socket --execute "DROP DATABASE IF EXISTS app_primary; DROP DATABASE IF EXISTS app_audit; DROP USER IF EXISTS 'app_primary_user'@'localhost'; DROP USER IF EXISTS 'app_audit_user'@'localhost'" >/dev/null 2>&1 || true
  runuser -u postgres -- dropdb --if-exists demo_db >/dev/null 2>&1 || true
  runuser -u postgres -- dropuser --if-exists demo_user >/dev/null 2>&1 || true
  if [[ "$owns_test_root" == 1 ]]; then
    rm -rf "$INSTALLER_TEST_ROOT"
  fi
}
trap cleanup EXIT INT TERM

"$SCRIPT_DIR/postgresql-live.sh"
"$SCRIPT_DIR/mysql-live.sh"
"$SCRIPT_DIR/redis-live.sh"

"$LUMIC_BIN" app create service-demo --domain service.example.test --runtime static --json >/dev/null
"$LUMIC_BIN" managed-service attach primary-db service-demo --role database --database demo_db --user demo_user >"$INSTALLER_TEST_ROOT/app-postgresql.json"
"$LUMIC_BIN" managed-service attach mysql service-demo --role primary --database app_primary --user app_primary_user >"$INSTALLER_TEST_ROOT/app-mysql-primary.json"
"$LUMIC_BIN" managed-service attach mysql service-demo --role audit --database app_audit --user app_audit_user >"$INSTALLER_TEST_ROOT/app-mysql-audit.json"
assert_binding_exists secret:// "$LUMIC_STATE_DIR/resources.json"
assert_secret_not_in_state "$LUMIC_STATE_DIR/resources.json"
assert_no_duplicate_resources "$LUMIC_STATE_DIR/resources.json"
"$LUMIC_BIN" managed-service inspect primary-db >"$INSTALLER_TEST_ROOT/postgresql-status.json"
assert_file_contains "$INSTALLER_TEST_ROOT/postgresql-status.json" postgresql.service
"$LUMIC_BIN" events --json >"$INSTALLER_TEST_ROOT/events.json"
assert_file_contains "$INSTALLER_TEST_ROOT/events.json" managed_service
write_installer_result managed-services application passed
