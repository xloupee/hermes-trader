#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly STATE_DIR="${HERMES_SHADOW_STATE_DIR:-$BASE_DIR/../.runtime/hermes-shadow}"
readonly PID_FILE="$STATE_DIR/shadow.pid"

if [[ ! -s "$PID_FILE" ]] || ! kill -0 "$(<"$PID_FILE")" 2>/dev/null; then
  echo "Hermes reserve shadow is not running"
  exit 1
fi

readonly PID="$(<"$PID_FILE")"
readonly RUN_DIR="$(readlink -f "$STATE_DIR/current")"
echo "running pid=$PID"
ps -o pid,ppid,psr,ni,%cpu,%mem,etime,cmd -p "$PID"
echo "run_dir=$RUN_DIR"
for file in cache-bootstrap.jsonl shadow.jsonl restarts.log cache-bootstrap.stderr shadow.stderr tail.stderr supervisor.stderr; do
  if [[ -f "$RUN_DIR/$file" ]]; then
    printf '%s lines=' "$file"; wc -l <"$RUN_DIR/$file"
  fi
done
if [[ -s "$STATE_DIR/reserve-cache.json" ]]; then
  checkpoint_block=$(jq -r .block_number "$STATE_DIR/reserve-cache.json" 2>/dev/null || echo unavailable)
  checkpoint_pairs=$(jq '.pairs|length' "$STATE_DIR/reserve-cache.json" 2>/dev/null || echo unavailable)
  echo "checkpoint_block=$checkpoint_block"
  echo "checkpoint_pairs=$checkpoint_pairs"
fi
