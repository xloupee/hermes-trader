#!/usr/bin/env bash
set -euo pipefail

WORKER_DIR="${JITO_WORKER_DIR:-/opt/jito-feed-probe-watch}"
APP_ENV_FILE="${JITO_APP_ENV_FILE:-/opt/pumpfun-migration-bot/.env}"
WORKER_ENV_FILE="${JITO_WORKER_ENV_FILE:-/etc/jito-copy-live.env}"
LIVE_SERVICE="${JITO_LIVE_SERVICE:-jito-copy-live.service}"
SYNC_SERVICE="${JITO_SYNC_SERVICE:-jito-copy-sync.service}"
EXECUTIONS_PATH="${JITO_COPY_EXECUTIONS_PATH:-/var/log/jito-copy-executions-vps.jsonl}"
BACKUP_DIR="${JITO_CANARY_BACKUP_DIR:-$WORKER_DIR/backups}"
MARKER_FILE="${JITO_CANARY_MARKER_FILE:-/var/log/jito-copy-canary-current.env}"
DEFAULT_BASELINE_TIP="${JITO_CANARY_BASELINE_HELIUS_TIP_LAMPORTS:-387500}"
DEFAULT_BASELINE_PRIORITY="${JITO_CANARY_BASELINE_PRIORITY_FEE_MICRO_LAMPORTS:-968750}"
DEFAULT_BASELINE_RETRIES="${JITO_CANARY_BASELINE_SEND_MAX_RETRIES:-0}"
DEFAULT_COPY_WALLET_BALANCE_REFRESH_MS="${JITO_CANARY_COPY_WALLET_BALANCE_REFRESH_MS:-5000}"
DEFAULT_COPY_WALLET_BALANCE_STALE_MS="${JITO_CANARY_COPY_WALLET_BALANCE_STALE_MS:-120000}"
DEFAULT_BLOCKHASH_REFRESH_MS="${JITO_CANARY_BLOCKHASH_REFRESH_MS:-500}"
DEFAULT_BLOCKHASH_REFRESH_TIMEOUT_MS="${JITO_CANARY_BLOCKHASH_REFRESH_TIMEOUT_MS:-1200}"
DEFAULT_BLOCKHASH_STALE_MS="${JITO_CANARY_BLOCKHASH_STALE_MS:-30000}"

usage() {
  cat <<'USAGE'
Usage:
  landing-canary-control.sh list
  landing-canary-control.sh status
  landing-canary-control.sh mark <name> [since-iso]
  landing-canary-control.sh score [since-iso] [min-position-eligible]
  landing-canary-control.sh score-recent [last-sent] [min-position-eligible]
  landing-canary-control.sh compare [baseline-since-iso] [canary-since-iso] [canary-until-iso]
  landing-canary-control.sh ready
  landing-canary-control.sh apply <name>
  landing-canary-control.sh restore <backup-env-file>

Canaries:
  baseline       Helius Sender FRA SWQoS, tip 387500, priority 968750, retries 0
  tip-250k       Same as baseline, Helius Sender tip 250000
  tip-500k       Same as baseline, Helius Sender tip 500000
  priority-750k  Same as baseline, priority fee 750000
  priority-1250k Same as baseline, priority fee 1250000 and matching max cap
  priority-1453k Same as baseline, priority fee 1453125 and matching max cap (1.5x baseline)
  priority-1938k Same as baseline, priority fee 1937500 and matching max cap (2x baseline)
  dynamic-priority-2500k Helius only, retries 0, 200k tip, early/mid priority 2500000
  retries-0      Same as baseline, send max retries 0
  retries-1      Same as baseline, send max retries 1
  retries-3      Diagnostic rollback shape, send max retries 3
  tip-fixed      Baseline with plural Helius tip-account pool disabled
  tip-rotated    Baseline with JITO_CANARY_HELIUS_TIP_ACCOUNTS rotated per tx
  blockhash-processed Baseline with JITO_BLOCKHASH_COMMITMENT=processed
  blockhash-confirmed Baseline with JITO_BLOCKHASH_COMMITMENT=confirmed
  account-priority-cache Baseline plus warm writable-account getRecentPrioritizationFees cache
  helius-sender-max Helius Sender Max only, 1000000 lamport Sender tip
  helius-regional-fanout Helius Sender only, same fee shape, multiple regional Sender endpoints
  fast Helius Sender regional fanout plus Nozomi
  turbo Helius Sender regional fanout plus all non-Beam provider lanes
  nozomi-only   Nozomi JSON-RPC only with Nozomi tip, for delivery-lane isolation
  helius-nozomi-stack Helius Sender plus Nozomi same-signature fanout with both tips
  nozomi-api-v2-only Nozomi API v2 only with Nozomi tip
  helius-nozomi-api-v2-stack Helius Sender plus Nozomi API v2 fanout
  nozomi-api-v2-regional-only Nozomi API v2 multi-region fanout only
  helius-nozomi-api-v2-regional-stack Helius Sender plus Nozomi API v2 multi-region fanout
  astralane-only Astralane IrisB binary HTTP only with Astralane tip, for delivery-lane isolation
  helius-astralane-stack Helius Sender plus Astralane same-signature fanout
  helius-nozomi-astralane-stack Helius Sender plus Nozomi plus Astralane fanout
  helius-nozomi-astralane-lunar-stack Helius Sender plus Nozomi plus Astralane plus Lunar Lander fanout
  lunar-lander-only Lunar Lander binary HTTP only with Lunar tip, for delivery-lane isolation
  helius-lunar-lander-stack Helius Sender plus Lunar Lander same-signature fanout
  circular-fast-only Circular Fast JSON-RPC only with FAST tip, for delivery-lane isolation
  helius-circular-fast-stack Helius Sender plus Circular Fast same-signature fanout
  erpc-swqos-only ERPC SWQoS JSON-RPC only, for delivery-lane isolation
  helius-erpc-swqos-stack Helius Sender plus ERPC SWQoS same-signature fanout
  beam-only     RPC Fast Beam only with Beam provider tip
  helius-beam-stack Helius Sender plus Beam same-signature fanout
  helius-nozomi-beam-stack Helius Sender plus Nozomi plus Beam same-signature fanout
  zero-slot-only 0slot JSON-RPC only with 0slot tip, for delivery-lane isolation
  helius-zero-slot-stack Helius Sender plus 0slot same-signature fanout
  helius-nozomi-zero-slot-stack Helius Sender plus Nozomi plus 0slot fanout
  all-non-beam-stack Helius Sender plus Nozomi plus 0slot plus Jet if enabled
  tpu-jet-fanout Helius baseline plus same-signature Yellowstone Jet sidecar fanout
  tpu-jet-only Same fee shape, Yellowstone Jet sidecar only
  tpu-jet-cheap Jet sidecar only with Helius Sender tip disabled; requires JITO_CANARY_ALLOW_CHEAP_TPU=YES
  tpu-quic-current-leader-fanout Helius baseline plus direct TPU QUIC current-leader fanout
  tpu-quic-current-leader-only Same fee shape, direct TPU QUIC current-leader only
  tpu-quic-fanout Legacy multi-leader direct TPU QUIC fanout; not recommended for timeout retest
  tpu-quic-only Legacy same fee shape, multi-leader direct TPU QUIC only
  tpu-quic-cheap TPU QUIC only with Helius Sender tip disabled; requires JITO_CANARY_ALLOW_CHEAP_TPU=YES
USAGE
}

