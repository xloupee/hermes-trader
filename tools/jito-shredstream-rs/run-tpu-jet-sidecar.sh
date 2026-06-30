#!/usr/bin/env bash
set -euo pipefail

APP_ENV_FILE="${JITO_APP_ENV_FILE:-/opt/pumpfun-migration-bot/.env}"
WORKER_ENV_FILE="${JITO_WORKER_ENV_FILE:-/etc/jito-copy-live.env}"
WORKER_DIR="${JITO_WORKER_DIR:-/opt/jito-feed-probe-watch}"
SIDECAR_BIN="${JITO_TPU_JET_SIDECAR_BIN:-$WORKER_DIR/spikes/yellowstone-jet-compat/target/release/yellowstone-jet-sidecar}"

load_env_file() {
  local env_file="$1"
  local line key value

  [[ -f "$env_file" ]] || return 0
  while IFS= read -r line || [[ -n "$line" ]]; do
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
    [[ -z "$line" || "${line:0:1}" == "#" || "$line" != *"="* ]] && continue

    key="${line%%=*}"
    value="${line#*=}"
    [[ "$key" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] || continue

    export "$key=$value"
  done < "$env_file"
}

load_env_file "$APP_ENV_FILE"
load_env_file "$WORKER_ENV_FILE"

export JITO_TPU_JET_RPC_URL="${JITO_TPU_JET_RPC_URL:-${SOLANA_RPC_URL:-}}"
export JITO_TPU_JET_WS_URL="${JITO_TPU_JET_WS_URL:-${JITO_TPU_JET_GRPC_URL:-${JITO_ERPC_YELLOWSTONE_GRPC_URL:-}}}"
export JITO_TPU_JET_GRPC_URL="${JITO_TPU_JET_GRPC_URL:-$JITO_TPU_JET_WS_URL}"
export JITO_TPU_JET_GRPC_X_TOKEN="${JITO_TPU_JET_GRPC_X_TOKEN:-${JITO_ERPC_YELLOWSTONE_GRPC_X_TOKEN:-}}"
export JITO_TPU_JET_SIDECAR_BIND="${JITO_TPU_JET_SIDECAR_BIND:-127.0.0.1:8787}"
export JITO_TPU_JET_FANOUT_SLOTS="${JITO_TPU_JET_FANOUT_SLOTS:-1}"
export JITO_TPU_JET_TIMEOUT_MS="${JITO_TPU_JET_TIMEOUT_MS:-30}"

if [[ -z "${JITO_TPU_JET_RPC_URL:-}" ]]; then
  echo "JITO_TPU_JET_RPC_URL or SOLANA_RPC_URL must be set in $APP_ENV_FILE or $WORKER_ENV_FILE" >&2
  exit 1
fi
if [[ -z "${JITO_TPU_JET_GRPC_URL:-}" ]]; then
  echo "JITO_TPU_JET_GRPC_URL, JITO_TPU_JET_WS_URL, or JITO_ERPC_YELLOWSTONE_GRPC_URL must be set in $APP_ENV_FILE or $WORKER_ENV_FILE" >&2
  exit 1
fi
if [[ ! -x "$SIDECAR_BIN" ]]; then
  echo "Yellowstone Jet sidecar binary not found or not executable: $SIDECAR_BIN" >&2
  exit 1
fi

echo "TPU Jet sidecar starting"
echo "  bind: $JITO_TPU_JET_SIDECAR_BIND"
echo "  rpc url: configured"
echo "  grpc/ws url: configured"
if [[ -n "${JITO_TPU_JET_GRPC_X_TOKEN:-}" ]]; then
  echo "  grpc x-token: configured"
else
  echo "  grpc x-token: unset"
fi
echo "  fanout/lookahead slots: $JITO_TPU_JET_FANOUT_SLOTS"
echo "  send timeout ms: $JITO_TPU_JET_TIMEOUT_MS"

cd "$WORKER_DIR"
exec "$SIDECAR_BIN"
