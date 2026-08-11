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

"$LUMIC_BIN" environment secret-generate incident-key > "$TEST_ROOT/secret.json"
SECRET_VALUE="$(sed -n '1p' "$LUMIC_STATE_DIR/secrets/incident-key")"
"$LUMIC_BIN" operations webhook-plan local-hook \
  https://hooks.example.test/hook incident-key > "$TEST_ROOT/webhook-plan.json"
grep -q 'operations.configuration.apply' "$TEST_ROOT/webhook-plan.json"
"$LUMIC_BIN" operations webhook-apply local-hook \
  https://hooks.example.test/hook incident-key > "$TEST_ROOT/webhook.json"
"$LUMIC_BIN" operations subscribe failures local-hook \
  --event provider.failed > "$TEST_ROOT/subscription.json"

"$LUMIC_BIN" operations provider-signal provider.failed provider demo \
  --severity error --summary 'controlled reference failure' \
  --payload '{"check":"failed","secret_reference":"incident-key"}' \
  > "$TEST_ROOT/signal.json"
"$LUMIC_BIN" operations timeline --entity-id demo --limit 10 \
  > "$TEST_ROOT/timeline.json"
"$LUMIC_BIN" operations incident --entity-id demo --limit 10 \
  > "$TEST_ROOT/incident.json"
"$LUMIC_BIN" operations deliveries --limit 10 > "$TEST_ROOT/deliveries.json"
"$LUMIC_BIN" audit --json > "$TEST_ROOT/audit.json"

grep -q 'controlled reference failure' "$TEST_ROOT/timeline.json"
grep -q 'provider.failed' "$TEST_ROOT/incident.json"
grep -q 'failure findings' "$TEST_ROOT/incident.json"
grep -q '"status": "pending"' "$TEST_ROOT/deliveries.json"
grep -q 'operations.subscription.configure' "$TEST_ROOT/audit.json"
if grep -Fq "$SECRET_VALUE" "$TEST_ROOT/webhook-plan.json" "$TEST_ROOT/webhook.json" \
  "$TEST_ROOT/subscription.json" "$TEST_ROOT/signal.json" "$TEST_ROOT/timeline.json" \
  "$TEST_ROOT/incident.json" "$TEST_ROOT/deliveries.json" "$TEST_ROOT/audit.json"; then
  echo 'operations output disclosed a secret value' >&2
  exit 1
fi

"$LUMIC_BIN" operations rollback-configuration > "$TEST_ROOT/rollback.json"
grep -q '"restored": true' "$TEST_ROOT/rollback.json"
"$LUMIC_BIN" operations deliveries --limit 10 > "$TEST_ROOT/post-rollback-deliveries.json"
grep -q '"status": "pending"' "$TEST_ROOT/post-rollback-deliveries.json"

if "$LUMIC_BIN" operations webhook-plan unsafe http://example.test/hook incident-key \
  > /dev/null 2>&1; then
  echo 'remote plaintext webhook was accepted' >&2
  exit 1
fi

echo 'Epic E operational history and automation smoke test passed.'