load_env_file() {
  local env_file="$1" line key value
  [[ -f "$env_file" ]] || return 0
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

set_env_var() {
  local file="$1" key="$2" value="$3" tmp
  tmp="$(mktemp)"
  awk -v key="$key" -v value="$value" '
    BEGIN { written = 0 }
    $0 ~ "^[[:space:]]*#" { print; next }
    index($0, key "=") == 1 {
      print key "=" value
      written = 1
      next
    }
    { print }
    END {
      if (!written) {
        print key "=" value
      }
    }
  ' "$file" > "$tmp"
  cat "$tmp" > "$file"
  rm -f "$tmp"
}

backup_env() {
  local stamp backup
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  mkdir -p "$BACKUP_DIR/canary-$stamp"
  backup="$BACKUP_DIR/canary-$stamp/$(basename "$WORKER_ENV_FILE")"
  cp -a "$WORKER_ENV_FILE" "$backup"
  printf '%s\n' "$backup"
}

trim_value() {
  local value="$1"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "$value"
}

csv_count() {
  local values="$1" count=0 part
  IFS=',' read -r -a parts <<< "$values"
  for part in "${parts[@]:-}"; do
    part="$(trim_value "$part")"
    [[ -n "$part" ]] && count=$((count + 1))
  done
  printf '%s' "$count"
}

nozomi_api_v2_urls_from_urls() {
  local values="$1" result="" part trimmed base query converted
  IFS=',' read -r -a parts <<< "$values"
  for part in "${parts[@]:-}"; do
    trimmed="$(trim_value "$part")"
    [[ -z "$trimmed" ]] && continue
    query=""
    if [[ "$trimmed" == *"?"* ]]; then
      query="${trimmed#*\?}"
      query="${query%%#*}"
    fi
    base="${trimmed%%\?*}"
    base="${base%%#*}"
    base="${base%/}"
    if [[ "$base" == */api/sendTransaction2 ]]; then
      converted="$trimmed"
    else
      converted="$base/api/sendTransaction2"
      [[ -n "$query" ]] && converted="$converted?$query"
    fi
    if [[ -z "$result" ]]; then
      result="$converted"
    else
      result="$result,$converted"
    fi
  done
  printf '%s' "$result"
}

nozomi_first_query() {
  local values="$1" part trimmed query
  if [[ -n "${JITO_CANARY_NOZOMI_API_KEY:-}" ]]; then
    printf 'c=%s' "$JITO_CANARY_NOZOMI_API_KEY"
    return 0
  fi
  IFS=',' read -r -a parts <<< "$values"
  for part in "${parts[@]:-}"; do
    trimmed="$(trim_value "$part")"
    [[ "$trimmed" == *"?"* ]] || continue
    query="${trimmed#*\?}"
    query="${query%%#*}"
    if [[ -n "$query" ]]; then
      printf '%s' "$query"
      return 0
    fi
  done
  return 0
}

nozomi_api_v2_region_urls() {
  local query hosts result="" host base converted
  query="$(nozomi_first_query "$CANARY_NOZOMI_URLS")"
  [[ -n "$query" ]] || return 0
  hosts="${JITO_CANARY_NOZOMI_API_V2_REGION_HOSTS:-ewr1.nozomi.temporal.xyz,ash1.nozomi.temporal.xyz,pit1.nozomi.temporal.xyz,edge.nozomi.temporal.xyz}"
  IFS=',' read -r -a parts <<< "$hosts"
  for host in "${parts[@]:-}"; do
    host="$(trim_value "$host")"
    [[ -z "$host" ]] && continue
    base="${host%/}"
    if [[ "$base" != *"://"* ]]; then
      base="https://$base"
    fi
    converted="$base/api/sendTransaction2?$query"
    if [[ -z "$result" ]]; then
      result="$converted"
    else
      result="$result,$converted"
    fi
  done
  printf '%s' "$result"
}

use_nozomi_api_v2_urls() {
  CANARY_NOZOMI_URLS="${JITO_CANARY_NOZOMI_API_V2_URLS:-$(nozomi_api_v2_urls_from_urls "$CANARY_NOZOMI_URLS")}"
}

use_nozomi_api_v2_region_urls() {
  CANARY_NOZOMI_URLS="${JITO_CANARY_NOZOMI_API_V2_REGION_URLS:-${JITO_CANARY_NOZOMI_API_V2_URLS:-$(nozomi_api_v2_region_urls)}}"
}

canary_values() {
  local name="$1"
  CANARY_HELIUS_TIP="$DEFAULT_BASELINE_TIP"
  CANARY_PRIORITY="$DEFAULT_BASELINE_PRIORITY"
  CANARY_MAX_PRIORITY="$DEFAULT_BASELINE_PRIORITY"
  CANARY_RETRIES="$DEFAULT_BASELINE_RETRIES"
  CANARY_LANE_MODE="helius-sender-only"
  CANARY_HELIUS_ENABLED="true"
  CANARY_HELIUS_SWQOS_ONLY="${JITO_HELIUS_SENDER_SWQOS_ONLY:-false}"
  CANARY_HELIUS_URLS="${JITO_HELIUS_SENDER_URLS:-}"
  CANARY_TPU_JET_ENABLED="false"
  CANARY_TPU_JET_FANOUT_SLOTS="${JITO_CANARY_TPU_JET_FANOUT_SLOTS:-${JITO_TPU_JET_FANOUT_SLOTS:-1}}"
  CANARY_TPU_JET_TIMEOUT_MS="${JITO_CANARY_TPU_JET_TIMEOUT_MS:-${JITO_TPU_JET_TIMEOUT_MS:-30}}"
  CANARY_TPU_QUIC_ENABLED="false"
  CANARY_TPU_QUIC_FANOUT_SLOTS="${JITO_CANARY_TPU_QUIC_FANOUT_SLOTS:-${JITO_TPU_QUIC_FANOUT_SLOTS:-12}}"
  CANARY_TPU_QUIC_TIMEOUT_MS="${JITO_CANARY_TPU_QUIC_TIMEOUT_MS:-${JITO_TPU_QUIC_TIMEOUT_MS:-30}}"
  CANARY_HELIUS_TIP_ACCOUNT="${JITO_HELIUS_SENDER_TIP_ACCOUNT:-}"
  CANARY_HELIUS_TIP_ACCOUNTS="${JITO_HELIUS_SENDER_TIP_ACCOUNTS:-}"
  CANARY_NOZOMI_ENABLED="false"
  CANARY_NOZOMI_URLS="${JITO_CANARY_NOZOMI_URLS:-${JITO_NOZOMI_URLS:-}}"
  CANARY_NOZOMI_TIP="${JITO_CANARY_NOZOMI_TIP_LAMPORTS:-${JITO_NOZOMI_TIP_LAMPORTS:-1000000}}"
  CANARY_NOZOMI_TIP_ACCOUNT="${JITO_CANARY_NOZOMI_TIP_ACCOUNT:-${JITO_NOZOMI_TIP_ACCOUNT:-TEMPaMeCRFAS9EKF53Jd6KpHxgL47uWLcpFArU1Fanq}}"
  CANARY_NOZOMI_TIP_ACCOUNTS="${JITO_CANARY_NOZOMI_TIP_ACCOUNTS:-${JITO_NOZOMI_TIP_ACCOUNTS:-}}"
  CANARY_ASTRALANE_ENABLED="false"
  CANARY_ASTRALANE_URLS="${JITO_CANARY_ASTRALANE_URLS:-${JITO_ASTRALANE_URLS:-https://lim.gateway.astralane.io/irisb}}"
  CANARY_ASTRALANE_API_KEY="${JITO_CANARY_ASTRALANE_API_KEY:-${JITO_ASTRALANE_API_KEY:-}}"
  CANARY_ASTRALANE_TIP="${JITO_CANARY_ASTRALANE_TIP_LAMPORTS:-${JITO_ASTRALANE_TIP_LAMPORTS:-1000000}}"
  CANARY_ASTRALANE_TIP_ACCOUNT="${JITO_CANARY_ASTRALANE_TIP_ACCOUNT:-${JITO_ASTRALANE_TIP_ACCOUNT:-astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm}}"
  CANARY_ASTRALANE_TIP_ACCOUNTS="${JITO_CANARY_ASTRALANE_TIP_ACCOUNTS:-${JITO_ASTRALANE_TIP_ACCOUNTS:-}}"
  CANARY_ASTRALANE_MEV_PROTECT="${JITO_CANARY_ASTRALANE_MEV_PROTECT:-${JITO_ASTRALANE_MEV_PROTECT:-false}}"
  CANARY_ASTRALANE_SWQOS_ONLY="${JITO_CANARY_ASTRALANE_SWQOS_ONLY:-${JITO_ASTRALANE_SWQOS_ONLY:-false}}"
  CANARY_LUNAR_LANDER_ENABLED="false"
  CANARY_LUNAR_LANDER_URLS="${JITO_CANARY_LUNAR_LANDER_URLS:-${JITO_LUNAR_LANDER_URLS:-http://fra.lunar-lander.hellomoon.io/send-bin}}"
  CANARY_LUNAR_LANDER_API_KEY="${JITO_CANARY_LUNAR_LANDER_API_KEY:-${JITO_LUNAR_LANDER_API_KEY:-}}"
  CANARY_LUNAR_LANDER_TIP="${JITO_CANARY_LUNAR_LANDER_TIP_LAMPORTS:-${JITO_LUNAR_LANDER_TIP_LAMPORTS:-1000000}}"
  CANARY_LUNAR_LANDER_TIP_ACCOUNT="${JITO_CANARY_LUNAR_LANDER_TIP_ACCOUNT:-${JITO_LUNAR_LANDER_TIP_ACCOUNT:-moon17L6BgxXRX5uHKudAmqVF96xia9h8ygcmG2sL3F}}"
  CANARY_LUNAR_LANDER_TIP_ACCOUNTS="${JITO_CANARY_LUNAR_LANDER_TIP_ACCOUNTS:-${JITO_LUNAR_LANDER_TIP_ACCOUNTS:-}}"
  CANARY_LUNAR_LANDER_MEV_PROTECT="${JITO_CANARY_LUNAR_LANDER_MEV_PROTECT:-${JITO_LUNAR_LANDER_MEV_PROTECT:-false}}"
  CANARY_CIRCULAR_FAST_ENABLED="false"
  CANARY_CIRCULAR_FAST_URLS="${JITO_CANARY_CIRCULAR_FAST_URLS:-${JITO_CIRCULAR_FAST_URLS:-https://fra.fast.circular.fi/transactions}}"
  CANARY_CIRCULAR_FAST_API_KEY="${JITO_CANARY_CIRCULAR_FAST_API_KEY:-${JITO_CIRCULAR_FAST_API_KEY:-}}"
  CANARY_CIRCULAR_FAST_TIP="${JITO_CANARY_CIRCULAR_FAST_TIP_LAMPORTS:-${JITO_CIRCULAR_FAST_TIP_LAMPORTS:-1000000}}"
  CANARY_CIRCULAR_FAST_TIP_ACCOUNT="${JITO_CANARY_CIRCULAR_FAST_TIP_ACCOUNT:-${JITO_CIRCULAR_FAST_TIP_ACCOUNT:-FAST3dMFZvESiEipBvLSiXq3QCV51o3xuoHScqRU6cB6}}"
  CANARY_CIRCULAR_FAST_TIP_ACCOUNTS="${JITO_CANARY_CIRCULAR_FAST_TIP_ACCOUNTS:-${JITO_CIRCULAR_FAST_TIP_ACCOUNTS:-}}"
  CANARY_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION="${JITO_CANARY_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION:-${JITO_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION:-false}}"
  CANARY_ERPC_SWQOS_ENABLED="false"
  CANARY_ERPC_SWQOS_URLS="${JITO_CANARY_ERPC_SWQOS_URLS:-${JITO_ERPC_SWQOS_URLS:-}}"
  CANARY_ERPC_LEADER_SLOTS_ENABLED="${JITO_CANARY_ERPC_LEADER_SLOTS_ENABLED:-${JITO_ERPC_LEADER_SLOTS_ENABLED:-false}}"
  CANARY_ERPC_LEADER_SLOTS_URL="${JITO_CANARY_ERPC_LEADER_SLOTS_URL:-${JITO_ERPC_LEADER_SLOTS_URL:-https://edge.erpc.global}}"
  CANARY_ERPC_API_KEY="${JITO_CANARY_ERPC_API_KEY:-${JITO_ERPC_API_KEY:-}}"
  CANARY_BEAM_ENABLED="false"
  CANARY_BEAM_URL="${JITO_CANARY_BEAM_URL:-${JITO_BEAM_URL:-https://beam.rpcfast.com}}"
  CANARY_BEAM_TOKEN="${JITO_CANARY_BEAM_TOKEN:-${JITO_BEAM_TOKEN:-}}"
  CANARY_BEAM_PROVIDER="${JITO_CANARY_BEAM_PROVIDER:-${JITO_BEAM_PROVIDER:-bloxroute}}"
  CANARY_BEAM_MODE="${JITO_CANARY_BEAM_MODE:-${JITO_BEAM_MODE:-fastest}}"
  CANARY_BEAM_TIP="${JITO_CANARY_BEAM_TIP_LAMPORTS:-${JITO_BEAM_TIP_LAMPORTS:-1000000}}"
  CANARY_BEAM_TIP_ACCOUNTS="${JITO_CANARY_BEAM_TIP_ACCOUNTS:-${JITO_BEAM_TIP_ACCOUNTS:-}}"
  CANARY_ZERO_SLOT_ENABLED="false"
  CANARY_ZERO_SLOT_URLS="${JITO_CANARY_ZERO_SLOT_URLS:-${JITO_ZERO_SLOT_URLS:-}}"
  CANARY_ZERO_SLOT_API_KEY="${JITO_CANARY_ZERO_SLOT_API_KEY:-${JITO_ZERO_SLOT_API_KEY:-}}"
  CANARY_ZERO_SLOT_TIP="${JITO_CANARY_ZERO_SLOT_TIP_LAMPORTS:-${JITO_ZERO_SLOT_TIP_LAMPORTS:-1000000}}"
  CANARY_ZERO_SLOT_TIP_ACCOUNTS="${JITO_CANARY_ZERO_SLOT_TIP_ACCOUNTS:-${JITO_ZERO_SLOT_TIP_ACCOUNTS:-}}"
  CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_MAX_PROVIDER_TIP_LAMPORTS:-${JITO_MAX_PROVIDER_TIP_LAMPORTS:-1387500}}"
  CANARY_MAX_SIGNED_TX_BYTES="${JITO_CANARY_MAX_SIGNED_TX_BYTES:-${JITO_MAX_SIGNED_TX_BYTES:-1232}}"
  CANARY_MAX_INSTRUCTION_COUNT="${JITO_CANARY_MAX_INSTRUCTION_COUNT:-${JITO_MAX_INSTRUCTION_COUNT:-8}}"
  CANARY_MAX_WRITABLE_ACCOUNT_COUNT="${JITO_CANARY_MAX_WRITABLE_ACCOUNT_COUNT:-${JITO_MAX_WRITABLE_ACCOUNT_COUNT:-16}}"
  CANARY_BLOCKHASH_COMMITMENT="${JITO_BLOCKHASH_COMMITMENT:-processed}"
  CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
  CANARY_ACCOUNT_PRIORITY_FEE_REFRESH_MS="${JITO_ACCOUNT_PRIORITY_FEE_REFRESH_MS:-1000}"
  CANARY_ACCOUNT_PRIORITY_FEE_STALE_MS="${JITO_ACCOUNT_PRIORITY_FEE_STALE_MS:-5000}"
  CANARY_ACCOUNT_PRIORITY_FEE_PERCENTILE="${JITO_ACCOUNT_PRIORITY_FEE_PERCENTILE:-75}"
  CANARY_DYNAMIC_PRIORITY_ENABLED="false"
  CANARY_DYNAMIC_PRIORITY_BASELINE="0"
  CANARY_DYNAMIC_PRIORITY_AGGRESSIVE="0"
  CANARY_DYNAMIC_PRIORITY_PANIC="0"
  CANARY_DYNAMIC_PRIORITY_MAX="0"

  case "$name" in
    baseline) ;;
    tip-250k) CANARY_HELIUS_TIP=250000 ;;
    tip-500k) CANARY_HELIUS_TIP=500000 ;;
    priority-750k)
      CANARY_PRIORITY=750000
      CANARY_MAX_PRIORITY="$DEFAULT_BASELINE_PRIORITY"
      ;;
    priority-1250k)
      CANARY_PRIORITY=1250000
      CANARY_MAX_PRIORITY=1250000
      ;;
    priority-1453k|priority-1.5x)
      CANARY_PRIORITY=1453125
      CANARY_MAX_PRIORITY=1453125
      ;;
    priority-1938k|priority-2x)
      CANARY_PRIORITY=1937500
      CANARY_MAX_PRIORITY=1937500
      ;;
    dynamic-priority-2500k)
      CANARY_HELIUS_TIP=200000
      CANARY_PRIORITY=1250000
      CANARY_MAX_PRIORITY=2500000
      CANARY_RETRIES=0
      CANARY_DYNAMIC_PRIORITY_ENABLED="true"
      CANARY_DYNAMIC_PRIORITY_BASELINE=1250000
      CANARY_DYNAMIC_PRIORITY_AGGRESSIVE=2500000
      CANARY_DYNAMIC_PRIORITY_PANIC=0
      CANARY_DYNAMIC_PRIORITY_MAX=2500000
      ;;
    retries-0) CANARY_RETRIES=0 ;;
    retries-1) CANARY_RETRIES=1 ;;
    retries-3) CANARY_RETRIES=3 ;;
    tip-fixed)
      CANARY_HELIUS_TIP_ACCOUNTS=""
      ;;
    tip-rotated)
      CANARY_HELIUS_TIP_ACCOUNTS="${JITO_CANARY_HELIUS_TIP_ACCOUNTS:-}"
      if [[ -z "$CANARY_HELIUS_TIP_ACCOUNTS" ]]; then
        echo "tip-rotated requires JITO_CANARY_HELIUS_TIP_ACCOUNTS" >&2
        exit 2
      fi
      ;;
    blockhash-processed) CANARY_BLOCKHASH_COMMITMENT="processed" ;;
    blockhash-confirmed) CANARY_BLOCKHASH_COMMITMENT="confirmed" ;;
    account-priority-cache)
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="true"
      CANARY_ACCOUNT_PRIORITY_FEE_REFRESH_MS="${JITO_CANARY_ACCOUNT_PRIORITY_FEE_REFRESH_MS:-1000}"
      CANARY_ACCOUNT_PRIORITY_FEE_STALE_MS="${JITO_CANARY_ACCOUNT_PRIORITY_FEE_STALE_MS:-5000}"
      CANARY_ACCOUNT_PRIORITY_FEE_PERCENTILE="${JITO_CANARY_ACCOUNT_PRIORITY_FEE_PERCENTILE:-75}"
      ;;
    helius-sender-max)
      CANARY_LANE_MODE="helius-sender-max"
      CANARY_HELIUS_ENABLED="true"
      CANARY_HELIUS_SWQOS_ONLY="false"
      CANARY_HELIUS_TIP=1000000
      CANARY_NOZOMI_ENABLED="false"
      CANARY_ASTRALANE_ENABLED="false"
      CANARY_LUNAR_LANDER_ENABLED="false"
      CANARY_CIRCULAR_FAST_ENABLED="false"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="false"
      CANARY_TPU_JET_ENABLED="false"
      CANARY_TPU_QUIC_ENABLED="false"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_HELIUS_SENDER_MAX_PROVIDER_TIP_LAMPORTS:-1000000}"
      ;;
    helius-regional-fanout)
      CANARY_LANE_MODE="helius-sender-only"
      CANARY_HELIUS_ENABLED="true"
      CANARY_HELIUS_URLS="${JITO_CANARY_HELIUS_REGION_URLS:-}"
      CANARY_NOZOMI_ENABLED="false"
      CANARY_ASTRALANE_ENABLED="false"
      CANARY_LUNAR_LANDER_ENABLED="false"
      CANARY_BEAM_ENABLED="false"
      if [[ -z "$CANARY_HELIUS_URLS" ]]; then
        echo "helius-regional-fanout requires JITO_CANARY_HELIUS_REGION_URLS" >&2
        exit 2
      fi
      ;;
    fast)
      CANARY_LANE_MODE="fast"
      CANARY_HELIUS_ENABLED="true"
      CANARY_HELIUS_URLS="${JITO_CANARY_HELIUS_REGION_URLS:-}"
      CANARY_NOZOMI_ENABLED="true"
      CANARY_ASTRALANE_ENABLED="false"
      CANARY_LUNAR_LANDER_ENABLED="false"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="false"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_FAST_MAX_PROVIDER_TIP_LAMPORTS:-1387500}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      if [[ -z "$CANARY_HELIUS_URLS" ]]; then
        echo "fast requires JITO_CANARY_HELIUS_REGION_URLS" >&2
        exit 2
      fi
      ;;
    nozomi-only)
      CANARY_LANE_MODE="nozomi-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      CANARY_HELIUS_TIP_ACCOUNTS=""
      CANARY_NOZOMI_ENABLED="true"
      ;;
    nozomi-api-v2-only)
      CANARY_LANE_MODE="nozomi-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      CANARY_HELIUS_TIP_ACCOUNTS=""
      CANARY_NOZOMI_ENABLED="true"
      use_nozomi_api_v2_urls
      ;;
    nozomi-api-v2-regional-only)
      CANARY_LANE_MODE="nozomi-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      CANARY_HELIUS_TIP_ACCOUNTS=""
      CANARY_NOZOMI_ENABLED="true"
      use_nozomi_api_v2_region_urls
      ;;
    helius-nozomi-stack)
      CANARY_LANE_MODE="helius-nozomi-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="true"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    helius-nozomi-api-v2-stack)
      CANARY_LANE_MODE="helius-nozomi-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="true"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      use_nozomi_api_v2_urls
      ;;
    helius-nozomi-api-v2-regional-stack)
      CANARY_LANE_MODE="helius-nozomi-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="true"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      use_nozomi_api_v2_region_urls
      ;;
    astralane-only)
      CANARY_LANE_MODE="astralane-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      CANARY_HELIUS_TIP_ACCOUNTS=""
      CANARY_NOZOMI_ENABLED="false"
      CANARY_ASTRALANE_ENABLED="true"
      CANARY_LUNAR_LANDER_ENABLED="false"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="false"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_ASTRALANE_MAX_PROVIDER_TIP_LAMPORTS:-1000000}"
      ;;
    helius-astralane-stack)
      CANARY_LANE_MODE="helius-astralane-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="false"
      CANARY_ASTRALANE_ENABLED="true"
      CANARY_LUNAR_LANDER_ENABLED="false"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="false"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_ASTRALANE_STACK_MAX_PROVIDER_TIP_LAMPORTS:-1387500}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    helius-nozomi-astralane-stack)
      CANARY_LANE_MODE="helius-nozomi-astralane-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="true"
      CANARY_ASTRALANE_ENABLED="true"
      CANARY_LUNAR_LANDER_ENABLED="false"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="false"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_ASTRALANE_NOZOMI_STACK_MAX_PROVIDER_TIP_LAMPORTS:-2387500}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    helius-nozomi-astralane-lunar-stack)
      CANARY_LANE_MODE="helius-nozomi-astralane-lunar-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="true"
      CANARY_ASTRALANE_ENABLED="true"
      CANARY_LUNAR_LANDER_ENABLED="true"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="false"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_ASTRALANE_NOZOMI_LUNAR_STACK_MAX_PROVIDER_TIP_LAMPORTS:-3387500}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    lunar-lander-only)
      CANARY_LANE_MODE="lunar-lander-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      CANARY_HELIUS_TIP_ACCOUNTS=""
      CANARY_NOZOMI_ENABLED="false"
      CANARY_ASTRALANE_ENABLED="false"
      CANARY_LUNAR_LANDER_ENABLED="true"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="false"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_LUNAR_LANDER_MAX_PROVIDER_TIP_LAMPORTS:-1000000}"
      ;;
    helius-lunar-lander-stack)
      CANARY_LANE_MODE="helius-lunar-lander-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="false"
      CANARY_ASTRALANE_ENABLED="false"
      CANARY_LUNAR_LANDER_ENABLED="true"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="false"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_LUNAR_LANDER_STACK_MAX_PROVIDER_TIP_LAMPORTS:-1387500}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    circular-fast-only)
      CANARY_LANE_MODE="circular-fast-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      CANARY_HELIUS_TIP_ACCOUNTS=""
      CANARY_NOZOMI_ENABLED="false"
      CANARY_ASTRALANE_ENABLED="false"
      CANARY_LUNAR_LANDER_ENABLED="false"
      CANARY_CIRCULAR_FAST_ENABLED="true"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="false"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_CIRCULAR_FAST_MAX_PROVIDER_TIP_LAMPORTS:-1000000}"
      ;;
    helius-circular-fast-stack)
      CANARY_LANE_MODE="helius-circular-fast-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="false"
      CANARY_ASTRALANE_ENABLED="false"
      CANARY_LUNAR_LANDER_ENABLED="false"
      CANARY_CIRCULAR_FAST_ENABLED="true"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="false"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_CIRCULAR_FAST_STACK_MAX_PROVIDER_TIP_LAMPORTS:-1387500}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    erpc-swqos-only)
      CANARY_LANE_MODE="erpc-swqos-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      CANARY_HELIUS_TIP_ACCOUNTS=""
      CANARY_NOZOMI_ENABLED="false"
      CANARY_ERPC_SWQOS_ENABLED="true"
      ;;
    helius-erpc-swqos-stack)
      CANARY_LANE_MODE="helius-erpc-swqos-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="false"
      CANARY_ERPC_SWQOS_ENABLED="true"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    beam-only)
      CANARY_LANE_MODE="beam-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      CANARY_HELIUS_TIP_ACCOUNTS=""
      CANARY_NOZOMI_ENABLED="false"
      CANARY_BEAM_ENABLED="true"
      ;;
    helius-beam-stack)
      CANARY_LANE_MODE="helius-beam-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="false"
      CANARY_BEAM_ENABLED="true"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    helius-nozomi-beam-stack)
      CANARY_LANE_MODE="helius-nozomi-beam-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="true"
      CANARY_BEAM_ENABLED="true"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_BEAM_STACK_MAX_PROVIDER_TIP_LAMPORTS:-2500000}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    zero-slot-only)
      CANARY_LANE_MODE="zero-slot-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      CANARY_HELIUS_TIP_ACCOUNTS=""
      CANARY_NOZOMI_ENABLED="false"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="true"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_ZERO_SLOT_MAX_PROVIDER_TIP_LAMPORTS:-1000000}"
      ;;
    helius-zero-slot-stack)
      CANARY_LANE_MODE="helius-zero-slot-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="false"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="true"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_ZERO_SLOT_STACK_MAX_PROVIDER_TIP_LAMPORTS:-1200000}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    helius-nozomi-zero-slot-stack)
      CANARY_LANE_MODE="helius-nozomi-zero-slot-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="true"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="true"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_ZERO_SLOT_NOZOMI_STACK_MAX_PROVIDER_TIP_LAMPORTS:-2200000}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    all-non-beam-stack)
      CANARY_LANE_MODE="all-non-beam-stack"
      CANARY_HELIUS_ENABLED="true"
      CANARY_NOZOMI_ENABLED="true"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="true"
      CANARY_TPU_JET_ENABLED="${JITO_CANARY_ALL_NON_BEAM_TPU_JET_ENABLED:-${JITO_TPU_JET_ENABLED:-false}}"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_ALL_NON_BEAM_MAX_PROVIDER_TIP_LAMPORTS:-2387500}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      ;;
    turbo)
      CANARY_LANE_MODE="turbo"
      CANARY_HELIUS_ENABLED="true"
      CANARY_HELIUS_URLS="${JITO_CANARY_HELIUS_REGION_URLS:-}"
      CANARY_NOZOMI_ENABLED="true"
      CANARY_ASTRALANE_ENABLED="true"
      CANARY_LUNAR_LANDER_ENABLED="true"
      CANARY_BEAM_ENABLED="false"
      CANARY_ZERO_SLOT_ENABLED="true"
      CANARY_TPU_JET_ENABLED="true"
      CANARY_TPU_JET_FANOUT_SLOTS="${JITO_CANARY_TPU_JET_FANOUT_SLOTS:-1}"
      CANARY_TPU_JET_TIMEOUT_MS="${JITO_CANARY_TPU_JET_TIMEOUT_MS:-30}"
      CANARY_MAX_PROVIDER_TIP_LAMPORTS="${JITO_CANARY_TURBO_MAX_PROVIDER_TIP_LAMPORTS:-4387500}"
      CANARY_ACCOUNT_PRIORITY_FEE_ENABLED="${JITO_CANARY_STACK_ACCOUNT_PRIORITY_FEE_ENABLED:-false}"
      if [[ -z "$CANARY_HELIUS_URLS" ]]; then
        echo "turbo requires JITO_CANARY_HELIUS_REGION_URLS" >&2
        exit 2
      fi
      ;;
    tpu-jet-fanout)
      CANARY_LANE_MODE="helius-tpu-jet"
      CANARY_TPU_JET_ENABLED="true"
      CANARY_TPU_JET_FANOUT_SLOTS="${JITO_CANARY_TPU_JET_FANOUT_SLOTS:-1}"
      CANARY_TPU_JET_TIMEOUT_MS="${JITO_CANARY_TPU_JET_TIMEOUT_MS:-30}"
      ;;
    tpu-jet-only)
      CANARY_LANE_MODE="tpu-jet-helius-tip"
      CANARY_TPU_JET_ENABLED="true"
      CANARY_TPU_JET_FANOUT_SLOTS="${JITO_CANARY_TPU_JET_FANOUT_SLOTS:-1}"
      CANARY_TPU_JET_TIMEOUT_MS="${JITO_CANARY_TPU_JET_TIMEOUT_MS:-30}"
      ;;
    tpu-jet-cheap)
      case "$(printf '%s' "${JITO_CANARY_ALLOW_CHEAP_TPU:-false}" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *)
          echo "tpu-jet-cheap requires JITO_CANARY_ALLOW_CHEAP_TPU=YES after tpu-jet-only proves better" >&2
          exit 2
          ;;
      esac
      CANARY_LANE_MODE="tpu-jet-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_TPU_JET_ENABLED="true"
      CANARY_TPU_JET_FANOUT_SLOTS="${JITO_CANARY_TPU_JET_FANOUT_SLOTS:-1}"
      CANARY_TPU_JET_TIMEOUT_MS="${JITO_CANARY_TPU_JET_TIMEOUT_MS:-30}"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      ;;
    tpu-quic-fanout)
      CANARY_LANE_MODE="helius-tpu-quic"
      CANARY_TPU_QUIC_ENABLED="true"
      ;;
    tpu-quic-only)
      CANARY_LANE_MODE="tpu-quic-helius-tip"
      CANARY_TPU_QUIC_ENABLED="true"
      ;;
    tpu-quic-current-leader-fanout)
      CANARY_LANE_MODE="helius-tpu-quic"
      CANARY_TPU_QUIC_ENABLED="true"
      CANARY_TPU_QUIC_FANOUT_SLOTS=1
      ;;
    tpu-quic-current-leader-only)
      CANARY_LANE_MODE="tpu-quic-helius-tip"
      CANARY_TPU_QUIC_ENABLED="true"
      CANARY_TPU_QUIC_FANOUT_SLOTS=1
      ;;
    tpu-quic-cheap)
      case "$(printf '%s' "${JITO_CANARY_ALLOW_CHEAP_TPU:-false}" | tr '[:upper:]' '[:lower:]')" in
        yes|true|1|on) ;;
        *)
          echo "tpu-quic-cheap requires JITO_CANARY_ALLOW_CHEAP_TPU=YES after tpu-quic-only proves better" >&2
          exit 2
          ;;
      esac
      CANARY_LANE_MODE="tpu-quic-only"
      CANARY_HELIUS_ENABLED="false"
      CANARY_TPU_QUIC_ENABLED="true"
      CANARY_HELIUS_TIP=0
      CANARY_HELIUS_TIP_ACCOUNT=""
      ;;
    *)
      echo "unknown canary: $name" >&2
      usage >&2
      exit 2
      ;;
  esac
}

