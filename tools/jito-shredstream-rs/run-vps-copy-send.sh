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

if [[ -z "${JITO_STATE_RPC_URLS:-}" && -z "${SOLANA_RPC_URL:-}" ]]; then
  echo "JITO_STATE_RPC_URLS or SOLANA_RPC_URL must be set in $APP_ENV_FILE or $WORKER_ENV_FILE" >&2
  exit 1
fi
: "${JITO_ARM_LIVE_COPY_SEND:?set JITO_ARM_LIVE_COPY_SEND=YES to allow live copy send}"

if [[ "$JITO_ARM_LIVE_COPY_SEND" != "YES" ]]; then
  echo "JITO_ARM_LIVE_COPY_SEND must be exactly YES" >&2
  exit 1
fi

export JITO_SHREDSTREAM_PROXY_URL="${JITO_SHREDSTREAM_PROXY_URL:-http://127.0.0.1:9999}"
export SHREDSTREAM_TARGET_WALLETS="${SHREDSTREAM_TARGET_WALLETS:-A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS}"
if [[ -n "${JITO_TELEGRAM_SNAPSHOT_PATH:-}" ]]; then
  export JITO_TELEGRAM_SNAPSHOT_PATH
fi
export JITO_COPY_WALLET="${JITO_COPY_WALLET:-FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W}"
export JITO_COPY_KEYPAIR_PATH="${JITO_COPY_KEYPAIR_PATH:-/etc/jito-copy-keypair.json}"
export JITO_MAX_COPY_SOL="${JITO_MAX_COPY_SOL:-0}"
export JITO_MAX_TOTAL_COPY_SPEND_SOL="${JITO_MAX_TOTAL_COPY_SPEND_SOL:-0}"
export JITO_COPY_WALLET_BALANCE_REFRESH_MS="${JITO_COPY_WALLET_BALANCE_REFRESH_MS:-5000}"
export JITO_COPY_WALLET_BALANCE_STALE_MS="${JITO_COPY_WALLET_BALANCE_STALE_MS:-120000}"
export JITO_BALANCE_CACHE_RPC_URLS="${JITO_BALANCE_CACHE_RPC_URLS:-}"
export JITO_BLOCKHASH_REFRESH_MS="${JITO_BLOCKHASH_REFRESH_MS:-500}"
export JITO_BLOCKHASH_REFRESH_TIMEOUT_MS="${JITO_BLOCKHASH_REFRESH_TIMEOUT_MS:-1200}"
export JITO_BLOCKHASH_STALE_MS="${JITO_BLOCKHASH_STALE_MS:-30000}"
export JITO_BLOCKHASH_RPC_URLS="${JITO_BLOCKHASH_RPC_URLS:-}"
export JITO_MIGRATED_AMM_MIN_COPY_SOL="${JITO_MIGRATED_AMM_MIN_COPY_SOL:-0.00099}"
export JITO_MIGRATED_AMM_SMALL_COPY_MODE="${JITO_MIGRATED_AMM_SMALL_COPY_MODE:-skip}"
export JITO_FAST_COPY_SEND="${JITO_FAST_COPY_SEND:-YES}"
export JITO_SEND_FANOUT="${JITO_SEND_FANOUT:-false}"
export JITO_SEND_LANE_MODE="${JITO_SEND_LANE_MODE:-mixed}"
if [[ -z "${JITO_SEND_RPC_URLS:-}" ]]; then
  export JITO_SEND_RPC_URLS="${DIRECT_EXECUTION_SEND_RPC_URLS:-${SOLANA_RPC_URL:-}}"
fi
export JITO_SELL_SEND_RPC_URLS="${JITO_SELL_SEND_RPC_URLS:-${DIRECT_EXECUTION_SELL_SEND_RPC_URLS:-}}"
export JITO_BLOCK_ENGINE_SEND_URLS="${JITO_BLOCK_ENGINE_SEND_URLS:-${DIRECT_EXECUTION_JITO_SEND_URLS:-}}"
export JITO_BLOCK_ENGINE_AUTH_UUID="${JITO_BLOCK_ENGINE_AUTH_UUID:-${DIRECT_EXECUTION_JITO_AUTH_UUID:-}}"
export JITO_HELIUS_SENDER_ENABLED="${JITO_HELIUS_SENDER_ENABLED:-false}"
export JITO_HELIUS_SENDER_URLS="${JITO_HELIUS_SENDER_URLS:-}"
export JITO_HELIUS_SENDER_SWQOS_ONLY="${JITO_HELIUS_SENDER_SWQOS_ONLY:-false}"
export JITO_HELIUS_SENDER_TIP_LAMPORTS="${JITO_HELIUS_SENDER_TIP_LAMPORTS:-}"
export JITO_HELIUS_SENDER_TIP_ACCOUNT="${JITO_HELIUS_SENDER_TIP_ACCOUNT:-}"
export JITO_HELIUS_SENDER_TIP_ACCOUNTS="${JITO_HELIUS_SENDER_TIP_ACCOUNTS:-}"
export JITO_NOZOMI_ENABLED="${JITO_NOZOMI_ENABLED:-false}"
export JITO_NOZOMI_URLS="${JITO_NOZOMI_URLS:-}"
export JITO_NOZOMI_TIP_LAMPORTS="${JITO_NOZOMI_TIP_LAMPORTS:-}"
export JITO_NOZOMI_TIP_ACCOUNT="${JITO_NOZOMI_TIP_ACCOUNT:-}"
export JITO_NOZOMI_TIP_ACCOUNTS="${JITO_NOZOMI_TIP_ACCOUNTS:-}"
export JITO_ASTRALANE_ENABLED="${JITO_ASTRALANE_ENABLED:-false}"
export JITO_ASTRALANE_URLS="${JITO_ASTRALANE_URLS:-https://lim.gateway.astralane.io/irisb}"
export JITO_ASTRALANE_API_KEY="${JITO_ASTRALANE_API_KEY:-}"
export JITO_ASTRALANE_TIP_LAMPORTS="${JITO_ASTRALANE_TIP_LAMPORTS:-1000000}"
export JITO_ASTRALANE_TIP_ACCOUNT="${JITO_ASTRALANE_TIP_ACCOUNT:-astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm}"
export JITO_ASTRALANE_TIP_ACCOUNTS="${JITO_ASTRALANE_TIP_ACCOUNTS:-$JITO_ASTRALANE_TIP_ACCOUNT}"
export JITO_ASTRALANE_MEV_PROTECT="${JITO_ASTRALANE_MEV_PROTECT:-false}"
export JITO_ASTRALANE_SWQOS_ONLY="${JITO_ASTRALANE_SWQOS_ONLY:-false}"
export JITO_LUNAR_LANDER_ENABLED="${JITO_LUNAR_LANDER_ENABLED:-false}"
export JITO_LUNAR_LANDER_URLS="${JITO_LUNAR_LANDER_URLS:-http://fra.lunar-lander.hellomoon.io/send-bin}"
export JITO_LUNAR_LANDER_API_KEY="${JITO_LUNAR_LANDER_API_KEY:-}"
export JITO_LUNAR_LANDER_TIP_LAMPORTS="${JITO_LUNAR_LANDER_TIP_LAMPORTS:-1000000}"
export JITO_LUNAR_LANDER_TIP_ACCOUNT="${JITO_LUNAR_LANDER_TIP_ACCOUNT:-moon17L6BgxXRX5uHKudAmqVF96xia9h8ygcmG2sL3F}"
export JITO_LUNAR_LANDER_TIP_ACCOUNTS="${JITO_LUNAR_LANDER_TIP_ACCOUNTS:-$JITO_LUNAR_LANDER_TIP_ACCOUNT}"
export JITO_LUNAR_LANDER_MEV_PROTECT="${JITO_LUNAR_LANDER_MEV_PROTECT:-false}"
export JITO_CIRCULAR_FAST_ENABLED="${JITO_CIRCULAR_FAST_ENABLED:-false}"
export JITO_CIRCULAR_FAST_URLS="${JITO_CIRCULAR_FAST_URLS:-https://fra.fast.circular.fi/transactions}"
export JITO_CIRCULAR_FAST_API_KEY="${JITO_CIRCULAR_FAST_API_KEY:-}"
export JITO_CIRCULAR_FAST_TIP_LAMPORTS="${JITO_CIRCULAR_FAST_TIP_LAMPORTS:-1000000}"
export JITO_CIRCULAR_FAST_TIP_ACCOUNT="${JITO_CIRCULAR_FAST_TIP_ACCOUNT:-FAST3dMFZvESiEipBvLSiXq3QCV51o3xuoHScqRU6cB6}"
export JITO_CIRCULAR_FAST_TIP_ACCOUNTS="${JITO_CIRCULAR_FAST_TIP_ACCOUNTS:-$JITO_CIRCULAR_FAST_TIP_ACCOUNT}"
export JITO_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION="${JITO_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION:-false}"
export JITO_ERPC_SWQOS_ENABLED="${JITO_ERPC_SWQOS_ENABLED:-false}"
export JITO_ERPC_SWQOS_URLS="${JITO_ERPC_SWQOS_URLS:-}"
export JITO_ERPC_LEADER_SLOTS_ENABLED="${JITO_ERPC_LEADER_SLOTS_ENABLED:-false}"
export JITO_ERPC_LEADER_SLOTS_URL="${JITO_ERPC_LEADER_SLOTS_URL:-https://edge.erpc.global}"
export JITO_ERPC_API_KEY="${JITO_ERPC_API_KEY:-}"
export JITO_ERPC_LEADER_SLOTS_REFRESH_MS="${JITO_ERPC_LEADER_SLOTS_REFRESH_MS:-5000}"
export JITO_ERPC_LEADER_SLOTS_STALE_MS="${JITO_ERPC_LEADER_SLOTS_STALE_MS:-15000}"
export JITO_ERPC_YELLOWSTONE_GRPC_URL="${JITO_ERPC_YELLOWSTONE_GRPC_URL:-}"
export JITO_ERPC_YELLOWSTONE_GRPC_X_TOKEN="${JITO_ERPC_YELLOWSTONE_GRPC_X_TOKEN:-}"
export JITO_SHREDER_FASTLANE_GRPC_URL="${JITO_SHREDER_FASTLANE_GRPC_URL:-}"
export JITO_SHREDER_FASTLANE_GRPC_X_TOKEN="${JITO_SHREDER_FASTLANE_GRPC_X_TOKEN:-}"
export JITO_BEAM_ENABLED="${JITO_BEAM_ENABLED:-false}"
export JITO_BEAM_URL="${JITO_BEAM_URL:-https://beam.rpcfast.com}"
export JITO_BEAM_TOKEN="${JITO_BEAM_TOKEN:-}"
export JITO_BEAM_PROVIDER="${JITO_BEAM_PROVIDER:-bloxroute}"
export JITO_BEAM_MODE="${JITO_BEAM_MODE:-fastest}"
export JITO_BEAM_TIP_LAMPORTS="${JITO_BEAM_TIP_LAMPORTS:-}"
export JITO_BEAM_TIP_ACCOUNTS="${JITO_BEAM_TIP_ACCOUNTS:-}"
export JITO_ZERO_SLOT_ENABLED="${JITO_ZERO_SLOT_ENABLED:-false}"
export JITO_ZERO_SLOT_URLS="${JITO_ZERO_SLOT_URLS:-}"
export JITO_ZERO_SLOT_API_KEY="${JITO_ZERO_SLOT_API_KEY:-}"
export JITO_ZERO_SLOT_TIP_LAMPORTS="${JITO_ZERO_SLOT_TIP_LAMPORTS:-}"
export JITO_ZERO_SLOT_TIP_ACCOUNTS="${JITO_ZERO_SLOT_TIP_ACCOUNTS:-}"
export JITO_TPU_JET_ENABLED="${JITO_TPU_JET_ENABLED:-false}"
export JITO_TPU_JET_RPC_URL="${JITO_TPU_JET_RPC_URL:-${SOLANA_RPC_URL:-}}"
export JITO_TPU_JET_WS_URL="${JITO_TPU_JET_WS_URL:-${JITO_SHREDER_FASTLANE_GRPC_URL:-${JITO_ERPC_YELLOWSTONE_GRPC_URL:-}}}"
export JITO_TPU_JET_SIDECAR_URL="${JITO_TPU_JET_SIDECAR_URL:-http://127.0.0.1:8787}"
export JITO_TPU_JET_FANOUT_SLOTS="${JITO_TPU_JET_FANOUT_SLOTS:-1}"
export JITO_TPU_JET_TIMEOUT_MS="${JITO_TPU_JET_TIMEOUT_MS:-30}"
export JITO_TPU_QUIC_ENABLED="${JITO_TPU_QUIC_ENABLED:-false}"
export JITO_TPU_QUIC_RPC_URL="${JITO_TPU_QUIC_RPC_URL:-${SOLANA_RPC_URL:-}}"
export JITO_TPU_QUIC_WS_URL="${JITO_TPU_QUIC_WS_URL:-}"
export JITO_TPU_QUIC_FANOUT_SLOTS="${JITO_TPU_QUIC_FANOUT_SLOTS:-12}"
export JITO_TPU_QUIC_TIMEOUT_MS="${JITO_TPU_QUIC_TIMEOUT_MS:-30}"
export JITO_SIMULATE_COPY_TX="${JITO_SIMULATE_COPY_TX:-false}"
export JITO_ENABLE_COPY_SEND="${JITO_ENABLE_COPY_SEND:-true}"
export JITO_ONE_SHOT_COPY_SEND="${JITO_ONE_SHOT_COPY_SEND:-false}"
export JITO_DRY_RUN="${JITO_DRY_RUN:-false}"
export JITO_AUTO_SELL_AFTER_BUY="${JITO_AUTO_SELL_AFTER_BUY:-false}"
export JITO_AUTO_SELL_DELAY_MS="${JITO_AUTO_SELL_DELAY_MS:-1000}"
export JITO_RUST_TRAILING_SELLS_ENABLED="${JITO_RUST_TRAILING_SELLS_ENABLED:-false}"
export JITO_RUST_TRAILING_SELL_CONFIRMATION_TIMEOUT_MS="${JITO_RUST_TRAILING_SELL_CONFIRMATION_TIMEOUT_MS:-30000}"
export JITO_RUST_TRAILING_SELL_CONFIRMATION_POLL_MS="${JITO_RUST_TRAILING_SELL_CONFIRMATION_POLL_MS:-100}"
export JITO_SIMULATE_AUTO_SELL="${JITO_SIMULATE_AUTO_SELL:-false}"
export JITO_ISOLATE_BUY_LATENCY_TEST="${JITO_ISOLATE_BUY_LATENCY_TEST:-false}"
ISOLATE_BUY_LATENCY_TEST_NORMALIZED="$(printf '%s' "$JITO_ISOLATE_BUY_LATENCY_TEST" | tr '[:upper:]' '[:lower:]')"
case "$ISOLATE_BUY_LATENCY_TEST_NORMALIZED" in
  yes|true|1|on)
    export JITO_AUTO_SELL_AFTER_BUY=false
    export JITO_SIMULATE_AUTO_SELL=false
    ;;
