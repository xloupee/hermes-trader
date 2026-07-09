#!/usr/bin/env bash
set -euo pipefail

PROC_NET_SNMP="${JITO_SHREDSTREAM_PROC_NET_SNMP:-/proc/net/snmp}"
INTERVAL_SECONDS="${JITO_SHREDSTREAM_HEALTH_INTERVAL_SECONDS:-10}"
MAX_RCVBUF_ERROR_DELTA="${JITO_SHREDSTREAM_MAX_RCVBUF_ERROR_DELTA:-0}"
MAX_NO_PORTS_DELTA="${JITO_SHREDSTREAM_MAX_NO_PORTS_DELTA:-0}"
MAX_FEC_MISSES="${JITO_SHREDSTREAM_MAX_FEC_MISSES:-0}"
SERVICE_NAME="${JITO_SHREDSTREAM_PROXY_SERVICE:-jito-shredstream-proxy.service}"
JOURNALCTL_BIN="${JITO_SHREDSTREAM_JOURNALCTL_BIN:-journalctl}"
SYSTEMCTL_BIN="${JITO_SHREDSTREAM_SYSTEMCTL_BIN:-systemctl}"
SLEEP_BIN="${JITO_SHREDSTREAM_SLEEP_BIN:-sleep}"
SS_BIN="${JITO_SHREDSTREAM_SS_BIN:-ss}"
GRPC_PORT="${JITO_SHREDSTREAM_GRPC_PORT:-9999}"

for pair in \
  "JITO_SHREDSTREAM_HEALTH_INTERVAL_SECONDS:$INTERVAL_SECONDS" \
  "JITO_SHREDSTREAM_MAX_RCVBUF_ERROR_DELTA:$MAX_RCVBUF_ERROR_DELTA" \
  "JITO_SHREDSTREAM_MAX_NO_PORTS_DELTA:$MAX_NO_PORTS_DELTA" \
  "JITO_SHREDSTREAM_MAX_FEC_MISSES:$MAX_FEC_MISSES"; do
  name="${pair%%:*}"
  value="${pair#*:}"
  if [[ ! "$value" =~ ^[0-9]+$ ]]; then
    echo "$name must be a nonnegative integer; got $value" >&2
    exit 1
  fi
done
if [[ ! "$GRPC_PORT" =~ ^[0-9]+$ ]] || (( GRPC_PORT < 1 || GRPC_PORT > 65535 )); then
  echo "JITO_SHREDSTREAM_GRPC_PORT must be a valid TCP port; got $GRPC_PORT" >&2
  exit 1
fi

if ! "$SYSTEMCTL_BIN" is-active --quiet "$SERVICE_NAME"; then
  echo "$SERVICE_NAME is not active" >&2
  exit 1
fi
if ! listener_output="$($SS_BIN -ltnH)"; then
  echo "failed to inspect TCP listeners with $SS_BIN" >&2
  exit 1
fi
if ! printf '%s\n' "$listener_output" | awk -v port=":$GRPC_PORT" 'substr($4, length($4) - length(port) + 1) == port { found=1 } END { exit !found }'; then
  echo "$SERVICE_NAME has no gRPC TCP listener on port $GRPC_PORT" >&2
  exit 1
fi

read_udp_counters() {
  if [[ ! -r "$PROC_NET_SNMP" ]]; then
    echo "cannot read $PROC_NET_SNMP" >&2
    return 1
  fi
  awk '
    $1 == "Udp:" && !seen_header {
      for (i = 2; i <= NF; i++) index_by_name[$i] = i
      seen_header = 1
      next
    }
    $1 == "Udp:" && seen_header {
      if (!("RcvbufErrors" in index_by_name) || !("NoPorts" in index_by_name) || !("InErrors" in index_by_name)) exit 2
      print $(index_by_name["RcvbufErrors"]), $(index_by_name["NoPorts"]), $(index_by_name["InErrors"])
      exit
    }
  ' "$PROC_NET_SNMP"
}

before="$(read_udp_counters)"
read -r before_rcvbuf before_no_ports before_in_errors <<< "$before"
if [[ -z "${before_rcvbuf:-}" || -z "${before_no_ports:-}" || -z "${before_in_errors:-}" ]]; then
  echo "failed to parse UDP counters from $PROC_NET_SNMP" >&2
  exit 1
fi

if (( INTERVAL_SECONDS > 0 )); then
  "$SLEEP_BIN" "$INTERVAL_SECONDS"
fi

after="$(read_udp_counters)"
read -r after_rcvbuf after_no_ports after_in_errors <<< "$after"
rcvbuf_delta=$((after_rcvbuf - before_rcvbuf))
no_ports_delta=$((after_no_ports - before_no_ports))
in_errors_delta=$((after_in_errors - before_in_errors))

printf 'UdpRcvbufErrors total=%s delta=%s intervalSeconds=%s\n' "$after_rcvbuf" "$rcvbuf_delta" "$INTERVAL_SECONDS"
printf 'UdpNoPorts total=%s delta=%s intervalSeconds=%s\n' "$after_no_ports" "$no_ports_delta" "$INTERVAL_SECONDS"
printf 'UdpInErrors total=%s delta=%s intervalSeconds=%s\n' "$after_in_errors" "$in_errors_delta" "$INTERVAL_SECONDS"

fec_since="${JITO_SHREDSTREAM_FEC_SINCE:-}"
if [[ -z "$fec_since" ]]; then
  if ! fec_since="$("$SYSTEMCTL_BIN" show -p ActiveEnterTimestamp --value "$SERVICE_NAME")"; then
    echo "failed to read $SERVICE_NAME activation timestamp" >&2
    exit 1
  fi
  if [[ -z "$fec_since" ]]; then
    echo "$SERVICE_NAME has no activation timestamp" >&2
    exit 1
  fi
fi
if ! fec_output="$("$JOURNALCTL_BIN" -u "$SERVICE_NAME" --since "$fec_since" --no-pager)"; then
  echo "failed to read $SERVICE_NAME journal" >&2
  exit 1
fi
fec_misses="$(printf '%s\n' "$fec_output" | awk '{ line = tolower($0) } line ~ /miss(ed|ing)?.*fec|fec.*miss(ed|ing)?/ { count++ } END { print count + 0 }')"
printf 'FecMissLogLines service=%s count=%s since=%s\n' "$SERVICE_NAME" "$fec_misses" "$fec_since"

failed=0
if (( rcvbuf_delta < 0 || no_ports_delta < 0 || in_errors_delta < 0 )); then
  echo "UDP counters reset during the sample; rerun after the proxy is stable" >&2
  failed=1
fi
if (( rcvbuf_delta > MAX_RCVBUF_ERROR_DELTA )); then
  echo "UdpRcvbufErrors delta $rcvbuf_delta exceeds $MAX_RCVBUF_ERROR_DELTA" >&2
  failed=1
fi
if (( no_ports_delta > MAX_NO_PORTS_DELTA )); then
  echo "UdpNoPorts delta $no_ports_delta exceeds $MAX_NO_PORTS_DELTA" >&2
  failed=1
fi
if (( fec_misses > MAX_FEC_MISSES )); then
  echo "FEC miss log count $fec_misses exceeds $MAX_FEC_MISSES" >&2
  failed=1
fi
exit "$failed"
