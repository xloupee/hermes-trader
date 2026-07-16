# V2 paper-trader validation

## Safety boundary

The paper command consumes typed candidate records only. It has no wallet,
key-loading, signing, RPC, or transaction-submission dependency. It can replay
a JSONL capture or consume the live probe on standard input.

It rejects:

- missing typed V2 intent;
- zero input or observed minimum output;
- expired deadline (with configurable grace);
- path length above policy;
- arithmetic overflow; and
- a proportional minimum output that rounds to zero.

## Captured fixture

The read-only live V2 capture at `.runtime/v2-observe.jsonl` contains 46 typed
candidate records. Independent JSON inspection found 43 policy-eligible
records and 3 records with zero `amountOutMin`.

The release build replayed all 46 records successfully:

```text
43 follow
 3 reject:zero_minimum_output
```

The library and binary suites pass 20 tests (15 + 5), including policy caps,
proportional minimum scaling, expiry, zero minimum, scaling-to-zero, clock
correction, and parser coverage. The replayed release binary has SHA-256
`6f5a8d1b89b50a3534dc3b311641d484520cfbe77da2e82261a6c5a0fb350698`.

## Unified launchpad observer

`hermes-launchpad-paper` is a separate paper-only observer for Bow,
LaunchHood V3, Clanker, Bankr/Doppler, Pons, Flap, Hood, and the explicitly
gated discovery profiles. It exposes no signer or broadcaster. Every matched
record includes the Nitro receive sequence, L1 block/timestamp, local receive
time, and observer latency. It also emits:

- an independent sizing/slippage/exit plan that never reuses leader amounts;
- `expected_output: null` and `min_receive: null` until a warm independently
  validated market quote exists; and
- an asynchronous receipt/event reconciliation request which is never an
  initial decision dependency.

The live topology is:

```text
hermes-feed probe --record <mode-0600 FIFO>   (stdout remains metrics only)
  -> tee raw-feed.jsonl
  -> hermes-launchpad-paper --input -
```

Receipt/event evidence must be collected independently and may be supplied
after feed EOF with `--reconciliation-input <evidence.jsonl>`. Each JSONL row
contains `tx_hash`, `launchpad`, `receipt_status`, `protocol_event_match`, and
`observed_unix_ns`. The final metrics distinguish confirmed observations,
false positives, missed transactions, unreconciled observations, and p50/p95
reconciliation latency. Missing evidence is `unreconciled`, never silently
counted as a false positive.
