#!/usr/bin/env bash
set -euo pipefail

WORKER_DIR="${JITO_WORKER_DIR:-/opt/jito-feed-probe-watch}"
APP_ENV_FILE="${JITO_APP_ENV_FILE:-/opt/pumpfun-migration-bot/.env}"
WORKER_ENV_FILE="${JITO_WORKER_ENV_FILE:-/etc/jito-copy-live.env}"
MARKER_FILE="${JITO_CANARY_MARKER_FILE:-/var/log/jito-copy-canary-current.env}"
EXECUTIONS_PATH="${JITO_COPY_EXECUTIONS_PATH:-/var/log/jito-copy-executions-vps.jsonl}"
MIN_SENT="${JITO_CANARY_SEQUENCE_MIN_SENT:-${JITO_CANARY_MIN_SENT:-10}}"
MIN_POSITION_ELIGIBLE="${JITO_CANARY_SEQUENCE_MIN_POSITION_ELIGIBLE:-5}"
SEQUENCE=(${JITO_CANARY_SEQUENCE:-tip-rotated blockhash-confirmed account-priority-cache})

usage() {
  cat <<'USAGE'
Usage:
  landing-canary-sequence.sh status
  landing-canary-sequence.sh next

Runs the remaining landing canary sequence with hard gates:
  baseline/fixed tip + processed blockhash
  tip-rotated
  blockhash-confirmed
  account-priority-cache

The script never advances to the next canary unless the current window has
enough unique sent rows and txDelta-covered rows. The full score is printed for
context, but a bad txDelta target does not block starting the next experiment.
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

current_utc_iso() {
  date -u +%Y-%m-%dT%H:%M:%SZ
}

ensure_worker_dir() {
  cd "$WORKER_DIR"
}

marker_name() {
  load_env_file "$MARKER_FILE"
  printf '%s\n' "${CANARY_NAME:-}"
}

marker_started_at() {
  load_env_file "$MARKER_FILE"
  printf '%s\n' "${CANARY_STARTED_AT_ISO:-}"
}

sequence_index() {
  local name="$1" index=0
  for candidate in "${SEQUENCE[@]}"; do
    if [[ "$candidate" == "$name" ]]; then
      printf '%s\n' "$index"
      return 0
    fi
    index=$((index + 1))
  done
  return 1
}

next_after() {
  local name="$1" index next_index
  if [[ -z "$name" || "$name" == "baseline" ]]; then
    printf '%s\n' "${SEQUENCE[0]:-}"
    return 0
  fi
  index="$(sequence_index "$name")" || return 1
  next_index=$((index + 1))
  if (( next_index >= ${#SEQUENCE[@]} )); then
    return 1
  fi
  printf '%s\n' "${SEQUENCE[$next_index]}"
}

score_current() {
  local since="$1"
  ./landing-canary-control.sh score "$since" "$MIN_POSITION_ELIGIBLE"
}

gate_current_window() {
  local since="$1"
  local json_path
  json_path="$(mktemp)"
  load_env_file "$APP_ENV_FILE"
  load_env_file "$WORKER_ENV_FILE"
  set +e
  node "$WORKER_DIR/landing-scoreboard-report.mjs" \
    --executions="$EXECUTIONS_PATH" \
    --sent-only \
    --since-iso="$since" \
    --limit=0 \
    --position-enrich-limit="${JITO_CANARY_POSITION_ENRICH_LIMIT:-100}" \
    --min-position-eligible="$MIN_POSITION_ELIGIBLE" \
    --min-tx-delta-coverage="${JITO_CANARY_MIN_TX_DELTA_COVERAGE:-0.9}" \
    --min-canary-sent="$MIN_SENT" \
    --target-tx-delta="${JITO_CANARY_TARGET_TX_DELTA:-10}" \
    --json >"$json_path"
  local score_rc=$?
  set -e
  set +e
  python3 - "$json_path" "$MIN_SENT" "$MIN_POSITION_ELIGIBLE" <<'PY'
import json
import sys

path, min_sent_raw, min_position_raw = sys.argv[1:4]
min_sent = int(min_sent_raw)
min_position = int(min_position_raw)

with open(path, errors="replace") as handle:
    scoreboard = json.load(handle)

summary = scoreboard.get("summary", {})
sample_gate = scoreboard.get("sampleGate", {})
tx_delta_gate = scoreboard.get("txDeltaGate", {})
sent = summary.get("sent", 0)
landed = summary.get("landed", 0)
position_eligible = summary.get("positionEligible", 0)
tx_delta_present = summary.get("txDeltaPresent", 0)

print(
    f"gate_counts sent={sent} landed={landed} positionEligible={position_eligible} "
    f"txDeltaPresent={tx_delta_present} minSent={min_sent} minPositionEligible={min_position}"
)
if not sample_gate.get("ok"):
    print(f"gate_wait {sample_gate.get('reason', f'sent rows {sent} below required {min_sent}')}", file=sys.stderr)
    raise SystemExit(1)
if not tx_delta_gate.get("ok"):
    print(
        f"gate_wait {tx_delta_gate.get('reason', f'txDelta-covered rows {tx_delta_present} below required {min_position}')}",
        file=sys.stderr,
    )
    raise SystemExit(1)
PY
  local gate_rc=$?
  set -e
  rm -f "$json_path"
  return "$gate_rc"
}

status() {
  ensure_worker_dir
  load_env_file "$MARKER_FILE"
  local name="${CANARY_NAME:-none}"
  local since="${CANARY_STARTED_AT_ISO:-}"
  local next=""
  next="$(next_after "$name" 2>/dev/null || true)"
  echo "current=${name}"
  echo "current_since=${since}"
  echo "baseline_since=${CANARY_BASELINE_STARTED_AT_ISO:-}"
  echo "next=${next:-done}"
  echo "sequence=${SEQUENCE[*]}"
  echo "min_sent=$MIN_SENT"
  echo "min_position_eligible=$MIN_POSITION_ELIGIBLE"
  if [[ -n "$since" ]]; then
    set +e
    score_current "$since"
    local score_rc=$?
    gate_current_window "$since"
    local gate_rc=$?
    set -e
    echo "score_exit=$score_rc"
    echo "gate_exit=$gate_rc"
  fi
}

advance_next() {
  ensure_worker_dir
  load_env_file "$MARKER_FILE"
  local name="${CANARY_NAME:-}"
  local since="${CANARY_STARTED_AT_ISO:-}"
  local next=""

  if [[ -z "$name" || -z "$since" ]]; then
    echo "No active marker; marking baseline first." >&2
    ./landing-canary-control.sh mark baseline
    return 0
  fi

  echo "Scoring current canary '$name' since $since before advancing..." >&2
  set +e
  score_current "$since"
  local score_rc=$?
  set -e
  echo "score_exit=$score_rc"
  gate_current_window "$since"

  if [[ "$name" != "baseline" ]]; then
    local until_iso
    until_iso="$(current_utc_iso)"
    echo "Comparison for completed canary '$name' ending $until_iso:" >&2
    ./landing-canary-control.sh compare "" "$since" "$until_iso" || true
  fi

  next="$(next_after "$name" 2>/dev/null || true)"
  if [[ -z "$next" ]]; then
    echo "sequence_complete=$name"
    return 0
  fi

  if [[ "$next" == "tip-rotated" ]]; then
    load_env_file "${JITO_APP_ENV_FILE:-/opt/pumpfun-migration-bot/.env}"
    load_env_file "${JITO_WORKER_ENV_FILE:-/etc/jito-copy-live.env}"
    if [[ -z "${JITO_CANARY_HELIUS_TIP_ACCOUNTS:-}" ]]; then
      echo "Cannot apply tip-rotated: JITO_CANARY_HELIUS_TIP_ACCOUNTS is not configured." >&2
      exit 2
    fi
  fi

  echo "applying_next=$next"
  JITO_CANARY_SKIP_BASELINE_GATE=YES ./landing-canary-control.sh apply "$next"
}

command="${1:-}"
case "$command" in
  status)
    status
    ;;
  next)
    advance_next
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac
