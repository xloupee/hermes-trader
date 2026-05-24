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
  SubscriberRecord,
  SubscriberStore,
  TelegramCallbackQuery,
  TelegramChatId,
  TelegramMessage,
  TelegramReplyMarkup,
  TelegramUpdate,
  TradingWallet,
  TrailingSellConfig,
  TrailingSellPercentBasis,
  TrailingSellStep,
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
const MAX_TRAILING_SELL_STEPS = 20;
const DEFAULT_COPY_TRADE_SLIPPAGE = 10;
const DEFAULT_COPY_TRADE_PRIORITY_FEE = 0.00005;

type PendingCopyInputAction =
  | "copy_amount"
  | "rename_trading_wallet"
  | "buy_slippage"
  | "buy_priority_fee"
  | "sell_slippage"
  | "sell_priority_fee"
  | "trailing_step"
  | "trailing_steps"
  | "trailing_formula";
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
  walletAddress?: string;
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

function escapeHtml(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

function formatSolAmount(value: number): string {
  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits: 9
  }).format(value);
}

function formatPercent(value: number): string {
  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits: 4
  }).format(value);
}

function effectiveBuySlippage(subscriber: SubscriberRecord | null | undefined, config: LegacyBotConfig): number {
  return subscriber?.copyTradeBuySlippagePercent ?? config.copyTradeSlippage ?? DEFAULT_COPY_TRADE_SLIPPAGE;
}

function effectiveBuyPriorityFee(subscriber: SubscriberRecord | null | undefined, config: LegacyBotConfig): number {
  return subscriber?.copyTradeBuyPriorityFeeSol ?? config.copyTradePriorityFee ?? DEFAULT_COPY_TRADE_PRIORITY_FEE;
}

function effectiveSellSlippage(subscriber: SubscriberRecord | null | undefined, config: LegacyBotConfig): number {
  return subscriber?.copyTradeSellSlippagePercent ?? config.copyTradeSlippage ?? DEFAULT_COPY_TRADE_SLIPPAGE;
}

function effectiveSellPriorityFee(subscriber: SubscriberRecord | null | undefined, config: LegacyBotConfig): number {
  return subscriber?.copyTradeSellPriorityFeeSol ?? config.copyTradePriorityFee ?? DEFAULT_COPY_TRADE_PRIORITY_FEE;
}

function settingSource(value: number | null | undefined): string {
  return value === null || value === undefined ? "Inherited" : "Custom";
}

export function canCreatePumpPortalTradingWalletInChat(chatType?: string | null): boolean {
  return !chatType || chatType === "private";
}

export function tradingWalletCreationBlockedText(chatType?: string | null): string | null {
  if (canCreatePumpPortalTradingWalletInChat(chatType)) {
    return null;
  }

  return [
    "<b>Trading wallet creation is only available in a private Telegram chat.</b>",
    "",
    "PumpPortal trading wallets reveal a private key once. Open a 1:1 chat with this bot and run /mywallets there so the key is not posted in a group, supergroup, or channel."
  ].join("\n");
}

export function tradingWalletBackupWarningText(): string {
  return [
    "<b>Hot-wallet/private-key warning</b>",
    "This creates a PumpPortal hot wallet for copy buys.",
    "The private key is shown once. Back it up somewhere private before depositing SOL.",
    "Anyone who sees that key can spend the wallet, and the bot cannot recover it later."
  ].join("\n");
}

export function formatTradingWalletCreateConfirmText({
  existingPublicKey = null
}: {
  existingPublicKey?: string | null;
} = {}): string {
  if (!existingPublicKey) {
    return [
      "<b>Create Trading Wallet?</b>",
      "",
      "This creates a PumpPortal trading wallet and makes it your active copytrade wallet.",
      "",
      tradingWalletBackupWarningText()
    ].join("\n");
  }

  return [
    "<b>Create New Trading Wallet?</b>",
    "",
    "This creates a fresh PumpPortal trading wallet and makes it your active copytrade wallet.",
    "",
    "<b>Current wallet</b>",
    `<code>${escapeHtml(existingPublicKey)}</code>`,
    "",
    "Your old wallet will still exist on-chain, but the bot will use the new wallet for future copy buys.",
    "",
    tradingWalletBackupWarningText()
  ].join("\n");
}

export function parseSlippageInput(value: string): number | null {
  const normalized = value.trim().replace(/%$/, "").trim();
  const parsed = Number(normalized);

  if (!Number.isFinite(parsed) || parsed < 0.1 || parsed > 100) {
    return null;
  }

  return parsed;
}

export function parsePriorityFeeInput(value: string): number | null {
  const parsed = Number(value.trim());

  if (!Number.isFinite(parsed) || parsed <= 0 || parsed > 1) {
    return null;
  }

  return parsed;
}

function shortWallet(value: string): string {
  return value.length > 12 ? `${value.slice(0, 6)}...${value.slice(-6)}` : value;
}

function formatCopyTradeWalletSummary(wallet: WatchedWallet): string {
  return wallet.label ? escapeHtml(wallet.label) : shortWallet(wallet.address);
}

export function formatCopyTradeDashboardText({
  tradingWalletPublicKey,
  copyAmountSol,
  copyTradeWallets,
  buySlippagePercent = DEFAULT_COPY_TRADE_SLIPPAGE,
  buyPriorityFeeSol = DEFAULT_COPY_TRADE_PRIORITY_FEE,
  sellSlippagePercent = DEFAULT_COPY_TRADE_SLIPPAGE,
  sellPriorityFeeSol = DEFAULT_COPY_TRADE_PRIORITY_FEE,
  now = new Date()
}: {
  tradingWalletPublicKey: string | null;
  copyAmountSol: number | null;
  copyTradeWallets: WatchedWallet[];
  buySlippagePercent?: number;
  buyPriorityFeeSol?: number;
  sellSlippagePercent?: number;
  sellPriorityFeeSol?: number;
  now?: Date;
}): string {
  const ready = Boolean(tradingWalletPublicKey && copyAmountSol && copyTradeWallets.length > 0);
  const trailingSellStatus = copyTradeWallets.length === 0
    ? "Add wallets first"
    : copyTradeWallets.some((wallet) => wallet.trailingSellConfig)
      ? "Configured per wallet"
      : "Not configured";
  const walletLines = copyTradeWallets.length === 0
    ? ["No Copytrade Wallets yet."]
    : copyTradeWallets.map((wallet) => `└ ${formatCopyTradeWalletSummary(wallet)}`);

  return [
    "<b>🔎 Copy Trading</b>",
    "",
    "Automatically mirror trades from selected wallets in real time.",
    "",
    "<b>👛 Trading Wallet:</b>",
    tradingWalletPublicKey ? `└ ${shortWallet(tradingWalletPublicKey)}` : "└ Not created",
    "",
    `<b>💰 Copy Amount:</b> ${copyAmountSol ? `${formatSolAmount(copyAmountSol)} SOL` : "Not set"}`,
    `<b>⚙️ Buy:</b> ${formatPercent(buySlippagePercent)}% slip / ${formatSolAmount(buyPriorityFeeSol)} SOL priority`,
    `<b>⚙️ Sell:</b> ${formatPercent(sellSlippagePercent)}% slip / ${formatSolAmount(sellPriorityFeeSol)} SOL priority`,
    "",
    `<b>🎯 Copytrade Wallets:</b> ${copyTradeWallets.length}`,
    ...walletLines,
    "",
    ready ? "🟢 Setup is <b>active</b>" : "🔴 Setup is <b>inactive</b>",
    "",
    `<b>📉 Trailing Sells:</b> ${trailingSellStatus}`,
    "",
    `🕒 Last updated: ${formatDashboardTime(now)}`
  ].join("\n");
}

