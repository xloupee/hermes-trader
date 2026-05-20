import { clearTelegramWebhook, getTelegramBotInfo, getTelegramUpdates, sendTelegramMessage } from "./telegram.js";

export function commandFromMessage(message) {
  const text = message?.text?.trim();

  if (!text?.startsWith("/")) {
    return null;
  }

  const [rawCommand, ...args] = text.split(/\s+/);
  const command = rawCommand.toLowerCase().split("@")[0];

  return {
    command,
    args
  };
}

export function setupStatus(config) {
  return [
    "<b>Setup status</b>",
    `${config.telegramToken ? "OK" : "Missing"} TELEGRAM_BOT_TOKEN`,
    `${config.telegramChatId ? "OK" : "Missing"} TELEGRAM_CHAT_ID`,
    `${config.pumpPortalApiKey ? "OK" : "Missing"} PUMPPORTAL_API_KEY`,
    `<b>PumpPortal URL:</b> <code>${config.pumpPortalWsUrl}</code>`
  ].join("\n");
}

export function helpText(chatId) {
  return [
    "<b>Pump.fun notifier bot</b>",
    "",
    "Commands:",
    "/start - Show setup help",
    "/status - Check configured env vars",
    "/chatid - Show this Telegram chat id",
    "/test - Send a sample migration alert",
    "/help - Show commands",
    "",
    `This chat id is <code>${chatId}</code>. Add it to <code>TELEGRAM_CHAT_ID</code> in <code>.env</code>.`
  ].join("\n");
}

export function createTelegramCommandPoller({ config, testMessage }) {
  let nextOffset;
  let shouldPoll = true;

  async function reply(chatId, text) {
    await sendTelegramMessage({
      token: config.telegramToken,
      chatId,
      text
    });
  }

  async function handleUpdate(update) {
    const message = update.message || update.channel_post;
    const chatId = message?.chat?.id;

    if (!chatId) {
      return;
    }

    const parsed = commandFromMessage(message);

    if (!parsed) {
      await reply(chatId, "Send /help to see the bot commands.");
      return;
    }

    switch (parsed.command) {
      case "/start":
      case "/help":
        await reply(chatId, helpText(chatId));
        break;
      case "/chatid":
        await reply(chatId, `<b>Chat id:</b> <code>${chatId}</code>`);
        break;
      case "/status":
        await reply(chatId, setupStatus(config));
        break;
      case "/test":
        await reply(chatId, testMessage());
        break;
      default:
        await reply(chatId, "Unknown command. Send /help to see the bot commands.");
    }
  }

  async function pollOnce() {
    const updates = await getTelegramUpdates({
      token: config.telegramToken,
      offset: nextOffset,
      timeout: 25
    });

    for (const update of updates) {
      nextOffset = update.update_id + 1;

      try {
        await handleUpdate(update);
      } catch (error) {
        console.error("Failed to handle Telegram update:", error);
      }
    }
  }

  return {
    async start() {
      const bot = await getTelegramBotInfo({ token: config.telegramToken });
      await clearTelegramWebhook({ token: config.telegramToken });
      console.log(`Telegram bot ready: @${bot.username}`);
      console.log("Polling for /start, /help, /chatid, /status, and /test");

      while (shouldPoll) {
        try {
          await pollOnce();
        } catch (error) {
          console.error("Telegram polling failed:", error.message);
          await new Promise((resolve) => setTimeout(resolve, 3000));
        }
      }
    },
    stop() {
      shouldPoll = false;
    }
  };
}
