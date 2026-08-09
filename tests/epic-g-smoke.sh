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

"$LUMIC_BIN" personality show > "$TEST_ROOT/default-personality.txt"
"$LUMIC_BIN" personality set grumpy --json > "$TEST_ROOT/personality.json"
"$LUMIC_BIN" operations provider-signal deployment.recovered application demo \
  --severity info --summary 'controlled Epic G recovery event' > "$TEST_ROOT/event.json"
"$LUMIC_BIN" operations timeline --event-type deployment.recovered > "$TEST_ROOT/timeline.json"
"$LUMIC_BIN" how-are-you --period-hours 24 > "$TEST_ROOT/attention.txt"
"$LUMIC_BIN" how-are-you --period-hours 24 --json > "$TEST_ROOT/attention.json"

grep -qx 'professional' "$TEST_ROOT/default-personality.txt"
grep -q '"value": "grumpy"' "$TEST_ROOT/personality.json"
grep -q '^HEALTH: ' "$TEST_ROOT/attention.txt"
grep -q 'node.personality.changed for node:local' "$TEST_ROOT/attention.txt"
grep -q '"personality": "grumpy"' "$TEST_ROOT/attention.json"
grep -q '"facts": \[' "$TEST_ROOT/attention.json"
grep -q '"active_incidents": \[' "$TEST_ROOT/attention.json"
grep -q '"upcoming_attention": \[' "$TEST_ROOT/attention.json"
grep -q '"recommendations": \[' "$TEST_ROOT/attention.json"
grep -q 'controlled Epic G recovery event' "$TEST_ROOT/event.json"
grep -q 'controlled Epic G recovery event' "$TEST_ROOT/timeline.json"

if "$LUMIC_BIN" how-are-you --period-hours 0 >/dev/null 2>&1; then
  echo 'attention accepted an invalid zero-hour period' >&2
  exit 1
fi

echo 'Epic G attention and personality smoke test passed.'
