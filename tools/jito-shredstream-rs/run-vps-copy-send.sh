#!/usr/bin/env bash
set -euo pipefail

APP_ENV_FILE="${JITO_APP_ENV_FILE:-/opt/pumpfun-migration-bot/.env}"
WORKER_ENV_FILE="${JITO_WORKER_ENV_FILE:-/etc/jito-copy-live.env}"
WORKER_DIR="${JITO_WORKER_DIR:-/opt/jito-feed-probe-watch}"
WORKER_BIN="${JITO_WORKER_BIN:-$WORKER_DIR/target/release/jito-feed-probe}"

load_env_file() {
  local env_file="$1"
  local line key value

  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "$line" || "${line:0:1}" == "#" || "$line" != *"="* ]] && continue

    key="${line%%=*}"
    value="${line#*=}"
    [[ "$key" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || continue

    export "$key=$value"
  done < "$env_file"
}

if [[ -f "$APP_ENV_FILE" ]]; then
  load_env_file "$APP_ENV_FILE"
fi

if [[ -f "$WORKER_ENV_FILE" ]]; then
  load_env_file "$WORKER_ENV_FILE"
fi

: "${SOLANA_RPC_URL:?SOLANA_RPC_URL must be set in $APP_ENV_FILE or $WORKER_ENV_FILE}"
: "${JITO_ARM_LIVE_COPY_SEND:?set JITO_ARM_LIVE_COPY_SEND=YES to allow live copy send}"

if [[ "$JITO_ARM_LIVE_COPY_SEND" != "YES" ]]; then
  echo "JITO_ARM_LIVE_COPY_SEND must be exactly YES" >&2
  exit 1
fi

export JITO_SHREDSTREAM_PROXY_URL="${JITO_SHREDSTREAM_PROXY_URL:-http://127.0.0.1:9999}"
export SHREDSTREAM_TARGET_WALLETS="${SHREDSTREAM_TARGET_WALLETS:-A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS}"
export JITO_COPY_WALLET="${JITO_COPY_WALLET:-FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W}"
export JITO_COPY_KEYPAIR_PATH="${JITO_COPY_KEYPAIR_PATH:-/etc/jito-copy-keypair.json}"
export JITO_MAX_COPY_SOL="${JITO_MAX_COPY_SOL:-0.001}"
export JITO_FAST_COPY_SEND="${JITO_FAST_COPY_SEND:-YES}"
export JITO_SEND_FANOUT="${JITO_SEND_FANOUT:-false}"
export JITO_SEND_RPC_URLS="${JITO_SEND_RPC_URLS:-${DIRECT_EXECUTION_SEND_RPC_URLS:-$SOLANA_RPC_URL}}"
export JITO_BLOCK_ENGINE_SEND_URLS="${JITO_BLOCK_ENGINE_SEND_URLS:-${DIRECT_EXECUTION_JITO_SEND_URLS:-}}"
export JITO_BLOCK_ENGINE_AUTH_UUID="${JITO_BLOCK_ENGINE_AUTH_UUID:-${DIRECT_EXECUTION_JITO_AUTH_UUID:-}}"
export JITO_SIMULATE_COPY_TX="${JITO_SIMULATE_COPY_TX:-false}"
export JITO_ENABLE_COPY_SEND="${JITO_ENABLE_COPY_SEND:-true}"
export JITO_ONE_SHOT_COPY_SEND="${JITO_ONE_SHOT_COPY_SEND:-false}"
export JITO_DRY_RUN="${JITO_DRY_RUN:-false}"
export JITO_AUTO_SELL_AFTER_BUY="${JITO_AUTO_SELL_AFTER_BUY:-false}"
export JITO_AUTO_SELL_DELAY_MS="${JITO_AUTO_SELL_DELAY_MS:-1000}"
export JITO_PRIORITY_FEE_MICRO_LAMPORTS="${JITO_PRIORITY_FEE_MICRO_LAMPORTS:-${DIRECT_EXECUTION_PRIORITY_FEE_MICRO_LAMPORTS:-}}"
export JITO_TIP_LAMPORTS="${JITO_TIP_LAMPORTS:-${DIRECT_EXECUTION_JITO_TIP_LAMPORTS:-}}"
export JITO_TIP_ACCOUNT="${JITO_TIP_ACCOUNT:-${DIRECT_EXECUTION_JITO_TIP_ACCOUNT:-}}"
export JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS="${JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS:-500000}"
export JITO_MAX_TIP_LAMPORTS="${JITO_MAX_TIP_LAMPORTS:-50000}"
export JITO_COPY_EXECUTIONS_PATH="${JITO_COPY_EXECUTIONS_PATH:-/var/log/jito-copy-executions-vps.jsonl}"
export JITO_ADDRESS_LOOKUP_TABLES="${JITO_ADDRESS_LOOKUP_TABLES:-4vX5U9XsiY11infmC13d6VFPjvUqtuRw744r4o94dyow}"
export JITO_DISABLE_SIGNAL_OBSERVATIONS="${JITO_DISABLE_SIGNAL_OBSERVATIONS:-true}"

validate_nonnegative_int() {
  local name="$1"
  local value="${2:-}"

  [[ -z "$value" ]] && return 0
  if [[ ! "$value" =~ ^[0-9]+$ ]]; then
    echo "$name must be a nonnegative integer; got $value" >&2
    exit 1
  fi
}

validate_capped_int() {
  local name="$1"
  local value="${2:-}"
  local cap_name="$3"
  local cap_value="${4:-}"

  validate_nonnegative_int "$name" "$value"
  validate_nonnegative_int "$cap_name" "$cap_value"
  [[ -z "$value" || -z "$cap_value" ]] && return 0
  if (( value > cap_value )); then
    echo "$name must be <= $cap_name ($cap_value); got $value" >&2
    exit 1
  fi
}

case "$JITO_MAX_COPY_SOL" in
  0.001|0.000[0-9]*)
    ;;
  *)
    echo "JITO_MAX_COPY_SOL must be 0.001 or lower for first VPS live send; got $JITO_MAX_COPY_SOL" >&2
    exit 1
    ;;
