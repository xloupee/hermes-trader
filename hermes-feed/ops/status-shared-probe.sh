#!/usr/bin/env bash
set -euo pipefail

readonly RUN_DIR=/srv/hermes-probe
readonly PID_FILE="$RUN_DIR/fra1-shared.pid"

if [[ ! -s "$PID_FILE" ]]; then
  echo "No Hermes PID file"
  exit 1
fi

readonly PID="$(<"$PID_FILE")"
if kill -0 "$PID" 2>/dev/null; then
  echo "running pid=$PID"
  ps -o pid,ppid,psr,ni,%cpu,%mem,etime,cmd -p "$PID"
else
  echo "stopped pid=$PID"
fi

wc -l "$RUN_DIR/fra1-shared.jsonl" 2>/dev/null || true
du -h "$RUN_DIR/fra1-shared.jsonl" 2>/dev/null || true
tail -n 5 "$RUN_DIR/fra1-shared.stderr" 2>/dev/null || true
