#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly STATE_DIR="${HERMES_STATE_DIR:-$BASE_DIR/../.runtime/hermes-live}"
readonly PID_FILE="$STATE_DIR/paper-live.pid"

if [[ ! -s "$PID_FILE" ]]; then
  echo "Hermes paper runtime is not running"
  exit 0
fi

readonly PID="$(<"$PID_FILE")"
if kill -0 "$PID" 2>/dev/null; then
  kill -- "-$PID"
  for _ in {1..50}; do
    kill -0 "$PID" 2>/dev/null || break
    sleep 0.1
  done
fi
rm -f "$PID_FILE"
echo "Stopped Hermes paper runtime"
