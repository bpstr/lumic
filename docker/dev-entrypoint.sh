#!/usr/bin/env bash
set -u

lumic ui token rotate

lumicd &
daemon_pid=$!

socat \
    TCP-LISTEN:18080,bind=0.0.0.0,reuseaddr,fork \
    TCP:127.0.0.1:8080 &
proxy_pid=$!

shutdown() {
    trap - EXIT INT TERM
    kill "$daemon_pid" "$proxy_pid" 2>/dev/null || true
    wait "$daemon_pid" "$proxy_pid" 2>/dev/null || true
}

trap shutdown EXIT INT TERM

wait -n "$daemon_pid" "$proxy_pid"
status=$?
shutdown
exit "$status"
