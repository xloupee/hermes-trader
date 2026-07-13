#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$BASE_DIR/ops/noxa-observer-common.sh"
umask 077

readonly BIN="$(canonical_runtime_path \
  "${HERMES_NOXA_BIN:?HERMES_NOXA_BIN is required}" "observer binary")"
[[ -x "$BIN" ]] || {
  echo "Missing immutable observer binary: $BIN" >&2
  exit 1
}
readonly RUN_DIR="$(canonical_runtime_path \
  "${HERMES_NOXA_RUN_DIR:?HERMES_NOXA_RUN_DIR is required}" "run directory")"
readonly PID_FILE="$(canonical_runtime_path \
  "${HERMES_NOXA_PID_FILE:?HERMES_NOXA_PID_FILE is required}" "PID file")"
readonly FEED_URL="${HERMES_NOXA_FEED_URL:-wss://feed.mainnet.chain.robinhood.com}"
readonly RPC_URL="${HERMES_NOXA_RPC_URL:-https://rpc.mainnet.chain.robinhood.com}"
readonly AMOUNT_IN="${HERMES_NOXA_AMOUNT_IN:-10000000000000000}"
readonly SLIPPAGE_BPS="${HERMES_NOXA_SLIPPAGE_BPS:-500}"

mkdir -p "$RUN_DIR"
trap 'remove_owned_pid_record "$PID_FILE"' EXIT
write_current_pid_record "$PID_FILE"

args=(
  observe
  --feed-url "$FEED_URL"
  --rpc-url "$RPC_URL"
  --warmup-seconds 10
  --amount-in "$AMOUNT_IN"
  --slippage-bps "$SLIPPAGE_BPS"
)
if [[ -n "${HERMES_NOXA_RECIPIENT:-}" ]]; then
  args+=(--recipient "$HERMES_NOXA_RECIPIENT")
fi

restarts=0
while true; do
  if "$BIN" "${args[@]}" >>"$RUN_DIR/events.jsonl" 2>>"$RUN_DIR/observer.stderr"; then
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
