#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
runner=$script_dir/run-launchpad-paper-local.sh
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT

bin_dir=$tmp_dir/bin
output_dir=$tmp_dir/evidence
mkdir -p "$bin_dir"
expected_pins=$tmp_dir/expected-pins.json
observed_snapshot=$tmp_dir/observed-snapshot.json
printf '{"kind":"expected"}\n' > "$expected_pins"
printf '{"kind":"observed"}\n' > "$observed_snapshot"

cat > "$bin_dir/hermes-feed" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$#" -eq 1 && "$1" == --print-self-digest ]]; then
  printf '0x%064d\n' 1
  exit 0
fi
[[ "$1" == --expected-self-keccak256 && "$2" == 0x0000000000000000000000000000000000000000000000000000000000000001 && "$3" == probe && "$4" == --record ]]
record_path=$5
printf '{"state":"connected"}\n'
printf '{"frame":1}\n{"frame":2}\n' > "$record_path"
sleep 1
printf '{"state":"coverage_closed"}\n'
SH

cat > "$bin_dir/hermes-launchpad-paper" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$#" -eq 1 && "$1" == --print-self-digest ]]; then
  printf '0x%064d\n' 2
  exit 0
fi
if [[ "${FAKE_REPLACE_MISMATCH:-false}" == true ]]; then
  echo "current executable digest does not match launcher preflight" >&2
  exit 70
fi
[[ "$1" == --expected-self-keccak256 && "$2" == 0x0000000000000000000000000000000000000000000000000000000000000002 ]]
count=0
if [[ -f "$FAKE_PAPER_COUNT" ]]; then
  read -r count < "$FAKE_PAPER_COUNT"
fi
count=$((count + 1))
printf '%s\n' "$count" > "$FAKE_PAPER_COUNT"
printf '%s\n' "$@" > "$FAKE_STATE_DIR/paper-$count.args"
if [[ "$count" -eq 1 ]]; then
  printf '{"record_type":"launchpad_paper_capabilities"}\n'
  cat > "$FAKE_OBSERVER_BYTES"
  printf '{"record_type":"launchpad_paper_observation"}\n'
else
  if [[ "${FAKE_FAIL_FINALIZER:-false}" == true ]]; then
    exit 19
  fi
  printf '{"record_type":"launchpad_paper_finalized_plan","execution_eligible":false,"broadcast":false}\n'
fi
SH

cat > "$bin_dir/hermes-launchpad-chain-head" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$#" -eq 1 && "$1" == --print-self-digest ]]; then
  printf '0x%064d\n' 4
  exit 0
fi
[[ "$1" == --expected-self-keccak256 && "$2" == 0x0000000000000000000000000000000000000000000000000000000000000004 && "$3" == --shell-fields ]]
count=0
if [[ -f "$FAKE_HEAD_COUNT" ]]; then
  read -r count < "$FAKE_HEAD_COUNT"
fi
count=$((count + 1))
printf '%s\n' "$count" > "$FAKE_HEAD_COUNT"
printf '%s hash-%s\n' "$((100 + count))" "$count"
SH

cat > "$bin_dir/hermes-launchpad-reconcile" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$#" -eq 1 && "$1" == --print-self-digest ]]; then
  printf '0x%064d\n' 3
  exit 0
fi
[[ "$1" == --expected-self-keccak256 && "$2" == 0x0000000000000000000000000000000000000000000000000000000000000003 ]]
printf '%s\n' "$@" > "$FAKE_RECONCILE_ARGS"
printf '{"record_type":"launchpad_reconciliation_evidence"}\n'
SH

cat > "$bin_dir/hermes-launchpad-readiness" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$#" -eq 1 && "$1" == --print-self-digest ]]; then
  printf '0x%064d\n' 5
  exit 0
fi
[[ "$1" == --expected-self-keccak256 && "$2" == 0x0000000000000000000000000000000000000000000000000000000000000005 && "$3" == --complete-session ]]
output_dir=$4
[[ "$5" == --paper-bin && "$6" == "$HERMES_LAUNCHPAD_PAPER_BIN" ]]
printf '{"record_type":"launchpad_paper_session_completion","completed":true}\n' > "$output_dir/session-completion-manifest.json"
chmod 600 "$output_dir/session-completion-manifest.json"
SH

