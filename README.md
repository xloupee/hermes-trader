# Pump.fun Migration Telegram Bot

Telegram notifier for Pump.fun coin migrations using PumpPortal's realtime websocket.

## What the bot listens to

PumpPortal's Data API exposes a websocket at:

```text
wss://pumpportal.fun/api/data?api-key=your-api-key-here
```

After connecting, this bot sends:

```json
{ "method": "subscribeMigration" }
```

The docs list `subscribeMigration` as free, and warn to use only one websocket connection at a time.

## Setup

1. Create a Telegram bot with [BotFather](https://t.me/BotFather), then copy the bot token.
2. Copy the example env file:

```bash
cp .env.example .env
```

3. Fill in `TELEGRAM_BOT_TOKEN`. `PUMPPORTAL_API_KEY` is optional for migrations, but supported if you have one.
4. Install dependencies:

```bash
npm install
```

5. Start the bot:

```bash
npm start
```

6. In Telegram, open your bot and send `/start`.
7. Copy the chat id from `/start` or `/chatid`, then add it to `.env` as `TELEGRAM_CHAT_ID`.
8. Send `/test` to confirm Telegram alerts render correctly.
9. Restart the bot so automatic migration alerts can be sent to that chat:

```bash
npm start
```

If `TELEGRAM_CHAT_ID` is empty, commands still work, but live migration alerts are skipped until you add it.

You can also use the one-shot chat-id helper after messaging the bot:

```bash
npm run chat-id
```

## Bot commands

- `/start` - Show setup help and the current chat id.
- `/help` - Show the command list.
- `/chatid` - Print the chat id to put in `.env`.
- `/status` - Show which environment variables are configured.
- `/mode` - Show the current alert mode.
- `/migrations` - Watch migrated coins only.
- `/newtokens` - Watch newly created tokens only.
- `/test` - Send a sample migration alert without needing PumpPortal.

## Notes

- Keep only one running instance of this bot per PumpPortal API key.
- If PumpPortal sends a new migration payload shape, the bot still sends a notification with the raw event JSON.
- Telegram messages are HTML escaped before sending.
- To inspect recent on-chain migrations without waiting for a live event, run `npm run past-migrations -- 10`.
- Set `SOLANA_RPC_URL` in `.env` if public Solana RPC rate limits you.