esac
export JITO_SEND_MAX_RETRIES="${JITO_SEND_MAX_RETRIES:-3}"
export JITO_SEND_HTTP_TIMEOUT_MS="${JITO_SEND_HTTP_TIMEOUT_MS:-750}"
export JITO_PRIORITY_FEE_MICRO_LAMPORTS="${JITO_PRIORITY_FEE_MICRO_LAMPORTS:-${DIRECT_EXECUTION_PRIORITY_FEE_MICRO_LAMPORTS:-}}"
export JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS="${JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS:-500000}"
export JITO_DYNAMIC_PRIORITY_FEE_ENABLED="${JITO_DYNAMIC_PRIORITY_FEE_ENABLED:-false}"
export JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS="${JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS:-$JITO_PRIORITY_FEE_MICRO_LAMPORTS}"
JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS="${JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS:-}"
JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS="${JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS:-}"
JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS="${JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS:-${JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS:-}}"
if [[ -n "$JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS" ]]; then
  export JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS
fi
if [[ -n "$JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS" ]]; then
  export JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS
fi
if [[ -n "$JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS" ]]; then
  export JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS
fi
export JITO_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
export JITO_ACCOUNT_PRIORITY_FEE_REFRESH_MS="${JITO_ACCOUNT_PRIORITY_FEE_REFRESH_MS:-1000}"
export JITO_ACCOUNT_PRIORITY_FEE_STALE_MS="${JITO_ACCOUNT_PRIORITY_FEE_STALE_MS:-5000}"
export JITO_ACCOUNT_PRIORITY_FEE_PERCENTILE="${JITO_ACCOUNT_PRIORITY_FEE_PERCENTILE:-75}"
export JITO_PRIORITY_FEE_RPC_URLS="${JITO_PRIORITY_FEE_RPC_URLS:-}"
export JITO_TIP_LAMPORTS="${JITO_TIP_LAMPORTS:-${DIRECT_EXECUTION_JITO_TIP_LAMPORTS:-}}"
export JITO_TIP_ACCOUNT="${JITO_TIP_ACCOUNT:-${DIRECT_EXECUTION_JITO_TIP_ACCOUNT:-}}"
export JITO_SELL_PRIORITY_FEE_MICRO_LAMPORTS="${JITO_SELL_PRIORITY_FEE_MICRO_LAMPORTS:-$JITO_PRIORITY_FEE_MICRO_LAMPORTS}"
export JITO_SELL_TIP_LAMPORTS="${JITO_SELL_TIP_LAMPORTS:-$JITO_TIP_LAMPORTS}"
export JITO_SELL_TIP_ACCOUNT="${JITO_SELL_TIP_ACCOUNT:-$JITO_TIP_ACCOUNT}"
export JITO_MAX_TIP_LAMPORTS="${JITO_MAX_TIP_LAMPORTS:-50000}"
export JITO_MAX_PROVIDER_TIP_LAMPORTS="${JITO_MAX_PROVIDER_TIP_LAMPORTS:-}"
export JITO_MAX_SIGNED_TX_BYTES="${JITO_MAX_SIGNED_TX_BYTES:-}"
export JITO_MAX_INSTRUCTION_COUNT="${JITO_MAX_INSTRUCTION_COUNT:-}"
export JITO_MAX_WRITABLE_ACCOUNT_COUNT="${JITO_MAX_WRITABLE_ACCOUNT_COUNT:-}"
export JITO_COPY_EXECUTIONS_PATH="${JITO_COPY_EXECUTIONS_PATH:-/var/log/jito-copy-executions-vps.jsonl}"
export JITO_COPY_EXECUTIONS_WRITE_QUEUE_CAPACITY="${JITO_COPY_EXECUTIONS_WRITE_QUEUE_CAPACITY:-1024}"
export JITO_COPY_EXECUTIONS_FLUSH_INTERVAL_MS="${JITO_COPY_EXECUTIONS_FLUSH_INTERVAL_MS:-250}"
export JITO_COPY_EXECUTION_CONCURRENCY="${JITO_COPY_EXECUTION_CONCURRENCY:-4}"
export JITO_COPY_EXECUTION_QUEUE_CAPACITY="${JITO_COPY_EXECUTION_QUEUE_CAPACITY:-1024}"
export JITO_ADDRESS_LOOKUP_TABLES="${JITO_ADDRESS_LOOKUP_TABLES:-4vX5U9XsiY11infmC13d6VFPjvUqtuRw744r4o94dyow}"
export JITO_DISABLE_SIGNAL_OBSERVATIONS="${JITO_DISABLE_SIGNAL_OBSERVATIONS:-true}"
export JITO_SIGNAL_OBSERVATION_QUEUE_CAPACITY="${JITO_SIGNAL_OBSERVATION_QUEUE_CAPACITY:-4096}"
export JITO_PRINT_FEED_EVENTS="${JITO_PRINT_FEED_EVENTS:-false}"
export JITO_PRINT_MENTIONS="${JITO_PRINT_MENTIONS:-false}"
export JITO_WARM_SEND_ENDPOINTS="${JITO_WARM_SEND_ENDPOINTS:-true}"
export JITO_SEND_ENDPOINT_WARM_INTERVAL_MS="${JITO_SEND_ENDPOINT_WARM_INTERVAL_MS:-15000}"

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
validate_capped_int \
  JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS \
  "$JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS" \
  JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS \
  "$JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS"
validate_capped_int \
  JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS \
  "$JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS" \
  JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS \
  "$JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS"
