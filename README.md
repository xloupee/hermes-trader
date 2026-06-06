# Pump.fun Migration Telegram Bot

Telegram notifier for Pump.fun coin migrations using PumpPortal's realtime websocket.

## What the bot listens to

PumpPortal's Data API exposes a websocket at:

```text
wss://pumpportal.fun/api/data?api-key=your-api-key-here
```

After connecting, this bot sends PumpPortal subscriptions for token events:

```json
{ "method": "subscribeNewToken" }
{ "method": "subscribeMigration" }
```

Watched-wallet swap alerts use Helius enhanced webhooks at:

```text
POST /webhooks/helius
```

The bot keeps one PumpPortal websocket connection open for token events and filters token, migration, and watched-wallet alerts per verified Telegram user.

## Setup

1. Create a Telegram bot with [BotFather](https://t.me/BotFather), then copy the bot token.
2. Copy the example env file:

```bash
cp .env.example .env
```

3. Fill in `TELEGRAM_BOT_TOKEN`. Set `TELEGRAM_VERIFY_CODE` to a private invite code people must send before receiving alerts. Set `SUPABASE_URL` and `SUPABASE_SERVICE_ROLE_KEY` to store subscribers in Supabase; `SUPABASE_SERVICE_KEY` and `SUPABASE_SERVICE_ROLE` are also accepted as service-role aliases. Omit the Supabase values to keep using the local JSON fallback. `PUMPPORTAL_API_KEY` is optional for migrations, but supported if you have one. For wallet swap alerts, set `HELIUS_API_KEY`, `HELIUS_WEBHOOK_PUBLIC_URL`, and `HELIUS_WEBHOOK_AUTH_HEADER`. For Bloom-style generated trading wallets, set `PUMPPORTAL_WALLET_KEY_ENCRYPTION_SECRET` to a 32+ character random secret before users create wallets.
4. Install dependencies:

```bash
npm install
```

5. Start the bot:

```bash
npm start
```

6. In Telegram, open your bot and send `/start`.
7. Send `/verify your-code` using the value from `TELEGRAM_VERIFY_CODE`.
8. Choose your alert types with `/alerts`; tap a button again to turn that alert type on or off.
9. To monitor wallet swaps, send `/trackwallets`, then use the dashboard buttons to add, rename, remove, or list tracked wallets. Wallet swap alerts require the Helius webhook env vars.
10. Restart the bot if you changed `.env`:

```bash
npm start
```

Verified subscribers and their alert modes are stored in Supabase when `SUPABASE_URL` and `SUPABASE_SERVICE_ROLE_KEY` are set. The service role key is server-only and must not be exposed in browser code. If either Supabase value is missing, subscribers are stored in `TELEGRAM_SUBSCRIBERS_PATH`. `TELEGRAM_CHAT_ID` is optional for the JSON fallback, but Supabase mode uses the database as the source of truth.

To import an existing JSON subscriber file into Supabase after applying the database migration, run:

```bash
npm run import-subscribers
```

You can also pass a custom JSON path:

```bash
npm run import-subscribers -- data/telegram-subscribers.json
```

## Bot commands

- `/start` - Start notifications.
- `/verify <code>` - Verify this chat for notifications.
- `/stop` - Stop notifications for this chat.
- `/help` - Show the command list.
- `/alerts` - Open the alert mode dashboard.
- `/trackwallets` - Open the tracked-wallet dashboard.
- `/mywallets` - Open the generated trading wallet dashboard.
- `/copytrade` - Open the copy trade setup dashboard.

## Notes

- Keep only one running instance of this bot per PumpPortal API key.
- If PumpPortal sends a new migration payload shape, the bot still sends a notification with the raw event JSON.
- Telegram messages are HTML escaped before sending.
- To inspect recent on-chain migrations without waiting for a live event, run `npm run past-migrations -- 10`.
- Set `SOLANA_RPC_URL` in `.env` if public Solana RPC rate limits you.
- Wallet swap monitor events are stored in `WALLET_TRADE_LOG_PATH`.
- `/mywallets` creates one PumpPortal Lightning trading wallet per verified chat. The bot shows the private key once and does not store it. Users must save it themselves and deposit SOL to the public address.
- Copytrade auto buys require a generated trading wallet from `/mywallets`, a copy amount, Copytrade Wallets from `/copytrade`, `COPY_TRADE_ENABLED=true`, and `COPY_TRADE_DRY_RUN=false`. Unless both switches are set that way, the bot logs and sends the intended copy buy but does not submit a PumpPortal order.
- Optional execution risk controls run before copy-trade submissions: `COPY_TRADE_MAX_BUY_SOL`, `COPY_TRADE_DAILY_SOL_CAP`, `COPY_TRADE_MAX_SIGNAL_AGE_MS`, `COPY_TRADE_MAX_SLIPPAGE`, `COPY_TRADE_MAX_PRIORITY_FEE`, `COPY_TRADE_MAX_COPY_WALLETS_PER_CHAT`, and optional `COPY_TRADE_ALLOWED_SOURCES` comma-separated source allowlist. Violations are logged and sent as skipped copy buys. Set any numeric cap to `0` to disable it; launch config uses `0` for all numeric caps. The daily cap is in-memory, counts attempted live submissions, and resets on UTC day boundaries. Keep only one running bot instance and redeploy/restart carefully.
- Copytrade mirrors each distinct target buy transaction. Exact duplicate webhook deliveries for the same observed transaction are ignored while that copy buy is in flight.
- Treat generated trading wallets as hot wallets. Anyone with the private key can withdraw funds, and anyone with the linked API key can trade from the funded wallet.
- Keep `COPY_TRADE_ENABLED=false` or `COPY_TRADE_DRY_RUN=true` as the kill switch. First live run recommendation: fund only the SOL you are willing to trade from the hot wallet, set the user's copy amount intentionally in Telegram, use `COPY_TRADE_ENABLED=true`, `COPY_TRADE_DRY_RUN=false`, `COPY_TRADE_SLIPPAGE=10`, `COPY_TRADE_PRIORITY_FEE=0.00005`, and keep launch caps disabled with `COPY_TRADE_MAX_BUY_SOL=0`, `COPY_TRADE_DAILY_SOL_CAP=0`, `COPY_TRADE_MAX_SIGNAL_AGE_MS=0`, `COPY_TRADE_MAX_SLIPPAGE=0`, `COPY_TRADE_MAX_PRIORITY_FEE=0`, and `COPY_TRADE_MAX_COPY_WALLETS_PER_CHAT=0`. Watch logs, Telegram, and Solscan during launch.
- `COPY_TRADE_TRAILING_SELL_ENABLED=true` schedules live PumpPortal Lightning percentage sells after a successful auto copy buy. Scheduled sell timers are in-memory and are cleared if the bot restarts; they only run after live copy buys are enabled and submitted.
- `COPY_TRADE_BUY_PRESSURE_SELL_ENABLED=true` enables the bot-wide buy-pressure exit capability after a confirmed auto copy buy, but each Telegram user must still opt in from `/copytrade` -> Settings. Users can also override their timeout there; otherwise `COPY_TRADE_BUY_PRESSURE_SELL_TIMEOUT_MS` is inherited. It watches the copied mint through PumpPortal token trades plus any Helius watched-wallet trade events, sells `COPY_TRADE_BUY_PRESSURE_SELL_PERCENT` after `COPY_TRADE_BUY_PRESSURE_SELL_MIN_BUYS` and optional `COPY_TRADE_BUY_PRESSURE_SELL_MIN_TOTAL_SOL` are met, or sells on the configured timeout as a fallback. Watchers are persisted in `COPY_TRADE_BUY_PRESSURE_SELL_STATE_PATH` and resume timeout fallback after restart; live/dry-run gates, sell slippage/priority settings, and copied-position token balance checks still apply.
- `DIRECT_EXECUTION_BLOCKHASH_CACHE_MS` and `DIRECT_EXECUTION_BLOCKHASH_WARM_INTERVAL_MS` keep a recent blockhash ready for live direct sends so each copy buy does not wait on a fresh blockhash RPC call.
- `DIRECT_EXECUTION_SDK_WARM_INTERVAL_MS` proactively refreshes direct Pump SDK global/fee config caches so a live copy buy does not pay the refresh cost.
- `DIRECT_EXECUTION_SIMULATE_BEFORE_SEND=false` skips the bot's explicit pre-send simulation for lower latency. Keep it `true` until direct execution has proven clean in canary, then disable only if you accept that bad transactions may fail on-chain instead of being caught locally.
- `DIRECT_EXECUTION_SKIP_PREFLIGHT=true` and `DIRECT_EXECUTION_MAX_RETRIES=0` are the fastest promoted ShredStream/direct-copy settings once canary direct execution is already clean. Keep higher retries while validating a new RPC provider, then lower them only when background confirmation and logs are being watched.
- `DIRECT_EXECUTION_SEND_RPC_URLS` is an optional comma-separated list of extra RPC URLs for raw-send fanout. The bot signs once, sends the same transaction to the primary `SOLANA_RPC_URL` plus each fanout URL, and returns the first signature.
- `DIRECT_EXECUTION_JITO_SEND_URLS` is an optional comma-separated list of Jito Block Engine base URLs, such as `https://frankfurt.mainnet.block-engine.jito.wtf` or `https://london.mainnet.block-engine.jito.wtf`, to race alongside normal raw-send RPCs through Jito `sendTransaction`. Set `DIRECT_EXECUTION_JITO_AUTH_UUID` if your Jito access uses UUID auth.
- `CASHBACK_ENABLED=true` accrues user cashback only from this bot's collected direct-execution `PLATFORM_FEE_*` amount. It does not rebate gross trade volume, trading losses, network fees, or PumpPortal Lightning fees. Configure `CASHBACK_FEE_SHARE_BPS`, `CASHBACK_MIN_CLAIM_SOL`, `CASHBACK_PAYOUT_WALLET_PUBLIC_KEY`, `CASHBACK_PAYOUT_WALLET_SECRET_KEY`, and optional `CASHBACK_MAX_PAYOUT_SOL_PER_DAY`, then apply the cashback Supabase migrations. Users can view cashback and save/change their own payout wallet from `/cashback`; operators can reconcile with `npm run cashback-reconciliation-report`.
- `YELLOWSTONE_ENABLED=true` starts an optional Yellowstone gRPC watched-wallet stream. For a QuickNode trial, set `YELLOWSTONE_ENDPOINT` to the gRPC endpoint on port `10000` and `YELLOWSTONE_TOKEN` to the token from the original QuickNode RPC URL. Keep `YELLOWSTONE_SHADOW_ONLY=true` at first; shadow mode writes/logs Yellowstone candidates but does not trigger copy buys or Telegram alerts.
- Expose `WEBHOOK_PORT` through your reverse proxy at the exact `HELIUS_WEBHOOK_PUBLIC_URL`, and forward the `Authorization` header unchanged.
- `GEYSER_ENABLED=true` starts an observe-only Yellowstone Geyser wallet feed using `GEYSER_GRPC_URL`. By default it writes parsed diagnostics to `WALLET_TRADE_LOG_PATH` and does not trigger copy buys, sells, Telegram alerts, or buy-pressure exits. `COPY_TRADE_SIGNAL_PROVIDER=parallel` races PumpPortal and Geyser watched-wallet buys through the same copy-buy handler; the first signature/target-wallet/mint signal wins and later duplicates are logged without submitting another buy attempt. Keep one bot instance so the shared Geyser stream stays well under the provider's stream limit.
- `SHREDSTREAM_WALLET_OBSERVER_ENABLED=true` starts an observe-only ShredStream watched-wallet feed using the same `SHREDSTREAM_SOURCE`, `SHREDSTREAM_GRPC_URL`, and optional `SHREDSTREAM_DECODER_CMD` settings as the discovery listener. It writes `provider="shredstream"` rows to `WALLET_TRADE_LOG_PATH` for matching watched/copytrade wallets and does not trigger copy buys, sells, Telegram alerts, or buy-pressure exits. Live ShredStream copy buys belong to the long-running Rust worker in `tools/jito-shredstream-rs`, not this TypeScript observer.
- `SHREDSTREAM_WALLET_OBSERVER_STATS_INTERVAL_MS=60000` controls the observer stats log cadence. The stats line reports records read, parse errors, decoded Pump/PumpSwap buy/sell candidates, watched-wallet matches, diagnostic-vs-real wallet matches, and emitted ShredStream wallet rows.
- `WALLET_FEED_DIAGNOSTIC_WALLETS=address[:label],...` adds operator-only wallets to PumpPortal/Geyser/ShredStream feed observation. Diagnostic wallets write rows to `WALLET_TRADE_LOG_PATH` with `raw.diagnosticWallet=true`, but they are not subscriber watchlists, do not send Telegram alerts, and do not submit copy trades. They remain ShredStream shadow-only even if `COPY_TRADE_SIGNAL_PROVIDER=shredstream` or `all`.
- `COPY_TRADE_SIGNAL_PROVIDER=shredstream` is no longer a TypeScript ShredStream copy-buy promotion path. Use it only as an operator signal that ShredStream live copies are handled by the Rust worker. The TypeScript bot can still race PumpPortal/Geyser in `parallel` mode; ShredStream live copy buys must run through `tools/jito-shredstream-rs` with a hot Telegram snapshot.
- Compare PumpPortal, Helius, Geyser, and ShredStream accepted wallet events with `npm run wallet-feed-report -- --path=logs/wallet-trades.jsonl`. Add `--since=2026-05-29T00:00:00Z`, `--copyable-only=true`, `--include-diagnostic=false`, or `--limit=50` for a narrower report. Use `npm run wallet-feed-readiness-report -- --role=copytrade --since=<ISO time>` to see the active real copytrade wallets and their provider evidence. Use `npm run shredstream-promotion-gate -- --since=<ISO time>` as the fail-closed gate before changing `COPY_TRADE_SIGNAL_PROVIDER` to `shredstream` or `all`.
- [VA RPC and Geyser runbook](docs/va-geyser-runbook.md) documents the VA endpoints, IP allowlisting, smoke tests, canary env, feed comparison, rollback paths, and the 20-stream Geyser limit.

## ShredStream discovery prototype

PumpPortal remains the production token discovery source. The ShredStream path is a standalone prototype for decoding deshredded transaction JSONL and measuring whether a lower-level feed can beat PumpPortal before it is wired into copy-trading.

Run the prototype against a local JSONL capture:

```bash
SHREDSTREAM_DISCOVERY_ENABLED=true \
SHREDSTREAM_INPUT_PATH=path/to/deshred-sample.jsonl \
SHREDSTREAM_EVENT_LOG_PATH=logs/shred-pump-events.jsonl \
npm run shred-listener
```

Use `SHREDSTREAM_INPUT_PATH=-` to read JSONL from stdin. The input shape is intentionally lightweight so it can be fed by Jito ShredStream's gRPC/deshred sample client later:

```json
{
  "slot": 123,
  "signature": "tx-signature",
  "receivedAtMs": 1710000000000,
  "accountKeys": ["program-or-account-pubkey"],
  "instructions": [
    {
      "programIdIndex": 0,
      "accounts": [0],
      "dataBase64": "base64-instruction-data"
    }
  ]
}
```

Safety defaults:

- `SHREDSTREAM_DISCOVERY_ENABLED=true` is required or the process exits without reading input.
- The listener only writes normalized events to `SHREDSTREAM_EVENT_LOG_PATH`; it does not send Telegram messages or submit trades.
- The decoder only emits Pump/PumpSwap events for the allowlisted program IDs and preserves unknown Pump instructions for later analysis.
- ShredStream is not part of the copy-trade race in this cut. Use it to observe and compare before promoting it to a trigger.

Run the prototype against a local Jito ShredStream proxy gRPC service:

```bash
SHREDSTREAM_DISCOVERY_ENABLED=true \
SHREDSTREAM_SOURCE=grpc \
SHREDSTREAM_GRPC_URL=127.0.0.1:9999 \
SHREDSTREAM_EVENT_LOG_PATH=logs/shred-pump-events.jsonl \
npm run shred-listener
```

The gRPC mode shells out to the Rust sidecar in `tools/shredstream-rs`. The sidecar subscribes to Jito `ShredstreamProxy.SubscribeEntries`, bincode-decodes `Vec<solana_entry::entry::Entry>` with Solana `2.2.1` types to match the current Jito proxy, and writes normalized transaction JSONL to stdout for the Node listener.

Optional override:

```bash
SHREDSTREAM_DECODER_CMD="tools/shredstream-rs/target/release/shredstream-rs watch --grpc-url {grpcUrl}"
```

Validate the sidecar with:

```bash
npm run shredstream-decoder:check
npm run shredstream-decoder:test
```

Compare ShredStream against PumpPortal discovery timing by enabling the PumpPortal observation log on the main bot and running the standalone report:

```bash
SHREDSTREAM_COMPARE_ENABLED=true
PUMPPORTAL_DISCOVERY_LOG_PATH=logs/pumpportal-discovery-events.jsonl
```

Then, after both logs have data:

```bash
npm run shredstream-latency-report -- \
  --pumpportal logs/pumpportal-discovery-events.jsonl \
  --shredstream logs/shred-pump-events.jsonl \
  --comparisons-out logs/shredstream-latency-comparisons.jsonl
```

The report matches by signature/instruction when both feeds expose it, then by signature/mint, then by create mint within a short time window. Negative `shred_minus_pumpportal_ms` means ShredStream arrived first.

The main bot can also run a passive watched-wallet ShredStream observer once the Jito proxy is available locally:

```bash
SHREDSTREAM_WALLET_OBSERVER_ENABLED=true
SHREDSTREAM_WALLET_OBSERVER_STATS_INTERVAL_MS=60000
WALLET_FEED_DIAGNOSTIC_WALLETS=HighVolumeWallet111111111111111111111111111:hv
SHREDSTREAM_SOURCE=grpc
SHREDSTREAM_GRPC_URL=127.0.0.1:9999
SHREDSTREAM_DECODER_CMD="tools/shredstream-rs/target/release/shredstream-rs watch --grpc-url {grpcUrl}"
```

This observer only writes `provider="shredstream"` rows to `WALLET_TRADE_LOG_PATH`. Compare the observed wallet feed with:

```bash
npm run wallet-feed-report -- --path=logs/wallet-trades.jsonl
npm run wallet-feed-report -- --path=logs/wallet-trades.jsonl --copyable-only=true --include-diagnostic=false
npm run wallet-feed-readiness-report -- --role=copytrade --since=2026-05-30T00:00:00Z
```

Wallet-feed comparisons are grouped by signature, target wallet, and mint so duplicate providers for the same watched-wallet trade can be timed without turning ShredStream into a copytrade trigger yet.

Backtest whether ShredStream would have seen existing PumpPortal/Geyser wallet rows with:

```bash
npm run shredstream-wallet-coverage-report -- \
  --wallet logs/wallet-trades.jsonl \
  --shredstream logs/shred-pump-events.jsonl
```

That coverage report also matches by signature, target wallet, and mint. Use `--since`, `--until`, `--providers=pumpportal,geyser`, and `--limit=50` to narrow the overlap window.

Before promotion, the real-copytrade gate must pass:

```bash
npm run shredstream-promotion-gate -- --since=2026-05-30T00:00:00Z
```

The gate intentionally ignores ambient `GATE_*` env defaults. Use explicit CLI flags only if you are deliberately changing rollout criteria:

```bash
npm run shredstream-promotion-gate -- \
  --since=2026-05-30T00:00:00Z \
  --min-copyable-buys=3 \
  --min-shredstream-copyable-buys=3 \
  --min-matched-copyable-groups=3
```

Do not promote if the gate prints `Result=FAIL`. A healthy first promotion window should show at least one active real copytrade wallet, real copyable buys, ShredStream copyable buys for those wallets, and matched copyable groups against another provider. Diagnostic-wallet rows do not count.

After the observed ShredStream wallet rows match the existing feed and the gate passes, promote the Rust worker deliberately. Keep TypeScript as the Telegram/settings/snapshot control plane and post-submit notification surface; do not route ShredStream copy buys through `handleWalletTradeSignal`.

```bash
COPY_TRADE_SIGNAL_PROVIDER=shredstream
COPY_TRADE_HOT_SNAPSHOT_ENABLED=true
COPY_TRADE_HOT_SNAPSHOT_RELOAD_COMMAND="systemctl restart jito-copy-live.service"
JITO_TELEGRAM_SNAPSHOT_PATH=/var/lib/pumpfun/copytrade-hot-snapshot.json
JITO_FAST_COPY_SEND=YES
JITO_DISABLE_SIGNAL_OBSERVATIONS=true
```

That mode does not make the TypeScript ShredStream observer submit buys. The Rust worker preloads the snapshot, matches/builds/signs/sends in-process, and writes post-submit execution records for Telegram/dashboard follow-up.

Rollback is just the signal provider env:

```bash
COPY_TRADE_SIGNAL_PROVIDER=parallel
# or the most conservative path:
COPY_TRADE_SIGNAL_PROVIDER=pumpportal
```

Address lookup table accounts are hydrated by the ShredStream source before Node decoding when ALT lookups are present. The live side-by-side reports still need to keep measuring missed/unknown events before this becomes a trading signal.

## Direct trading SDK trust review

This repo currently uses PumpPortal Lightning for live copy-trade execution, which charges an extra execution fee and uses a PumpPortal-linked trading API key. The planned direct-trading migration is to keep PumpPortal data feeds temporarily, but replace Lightning execution with locally built, locally signed Solana transactions.

The intended SDKs are usable, but should be treated as live-trading supply-chain dependencies rather than blindly trusted packages:

- `@pump-fun/pump-sdk@1.36.0` is the Pump bonding-curve SDK.
- `@pump-fun/pump-swap-sdk@1.16.0` is the PumpSwap AMM SDK.
- Both packages are published under the `@pump-fun` npm scope, have matching Baton maintainers, registry signatures, SHA512 integrity, and tarballs shaped like normal SDK packages with source, dist files, IDLs, program IDs, and buy/sell instruction builders.
- The public `pump-fun/pump-public-docs` repo links to `@pump-fun/pump-sdk` and appears to be the best public canonical Pump docs source.
- The SDK package metadata points at `github.com/pump-fun/pump-sdk` and `github.com/pump-fun/pump-swap-sdk`, but those source repos are not publicly readable right now, so the npm packages are not fully source-verifiable from GitHub.
- No install, postinstall, prepare, bin, shell execution, direct filesystem, direct secret handling, or obvious key-exfiltration behavior was found in the two Pump SDK tarballs during the local audit.
- Red flags to keep in mind: `docs.pump.fun`/docs root behavior is messy, no npm provenance attestations were found for the target versions, `@pump-fun/pump-sdk` pulls `@pump-fun/agent-payments-sdk@1.0.7`, and the wider Solana dependency stack has normal npm audit noise and a few transitive native install scripts.

Direct-trading implementation must add a verification layer before signing any transaction:

- Pin exact SDK versions and commit `package-lock.json` integrity hashes.
- Re-audit package tarballs before SDK upgrades.
- Build transactions in dry-run first and simulate every new route before live sends.
- Reject any generated instruction whose program ID is outside the expected allowlist.
- Start live canaries with tiny funded hot wallets only.

Expected program allowlist for direct Pump execution:

```text
Pump bonding curve: 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P
PumpSwap AMM:       pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA
Pump fee program:   pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ
System program:     11111111111111111111111111111111
SPL Token:          TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA
Token 2022:         TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb
Associated Token:   ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL
Compute Budget:     ComputeBudget111111111111111111111111111111
```

## Research

- [Copy trading wallet research](docs/copy-trading-research.md) - technical plan for adding wallet trade monitoring, dry-run copy-trade planning, and guarded local execution.
- [VA RPC and Geyser runbook](docs/va-geyser-runbook.md) - operator steps for the VA RPC endpoint, Geyser feed, parallel signal race mode, and rollback.
