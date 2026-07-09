#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$TMP_DIR/proc/sys/net/core"
printf '8388608\n' > "$TMP_DIR/proc/sys/net/core/rmem_default"
printf '16777216\n' > "$TMP_DIR/proc/sys/net/core/rmem_max"

JITO_SHREDSTREAM_PROC_SYS_ROOT="$TMP_DIR/proc/sys" \
JITO_SHREDSTREAM_UDP_RCVBUF_BYTES=8388608 \
  "$ROOT_DIR/prepare-shredstream-udp.sh" check > "$TMP_DIR/prepare.out"
grep -q 'net.core.rmem_default=8388608' "$TMP_DIR/prepare.out"

printf '212992\n' > "$TMP_DIR/proc/sys/net/core/rmem_default"
if JITO_SHREDSTREAM_PROC_SYS_ROOT="$TMP_DIR/proc/sys" \
  "$ROOT_DIR/prepare-shredstream-udp.sh" check > /dev/null 2> "$TMP_DIR/prepare.err"; then
  echo "low receive buffer should fail preflight" >&2
  exit 1
fi
grep -q 'below required' "$TMP_DIR/prepare.err"

if JITO_SHREDSTREAM_PROC_SYS_ROOT="$TMP_DIR/proc/sys" \
  "$ROOT_DIR/prepare-shredstream-udp.sh" apply > /dev/null 2> "$TMP_DIR/apply.err"; then
  echo "apply should require an explicit opt-in" >&2
  exit 1
fi
grep -q 'JITO_SHREDSTREAM_ALLOW_SYSCTL_APPLY=YES' "$TMP_DIR/apply.err"

cat > "$TMP_DIR/mock-proxy" <<'SH'
#!/usr/bin/env bash
printf 'dest=%s\n' "${DEST_IP_PORTS-unset}" > "$MOCK_PROXY_OUTPUT"
printf 'args=%s\n' "$*" >> "$MOCK_PROXY_OUTPUT"
SH
chmod +x "$TMP_DIR/mock-proxy"

DEST_IP_PORTS=127.0.0.1:8001 \
MOCK_PROXY_OUTPUT="$TMP_DIR/proxy.out" \
JITO_SHREDSTREAM_PROXY_BIN="$TMP_DIR/mock-proxy" \
JITO_SHREDSTREAM_UDP_PREFLIGHT_MODE=off \
  "$ROOT_DIR/run-shredstream-proxy.sh" --grpc-service-port 9999
grep -q '^dest=unset$' "$TMP_DIR/proxy.out"
grep -q '^args=--grpc-service-port 9999$' "$TMP_DIR/proxy.out"

if MOCK_PROXY_OUTPUT="$TMP_DIR/proxy.out" \
  JITO_SHREDSTREAM_PROXY_BIN="$TMP_DIR/mock-proxy" \
  JITO_SHREDSTREAM_UDP_PREFLIGHT_MODE=off \
  "$ROOT_DIR/run-shredstream-proxy.sh" --dest-ip-ports=127.0.0.1:8001 \
  > /dev/null 2> "$TMP_DIR/proxy.err"; then
  echo "gRPC-only proxy should reject raw UDP destination arguments" >&2
  exit 1
fi
grep -q 'raw UDP forwarding is forbidden' "$TMP_DIR/proxy.err"

DEST_IP_PORTS=10.0.0.2:8001 \
MOCK_PROXY_OUTPUT="$TMP_DIR/proxy.out" \
JITO_SHREDSTREAM_PROXY_BIN="$TMP_DIR/mock-proxy" \
JITO_SHREDSTREAM_GRPC_ONLY=false \
JITO_SHREDSTREAM_UDP_PREFLIGHT_MODE=off \
  "$ROOT_DIR/run-shredstream-proxy.sh"
grep -q '^dest=10.0.0.2:8001$' "$TMP_DIR/proxy.out"