write_marker() {
  local name="$1" since_iso="$2" backup="${3:-}"
  local baseline_since_iso="${CANARY_BASELINE_STARTED_AT_ISO:-}"
  if [[ -z "$baseline_since_iso" && "$name" == "baseline" ]]; then
    baseline_since_iso="$since_iso"
  fi
  cat > "$MARKER_FILE" <<EOF_MARKER
CANARY_NAME=$name
CANARY_STARTED_AT_ISO=$since_iso
CANARY_BASELINE_STARTED_AT_ISO=$baseline_since_iso
CANARY_BACKUP_ENV=$backup
CANARY_HELIUS_TIP_LAMPORTS=${CANARY_HELIUS_TIP:-}
CANARY_PRIORITY_FEE_MICRO_LAMPORTS=${CANARY_PRIORITY:-}
CANARY_SEND_MAX_RETRIES=${CANARY_RETRIES:-}
CANARY_SEND_LANE_MODE=${CANARY_LANE_MODE:-}
CANARY_HELIUS_URL_COUNT=$(if [[ -n "${CANARY_HELIUS_URLS:-}" ]]; then printf '%s' "$CANARY_HELIUS_URLS" | awk -F, '{print NF}'; else printf '0'; fi)
CANARY_HELIUS_REGION_URLS_CONFIGURED=$([[ -n "${CANARY_HELIUS_URLS:-}" ]] && echo true || echo false)
CANARY_HELIUS_SWQOS_ONLY=${CANARY_HELIUS_SWQOS_ONLY:-}
CANARY_HELIUS_TIP_ACCOUNTS=${CANARY_HELIUS_TIP_ACCOUNTS:-}
CANARY_NOZOMI_ENABLED=${CANARY_NOZOMI_ENABLED:-}
CANARY_NOZOMI_URLS_CONFIGURED=$([[ -n "${CANARY_NOZOMI_URLS:-}" ]] && echo true || echo false)
CANARY_NOZOMI_URL_COUNT=$(csv_count "${CANARY_NOZOMI_URLS:-}")
CANARY_NOZOMI_API_V2=$([[ "${CANARY_NOZOMI_URLS:-}" == *"/api/sendTransaction2"* ]] && echo true || echo false)
CANARY_NOZOMI_TIP_LAMPORTS=${CANARY_NOZOMI_TIP:-}
CANARY_NOZOMI_TIP_ACCOUNT_CONFIGURED=$([[ -n "${CANARY_NOZOMI_TIP_ACCOUNT:-}" ]] && echo true || echo false)
CANARY_NOZOMI_TIP_ACCOUNTS_CONFIGURED=$([[ -n "${CANARY_NOZOMI_TIP_ACCOUNTS:-}" ]] && echo true || echo false)
CANARY_ASTRALANE_ENABLED=${CANARY_ASTRALANE_ENABLED:-}
CANARY_ASTRALANE_URLS_CONFIGURED=$([[ -n "${CANARY_ASTRALANE_URLS:-}" ]] && echo true || echo false)
CANARY_ASTRALANE_API_KEY_CONFIGURED=$([[ -n "${CANARY_ASTRALANE_API_KEY:-}" ]] && echo true || echo false)
CANARY_ASTRALANE_TIP_LAMPORTS=${CANARY_ASTRALANE_TIP:-}
CANARY_ASTRALANE_TIP_ACCOUNT_CONFIGURED=$([[ -n "${CANARY_ASTRALANE_TIP_ACCOUNT:-}" ]] && echo true || echo false)
CANARY_ASTRALANE_TIP_ACCOUNTS_CONFIGURED=$([[ -n "${CANARY_ASTRALANE_TIP_ACCOUNTS:-}" ]] && echo true || echo false)
CANARY_ASTRALANE_MEV_PROTECT=${CANARY_ASTRALANE_MEV_PROTECT:-}
CANARY_ASTRALANE_SWQOS_ONLY=${CANARY_ASTRALANE_SWQOS_ONLY:-}
CANARY_LUNAR_LANDER_ENABLED=${CANARY_LUNAR_LANDER_ENABLED:-}
CANARY_LUNAR_LANDER_URLS_CONFIGURED=$([[ -n "${CANARY_LUNAR_LANDER_URLS:-}" ]] && echo true || echo false)
CANARY_LUNAR_LANDER_API_KEY_CONFIGURED=$([[ -n "${CANARY_LUNAR_LANDER_API_KEY:-}" ]] && echo true || echo false)
CANARY_LUNAR_LANDER_TIP_LAMPORTS=${CANARY_LUNAR_LANDER_TIP:-}
CANARY_LUNAR_LANDER_TIP_ACCOUNT_CONFIGURED=$([[ -n "${CANARY_LUNAR_LANDER_TIP_ACCOUNT:-}" ]] && echo true || echo false)
CANARY_LUNAR_LANDER_TIP_ACCOUNTS_CONFIGURED=$([[ -n "${CANARY_LUNAR_LANDER_TIP_ACCOUNTS:-}" ]] && echo true || echo false)
CANARY_LUNAR_LANDER_MEV_PROTECT=${CANARY_LUNAR_LANDER_MEV_PROTECT:-}
CANARY_CIRCULAR_FAST_ENABLED=${CANARY_CIRCULAR_FAST_ENABLED:-}
CANARY_CIRCULAR_FAST_URLS_CONFIGURED=$([[ -n "${CANARY_CIRCULAR_FAST_URLS:-}" ]] && echo true || echo false)
CANARY_CIRCULAR_FAST_API_KEY_CONFIGURED=$([[ -n "${CANARY_CIRCULAR_FAST_API_KEY:-}" ]] && echo true || echo false)
CANARY_CIRCULAR_FAST_TIP_LAMPORTS=${CANARY_CIRCULAR_FAST_TIP:-}
CANARY_CIRCULAR_FAST_TIP_ACCOUNT_CONFIGURED=$([[ -n "${CANARY_CIRCULAR_FAST_TIP_ACCOUNT:-}" ]] && echo true || echo false)
CANARY_CIRCULAR_FAST_TIP_ACCOUNTS_CONFIGURED=$([[ -n "${CANARY_CIRCULAR_FAST_TIP_ACCOUNTS:-}" ]] && echo true || echo false)
CANARY_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION=${CANARY_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION:-}
CANARY_ERPC_SWQOS_ENABLED=${CANARY_ERPC_SWQOS_ENABLED:-}
CANARY_ERPC_SWQOS_URLS_CONFIGURED=$([[ -n "${CANARY_ERPC_SWQOS_URLS:-}" ]] && echo true || echo false)
CANARY_ERPC_LEADER_SLOTS_ENABLED=${CANARY_ERPC_LEADER_SLOTS_ENABLED:-}
CANARY_ERPC_LEADER_SLOTS_URL_CONFIGURED=$([[ -n "${CANARY_ERPC_LEADER_SLOTS_URL:-}" ]] && echo true || echo false)
CANARY_ERPC_API_KEY_CONFIGURED=$([[ -n "${CANARY_ERPC_API_KEY:-}" ]] && echo true || echo false)
CANARY_BEAM_ENABLED=${CANARY_BEAM_ENABLED:-}
CANARY_BEAM_URL_CONFIGURED=$([[ -n "${CANARY_BEAM_URL:-}" ]] && echo true || echo false)
CANARY_BEAM_TOKEN_CONFIGURED=$([[ -n "${CANARY_BEAM_TOKEN:-}" ]] && echo true || echo false)
CANARY_BEAM_PROVIDER=${CANARY_BEAM_PROVIDER:-}
CANARY_BEAM_MODE=${CANARY_BEAM_MODE:-}
CANARY_BEAM_TIP_LAMPORTS=${CANARY_BEAM_TIP:-}
CANARY_BEAM_TIP_ACCOUNTS_CONFIGURED=$([[ -n "${CANARY_BEAM_TIP_ACCOUNTS:-}" ]] && echo true || echo false)
CANARY_ZERO_SLOT_ENABLED=${CANARY_ZERO_SLOT_ENABLED:-}
CANARY_ZERO_SLOT_URLS_CONFIGURED=$([[ -n "${CANARY_ZERO_SLOT_URLS:-}" ]] && echo true || echo false)
CANARY_ZERO_SLOT_API_KEY_CONFIGURED=$([[ -n "${CANARY_ZERO_SLOT_API_KEY:-}" ]] && echo true || echo false)
CANARY_ZERO_SLOT_TIP_LAMPORTS=${CANARY_ZERO_SLOT_TIP:-}
CANARY_ZERO_SLOT_TIP_ACCOUNTS_CONFIGURED=$([[ -n "${CANARY_ZERO_SLOT_TIP_ACCOUNTS:-}" ]] && echo true || echo false)
CANARY_MAX_PROVIDER_TIP_LAMPORTS=${CANARY_MAX_PROVIDER_TIP_LAMPORTS:-}
CANARY_MAX_SIGNED_TX_BYTES=${CANARY_MAX_SIGNED_TX_BYTES:-}
CANARY_MAX_INSTRUCTION_COUNT=${CANARY_MAX_INSTRUCTION_COUNT:-}
CANARY_MAX_WRITABLE_ACCOUNT_COUNT=${CANARY_MAX_WRITABLE_ACCOUNT_COUNT:-}
CANARY_BLOCKHASH_COMMITMENT=${CANARY_BLOCKHASH_COMMITMENT:-}
CANARY_ACCOUNT_PRIORITY_FEE_ENABLED=${CANARY_ACCOUNT_PRIORITY_FEE_ENABLED:-}
CANARY_ACCOUNT_PRIORITY_FEE_REFRESH_MS=${CANARY_ACCOUNT_PRIORITY_FEE_REFRESH_MS:-}
CANARY_ACCOUNT_PRIORITY_FEE_STALE_MS=${CANARY_ACCOUNT_PRIORITY_FEE_STALE_MS:-}
CANARY_ACCOUNT_PRIORITY_FEE_PERCENTILE=${CANARY_ACCOUNT_PRIORITY_FEE_PERCENTILE:-}
CANARY_TPU_JET_ENABLED=${CANARY_TPU_JET_ENABLED:-}
CANARY_TPU_JET_FANOUT_SLOTS=${CANARY_TPU_JET_FANOUT_SLOTS:-}
CANARY_TPU_JET_TIMEOUT_MS=${CANARY_TPU_JET_TIMEOUT_MS:-}
CANARY_TPU_QUIC_ENABLED=${CANARY_TPU_QUIC_ENABLED:-}
CANARY_TPU_QUIC_FANOUT_SLOTS=${CANARY_TPU_QUIC_FANOUT_SLOTS:-}
CANARY_TPU_QUIC_TIMEOUT_MS=${CANARY_TPU_QUIC_TIMEOUT_MS:-}
CANARY_DYNAMIC_PRIORITY_FEE_ENABLED=${CANARY_DYNAMIC_PRIORITY_ENABLED:-}
CANARY_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS=${CANARY_DYNAMIC_PRIORITY_BASELINE:-}
CANARY_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS=${CANARY_DYNAMIC_PRIORITY_AGGRESSIVE:-}
CANARY_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS=${CANARY_DYNAMIC_PRIORITY_PANIC:-}
CANARY_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS=${CANARY_DYNAMIC_PRIORITY_MAX:-}
EOF_MARKER
}

