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

3. Fill in `TELEGRAM_BOT_TOKEN`. Set `TELEGRAM_VERIFY_CODE` to a private invite code people must send before receiving alerts. Set `SUPABASE_URL` and `SUPABASE_SERVICE_ROLE_KEY` to store subscribers in Supabase; `SUPABASE_SERVICE_KEY` and `SUPABASE_SERVICE_ROLE` are also accepted as service-role aliases. Omit the Supabase values to keep using the local JSON fallback. `PUMPPORTAL_API_KEY` is optional for migrations, but supported if you have one. For wallet swap alerts, set `HELIUS_API_KEY`, `HELIUS_WEBHOOK_PUBLIC_URL`, and `HELIUS_WEBHOOK_AUTH_HEADER`.
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
9. To monitor wallet swaps, send `/wallets`, then use the dashboard buttons to add, rename, remove, or list watched wallets. Wallet swap alerts require the Helius webhook env vars.
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
- `/wallets` - Open the watched-wallet dashboard.
- `/copytrade` - Open the copy trade setup dashboard.

## Notes

- Keep only one running instance of this bot per PumpPortal API key.
- If PumpPortal sends a new migration payload shape, the bot still sends a notification with the raw event JSON.
- Telegram messages are HTML escaped before sending.
- To inspect recent on-chain migrations without waiting for a live event, run `npm run past-migrations -- 10`.
- Set `SOLANA_RPC_URL` in `.env` if public Solana RPC rate limits you.
- Wallet swap monitor events are stored in `WALLET_TRADE_LOG_PATH`.
- Wallet swap alerts include copy-trade details when copy wallet(s), amount, and target are configured through `/copytrade`. For copyable SOL-to-token buys, the bot asks PumpPortal `trade-local` to build one unsigned local transaction per configured copy wallet and reports whether each build worked. This is alert output only; it does not sign or execute trades.
- Expose `WEBHOOK_PORT` through your reverse proxy at the exact `HELIUS_WEBHOOK_PUBLIC_URL`, and forward the `Authorization` header unchanged.

## Research

- [Copy trading wallet research](docs/copy-trading-research.md) - technical plan for adding wallet trade monitoring, dry-run copy-trade planning, and guarded local execution.
