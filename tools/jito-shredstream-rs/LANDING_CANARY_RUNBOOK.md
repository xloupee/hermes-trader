# Landing Canary Runbook

This runbook is for live copy-buy landing canaries after measurement is healthy.
It must not be used to change detector, parser, copy sizing, or route logic in
the same window as a fee/tip/retry experiment.

## Current Baseline

- Lane: Helius Sender FRA SWQoS only.
- `JITO_SEND_LANE_MODE=helius-sender-only`
- `JITO_HELIUS_SENDER_SWQOS_ONLY=true`
- `JITO_HELIUS_SENDER_TIP_LAMPORTS=387500`
- `JITO_PRIORITY_FEE_MICRO_LAMPORTS=968750`
- `JITO_SEND_MAX_RETRIES=0`

The retry baseline is intentionally zero. A provider retry after the first send
is expected to be too late for the landing race; score this by fresh landing
rows rather than by first ACK.

Score canaries with landing results, not ACK:

- landed rate
- same-slot rate
- `slotDelta`
- `targetTxIndex`, `copyTxIndex`
- `txDelta`
- failed-on-chain and submitted-not-landed rate
- configured and observed fee/tip cost

The scoreboard must pass the `txDelta` coverage gate before any canary window can
be promoted.

## Commands

Run these on the Droplet from `/opt/jito-feed-probe-watch`.

```sh
./landing-canary-control.sh status
./landing-canary-control.sh mark baseline
./landing-canary-control.sh score
./landing-canary-control.sh score-recent 50
./landing-canary-control.sh compare
./landing-canary-control.sh ready
```

Do not start canarying until the baseline window has enough sent copy buys for a
real comparison. A single post-restart buy is only a smoke test.
`ready` exits with the strict baseline gate status; recent historical context is
report-only.

Apply one canary at a time:

```sh
./landing-canary-control.sh apply tip-250k
./landing-canary-control.sh score
./landing-canary-control.sh compare
./landing-canary-control.sh restore /opt/jito-feed-probe-watch/backups/canary-YYYYMMDDTHHMMSSZ/jito-copy-live.env
```

For the landing telemetry canary sequence in this runbook, use the gated helper
after marking baseline:

```sh
./landing-canary-sequence.sh status
./landing-canary-sequence.sh next
```

`next` refuses to advance unless the current window passes
`landing-canary-control.sh score`, so it will wait on the baseline sample before
applying `tip-rotated`, then `blockhash-confirmed`, then
`account-priority-cache`.

The script backs up `/etc/jito-copy-live.env`, restarts only
`jito-copy-live.service`, writes `/var/log/jito-copy-canary-current.env`, and
keeps each canary to the lane/fee shape named by the apply target.

Candidate applies are baseline-gated. By default `apply tip-250k`,
`apply priority-750k`, and other non-baseline canaries first run
`./landing-canary-control.sh score` and abort before editing env if the baseline
window does not meet the minimum scored-row and `txDelta` coverage thresholds.

## Order

1. Baseline: current config until sample is large enough.
2. Helius Sender tip: `tip-250k`, then `tip-500k`.
3. Priority fee: `priority-1453k`, then `priority-1938k`; keep retries at `0`.
4. Retries: `retries-1` or `retries-3` only as rollback/diagnostic shapes.
5. Nozomi delivery isolation, only after `JITO_NOZOMI_URLS` is configured with
   the API-keyed endpoint and the tip account is confirmed:
   `nozomi-only` applies `JITO_SEND_LANE_MODE=nozomi-only`,
   `JITO_NOZOMI_ENABLED=true`, and a Nozomi tip of at least `1000000`
   lamports. This is a lane test, not the final stack.
6. Helius + Nozomi same-signature stack:
   `helius-nozomi-stack` applies `JITO_SEND_LANE_MODE=helius-nozomi-stack`,
   keeps Helius Sender enabled, enables Nozomi, signs one transaction containing
   the Helius tip and Nozomi tip, then fans out identical bytes to both
   providers. This costs both provider tips on every landed transaction, so
   judge it by landed rate, same-slot rate, `txDelta`, submitted-not-landed,
   and total configured tip cost.