validate_capped_int \
  JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS \
  "$JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS" \
  JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS \
  "$JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS"
validate_capped_int JITO_TIP_LAMPORTS "$JITO_TIP_LAMPORTS" JITO_MAX_TIP_LAMPORTS "$JITO_MAX_TIP_LAMPORTS"
validate_nonnegative_int JITO_COPY_WALLET_BALANCE_REFRESH_MS "$JITO_COPY_WALLET_BALANCE_REFRESH_MS"
validate_nonnegative_int JITO_COPY_WALLET_BALANCE_STALE_MS "$JITO_COPY_WALLET_BALANCE_STALE_MS"
validate_nonnegative_int JITO_BLOCKHASH_REFRESH_MS "$JITO_BLOCKHASH_REFRESH_MS"
validate_nonnegative_int JITO_BLOCKHASH_REFRESH_TIMEOUT_MS "$JITO_BLOCKHASH_REFRESH_TIMEOUT_MS"
validate_nonnegative_int JITO_BLOCKHASH_STALE_MS "$JITO_BLOCKHASH_STALE_MS"
validate_nonnegative_int JITO_HELIUS_SENDER_TIP_LAMPORTS "$JITO_HELIUS_SENDER_TIP_LAMPORTS"
validate_nonnegative_int JITO_NOZOMI_TIP_LAMPORTS "$JITO_NOZOMI_TIP_LAMPORTS"
validate_nonnegative_int JITO_ASTRALANE_TIP_LAMPORTS "$JITO_ASTRALANE_TIP_LAMPORTS"
validate_nonnegative_int JITO_LUNAR_LANDER_TIP_LAMPORTS "$JITO_LUNAR_LANDER_TIP_LAMPORTS"
validate_nonnegative_int JITO_ERPC_LEADER_SLOTS_REFRESH_MS "$JITO_ERPC_LEADER_SLOTS_REFRESH_MS"
validate_nonnegative_int JITO_ERPC_LEADER_SLOTS_STALE_MS "$JITO_ERPC_LEADER_SLOTS_STALE_MS"
validate_nonnegative_int JITO_BEAM_TIP_LAMPORTS "$JITO_BEAM_TIP_LAMPORTS"
validate_nonnegative_int JITO_ZERO_SLOT_TIP_LAMPORTS "$JITO_ZERO_SLOT_TIP_LAMPORTS"
validate_nonnegative_int JITO_MAX_PROVIDER_TIP_LAMPORTS "$JITO_MAX_PROVIDER_TIP_LAMPORTS"
validate_nonnegative_int JITO_MAX_SIGNED_TX_BYTES "$JITO_MAX_SIGNED_TX_BYTES"
validate_nonnegative_int JITO_MAX_INSTRUCTION_COUNT "$JITO_MAX_INSTRUCTION_COUNT"
validate_nonnegative_int JITO_MAX_WRITABLE_ACCOUNT_COUNT "$JITO_MAX_WRITABLE_ACCOUNT_COUNT"
validate_nonnegative_int JITO_TPU_JET_FANOUT_SLOTS "$JITO_TPU_JET_FANOUT_SLOTS"
validate_nonnegative_int JITO_TPU_JET_TIMEOUT_MS "$JITO_TPU_JET_TIMEOUT_MS"
validate_nonnegative_int JITO_TPU_QUIC_FANOUT_SLOTS "$JITO_TPU_QUIC_FANOUT_SLOTS"
validate_nonnegative_int JITO_TPU_QUIC_TIMEOUT_MS "$JITO_TPU_QUIC_TIMEOUT_MS"
validate_capped_int \
  JITO_SELL_PRIORITY_FEE_MICRO_LAMPORTS \
  "$JITO_SELL_PRIORITY_FEE_MICRO_LAMPORTS" \
  JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS \
  "$JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS"
validate_capped_int JITO_SELL_TIP_LAMPORTS "$JITO_SELL_TIP_LAMPORTS" JITO_MAX_TIP_LAMPORTS "$JITO_MAX_TIP_LAMPORTS"
validate_nonnegative_int JITO_SEND_MAX_RETRIES "$JITO_SEND_MAX_RETRIES"
validate_nonnegative_int JITO_SEND_HTTP_TIMEOUT_MS "$JITO_SEND_HTTP_TIMEOUT_MS"
validate_nonnegative_int JITO_RUST_TRAILING_SELL_CONFIRMATION_TIMEOUT_MS "$JITO_RUST_TRAILING_SELL_CONFIRMATION_TIMEOUT_MS"
validate_nonnegative_int JITO_RUST_TRAILING_SELL_CONFIRMATION_POLL_MS "$JITO_RUST_TRAILING_SELL_CONFIRMATION_POLL_MS"
validate_nonnegative_int JITO_COPY_EXECUTION_CONCURRENCY "$JITO_COPY_EXECUTION_CONCURRENCY"
validate_nonnegative_int JITO_COPY_EXECUTION_QUEUE_CAPACITY "$JITO_COPY_EXECUTION_QUEUE_CAPACITY"
validate_nonnegative_int JITO_SIGNAL_OBSERVATION_QUEUE_CAPACITY "$JITO_SIGNAL_OBSERVATION_QUEUE_CAPACITY"

validate_positive_sol() {
  local name="$1"
  local value="${2:-}"

  [[ -z "$value" ]] && return 0
  if ! awk -v value="$value" 'BEGIN { exit !(value ~ /^[0-9]+([.][0-9]+)?$/ && value + 0 > 0) }'; then
    echo "$name must be a positive SOL amount; got $value" >&2
    exit 1
  fi
}

validate_nonnegative_sol() {
  local name="$1"
  local value="${2:-}"

  [[ -z "$value" ]] && return 0
  if ! awk -v value="$value" 'BEGIN { exit !(value ~ /^[0-9]+([.][0-9]+)?$/ && value + 0 >= 0) }'; then
    echo "$name must be a nonnegative SOL amount; got $value" >&2
    exit 1
  fi
}

validate_nonnegative_sol JITO_MAX_COPY_SOL "$JITO_MAX_COPY_SOL"
validate_nonnegative_sol JITO_MAX_TOTAL_COPY_SPEND_SOL "$JITO_MAX_TOTAL_COPY_SPEND_SOL"
validate_positive_sol JITO_MIGRATED_AMM_MIN_COPY_SOL "$JITO_MIGRATED_AMM_MIN_COPY_SOL"
case "$JITO_MIGRATED_AMM_SMALL_COPY_MODE" in
  skip|floor)
    ;;
  *)
    echo "JITO_MIGRATED_AMM_SMALL_COPY_MODE must be skip or floor; got $JITO_MIGRATED_AMM_SMALL_COPY_MODE" >&2
    exit 1
    ;;
esac

SEND_LANE_MODE_NORMALIZED="$(printf '%s' "$JITO_SEND_LANE_MODE" | tr '[:upper:]' '[:lower:]' | tr '-' '_')"
case "$SEND_LANE_MODE_NORMALIZED" in
  mixed|rpc_only|jito_only|helius_sender_only|nozomi_only|helius_nozomi_stack|astralane_only|helius_astralane_stack|helius_nozomi_astralane_stack|helius_nozomi_astralane_lunar_stack|lunar_lander_only|helius_lunar_lander_stack|circular_fast_only|helius_circular_fast_stack|erpc_swqos_only|helius_erpc_swqos_stack|beam_only|helius_beam_stack|helius_nozomi_beam_stack|zero_slot_only|helius_zero_slot_stack|helius_nozomi_zero_slot_stack|all_non_beam_stack|helius_tpu_jet|helius_tpu_quic|tpu_jet_helius_tip|tpu_quic_helius_tip|tpu_jet_only|tpu_quic_only)
    export JITO_SEND_LANE_MODE="${SEND_LANE_MODE_NORMALIZED//_/-}"
    ;;
  *)
    echo "JITO_SEND_LANE_MODE must be mixed, rpc_only/rpc-only, jito_only/jito-only, helius_sender_only/helius-sender-only, nozomi_only/nozomi-only, helius_nozomi_stack/helius-nozomi-stack, astralane_only/astralane-only, helius_astralane_stack/helius-astralane-stack, helius_nozomi_astralane_stack/helius-nozomi-astralane-stack, helius_nozomi_astralane_lunar_stack/helius-nozomi-astralane-lunar-stack, lunar_lander_only/lunar-lander-only, helius_lunar_lander_stack/helius-lunar-lander-stack, circular_fast_only/circular-fast-only, helius_circular_fast_stack/helius-circular-fast-stack, erpc_swqos_only/erpc-swqos-only, helius_erpc_swqos_stack/helius-erpc-swqos-stack, beam_only/beam-only, helius_beam_stack/helius-beam-stack, helius_nozomi_beam_stack/helius-nozomi-beam-stack, zero_slot_only/zero-slot-only, helius_zero_slot_stack/helius-zero-slot-stack, helius_nozomi_zero_slot_stack/helius-nozomi-zero-slot-stack, all_non_beam_stack/all-non-beam-stack, helius_tpu_jet/helius-tpu-jet, helius_tpu_quic/helius-tpu-quic, tpu_jet_helius_tip/tpu-jet-helius-tip, tpu_quic_helius_tip/tpu-quic-helius-tip, tpu_jet_only/tpu-jet-only, or tpu_quic_only/tpu-quic-only; got $JITO_SEND_LANE_MODE" >&2
    exit 1
    ;;
esac
case "$SEND_LANE_MODE_NORMALIZED" in
  mixed|jito_only)
    if [[ -n "$JITO_TIP_LAMPORTS" && "$JITO_TIP_LAMPORTS" != "0" && -z "$JITO_TIP_ACCOUNT" ]]; then
      echo "JITO_TIP_ACCOUNT must be set when JITO_TIP_LAMPORTS is positive" >&2
      exit 1
    fi
    if [[ -n "$JITO_SELL_TIP_LAMPORTS" && "$JITO_SELL_TIP_LAMPORTS" != "0" && -z "$JITO_SELL_TIP_ACCOUNT" ]]; then
      echo "JITO_SELL_TIP_ACCOUNT must be set when JITO_SELL_TIP_LAMPORTS is positive" >&2
      exit 1
    fi
    ;;
