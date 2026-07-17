#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$BASE_DIR/ops/noxa-observer-common.sh"
umask 077

readonly STATE_DIR="$(canonical_runtime_path \
  "${HERMES_NOXA_MEASUREMENT_STATE_DIR:-$WORKTREE_ROOT/.runtime/hermes-noxa-measurement}" \
  "measurement state directory")"
readonly SOURCE_BIN="$BASE_DIR/target/release/hermes-noxa"
mkdir -p "$STATE_DIR"
readonly PID_FILE="$(canonical_child_path \
  "$STATE_DIR/measurement.pid" "$STATE_DIR" "measurement PID file")"
readonly LOCK_FILE="$(canonical_child_path \
  "$STATE_DIR/measurement.lock" "$STATE_DIR" "measurement lock file")"
exec 9>"$LOCK_FILE"
flock -x 9

if [[ -s "$PID_FILE" ]]; then
  if read -r existing_pid existing_pgid existing_starttime \
      < <(read_pid_record "$PID_FILE") \
      && pid_record_is_live "$existing_pid" "$existing_pgid" "$existing_starttime"; then
    echo "Hermes NOXA measurement is already running as PID $existing_pid" >&2
    exit 1
  fi
  rm -f -- "$PID_FILE"
fi
[[ -x "$SOURCE_BIN" ]] || {
  echo "Missing release binary: $SOURCE_BIN" >&2
  exit 1
}

readonly RUN_ID="$(date --utc +%Y%m%dT%H%M%SZ)-$$"
readonly RUN_DIR="$(canonical_child_path \
  "$STATE_DIR/runs/$RUN_ID" "$STATE_DIR" "measurement run directory")"
mkdir -p "$RUN_DIR"
ln -sfn "$RUN_DIR" "$STATE_DIR/current"

readonly BIN="$(canonical_child_path \
  "$RUN_DIR/hermes-noxa" "$RUN_DIR" "immutable measurement binary")"
install --mode=0500 -- "$SOURCE_BIN" "$BIN"
sha256sum "$BIN" >"$RUN_DIR/binary.sha256"
{
  echo "started_utc=$(date --utc +%Y-%m-%dT%H:%M:%SZ)"
  echo "duration_seconds=${HERMES_NOXA_MEASUREMENT_SECONDS:-7200}"
  echo "status_interval_seconds=${HERMES_NOXA_STATUS_INTERVAL_SECONDS:-30}"
  echo "l1_provider=publicnode.com"
  echo "observer_run=$(readlink -f "$WORKTREE_ROOT/.runtime/hermes-noxa/current" 2>/dev/null || true)"
} >"$RUN_DIR/manifest"

HERMES_NOXA_MEASUREMENT_RUN_DIR="$RUN_DIR" \
HERMES_NOXA_MEASUREMENT_PID_FILE="$PID_FILE" \
HERMES_NOXA_BIN="$BIN" \
nohup setsid --fork \
  "$BASE_DIR/ops/run-noxa-measurement.sh" \
  >"$RUN_DIR/supervisor.stdout" \
  2>"$RUN_DIR/supervisor.stderr" \
  </dev/null 9>&- &

for _ in {1..100}; do
  [[ -s "$PID_FILE" ]] && break
  sleep 0.02
done
if ! read -r PID PGID STARTTIME < <(read_pid_record "$PID_FILE") \
    || ! pid_record_is_live "$PID" "$PGID" "$STARTTIME"; then
  echo "NOXA measurement did not publish a live supervisor identity" >&2
  exit 1
fi

echo "Started Hermes NOXA measurement as PID $PID"
echo "Run directory: $RUN_DIR"