current_service_start_iso() {
  systemctl show -p ActiveEnterTimestamp --value "$LIVE_SERVICE" | xargs -I{} date -u -d "{}" +%Y-%m-%dT%H:%M:%SZ
}

print_status() {
  load_env_file "$APP_ENV_FILE"
  load_env_file "$WORKER_ENV_FILE"
  echo "live_service=$(systemctl is-active "$LIVE_SERVICE" || true)"
  echo "sync_service=$(systemctl is-active "$SYNC_SERVICE" || true)"
  echo "live_started_at=$(current_service_start_iso || true)"
  echo "send_lane_mode=${JITO_SEND_LANE_MODE:-}"
  echo "helius_sender_enabled=${JITO_HELIUS_SENDER_ENABLED:-}"
  echo "helius_sender_url_count=$(if [[ -n "${JITO_HELIUS_SENDER_URLS:-}" ]]; then printf '%s' "$JITO_HELIUS_SENDER_URLS" | awk -F, '{print NF}'; else printf '0'; fi)"
  echo "helius_sender_swqos_only=${JITO_HELIUS_SENDER_SWQOS_ONLY:-}"
  echo "helius_sender_tip_lamports=${JITO_HELIUS_SENDER_TIP_LAMPORTS:-}"
  echo "nozomi_enabled=${JITO_NOZOMI_ENABLED:-}"
  echo "nozomi_urls=$([[ -n "${JITO_NOZOMI_URLS:-}" ]] && echo configured || echo empty)"
  echo "nozomi_tip_lamports=${JITO_NOZOMI_TIP_LAMPORTS:-}"
  echo "nozomi_tip_account=$([[ -n "${JITO_NOZOMI_TIP_ACCOUNT:-}" ]] && echo configured || echo empty)"
  echo "nozomi_tip_accounts=$([[ -n "${JITO_NOZOMI_TIP_ACCOUNTS:-}" ]] && echo configured || echo empty)"
  echo "astralane_enabled=${JITO_ASTRALANE_ENABLED:-}"
  echo "astralane_urls=$([[ -n "${JITO_ASTRALANE_URLS:-}" ]] && echo configured || echo empty)"
  echo "astralane_api_key=$([[ -n "${JITO_ASTRALANE_API_KEY:-}" ]] && echo configured || echo empty)"
  echo "astralane_tip_lamports=${JITO_ASTRALANE_TIP_LAMPORTS:-}"
  echo "astralane_tip_account=$([[ -n "${JITO_ASTRALANE_TIP_ACCOUNT:-}" ]] && echo configured || echo empty)"
  echo "astralane_tip_accounts=$([[ -n "${JITO_ASTRALANE_TIP_ACCOUNTS:-}" ]] && echo configured || echo empty)"
  echo "astralane_mev_protect=${JITO_ASTRALANE_MEV_PROTECT:-}"
  echo "astralane_swqos_only=${JITO_ASTRALANE_SWQOS_ONLY:-}"
  echo "lunar_lander_enabled=${JITO_LUNAR_LANDER_ENABLED:-}"
  echo "lunar_lander_urls=$([[ -n "${JITO_LUNAR_LANDER_URLS:-}" ]] && echo configured || echo empty)"
  echo "lunar_lander_api_key=$([[ -n "${JITO_LUNAR_LANDER_API_KEY:-}" ]] && echo configured || echo empty)"
  echo "lunar_lander_tip_lamports=${JITO_LUNAR_LANDER_TIP_LAMPORTS:-}"
  echo "lunar_lander_tip_account=$([[ -n "${JITO_LUNAR_LANDER_TIP_ACCOUNT:-}" ]] && echo configured || echo empty)"
  echo "lunar_lander_tip_accounts=$([[ -n "${JITO_LUNAR_LANDER_TIP_ACCOUNTS:-}" ]] && echo configured || echo empty)"
  echo "lunar_lander_mev_protect=${JITO_LUNAR_LANDER_MEV_PROTECT:-}"
  echo "circular_fast_enabled=${JITO_CIRCULAR_FAST_ENABLED:-}"
  echo "circular_fast_urls=$([[ -n "${JITO_CIRCULAR_FAST_URLS:-}" ]] && echo configured || echo empty)"
  echo "circular_fast_api_key=$([[ -n "${JITO_CIRCULAR_FAST_API_KEY:-}" ]] && echo configured || echo empty)"
  echo "circular_fast_tip_lamports=${JITO_CIRCULAR_FAST_TIP_LAMPORTS:-}"
  echo "circular_fast_tip_account=$([[ -n "${JITO_CIRCULAR_FAST_TIP_ACCOUNT:-}" ]] && echo configured || echo empty)"
  echo "circular_fast_tip_accounts=$([[ -n "${JITO_CIRCULAR_FAST_TIP_ACCOUNTS:-}" ]] && echo configured || echo empty)"
  echo "circular_fast_front_running_protection=${JITO_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION:-}"
  echo "erpc_swqos_enabled=${JITO_ERPC_SWQOS_ENABLED:-}"
  echo "erpc_swqos_urls=$([[ -n "${JITO_ERPC_SWQOS_URLS:-}" ]] && echo configured || echo empty)"
  echo "erpc_leader_slots_enabled=${JITO_ERPC_LEADER_SLOTS_ENABLED:-}"
  echo "erpc_leader_slots_url=$([[ -n "${JITO_ERPC_LEADER_SLOTS_URL:-}" ]] && echo configured || echo empty)"
  echo "erpc_api_key=$([[ -n "${JITO_ERPC_API_KEY:-}" ]] && echo configured || echo empty)"
  echo "erpc_yellowstone_grpc_url=$([[ -n "${JITO_ERPC_YELLOWSTONE_GRPC_URL:-}" ]] && echo configured || echo empty)"
  echo "erpc_yellowstone_grpc_x_token=$([[ -n "${JITO_ERPC_YELLOWSTONE_GRPC_X_TOKEN:-}" ]] && echo configured || echo empty)"
  echo "beam_enabled=${JITO_BEAM_ENABLED:-}"
  echo "beam_url=$([[ -n "${JITO_BEAM_URL:-}" ]] && echo configured || echo empty)"
  echo "beam_token=$([[ -n "${JITO_BEAM_TOKEN:-}" ]] && echo configured || echo empty)"
  echo "beam_provider=${JITO_BEAM_PROVIDER:-}"
  echo "beam_mode=${JITO_BEAM_MODE:-}"
  echo "beam_tip_lamports=${JITO_BEAM_TIP_LAMPORTS:-}"
  echo "beam_tip_accounts=$([[ -n "${JITO_BEAM_TIP_ACCOUNTS:-}" ]] && echo configured || echo empty)"
  echo "zero_slot_enabled=${JITO_ZERO_SLOT_ENABLED:-}"
  echo "zero_slot_urls=$([[ -n "${JITO_ZERO_SLOT_URLS:-}" ]] && echo configured || echo empty)"
  echo "zero_slot_api_key=$([[ -n "${JITO_ZERO_SLOT_API_KEY:-}" ]] && echo configured || echo empty)"
  echo "zero_slot_tip_lamports=${JITO_ZERO_SLOT_TIP_LAMPORTS:-}"
  echo "zero_slot_tip_accounts=$([[ -n "${JITO_ZERO_SLOT_TIP_ACCOUNTS:-}" ]] && echo configured || echo empty)"
  echo "tpu_jet_enabled=${JITO_TPU_JET_ENABLED:-}"
  echo "tpu_jet_rpc_url=$([[ -n "${JITO_TPU_JET_RPC_URL:-}" ]] && echo configured || echo empty)"
  echo "tpu_jet_ws_url=$([[ -n "${JITO_TPU_JET_WS_URL:-}" ]] && echo configured || echo empty)"
  echo "tpu_jet_sidecar_url=$([[ -n "${JITO_TPU_JET_SIDECAR_URL:-}" ]] && echo configured || echo empty)"
  echo "tpu_jet_fanout_slots=${JITO_TPU_JET_FANOUT_SLOTS:-}"
  echo "tpu_jet_timeout_ms=${JITO_TPU_JET_TIMEOUT_MS:-}"
  echo "tpu_quic_enabled=${JITO_TPU_QUIC_ENABLED:-}"
  echo "tpu_quic_rpc_url=$([[ -n "${JITO_TPU_QUIC_RPC_URL:-}" ]] && echo configured || echo empty)"
  echo "tpu_quic_ws_url=$([[ -n "${JITO_TPU_QUIC_WS_URL:-}" ]] && echo configured || echo empty)"
  echo "tpu_quic_fanout_slots=${JITO_TPU_QUIC_FANOUT_SLOTS:-}"
  echo "tpu_quic_timeout_ms=${JITO_TPU_QUIC_TIMEOUT_MS:-}"
  echo "priority_fee_micro_lamports=${JITO_PRIORITY_FEE_MICRO_LAMPORTS:-}"
  echo "max_priority_fee_micro_lamports=${JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS:-}"
  echo "dynamic_priority_fee_enabled=${JITO_DYNAMIC_PRIORITY_FEE_ENABLED:-}"
  echo "dynamic_priority_fee_baseline_micro_lamports=${JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS:-}"
  echo "dynamic_priority_fee_aggressive_micro_lamports=${JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS:-}"
  echo "dynamic_priority_fee_panic_micro_lamports=${JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS:-}"
  echo "dynamic_priority_fee_max_micro_lamports=${JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS:-}"
  echo "account_priority_fee_enabled=${JITO_ACCOUNT_PRIORITY_FEE_ENABLED:-}"
  echo "account_priority_fee_refresh_ms=${JITO_ACCOUNT_PRIORITY_FEE_REFRESH_MS:-}"
  echo "account_priority_fee_stale_ms=${JITO_ACCOUNT_PRIORITY_FEE_STALE_MS:-}"
  echo "account_priority_fee_percentile=${JITO_ACCOUNT_PRIORITY_FEE_PERCENTILE:-}"
  echo "send_max_retries=${JITO_SEND_MAX_RETRIES:-$DEFAULT_BASELINE_RETRIES}"
  echo "copy_wallet_balance_refresh_ms=${JITO_COPY_WALLET_BALANCE_REFRESH_MS:-$DEFAULT_COPY_WALLET_BALANCE_REFRESH_MS}"
  echo "copy_wallet_balance_stale_ms=${JITO_COPY_WALLET_BALANCE_STALE_MS:-$DEFAULT_COPY_WALLET_BALANCE_STALE_MS}"
  echo "blockhash_refresh_ms=${JITO_BLOCKHASH_REFRESH_MS:-$DEFAULT_BLOCKHASH_REFRESH_MS}"
  echo "blockhash_refresh_timeout_ms=${JITO_BLOCKHASH_REFRESH_TIMEOUT_MS:-$DEFAULT_BLOCKHASH_REFRESH_TIMEOUT_MS}"
  echo "blockhash_stale_ms=${JITO_BLOCKHASH_STALE_MS:-$DEFAULT_BLOCKHASH_STALE_MS}"
  echo "max_provider_tip_lamports=${JITO_MAX_PROVIDER_TIP_LAMPORTS:-}"
  echo "max_signed_tx_bytes=${JITO_MAX_SIGNED_TX_BYTES:-}"
  echo "max_instruction_count=${JITO_MAX_INSTRUCTION_COUNT:-}"
  echo "max_writable_account_count=${JITO_MAX_WRITABLE_ACCOUNT_COUNT:-}"
  echo "jito_block_engine_send_urls=$([[ -n "${JITO_BLOCK_ENGINE_SEND_URLS:-}" ]] && echo configured || echo empty)"
  if [[ -f "$MARKER_FILE" ]]; then
    echo "marker=$MARKER_FILE"
    sed -n '1,20p' "$MARKER_FILE"
  else
    echo "marker=none"
  fi
}