esac
case "$SEND_LANE_MODE_NORMALIZED" in
  rpc_only)
    if [[ -z "$JITO_SEND_RPC_URLS" ]]; then
      echo "JITO_SEND_LANE_MODE=rpc_only requires SOLANA_RPC_URL or JITO_SEND_RPC_URLS" >&2
      exit 1
    fi
    ;;
  jito_only)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=jito_only requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    if [[ -z "$JITO_BLOCK_ENGINE_SEND_URLS" ]]; then
      echo "JITO_SEND_LANE_MODE=jito_only requires JITO_BLOCK_ENGINE_SEND_URLS" >&2
      exit 1
    fi
    ;;
  helius_sender_only)
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_sender_only requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  nozomi_only)
    case "$(printf '%s' "$JITO_NOZOMI_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=nozomi_only requires JITO_NOZOMI_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_nozomi_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_stack requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_NOZOMI_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_stack requires JITO_NOZOMI_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  astralane_only)
    case "$(printf '%s' "$JITO_ASTRALANE_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=astralane_only requires JITO_ASTRALANE_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_astralane_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_astralane_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_astralane_stack requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_ASTRALANE_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_astralane_stack requires JITO_ASTRALANE_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_nozomi_astralane_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_astralane_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_astralane_stack requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_NOZOMI_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_astralane_stack requires JITO_NOZOMI_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_ASTRALANE_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_astralane_stack requires JITO_ASTRALANE_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_nozomi_astralane_lunar_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_astralane_lunar_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    for required in JITO_HELIUS_SENDER_ENABLED JITO_NOZOMI_ENABLED JITO_ASTRALANE_ENABLED JITO_LUNAR_LANDER_ENABLED; do
      case "$(printf '%s' "${!required}" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_SEND_LANE_MODE=helius_nozomi_astralane_lunar_stack requires $required=YES" >&2; exit 1 ;;
      esac
    done
    ;;
  lunar_lander_only)
    case "$(printf '%s' "$JITO_LUNAR_LANDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=lunar_lander_only requires JITO_LUNAR_LANDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_lunar_lander_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_lunar_lander_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_lunar_lander_stack requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_LUNAR_LANDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_lunar_lander_stack requires JITO_LUNAR_LANDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  circular_fast_only)
    case "$(printf '%s' "$JITO_CIRCULAR_FAST_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=circular_fast_only requires JITO_CIRCULAR_FAST_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_circular_fast_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_circular_fast_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_circular_fast_stack requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_CIRCULAR_FAST_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_circular_fast_stack requires JITO_CIRCULAR_FAST_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  erpc_swqos_only)
    case "$(printf '%s' "$JITO_ERPC_SWQOS_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=erpc_swqos_only requires JITO_ERPC_SWQOS_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_erpc_swqos_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_erpc_swqos_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_erpc_swqos_stack requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_ERPC_SWQOS_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_erpc_swqos_stack requires JITO_ERPC_SWQOS_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  beam_only)
    case "$(printf '%s' "$JITO_BEAM_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=beam_only requires JITO_BEAM_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_beam_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_beam_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_beam_stack requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_BEAM_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_beam_stack requires JITO_BEAM_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_nozomi_beam_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_beam_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_beam_stack requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_NOZOMI_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_beam_stack requires JITO_NOZOMI_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_BEAM_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_beam_stack requires JITO_BEAM_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  zero_slot_only)
    case "$(printf '%s' "$JITO_ZERO_SLOT_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=zero_slot_only requires JITO_ZERO_SLOT_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_zero_slot_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_zero_slot_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_zero_slot_stack requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_ZERO_SLOT_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_zero_slot_stack requires JITO_ZERO_SLOT_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_nozomi_zero_slot_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_zero_slot_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_zero_slot_stack requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_NOZOMI_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_zero_slot_stack requires JITO_NOZOMI_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_ZERO_SLOT_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_nozomi_zero_slot_stack requires JITO_ZERO_SLOT_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  all_non_beam_stack)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=all_non_beam_stack requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    for required in JITO_HELIUS_SENDER_ENABLED JITO_NOZOMI_ENABLED JITO_ZERO_SLOT_ENABLED; do
      case "$(printf '%s' "${!required}" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_SEND_LANE_MODE=all_non_beam_stack requires $required=YES" >&2; exit 1 ;;
      esac
    done
    ;;
  helius_tpu_jet)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_tpu_jet requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_tpu_jet requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_TPU_JET_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_tpu_jet requires JITO_TPU_JET_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  helius_tpu_quic)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_tpu_quic requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_tpu_quic requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_TPU_QUIC_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=helius_tpu_quic requires JITO_TPU_QUIC_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  tpu_jet_helius_tip)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=tpu_jet_helius_tip requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=tpu_jet_helius_tip requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_TPU_JET_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=tpu_jet_helius_tip requires JITO_TPU_JET_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  tpu_quic_helius_tip)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=tpu_quic_helius_tip requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=tpu_quic_helius_tip requires JITO_HELIUS_SENDER_ENABLED=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_TPU_QUIC_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=tpu_quic_helius_tip requires JITO_TPU_QUIC_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  tpu_jet_only)
    case "$(printf '%s' "$JITO_TPU_JET_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=tpu_jet_only requires JITO_TPU_JET_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
  tpu_quic_only)
    case "$(printf '%s' "$JITO_TPU_QUIC_ENABLED" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_SEND_LANE_MODE=tpu_quic_only requires JITO_TPU_QUIC_ENABLED=YES" >&2; exit 1 ;;
    esac
    ;;
esac
HELIUS_SENDER_ENABLED_NORMALIZED="$(printf '%s' "$JITO_HELIUS_SENDER_ENABLED" | tr '[:upper:]' '[:lower:]')"
HELIUS_SENDER_SWQOS_NORMALIZED="$(printf '%s' "$JITO_HELIUS_SENDER_SWQOS_ONLY" | tr '[:upper:]' '[:lower:]')"
NOZOMI_ENABLED_NORMALIZED="$(printf '%s' "$JITO_NOZOMI_ENABLED" | tr '[:upper:]' '[:lower:]')"
ASTRALANE_ENABLED_NORMALIZED="$(printf '%s' "$JITO_ASTRALANE_ENABLED" | tr '[:upper:]' '[:lower:]')"
LUNAR_LANDER_ENABLED_NORMALIZED="$(printf '%s' "$JITO_LUNAR_LANDER_ENABLED" | tr '[:upper:]' '[:lower:]')"
CIRCULAR_FAST_ENABLED_NORMALIZED="$(printf '%s' "$JITO_CIRCULAR_FAST_ENABLED" | tr '[:upper:]' '[:lower:]')"
ERPC_SWQOS_ENABLED_NORMALIZED="$(printf '%s' "$JITO_ERPC_SWQOS_ENABLED" | tr '[:upper:]' '[:lower:]')"
ERPC_LEADER_SLOTS_ENABLED_NORMALIZED="$(printf '%s' "$JITO_ERPC_LEADER_SLOTS_ENABLED" | tr '[:upper:]' '[:lower:]')"
BEAM_ENABLED_NORMALIZED="$(printf '%s' "${JITO_BEAM_ENABLED:-false}" | tr '[:upper:]' '[:lower:]')"
ZERO_SLOT_ENABLED_NORMALIZED="$(printf '%s' "${JITO_ZERO_SLOT_ENABLED:-false}" | tr '[:upper:]' '[:lower:]')"
TPU_JET_ENABLED_NORMALIZED="$(printf '%s' "$JITO_TPU_JET_ENABLED" | tr '[:upper:]' '[:lower:]')"
TPU_QUIC_ENABLED_NORMALIZED="$(printf '%s' "$JITO_TPU_QUIC_ENABLED" | tr '[:upper:]' '[:lower:]')"
case "$HELIUS_SENDER_ENABLED_NORMALIZED" in
  yes|true|1|on)
    case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_HELIUS_SENDER_ENABLED requires JITO_SEND_FANOUT=YES" >&2; exit 1 ;;
    esac
    case "$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_HELIUS_SENDER_ENABLED requires JITO_FAST_COPY_SEND=YES" >&2; exit 1 ;;
    esac
    if [[ -z "$JITO_HELIUS_SENDER_URLS" ]]; then
      echo "JITO_HELIUS_SENDER_ENABLED requires JITO_HELIUS_SENDER_URLS" >&2
      exit 1
    fi
    if [[ -z "$JITO_PRIORITY_FEE_MICRO_LAMPORTS" || "$JITO_PRIORITY_FEE_MICRO_LAMPORTS" == "0" ]]; then
      echo "JITO_HELIUS_SENDER_ENABLED requires JITO_PRIORITY_FEE_MICRO_LAMPORTS" >&2
      exit 1
    fi
    if [[ -z "$JITO_HELIUS_SENDER_TIP_LAMPORTS" || "$JITO_HELIUS_SENDER_TIP_LAMPORTS" == "0" ]]; then
      echo "JITO_HELIUS_SENDER_ENABLED requires JITO_HELIUS_SENDER_TIP_LAMPORTS" >&2
      exit 1
    fi
    HELIUS_SENDER_MIN_TIP=200000
    case "$HELIUS_SENDER_SWQOS_NORMALIZED" in
      yes|true|1|on) HELIUS_SENDER_MIN_TIP=5000 ;;
    esac
    if (( JITO_HELIUS_SENDER_TIP_LAMPORTS < HELIUS_SENDER_MIN_TIP )); then
      echo "JITO_HELIUS_SENDER_TIP_LAMPORTS must be >= $HELIUS_SENDER_MIN_TIP lamports" >&2
      exit 1
    fi
    if [[ -z "$JITO_HELIUS_SENDER_TIP_ACCOUNT" ]]; then
      echo "JITO_HELIUS_SENDER_ENABLED requires JITO_HELIUS_SENDER_TIP_ACCOUNT" >&2
      exit 1
    fi
    ;;
