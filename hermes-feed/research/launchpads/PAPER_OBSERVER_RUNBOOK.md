# Paper observer and independent reconciliation

This workflow is local, read-only, and broadcast-free. It does not load a
wallet, keystore, signer, or transaction sender.

The first complete multi-protocol local window and its fail-closed findings are
recorded in [PAPER_OBSERVER_SAMPLE_2026-07-16.md](PAPER_OBSERVER_SAMPLE_2026-07-16.md).

## Build and startup inputs

Build the runtime binaries:

```sh
cargo build --release \
  --bin hermes-feed \
  --bin hermes-launchpad-chain-head \
  --bin hermes-launchpad-paper \
  --bin hermes-launchpad-reconcile \
  --bin hermes-launchpad-readiness
```

The expected-pin document and fresh observed snapshot must be different files.
Expected values come from reviewed protocol evidence; a fresh observed hash is
never copied into expected configuration.

Schema version 4 binds the expected document to its full reviewed historical
L2 hash, L1 number, and timestamp, and binds every fresh snapshot to a
confirmed block with the same identity fields. The snapshot tool re-reads that
block after all code, storage, semantic, and proof work. EIP-1967, Safe, and
EIP-7702 implementation relationships are derived from chain state at that
block; expected addresses are comparison inputs, never observation labels.

LaunchHood startup separately requires the exact factory runtime and the
factory's reviewed immutable `TOKEN_IMPL` dependency: implementation address
`0x5fdf73abc7a232d91b03638c2f9a52c16ab0e3be`, 6,821 runtime bytes, and
runtime hash
`0xc4717d14bba5f205e8d92a9bf736e038467a353ce7053fcefa5c17da1dec6a47`.
The factory runtime binds the immutable address; the fresh implementation pin
independently binds the code at that address. Neither pin enables execution.

## Correct local topology

Use the wrapper so raw frames and probe metrics cannot be confused:

```sh
scripts/run-launchpad-paper-local.sh \
  config/launchpad-expected-pins.production.json \
  .runtime/launchpad-observed-startup.json \
  .runtime/paper-session \
  --url wss://feed.mainnet.chain.robinhood.com \
  --source direct \
  --warmup-seconds 10 \
  --duration-seconds 300 \
  --emit-tx-hashes
```

The data path uses separately tracked producer, tee, and observer children:

```text
hermes-feed probe --record private-mode-0600-raw-FIFO
  -> tee raw-feed.jsonl -> private-mode-0600-observer-FIFO
  -> hermes-launchpad-paper --acquisition live --input - < private-mode-0600-observer-FIFO
```

Probe stdout contains metrics only. The wrapper requires the exact paper
capability record, starts the probe, and waits for its connected record before
sampling the start head. While the producer is still alive it continuously
retains the latest completed head response as the cutoff anchor. It rejects a
session with any connect/read/disconnect error, waits for recorder and FIFO
drain, and checks the tee and observer exit statuses independently before
starting receipt/event work. Existing evidence paths are never overwritten.

The wrapper persists the exact start and cutoff number/hash pairs as
mode-`0600` `start-anchor.txt` and `cutoff-anchor.txt` files after a successful
run. These files are durable session evidence, are passed unchanged to the
reconciler, and are never removed by normal FIFO cleanup.

Before starting any child, the wrapper freezes the reviewed expected pins and
fresh observed snapshot as mode-`0600` files inside the session directory. The
observer, reconciler, and finalizer all hash and decode those exact bytes. The
observer capability row binds the acquisition kind, both content hashes, and
the exact observer executable hash. Reconciliation carries that authority
forward with its own executable and observer-output hashes. Finalization rejects
missing, duplicate, changed, or cross-phase provenance before emitting a
readiness row.

## Independent truth boundary

The collector is driven by the observer's explicit
`report.reconciliation_requests` handoff, not by scraping detections as an
implicit RPC work queue. Every request must name
`independent_receipt_and_protocol_events`, remain outside the initial decision
path, and match exactly one observation's transaction hash, launchpad, feed
sequence, L1 block, and L1 timestamp. Missing, duplicate, orphaned, or
provenance-mismatched requests abort collection before any evidence is scored.

The reconciler scans the exact L2 interval `(start_head, cutoff_head]`. It waits
for the configured confirmations and emits one
`launchpad_ground_truth_window` manifest. Every primary log hit retains its
block hash, transaction index, log index, emitter, and topic, and must match the
canonical successful receipt. The start and cutoff hashes are re-read after
collection to reject a reorg.

For an already captured observer window, the same read-only collector can be
run directly. Its stdout is the receipt/event and quote JSONL consumed by the
paper finalizer:

```sh
hermes-launchpad-reconcile \
  --acquisition live \
  --input .runtime/paper-session/launchpad-paper.jsonl \
  --expected-pins .runtime/paper-session/expected-pins.input.json \
  --observed-startup-snapshot .runtime/paper-session/observed-startup-snapshot.input.json \
  --concurrency 1 \
  --ground-truth-start-head "$START_HEAD" \
  --ground-truth-start-hash "$START_HASH" \
  --ground-truth-cutoff-head "$CUTOFF_HEAD" \
  --ground-truth-cutoff-hash "$CUTOFF_HASH" \
  > .runtime/paper-session/reconciliation-evidence.jsonl
```

Finalize with both immutable phase outputs and the same acquisition label and
startup-input bytes:

```sh
hermes-launchpad-paper \
  --acquisition live \
  --expected-pins .runtime/paper-session/expected-pins.input.json \
  --observed-startup-snapshot .runtime/paper-session/observed-startup-snapshot.input.json \
  --observer-output-input .runtime/paper-session/launchpad-paper.jsonl \
  --reconciliation-input .runtime/paper-session/reconciliation-evidence.jsonl \
  > .runtime/paper-session/launchpad-paper-finalized.jsonl
```

Use `--acquisition replay` from the first paper-observer phase onward when
decoding saved raw-feed bytes. Relabeling only the finalizer cannot turn replay
or old-build output into live evidence: the phase records and executable hashes
must agree end to end.

The command has no wallet, signer, keystore, transaction construction, or
broadcast interface. Expected pins and the fresh observed snapshot must remain
separate files.

Reconciliation defaults to concurrency `1` for deterministic, low-pressure
evidence collection. Set `HERMES_RECONCILE_CONCURRENCY` only for an explicitly
reviewed local run; the wrapper always forwards the selected value with
`--concurrency`.

Primary launch anchors are exact emitter/topic pairs for Bow, LaunchHood V3,
Clanker, Bankr/Doppler Airlock, current and legacy Pons, and Hood. Shared pool,
swap, locker, EntryPoint, and migration events are enrichment only. Multiple
Hood primary events in one transaction are deduplicated to one protocol key
while their individual log identities remain bound to the receipt.

## Metric semantics

The finalizer emits one metrics row for every supported launchpad, including
zero-activity rows.

- `confirmed_observations`: observer claims with an independently discovered
  canonical primary event in the anchored range.
- `missed_transactions`: all ground-truth protocol keys without a claim. This
  is intentional end-to-end protocol-event recall, so unsupported outer
  envelopes remain misses rather than being removed from the denominator.
- `detector_misses`: missed transactions whose hash was present in the raw feed
  inventory.
- `feed_coverage_misses`: missed transactions absent from that inventory.
- `false_positives`: in-range, receipt-resolved claims without ground truth.
- `out_of_scope_observations`: claims whose receipts lie outside the anchored
  interval, including initial websocket backlog.
- `observation_latency_*`: probe source-receive-to-observation latency. The
  wrapper timestamp is preserved through FIFO and tee backlog. Direct
  unwrapped inputs start this clock when the decoder receives the input. It
  never includes EOF, confirmation waiting, or RPC work.
- `reconciliation_rpc_duration_*`: separate post-EOF receipt/state RPC time.
- `quote_available`, `quote_blocked`, and `quote_not_applicable`: quote status
  over raw ground-truth transactions; quote failure never erases event truth.
- `independent_quote_validation_*`: quote-eligible ground-truth transactions
  split into missing quote records, independently rederived matches, and
  mismatches. A malformed or coordinated forged quote is counted as a mismatch
  before final plan construction fails closed.
- `entry_direction_*` and `exit_direction_*`: the same eligible population,
  scored against the independently reconciled token and the canonical quote
  asset. Typed Hood curve legs use independently rederived buy/sell semantics.

Action, token, and address-pool comparisons are counted only when independent
truth contains that field. V4 `pool_id` comparison remains separate from an
address pool and must not be coerced into the address field.

## Final paper plans

A typed quote can become a finalized paper plan only when all of these agree:

1. the complete anchored coverage manifest;
2. the independently discovered canonical event and successful receipt;
3. the quote's L2 block number, block hash, and transaction index;
4. normalized action, token, and address-pool identity where applicable;
5. `quote_status = available`; and
6. the original observer feed sequence.

Blocked, unsupported, migration-only, out-of-range, or missed transactions
produce evidence and metrics but never a finalized plan. Final plans remain
`execution_eligible: false` and `broadcast: false`.

Each finalized plan preserves the independently rederived immediate
full-position exit quote in the existing `exit_expected_output` and
`exit_min_receive` fields. That quote simulates an exit immediately after the
paper entry at the reconciled state; it is not evidence that a later policy
trigger fired. The additive `exit_plan` object separately records the configured
take-profit, stop-loss, and maximum-hold policy, including WETH-denominated
thresholds derived from the independent entry size. Its trigger source is a
future independent warm full-position quote, its static-finalization status is
not evaluated, and it always remains non-executable/non-broadcast. Zero,
out-of-range, overflowing, or entry-size-collapsed trigger policies fail plan
finalization closed.

