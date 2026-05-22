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

3. Fill in `TELEGRAM_BOT_TOKEN`. Set `TELEGRAM_VERIFY_CODE` to a private invite code people must send before receiving alerts. `PUMPPORTAL_API_KEY` is optional for migrations, but supported if you have one. For `/watch` swap alerts, set `HELIUS_API_KEY`, `HELIUS_WEBHOOK_PUBLIC_URL`, and `HELIUS_WEBHOOK_AUTH_HEADER`.
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
8. Choose your alert mode with `/migrations`, `/newtokens`, or `/both`.
9. To monitor wallet swaps, send `/watch wallet-address optional-nickname`. Wallet swap alerts require the Helius webhook env vars.
10. Restart the bot if you changed `.env`:

```bash
npm start
```

Verified subscribers and their alert modes are stored in `TELEGRAM_SUBSCRIBERS_PATH`. `TELEGRAM_CHAT_ID` is optional, but when present it seeds that chat as an existing verified subscriber in migration-only mode.

## Bot commands

- `/start` - Start notifications.
- `/verify <code>` - Verify this chat for notifications.
- `/stop` - Stop notifications for this chat.
- `/help` - Show the command list.
- `/migrations` - Watch migrated coins only for this chat.
- `/newtokens` - Watch newly created tokens only for this chat.
- `/both` - Watch new tokens and migrated coins for this chat.
- `/watch <wallet> [nickname]` - Watch a wallet's swaps for this chat.
- `/renamewallet <wallet> <nickname|->` - Rename or clear a watched wallet nickname.
- `/unwatch <wallet>` - Stop watching a wallet for this chat.
- `/wallets` - List watched wallets and nicknames for this chat.
- `/copywallet <public-wallet>` - Save this chat's copy wallet public address.
- `/copyamount <sol>` - Save this chat's fixed copy size in SOL.
- `/copytrade` - Open the copy trade setup dashboard.
- `/copystatus` - Show copy settings and watched wallets.

## Notes

- Keep only one running instance of this bot per PumpPortal API key.
- If PumpPortal sends a new migration payload shape, the bot still sends a notification with the raw event JSON.
- Telegram messages are HTML escaped before sending.
- To inspect recent on-chain migrations without waiting for a live event, run `npm run past-migrations -- 10`.
- Set `SOLANA_RPC_URL` in `.env` if public Solana RPC rate limits you.
- Wallet swap monitor events are stored in `WALLET_TRADE_LOG_PATH`.
- Wallet swap alerts include copy-trade details when `/copywallet`, `/copyamount`, and a `/copytrade` target are configured. This is alert output only; it does not execute trades.
- Expose `WEBHOOK_PORT` through your reverse proxy at the exact `HELIUS_WEBHOOK_PUBLIC_URL`, and forward the `Authorization` header unchanged.

## Research

- [Copy trading wallet research](docs/copy-trading-research.md) - technical plan for adding wallet trade monitoring, dry-run copy-trade planning, and guarded local execution.
