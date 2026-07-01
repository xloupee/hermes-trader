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
5. Helius regional fanout: `helius-regional-fanout`, only after
   `JITO_CANARY_HELIUS_REGION_URLS` is set to comma-separated regional Sender
   endpoints for the canary window. This keeps
   `JITO_SEND_LANE_MODE=helius-sender-only`, disables Nozomi, Astralane, and
   Beam, preserves the baseline Helius tip, priority fee, and retries, and sends
   the same signed transaction bytes to multiple Helius Sender regions.
6. Nozomi delivery isolation, only after `JITO_NOZOMI_URLS` is configured with
   the API-keyed endpoint and the tip account is confirmed:
   `nozomi-only` applies `JITO_SEND_LANE_MODE=nozomi-only`,
   `JITO_NOZOMI_ENABLED=true`, and a Nozomi tip of at least `1000000`
   lamports. This is a lane test, not the final stack.
7. Helius + Nozomi same-signature stack:
   `helius-nozomi-stack` applies `JITO_SEND_LANE_MODE=helius-nozomi-stack`,
   keeps Helius Sender enabled, enables Nozomi, signs one transaction containing
   the Helius tip and Nozomi tip, then fans out identical bytes to both
   providers. This costs both provider tips on every landed transaction, so
   judge it by landed rate, same-slot rate, `txDelta`, submitted-not-landed,
   and total configured tip cost.
8. Astralane IrisB, only after `JITO_ASTRALANE_API_KEY`,
   `JITO_ASTRALANE_URLS`, and the Astralane tip account(s) are configured.
   Start with `astralane-only` for delivery-lane isolation, then
   `helius-astralane-stack` as the first same-signature race, then
   `helius-nozomi-astralane-stack` only if the extra provider-tip cost is
   intentional. IrisB is binary HTTP, not QUIC: it returns ACK/signature/error
   telemetry, but the canary is still judged by landed rate, same-slot rate,
   `txDelta`, failed-on-chain, submitted-not-landed, and configured tip cost.
9. Lunar Lander, only after `JITO_LUNAR_LANDER_API_KEY`,
   `JITO_LUNAR_LANDER_URLS`, and the Lunar Lander tip account(s) are
   configured. Start with `lunar-lander-only` for delivery-lane isolation,
   then `helius-lunar-lander-stack` as the first same-signature race, then
   `helius-nozomi-astralane-lunar-stack` when adding Lunar to the current
   Helius + Nozomi + Astralane lane stack is intentional. Lunar
   Lander is binary HTTP at `/send-bin`, requires a provider tip of at least
   `1000000` lamports in the signed transaction, and should be judged by
   landed rate, same-slot rate, `txDelta`, submitted-not-landed, failed-on-chain,
   and configured tip cost, not first ACK.
10. Circular Fast, only after `JITO_CIRCULAR_FAST_API_KEY`,
   `JITO_CIRCULAR_FAST_URLS`, and the FAST tip account(s) are configured.
   Start with `circular-fast-only` for delivery-lane isolation, then
   `helius-circular-fast-stack` as the first same-signature race. Circular Fast
   uses JSON-RPC `sendTransaction` at `/transactions` with `x-api-key` auth and
   returns `{ signature, uuid }` under `result`, so it has a provider-specific
   response parser. The default canary tip is `1000000` lamports; score it by
   landed rate, same-slot rate, `txDelta`, submitted-not-landed,
   failed-on-chain, and total configured tip cost.
11. ERPC SWQoS, only after `JITO_ERPC_SWQOS_URLS` is configured. Start with
   `erpc-swqos-only` for delivery-lane isolation, then
   `helius-erpc-swqos-stack` for Helius Sender + ERPC SWQoS same-signature
   fanout. This is a normal JSON-RPC send surface, so the worker does not add a
   provider tip instruction for ERPC SWQoS. Score by landed position and
   submitted-not-landed rate; do not promote on first ACK alone.
