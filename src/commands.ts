import {
  clearTelegramWebhook,
  getTelegramBotInfo,
  getTelegramUpdates,
  sendTelegramMessage,
  setTelegramCommands
} from "./telegram.js";
import { errorMessage } from "./types.js";
import { isValidSolanaAddress } from "./wallet-monitor.js";
import type { AlertModeValue, LegacyBotConfig, SubscriberStore, TelegramChatId, TelegramMessage, TelegramUpdate } from "./types.js";

const telegramCommands = [
  { command: "start", description: "Start notifications" },
  { command: "verify", description: "Verify this chat" },
  { command: "stop", description: "Stop notifications" },
  { command: "migrations", description: "Watch migrated coins only" },
  { command: "newtokens", description: "Watch newly created tokens only" },
  { command: "both", description: "Watch new tokens and migrated coins" },
  { command: "watch", description: "Watch a wallet" },
  { command: "unwatch", description: "Stop watching a wallet" },
  { command: "wallets", description: "List watched wallets" },
  { command: "copywallet", description: "Add copy wallet" },
  { command: "uncopywallet", description: "Remove copy wallet" },
  { command: "copywallets", description: "List copy wallets" },
  { command: "copyamount", description: "Set copy amount" },
  { command: "copystatus", description: "Show copy settings" },
  { command: "copytest", description: "Test recent copy alerts" },
  { command: "help", description: "Show commands" }
];

interface ParsedCommand {
  command: string;
  args: string[];
}

interface CommandPollerOptions {
  config: LegacyBotConfig;
  testMessage: () => string;
  subscribers?: SubscriberStore;
  onWalletWatchlistChange?: () => string | void | Promise<string | void>;
  onCopyTest?: (chatId: TelegramChatId) => string | Promise<string>;
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

function modeLabel(mode: AlertModeValue): string {
  if (mode === "migrations") {
    return "Migrated coins only";
  }

  if (mode === "newtokens") {
    return "New tokens only";
  }

  return "New tokens and migrated coins";
}

function chooseModeText(): string {
  return [
    "Choose what you want to watch:",
    "/migrations - Migrated coins only",
    "/newtokens - Newly created tokens only",
    "/both - New tokens and migrated coins"
  ].join("\n");
}

function verificationPrompt(): string {
  return ["<b>Verification required.</b>", "", "Send:", "<code>/verify your-code</code>"].join("\n");
}

export function helpText(_chatId?: TelegramChatId): string {
  return [
    "<b>Pump.fun notifier bot</b>",
    "",
    "Commands:",
    "/start - Start notifications",
    "/verify &lt;code&gt; - Verify this chat",
    "/stop - Stop notifications",
    "/migrations - Watch migrated coins only",
    "/newtokens - Watch newly created tokens only",
    "/both - Watch new tokens and migrated coins",
    "/watch &lt;wallet&gt; [label] - Watch a wallet's swaps",
    "/unwatch &lt;wallet&gt; - Stop watching a wallet",
    "/wallets - List watched wallets",
    "/copywallet &lt;public-wallet&gt; - Add a copy wallet public address",
    "/uncopywallet &lt;public-wallet&gt; - Remove a copy wallet",
    "/copywallets - List copy wallets",
    "/copyamount &lt;sol&gt; - Set your fixed copy buy amount",
    "/copystatus - Show copy-trade dry-run settings",
    "/copytest - Scan recent buys for watched wallets",
    "/help - Show commands"
  ].join("\n");
}

export function createTelegramCommandPoller({
  config,
  testMessage: _testMessage,
  subscribers,
  onWalletWatchlistChange,
  onCopyTest
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
        await reply(chatId, await startNotifications(chatId, parsed.args));
        break;
      case "/help":
        await reply(chatId, helpText(chatId));
        break;
      case "/verify":
        await reply(chatId, await verifyChat(chatId, parsed.args));
        break;
      case "/stop":
        await reply(chatId, await stopNotifications(chatId));
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
      case "/watch":
        await reply(chatId, await watchWallet(chatId, parsed.args));
        break;
      case "/unwatch":
        await reply(chatId, await unwatchWallet(chatId, parsed.args));
        break;
      case "/wallets":
        await reply(chatId, listWallets(chatId));
        break;
      case "/copywallet":
        await reply(chatId, await addCopyWallet(chatId, parsed.args));
        break;
      case "/uncopywallet":
        await reply(chatId, await removeCopyWallet(chatId, parsed.args));
        break;
      case "/copywallets":
        await reply(chatId, listCopyWallets(chatId));
        break;
      case "/copyamount":
        await reply(chatId, await setCopyAmount(chatId, parsed.args));
        break;
      case "/copystatus":
        await reply(chatId, copyStatus(chatId));
        break;
      case "/copytest":
        await reply(chatId, await copyTest(chatId));
        break;
      default:
        await reply(chatId, "Unknown command. Send /help to see the bot commands.");
    }
  }

