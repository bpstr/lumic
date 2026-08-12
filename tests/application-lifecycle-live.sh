#!/usr/bin/env bash
set -Eeuo pipefail

: "${LUMIC_TEST_BINARY:?set LUMIC_TEST_BINARY to the built lumic CLI}"
: "${LUMIC_APPLICATION_LIVE:?set LUMIC_APPLICATION_LIVE=1 on a clean systemd host}"

LUMIC_BIN="$(realpath "$LUMIC_TEST_BINARY")"
TEST_ROOT="$(mktemp -d /var/tmp/lumic-application-live.XXXXXX)"
APP="node-live-$$"
DOMAIN="$APP.example.test"
SOURCE="$TEST_ROOT/source"
TIMER_OUTPUT="/tmp/lumic-$APP-timer-ran"
export LUMIC_STATE_DIR="$TEST_ROOT/state"
export LUMIC_APPS_ROOT="$TEST_ROOT/apps"
export GIT_AUTHOR_NAME="Lumic CI"
export GIT_AUTHOR_EMAIL="ci@lumic.invalid"
export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"

cleanup() {
  "$LUMIC_BIN" app delete "$APP" >/dev/null 2>&1 || true
  while read -r unit _; do
    [ -n "$unit" ] && systemctl disable --now "$unit" >/dev/null 2>&1 || true
  done < <(systemctl list-unit-files "lumic-app-$APP-*" --no-legend 2>/dev/null || true)
  rm -f "/etc/nginx/sites-enabled/lumic-$APP.conf" "/etc/nginx/sites-available/lumic-$APP.conf"
  rm -f "$TIMER_OUTPUT"
  nginx -t >/dev/null 2>&1 && systemctl reload nginx >/dev/null 2>&1 || true
  rm -rf "$TEST_ROOT"
}

report_error() {
  local status="$?"
  local line="${1:-unknown}"
  local command="${2:-unknown}"
  trap - ERR
  echo "application lifecycle test failed at line $line: $command" >&2
  echo "collecting Lumic unit diagnostics" >&2
  while read -r unit _; do
    [ -n "$unit" ] || continue
    echo "===== $unit =====" >&2
    systemctl cat "$unit" >&2 || true
    systemctl status --no-pager --full "$unit" >&2 || true
    journalctl --no-pager --unit "$unit" --lines 100 >&2 || true
  done < <(systemctl list-unit-files "lumic-app-$APP-*" --no-legend 2>/dev/null || true)
  return "$status"
}

trap cleanup EXIT INT TERM
trap 'report_error "$LINENO" "$BASH_COMMAND"' ERR

mkdir -p "$SOURCE"
chmod 0755 "$TEST_ROOT" "$SOURCE"
git init -q -b main "$SOURCE"

write_server() {
  status="$1"
  version="$2"
  cat > "$SOURCE/server.js" <<EOF
const http = require('http');
http.createServer((_request, response) => {
  response.writeHead($status, {'content-type': 'text/plain'});
  response.end('$version');
}).listen(Number(process.env.PORT));
EOF
}

write_manifest() {
  before="$1"
  cat > "$SOURCE/lumic.yaml" <<EOF
schema_version: 1
name: $APP
runtime:
  node: 22
  package_manager: npm
web:
  command: ["node", "server.js"]
  port: 3310
workers:
  queue:
    command: ["node", "worker.js"]
    restart: on_failure
    health:
      command: ["node", "worker-health.js"]
      interval_seconds: 5
cron:
  heartbeat:
    command: ["/usr/bin/touch", "$TIMER_OUTPUT"]
    schedule: "* * * * *"
deployment:
  before: $before
  drain_seconds: 1
  retain_releases: 3
health:
  path: /health
  expect: 200
  timeout_seconds: 15
EOF
}

