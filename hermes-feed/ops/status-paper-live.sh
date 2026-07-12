#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly STATE_DIR="${HERMES_STATE_DIR:-$BASE_DIR/../.runtime/hermes-live}"
readonly PID_FILE="$STATE_DIR/paper-live.pid"

if [[ ! -s "$PID_FILE" ]] || ! kill -0 "$(<"$PID_FILE")" 2>/dev/null; then
  echo "Hermes paper runtime is not running"
  exit 1
fi

readonly PID="$(<"$PID_FILE")"
readonly RUN_DIR="$(readlink -f "$STATE_DIR/current")"
echo "running pid=$PID"
ps -o pid,ppid,psr,ni,%cpu,%mem,etime,cmd -p "$PID"
echo "run_dir=$RUN_DIR"
for file in feed.jsonl paper-decisions.jsonl restarts.log probe.stderr paper.stderr supervisor.stderr; do
  if [[ -f "$RUN_DIR/$file" ]]; then
    printf '%s lines=' "$file"
    wc -l <"$RUN_DIR/$file"
  fi
done
