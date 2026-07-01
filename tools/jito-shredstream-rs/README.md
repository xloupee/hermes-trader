# Jito ShredStream Feed Probe

Rust-only probe for the local Jito ShredStream proxy. It connects to
`ShredstreamProxy.SubscribeEntries`, decodes bincode `Vec<Entry>` payloads,
scans transactions for tracked wallets, and prints normalized copytrade events.
It currently recognizes direct Pump bonding-curve instructions, Pump AMM
instructions, and the observed FLASHX-routed Pump buy/sell layout.

The default live watcher is observe-only. It does not submit trades, write to
Telegram, or touch Supabase. Local copy simulation/send harnesses exist for
first-live testing, but they are disabled unless explicitly armed with local
keypair and guard environment variables.

## Run

On the VPS where `jito-shredstream-proxy` is listening on `127.0.0.1:9999`:

```bash
cargo run --manifest-path tools/jito-shredstream-rs/Cargo.toml -- \
  live \
  --endpoint http://127.0.0.1:9999 \
  --target-wallet <TARGET_WALLET> \
  --stats
```

Multiple wallets can be comma-separated:

```bash
SHREDSTREAM_TARGET_WALLETS=<WALLET_A>,<WALLET_B> \
cargo run --manifest-path tools/jito-shredstream-rs/Cargo.toml -- live
```

Use `--include-rejections` only for short debugging runs. It prints one line per
non-matching transaction and gets noisy fast.

Use `--print-mentions` during parser coverage work. Mention-only lines are
classified as `nonTrade`, `unsupportedRoute`, or `unknown` so parser misses do
not get mixed with simple wallet activity.

## Output

Matched trades are emitted as JSONL with schema `copytrade.feed.event.v1`.
Amounts are included only when the shred-visible outer instruction exposes
them directly. FLASHX-routed buys include the SOL input amount but not the exact
post-trade token output, because Jito entries do not include inner token balance
metadata.

```json
{
  "schema": "copytrade.feed.event.v1",
  "provider": "shredstream",
  "source": "jito-proxy",
  "targetWallet": "...",
  "action": "buy",
  "mint": "...",
  "signature": "...",
  "route": "flashx-pump",
  "solAmount": 0.00099,
  "input": { "mint": "So11111111111111111111111111111111111111112", "amount": 0.00099 },
  "output": { "mint": "..." },
  "copyable": true
}
```

Mention-only transactions are emitted as classified JSONL:

```json
{
  "schema": "copytrade.feed.walletMention.nonTrade.v1",
  "provider": "shredstream",
  "source": "jito-proxy",
  "targetWallet": "...",
  "signature": "...",
  "reason": "only system/compute/token housekeeping programs"
}
```

## Local Copy Simulation

Run this on the Mac, not the VPS. It opens an SSH tunnel to the VPS ShredStream
proxy, uses the local copy wallet keypair, signs a copy transaction, and calls
`simulateTransaction`. It cannot send because it forces
`JITO_ENABLE_COPY_SEND=false` and `JITO_DRY_RUN=true`.

```bash
tools/jito-shredstream-rs/run-local-copy-sim-vps.sh
```

When a tiny tracked-wallet buy lands, verify the signed simulation gate:

```bash
node tools/jito-shredstream-rs/verify-copy-simulation.mjs
```

Or wait for the next simulation row and verify it automatically:

```bash
node tools/jito-shredstream-rs/wait-for-copy-simulation.mjs
```

The verifier requires a clean `copytrade.localExecution.v1` row with:

- `simulationRequested: true`
- `sendEnabled: false`
- `dryRun: true`
- `signed: true`
- `simulated: true`
- `sent: false`
- `routeLayout: "direct-pump"`
- `instructionCount: 3`
- `maxCopySol <= 0.001`
- no simulation error

## Local Tiny Send

Only run this after the simulation verifier passes for a live buy. This still
runs on the Mac and still uses the local copy wallet keypair. It opens the VPS
tunnel, simulates first, and sends only if explicitly armed:

```bash
JITO_ARM_LIVE_COPY_SEND=YES tools/jito-shredstream-rs/run-local-copy-send-vps.sh
```

The send harness forces:

- `JITO_SIMULATE_COPY_TX=true`
- `JITO_ENABLE_COPY_SEND=true`
- `JITO_ONE_SHOT_COPY_SEND=false` by default, so it keeps listening until you stop the terminal
- `JITO_DRY_RUN=false`
- `JITO_AUTO_SELL_AFTER_BUY=true` by default in the armed local send harness
- `JITO_AUTO_SELL_DELAY_MS=1000` by default
- `JITO_SIMULATE_AUTO_SELL=true` by default in the non-fast local send harness
- `JITO_ISOLATE_BUY_LATENCY_TEST=false` by default
- `JITO_SEND_MAX_RETRIES=3` by default
- `JITO_SEND_HTTP_TIMEOUT_MS=5000` by default for non-fast local sends
- `JITO_MAX_COPY_SOL=0.001` by default

It refuses to run if `JITO_ARM_LIVE_COPY_SEND` is not exactly `YES`, or if
`JITO_MAX_COPY_SOL` is above `0.001`. To run a single-copy test instead, set
`JITO_ONE_SHOT_COPY_SEND=true`. To buy without the automatic post-buy sell, set
`JITO_AUTO_SELL_AFTER_BUY=false`. For clean buy slot/latency tests, set
`JITO_ISOLATE_BUY_LATENCY_TEST=true`; it forces auto-sell and auto-sell
simulation off even if the sourced live environment enabled them.

The auto-sell path is deliberately narrow: it only runs after a copied buy was
sent, waits the configured delay, reads the copy wallet token account balance,
builds a FLASHX sell for that token balance, and sends it. Local non-fast live
tests default `JITO_SIMULATE_AUTO_SELL=true`, which keeps the old simulate-and-
block guard. Fast live profiles default `JITO_SIMULATE_AUTO_SELL=false`; set it
to `true` only for debug runs where the extra RPC round trip is worth it.

For a one-shot fast-send test, arm the separate fast profile:

```bash
JITO_ARM_LIVE_COPY_SEND=YES \
JITO_FAST_COPY_SEND=YES \
JITO_ONE_SHOT_COPY_SEND=true \
tools/jito-shredstream-rs/run-local-copy-send-vps.sh
```

Fast mode keeps the same max-copy-SOL and keypair guards, but skips pre-submit
JSONL plan writes, disables pre-send simulation unless explicitly overridden,
and submits with `skipPreflight: true`. It defaults
`JITO_AUTO_SELL_AFTER_BUY=false` so the copied buy path is measured without the
post-buy sell experiment, and `JITO_SIMULATE_AUTO_SELL=false` when autosell is
explicitly enabled. Fast local sends default `JITO_SEND_HTTP_TIMEOUT_MS=750`.

## VPS Tiny Send

The production-shaped copy worker runs on the VPS, next to the local
ShredStream proxy, without the Mac SSH tunnel:

```bash
JITO_ARM_LIVE_COPY_SEND=YES /opt/jito-feed-probe-watch/run-vps-copy-send.sh
```

The VPS launcher sources `/opt/pumpfun-migration-bot/.env`, then
`/etc/jito-copy-live.env`, and runs the release `jito-feed-probe` binary. It
keeps `JITO_MAX_COPY_SOL <= 0.001`, `JITO_FAST_COPY_SEND=YES`,
`JITO_ONE_SHOT_COPY_SEND=false`, `JITO_SIMULATE_AUTO_SELL=false`, and
`JITO_DISABLE_SIGNAL_OBSERVATIONS=true` by default. It also defaults
`JITO_SEND_MAX_RETRIES=3` and `JITO_SEND_HTTP_TIMEOUT_MS=750`. For clean buy
latency tests against a live env that has auto-sell enabled, set
`JITO_ISOLATE_BUY_LATENCY_TEST=true`. Dashboard/report syncing should run as a
separate post-submit service.

State RPC is separate from send lanes:

```bash
JITO_STATE_RPC_URLS=https://state-rpc-a.example,https://state-rpc-b.example
JITO_BLOCKHASH_STALE_MS=5000
```

The state RPC pool is used for warm blockhash refreshes, copy-wallet balance
refreshes, address lookup table preload, simulation, token/account reads, and
post-submit confirmation checks. The buy hot path reads these warm caches in
memory and fails closed if the blockhash or balance state is stale. If
`JITO_STATE_RPC_URLS` is unset, the worker falls back to `SOLANA_RPC_URL` for
backward compatibility.

