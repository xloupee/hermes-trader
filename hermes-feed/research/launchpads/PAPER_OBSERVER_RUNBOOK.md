# Paper observer and independent reconciliation

This workflow is local, read-only, and broadcast-free. It does not load a
wallet, keystore, signer, or transaction sender.

The first complete multi-protocol local window and its fail-closed findings are
recorded in [PAPER_OBSERVER_SAMPLE_2026-07-16.md](PAPER_OBSERVER_SAMPLE_2026-07-16.md).

## Build and startup inputs

Build the four runtime binaries:

```sh
cargo build --release \
  --bin hermes-feed \
  --bin hermes-launchpad-chain-head \
  --bin hermes-launchpad-paper \
  --bin hermes-launchpad-reconcile
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
  -> hermes-launchpad-paper --input observer-FIFO
```

Probe stdout contains metrics only. The wrapper requires the exact paper
capability record, starts the probe, and waits for its connected record before
sampling the start head. While the producer is still alive it continuously
retains the latest completed head response as the cutoff anchor. It rejects a
session with any connect/read/disconnect error, waits for recorder and FIFO
drain, and checks the tee and observer exit statuses independently before
starting receipt/event work. Existing evidence paths are never overwritten.

## Independent truth boundary

The reconciler scans the exact L2 interval `(start_head, cutoff_head]`. It waits
for the configured confirmations and emits one
`launchpad_ground_truth_window` manifest. Every primary log hit retains its
block hash, transaction index, log index, emitter, and topic, and must match the
canonical successful receipt. The start and cutoff hashes are re-read after
collection to reject a reorg.

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
