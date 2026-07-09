#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROXY_BIN="${JITO_SHREDSTREAM_PROXY_BIN:-/usr/local/bin/jito-shredstream-proxy}"
GRPC_ONLY="$(printf '%s' "${JITO_SHREDSTREAM_GRPC_ONLY:-true}" | tr '[:upper:]' '[:lower:]')"
PREFLIGHT_MODE="$(printf '%s' "${JITO_SHREDSTREAM_UDP_PREFLIGHT_MODE:-require}" | tr '[:upper:]' '[:lower:]')"
PREPARE_SCRIPT="${JITO_SHREDSTREAM_UDP_PREPARE_SCRIPT:-$ROOT_DIR/prepare-shredstream-udp.sh}"

if [[ ! -x "$PROXY_BIN" ]]; then
  echo "ShredStream proxy binary not found or not executable: $PROXY_BIN" >&2
  exit 1
fi

case "$GRPC_ONLY" in
  true|yes|1|on)
    for arg in "$@"; do
      case "$arg" in
        --dest-ip-ports|--dest-ip-ports=*)
          echo "raw UDP forwarding is forbidden when JITO_SHREDSTREAM_GRPC_ONLY=true" >&2
          exit 1
          ;;
      esac
    done
    # DEST_IP_PORTS used to point at an unbound 127.0.0.1:8001. The worker
    # consumes the local gRPC stream, so leaving this unset avoids UDP NoPorts
    # and ICMP churn without changing the gRPC contract.
    unset DEST_IP_PORTS
    ;;
  false|no|0|off)
    if [[ -z "${DEST_IP_PORTS:-}" ]]; then
      echo "DEST_IP_PORTS must be set when JITO_SHREDSTREAM_GRPC_ONLY=false" >&2
      exit 1
    fi
    ;;
  *)
    echo "JITO_SHREDSTREAM_GRPC_ONLY must be true or false; got ${JITO_SHREDSTREAM_GRPC_ONLY:-}" >&2
    exit 1
    ;;
esac

case "$PREFLIGHT_MODE" in
  require)
    "$PREPARE_SCRIPT" check
    ;;
  warn)
    if ! "$PREPARE_SCRIPT" check; then
      echo "warning: ShredStream UDP receive buffers are below the requested size" >&2
    fi
    ;;
  off)
    ;;
  *)
    echo "JITO_SHREDSTREAM_UDP_PREFLIGHT_MODE must be require, warn, or off; got $PREFLIGHT_MODE" >&2
    exit 1
    ;;
esac

exec "$PROXY_BIN" "$@"
