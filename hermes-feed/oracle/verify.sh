#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: verify.sh <rust-output.jsonl> <go-output.jsonl>" >&2
  exit 2
fi

readonly RUST_OUTPUT="$1"
readonly GO_OUTPUT="$2"

readonly RUST_COUNT="$(grep -c '"record_type":"transaction"' "$RUST_OUTPUT")"
readonly GO_COUNT="$(grep -c '"record_type":"transaction"' "$GO_OUTPUT")"
readonly RUST_SHA="$(grep '"record_type":"transaction"' "$RUST_OUTPUT" | sha256sum | cut -d' ' -f1)"
readonly GO_SHA="$(grep '"record_type":"transaction"' "$GO_OUTPUT" | sha256sum | cut -d' ' -f1)"

echo "rust_count=$RUST_COUNT"
echo "go_count=$GO_COUNT"
echo "rust_sha256=$RUST_SHA"
echo "go_sha256=$GO_SHA"
cmp \
  <(grep '"record_type":"transaction"' "$RUST_OUTPUT") \
  <(grep '"record_type":"transaction"' "$GO_OUTPUT")
echo "Nitro differential fingerprints match byte-for-byte"
