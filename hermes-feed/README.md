# Hermes feed probe

The NOXA-specific sparse Uniswap V3 observer and paper trader is implemented
as the separate `hermes-noxa` binary. See
[NOXA_LOW_LATENCY.md](NOXA_LOW_LATENCY.md) for the verified contract semantics,
trigger research, architecture, live pause status, commands, and promotion
gates. It shares the narrow, live-cross-checked Nitro decoder but does not alter
the V2 runtime. That decoder still has the differential-testing gate described
below before it may authorize a value-bearing trader.
The `ops/start-noxa-observer.sh`, `status-noxa-observer.sh`, and
`stop-noxa-observer.sh` scripts use a separate `.runtime/hermes-noxa` state
directory and run only the no-key/no-sender observer.
The matching `start-noxa-measurement.sh` workflow runs a bounded two-hour
parent-head/feed calibration and 30-second factory-status polling in its own
`.runtime/hermes-noxa-measurement` directory.

`hermes-feed` is the measurement-first Robinhood Chain component. It reads a
Nitro sequencer feed, verifies contiguous sequence numbers, decodes only the
L2 message kinds needed for signed Ethereum transactions, filters router
destinations before recovering senders, and emits newline-delimited latency
records.

It does not submit transactions and is safe to run without wallet keys.

## Feed topology

Start with the official Nitro relay on the same host and leave its local output
uncompressed:

```text
wss://feed.mainnet.chain.robinhood.com
  -> pinned official Nitro relay
  -> ws://127.0.0.1:9642
  -> hermes-feed probe
```

Robinhood currently documents Nitro `v3.11.2-3599aca`. One relay per probe
host can be started with:

```bash
docker run --rm \
  -p 127.0.0.1:9642:9642 \
  --entrypoint relay \
  offchainlabs/nitro-node:v3.11.2-3599aca \
  --node.feed.output.addr=0.0.0.0 \
  --node.feed.input.url=wss://feed.mainnet.chain.robinhood.com \
  --chain.id=4663
```

Pin the image version; do not use `latest` in a latency experiment.

The direct Robinhood feed is a separate experiment, not something we assume is
faster. Compare direct and relay output by identical sequence numbers.

## Run

```bash
cargo run --release -- probe \
  --url ws://127.0.0.1:9642 \
  --source fra-relay \
  --record frames.jsonl \
  --router 0x89e5db8b5aa49aa85ac63f691524311aeb649eba \
  --selector 0x38ed1739 \
  --watch 0x0000000000000000000000000000000000000002
```

