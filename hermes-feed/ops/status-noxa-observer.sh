#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$BASE_DIR/ops/noxa-observer-common.sh"
umask 077

readonly STATE_DIR="$(canonical_runtime_path \
  "${HERMES_NOXA_STATE_DIR:-$WORKTREE_ROOT/.runtime/hermes-noxa}" "state directory")"

if [[ ! -d "$STATE_DIR" ]]; then
  echo "Hermes NOXA observer is not running"
  exit 1
fi
readonly PID_FILE="$(canonical_child_path \
  "$STATE_DIR/observer.pid" "$STATE_DIR" "PID file")"
readonly LOCK_FILE="$(canonical_child_path \
  "$STATE_DIR/observer.lock" "$STATE_DIR" "lock file")"
exec 9>"$LOCK_FILE"
flock -s 9

if ! read -r PID PGID STARTTIME < <(read_pid_record "$PID_FILE") \
    || ! pid_record_is_live "$PID" "$PGID" "$STARTTIME"; then
  echo "Hermes NOXA observer is not running"
  exit 1
fi
readonly PID PGID STARTTIME
readonly RUNS_DIR="$(canonical_child_path "$STATE_DIR/runs" "$STATE_DIR" "runs directory")"
readonly RUN_DIR="$(canonical_child_path \
  "$(readlink -f "$STATE_DIR/current")" "$RUNS_DIR" "current run directory")"
echo "running pid=$PID"
ps -o pid,ppid,pgid,psr,ni,%cpu,%mem,etime,cmd -p "$PID"
echo "run_dir=$RUN_DIR"
for file in events.jsonl restarts.log observer.stderr supervisor.stderr; do
  if [[ -f "$RUN_DIR/$file" ]]; then
    printf '%s lines=' "$file"
    wc -l <"$RUN_DIR/$file"
  fi
done
