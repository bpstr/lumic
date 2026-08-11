#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

"$LUMIC_BIN" managed-service install cache redis >"$INSTALLER_TEST_ROOT/redis.json"
assert_json '.changed == true and (.message | startswith("healthy:"))' "$INSTALLER_TEST_ROOT/redis.json"
assert_idempotent_install cache redis "$INSTALLER_TEST_ROOT/redis-second.json"
assert_service_active redis-server.service
assert_port_listening 6379
test "$(redis-cli --raw PING)" = PONG
redis-cli --raw SET lumic:installer:probe redis-ok >/dev/null
test "$(redis-cli --raw GET lumic:installer:probe)" = redis-ok

"$LUMIC_BIN" managed-service configure cache --setting maxmemory=134217728 --setting maxmemory_policy=allkeys-lru >"$INSTALLER_TEST_ROOT/redis-configure.json"
assert_json '.changed == true' "$INSTALLER_TEST_ROOT/redis-configure.json"
test "$(redis-cli --raw CONFIG GET maxmemory | tail -n 1)" = 134217728
"$LUMIC_BIN" managed-service configure cache --setting maxmemory=268435456 --setting maxmemory_policy=allkeys-lru >"$INSTALLER_TEST_ROOT/redis-reconfigure.json"
assert_json '.changed == true' "$INSTALLER_TEST_ROOT/redis-reconfigure.json"
test "$(redis-cli --raw CONFIG GET maxmemory | tail -n 1)" = 268435456
"$LUMIC_BIN" managed-service configure cache --setting maxmemory=268435456 --setting maxmemory_policy=allkeys-lru >"$INSTALLER_TEST_ROOT/redis-configure-second.json"
assert_json '.changed == false' "$INSTALLER_TEST_ROOT/redis-configure-second.json"

"$LUMIC_BIN" managed-service declare-dependency primary-db cache --purpose 'application cache' >"$INSTALLER_TEST_ROOT/dependency.json"
"$LUMIC_BIN" managed-service backup cache >"$INSTALLER_TEST_ROOT/redis-backup.json"
assert_json '.status == "completed"' "$INSTALLER_TEST_ROOT/redis-backup.json"
"$LUMIC_BIN" managed-service restart cache >"$INSTALLER_TEST_ROOT/redis-restart.json"
assert_file_contains "$INSTALLER_TEST_ROOT/redis-restart.json" healthy
write_installer_result redis live passed
