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
export JITO_CANARY_ASTRALANE_URLS="https://lim.gateway.astralane.io/irisb"
export JITO_CANARY_ASTRALANE_API_KEY="test-astralane-key"
export JITO_CANARY_ASTRALANE_TIP_LAMPORTS=1000000
export JITO_CANARY_ASTRALANE_TIP_ACCOUNT="astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm"
export JITO_CANARY_ASTRALANE_TIP_ACCOUNTS="astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm,astra9xWY93QyfG6yM8zwsKsRodscjQ2uU2HKNL5prk"
export JITO_CANARY_HELIUS_REGION_URLS="http://fra-sender.helius-rpc.com?api-key=test,http://ams-sender.helius-rpc.com?api-key=test,http://lon-sender.helius-rpc.com?api-key=test,http://ewr-sender.helius-rpc.com?api-key=test,http://slc-sender.helius-rpc.com?api-key=test"

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
assert_marker CANARY_ASTRALANE_TIP_ACCOUNT_CONFIGURED true
assert_marker CANARY_ASTRALANE_TIP_ACCOUNTS_CONFIGURED true
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1000000

"$CONTROL" mark helius-astralane-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-astralane-stack
assert_marker CANARY_HELIUS_TIP_LAMPORTS 387500
assert_marker CANARY_ASTRALANE_ENABLED true
assert_marker CANARY_NOZOMI_ENABLED false
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1387500

"$CONTROL" mark helius-nozomi-astralane-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE helius-nozomi-astralane-stack
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_ASTRALANE_ENABLED true
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 2387500

"$CONTROL" mark all-non-beam-stack 2026-06-25T00:00:00Z >/dev/null 2>/dev/null
assert_marker CANARY_SEND_LANE_MODE all-non-beam-stack
assert_marker CANARY_NOZOMI_ENABLED true
assert_marker CANARY_ASTRALANE_ENABLED false
assert_marker CANARY_BEAM_ENABLED false
assert_marker CANARY_MAX_PROVIDER_TIP_LAMPORTS 1387500

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
chmod +x "$MOCK_BIN/systemctl" "$MOCK_BIN/sleep" "$MOCK_BIN/date"
PATH="$MOCK_BIN:$PATH" JITO_CANARY_SKIP_BASELINE_GATE=YES "$CONTROL" apply helius-regional-fanout >/dev/null
assert_env JITO_SEND_LANE_MODE helius-sender-only
assert_env JITO_HELIUS_SENDER_ENABLED true
assert_env JITO_HELIUS_SENDER_URLS "$JITO_CANARY_HELIUS_REGION_URLS"
assert_env JITO_NOZOMI_ENABLED false
assert_env JITO_ASTRALANE_ENABLED false
assert_env JITO_BEAM_ENABLED false

echo "landing canary control tests passed"
