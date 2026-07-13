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
readonly MEASUREMENT_RESTARTS="$RUN_DIR/measurement-restarts.log"
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
readonly WINDOW_START_NS="$((STARTED_EPOCH * 1000000000))"
if [[ -n "$COMPLETED_UTC" ]]; then
  readonly WINDOW_END_NS="$((COMPLETED_EPOCH * 1000000000))"
else
  readonly WINDOW_END_NS="$(( $(date +%s) * 1000000000 ))"
fi

for file in "$EVENTS" "$BOUNDARY" "$FACTORY_STATUS"; do
  [[ -f "$file" ]] || {
    echo "Missing measurement input: $file" >&2
    exit 1
  }
done
readonly BINARY_SHA256="$(awk 'NR == 1 {print $1}' "$RUN_DIR/binary.sha256")"
if [[ -x "$RUN_DIR/hermes-noxa" ]]; then
  readonly MEASUREMENT_BINARY_IMMUTABLE=true
else
  readonly MEASUREMENT_BINARY_IMMUTABLE=false
fi
if [[ -x "$OBSERVER_RUN/hermes-noxa" ]]; then
  readonly OBSERVER_BINARY_IMMUTABLE=true
else
  readonly OBSERVER_BINARY_IMMUTABLE=false
fi
readonly EVENTS_PREFIX_BYTES="$(stat --format='%s' "$EVENTS")"
readonly EVENTS_SHA256="$(head --bytes="$EVENTS_PREFIX_BYTES" "$EVENTS" | sha256sum | awk '{print $1}')"
readonly BOUNDARY_SHA256="$(sha256sum "$BOUNDARY" | awk '{print $1}')"
readonly FACTORY_STATUS_SHA256="$(sha256sum "$FACTORY_STATUS" | awk '{print $1}')"
if [[ -f "$MEASUREMENT_RESTARTS" ]]; then
  readonly BOUNDARY_RUNS="$(awk '/boundary_status=/ {count++} END {print count + 0}' "$MEASUREMENT_RESTARTS")"
  readonly BOUNDARY_FAILED_RUNS="$(awk '
    /boundary_status=/ {
      status = $0
      sub(/^.*boundary_status=/, "", status)
      sub(/ .*/, "", status)
      if (status != "0") count++
    }
    END {print count + 0}
  ' "$MEASUREMENT_RESTARTS")"
  readonly FACTORY_POLL_FAILURES="$(awk '/status_poll_failed/ {count++} END {print count + 0}' "$MEASUREMENT_RESTARTS")"
else
  readonly BOUNDARY_RUNS=0
  readonly BOUNDARY_FAILED_RUNS=0
  readonly FACTORY_POLL_FAILURES=0
fi
readonly BOUNDARY_STDERR_BYTES="$(wc -c <"$RUN_DIR/boundary.stderr")"
readonly FACTORY_STDERR_BYTES="$(wc -c <"$RUN_DIR/factory-status.stderr")"
readonly MEASUREMENT_SUPERVISOR_STDERR_BYTES="$(wc -c <"$RUN_DIR/supervisor.stderr")"
readonly OBSERVER_STDERR_BYTES="$(wc -c <"$OBSERVER_RUN/observer.stderr")"
readonly OBSERVER_SUPERVISOR_STDERR_BYTES="$(wc -c <"$OBSERVER_RUN/supervisor.stderr")"