12. RPC Fast Beam, only after `JITO_BEAM_TOKEN` and provider-specific
   `JITO_BEAM_TIP_ACCOUNTS` are configured. Start with `beam-only` for smoke,
   then `helius-beam-stack` as a Nozomi-replacement test, then
   `helius-nozomi-beam-stack` only when the higher tip cost is intentional.
   Beam requires its tip transfer inside the transaction before signing; do not
   race a Beam-specific signature against a different Helius/Nozomi signature.
   The triple stack raises `JITO_MAX_PROVIDER_TIP_LAMPORTS` to `2500000` by
   default and must be judged by landing position and failed-on-chain rate, not
   ACK speed.
13. 0slot, only after `JITO_ZERO_SLOT_URLS`, `JITO_ZERO_SLOT_API_KEY`, and
   `JITO_ZERO_SLOT_TIP_ACCOUNTS` are configured. Start with `zero-slot-only`,
   then `helius-zero-slot-stack`, then `helius-nozomi-zero-slot-stack` only if
   the cost is justified. The default 0slot canary tip is `1000000` lamports, so
   keep the sample tiny until it proves value.
14. All non-Beam stack: `all-non-beam-stack` applies Helius Sender + Nozomi +
   0slot, and includes TPU Jet only if
   `JITO_CANARY_ALL_NON_BEAM_TPU_JET_ENABLED` or the existing
   `JITO_TPU_JET_ENABLED` is true. Astralane, Beam, and direct TPU QUIC are
   excluded. Use `helius-nozomi-astralane-stack` when Astralane cost is
   intentional. The default provider-tip cap is `2387500` lamports because the
   transaction pays every included provider tip if it lands.
15. Yellowstone Jet sidecar, only after the sidecar build is deployed and
   `JITO_TPU_JET_RPC_URL` / `JITO_TPU_JET_WS_URL` /
   `JITO_TPU_JET_SIDECAR_URL` are configured:
   `tpu-jet-fanout` applies `JITO_SEND_LANE_MODE=helius-tpu-jet` for Helius +
   Jet same-signature fanout, then `tpu-jet-only`, which applies
   `JITO_SEND_LANE_MODE=tpu-jet-helius-tip` for Jet-only sending with the same
   Helius-tip transaction shape.
16. ERPC Yellowstone gRPC can be staged as the Jet source by setting
   `JITO_ERPC_YELLOWSTONE_GRPC_URL` and, if required,
   `JITO_ERPC_YELLOWSTONE_GRPC_X_TOKEN`. The sidecar launcher maps those into
   `JITO_TPU_JET_GRPC_URL` / `JITO_TPU_JET_GRPC_X_TOKEN`. Keep this as a
   sidecar credential path; the copy-buy hot path must not call Yellowstone on
   signal.
17. ERPC Leader Slot API can be enabled after SWQoS and Jet are understood by
   setting `JITO_ERPC_LEADER_SLOTS_ENABLED=true`, `JITO_ERPC_API_KEY`, and
   optionally `JITO_ERPC_LEADER_SLOTS_URL=https://edge.erpc.global`. This is a
   background cache input for smarter direct-TPU targeting. A trade may only
   read the warmed cache and must fail closed if the cache is stale.
18. Direct TPU QUIC fallback, only after `JITO_TPU_QUIC_RPC_URL` /
   `JITO_TPU_QUIC_WS_URL` are configured. Use the current-leader variants for
   the next retest:
   `tpu-quic-current-leader-fanout` applies
   `JITO_SEND_LANE_MODE=helius-tpu-quic`, enables TPU QUIC, and sets
   `JITO_TPU_QUIC_FANOUT_SLOTS=1`; then
   `tpu-quic-current-leader-only` applies
   `JITO_SEND_LANE_MODE=tpu-quic-helius-tip` with the same Helius-tip
   transaction shape and `JITO_TPU_QUIC_FANOUT_SLOTS=1`.
   Do not use the legacy multi-leader `tpu-quic-fanout` / `tpu-quic-only`
   shape for the timeout retest: Solana's TPU client waits for all selected
   leader sends, so larger fanout can recreate the previous 100ms timeout.