score_window() {
  local since_iso="${1:-}" min_position_eligible="${2:-5}"
  if [[ -z "$since_iso" && -f "$MARKER_FILE" ]]; then
    load_env_file "$MARKER_FILE"
    since_iso="${CANARY_STARTED_AT_ISO:-}"
  fi
  if [[ -z "$since_iso" ]]; then
    since_iso="$(current_service_start_iso)"
  fi

  load_env_file "$APP_ENV_FILE"
  load_env_file "$WORKER_ENV_FILE"
  cd "$WORKER_DIR"
  node "$WORKER_DIR/landing-scoreboard-report.mjs" \
    --executions="$EXECUTIONS_PATH" \
    --sent-only \
    --since-iso="$since_iso" \
    --limit="${JITO_CANARY_REPORT_LIMIT:-20}" \
    --position-enrich-limit="${JITO_CANARY_POSITION_ENRICH_LIMIT:-100}" \
    --min-position-eligible="$min_position_eligible" \
    --min-tx-delta-coverage="${JITO_CANARY_MIN_TX_DELTA_COVERAGE:-0.9}" \
    --min-canary-sent="${JITO_CANARY_MIN_SENT:-10}" \
    --target-tx-delta="${JITO_CANARY_TARGET_TX_DELTA:-10}"
}

