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
  | "my_wallet"
  | "rename_my_wallet"
  | "remove_my_wallet"
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
    "/mywallets - Manage my wallets",
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
    "/mywallets - Open my wallets dashboard",
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

    if (data === "mywallets:list") {
      await reply(chatId, listMyWallets(chatId));
      return;
    }

    if (data === "mywallets:add") {
      setPendingWalletInput(chatId, "my_wallet");
      await reply(chatId, "Send your public wallet address. You can include a nickname after it.");
      return;
    }

    if (data === "mywallets:rename") {
      setPendingWalletInput(chatId, "rename_my_wallet");
      await reply(chatId, "Send <code>wallet-address nickname</code> to rename a My Wallet, or <code>wallet-address -</code> to clear.");
      return;
    }

    if (data === "mywallets:remove") {
      setPendingWalletInput(chatId, "remove_my_wallet");
      await reply(chatId, "Send the My Wallet address you want to remove.");
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
          { text: `${alertEnabled(mode, "migrations") ? "ON" : "OFF"} - Migrated Coins`, callback_data: "alerts:toggle:migrations" }
        ],
        [{ text: `${alertEnabled(mode, "newtokens") ? "ON" : "OFF"} - New Tokens`, callback_data: "alerts:toggle:newtokens" }]
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

    if (pending.action === "my_wallet") {
      const nicknameError = validateNickname(label);

      if (nicknameError) {
        return { text: nicknameError };
      }

      pendingWalletInputs.delete(String(chatId));
      const updated = await subscribers?.addMyWallet(chatId, wallet, label);

      if (!updated) {
        return { text: verificationPrompt() };
      }

      const dashboard = myWalletDashboard(chatId);
      return {
        text: `${label ? `<b>My Wallet saved:</b> ${escapeWalletLabel(label)}\n` : "<b>My Wallet saved:</b>\n"}<code>${wallet}</code>\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    if (pending.action === "rename_my_wallet") {
      if (!label) {
        return {
          text: "Send <code>wallet-address nickname</code> to rename a My Wallet, or <code>wallet-address -</code> to clear."
        };
      }

      const nextLabel = label === "-" ? null : label;
      const nicknameError = nextLabel === null ? null : validateNickname(nextLabel);

      if (nicknameError) {
        return { text: nicknameError };
      }

      pendingWalletInputs.delete(String(chatId));
      const updated = await subscribers?.renameMyWallet(chatId, wallet, nextLabel);

      if (!updated) {
        return { text: "That My Wallet is not configured in this chat." };
      }

      const dashboard = myWalletDashboard(chatId);
      return {
        text: `${nextLabel === null ? "<b>Cleared My Wallet nickname:</b>" : `<b>Renamed My Wallet:</b> ${escapeWalletLabel(nextLabel)}`}\n<code>${wallet}</code>\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    if (pending.action === "remove_my_wallet") {
      pendingWalletInputs.delete(String(chatId));
      const removed = await subscribers?.removeMyWallet(chatId, wallet);

      if (!removed) {
        return { text: "That My Wallet is not configured in this chat." };
      }

      const dashboard = myWalletDashboard(chatId);
      return {
        text: `<b>Removed My Wallet:</b>\n<code>${wallet}</code>\n\n${dashboard.text}`,
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
          { text: "Add Wallet", callback_data: "trackwallets:add" },
          { text: "Rename", callback_data: "trackwallets:rename" }
        ],
        [
          { text: "Remove", callback_data: "trackwallets:remove" },
          { text: "List", callback_data: "trackwallets:list" }
        ]
      ]
    };
  }

  function myWalletDashboard(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const gate = requireVerified(chatId);

    if (gate) {
      return {
        text: gate,
        replyMarkup: myWalletDashboardReplyMarkup()
      };
    }

    const wallets = subscribers?.listMyWallets(chatId) || [];
    const text = [
      "<b>My wallets</b>",
      `<b>My wallets:</b> ${wallets.length}`,
      wallets.length === 0 ? "No My Wallets yet." : wallets.map((wallet) => formatWalletSummary(wallet)).join("\n"),
      "",
      "Use the buttons below to manage your public wallets."
    ].join("\n");

    return {
      text,
      replyMarkup: myWalletDashboardReplyMarkup()
    };
  }

  function myWalletDashboardReplyMarkup(): TelegramReplyMarkup {
    return {
      inline_keyboard: [
        [
          { text: "Add Wallet", callback_data: "mywallets:add" },
          { text: "Rename", callback_data: "mywallets:rename" }
        ],
        [
          { text: "Remove", callback_data: "mywallets:remove" },
          { text: "List", callback_data: "mywallets:list" }
        ]
      ]
    };
  }

  function formatWalletSummary(wallet: WatchedWallet): string {
    return wallet.label ? `${escapeWalletLabel(wallet.label)} - <code>${wallet.address}</code>` : `<code>${wallet.address}</code>`;
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
    const myWallets = subscribers?.listMyWallets(chatId) || [];
    const text = [
      "<b>Copy trade</b>",
      `<b>My wallets:</b> ${myWallets.length}`,
      myWallets.length === 0 ? "No My Wallets yet. Add one in /mywallets before copytrade simulations can run." : myWallets.map((wallet) => formatWalletSummary(wallet)).join("\n"),
      `<b>Copy amount:</b> ${subscriber?.copyAmountSol ? `${formatSolAmount(subscriber.copyAmountSol)} SOL` : "Not set"}`,
      `<b>Copytrade wallets:</b> ${copyTradeWallets.length}`,
      copyTradeWallets.length === 0 ? "No Copytrade Wallets yet." : copyTradeWallets.map((wallet) => formatWalletSummary(wallet)).join("\n"),
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
          { text: "My Wallets", callback_data: "copytrade:mywallets" }
        ],
        [
          { text: "Set Amount", callback_data: "copytrade:set_amount" },
          { text: "Add Copytrade Wallet", callback_data: "copytrade:add_wallet" }
        ],
        [
          { text: "Rename Copytrade", callback_data: "copytrade:rename_wallet" },
          { text: "Remove Copytrade", callback_data: "copytrade:remove_trade_wallet" }
        ],
        [{ text: "List Copytrade Wallets", callback_data: "copytrade:wallets" }]
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

  function listMyWallets(chatId: TelegramChatId): string {
    const gate = requireVerified(chatId);

    if (gate) {
      return gate;
    }

    const wallets = subscribers?.listMyWallets(chatId) || [];

    if (wallets.length === 0) {
      return "No My Wallets for this chat. Send /mywallets and tap Add Wallet.";
    }

    return [
      "<b>My Wallets</b>",
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
