#!/usr/bin/env bash
set -euo pipefail

if (( $# == 0 )); then
  echo "usage: $0 CANARY.jsonl [CANARY.jsonl ...]" >&2
  exit 64
fi

for input in "$@"; do
  [[ -f "$input" ]] || {
    echo "missing canary log: $input" >&2
    exit 66
  }
done

jq -s '
  def percentile($values; $p):
    ($values | sort) as $sorted
    | ($sorted | length) as $n
    | if $n == 0 then null
      else $sorted[((($n * $p + 99) / 100 | floor) - 1) | if . < 0 then 0 else . end]
      end;
  map(select(.record_type == "noxa_testnet_canary_submission"
          or .record_type == "noxa_testnet_canary_reconciled"))
  | group_by(.source // "unspecified")
  | map(
      . as $records
      | [$records[] | select(.record_type == "noxa_testnet_canary_submission")] as $submissions
      | [$records[] | select(.record_type == "noxa_testnet_canary_reconciled")] as $reconciled
      | [$submissions[] | .submission_elapsed_ns] as $submit_ns
      | [$reconciled[] | select(.included == true) | .submit_to_receipt_ns] as $receipt_ns
      | {
          source: ($records[0].source // "unspecified"),
          submissions: ($submissions | length),
          included_receipts: ([$reconciled[] | select(.included == true)] | length),
          unresolved: ([$reconciled[] | select(.included != true)] | length),
          network_attempts: ([$submissions[] | .network_attempts] | add // 0),
          decisions: ([$submissions[] | .decision.decision] | group_by(.) | map({key: .[0], value: length}) | from_entries),
          submission_elapsed_ns: {
            p50: percentile($submit_ns; 50),
            p95: percentile($submit_ns; 95),
            p99: percentile($submit_ns; 99),
            max: ($submit_ns | max // null)
          },
          submit_to_receipt_ns: {
            p50: percentile($receipt_ns; 50),
            p95: percentile($receipt_ns; 95),
            p99: percentile($receipt_ns; 99),
            max: ($receipt_ns | max // null)
          }
        }
    )
  | {record_type: "noxa_testnet_canary_benchmark_summary", sources: .}
' "$@"
