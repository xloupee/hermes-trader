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
- `JITO_SEND_MAX_RETRIES=3`

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

Run these on the VPS from `/opt/jito-feed-probe-watch`.

```sh
./landing-canary-control.sh status
./landing-canary-control.sh mark baseline
./landing-canary-control.sh score
./landing-canary-control.sh score-recent 50
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
./landing-canary-control.sh restore /opt/jito-feed-probe-watch/backups/canary-YYYYMMDDTHHMMSSZ/jito-copy-live.env
```

The script backs up `/etc/jito-copy-live.env`, restarts only
`jito-copy-live.service`, writes `/var/log/jito-copy-canary-current.env`, and
keeps the lane locked to Helius Sender FRA SWQoS.

Candidate applies are baseline-gated. By default `apply tip-250k`,
`apply priority-750k`, and other non-baseline canaries first run
`./landing-canary-control.sh score` and abort before editing env if the baseline
window does not meet the minimum scored-row and `txDelta` coverage thresholds.

## Order

1. Baseline: current config until sample is large enough.
2. Helius Sender tip: `tip-250k`, then `tip-500k`.
3. Priority fee: `priority-750k`, then `priority-1250k`.
4. Retries: `retries-0`, `retries-1`, then `retries-3`.

Run one variable per window. Roll back immediately if landed rate, same-slot
rate, `slotDelta`, `txDelta`, failures, or cost regress materially.

Do not promote from:

- too few sent buys
- missing `txDelta` coverage
- only faster ACK
- route mix that does not match baseline
- higher cost without a landing improvement
- any change that adds pre-submit Telegram, Supabase, DB, filesystem, metadata,
  price, or config lookups