score_recent() {
  local last_sent="${1:-50}" min_position_eligible="${2:-5}"
  load_env_file "$APP_ENV_FILE"
  load_env_file "$WORKER_ENV_FILE"
  cd "$WORKER_DIR"
  node "$WORKER_DIR/landing-scoreboard-report.mjs" \
    --executions="$EXECUTIONS_PATH" \
    --sent-only \
    --last-sent="$last_sent" \
    --limit="${JITO_CANARY_REPORT_LIMIT:-20}" \
    --position-enrich-limit="${JITO_CANARY_POSITION_ENRICH_LIMIT:-100}" \
    --min-position-eligible="$min_position_eligible" \
    --min-tx-delta-coverage="${JITO_CANARY_MIN_TX_DELTA_COVERAGE:-0.9}" \
    --min-canary-sent="${JITO_CANARY_MIN_SENT:-10}" \
    --target-tx-delta="${JITO_CANARY_TARGET_TX_DELTA:-10}"
}

compare_windows() {
  local baseline_since_iso="${1:-}" canary_since_iso="${2:-}" canary_until_iso="${3:-}"
  if [[ -f "$MARKER_FILE" ]]; then
    load_env_file "$MARKER_FILE"
  fi
  baseline_since_iso="${baseline_since_iso:-${CANARY_BASELINE_STARTED_AT_ISO:-}}"
  canary_since_iso="${canary_since_iso:-${CANARY_STARTED_AT_ISO:-}}"
  if [[ -z "$baseline_since_iso" ]]; then
    echo "baseline since ISO is required; run 'mark baseline' before applying a canary or pass it explicitly" >&2
    exit 2
  fi
  if [[ -z "$canary_since_iso" ]]; then
    echo "canary since ISO is required; apply a canary or pass it explicitly" >&2
    exit 2
  fi

  load_env_file "$APP_ENV_FILE"
  load_env_file "$WORKER_ENV_FILE"
  cd "$WORKER_DIR"
  local args=(
    "$WORKER_DIR/landing-scoreboard-report.mjs"
    --executions="$EXECUTIONS_PATH"
    --sent-only
    --promotion-compare
    --baseline-since-iso="$baseline_since_iso"
    --baseline-until-iso="$canary_since_iso"
    --canary-since-iso="$canary_since_iso"
    --limit="${JITO_CANARY_REPORT_LIMIT:-20}"
    --position-enrich-limit="${JITO_CANARY_POSITION_ENRICH_LIMIT:-100}"
    --min-position-eligible="${JITO_CANARY_COMPARE_MIN_POSITION_ELIGIBLE:-${JITO_CANARY_BASELINE_MIN_POSITION_ELIGIBLE:-5}}"
    --min-tx-delta-coverage="${JITO_CANARY_MIN_TX_DELTA_COVERAGE:-0.9}"
    --min-canary-sent="${JITO_CANARY_MIN_SENT:-10}"
    --promotion-tx-delta-target="${JITO_CANARY_PROMOTION_TX_DELTA_TARGET:-50}"
    --allow-p90-observed-to-signed-regression-ms="${JITO_CANARY_ALLOW_P90_OBSERVED_TO_SIGNED_REGRESSION_MS:-0}"
    --allow-p90-observed-to-submitted-regression-ms="${JITO_CANARY_ALLOW_P90_OBSERVED_TO_SUBMITTED_REGRESSION_MS:-0}"
  )
  if [[ -n "$canary_until_iso" ]]; then
    args+=(--canary-until-iso="$canary_until_iso")
  fi
  node "${args[@]}"
}

ready_report() {
  local strict_rc recent_rc
  echo "== canary status =="
  print_status
  echo ""
  echo "== strict baseline gate =="
  set +e
  score_window "" "${JITO_CANARY_BASELINE_MIN_POSITION_ELIGIBLE:-5}"
  strict_rc=$?
  set -e
  echo "strict_baseline_gate_exit=$strict_rc"
  echo ""
  echo "== recent historical context =="
  set +e
  score_recent "${JITO_CANARY_RECENT_LAST_SENT:-50}" "${JITO_CANARY_RECENT_MIN_POSITION_ELIGIBLE:-5}"
  recent_rc=$?
  set -e
  echo "recent_context_gate_exit=$recent_rc"
  return "$strict_rc"
}

baseline_gate() {
  local name="$1"
  if [[ "$name" == "baseline" ]]; then
    return 0
  fi
  case "$(printf '%s' "${JITO_CANARY_SKIP_BASELINE_GATE:-false}" | tr '[:upper:]' '[:lower:]')" in
    yes|true|1|on)
      echo "baseline gate skipped by JITO_CANARY_SKIP_BASELINE_GATE=$JITO_CANARY_SKIP_BASELINE_GATE" >&2
      return 0
      ;;
  esac

  echo "checking baseline gate before applying $name" >&2
  score_window "" "${JITO_CANARY_BASELINE_MIN_POSITION_ELIGIBLE:-5}"
}

jet_sidecar_health_url() {
  local base="${JITO_TPU_JET_SIDECAR_URL%/}"
  base="${base%/send}"
  printf '%s/health\n' "$base"
}

require_jet_sidecar_ready() {
  local health_url
  if ! command -v curl >/dev/null 2>&1; then
    echo "curl is required to preflight the Jet sidecar before applying a Jet canary" >&2
    exit 2
  fi
  health_url="$(jet_sidecar_health_url)"
  if ! curl -fsS --max-time "${JITO_CANARY_TPU_JET_HEALTH_TIMEOUT_SECONDS:-2}" "$health_url" >/dev/null; then
    echo "Jet sidecar health check failed at $health_url; start jito-tpu-jet-sidecar.service before applying this canary" >&2
    exit 2
  fi
}

require_nozomi_ready() {
  local name="$1"
  if [[ -z "${CANARY_NOZOMI_URLS:-}" ]]; then
    echo "$name requires JITO_CANARY_NOZOMI_URLS or JITO_NOZOMI_URLS in $WORKER_ENV_FILE or $APP_ENV_FILE" >&2
    exit 2
  fi
  if [[ -z "${CANARY_NOZOMI_TIP:-}" || ! "$CANARY_NOZOMI_TIP" =~ ^[0-9]+$ ]]; then
    echo "$name requires numeric JITO_CANARY_NOZOMI_TIP_LAMPORTS or JITO_NOZOMI_TIP_LAMPORTS" >&2
    exit 2
  fi
  if (( CANARY_NOZOMI_TIP < 1000000 )); then
    echo "$name requires Nozomi tip >= 1000000 lamports" >&2
    exit 2
  fi
  if [[ -z "${CANARY_NOZOMI_TIP_ACCOUNT:-}" ]]; then
    echo "$name requires JITO_CANARY_NOZOMI_TIP_ACCOUNT or JITO_NOZOMI_TIP_ACCOUNT" >&2
    exit 2
  fi
}

require_erpc_swqos_ready() {
  local name="$1"
  if [[ -z "${CANARY_ERPC_SWQOS_URLS:-}" ]]; then
    echo "$name requires JITO_CANARY_ERPC_SWQOS_URLS or JITO_ERPC_SWQOS_URLS" >&2
    exit 2
  fi
}