jq -n \
  --arg measurement_run "$RUN_DIR" \
  --arg observer_run "$OBSERVER_RUN" \
  --arg started_utc "$STARTED_UTC" \
  --arg completed_utc "$COMPLETED_UTC" \
  --argjson requested_duration_seconds "$REQUESTED_DURATION_SECONDS" \
  --argjson wall_duration_seconds "$WALL_DURATION_SECONDS" \
  --argjson window_start_ns "$WINDOW_START_NS" \
  --argjson window_end_ns "$WINDOW_END_NS" \
  --arg binary_sha256 "$BINARY_SHA256" \
  --argjson measurement_binary_immutable "$MEASUREMENT_BINARY_IMMUTABLE" \
  --argjson observer_binary_immutable "$OBSERVER_BINARY_IMMUTABLE" \
  --arg events_sha256 "$EVENTS_SHA256" \
  --argjson events_prefix_bytes "$EVENTS_PREFIX_BYTES" \
  --arg boundary_sha256 "$BOUNDARY_SHA256" \
  --arg factory_status_sha256 "$FACTORY_STATUS_SHA256" \
  --argjson boundary_runs "$BOUNDARY_RUNS" \
  --argjson boundary_failed_runs "$BOUNDARY_FAILED_RUNS" \
  --argjson factory_poll_failures "$FACTORY_POLL_FAILURES" \
  --argjson boundary_stderr_bytes "$BOUNDARY_STDERR_BYTES" \
  --argjson factory_stderr_bytes "$FACTORY_STDERR_BYTES" \
  --argjson measurement_supervisor_stderr_bytes "$MEASUREMENT_SUPERVISOR_STDERR_BYTES" \
  --argjson observer_stderr_bytes "$OBSERVER_STDERR_BYTES" \
  --argjson observer_supervisor_stderr_bytes "$OBSERVER_SUPERVISOR_STDERR_BYTES" \
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
  ($events
    | map(select(
        (.received_unix_ns? != null)
        and (.received_unix_ns >= $window_start_ns)
        and (.received_unix_ns <= $window_end_ns)
      ))) as $timestamped_events |
  ($timestamped_events
    | map(select(.record_type == "noxa_factory_call_observed") | .tx_hash)) as $window_tx_hashes |
  ($events
    | map(select(
        ((.received_unix_ns? != null)
          and (.received_unix_ns >= $window_start_ns)
          and (.received_unix_ns <= $window_end_ns))
        or ((.received_unix_ns? == null)
          and (.tx_hash? != null)
          and (.tx_hash as $hash | $window_tx_hashes | index($hash) != null))
      ))) as $events |
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
    provenance: {
      binary_sha256: $binary_sha256,
      measurement_binary_immutable: $measurement_binary_immutable,
      observer_binary_immutable: $observer_binary_immutable,
      binary_scope_note: (
        if $measurement_binary_immutable and $observer_binary_immutable
        then "Every subprocess in this run used its run-local immutable binary copy."
        else "Legacy run: long-lived processes retain their start binary, but periodic subprocesses referenced the mutable release path; inspect zero-restart evidence and binary hashes."
        end
      ),
      observer_journal_prefix_bytes: $events_prefix_bytes,
      observer_journal_prefix_sha256: $events_sha256,
      observer_statistics_window_start_unix_ns: $window_start_ns,
      observer_statistics_window_end_unix_ns: $window_end_ns,
      boundary_sha256: $boundary_sha256,
      factory_status_sha256: $factory_status_sha256
    },
    process_health: {
      boundary_runs: $boundary_runs,
      boundary_failed_runs: $boundary_failed_runs,
      factory_poll_failures: $factory_poll_failures,
      boundary_stderr_bytes: $boundary_stderr_bytes,
      factory_status_stderr_bytes: $factory_stderr_bytes,
      measurement_supervisor_stderr_bytes: $measurement_supervisor_stderr_bytes,
      observer_stderr_bytes: $observer_stderr_bytes,
      observer_supervisor_stderr_bytes: $observer_supervisor_stderr_bytes,
      factory_watch_errors: (
        $events | map(select(.record_type == "noxa_factory_status_watch_error")) | length
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
      first_health_sequence: ($first_health.sequence.last // null),
      sequence_rate_per_second: (
        if ($first_health.received_unix_ns and $last_health.received_unix_ns)
          and ($last_health.received_unix_ns > $first_health.received_unix_ns)
        then (($last_health.sequence.last - $first_health.sequence.last) * 1000000000
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
    connections: {
      events: ($events | map(select(.record_type == "noxa_connection")) | length),
      disconnects: (
        $events
        | map(select(.record_type == "noxa_connection" and .state != "connected"))
        | length
      ),
      by_state: (
        $events
        | map(select(.record_type == "noxa_connection") | .state)
        | sort
        | group_by(.)
        | map({key: .[0], value: length})
        | from_entries
      )
    }
  }'
