#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

VPS_HOST="${JITO_COPY_TEST_VPS_HOST:-root@157.90.240.233}"
LOCAL_PORT="${JITO_COPY_TEST_LOCAL_PORT:-9999}"
REMOTE_PORT="${JITO_COPY_TEST_REMOTE_PORT:-9999}"
TUNNEL_READY_TIMEOUT_SECONDS="${JITO_COPY_TEST_TUNNEL_READY_TIMEOUT_SECONDS:-10}"

cleanup() {
  if [[ -n "${TUNNEL_PID:-}" ]]; then
    kill "$TUNNEL_PID" >/dev/null 2>&1 || true
    wait "$TUNNEL_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

ssh \
  -o ExitOnForwardFailure=yes \
  -o ServerAliveInterval=15 \
  -N \
  -L "${LOCAL_PORT}:127.0.0.1:${REMOTE_PORT}" \
  "$VPS_HOST" &
TUNNEL_PID=$!

for ((attempt = 0; attempt < TUNNEL_READY_TIMEOUT_SECONDS * 10; attempt += 1)); do
  if ! kill -0 "$TUNNEL_PID" >/dev/null 2>&1; then
    echo "SSH tunnel exited before local port ${LOCAL_PORT} became ready" >&2
    exit 1
  fi

  if nc -z 127.0.0.1 "$LOCAL_PORT" >/dev/null 2>&1; then
    break
  fi

  sleep 0.1
done

if ! nc -z 127.0.0.1 "$LOCAL_PORT" >/dev/null 2>&1; then
  echo "timed out waiting for local tunnel port ${LOCAL_PORT}" >&2
  exit 1
fi

export JITO_SHREDSTREAM_PROXY_URL="http://127.0.0.1:${LOCAL_PORT}"

tools/jito-shredstream-rs/run-local-copy-sim.sh
