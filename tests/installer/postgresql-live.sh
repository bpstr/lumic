#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

"$LUMIC_BIN" managed-service install primary-db postgresql >"$INSTALLER_TEST_ROOT/postgresql.json"
assert_json '.changed == true and (.message | startswith("healthy:"))' "$INSTALLER_TEST_ROOT/postgresql.json"
assert_idempotent_install primary-db postgresql "$INSTALLER_TEST_ROOT/postgresql-second.json"
"$LUMIC_BIN" managed-service detect postgresql >"$INSTALLER_TEST_ROOT/postgresql-detect.json"
assert_file_contains "$INSTALLER_TEST_ROOT/postgresql-detect.json" postgresql.service
assert_service_active postgresql.service
assert_port_listening 5432

"$LUMIC_BIN" managed-service user-create primary-db demo_user >"$INSTALLER_TEST_ROOT/postgresql-user.json"
assert_secret_reference "$INSTALLER_TEST_ROOT/postgresql-user.json"
"$LUMIC_BIN" managed-service database-create primary-db demo_db --owner demo_user >"$INSTALLER_TEST_ROOT/postgresql-database.json"
"$LUMIC_BIN" managed-service grant primary-db demo_db demo_user >/dev/null
runuser -u postgres -- psql --no-psqlrc --set ON_ERROR_STOP=1 --dbname demo_db <<'SQL'
CREATE TABLE lumic_installer_probe (value text NOT NULL);
INSERT INTO lumic_installer_probe VALUES ('postgresql-ok');
SQL
test "$(runuser -u postgres -- psql --no-psqlrc --tuples-only --no-align --dbname demo_db --command 'SELECT value FROM lumic_installer_probe LIMIT 1')" = postgresql-ok
"$LUMIC_BIN" managed-service backup primary-db --database demo_db >"$INSTALLER_TEST_ROOT/postgresql-backup.json"
assert_json '.status == "completed"' "$INSTALLER_TEST_ROOT/postgresql-backup.json"
write_installer_result postgresql live passed
