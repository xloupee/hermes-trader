#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -lt 5 ]]; then
  echo "usage: $0 EXPECTED_PINS EXPECTED_PINS_KECCAK256 CAMPAIGN_DIR WINDOW_NAME [WINDOW_NAME...] -- [PROBE_ARGS...]" >&2
  exit 64
fi

expected_pins_source=$1
reviewed_expected_pins_digest=$(printf '%s' "$2" | tr '[:upper:]' '[:lower:]')
campaign_dir=$3
shift 3
window_names=()
while [[ "$#" -gt 0 && "$1" != -- ]]; do
  window_names+=("$1")
  shift
done
if [[ "$#" -eq 0 || "$1" != -- || "${#window_names[@]}" -eq 0 ]]; then
  echo "campaign requires one or more window names followed by --" >&2
  exit 64
fi
shift
probe_args=("$@")

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_dir=$(cd "$script_dir/.." && pwd)
target_dir=$repo_dir/target/release
local_runner=${HERMES_LAUNCHPAD_LOCAL_RUNNER:-$script_dir/run-launchpad-paper-local.sh}
snapshot_bin=${HERMES_LAUNCHPAD_PIN_SNAPSHOT_BIN:-$target_dir/hermes-launchpad-pin-snapshot}
report_bin=${HERMES_LAUNCHPAD_EVIDENCE_REPORT_BIN:-$target_dir/hermes-launchpad-evidence-report}
feed_bin=${HERMES_FEED_BIN:-$target_dir/hermes-feed}
paper_bin=${HERMES_LAUNCHPAD_PAPER_BIN:-$target_dir/hermes-launchpad-paper}
reconcile_bin=${HERMES_LAUNCHPAD_RECONCILE_BIN:-$target_dir/hermes-launchpad-reconcile}
head_bin=${HERMES_LAUNCHPAD_CHAIN_HEAD_BIN:-$target_dir/hermes-launchpad-chain-head}
readiness_bin=${HERMES_LAUNCHPAD_READINESS_BIN:-$target_dir/hermes-launchpad-readiness}

if [[ ! -f "$expected_pins_source" || -L "$expected_pins_source" ]]; then
  echo "expected pins must be an existing regular non-symlink file" >&2
  exit 66
fi
if [[ ! "$reviewed_expected_pins_digest" =~ ^0x[0-9a-f]{64}$ ]]; then
  echo "reviewed expected-pin digest must be lowercase 0x plus 64 hex digits" >&2
  exit 65
fi
for path in "$local_runner" "$snapshot_bin" "$report_bin" "$feed_bin" "$paper_bin" "$reconcile_bin" "$head_bin" "$readiness_bin"; do
  if [[ ! -x "$path" || -L "$path" ]]; then
    echo "required campaign executable is missing, non-executable, or a symlink: $path" >&2
    exit 69
  fi
done