The router in this example is Robinhood Chain's canonical Uniswap v2
`V2Router02`. Keep router addresses explicit rather than silently assuming the
same deployment addresses as another EVM chain. Verify it against the
[official Uniswap deployment list](https://developers.uniswap.org/docs/protocols/v2/deployments).
The selector shown is `swapExactTokensForTokens`. Selector filtering occurs
before signer recovery, so other calls to the same router stay off the
secp256k1 hot path. Repeat `--selector` for every explicitly supported method.

For matched candidates, Hermes read-only decodes these canonical v2 exact-input
methods into typed intents:

- `swapExactTokensForTokens` (`0x38ed1739`)
- `swapExactETHForTokens` (`0x7ff36ab5`)
- `swapExactTokensForETH` (`0x18cbafe5`)

The intent contains input amount, minimum output, complete path, recipient and
deadline. ETH input comes from the signed transaction value. Paths shorter than
two tokens or containing the zero address are rejected. This is observation
only; no quote, nonce, signer or transaction sender is present yet.

See [V2_VALIDATION.md](V2_VALIDATION.md) for the live feed and RPC cross-check.

With no `--router`, the program stays in decode-only mode and deliberately
does not recover every transaction signer.

Every stdout line is JSON. `frame` records contain socket-arrival time,
per-stage processing time, message counts, and cumulative sequence health.
`candidate` records appear only after destination filtering and signer
recovery. `connection` records make disconnects and reconnect attempts
explicit. The probe reconnects with bounded exponential backoff while keeping
its sequence tracker alive across connections.

Frames from the first 10 seconds of every connection are marked
`"warmup":true`; change this with `--warmup-seconds`. The comparator excludes
those catch-up frames automatically.

Replay recorded frames without the network:

```bash
cargo run --release -- replay --input frames.jsonl --source replay
```

Compare stdout logs from multiple sources by matching sequence numbers:

```bash
cargo run --release -- compare \
  --input fra-probe.jsonl \
  --input iad-probe.jsonl \
  --clock-offset fra1=-683538 \
  --max-clock-uncertainty-ns 4963762
```

Summarize one probe log after or during a run:

```bash
cargo run --release -- summarize --input fra-probe.jsonl
```

Evaluate captured V2 candidates with the paper-only policy:

```bash
cargo run --release -- paper \
  --input v2-candidates.jsonl \
  --max-amount-in 10000000000000000
```

For a live paper stream, pipe the probe into `paper` (`--input -` reads
standard input):

```bash
hermes-feed probe \
  --url wss://feed.mainnet.chain.robinhood.com \
  --router 0x89e5db8b5aa49aa85ac63f691524311aeb649eba \
  --selector 0x38ed1739 \
  --selector 0x7ff36ab5 \
  --selector 0x18cbafe5 \
  --watch 0xYOUR_WATCHED_WALLET \
| hermes-feed paper --input - --max-amount-in 10000000000000000
```

The paper command never loads a key, signs, or submits a transaction. It caps
the simulated input, scales the observed minimum output proportionally, and
rejects missing typed intents, zero amounts, zero minimum output, stale
deadlines, arithmetic overflow, scaled minimums that round to zero, and paths
longer than the configured limit.

Fetch a block-consistent reserve snapshot for a path using the official public
RPC and canonical V2 factory defaults:

```bash
hermes-feed snapshot \
  --token 0xTOKEN_IN \
  --token 0xTOKEN_OUT \
  > reserve-snapshot.json
```

Apply the leader swap first and then quote the follower against the resulting
reserves:

```bash
hermes-feed simulate \
  --input candidate.jsonl \
  --snapshot reserve-snapshot.json \
  --max-amount-in 10000000000000000
```

`snapshot` validates chain ID 4663 and pins factory discovery, token ordering,
and reserve reads to one L2 block. `simulate` journals every hop's before-state
and both leader and follower outputs. This is intentionally separate from the
feed hot path: live validation found on-demand public-RPC snapshots too slow to
use after a signal, so the next runtime stage is a continuously refreshed
pre-signal reserve cache. See [RESERVE_SIMULATION.md](RESERVE_SIMULATION.md).

Bootstrap and maintain the complete confirmed cache:

```bash
hermes-feed cache \
  --checkpoint reserve-cache.json \
  --confirmations 2
```

Join an existing probe stream to that cache without candidate-time RPC:

```bash
hermes-feed probe ... \
| hermes-feed shadow \
    --input - \
    --checkpoint reserve-cache.json \
    --max-amount-in 10000000000000000
```

Factory reads are collapsed through the deterministic Multicall3 deployment.
The cache polls confirmed `Sync` log ranges, rejects skipped ranges, verifies
the checkpoint block hash on restart, writes checkpoints atomically, and loads
new factory pairs incrementally. See [CACHE_VALIDATION.md](CACHE_VALIDATION.md).

The `ops/start-paper-live.sh`, `status-paper-live.sh`, and
`stop-paper-live.sh` scripts run this pipeline as an isolated, resource-limited
paper observer. Runtime state is stored under the ignored
`.runtime/hermes-live` directory. Set `HERMES_WATCH_ADDRESS` before starting to
restrict copying to one sender; when unset, every supported V2 sender is
observed. No wallet or live sender is supported by these scripts.

The reserve-aware shadow is managed separately with
`ops/start-shadow-live.sh`, `status-shadow-live.sh`, and
`stop-shadow-live.sh`. It tails the existing feed journal, maintains a
two-confirmation reserve cache, loads new factory pairs incrementally, and
records leader-then-follower decisions. It has no signer or sender.

The summary excludes warmup frames and reports duration, sequence rate,
sequence health, reconnect/error counts, unsupported message kinds, average
local work per feed message, and p50/p95/p99/max timings for JSON, base64,
Nitro traversal and EIP-2718 decoding.

The comparator reports wins and p50/p95/p99/max lag from the earliest source
for sequences present in every input. It refuses to declare `winner` until at
least 10,000 exact sequences overlap, and a source with a sequence gap or a
missing sequence is ineligible. Override the sample floor only for tests with
`--min-matched-sequences`.

Cross-host wall clocks must be calibrated. `--clock-offset` is repeatable and
means `SOURCE=source_clock_minus_reference_clock_ns`; the comparator subtracts
that offset from the source timestamps. Set `--max-clock-uncertainty-ns` to the
measurement error bound. A winner is withheld unless its p95 lead exceeds the
bound.

The probe marks the initial catch-up window automatically: a new feed
connection can receive many cached messages in its first frames, and the
comparator excludes those frames.

## Multi-region experiment

Run the same release binary in candidate regions for 24 hours. Keep clocks
synchronized with chrony, but compare matching sequence numbers rather than
relying on ping alone.

For each region collect:

- feed arrival time for every `sequenceNumber`;
- p50, p95, p99, maximum and disconnect count;
- gaps, missing sequences, duplicates and reordered messages;
- JSON, base64, Nitro walk, EIP-2718 decode, filter and recovery time;
- direct-feed versus local-relay arrival delta on the same host.

Choose the deployment region only after measuring testnet transaction
submission from target-feed arrival to the follower transaction appearing in
the feed. RPC acknowledgement time is not a proxy for sequencer ordering.

## Narrow-decoder safety boundary

The decoder supports outer L1 message kind `3`, inner signed-transaction kind
`4`, and recursively nested inner batch kind `3` to depth 16. Other kinds are
counted but not interpreted. Batch lengths are unsigned 64-bit big-endian
values, matching Nitro's canonical `BytestringFromReader` representation.

Before this decoder may drive a trader, recorded mainnet fixtures must be
differential-tested against Nitro's canonical Go `ParseL2Transactions`, and
any sequence gap must halt trading until reconciled.

Candidate emission is fail-closed: after any detected gap or missing sequence,
frames continue to be decoded and measured, but candidate records are
suppressed for the remainder of the process. Reconciliation currently means an
operator-reviewed restart from a known contiguous cursor; Hermes never silently
re-enables candidates after a gap.
