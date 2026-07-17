#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -lt 3 ]]; then
  echo "usage: $0 EXPECTED_PINS OBSERVED_SNAPSHOT OUTPUT_DIR [PROBE_ARGS...]" >&2
  exit 64
fi

expected_pins_source=$1
observed_snapshot_source=$2
output_dir=$3
shift 3
probe_args=("$@")

feed_bin=${HERMES_FEED_BIN:-./target/release/hermes-feed}
paper_bin=${HERMES_LAUNCHPAD_PAPER_BIN:-./target/release/hermes-launchpad-paper}
reconcile_bin=${HERMES_LAUNCHPAD_RECONCILE_BIN:-./target/release/hermes-launchpad-reconcile}
head_bin=${HERMES_LAUNCHPAD_CHAIN_HEAD_BIN:-./target/release/hermes-launchpad-chain-head}
readiness_bin=${HERMES_LAUNCHPAD_READINESS_BIN:-./target/release/hermes-launchpad-readiness}
reconcile_concurrency=${HERMES_RECONCILE_CONCURRENCY:-1}

if [[ ! -f "$expected_pins_source" || ! -f "$observed_snapshot_source" ]]; then
  echo "expected pins and observed snapshot must be existing regular files" >&2
  exit 66
fi
if [[ "$expected_pins_source" -ef "$observed_snapshot_source" ]]; then
  echo "expected pins and observed snapshot must be independent files" >&2
  exit 65
fi
if [[ ! -x "$feed_bin" || ! -x "$paper_bin" || ! -x "$reconcile_bin" || ! -x "$head_bin" || ! -x "$readiness_bin" ]]; then
  echo "build release binaries first: cargo build --release --bin hermes-feed --bin hermes-launchpad-paper --bin hermes-launchpad-reconcile --bin hermes-launchpad-chain-head --bin hermes-launchpad-readiness" >&2
  exit 69
fi

preflight_digest() {
  local path=$1
  local digest
  digest=$("$path" --print-self-digest)
  if [[ ! "$digest" =~ ^0x[0-9a-fA-F]{64}$ ]]; then
    echo "executable $path returned an invalid self digest" >&2
    exit 69
  fi
  printf '%s\n' "$digest"
}

# Capture every executable's exact bytes before any phase starts. Each process
# recomputes and verifies this value after exec, closing pathname-replacement
# races between launcher preflight and process startup.
feed_digest=$(preflight_digest "$feed_bin")
paper_digest=$(preflight_digest "$paper_bin")
reconcile_digest=$(preflight_digest "$reconcile_bin")
head_digest=$(preflight_digest "$head_bin")
readiness_digest=$(preflight_digest "$readiness_bin")

umask 077
mkdir -p "$output_dir"
chmod 700 "$output_dir"

raw_fifo=$output_dir/raw-feed.fifo
observer_fifo=$output_dir/observer-feed.fifo
raw_feed=$output_dir/raw-feed.jsonl
raw_feed_partial=$raw_feed.partial
observer_output=$output_dir/launchpad-paper.jsonl
observer_output_partial=$observer_output.partial
probe_metrics=$output_dir/probe-metrics.jsonl
probe_metrics_partial=$probe_metrics.partial
reconciliation_output=$output_dir/reconciliation-evidence.jsonl
reconciliation_output_partial=$reconciliation_output.partial
finalized_output=$output_dir/launchpad-paper-finalized.jsonl
finalized_output_partial=$finalized_output.partial
start_anchor=$output_dir/start-anchor.txt
start_anchor_partial=$start_anchor.partial
cutoff_anchor=$output_dir/cutoff-anchor.txt
cutoff_anchor_partial=$cutoff_anchor.partial
expected_pins=$output_dir/expected-pins.input.json
observed_snapshot=$output_dir/observed-startup-snapshot.input.json
completion_manifest=$output_dir/session-completion-manifest.json

for path in "$raw_fifo" "$observer_fifo" "$raw_feed" "$observer_output" \
  "$probe_metrics" "$reconciliation_output" "$finalized_output" "$start_anchor" \
  "$cutoff_anchor" "$expected_pins" "$observed_snapshot"; do
  if [[ -e "$path" ]]; then
    echo "refusing to overwrite existing evidence path $path" >&2
    exit 73
  fi
