#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROXY_BIN="${JITO_SHREDSTREAM_PROXY_BIN:-/usr/local/bin/jito-shredstream-proxy}"
PREFLIGHT="${DOUBLEZERO_SHADOW_PREFLIGHT:-$ROOT_DIR/doublezero-edge-shadow-preflight.sh}"
DEVICE="${DOUBLEZERO_DEVICE:-doublezero1}"
MULTICAST_GROUP="${DOUBLEZERO_MULTICAST_GROUP:-}"
MULTICAST_PORT="${DOUBLEZERO_MULTICAST_PORT:-20001}"
GRPC_PORT="${DOUBLEZERO_SHADOW_GRPC_PORT:-10099}"

if [[ "${DOUBLEZERO_SHADOW_ENABLED:-false}" != "true" ]]; then
  echo "DoubleZero shadow is default-off; set DOUBLEZERO_SHADOW_ENABLED=true only after completing the runbook gates" >&2
  exit 1
fi

if [[ "${DOUBLEZERO_SHADOW_ALLOW_EXECUTION:-false}" != "false" ]]; then
  echo "DoubleZero shadow refuses to start when execution is allowed" >&2
  exit 1
fi

[[ -x "$PROXY_BIN" ]] || {
  echo "proxy binary is missing or not executable: $PROXY_BIN" >&2
  exit 1
}

"$PREFLIGHT"

# Do not inherit a Jito destination or authentication environment. The shadow
# is multicast input -> the existing SubscribeEntries gRPC contract only.
unset DEST_IP_PORTS ENDPOINT_DISCOVERY_URL DISCOVERED_ENDPOINTS_PORT
unset BLOCK_ENGINE_URL AUTH_URL AUTH_KEYPAIR DESIRED_REGIONS

exec "$PROXY_BIN" forward-only \
  --src-bind-port 0 \
  --multicast-bind-ip "$MULTICAST_GROUP" \
  --multicast-device "$DEVICE" \
  --multicast-subscribe-port "$MULTICAST_PORT" \
  --grpc-service-port "$GRPC_PORT"