  async function startNotifications(chatId: TelegramChatId, args: string[]): Promise<string> {
    const subscriber = subscribers?.get(chatId);

    if (subscriber) {
      if (!subscriber.mode) {
        return `<b>You are verified.</b>\nNotifications are not on yet.\n\n${chooseModeText()}`;
      }

      return `<b>You are verified.</b>\nCurrent alerts: ${modeLabel(subscriber.mode)}\n\n${helpText(chatId)}`;
    }

    if (args.length > 0) {
      return verifyChat(chatId, args);
    }

    if (config.telegramVerifyCode) {
      return verificationPrompt();
    }

    return verifyChat(chatId, args);
  }

  async function verifyChat(chatId: TelegramChatId, args: string[]): Promise<string> {
    if (!subscribers) {
      return "Subscriber verification is not available in this bot process.";
    }

    if (config.telegramVerifyCode) {
      const submittedCode = args.join(" ").trim();

      if (!submittedCode) {
        return "Send <code>/verify your-code</code> to turn on notifications for this chat.";
      }

      if (submittedCode !== config.telegramVerifyCode) {
        return "Invalid verification code.";
      }
    }

    await subscribers.add(chatId);
    return `<b>Verified.</b>\n\n${chooseModeText()}`;
  }

  function requireVerified(chatId: TelegramChatId): string | null {
    if (!subscribers) {
      return "Subscriber controls are not available in this bot process.";
    }

    if (!subscribers.has(chatId)) {
      return verificationPrompt();
    }

    return null;
  }

  async function stopNotifications(chatId: TelegramChatId): Promise<string> {
    if (!subscribers) {
      return "Subscriber controls are not available in this bot process.";
    }

    await subscribers.remove(chatId);
    return "Notifications are off for this chat.";
  }

  async function changeMode(chatId: TelegramChatId, requestedMode: AlertModeValue): Promise<string> {
    if (!subscribers) {
      return "Subscriber controls are not available in this bot process.";
    }

    if (!subscribers.has(chatId)) {
      return verificationPrompt();
    }

    const updated = await subscribers.setMode(chatId, requestedMode);

    if (!updated) {
      return verificationPrompt();
    }

    return `<b>Now watching:</b> ${modeLabel(requestedMode)}`;
  }

  async function watchWallet(chatId: TelegramChatId, args: string[]): Promise<string> {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallet = args[0]?.trim();
    const label = args.slice(1).join(" ").trim();

    if (!wallet) {
      return "Send <code>/watch wallet-address optional-label</code> to monitor a wallet.";
    }

    if (!isValidSolanaAddress(wallet)) {
      return "That does not look like a Solana wallet address.";
    }

    const updated = await subscribers?.watchWallet(chatId, wallet, label);

    if (!updated) {
      return verificationPrompt();
    }

    const syncWarning = await onWalletWatchlistChange?.();
    const success = label
      ? `<b>Watching wallet:</b> ${escapeWalletLabel(label)}\n<code>${wallet}</code>`
      : `<b>Watching wallet:</b>\n<code>${wallet}</code>`;

    return syncWarning ? `${success}\n\n${syncWarning}` : success;
  }

  async function unwatchWallet(chatId: TelegramChatId, args: string[]): Promise<string> {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallet = args[0]?.trim();

    if (!wallet) {
      return "Send <code>/unwatch wallet-address</code> to stop monitoring a wallet.";
    }

    if (!isValidSolanaAddress(wallet)) {
      return "That does not look like a Solana wallet address.";
    }

    const removed = await subscribers?.unwatchWallet(chatId, wallet);

    if (!removed) {
      return "That wallet was not being watched in this chat.";
    }

    const syncWarning = await onWalletWatchlistChange?.();
    const success = `<b>Stopped watching wallet:</b>\n<code>${wallet}</code>`;
    return syncWarning ? `${success}\n\n${syncWarning}` : success;
  }

  function listWallets(chatId: TelegramChatId): string {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallets = subscribers?.listWatchedWallets(chatId) || [];

    if (wallets.length === 0) {
      return "No watched wallets for this chat. Add one with <code>/watch wallet-address optional-label</code>.";
    }

    return [
      "<b>Watched wallets</b>",
      ...wallets.map((wallet) =>
        wallet.label ? `${escapeWalletLabel(wallet.label)}\n<code>${wallet.address}</code>` : `<code>${wallet.address}</code>`
      )
    ].join("\n\n");
  }

