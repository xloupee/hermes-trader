#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTROL="$SCRIPT_DIR/landing-canary-control.sh"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

export JITO_CANARY_MARKER_FILE="$TMP_DIR/current.env"
export JITO_APP_ENV_FILE="$TMP_DIR/app.env"
export JITO_WORKER_ENV_FILE="$TMP_DIR/worker.env"
export JITO_WORKER_DIR="$TMP_DIR/worker"
export JITO_CANARY_BACKUP_DIR="$TMP_DIR/backups"
export JITO_CANARY_TPU_QUIC_TIMEOUT_MS=100
export JITO_CANARY_BEAM_TOKEN="test-token"
export JITO_CANARY_BEAM_PROVIDER="bloxroute"
export JITO_CANARY_BEAM_MODE="fastest"
export JITO_CANARY_BEAM_TIP_LAMPORTS=1000000
export JITO_CANARY_BEAM_TIP_ACCOUNTS="rfBP8KJ6KMqvBhmqaV7EoNHVexXQdn1sX4CJ9aLv5w2,rfBkmha9yK5QS7h562Pn6Bfw6cPjsrgVqgcnnXBoXj7"
export JITO_CANARY_ZERO_SLOT_URLS="https://ny.0slot.trade"
export JITO_CANARY_ZERO_SLOT_API_KEY="test-zero-slot-key"
export JITO_CANARY_ZERO_SLOT_TIP_LAMPORTS=1000000
export JITO_CANARY_ZERO_SLOT_TIP_ACCOUNTS="HWEoBxYs7ssKuudEjzjmpfJVX7Dvi7wescFsVx2L5yoY"
export JITO_CANARY_ASTRALANE_API_KEY="test-astralane-key"
export JITO_CANARY_ASTRALANE_TIP_LAMPORTS=1000000
export JITO_CANARY_ASTRALANE_TIP_ACCOUNT="astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm"
export JITO_CANARY_ASTRALANE_TIP_ACCOUNTS="astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm,astra9xWY93QyfG6yM8zwsKsRodscjQ2uU2HKNL5prk"
export JITO_CANARY_LUNAR_LANDER_API_KEY="test-lunar-key"
export JITO_CANARY_LUNAR_LANDER_TIP_LAMPORTS=1000000
export JITO_CANARY_LUNAR_LANDER_TIP_ACCOUNT="moon17L6BgxXRX5uHKudAmqVF96xia9h8ygcmG2sL3F"
export JITO_CANARY_LUNAR_LANDER_TIP_ACCOUNTS="moon17L6BgxXRX5uHKudAmqVF96xia9h8ygcmG2sL3F,moon26g6M87pkyWyg3Uzz3P9dYfnPtnSVwQ4RXrJihDD"
export JITO_CANARY_CIRCULAR_FAST_API_KEY="test-circular-key"
export JITO_CANARY_CIRCULAR_FAST_TIP_LAMPORTS=1000000
export JITO_CANARY_CIRCULAR_FAST_TIP_ACCOUNT="FAST3dMFZvESiEipBvLSiXq3QCV51o3xuoHScqRU6cB6"
export JITO_CANARY_CIRCULAR_FAST_TIP_ACCOUNTS="FAST3dMFZvESiEipBvLSiXq3QCV51o3xuoHScqRU6cB6,FASTHPWR9bCUVZHJofp8Yr5rywxPgZnY6tDKZa2umHLB"
export JITO_CANARY_HELIUS_REGION_URLS="http://fra-sender.helius-rpc.com?api-key=test,http://ams-sender.helius-rpc.com?api-key=test,http://lon-sender.helius-rpc.com?api-key=test,http://ewr-sender.helius-rpc.com?api-key=test,http://slc-sender.helius-rpc.com?api-key=test"
export JITO_CANARY_NOZOMI_URLS="https://nozomi.example.com?c=test-key"
export JITO_CANARY_NOZOMI_API_V2_REGION_HOSTS="ewr1.nozomi.temporal.xyz,pit1.nozomi.temporal.xyz"
export JITO_CANARY_ERPC_SWQOS_URLS="https://swqos.erpc.global"
export JITO_CANARY_ERPC_API_KEY="test-erpc-key"

mkdir -p "$JITO_WORKER_DIR"
: > "$JITO_APP_ENV_FILE"
: > "$JITO_WORKER_ENV_FILE"

assert_marker() {
  local key="$1" expected="$2"
  if ! grep -qx "$key=$expected" "$JITO_CANARY_MARKER_FILE"; then
    echo "expected marker $key=$expected" >&2
    echo "actual marker:" >&2
    cat "$JITO_CANARY_MARKER_FILE" >&2
    exit 1
  fi
}

assert_env() {
  local key="$1" expected="$2"
  if ! grep -qx "$key=$expected" "$JITO_WORKER_ENV_FILE"; then
    echo "expected env $key=$expected" >&2
    echo "actual worker env:" >&2
    cat "$JITO_WORKER_ENV_FILE" >&2
    exit 1
  fi
}

"$CONTROL" mark tpu-quic-current-leader-fanout 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-tpu-quic
assert_marker CANARY_TPU_QUIC_ENABLED true
assert_marker CANARY_TPU_QUIC_FANOUT_SLOTS 1
assert_marker CANARY_TPU_QUIC_TIMEOUT_MS 100

"$CONTROL" mark tpu-quic-current-leader-only 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE tpu-quic-helius-tip
assert_marker CANARY_TPU_QUIC_ENABLED true
assert_marker CANARY_TPU_QUIC_FANOUT_SLOTS 1
assert_marker CANARY_TPU_QUIC_TIMEOUT_MS 100

"$CONTROL" mark tpu-jet-fanout 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-tpu-jet
assert_marker CANARY_TPU_JET_ENABLED true
assert_marker CANARY_TPU_JET_FANOUT_SLOTS 1
assert_marker CANARY_TPU_JET_TIMEOUT_MS 30

"$CONTROL" mark tpu-jet-only 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE tpu-jet-helius-tip
assert_marker CANARY_TPU_JET_ENABLED true
assert_marker CANARY_TPU_JET_FANOUT_SLOTS 1
assert_marker CANARY_TPU_JET_TIMEOUT_MS 30

"$CONTROL" mark helius-sender-max 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-sender-max
assert_marker CANARY_HELIUS_TIP_LAMPORTS 1000000
assert_marker CANARY_HELIUS_SWQOS_ONLY false
assert_marker CANARY_NOZOMI_ENABLED false
assert_marker CANARY_ASTRALANE_ENABLED false
assert_marker CANARY_LUNAR_LANDER_ENABLED false
assert_marker CANARY_CIRCULAR_FAST_ENABLED false
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_ZERO_SLOT_ENABLED false
assert_marker CANARY_TPU_JET_ENABLED false
assert_marker CANARY_TPU_QUIC_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1000000

"$CONTROL" mark nozomi-api-v2-only 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE nozomi-only
assert_marker CANARY_HELIUS_TIP_LAMPORTS 0
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_NOZOMI_URLS_CONFIGURED true
assert_marker CANARY_NOZOMI_URL_COUNT 1
assert_marker CANARY_NOZOMI_API_V2 true

"$CONTROL" mark helius-nozomi-api-v2-regional-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-nozomi-stack
assert_marker CANARY_HELIUS_TIP_LAMPORTS 387500
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_NOZOMI_URLS_CONFIGURED true
assert_marker CANARY_NOZOMI_URL_COUNT 2
assert_marker CANARY_NOZOMI_API_V2 true