function formatDashboardTime(value: Date): string {
  return value.toLocaleTimeString("en-US", {
    hour12: false,
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit"
  });
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
  const inherited = inheritedTrailingSellConfig(config);

  if (!inherited?.enabled) {
    return ["<b>Trailing sells:</b> Inherited off"];
  }

  return [
    "<b>Trailing sells:</b> Inherited on",
    `<b>Trailing schedule:</b> ${formatTrailingSellSteps(inherited.steps)}`
  ];
}

export function inheritedTrailingSellConfig(config: LegacyBotConfig, now = new Date().toISOString()): TrailingSellConfig | null {
  if (!config.copyTradeTrailingSellEnabled) {
    return null;
  }

  return defaultTrailingSellConfig(config, true, now);
}

function defaultTrailingSellConfig(config: LegacyBotConfig, enabled = true, now = new Date().toISOString()): TrailingSellConfig {
  const holdMs = Math.max(0, Math.floor(config.copyTradeTrailingSellHoldMs || 2000));
  const intervalMs = Math.max(1, Math.floor(config.copyTradeTrailingSellIntervalMs || 2000));
  const firstPercent = clampPercent(config.copyTradeTrailingSellFirstPercent || 20);
  const trailPercent = clampPercent(config.copyTradeTrailingSellTrailPercent || 20);
  const maxBuilds = Math.min(MAX_TRAILING_SELL_STEPS, Math.max(1, Math.floor(config.copyTradeTrailingSellMaxBuilds || 5)));
  const percents = maxBuilds <= 1
    ? [100]
    : [firstPercent, ...Array.from({ length: Math.max(0, maxBuilds - 2) }, () => trailPercent), 100];

  return {
    enabled,
    mode: "formula",
    percentBasis: "remaining_balance",
    steps: percents.map((percent, index) => ({
      delayMs: holdMs + intervalMs * index,
      percent
    })),
    updatedAt: now
  };
}

function clampPercent(value: number): number {
  return Math.min(100, Math.max(0.000001, value));
}

function formatDuration(ms: number): string {
  if (ms === 0) {
    return "0s";
  }

  if (ms % 3_600_000 === 0) {
    return `${ms / 3_600_000}h`;
  }

  if (ms % 60_000 === 0) {
    return `${ms / 60_000}m`;
  }

  if (ms % 1000 === 0) {
    return `${ms / 1000}s`;
  }

  return `${ms}ms`;
}

function formatTrailingSellSteps(steps: TrailingSellStep[]): string {
  return steps.length === 0
    ? "No steps"
    : steps.map((step) => `${formatTrailingPercent(step.percent)}% after ${formatDuration(step.delayMs)}`).join(" | ");
}

function formatTrailingPercent(value: number): string {
  return Number.isInteger(value) ? String(value) : value.toFixed(4).replace(/0+$/, "").replace(/\.$/, "");
}

function parseDurationMs(value: string): number | null {
  const match = value.trim().toLowerCase().match(/^(\d+(?:\.\d+)?)(s|m|h)?$/);

  if (!match) {
    return null;
  }

  const amount = Number(match[1]);
  const unit = match[2] || "s";

  if (!Number.isFinite(amount) || amount < 0) {
    return null;
  }

  const multiplier = unit === "h" ? 3_600_000 : unit === "m" ? 60_000 : 1000;
  return Math.floor(amount * multiplier);
}

function parsePercentToken(value: string): number | null {
  const number = Number(value.trim().replace(/%$/, ""));

  if (!Number.isFinite(number) || number <= 0 || number > 100) {
    return null;
  }

  return number;
}

export function parseTrailingSellStepInput(value: string): TrailingSellStep | null {
  const parts = value.trim().replace(/\s+after\s+/i, " ").split(/\s+/).filter(Boolean);

  if (parts.length !== 2) {
    return null;
  }

  const percent = parsePercentToken(parts[0]);
  const delayMs = parseDurationMs(parts[1]);

  if (percent === null || delayMs === null) {
    return null;
  }

  return { percent, delayMs };
}

function normalizeTrailingSellStepEntries(value: string): string[] {
  const rawEntries = value.split(/[\n,;]+/).map((entry) => entry.trim()).filter(Boolean);
  const entries: string[] = [];

  for (let index = 0; index < rawEntries.length; index += 1) {
    const current = rawEntries[index];
    const next = rawEntries[index + 1];

    if (
      current &&
      next &&
      parsePercentToken(current) !== null &&
      parseDurationMs(next) !== null
    ) {
      entries.push(`${current} ${next}`);
      index += 1;
      continue;
    }

    entries.push(current);
  }

  return entries;
}

export function parseTrailingSellStepsInput(value: string): TrailingSellStep[] | null {
  const rawEntries = normalizeTrailingSellStepEntries(value);
  const steps = rawEntries
    .map((entry) => parseTrailingSellStepInput(entry))
    .filter((step): step is TrailingSellStep => Boolean(step))
    .sort((left, right) => left.delayMs - right.delayMs);

  if (rawEntries.length === 0 || rawEntries.length !== steps.length || steps.length > MAX_TRAILING_SELL_STEPS) {
    return null;
  }

  return steps;
}

