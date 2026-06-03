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
- `JITO_DRY_RUN=false`
- `JITO_MAX_COPY_SOL=0.001` by default

It refuses to run if `JITO_ARM_LIVE_COPY_SEND` is not exactly `YES`, or if
`JITO_MAX_COPY_SOL` is above `0.001`.

After a send, verify the on-chain result:

```bash
node tools/jito-shredstream-rs/verify-copy-execution.mjs
```

The execution verifier reads the local send JSONL, fetches the sent transaction
from RPC, and reports the copy signature, mint, copy wallet token delta, SOL
delta, status, and observed-to-submit/signature timing fields.