"$CONTROL" mark beam-only 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE beam-only
assert_marker CANARY_BEAM_ENABLED true
assert_marker CANARY_BEAM_TOKEN_CONFIGURED true
assert_marker CANARY_BEAM_PROVIDER bloxroute
assert_marker CANARY_BEAM_MODE fastest
assert_marker CANARY_BEAM_TIP_LAMPORTS 1000000
assert_marker CANARY_BEAM_TIP_ACCOUNTS_CONFIGURED true

"$CONTROL" mark helius-beam-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-beam-stack
assert_marker CANARY_HELIUS_TIP_LAMPORTS 387500
assert_marker CANARY_BEAM_ENABLED true
assert_marker CANARY_NOZOMI_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1387500

"$CONTROL" mark helius-nozomi-beam-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-nozomi-beam-stack
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_BEAM_ENABLED true
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 2500000

"$CONTROL" mark astralane-only 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE astralane-only
assert_marker CANARY_ASTRALANE_ENABLED true
assert_marker CANARY_ASTRALANE_URLS_CONFIGURED true
assert_marker CANARY_ASTRALANE_API_KEY_CONFIGURED true
assert_marker CANARY_ASTRALANE_TIP_LAMPORTS 1000000
assert_marker CANARY_ASTRALANE_TIP_ACCOUNTS_CONFIGURED true
assert_marker CANARY_HELIUS_TIP_LAMPORTS 0
assert_marker CANARY_NOZOMI_ENABLED false
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_ZERO_SLOT_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1000000

"$CONTROL" mark helius-astralane-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-astralane-stack
assert_marker CANARY_ASTRALANE_ENABLED true
assert_marker CANARY_HELIUS_TIP_LAMPORTS 387500
assert_marker CANARY_NOZOMI_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1387500

"$CONTROL" mark helius-nozomi-astralane-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-nozomi-astralane-stack
assert_marker CANARY_ASTRALANE_ENABLED true
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_ZERO_SLOT_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 2387500

"$CONTROL" mark helius-nozomi-astralane-lunar-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-nozomi-astralane-lunar-stack
assert_marker CANARY_ASTRALANE_ENABLED true
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_LUNAR_LANDER_ENABLED true
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_ZERO_SLOT_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 3387500

"$CONTROL" mark lunar-lander-only 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE lunar-lander-only
assert_marker CANARY_LUNAR_LANDER_ENABLED true
assert_marker CANARY_LUNAR_LANDER_URLS_CONFIGURED true
assert_marker CANARY_LUNAR_LANDER_API_KEY_CONFIGURED true
assert_marker CANARY_LUNAR_LANDER_TIP_LAMPORTS 1000000
assert_marker CANARY_LUNAR_LANDER_TIP_ACCOUNTS_CONFIGURED true
assert_marker CANARY_HELIUS_TIP_LAMPORTS 0
assert_marker CANARY_NOZOMI_ENABLED false
assert_marker CANARY_ASTRALANE_ENABLED false
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_ZERO_SLOT_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1000000

"$CONTROL" mark helius-lunar-lander-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-lunar-lander-stack
assert_marker CANARY_LUNAR_LANDER_ENABLED true
assert_marker CANARY_HELIUS_TIP_LAMPORTS 387500
assert_marker CANARY_NOZOMI_ENABLED false
assert_marker CANARY_ASTRALANE_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1387500

"$CONTROL" mark circular-fast-only 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE circular-fast-only
assert_marker CANARY_CIRCULAR_FAST_ENABLED true
assert_marker CANARY_CIRCULAR_FAST_URLS_CONFIGURED true
assert_marker CANARY_CIRCULAR_FAST_API_KEY_CONFIGURED true
assert_marker CANARY_CIRCULAR_FAST_TIP_LAMPORTS 1000000
assert_marker CANARY_CIRCULAR_FAST_TIP_ACCOUNTS_CONFIGURED true
assert_marker CANARY_HELIUS_TIP_LAMPORTS 0
assert_marker CANARY_NOZOMI_ENABLED false
assert_marker CANARY_ASTRALANE_ENABLED false
assert_marker CANARY_LUNAR_LANDER_ENABLED false
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_ZERO_SLOT_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1000000

