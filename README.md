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
- Expose `WEBHOOK_PORT` through your reverse proxy at the exact `HELIUS_WEBHOOK_PUBLIC_URL`, and forward the `Authorization` header unchanged.

## Research

- [Copy trading wallet research](docs/copy-trading-research.md) - technical plan for adding wallet trade monitoring, dry-run copy-trade planning, and guarded local execution.