19. Cheaper TPU-only shape: `tpu-jet-cheap` or `tpu-quic-cheap` only after the
   matching same-fee TPU-only window proves better. These switch to
   `tpu-jet-only` / `tpu-quic-only` lane modes with Helius Sender disabled and
   are guarded by `JITO_CANARY_ALLOW_CHEAP_TPU=YES`.

## Paid Tip Lane Safety

bloXroute Trader API canaries were removed from this worker because that route
requires a paid provider account. RPC Fast Beam may still use
`JITO_BEAM_PROVIDER=bloxroute`; that is Beam configuration, not the bloXroute
Trader API lane.

These are paid-tip lanes. The worker builds one transaction containing the
active Helius, Nozomi, Astralane, Lunar Lander, Circular Fast, Beam, and/or
0slot tips before signing, then fans out the identical signed bytes. This is
required for
duplicate-buy safety. New tip-funded providers should follow the same
same-signature pattern.
Score these lanes by landed rate, same-slot rate, `txDelta`, failed-on-chain
rate, submitted-not-landed rate, and total provider-tip cost. First ACK is only
delivery telemetry.

## Helius Regional Fanout Canary

Use `helius-regional-fanout` to test one thing: same signed transaction,
multiple Helius Sender regions. This is not a new transaction shape and should
not be mixed with Nozomi, Astralane, Beam, Jito, TPU Jet, or TPU QUIC in the
same window.

Required canary-only env:

```sh
JITO_CANARY_HELIUS_REGION_URLS=http://fra-sender.helius-rpc.com?api-key=<key>,http://ams-sender.helius-rpc.com?api-key=<key>,http://lon-sender.helius-rpc.com?api-key=<key>,http://ewr-sender.helius-rpc.com?api-key=<key>,http://slc-sender.helius-rpc.com?api-key=<key>
```

Then apply:

```sh
./landing-canary-control.sh apply helius-regional-fanout
```

The helper writes those URLs to `JITO_HELIUS_SENDER_URLS`, keeps
`JITO_SEND_LANE_MODE=helius-sender-only`, keeps the baseline Helius Sender tip,
priority fee, and `JITO_SEND_MAX_RETRIES=0`, and disables Nozomi, Astralane,
Lunar Lander, and Beam. The Rust worker appends `/fast` to each regional base
URL and preserves the `api-key` query.

This canary sends five Helius HTTP requests per buy when five regions are
configured, but it is still one signed Solana transaction with one signature and
one on-chain fee/tip if it lands. The extra requests only test delivery path
quality.

Score by same-slot rate, `slotDelta`, `txDelta`, `txDelta` coverage, failed or
submitted-not-landed rows, and configured cost. Do not promote or reject based
on first ACK alone. Pull the latest Droplet state first, anchor the score window
to `systemctl show -p ActiveEnterTimestamp --value jito-copy-live.service`, and
wait for 20-30 sent buys with at least 90% `txDelta` coverage before deciding.

## Astralane IrisB Canary

Required env before applying an Astralane canary:

```sh
JITO_ASTRALANE_URLS=https://lim.gateway.astralane.io/irisb
JITO_ASTRALANE_API_KEY=<api-key>
JITO_ASTRALANE_TIP_LAMPORTS=1000000
JITO_ASTRALANE_TIP_ACCOUNT=astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm
JITO_ASTRALANE_TIP_ACCOUNTS=astra4uejePWneqNaJKuFFA8oonqCE1sqF6b45kDMZm
JITO_ASTRALANE_MEV_PROTECT=false
JITO_ASTRALANE_SWQOS_ONLY=false
```

