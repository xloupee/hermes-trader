#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
elif [[ -f "${JITO_ENV_FILE:-$HOME/Documents/pumpfunnoti/.env}" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "${JITO_ENV_FILE:-$HOME/Documents/pumpfunnoti/.env}"
  set +a
fi

: "${SOLANA_RPC_URL:?SOLANA_RPC_URL must be set in the environment or .env}"
: "${JITO_ARM_LIVE_COPY_SEND:?set JITO_ARM_LIVE_COPY_SEND=YES to allow this local live-send harness}"

if [[ "$JITO_ARM_LIVE_COPY_SEND" != "YES" ]]; then
  echo "JITO_ARM_LIVE_COPY_SEND must be exactly YES" >&2
  exit 1
fi

export JITO_SHREDSTREAM_PROXY_URL="${JITO_SHREDSTREAM_PROXY_URL:-http://127.0.0.1:9999}"
export SHREDSTREAM_TARGET_WALLETS="${SHREDSTREAM_TARGET_WALLETS:-A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS}"
export JITO_COPY_WALLET="${JITO_COPY_WALLET:-FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W}"
export JITO_COPY_KEYPAIR_PATH="${JITO_COPY_KEYPAIR_PATH:-$HOME/.config/solana/copytrade-planning-keypair.json}"
export JITO_MAX_COPY_SOL="${JITO_MAX_COPY_SOL:-0.001}"
export JITO_FAST_COPY_SEND="${JITO_FAST_COPY_SEND:-false}"
export JITO_SEND_FANOUT="${JITO_SEND_FANOUT:-false}"
export JITO_SEND_RPC_URLS="${JITO_SEND_RPC_URLS:-${DIRECT_EXECUTION_SEND_RPC_URLS:-$SOLANA_RPC_URL}}"
export JITO_BLOCK_ENGINE_SEND_URLS="${JITO_BLOCK_ENGINE_SEND_URLS:-${DIRECT_EXECUTION_JITO_SEND_URLS:-}}"
export JITO_BLOCK_ENGINE_AUTH_UUID="${JITO_BLOCK_ENGINE_AUTH_UUID:-${DIRECT_EXECUTION_JITO_AUTH_UUID:-}}"
FAST_COPY_SEND_NORMALIZED="$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')"
case "$FAST_COPY_SEND_NORMALIZED" in
  yes|true|1|on)
    export JITO_SIMULATE_COPY_TX="${JITO_SIMULATE_COPY_TX:-false}"
    export JITO_AUTO_SELL_AFTER_BUY="${JITO_AUTO_SELL_AFTER_BUY:-false}"
    ;;
  *)
    export JITO_SIMULATE_COPY_TX="${JITO_SIMULATE_COPY_TX:-true}"
    export JITO_AUTO_SELL_AFTER_BUY="${JITO_AUTO_SELL_AFTER_BUY:-true}"
    ;;
esac
export JITO_ENABLE_COPY_SEND=true
export JITO_ONE_SHOT_COPY_SEND="${JITO_ONE_SHOT_COPY_SEND:-false}"
export JITO_DRY_RUN=false
export JITO_AUTO_SELL_DELAY_MS="${JITO_AUTO_SELL_DELAY_MS:-1000}"
export JITO_PRIORITY_FEE_MICRO_LAMPORTS="${JITO_PRIORITY_FEE_MICRO_LAMPORTS:-${DIRECT_EXECUTION_PRIORITY_FEE_MICRO_LAMPORTS:-}}"
export JITO_TIP_LAMPORTS="${JITO_TIP_LAMPORTS:-${DIRECT_EXECUTION_JITO_TIP_LAMPORTS:-}}"
export JITO_TIP_ACCOUNT="${JITO_TIP_ACCOUNT:-${DIRECT_EXECUTION_JITO_TIP_ACCOUNT:-}}"
export JITO_COPY_EXECUTIONS_PATH="${JITO_COPY_EXECUTIONS_PATH:-/tmp/jito-copy-executions-local-send.jsonl}"
export JITO_UNSIGNED_TX_PLANS_PATH="${JITO_UNSIGNED_TX_PLANS_PATH:-/tmp/jito-unsigned-tx-plans-local-send.jsonl}"
export JITO_COPY_TX_PLANS_PATH="${JITO_COPY_TX_PLANS_PATH:-/tmp/jito-copy-tx-plans-local-send.jsonl}"
export JITO_EXECUTION_PLANS_PATH="${JITO_EXECUTION_PLANS_PATH:-/tmp/jito-execution-plans-local-send.jsonl}"
export JITO_SHADOW_SIGNALS_PATH="${JITO_SHADOW_SIGNALS_PATH:-/tmp/jito-shadow-signals-local-send.jsonl}"
export JITO_ADDRESS_LOOKUP_TABLES="${JITO_ADDRESS_LOOKUP_TABLES:-4vX5U9XsiY11infmC13d6VFPjvUqtuRw744r4o94dyow}"
export JITO_SYNC_COPY_EXECUTIONS="${JITO_SYNC_COPY_EXECUTIONS:-true}"