"$CONTROL" mark helius-circular-fast-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-circular-fast-stack
assert_marker CANARY_CIRCULAR_FAST_ENABLED true
assert_marker CANARY_HELIUS_TIP_LAMPORTS 387500
assert_marker CANARY_NOZOMI_ENABLED false
assert_marker CANARY_ASTRALANE_ENABLED false
assert_marker CANARY_LUNAR_LANDER_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1387500

"$CONTROL" mark erpc-swqos-only 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE erpc-swqos-only
assert_marker CANARY_ERPC_SWQOS_ENABLED true
assert_marker CANARY_ERPC_SWQOS_URLS_CONFIGURED true
assert_marker CANARY_HELIUS_TIP_LAMPORTS 0
assert_marker CANARY_NOZOMI_ENABLED false

"$CONTROL" mark helius-erpc-swqos-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-erpc-swqos-stack
assert_marker CANARY_ERPC_SWQOS_ENABLED true
assert_marker CANARY_HELIUS_TIP_LAMPORTS 387500
assert_marker CANARY_NOZOMI_ENABLED false

"$CONTROL" mark zero-slot-only 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE zero-slot-only
assert_marker CANARY_ZERO_SLOT_ENABLED true
assert_marker CANARY_ZERO_SLOT_URLS_CONFIGURED true
assert_marker CANARY_ZERO_SLOT_API_KEY_CONFIGURED true
assert_marker CANARY_ZERO_SLOT_TIP_LAMPORTS 1000000
assert_marker CANARY_ZERO_SLOT_TIP_ACCOUNTS_CONFIGURED true
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1000000

"$CONTROL" mark helius-zero-slot-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-zero-slot-stack
assert_marker CANARY_ZERO_SLOT_ENABLED true
assert_marker CANARY_HELIUS_TIP_LAMPORTS 387500
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1200000

"$CONTROL" mark helius-nozomi-zero-slot-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-nozomi-zero-slot-stack
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_ZERO_SLOT_ENABLED true
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 2200000

"$CONTROL" mark all-non-beam-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE all-non-beam-stack
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_ZERO_SLOT_ENABLED true
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 2387500

if "$CONTROL" mark helius-regional-fanout 2026-06-25T00:00:00Z >/dev/null 2>/dev/null; then
  assert_marker CANARY_SEND_LANE_MODE helius-sender-only
  assert_marker CANARY_HELIUS_URL_COUNT 5
  assert_marker CANARY_HELIUS_REGION_URLS_CONFIGURED true
  assert_marker CANARY_NOZOMI_ENABLED false
  assert_marker CANARY_ASTRALANE_ENABLED false
  assert_marker CANARY_BEAM_ENABLED false
else
  echo "helius-regional-fanout mark failed unexpectedly" >&2
  exit 1
fi

"$CONTROL" mark fast 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE fast
assert_marker CANARY_HELIUS_TIP_LAMPORTS 1000000
assert_marker CANARY_HELIUS_SWQOS_ONLY false
assert_marker CANARY_HELIUS_URL_COUNT 5
assert_marker CANARY_HELIUS_REGION_URLS_CONFIGURED true
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_ASTRALANE_ENABLED false
assert_marker CANARY_LUNAR_LANDER_ENABLED false
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_ZERO_SLOT_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 2000000

"$CONTROL" mark turbo 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE turbo
assert_marker CANARY_HELIUS_URL_COUNT 5
assert_marker CANARY_HELIUS_REGION_URLS_CONFIGURED true
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_ASTRALANE_ENABLED true
assert_marker CANARY_LUNAR_LANDER_ENABLED true
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_ZERO_SLOT_ENABLED true
assert_marker CANARY_TPU_JET_ENABLED true
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 4387500