The canary helper also accepts `JITO_CANARY_ASTRALANE_URLS`,
`JITO_CANARY_ASTRALANE_API_KEY`, `JITO_CANARY_ASTRALANE_TIP_LAMPORTS`,
`JITO_CANARY_ASTRALANE_TIP_ACCOUNT`, `JITO_CANARY_ASTRALANE_TIP_ACCOUNTS`,
`JITO_CANARY_ASTRALANE_MEV_PROTECT`, and
`JITO_CANARY_ASTRALANE_SWQOS_ONLY` so IrisB can be staged without changing the
steady-state env first.

The canary modes are explicit because Astralane adds a `1000000` lamport
provider tip:

- `astralane-only`: IrisB only, Helius Sender and Nozomi disabled.
- `helius-astralane-stack`: Helius Sender + IrisB, same signed bytes.
- `helius-nozomi-astralane-stack`: Helius Sender + Nozomi + IrisB, same signed
  bytes.
- `helius-nozomi-astralane-lunar-stack`: Helius Sender + Nozomi + IrisB +
  Lunar Lander, same signed bytes, highest configured provider-tip cost.

Do not treat IrisB as a TPU lane or a QUIC lane. It is an outbound send
provider and should be scored by landed confirmation, not by first ACK alone.

## Lunar Lander Canary

Required env before applying a Lunar Lander canary:

```sh
JITO_LUNAR_LANDER_URLS=http://fra.lunar-lander.hellomoon.io/send-bin
JITO_LUNAR_LANDER_API_KEY=<api-key>
JITO_LUNAR_LANDER_TIP_LAMPORTS=1000000
JITO_LUNAR_LANDER_TIP_ACCOUNT=moon17L6BgxXRX5uHKudAmqVF96xia9h8ygcmG2sL3F
JITO_LUNAR_LANDER_TIP_ACCOUNTS=moon17L6BgxXRX5uHKudAmqVF96xia9h8ygcmG2sL3F
JITO_LUNAR_LANDER_MEV_PROTECT=false
```

Use the HTTP `/send-bin` endpoint on the Droplet; prior live probes showed the
HTTPS `/send-bin` path returning `404`.

The canary helper also accepts `JITO_CANARY_LUNAR_LANDER_URLS`,
`JITO_CANARY_LUNAR_LANDER_API_KEY`,
`JITO_CANARY_LUNAR_LANDER_TIP_LAMPORTS`,
`JITO_CANARY_LUNAR_LANDER_TIP_ACCOUNT`,
`JITO_CANARY_LUNAR_LANDER_TIP_ACCOUNTS`, and
`JITO_CANARY_LUNAR_LANDER_MEV_PROTECT` so the lane can be staged without
changing the steady-state env first.

The canary modes are explicit because Lunar Lander adds a `1000000` lamport
provider tip:

- `lunar-lander-only`: Lunar Lander `/send-bin` only, Helius Sender and Nozomi disabled.
- `helius-lunar-lander-stack`: Helius Sender + Lunar Lander, same signed bytes.
- `helius-nozomi-astralane-lunar-stack`: Helius Sender + Nozomi + Astralane +
  Lunar Lander, same signed bytes.

Do not hardcode API keys in scripts or checked-in env files. Use the Droplet env
file or canary override env, then score by landed confirmation and `txDelta`.
The first useful window should be small because every landed transaction pays
the Lunar provider tip.

## ERPC Surfaces

ERPC is three separate integrations:

```sh
# SWQoS JSON-RPC send lane
JITO_ERPC_SWQOS_ENABLED=true
JITO_ERPC_SWQOS_URLS=<erpc-swqos-send-url>

# Leader Slot API background cache
JITO_ERPC_LEADER_SLOTS_ENABLED=true
JITO_ERPC_LEADER_SLOTS_URL=https://edge.erpc.global
JITO_ERPC_API_KEY=<api-key>
JITO_ERPC_LEADER_SLOTS_REFRESH_MS=5000
JITO_ERPC_LEADER_SLOTS_STALE_MS=15000

# Yellowstone gRPC provider for the Jet sidecar
JITO_ERPC_YELLOWSTONE_GRPC_URL=http://grpc-fra1-burst.erpc.global
JITO_ERPC_YELLOWSTONE_GRPC_X_TOKEN=<x-token-if-required>
```

