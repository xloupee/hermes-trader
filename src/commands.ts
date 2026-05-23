import {
  answerTelegramCallbackQuery,
  clearTelegramWebhook,
  getTelegramBotInfo,
  getTelegramUpdates,
  sendTelegramMessage,
  setTelegramCommands
} from "./telegram.js";
import { errorMessage } from "./types.js";
import { isValidSolanaAddress } from "./wallet-monitor.js";
import type {
  AlertModeValue,
  LegacyBotConfig,
  SubscriberRecord,
  SubscriberStore,
  TelegramCallbackQuery,
  TelegramChatId,
  TelegramMessage,
  TelegramReplyMarkup,
  TelegramUpdate,
  WatchedWallet
} from "./types.js";

const telegramCommands = [
  { command: "start", description: "Start notifications" },
  { command: "verify", description: "Verify this chat" },
  { command: "stop", description: "Stop notifications" },
  { command: "migrations", description: "Watch migrated coins only" },
  { command: "newtokens", description: "Watch newly created tokens only" },
  { command: "both", description: "Watch new tokens and migrated coins" },
  { command: "watch", description: "Watch a wallet" },
  { command: "renamewallet", description: "Rename a watched wallet" },
  { command: "unwatch", description: "Stop watching a wallet" },
  { command: "wallets", description: "List watched wallets" },
  { command: "copywallet", description: "Set copy wallet" },
  { command: "copyamount", description: "Set copy amount" },
  { command: "copytrade", description: "Open copy trade menu" },
  { command: "copystatus", description: "Show copy settings" },
  { command: "help", description: "Show commands" }
];

const MAX_WALLET_NICKNAME_LENGTH = 48;
const PENDING_COPY_INPUT_TTL_MS = 10 * 60 * 1000;

type PendingCopyInputAction = "copy_wallet" | "copy_amount";

interface PendingCopyInput {
  action: PendingCopyInputAction;
  expiresAt: number;
}

interface ParsedCommand {
  command: string;
  args: string[];
}

interface CommandPollerOptions {
  config: LegacyBotConfig;
  testMessage: () => string;
  subscribers?: SubscriberStore;
  onWalletWatchlistChange?: () => string | void | Promise<string | void>;
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
    "/watch &lt;wallet&gt; [nickname] - Watch a wallet's swaps",
    "/renamewallet &lt;wallet&gt; &lt;nickname|-&gt; - Rename or clear a wallet nickname",
    "/unwatch &lt;wallet&gt; - Stop watching a wallet",
    "/wallets - List watched wallets and nicknames",
    "/copywallet &lt;public-wallet&gt; - Save your copy wallet public address",
    "/copyamount &lt;sol&gt; - Save your fixed copy size",
    "/copytrade - Open copy trade setup menu",
    "/copystatus - Show copy settings and watched wallets",
    "/help - Show commands"
  ].join("\n");
}

