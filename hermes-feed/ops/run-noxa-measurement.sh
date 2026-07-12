#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$BASE_DIR/ops/noxa-observer-common.sh"
umask 077

readonly BIN="$BASE_DIR/target/release/hermes-noxa"
readonly RUN_DIR="$(canonical_runtime_path \
  "${HERMES_NOXA_MEASUREMENT_RUN_DIR:?HERMES_NOXA_MEASUREMENT_RUN_DIR is required}" \
  "measurement run directory")"
readonly PID_FILE="$(canonical_runtime_path \
  "${HERMES_NOXA_MEASUREMENT_PID_FILE:?HERMES_NOXA_MEASUREMENT_PID_FILE is required}" \
  "measurement PID file")"
readonly DURATION_SECONDS="${HERMES_NOXA_MEASUREMENT_SECONDS:-7200}"
readonly STATUS_INTERVAL_SECONDS="${HERMES_NOXA_STATUS_INTERVAL_SECONDS:-30}"
readonly L1_WS_URL="${HERMES_NOXA_L1_WS_URL:-wss://ethereum-rpc.publicnode.com}"

[[ "$DURATION_SECONDS" =~ ^[1-9][0-9]*$ ]] || {
  echo "Measurement duration must be a positive integer" >&2
  exit 1
}
[[ "$STATUS_INTERVAL_SECONDS" =~ ^[1-9][0-9]*$ ]] || {
  echo "Status interval must be a positive integer" >&2
  exit 1
}

mkdir -p "$RUN_DIR"
write_current_pid_record "$PID_FILE"
readonly START_EPOCH="$(date +%s)"
readonly END_EPOCH="$((START_EPOCH + DURATION_SECONDS))"
status_pid=""

cleanup() {
  trap - EXIT INT TERM
  if [[ -n "$status_pid" ]] && kill -0 "$status_pid" 2>/dev/null; then
    kill "$status_pid" 2>/dev/null || true
    wait "$status_pid" 2>/dev/null || true
  fi
  remove_owned_pid_record "$PID_FILE"
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

status_loop() {
  while (( $(date +%s) < END_EPOCH )); do
    if ! "$BIN" status >>"$RUN_DIR/factory-status.jsonl" \
        2>>"$RUN_DIR/factory-status.stderr"; then
      printf '%s status_poll_failed\n' "$(date --utc +%Y-%m-%dT%H:%M:%SZ)" \
        >>"$RUN_DIR/measurement-restarts.log"
    fi
    sleep "$STATUS_INTERVAL_SECONDS"
  done
}
status_loop &
status_pid="$!"

restarts=0
while (( $(date +%s) < END_EPOCH )); do
  remaining="$((END_EPOCH - $(date +%s)))"
  if "$BIN" calibrate-boundary \
      --l1-ws-url "$L1_WS_URL" \
      --warmup-seconds 10 \
      --samples 1000000 \
      --run-seconds "$remaining" \
      >>"$RUN_DIR/boundary.jsonl" \
      2>>"$RUN_DIR/boundary.stderr"; then
    status=0
  else
    status=$?
  fi
  restarts=$((restarts + 1))
  printf '%s boundary_status=%s run=%s\n' \
    "$(date --utc +%Y-%m-%dT%H:%M:%SZ)" "$status" "$restarts" \
    >>"$RUN_DIR/measurement-restarts.log"
  (( $(date +%s) >= END_EPOCH )) && break
  sleep 1
done

wait "$status_pid" 2>/dev/null || true
status_pid=""
printf '%s completed duration_seconds=%s\n' \
  "$(date --utc +%Y-%m-%dT%H:%M:%SZ)" "$DURATION_SECONDS" \
  >"$RUN_DIR/completed"