export function parseTrailingSellFormulaInput(value: string): TrailingSellStep[] | null {
  const parts = value.trim().split(/\s+/).filter(Boolean);

  if (parts.length !== 5) {
    return null;
  }

  const firstPercent = parsePercentToken(parts[0]);
  const firstDelayMs = parseDurationMs(parts[1]);
  const repeatPercent = parsePercentToken(parts[2]);
  const intervalMs = parseDurationMs(parts[3]);
  const finalDelayMs = parseDurationMs(parts[4]);

  if (
    firstPercent === null ||
    firstDelayMs === null ||
    repeatPercent === null ||
    intervalMs === null ||
    finalDelayMs === null ||
    intervalMs <= 0 ||
    finalDelayMs <= firstDelayMs
  ) {
    return null;
  }

  const steps: TrailingSellStep[] = [{ percent: firstPercent, delayMs: firstDelayMs }];
  let nextDelayMs = firstDelayMs + intervalMs;

  while (nextDelayMs < finalDelayMs && steps.length < MAX_TRAILING_SELL_STEPS - 1) {
    steps.push({ percent: repeatPercent, delayMs: nextDelayMs });
    nextDelayMs += intervalMs;
  }

  steps.push({ percent: 100, delayMs: finalDelayMs });
  return steps;
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
  return [
    "<b>🚀 Welcome to Pump.fun Notifier</b>",
    "",
    "Real-time token alerts, wallet tracking, and Bloom-style copy trading from Telegram.",
    "",
    "🔴 Setup is <b>inactive</b>",
    "",
    "<b>🔐 Verification Required</b>",
    "Send:",
    "<code>/verify your-code</code>",
    "",
    "📚 <b>Dashboards</b>",
    "├ /alerts - Token alerts",
    "├ /trackwallets - Wallet tracking",
    "├ /mywallets - Trading wallet",
    "└ /copytrade - Copy trading",
    "",
    `🕒 Last updated: ${formatDashboardTime(new Date())}`
  ].join("\n");
}

export function formatStartDashboardText(subscriber: SubscriberRecord): string {
  const tokenAlertsActive = Boolean(subscriber.mode);
  const tradingWalletStatus = subscriber.tradingWallet ? shortWallet(subscriber.tradingWallet.publicKey) : "Not created";
  const copyReady = Boolean(subscriber.tradingWallet && subscriber.copyAmountSol && subscriber.copyTradeWallets.length > 0);
  const copyWalletLines = subscriber.copyTradeWallets.length === 0
    ? ["└ No Copytrade Wallets yet"]
    : subscriber.copyTradeWallets.slice(0, 3).map((wallet) => `└ ${formatCopyTradeWalletSummary(wallet)}`);
  const moreCopyWallets = subscriber.copyTradeWallets.length > 3
    ? [`└ +${subscriber.copyTradeWallets.length - 3} more in /copytrade`]
    : [];

  return [
    "<b>🚀 Welcome to Pump.fun Notifier</b>",
    "",
    "Real-time token alerts, wallet tracking, and Bloom-style copy trading from Telegram.",
    "",
    tokenAlertsActive || copyReady ? "🟢 Setup is <b>active</b>" : "🔴 Setup is <b>inactive</b>",
    "",
    "<b>🔔 Token Alerts</b>",
    `└ ${tokenAlertsActive ? modeLabel(subscriber.mode as AlertModeValue) : "Paused"}`,
    "",
    "<b>👀 Tracked Wallets</b>",
    `└ ${subscriber.watchedWallets.length}`,
    "",
    "<b>👛 Trading Wallet</b>",
    `└ ${tradingWalletStatus}`,
    "",
    "<b>⚡ Copy Trading</b>",
    `├ Amount: ${subscriber.copyAmountSol ? `${formatSolAmount(subscriber.copyAmountSol)} SOL` : "Not set"}`,
    `├ Wallets: ${subscriber.copyTradeWallets.length}`,
    ...copyWalletLines,
    ...moreCopyWallets,
    "",
    "📚 <b>Dashboards</b>",
    "├ /alerts - Token alerts",
    "├ /trackwallets - Wallet tracking",
    "├ /mywallets - Trading wallet",
    "└ /copytrade - Copy trading",
    "",
    `🕒 Last updated: ${formatDashboardTime(new Date())}`
  ].join("\n");
}