7. Yellowstone Jet sidecar, only after the sidecar build is deployed and
   `JITO_TPU_JET_RPC_URL` / `JITO_TPU_JET_WS_URL` /
   `JITO_TPU_JET_SIDECAR_URL` are configured:
   `tpu-jet-fanout` applies `JITO_SEND_LANE_MODE=helius-tpu-jet` for Helius +
   Jet same-signature fanout, then `tpu-jet-only`, which applies
   `JITO_SEND_LANE_MODE=tpu-jet-helius-tip` for Jet-only sending with the same
   Helius-tip transaction shape.
8. Direct TPU QUIC fallback, only after the direct TPU build is deployed and
   `JITO_TPU_QUIC_RPC_URL` / `JITO_TPU_QUIC_WS_URL` are configured:
   `tpu-quic-fanout` applies `JITO_SEND_LANE_MODE=helius-tpu-quic` for Helius
   + TPU same-signature fanout, then `tpu-quic-only`, which applies
   `JITO_SEND_LANE_MODE=tpu-quic-helius-tip` for TPU-only sending with the same
   Helius-tip transaction shape.
9. Cheaper TPU-only shape: `tpu-jet-cheap` or `tpu-quic-cheap` only after the
   matching same-fee TPU-only window proves better. These switch to
   `tpu-jet-only` / `tpu-quic-only` lane modes with Helius Sender disabled and
   are guarded by `JITO_CANARY_ALLOW_CHEAP_TPU=YES`.

## Nozomi Stack Canary

Required env before applying either Nozomi canary:

```sh
JITO_NOZOMI_URLS=https://nozomi.temporal.xyz/?c=<api-key>
JITO_NOZOMI_TIP_LAMPORTS=1000000
JITO_NOZOMI_TIP_ACCOUNT=TEMPaMeCRFAS9EKF53Jd6KpHxgL47uWLcpFArU1Fanq
```

The canary helper also accepts `JITO_CANARY_NOZOMI_URLS`,
`JITO_CANARY_NOZOMI_TIP_LAMPORTS`, `JITO_CANARY_NOZOMI_TIP_ACCOUNT`, and
`JITO_CANARY_NOZOMI_TIP_ACCOUNTS` so the Nozomi endpoint can be staged without
changing the steady-state env first.

The Helius + Nozomi stack is not two signed variants. The worker builds one
transaction with the current priority fee, the Helius Sender tip, the Nozomi
tip, and the swap, signs once, serializes once, then sends those identical bytes
to Helius Sender and Nozomi JSON-RPC. This avoids duplicate-buy risk from
provider-specific signatures, but it increases transaction shape and tip cost.

Guardrails for the stack:

```sh
JITO_MAX_PROVIDER_TIP_LAMPORTS=1387500
JITO_MAX_SIGNED_TX_BYTES=1232
JITO_MAX_INSTRUCTION_COUNT=8
JITO_MAX_WRITABLE_ACCOUNT_COUNT=16
```

Raise those only if the worker emits a guarded skip and the skipped transaction
shape is understood. Do not use first ACK as the decision metric; Nozomi can win
ACK while Helius, or vice versa, may be the path that actually improves landed
position.

## Dynamic Priority Fee Canary

Use dynamic priority as a one-variable canary. Keep the sender lane, Helius
Sender tip, send retries, parser, detector, and copy sizing fixed for the whole
window.

First static-bucket shape:

```sh
JITO_DYNAMIC_PRIORITY_FEE_ENABLED=true
JITO_DYNAMIC_PRIORITY_FEE_BASELINE_MICRO_LAMPORTS=1250000
JITO_DYNAMIC_PRIORITY_FEE_AGGRESSIVE_MICRO_LAMPORTS=2500000
JITO_DYNAMIC_PRIORITY_FEE_MAX_MICRO_LAMPORTS=2500000
JITO_HELIUS_SENDER_TIP_LAMPORTS=200000
JITO_SEND_MAX_RETRIES=0
```

The Rust worker selects `aggressive` for early/mid source-position buckets and
`baseline` for late/unknown buckets without any pre-submit network, file,
Supabase, Telegram, JSON, or RPC lookups. Score by `feeProfileName` and
`sourcePositionBucket` in `landing-scoreboard-report.mjs`; wait for at least
10-20 sent rows per bucket before deciding.

