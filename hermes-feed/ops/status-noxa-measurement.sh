#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$BASE_DIR/ops/noxa-observer-common.sh"
umask 077

readonly STATE_DIR="$(canonical_runtime_path \
  "${HERMES_NOXA_MEASUREMENT_STATE_DIR:-$WORKTREE_ROOT/.runtime/hermes-noxa-measurement}" \
  "measurement state directory")"
[[ -d "$STATE_DIR" ]] || {
  echo "Hermes NOXA measurement has not started"
  exit 1
}
readonly PID_FILE="$(canonical_child_path \
  "$STATE_DIR/measurement.pid" "$STATE_DIR" "measurement PID file")"
readonly LOCK_FILE="$(canonical_child_path \
  "$STATE_DIR/measurement.lock" "$STATE_DIR" "measurement lock file")"
exec 9>"$LOCK_FILE"
flock -s 9

readonly RUNS_DIR="$(canonical_child_path "$STATE_DIR/runs" "$STATE_DIR" "runs directory")"
readonly RUN_DIR="$(canonical_child_path \
  "$(readlink -f "$STATE_DIR/current")" "$RUNS_DIR" "current run directory")"
if read -r PID PGID STARTTIME < <(read_pid_record "$PID_FILE") \
    && pid_record_is_live "$PID" "$PGID" "$STARTTIME"; then
  echo "running pid=$PID"
  ps -o pid,ppid,pgid,psr,ni,%cpu,%mem,etime,cmd -p "$PID"
else
  echo "not_running"
fi
echo "run_dir=$RUN_DIR"
for file in boundary.jsonl factory-status.jsonl measurement-restarts.log \
    boundary.stderr factory-status.stderr supervisor.stderr; do
  if [[ -f "$RUN_DIR/$file" ]]; then
    printf '%s lines=' "$file"
    wc -l <"$RUN_DIR/$file"
  fi
done
[[ ! -f "$RUN_DIR/completed" ]] || cat "$RUN_DIR/completed"
