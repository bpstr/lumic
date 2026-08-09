#!/usr/bin/env sh
set -eu

: "${LUMIC_TEST_BINARY:?set LUMIC_TEST_BINARY to the built lumic CLI}"
case "$LUMIC_TEST_BINARY" in
  /*) LUMIC_BIN="$LUMIC_TEST_BINARY" ;;
  *) LUMIC_BIN="$(pwd)/$LUMIC_TEST_BINARY" ;;
esac

TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT INT TERM
A_STATE="$TEST_ROOT/production-state"
A_APPS="$TEST_ROOT/production-apps"
B_STATE="$TEST_ROOT/staging-state"
B_APPS="$TEST_ROOT/staging-apps"

run_a() {
  LUMIC_STATE_DIR="$A_STATE" LUMIC_APPS_ROOT="$A_APPS" "$LUMIC_BIN" "$@"
}

run_b() {
  LUMIC_STATE_DIR="$B_STATE" LUMIC_APPS_ROOT="$B_APPS" "$LUMIC_BIN" "$@"
}

export GIT_AUTHOR_NAME="Lumic CI"
export GIT_AUTHOR_EMAIL="ci@lumic.invalid"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"
SOURCE="$TEST_ROOT/source"
git init -q -b main "$SOURCE"
printf '%s\n' 'shared release' > "$SOURCE/index.html"
git -C "$SOURCE" add index.html
git -C "$SOURCE" commit -q -m initial

run_a infrastructure init production --name Production --role app --role git \
  > "$TEST_ROOT/production-node.json"
run_a app create shop --domain shop.example.test --runtime static \
  > "$TEST_ROOT/production-app.json"
run_a app repository set shop "file://$SOURCE" > "$TEST_ROOT/repository.json"
run_a environment secret-generate production-key > "$TEST_ROOT/production-secret.json"
run_a environment reference-set shop APP_KEY production-key \
  > "$TEST_ROOT/production-reference.json"
run_a git host shop > "$TEST_ROOT/hosted.json"
run_a git mirror source-cache "file://$SOURCE" > "$TEST_ROOT/mirror.json"
run_a git trigger shop shop > "$TEST_ROOT/trigger.json"
test -d "$A_STATE/git/hosted/shop.git"
test -d "$A_STATE/git/mirrors/source-cache.git"
grep -q '/usr/local/bin/lumic git receive shop' \
  "$A_STATE/git/hosted/shop.git/hooks/post-receive"
run_a environment export shop production --tier production \
  --output "$TEST_ROOT/production.bundle.json"
run_a infrastructure enrollment --endpoint https://production.example.test/mcp \
  --output "$TEST_ROOT/production.enrollment.json"

run_b infrastructure init staging --name Staging --role app --role worker \
  > "$TEST_ROOT/staging-node.json"
run_b environment secret-generate staging-key > "$TEST_ROOT/staging-secret.json"
run_b infrastructure enrollment --endpoint https://staging.example.test/mcp \
  --output "$TEST_ROOT/staging.enrollment.json"
run_a infrastructure register "$TEST_ROOT/staging.enrollment.json" \
  > "$TEST_ROOT/production-trust.json"
run_b infrastructure register "$TEST_ROOT/production.enrollment.json" \
  > "$TEST_ROOT/staging-trust.json"

if run_b environment import "$TEST_ROOT/production.bundle.json" \
  --target unsafe-copy --tier staging --domain unsafe.example.test \
  > /dev/null 2>&1; then
  echo 'environment import accepted a source-node secret reference' >&2
  exit 1
fi
run_b environment import "$TEST_ROOT/production.bundle.json" \
  --target shop-staging --tier staging --domain staging.shop.example.test \
  --env APP_KEY=staging-key > "$TEST_ROOT/staging-app.json"
run_b environment export shop-staging staging --tier staging \
  --output "$TEST_ROOT/staging.bundle.json"
run_b environment diff "$TEST_ROOT/production.bundle.json" \
  "$TEST_ROOT/staging.bundle.json" > "$TEST_ROOT/configuration-diff.json"
grep -q 'staging.shop.example.test' "$TEST_ROOT/configuration-diff.json"
grep -q '"sensitive": true' "$TEST_ROOT/configuration-diff.json"
if grep -q 'production-key\|staging-key' "$TEST_ROOT/configuration-diff.json"; then
  echo 'configuration diff disclosed a secret reference' >&2
  exit 1
fi

run_b infrastructure endpoint database-staging \
  --provider-node production --provider-kind managed-service --provider postgres \
  --consumer-node staging --consumer-kind application --consumer shop-staging \
  --protocol tcp --host 127.0.0.1 --port 5432 --secret-reference staging-key \
  > "$TEST_ROOT/endpoint.json"
run_b infrastructure membership --kind worker --environment staging \
  --application shop-staging --node staging > "$TEST_ROOT/membership.json"
run_a infrastructure coordinate release-1 \
  --member production=shop --member staging=shop-staging \
  > "$TEST_ROOT/coordination.json"
COORDINATION_ID="$(sed -n 's/  "id": "\(coordination-[^"]*\)",/\1/p' "$TEST_ROOT/coordination.json" | head -n 1)"
test -n "$COORDINATION_ID"

run_a app deploy shop > "$TEST_ROOT/production-deployment.json"
run_a infrastructure report "$COORDINATION_ID" --node production \
  --status succeeded --healthy true --message deployed \
  > "$TEST_ROOT/production-report.json"
run_a infrastructure sign --target staging --operation application.deploy \
  --application shop-staging --output "$TEST_ROOT/staging-request.json"
run_b infrastructure apply "$TEST_ROOT/staging-request.json" \
  > "$TEST_ROOT/staging-deployment.json"
test -f "$B_APPS/shop-staging/current/index.html"
grep -q 'shared release' "$B_APPS/shop-staging/current/index.html"
if run_b infrastructure apply "$TEST_ROOT/staging-request.json" > /dev/null 2>&1; then
  echo 'remote operation replay was accepted' >&2
  exit 1
fi
run_a infrastructure report "$COORDINATION_ID" --node staging \
  --status succeeded --healthy true --message deployed \
  > "$TEST_ROOT/staging-report.json"
grep -q '"status": "succeeded"' "$TEST_ROOT/staging-report.json"

run_a infrastructure status > "$TEST_ROOT/production-status.json"
run_b infrastructure status > "$TEST_ROOT/staging-status.json"
grep -q '"trust_status": "trusted"' "$TEST_ROOT/production-status.json"
grep -q '"tier": "staging"' "$TEST_ROOT/staging-status.json"