printf '%s\n' 'setInterval(() => {}, 1000);' > "$SOURCE/worker.js"
printf '%s\n' 'process.exit(0);' > "$SOURCE/worker-health.js"
cat > "$SOURCE/package.json" <<EOF
{
  "name": "$APP",
  "version": "1.0.0",
  "private": true
}
EOF
write_server 200 release-one
write_manifest '[]'
git -C "$SOURCE" add .
git -C "$SOURCE" commit -q -m initial

"$LUMIC_BIN" app create "$APP" --domain "$DOMAIN" --runtime node >/dev/null
"$LUMIC_BIN" app repository set "$APP" "file://$SOURCE" --branch main >/dev/null
"$LUMIC_BIN" app manifest plan "$APP" --repository-root "$SOURCE" >/dev/null
"$LUMIC_BIN" app manifest apply "$APP" --repository-root "$SOURCE" >/dev/null
"$LUMIC_BIN" app provision "$APP" --runtime-version 22 >/dev/null
"$LUMIC_BIN" app deploy "$APP" --json > "$TEST_ROOT/first.json"
grep -q '"status": "completed"' "$TEST_ROOT/first.json"
systemctl is-active --quiet "lumic-app-$APP-queue.service"
systemctl is-active --quiet "lumic-app-$APP-heartbeat.timer"
systemctl is-active --quiet "lumic-app-$APP-queue-health.timer"
systemctl start "lumic-app-$APP-heartbeat.service"
test -f "$TIMER_OUTPUT"
grep -q '3310' "/etc/nginx/sites-available/lumic-$APP.conf"
test "$(curl -fsS -H "Host: $DOMAIN" http://127.0.0.1/health)" = release-one

write_server 200 release-two
git -C "$SOURCE" add server.js
git -C "$SOURCE" commit -q -m second
"$LUMIC_BIN" app deploy "$APP" --json > "$TEST_ROOT/second.json"
test "$(curl -fsS -H "Host: $DOMAIN" http://127.0.0.1/health)" = release-two
grep -q '3311' "/etc/nginx/sites-available/lumic-$APP.conf"

known_good="$(readlink "$LUMIC_APPS_ROOT/$APP/current")"
write_server 500 rejected
git -C "$SOURCE" add server.js
git -C "$SOURCE" commit -q -m unhealthy
if "$LUMIC_BIN" app deploy "$APP" >/dev/null 2>&1; then
  echo 'unhealthy deployment unexpectedly succeeded' >&2
  exit 1
fi
test "$(readlink "$LUMIC_APPS_ROOT/$APP/current")" = "$known_good"
test "$(curl -fsS -H "Host: $DOMAIN" http://127.0.0.1/health)" = release-two

write_server 200 cancelled
write_manifest '[["/usr/bin/sleep", "20"]]'
git -C "$SOURCE" add .
git -C "$SOURCE" commit -q -m cancellable
"$LUMIC_BIN" app manifest apply "$APP" --repository-root "$SOURCE" >/dev/null
"$LUMIC_BIN" app deploy "$APP" > "$TEST_ROOT/cancelled-deploy.txt" 2>&1 &
deploy_pid=$!
deployment_id=""
for _ in $(seq 1 50); do
  deployment_id="$("$LUMIC_BIN" app deployments "$APP" --json | jq -r '.[0] | select(.status == "started") | .id' 2>/dev/null || true)"
  [ -n "$deployment_id" ] && break
  sleep 0.2
done
test -n "$deployment_id"
"$LUMIC_BIN" app cancel "$APP" "$deployment_id" >/dev/null
wait "$deploy_pid" || true
"$LUMIC_BIN" app deployments "$APP" --json > "$TEST_ROOT/deployments.json"
jq -e --arg id "$deployment_id" '.[] | select(.id == $id and .status == "cancelled")' "$TEST_ROOT/deployments.json" >/dev/null
test "$(readlink "$LUMIC_APPS_ROOT/$APP/current")" = "$known_good"

echo 'Clean-host Node blue/green, systemd, nginx, cancellation, and recovery acceptance test passed.'
