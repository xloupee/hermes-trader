#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNNER="$ROOT_DIR/run-vps-copy-send.sh"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

MOCK_WORKER="$TMP_DIR/mock-worker"
cat > "$MOCK_WORKER" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$JITO_SEND_LANE_MODE" > "$MOCK_WORKER_MODE_PATH"
exit 0
SH
chmod +x "$MOCK_WORKER"
touch "$TMP_DIR/copy-keypair.json"

base_env=(
  JITO_APP_ENV_FILE="$TMP_DIR/missing-app.env"
  JITO_WORKER_ENV_FILE="$TMP_DIR/missing-worker.env"
  JITO_WORKER_DIR="$TMP_DIR"
  JITO_WORKER_BIN="$MOCK_WORKER"
  MOCK_WORKER_MODE_PATH="$TMP_DIR/mode.out"
  JITO_ARM_LIVE_COPY_SEND=YES
  JITO_COPY_KEYPAIR_PATH="$TMP_DIR/copy-keypair.json"
  SOLANA_RPC_URL=https://rpc.example
  JITO_PRIORITY_FEE_MICRO_LAMPORTS=500000
  JITO_SEND_FANOUT=YES
  JITO_FAST_COPY_SEND=YES
  JITO_HELIUS_SENDER_ENABLED=YES
  JITO_HELIUS_SENDER_URLS=https://sender.helius-rpc.com
  JITO_HELIUS_SENDER_TIP_LAMPORTS=200000
  JITO_HELIUS_SENDER_TIP_ACCOUNT=FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W
  JITO_NOZOMI_ENABLED=YES
  JITO_NOZOMI_URLS=https://nozomi.example.com
  JITO_NOZOMI_TIP_LAMPORTS=1000000
  JITO_NOZOMI_TIP_ACCOUNT=FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W
  JITO_ASTRALANE_ENABLED=YES
  JITO_ASTRALANE_URLS=https://lim.gateway.astralane.io/irisb
  JITO_ASTRALANE_API_KEY=astralane-key
  JITO_ASTRALANE_TIP_LAMPORTS=1000000
  JITO_ASTRALANE_TIP_ACCOUNT=FqhpPL63symHForRGfxPbGi4wDpe5jQqAVjntbbBqA5W
  JITO_LUNAR_LANDER_ENABLED=YES
  JITO_LUNAR_LANDER_URLS=http://fra.lunar-lander.hellomoon.io/send-bin
  JITO_LUNAR_LANDER_API_KEY=lunar-key
  JITO_LUNAR_LANDER_TIP_LAMPORTS=1000000
  JITO_LUNAR_LANDER_TIP_ACCOUNT=moon17L6BgxXRX5uHKudAmqVF96xia9h8ygcmG2sL3F
  JITO_ZERO_SLOT_ENABLED=YES
  JITO_ZERO_SLOT_URLS=https://ny.0slot.trade
  JITO_ZERO_SLOT_API_KEY=zero-slot-key
  JITO_ZERO_SLOT_TIP_LAMPORTS=1000000
  JITO_ZERO_SLOT_TIP_ACCOUNTS=4ACfpUFoaSD9bfPdeu6DBt89gB6ENTeHBXCAi87NhDEE
  JITO_TPU_JET_ENABLED=YES
  JITO_TPU_JET_RPC_URL=https://jet-rpc.example.com
  JITO_TPU_JET_WS_URL=https://jet-grpc.example.com
  JITO_TPU_JET_SIDECAR_URL=http://127.0.0.1:8787
)

run_ok() {
  local mode="$1"
  rm -f "$TMP_DIR/mode.out"
  env "${base_env[@]}" JITO_SEND_LANE_MODE="$mode" "$RUNNER" >/dev/null
  if [[ "$(cat "$TMP_DIR/mode.out")" != "$mode" ]]; then
    echo "expected worker to receive JITO_SEND_LANE_MODE=$mode" >&2
    exit 1
  fi
}

run_ok fast
run_ok turbo

if env "${base_env[@]}" JITO_SEND_LANE_MODE=fast JITO_NOZOMI_ENABLED=false "$RUNNER" >/dev/null 2>"$TMP_DIR/fast.err"; then
  echo "fast should require JITO_NOZOMI_ENABLED=YES" >&2
  exit 1
fi
if ! grep -q "JITO_SEND_LANE_MODE=fast requires JITO_NOZOMI_ENABLED=YES" "$TMP_DIR/fast.err"; then
  echo "missing expected fast validation error" >&2
  cat "$TMP_DIR/fast.err" >&2
  exit 1
fi

if env "${base_env[@]}" JITO_SEND_LANE_MODE=turbo JITO_TPU_JET_ENABLED=false "$RUNNER" >/dev/null 2>"$TMP_DIR/turbo.err"; then
  echo "turbo should require JITO_TPU_JET_ENABLED=YES" >&2
  exit 1
fi
if ! grep -q "JITO_SEND_LANE_MODE=turbo requires JITO_TPU_JET_ENABLED=YES" "$TMP_DIR/turbo.err"; then
  echo "missing expected turbo validation error" >&2
  cat "$TMP_DIR/turbo.err" >&2
  exit 1
fi

echo "run-vps-copy-send preflight tests passed"
