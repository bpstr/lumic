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
export GIT_AUTHOR_NAME="Lumic CI"
export GIT_AUTHOR_EMAIL="ci@lumic.invalid"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"

make_repository() {
  repo="$1"
  entry="$2"
  content="$3"
  git init -q -b main "$repo"
  printf '%s\n' "$content" > "$repo/$entry"
  git -C "$repo" add "$entry"
  git -C "$repo" commit -q -m initial
}

STATIC_REPO="$TEST_ROOT/static-source"
make_repository "$STATIC_REPO" index.html 'static release one'
"$LUMIC_BIN" app create static-demo --domain static.example.test --runtime static --json >/dev/null
"$LUMIC_BIN" app repository set static-demo "file://$STATIC_REPO" --branch main >/dev/null
"$LUMIC_BIN" app plan static-demo >/dev/null
"$LUMIC_BIN" app deploy static-demo --json > "$TEST_ROOT/static-first.json"
grep -q '"status": "completed"' "$TEST_ROOT/static-first.json"
grep -q 'static release one' "$LUMIC_APPS_ROOT/static-demo/current/index.html"

printf '%s\n' 'static release two' > "$STATIC_REPO/index.html"
git -C "$STATIC_REPO" add index.html
git -C "$STATIC_REPO" commit -q -m second
"$LUMIC_BIN" app deploy static-demo --json > "$TEST_ROOT/static-second.json"
grep -q 'static release two' "$LUMIC_APPS_ROOT/static-demo/current/index.html"
"$LUMIC_BIN" app rollback static-demo --json >/dev/null
grep -q 'static release one' "$LUMIC_APPS_ROOT/static-demo/current/index.html"

PHP_REPO="$TEST_ROOT/php-source"
make_repository "$PHP_REPO" index.php '<?php echo "healthy";'
"$LUMIC_BIN" app create php-demo --domain php.example.test --runtime php --json >/dev/null
"$LUMIC_BIN" app repository set php-demo "file://$PHP_REPO" --branch main >/dev/null
"$LUMIC_BIN" app plan php-demo >/dev/null
"$LUMIC_BIN" app deploy php-demo --json > "$TEST_ROOT/php.json"
grep -q '"status": "completed"' "$TEST_ROOT/php.json"
grep -q 'echo "healthy"' "$LUMIC_APPS_ROOT/php-demo/current/index.php"

"$LUMIC_BIN" app deployments static-demo --json > "$TEST_ROOT/deployments.json"
grep -q '"automatic_rollback"' "$TEST_ROOT/deployments.json"
"$LUMIC_BIN" events --json > "$TEST_ROOT/events.json"
grep -q 'deployment.succeeded' "$TEST_ROOT/events.json"
"$LUMIC_BIN" audit --json > "$TEST_ROOT/audit.json"
grep -q 'application.deploy' "$TEST_ROOT/audit.json"