done
for path in "$raw_feed_partial" "$observer_output_partial" "$probe_metrics_partial" \
  "$reconciliation_output_partial" "$finalized_output_partial" \
  "$start_anchor_partial" "$cutoff_anchor_partial" "$completion_manifest" \
  "$completion_manifest.partial"; do
  if [[ -e "$path" ]]; then
    echo "refusing to overwrite existing evidence path $path" >&2
    exit 73
  fi
done

# Freeze both startup authorities before any child starts. Every phase hashes
# and decodes these exact bytes, so a caller cannot mutate an external path
# between observation, reconciliation, and finalization.
cp "$expected_pins_source" "$expected_pins"
cp "$observed_snapshot_source" "$observed_snapshot"
chmod 600 "$expected_pins" "$observed_snapshot"

mkfifo "$raw_fifo" "$observer_fifo"
chmod 600 "$raw_fifo" "$observer_fifo"

fifo_keeper_pid=
tee_pid=
paper_pid=
probe_pid=
sampler_pid=
cleanup() {
  local pid
  for pid in "$sampler_pid" "$probe_pid" "$fifo_keeper_pid" "$tee_pid" "$paper_pid"; do
    if [[ -n "$pid" ]]; then kill "$pid" 2>/dev/null || true; fi
  done
  for pid in "$sampler_pid" "$probe_pid" "$fifo_keeper_pid" "$tee_pid" "$paper_pid"; do
    if [[ -n "$pid" ]]; then wait "$pid" 2>/dev/null || true; fi
  done
  rm -f "$raw_fifo" "$observer_fifo"
}
trap cleanup EXIT INT TERM HUP

# Separate children make tee/raw-file failures and paper failures independently
# observable. The no-output writer keeps the raw FIFO open until probe drain is
# complete, without leaking an inherited shell descriptor.
"$paper_bin" \
  --expected-self-keccak256 "$paper_digest" \
  --acquisition live \
  --expected-pins "$expected_pins" \
  --observed-startup-snapshot "$observed_snapshot" \
  --input - \
  < "$observer_fifo" \
  > "$observer_output_partial" &
paper_pid=$!

tee "$raw_feed_partial" < "$raw_fifo" > "$observer_fifo" &
tee_pid=$!

tail -f /dev/null > "$raw_fifo" &
fifo_keeper_pid=$!

# Require the exact startup capability record, not merely nonempty output.
ready_attempts=0
until grep -q '"record_type":"launchpad_paper_capabilities"' "$observer_output_partial" 2>/dev/null; do
  if ! kill -0 "$paper_pid" 2>/dev/null || ! kill -0 "$tee_pid" 2>/dev/null; then
    echo "paper observer pipeline exited before startup readiness" >&2
    exit 70
  fi
  ((ready_attempts += 1))
  if [[ "$ready_attempts" -ge 200 ]]; then
    echo "paper observer startup readiness timed out" >&2
    exit 70
  fi
  sleep 0.05
done

# Start the websocket producer first and do not begin the scored coverage
# window until its explicit connected record is durable in the metrics file.
"$feed_bin" --expected-self-keccak256 "$feed_digest" probe --record "$raw_fifo" "${probe_args[@]}" > "$probe_metrics_partial" &
probe_pid=$!
connected_attempts=0
until grep -q '"state":"connected"' "$probe_metrics_partial" 2>/dev/null; do
  if ! kill -0 "$probe_pid" 2>/dev/null; then
    wait "$probe_pid" || true
    echo "probe exited before websocket readiness" >&2
    exit 70
  fi
  ((connected_attempts += 1))
  if [[ "$connected_attempts" -ge 600 ]]; then
    echo "probe websocket readiness timed out" >&2
    exit 70
  fi
  sleep 0.05
done

