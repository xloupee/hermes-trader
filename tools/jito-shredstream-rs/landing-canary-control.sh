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
DEFAULT_BASELINE_RETRIES="${JITO_CANARY_BASELINE_SEND_MAX_RETRIES:-3}"

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
  baseline       Helius Sender FRA SWQoS, tip 387500, priority 968750, retries 3
  tip-250k       Same as baseline, Helius Sender tip 250000
  tip-500k       Same as baseline, Helius Sender tip 500000
  priority-750k  Same as baseline, priority fee 750000
  priority-1250k Same as baseline, priority fee 1250000 and matching max cap
  dynamic-priority-2500k Helius only, retries 0, 200k tip, early/mid priority 2500000
  retries-0      Same as baseline, send max retries 0
  retries-1      Same as baseline, send max retries 1
  retries-3      Return to baseline, send max retries 3
  tpu-jet-fanout Helius baseline plus same-signature Yellowstone Jet sidecar fanout
  tpu-jet-only Same fee shape, Yellowstone Jet sidecar only
  tpu-jet-cheap Jet sidecar only with Helius Sender tip disabled; requires JITO_CANARY_ALLOW_CHEAP_TPU=YES
  tpu-quic-fanout Helius baseline plus same-signature direct TPU QUIC fanout
  tpu-quic-only Same fee shape, direct TPU QUIC only
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

canary_values() {
  local name="$1"
  CANARY_HELIUS_TIP="$DEFAULT_BASELINE_TIP"
  CANARY_PRIORITY="$DEFAULT_BASELINE_PRIORITY"
  CANARY_MAX_PRIORITY="$DEFAULT_BASELINE_PRIORITY"
  CANARY_RETRIES="$DEFAULT_BASELINE_RETRIES"
  CANARY_LANE_MODE="helius-sender-only"
  CANARY_HELIUS_ENABLED="true"
  CANARY_TPU_JET_ENABLED="false"
  CANARY_TPU_QUIC_ENABLED="false"
  CANARY_HELIUS_TIP_ACCOUNT="${JITO_HELIUS_SENDER_TIP_ACCOUNT:-}"
  CANARY_DYNAMIC_PRIORITY_ENABLED="false"
  CANARY_DYNAMIC_PRIORITY_BASELINE=""
  CANARY_DYNAMIC_PRIORITY_AGGRESSIVE=""
  CANARY_DYNAMIC_PRIORITY_PANIC=""
  CANARY_DYNAMIC_PRIORITY_MAX=""

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
    dynamic-priority-2500k)
      CANARY_HELIUS_TIP=200000
      CANARY_PRIORITY=1250000
      CANARY_MAX_PRIORITY=2500000
      CANARY_RETRIES=0
      CANARY_DYNAMIC_PRIORITY_ENABLED="true"
      CANARY_DYNAMIC_PRIORITY_BASELINE=1250000
      CANARY_DYNAMIC_PRIORITY_AGGRESSIVE=2500000
      CANARY_DYNAMIC_PRIORITY_MAX=2500000
      ;;
    retries-0) CANARY_RETRIES=0 ;;
    retries-1) CANARY_RETRIES=1 ;;
    retries-3) CANARY_RETRIES=3 ;;
    tpu-jet-fanout)
      CANARY_LANE_MODE="helius-tpu-jet"
      CANARY_TPU_JET_ENABLED="true"
      ;;
    tpu-jet-only)
      CANARY_LANE_MODE="tpu-jet-helius-tip"
      CANARY_TPU_JET_ENABLED="true"
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
CANARY_TPU_JET_ENABLED=${CANARY_TPU_JET_ENABLED:-}
CANARY_TPU_QUIC_ENABLED=${CANARY_TPU_QUIC_ENABLED:-}
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
  echo "helius_sender_swqos_only=${JITO_HELIUS_SENDER_SWQOS_ONLY:-}"
  echo "helius_sender_tip_lamports=${JITO_HELIUS_SENDER_TIP_LAMPORTS:-}"
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
  echo "send_max_retries=${JITO_SEND_MAX_RETRIES:-$DEFAULT_BASELINE_RETRIES}"
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
    # shellcheck disable=SC1090
    source "$MARKER_FILE"
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
    # shellcheck disable=SC1090
    source "$MARKER_FILE"
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

apply_canary() {
  local name="$1" backup since_iso
  load_env_file "$APP_ENV_FILE"
  load_env_file "$WORKER_ENV_FILE"
  if [[ -f "$MARKER_FILE" ]]; then
    # shellcheck disable=SC1090
    source "$MARKER_FILE"
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
  baseline_gate "$name"
  backup="$(backup_env)"

  set_env_var "$WORKER_ENV_FILE" JITO_SEND_LANE_MODE "$CANARY_LANE_MODE"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_ENABLED "$CANARY_HELIUS_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_SWQOS_ONLY "true"
  set_env_var "$WORKER_ENV_FILE" JITO_BLOCK_ENGINE_SEND_URLS ""
  set_env_var "$WORKER_ENV_FILE" JITO_TIP_LAMPORTS "0"
  set_env_var "$WORKER_ENV_FILE" JITO_TPU_JET_ENABLED "$CANARY_TPU_JET_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_TPU_QUIC_ENABLED "$CANARY_TPU_QUIC_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_SEND_FANOUT "YES"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_TIP_LAMPORTS "$CANARY_HELIUS_TIP"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_TIP_ACCOUNT "$CANARY_HELIUS_TIP_ACCOUNT"
  set_env_var "$WORKER_ENV_FILE" JITO_PRIORITY_FEE_MICRO_LAMPORTS "$CANARY_PRIORITY"
  set_env_var "$WORKER_ENV_FILE" JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS "$CANARY_MAX_PRIORITY"
  set_env_var "$WORKER_ENV_FILE" JITO_DYNAMIC_PRIORITY_FEE_ENABLED "$CANARY_DYNAMIC_PRIORITY_ENABLED"
  set_env_var "$WORKER_ENV_FILE" JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS "$CANARY_DYNAMIC_PRIORITY_BASELINE"
  set_env_var "$WORKER_ENV_FILE" JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS "$CANARY_DYNAMIC_PRIORITY_AGGRESSIVE"
  set_env_var "$WORKER_ENV_FILE" JITO_DYNAMIC_PRIORITY_FEE_PANIC_MICRO_LAMPORTS "$CANARY_DYNAMIC_PRIORITY_PANIC"
  set_env_var "$WORKER_ENV_FILE" JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS "$CANARY_DYNAMIC_PRIORITY_MAX"
  set_env_var "$WORKER_ENV_FILE" JITO_SEND_MAX_RETRIES "$CANARY_RETRIES"

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