require_astralane_ready() {
  local name="$1"
  if [[ -z "${CANARY_ASTRALANE_URLS:-}" ]]; then
    echo "$name requires JITO_CANARY_ASTRALANE_URLS or JITO_ASTRALANE_URLS in $WORKER_ENV_FILE or $APP_ENV_FILE" >&2
    exit 2
  fi
  if [[ -z "${CANARY_ASTRALANE_API_KEY:-}" ]]; then
    echo "$name requires JITO_CANARY_ASTRALANE_API_KEY or JITO_ASTRALANE_API_KEY" >&2
    exit 2
  fi
  if [[ -z "${CANARY_ASTRALANE_TIP:-}" || ! "$CANARY_ASTRALANE_TIP" =~ ^[0-9]+$ ]]; then
    echo "$name requires numeric JITO_CANARY_ASTRALANE_TIP_LAMPORTS or JITO_ASTRALANE_TIP_LAMPORTS" >&2
    exit 2
  fi
  if (( CANARY_ASTRALANE_TIP < 1000000 )); then
    echo "$name requires Astralane tip >= 1000000 lamports" >&2
    exit 2
  fi
  if [[ -z "${CANARY_ASTRALANE_TIP_ACCOUNT:-}" && -z "${CANARY_ASTRALANE_TIP_ACCOUNTS:-}" ]]; then
    echo "$name requires JITO_CANARY_ASTRALANE_TIP_ACCOUNT/JITO_ASTRALANE_TIP_ACCOUNT or JITO_CANARY_ASTRALANE_TIP_ACCOUNTS/JITO_ASTRALANE_TIP_ACCOUNTS" >&2
    exit 2
  fi
}

require_lunar_lander_ready() {
  local name="$1"
  if [[ -z "${CANARY_LUNAR_LANDER_URLS:-}" ]]; then
    echo "$name requires JITO_CANARY_LUNAR_LANDER_URLS or JITO_LUNAR_LANDER_URLS in $WORKER_ENV_FILE or $APP_ENV_FILE" >&2
    exit 2
  fi
  if [[ -z "${CANARY_LUNAR_LANDER_API_KEY:-}" ]]; then
    echo "$name requires JITO_CANARY_LUNAR_LANDER_API_KEY or JITO_LUNAR_LANDER_API_KEY" >&2
    exit 2
  fi
  if [[ -z "${CANARY_LUNAR_LANDER_TIP:-}" || ! "$CANARY_LUNAR_LANDER_TIP" =~ ^[0-9]+$ ]]; then
    echo "$name requires numeric JITO_CANARY_LUNAR_LANDER_TIP_LAMPORTS or JITO_LUNAR_LANDER_TIP_LAMPORTS" >&2
    exit 2
  fi
  if (( CANARY_LUNAR_LANDER_TIP < 1000000 )); then
    echo "$name requires Lunar Lander tip >= 1000000 lamports" >&2
    exit 2
  fi
  if [[ -z "${CANARY_LUNAR_LANDER_TIP_ACCOUNT:-}" && -z "${CANARY_LUNAR_LANDER_TIP_ACCOUNTS:-}" ]]; then
    echo "$name requires JITO_CANARY_LUNAR_LANDER_TIP_ACCOUNT/JITO_LUNAR_LANDER_TIP_ACCOUNT or JITO_CANARY_LUNAR_LANDER_TIP_ACCOUNTS/JITO_LUNAR_LANDER_TIP_ACCOUNTS" >&2
    exit 2
  fi
}

require_circular_fast_ready() {
  local name="$1"
  if [[ -z "${CANARY_CIRCULAR_FAST_URLS:-}" ]]; then
    echo "$name requires JITO_CANARY_CIRCULAR_FAST_URLS or JITO_CIRCULAR_FAST_URLS in $WORKER_ENV_FILE or $APP_ENV_FILE" >&2
    exit 2
  fi
  if [[ -z "${CANARY_CIRCULAR_FAST_API_KEY:-}" ]]; then
    echo "$name requires JITO_CANARY_CIRCULAR_FAST_API_KEY or JITO_CIRCULAR_FAST_API_KEY" >&2
    exit 2
  fi
  if [[ -z "${CANARY_CIRCULAR_FAST_TIP:-}" || ! "$CANARY_CIRCULAR_FAST_TIP" =~ ^[0-9]+$ ]]; then
    echo "$name requires numeric JITO_CANARY_CIRCULAR_FAST_TIP_LAMPORTS or JITO_CIRCULAR_FAST_TIP_LAMPORTS" >&2
    exit 2
  fi
  if (( CANARY_CIRCULAR_FAST_TIP < 1000000 )); then
    echo "$name requires Circular Fast tip >= 1000000 lamports" >&2
    exit 2
  fi
  if [[ -z "${CANARY_CIRCULAR_FAST_TIP_ACCOUNT:-}" && -z "${CANARY_CIRCULAR_FAST_TIP_ACCOUNTS:-}" ]]; then
    echo "$name requires JITO_CANARY_CIRCULAR_FAST_TIP_ACCOUNT/JITO_CIRCULAR_FAST_TIP_ACCOUNT or JITO_CANARY_CIRCULAR_FAST_TIP_ACCOUNTS/JITO_CIRCULAR_FAST_TIP_ACCOUNTS" >&2
    exit 2
  fi
}

require_beam_ready() {
  local name="$1" provider mode
  if [[ -z "${CANARY_BEAM_URL:-}" ]]; then
    echo "$name requires JITO_CANARY_BEAM_URL or JITO_BEAM_URL" >&2
    exit 2
  fi
  if [[ -z "${CANARY_BEAM_TOKEN:-}" ]]; then
    echo "$name requires JITO_CANARY_BEAM_TOKEN or JITO_BEAM_TOKEN" >&2
    exit 2
  fi
  provider="$(printf '%s' "${CANARY_BEAM_PROVIDER:-}" | tr '[:upper:]' '[:lower:]')"
  case "$provider" in
    bloxroute|astralane|falcon) ;;
    *)
      echo "$name requires JITO_CANARY_BEAM_PROVIDER or JITO_BEAM_PROVIDER to be bloxroute, astralane, or falcon" >&2
      exit 2
      ;;
  esac
  mode="$(printf '%s' "${CANARY_BEAM_MODE:-}" | tr '[:upper:]' '[:lower:]')"
  case "$mode" in
    fastest|mev_protect) ;;
    *)
      echo "$name requires JITO_CANARY_BEAM_MODE or JITO_BEAM_MODE to be fastest or mev_protect" >&2
      exit 2
      ;;
  esac
  if [[ "$provider" == "falcon" && "$mode" == "mev_protect" ]]; then
    echo "$name cannot use JITO_BEAM_MODE=mev_protect with falcon" >&2
    exit 2
  fi
  if [[ -z "${CANARY_BEAM_TIP:-}" || ! "$CANARY_BEAM_TIP" =~ ^[0-9]+$ ]]; then
    echo "$name requires numeric JITO_CANARY_BEAM_TIP_LAMPORTS or JITO_BEAM_TIP_LAMPORTS" >&2
    exit 2
  fi
  if (( CANARY_BEAM_TIP < 1000000 )); then
    echo "$name requires Beam tip >= 1000000 lamports" >&2
    exit 2
  fi
  if [[ -z "${CANARY_BEAM_TIP_ACCOUNTS:-}" ]]; then
    echo "$name requires JITO_CANARY_BEAM_TIP_ACCOUNTS or JITO_BEAM_TIP_ACCOUNTS" >&2
    exit 2
  fi
}

require_zero_slot_ready() {
  local name="$1"
  if [[ -z "${CANARY_ZERO_SLOT_URLS:-}" ]]; then
    echo "$name requires JITO_CANARY_ZERO_SLOT_URLS or JITO_ZERO_SLOT_URLS" >&2
    exit 2
  fi
  if [[ -z "${CANARY_ZERO_SLOT_API_KEY:-}" ]]; then
    echo "$name requires JITO_CANARY_ZERO_SLOT_API_KEY or JITO_ZERO_SLOT_API_KEY" >&2
    exit 2
  fi
  if [[ -z "${CANARY_ZERO_SLOT_TIP:-}" || ! "$CANARY_ZERO_SLOT_TIP" =~ ^[0-9]+$ ]]; then
    echo "$name requires numeric JITO_CANARY_ZERO_SLOT_TIP_LAMPORTS or JITO_ZERO_SLOT_TIP_LAMPORTS" >&2
    exit 2
  fi
  if (( CANARY_ZERO_SLOT_TIP < 1000000 )); then
    echo "$name requires 0slot tip >= 1000000 lamports" >&2
    exit 2
  fi
  if [[ -z "${CANARY_ZERO_SLOT_TIP_ACCOUNTS:-}" ]]; then
    echo "$name requires JITO_CANARY_ZERO_SLOT_TIP_ACCOUNTS or JITO_ZERO_SLOT_TIP_ACCOUNTS" >&2
    exit 2
  fi
}