unset JITO_CANARY_HELIUS_REGION_URLS
if "$CONTROL" mark helius-regional-fanout 2026-06-25T00:00:00Z >/dev/null 2>"$TMP_DIR/missing-helius-regions.err"; then
  echo "helius-regional-fanout should require JITO_CANARY_HELIUS_REGION_URLS" >&2
  exit 1
fi
if ! grep -q "requires JITO_CANARY_HELIUS_REGION_URLS" "$TMP_DIR/missing-helius-regions.err"; then
  echo "missing expected Helius region URL error" >&2
  cat "$TMP_DIR/missing-helius-regions.err" >&2
  exit 1
fi
export JITO_CANARY_HELIUS_REGION_URLS="http://fra-sender.helius-rpc.com?api-key=test,http://ams-sender.helius-rpc.com?api-key=test,http://lon-sender.helius-rpc.com?api-key=test,http://ewr-sender.helius-rpc.com?api-key=test,http://slc-sender.helius-rpc.com?api-key=test"

MOCK_BIN="$TMP_DIR/bin"
mkdir -p "$MOCK_BIN"
cat > "$MOCK_BIN/systemctl" <<'SH'
#!/usr/bin/env bash
case "$1" in
  restart)
    exit 0
    ;;
  is-active)
    if [[ "${2:-}" == "--quiet" ]]; then
      exit 0
    fi
    echo active
    exit 0
    ;;
  show)
    if [[ "$*" == *"ActiveEnterTimestamp"* ]]; then
      echo "Sat 2026-06-27 00:00:00 UTC"
      exit 0
    fi
    exit 0
    ;;
  *)
    exit 0
    ;;
esac
SH
cat > "$MOCK_BIN/sleep" <<'SH'
#!/usr/bin/env bash
exit 0
SH
cat > "$MOCK_BIN/date" <<'SH'
#!/usr/bin/env bash
for arg in "$@"; do
  if [[ "$arg" == "+%Y-%m-%dT%H:%M:%SZ" ]]; then
    echo "2026-06-27T00:00:00Z"
    exit 0
  fi
done
/bin/date "$@"
SH
cat > "$MOCK_BIN/curl" <<'SH'
#!/usr/bin/env bash
exit 0
SH
chmod +x "$MOCK_BIN/systemctl" "$MOCK_BIN/sleep" "$MOCK_BIN/date" "$MOCK_BIN/curl"
PATH="$MOCK_BIN:$PATH" JITO_CANARY_SKIP_BASELINE_GATE=YES "$CONTROL" apply helius-regional-fanout >/dev/null
assert_env JITO_SEND_LANE_MODE helius-sender-only
assert_env JITO_HELIUS_SENDER_ENABLED true
assert_env JITO_HELIUS_SENDER_URLS "$JITO_CANARY_HELIUS_REGION_URLS"
assert_env JITO_NOZOMI_ENABLED false
assert_env JITO_ASTRALANE_ENABLED false
assert_env JITO_LUNAR_LANDER_ENABLED false
assert_env JITO_CIRCULAR_FAST_ENABLED false
assert_env JITO_BEAM_ENABLED false

PATH="$MOCK_BIN:$PATH" JITO_CANARY_SKIP_BASELINE_GATE=YES "$CONTROL" apply fast >/dev/null
assert_env JITO_SEND_LANE_MODE fast
assert_env JITO_HELIUS_SENDER_ENABLED true
assert_env JITO_HELIUS_SENDER_TIP_LAMPORTS 1000000
assert_env JITO_HELIUS_SENDER_SWQOS_ONLY false
assert_env JITO_HELIUS_SENDER_URLS "$JITO_CANARY_HELIUS_REGION_URLS"
assert_env JITO_NOZOMI_ENABLED true
assert_env JITO_ASTRALANE_ENABLED false
assert_env JITO_LUNAR_LANDER_ENABLED false
assert_env JITO_BEAM_ENABLED false
assert_env JITO_ZERO_SLOT_ENABLED false