esac
case "$NOZOMI_ENABLED_NORMALIZED" in
  yes|true|1|on)
    if [[ "$SEND_LANE_MODE_NORMALIZED" != "nozomi_only" ]]; then
      case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_NOZOMI_ENABLED requires JITO_SEND_FANOUT=YES unless JITO_SEND_LANE_MODE=nozomi_only" >&2; exit 1 ;;
      esac
    fi
    case "$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_NOZOMI_ENABLED requires JITO_FAST_COPY_SEND=YES" >&2; exit 1 ;;
    esac
    if [[ -z "$JITO_NOZOMI_URLS" ]]; then
      echo "JITO_NOZOMI_ENABLED requires JITO_NOZOMI_URLS" >&2
      exit 1
    fi
    if [[ -z "$JITO_PRIORITY_FEE_MICRO_LAMPORTS" || "$JITO_PRIORITY_FEE_MICRO_LAMPORTS" == "0" ]]; then
      echo "JITO_NOZOMI_ENABLED requires JITO_PRIORITY_FEE_MICRO_LAMPORTS" >&2
      exit 1
    fi
    if [[ -z "$JITO_NOZOMI_TIP_LAMPORTS" || "$JITO_NOZOMI_TIP_LAMPORTS" == "0" ]]; then
      echo "JITO_NOZOMI_ENABLED requires JITO_NOZOMI_TIP_LAMPORTS" >&2
      exit 1
    fi
    if (( JITO_NOZOMI_TIP_LAMPORTS < 1000000 )); then
      echo "JITO_NOZOMI_TIP_LAMPORTS must be >= 1000000 lamports" >&2
      exit 1
    fi
    if [[ -z "$JITO_NOZOMI_TIP_ACCOUNT" ]]; then
      echo "JITO_NOZOMI_ENABLED requires JITO_NOZOMI_TIP_ACCOUNT" >&2
      exit 1
    fi
    ;;
esac
case "$ASTRALANE_ENABLED_NORMALIZED" in
  yes|true|1|on)
    if [[ "$SEND_LANE_MODE_NORMALIZED" != "astralane_only" ]]; then
      case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_ASTRALANE_ENABLED requires JITO_SEND_FANOUT=YES unless JITO_SEND_LANE_MODE=astralane_only" >&2; exit 1 ;;
      esac
    fi
    case "$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_ASTRALANE_ENABLED requires JITO_FAST_COPY_SEND=YES" >&2; exit 1 ;;
    esac
    if [[ -z "$JITO_ASTRALANE_URLS" ]]; then
      echo "JITO_ASTRALANE_ENABLED requires JITO_ASTRALANE_URLS" >&2
      exit 1
    fi
    if [[ -z "$JITO_ASTRALANE_API_KEY" ]]; then
      echo "JITO_ASTRALANE_ENABLED requires JITO_ASTRALANE_API_KEY" >&2
      exit 1
    fi
    if [[ -z "$JITO_PRIORITY_FEE_MICRO_LAMPORTS" || "$JITO_PRIORITY_FEE_MICRO_LAMPORTS" == "0" ]]; then
      echo "JITO_ASTRALANE_ENABLED requires JITO_PRIORITY_FEE_MICRO_LAMPORTS" >&2
      exit 1
    fi
    if [[ -z "$JITO_ASTRALANE_TIP_LAMPORTS" || "$JITO_ASTRALANE_TIP_LAMPORTS" == "0" ]]; then
      echo "JITO_ASTRALANE_ENABLED requires JITO_ASTRALANE_TIP_LAMPORTS" >&2
      exit 1
    fi
    if (( JITO_ASTRALANE_TIP_LAMPORTS < 1000000 )); then
      echo "JITO_ASTRALANE_TIP_LAMPORTS must be >= 1000000 lamports" >&2
      exit 1
    fi
    if [[ -z "$JITO_ASTRALANE_TIP_ACCOUNT" && -z "$JITO_ASTRALANE_TIP_ACCOUNTS" ]]; then
      echo "JITO_ASTRALANE_ENABLED requires JITO_ASTRALANE_TIP_ACCOUNT or JITO_ASTRALANE_TIP_ACCOUNTS" >&2
      exit 1
    fi
    ;;
esac
case "$LUNAR_LANDER_ENABLED_NORMALIZED" in
  yes|true|1|on)
    if [[ "$SEND_LANE_MODE_NORMALIZED" != "lunar_lander_only" ]]; then
      case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_LUNAR_LANDER_ENABLED requires JITO_SEND_FANOUT=YES unless JITO_SEND_LANE_MODE=lunar_lander_only" >&2; exit 1 ;;
      esac
    fi
    case "$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_LUNAR_LANDER_ENABLED requires JITO_FAST_COPY_SEND=YES" >&2; exit 1 ;;
    esac
    if [[ -z "$JITO_LUNAR_LANDER_URLS" ]]; then
      echo "JITO_LUNAR_LANDER_ENABLED requires JITO_LUNAR_LANDER_URLS" >&2
      exit 1
    fi
    if [[ -z "$JITO_LUNAR_LANDER_API_KEY" ]]; then
      echo "JITO_LUNAR_LANDER_ENABLED requires JITO_LUNAR_LANDER_API_KEY" >&2
      exit 1
    fi
    if [[ -z "$JITO_LUNAR_LANDER_TIP_LAMPORTS" || "$JITO_LUNAR_LANDER_TIP_LAMPORTS" == "0" ]]; then
      echo "JITO_LUNAR_LANDER_ENABLED requires JITO_LUNAR_LANDER_TIP_LAMPORTS" >&2
      exit 1
    fi
    if (( JITO_LUNAR_LANDER_TIP_LAMPORTS < 1000000 )); then
      echo "JITO_LUNAR_LANDER_TIP_LAMPORTS must be >= 1000000 lamports" >&2
      exit 1
    fi
    if [[ -z "$JITO_LUNAR_LANDER_TIP_ACCOUNT" && -z "$JITO_LUNAR_LANDER_TIP_ACCOUNTS" ]]; then
      echo "JITO_LUNAR_LANDER_ENABLED requires JITO_LUNAR_LANDER_TIP_ACCOUNT or JITO_LUNAR_LANDER_TIP_ACCOUNTS" >&2
      exit 1
    fi
    ;;
esac
case "$CIRCULAR_FAST_ENABLED_NORMALIZED" in
  yes|true|1|on)
    if [[ "$SEND_LANE_MODE_NORMALIZED" != "circular_fast_only" ]]; then
      case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_CIRCULAR_FAST_ENABLED requires JITO_SEND_FANOUT=YES unless JITO_SEND_LANE_MODE=circular_fast_only" >&2; exit 1 ;;
      esac
    fi
    case "$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_CIRCULAR_FAST_ENABLED requires JITO_FAST_COPY_SEND=YES" >&2; exit 1 ;;
    esac
    if [[ -z "$JITO_CIRCULAR_FAST_URLS" ]]; then
      echo "JITO_CIRCULAR_FAST_ENABLED requires JITO_CIRCULAR_FAST_URLS" >&2
      exit 1
    fi
    if [[ -z "$JITO_CIRCULAR_FAST_API_KEY" ]]; then
      echo "JITO_CIRCULAR_FAST_ENABLED requires JITO_CIRCULAR_FAST_API_KEY" >&2
      exit 1
    fi
    if [[ -z "$JITO_PRIORITY_FEE_MICRO_LAMPORTS" || "$JITO_PRIORITY_FEE_MICRO_LAMPORTS" == "0" ]]; then
      echo "JITO_CIRCULAR_FAST_ENABLED requires JITO_PRIORITY_FEE_MICRO_LAMPORTS" >&2
      exit 1
    fi
    if [[ -z "$JITO_CIRCULAR_FAST_TIP_LAMPORTS" || "$JITO_CIRCULAR_FAST_TIP_LAMPORTS" == "0" ]]; then
      echo "JITO_CIRCULAR_FAST_ENABLED requires JITO_CIRCULAR_FAST_TIP_LAMPORTS" >&2
      exit 1
    fi
    if (( JITO_CIRCULAR_FAST_TIP_LAMPORTS < 1000000 )); then
      echo "JITO_CIRCULAR_FAST_TIP_LAMPORTS must be >= 1000000 lamports" >&2
      exit 1
    fi
    if [[ -z "$JITO_CIRCULAR_FAST_TIP_ACCOUNT" && -z "$JITO_CIRCULAR_FAST_TIP_ACCOUNTS" ]]; then
      echo "JITO_CIRCULAR_FAST_ENABLED requires JITO_CIRCULAR_FAST_TIP_ACCOUNT or JITO_CIRCULAR_FAST_TIP_ACCOUNTS" >&2
      exit 1
    fi
    ;;
esac
case "$ERPC_SWQOS_ENABLED_NORMALIZED" in
  yes|true|1|on)
    if [[ "$SEND_LANE_MODE_NORMALIZED" != "erpc_swqos_only" ]]; then
      case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_ERPC_SWQOS_ENABLED requires JITO_SEND_FANOUT=YES unless JITO_SEND_LANE_MODE=erpc_swqos_only" >&2; exit 1 ;;
      esac
    fi
    case "$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_ERPC_SWQOS_ENABLED requires JITO_FAST_COPY_SEND=YES" >&2; exit 1 ;;
    esac
    if [[ -z "$JITO_ERPC_SWQOS_URLS" ]]; then
      echo "JITO_ERPC_SWQOS_ENABLED requires JITO_ERPC_SWQOS_URLS" >&2
      exit 1
    fi
    ;;
esac
case "$ERPC_LEADER_SLOTS_ENABLED_NORMALIZED" in
  yes|true|1|on)
    if [[ -z "$JITO_ERPC_LEADER_SLOTS_URL" ]]; then
      echo "JITO_ERPC_LEADER_SLOTS_ENABLED requires JITO_ERPC_LEADER_SLOTS_URL" >&2
      exit 1
    fi
    if [[ -z "$JITO_ERPC_API_KEY" ]]; then
      echo "JITO_ERPC_LEADER_SLOTS_ENABLED requires JITO_ERPC_API_KEY" >&2
      exit 1
    fi
    if [[ "$JITO_ERPC_LEADER_SLOTS_REFRESH_MS" == "0" ]]; then
      echo "JITO_ERPC_LEADER_SLOTS_REFRESH_MS must be positive" >&2
      exit 1
    fi
    if [[ "$JITO_ERPC_LEADER_SLOTS_STALE_MS" == "0" ]]; then
      echo "JITO_ERPC_LEADER_SLOTS_STALE_MS must be positive" >&2
      exit 1
    fi
    ;;
