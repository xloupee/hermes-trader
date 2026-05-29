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
- Execution risk controls run before PumpPortal calls: `COPY_TRADE_MAX_BUY_SOL`, `COPY_TRADE_DAILY_SOL_CAP`, `COPY_TRADE_MAX_SIGNAL_AGE_MS`, `COPY_TRADE_MAX_SLIPPAGE`, `COPY_TRADE_MAX_PRIORITY_FEE`, `COPY_TRADE_MAX_COPY_WALLETS_PER_CHAT`, and optional `COPY_TRADE_ALLOWED_SOURCES` comma-separated Helius source allowlist. Violations are logged and sent as skipped copy buys. The daily cap is in-memory, counts attempted live submissions, and resets on UTC day boundaries, so keep only one running bot instance and redeploy/restart carefully.
- Copytrade mirrors each distinct target buy transaction. Exact duplicate webhook deliveries for the same observed transaction are ignored while that copy buy is in flight.
- Treat generated trading wallets as hot wallets. Anyone with the private key can withdraw funds, and anyone with the linked API key can trade from the funded wallet.
- Keep `COPY_TRADE_ENABLED=false` or `COPY_TRADE_DRY_RUN=true` as the kill switch. First live run recommendation: fund only about `0.02-0.05 SOL`, set copy amount around `0.001-0.005 SOL`, copy one target wallet, use `COPY_TRADE_ENABLED=true`, `COPY_TRADE_DRY_RUN=false`, `COPY_TRADE_SLIPPAGE=10`, `COPY_TRADE_PRIORITY_FEE=0.00005`, `COPY_TRADE_MAX_BUY_SOL=0.005`, `COPY_TRADE_DAILY_SOL_CAP=0.02`, `COPY_TRADE_MAX_SIGNAL_AGE_MS=60000`, `COPY_TRADE_MAX_SLIPPAGE=15`, `COPY_TRADE_MAX_PRIORITY_FEE=0.0002`, `COPY_TRADE_MAX_COPY_WALLETS_PER_CHAT=1`, optionally set `COPY_TRADE_ALLOWED_SOURCES` after observing real Helius `source` values in dry-run, and watch logs, Telegram, and Solscan before increasing exposure.
- `COPY_TRADE_TRAILING_SELL_ENABLED=true` schedules live PumpPortal Lightning percentage sells after a successful auto copy buy. Scheduled sell timers are in-memory and are cleared if the bot restarts; they only run after live copy buys are enabled and submitted.
- `COPY_TRADE_BUY_PRESSURE_SELL_ENABLED=true` enables the bot-wide buy-pressure exit capability after a confirmed auto copy buy, but each Telegram user must still opt in from `/copytrade` -> Settings. Users can also override their timeout there; otherwise `COPY_TRADE_BUY_PRESSURE_SELL_TIMEOUT_MS` is inherited. It watches the copied mint through PumpPortal token trades plus any Helius watched-wallet trade events, sells `COPY_TRADE_BUY_PRESSURE_SELL_PERCENT` after `COPY_TRADE_BUY_PRESSURE_SELL_MIN_BUYS` and optional `COPY_TRADE_BUY_PRESSURE_SELL_MIN_TOTAL_SOL` are met, or sells on the configured timeout as a fallback. Watchers are persisted in `COPY_TRADE_BUY_PRESSURE_SELL_STATE_PATH` and resume timeout fallback after restart; live/dry-run gates, sell slippage/priority settings, and copied-position token balance checks still apply.
- `DIRECT_EXECUTION_BLOCKHASH_CACHE_MS` and `DIRECT_EXECUTION_BLOCKHASH_WARM_INTERVAL_MS` keep a recent blockhash ready for live direct sends so each copy buy does not wait on a fresh blockhash RPC call.
- `YELLOWSTONE_ENABLED=true` starts an optional Yellowstone gRPC watched-wallet stream. For a QuickNode trial, set `YELLOWSTONE_ENDPOINT` to the gRPC endpoint on port `10000` and `YELLOWSTONE_TOKEN` to the token from the original QuickNode RPC URL. Keep `YELLOWSTONE_SHADOW_ONLY=true` at first; shadow mode writes/logs Yellowstone candidates but does not trigger copy buys or Telegram alerts.
- Expose `WEBHOOK_PORT` through your reverse proxy at the exact `HELIUS_WEBHOOK_PUBLIC_URL`, and forward the `Authorization` header unchanged.

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
