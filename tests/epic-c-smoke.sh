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

"$LUMIC_BIN" recipe catalog > "$TEST_ROOT/catalog.json"
grep -q '"id": "static-git"' "$TEST_ROOT/catalog.json"
grep -q '"version": "1.0.0"' "$TEST_ROOT/catalog.json"
"$LUMIC_BIN" recipe plan static-git demo demo.example.test \
  --repository https://example.test/demo.git > "$TEST_ROOT/plan.json"
grep -q 'recipe.apply' "$TEST_ROOT/plan.json"
if "$LUMIC_BIN" recipe plan static-git demo demo.example.test > /dev/null 2>&1; then
  echo 'static-git plan accepted a missing repository' >&2
  exit 1
fi
"$LUMIC_BIN" server --help | grep -q 'snapshot'
"$LUMIC_BIN" server --help | grep -q 'remediate-journal'

if [ "${LUMIC_EPIC_C_LIVE:-0}" != 1 ]; then
  exit 0
fi

export GIT_AUTHOR_NAME="Lumic CI"
export GIT_AUTHOR_EMAIL="ci@lumic.invalid"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"
SOURCE="$TEST_ROOT/source"
git init -q -b main "$SOURCE"
printf '%s\n' 'recipe release' > "$SOURCE/index.html"
git -C "$SOURCE" add index.html
git -C "$SOURCE" commit -q -m initial

"$LUMIC_BIN" recipe install static-git demo demo.example.test \
  --repository "file://$SOURCE" > "$TEST_ROOT/install.json"
grep -q '"changed": true' "$TEST_ROOT/install.json"
grep -q 'recipe release' "$LUMIC_APPS_ROOT/demo/current/index.html"
"$LUMIC_BIN" recipe install static-git demo demo.example.test \
  --repository "file://$SOURCE" > "$TEST_ROOT/idempotent.json"
grep -q '"changed": false' "$TEST_ROOT/idempotent.json"
"$LUMIC_BIN" server snapshot > "$TEST_ROOT/host.json"
grep -q '"listeners"' "$TEST_ROOT/host.json"
grep -q '"updates"' "$TEST_ROOT/host.json"
"$LUMIC_BIN" recipe uninstall demo > "$TEST_ROOT/uninstall.json"
grep -q 'moved to Lumic trash' "$TEST_ROOT/uninstall.json"
test ! -e "$LUMIC_APPS_ROOT/demo"