  async function addCopyWallet(chatId: TelegramChatId, args: string[]): Promise<string> {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallet = args[0]?.trim();

    if (!wallet) {
      return "Send <code>/copywallet public-wallet-address</code> to add a copy wallet.";
    }

    if (!isValidSolanaAddress(wallet)) {
      return "That does not look like a Solana public wallet address.";
    }

    const updated = await subscribers?.setCopyWallet(chatId, wallet);

    if (!updated) {
      return verificationPrompt();
    }

    const copyWallets = subscribers?.listCopyWallets(chatId) || [];

    return `<b>Copy wallet added:</b>\n<code>${wallet}</code>\n\n<b>Total copy wallets:</b> ${copyWallets.length}\n\nThis is build-only. The bot will not sign or send transactions.`;
  }

  async function removeCopyWallet(chatId: TelegramChatId, args: string[]): Promise<string> {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallet = args[0]?.trim();

    if (!wallet) {
      return "Send <code>/uncopywallet public-wallet-address</code> to remove a copy wallet.";
    }

    if (!isValidSolanaAddress(wallet)) {
      return "That does not look like a Solana public wallet address.";
    }

    const removed = await subscribers?.removeCopyWallet(chatId, wallet);

    if (!removed) {
      return "That copy wallet was not configured for this chat.";
    }

    return `<b>Copy wallet removed:</b>\n<code>${wallet}</code>`;
  }

  function listCopyWallets(chatId: TelegramChatId): string {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const copyWallets = subscribers?.listCopyWallets(chatId) || [];

    if (copyWallets.length === 0) {
      return "No copy wallets for this chat. Add one with <code>/copywallet public-wallet-address</code>.";
    }

    return ["<b>Copy wallets</b>", ...copyWallets.map((wallet) => `<code>${wallet}</code>`)].join("\n\n");
  }

  async function setCopyAmount(chatId: TelegramChatId, args: string[]): Promise<string> {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const rawAmount = args[0]?.trim();
    const amount = Number(rawAmount);

    if (!rawAmount) {
      return "Send <code>/copyamount 0.01</code> to set your fixed copy buy size.";
    }

    if (!Number.isFinite(amount) || amount <= 0) {
      return "Copy amount must be a number greater than 0.";
    }

    const updated = await subscribers?.setCopySolAmount(chatId, amount);

    if (!updated) {
      return verificationPrompt();
    }

    return `<b>Copy amount set:</b> ${formatSolAmount(amount)} SOL`;
  }

  function copyStatus(chatId: TelegramChatId): string {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const subscriber = subscribers?.get(chatId);
    const wallets = subscribers?.listWatchedWallets(chatId) || [];
    const copyWallets = subscribers?.listCopyWallets(chatId) || [];
    const copyAmount = subscriber?.copySolAmount ?? config.copyDefaultSolAmount ?? 0.01;
    const buildReady = copyWallets.length > 0 ? "Ready for PumpPortal unsigned tx builds" : "Set /copywallet to enable PumpPortal unsigned tx builds";

    return [
      "<b>Copy dry-run status</b>",
      "",
      `<b>Copy wallets:</b> ${copyWallets.length}`,
      ...copyWallets.map((wallet) => `<code>${wallet}</code>`),
      `<b>Copy amount:</b> ${formatSolAmount(copyAmount)} SOL`,
      `<b>Build-only:</b> ${buildReady}`,
      `<b>Watched wallets:</b> ${wallets.length}`,
      ...wallets.map((wallet) => (wallet.label ? `${escapeWalletLabel(wallet.label)} - <code>${wallet.address}</code>` : `<code>${wallet.address}</code>`))
    ].join("\n");
  }

  async function copyTest(chatId: TelegramChatId): Promise<string> {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallets = subscribers?.listWatchedWallets(chatId) || [];

    if (wallets.length === 0) {
      return "No watched wallets for this chat. Add one with <code>/watch wallet-address optional-label</code>.";
    }

    if (!onCopyTest) {
      return "Copy test is not available in this bot process.";
    }

    return onCopyTest(chatId);
  }

  function escapeWalletLabel(value: string): string {
    return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
  }

  function formatSolAmount(value: number): string {
    return new Intl.NumberFormat("en-US", {
      maximumFractionDigits: Math.abs(value) < 0.001 ? 9 : 6
    }).format(value);
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
      await setTelegramCommands({ token: config.telegramToken, commands: telegramCommands });
      console.log(`Telegram bot ready: @${bot.username}`);
      console.log(
        "Polling for /start, /help, /verify, /stop, /migrations, /newtokens, /both, /watch, /unwatch, /wallets, /copywallet, /uncopywallet, /copywallets, /copyamount, /copystatus, and /copytest"
      );

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
