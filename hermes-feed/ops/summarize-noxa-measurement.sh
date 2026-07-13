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
readonly STARTED_UTC="$(sed -n 's/^started_utc=//p' "$RUN_DIR/manifest")"
readonly REQUESTED_DURATION_SECONDS="$(sed -n 's/^duration_seconds=//p' "$RUN_DIR/manifest")"
[[ "$REQUESTED_DURATION_SECONDS" =~ ^[1-9][0-9]*$ ]] || {
  echo "Invalid requested duration in measurement manifest" >&2
  exit 1
}
if [[ -f "$RUN_DIR/completed" ]]; then
  readonly COMPLETED=true
  readonly COMPLETED_UTC="$(awk 'NR == 1 {print $1}' "$RUN_DIR/completed")"
else
  readonly COMPLETED=false
  readonly COMPLETED_UTC=""
fi
readonly STARTED_EPOCH="$(date --date="$STARTED_UTC" +%s)"
if [[ -n "$COMPLETED_UTC" ]]; then
  readonly COMPLETED_EPOCH="$(date --date="$COMPLETED_UTC" +%s)"
  readonly WALL_DURATION_SECONDS="$((COMPLETED_EPOCH - STARTED_EPOCH))"
else
  readonly WALL_DURATION_SECONDS=0
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
  --arg started_utc "$STARTED_UTC" \
  --arg completed_utc "$COMPLETED_UTC" \
  --argjson requested_duration_seconds "$REQUESTED_DURATION_SECONDS" \
  --argjson wall_duration_seconds "$WALL_DURATION_SECONDS" \
  --argjson completed "$COMPLETED" \
  --slurpfile events "$EVENTS" \
  --slurpfile boundary "$BOUNDARY" \
  --slurpfile factory "$FACTORY_STATUS" '
  def percentile($values; $percent):
    if ($values | length) == 0 then null
    else $values[(((($values | length) * $percent / 100) | ceil) - 1)]
    end;
  def distribution($values): {
    samples: ($values | length),
    p50_ns: percentile($values; 50),
    p95_ns: percentile($values; 95),
    p99_ns: percentile($values; 99),
    min_ns: ($values | first // null),
    max_ns: ($values | last // null)
  };
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
  ($boundary_ns | map(select(. <= 30000000000))) as $prompt_boundary_ns |
  ($boundary_ns | map(select(. > 30000000000))) as $delayed_boundary_ns |
  ((($factory | map(select(.record_type == "noxa_factory_status")))
    + ($events | map(select(.record_type == "noxa_factory_status_watch"))))) as $statuses |
  {
    record_type: "noxa_two_hour_measurement_summary",
    measurement_run: $measurement_run,
    observer_run: $observer_run,
    completed: $completed,
    capture_window: {
      started_utc: $started_utc,
      completed_utc: (if $completed_utc == "" then null else $completed_utc end),
      requested_duration_seconds: $requested_duration_seconds,
      wall_duration_seconds: (if $completed then $wall_duration_seconds else null end),
      duration_requirement_met: (
        $completed and ($wall_duration_seconds >= $requested_duration_seconds)
      )
    },
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
      observable: "parent newHeads arrival to first post-warmup feed message carrying the same L1 header number",
      caveat: "This is not pure network latency: an idle or delayed L2 message can first carry an older L1 header much later.",
      all_samples: distribution($boundary_ns),
      prompt_at_most_30s: distribution($prompt_boundary_ns),
      delayed_over_30s: distribution($delayed_boundary_ns)
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
