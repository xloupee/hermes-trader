#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly STATE_DIR="${HERMES_SHADOW_STATE_DIR:-$BASE_DIR/../.runtime/hermes-shadow}"
readonly PID_FILE="$STATE_DIR/shadow.pid"

mkdir -p "$STATE_DIR"
umask 077
if [[ -s "$PID_FILE" ]] && kill -0 "$(<"$PID_FILE")" 2>/dev/null; then
  echo "Hermes reserve shadow is already running as PID $(<"$PID_FILE")" >&2
  exit 1
fi

readonly RUN_ID="$(date --utc +%Y%m%dT%H%M%SZ)"
readonly RUN_DIR="$STATE_DIR/runs/$RUN_ID"
mkdir -p "$RUN_DIR"
ln -sfn "$RUN_DIR" "$STATE_DIR/current"
install -m 0755 "$BASE_DIR/target/release/hermes-feed" "$RUN_DIR/hermes-feed"
sha256sum "$RUN_DIR/hermes-feed" >"$RUN_DIR/binary.sha256"
{
  echo "started_utc=$(date --utc +%Y-%m-%dT%H:%M:%SZ)"
  echo "mode=reserve_aware_shadow"
  echo "confirmations=2"
  echo "max_amount_in=${HERMES_MAX_AMOUNT_IN:-10000000000000000}"
  echo "source_feed=${HERMES_SOURCE_FEED:-$BASE_DIR/../.runtime/hermes-live/current/feed.jsonl}"
} >"$RUN_DIR/manifest"

HERMES_SHADOW_RUN_DIR="$RUN_DIR" nohup setsid prlimit \
  --as=1073741824 --fsize=2147483648 --nproc=64 --nofile=1024 -- \
  nice -n 10 "$BASE_DIR/ops/run-shadow-live.sh" \
  >"$RUN_DIR/supervisor.stdout" 2>"$RUN_DIR/supervisor.stderr" </dev/null &

printf '%s\n' "$!" >"$PID_FILE"
echo "Started Hermes reserve shadow as PID $(<"$PID_FILE")"
echo "Run directory: $RUN_DIR"
