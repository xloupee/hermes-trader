#!/bin/sh
set -eu

if [ "$#" -lt 3 ]; then
  echo "usage: $0 EXPECTED_PINS OBSERVED_SNAPSHOT OUTPUT_DIR [PROBE_ARGS...]" >&2
  exit 64
fi

expected_pins=$1
observed_snapshot=$2
output_dir=$3
shift 3

feed_bin=${HERMES_FEED_BIN:-./target/release/hermes-feed}
paper_bin=${HERMES_LAUNCHPAD_PAPER_BIN:-./target/release/hermes-launchpad-paper}
reconcile_bin=${HERMES_LAUNCHPAD_RECONCILE_BIN:-./target/release/hermes-launchpad-reconcile}

if [ ! -f "$expected_pins" ] || [ ! -f "$observed_snapshot" ]; then
  echo "expected pins and observed snapshot must be existing regular files" >&2
  exit 66
fi
if [ "$expected_pins" -ef "$observed_snapshot" ]; then
  echo "expected pins and observed snapshot must be independent files" >&2
  exit 65
fi
if [ ! -x "$feed_bin" ] || [ ! -x "$paper_bin" ] || [ ! -x "$reconcile_bin" ]; then
  echo "build release binaries first: cargo build --release --bin hermes-feed --bin hermes-launchpad-paper --bin hermes-launchpad-reconcile" >&2
  exit 69
fi

umask 077
mkdir -p "$output_dir"
chmod 700 "$output_dir"

fifo=$output_dir/raw-feed.fifo
raw_feed=$output_dir/raw-feed.jsonl
observer_output=$output_dir/launchpad-paper.jsonl
probe_metrics=$output_dir/probe-metrics.jsonl
reconciliation_output=$output_dir/reconciliation-evidence.jsonl
finalized_output=$output_dir/launchpad-paper-finalized.jsonl

if [ -e "$fifo" ]; then
  echo "refusing to replace existing FIFO path $fifo" >&2
  exit 73
fi

mkfifo "$fifo"
chmod 600 "$fifo"

pipeline_pid=
cleanup() {
  if [ -n "$pipeline_pid" ]; then kill "$pipeline_pid" 2>/dev/null || true; fi
  wait "$pipeline_pid" 2>/dev/null || true
  rm -f "$fifo"
}
trap cleanup EXIT INT TERM HUP

tee "$raw_feed" < "$fifo" | "$paper_bin" \
  --expected-pins "$expected_pins" \
  --observed-startup-snapshot "$observed_snapshot" \
  --input - \
  > "$observer_output" &
pipeline_pid=$!

# The probe writes replayable raw frames only to the private FIFO. Its stdout
# remains a metrics stream and is captured separately.
"$feed_bin" probe --record "$fifo" "$@" > "$probe_metrics"
wait "$pipeline_pid"
pipeline_pid=

# Receipt/event work begins only after the feed pipeline has closed. The
# collector also appends fully validated Bow/LaunchHood V3 and Clanker V4
# quote records, using the same reviewed-vs-observed startup pin boundary.
"$reconcile_bin" \
  --input "$observer_output" \
  --expected-pins "$expected_pins" \
  --observed-startup-snapshot "$observed_snapshot" \
  > "$reconciliation_output"

# Join against the original observer timestamps and feed sequences. Do not
# replay the raw feed here: replay-time timestamps would corrupt latency.
"$paper_bin" \
  --expected-pins "$expected_pins" \
  --observed-startup-snapshot "$observed_snapshot" \
  --observer-output-input "$observer_output" \
  --reconciliation-input "$reconciliation_output" \
  > "$finalized_output"

chmod 600 "$raw_feed" "$observer_output" "$probe_metrics" \
  "$reconciliation_output" "$finalized_output"