esac
case "$BEAM_ENABLED_NORMALIZED" in
  yes|true|1|on)
    if [[ "$SEND_LANE_MODE_NORMALIZED" != "beam_only" ]]; then
      case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_BEAM_ENABLED requires JITO_SEND_FANOUT=YES unless JITO_SEND_LANE_MODE=beam_only" >&2; exit 1 ;;
      esac
    fi
    case "$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_BEAM_ENABLED requires JITO_FAST_COPY_SEND=YES" >&2; exit 1 ;;
    esac
    if [[ -z "${JITO_BEAM_URL:-}" ]]; then
      echo "JITO_BEAM_ENABLED requires JITO_BEAM_URL" >&2
      exit 1
    fi
    if [[ -z "${JITO_BEAM_TOKEN:-}" ]]; then
      echo "JITO_BEAM_ENABLED requires JITO_BEAM_TOKEN" >&2
      exit 1
    fi
    case "${JITO_BEAM_PROVIDER:-}" in
      bloxroute|astralane|falcon) ;;
      *) echo "JITO_BEAM_PROVIDER must be bloxroute, astralane, or falcon" >&2; exit 1 ;;
    esac
    case "${JITO_BEAM_MODE:-}" in
      fastest|mev_protect) ;;
      *) echo "JITO_BEAM_MODE must be fastest or mev_protect" >&2; exit 1 ;;
    esac
    if [[ "${JITO_BEAM_PROVIDER:-}" == "falcon" && "${JITO_BEAM_MODE:-}" == "mev_protect" ]]; then
      echo "JITO_BEAM_MODE=mev_protect is not supported with falcon" >&2
      exit 1
    fi
    if [[ -z "${JITO_BEAM_TIP_LAMPORTS:-}" || "$JITO_BEAM_TIP_LAMPORTS" == "0" ]]; then
      echo "JITO_BEAM_ENABLED requires JITO_BEAM_TIP_LAMPORTS" >&2
      exit 1
    fi
    if (( JITO_BEAM_TIP_LAMPORTS < 1000000 )); then
      echo "JITO_BEAM_TIP_LAMPORTS must be >= 1000000 lamports" >&2
      exit 1
    fi
    if [[ -z "${JITO_BEAM_TIP_ACCOUNTS:-}" ]]; then
      echo "JITO_BEAM_ENABLED requires JITO_BEAM_TIP_ACCOUNTS" >&2
      exit 1
    fi
    ;;
esac
case "$ZERO_SLOT_ENABLED_NORMALIZED" in
  yes|true|1|on)
    if [[ "$SEND_LANE_MODE_NORMALIZED" != "zero_slot_only" ]]; then
      case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_ZERO_SLOT_ENABLED requires JITO_SEND_FANOUT=YES unless JITO_SEND_LANE_MODE=zero_slot_only" >&2; exit 1 ;;
      esac
    fi
    case "$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_ZERO_SLOT_ENABLED requires JITO_FAST_COPY_SEND=YES" >&2; exit 1 ;;
    esac
    if [[ -z "${JITO_ZERO_SLOT_URLS:-}" ]]; then
      echo "JITO_ZERO_SLOT_ENABLED requires JITO_ZERO_SLOT_URLS" >&2
      exit 1
    fi
    if [[ -z "${JITO_ZERO_SLOT_API_KEY:-}" ]]; then
      echo "JITO_ZERO_SLOT_ENABLED requires JITO_ZERO_SLOT_API_KEY" >&2
      exit 1
    fi
    if [[ -z "${JITO_ZERO_SLOT_TIP_LAMPORTS:-}" || "$JITO_ZERO_SLOT_TIP_LAMPORTS" == "0" ]]; then
      echo "JITO_ZERO_SLOT_ENABLED requires JITO_ZERO_SLOT_TIP_LAMPORTS" >&2
      exit 1
    fi
    if (( JITO_ZERO_SLOT_TIP_LAMPORTS < 1000000 )); then
      echo "JITO_ZERO_SLOT_TIP_LAMPORTS must be >= 1000000 lamports" >&2
      exit 1
    fi
    if [[ -z "${JITO_ZERO_SLOT_TIP_ACCOUNTS:-}" ]]; then
      echo "JITO_ZERO_SLOT_ENABLED requires JITO_ZERO_SLOT_TIP_ACCOUNTS" >&2
      exit 1
    fi
    ;;
esac
case "$TPU_JET_ENABLED_NORMALIZED" in
  yes|true|1|on)
    if [[ "$SEND_LANE_MODE_NORMALIZED" != "tpu_jet_only" ]]; then
      case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_TPU_JET_ENABLED requires JITO_SEND_FANOUT=YES unless JITO_SEND_LANE_MODE=tpu_jet_only" >&2; exit 1 ;;
      esac
    fi
    case "$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_TPU_JET_ENABLED requires JITO_FAST_COPY_SEND=YES" >&2; exit 1 ;;
    esac
    if [[ -z "$JITO_TPU_JET_RPC_URL" ]]; then
      echo "JITO_TPU_JET_ENABLED requires JITO_TPU_JET_RPC_URL" >&2
      exit 1
    fi
    if [[ -z "$JITO_TPU_JET_WS_URL" ]]; then
      echo "JITO_TPU_JET_ENABLED requires JITO_TPU_JET_WS_URL" >&2
      exit 1
    fi
    if [[ -z "$JITO_TPU_JET_SIDECAR_URL" ]]; then
      echo "JITO_TPU_JET_ENABLED requires JITO_TPU_JET_SIDECAR_URL" >&2
      exit 1
    fi
    if [[ "$JITO_TPU_JET_FANOUT_SLOTS" == "0" ]]; then
      echo "JITO_TPU_JET_FANOUT_SLOTS must be positive" >&2
      exit 1
    fi
    ;;
esac
case "$TPU_QUIC_ENABLED_NORMALIZED" in
  yes|true|1|on)
    if [[ "$SEND_LANE_MODE_NORMALIZED" != "tpu_quic_only" ]]; then
      case "$(printf '%s' "$JITO_SEND_FANOUT" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *) echo "JITO_TPU_QUIC_ENABLED requires JITO_SEND_FANOUT=YES unless JITO_SEND_LANE_MODE=tpu_quic_only" >&2; exit 1 ;;
      esac
    fi
    case "$(printf '%s' "$JITO_FAST_COPY_SEND" | tr '[:upper:]' '[:lower:]')" in
      yes|true|1|on) ;;
      *) echo "JITO_TPU_QUIC_ENABLED requires JITO_FAST_COPY_SEND=YES" >&2; exit 1 ;;
    esac
    if [[ -z "$JITO_TPU_QUIC_RPC_URL" ]]; then
      echo "JITO_TPU_QUIC_ENABLED requires JITO_TPU_QUIC_RPC_URL" >&2
      exit 1
    fi
    if [[ -z "$JITO_TPU_QUIC_WS_URL" ]]; then
      echo "JITO_TPU_QUIC_ENABLED requires JITO_TPU_QUIC_WS_URL" >&2
      exit 1
    fi
    if [[ "$JITO_TPU_QUIC_FANOUT_SLOTS" == "0" ]]; then
      echo "JITO_TPU_QUIC_FANOUT_SLOTS must be positive" >&2
      exit 1
    fi
    ;;
esac

echo "DROPLET LIVE COPY SEND IS ARMED"
echo "  proxy: $JITO_SHREDSTREAM_PROXY_URL"
echo "  target: $SHREDSTREAM_TARGET_WALLETS"
echo "  copy wallet: $JITO_COPY_WALLET"
echo "  max copy SOL: $JITO_MAX_COPY_SOL"
echo "  max total copy spend SOL: $JITO_MAX_TOTAL_COPY_SPEND_SOL"
echo "  migrated AMM min copy SOL: $JITO_MIGRATED_AMM_MIN_COPY_SOL"
echo "  migrated AMM small copy mode: $JITO_MIGRATED_AMM_SMALL_COPY_MODE"
echo "  executions: $JITO_COPY_EXECUTIONS_PATH"
echo "  copy executions write queue capacity: $JITO_COPY_EXECUTIONS_WRITE_QUEUE_CAPACITY"
echo "  copy executions flush interval ms: $JITO_COPY_EXECUTIONS_FLUSH_INTERVAL_MS"
echo "  copy execution concurrency: $JITO_COPY_EXECUTION_CONCURRENCY"
echo "  copy execution queue capacity: $JITO_COPY_EXECUTION_QUEUE_CAPACITY"
echo "  fast copy send: $JITO_FAST_COPY_SEND"
echo "  send fanout: $JITO_SEND_FANOUT"
echo "  send lane mode: $JITO_SEND_LANE_MODE"
echo "  state rpc urls: $(if [[ -n "${JITO_STATE_RPC_URLS:-}" ]]; then printf '%s' "$JITO_STATE_RPC_URLS" | awk -F, '{print NF}'; elif [[ -n "${SOLANA_RPC_URL:-}" ]]; then printf '1'; else printf '0'; fi) configured"
echo "  send rpc urls: $(if [[ -n "${JITO_SEND_RPC_URLS:-}" ]]; then printf '%s' "$JITO_SEND_RPC_URLS" | awk -F, '{print NF}'; else printf '0'; fi) configured"
echo "  sell send rpc urls: $(if [[ -n "${JITO_SELL_SEND_RPC_URLS:-}" ]]; then printf '%s' "$JITO_SELL_SEND_RPC_URLS" | awk -F, '{print NF}'; else printf '0'; fi) configured"
if [[ -n "$JITO_BLOCK_ENGINE_SEND_URLS" ]]; then
  echo "  jito send urls: $(printf '%s' "$JITO_BLOCK_ENGINE_SEND_URLS" | awk -F, '{print NF}') configured"
else
  echo "  jito send urls: 0 configured"
fi
echo "  helius sender enabled: $JITO_HELIUS_SENDER_ENABLED"
echo "  helius sender urls: $(if [[ -n "$JITO_HELIUS_SENDER_URLS" ]]; then printf '%s' "$JITO_HELIUS_SENDER_URLS" | awk -F, '{print NF}'; else printf '0'; fi) configured"
echo "  helius sender swqos only: $JITO_HELIUS_SENDER_SWQOS_ONLY"
echo "  helius sender tip lamports: ${JITO_HELIUS_SENDER_TIP_LAMPORTS:-0}"
if [[ -n "$JITO_HELIUS_SENDER_TIP_ACCOUNT" ]]; then
  echo "  helius sender tip account: configured"