esac

if [[ ! -x "$WORKER_BIN" ]]; then
  echo "jito-feed-probe binary not found or not executable: $WORKER_BIN" >&2
  exit 1
fi

if [[ ! -f "$JITO_COPY_KEYPAIR_PATH" ]]; then
  echo "copy keypair not found: $JITO_COPY_KEYPAIR_PATH" >&2
  exit 1
fi

validate_capped_int \
  JITO_PRIORITY_FEE_MICRO_LAMPORTS \
  "$JITO_PRIORITY_FEE_MICRO_LAMPORTS" \
  JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS \
  "$JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS"
validate_capped_int JITO_TIP_LAMPORTS "$JITO_TIP_LAMPORTS" JITO_MAX_TIP_LAMPORTS "$JITO_MAX_TIP_LAMPORTS"

if [[ -n "$JITO_TIP_LAMPORTS" && "$JITO_TIP_LAMPORTS" != "0" && -z "$JITO_TIP_ACCOUNT" ]]; then
  echo "JITO_TIP_ACCOUNT must be set when JITO_TIP_LAMPORTS is positive" >&2
  exit 1
fi

echo "VPS LIVE COPY SEND IS ARMED"
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
echo "  priority fee micro lamports: ${JITO_PRIORITY_FEE_MICRO_LAMPORTS:-0}"
echo "  max priority fee micro lamports: $JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS"
echo "  jito tip lamports: ${JITO_TIP_LAMPORTS:-0}"
echo "  max jito tip lamports: $JITO_MAX_TIP_LAMPORTS"
if [[ -n "$JITO_TIP_ACCOUNT" ]]; then
  echo "  jito tip account: configured"
else
  echo "  jito tip account: unset"
fi
echo "  signal observations disabled: $JITO_DISABLE_SIGNAL_OBSERVATIONS"

cd "$WORKER_DIR"
exec "$WORKER_BIN" live --print-mentions