Send fanout is default-off:

```bash
JITO_SEND_FANOUT=YES
JITO_SEND_LANE_MODE=mixed
JITO_SEND_RPC_URLS=https://rpc-a.example,https://rpc-b.example
JITO_BLOCK_ENGINE_SEND_URLS=https://frankfurt.mainnet.block-engine.jito.wtf,https://london.mainnet.block-engine.jito.wtf
```

When fanout is enabled, the worker starts one send for every configured RPC
concurrently, records the first successful ACK as `sendRpcWinner`, and lets the
remaining send tasks keep running instead of cancelling them. It stores only a
sanitized host label, not query strings or API keys. If
`JITO_SEND_RPC_URLS` is unset, the VPS launcher falls back to
`DIRECT_EXECUTION_SEND_RPC_URLS`, then `SOLANA_RPC_URL` when present. This is
separate from `JITO_STATE_RPC_URLS`, so a read RPC does not automatically become
a send lane. If
`JITO_BLOCK_ENGINE_SEND_URLS` is unset, it falls back to
`DIRECT_EXECUTION_JITO_SEND_URLS` and posts to Jito
`/api/v1/transactions`, with `JITO_BLOCK_ENGINE_AUTH_UUID` inherited from
`DIRECT_EXECUTION_JITO_AUTH_UUID` when present.

`JITO_SEND_LANE_MODE` is resolved at startup and never from per-signal IO:

- `mixed`: current behavior. Build/sign one transaction with configured Jito and
  Helius Sender tips, then fan out the same signed bytes to all enabled lane
  families.
- `rpc_only`: use RPC endpoints and priority fee only.
- `jito_only`: use Jito block-engine endpoints and Jito tip only. Requires
  `JITO_SEND_FANOUT=YES` and `JITO_BLOCK_ENGINE_SEND_URLS`.
- `helius_sender_only`: use Helius Sender endpoints and Sender tip only.
  Requires `JITO_HELIUS_SENDER_ENABLED=YES`.
- `helius_sender_max`: use Helius Sender `/fast` endpoints with the Sender Max
  tip floor. Requires `JITO_HELIUS_SENDER_ENABLED=YES`,
  `JITO_HELIUS_SENDER_SWQOS_ONLY=false`, and a Sender tip of at least
  `1000000` lamports.
- `nozomi_only`: use Nozomi endpoints and a Nozomi tip only. `JITO_NOZOMI_URLS`
  can contain standard JSON-RPC URLs or API v2 `/api/sendTransaction2` URLs.
- `helius_nozomi_stack`: use Helius Sender plus Nozomi same-signature fanout.
  The transaction contains both active provider tips, signs once, and sends the
  identical serialized bytes to each configured lane.
- `helius_tpu_jet`: use Helius Sender plus the local Yellowstone Jet sidecar
  lane. This is the canary mode for same-signature Helius + Jet fanout without
  adding RPC/Jito lanes.
- `helius_tpu_quic`: use Helius Sender plus direct TPU QUIC. This is the
  canary mode for same-signature Helius + TPU QUIC fanout without adding
  RPC/Jito lanes.
- `tpu_jet_helius_tip`: use the local Yellowstone Jet sidecar lane only while
  keeping the Helius Sender tip in the transaction. This isolates lane quality
  before testing cheaper no-tip TPU sends.
- `tpu_quic_helius_tip`: use direct TPU QUIC only while keeping the Helius
  Sender tip in the transaction.
- `tpu_jet_only`: use the local Yellowstone Jet sidecar lane only. Requires
  `JITO_TPU_JET_ENABLED=YES`, `JITO_TPU_JET_RPC_URL`,
  `JITO_TPU_JET_WS_URL`, and `JITO_TPU_JET_SIDECAR_URL`. This mode does not
  include the Helius Sender tip and is reserved for the cheaper TPU-only canary.
- `tpu_quic_only`: use the direct TPU QUIC lane only. This is the safe fallback
  lane for the Yellowstone Jet spike because it stays on the worker's Solana
  2.2.1 dependency stack. Requires `JITO_TPU_QUIC_ENABLED=YES`,
  `JITO_TPU_QUIC_RPC_URL`, and `JITO_TPU_QUIC_WS_URL`. This mode does not
  include the Helius Sender tip and is reserved for the cheaper TPU-only canary.