else
  echo "  helius sender tip account: unset"
fi
echo "  nozomi enabled: $JITO_NOZOMI_ENABLED"
echo "  nozomi urls: $(if [[ -n "$JITO_NOZOMI_URLS" ]]; then printf '%s' "$JITO_NOZOMI_URLS" | awk -F, '{print NF}'; else printf '0'; fi) configured"
echo "  nozomi tip lamports: ${JITO_NOZOMI_TIP_LAMPORTS:-0}"
if [[ -n "$JITO_NOZOMI_TIP_ACCOUNT" ]]; then
  echo "  nozomi tip account: configured"
else
  echo "  nozomi tip account: unset"
fi
echo "  astralane enabled: $JITO_ASTRALANE_ENABLED"
echo "  astralane urls: $(if [[ -n "$JITO_ASTRALANE_URLS" ]]; then printf '%s' "$JITO_ASTRALANE_URLS" | awk -F, '{print NF}'; else printf '0'; fi) configured"
if [[ -n "$JITO_ASTRALANE_API_KEY" ]]; then
  echo "  astralane api key: configured"
else
  echo "  astralane api key: unset"
fi
echo "  astralane tip lamports: ${JITO_ASTRALANE_TIP_LAMPORTS:-0}"
if [[ -n "$JITO_ASTRALANE_TIP_ACCOUNT" ]]; then
  echo "  astralane tip account: configured"
else
  echo "  astralane tip account: unset"
fi
if [[ -n "${JITO_ASTRALANE_TIP_ACCOUNTS:-}" ]]; then
  echo "  astralane tip accounts: configured"
else
  echo "  astralane tip accounts: unset"
fi
echo "  astralane mev protect: $JITO_ASTRALANE_MEV_PROTECT"
echo "  astralane swqos only: $JITO_ASTRALANE_SWQOS_ONLY"
echo "  lunar lander enabled: $JITO_LUNAR_LANDER_ENABLED"
echo "  lunar lander urls: $(if [[ -n "$JITO_LUNAR_LANDER_URLS" ]]; then printf '%s' "$JITO_LUNAR_LANDER_URLS" | awk -F, '{print NF}'; else printf '0'; fi) configured"
if [[ -n "$JITO_LUNAR_LANDER_API_KEY" ]]; then
  echo "  lunar lander api key: configured"
else
  echo "  lunar lander api key: unset"
fi
echo "  lunar lander tip lamports: ${JITO_LUNAR_LANDER_TIP_LAMPORTS:-0}"
if [[ -n "$JITO_LUNAR_LANDER_TIP_ACCOUNT" ]]; then
  echo "  lunar lander tip account: configured"
else
  echo "  lunar lander tip account: unset"
fi
if [[ -n "${JITO_LUNAR_LANDER_TIP_ACCOUNTS:-}" ]]; then
  echo "  lunar lander tip accounts: configured"
else
  echo "  lunar lander tip accounts: unset"
fi
echo "  lunar lander mev protect: $JITO_LUNAR_LANDER_MEV_PROTECT"
echo "  circular fast enabled: $JITO_CIRCULAR_FAST_ENABLED"
echo "  circular fast urls: $(if [[ -n "$JITO_CIRCULAR_FAST_URLS" ]]; then printf '%s' "$JITO_CIRCULAR_FAST_URLS" | awk -F, '{print NF}'; else printf '0'; fi) configured"
if [[ -n "$JITO_CIRCULAR_FAST_API_KEY" ]]; then
  echo "  circular fast api key: configured"
else
  echo "  circular fast api key: unset"
fi
echo "  circular fast tip lamports: ${JITO_CIRCULAR_FAST_TIP_LAMPORTS:-0}"
if [[ -n "$JITO_CIRCULAR_FAST_TIP_ACCOUNT" ]]; then
  echo "  circular fast tip account: configured"
else
  echo "  circular fast tip account: unset"
fi
if [[ -n "${JITO_CIRCULAR_FAST_TIP_ACCOUNTS:-}" ]]; then
  echo "  circular fast tip accounts: configured"
else
  echo "  circular fast tip accounts: unset"
fi
echo "  circular fast front-running protection: $JITO_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION"
echo "  erpc swqos enabled: $JITO_ERPC_SWQOS_ENABLED"
echo "  erpc swqos urls: $(if [[ -n "$JITO_ERPC_SWQOS_URLS" ]]; then printf '%s' "$JITO_ERPC_SWQOS_URLS" | awk -F, '{print NF}'; else printf '0'; fi) configured"
echo "  erpc leader slots enabled: $JITO_ERPC_LEADER_SLOTS_ENABLED"
if [[ -n "$JITO_ERPC_LEADER_SLOTS_URL" ]]; then
  echo "  erpc leader slots url: configured"
else
  echo "  erpc leader slots url: unset"
fi
if [[ -n "$JITO_ERPC_API_KEY" ]]; then
  echo "  erpc api key: configured"
else
  echo "  erpc api key: unset"
fi
echo "  erpc leader slots refresh/stale ms: $JITO_ERPC_LEADER_SLOTS_REFRESH_MS/$JITO_ERPC_LEADER_SLOTS_STALE_MS"
if [[ -n "$JITO_ERPC_YELLOWSTONE_GRPC_URL" ]]; then
  echo "  erpc yellowstone grpc url: configured"
else
  echo "  erpc yellowstone grpc url: unset"
fi
if [[ -n "$JITO_ERPC_YELLOWSTONE_GRPC_X_TOKEN" ]]; then
  echo "  erpc yellowstone grpc x-token: configured"
else
  echo "  erpc yellowstone grpc x-token: unset"
fi
if [[ -n "$JITO_SHREDER_FASTLANE_GRPC_URL" ]]; then
  echo "  shreder fastlane grpc url: configured"
else
  echo "  shreder fastlane grpc url: unset"
fi
if [[ -n "$JITO_SHREDER_FASTLANE_GRPC_X_TOKEN" ]]; then
  echo "  shreder fastlane grpc x-token: configured"
else
  echo "  shreder fastlane grpc x-token: unset"
fi
echo "  beam enabled: ${JITO_BEAM_ENABLED:-false}"
if [[ -n "${JITO_BEAM_URL:-}" ]]; then
  echo "  beam url: configured"
else
  echo "  beam url: unset"
fi
if [[ -n "${JITO_BEAM_TOKEN:-}" ]]; then
  echo "  beam token: configured"
else
  echo "  beam token: unset"
fi
echo "  beam provider: ${JITO_BEAM_PROVIDER:-}"
echo "  beam mode: ${JITO_BEAM_MODE:-}"
echo "  beam tip lamports: ${JITO_BEAM_TIP_LAMPORTS:-0}"
if [[ -n "${JITO_BEAM_TIP_ACCOUNTS:-}" ]]; then
  echo "  beam tip accounts: configured"
else
  echo "  beam tip accounts: unset"
fi
echo "  zero slot enabled: ${JITO_ZERO_SLOT_ENABLED:-false}"
echo "  zero slot urls: $(if [[ -n "${JITO_ZERO_SLOT_URLS:-}" ]]; then printf '%s' "$JITO_ZERO_SLOT_URLS" | awk -F, '{print NF}'; else printf '0'; fi) configured"
if [[ -n "${JITO_ZERO_SLOT_API_KEY:-}" ]]; then
  echo "  zero slot api key: configured"
else
  echo "  zero slot api key: unset"
fi
echo "  zero slot tip lamports: ${JITO_ZERO_SLOT_TIP_LAMPORTS:-0}"
if [[ -n "${JITO_ZERO_SLOT_TIP_ACCOUNTS:-}" ]]; then
  echo "  zero slot tip accounts: configured"
else
  echo "  zero slot tip accounts: unset"
fi
echo "  tpu jet enabled: $JITO_TPU_JET_ENABLED"
if [[ -n "$JITO_TPU_JET_RPC_URL" ]]; then
  echo "  tpu jet rpc url: configured"
else
  echo "  tpu jet rpc url: unset"
fi
if [[ -n "$JITO_TPU_JET_WS_URL" ]]; then
  echo "  tpu jet ws/grpc url: configured"
  if [[ -n "$JITO_SHREDER_FASTLANE_GRPC_URL" && "$JITO_TPU_JET_WS_URL" == "$JITO_SHREDER_FASTLANE_GRPC_URL" ]]; then
    echo "  tpu jet ws/grpc source: shreder fastlane"
  elif [[ -n "$JITO_ERPC_YELLOWSTONE_GRPC_URL" && "$JITO_TPU_JET_WS_URL" == "$JITO_ERPC_YELLOWSTONE_GRPC_URL" ]]; then
    echo "  tpu jet ws/grpc source: erpc yellowstone"
  else
    echo "  tpu jet ws/grpc source: direct tpu jet env"
  fi
else
  echo "  tpu jet ws/grpc url: unset"
fi
if [[ -n "$JITO_TPU_JET_SIDECAR_URL" ]]; then
  echo "  tpu jet sidecar url: configured"
else
  echo "  tpu jet sidecar url: unset"
fi
echo "  tpu jet fanout slots: $JITO_TPU_JET_FANOUT_SLOTS"
echo "  tpu jet timeout ms: $JITO_TPU_JET_TIMEOUT_MS"
echo "  tpu quic enabled: $JITO_TPU_QUIC_ENABLED"
if [[ -n "$JITO_TPU_QUIC_RPC_URL" ]]; then
  echo "  tpu quic rpc url: configured"
else
  echo "  tpu quic rpc url: unset"
fi
if [[ -n "$JITO_TPU_QUIC_WS_URL" ]]; then
  echo "  tpu quic ws url: configured"
else
  echo "  tpu quic ws url: unset"
