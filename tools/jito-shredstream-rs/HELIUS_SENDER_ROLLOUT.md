# Helius Sender Lane Rollout

This worker keeps Helius Sender off by default. The default VPS deploy should preserve the existing Jito/RPC behavior until the Sender env is explicitly enabled.

## Live baseline

Recent VPS sample from `/var/log/jito-copy-executions-vps.jsonl`, generated `2026-06-08T20:44:28.434Z`:

| Metric | Value |
| --- | --- |
| Total log rows scanned | 1748 |
| Sent buys | 377 |
| Recent sent-buy sample | 100 |
| Slot delta distribution | slot 0: 5, +1: 12, +2: 2, missing: 81 |
| Submitted-not-landed | 32 |
| First ACK lane | Frankfurt Jito: 97, London Jito: 2, RPC primary: 1 |
| observedToSendSubmittedMs | min 0, p50 1, p90 2, max 9 |
| observedToSignatureReturnedMs | min 7, p50 9, p90 18, max 152 |
| sendLaneMs | min 6, p50 8, p90 16, max 151 |
| Lane errors | Frankfurt Jito: 1, London Jito: 1 |

Use the same metrics after canary. ACK speed is useful, but success is judged by landing: more slot 0, fewer +1/+2, fewer submitted-not-landed, acceptable tip cost, and no hot-path latency regression.

## Default-off config

```sh
JITO_HELIUS_SENDER_ENABLED=false
JITO_HELIUS_SENDER_URLS=
JITO_HELIUS_SENDER_SWQOS_ONLY=false
JITO_HELIUS_SENDER_TIP_LAMPORTS=
JITO_HELIUS_SENDER_TIP_ACCOUNT=
```

When `JITO_HELIUS_SENDER_ENABLED` is true, startup fails closed unless:

- `JITO_SEND_FANOUT=YES`
- `JITO_FAST_COPY_SEND=YES`
- `JITO_PRIORITY_FEE_MICRO_LAMPORTS` is positive
- `JITO_HELIUS_SENDER_URLS` is non-empty
- `JITO_HELIUS_SENDER_TIP_LAMPORTS` is set and meets the selected mode minimum
- `JITO_HELIUS_SENDER_TIP_ACCOUNT` is a valid Solana pubkey

Tip minimums:

- Sender `/fast`: `200000` lamports
- Sender `swqos_only`: `5000` lamports

## Runtime workflow

1. Startup loads watched wallets, copy settings, keypairs, routing config, fee config, RPC/Jito/Sender endpoints, and tip settings into memory.
2. If Sender is disabled, endpoint selection remains the existing Jito/RPC fanout.
3. If Sender is enabled, endpoint selection adds `helius_sender` lanes with explicit labels like `helius-sender-1-fast:sender.helius-rpc.com` or `helius-sender-1-swqos:sender.helius-rpc.com`.
4. On a watched-wallet buy, Rust decodes, matches, classifies, plans, builds, signs, serializes, and submits without Telegram, Supabase, filesystem, dashboard, or config network calls before submit.
5. The transaction builder emits one compatible transaction containing the configured compute-unit price, existing Jito tip, and Sender tip. If Jito and Sender use the same tip account, the transfer is merged to the larger lamport value.
6. The same serialized transaction is fanned out to RPC, Jito, and Sender lanes.
7. First ACK behavior is preserved, while slower lanes continue so attribution can record every attempt.
8. Post-submit workers handle confirmation, slot diagnostics, dashboard sync, Telegram, Supabase, and trailing sells.

## Attribution

`copytrade.sendLaneAttribution.v1` records:

- `firstAckLane`
- every attempt in `allAttempts`
- lane `label`
- lane `kind`: `rpc`, `jito`, or `helius_sender`
- Sender `mode`: `fast` or `swqos`
- attempt `durationMs`
- `ackAt`
- `error`

This distinguishes normal Helius RPC from Helius Sender and prevents first ACK from being mistaken for proven landing.

## Deploy and canary gates

1. Deploy the code with Sender disabled.
2. Confirm the worker starts with Sender disabled and current Jito/RPC behavior unchanged.
3. Enable Sender only for a tiny controlled copy size.
4. Keep current Jito lanes enabled during the canary.
5. Compare canary rows against the baseline by landing and cost, not only by ACK timing.
