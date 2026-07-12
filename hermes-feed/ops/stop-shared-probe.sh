#!/usr/bin/env bash
set -euo pipefail

readonly PID_FILE=/srv/hermes-probe/fra1-shared.pid

if [[ ! -s "$PID_FILE" ]]; then
  echo "No Hermes PID file"
  exit 0
fi

readonly PID="$(<"$PID_FILE")"
if kill -0 "$PID" 2>/dev/null; then
  kill --signal TERM "$PID"
  echo "Requested stop for Hermes PID $PID"
else
  echo "Hermes PID $PID is not running"
fi