start_fields=$("$head_bin" --expected-self-keccak256 "$head_digest" --shell-fields)
start_head=${start_fields%% *}
start_hash=${start_fields#* }
printf '%s\n' "$start_fields" > "$start_anchor_partial"

# Retain the latest anchor whose RPC response completed before the probe's
# durable coverage-closed barrier. This excludes recorder drain and post-probe
# RPC time from the scored feed-coverage interval.
(
  while kill -0 "$probe_pid" 2>/dev/null \
    && ! grep -q '"state":"coverage_closed"' "$probe_metrics_partial" 2>/dev/null; do
    fields=$("$head_bin" --expected-self-keccak256 "$head_digest" --shell-fields)
    if kill -0 "$probe_pid" 2>/dev/null \
      && ! grep -q '"state":"coverage_closed"' "$probe_metrics_partial" 2>/dev/null; then
      printf '%s\n' "$fields" > "$cutoff_anchor_partial"
    fi
    sleep 0.05
  done
) &
sampler_pid=$!

if ! wait "$probe_pid"; then
  probe_pid=
  echo "probe failed" >&2
  exit 70
fi
probe_pid=
wait "$sampler_pid"
sampler_pid=

if grep -Eq '"state":"(connect_error|read_error|disconnected)"' "$probe_metrics_partial"; then
  echo "probe connection was not continuous through the scored window" >&2
  exit 70
fi
if [[ ! -s "$cutoff_anchor_partial" ]]; then
  echo "probe ended before a live cutoff anchor could be sampled" >&2
  exit 70
fi
read -r cutoff_head cutoff_hash < "$cutoff_anchor_partial"

kill "$fifo_keeper_pid" 2>/dev/null || true
wait "$fifo_keeper_pid" 2>/dev/null || true
fifo_keeper_pid=
if ! wait "$tee_pid"; then
  tee_pid=
  echo "raw-feed tee failed" >&2
  exit 70
fi
tee_pid=
if ! wait "$paper_pid"; then
  paper_pid=
  echo "paper observer failed" >&2
  exit 70
fi
paper_pid=

chmod 600 "$raw_feed_partial" "$observer_output_partial" "$probe_metrics_partial" \
  "$start_anchor_partial" "$cutoff_anchor_partial"
mv "$raw_feed_partial" "$raw_feed"
mv "$observer_output_partial" "$observer_output"
mv "$probe_metrics_partial" "$probe_metrics"
mv "$start_anchor_partial" "$start_anchor"
mv "$cutoff_anchor_partial" "$cutoff_anchor"

"$reconcile_bin" \
  --expected-self-keccak256 "$reconcile_digest" \
  --acquisition live \
  --input "$observer_output" \
  --expected-pins "$expected_pins" \
  --observed-startup-snapshot "$observed_snapshot" \
  --concurrency "$reconcile_concurrency" \
  --ground-truth-start-head "$start_head" \
  --ground-truth-start-hash "$start_hash" \
  --ground-truth-cutoff-head "$cutoff_head" \
  --ground-truth-cutoff-hash "$cutoff_hash" \
  > "$reconciliation_output_partial"
chmod 600 "$reconciliation_output_partial"
mv "$reconciliation_output_partial" "$reconciliation_output"

"$paper_bin" \
  --expected-self-keccak256 "$paper_digest" \
  --acquisition live \
  --expected-pins "$expected_pins" \
  --observed-startup-snapshot "$observed_snapshot" \
  --observer-output-input "$observer_output" \
  --reconciliation-input "$reconciliation_output" \
  > "$finalized_output_partial"
chmod 600 "$finalized_output_partial"
mv "$finalized_output_partial" "$finalized_output"

chmod 600 "$raw_feed" "$observer_output" "$probe_metrics" \
  "$reconciliation_output" "$finalized_output" "$start_anchor" "$cutoff_anchor" \
  "$expected_pins" "$observed_snapshot"

# The completion manifest is the final publication step. No failed or partial
# phase can leave a promotion-eligible session directory.
"$readiness_bin" \
  --expected-self-keccak256 "$readiness_digest" \
  --complete-session "$output_dir" \
  --feed-keccak256 "$feed_digest" \
  --paper-keccak256 "$paper_digest" \
  --reconciler-keccak256 "$reconcile_digest" \
  --chain-head-keccak256 "$head_digest"
