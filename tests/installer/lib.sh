#!/usr/bin/env bash

set -Eeuo pipefail

: "${LUMIC_TEST_BINARY:?set LUMIC_TEST_BINARY to the built lumic CLI}"
case "$LUMIC_TEST_BINARY" in
  /*) LUMIC_BIN="$LUMIC_TEST_BINARY" ;;
  *) LUMIC_BIN="$(pwd)/$LUMIC_TEST_BINARY" ;;
esac

installer_report_error() {
  local status="$1"
  local source_file="$2"
  local line="$3"
  local command="$4"
  trap - ERR
  printf 'installer test failed at %s:%s: %s\n' "$source_file" "$line" "$command" >&2
  if [[ -n "${INSTALLER_TEST_ROOT:-}" && -n "${INSTALLER_RESULTS_DIR:-}" ]]; then
    mkdir -p "$INSTALLER_RESULTS_DIR/diagnostics"
    find "$INSTALLER_TEST_ROOT" -maxdepth 1 -type f \
      \( -name '*.json' -o -name '*.error' \) \
      -exec cp {} "$INSTALLER_RESULTS_DIR/diagnostics/" \;
  fi
  return "$status"
}

trap 'installer_report_error "$?" "${BASH_SOURCE[0]:-unknown}" "$LINENO" "$BASH_COMMAND"' ERR

assert_json() {
  local expression="$1"
  local file="$2"
  jq -e "$expression" "$file" >/dev/null
}

assert_file_contains() {
  local file="$1"
  local expected="$2"
  grep -Fq "$expected" "$file"
}

assert_file_mode() {
  local file="$1"
  local expected="$2"
  test "$(stat -c '%a' "$file")" = "$expected"
}

assert_secret_reference() {
  local output="$1"
  local reference
  reference="$(jq -er '.secret_reference | select(test("^[a-z0-9._-]+$"))' "$output")"
  test -f "$LUMIC_STATE_DIR/secrets/$reference"
  assert_file_mode "$LUMIC_STATE_DIR/secrets/$reference" 600
}

assert_service_active() {
  systemctl is-active --quiet "$1"
}

assert_port_listening() {
  local port="$1"
  timeout 10 bash -c "until (echo >/dev/tcp/127.0.0.1/$port) >/dev/null 2>&1; do sleep 1; done"
}

assert_http_status() {
  local url="$1"
  local expected="$2"
  test "$(curl --silent --output /dev/null --write-out '%{http_code}' "$url")" = "$expected"
}

assert_secret_not_in_state() {
  local state_file="$1"
  if jq -e '.. | objects | select(has("password")) | .password | select(type == "string" and (startswith("secret://") | not))' "$state_file" >/dev/null; then
    echo "plaintext password found in $state_file" >&2
    return 1
  fi
}

assert_idempotent_install() {
  local service="$1"
  local definition="$2"
  local output="$3"
  "$LUMIC_BIN" managed-service install "$service" "$definition" >"$output"
  assert_json '.changed == false' "$output"
}

assert_resource_exists() {
  local resource_id="$1"
  local state_file="$2"
  jq -e --arg id "$resource_id" '[.resources[] | select(.resource.id == $id)] | length == 1' "$state_file" >/dev/null
}

assert_binding_exists() {
  local value="$1"
  local state_file="$2"
  jq -e --arg value "$value" '[.. | strings | select(startswith($value))] | length > 0' "$state_file" >/dev/null
}

assert_no_duplicate_resources() {
  local state_file="$1"
  jq -e '[.resources[].resource.id] as $ids | ($ids | length) == ($ids | unique | length)' "$state_file" >/dev/null
}

write_installer_result() {
  local integration="$1"
  local level="$2"
  local status="$3"
  local platform
  if [[ -r /etc/os-release ]]; then
    platform="$(. /etc/os-release && printf '%s-%s' "$ID" "$VERSION_ID")"
  else
    platform="$(uname -s)-$(uname -m)"
  fi
  mkdir -p "$INSTALLER_RESULTS_DIR"
  jq -n \
    --arg integration "$integration" \
    --arg level "$level" \
    --arg status "$status" \
    --arg platform "$platform" \
    '{integration: $integration, level: $level, status: $status, platform: $platform}' \
    >"$INSTALLER_RESULTS_DIR/$integration-$level.json"
}
