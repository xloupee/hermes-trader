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
  { command: "alerts", description: "Open alert mode dashboard" },
  { command: "wallets", description: "Open wallet dashboard" },
  { command: "copytrade", description: "Open copy trade menu" },
  { command: "help", description: "Show commands" }
];

const MAX_WALLET_NICKNAME_LENGTH = 48;
const PENDING_COPY_INPUT_TTL_MS = 10 * 60 * 1000;

type PendingCopyInputAction = "copy_wallet" | "remove_copy_wallet" | "copy_amount";
type PendingWalletInputAction = "watch_wallet" | "rename_wallet" | "unwatch_wallet" | "copy_target_wallet";
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
    "/wallets - Manage watched wallets",
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
    "/wallets - Open wallet dashboard",
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
      case "/wallets":
        {
          const dashboard = walletDashboard(chatId);
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

    if (data === "wallets:dashboard") {
      const dashboard = walletDashboard(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "wallets:list") {
      await reply(chatId, listWallets(chatId));
      return;
    }

    if (data === "wallets:add") {
      setPendingWalletInput(chatId, "watch_wallet");
      await reply(chatId, "Send the wallet address you want to watch. You can include a nickname after it.");
      return;
    }

    if (data === "wallets:rename") {
      setPendingWalletInput(chatId, "rename_wallet");
      await reply(chatId, "Send <code>wallet-address nickname</code> to rename, or <code>wallet-address -</code> to clear.");
      return;
    }

    if (data === "wallets:remove") {
      setPendingWalletInput(chatId, "unwatch_wallet");
      await reply(chatId, "Send the wallet address you want to stop watching.");
      return;
    }

    if (data === "copytrade:set_wallet") {
      setPendingCopyInput(chatId, "copy_wallet");
      await reply(chatId, "Send the public wallet address you want to add as a copy wallet.");
      return;
    }

    if (data === "copytrade:remove_wallet") {
      setPendingCopyInput(chatId, "remove_copy_wallet");
      await reply(chatId, "Send the public copy wallet address you want to remove.");
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
      const dashboard = walletDashboard(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "copytrade:target:add") {
      setPendingWalletInput(chatId, "copy_target_wallet");
      await reply(chatId, "Send the wallet address you want to copytrade. You can include a nickname after it.");
      return;
    }

    if (data === "copytrade:target:clear") {
      const updated = await subscribers?.setCopyTargetWallet(chatId, null);

      if (!updated) {
        await reply(chatId, verificationPrompt());
        return;
      }

      const dashboard = copyTradeDashboard(chatId);
      await reply(chatId, `<b>Copytrade wallet cleared.</b>\n\n${dashboard.text}`, dashboard.replyMarkup);
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
      await reply(chatId, `<b>Copytrade wallet saved.</b>\n\n${dashboard.text}`, dashboard.replyMarkup);
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
      return "Send /wallets to open the wallet dashboard.";
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
      return "Send /wallets to open the wallet dashboard and rename a watched wallet.";
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
      return "Send /wallets to open the wallet dashboard and remove a watched wallet.";
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
      return "No watched wallets for this chat. Send /wallets and tap Add Wallet.";
    }

    return [
      "<b>Watched wallets</b>",
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
        text: "That wallet setup step expired. Send /wallets to start again."
      };
    }

    const value = message.text?.trim() || "";
    const [wallet, ...rest] = value.split(/\s+/);
    const label = rest.join(" ").trim();

    if (!wallet || !isValidSolanaAddress(wallet)) {
      return {
        text: "That does not look like a Solana wallet address. Send a wallet address, or send /wallets to restart."
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
      const dashboard = walletDashboard(chatId);
      return {
        text: `${label ? `<b>Watching wallet:</b> ${escapeWalletLabel(label)}\n` : "<b>Watching wallet:</b>\n"}<code>${wallet}</code>${syncWarning ? `\n\n${syncWarning}` : ""}\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    if (pending.action === "copy_target_wallet") {
      const nicknameError = validateNickname(label);

      if (nicknameError) {
        return { text: nicknameError };
      }

      pendingWalletInputs.delete(String(chatId));
      const updated = await subscribers?.watchWallet(chatId, wallet, label);

      if (!updated) {
        return { text: verificationPrompt() };
      }

      const targetUpdated = await subscribers?.setCopyTargetWallet(chatId, wallet);

      if (!targetUpdated) {
        return { text: "That wallet could not be saved as your copytrade wallet." };
      }

      const syncWarning = await onWalletWatchlistChange?.();
      const dashboard = copyTradeDashboard(chatId);
      return {
        text: `${label ? `<b>Copytrade wallet saved:</b> ${escapeWalletLabel(label)}\n` : "<b>Copytrade wallet saved:</b>\n"}<code>${wallet}</code>${syncWarning ? `\n\n${syncWarning}` : ""}\n\n${dashboard.text}`,
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

      const dashboard = walletDashboard(chatId);
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
    const dashboard = walletDashboard(chatId);
    return {
      text: `<b>Stopped watching wallet:</b>\n<code>${wallet}</code>${syncWarning ? `\n\n${syncWarning}` : ""}\n\n${dashboard.text}`,
      replyMarkup: dashboard.replyMarkup
    };
  }

  function walletDashboard(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const gate = requireVerified(chatId);

    if (gate) {
      return {
        text: gate,
        replyMarkup: walletDashboardReplyMarkup()
      };
    }

    const wallets = subscribers?.listWatchedWallets(chatId) || [];
    const text = [
      "<b>Wallets</b>",
      `<b>Watched wallets:</b> ${wallets.length}`,
      wallets.length === 0 ? "No watched wallets yet." : wallets.map((wallet) => formatWalletSummary(wallet)).join("\n"),
      "",
      "Use the buttons below to manage watched wallets."
    ].join("\n");

    return {
      text,
      replyMarkup: walletDashboardReplyMarkup()
    };
  }

  function walletDashboardReplyMarkup(): TelegramReplyMarkup {
    return {
      inline_keyboard: [
        [
          { text: "Add Wallet", callback_data: "wallets:add" },
          { text: "Rename", callback_data: "wallets:rename" }
        ],
        [
          { text: "Remove", callback_data: "wallets:remove" },
          { text: "List", callback_data: "wallets:list" }
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
        text: `<b>Copy wallet added:</b>\n<code>${value}</code>\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    if (pending.action === "remove_copy_wallet") {
      if (!isValidSolanaAddress(value)) {
        return {
          text: "That does not look like a Solana wallet address. Send a public wallet address, or send /copytrade to restart."
        };
      }

      pendingCopyInputs.delete(String(chatId));
      const removed = await subscribers?.removeCopyWallet(chatId, value);

      if (!removed) {
        return {
          text: "That copy wallet was not configured for this chat. Send /copytrade to reopen the menu."
        };
      }

      const dashboard = copyTradeDashboard(chatId);
      return {
        text: `<b>Copy wallet removed:</b>\n<code>${value}</code>\n\n${dashboard.text}`,
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
    const copyWallets = subscribers?.listCopyWallets(chatId) || [];
    const text = [
      "<b>Copy trade</b>",
      `<b>Copy wallets:</b> ${copyWallets.length}`,
      ...copyWallets.map((wallet) => `<code>${wallet}</code>`),
      `<b>Copy amount:</b> ${subscriber?.copyAmountSol ? `${formatSolAmount(subscriber.copyAmountSol)} SOL` : "Not set"}`,
      `<b>Copytrade wallet:</b> ${formatCopyTarget(subscriber)}`,
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
          { text: "Add Wallet", callback_data: "copytrade:set_wallet" }
        ],
        [{ text: "Remove Wallet", callback_data: "copytrade:remove_wallet" }],
        [
          { text: "Set Amount", callback_data: "copytrade:set_amount" },
          { text: "Copytrade Wallet", callback_data: "copytrade:choose_target" }
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
        text: "<b>Copytrade wallet</b>\n\nNo watched wallets yet. Tap Set Copytrade Wallet and send the wallet you want to copy.",
        replyMarkup: {
          inline_keyboard: [
            [{ text: "Set Copytrade Wallet", callback_data: "copytrade:target:add" }],
            [{ text: "Back", callback_data: "copytrade:dashboard" }]
          ]
        }
      };
    }

    return {
      text: "<b>Copytrade wallet</b>\n\nPick an existing watched wallet, or set a new one directly.",
      replyMarkup: {
        inline_keyboard: [
          ...wallets.map((wallet) => [
            {
              text: walletButtonLabel(wallet),
              callback_data: `copytrade:target:${wallet.address}`
            }
          ]),
          [{ text: "Set Copytrade Wallet", callback_data: "copytrade:target:add" }],
          [{ text: "Clear Copytrade Wallet", callback_data: "copytrade:target:clear" }],
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
      console.log("Polling for /start, /help, /verify, /stop, /alerts, /wallets, and /copytrade");

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
