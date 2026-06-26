#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTROL="$SCRIPT_DIR/landing-canary-control.sh"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

export JITO_CANARY_MARKER_FILE="$TMP_DIR/current.env"
export JITO_APP_ENV_FILE="$TMP_DIR/app.env"
export JITO_WORKER_ENV_FILE="$TMP_DIR/worker.env"
export JITO_CANARY_TPU_QUIC_TIMEOUT_MS=100

: > "$JITO_APP_ENV_FILE"
: > "$JITO_WORKER_ENV_FILE"

assert_marker() {
  local key="$1" expected="$2"
  if ! grep -qx "$key=$expected" "$JITO_CANARY_MARKER_FILE"; then
    echo "expected marker $key=$expected" >&2
    echo "actual marker:" >&2
    cat "$JITO_CANARY_MARKER_FILE" >&2
    exit 1
  fi
}

"$CONTROL" mark tpu-quic-current-leader-fanout 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-tpu-quic
assert_marker CANARY_TPU_QUIC_ENABLED true
assert_marker CANARY_TPU_QUIC_FANOUT_SLOTS 1
assert_marker CANARY_TPU_QUIC_TIMEOUT_MS 100

"$CONTROL" mark tpu-quic-current-leader-only 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE tpu-quic-helius-tip
assert_marker CANARY_TPU_QUIC_ENABLED true
assert_marker CANARY_TPU_QUIC_FANOUT_SLOTS 1
assert_marker CANARY_TPU_QUIC_TIMEOUT_MS 100

echo "landing canary control tests passed"