apply_canary() {
  local name="$1" backup since_iso
  load_env_file "$APP_ENV_FILE"
  load_env_file "$WORKER_ENV_FILE"
  if [[ -f "$MARKER_FILE" ]]; then
    load_env_file "$MARKER_FILE"
    if [[ "${CANARY_NAME:-}" == "baseline" && -z "${CANARY_BASELINE_STARTED_AT_ISO:-}" ]]; then
      CANARY_BASELINE_STARTED_AT_ISO="${CANARY_STARTED_AT_ISO:-}"
    fi
  fi
  canary_values "$name"
  case "$CANARY_TPU_JET_ENABLED" in
    true)
      if [[ -z "${JITO_TPU_JET_RPC_URL:-}" ]]; then
        echo "$name requires JITO_TPU_JET_RPC_URL in $WORKER_ENV_FILE or $APP_ENV_FILE" >&2
        exit 2
      fi
      if [[ -z "${JITO_TPU_JET_WS_URL:-}" ]]; then
        echo "$name requires JITO_TPU_JET_WS_URL in $WORKER_ENV_FILE or $APP_ENV_FILE" >&2
        exit 2
      fi
      if [[ -z "${JITO_TPU_JET_SIDECAR_URL:-}" ]]; then
        echo "$name requires JITO_TPU_JET_SIDECAR_URL in $WORKER_ENV_FILE or $APP_ENV_FILE" >&2
        exit 2
      fi
      require_jet_sidecar_ready
      ;;
  esac
  case "$CANARY_TPU_QUIC_ENABLED" in
    true)
      if [[ -z "${JITO_TPU_QUIC_RPC_URL:-}" ]]; then
        echo "$name requires JITO_TPU_QUIC_RPC_URL in $WORKER_ENV_FILE or $APP_ENV_FILE" >&2
        exit 2
      fi
      if [[ -z "${JITO_TPU_QUIC_WS_URL:-}" ]]; then
        echo "$name requires JITO_TPU_QUIC_WS_URL in $WORKER_ENV_FILE or $APP_ENV_FILE" >&2
        exit 2
      fi
      ;;
  esac
  case "$CANARY_NOZOMI_ENABLED" in
    true)
      require_nozomi_ready "$name"
      ;;
  esac
  case "$CANARY_ASTRALANE_ENABLED" in
    true)
      require_astralane_ready "$name"
      ;;
  esac
  case "$CANARY_LUNAR_LANDER_ENABLED" in
    true)
      require_lunar_lander_ready "$name"
      ;;
  esac
  case "$CANARY_CIRCULAR_FAST_ENABLED" in
    true)
      require_circular_fast_ready "$name"
      ;;
  esac
  case "$CANARY_ERPC_SWQOS_ENABLED" in
    true)
      require_erpc_swqos_ready "$name"
      ;;
  esac
  case "$CANARY_BEAM_ENABLED" in
    true)
      require_beam_ready "$name"
      ;;
  esac
  case "$CANARY_ZERO_SLOT_ENABLED" in
    true)
      require_zero_slot_ready "$name"
      ;;
  esac
  baseline_gate "$name"
  backup="$(backup_env)"

  set_env_var "$WORKER_ENV_FILE" JITO_SEND_LANE_MODE "$CANARY_LANE_MODE"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_ENABLED "$CANARY_HELIUS_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_URLS "$CANARY_HELIUS_URLS"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_SWQOS_ONLY "$CANARY_HELIUS_SWQOS_ONLY"
  set_env_var "$WORKER_ENV_FILE" JITO_BLOCK_ENGINE_SEND_URLS ""
  set_env_var "$WORKER_ENV_FILE" JITO_TIP_LAMPORTS "0"
  set_env_var "$WORKER_ENV_FILE" JITO_TPU_JET_ENABLED "$CANARY_TPU_JET_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_TPU_JET_FANOUT_SLOTS "$CANARY_TPU_JET_FANOUT_SLOTS"
  set_env_var "$WORKER_ENV_FILE" JITO_TPU_JET_TIMEOUT_MS "$CANARY_TPU_JET_TIMEOUT_MS"
  set_env_var "$WORKER_ENV_FILE" JITO_TPU_QUIC_ENABLED "$CANARY_TPU_QUIC_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_TPU_QUIC_FANOUT_SLOTS "$CANARY_TPU_QUIC_FANOUT_SLOTS"
  set_env_var "$WORKER_ENV_FILE" JITO_TPU_QUIC_TIMEOUT_MS "$CANARY_TPU_QUIC_TIMEOUT_MS"
  set_env_var "$WORKER_ENV_FILE" JITO_SEND_FANOUT "YES"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_TIP_LAMPORTS "$CANARY_HELIUS_TIP"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_TIP_ACCOUNT "$CANARY_HELIUS_TIP_ACCOUNT"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_TIP_ACCOUNTS "$CANARY_HELIUS_TIP_ACCOUNTS"
  set_env_var "$WORKER_ENV_FILE" JITO_NOZOMI_ENABLED "$CANARY_NOZOMI_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_NOZOMI_URLS "$CANARY_NOZOMI_URLS"
  set_env_var "$WORKER_ENV_FILE" JITO_NOZOMI_TIP_LAMPORTS "$CANARY_NOZOMI_TIP"
  set_env_var "$WORKER_ENV_FILE" JITO_NOZOMI_TIP_ACCOUNT "$CANARY_NOZOMI_TIP_ACCOUNT"
  set_env_var "$WORKER_ENV_FILE" JITO_NOZOMI_TIP_ACCOUNTS "$CANARY_NOZOMI_TIP_ACCOUNTS"
  set_env_var "$WORKER_ENV_FILE" JITO_ASTRALANE_ENABLED "$CANARY_ASTRALANE_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_ASTRALANE_URLS "$CANARY_ASTRALANE_URLS"
  set_env_var "$WORKER_ENV_FILE" JITO_ASTRALANE_API_KEY "$CANARY_ASTRALANE_API_KEY"
  set_env_var "$WORKER_ENV_FILE" JITO_ASTRALANE_TIP_LAMPORTS "$CANARY_ASTRALANE_TIP"
  set_env_var "$WORKER_ENV_FILE" JITO_ASTRALANE_TIP_ACCOUNT "$CANARY_ASTRALANE_TIP_ACCOUNT"
  set_env_var "$WORKER_ENV_FILE" JITO_ASTRALANE_TIP_ACCOUNTS "$CANARY_ASTRALANE_TIP_ACCOUNTS"
  set_env_var "$WORKER_ENV_FILE" JITO_ASTRALANE_MEV_PROTECT "$CANARY_ASTRALANE_MEV_PROTECT"
  set_env_var "$WORKER_ENV_FILE" JITO_ASTRALANE_SWQOS_ONLY "$CANARY_ASTRALANE_SWQOS_ONLY"
  set_env_var "$WORKER_ENV_FILE" JITO_LUNAR_LANDER_ENABLED "$CANARY_LUNAR_LANDER_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_LUNAR_LANDER_URLS "$CANARY_LUNAR_LANDER_URLS"
  set_env_var "$WORKER_ENV_FILE" JITO_LUNAR_LANDER_API_KEY "$CANARY_LUNAR_LANDER_API_KEY"
  set_env_var "$WORKER_ENV_FILE" JITO_LUNAR_LANDER_TIP_LAMPORTS "$CANARY_LUNAR_LANDER_TIP"
  set_env_var "$WORKER_ENV_FILE" JITO_LUNAR_LANDER_TIP_ACCOUNT "$CANARY_LUNAR_LANDER_TIP_ACCOUNT"
  set_env_var "$WORKER_ENV_FILE" JITO_LUNAR_LANDER_TIP_ACCOUNTS "$CANARY_LUNAR_LANDER_TIP_ACCOUNTS"
  set_env_var "$WORKER_ENV_FILE" JITO_LUNAR_LANDER_MEV_PROTECT "$CANARY_LUNAR_LANDER_MEV_PROTECT"
  set_env_var "$WORKER_ENV_FILE" JITO_CIRCULAR_FAST_ENABLED "$CANARY_CIRCULAR_FAST_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_CIRCULAR_FAST_URLS "$CANARY_CIRCULAR_FAST_URLS"
  set_env_var "$WORKER_ENV_FILE" JITO_CIRCULAR_FAST_API_KEY "$CANARY_CIRCULAR_FAST_API_KEY"
  set_env_var "$WORKER_ENV_FILE" JITO_CIRCULAR_FAST_TIP_LAMPORTS "$CANARY_CIRCULAR_FAST_TIP"
  set_env_var "$WORKER_ENV_FILE" JITO_CIRCULAR_FAST_TIP_ACCOUNT "$CANARY_CIRCULAR_FAST_TIP_ACCOUNT"
  set_env_var "$WORKER_ENV_FILE" JITO_CIRCULAR_FAST_TIP_ACCOUNTS "$CANARY_CIRCULAR_FAST_TIP_ACCOUNTS"
  set_env_var "$WORKER_ENV_FILE" JITO_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION "$CANARY_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION"
  set_env_var "$WORKER_ENV_FILE" JITO_ERPC_SWQOS_ENABLED "$CANARY_ERPC_SWQOS_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_ERPC_SWQOS_URLS "$CANARY_ERPC_SWQOS_URLS"
  set_env_var "$WORKER_ENV_FILE" JITO_ERPC_LEADER_SLOTS_ENABLED "$CANARY_ERPC_LEADER_SLOTS_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_ERPC_LEADER_SLOTS_URL "$CANARY_ERPC_LEADER_SLOTS_URL"
  set_env_var "$WORKER_ENV_FILE" JITO_ERPC_API_KEY "$CANARY_ERPC_API_KEY"
  set_env_var "$WORKER_ENV_FILE" JITO_BEAM_ENABLED "$CANARY_BEAM_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_BEAM_URL "$CANARY_BEAM_URL"
  set_env_var "$WORKER_ENV_FILE" JITO_BEAM_TOKEN "$CANARY_BEAM_TOKEN"
  set_env_var "$WORKER_ENV_FILE" JITO_BEAM_PROVIDER "$CANARY_BEAM_PROVIDER"
  set_env_var "$WORKER_ENV_FILE" JITO_BEAM_MODE "$CANARY_BEAM_MODE"
  set_env_var "$WORKER_ENV_FILE" JITO_BEAM_TIP_LAMPORTS "$CANARY_BEAM_TIP"
  set_env_var "$WORKER_ENV_FILE" JITO_BEAM_TIP_ACCOUNTS "$CANARY_BEAM_TIP_ACCOUNTS"
  set_env_var "$WORKER_ENV_FILE" JITO_ZERO_SLOT_ENABLED "$CANARY_ZERO_SLOT_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_ZERO_SLOT_URLS "$CANARY_ZERO_SLOT_URLS"
  set_env_var "$WORKER_ENV_FILE" JITO_ZERO_SLOT_API_KEY "$CANARY_ZERO_SLOT_API_KEY"
  set_env_var "$WORKER_ENV_FILE" JITO_ZERO_SLOT_TIP_LAMPORTS "$CANARY_ZERO_SLOT_TIP"
  set_env_var "$WORKER_ENV_FILE" JITO_ZERO_SLOT_TIP_ACCOUNTS "$CANARY_ZERO_SLOT_TIP_ACCOUNTS"
  set_env_var "$WORKER_ENV_FILE" JITO_BLOCKHASH_COMMITMENT "$CANARY_BLOCKHASH_COMMITMENT"
  set_env_var "$WORKER_ENV_FILE" JITO_PRIORITY_FEE_MICRO_LAMPORTS "$CANARY_PRIORITY"
  set_env_var "$WORKER_ENV_FILE" JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS "$CANARY_MAX_PRIORITY"
  set_env_var "$WORKER_ENV_FILE" JITO_DYNAMIC_PRIORITY_FEE_ENABLED "$CANARY_DYNAMIC_PRIORITY_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS "$CANARY_DYNAMIC_PRIORITY_BASELINE"
  set_env_var "$WORKER_ENV_FILE" JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS "$CANARY_DYNAMIC_PRIORITY_AGGRESSIVE"
  set_env_var "$WORKER_ENV_FILE" JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS "$CANARY_DYNAMIC_PRIORITY_PANIC"
  set_env_var "$WORKER_ENV_FILE" JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS "$CANARY_DYNAMIC_PRIORITY_MAX"
  set_env_var "$WORKER_ENV_FILE" JITO_ACCOUNT_PRIORITY_FEE_ENABLED "$CANARY_ACCOUNT_PRIORITY_FEE_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_ACCOUNT_PRIORITY_FEE_REFRESH_MS "$CANARY_ACCOUNT_PRIORITY_FEE_REFRESH_MS"
  set_env_var "$WORKER_ENV_FILE" JITO_ACCOUNT_PRIORITY_FEE_STALE_MS "$CANARY_ACCOUNT_PRIORITY_FEE_STALE_MS"
  set_env_var "$WORKER_ENV_FILE" JITO_ACCOUNT_PRIORITY_FEE_PERCENTILE "$CANARY_ACCOUNT_PRIORITY_FEE_PERCENTILE"
  set_env_var "$WORKER_ENV_FILE" JITO_SEND_MAX_RETRIES "$CANARY_RETRIES"
  set_env_var "$WORKER_ENV_FILE" JITO_MAX_PROVIDER_TIP_LAMPORTS "$CANARY_MAX_PROVIDER_TIP_LAMPORTS"
  set_env_var "$WORKER_ENV_FILE" JITO_MAX_SIGNED_TX_BYTES "$CANARY_MAX_SIGNED_TX_BYTES"
  set_env_var "$WORKER_ENV_FILE" JITO_MAX_INSTRUCTION_COUNT "$CANARY_MAX_INSTRUCTION_COUNT"
  set_env_var "$WORKER_ENV_FILE" JITO_MAX_WRITABLE_ACCOUNT_COUNT "$CANARY_MAX_WRITABLE_ACCOUNT_COUNT"

  systemctl restart "$LIVE_SERVICE"
  sleep 2
  systemctl is-active --quiet "$LIVE_SERVICE"
  since_iso="$(current_service_start_iso)"
  write_marker "$name" "$since_iso" "$backup"
  echo "applied=$name"
  echo "backup=$backup"
  print_status
}

restore_env() {
  local backup="$1"
  if [[ ! -f "$backup" ]]; then
    echo "backup env file not found: $backup" >&2
    exit 2
  fi
  cp -a "$backup" "$WORKER_ENV_FILE"
  systemctl restart "$LIVE_SERVICE"
  sleep 2
  systemctl is-active --quiet "$LIVE_SERVICE"
  rm -f "$MARKER_FILE"
  echo "restored=$backup"
  print_status
}

command="${1:-}"
case "$command" in
  list)
    usage
    ;;
  status)
    print_status
    ;;
  mark)
    name="${2:-baseline}"
    load_env_file "$APP_ENV_FILE"
    load_env_file "$WORKER_ENV_FILE"
    canary_values "$name"
    write_marker "$name" "${3:-$(current_service_start_iso)}"
    print_status
    ;;
  score)
    score_window "${2:-}" "${3:-5}"
    ;;
  score-recent)
    score_recent "${2:-50}" "${3:-5}"
    ;;
  compare)
    compare_windows "${2:-}" "${3:-}" "${4:-}"
    ;;
  ready)
    ready_report
    ;;
  apply)
    [[ -n "${2:-}" ]] || { usage >&2; exit 2; }
    apply_canary "$2"
    ;;
  restore)
    [[ -n "${2:-}" ]] || { usage >&2; exit 2; }
    restore_env "$2"
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
