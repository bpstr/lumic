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

"$LUMIC_BIN" app create shop --domain shop.example --runtime php --json > "$TEST_ROOT/app.json"
APP_ROOT="$LUMIC_APPS_ROOT/shop"
printf '%s\n' '{"require":{"laravel/framework":"^12.0","laravel/horizon":"^5.0"}}' > "$APP_ROOT/composer.json"
printf '%s\n' '<?php' > "$APP_ROOT/artisan"
printf '%s\n' '# preserved comment' 'APP_NAME=Shop' 'APP_KEY=epic-f-secret-value' > "$APP_ROOT/.env"

"$LUMIC_BIN" intelligence catalog > "$TEST_ROOT/catalog.json"
"$LUMIC_BIN" intelligence fingerprint shop > "$TEST_ROOT/fingerprint.json"
"$LUMIC_BIN" intelligence config shop > "$TEST_ROOT/config.json"
"$LUMIC_BIN" intelligence plan shop > "$TEST_ROOT/plan.json"
"$LUMIC_BIN" intelligence graph shop > "$TEST_ROOT/graph.json"
"$LUMIC_BIN" operations provider-signal deployment.health_failed application shop \
  --severity error --summary 'controlled post-deploy health failure' \
  --payload '{"password":"must-not-escape","health":"failed"}' > "$TEST_ROOT/signal.json"
"$LUMIC_BIN" intelligence incident --app shop --limit 20 > "$TEST_ROOT/incident.json"

grep -q 'laravel-redis@1' "$TEST_ROOT/catalog.json"
grep -q '"framework": "laravel"' "$TEST_ROOT/fingerprint.json"
grep -q '"confidence": "high"' "$TEST_ROOT/fingerprint.json"
grep -q '"secret_values_exposed": false' "$TEST_ROOT/config.json"
grep -q '"install_required": true' "$TEST_ROOT/plan.json"
grep -q 'REDIS_HOST' "$TEST_ROOT/plan.json"
grep -q 'service:redis' "$TEST_ROOT/plan.json"
grep -q 'application:shop' "$TEST_ROOT/graph.json"
grep -q 'controlled post-deploy health failure' "$TEST_ROOT/incident.json"
grep -q '\[redacted\]' "$TEST_ROOT/incident.json"

if grep -Fq 'epic-f-secret-value' "$TEST_ROOT/fingerprint.json" "$TEST_ROOT/config.json" "$TEST_ROOT/plan.json" "$TEST_ROOT/graph.json"; then
  echo 'application intelligence output disclosed a dotenv secret' >&2
  exit 1
fi
if grep -Fq 'must-not-escape' "$TEST_ROOT/incident.json"; then
  echo 'incident context did not redact a sensitive payload field' >&2
  exit 1
fi

if [ "${LUMIC_EPIC_F_LIVE:-0}" = "1" ]; then
  "$LUMIC_BIN" intelligence apply shop > "$TEST_ROOT/apply.json"
  SNAPSHOT_ID="$(jq -r '.snapshot_id' "$TEST_ROOT/apply.json")"
  test "$SNAPSHOT_ID" != null
  grep -q '^REDIS_HOST="127.0.0.1"$' "$APP_ROOT/.env"
  grep -q '^CACHE_STORE="redis"$' "$APP_ROOT/.env"
  grep -q '^SESSION_DRIVER="redis"$' "$APP_ROOT/.env"
  grep -q '^QUEUE_CONNECTION="redis"$' "$APP_ROOT/.env"
  grep -q '^# preserved comment$' "$APP_ROOT/.env"
  "$LUMIC_BIN" app inspect shop --json > "$TEST_ROOT/app-after-apply.json"
  grep -q '"service_id": "redis"' "$TEST_ROOT/app-after-apply.json"
  "$LUMIC_BIN" managed-service inspect redis > "$TEST_ROOT/redis.json"
  grep -q '"health": "healthy"' "$TEST_ROOT/redis.json"
  "$LUMIC_BIN" intelligence rollback shop "$SNAPSHOT_ID" > "$TEST_ROOT/rollback.json"
  grep -q '^APP_KEY=epic-f-secret-value$' "$APP_ROOT/.env"
  if grep -q '^REDIS_HOST=' "$APP_ROOT/.env"; then
    echo 'configuration rollback did not restore the original dotenv file' >&2
    exit 1
  fi
fi

echo 'Epic F application and incident intelligence smoke test passed.'