Nozomi is default-off:

```bash
JITO_NOZOMI_ENABLED=false
JITO_NOZOMI_URLS=https://nozomi.temporal.xyz/?c=<api-key>
JITO_NOZOMI_TIP_LAMPORTS=1000000
JITO_NOZOMI_TIP_ACCOUNT=TEMPaMeCRFAS9EKF53Jd6KpHxgL47uWLcpFArU1Fanq
```

Nozomi API v2 URLs are detected from the path:

```bash
JITO_NOZOMI_URLS=https://pit1.nozomi.temporal.xyz/api/sendTransaction2?c=<api-key>
```

API v2 sends the base64 transaction bytes as `text/plain` and returns `200 OK`
with no signature body, so the worker records the already-known signed
transaction signature with `signatureReturned=false`. Use
`landing-canary-control.sh` selectors such as `nozomi-api-v2-only`,
`nozomi-api-v2-regional-only`, or `helius-nozomi-api-v2-regional-stack` to
stage API v2 without changing the steady-state env first.

Yellowstone Jet is default-off and runs through a local sidecar so the main
worker does not link the Jet Solana 3.x dependency graph:

```bash
JITO_TPU_JET_ENABLED=false
JITO_TPU_JET_RPC_URL=https://rpc.example
JITO_TPU_JET_WS_URL=https://yellowstone-grpc.example
JITO_TPU_JET_SIDECAR_URL=http://127.0.0.1:8787
JITO_TPU_JET_FANOUT_SLOTS=12
JITO_TPU_JET_TIMEOUT_MS=30
```

The Droplet launcher is `run-tpu-jet-sidecar.sh`; the matching systemd template
is `systemd/jito-tpu-jet-sidecar.service`.

Direct TPU QUIC is default-off:

```bash
JITO_TPU_QUIC_ENABLED=false
JITO_TPU_QUIC_RPC_URL=https://rpc.example
JITO_TPU_QUIC_WS_URL=wss://rpc.example
JITO_TPU_QUIC_FANOUT_SLOTS=12
JITO_TPU_QUIC_TIMEOUT_MS=30
```

When enabled in `mixed`, the worker sends the same signed wire transaction bytes
to TPU Jet/QUIC alongside the HTTP lanes. TPU dispatch telemetry is local
dispatch only; it is not treated as ACK or landing proof. Landing quality still
comes from confirmation, `slotDelta`, and `txDelta`.

Do not race lane-specific signed variants. Different fee/tip instructions
produce different messages and signatures, so multiple variants can land as
duplicate buys. The safe invariant is still one message, one signature, one
serialized payload, then fanout of the identical bytes selected by the mode.

Priority fee and Jito tip are explicit, capped runtime knobs:

```bash
JITO_PRIORITY_FEE_MICRO_LAMPORTS=250000
JITO_MAX_PRIORITY_FEE_MICRO_LAMPORTS=500000
JITO_TIP_LAMPORTS=10000
JITO_MAX_TIP_LAMPORTS=50000
JITO_TIP_ACCOUNT=96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5
```

The launcher rejects non-integer values, priority fees above the configured
cap, tips above the configured cap, and positive tips without a tip account.

After a send, verify the on-chain result:

```bash
node tools/jito-shredstream-rs/verify-copy-execution.mjs
```

The execution verifier reads the local send JSONL, fetches the sent transaction
from RPC, and reports the copy signature, mint, copy wallet token delta, SOL
delta, gross SOL spend, network fee, extra spend beyond the observed buy amount,
status, slot delta, and observed-to-submit/signature timing fields.

The Supabase sync helper also computes block-position diagnostics:
`targetSlot`, `copySlot`, `slotDelta`, `targetTxIndex`, `copyTxIndex`, and
`sameSlotTxDelta`. In watch mode it refreshes recent rows so transactions that
were not visible at the first confirmed lookup can still fill in after landing.
The VPS sync launcher checks for newly appended execution rows every
`JITO_SYNC_INTERVAL_MS` (default `1000`) and refreshes recent sent rows every
`JITO_SYNC_REFRESH_INTERVAL_MS` (default `5000`) so fresh benchmark rows can
arrive quickly without re-running the heavier confirmation diagnostics every
tick.
