#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$BASE_DIR/ops/noxa-observer-common.sh"
umask 077

readonly STATE_DIR="$(canonical_runtime_path \
  "${HERMES_NOXA_STATE_DIR:-$WORKTREE_ROOT/.runtime/hermes-noxa}" "state directory")"

if [[ ! -d "$STATE_DIR" ]]; then
  echo "Hermes NOXA observer is not running"
  exit 0
fi
readonly PID_FILE="$(canonical_child_path \
  "$STATE_DIR/observer.pid" "$STATE_DIR" "PID file")"
readonly LOCK_FILE="$(canonical_child_path \
  "$STATE_DIR/observer.lock" "$STATE_DIR" "lock file")"
exec 9>"$LOCK_FILE"
flock -x 9

if ! read -r PID PGID STARTTIME < <(read_pid_record "$PID_FILE") \
    || ! pid_record_is_live "$PID" "$PGID" "$STARTTIME"; then
  rm -f -- "$PID_FILE"
  echo "Hermes NOXA observer is not running"
  exit 0
fi
readonly PID PGID STARTTIME

kill -- "-$PGID"
for _ in {1..50}; do
  pid_record_is_live "$PID" "$PGID" "$STARTTIME" || break
  sleep 0.1
done
if pid_record_is_live "$PID" "$PGID" "$STARTTIME"; then
  echo "Hermes NOXA observer PID $PID did not stop; retaining $PID_FILE" >&2
  exit 1
fi
rm -f -- "$PID_FILE"
echo "Stopped Hermes NOXA observer"
