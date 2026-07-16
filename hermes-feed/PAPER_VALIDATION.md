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

The checked-in local runner enforces a mode-0600 FIFO, a mode-0700 output
directory, separate raw-feed/observer/probe-metrics files, and distinct
expected/observed pin documents:

```sh
cargo build --release \
  --bin hermes-feed \
  --bin hermes-launchpad-paper \
  --bin hermes-launchpad-reconcile \
  --bin hermes-launchpad-v3-paper-quote

scripts/run-launchpad-paper-local.sh \
  config/launchpad-expected-pins.production.json \
  .runtime/launchpad-observed-startup.json \
  .runtime/launchpad-paper-session
```

Receipt/event evidence must be collected independently and may be supplied
after feed EOF with `--reconciliation-input <evidence.jsonl>`. Each JSONL row
contains `tx_hash`, `launchpad`, `receipt_status`, `protocol_event_match`, and
`observed_unix_ns`. For fully validated Bow and LaunchHood V3 receipts, the
same stream also includes a typed `launchpad_v3_paper_quote` row. The final
metrics distinguish confirmed observations, false positives, missed
transactions, unreconciled observations, and p50/p95/p99 reconciliation
latency. Missing evidence is `unreconciled`, never silently counted as a false
positive.

The read-only collector produces that evidence directly from observer output:

```sh
target/release/hermes-launchpad-reconcile \
  --input .runtime/launchpad-paper-session/launchpad-paper.jsonl \
  > .runtime/launchpad-paper-session/reconciliation-evidence.jsonl
```

It accepts only successful chain-4663 receipts with exact protocol emitter and
event signatures for Bow, LaunchHood V3, Clanker, Bankr/Doppler, Pons, Hood,
Flap, Noxa, and Klik. Trench and LeaveHood remain unmatched until their event
semantics are independently verified.

Bow and LaunchHood V3 receive stronger reconciliation than topic matching.
The collector fetches the original transaction off-path, verifies its exact
factory/sender/value/block envelope, reconstructs canonical V3 state from
ordered `PoolCreated`, `Initialize`, `Mint`, and `Swap` logs, and binds the
quote version to the receipt block hash. LaunchHood's embedded buy is accepted
only when the local V3 model exactly reproduces its input, output, ending
price, tick, and liquidity. Bow's proven zero-value profile rejects any swap.

The default independent paper size is 0.001 WETH with 1% slippage and a hard
0.01 WETH maximum. A quote row contains non-null entry output/minimum and an
immediate full-position exit quote. Feeding the mixed evidence stream back to
`hermes-launchpad-paper --reconciliation-input` emits a
`launchpad_paper_finalized_plan` row joined to the original feed sequence.
These plans remain `quoted_restriction_gated`, `execution_eligible: false`,
and `broadcast: false`; receipt-end quoting does not claim that Bow token
limits or LaunchHood's 366-L1-block wallet restriction have cleared.

One proof transaction can also be inspected directly without writing any
state:

```sh
target/release/hermes-launchpad-v3-paper-quote \
  --tx-hash 0x1adcd30a5de19423f56b93d91df33d950179ed7ef4f9d4aae31fca13f72fc009 \
  --launchpad bow
```