The first canary after credentials should be `erpc-swqos-only` or
`helius-erpc-swqos-stack`. Only after that has a real landed-position sample
should the ERPC Yellowstone gRPC endpoint be used by the Jet sidecar. Leader
Slots should be the last step because it changes TPU targeting logic rather
than provider send quality.

### ERPC Burst Geyser gRPC Readiness

For a Frankfurt Droplet, use the endpoint shown in the ERPC dashboard after
registering the Droplet IP. ERPC's public docs list the Frankfurt Burst endpoint
as:

```sh
JITO_ERPC_YELLOWSTONE_GRPC_URL=http://grpc-fra1-burst.erpc.global
```

ERPC says Burst is still a full Yellowstone/Geyser gRPC stream, separate from
Direct Shreds/ShredStream, so it can feed the Yellowstone Jet sidecar. If the
dashboard shows a different URL for the trial, use the dashboard URL.

Allow inbound/outbound ICMP to ERPC's gRPC load-balancer ping-source IPs if the
Droplet firewall blocks ping; ERPC uses those pings to select the nearest
regional node. For Frankfurt, the regular gRPC ping-source IPs listed by ERPC
are `185.191.118.149`, `185.191.118.177`, and `185.191.118.206`. The public
Frankfurt Burst load-balancer IP is `64.130.41.234`.

For the first Jet trial window, keep lookahead narrow:

```sh
JITO_TPU_JET_FANOUT_SLOTS=1
JITO_TPU_JET_TIMEOUT_MS=30
JITO_SEND_LANE_MODE=helius-tpu-jet
JITO_TPU_JET_ENABLED=true
```

This keeps Helius as the baseline lane and adds Jet as dispatch-only telemetry.
Do not start with `tpu-jet-only`; use it only after `helius-tpu-jet` proves no
landing regression.

## 0slot Canaries

bloXroute Trader API canaries were removed from this worker because that route
requires a paid provider account. RPC Fast Beam may still use
`JITO_BEAM_PROVIDER=bloxroute`; that is Beam configuration, not the bloXroute
Trader API lane.

Required 0slot env before applying a 0slot canary:

```sh
JITO_ZERO_SLOT_URLS=https://ny.0slot.trade
JITO_ZERO_SLOT_API_KEY=<api-key>
JITO_ZERO_SLOT_TIP_LAMPORTS=1000000
JITO_ZERO_SLOT_TIP_ACCOUNTS=<comma-separated-tip-accounts>
```

