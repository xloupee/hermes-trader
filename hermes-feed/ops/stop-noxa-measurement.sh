#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$BASE_DIR/ops/noxa-observer-common.sh"
umask 077

readonly STATE_DIR="$(canonical_runtime_path \
  "${HERMES_NOXA_MEASUREMENT_STATE_DIR:-$WORKTREE_ROOT/.runtime/hermes-noxa-measurement}" \
  "measurement state directory")"
[[ -d "$STATE_DIR" ]] || {
  echo "Hermes NOXA measurement is not running"
  exit 0
}
readonly PID_FILE="$(canonical_child_path \
  "$STATE_DIR/measurement.pid" "$STATE_DIR" "measurement PID file")"
readonly LOCK_FILE="$(canonical_child_path \
  "$STATE_DIR/measurement.lock" "$STATE_DIR" "measurement lock file")"
exec 9>"$LOCK_FILE"
flock -x 9

if ! read -r PID PGID STARTTIME < <(read_pid_record "$PID_FILE") \
    || ! pid_record_is_live "$PID" "$PGID" "$STARTTIME"; then
  rm -f -- "$PID_FILE"
  echo "Hermes NOXA measurement is not running"
  exit 0
fi
readonly PID PGID STARTTIME
kill -- "-$PGID"
for _ in {1..50}; do
  pid_record_is_live "$PID" "$PGID" "$STARTTIME" || break
  sleep 0.1
done
if pid_record_is_live "$PID" "$PGID" "$STARTTIME"; then
  kill -KILL -- "-$PGID"
  for _ in {1..20}; do
    pid_record_is_live "$PID" "$PGID" "$STARTTIME" || break
    sleep 0.1
  done
fi
if pid_record_is_live "$PID" "$PGID" "$STARTTIME"; then
  echo "Hermes NOXA measurement PID $PID survived TERM and KILL; retaining $PID_FILE" >&2
  exit 1
fi
rm -f -- "$PID_FILE"
echo "Stopped Hermes NOXA measurement"