export function createTelegramCommandPoller({
  config,
  testMessage: _testMessage,
  subscribers,
  onWalletWatchlistChange
}: CommandPollerOptions): TelegramCommandPoller {
  let nextOffset: number | undefined;
  let shouldPoll = true;
  const pendingCopyInputs = new Map<string, PendingCopyInput>();

  async function reply(chatId: TelegramChatId, text: string, replyMarkup?: TelegramReplyMarkup): Promise<void> {
    await sendTelegramMessage({
      token: config.telegramToken,
      chatId,
      text,
      replyMarkup
    });
  }

  async function handleUpdate(update: TelegramUpdate): Promise<void> {
    if (update.callback_query) {
      await handleCallbackQuery(update.callback_query);
      return;
    }

    const message = update.message || update.channel_post;
    const chatId = message?.chat?.id;

    if (!chatId) {
      return;
    }

    const parsed = commandFromMessage(message);

    if (!parsed) {
      const pendingResponse = await handlePendingCopyInput(chatId, message);

      if (pendingResponse) {
        await reply(chatId, pendingResponse.text, pendingResponse.replyMarkup);
        return;
      }

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
      case "/renamewallet":
        await reply(chatId, await renameWallet(chatId, parsed.args));
        break;
      case "/unwatch":
        await reply(chatId, await unwatchWallet(chatId, parsed.args));
        break;
      case "/wallets":
        await reply(chatId, listWallets(chatId));
        break;
      case "/copywallet":
        await reply(chatId, await setCopyWallet(chatId, parsed.args));
        break;
      case "/copyamount":
        await reply(chatId, await setCopyAmount(chatId, parsed.args));
        break;
      case "/copytrade": {
        const dashboard = copyTradeDashboard(chatId);
        await reply(chatId, dashboard.text, dashboard.replyMarkup);
        break;
      }
      case "/copystatus":
        await reply(chatId, copyStatus(chatId));
        break;
      default:
        await reply(chatId, "Unknown command. Send /help to see the bot commands.");
    }
  }

  async function handleCallbackQuery(callbackQuery: TelegramCallbackQuery): Promise<void> {
    await answerTelegramCallbackQuery({ token: config.telegramToken, callbackQueryId: callbackQuery.id }).catch((error) => {
      console.warn(`Could not answer Telegram callback query: ${errorMessage(error)}`);
    });

    const chatId = callbackQuery.message?.chat?.id;

    if (!chatId) {
      return;
    }

    const data = callbackQuery.data || "";
    const gate = requireVerified(chatId);

    if (gate) {
      await reply(chatId, gate);
      return;
    }

    if (data === "copytrade:status" || data === "copytrade:dashboard") {
      const dashboard = copyTradeDashboard(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "copytrade:set_wallet") {
      setPendingCopyInput(chatId, "copy_wallet");
      await reply(chatId, "Send the public wallet address you want to use as your copy wallet.");
      return;
    }

    if (data === "copytrade:set_amount") {
      setPendingCopyInput(chatId, "copy_amount");
      await reply(chatId, "Send the fixed copy size in SOL, for example <code>0.1</code>.");
      return;
    }

    if (data === "copytrade:choose_target") {
      const targetPicker = copyTargetPicker(chatId);
      await reply(chatId, targetPicker.text, targetPicker.replyMarkup);
      return;
    }

    if (data === "copytrade:wallets") {
      await reply(chatId, listWallets(chatId));
      return;
    }

    if (data.startsWith("copytrade:target:")) {
      const targetWallet = data.slice("copytrade:target:".length);
      const updated = await subscribers?.setCopyTargetWallet(chatId, targetWallet);

      if (!updated) {
        await reply(chatId, "That wallet is not being watched in this chat.");
        return;
      }

      const dashboard = copyTradeDashboard(chatId);
      await reply(chatId, `<b>Copy target saved.</b>\n\n${dashboard.text}`, dashboard.replyMarkup);
      return;
    }

    if (data === "copytrade:target:clear") {
      const updated = await subscribers?.setCopyTargetWallet(chatId, null);

      if (!updated) {
        await reply(chatId, verificationPrompt());
        return;
      }

      const dashboard = copyTradeDashboard(chatId);
      await reply(chatId, `<b>Copy target cleared.</b>\n\n${dashboard.text}`, dashboard.replyMarkup);
      return;
    }

    await reply(chatId, "That copy trade action is no longer available. Send /copytrade to reopen the menu.");
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
    const nickname = args.slice(1).join(" ").trim();

    if (!wallet) {
      return "Send <code>/watch wallet-address optional-nickname</code> to monitor a wallet.";
    }

    if (!isValidSolanaAddress(wallet)) {
      return "That does not look like a Solana wallet address.";
    }

    const nicknameError = validateNickname(nickname);

    if (nicknameError) {
      return nicknameError;
    }

    const updated = await subscribers?.watchWallet(chatId, wallet, nickname);

    if (!updated) {
      return verificationPrompt();
    }

    const syncWarning = await onWalletWatchlistChange?.();
    const success = nickname
      ? `<b>Watching wallet:</b> ${escapeWalletLabel(nickname)}\n<code>${wallet}</code>`
      : `<b>Watching wallet:</b>\n<code>${wallet}</code>`;

    return syncWarning ? `${success}\n\n${syncWarning}` : success;
  }

  async function renameWallet(chatId: TelegramChatId, args: string[]): Promise<string> {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallet = args[0]?.trim();
    const nickname = args.slice(1).join(" ").trim();

    if (!wallet || !nickname) {
      return "Send <code>/renamewallet wallet-address nickname</code> to rename, or <code>/renamewallet wallet-address -</code> to clear.";
    }

    if (!isValidSolanaAddress(wallet)) {
      return "That does not look like a Solana wallet address.";
    }

    const nextNickname = nickname === "-" ? null : nickname;
    const nicknameError = nextNickname === null ? null : validateNickname(nextNickname);

    if (nicknameError) {
      return nicknameError;
    }

    const updated = await subscribers?.renameWallet(chatId, wallet, nextNickname);

    if (!updated) {
      return "That wallet is not being watched in this chat.";
    }

    if (nextNickname === null) {
      return `<b>Cleared wallet nickname:</b>\n<code>${wallet}</code>`;
    }

    return `<b>Renamed wallet:</b> ${escapeWalletLabel(nextNickname)}\n<code>${wallet}</code>`;
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
      return "No watched wallets for this chat. Add one with <code>/watch wallet-address optional-nickname</code>.";
    }

    return [
      "<b>Watched wallets</b>",
      ...wallets.map((wallet) =>
        wallet.label ? `${escapeWalletLabel(wallet.label)}\n<code>${wallet.address}</code>` : `<code>${wallet.address}</code>`
      )
    ].join("\n\n");
  }

  async function setCopyWallet(chatId: TelegramChatId, args: string[]): Promise<string> {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallet = args[0]?.trim();

    if (!wallet) {
      return "Send <code>/copywallet public-wallet-address</code> to save your copy wallet.";
    }

    if (!isValidSolanaAddress(wallet)) {
      return "That does not look like a Solana wallet address.";
    }

    const updated = await subscribers?.setCopyWallet(chatId, wallet);

    if (!updated) {
      return verificationPrompt();
    }

    return `<b>Copy wallet saved:</b>\n<code>${wallet}</code>`;
  }

  async function setCopyAmount(chatId: TelegramChatId, args: string[]): Promise<string> {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const rawAmount = args[0]?.trim();
    const amount = Number(rawAmount);

    if (!rawAmount || !Number.isFinite(amount) || amount <= 0) {
      return "Send <code>/copyamount 0.1</code> to save a fixed copy size in SOL.";
    }

    const updated = await subscribers?.setCopyAmountSol(chatId, amount);

    if (!updated) {
      return verificationPrompt();
    }

    return `<b>Copy amount saved:</b> ${formatSolAmount(amount)} SOL`;
  }

  function setPendingCopyInput(chatId: TelegramChatId, action: PendingCopyInputAction): void {
    pendingCopyInputs.set(String(chatId), {
      action,
      expiresAt: Date.now() + PENDING_COPY_INPUT_TTL_MS
    });
  }

  async function handlePendingCopyInput(
    chatId: TelegramChatId,
    message: TelegramMessage
  ): Promise<{ text: string; replyMarkup?: TelegramReplyMarkup } | null> {
    const pending = pendingCopyInputs.get(String(chatId));

    if (!pending) {
      return null;
    }

    if (pending.expiresAt < Date.now()) {
      pendingCopyInputs.delete(String(chatId));
      return {
        text: "That copy trade setup step expired. Send /copytrade to start again."
      };
    }

    const value = message.text?.trim() || "";

    if (pending.action === "copy_wallet") {
      if (!isValidSolanaAddress(value)) {
        return {
          text: "That does not look like a Solana wallet address. Send a public wallet address, or send /copytrade to restart."
        };
      }

      pendingCopyInputs.delete(String(chatId));
      const updated = await subscribers?.setCopyWallet(chatId, value);

      if (!updated) {
        return { text: verificationPrompt() };
      }

      const dashboard = copyTradeDashboard(chatId);
      return {
        text: `<b>Copy wallet saved:</b>\n<code>${value}</code>\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    const amount = Number(value);

    if (!Number.isFinite(amount) || amount <= 0) {
      return {
        text: "That does not look like a valid SOL amount. Send a number like <code>0.1</code>, or send /copytrade to restart."
      };
    }

    pendingCopyInputs.delete(String(chatId));
    const updated = await subscribers?.setCopyAmountSol(chatId, amount);

    if (!updated) {
      return { text: verificationPrompt() };
    }

    const dashboard = copyTradeDashboard(chatId);
    return {
      text: `<b>Copy amount saved:</b> ${formatSolAmount(amount)} SOL\n\n${dashboard.text}`,
      replyMarkup: dashboard.replyMarkup
    };
  }

  function copyStatus(chatId: TelegramChatId): string {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const subscriber = subscribers?.get(chatId);
    const wallets = subscriber?.watchedWallets || [];
    const watchedWalletLines =
      wallets.length === 0
        ? ["No watched wallets."]
        : wallets.map((wallet) =>
            wallet.label ? `${escapeWalletLabel(wallet.label)}\n<code>${wallet.address}</code>` : `<code>${wallet.address}</code>`
          );

    return [
      "<b>Copy settings</b>",
      `<b>Copy wallet:</b> ${subscriber?.copyWalletAddress ? `<code>${subscriber.copyWalletAddress}</code>` : "Not set"}`,
      `<b>Copy amount:</b> ${subscriber?.copyAmountSol ? `${formatSolAmount(subscriber.copyAmountSol)} SOL` : "Not set"}`,
      `<b>Copy target:</b> ${formatCopyTarget(subscriber || null)}`,
      "",
      "<b>Watched wallets</b>",
      ...watchedWalletLines
    ].join("\n");
  }

  function copyTradeDashboard(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const gate = requireVerified(chatId);

    if (gate) {
      return {
        text: gate,
        replyMarkup: copyTradeDashboardReplyMarkup()
      };
    }

    const subscriber = subscribers?.get(chatId) || null;
    const watchedWalletCount = subscriber?.watchedWallets.length || 0;
    const text = [
      "<b>Copy trade</b>",
      `<b>Copy wallet:</b> ${subscriber?.copyWalletAddress ? `<code>${subscriber.copyWalletAddress}</code>` : "Not set"}`,
      `<b>Copy amount:</b> ${subscriber?.copyAmountSol ? `${formatSolAmount(subscriber.copyAmountSol)} SOL` : "Not set"}`,
      `<b>Copy target:</b> ${formatCopyTarget(subscriber)}`,
      `<b>Watched wallets:</b> ${watchedWalletCount}`,
      "",
      "Use the buttons below to manage copy trade settings."
    ].join("\n");

    return {
      text,
      replyMarkup: copyTradeDashboardReplyMarkup()
    };
  }

  function copyTradeDashboardReplyMarkup(): TelegramReplyMarkup {
    return {
      inline_keyboard: [
        [
          { text: "Status", callback_data: "copytrade:status" },
          { text: "Set Wallet", callback_data: "copytrade:set_wallet" }
        ],
        [
          { text: "Set Amount", callback_data: "copytrade:set_amount" },
          { text: "Choose Target", callback_data: "copytrade:choose_target" }
        ],
        [{ text: "Watched Wallets", callback_data: "copytrade:wallets" }]
      ]
    };
  }

  function copyTargetPicker(chatId: TelegramChatId): { text: string; replyMarkup?: TelegramReplyMarkup } {
    const gate = requireVerified(chatId);

    if (gate) {
      return { text: gate };
    }

    const wallets = subscribers?.listWatchedWallets(chatId) || [];

    if (wallets.length === 0) {
      return {
        text: "No watched wallets yet. Add one with <code>/watch wallet-address optional-nickname</code>."
      };
    }

    return {
      text: "<b>Choose copy target</b>",
      replyMarkup: {
        inline_keyboard: [
          ...wallets.map((wallet) => [
            {
              text: walletButtonLabel(wallet),
              callback_data: `copytrade:target:${wallet.address}`
            }
          ]),
          [{ text: "Clear Target", callback_data: "copytrade:target:clear" }],
          [{ text: "Back", callback_data: "copytrade:dashboard" }]
        ]
      }
    };
  }

  function formatCopyTarget(subscriber: SubscriberRecord | null): string {
    const target = subscriber?.copyTargetWalletAddress;

    if (!target) {
      return "Not set";
    }

    const wallet = subscriber?.watchedWallets.find((entry) => entry.address === target);

    if (wallet?.label) {
      return `${escapeWalletLabel(wallet.label)} <code>${target}</code>`;
    }

    return `<code>${target}</code>`;
  }

  function walletButtonLabel(wallet: WatchedWallet): string {
    return wallet.label || shortenAddress(wallet.address);
  }

  function shortenAddress(value: string): string {
    return value.length <= 16 ? value : `${value.slice(0, 6)}...${value.slice(-6)}`;
  }

  function escapeWalletLabel(value: string): string {
    return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
  }

  function formatSolAmount(value: number): string {
    return new Intl.NumberFormat("en-US", {
      maximumFractionDigits: 9
    }).format(value);
  }

  function validateNickname(value: string): string | null {
    if (value.length > MAX_WALLET_NICKNAME_LENGTH) {
      return `Wallet nickname must be ${MAX_WALLET_NICKNAME_LENGTH} characters or fewer.`;
    }

    return null;
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
        "Polling for /start, /help, /verify, /stop, /migrations, /newtokens, /both, /watch, /renamewallet, /unwatch, /wallets, /copywallet, /copyamount, /copytrade, and /copystatus"
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