cat > "$TMP_DIR/snmp.before" <<'EOF'
Udp: InDatagrams NoPorts InErrors OutDatagrams RcvbufErrors SndbufErrors InCsumErrors IgnoredMulti MemErrors
Udp: 100 200 10 400 20 0 0 0 0
EOF
cat > "$TMP_DIR/snmp.after" <<'EOF'
Udp: InDatagrams NoPorts InErrors OutDatagrams RcvbufErrors SndbufErrors InCsumErrors IgnoredMulti MemErrors
Udp: 150 203 12 450 24 0 0 0 0
EOF
cp "$TMP_DIR/snmp.before" "$TMP_DIR/snmp.current"
cat > "$TMP_DIR/mock-sleep" <<'SH'
#!/usr/bin/env bash
cp "$MOCK_SNMP_AFTER" "$MOCK_SNMP_CURRENT"
SH
cat > "$TMP_DIR/mock-journalctl" <<'SH'
#!/usr/bin/env bash
echo 'proxy missed FEC set for slot 123'
SH
cat > "$TMP_DIR/mock-systemctl" <<'SH'
#!/usr/bin/env bash
if [[ "$1" == "is-active" ]]; then exit 0; fi
if [[ "$1" == "show" ]]; then echo '2026-07-09 00:00:00 UTC'; exit 0; fi
exit 1
SH
cat > "$TMP_DIR/mock-ss" <<'SH'
#!/usr/bin/env bash
echo 'LISTEN 0 128 127.0.0.1:9999 0.0.0.0:*'
SH
chmod +x "$TMP_DIR/mock-sleep" "$TMP_DIR/mock-journalctl" "$TMP_DIR/mock-systemctl" "$TMP_DIR/mock-ss"

if MOCK_SNMP_AFTER="$TMP_DIR/snmp.after" \
  MOCK_SNMP_CURRENT="$TMP_DIR/snmp.current" \
  JITO_SHREDSTREAM_PROC_NET_SNMP="$TMP_DIR/snmp.current" \
  JITO_SHREDSTREAM_SLEEP_BIN="$TMP_DIR/mock-sleep" \
  JITO_SHREDSTREAM_JOURNALCTL_BIN="$TMP_DIR/mock-journalctl" \
  JITO_SHREDSTREAM_SYSTEMCTL_BIN="$TMP_DIR/mock-systemctl" \
  JITO_SHREDSTREAM_SS_BIN="$TMP_DIR/mock-ss" \
  JITO_SHREDSTREAM_FEC_SINCE='2026-07-09 00:00:00 UTC' \
  JITO_SHREDSTREAM_HEALTH_INTERVAL_SECONDS=1 \
  "$ROOT_DIR/check-shredstream-feed-health.sh" > "$TMP_DIR/health.out" 2> "$TMP_DIR/health.err"; then
  echo "health check should fail on packet drops and FEC misses" >&2
  exit 1
fi
grep -q 'UdpRcvbufErrors total=24 delta=4' "$TMP_DIR/health.out"
grep -q 'UdpNoPorts total=203 delta=3' "$TMP_DIR/health.out"
grep -q 'FecMissLogLines.*count=1' "$TMP_DIR/health.out"
grep -q 'UdpRcvbufErrors delta 4 exceeds 0' "$TMP_DIR/health.err"

cat > "$TMP_DIR/mock-systemctl-inactive" <<'SH'
#!/usr/bin/env bash
exit 3
SH
chmod +x "$TMP_DIR/mock-systemctl-inactive"
if JITO_SHREDSTREAM_PROC_NET_SNMP="$TMP_DIR/snmp.current" \
  JITO_SHREDSTREAM_SYSTEMCTL_BIN="$TMP_DIR/mock-systemctl-inactive" \
  JITO_SHREDSTREAM_SS_BIN="$TMP_DIR/mock-ss" \
  JITO_SHREDSTREAM_HEALTH_INTERVAL_SECONDS=0 \
  "$ROOT_DIR/check-shredstream-feed-health.sh" > /dev/null 2> "$TMP_DIR/inactive.err"; then
  echo "health check must fail when the proxy service is unavailable" >&2
  exit 1
fi
grep -q 'is not active' "$TMP_DIR/inactive.err"

echo "ShredStream feed ops tests passed"