export function helpText(_chatId?: TelegramChatId): string {
  return [
    "<b>📚 Pump.fun Notifier Help</b>",
    "",
    "Everything runs through dashboards. Open one, tap buttons, and the bot will guide the next step.",
    "",
    "<b>🚀 Quick Start</b>",
    "├ /start - Open your status page",
    "├ /verify &lt;code&gt; - Verify this chat",
    "└ /stop - Stop notifications",
    "",
    "<b>🔔 Alerts</b>",
    "└ /alerts - Toggle migrated coins and new tokens",
    "",
    "<b>👀 Wallet Tracking</b>",
    "└ /trackwallets - Track wallets for normal trade alerts",
    "",
    "<b>👛 Trading Wallet</b>",
    "└ /mywallets - Create or view your PumpPortal trading wallet",
    "",
    "<b>⚡ Copy Trading</b>",
    "└ /copytrade - Configure copy amount, wallets, and trailing sells",
    "",
    `🕒 Last updated: ${formatDashboardTime(new Date())}`
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
    const chatType = callbackQuery.message?.chat?.type;
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
      const dashboard = myWalletCreateConfirm(chatId, { chatType });
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "mywallets:create_confirm") {
      await createTradingWallet(chatId, { chatType });
      return;
    }

    if (data === "mywallets:new") {
      const dashboard = myWalletCreateConfirm(chatId, { replaceExisting: true, chatType });
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "mywallets:new_confirm") {
      await createTradingWallet(chatId, { replaceExisting: true, chatType });
      return;
    }

    if (data === "mywallets:rename") {
      setPendingCopyInput(chatId, "rename_trading_wallet");
      await reply(chatId, "Send a trading wallet nickname, or send <code>-</code> to clear it.");
      return;
    }

    if (data === "mywallets:switch") {
      const dashboard = myWalletSwitchPicker(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data.startsWith("mywallets:select:")) {
      const walletIndex = Number(data.slice("mywallets:select:".length));
      await selectTradingWalletByIndex(chatId, walletIndex);
      return;
    }

    if (data === "copytrade:set_amount") {
      setPendingCopyInput(chatId, "copy_amount");
      await reply(chatId, "Send the fixed copy size in SOL, for example <code>0.1</code>.");
      return;
    }

    if (data === "copytrade:settings") {
      const dashboard = copyTradeSettingsDashboard(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "copytrade:settings:buy_slippage") {
      setPendingCopyInput(chatId, "buy_slippage");
      await reply(chatId, "Send buy slippage percent, for example <code>10</code>, <code>10%</code>, or <code>2.5</code>.");
      return;
    }

    if (data === "copytrade:settings:buy_priority") {
      setPendingCopyInput(chatId, "buy_priority_fee");
      await reply(chatId, "Send buy priority fee in SOL, for example <code>0.00005</code>.");
      return;
    }

    if (data === "copytrade:settings:sell_slippage") {
      setPendingCopyInput(chatId, "sell_slippage");
      await reply(chatId, "Send sell slippage percent, for example <code>10</code>, <code>10%</code>, or <code>2.5</code>.");
      return;
    }

    if (data === "copytrade:settings:sell_priority") {
      setPendingCopyInput(chatId, "sell_priority_fee");
      await reply(chatId, "Send sell priority fee in SOL, for example <code>0.00005</code>.");
      return;
    }

    if (data === "copytrade:settings:reset") {
      const updated = await subscribers?.resetCopyTradeExecutionSettings(chatId);

      if (!updated) {
        await reply(chatId, verificationPrompt());
        return;
      }

      const dashboard = copyTradeSettingsDashboard(chatId);
      await reply(chatId, `<b>Execution settings reset to inherited defaults.</b>\n\n${dashboard.text}`, dashboard.replyMarkup);
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

    if (data === "copytrade:remove_trade_wallet" || data === "copytrade:remove_picker") {
      const dashboard = copyTradeRemovePicker(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "copytrade:stop") {
      const dashboard = copyTradeStopPicker(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "copytrade:stop_confirm") {
      const dashboard = copyTradeStopPicker(chatId);
      await reply(chatId, `Choose which Copytrade Wallet to stop.\n\n${dashboard.text}`, dashboard.replyMarkup);
      return;
    }

    if (data.startsWith("copytrade:stop_one:")) {
      const walletIndex = Number(data.slice("copytrade:stop_one:".length));
      const dashboard = copyTradeStopConfirm(chatId, walletIndex);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data.startsWith("copytrade:stop_confirm:")) {
      const walletIndex = Number(data.slice("copytrade:stop_confirm:".length));
      await stopCopyTradingByIndex(chatId, walletIndex);
      return;
    }

    if (data.startsWith("copytrade:remove_one:")) {
      const walletIndex = Number(data.slice("copytrade:remove_one:".length));
      const dashboard = copyTradeRemoveConfirm(chatId, walletIndex);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data.startsWith("copytrade:remove_confirm:")) {
      const walletIndex = Number(data.slice("copytrade:remove_confirm:".length));
      await removeCopyTradeWalletByIndex(chatId, walletIndex);
      return;
    }

    if (data === "copytrade:remove_all") {
      const dashboard = copyTradeRemoveAllConfirm(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data === "copytrade:remove_all_confirm") {
      await removeAllCopyTradeWallets(chatId);
      return;
    }

    if (data === "copytrade:trailing") {
      const dashboard = copyTradeTrailingWalletPicker(chatId);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (data.startsWith("copytrade:trail:")) {
      await handleTrailingSellCallback(chatId, data);
      return;
    }

    await reply(chatId, "That copy trade action is no longer available. Send /copytrade to reopen the menu.");
  }

  async function handleTrailingSellCallback(chatId: TelegramChatId, data: string): Promise<void> {
    const parts = data.split(":");
    const action = parts[2] || "";
    const walletIndex = Number(parts[3]);
    const wallet = copyTradeWalletByIndex(chatId, walletIndex);

    if (!wallet) {
      await reply(chatId, "That Copytrade Wallet is no longer available. Send /copytrade to reopen the menu.");
      return;
    }

    if (action === "open") {
      const dashboard = trailingSellWalletDashboard(chatId, walletIndex);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (action === "toggle") {
      const nextConfig = {
        ...effectiveTrailingSellConfig(wallet),
        enabled: !effectiveTrailingSellConfig(wallet).enabled,
        updatedAt: new Date().toISOString()
      };
      await subscribers?.setCopyTradeWalletTrailingSellConfig(chatId, wallet.address, nextConfig);
      const dashboard = trailingSellWalletDashboard(chatId, walletIndex);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (action === "reset") {
      await subscribers?.setCopyTradeWalletTrailingSellConfig(chatId, wallet.address, null);
      const dashboard = trailingSellWalletDashboard(chatId, walletIndex);
      await reply(chatId, `<b>Trailing sells reset to inherited defaults.</b>\n\n${dashboard.text}`, dashboard.replyMarkup);
      return;
    }

    if (action === "basis") {
      const nextBasis: TrailingSellPercentBasis = effectiveTrailingSellConfig(wallet).percentBasis === "remaining_balance"
        ? "original_position"
        : "remaining_balance";
      const nextConfig = {
        ...effectiveTrailingSellConfig(wallet),
        percentBasis: nextBasis,
        updatedAt: new Date().toISOString()
      };
      await subscribers?.setCopyTradeWalletTrailingSellConfig(chatId, wallet.address, nextConfig);
      const dashboard = trailingSellWalletDashboard(chatId, walletIndex);
      await reply(chatId, dashboard.text, dashboard.replyMarkup);
      return;
    }

    if (action === "add") {
      setPendingCopyInput(chatId, "trailing_step", wallet.address);
      await reply(chatId, "Send one trailing sell step like <code>20% 10s</code>, <code>50 2m</code>, or <code>100% 1h</code>.");
      return;
    }

    if (action === "edit") {
      setPendingCopyInput(chatId, "trailing_steps", wallet.address);
      await reply(
        chatId,
        "Send all trailing sell steps, separated by commas or new lines.\nExample: <code>20% 10s, 30% 2m, 100% 10m</code>."
      );
      return;
    }

    if (action === "preset") {
      setPendingCopyInput(chatId, "trailing_formula", wallet.address);
      await reply(
        chatId,
        "Send a formula as <code>first% firstDelay repeat% interval finalDelay</code>.\nExample: <code>20% 10s 20% 30s 5m</code>."
      );
      return;
    }

    await reply(chatId, "That trailing sell action is no longer available. Send /copytrade to reopen the menu.");
  }

  function isToggleAlertType(value: string): value is ToggleAlertType {
    return value === "migrations" || value === "newtokens";
  }

  async function startNotifications(chatId: TelegramChatId, args: string[]): Promise<string> {
    const subscriber = subscribers?.get(chatId);

    if (subscriber) {
      return formatStartDashboardText(subscriber);
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
    const subscriber = subscribers.get(chatId);
    return subscriber ? formatStartDashboardText(subscriber) : `<b>Verified.</b>\n\n${chooseModeText()}`;
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
    const tradingWallets = subscribers?.listTradingWallets(chatId) || [];
    const otherWalletCount = Math.max(0, tradingWallets.length - (tradingWallet ? 1 : 0));
    const text = tradingWallet
      ? [
          "<b>👛 Trading Wallet</b>",
          "",
          "Deposit SOL here to enable auto copy buys.",
          "",
          `<b>📚 Saved Wallets:</b> ${tradingWallets.length}`,
          "",
          `<b>🏷️ Name:</b> ${tradingWallet.label ? escapeWalletLabel(tradingWallet.label) : "Not set"}`,
          "",
          "<b>📥 Deposit Address</b>",
          `<code>${tradingWallet.publicKey}</code>`,
          "",
          `<b>🔑 API key:</b> Saved ending in <code>${tradingWallet.apiKeyLast4}</code>`,
          "<b>🔐 Private key:</b> Shown once when created. The bot cannot recover it.",
          "",
          "🟢 Wallet is ready once it has SOL.",
          otherWalletCount > 0 ? `🔁 ${otherWalletCount} other wallet${otherWalletCount === 1 ? "" : "s"} saved. Use Switch Wallet to change active wallets.` : null,
          "",
          `🕒 Last updated: ${formatDashboardTime(new Date())}`
        ].filter((line): line is string => line !== null).join("\n")
      : [
          "<b>👛 Trading Wallet</b>",
          "",
          "No trading wallet yet.",
          "",
          "Create a PumpPortal trading wallet to enable Bloom-style copy buys.",
          "",
          "🔴 Setup is <b>inactive</b>",
          "",
          `🕒 Last updated: ${formatDashboardTime(new Date())}`
        ].join("\n");

    return {
      text,
      replyMarkup: myWalletDashboardReplyMarkup(tradingWallet?.publicKey || null)
    };
  }

  function myWalletDashboardReplyMarkup(publicKey: string | null): TelegramReplyMarkup {
    if (!publicKey) {
      return {
        inline_keyboard: [[{ text: "🚀 Create Wallet", callback_data: "mywallets:create" }]]
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
          { text: "✏️ Rename", callback_data: "mywallets:rename" }
        ],
        [
          { text: "➕ New Wallet", callback_data: "mywallets:new" },
          { text: "🔁 Switch Wallet", callback_data: "mywallets:switch" }
        ],
        [{ text: "📊 Status", callback_data: "mywallets:dashboard" }]
      ]
    };
  }

  function myWalletSwitchPicker(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const wallets = subscribers?.listTradingWallets(chatId) || [];
    const activeWallet = subscribers?.getTradingWallet(chatId) || null;

    if (wallets.length === 0) {
      return {
        text: "No trading wallets yet.",
        replyMarkup: {
          inline_keyboard: [
            [{ text: "🚀 Create Wallet", callback_data: "mywallets:create" }],
            [{ text: "↩️ Back", callback_data: "mywallets:dashboard" }]
          ]
        }
      };
    }

    return {
      text: [
        "<b>🔁 Switch Trading Wallet</b>",
        "",
        "Choose which wallet the bot should use for future copy buys."
      ].join("\n"),
      replyMarkup: {
        inline_keyboard: [
          ...wallets.map((wallet, index) => [
            {
              text: `${wallet.publicKey === activeWallet?.publicKey ? "🟢" : "⚪"} ${formatTradingWalletButtonLabel(wallet)}`,
              callback_data: `mywallets:select:${index}`
            }
          ]),
          [{ text: "↩️ Back", callback_data: "mywallets:dashboard" }]
        ]
      }
    };
  }

  async function selectTradingWalletByIndex(chatId: TelegramChatId, walletIndex: number): Promise<void> {
    const wallets = subscribers?.listTradingWallets(chatId) || [];
    const wallet = Number.isInteger(walletIndex) && walletIndex >= 0 ? wallets[walletIndex] : null;

    if (!wallet) {
      const picker = myWalletSwitchPicker(chatId);
      await reply(chatId, `That trading wallet is no longer available.\n\n${picker.text}`, picker.replyMarkup);
      return;
    }

    const updated = await subscribers?.setActiveTradingWallet(chatId, wallet.publicKey);

    if (!updated) {
      await reply(chatId, verificationPrompt());
      return;
    }

    const dashboard = myWalletDashboard(chatId);
    await reply(chatId, `<b>Active trading wallet updated:</b> ${formatTradingWalletButtonLabel(wallet)}\n\n${dashboard.text}`, dashboard.replyMarkup);
  }

  function formatTradingWalletButtonLabel(wallet: TradingWallet): string {
    return wallet.label ? escapeWalletLabel(wallet.label) : shortWallet(wallet.publicKey);
  }

  function myWalletCreateConfirm(
    chatId: TelegramChatId,
    {
      replaceExisting = false,
      chatType
    }: {
      replaceExisting?: boolean;
      chatType?: string | null;
    } = {}
  ): { text: string; replyMarkup: TelegramReplyMarkup } {
    const blockedText = tradingWalletCreationBlockedText(chatType);

    if (blockedText) {
      return {
        text: blockedText,
        replyMarkup: myWalletBackReplyMarkup()
      };
    }

    const existing = subscribers?.getTradingWallet(chatId);

    if (existing && !replaceExisting) {
      const dashboard = myWalletDashboard(chatId);
      return {
        text: `<b>Trading wallet already exists.</b>\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    return {
      text: formatTradingWalletCreateConfirmText({
        existingPublicKey: replaceExisting ? existing?.publicKey || null : null
      }),
      replyMarkup: {
        inline_keyboard: [
          [
            {
              text: replaceExisting && existing ? "✅ Create New Wallet" : "✅ Create Wallet",
              callback_data: replaceExisting && existing ? "mywallets:new_confirm" : "mywallets:create_confirm"
            }
          ],
          [{ text: "↩️ Back", callback_data: "mywallets:dashboard" }]
        ]
      }
    };
  }

  function myWalletBackReplyMarkup(): TelegramReplyMarkup {
    return {
      inline_keyboard: [[{ text: "↩️ Back", callback_data: "mywallets:dashboard" }]]
    };
  }

  async function createTradingWallet(
    chatId: TelegramChatId,
    {
      replaceExisting = false,
      chatType
    }: {
      replaceExisting?: boolean;
      chatType?: string | null;
    } = {}
  ): Promise<void> {
    const blockedText = tradingWalletCreationBlockedText(chatType);

    if (blockedText) {
      await reply(chatId, blockedText, myWalletBackReplyMarkup());
      return;
    }

    const existing = subscribers?.getTradingWallet(chatId);

    if (existing && !replaceExisting) {
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
      label: null,
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
        replaceExisting ? "<b>New trading wallet created.</b>" : "<b>Trading wallet created.</b>",
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
        ...(replaceExisting && existing
          ? [
              "<b>Previous active wallet</b>",
              `<code>${existing.publicKey}</code>`,
              ""
            ]
          : []),
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

  function setPendingCopyInput(chatId: TelegramChatId, action: PendingCopyInputAction, walletAddress?: string): void {
    pendingCopyInputs.set(String(chatId), {
      action,
      expiresAt: Date.now() + PENDING_COPY_INPUT_TTL_MS,
      walletAddress
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

    if (pending.action === "rename_trading_wallet") {
      const nextLabel = value === "-" ? null : value;
      const nicknameError = nextLabel === null ? null : validateNickname(nextLabel);

      if (nicknameError) {
        return { text: nicknameError };
      }

      pendingCopyInputs.delete(String(chatId));
      const updated = await subscribers?.renameTradingWallet(chatId, nextLabel);

      if (!updated) {
        return { text: "No trading wallet found yet. Open /mywallets to create one." };
      }

      const dashboard = myWalletDashboard(chatId);
      return {
        text: `${nextLabel === null ? "<b>Trading wallet nickname cleared.</b>" : `<b>Trading wallet renamed:</b> ${escapeWalletLabel(nextLabel)}`}\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    if (pending.action === "trailing_step" || pending.action === "trailing_steps" || pending.action === "trailing_formula") {
      const walletAddress = pending.walletAddress;
      const wallet = walletAddress
        ? (subscribers?.listCopyTradeWallets(chatId) || []).find((entry) => entry.address === walletAddress) || null
        : null;

      if (!wallet) {
        pendingCopyInputs.delete(String(chatId));
        return { text: "That Copytrade Wallet is no longer available. Send /copytrade to reopen the menu." };
      }

      const current = effectiveTrailingSellConfig(wallet);
      const parsedSteps =
        pending.action === "trailing_formula"
          ? parseTrailingSellFormulaInput(value)
          : pending.action === "trailing_steps"
            ? parseTrailingSellStepsInput(value)
            : (() => {
                const step = parseTrailingSellStepInput(value);
                return step ? [...current.steps, step].sort((left, right) => left.delayMs - right.delayMs) : null;
              })();

      if (!parsedSteps || parsedSteps.length === 0 || parsedSteps.length > MAX_TRAILING_SELL_STEPS) {
        return {
          text:
            pending.action === "trailing_formula"
              ? "That formula is not valid. Use <code>20% 10s 20% 30s 5m</code>."
              : "That step list is not valid. Use entries like <code>20% 10s</code>, up to 20 steps."
        };
      }

      pendingCopyInputs.delete(String(chatId));
      const nextConfig: TrailingSellConfig = {
        enabled: true,
        mode: pending.action === "trailing_formula" ? "formula" : "custom_steps",
        percentBasis: current.percentBasis,
        steps: parsedSteps,
        updatedAt: new Date().toISOString()
      };
      const updated = await subscribers?.setCopyTradeWalletTrailingSellConfig(chatId, wallet.address, nextConfig);

      if (!updated) {
        return { text: verificationPrompt() };
      }

      const walletIndex = (subscribers?.listCopyTradeWallets(chatId) || []).findIndex((entry) => entry.address === wallet.address);
      const dashboard = trailingSellWalletDashboard(chatId, walletIndex);
      return {
        text: `<b>Trailing sell schedule saved.</b>\n\n${dashboard.text}`,
        replyMarkup: dashboard.replyMarkup
      };
    }

    if (
      pending.action === "buy_slippage" ||
      pending.action === "sell_slippage" ||
      pending.action === "buy_priority_fee" ||
      pending.action === "sell_priority_fee"
    ) {
      const isSlippage = pending.action === "buy_slippage" || pending.action === "sell_slippage";
      const parsed = isSlippage ? parseSlippageInput(value) : parsePriorityFeeInput(value);

      if (parsed === null) {
        return {
          text: isSlippage
            ? "That slippage is not valid. Send a percent from <code>0.1</code> to <code>100</code>, like <code>10%</code>."
            : "That priority fee is not valid. Send a SOL amount greater than <code>0</code> and no more than <code>1</code>, like <code>0.00005</code>."
        };
      }

      pendingCopyInputs.delete(String(chatId));
      const updated =
        pending.action === "buy_slippage"
          ? await subscribers?.setCopyTradeBuySlippage(chatId, parsed)
          : pending.action === "buy_priority_fee"
            ? await subscribers?.setCopyTradeBuyPriorityFee(chatId, parsed)
            : pending.action === "sell_slippage"
              ? await subscribers?.setCopyTradeSellSlippage(chatId, parsed)
              : await subscribers?.setCopyTradeSellPriorityFee(chatId, parsed);

      if (!updated) {
        return { text: verificationPrompt() };
      }

      const dashboard = copyTradeSettingsDashboard(chatId);
      const label =
        pending.action === "buy_slippage"
          ? "Buy slippage"
          : pending.action === "buy_priority_fee"
            ? "Buy priority fee"
            : pending.action === "sell_slippage"
              ? "Sell slippage"
              : "Sell priority fee";
      const formatted = isSlippage ? `${formatPercent(parsed)}%` : `${formatSolAmount(parsed)} SOL`;
      return {
        text: `<b>${label} saved:</b> ${formatted}\n\n${dashboard.text}`,
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
    const copyTradeWallets = subscribers?.listCopyTradeWallets(chatId) || [];
    const tradingWallet = subscribers?.getTradingWallet(chatId) || null;
    const text = formatCopyTradeDashboardText({
      tradingWalletPublicKey: tradingWallet?.publicKey || null,
      copyAmountSol: subscriber?.copyAmountSol || null,
      copyTradeWallets,
      buySlippagePercent: effectiveBuySlippage(subscriber, config),
      buyPriorityFeeSol: effectiveBuyPriorityFee(subscriber, config),
      sellSlippagePercent: effectiveSellSlippage(subscriber, config),
      sellPriorityFeeSol: effectiveSellPriorityFee(subscriber, config)
    });

    return {
      text,
      replyMarkup: copyTradeDashboardReplyMarkup()
    };
  }

  function copyTradeDashboardReplyMarkup(): TelegramReplyMarkup {
    return {
      inline_keyboard: [
        [
          { text: "💰 Amount", callback_data: "copytrade:set_amount" },
          { text: "➕ Add Wallet", callback_data: "copytrade:add_wallet" }
        ],
        [
          { text: "✏️ Rename", callback_data: "copytrade:rename_wallet" },
          { text: "🗑️ Remove", callback_data: "copytrade:remove_trade_wallet" }
        ],
        [
          { text: "📉 Trailing Sells", callback_data: "copytrade:trailing" },
          { text: "⚙️ Settings", callback_data: "copytrade:settings" }
        ],
        [
          { text: "⏹️ Stop Copy Trading", callback_data: "copytrade:stop" },
          { text: "📋 List Wallets", callback_data: "copytrade:wallets" }
        ]
      ]
    };
  }

  function copyTradeSettingsDashboard(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const gate = requireVerified(chatId);

    if (gate) {
      return {
        text: gate,
        replyMarkup: copyTradeDashboardReplyMarkup()
      };
    }

    const subscriber = subscribers?.get(chatId) || null;
    const text = [
      "<b>⚙️ Execution Settings</b>",
      "",
      "Tune buy and sell execution separately. Custom values override inherited env defaults.",
      "",
      "<b>🟢 Buy Settings</b>",
      `├ Slippage: ${formatPercent(effectiveBuySlippage(subscriber, config))}% (${settingSource(subscriber?.copyTradeBuySlippagePercent)})`,
      `└ Priority: ${formatSolAmount(effectiveBuyPriorityFee(subscriber, config))} SOL (${settingSource(subscriber?.copyTradeBuyPriorityFeeSol)})`,
      "",
      "<b>🔴 Sell Settings</b>",
      `├ Slippage: ${formatPercent(effectiveSellSlippage(subscriber, config))}% (${settingSource(subscriber?.copyTradeSellSlippagePercent)})`,
      `└ Priority: ${formatSolAmount(effectiveSellPriorityFee(subscriber, config))} SOL (${settingSource(subscriber?.copyTradeSellPriorityFeeSol)})`,
      "",
      `🕒 Last updated: ${formatDashboardTime(new Date())}`
    ].join("\n");

    return {
      text,
      replyMarkup: {
        inline_keyboard: [
          [
            { text: "🟢 Buy Slippage", callback_data: "copytrade:settings:buy_slippage" },
            { text: "🟢 Buy Priority", callback_data: "copytrade:settings:buy_priority" }
          ],
          [
            { text: "🔴 Sell Slippage", callback_data: "copytrade:settings:sell_slippage" },
            { text: "🔴 Sell Priority", callback_data: "copytrade:settings:sell_priority" }
          ],
          [{ text: "♻️ Reset Defaults", callback_data: "copytrade:settings:reset" }],
          [{ text: "↩️ Back", callback_data: "copytrade:dashboard" }]
        ]
      }
    };
  }

  function copyTradeRemovePicker(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const wallets = subscribers?.listCopyTradeWallets(chatId) || [];

    if (wallets.length === 0) {
      return {
        text: "No Copytrade Wallets to remove yet.",
        replyMarkup: {
          inline_keyboard: [[{ text: "↩️ Back", callback_data: "copytrade:dashboard" }]]
        }
      };
    }

    return {
      text: ["<b>Remove Copytrade Wallet</b>", "Choose a wallet to stop copytrading."].join("\n"),
      replyMarkup: {
        inline_keyboard: [
          ...wallets.map((wallet, index) => [
            {
              text: `🗑️ ${formatCopyTradeWalletSummary(wallet)}`,
              callback_data: `copytrade:remove_one:${index}`
            }
          ]),
          [{ text: "🗑️ Remove All", callback_data: "copytrade:remove_all" }],
          [{ text: "↩️ Back", callback_data: "copytrade:dashboard" }]
        ]
      }
    };
  }

  function copyTradeRemoveConfirm(chatId: TelegramChatId, walletIndex: number): { text: string; replyMarkup: TelegramReplyMarkup } {
    const wallet = copyTradeWalletByIndex(chatId, walletIndex);

    if (!wallet) {
      const picker = copyTradeRemovePicker(chatId);
      return {
        text: `That Copytrade Wallet is no longer available.\n\n${picker.text}`,
        replyMarkup: picker.replyMarkup
      };
    }

    return {
      text: [
        "<b>Confirm removal</b>",
        `Stop copytrading ${formatCopyTradeWalletSummary(wallet)}?`,
        "",
        "This will remove it from Copytrade Wallets only."
      ].join("\n"),
      replyMarkup: {
        inline_keyboard: [
          [{ text: "✅ Confirm Remove", callback_data: `copytrade:remove_confirm:${walletIndex}` }],
          [{ text: "↩️ Back", callback_data: "copytrade:remove_picker" }]
        ]
      }
    };
  }

  function copyTradeRemoveAllConfirm(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const walletCount = subscribers?.listCopyTradeWallets(chatId).length || 0;

    if (walletCount === 0) {
      return copyTradeRemovePicker(chatId);
    }

    return {
      text: [
        "<b>Confirm remove all</b>",
        `Stop copytrading all ${walletCount} Copytrade Wallet${walletCount === 1 ? "" : "s"}?`,
        "",
        "This will not delete your trading wallet."
      ].join("\n"),
      replyMarkup: {
        inline_keyboard: [
          [{ text: "✅ Confirm Remove All", callback_data: "copytrade:remove_all_confirm" }],
          [{ text: "↩️ Back", callback_data: "copytrade:remove_picker" }]
        ]
      }
    };
  }

  function copyTradeStopPicker(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const wallets = subscribers?.listCopyTradeWallets(chatId) || [];

    if (wallets.length === 0) {
      return {
        text: "Copy trading is already stopped. Add a Copytrade Wallet to turn it back on.",
        replyMarkup: {
          inline_keyboard: [[{ text: "↩️ Back", callback_data: "copytrade:dashboard" }]]
        }
      };
    }

    return {
      text: [
        "<b>⏹️ Stop Copy Trading</b>",
        "",
        "Choose the target wallet you want to stop copytrading.",
        "Your trading wallet, amount, and execution settings stay saved."
      ].join("\n"),
      replyMarkup: {
        inline_keyboard: [
          ...wallets.map((wallet, index) => [
            {
              text: `⏹️ ${formatCopyTradeWalletSummary(wallet)}`,
              callback_data: `copytrade:stop_one:${index}`
            }
          ]),
          [{ text: "↩️ Back", callback_data: "copytrade:dashboard" }]
        ]
      }
    };
  }

  function copyTradeStopConfirm(chatId: TelegramChatId, walletIndex: number): { text: string; replyMarkup: TelegramReplyMarkup } {
    const wallet = copyTradeWalletByIndex(chatId, walletIndex);

    if (!wallet) {
      const picker = copyTradeStopPicker(chatId);
      return {
        text: `That Copytrade Wallet is no longer available.\n\n${picker.text}`,
        replyMarkup: picker.replyMarkup
      };
    }

    return {
      text: [
        "<b>Confirm stop</b>",
        `Stop copytrading ${formatCopyTradeWalletSummary(wallet)}?`,
        "",
        "This only removes this target from Copytrade Wallets.",
        "Your trading wallet, amount, settings, and other targets stay saved."
      ].join("\n"),
      replyMarkup: {
        inline_keyboard: [
          [{ text: "✅ Confirm Stop", callback_data: `copytrade:stop_confirm:${walletIndex}` }],
          [{ text: "↩️ Back", callback_data: "copytrade:stop" }]
        ]
      }
    };
  }

  async function removeCopyTradeWalletByIndex(chatId: TelegramChatId, walletIndex: number): Promise<void> {
    const wallet = copyTradeWalletByIndex(chatId, walletIndex);

    if (!wallet) {
      const picker = copyTradeRemovePicker(chatId);
      await reply(chatId, `That Copytrade Wallet is no longer available.\n\n${picker.text}`, picker.replyMarkup);
      return;
    }

    const removed = await subscribers?.unwatchCopyTradeWallet(chatId, wallet.address);

    if (!removed) {
      const picker = copyTradeRemovePicker(chatId);
      await reply(chatId, `That Copytrade Wallet is no longer available.\n\n${picker.text}`, picker.replyMarkup);
      return;
    }

    const syncWarning = await onWalletWatchlistChange?.();
    const dashboard = copyTradeDashboard(chatId);
    await reply(
      chatId,
      `<b>Removed Copytrade Wallet:</b> ${formatCopyTradeWalletSummary(wallet)}${syncWarning ? `\n\n${syncWarning}` : ""}\n\n${dashboard.text}`,
      dashboard.replyMarkup
    );
  }

  async function removeAllCopyTradeWallets(chatId: TelegramChatId): Promise<void> {
    const removedCount = await subscribers?.unwatchAllCopyTradeWallets(chatId);

    if (!removedCount) {
      const picker = copyTradeRemovePicker(chatId);
      await reply(chatId, picker.text, picker.replyMarkup);
      return;
    }

    const syncWarning = await onWalletWatchlistChange?.();
    const dashboard = copyTradeDashboard(chatId);
    await reply(
      chatId,
      `<b>Removed ${removedCount} Copytrade Wallet${removedCount === 1 ? "" : "s"}.</b>${syncWarning ? `\n\n${syncWarning}` : ""}\n\n${dashboard.text}`,
      dashboard.replyMarkup
    );
  }

  async function stopCopyTradingByIndex(chatId: TelegramChatId, walletIndex: number): Promise<void> {
    const wallet = copyTradeWalletByIndex(chatId, walletIndex);

    if (!wallet) {
      const picker = copyTradeStopPicker(chatId);
      await reply(chatId, `That Copytrade Wallet is no longer available.\n\n${picker.text}`, picker.replyMarkup);
      return;
    }

    const removed = await subscribers?.unwatchCopyTradeWallet(chatId, wallet.address);

    if (!removed) {
      const picker = copyTradeStopPicker(chatId);
      await reply(chatId, `That Copytrade Wallet is no longer available.\n\n${picker.text}`, picker.replyMarkup);
      return;
    }

    const syncWarning = await onWalletWatchlistChange?.();
    const dashboard = copyTradeDashboard(chatId);
    await reply(
      chatId,
      `<b>⏹️ Copy trading stopped for:</b> ${formatCopyTradeWalletSummary(wallet)}${syncWarning ? `\n\n${syncWarning}` : ""}\n\n${dashboard.text}`,
      dashboard.replyMarkup
    );
  }

  function copyTradeWalletByIndex(chatId: TelegramChatId, walletIndex: number): WatchedWallet | null {
    if (!Number.isInteger(walletIndex) || walletIndex < 0) {
      return null;
    }

    return (subscribers?.listCopyTradeWallets(chatId) || [])[walletIndex] || null;
  }

  function effectiveTrailingSellConfig(wallet: WatchedWallet): TrailingSellConfig {
    return wallet.trailingSellConfig || defaultTrailingSellConfig(config, Boolean(config.copyTradeTrailingSellEnabled));
  }

  function trailingSellStatusLabel(wallet: WatchedWallet): string {
    const effective = effectiveTrailingSellConfig(wallet);

    if (!wallet.trailingSellConfig) {
      return effective.enabled ? "Inherited on" : "Inherited off";
    }

    return effective.enabled ? "Custom on" : "Custom off";
  }

  function copyTradeTrailingWalletPicker(chatId: TelegramChatId): { text: string; replyMarkup: TelegramReplyMarkup } {
    const gate = requireVerified(chatId);

    if (gate) {
      return {
        text: gate,
        replyMarkup: copyTradeDashboardReplyMarkup()
      };
    }

    const wallets = subscribers?.listCopyTradeWallets(chatId) || [];

    if (wallets.length === 0) {
      return {
        text: "No Copytrade Wallets yet. Add one from /copytrade first.",
        replyMarkup: copyTradeDashboardReplyMarkup()
      };
    }

    return {
      text: ["<b>Trailing sells</b>", "Choose a Copytrade Wallet to configure."].join("\n"),
      replyMarkup: {
        inline_keyboard: [
          ...wallets.map((wallet, index) => [
            {
              text: `📉 ${wallet.label ? wallet.label : shortWallet(wallet.address)}`,
              callback_data: `copytrade:trail:open:${index}`
            }
          ]),
          [{ text: "↩️ Back", callback_data: "copytrade:dashboard" }]
        ]
      }
    };
  }

  function trailingSellWalletDashboard(chatId: TelegramChatId, walletIndex: number): { text: string; replyMarkup: TelegramReplyMarkup } {
    const wallet = copyTradeWalletByIndex(chatId, walletIndex);

    if (!wallet) {
      return {
        text: "That Copytrade Wallet is no longer available. Send /copytrade to reopen the menu.",
        replyMarkup: copyTradeDashboardReplyMarkup()
      };
    }

    const effective = effectiveTrailingSellConfig(wallet);
    const inherited = !wallet.trailingSellConfig;
    const enabledLine = effective.enabled ? "🟢 Trailing sells are <b>active</b>" : "🔴 Trailing sells are <b>inactive</b>";
    const sourceLine = inherited ? "Inherited env defaults" : "Custom wallet setup";
    const walletName = wallet.label ? escapeWalletLabel(wallet.label) : shortWallet(wallet.address);
    const text = [
      "<b>📉 Trailing Sells</b>",
      "",
      "Configure automated sell steps after this wallet triggers a copy buy.",
      "",
      `<b>🎯 Wallet:</b> ${walletName}`,
      wallet.label ? `└ <code>${wallet.address}</code>` : null,
      "",
      enabledLine,
      "",
      `<b>⚙️ Source:</b> ${sourceLine}`,
      `<b>📐 Basis:</b> ${effective.percentBasis === "remaining_balance" ? "Remaining balance" : "Original position"}`,
      `<b>🧩 Mode:</b> ${effective.mode === "formula" ? "Formula preset" : "Custom steps"}`,
      "",
      "<b>🕒 Sell Schedule</b>",
      ...effective.steps.map((step, index) => `├ ${index + 1}. Sell ${formatTrailingPercent(step.percent)}% after ${formatDuration(step.delayMs)}`),
      "",
      inherited
        ? "⚪ Using inherited defaults until you customize this wallet."
        : "♻️ Reset returns this wallet to inherited defaults.",
      "",
      `🕒 Last updated: ${formatDashboardTime(new Date())}`
    ]
      .filter((line): line is string => line !== null)
      .join("\n");

    return {
      text,
      replyMarkup: trailingSellWalletDashboardReplyMarkup(walletIndex, effective.enabled)
    };
  }

  function trailingSellWalletDashboardReplyMarkup(walletIndex: number, enabled: boolean): TelegramReplyMarkup {
    return {
      inline_keyboard: [
        [
          {
            text: enabled ? "⚪ Disable" : "🟢 Enable",
            callback_data: `copytrade:trail:toggle:${walletIndex}`
          },
          { text: "♻️ Reset", callback_data: `copytrade:trail:reset:${walletIndex}` }
        ],
        [
          { text: "⚙️ Preset", callback_data: `copytrade:trail:preset:${walletIndex}` },
          { text: "➕ Add Step", callback_data: `copytrade:trail:add:${walletIndex}` }
        ],
        [
          { text: "✏️ Edit Steps", callback_data: `copytrade:trail:edit:${walletIndex}` },
          { text: "🔁 Basis", callback_data: `copytrade:trail:basis:${walletIndex}` }
        ],
        [{ text: "↩️ Back", callback_data: "copytrade:trailing" }]
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
    return escapeHtml(value);
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
