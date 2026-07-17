#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
runner=$script_dir/run-launchpad-paper-campaign.sh
fixture_dir=$script_dir/../tests/fixtures/launchpad-evidence-report
tmp_dir=$(mktemp -d)
trap 'rm -rf "$tmp_dir"' EXIT
bin_dir=$tmp_dir/bin
mkdir "$bin_dir"

make_digest_bin() {
  local name=$1 digit=$2
  local path=$bin_dir/$name
  cp /dev/null "$path"
  chmod 600 "$path"
  printf '%s\n' '#!/usr/bin/env bash' 'set -euo pipefail' \
    '[[ "$#" -eq 1 && "$1" == --print-self-digest ]]' \
    "printf '0x%064d\\n' $digit" >> "$path"
  chmod 700 "$path"
}
make_digest_bin hermes-feed 1
make_digest_bin hermes-launchpad-paper 2
make_digest_bin hermes-launchpad-reconcile 3
make_digest_bin hermes-launchpad-chain-head 4

cat > "$bin_dir/hermes-launchpad-readiness" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$#" -eq 1 && "$1" == --print-self-digest ]]; then
  printf '0x%064d\n' 5
  exit 0
fi
printf '{"record_type":"launchpad_paper_readiness","launchpad":"bow","paper_evidence_ready":false,"authorizes_canary":false,"execution_eligible":false,"input_trust":"completed_session_manifest","failures":[]}\n'
SH

cat > "$bin_dir/hermes-launchpad-pin-snapshot" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
output=
while [[ "$#" -gt 0 ]]; do
  if [[ "$1" == --snapshot-output ]]; then output=$2; shift 2; else shift; fi
done
name=$(basename "$output")
name=${name%.partial}
name=${name%.json}
fixture=$FIXTURE_DIR/success-window-a.json
case "$name" in
  window-b) fixture=$FIXTURE_DIR/success-window-b.json ;;
  stale) fixture=$FIXTURE_DIR/stale-window.json ;;
  overlap) fixture=$FIXTURE_DIR/overlap-window.json ;;
  tamper) fixture=$FIXTURE_DIR/hash-tamper-window.json ;;
esac
cp "$fixture" "$output"
printf '{"record_type":"snapshot_report","window":"%s"}\n' "$name"
SH

cat > "$bin_dir/local-runner" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
snapshot=$2
session=$3
name=$(basename "$session")
mkdir "$session"
chmod 700 "$session"
cp "$snapshot" "$session/window-fixture.json"
chmod 600 "$session/window-fixture.json"
printf 'partial\n' > "$session/raw-feed.jsonl.partial"
chmod 600 "$session/raw-feed.jsonl.partial"
if [[ "${TEST_SCENARIO:-}" == interrupt ]]; then
  printf 'ready\n' > "$INTERRUPT_SENTINEL"
  trap 'exit 143' TERM INT HUP
  while :; do sleep 1; done
fi
if [[ "${TEST_SCENARIO:-}" == partial && "$name" == window-b ]]; then exit 19; fi
printf '{"record_type":"launchpad_paper_session_completion","completed":true}\n' > "$session/session-completion-manifest.json"
chmod 600 "$session/session-completion-manifest.json"
if [[ "${TEST_SCENARIO:-}" == tuple && "$name" == window-a ]]; then
  # Replacement is observed by the campaign's next preflight.
  sed -i.bak 's/printf '\''0x%064d\\n'\'' 1/printf '\''0x%064d\\n'\'' 9/' "$HERMES_FEED_BIN"
  rm -f "$HERMES_FEED_BIN.bak"
fi
SH

cat > "$bin_dir/hermes-launchpad-evidence-report" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$1" == --print-file-keccak256 ]]; then
  printf '0x%064d\n' 6
  exit 0
fi
if [[ "$1" == --print-self-digest ]]; then
  printf '0x%064d\n' 8
  exit 0
fi
args=" $* "
case "${TEST_SCENARIO:-}" in
  stale) echo 'observed startup snapshot is stale at session start' >&2; exit 70 ;;
  overlap) echo 'completed campaign windows overlap' >&2; exit 70 ;;
  tamper) echo 'session artifact changed after completion' >&2; exit 70 ;;
esac
if [[ "${TEST_SCENARIO:-}" == partial ]]; then
  [[ "$args" == *" --partial-session-dir "*"window-b"* ]]
