#!/usr/bin/env bash
set -euo pipefail

# Read-only preflight for a DoubleZero Edge shadow feed. This script never
# creates interfaces, changes routes, starts services, or enables execution.

IP_BIN="${IP_BIN:-ip}"
SS_BIN="${SS_BIN:-ss}"
DEVICE="${DOUBLEZERO_DEVICE:-doublezero1}"
SHADOW_PORT="${DOUBLEZERO_SHADOW_GRPC_PORT:-10099}"
PRIMARY_PORT="${JITO_PRIMARY_GRPC_PORT:-9999}"
MULTICAST_GROUP="${DOUBLEZERO_MULTICAST_GROUP:-}"
CONSUMER_MODE="${DOUBLEZERO_SHADOW_CONSUMER_MODE:-observer}"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

require_true() {
  local name="$1"
  [[ "${!name:-false}" == "true" ]] || fail "$name must be explicitly set to true"
}

valid_port() {
  [[ "$1" =~ ^[0-9]+$ ]] && (( $1 >= 1 && $1 <= 65535 ))
}

[[ "${DOUBLEZERO_SHADOW_ALLOW_EXECUTION:-false}" == "false" ]] || \
  fail "DOUBLEZERO_SHADOW_ALLOW_EXECUTION must remain false; a shadow proxy must never execute trades"

case "$CONSUMER_MODE" in
  observer)
    ;;
  global-arbiter)
    require_true DOUBLEZERO_GLOBAL_ARBITER_VALIDATED
    ;;
  *)
    fail "DOUBLEZERO_SHADOW_CONSUMER_MODE must be observer or global-arbiter"
    ;;
esac

# These are deliberate human gates. A local interface alone cannot prove that
# DigitalOcean passes GRE, BGP, and multicast traffic end to end.
require_true DOUBLEZERO_DO_GRE_VALIDATED
require_true DOUBLEZERO_DO_BGP_VALIDATED
require_true DOUBLEZERO_DO_MULTICAST_VALIDATED
require_true DOUBLEZERO_GRPC_ONLY_PROXY_VALIDATED

valid_port "$SHADOW_PORT" || fail "invalid DOUBLEZERO_SHADOW_GRPC_PORT: $SHADOW_PORT"
valid_port "$PRIMARY_PORT" || fail "invalid JITO_PRIMARY_GRPC_PORT: $PRIMARY_PORT"
[[ "$SHADOW_PORT" != "$PRIMARY_PORT" ]] || fail "shadow gRPC port must differ from primary port $PRIMARY_PORT"

[[ "$MULTICAST_GROUP" =~ ^(22[4-9]|23[0-9])\.([0-9]{1,3}\.){2}[0-9]{1,3}$ ]] || \
  fail "DOUBLEZERO_MULTICAST_GROUP must be an IPv4 multicast address in 224.0.0.0/4"

link_output="$($IP_BIN -details link show dev "$DEVICE" 2>/dev/null)" || \
  fail "network device $DEVICE does not exist"
grep -Eq '<[^>]*UP[^>]*>' <<<"$link_output" || fail "network device $DEVICE is not UP"
grep -Eq '<[^>]*MULTICAST[^>]*>' <<<"$link_output" || fail "network device $DEVICE lacks the MULTICAST flag"

addr_output="$($IP_BIN -4 address show dev "$DEVICE" 2>/dev/null)" || \
  fail "cannot inspect IPv4 addresses on $DEVICE"
grep -Eq '(^|[[:space:]])inet[[:space:]]+[0-9]+\.' <<<"$addr_output" || \
  fail "network device $DEVICE has no IPv4 address"

route_output="$($IP_BIN route get "$MULTICAST_GROUP" 2>/dev/null)" || \
  fail "no route to multicast group $MULTICAST_GROUP"
grep -Eq "(^|[[:space:]])dev[[:space:]]+$DEVICE([[:space:]]|$)" <<<"$route_output" || \
  fail "multicast group $MULTICAST_GROUP is not routed through $DEVICE"

if ! listen_output="$($SS_BIN -H -ltn "sport = :$SHADOW_PORT")"; then
  fail "cannot inspect TCP listeners with $SS_BIN"
fi
[[ -z "$listen_output" ]] || fail "shadow gRPC port $SHADOW_PORT is already listening"

echo "PASS: DoubleZero shadow preflight is read-only and all explicit gates passed"
echo "device=$DEVICE multicast_group=$MULTICAST_GROUP grpc_port=$SHADOW_PORT consumer_mode=$CONSUMER_MODE"