for ((name_index = 0; name_index < ${#window_names[@]}; name_index += 1)); do
  name=${window_names[name_index]}
  if [[ ! "$name" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ || "$name" == . || "$name" == .. ]]; then
    echo "invalid window name: $name" >&2
    exit 65
  fi
  for ((prior_index = 0; prior_index < name_index; prior_index += 1)); do
    if [[ "${window_names[prior_index]}" == "$name" ]]; then
      echo "duplicate window name: $name" >&2
      exit 65
    fi
  done
done

preflight_digest() {
  local path=$1
  local digest
  digest=$("$path" --print-self-digest)
  digest=$(printf '%s' "$digest" | tr '[:upper:]' '[:lower:]')
  if [[ ! "$digest" =~ ^0x[0-9a-f]{64}$ ]]; then
    echo "executable $path returned an invalid self digest" >&2
    exit 69
  fi
  printf '%s\n' "$digest"
}

feed_digest=$(preflight_digest "$feed_bin")
paper_digest=$(preflight_digest "$paper_bin")
reconcile_digest=$(preflight_digest "$reconcile_bin")
head_digest=$(preflight_digest "$head_bin")
readiness_digest=$(preflight_digest "$readiness_bin")
report_digest=$(preflight_digest "$report_bin")
snapshot_digest=$("$report_bin" --print-file-keccak256 "$snapshot_bin")
local_runner_digest=$("$report_bin" --print-file-keccak256 "$local_runner")

umask 077
if [[ -e "$campaign_dir" ]]; then
  echo "refusing to reuse existing campaign directory $campaign_dir" >&2
  exit 73
fi
mkdir "$campaign_dir"
chmod 700 "$campaign_dir"
mkdir "$campaign_dir/windows" "$campaign_dir/snapshots" "$campaign_dir/snapshot-reports"
chmod 700 "$campaign_dir/windows" "$campaign_dir/snapshots" "$campaign_dir/snapshot-reports"

frozen_pins=$campaign_dir/expected-pins.locked.json
cp "$expected_pins_source" "$frozen_pins"
chmod 600 "$frozen_pins"
actual_pins_digest=$("$report_bin" --print-file-keccak256 "$frozen_pins")
actual_pins_digest=$(printf '%s' "$actual_pins_digest" | tr '[:upper:]' '[:lower:]')
if [[ "$actual_pins_digest" != "$reviewed_expected_pins_digest" ]]; then
  echo "expected-pin bytes do not match independently reviewed digest" >&2
  exit 65
fi

campaign_lock=$campaign_dir/campaign-lock.json
printf '{"record_type":"launchpad_paper_campaign_lock","schema_version":1,"expected_pins_content_keccak256":"%s","executables":{"feed_keccak256":"%s","paper_keccak256":"%s","reconciler_keccak256":"%s","chain_head_keccak256":"%s","readiness_keccak256":"%s"},"orchestration":{"report_keccak256":"%s","pin_snapshot_keccak256":"%s","local_runner_keccak256":"%s"}}\n' \
  "$actual_pins_digest" "$feed_digest" "$paper_digest" "$reconcile_digest" "$head_digest" "$readiness_digest" "$report_digest" "$snapshot_digest" "$local_runner_digest" > "$campaign_lock"
chmod 600 "$campaign_lock"
campaign_lock_digest=$("$report_bin" --print-file-keccak256 "$campaign_lock")

active_pid=
forward_signal() {
  local signal=$1
  trap - INT TERM HUP
  if [[ -n "$active_pid" ]]; then
    kill -"$signal" "$active_pid" 2>/dev/null || true
    wait "$active_pid" 2>/dev/null || true
  fi
  exit 128
}
trap 'forward_signal INT' INT
trap 'forward_signal TERM' TERM
trap 'forward_signal HUP' HUP

assert_tuple_unchanged() {
  [[ "$(preflight_digest "$feed_bin")" == "$feed_digest" ]] || { echo "feed executable changed during campaign" >&2; return 70; }
  [[ "$(preflight_digest "$paper_bin")" == "$paper_digest" ]] || { echo "paper executable changed during campaign" >&2; return 70; }
  [[ "$(preflight_digest "$reconcile_bin")" == "$reconcile_digest" ]] || { echo "reconciler executable changed during campaign" >&2; return 70; }
  [[ "$(preflight_digest "$head_bin")" == "$head_digest" ]] || { echo "chain-head executable changed during campaign" >&2; return 70; }
  [[ "$(preflight_digest "$readiness_bin")" == "$readiness_digest" ]] || { echo "readiness executable changed during campaign" >&2; return 70; }
}

assert_pins_unchanged() {
  local current
  current=$("$report_bin" --print-file-keccak256 "$frozen_pins")
  current=$(printf '%s' "$current" | tr '[:upper:]' '[:lower:]')
  [[ "$current" == "$actual_pins_digest" ]] || {
    echo "locked expected-pin bytes changed during campaign" >&2
    return 70
  }
}

assert_orchestration_unchanged() {
  [[ "$(preflight_digest "$report_bin")" == "$report_digest" ]] || { echo "report executable changed during campaign" >&2; return 70; }
  [[ "$("$report_bin" --print-file-keccak256 "$snapshot_bin")" == "$snapshot_digest" ]] || { echo "snapshot executable changed during campaign" >&2; return 70; }
  [[ "$("$report_bin" --print-file-keccak256 "$local_runner")" == "$local_runner_digest" ]] || { echo "local runner changed during campaign" >&2; return 70; }
}

completed_sessions=()
partial_sessions=()
failed_windows=0
for name in "${window_names[@]}"; do
  assert_tuple_unchanged
  assert_pins_unchanged
  assert_orchestration_unchanged
  snapshot=$campaign_dir/snapshots/$name.json
  snapshot_partial=$snapshot.partial
  snapshot_report=$campaign_dir/snapshot-reports/$name.json
  snapshot_report_partial=$snapshot_report.partial
  session_dir=$campaign_dir/windows/$name
  if [[ -e "$snapshot" || -e "$snapshot_report" || -e "$session_dir" ]]; then
    echo "refusing to overwrite campaign window $name" >&2
    exit 73
  fi
  if "$snapshot_bin" --expected-pins "$frozen_pins" --snapshot-output "$snapshot_partial" > "$snapshot_report_partial" \
    && [[ -s "$snapshot_partial" && -s "$snapshot_report_partial" ]]; then
    chmod 600 "$snapshot_partial" "$snapshot_report_partial"
    mv "$snapshot_partial" "$snapshot"
    mv "$snapshot_report_partial" "$snapshot_report"
  else
    mkdir "$session_dir"
    chmod 700 "$session_dir"
    printf 'snapshot acquisition failed; see %s\n' "$snapshot_report_partial" > "$session_dir/snapshot-acquisition.partial"
    chmod 600 "$session_dir/snapshot-acquisition.partial" "$snapshot_report_partial"
    partial_sessions+=("$session_dir")
    ((failed_windows += 1))
    echo "window $name snapshot acquisition failed; preserving partial evidence and excluding it" >&2
    continue
  fi
  assert_tuple_unchanged

  HERMES_FEED_BIN="$feed_bin" \
  HERMES_LAUNCHPAD_PAPER_BIN="$paper_bin" \
  HERMES_LAUNCHPAD_RECONCILE_BIN="$reconcile_bin" \
  HERMES_LAUNCHPAD_CHAIN_HEAD_BIN="$head_bin" \
  HERMES_LAUNCHPAD_READINESS_BIN="$readiness_bin" \
    "$local_runner" "$frozen_pins" "$snapshot" "$session_dir" ${probe_args[@]+"${probe_args[@]}"} &
  active_pid=$!
  if wait "$active_pid"; then
    active_pid=
    if [[ ! -f "$session_dir/session-completion-manifest.json" ]]; then
      echo "window $name returned success without a completion manifest" >&2
      exit 70
    fi
    completed_sessions+=("$session_dir")
  else
    active_pid=
    partial_sessions+=("$session_dir")
    ((failed_windows += 1))
    echo "window $name failed; preserving partial evidence and excluding it from trusted aggregation" >&2
  fi
done

if [[ "${#completed_sessions[@]}" -eq 0 ]]; then
  echo "campaign has no completed sessions to aggregate" >&2
  exit 70
fi
assert_tuple_unchanged
assert_pins_unchanged
assert_orchestration_unchanged

readiness_partial=$campaign_dir/authoritative-readiness.jsonl.partial
readiness_output=$campaign_dir/authoritative-readiness.jsonl
readiness_args=(--expected-self-keccak256 "$readiness_digest")
for session in "${completed_sessions[@]}"; do readiness_args+=(--session-dir "$session"); done
"$readiness_bin" "${readiness_args[@]}" > "$readiness_partial"
chmod 600 "$readiness_partial"
mv "$readiness_partial" "$readiness_output"
assert_tuple_unchanged
assert_pins_unchanged
assert_orchestration_unchanged

report_partial=$campaign_dir/launchpad-evidence-report.jsonl.partial
report_output=$campaign_dir/launchpad-evidence-report.jsonl
report_args=(--expected-self-keccak256 "$report_digest" --campaign-lock "$campaign_lock" --snapshot-keccak256 "$snapshot_digest" --local-runner-keccak256 "$local_runner_digest" --readiness-output "$readiness_output" --expected-pins "$frozen_pins" --readiness-keccak256 "$readiness_digest")
for session in "${completed_sessions[@]}"; do report_args+=(--session-dir "$session"); done
if [[ "$failed_windows" -gt 0 ]]; then
  for session in "${partial_sessions[@]}"; do report_args+=(--partial-session-dir "$session"); done
fi
"$report_bin" "${report_args[@]}" > "$report_partial"
chmod 600 "$report_partial"
assert_tuple_unchanged
assert_pins_unchanged
assert_orchestration_unchanged
[[ "$("$report_bin" --print-file-keccak256 "$campaign_lock")" == "$campaign_lock_digest" ]] || { echo "campaign lock changed before report publication" >&2; exit 70; }
mv "$report_partial" "$report_output"

if [[ "$failed_windows" -ne 0 ]]; then
  echo "campaign report excludes $failed_windows failed or partial window(s)" >&2
  exit 70
fi
