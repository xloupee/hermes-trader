#!/usr/bin/env bash
set -euo pipefail

readonly BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$BASE_DIR/ops/noxa-observer-common.sh"

readonly STATE_DIR="$(canonical_runtime_path \
  "${HERMES_NOXA_MEASUREMENT_STATE_DIR:-$WORKTREE_ROOT/.runtime/hermes-noxa-measurement}" \
  "measurement state directory")"
readonly RUNS_DIR="$(canonical_child_path "$STATE_DIR/runs" "$STATE_DIR" "runs directory")"
readonly RUN_DIR="$(canonical_child_path \
  "$(readlink -f "$STATE_DIR/current")" "$RUNS_DIR" "current run directory")"
readonly OBSERVER_RUN_RAW="$(sed -n 's/^observer_run=//p' "$RUN_DIR/manifest")"
readonly OBSERVER_ROOT="$(canonical_runtime_path \
  "$WORKTREE_ROOT/.runtime/hermes-noxa/runs" "observer runs directory")"
readonly OBSERVER_RUN="$(canonical_child_path \
  "$OBSERVER_RUN_RAW" "$OBSERVER_ROOT" "observer run directory")"
readonly EVENTS="$OBSERVER_RUN/events.jsonl"
readonly BOUNDARY="$RUN_DIR/boundary.jsonl"
readonly FACTORY_STATUS="$RUN_DIR/factory-status.jsonl"
if [[ -f "$RUN_DIR/completed" ]]; then
  readonly COMPLETED=true
else
  readonly COMPLETED=false
fi

for file in "$EVENTS" "$BOUNDARY" "$FACTORY_STATUS"; do
  [[ -f "$file" ]] || {
    echo "Missing measurement input: $file" >&2
    exit 1
  }
done

jq -n \
  --arg measurement_run "$RUN_DIR" \
  --arg observer_run "$OBSERVER_RUN" \
  --argjson completed "$COMPLETED" \
  --slurpfile events "$EVENTS" \
  --slurpfile boundary "$BOUNDARY" \
  --slurpfile factory "$FACTORY_STATUS" '
  def percentile($values; $percent):
    if ($values | length) == 0 then null
    else $values[(((($values | length) * $percent / 100) | ceil) - 1)]
    end;
  ($events | map(select(.record_type == "noxa_feed_health"))) as $health |
  ($health | first // {}) as $first_health |
  ($health | last // {}) as $last_health |
  ($events | map(select(.record_type == "noxa_factory_call_observed"))) as $calls |
  ($events | map(select(.record_type == "noxa_launch_reverted"))) as $reverted |
  ($events | map(select(.record_type == "noxa_launch_verified_shadow"))) as $verified |
  ($events | map(select(.record_type == "noxa_receipt_verification_error"))) as $verify_errors |
  ($events | map(select(.record_type == "noxa_receipt_verification_dropped"))) as $verify_dropped |
  (($reverted + $verified) | map(.receipt_visibility_ns) | sort) as $receipt_ns |
  ($boundary | map(select(.record_type == "noxa_boundary_sample") | .head_to_feed_ns) | sort) as $boundary_ns |
  ($factory | map(select(.record_type == "noxa_factory_status"))) as $statuses |
  {
    record_type: "noxa_two_hour_measurement_summary",
    measurement_run: $measurement_run,
    observer_run: $observer_run,
    completed: $completed,
    feed: {
      health_records: ($health | length),
      first_sequence: $last_health.sequence.first,
      last_sequence: $last_health.sequence.last,
      gaps: ($last_health.sequence.gaps // null),
      missing: ($last_health.sequence.missing // null),
      duplicates_or_reordered: ($last_health.sequence.duplicates_or_reordered // null),
      reconnects: ($last_health.reconnects // null),
      measured_duration_ns: (
        if ($first_health.received_unix_ns and $last_health.received_unix_ns)
        then ($last_health.received_unix_ns - $first_health.received_unix_ns)
        else null end
      ),
      sequence_rate_per_second: (
        if ($first_health.received_unix_ns and $last_health.received_unix_ns)
          and ($last_health.received_unix_ns > $first_health.received_unix_ns)
        then (($last_health.sequence.last - $first_health.sequence.first + 1) * 1000000000
          / ($last_health.received_unix_ns - $first_health.received_unix_ns))
        else null end
      ),
      last_interval_messages: ($last_health.feed_messages_since_last_health // null),
      last_interval_signed_transactions: ($last_health.signed_transactions_since_last_health // null)
    },
    launches: {
      factory_calls_observed: ($calls | length),
      reverted: ($reverted | length),
      verified: ($verified | length),
      verification_errors: ($verify_errors | length),
      verifier_saturation_drops: ($verify_dropped | length),
      receipt_visibility_ns: {
        samples: ($receipt_ns | length),
        p50: percentile($receipt_ns; 50),
        p95: percentile($receipt_ns; 95),
        p99: percentile($receipt_ns; 99),
        max: ($receipt_ns | last // null)
      }
    },
    boundary: {
      samples: ($boundary_ns | length),
      p50_ns: percentile($boundary_ns; 50),
      p95_ns: percentile($boundary_ns; 95),
      p99_ns: percentile($boundary_ns; 99),
      min_ns: ($boundary_ns | first // null),
      max_ns: ($boundary_ns | last // null)
    },
    rpc: ($last_health.rpc // null),
    factory: {
      samples: ($statuses | length),
      ever_enabled: ($statuses | any(.status.launch_enabled == true)),
      enabled_samples: ($statuses | map(select(.status.launch_enabled == true)) | length),
      first: ($statuses | first // null),
      last: ($statuses | last // null),
      runtime_hash_mismatches: ($statuses | map(select(.runtime_hash_matches_pin != true)) | length)
    },
    connections: (
      $events
      | map(select(.record_type == "noxa_connection") | .state)
      | sort
      | group_by(.)
      | map({key: .[0], value: length})
      | from_entries
    )
  }'
