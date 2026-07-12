#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly STATE_DIR="${HERMES_SHADOW_STATE_DIR:-$BASE_DIR/../.runtime/hermes-shadow}"
readonly RUN_DIR="${HERMES_SHADOW_RUN_DIR:?HERMES_SHADOW_RUN_DIR is required}"
readonly BIN="$RUN_DIR/hermes-feed"
readonly CHECKPOINT="$STATE_DIR/reserve-cache.json"
readonly SOURCE_FEED="${HERMES_SOURCE_FEED:-$BASE_DIR/../.runtime/hermes-live/current/feed.jsonl}"
readonly MAX_AMOUNT_IN="${HERMES_MAX_AMOUNT_IN:-10000000000000000}"

mkdir -p "$STATE_DIR" "$RUN_DIR"
umask 077
if [[ ! -s "$CHECKPOINT" ]]; then
  "$BIN" cache --checkpoint "$CHECKPOINT" --confirmations 2 \
    --batch-size 300 --run-seconds 1 \
    >>"$RUN_DIR/cache-bootstrap.jsonl" 2>>"$RUN_DIR/cache-bootstrap.stderr"
fi

restarts=0
while true; do
  if tail -n 0 -F "$SOURCE_FEED" 2>>"$RUN_DIR/tail.stderr" \
    | "$BIN" shadow --input - --checkpoint "$CHECKPOINT" \
        --max-amount-in "$MAX_AMOUNT_IN" \
        >>"$RUN_DIR/shadow.jsonl" 2>>"$RUN_DIR/shadow.stderr"; then
    status=0
  else
    status=$?
  fi
  restarts=$((restarts + 1))
  printf '%s status=%s restart=%s\n' \
    "$(date --utc +%Y-%m-%dT%H:%M:%SZ)" "$status" "$restarts" \
    >>"$RUN_DIR/restarts.log"
  sleep 1
done
