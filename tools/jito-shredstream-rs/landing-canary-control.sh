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

usage() {
  cat <<'USAGE'
Usage:
  landing-canary-control.sh list
  landing-canary-control.sh status
  landing-canary-control.sh mark <name> [since-iso]
  landing-canary-control.sh score [since-iso] [min-position-eligible]
  landing-canary-control.sh score-recent [last-sent] [min-position-eligible]
  landing-canary-control.sh ready
  landing-canary-control.sh apply <name>
  landing-canary-control.sh restore <backup-env-file>

Canaries:
  baseline       Helius Sender FRA SWQoS, tip 387500, priority 968750, retries 0
  tip-250k       Same as baseline, Helius Sender tip 250000
  tip-500k       Same as baseline, Helius Sender tip 500000
  priority-750k  Same as baseline, priority fee 750000
  priority-1250k Same as baseline, priority fee 1250000 and matching max cap
  retries-0      Same as baseline, send max retries 0
  retries-1      Same as baseline, send max retries 1
  retries-3      Legacy comparison only, send max retries 3
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
    retries-0) CANARY_RETRIES=0 ;;
    retries-1) CANARY_RETRIES=1 ;;
    retries-3) CANARY_RETRIES=3 ;;
    *)
      echo "unknown canary: $name" >&2
      usage >&2
      exit 2
      ;;
  esac
}

write_marker() {
  local name="$1" since_iso="$2" backup="${3:-}"
  cat > "$MARKER_FILE" <<EOF_MARKER
CANARY_NAME=$name
CANARY_STARTED_AT_ISO=$since_iso
CANARY_BACKUP_ENV=$backup
CANARY_HELIUS_TIP_LAMPORTS=${CANARY_HELIUS_TIP:-}
CANARY_PRIORITY_FEE_MICRO_LAMPORTS=${CANARY_PRIORITY:-}
CANARY_SEND_MAX_RETRIES=${CANARY_RETRIES:-}
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
  echo "priority_fee_micro_lamports=${JITO_PRIORITY_FEE_MICRO_LAMPORTS:-}"
  echo "max_priority_fee_micro_lamports=${JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS:-}"
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

apply_canary() {
  local name="$1" backup since_iso
  canary_values "$name"
  baseline_gate "$name"
  backup="$(backup_env)"

  set_env_var "$WORKER_ENV_FILE" JITO_SEND_LANE_MODE "helius-sender-only"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_ENABLED "true"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_SWQOS_ONLY "true"
  set_env_var "$WORKER_ENV_FILE" JITO_BLOCK_ENGINE_SEND_URLS ""
  set_env_var "$WORKER_ENV_FILE" JITO_TIP_LAMPORTS "0"
  set_env_var "$WORKER_ENV_FILE" JITO_HELIUS_SENDER_TIP_LAMPORTS "$CANARY_HELIUS_TIP"
  set_env_var "$WORKER_ENV_FILE" JITO_PRIORITY_FEE_MICRO_LAMPORTS "$CANARY_PRIORITY"
  set_env_var "$WORKER_ENV_FILE" JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS "$CANARY_MAX_PRIORITY"
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
