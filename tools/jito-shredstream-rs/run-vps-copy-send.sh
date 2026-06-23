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
export JITO_TPU_JET_ENABLED="${JITO_TPU_JET_ENABLED:-false}"
export JITO_TPU_JET_RPC_URL="${JITO_TPU_JET_RPC_URL:-${SOLANA_RPC_URL:-}}"
export JITO_TPU_JET_WS_URL="${JITO_TPU_JET_WS_URL:-}"
export JITO_TPU_JET_SIDECAR_URL="${JITO_TPU_JET_SIDECAR_URL:-http://127.0.0.1:8787}"
export JITO_TPU_JET_FANOUT_SLOTS="${JITO_TPU_JET_FANOUT_SLOTS:-12}"
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
export JITO_TIP_LAMPORTS="${JITO_TIP_LAMPORTS:-${DIRECT_EXECUTION_JITO_TIP_LAMPORTS:-}}"
export JITO_TIP_ACCOUNT="${JITO_TIP_ACCOUNT:-${DIRECT_EXECUTION_JITO_TIP_ACCOUNT:-}}"
export JITO_SELL_PRIORITY_FEE_MICRO_LAMPORTS="${JITO_SELL_PRIORITY_FEE_MICRO_LAMPORTS:-$JITO_PRIORITY_FEE_MICRO_LAMPORTS}"
export JITO_SELL_TIP_LAMPORTS="${JITO_SELL_TIP_LAMPORTS:-$JITO_TIP_LAMPORTS}"
export JITO_SELL_TIP_ACCOUNT="${JITO_SELL_TIP_ACCOUNT:-$JITO_TIP_ACCOUNT}"
export JITO_MAX_TIP_LAMPORTS="${JITO_MAX_TIP_LAMPORTS:-50000}"
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
validate_nonnegative_int JITO_HELIUS_SENDER_TIP_LAMPORTS "$JITO_HELIUS_SENDER_TIP_LAMPORTS"
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
  mixed|rpc_only|jito_only|helius_sender_only|helius_tpu_jet|helius_tpu_quic|tpu_jet_helius_tip|tpu_quic_helius_tip|tpu_jet_only|tpu_quic_only)
    export JITO_SEND_LANE_MODE="${SEND_LANE_MODE_NORMALIZED//_/-}"
    ;;
  *)
    echo "JITO_SEND_LANE_MODE must be mixed, rpc_only/rpc-only, jito_only/jito-only, helius_sender_only/helius-sender-only, helius_tpu_jet/helius-tpu-jet, helius_tpu_quic/helius-tpu-quic, tpu_jet_helius_tip/tpu-jet-helius-tip, tpu_quic_helius_tip/tpu-quic-helius-tip, tpu_jet_only/tpu-jet-only, or tpu_quic_only/tpu-quic-only; got $JITO_SEND_LANE_MODE" >&2
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
echo "  tpu jet enabled: $JITO_TPU_JET_ENABLED"
if [[ -n "$JITO_TPU_JET_RPC_URL" ]]; then
  echo "  tpu jet rpc url: configured"
else
  echo "  tpu jet rpc url: unset"
fi
if [[ -n "$JITO_TPU_JET_WS_URL" ]]; then
  echo "  tpu jet ws/grpc url: configured"
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
echo "  priority fee micro lamports: ${JITO_PRIORITY_FEE_MICRO_LAMPORTS:-0}"
echo "  max priority fee micro lamports: $JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS"
echo "  dynamic priority fee enabled: $JITO_DYNAMIC_PRIORITY_FEE_ENABLED"
echo "  dynamic priority baseline/aggressive/panic/max micro lamports: ${JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS:-0}/${JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS:-0}/${JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS:-0}/${JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS:-0}"
echo "  jito tip lamports: ${JITO_TIP_LAMPORTS:-0}"
echo "  max jito tip lamports: $JITO_MAX_TIP_LAMPORTS"
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
exec "$WORKER_BIN" live "${LIVE_ARGS[@]}"