Primary dynamic-priority gates:

- p50/p90 `txDelta`
- `txDelta<=50`
- `txDelta<=10`
- same-slot rate
- landed rate
- configured fee/tip SOL per sent and per landed improvement

## Warm State Guardrails

The copy-buy hot path must use preloaded state. It should not fetch blockhashes
or balances on signal, but the warm caches must also be tolerant of ordinary RPC
jitter.

Recommended live values:

```sh
JITO_BLOCKHASH_REFRESH_MS=500
JITO_BLOCKHASH_REFRESH_TIMEOUT_MS=1200
JITO_BLOCKHASH_STALE_MS=30000
JITO_COPY_WALLET_BALANCE_REFRESH_MS=5000
JITO_COPY_WALLET_BALANCE_STALE_MS=120000
```

If buys stop, check skip reasons before changing landing providers:

```sh
node -e 'const fs=require("fs");const rows=fs.readFileSync("/var/log/jito-copy-executions-vps.jsonl","utf8").trim().split(/\n/).map(JSON.parse).filter(r=>r.schema==="copytrade.localExecution.v1"&&r.observedAction==="buy").slice(-30);console.log(rows.map(r=>[new Date(r.observedAtMs).toISOString(),r.decision,r.reason||"sent"]).join("\n"))'
```

Warm-state skips mean the bot detected a buy and intentionally failed closed
before signing. Provider tips and sender lanes do not fix those.

Keep `JITO_ACCOUNT_PRIORITY_FEE_ENABLED=false` during provider-stack canaries
unless the account-priority cache is the specific variable under test. That
cache can add many `getRecentPrioritizationFees` reads after writable accounts
are observed, which can starve the same state RPC used by warm balance and
blockhash caches.

## Jet Sidecar Service

Build the sidecar on the Droplet from `/opt/jito-feed-probe-watch` before the
Jet canary:

```bash
cargo build --release --manifest-path spikes/yellowstone-jet-compat/Cargo.toml --bin yellowstone-jet-sidecar
```

Use a local-only sidecar service so the main worker sends to
`http://127.0.0.1:8787/send`:

```bash
cp /opt/jito-feed-probe-watch/systemd/jito-tpu-jet-sidecar.service /etc/systemd/system/jito-tpu-jet-sidecar.service
systemctl daemon-reload
systemctl enable --now jito-tpu-jet-sidecar.service
systemctl is-active --quiet jito-tpu-jet-sidecar.service
```

Unit template:

```ini
[Unit]
Description=Jito Copy Yellowstone Jet TPU Sidecar
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
WorkingDirectory=/opt/jito-feed-probe-watch
Environment=JITO_WORKER_DIR=/opt/jito-feed-probe-watch
ExecStart=/opt/jito-feed-probe-watch/run-tpu-jet-sidecar.sh
Restart=always
RestartSec=2
User=root

[Install]
WantedBy=multi-user.target
```

The sidecar must be active before enabling `JITO_TPU_JET_ENABLED=YES`; the live
worker warms `/health` at startup and fails closed if the sidecar is unavailable.
`landing-canary-control.sh apply tpu-jet-*` also checks the configured sidecar
`/health` endpoint before it edits env or restarts the worker.

Run one variable per window. Roll back immediately if landed rate, same-slot
rate, `slotDelta`, `txDelta`, failures, or cost regress materially.

Promotion is a comparison against the baseline window, not a single-window ACK
win. The coded helper `evaluatePromotionCandidate` requires no landed-rate
regression plus improvement in same-slot rate, p50/p90 `txDelta`, and
`txDelta<=50` rate, and no p90 regression in `observedToSignedMs` or
`observedToSendSubmittedMs`. It also fails if one observed trade maps to more
than one copy send signature in the canary window. Run
`./landing-canary-control.sh compare` after each candidate window; it compares
`[baseline start, canary start)` against `[canary start, now)` using the marker
timestamps. Treat any missing comparison field as unknown, not pass.

Do not promote from:

- too few sent buys
- missing `txDelta` coverage
- only faster ACK
- route mix that does not match baseline
- higher cost without a landing improvement
- TPU dispatch success without landing improvement
- any change that adds pre-submit Telegram, Supabase, DB, filesystem, metadata,
  price, or config lookups