These are paid-tip lanes. The worker builds one transaction containing the
active Helius, Nozomi, Astralane, Lunar Lander, Beam, and/or 0slot tips before
signing, then fans out the identical signed bytes. This is required for
duplicate-buy safety.
Score these lanes by landed rate, same-slot rate, `txDelta`, failed-on-chain
rate, submitted-not-landed rate, and total provider-tip cost. First ACK is only
delivery telemetry.

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
JITO_BLOCKHASH_RPC_URLS=
JITO_COPY_WALLET_BALANCE_REFRESH_MS=5000
JITO_COPY_WALLET_BALANCE_STALE_MS=120000
JITO_BALANCE_CACHE_RPC_URLS=
JITO_PRIORITY_FEE_RPC_URLS=
```

If buys stop, check skip reasons before changing landing providers:

```sh
node -e 'const fs=require("fs");const rows=fs.readFileSync("/var/log/jito-copy-executions-vps.jsonl","utf8").trim().split(/\n/).map(JSON.parse).filter(r=>r.schema==="copytrade.localExecution.v1"&&r.observedAction==="buy").slice(-30);console.log(rows.map(r=>[new Date(r.observedAtMs).toISOString(),r.decision,r.reason||"sent"]).join("\n"))'
```

Warm-state skips mean the bot detected a buy and intentionally failed closed
before signing. Provider tips and sender lanes do not fix those.

Keep `JITO_ACCOUNT_PRIORITY_FEE_ENABLED=false` during provider-stack canaries
unless the account-priority cache is the specific variable under test. For an
ERPC hotpath-cache canary, enable only the priority-fee cache first:

```sh
JITO_ACCOUNT_PRIORITY_FEE_ENABLED=true
JITO_PRIORITY_FEE_RPC_URLS=https://edge.erpc.global?api-key=<key>,https://solana-rpc.publicnode.com,https://mainnet.helius-rpc.com/?api-key=<key>
JITO_ACCOUNT_PRIORITY_FEE_REFRESH_MS=5000
JITO_ACCOUNT_PRIORITY_FEE_STALE_MS=30000
JITO_ACCOUNT_PRIORITY_FEE_PERCENTILE=75
```

Do not put ERPC into `JITO_SYNC_RPC_URL` for this canary. Score only fresh rows
after restart and confirm `accountPriorityFeeSourceRpc`,
`accountPriorityFeeAgeMs`, and `accountPriorityFeeApplied` are populated without
new `missing warm blockhash`, `copy wallet balance cache stale`, or
`copy wallet balance cache missing` skips. Cache refreshers back off providers
on HTTP 429, timeout, and 5xx failures, but the hot path still only reads memory.

## Jet Sidecar Service

Direct TPU QUIC dispatch telemetry is local dispatch only, not an ACK. Judge it
by landed rate, same-slot rate, `slotDelta`, `txDelta`, failed-on-chain count,
and total configured cost. For the current-leader QUIC retest, require the
canary marker to show `CANARY_TPU_QUIC_FANOUT_SLOTS=1` and watch
`timeoutMs` / `fanoutSlots` in `copytrade.sendLaneAttribution.v1`.

Build the sidecar on the Droplet from `/opt/jito-feed-probe-watch` before the
Jet canary:

```bash
cargo build --release --manifest-path spikes/yellowstone-jet-compat/Cargo.toml --bin yellowstone-jet-sidecar
```

Required env before starting the sidecar:

```sh
JITO_TPU_JET_RPC_URL=<state-rpc-url-or-SOLANA_RPC_URL>
JITO_ERPC_YELLOWSTONE_GRPC_URL=http://grpc-fra1-burst.erpc.global
JITO_ERPC_YELLOWSTONE_GRPC_X_TOKEN=<x-token-if-dashboard-provides-one>
# Or, for a Shreder Fastlane trial:
JITO_SHREDER_FASTLANE_GRPC_URL=<shreder-fastlane-yellowstone-url>
JITO_SHREDER_FASTLANE_GRPC_X_TOKEN=<x-token-if-required>
JITO_TPU_JET_SIDECAR_URL=http://127.0.0.1:8787
JITO_TPU_JET_FANOUT_SLOTS=1
JITO_TPU_JET_TIMEOUT_MS=30
```

The sidecar launcher maps `JITO_ERPC_YELLOWSTONE_GRPC_URL` into
`JITO_TPU_JET_GRPC_URL` and maps `JITO_ERPC_YELLOWSTONE_GRPC_X_TOKEN` into
`JITO_TPU_JET_GRPC_X_TOKEN`. It also accepts
`JITO_SHREDER_FASTLANE_GRPC_URL` and `JITO_SHREDER_FASTLANE_GRPC_X_TOKEN` for
Shreder Fastlane trials. Do not set both provider aliases for the same canary;
prefer direct `JITO_TPU_JET_GRPC_URL` only when intentionally overriding the
provider-specific aliases.

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
