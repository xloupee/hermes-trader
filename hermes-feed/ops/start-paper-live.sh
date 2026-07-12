#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly STATE_DIR="${HERMES_STATE_DIR:-$BASE_DIR/../.runtime/hermes-live}"
readonly PID_FILE="$STATE_DIR/paper-live.pid"

mkdir -p "$STATE_DIR"
umask 077

if [[ -s "$PID_FILE" ]] && kill -0 "$(<"$PID_FILE")" 2>/dev/null; then
  echo "Hermes paper runtime is already running as PID $(<"$PID_FILE")" >&2
  exit 1
fi

readonly RUN_ID="$(date --utc +%Y%m%dT%H%M%SZ)"
readonly RUN_DIR="$STATE_DIR/runs/$RUN_ID"
mkdir -p "$RUN_DIR"
ln -sfn "$RUN_DIR" "$STATE_DIR/current"

sha256sum "$BASE_DIR/target/release/hermes-feed" >"$RUN_DIR/binary.sha256"
{
  echo "started_utc=$(date --utc +%Y-%m-%dT%H:%M:%SZ)"
  echo "mode=paper_only"
  echo "feed=wss://feed.mainnet.chain.robinhood.com"
  echo "router=${HERMES_V2_ROUTER:-0x89e5db8b5aa49aa85ac63f691524311aeb649eba}"
  echo "watch=${HERMES_WATCH_ADDRESS:-all_supported_v2_senders}"
  echo "max_amount_in=${HERMES_MAX_AMOUNT_IN:-10000000000000000}"
} >"$RUN_DIR/manifest"

HERMES_RUN_DIR="$RUN_DIR" nohup setsid prlimit \
  --as=805306368 \
  --fsize=1073741824 \
  --nproc=64 \
  --nofile=1024 \
  -- nice -n 10 \
  "$BASE_DIR/ops/run-paper-live.sh" \
  >"$RUN_DIR/supervisor.stdout" \
  2>"$RUN_DIR/supervisor.stderr" \
  </dev/null &

printf '%s\n' "$!" >"$PID_FILE"
echo "Started Hermes paper runtime as PID $(<"$PID_FILE")"
echo "Run directory: $RUN_DIR"
