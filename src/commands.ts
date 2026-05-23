import {
  answerTelegramCallbackQuery,
  clearTelegramWebhook,
  getTelegramBotInfo,
  getTelegramUpdates,
  sendTelegramMessage,
  setTelegramCommands
} from "./telegram.js";
import { createPumpPortalLightningWallet } from "./pumpportal.js";
import { encryptSecret, encryptionSecretReady } from "./secrets.js";
import { errorMessage } from "./types.js";
import { isValidSolanaAddress } from "./wallet-monitor.js";
import type {
  AlertModeValue,
  LegacyBotConfig,
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
  { command: "alerts", description: "Open alert mode dashboard" },
  { command: "trackwallets", description: "Open track wallet dashboard" },
  { command: "mywallets", description: "Open my wallets dashboard" },
  { command: "copytrade", description: "Open copy trade menu" },
  { command: "help", description: "Show commands" }
];

const MAX_WALLET_NICKNAME_LENGTH = 48;
const PENDING_COPY_INPUT_TTL_MS = 10 * 60 * 1000;

type PendingCopyInputAction = "copy_amount";
type PendingWalletInputAction =
  | "watch_wallet"
  | "rename_wallet"
  | "unwatch_wallet"
  | "copytrade_wallet"
  | "rename_copytrade_wallet"
  | "remove_copytrade_wallet";
type ToggleAlertType = Exclude<AlertModeValue, "both">;

interface PendingCopyInput {
  action: PendingCopyInputAction;
  expiresAt: number;
}