fi
echo "  tpu quic fanout slots: $JITO_TPU_QUIC_FANOUT_SLOTS"
echo "  tpu quic timeout ms: $JITO_TPU_QUIC_TIMEOUT_MS"
echo "  simulate copy tx: $JITO_SIMULATE_COPY_TX"
echo "  send enabled: $JITO_ENABLE_COPY_SEND"
echo "  one shot: $JITO_ONE_SHOT_COPY_SEND"
echo "  dry run: $JITO_DRY_RUN"
echo "  auto sell after buy: $JITO_AUTO_SELL_AFTER_BUY"
echo "  rust trailing sells: $JITO_RUST_TRAILING_SELLS_ENABLED"
echo "  direct Pump cashback guard fail-open: ${JITO_DIRECT_PUMP_CASHBACK_GUARD_FAIL_OPEN:-false}"
echo "  rust trailing sell confirmation timeout ms: $JITO_RUST_TRAILING_SELL_CONFIRMATION_TIMEOUT_MS"
echo "  rust trailing sell confirmation poll ms: $JITO_RUST_TRAILING_SELL_CONFIRMATION_POLL_MS"
echo "  simulate auto sell: $JITO_SIMULATE_AUTO_SELL"
echo "  isolate buy latency test: $JITO_ISOLATE_BUY_LATENCY_TEST"
echo "  send max retries: $JITO_SEND_MAX_RETRIES"
echo "  send http timeout ms: $JITO_SEND_HTTP_TIMEOUT_MS"
echo "  copy wallet balance refresh/stale ms: $JITO_COPY_WALLET_BALANCE_REFRESH_MS/$JITO_COPY_WALLET_BALANCE_STALE_MS"
echo "  balance cache rpc urls: $(if [[ -n "${JITO_BALANCE_CACHE_RPC_URLS:-}" ]]; then printf '%s' "$JITO_BALANCE_CACHE_RPC_URLS" | awk -F, '{print NF}'; else printf 'state fallback'; fi) configured"
echo "  blockhash refresh/timeout/stale ms: $JITO_BLOCKHASH_REFRESH_MS/$JITO_BLOCKHASH_REFRESH_TIMEOUT_MS/$JITO_BLOCKHASH_STALE_MS"
echo "  blockhash rpc urls: $(if [[ -n "${JITO_BLOCKHASH_RPC_URLS:-}" ]]; then printf '%s' "$JITO_BLOCKHASH_RPC_URLS" | awk -F, '{print NF}'; else printf 'state fallback'; fi) configured"
echo "  priority fee micro lamports: ${JITO_PRIORITY_FEE_MICRO_LAMPORTS:-0}"
echo "  max priority fee micro lamports: $JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS"
echo "  dynamic priority fee enabled: $JITO_DYNAMIC_PRIORITY_FEE_ENABLED"
echo "  dynamic priority baseline/aggressive/panic/max micro lamports: ${JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS:-0}/${JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS:-0}/${JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS:-0}/${JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS:-0}"
echo "  account priority fee enabled: $JITO_ACCOUNT_PRIORITY_FEE_ENABLED"
echo "  account priority fee refresh/stale/percentile: $JITO_ACCOUNT_PRIORITY_FEE_REFRESH_MS/$JITO_ACCOUNT_PRIORITY_FEE_STALE_MS/$JITO_ACCOUNT_PRIORITY_FEE_PERCENTILE"
echo "  priority fee rpc urls: $(if [[ -n "${JITO_PRIORITY_FEE_RPC_URLS:-}" ]]; then printf '%s' "$JITO_PRIORITY_FEE_RPC_URLS" | awk -F, '{print NF}'; else printf 'state fallback'; fi) configured"
echo "  jito tip lamports: ${JITO_TIP_LAMPORTS:-0}"
echo "  max jito tip lamports: $JITO_MAX_TIP_LAMPORTS"
echo "  max provider tip lamports: ${JITO_MAX_PROVIDER_TIP_LAMPORTS:-0}"
echo "  max signed tx bytes: ${JITO_MAX_SIGNED_TX_BYTES:-0}"
echo "  max instruction count: ${JITO_MAX_INSTRUCTION_COUNT:-0}"
echo "  max writable account count: ${JITO_MAX_WRITABLE_ACCOUNT_COUNT:-0}"
echo "  sell priority fee micro lamports: ${JITO_SELL_PRIORITY_FEE_MICRO_LAMPORTS:-0}"
echo "  sell jito tip lamports: ${JITO_SELL_TIP_LAMPORTS:-0}"
if [[ -n "$JITO_TIP_ACCOUNT" ]]; then
  echo "  jito tip account: configured"
else
  echo "  jito tip account: unset"
fi
if [[ -n "$JITO_SELL_TIP_ACCOUNT" ]]; then
  echo "  sell jito tip account: configured"
else
  echo "  sell jito tip account: unset"
fi
echo "  signal observations disabled: $JITO_DISABLE_SIGNAL_OBSERVATIONS"
echo "  signal observation queue capacity: $JITO_SIGNAL_OBSERVATION_QUEUE_CAPACITY"
echo "  print feed events: $JITO_PRINT_FEED_EVENTS"
echo "  print mentions: $JITO_PRINT_MENTIONS"
echo "  warm send endpoints: $JITO_WARM_SEND_ENDPOINTS"
echo "  send endpoint warm interval ms: $JITO_SEND_ENDPOINT_WARM_INTERVAL_MS"

cd "$WORKER_DIR"
LIVE_ARGS=()
if [[ -n "${JITO_TELEGRAM_SNAPSHOT_PATH:-}" ]]; then
  LIVE_ARGS+=(--telegram-snapshot-path "$JITO_TELEGRAM_SNAPSHOT_PATH")
fi
RUST_TRAILING_SELLS_NORMALIZED="$(printf '%s' "$JITO_RUST_TRAILING_SELLS_ENABLED" | tr '[:upper:]' '[:lower:]')"
case "$RUST_TRAILING_SELLS_NORMALIZED" in
  yes|true|1|on)
    LIVE_ARGS+=(--rust-trailing-sells-enabled)
    ;;
esac
LIVE_ARGS+=(
  --rust-trailing-sell-confirmation-timeout-ms "$JITO_RUST_TRAILING_SELL_CONFIRMATION_TIMEOUT_MS"
  --rust-trailing-sell-confirmation-poll-ms "$JITO_RUST_TRAILING_SELL_CONFIRMATION_POLL_MS"
)
if [[ -z "${JITO_HELIUS_SENDER_URLS:-}" ]]; then
  unset JITO_HELIUS_SENDER_URLS
fi
if [[ -z "${JITO_HELIUS_SENDER_TIP_LAMPORTS:-}" ]]; then
  unset JITO_HELIUS_SENDER_TIP_LAMPORTS
fi
if [[ -z "${JITO_HELIUS_SENDER_TIP_ACCOUNT:-}" ]]; then
  unset JITO_HELIUS_SENDER_TIP_ACCOUNT
fi
if [[ -z "${JITO_HELIUS_SENDER_TIP_ACCOUNTS:-}" ]]; then
  unset JITO_HELIUS_SENDER_TIP_ACCOUNTS
fi
if [[ -z "${JITO_NOZOMI_URLS:-}" ]]; then
  unset JITO_NOZOMI_URLS
fi
if [[ -z "${JITO_NOZOMI_TIP_LAMPORTS:-}" ]]; then
  unset JITO_NOZOMI_TIP_LAMPORTS
fi
if [[ -z "${JITO_NOZOMI_TIP_ACCOUNT:-}" ]]; then
  unset JITO_NOZOMI_TIP_ACCOUNT
fi
if [[ -z "${JITO_NOZOMI_TIP_ACCOUNTS:-}" ]]; then
  unset JITO_NOZOMI_TIP_ACCOUNTS
fi
if [[ -z "${JITO_ASTRALANE_URLS:-}" ]]; then
  unset JITO_ASTRALANE_URLS
fi
if [[ -z "${JITO_ASTRALANE_API_KEY:-}" ]]; then
  unset JITO_ASTRALANE_API_KEY
fi
if [[ -z "${JITO_ASTRALANE_TIP_LAMPORTS:-}" ]]; then
  unset JITO_ASTRALANE_TIP_LAMPORTS
fi
if [[ -z "${JITO_ASTRALANE_TIP_ACCOUNT:-}" ]]; then
  unset JITO_ASTRALANE_TIP_ACCOUNT
fi
if [[ -z "${JITO_ASTRALANE_TIP_ACCOUNTS:-}" ]]; then
  unset JITO_ASTRALANE_TIP_ACCOUNTS
fi
if [[ -z "${JITO_LUNAR_LANDER_URLS:-}" ]]; then
  unset JITO_LUNAR_LANDER_URLS
fi
if [[ -z "${JITO_LUNAR_LANDER_API_KEY:-}" ]]; then
  unset JITO_LUNAR_LANDER_API_KEY
fi
if [[ -z "${JITO_LUNAR_LANDER_TIP_LAMPORTS:-}" ]]; then
  unset JITO_LUNAR_LANDER_TIP_LAMPORTS
fi
if [[ -z "${JITO_LUNAR_LANDER_TIP_ACCOUNT:-}" ]]; then
  unset JITO_LUNAR_LANDER_TIP_ACCOUNT
fi
if [[ -z "${JITO_LUNAR_LANDER_TIP_ACCOUNTS:-}" ]]; then
  unset JITO_LUNAR_LANDER_TIP_ACCOUNTS
fi
if [[ -z "${JITO_ZERO_SLOT_URLS:-}" ]]; then
  unset JITO_ZERO_SLOT_URLS
fi
if [[ -z "${JITO_ZERO_SLOT_API_KEY:-}" ]]; then
  unset JITO_ZERO_SLOT_API_KEY
fi
if [[ -z "${JITO_ZERO_SLOT_TIP_LAMPORTS:-}" ]]; then
  unset JITO_ZERO_SLOT_TIP_LAMPORTS
fi
if [[ -z "${JITO_ZERO_SLOT_TIP_ACCOUNTS:-}" ]]; then
  unset JITO_ZERO_SLOT_TIP_ACCOUNTS
fi
if [[ -z "${JITO_MAX_PROVIDER_TIP_LAMPORTS:-}" ]]; then
  unset JITO_MAX_PROVIDER_TIP_LAMPORTS
fi
if [[ -z "${JITO_MAX_SIGNED_TX_BYTES:-}" ]]; then
  unset JITO_MAX_SIGNED_TX_BYTES
fi
if [[ -z "${JITO_MAX_INSTRUCTION_COUNT:-}" ]]; then
  unset JITO_MAX_INSTRUCTION_COUNT
fi
if [[ -z "${JITO_MAX_WRITABLE_ACCOUNT_COUNT:-}" ]]; then
  unset JITO_MAX_WRITABLE_ACCOUNT_COUNT
fi
exec "$WORKER_BIN" live "${LIVE_ARGS[@]}"
