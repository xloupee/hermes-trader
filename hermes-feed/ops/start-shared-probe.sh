#!/usr/bin/env bash
set -euo pipefail

readonly RUN_DIR=/srv/hermes-probe
readonly PID_FILE="$RUN_DIR/fra1-shared.pid"
readonly LOG_FILE="$RUN_DIR/fra1-shared.jsonl"
readonly STDERR_FILE="$RUN_DIR/fra1-shared.stderr"
readonly MANIFEST_FILE="$RUN_DIR/fra1-shared.manifest"

cd "$RUN_DIR"
umask 077

if [[ -s "$PID_FILE" ]] && kill -0 "$(<"$PID_FILE")" 2>/dev/null; then
  echo "Hermes probe is already running as PID $(<"$PID_FILE")" >&2
  exit 1
fi

sha256sum --check hermes-feed.sha256

{
  echo "started_utc=$(date --utc +%Y-%m-%dT%H:%M:%SZ)"
  echo "source=fra1-shared"
  echo "feed=wss://feed.mainnet.chain.robinhood.com"
  echo "cpu=3"
  echo "nice=10"
  echo "memory_limit_bytes=805306368"
  echo "file_size_limit_bytes=1073741824"
  echo "runtime=24h"
  sha256sum hermes-feed
} >"$MANIFEST_FILE"

nohup prlimit \
  --as=805306368 \
  --fsize=1073741824 \
  --nproc=64 \
  --nofile=1024 \
  -- timeout --signal=TERM --kill-after=10s 24h \
  taskset --cpu-list 3 \
  nice -n 10 \
  ./hermes-feed probe \
    --url wss://feed.mainnet.chain.robinhood.com \
    --source fra1-shared \
    --warmup-seconds 10 \
  >"$LOG_FILE" \
  2>"$STDERR_FILE" \
  </dev/null &

printf '%s\n' "$!" >"$PID_FILE"
echo "Started Hermes probe as PID $(<"$PID_FILE")"
