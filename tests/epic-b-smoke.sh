#!/usr/bin/env sh
set -eu

: "${LUMIC_TEST_BINARY:?set LUMIC_TEST_BINARY to the built lumic CLI}"
case "$LUMIC_TEST_BINARY" in
  /*) LUMIC_BIN="$LUMIC_TEST_BINARY" ;;
  *) LUMIC_BIN="$(pwd)/$LUMIC_TEST_BINARY" ;;
esac

TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT INT TERM
export LUMIC_STATE_DIR="$TEST_ROOT/state"
export LUMIC_APPS_ROOT="$TEST_ROOT/apps"

"$LUMIC_BIN" managed-service plan-install primary-db postgresql > "$TEST_ROOT/postgresql-plan.json"
grep -q 'managed_service.install' "$TEST_ROOT/postgresql-plan.json"
"$LUMIC_BIN" managed-service plan-install mysql mysql > "$TEST_ROOT/mysql-plan.json"
grep -q 'managed_service.install' "$TEST_ROOT/mysql-plan.json"
"$LUMIC_BIN" managed-service plan-install cache redis > "$TEST_ROOT/redis-plan.json"
grep -q 'managed_service.install' "$TEST_ROOT/redis-plan.json"
"$LUMIC_BIN" managed-service install primary-db postgresql --dry-run > "$TEST_ROOT/postgresql-dry-run.json"
grep -q 'dry run' "$TEST_ROOT/postgresql-dry-run.json"
"$LUMIC_BIN" managed-service install mysql mysql --dry-run > "$TEST_ROOT/mysql-dry-run.json"
grep -q 'dry run' "$TEST_ROOT/mysql-dry-run.json"
"$LUMIC_BIN" managed-service install cache redis --dry-run > "$TEST_ROOT/redis-dry-run.json"
grep -q 'dry run' "$TEST_ROOT/redis-dry-run.json"

TOKEN_OUTPUT="$("$LUMIC_BIN" ui token rotate)"
printf '%s\n' "$TOKEN_OUTPUT" | grep -q 'shown once'
test -s "$LUMIC_STATE_DIR/ui-admin-token.sha256"
if printf '%s\n' "$TOKEN_OUTPUT" | grep -F -f "$LUMIC_STATE_DIR/ui-admin-token.sha256" >/dev/null 2>&1; then
  echo 'stored UI credential must not be the printed token' >&2
  exit 1
fi

if [ "${LUMIC_MANAGED_SERVICE_LIVE:-0}" != 1 ]; then
  exit 0
fi

"$LUMIC_BIN" managed-service install primary-db postgresql > "$TEST_ROOT/postgresql.json"
grep -q 'healthy' "$TEST_ROOT/postgresql.json"
"$LUMIC_BIN" managed-service detect postgresql > "$TEST_ROOT/postgresql-detect.json"
grep -q 'postgresql.service' "$TEST_ROOT/postgresql-detect.json"
"$LUMIC_BIN" managed-service user-create primary-db demo_user > "$TEST_ROOT/user.json"
grep -q 'secret_reference' "$TEST_ROOT/user.json"
"$LUMIC_BIN" managed-service database-create primary-db demo_db --owner demo_user > "$TEST_ROOT/database.json"
grep -q 'demo_db' "$TEST_ROOT/database.json"
"$LUMIC_BIN" managed-service grant primary-db demo_db demo_user >/dev/null
"$LUMIC_BIN" managed-service backup primary-db --database demo_db > "$TEST_ROOT/backup.json"
grep -q 'completed' "$TEST_ROOT/backup.json"

"$LUMIC_BIN" managed-service install mysql mysql > "$TEST_ROOT/mysql.json"
grep -q 'healthy' "$TEST_ROOT/mysql.json"
"$LUMIC_BIN" managed-service detect mysql > "$TEST_ROOT/mysql-detect.json"
grep -q 'mysql.service' "$TEST_ROOT/mysql-detect.json"
for database in app_primary app_audit; do
  user="${database}_user"
  "$LUMIC_BIN" managed-service database-create mysql "$database" > "$TEST_ROOT/$database.json"
  "$LUMIC_BIN" managed-service user-create mysql "$user" > "$TEST_ROOT/$user.json"
  grep -q 'secret_reference' "$TEST_ROOT/$user.json"
  "$LUMIC_BIN" managed-service grant mysql "$database" "$user" >/dev/null
done
"$LUMIC_BIN" managed-service backup mysql --database app_primary > "$TEST_ROOT/mysql-backup.json"
grep -q 'completed' "$TEST_ROOT/mysql-backup.json"
MYSQL_BACKUP_ID="$(sed -n 's/.*"id": "\([^"]*\)".*/\1/p' "$TEST_ROOT/mysql-backup.json" | head -n 1)"
"$LUMIC_BIN" managed-service backup-verify "$MYSQL_BACKUP_ID" > "$TEST_ROOT/mysql-backup-verify.json"
grep -q '"format_valid": true' "$TEST_ROOT/mysql-backup-verify.json"

"$LUMIC_BIN" managed-service install cache redis > "$TEST_ROOT/redis.json"
grep -q 'healthy' "$TEST_ROOT/redis.json"
"$LUMIC_BIN" managed-service declare-dependency primary-db cache --purpose 'application cache' > "$TEST_ROOT/dependency.json"
grep -q 'dependency declared' "$TEST_ROOT/dependency.json"
"$LUMIC_BIN" managed-service backup cache > "$TEST_ROOT/redis-backup.json"
grep -q 'completed' "$TEST_ROOT/redis-backup.json"
"$LUMIC_BIN" managed-service restart cache > "$TEST_ROOT/restart.json"
grep -q 'healthy' "$TEST_ROOT/restart.json"

"$LUMIC_BIN" app create service-demo --domain service.example.test --runtime static --json >/dev/null
"$LUMIC_BIN" managed-service attach primary-db service-demo --role database --database demo_db --user demo_user > "$TEST_ROOT/app.json"
grep -q 'primary-db' "$TEST_ROOT/app.json"
"$LUMIC_BIN" managed-service attach mysql service-demo --role primary --database app_primary --user app_primary_user >/dev/null
"$LUMIC_BIN" managed-service attach mysql service-demo --role audit --database app_audit --user app_audit_user > "$TEST_ROOT/app-mysql.json"
grep -q 'app_primary' "$TEST_ROOT/app-mysql.json"
grep -q 'app_audit' "$TEST_ROOT/app-mysql.json"
grep -q 'secret://' "$LUMIC_STATE_DIR/resources.json"
"$LUMIC_BIN" managed-service inspect primary-db > "$TEST_ROOT/status.json"
grep -q 'postgresql.service' "$TEST_ROOT/status.json"
"$LUMIC_BIN" events --json > "$TEST_ROOT/events.json"
grep -q 'managed_service' "$TEST_ROOT/events.json"