PATH="$MOCK_BIN:$PATH" JITO_CANARY_SKIP_BASELINE_GATE=YES "$CONTROL" apply circular-fast-only >/dev/null
assert_env JITO_SEND_LANE_MODE circular-fast-only
assert_env JITO_HELIUS_SENDER_ENABLED false
assert_env JITO_CIRCULAR_FAST_ENABLED true
assert_env JITO_CIRCULAR_FAST_URLS https://fra.fast.circular.fi/transactions
assert_env JITO_CIRCULAR_FAST_API_KEY test-circular-key
assert_env JITO_CIRCULAR_FAST_TIP_LAMPORTS 1000000
assert_env JITO_CIRCULAR_FAST_TIP_ACCOUNT FAST3dMFZvESiEipBvLSiXq3QCV51o3xuoHScqRU6cB6
assert_env JITO_CIRCULAR_FAST_TIP_ACCOUNTS "$JITO_CANARY_CIRCULAR_FAST_TIP_ACCOUNTS"
assert_env JITO_CIRCULAR_FAST_FRONT_RUNNING_PROTECTION false

unset JITO_CANARY_CIRCULAR_FAST_API_KEY
awk '$0 !~ /^JITO_CIRCULAR_FAST_API_KEY=/' "$JITO_WORKER_ENV_FILE" > "$TMP_DIR/worker.no-circular-key"
mv "$TMP_DIR/worker.no-circular-key" "$JITO_WORKER_ENV_FILE"
if PATH="$MOCK_BIN:$PATH" JITO_CANARY_SKIP_BASELINE_GATE=YES "$CONTROL" apply circular-fast-only >/dev/null 2>"$TMP_DIR/missing-circular-key.err"; then
  echo "circular-fast-only should require JITO_CANARY_CIRCULAR_FAST_API_KEY" >&2
  exit 1
fi
if ! grep -q "requires JITO_CANARY_CIRCULAR_FAST_API_KEY" "$TMP_DIR/missing-circular-key.err"; then
  echo "missing expected Circular Fast API key error" >&2
  cat "$TMP_DIR/missing-circular-key.err" >&2
  exit 1
fi
export JITO_CANARY_CIRCULAR_FAST_API_KEY="test-circular-key"

export JITO_TPU_JET_RPC_URL="https://rpc.example"
export JITO_TPU_JET_WS_URL="http://grpc-fra1-burst.erpc.global"
export JITO_TPU_JET_SIDECAR_URL="http://127.0.0.1:8787"
PATH="$MOCK_BIN:$PATH" JITO_CANARY_SKIP_BASELINE_GATE=YES "$CONTROL" apply turbo >/dev/null
assert_env JITO_SEND_LANE_MODE turbo
assert_env JITO_HELIUS_SENDER_ENABLED true
assert_env JITO_HELIUS_SENDER_URLS "$JITO_CANARY_HELIUS_REGION_URLS"
assert_env JITO_NOZOMI_ENABLED true
assert_env JITO_ASTRALANE_ENABLED true
assert_env JITO_LUNAR_LANDER_ENABLED true
assert_env JITO_BEAM_ENABLED false
assert_env JITO_ZERO_SLOT_ENABLED true
assert_env JITO_TPU_JET_ENABLED true
assert_env JITO_MAX_PROVIDER_TIP_LAMPORTS 4387500

PATH="$MOCK_BIN:$PATH" JITO_CANARY_SKIP_BASELINE_GATE=YES "$CONTROL" apply tpu-jet-fanout >/dev/null
assert_env JITO_SEND_LANE_MODE helius-tpu-jet
assert_env JITO_TPU_JET_ENABLED true
assert_env JITO_TPU_JET_FANOUT_SLOTS 1
assert_env JITO_TPU_JET_TIMEOUT_MS 30

echo "landing canary control tests passed"
