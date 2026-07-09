#!/usr/bin/env bash
set -euo pipefail

ACTION="${1:-check}"
TARGET_BYTES="${JITO_SHREDSTREAM_UDP_RCVBUF_BYTES:-8388608}"
PROC_SYS_ROOT="${JITO_SHREDSTREAM_PROC_SYS_ROOT:-/proc/sys}"
SYSCTL_BIN="${JITO_SHREDSTREAM_SYSCTL_BIN:-sysctl}"

case "$TARGET_BYTES" in
  ''|*[!0-9]*)
    echo "JITO_SHREDSTREAM_UDP_RCVBUF_BYTES must be a positive integer; got $TARGET_BYTES" >&2
    exit 1
    ;;
esac
if (( TARGET_BYTES < 1048576 )); then
  echo "JITO_SHREDSTREAM_UDP_RCVBUF_BYTES must be at least 1048576; got $TARGET_BYTES" >&2
  exit 1
fi

read_setting() {
  local name="$1"
  local path="$PROC_SYS_ROOT/${name//.//}"

  if [[ ! -r "$path" ]]; then
    echo "cannot read $path" >&2
    return 1
  fi
  tr -d '[:space:]' < "$path"
}

check_settings() {
  local failed=0
  local name current

  for name in net.core.rmem_default net.core.rmem_max; do
    if ! current="$(read_setting "$name")"; then
      failed=1
      continue
    fi
    if [[ ! "$current" =~ ^[0-9]+$ ]]; then
      echo "$name is not an integer: $current" >&2
      failed=1
    elif (( current < TARGET_BYTES )); then
      echo "$name=$current is below required $TARGET_BYTES" >&2
      failed=1
    else
      echo "$name=$current (required >= $TARGET_BYTES)"
    fi
  done

  return "$failed"
}

case "$ACTION" in
  check)
    check_settings
    ;;
  apply)
    if [[ "${JITO_SHREDSTREAM_ALLOW_SYSCTL_APPLY:-}" != "YES" ]]; then
      echo "refusing to change kernel settings; set JITO_SHREDSTREAM_ALLOW_SYSCTL_APPLY=YES" >&2
      exit 1
    fi
    if (( EUID != 0 )); then
      echo "applying kernel receive buffers requires root" >&2
      exit 1
    fi
    "$SYSCTL_BIN" -w "net.core.rmem_max=$TARGET_BYTES"
    "$SYSCTL_BIN" -w "net.core.rmem_default=$TARGET_BYTES"
    check_settings
    ;;
  *)
    echo "usage: $0 [check|apply]" >&2
    exit 1
    ;;
esac
