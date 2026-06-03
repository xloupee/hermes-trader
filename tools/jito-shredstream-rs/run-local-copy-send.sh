#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi

: "${SOLANA_RPC_URL:?SOLANA_RPC_URL must be set in the environment or .env}"
: "${JITO_ARM_LIVE_COPY_SEND:?set JITO_ARM_LIVE_COPY_SEND=YES to allow this local live-send harness}"

if [[ "$JITO_ARM_LIVE_COPY_SEND" != "YES" ]]; then
  echo "JITO_ARM_LIVE_COPY_SEND must be exactly YES" >&2
  exit 1
fi

export JITO_SHREDSTREAM_PROXY_URL="${JITO_SHREDSTREAM_PROXY_URL:-http://127.0.0.1:9999}"
export SHREDSTREAM_TARGET_WALLETS="${SHREDSTREAM_TARGET_WALLETS:-A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS}"
export JITO_COPY_WALLET="${JITO_COPY_WALLET:-FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W}"
export JITO_COPY_KEYPAIR_PATH="${JITO_COPY_KEYPAIR_PATH:-$HOME/.config/solana/copytrade-planning-keypair.json}"
export JITO_MAX_COPY_SOL="${JITO_MAX_COPY_SOL:-0.001}"
export JITO_SIMULATE_COPY_TX=true
export JITO_ENABLE_COPY_SEND=true
export JITO_DRY_RUN=false
export JITO_COPY_EXECUTIONS_PATH="${JITO_COPY_EXECUTIONS_PATH:-/tmp/jito-copy-executions-local-send.jsonl}"
export JITO_UNSIGNED_TX_PLANS_PATH="${JITO_UNSIGNED_TX_PLANS_PATH:-/tmp/jito-unsigned-tx-plans-local-send.jsonl}"
export JITO_COPY_TX_PLANS_PATH="${JITO_COPY_TX_PLANS_PATH:-/tmp/jito-copy-tx-plans-local-send.jsonl}"
export JITO_EXECUTION_PLANS_PATH="${JITO_EXECUTION_PLANS_PATH:-/tmp/jito-execution-plans-local-send.jsonl}"
export JITO_SHADOW_SIGNALS_PATH="${JITO_SHADOW_SIGNALS_PATH:-/tmp/jito-shadow-signals-local-send.jsonl}"
export JITO_ADDRESS_LOOKUP_TABLES="${JITO_ADDRESS_LOOKUP_TABLES:-4vX5U9XsiY11infmC13d6VFPjvUqtuRw744r4o94dyow}"

case "$JITO_MAX_COPY_SOL" in
  0.001|0.000[0-9]*)
    ;;
  *)
    echo "JITO_MAX_COPY_SOL must be 0.001 or lower for first live send; got $JITO_MAX_COPY_SOL" >&2
    exit 1
    ;;
esac

echo "LOCAL LIVE COPY SEND IS ARMED"
echo "  proxy: $JITO_SHREDSTREAM_PROXY_URL"
echo "  target: $SHREDSTREAM_TARGET_WALLETS"
echo "  copy wallet: $JITO_COPY_WALLET"
echo "  max copy SOL: $JITO_MAX_COPY_SOL"
echo "  executions: $JITO_COPY_EXECUTIONS_PATH"
echo "  send enabled: $JITO_ENABLE_COPY_SEND"
echo "  dry run: $JITO_DRY_RUN"

cargo run --manifest-path tools/jito-shredstream-rs/Cargo.toml -- live --print-mentions
