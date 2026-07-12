#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$BASE_DIR/ops/noxa-observer-common.sh"
umask 077

readonly STATE_DIR="$(canonical_runtime_path \
  "${HERMES_NOXA_STATE_DIR:-$WORKTREE_ROOT/.runtime/hermes-noxa}" "state directory")"
readonly BIN="$BASE_DIR/target/release/hermes-noxa"

mkdir -p "$STATE_DIR"
readonly PID_FILE="$(canonical_child_path \
  "$STATE_DIR/observer.pid" "$STATE_DIR" "PID file")"
readonly LOCK_FILE="$(canonical_child_path \
  "$STATE_DIR/observer.lock" "$STATE_DIR" "lock file")"
exec 9>"$LOCK_FILE"
flock -x 9

if [[ -s "$PID_FILE" ]]; then
  if read -r existing_pid existing_pgid existing_starttime \
      < <(read_pid_record "$PID_FILE") \
      && pid_record_is_live "$existing_pid" "$existing_pgid" "$existing_starttime"; then
    echo "Hermes NOXA observer is already running as PID $existing_pid" >&2
    exit 1
  fi
  rm -f -- "$PID_FILE"
fi
if [[ ! -x "$BIN" ]]; then
  echo "Missing release binary: $BIN" >&2
  exit 1
fi

readonly RUN_ID="$(date --utc +%Y%m%dT%H%M%SZ)-$$"
readonly RUN_DIR="$(canonical_child_path \
  "$STATE_DIR/runs/$RUN_ID" "$STATE_DIR" "run directory")"
mkdir -p "$RUN_DIR"
ln -sfn "$RUN_DIR" "$STATE_DIR/current"

sha256sum "$BIN" >"$RUN_DIR/binary.sha256"
{
  echo "started_utc=$(date --utc +%Y-%m-%dT%H:%M:%SZ)"
  echo "mode=paper_only_no_signer_no_sender"
  echo "feed=${HERMES_NOXA_FEED_URL:-wss://feed.mainnet.chain.robinhood.com}"
  echo "rpc=${HERMES_NOXA_RPC_URL:-https://rpc.mainnet.chain.robinhood.com}"
  echo "amount_in=${HERMES_NOXA_AMOUNT_IN:-10000000000000000}"
  echo "slippage_bps=${HERMES_NOXA_SLIPPAGE_BPS:-500}"
  echo "recipient=${HERMES_NOXA_RECIPIENT:-not_set_no_calldata}"
} >"$RUN_DIR/manifest"

HERMES_NOXA_RUN_DIR="$RUN_DIR" \
HERMES_NOXA_PID_FILE="$PID_FILE" \
nohup setsid --fork prlimit \
  --as=805306368 \
  --fsize=1073741824 \
  --nproc=64 \
  --nofile=1024 \
  -- nice -n 5 \
  "$BASE_DIR/ops/run-noxa-observer.sh" \
  >"$RUN_DIR/supervisor.stdout" \
  2>"$RUN_DIR/supervisor.stderr" \
  </dev/null 9>&- &

for _ in {1..100}; do
  [[ -s "$PID_FILE" ]] && break
  sleep 0.02
done
if ! read -r PID PGID STARTTIME < <(read_pid_record "$PID_FILE") \
    || ! pid_record_is_live "$PID" "$PGID" "$STARTTIME"; then
  echo "NOXA observer did not publish a live supervisor identity" >&2
  exit 1
fi

echo "Started Hermes NOXA paper observer as PID $PID"
echo "Run directory: $RUN_DIR"