chmod +x "$bin_dir"/*

export HERMES_FEED_BIN=$bin_dir/hermes-feed
export HERMES_LAUNCHPAD_PAPER_BIN=$bin_dir/hermes-launchpad-paper
export HERMES_LAUNCHPAD_RECONCILE_BIN=$bin_dir/hermes-launchpad-reconcile
export HERMES_LAUNCHPAD_CHAIN_HEAD_BIN=$bin_dir/hermes-launchpad-chain-head
export HERMES_LAUNCHPAD_READINESS_BIN=$bin_dir/hermes-launchpad-readiness
export FAKE_STATE_DIR=$tmp_dir
export FAKE_PAPER_COUNT=$tmp_dir/paper-count
export FAKE_HEAD_COUNT=$tmp_dir/head-count
export FAKE_OBSERVER_BYTES=$tmp_dir/observer-stdin.jsonl
export FAKE_RECONCILE_ARGS=$tmp_dir/reconcile.args
unset HERMES_RECONCILE_CONCURRENCY || true

"$runner" "$expected_pins" "$observed_snapshot" "$output_dir" --source fake

first_paper_args=()
while IFS= read -r arg; do
  first_paper_args+=("$arg")
done < "$tmp_dir/paper-1.args"
input_dash=false
paper_live=false
for ((index = 0; index + 1 < ${#first_paper_args[@]}; index += 1)); do
  if [[ "${first_paper_args[index]}" == --input && "${first_paper_args[index + 1]}" == - ]]; then
    input_dash=true
    break
  fi
  if [[ "${first_paper_args[index]}" == --acquisition && "${first_paper_args[index + 1]}" == live ]]; then
    paper_live=true
  fi
done
if [[ "$input_dash" != true ]]; then
  echo "paper observer did not receive --input -" >&2
  exit 1
fi
if [[ "$paper_live" != true ]]; then
  echo "paper observer did not receive --acquisition live" >&2
  exit 1
fi

cmp "$expected_pins" "$output_dir/expected-pins.input.json"
cmp "$observed_snapshot" "$output_dir/observed-startup-snapshot.input.json"

cmp "$output_dir/raw-feed.jsonl" "$FAKE_OBSERVER_BYTES"
if [[ $(cat "$output_dir/raw-feed.jsonl") != $'{"frame":1}\n{"frame":2}' ]]; then
  echo "raw tee did not preserve the deterministic probe bytes" >&2
  exit 1
fi

for anchor in start-anchor.txt cutoff-anchor.txt expected-pins.input.json observed-startup-snapshot.input.json session-completion-manifest.json; do
  if [[ ! -s "$output_dir/$anchor" ]]; then
    echo "$anchor was not persisted after success" >&2
    exit 1
  fi
  if stat -f '%Lp' "$output_dir/$anchor" > "$tmp_dir/mode" 2>/dev/null; then
    read -r mode < "$tmp_dir/mode"
  else
    mode=$(stat -c '%a' "$output_dir/$anchor")
  fi
  if [[ "$mode" != 600 ]]; then
    echo "$anchor was not persisted mode 0600" >&2
    exit 1
  fi
done

reconcile_args=()
while IFS= read -r arg; do
  reconcile_args+=("$arg")
done < "$FAKE_RECONCILE_ARGS"
arg_value() {
  local name=$1
  local index
  for ((index = 0; index + 1 < ${#reconcile_args[@]}; index += 1)); do
    if [[ "${reconcile_args[index]}" == "$name" ]]; then
      printf '%s\n' "${reconcile_args[index + 1]}"
      return 0
    fi
  done
  return 1
}

if [[ "$(arg_value --concurrency)" != 1 ]]; then
  echo "reconciliation did not default to concurrency 1" >&2
  exit 1
fi
if [[ "$(arg_value --acquisition)" != live ]]; then
  echo "reconciliation did not receive --acquisition live" >&2
  exit 1
fi

second_paper_args=()
while IFS= read -r arg; do
  second_paper_args+=("$arg")
done < "$tmp_dir/paper-2.args"
finalizer_live=false
for ((index = 0; index + 1 < ${#second_paper_args[@]}; index += 1)); do
  if [[ "${second_paper_args[index]}" == --acquisition && "${second_paper_args[index + 1]}" == live ]]; then
    finalizer_live=true
    break
  fi
done
if [[ "$finalizer_live" != true ]]; then
  echo "paper finalizer did not receive --acquisition live" >&2
  exit 1
fi
read -r start_head start_hash < "$output_dir/start-anchor.txt"
read -r cutoff_head cutoff_hash < "$output_dir/cutoff-anchor.txt"
[[ "$(arg_value --ground-truth-start-head)" == "$start_head" ]]
[[ "$(arg_value --ground-truth-start-hash)" == "$start_hash" ]]
[[ "$(arg_value --ground-truth-cutoff-head)" == "$cutoff_head" ]]
[[ "$(arg_value --ground-truth-cutoff-hash)" == "$cutoff_hash" ]]

evidence_paths=(
  raw-feed.jsonl
  launchpad-paper.jsonl
  probe-metrics.jsonl
  reconciliation-evidence.jsonl
  launchpad-paper-finalized.jsonl
  start-anchor.txt
  cutoff-anchor.txt
  expected-pins.input.json
  observed-startup-snapshot.input.json
  session-completion-manifest.json
)
before=$tmp_dir/evidence-before.sha256
after=$tmp_dir/evidence-after.sha256
for path in "${evidence_paths[@]}"; do
  shasum -a 256 "$output_dir/$path"
done > "$before"

if "$runner" "$expected_pins" "$observed_snapshot" "$output_dir" --source fake \
  > "$tmp_dir/overwrite.out" 2> "$tmp_dir/overwrite.err"; then
  echo "wrapper overwrote an existing evidence directory" >&2
  exit 1
fi
grep -q 'refusing to overwrite existing evidence path' "$tmp_dir/overwrite.err"
for path in "${evidence_paths[@]}"; do
  shasum -a 256 "$output_dir/$path"
done > "$after"
cmp "$before" "$after"

# A failed finalizer leaves only its diagnostic partial and can never publish a
# canonical finalized file or completion manifest.
failure_output=$tmp_dir/finalizer-failure
rm -f "$FAKE_PAPER_COUNT" "$FAKE_HEAD_COUNT"
export FAKE_FAIL_FINALIZER=true
if "$runner" "$expected_pins" "$observed_snapshot" "$failure_output" --source fake \
  > "$tmp_dir/finalizer-failure.out" 2> "$tmp_dir/finalizer-failure.err"; then
  echo "wrapper accepted a failed finalizer" >&2
  exit 1
fi
unset FAKE_FAIL_FINALIZER
[[ -e "$failure_output/launchpad-paper-finalized.jsonl.partial" ]]
[[ ! -e "$failure_output/launchpad-paper-finalized.jsonl" ]]
[[ ! -e "$failure_output/session-completion-manifest.json" ]]

# Simulate replacement after launcher preflight: the child rejects the stale
# expected-self digest and no canonical observer or completion marker appears.
replacement_output=$tmp_dir/replacement-failure
rm -f "$FAKE_PAPER_COUNT" "$FAKE_HEAD_COUNT"
export FAKE_REPLACE_MISMATCH=true
if "$runner" "$expected_pins" "$observed_snapshot" "$replacement_output" --source fake \
  > "$tmp_dir/replacement.out" 2> "$tmp_dir/replacement.err"; then
  echo "wrapper accepted an executable self-digest mismatch" >&2
  exit 1
fi
unset FAKE_REPLACE_MISMATCH
grep -q 'current executable digest does not match launcher preflight' "$tmp_dir/replacement.err"
[[ -e "$replacement_output/launchpad-paper.jsonl.partial" ]]
[[ ! -e "$replacement_output/launchpad-paper.jsonl" ]]
[[ ! -e "$replacement_output/session-completion-manifest.json" ]]

echo "run-launchpad-paper-local regression passed"