## Meaningful-sample readiness gate

Readiness is a separate evidence aggregation step. It does not enable a wallet,
signer, broadcast path, deployment, or canary. The evaluator always emits
`authorizes_canary: false` and `execution_eligible: false`, even when
`paper_evidence_ready` is true. Promotion remains a separate, explicitly
authorized review after production-pin validation and paper evidence.

After anchored reconciliation, `hermes-launchpad-paper` automatically emits one
`launchpad_paper_readiness_window` JSON object for each of the six launchpads.
The rows are derived from the validated ground-truth manifest, reconciliation
metrics, promotion validation, and typed quote records; there is no CLI input
for hand-authored counts. Collect those emitted rows across independent runs
and feed them to:

```sh
cargo run --release --bin hermes-launchpad-readiness -- \
  --input .runtime/readiness-windows.jsonl \
  > .runtime/launchpad-readiness.jsonl
```

Each input row binds one reconciled measurement window with these fields:

```json
{
  "record_type": "launchpad_paper_readiness_window",
  "launchpad": "bankr_doppler",
  "coverage_from_l2_block": 1000,
  "coverage_to_l2_block": 1099,
  "start_head_hash": "0x1111111111111111111111111111111111111111111111111111111111111111",
  "cutoff_head_hash": "0x2222222222222222222222222222222222222222222222222222222222222222",
  "complete": true,
  "quote_eligible_confirmed_observations": 34,
  "profile_envelope_observations": {
    "curve_ticks_v1": 3,
    "curve_ticks_v2": 3,
    "direct_airlock": 3,
    "erc7579": 4
  },
  "false_positives": 0,
  "detector_misses": 0,
  "identity_mismatches": 0,
  "direction_mismatches": 0,
  "prediction_mismatches": 0,
  "quote_mismatches": 0,
  "provenance": {
    "schema_version": 1,
    "acquisition": "live",
    "expected_pins_content_keccak256": "0x...",
    "observed_snapshot_content_keccak256": "0x...",
    "observer_paper_binary_keccak256": "0x...",
    "reconciler_binary_keccak256": "0x...",
    "finalizer_paper_binary_keccak256": "0x...",
    "observer_output_content_keccak256": "0x...",
    "reconciliation_output_content_keccak256": "0x..."
  }
}
```

`complete` means receipt/event reconciliation finished and both nonzero
boundary hashes were confirmed canonical. Independent windows are
complete, non-overlapping L2 ranges. Duplicate or overlapping ranges and
incomplete windows cannot increase the sample or per-profile totals. Error
counters are conservatively accumulated over every submitted row, so discarded
or overlapping evidence cannot hide an error.

Promotion aggregation accepts only `live` rows with the same exact observer,
reconciler, and finalizer binary tuple and the same independently reviewed
expected-pin content hash. Missing provenance, replay rows, and mixed build or
expected-pin inputs fail closed. Fresh startup snapshots intentionally have
different content hashes across sessions; each hash is retained in aggregate
output after startup validation instead of forcing reuse of a stale snapshot.
Existing pre-provenance readiness rows can still be decoded by the new schema
but are explicitly ineligible for promotion and must not be backfilled by hand.

`quote_eligible_confirmed_observations` counts independently revalidated quote
records. Profile and envelope counters are derived from those same typed quote
records. Bankr curve version and outer envelope are orthogonal dimensions, so
one validated Bankr quote contributes once to its curve stratum and once to its
envelope stratum. Missing prediction or validation evidence is conservatively
counted with mismatches: token/pool fields form identity, entry/exit checks form
direction, action forms prediction, and independent quote replay forms quote.

The fixed readiness policy is evaluated independently for every launchpad:

- at least 100 quote-eligible confirmed observations;
- at least 10 observations for every supported profile or envelope;
- at least three independent complete windows; and
- zero false positives, detector misses, identity mismatches, direction
  mismatches, prediction mismatches, and quote mismatches.

Supported strata are fixed in code so an input cannot omit a difficult stratum:

- Bow: `zero_initial_buy`, `payable_initial_buy`;
- LaunchHood V3: `embedded_initial_buy`;
- Clanker: `extensionless_single_position`,
  `pinned_extension_five_position`;
- Bankr/Doppler: `curve_ticks_v1`, `curve_ticks_v2`, `curve_ticks_v3`,
  `direct_airlock`, `erc7579`;
- Pons: `current_generation`; and
- Hood: `current_curve`.

Unknown strata are rejected. Missing strata count as zero. The output always
contains one machine-readable `launchpad_paper_readiness` row for each of the
six launchpads. Missing, sparse, or no-activity launchpads therefore remain
explicitly not ready rather than disappearing from the report.
