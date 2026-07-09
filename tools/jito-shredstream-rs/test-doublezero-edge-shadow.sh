#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

cat >"$TMP_DIR/ip" <<'EOF'
#!/usr/bin/env bash
case "$*" in
  "-details link show dev doublezero1") echo '4: doublezero1: <UP,MULTICAST> mtu 1500 state UNKNOWN' ;;
  "-4 address show dev doublezero1") echo '    inet 10.10.10.2/30 scope global doublezero1' ;;
  "route get 233.1.2.3") echo '233.1.2.3 dev doublezero1 src 10.10.10.2' ;;
  *) exit 1 ;;
esac
EOF

cat >"$TMP_DIR/ss" <<'EOF'
#!/usr/bin/env bash
[[ "${TEST_PORT_BUSY:-false}" == "true" ]] && echo 'LISTEN 0 128 127.0.0.1:10099'
exit 0
EOF

cat >"$TMP_DIR/proxy" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >"${TEST_PROXY_ARGS:?}"
EOF

chmod +x "$TMP_DIR/ip" "$TMP_DIR/ss" "$TMP_DIR/proxy"

common_env=(
  IP_BIN="$TMP_DIR/ip"
  SS_BIN="$TMP_DIR/ss"
  DOUBLEZERO_DEVICE=doublezero1
  DOUBLEZERO_MULTICAST_GROUP=233.1.2.3
  DOUBLEZERO_DO_GRE_VALIDATED=true
  DOUBLEZERO_DO_BGP_VALIDATED=true
  DOUBLEZERO_DO_MULTICAST_VALIDATED=true
  DOUBLEZERO_GRPC_ONLY_PROXY_VALIDATED=true
  DOUBLEZERO_SHADOW_ALLOW_EXECUTION=false
)

env "${common_env[@]}" "$ROOT_DIR/doublezero-edge-shadow-preflight.sh" >"$TMP_DIR/pass.out"
grep -q '^PASS:' "$TMP_DIR/pass.out"

if env IP_BIN="$TMP_DIR/ip" SS_BIN="$TMP_DIR/ss" DOUBLEZERO_MULTICAST_GROUP=233.1.2.3 \
  "$ROOT_DIR/doublezero-edge-shadow-preflight.sh" >"$TMP_DIR/gates.out" 2>&1; then
  echo "expected missing DigitalOcean gates to fail" >&2
  exit 1
fi
grep -q 'DOUBLEZERO_DO_GRE_VALIDATED must be explicitly set to true' "$TMP_DIR/gates.out"

if env "${common_env[@]}" DOUBLEZERO_SHADOW_GRPC_PORT=9999 \
  "$ROOT_DIR/doublezero-edge-shadow-preflight.sh" >"$TMP_DIR/port.out" 2>&1; then
  echo "expected primary/shadow port collision to fail" >&2
  exit 1
fi
grep -q 'shadow gRPC port must differ' "$TMP_DIR/port.out"

if env "${common_env[@]}" DOUBLEZERO_SHADOW_ALLOW_EXECUTION=true \
  "$ROOT_DIR/doublezero-edge-shadow-preflight.sh" >"$TMP_DIR/execute.out" 2>&1; then
  echo "expected execution-enabled shadow to fail" >&2
  exit 1
fi
grep -q 'must remain false' "$TMP_DIR/execute.out"

if env "${common_env[@]}" TEST_PORT_BUSY=true \
  "$ROOT_DIR/doublezero-edge-shadow-preflight.sh" >"$TMP_DIR/busy.out" 2>&1; then
  echo "expected busy shadow port to fail" >&2
  exit 1
fi
grep -q 'already listening' "$TMP_DIR/busy.out"

if env "${common_env[@]}" JITO_SHREDSTREAM_PROXY_BIN="$TMP_DIR/proxy" \
  "$ROOT_DIR/run-doublezero-edge-shadow.sh" >"$TMP_DIR/default-off.out" 2>&1; then
  echo "expected default-off launcher to fail" >&2
  exit 1
fi
grep -q 'default-off' "$TMP_DIR/default-off.out"

TEST_PROXY_ARGS="$TMP_DIR/proxy.args" env "${common_env[@]}" \
  TEST_PROXY_ARGS="$TMP_DIR/proxy.args" \
  DOUBLEZERO_SHADOW_ENABLED=true \
  JITO_SHREDSTREAM_PROXY_BIN="$TMP_DIR/proxy" \
  "$ROOT_DIR/run-doublezero-edge-shadow.sh" >"$TMP_DIR/launch.out"

grep -q '^forward-only --src-bind-port 0 --multicast-bind-ip 233.1.2.3 --multicast-device doublezero1 --multicast-subscribe-port 20001 --grpc-service-port 10099$' \
  "$TMP_DIR/proxy.args"

echo "PASS: DoubleZero shadow preflight and default-off launcher"