interface PendingWalletInput {
  action: PendingWalletInputAction;
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

function alertEnabled(mode: AlertModeValue | null, alertType: ToggleAlertType): boolean {
  return mode === "both" || mode === alertType;
}

function formatTrailingSellDashboardStatus(config: LegacyBotConfig): string[] {
  if (!config.copyTradeTrailingSellEnabled) {
    return ["<b>Trailing sells:</b> Off"];
  }

  const holdMs = config.copyTradeTrailingSellHoldMs || 2000;
  const intervalMs = config.copyTradeTrailingSellIntervalMs || 2000;
  const firstPercent = config.copyTradeTrailingSellFirstPercent || 20;
  const trailPercent = config.copyTradeTrailingSellTrailPercent || 20;
  const maxBuilds = Math.max(1, Math.floor(config.copyTradeTrailingSellMaxBuilds || 5));
  const percents = maxBuilds <= 1
    ? [100]
    : [firstPercent, ...Array.from({ length: Math.max(0, maxBuilds - 2) }, () => trailPercent), 100];

  return [
    "<b>Trailing sells:</b> On",
    `<b>Trailing schedule:</b> ${percents
      .map((percent, index) => `${percent}% after ${(holdMs + intervalMs * index) / 1000}s`)
      .join(" | ")}`
  ];
}

export function toggleAlertMode(currentMode: AlertModeValue | null, alertType: ToggleAlertType): AlertModeValue | null {
  const migrationsEnabled = alertType === "migrations" ? !alertEnabled(currentMode, "migrations") : alertEnabled(currentMode, "migrations");
  const newtokensEnabled = alertType === "newtokens" ? !alertEnabled(currentMode, "newtokens") : alertEnabled(currentMode, "newtokens");

  if (migrationsEnabled && newtokensEnabled) {
    return "both";
  }

  if (migrationsEnabled) {
    return "migrations";
  }

  if (newtokensEnabled) {
    return "newtokens";
  }

  return null;
}

function chooseModeText(): string {
  return [
    "Open a dashboard:",
    "/alerts - Toggle token alerts",
    "/trackwallets - Manage tracked wallets",
    "/mywallets - Manage trading wallet",
    "/copytrade - Manage copy trade settings",
    "/help - Show all commands"
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
    "/alerts - Open alert mode dashboard",
    "/trackwallets - Open track wallet dashboard",
    "/mywallets - Open trading wallet dashboard",
    "/copytrade - Open copy trade setup menu",
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
  const pendingWalletInputs = new Map<string, PendingWalletInput>();

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
      const pendingWalletResponse = await handlePendingWalletInput(chatId, message);

      if (pendingWalletResponse) {
        await reply(chatId, pendingWalletResponse.text, pendingWalletResponse.replyMarkup);
        return;
      }

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
      case "/alerts":
        {
          const dashboard = alertDashboard(chatId);
          await reply(chatId, dashboard.text, dashboard.replyMarkup);
        }
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
      case "/trackwallets":
        {
          const dashboard = trackWalletDashboard(chatId);
          await reply(chatId, dashboard.text, dashboard.replyMarkup);
        }
        break;
      case "/mywallets":
        {
          const dashboard = myWalletDashboard(chatId);
          await reply(chatId, dashboard.text, dashboard.replyMarkup);
        }
        break;
      case "/copytrade": {
        const dashboard = copyTradeDashboard(chatId);
        await reply(chatId, dashboard.text, dashboard.replyMarkup);
        break;
      }
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

    if (data === "alerts:dashboard") {
      const dashboard = alertDashboard(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data.startsWith("alerts:toggle:")) {
      const alertType = data.slice("alerts:toggle:".length) as ToggleAlertType;

      if (!isToggleAlertType(alertType)) {
        await reply(chatId, "That alert toggle is no longer available. Send /alerts to reopen the menu.");
        return;
      }

      const nextMode = toggleAlertMode(subscribers?.get(chatId)?.mode || null, alertType);
      const updated = await subscribers?.setMode(chatId, nextMode);

      if (!updated) {
        await reply(chatId, verificationPrompt());
        return;
      }

      const dashboard = alertDashboard(chatId);
      await reply(chatId, `${formatAlertStatusLine(nextMode)}\n\n${dashboard.text}`, dashboard.replyMarkup);
      return;
    }

    if (data === "copytrade:status" || data === "copytrade:dashboard") {
      const dashboard = copyTradeDashboard(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "trackwallets:dashboard") {
      const dashboard = trackWalletDashboard(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "trackwallets:list") {
      await reply(chatId, listTrackWallets(chatId));
      return;
    }

    if (data === "trackwallets:add") {
      setPendingWalletInput(chatId, "watch_wallet");
      await reply(chatId, "Send the wallet address you want to track. You can include a nickname after it.");
      return;
    }

    if (data === "trackwallets:rename") {
      setPendingWalletInput(chatId, "rename_wallet");
      await reply(chatId, "Send <code>wallet-address nickname</code> to rename a tracked wallet, or <code>wallet-address -</code> to clear.");
      return;
    }

    if (data === "trackwallets:remove") {
      setPendingWalletInput(chatId, "unwatch_wallet");
      await reply(chatId, "Send the wallet address you want to stop tracking.");
      return;
    }

    if (data === "mywallets:dashboard" || data === "copytrade:mywallets") {
      const dashboard = myWalletDashboard(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "mywallets:create") {
      await createTradingWallet(chatId);
      return;
    }

    if (data === "copytrade:set_amount") {
      setPendingCopyInput(chatId, "copy_amount");
      await reply(chatId, "Send the fixed copy size in SOL, for example <code>0.1</code>.");
      return;
    }

    if (data === "copytrade:choose_target" || data === "copytrade:target:add") {
      setPendingWalletInput(chatId, "copytrade_wallet");
      await reply(chatId, "Send the wallet address you want to copytrade. You can include a nickname after it.");
      return;
    }

    if (data === "copytrade:target:clear") {
      await reply(chatId, "Copytrade Wallets are now managed individually. Send /copytrade and tap Remove Copytrade.");
      return;
    }

    if (data === "copytrade:wallets") {
      await reply(chatId, listCopyTradeWallets(chatId));
      return;
    }

    if (data === "copytrade:add_wallet") {
      setPendingWalletInput(chatId, "copytrade_wallet");
      await reply(chatId, "Send the wallet address you want to copytrade. You can include a nickname after it.");
      return;
    }

    if (data === "copytrade:rename_wallet") {
      setPendingWalletInput(chatId, "rename_copytrade_wallet");
      await reply(chatId, "Send <code>wallet-address nickname</code> to rename a Copytrade Wallet, or <code>wallet-address -</code> to clear.");
      return;
    }

    if (data === "copytrade:remove_trade_wallet") {
      setPendingWalletInput(chatId, "remove_copytrade_wallet");
      await reply(chatId, "Send the Copytrade Wallet address you want to remove.");
      return;
    }

    await reply(chatId, "That copy trade action is no longer available. Send /copytrade to reopen the menu.");
  }

  function isToggleAlertType(value: string): value is ToggleAlertType {
    return value === "migrations" || value === "newtokens";
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

  function alertDashboard(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const gate = requireVerified(chatId);

    if (gate) {
      return {
        text: gate,
        replyMarkup: alertDashboardReplyMarkup()
      };
    }

    const mode = subscribers?.get(chatId)?.mode || null;
    const text = [
      "<b>Alerts</b>",
      `<b>Migrated coins:</b> ${alertEnabled(mode, "migrations") ? "On" : "Off"}`,
      `<b>New tokens:</b> ${alertEnabled(mode, "newtokens") ? "On" : "Off"}`,
      `<b>Status:</b> ${mode ? "Token alerts active" : "Token alerts paused"}`,
      "",
      "Tap a button to turn that alert type on or off."
    ].join("\n");

    return {
      text,
      replyMarkup: alertDashboardReplyMarkup(mode)
    };
  }

  function alertDashboardReplyMarkup(mode: AlertModeValue | null = null): TelegramReplyMarkup {
    return {
      inline_keyboard: [
        [
          {
            text: `${alertEnabled(mode, "migrations") ? "🟢 ON" : "⚪ OFF"} - Migrated Coins`,
            callback_data: "alerts:toggle:migrations"
          }
        ],
        [
          {
            text: `${alertEnabled(mode, "newtokens") ? "🟢 ON" : "⚪ OFF"} - New Tokens`,
            callback_data: "alerts:toggle:newtokens"
          }
        ]
      ]
    };
  }

  function formatAlertStatusLine(mode: AlertModeValue | null): string {
    return mode ? `<b>Now watching:</b> ${modeLabel(mode)}` : "<b>Token alerts paused.</b>";
  }

  async function watchWallet(chatId: TelegramChatId, args: string[]): Promise<string> {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallet = args[0]?.trim();
    const nickname = args.slice(1).join(" ").trim();

    if (!wallet) {
      return "Send /trackwallets to open the track wallet dashboard.";
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
      return "Send /trackwallets to open the track wallet dashboard and rename a tracked wallet.";
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
      return "Send /trackwallets to open the track wallet dashboard and remove a tracked wallet.";
    }

    if (!isValidSolanaAddress(wallet)) {
      return "That does not look like a Solana wallet address.";
    }

    const removed = await subscribers?.unwatchWallet(chatId, wallet);

    if (!removed) {
      return "That wallet was not being tracked in this chat.";
    }

    const syncWarning = await onWalletWatchlistChange?.();
    const success = `<b>Stopped tracking wallet:</b>\n<code>${wallet}</code>`;
    return syncWarning ? `${success}\n\n${syncWarning}` : success;
  }

  function listTrackWallets(chatId: TelegramChatId): string {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallets = subscribers?.listWatchedWallets(chatId) || [];

    if (wallets.length === 0) {
      return "No tracked wallets for this chat. Send /trackwallets and tap Add Wallet.";
    }

    return [
      "<b>Tracked wallets</b>",
      ...wallets.map((wallet) =>
        wallet.label ? `${escapeWalletLabel(wallet.label)}\n<code>${wallet.address}</code>` : `<code>${wallet.address}</code>`
      )
    ].join("\n\n");
  }

  function setPendingWalletInput(chatId: TelegramChatId, action: PendingWalletInputAction): void {
    pendingWalletInputs.set(String(chatId), {
      action,
      expiresAt: Date.now() + PENDING_COPY_INPUT_TTL_MS
    });
  }

  async function handlePendingWalletInput(
    chatId: TelegramChatId,
    message: TelegramMessage
  ): Promise<{ text: string; replyMarkup?: TelegramReplyMarkup } | null> {
    const pending = pendingWalletInputs.get(String(chatId));

    if (!pending) {
      return null;
    }

    if (pending.expiresAt < Date.now()) {
      pendingWalletInputs.delete(String(chatId));
      return {
        text: "That wallet setup step expired. Send /trackwallets, /mywallets, or /copytrade to start again."
      };
    }

    const value = message.text?.trim() || "";
    const [wallet, ...rest] = value.split(/\s+/);
    const label = rest.join(" ").trim();

    if (!wallet || !isValidSolanaAddress(wallet)) {
      return {
        text: "That does not look like a Solana wallet address. Send a wallet address, or send /trackwallets, /mywallets, or /copytrade to restart."
      };
    }

    if (pending.action === "watch_wallet") {
      const nicknameError = validateNickname(label);

      if (nicknameError) {
        return { text: nicknameError };
      }

      pendingWalletInputs.delete(String(chatId));
      const updated = await subscribers?.watchWallet(chatId, wallet, label);

      if (!updated) {
        return { text: verificationPrompt() };
      }

      const syncWarning = await onWalletWatchlistChange?.();
      const dashboard = trackWalletDashboard(chatId);
      return {
        text: `${label ? `<b>Tracking wallet:</b> ${escapeWalletLabel(label)}\n` : "<b>Tracking wallet:</b>\n"}<code>${wallet}</code>${syncWarning ? `\n\n${syncWarning}` : ""}\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    if (pending.action === "copytrade_wallet") {
      const nicknameError = validateNickname(label);

      if (nicknameError) {
        return { text: nicknameError };
      }

      pendingWalletInputs.delete(String(chatId));
      const updated = await subscribers?.watchCopyTradeWallet(chatId, wallet, label);

      if (!updated) {
        return { text: verificationPrompt() };
      }

      const syncWarning = await onWalletWatchlistChange?.();
      const dashboard = copyTradeDashboard(chatId);
      return {
        text: `${label ? `<b>Copytrade wallet saved:</b> ${escapeWalletLabel(label)}\n` : "<b>Copytrade wallet saved:</b>\n"}<code>${wallet}</code>${syncWarning ? `\n\n${syncWarning}` : ""}\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    if (pending.action === "rename_copytrade_wallet") {
      if (!label) {
        return {
          text: "Send <code>wallet-address nickname</code> to rename a Copytrade Wallet, or <code>wallet-address -</code> to clear."
        };
      }

      const nextLabel = label === "-" ? null : label;
      const nicknameError = nextLabel === null ? null : validateNickname(nextLabel);

      if (nicknameError) {
        return { text: nicknameError };
      }

      pendingWalletInputs.delete(String(chatId));
      const updated = await subscribers?.renameCopyTradeWallet(chatId, wallet, nextLabel);

      if (!updated) {
        return { text: "That Copytrade Wallet is not configured in this chat." };
      }

      const dashboard = copyTradeDashboard(chatId);
      return {
        text: `${nextLabel === null ? "<b>Cleared Copytrade Wallet nickname:</b>" : `<b>Renamed Copytrade Wallet:</b> ${escapeWalletLabel(nextLabel)}`}\n<code>${wallet}</code>\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    if (pending.action === "remove_copytrade_wallet") {
      pendingWalletInputs.delete(String(chatId));
      const removed = await subscribers?.unwatchCopyTradeWallet(chatId, wallet);

      if (!removed) {
        return { text: "That Copytrade Wallet is not configured in this chat." };
      }

      const syncWarning = await onWalletWatchlistChange?.();
      const dashboard = copyTradeDashboard(chatId);
      return {
        text: `<b>Removed Copytrade Wallet:</b>\n<code>${wallet}</code>${syncWarning ? `\n\n${syncWarning}` : ""}\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    if (pending.action === "rename_wallet") {
      if (!label) {
        return {
          text: "Send <code>wallet-address nickname</code> to rename, or <code>wallet-address -</code> to clear."
        };
      }

      const nextLabel = label === "-" ? null : label;
      const nicknameError = nextLabel === null ? null : validateNickname(nextLabel);

      if (nicknameError) {
        return { text: nicknameError };
      }

      pendingWalletInputs.delete(String(chatId));
      const updated = await subscribers?.renameWallet(chatId, wallet, nextLabel);

      if (!updated) {
        return { text: "That wallet is not being watched in this chat." };
      }

      const dashboard = trackWalletDashboard(chatId);
      return {
        text: `${nextLabel === null ? "<b>Cleared wallet nickname:</b>" : `<b>Renamed wallet:</b> ${escapeWalletLabel(nextLabel)}`}\n<code>${wallet}</code>\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    pendingWalletInputs.delete(String(chatId));
    const removed = await subscribers?.unwatchWallet(chatId, wallet);

    if (!removed) {
      return { text: "That wallet was not being watched in this chat." };
    }

    const syncWarning = await onWalletWatchlistChange?.();
    const dashboard = trackWalletDashboard(chatId);
    return {
      text: `<b>Stopped tracking wallet:</b>\n<code>${wallet}</code>${syncWarning ? `\n\n${syncWarning}` : ""}\n\n${dashboard.text}`,
      replyMarkup: dashboard.replyMarkup
    };
  }

  function trackWalletDashboard(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const gate = requireVerified(chatId);

    if (gate) {
      return {
        text: gate,
        replyMarkup: trackWalletDashboardReplyMarkup()
      };
    }

    const wallets = subscribers?.listWatchedWallets(chatId) || [];
    const text = [
      "<b>Wallets</b>",
      `<b>Tracked wallets:</b> ${wallets.length}`,
      wallets.length === 0 ? "No tracked wallets yet." : wallets.map((wallet) => formatWalletSummary(wallet)).join("\n"),
      "",
      "Use the buttons below to manage tracked wallets."
    ].join("\n");

    return {
      text,
      replyMarkup: trackWalletDashboardReplyMarkup()
    };
  }

  function trackWalletDashboardReplyMarkup(): TelegramReplyMarkup {
    return {
      inline_keyboard: [
        [
          { text: "➕ Add Wallet", callback_data: "trackwallets:add" },
          { text: "✏️ Rename", callback_data: "trackwallets:rename" }
        ],
        [
          { text: "🗑️ Remove", callback_data: "trackwallets:remove" },
          { text: "📋 List", callback_data: "trackwallets:list" }
        ]
      ]
    };
  }

  function myWalletDashboard(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const gate = requireVerified(chatId);

    if (gate) {
      return {
        text: gate,
        replyMarkup: myWalletDashboardReplyMarkup(null)
      };
    }

    const tradingWallet = subscribers?.getTradingWallet(chatId) || null;
    const text = [
      "<b>Trading wallet</b>",
      tradingWallet
        ? `<b>Deposit address</b>\n<code>${tradingWallet.publicKey}</code>`
        : "No trading wallet yet.",
      tradingWallet ? `<b>API key:</b> saved ending in <code>${tradingWallet.apiKeyLast4}</code>` : "",
      tradingWallet ? "<b>Private key:</b> shown once when created. The bot cannot recover it." : "",
      "",
      tradingWallet
        ? "Deposit SOL here to enable auto copy buys."
        : "Tap below to create one."
    ]
      .filter(Boolean)
      .join("\n");

    return {
      text,
      replyMarkup: myWalletDashboardReplyMarkup(tradingWallet?.publicKey || null)
    };
  }

  function myWalletDashboardReplyMarkup(publicKey: string | null): TelegramReplyMarkup {
    if (!publicKey) {
      return {
        inline_keyboard: [[{ text: "🌸 Create Wallet", callback_data: "mywallets:create" }]]
      };
    }

    return {
      inline_keyboard: [
        [
          {
            text: "📋 Copy Address",
            copy_text: {
              text: publicKey
            }
          },
          { text: "📊 Status", callback_data: "mywallets:dashboard" }
        ]
      ]
    };
  }

  async function createTradingWallet(chatId: TelegramChatId): Promise<void> {
    const existing = subscribers?.getTradingWallet(chatId);

    if (existing) {
      const dashboard = myWalletDashboard(chatId);
      await reply(chatId, `<b>Trading wallet already exists.</b>\n\n${dashboard.text}`, dashboard.replyMarkup);
      return;
    }

    if (!encryptionSecretReady(config.pumpPortalWalletKeyEncryptionSecret)) {
      await reply(chatId, "Trading wallet creation is not configured yet. Missing <code>PUMPPORTAL_WALLET_KEY_ENCRYPTION_SECRET</code>.");
      return;
    }

    const result = await createPumpPortalLightningWallet({
      url: config.pumpPortalCreateWalletUrl || "https://pumpportal.fun/api/create-wallet"
    });

    if (!result.ok) {
      await reply(chatId, `Could not create trading wallet: ${escapeWalletLabel(result.errorText)}`);
      return;
    }

    const now = new Date().toISOString();
    const saved = await subscribers?.setTradingWallet(chatId, {
      publicKey: result.wallet.publicKey,
      encryptedApiKey: encryptSecret(result.wallet.apiKey, config.pumpPortalWalletKeyEncryptionSecret || ""),
      apiKeyLast4: last4(result.wallet.apiKey),
      createdAt: now,
      updatedAt: now
    });

    if (!saved) {
      await reply(chatId, verificationPrompt());
      return;
    }

    await reply(
      chatId,
      [
        "<b>Trading wallet created.</b>",
        "",
        "<b>Deposit SOL here:</b>",
        `<code>${result.wallet.publicKey}</code>`,
        "",
        "<b>Save this private key now.</b>",
        "The bot does not store it and cannot show it again.",
        "",
        "<b>Private key</b>",
        `<code>${escapeWalletLabel(result.wallet.privateKey)}</code>`,
        "",
        "Auto copy buys can use this wallet after you deposit SOL and set /copytrade."
      ].join("\n"),
      myWalletDashboardReplyMarkup(result.wallet.publicKey)
    );
  }

  function formatWalletSummary(wallet: WatchedWallet): string {
    return wallet.label ? `${escapeWalletLabel(wallet.label)} - <code>${wallet.address}</code>` : `<code>${wallet.address}</code>`;
  }

  function last4(value: string): string {
    return value.length <= 4 ? value : value.slice(-4);
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

  function copyTradeDashboard(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const gate = requireVerified(chatId);

    if (gate) {
      return {
        text: gate,
        replyMarkup: copyTradeDashboardReplyMarkup()
      };
    }

    const subscriber = subscribers?.get(chatId) || null;
    const copyTradeWallets = subscribers?.listCopyTradeWallets(chatId) || [];
    const tradingWallet = subscribers?.getTradingWallet(chatId) || null;
    const ready = Boolean(tradingWallet && subscriber?.copyAmountSol && copyTradeWallets.length > 0);
    const text = [
      "<b>Copy trade</b>",
      `<b>Trading wallet:</b> ${tradingWallet ? "Created" : "Missing"}`,
      tradingWallet ? `<code>${tradingWallet.publicKey}</code>` : "Create one in /mywallets.",
      `<b>Copy amount:</b> ${subscriber?.copyAmountSol ? `${formatSolAmount(subscriber.copyAmountSol)} SOL` : "Not set"}`,
      `<b>Copytrade wallets:</b> ${copyTradeWallets.length}`,
      copyTradeWallets.length === 0 ? "No Copytrade Wallets yet." : copyTradeWallets.map((wallet) => formatWalletSummary(wallet)).join("\n"),
      `<b>Auto buys:</b> ${ready ? "Ready" : "Not ready"}`,
      ...formatTrailingSellDashboardStatus(config),
      "",
      ready ? "Auto copy buys are enabled for matching SOL-to-token buys." : "Create a trading wallet, set amount, and add Copytrade Wallets to enable auto buys."
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
          { text: "📊 Status", callback_data: "copytrade:status" },
          { text: "👛 Wallet", callback_data: "copytrade:mywallets" }
        ],
        [
          { text: "💰 Amount", callback_data: "copytrade:set_amount" },
          { text: "➕ Add Wallet", callback_data: "copytrade:add_wallet" }
        ],
        [
          { text: "✏️ Rename", callback_data: "copytrade:rename_wallet" },
          { text: "🗑️ Remove", callback_data: "copytrade:remove_trade_wallet" }
        ],
        [{ text: "📋 List Wallets", callback_data: "copytrade:wallets" }]
      ]
    };
  }

  function listCopyTradeWallets(chatId: TelegramChatId): string {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallets = subscribers?.listCopyTradeWallets(chatId) || [];

    if (wallets.length === 0) {
      return "No Copytrade Wallets for this chat. Send /copytrade and tap Add Copytrade Wallet.";
    }

    return [
      "<b>Copytrade Wallets</b>",
      ...wallets.map((wallet) =>
        wallet.label ? `${escapeWalletLabel(wallet.label)}\n<code>${wallet.address}</code>` : `<code>${wallet.address}</code>`
      )
    ].join("\n\n");
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
      console.log("Polling for /start, /help, /verify, /stop, /alerts, /trackwallets, /mywallets, and /copytrade");

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
