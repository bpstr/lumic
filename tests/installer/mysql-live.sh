#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

"$LUMIC_BIN" managed-service install mysql mysql >"$INSTALLER_TEST_ROOT/mysql.json"
assert_json '.changed == true and (.message | startswith("healthy:"))' "$INSTALLER_TEST_ROOT/mysql.json"
assert_idempotent_install mysql mysql "$INSTALLER_TEST_ROOT/mysql-second.json"
"$LUMIC_BIN" managed-service detect mysql >"$INSTALLER_TEST_ROOT/mysql-detect.json"
assert_file_contains "$INSTALLER_TEST_ROOT/mysql-detect.json" mysql.service
assert_service_active mysql.service
assert_port_listening 3306

for database in app_primary app_audit; do
  user="${database}_user"
  "$LUMIC_BIN" managed-service database-create mysql "$database" >"$INSTALLER_TEST_ROOT/$database.json"
  "$LUMIC_BIN" managed-service user-create mysql "$user" >"$INSTALLER_TEST_ROOT/$user.json"
  assert_json '.secret_reference | startswith("secret://")' "$INSTALLER_TEST_ROOT/$user.json"
  "$LUMIC_BIN" managed-service grant mysql "$database" "$user" >/dev/null
done
mysql --protocol=socket --batch --skip-column-names app_primary <<'SQL'
CREATE TABLE lumic_installer_probe (value varchar(64) NOT NULL);
INSERT INTO lumic_installer_probe VALUES ('mysql-ok');
SQL
test "$(mysql --protocol=socket --batch --skip-column-names app_primary --execute 'SELECT value FROM lumic_installer_probe LIMIT 1')" = mysql-ok
"$LUMIC_BIN" managed-service backup mysql --database app_primary >"$INSTALLER_TEST_ROOT/mysql-backup.json"
assert_json '.status == "completed"' "$INSTALLER_TEST_ROOT/mysql-backup.json"
backup_id="$(jq -r '.id' "$INSTALLER_TEST_ROOT/mysql-backup.json")"
"$LUMIC_BIN" managed-service backup-verify "$backup_id" >"$INSTALLER_TEST_ROOT/mysql-backup-verify.json"
assert_json '.format_valid == true' "$INSTALLER_TEST_ROOT/mysql-backup-verify.json"
write_installer_result mysql live passed