case "$JITO_MAX_COPY_SOL" in
  0.001|0.000[0-9]*)
    ;;
  *)
    echo "JITO_MAX_COPY_SOL must be 0.001 or lower for first live send; got $JITO_MAX_COPY_SOL" >&2
    exit 1
    ;;
esac

echo "LOCAL LIVE COPY SEND IS ARMED"
echo "  proxy: $JITO_SHREDSTREAM_PROXY_URL"
echo "  target: $SHREDSTREAM_TARGET_WALLETS"
echo "  copy wallet: $JITO_COPY_WALLET"
echo "  max copy SOL: $JITO_MAX_COPY_SOL"
echo "  executions: $JITO_COPY_EXECUTIONS_PATH"
echo "  fast copy send: $JITO_FAST_COPY_SEND"
echo "  send fanout: $JITO_SEND_FANOUT"
echo "  send rpc urls: $(printf '%s' "$JITO_SEND_RPC_URLS" | awk -F, '{print NF}') configured"
if [[ -n "$JITO_BLOCK_ENGINE_SEND_URLS" ]]; then
  echo "  jito send urls: $(printf '%s' "$JITO_BLOCK_ENGINE_SEND_URLS" | awk -F, '{print NF}') configured"
else
  echo "  jito send urls: 0 configured"
fi
echo "  simulate copy tx: $JITO_SIMULATE_COPY_TX"
echo "  send enabled: $JITO_ENABLE_COPY_SEND"
echo "  one shot: $JITO_ONE_SHOT_COPY_SEND"
echo "  dry run: $JITO_DRY_RUN"
echo "  auto sell after buy: $JITO_AUTO_SELL_AFTER_BUY"
echo "  auto sell delay ms: $JITO_AUTO_SELL_DELAY_MS"
echo "  priority fee micro lamports: ${JITO_PRIORITY_FEE_MICRO_LAMPORTS:-0}"
echo "  jito tip lamports: ${JITO_TIP_LAMPORTS:-0}"
if [[ -n "$JITO_TIP_ACCOUNT" ]]; then
  echo "  jito tip account: configured"
else
  echo "  jito tip account: unset"
fi
echo "  dashboard sync: $JITO_SYNC_COPY_EXECUTIONS"

SYNC_PID=""
cleanup_sync() {
  if [[ -n "$SYNC_PID" ]]; then
    kill "$SYNC_PID" >/dev/null 2>&1 || true
    wait "$SYNC_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup_sync EXIT

if [[ "$JITO_SYNC_COPY_EXECUTIONS" == "true" ]]; then
  node tools/jito-shredstream-rs/sync-local-copy-executions-to-supabase.mjs \
    --watch \
    --executions="$JITO_COPY_EXECUTIONS_PATH" &
  SYNC_PID=$!
fi

cargo run --manifest-path tools/jito-shredstream-rs/Cargo.toml -- live --print-mentions
