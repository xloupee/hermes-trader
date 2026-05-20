import { clearTelegramWebhook, getTelegramBotInfo, getTelegramUpdates, sendTelegramMessage } from "./telegram.js";
import { errorMessage } from "./types.js";
import type { LegacyBotConfig, TelegramChatId, TelegramMessage, TelegramUpdate } from "./types.js";

interface ParsedCommand {
  command: string;
  args: string[];
}

interface SetAlertModeResult {
  ok: boolean;
  label?: string;
}

interface CommandPollerOptions {
  config: LegacyBotConfig;
  testMessage: () => string;
  setAlertMode?: (requestedMode: string) => Promise<SetAlertModeResult>;
  getModeLabel?: () => string;
}

interface TelegramCommandPoller {
  start: () => Promise<void>;
  stop: () => void;
}

export function commandFromMessage(message: TelegramMessage): ParsedCommand | null {
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

export function setupStatus(config: LegacyBotConfig): string {
  return [
    "<b>Setup status</b>",
    `${config.telegramToken ? "OK" : "Missing"} TELEGRAM_BOT_TOKEN`,
    `${config.telegramChatId ? "OK" : "Missing"} TELEGRAM_CHAT_ID`,
    `${config.pumpPortalApiKey ? "OK" : "Missing"} PUMPPORTAL_API_KEY`,
    `<b>Alert mode:</b> ${config.getModeLabel ? config.getModeLabel() : config.pumpPortalSubscriptionMethod}`,
    `<b>PumpPortal URL:</b> <code>${config.pumpPortalWsUrl}</code>`
  ].join("\n");
}

export function helpText(_chatId?: TelegramChatId): string {
  return [
    "<b>Pump.fun notifier bot</b>",
    "",
    "Commands:",
    "/start - Show setup help",
    "/migrations - Watch migrated coins only",
    "/newtokens - Watch newly created tokens only",
    "/both - Watch new tokens and migrated coins",
    "/help - Show commands"
  ].join("\n");
}

export function createTelegramCommandPoller({
  config,
  testMessage: _testMessage,
  setAlertMode,
  getModeLabel: _getModeLabel
}: CommandPollerOptions): TelegramCommandPoller {
  let nextOffset: number | undefined;
  let shouldPoll = true;

  async function reply(chatId: TelegramChatId, text: string): Promise<void> {
    await sendTelegramMessage({
      token: config.telegramToken,
      chatId,
      text
    });
  }

  async function handleUpdate(update: TelegramUpdate): Promise<void> {
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
      case "/migrations":
        await reply(chatId, await changeMode(chatId, "migrations"));
        break;
      case "/newtokens":
        await reply(chatId, await changeMode(chatId, "newtokens"));
        break;
      case "/both":
        await reply(chatId, await changeMode(chatId, "both"));
        break;
      default:
        await reply(chatId, "Unknown command. Send /help to see the bot commands.");
    }
  }

  async function changeMode(chatId: TelegramChatId, requestedMode: string): Promise<string> {
    if (config.telegramChatId && String(chatId) !== String(config.telegramChatId)) {
      return "Only the configured alert chat can change bot mode.";
    }

    if (!setAlertMode) {
      return "Mode switching is not available in this bot process.";
    }

    const mode = requestedMode.toLowerCase();
    const result = await setAlertMode(mode);

    if (!result.ok) {
      return [
        "<b>Unknown mode.</b>",
        "",
        "Use /migrations for migrated coins only.",
        "Use /newtokens for newly created tokens only.",
        "Use /both for both alert types."
      ].join("\n");
    }

    return `<b>Now watching:</b> ${result.label}`;
  }

  async function pollOnce(): Promise<void> {
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
      console.log("Polling for /start, /help, /migrations, /newtokens, and /both");

      while (shouldPoll) {
        try {
          await pollOnce();
        } catch (error) {
          console.error("Telegram polling failed:", errorMessage(error));
          await new Promise((resolve) => setTimeout(resolve, 3000));
        }
      }
    },
    stop() {
      shouldPoll = false;
    }
  };
}
