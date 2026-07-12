#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly BIN="$BASE_DIR/target/release/hermes-feed"
readonly STATE_DIR="${HERMES_STATE_DIR:-$BASE_DIR/../.runtime/hermes-live}"
readonly RUN_DIR="${HERMES_RUN_DIR:?HERMES_RUN_DIR is required}"
readonly ROUTER="${HERMES_V2_ROUTER:-0x89e5db8b5aa49aa85ac63f691524311aeb649eba}"
readonly MAX_AMOUNT_IN="${HERMES_MAX_AMOUNT_IN:-10000000000000000}"

mkdir -p "$STATE_DIR" "$RUN_DIR"
umask 077

probe_args=(
  probe
  --url wss://feed.mainnet.chain.robinhood.com
  --source fsn1-paper
  --warmup-seconds 10
  --router "$ROUTER"
  --selector 0x38ed1739
  --selector 0x7ff36ab5
  --selector 0x18cbafe5
)

if [[ -n "${HERMES_WATCH_ADDRESS:-}" ]]; then
  probe_args+=(--watch "$HERMES_WATCH_ADDRESS")
fi

restarts=0
while true; do
  if "$BIN" "${probe_args[@]}" 2>>"$RUN_DIR/probe.stderr" \
    | tee -a "$RUN_DIR/feed.jsonl" \
    | "$BIN" paper --input - --max-amount-in "$MAX_AMOUNT_IN" \
        >>"$RUN_DIR/paper-decisions.jsonl" 2>>"$RUN_DIR/paper.stderr"; then
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