fi
count=0
for arg in "$@"; do [[ "$arg" == --session-dir ]] && ((count += 1)) || true; done
printf '{"record_type":"launchpad_paper_campaign_report","accepted_window_count":%s}\n' "$count"
SH
chmod 700 "$bin_dir"/*

expected_pins=$tmp_dir/expected-pins.json
printf '{"reviewed":true}\n' > "$expected_pins"
chmod 600 "$expected_pins"
reviewed_digest=$(printf '0x%064d' 6)

export HERMES_FEED_BIN=$bin_dir/hermes-feed
export HERMES_LAUNCHPAD_PAPER_BIN=$bin_dir/hermes-launchpad-paper
export HERMES_LAUNCHPAD_RECONCILE_BIN=$bin_dir/hermes-launchpad-reconcile
export HERMES_LAUNCHPAD_CHAIN_HEAD_BIN=$bin_dir/hermes-launchpad-chain-head
export HERMES_LAUNCHPAD_READINESS_BIN=$bin_dir/hermes-launchpad-readiness
export HERMES_LAUNCHPAD_PIN_SNAPSHOT_BIN=$bin_dir/hermes-launchpad-pin-snapshot
export HERMES_LAUNCHPAD_EVIDENCE_REPORT_BIN=$bin_dir/hermes-launchpad-evidence-report
export HERMES_LAUNCHPAD_LOCAL_RUNNER=$bin_dir/local-runner
export FIXTURE_DIR=$fixture_dir

assert_mode() {
  local path=$1 expected=$2 mode
  if stat -f '%Lp' "$path" > "$tmp_dir/mode" 2>/dev/null; then read -r mode < "$tmp_dir/mode"; else mode=$(stat -c '%a' "$path"); fi
  [[ "$mode" == "$expected" ]]
}

# Successful multi-window aggregate and fresh, distinct snapshots.
success=$tmp_dir/success
TEST_SCENARIO= "$runner" "$expected_pins" "$reviewed_digest" "$success" window-a window-b -- --source fake
grep -q '"accepted_window_count":2' "$success/launchpad-evidence-report.jsonl"
! cmp -s "$success/snapshots/window-a.json" "$success/snapshots/window-b.json"
assert_mode "$success" 700
assert_mode "$success/campaign-lock.json" 600
assert_mode "$success/snapshots/window-a.json" 600

# A failed window remains on disk, is explicitly excluded, and makes the campaign nonzero.
partial=$tmp_dir/partial
if TEST_SCENARIO=partial "$runner" "$expected_pins" "$reviewed_digest" "$partial" window-a window-b --; then
  echo "campaign accepted a partial window" >&2; exit 1
fi
[[ -f "$partial/windows/window-b/raw-feed.jsonl.partial" ]]
[[ -f "$partial/launchpad-evidence-report.jsonl" ]]

# Executable tuple replacement is detected before another window starts.
tuple=$tmp_dir/tuple
if TEST_SCENARIO=tuple "$runner" "$expected_pins" "$reviewed_digest" "$tuple" window-a window-b -- > /dev/null 2> "$tmp_dir/tuple.err"; then
  echo "campaign accepted tuple drift" >&2; exit 1
fi
grep -q 'feed executable changed during campaign' "$tmp_dir/tuple.err"
make_digest_bin hermes-feed 1

# Reviewed pin mismatch fails before snapshot acquisition.
if "$runner" "$expected_pins" "$(printf '0x%064d' 7)" "$tmp_dir/pin-mismatch" window-a -- > /dev/null 2> "$tmp_dir/pin.err"; then
  echo "campaign accepted expected-pin mismatch" >&2; exit 1
fi
grep -q 'independently reviewed digest' "$tmp_dir/pin.err"
[[ ! -e "$tmp_dir/pin-mismatch/snapshots/window-a.json" ]]

# Focused stale, overlap, and tamper fixtures fail closed in validation/reporting.
for scenario in stale overlap tamper; do
  output=$tmp_dir/$scenario
  if TEST_SCENARIO=$scenario "$runner" "$expected_pins" "$reviewed_digest" "$output" window-a "$scenario" -- > /dev/null 2> "$tmp_dir/$scenario.err"; then
    echo "campaign accepted $scenario fixture" >&2; exit 1
  fi
  [[ ! -e "$output/launchpad-evidence-report.jsonl" ]]
done

# Interruption forwards to the active runner and preserves partial evidence.
interrupt=$tmp_dir/interrupt
export INTERRUPT_SENTINEL=$tmp_dir/interrupt-ready
TEST_SCENARIO=interrupt "$runner" "$expected_pins" "$reviewed_digest" "$interrupt" window-a -- > /dev/null 2> "$tmp_dir/interrupt.err" &
campaign_pid=$!
for _ in $(seq 1 200); do [[ -f "$INTERRUPT_SENTINEL" ]] && break; kill -0 "$campaign_pid" 2>/dev/null || break; sleep 0.01; done
[[ -f "$INTERRUPT_SENTINEL" ]]
kill -TERM "$campaign_pid"
if wait "$campaign_pid"; then echo "interrupted campaign returned success" >&2; exit 1; fi
[[ -f "$interrupt/windows/window-a/raw-feed.jsonl.partial" ]]
[[ ! -e "$interrupt/launchpad-evidence-report.jsonl" ]]

echo "run-launchpad-paper-campaign fixture tests passed"
