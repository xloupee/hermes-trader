#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly STATE_DIR="${HERMES_SHADOW_STATE_DIR:-$BASE_DIR/../.runtime/hermes-shadow}"
readonly PID_FILE="$STATE_DIR/shadow.pid"

if [[ ! -s "$PID_FILE" ]]; then
  echo "Hermes reserve shadow is not running"
  exit 0
fi
readonly PID="$(<"$PID_FILE")"
if kill -0 "$PID" 2>/dev/null; then
  kill -- "-$PID"
  for _ in {1..100}; do
    kill -0 "$PID" 2>/dev/null || break
    sleep 0.1
  done
fi
rm -f "$PID_FILE"
echo "Stopped Hermes reserve shadow"
