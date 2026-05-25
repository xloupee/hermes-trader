import "dotenv/config";
import { appendFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { buildMigrationReplyMarkup, extractMigrationData, formatMigrationMessage, getEventId } from "./format.js";
import { createTelegramCommandPoller } from "./commands.js";
import {
  copyTradeBuyRiskBlockedReason,
  copyTradeDailyBudgetKey,
  copyTradeRequestRiskBlockedReason,
  copyTradeWalletReserveBlockedReason,
  createInMemoryCopyTradeDailySolBudget,
  formatCopyTradeRiskControlLog
} from "./copytrade-risk-controls.js";
import {
  copyTradeLiveExecutionBlockedReason,
  copyTradeLiveExecutionEnabled,
  formatCopyTradeExecutionStateLog
} from "./copytrade-execution-mode.js";
import { copyBuySubmissionKey, createCopyBuySubmissionGuard } from "./copytrade-guard.js";
import {
  createJsonCopyTradeBuyIdempotencyStore,
  createSupabaseCopyTradeBuyIdempotencyStore,
  safelyCompleteCopyTradeBuyIdempotency,
  safelyFailCopyTradeBuyIdempotency
} from "./copytrade-idempotency.js";
import { createCopyTradeLatencyClock, createCopyTradeLatencyTracker } from "./copytrade-latency.js";
import { createHeliusWebhookServer, missingHeliusConfigWarning, syncHeliusWebhook } from "./helius.js";
import { heliusEventMentionsWatchedWallet, isHeliusSwapEvent, normalizeHeliusSwapData } from "./helius-swaps.js";
import {
  buildPumpPortalLightningBuyRequest,
  buildPumpPortalLightningSellRequest,
  createPumpPortalMigrationListener,
  executePumpPortalLightningTrade,
  PUMPPORTAL_CREATE_WALLET_URL,
  PUMPPORTAL_LIGHTNING_TRADE_URL,
  PUMPPORTAL_TRADE_LOCAL_URL
} from "./pumpportal.js";
import type { PumpPortalSubscription } from "./pumpportal.js";
import { decryptSecret, encryptionSecretReady } from "./secrets.js";
import { analyzeSolanaTransaction, getSolanaBalanceSol } from "./solana.js";
import { createSubscriberStore } from "./subscribers.js";
import { createSupabaseCopyTradeRecorderFromEnv, createSupabaseSubscriberStoreFromEnv } from "./subscribers-supabase.js";
import { sendTelegramMessage, sendTelegramPhoto } from "./telegram.js";
import { asRecord, errorMessage, isRecord, stringValue } from "./types.js";
import {
  buildWalletTradeReplyMarkup,
  formatAutoCopyBuyMessage,
  formatCopyTradeSimulationMessage,
  formatCopyTradeTrailingSellResultMessage,
  formatCopyTradeTrailingSellSkippedMessage,
  formatCopyTradeTrailingSellScheduledMessage,
  formatWalletTradeMessageWithCopySettings,
  getWalletTradeEventId,
  isCopyableSolToTokenBuy
} from "./wallet-monitor.js";
import type {
  AlertModeValue,
  BotConfig,
  CopyTradeExecutionRecord,
  LooseRecord,
  MigrationData,
  PumpPortalLightningTradeRequest,
  PumpPortalLightningTradeResult,
  PumpPortalTradePool,
  SubscriberRecord,
  TrailingSellConfig,
  TrailingSellStep,
  TransactionAnalysis,
  WalletTradeData,
  WatchedWallet
} from "./types.js";
import type { CopyTradeLatencyMilestoneDetails, CopyTradeLatencyTracker } from "./copytrade-latency.js";

function numberFromEnv(value: string | undefined, fallback: number): number {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
}

function positiveNumberFromEnv(value: string | undefined, fallback: number): number {
  const number = numberFromEnv(value, fallback);
  return number > 0 ? number : fallback;
}

function nonNegativeNumberFromEnv(value: string | undefined, fallback: number): number {
  const number = numberFromEnv(value, fallback);
  return number >= 0 ? number : fallback;
}

function positiveIntegerFromEnv(value: string | undefined, fallback: number): number {
  return Math.max(1, Math.floor(positiveNumberFromEnv(value, fallback)));
}

function percentFromEnv(value: string | undefined, fallback: number): number {
  const number = positiveNumberFromEnv(value, fallback);
  return Math.min(100, number);
}

function listFromEnv(value: string | undefined): string[] {
  return value
    ? [...new Set(value.split(",").map((entry) => entry.trim().toUpperCase()).filter(Boolean))]
    : [];
}

function finiteNumberValue(value: unknown): number | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

function pumpPortalPoolFromEnv(value: string | undefined): PumpPortalTradePool {
  return value === "pump" ||
    value === "pump-amm" ||
    value === "raydium" ||
    value === "raydium-cpmm" ||
    value === "launchlab" ||
    value === "bonk"
    ? value
    : "auto";
}

const config: BotConfig = {
  telegramToken: process.env.TELEGRAM_BOT_TOKEN,
  telegramChatId: process.env.TELEGRAM_CHAT_ID,
  telegramVerifyCode: process.env.TELEGRAM_VERIFY_CODE,
  telegramSubscribersPath: process.env.TELEGRAM_SUBSCRIBERS_PATH || "data/telegram-subscribers.json",
  supabaseUrl: process.env.SUPABASE_URL,
  supabaseServiceRoleKey: process.env.SUPABASE_SERVICE_ROLE_KEY || process.env.SUPABASE_SERVICE_KEY || process.env.SUPABASE_SERVICE_ROLE,
  pumpPortalApiKey: process.env.PUMPPORTAL_API_KEY,
  pumpPortalWsUrl: process.env.PUMPPORTAL_WS_URL || "wss://pumpportal.fun/api/data",
  pumpPortalTradeLocalUrl: process.env.PUMPPORTAL_TRADE_LOCAL_URL || PUMPPORTAL_TRADE_LOCAL_URL,
  pumpPortalCreateWalletUrl: process.env.PUMPPORTAL_CREATE_WALLET_URL || PUMPPORTAL_CREATE_WALLET_URL,
  pumpPortalLightningTradeUrl: process.env.PUMPPORTAL_LIGHTNING_TRADE_URL || PUMPPORTAL_LIGHTNING_TRADE_URL,
  pumpPortalWalletKeyEncryptionSecret: process.env.PUMPPORTAL_WALLET_KEY_ENCRYPTION_SECRET,
  alertMode: process.env.ALERT_MODE || process.env.PUMPPORTAL_ALERT_MODE || "migrations",
  solscanBaseUrl: process.env.SOLSCAN_BASE_URL || "https://solscan.io",
  pumpFunBaseUrl: process.env.PUMPFUN_BASE_URL || "https://pump.fun",
  migrationLogPath: process.env.MIGRATION_LOG_PATH || "logs/migrations.jsonl",
  walletTradeLogPath: process.env.WALLET_TRADE_LOG_PATH || "logs/wallet-trades.jsonl",
  copyTradeEmergencyStopPath: process.env.COPY_TRADE_EMERGENCY_STOP_PATH || "data/copytrade-emergency-stop.json",
  heliusApiKey: process.env.HELIUS_API_KEY,
  heliusApiBaseUrl: process.env.HELIUS_API_BASE_URL || "https://api-mainnet.helius-rpc.com",
  heliusWebhookAuthHeader: process.env.HELIUS_WEBHOOK_AUTH_HEADER,
  heliusWebhookId: process.env.HELIUS_WEBHOOK_ID,
  heliusWebhookPublicUrl: process.env.HELIUS_WEBHOOK_PUBLIC_URL,
  heliusWebhookStatePath: process.env.HELIUS_WEBHOOK_STATE_PATH || "data/helius-webhook.json",
  webhookPort: Number(process.env.WEBHOOK_PORT || 3000),
  pumpFunCoinApiBaseUrl: process.env.PUMPFUN_COIN_API_BASE_URL || "https://frontend-api-v3.pump.fun/coins",
  solUsdPriceUrl: process.env.SOL_USD_PRICE_URL || "https://api.coinbase.com/v2/prices/SOL-USD/spot",
  solanaRpcUrl: process.env.SOLANA_RPC_URL || "https://api.mainnet-beta.solana.com",
  transactionFlowEnabled: process.env.TRANSACTION_FLOW_ENABLED === "true",
  transactionAccountLabels: process.env.TRANSACTION_ACCOUNT_LABELS,
  shutdownReason: process.env.BOT_SHUTDOWN_REASON,
  copyTradeEnabled: process.env.COPY_TRADE_ENABLED === "true",
  copyTradeDryRun: process.env.COPY_TRADE_DRY_RUN !== "false",
  copyTradeSlippage: numberFromEnv(process.env.COPY_TRADE_SLIPPAGE, 10),
  copyTradePriorityFee: numberFromEnv(process.env.COPY_TRADE_PRIORITY_FEE, 0.00005),
  copyTradePool: pumpPortalPoolFromEnv(process.env.COPY_TRADE_POOL),
  copyTradeMaxBuySol: positiveNumberFromEnv(process.env.COPY_TRADE_MAX_BUY_SOL, 0.005),
  copyTradeDailySolCap: positiveNumberFromEnv(process.env.COPY_TRADE_DAILY_SOL_CAP, 0.02),
  copyTradeMinWalletReserveSol: nonNegativeNumberFromEnv(process.env.COPY_TRADE_MIN_WALLET_RESERVE_SOL, 0),
  copyTradeMaxSignalAgeMs: positiveIntegerFromEnv(process.env.COPY_TRADE_MAX_SIGNAL_AGE_MS, 60_000),
  copyTradeMaxCopyWalletsPerChat: positiveIntegerFromEnv(process.env.COPY_TRADE_MAX_COPY_WALLETS_PER_CHAT, 1),
  copyTradeAllowedSources: listFromEnv(process.env.COPY_TRADE_ALLOWED_SOURCES),
  copyTradeMaxSlippage: percentFromEnv(process.env.COPY_TRADE_MAX_SLIPPAGE, 15),
  copyTradeMaxPriorityFee: positiveNumberFromEnv(process.env.COPY_TRADE_MAX_PRIORITY_FEE, 0.0002),
  copyTradeTrailingSellEnabled: process.env.COPY_TRADE_TRAILING_SELL_ENABLED === "true",
  copyTradeTrailingSellHoldMs: positiveIntegerFromEnv(process.env.COPY_TRADE_TRAILING_SELL_HOLD_MS, 2000),
  copyTradeTrailingSellFirstPercent: percentFromEnv(process.env.COPY_TRADE_TRAILING_SELL_FIRST_PERCENT, 20),
  copyTradeTrailingSellTrailPercent: percentFromEnv(process.env.COPY_TRADE_TRAILING_SELL_TRAIL_PERCENT, 20),
  copyTradeTrailingSellIntervalMs: positiveIntegerFromEnv(process.env.COPY_TRADE_TRAILING_SELL_INTERVAL_MS, 2000),
  copyTradeTrailingSellMaxBuilds: positiveIntegerFromEnv(process.env.COPY_TRADE_TRAILING_SELL_MAX_BUILDS, 5)
};

const seenEvents = new Set<string>();
let cachedSolUsdPrice: number | undefined;
let cachedSolUsdPriceAt = 0;
const metadataCache = new Map<string, LooseRecord | null>();
const tokenInfoCache = new Map<string, LooseRecord | null>();
const trailingSellTimers = new Set<NodeJS.Timeout>();
const activeTrailingSellSchedules = new Set<string>();
const copyBuySubmissionGuard = createCopyBuySubmissionGuard();
const copyTradeDailySolBudget = createInMemoryCopyTradeDailySolBudget();
let isShuttingDown = false;
let copyTradeEmergencyStopped = false;
const pumpFunCoinInfoRetryDelaysMs = [0, 750, 1500, 3000, 6000];
const subscribers =
  config.supabaseUrl && config.supabaseServiceRoleKey
    ? createSupabaseSubscriberStoreFromEnv({
        url: config.supabaseUrl,
        serviceRoleKey: config.supabaseServiceRoleKey,
        initialChatIds: [config.telegramChatId]
      })
    : createSubscriberStore({
        path: config.telegramSubscribersPath,
        initialChatIds: [config.telegramChatId]
      });
const subscriberStoreLabel = config.supabaseUrl && config.supabaseServiceRoleKey ? "Supabase" : "JSON";
const copyTradeRecorder =
  config.supabaseUrl && config.supabaseServiceRoleKey
    ? createSupabaseCopyTradeRecorderFromEnv({
        url: config.supabaseUrl,
        serviceRoleKey: config.supabaseServiceRoleKey
      })
    : null;
const copyTradeBuyIdempotency =
  config.supabaseUrl && config.supabaseServiceRoleKey
    ? createSupabaseCopyTradeBuyIdempotencyStore({
        url: config.supabaseUrl,
        serviceRoleKey: config.supabaseServiceRoleKey
      })
    : createJsonCopyTradeBuyIdempotencyStore({
        path: process.env.COPY_TRADE_BUY_IDEMPOTENCY_PATH || "data/copytrade-buy-idempotency.json"
      });

const baseSubscriptionMethods: PumpPortalSubscription[] = ["subscribeNewToken", "subscribeMigration"];

function copyTradeBuyExecutionSettings(subscriber: SubscriberRecord): { slippage: number; priorityFee: number } {
  return {
    slippage: subscriber.copyTradeBuySlippagePercent ?? config.copyTradeSlippage,
    priorityFee: subscriber.copyTradeBuyPriorityFeeSol ?? config.copyTradePriorityFee
  };
}

function copyTradeSellExecutionSettings(subscriber: SubscriberRecord): { slippage: number; priorityFee: number } {
  return {
    slippage: subscriber.copyTradeSellSlippagePercent ?? config.copyTradeSlippage,
    priorityFee: subscriber.copyTradeSellPriorityFeeSol ?? config.copyTradePriorityFee
  };
}

function copyTradeExecutionModeConfig(): BotConfig & { copyTradeEmergencyStopped: boolean } {
  return {
    ...config,
    copyTradeEmergencyStopped
  };
}

function copyTradeLatencyMode(): "dry" | "live" {
  return copyTradeLiveExecutionEnabled(copyTradeExecutionModeConfig()) ? "live" : "dry";
}

function logCopyTradeExecutionState(): void {
  const modeConfig = copyTradeExecutionModeConfig();
  const message = formatCopyTradeExecutionStateLog(modeConfig);

  if (copyTradeLiveExecutionEnabled(modeConfig)) {
    console.warn(message);
  } else {
    console.log(message);
  }

  console.log(formatCopyTradeRiskControlLog(config));
}

async function loadCopyTradeEmergencyStop(): Promise<void> {
  try {
    const state = asRecord(JSON.parse(await readFile(config.copyTradeEmergencyStopPath, "utf8")));
    copyTradeEmergencyStopped = state.active === true;
  } catch (error) {
    if (isRecord(error) && error.code === "ENOENT") {
      copyTradeEmergencyStopped = false;
      return;
    }

    throw error;
  }
}

async function persistCopyTradeEmergencyStop(chatId: string | number): Promise<void> {
  const state = {
    active: true,
    activatedByChatId: String(chatId),
    activatedAt: new Date().toISOString()
  };

  await mkdir(dirname(config.copyTradeEmergencyStopPath), { recursive: true });
  await writeFile(config.copyTradeEmergencyStopPath, `${JSON.stringify(state, null, 2)}\n`, "utf8");
}

async function activateCopyTradeEmergencyStop(chatId: string | number): Promise<string> {
  copyTradeEmergencyStopped = true;
  await persistCopyTradeEmergencyStop(chatId);
  const message = formatCopyTradeExecutionStateLog(copyTradeExecutionModeConfig());

  console.warn(`Copy trade emergency stop activated by ${chatId}: ${message}`);
  return message;
}

function logCopyTradeLatency(
  tracker: CopyTradeLatencyTracker,
  details?: CopyTradeLatencyMilestoneDetails
): void {
  console.log(`Copy trade latency: ${JSON.stringify(tracker.format(details))}`);
}

function watchedWalletAddresses(): string[] {
  return [
    ...new Set(
      subscribers
        .list()
        .flatMap((subscriber) => [...(subscriber.watchedWallets || []), ...(subscriber.copyTradeWallets || [])])
        .map((wallet) => wallet.address)
        .filter(Boolean)
    )
  ].sort();
}

function copyTradeWalletAddresses(): string[] {
  return [
    ...new Set(
      subscribers
        .list()
        .flatMap((subscriber) => subscriber.copyTradeWallets || [])
        .map((wallet) => wallet.address)
        .filter(Boolean)
    )
  ].sort();
}

function activePumpPortalSubscriptions(): PumpPortalSubscription[] {
  const copyTradeWallets = copyTradeWalletAddresses();

  return [
    ...baseSubscriptionMethods,
    ...(copyTradeWallets.length > 0
      ? [
          {
            method: "subscribeAccountTrade",
            keys: copyTradeWallets
          }
        ]
      : [])
  ];
}

function activeSubscriptionMethodNames(): string[] {
  return activePumpPortalSubscriptions().map((subscription) =>
    typeof subscription === "string"
      ? subscription
      : `${subscription.method}${subscription.keys?.length ? `(${subscription.keys.length})` : ""}`
  );
}

function rememberEvent(id: string | null): boolean {
  if (!id) {
    return false;
  }

  if (seenEvents.has(id)) {
    return true;
  }

  seenEvents.add(id);

  if (seenEvents.size > 1000) {
    const oldest = seenEvents.values().next().value;
    if (oldest) {
      seenEvents.delete(oldest);
    }
  }

  return false;
}

async function handleMigration(event: LooseRecord): Promise<void> {
  const eventMode = classifyEventMode(event);

  if (!eventMode) {
    return;
  }

  const recipients = subscribers.list().filter((subscriber) => shouldSendEventToSubscriber(eventMode, subscriber));

  if (recipients.length === 0) {
    return;
  }

  const eventId = getEventId(event);
  if (rememberEvent(eventId)) {
    return;
  }

  const [solUsdPrice, tokenInfo, transactionAnalysis] = await Promise.all([
    getSolUsdPrice(),
    getPumpFunCoinInfo(event),
    getTransactionAnalysis(event)
  ]);
  const metadata = await getTokenMetadata(event, tokenInfo);
  const eventConfig = {
    ...config,
    alertModeLabel: "New tokens and migrated coins",
    activeSubscriptionMethods: activeSubscriptionMethodNames(),
    solUsdPrice,
    tokenInfo,
    metadata,
    transactionAnalysis
  };
  const migration = extractMigrationData(event, eventConfig);
  await writeMigrationLog(migration);
  console.log(`PumpPortal event: ${JSON.stringify(migration)}`);

  const text = formatMigrationMessage(event, eventConfig);
  const replyMarkup = buildMigrationReplyMarkup(event, eventConfig);

  await sendAlertToSubscribers({ subscribers: recipients, migration, text, replyMarkup });
}

async function handlePumpPortalEvent(event: LooseRecord): Promise<void> {
  if (await handlePumpPortalAccountTrade(event)) {
    return;
  }

  const eventMode = classifyEventMode(event);

  if (eventMode) {
    await handleMigration(event);
    return;
  }

  console.warn(`Skipping unknown PumpPortal event type: ${JSON.stringify(event)}`);
}

function walletTradeSeenEventId(trade: WalletTradeData): string | null {
  return trade.signature
    ? ["wallet-trade-source", trade.signature, trade.targetWallet].join(":")
    : getWalletTradeEventId(trade);
}

function pumpPortalAccountTradeAction(event: LooseRecord): "buy" | "sell" | null {
  const action = String(event.txType ?? event.type ?? event.eventType ?? event.action ?? "").toLowerCase();

  return action === "buy" || action === "sell" ? action : null;
}

function pumpPortalAccountTradeSource(event: LooseRecord): string {
  const source = stringValue(event.source);
  const pool = stringValue(event.pool);

  if (source) {
    return source;
  }

  if (pool && pool.toLowerCase().startsWith("pump")) {
    return "PUMP_FUN";
  }

  return "PUMPPORTAL_ACCOUNT_TRADE";
}

function normalizePumpPortalAccountTradeData({
  event,
  targetWallet,
  label
}: {
  event: LooseRecord;
  targetWallet: string;
  label?: string | null;
}): WalletTradeData | null {
  const action = pumpPortalAccountTradeAction(event);
  const trader = stringValue(event.traderPublicKey || event.trader || event.user || event.account || event.wallet);
  const mint = stringValue(event.mint || event.ca || event.token || event.tokenAddress || event.address);
  const signature = stringValue(event.signature || event.tx || event.txHash || event.transaction || event.transactionHash);

  if (!action || trader !== targetWallet || !mint || !signature) {
    return null;
  }

  const timestamp = finiteNumberValue(event.timestamp || event.blockTime || event.time) ?? Math.floor(Date.now() / 1000);
  const solAmount = finiteNumberValue(event.solAmount);
  const tokenAmount = finiteNumberValue(event.tokenAmount);
  const pool = stringValue(event.pool);
  const source = pumpPortalAccountTradeSource(event);
  const input = action === "buy"
    ? { mint: "So11111111111111111111111111111111111111112", symbol: "SOL", amount: solAmount }
    : { mint, symbol: null, amount: tokenAmount };
  const output = action === "buy"
    ? { mint, symbol: null, amount: tokenAmount }
    : { mint: "So11111111111111111111111111111111111111112", symbol: "SOL", amount: solAmount };

  return {
    observedAt: new Date().toISOString(),
    provider: "pumpportal",
    targetWallet,
    label: label || null,
    action,
    mint,
    signature,
    timestamp,
    feePayer: trader,
    source,
    input,
    output,
    solAmount,
    tokenAmount,
    pool,
    marketCapSol: finiteNumberValue(event.marketCapSol || event.marketCap),
    pumpFunUrl: `${config.pumpFunBaseUrl}/${mint}`,
    solscanTokenUrl: `${config.solscanBaseUrl}/token/${mint}`,
    solscanTxUrl: signature ? `${config.solscanBaseUrl}/tx/${signature}` : null,
    raw: {
      ...event,
      pumpPortalAccountTradeParser: {
        action,
        copyable: action === "buy"
      }
    }
  };
}

async function handlePumpPortalAccountTrade(event: LooseRecord): Promise<boolean> {
  const action = pumpPortalAccountTradeAction(event);

  if (!action) {
    return false;
  }

  const trader = stringValue(event.traderPublicKey || event.trader || event.user || event.account || event.wallet);

  if (!trader) {
    return false;
  }

  const entries = subscribers
    .list()
    .flatMap((subscriber) =>
      (subscriber.watchedWallets || [])
        .filter((wallet) => wallet.address === trader)
        .map((wallet) => ({ subscriber, wallet, label: wallet.label }))
    );
  const copyTradeEntries = subscribers
    .list()
    .flatMap((subscriber) =>
      (subscriber.copyTradeWallets || [])
        .filter((wallet) => wallet.address === trader)
        .map((wallet) => ({ subscriber, wallet, label: wallet.label }))
    );

  if (entries.length === 0 && copyTradeEntries.length === 0) {
    return false;
  }

  const trade = normalizePumpPortalAccountTradeData({
    event,
    targetWallet: trader,
    label: null
  });

  if (!trade) {
    return false;
  }

  const eventId = walletTradeSeenEventId(trade);

  if (rememberEvent(eventId)) {
    return true;
  }

  await writeWalletTradeLog(trade);
  console.log(`Wallet trade event: ${JSON.stringify(trade)}`);

  const receivedAtMs = Date.now();
  await Promise.all(copyTradeEntries.map((entry) =>
    sendCopyTradeSimulationAlert(
      entry.subscriber,
      {
        ...trade,
        label: entry.label
      },
      entry.wallet,
      {
        receivedAtMs,
        normalizedAtMs: receivedAtMs
      }
    )
  ));

  await Promise.all(entries.map((entry) =>
    sendWalletTradeAlert(entry.subscriber, {
      ...trade,
      label: entry.label
    })
  ));

  return true;
}

async function handleHeliusWebhookEvents(events: LooseRecord[]): Promise<void> {
  const receivedAtMs = Date.now();

  for (const event of events) {
    if (!isHeliusSwapEvent(event)) {
      continue;
    }

    await handleHeliusSwap(event, { receivedAtMs });
  }
}

async function handleHeliusSwap(event: LooseRecord, { receivedAtMs = Date.now() }: { receivedAtMs?: number } = {}): Promise<boolean> {
  const subscribersByWallet = new Map<string, Array<{ subscriber: SubscriberRecord; label: string | null }>>();
  const copyTradeSubscribersByWallet = new Map<string, Array<{ subscriber: SubscriberRecord; wallet: WatchedWallet; label: string | null }>>();

  for (const subscriber of subscribers.list()) {
    for (const wallet of subscriber.watchedWallets || []) {
      if (!heliusEventMentionsWatchedWallet(event, wallet.address)) {
        continue;
      }

      const entries = subscribersByWallet.get(wallet.address) || [];
      entries.push({ subscriber, label: wallet.label });
      subscribersByWallet.set(wallet.address, entries);
    }

    for (const wallet of subscriber.copyTradeWallets || []) {
      if (!heliusEventMentionsWatchedWallet(event, wallet.address)) {
        continue;
      }

      const entries = copyTradeSubscribersByWallet.get(wallet.address) || [];
      entries.push({ subscriber, wallet, label: wallet.label });
      copyTradeSubscribersByWallet.set(wallet.address, entries);
    }
  }

  const targetWallets = [...new Set([...subscribersByWallet.keys(), ...copyTradeSubscribersByWallet.keys()])].sort();

  if (targetWallets.length === 0) {
    return false;
  }

  for (const targetWallet of targetWallets) {
    const loggedTrade = normalizeHeliusSwapData({
      event,
      targetWallet,
      label: null,
      config
    });
    const normalizedAtMs = Date.now();
    const eventId = walletTradeSeenEventId(loggedTrade);

    if (rememberEvent(eventId)) {
      continue;
    }

    await writeWalletTradeLog(loggedTrade);
    console.log(`Wallet trade event: ${JSON.stringify(loggedTrade)}`);

    const copyTradeEntries = copyTradeSubscribersByWallet.get(targetWallet) || [];
    await Promise.all(copyTradeEntries.map((entry) =>
      sendCopyTradeSimulationAlert(
        entry.subscriber,
        {
          ...loggedTrade,
          label: entry.label
        },
        entry.wallet,
        {
          receivedAtMs,
          normalizedAtMs
        }
      )
    ));

    const entries = subscribersByWallet.get(targetWallet) || [];
    await Promise.all(entries.map((entry) =>
      sendWalletTradeAlert(entry.subscriber, {
        ...loggedTrade,
        label: entry.label
      })
    ));
  }

  return true;
}

async function sendWalletTradeAlert(subscriber: SubscriberRecord, trade: WalletTradeData): Promise<void> {
  try {
    await sendTelegramMessage({
      token: config.telegramToken,
      chatId: subscriber.chatId,
      text: formatWalletTradeMessageWithCopySettings(trade, null),
      replyMarkup: buildWalletTradeReplyMarkup(trade)
    });
  } catch (error) {
    console.warn(`Could not send wallet trade alert to ${subscriber.chatId}: ${errorMessage(error)}`);
  }
}

async function sendCopyTradeSimulationAlert(
  subscriber: SubscriberRecord,
  trade: WalletTradeData,
  copyTradeWallet: WatchedWallet,
  timing: { receivedAtMs?: number; normalizedAtMs?: number } = {}
): Promise<void> {
  const latencyTracker = createCopyTradeLatencyTracker({
    chatId: subscriber.chatId,
    sourceWallet: copyTradeWallet.address,
    tradingWallet: subscriber.tradingWallet?.publicKey || null,
    observedSignature: trade.signature,
    mint: trade.mint,
    mode: copyTradeLatencyMode()
  }, {
    clock: createCopyTradeLatencyClock(timing)
  });
  let latencyLogged = false;
  const logLatencyOnce = (details?: CopyTradeLatencyMilestoneDetails): void => {
    if (latencyLogged) {
      return;
    }

    logCopyTradeLatency(latencyTracker, details);
    latencyLogged = true;
  };
  const skipWithLatencyLog = (reason: string): void => {
    latencyTracker.skip(reason, { status: "skipped" });
    logLatencyOnce();
  };

  latencyTracker.mark("normalized");

  if (!isCopyableSolToTokenBuy(trade)) {
    skipWithLatencyLog("trade is not a copyable SOL-to-token buy");
    return;
  }

  if (!subscriber.copyAmountSol) {
    skipWithLatencyLog("copy amount is not configured");
    return;
  }

  if (!subscriber.tradingWallet) {
    skipWithLatencyLog("trading wallet is not configured");
    return;
  }

  const copyBuyKey = copyBuySubmissionKey({
    chatId: subscriber.chatId,
    tradingWalletPublicKey: subscriber.tradingWallet.publicKey,
    sourceWalletAddress: copyTradeWallet.address,
    observedSignature: trade.signature
  });
  let copyBuyReserved = false;
  let durableCopyBuyClaimKey: string | null = null;
  let result: PumpPortalLightningTradeResult | null = null;

  try {
    const executionSettings = copyTradeBuyExecutionSettings(subscriber);
    const request = buildPumpPortalLightningBuyRequest({
      trade,
      amountSol: subscriber.copyAmountSol,
      slippage: executionSettings.slippage,
      priorityFee: executionSettings.priorityFee,
      pool: config.copyTradePool
    });

    if (!request) {
      skipWithLatencyLog("could not build PumpPortal buy request");
      return;
    }
    latencyTracker.mark("request_built");

    const amountSol = typeof request.amount === "number" ? request.amount : null;
    const dailyBudgetKey = copyTradeDailyBudgetKey({
      chatId: subscriber.chatId,
      tradingWalletPublicKey: subscriber.tradingWallet.publicKey
    });
    const nowMs = Date.now();
    const riskBlockedReason = copyTradeBuyRiskBlockedReason({
      config,
      request,
      trade,
      copyTradeWalletCount: subscriber.copyTradeWallets.length,
      dailySpentSol: copyTradeDailySolBudget.spentSol({ key: dailyBudgetKey, nowMs }),
      nowMs
    });
    latencyTracker.mark("risk_checked", {
      status: riskBlockedReason ? "blocked" : "ok",
      reason: riskBlockedReason
    });
    if (riskBlockedReason) {
      skipWithLatencyLog(riskBlockedReason);
      await notifySkippedAutoCopyBuy({
        subscriber,
        trade,
        copyTradeWallet,
        request,
        reason: riskBlockedReason
      });
      return;
    }

    if (amountSol === null) {
      skipWithLatencyLog("copy buy amount is not a fixed SOL amount");
      await notifySkippedAutoCopyBuy({
        subscriber,
        trade,
        copyTradeWallet,
        request,
        reason: "copy buy amount is not a fixed SOL amount"
      });
      return;
    }

    const blockedReason = copyTradeLiveExecutionBlockedReason(copyTradeExecutionModeConfig());
    latencyTracker.mark("live_gate_checked", {
      status: blockedReason ? "blocked" : "ok",
      reason: blockedReason
    });
    if (blockedReason) {
      skipWithLatencyLog(blockedReason);
      await notifySkippedAutoCopyBuy({
        subscriber,
        trade,
        copyTradeWallet,
        request,
        reason: blockedReason
      });
      return;
    }

    if (config.copyTradeMinWalletReserveSol > 0) {
      let tradingWalletBalanceSol: number | null = null;

      try {
        tradingWalletBalanceSol = await getSolanaBalanceSol({
          address: subscriber.tradingWallet.publicKey,
          rpcUrl: config.solanaRpcUrl
        });
      } catch (error) {
        console.warn(`Could not fetch trading wallet balance for ${subscriber.chatId}: ${errorMessage(error)}`);
      }

      const reserveBlockedReason = copyTradeWalletReserveBlockedReason({
        config,
        request,
        tradingWalletBalanceSol
      });
      latencyTracker.mark("balance_checked", {
        status: reserveBlockedReason ? "blocked" : "ok",
        reason: reserveBlockedReason
      });

      if (reserveBlockedReason) {
        skipWithLatencyLog(reserveBlockedReason);
        await notifySkippedAutoCopyBuy({
          subscriber,
          trade,
          copyTradeWallet,
          request,
          reason: reserveBlockedReason
        });
        return;
      }
    }

    if (!encryptionSecretReady(config.pumpPortalWalletKeyEncryptionSecret)) {
      const reason = "missing PUMPPORTAL_WALLET_KEY_ENCRYPTION_SECRET";
      skipWithLatencyLog(reason);
      console.warn(`Skipping auto copy buy for ${subscriber.chatId}: ${reason}`);
      return;
    }

    if (!copyBuySubmissionGuard.reserve(copyBuyKey)) {
      skipWithLatencyLog("copy buy is already in flight");
      console.warn(
        `Skipping duplicate auto copy buy for ${subscriber.chatId}:${copyTradeWallet.address}:${trade.signature}: already in flight`
      );
      return;
    }
    copyBuyReserved = true;

    const finalBlockedReason = copyTradeLiveExecutionBlockedReason(copyTradeExecutionModeConfig());
    if (finalBlockedReason) {
      skipWithLatencyLog(finalBlockedReason);
      await notifySkippedAutoCopyBuy({
        subscriber,
        trade,
        copyTradeWallet,
        request,
        reason: finalBlockedReason
      });
      return;
    }

    const durableCopyBuyKey = copyBuyKey && trade.mint ? [copyBuyKey, trade.mint, "buy"].join(":") : null;

    if (!durableCopyBuyKey || !trade.signature || !trade.mint) {
      const reason = "copy buy idempotency key is unavailable";
      skipWithLatencyLog(reason);
      await notifySkippedAutoCopyBuy({
        subscriber,
        trade,
        copyTradeWallet,
        request,
        reason
      });
      return;
    }

    let idempotencyClaim;
    try {
      idempotencyClaim = await copyTradeBuyIdempotency.claimBuy({
        key: durableCopyBuyKey,
        chatId: subscriber.chatId,
        sourceWalletAddress: copyTradeWallet.address,
        tradingWalletPublicKey: subscriber.tradingWallet.publicKey,
        observedSignature: trade.signature,
        mint: trade.mint,
        amountSol,
        provider: trade.provider,
        request
      });
    } catch (error) {
      const reason = `copy buy idempotency claim failed: ${errorMessage(error)}`;
      skipWithLatencyLog(reason);
      await notifySkippedAutoCopyBuy({
        subscriber,
        trade,
        copyTradeWallet,
        request,
        reason
      });
      return;
    }
    if (!idempotencyClaim.claimed) {
      const existing = idempotencyClaim.existing;
      const reason = existing
        ? `copy buy was already claimed (${existing.status})`
        : "copy buy was already claimed";
      skipWithLatencyLog(reason);
      console.warn(
        `Skipping duplicate auto copy buy for ${subscriber.chatId}:${copyTradeWallet.address}:${trade.signature}: durable idempotency claimed`
      );
      await notifySkippedAutoCopyBuy({
        subscriber,
        trade,
        copyTradeWallet,
        request,
        reason
      });
      return;
    }
    durableCopyBuyClaimKey = durableCopyBuyKey;

    const budgetReservation = copyTradeDailySolBudget.reserve({
      key: dailyBudgetKey,
      amountSol,
      capSol: config.copyTradeDailySolCap,
      nowMs: Date.now()
    });
    if (!budgetReservation.ok) {
      const reason = budgetReservation.reason || "daily copy buy budget exceeded";
      await safelyFailCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, reason);
      skipWithLatencyLog(reason);
      await notifySkippedAutoCopyBuy({
        subscriber,
        trade,
        copyTradeWallet,
        request,
        reason
      });
      return;
    }

    const apiKey = decryptSecret(subscriber.tradingWallet.encryptedApiKey, config.pumpPortalWalletKeyEncryptionSecret || "");
    latencyTracker.mark("submit_started");
    result = await executePumpPortalLightningTrade({
      url: config.pumpPortalLightningTradeUrl,
      apiKey,
      request
    });
    await safelyCompleteCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, result);
    latencyTracker.mark("submit_finished", {
      status: result.ok ? "submitted" : "failed",
      reason: result.ok ? null : result.errorText,
      signature: result.signature
    });
    logLatencyOnce();

    if (result.ok) {
      scheduleCopyTradeTrailingSellsAfterConfirmation({
        subscriber,
        trade,
        apiKey,
        copyTradeWallet,
        buySignature: result.signature
      }).catch((error) => {
        console.warn(`Could not prepare trailing sells after copy buy confirmation: ${errorMessage(error)}`);
      });
    }

    await recordCopyTradeExecution({
      subscriber,
      trade,
      copyTradeWallet,
      tradingWalletPublicKey: subscriber.tradingWallet.publicKey,
      request,
      result
    });

    const message = formatAutoCopyBuyMessage({
      trade,
      tradingWalletPublicKey: subscriber.tradingWallet.publicKey,
      copyAmountSol: subscriber.copyAmountSol,
      result
    });

    if (message) {
      await sendTelegramMessage({
        token: config.telegramToken,
        chatId: subscriber.chatId,
        text: message,
        replyMarkup: buildWalletTradeReplyMarkup(trade)
      });
    }

  } catch (error) {
    if (durableCopyBuyClaimKey && !result) {
      await safelyFailCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, errorMessage(error));
    }
    latencyTracker.skip(errorMessage(error), { status: "failed" });
    logLatencyOnce();
    console.warn(`Could not execute auto copy buy for ${subscriber.chatId}: ${errorMessage(error)}`);
  } finally {
    if (copyBuyReserved) {
      copyBuySubmissionGuard.release(copyBuyKey);
    }
  }
}

async function scheduleCopyTradeTrailingSellsAfterConfirmation({
  subscriber,
  trade,
  apiKey,
  copyTradeWallet,
  buySignature
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  apiKey: string;
  copyTradeWallet: WatchedWallet;
  buySignature: string | null;
}): Promise<void> {
  if (!buySignature) {
    await notifySkippedTrailingSellSchedule({
      subscriber,
      trade,
      reason: "copy buy did not return a transaction signature"
    });
    return;
  }

  const confirmed = await waitForSignatureConfirmation(buySignature);
  if (!confirmed) {
    await notifySkippedTrailingSellSchedule({
      subscriber,
      trade,
      reason: "copy buy was not confirmed before trailing sell scheduling timeout"
    });
    return;
  }

  await scheduleCopyTradeTrailingSells({
    subscriber,
    trade,
    apiKey,
    copyTradeWallet
  });
}

async function notifySkippedTrailingSellSchedule({
  subscriber,
  trade,
  reason
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  reason: string;
}): Promise<void> {
  console.warn(`Skipping trailing sell schedule for ${subscriber.chatId}:${trade.mint || "unknown"}: ${reason}`);
  const message = formatCopyTradeTrailingSellSkippedMessage({ trade, reason });
  if (!message) {
    return;
  }

  try {
    await sendTelegramMessage({
      token: config.telegramToken,
      chatId: subscriber.chatId,
      text: message,
      replyMarkup: buildWalletTradeReplyMarkup(trade)
    });
  } catch (error) {
    console.warn(`Could not send skipped trailing sell schedule alert to ${subscriber.chatId}: ${errorMessage(error)}`);
  }
}

async function notifySkippedAutoCopyBuy({
  subscriber,
  trade,
  copyTradeWallet,
  request,
  reason
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  copyTradeWallet: WatchedWallet;
  request: PumpPortalLightningTradeRequest;
  reason: string;
}): Promise<void> {
  const sourceWallet = copyTradeWallet.label
    ? `${copyTradeWallet.label} (${copyTradeWallet.address})`
    : copyTradeWallet.address;
  console.warn(
    [
      `Skipping auto copy buy for ${subscriber.chatId}: ${reason}`,
      `intended PumpPortal buy ${request.amount} SOL of ${request.mint}`,
      `source wallet ${sourceWallet}`,
      `trading wallet ${subscriber.tradingWallet?.publicKey || "unknown"}`,
      `observed tx ${trade.signature || "unknown"}`
    ].join("; ")
  );

  const message = formatSkippedAutoCopyBuyMessage({ subscriber, trade, reason });
  if (!message) {
    return;
  }

  try {
    await sendTelegramMessage({
      token: config.telegramToken,
      chatId: subscriber.chatId,
      text: message,
      replyMarkup: buildWalletTradeReplyMarkup(trade)
    });
  } catch (error) {
    console.warn(`Could not send skipped auto copy buy alert to ${subscriber.chatId}: ${errorMessage(error)}`);
  }
}

function formatSkippedAutoCopyBuyMessage({
  subscriber,
  trade,
  reason
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  reason: string;
}): string | null {
  const simulationMessage = formatCopyTradeSimulationMessage(
    trade,
    {
      copyWalletAddress: subscriber.tradingWallet?.publicKey || null,
      copyAmountSol: subscriber.copyAmountSol
    },
    null
  );

  if (!simulationMessage) {
    return null;
  }

  const modeConfig = copyTradeExecutionModeConfig();
  const mode = copyTradeLiveExecutionEnabled(modeConfig)
    ? "Copy trade risk controls blocked this order"
    : modeConfig.copyTradeEmergencyStopped
      ? "Copy trade emergency stop is active"
      : config.copyTradeEnabled
      ? "Copy trade dry run is active"
      : "Live copy trading is disabled";

  return [
    simulationMessage,
    "",
    `🟡 ${mode}; no PumpPortal order was submitted.`,
    `<b>Reason:</b> <code>${reason}</code>`
  ].join("\n");
}

function buildTrailingSellSchedule({
  subscriber,
  trade,
  trailingSellConfig
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  trailingSellConfig: TrailingSellConfig | null;
}): Array<{ delayMs: number; request: PumpPortalLightningTradeRequest }> {
  if (!trailingSellConfig?.enabled || !trade.mint) {
    return [];
  }

  const sellPercents = trailingSellConfig.percentBasis === "original_position"
    ? originalPositionPercentsToRemainingBalancePercents(trailingSellConfig.steps)
    : trailingSellConfig.steps;
  const executionSettings = copyTradeSellExecutionSettings(subscriber);

  return sellPercents
    .map((step) => {
      const request = buildPumpPortalLightningSellRequest({
        mint: trade.mint || "",
        amountPercent: step.percent,
        slippage: executionSettings.slippage,
        priorityFee: executionSettings.priorityFee,
        pool: config.copyTradePool
      });

      if (!request) {
        return null;
      }

      return {
        delayMs: step.delayMs,
        request
      };
    })
    .filter((step): step is { delayMs: number; request: PumpPortalLightningTradeRequest } => Boolean(step));
}

function inheritedTrailingSellConfig(): TrailingSellConfig | null {
  if (!config.copyTradeTrailingSellEnabled) {
    return null;
  }

  return {
    enabled: true,
    mode: "formula",
    percentBasis: "remaining_balance",
    steps: trailingSellPercents({
      firstPercent: config.copyTradeTrailingSellFirstPercent,
      trailPercent: config.copyTradeTrailingSellTrailPercent,
      maxBuilds: config.copyTradeTrailingSellMaxBuilds
    }).map((percent, index) => ({
      percent,
      delayMs: config.copyTradeTrailingSellHoldMs + config.copyTradeTrailingSellIntervalMs * index
    })),
    updatedAt: new Date().toISOString()
  };
}

function resolveTrailingSellConfig(copyTradeWallet: WatchedWallet): TrailingSellConfig | null {
  return copyTradeWallet.trailingSellConfig || inheritedTrailingSellConfig();
}

function trailingSellPercents({
  firstPercent,
  trailPercent,
  maxBuilds
}: {
  firstPercent: number;
  trailPercent: number;
  maxBuilds: number;
}): number[] {
  if (maxBuilds <= 1) {
    return [100];
  }

  const middleBuilds = Math.max(0, maxBuilds - 2);
  return [firstPercent, ...Array.from({ length: middleBuilds }, () => trailPercent), 100];
}

function originalPositionPercentsToRemainingBalancePercents(steps: TrailingSellStep[]): TrailingSellStep[] {
  let remainingOriginalPercent = 100;

  return steps
    .map((step) => {
      if (remainingOriginalPercent <= 0) {
        return null;
      }

      const originalPercentToSell = Math.min(step.percent, remainingOriginalPercent);
      const remainingBalancePercent = Math.min(100, (originalPercentToSell / remainingOriginalPercent) * 100);
      remainingOriginalPercent -= originalPercentToSell;

      return {
        delayMs: step.delayMs,
        percent: Number(remainingBalancePercent.toFixed(6))
      };
    })
    .filter((step): step is TrailingSellStep => step !== null && step.percent > 0);
}

async function scheduleCopyTradeTrailingSells({
  subscriber,
  trade,
  apiKey,
  copyTradeWallet
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  apiKey: string;
  copyTradeWallet: WatchedWallet;
}): Promise<void> {
  const trailingSellConfig = resolveTrailingSellConfig(copyTradeWallet);
  const steps = buildTrailingSellSchedule({ subscriber, trade, trailingSellConfig });

  if (steps.length === 0) {
    return;
  }

  const scheduleKey = trailingSellScheduleKey({ subscriber, trade });
  if (activeTrailingSellSchedules.has(scheduleKey)) {
    console.warn(`Skipping overlapping trailing sell schedule for ${subscriber.chatId}:${trade.mint}: schedule already active`);
    return;
  }
  activeTrailingSellSchedules.add(scheduleKey);

  const scheduledMessage = formatCopyTradeTrailingSellScheduledMessage({
    trade,
    steps
  });

  if (scheduledMessage) {
    try {
      await sendTelegramMessage({
        token: config.telegramToken,
        chatId: subscriber.chatId,
        text: scheduledMessage,
        replyMarkup: buildWalletTradeReplyMarkup(trade)
      });
    } catch (error) {
      console.warn(`Could not send trailing sell schedule alert to ${subscriber.chatId}: ${errorMessage(error)}`);
    }
  }

  runTrailingSellSchedule({
    subscriber,
    trade,
    apiKey,
    steps,
    copyTradeWallet,
    scheduleKey
  }).catch((error) => {
    console.warn(`Trailing sell schedule failed for ${subscriber.chatId}: ${errorMessage(error)}`);
  });
}

function trailingSellScheduleKey({ subscriber, trade }: { subscriber: SubscriberRecord; trade: WalletTradeData }): string {
  return [
    subscriber.chatId,
    subscriber.tradingWallet?.publicKey || "unknown-wallet",
    trade.mint || "unknown-mint"
  ].join(":");
}

async function runTrailingSellSchedule({
  subscriber,
  trade,
  apiKey,
  steps,
  copyTradeWallet,
  scheduleKey
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  apiKey: string;
  steps: Array<{ delayMs: number; request: PumpPortalLightningTradeRequest }>;
  copyTradeWallet: WatchedWallet;
  scheduleKey: string;
}): Promise<void> {
  const startedAt = Date.now();
  const seenSellSignatures = new Set<string>();

  try {
    for (const [stepIndex, step] of steps.entries()) {
      if (isShuttingDown) {
        return;
      }

      const waitMs = Math.max(0, step.delayMs - (Date.now() - startedAt));
      await trackedDelay(waitMs);

      const result = await buildAndNotifyTrailingSell({
        subscriber,
        trade,
        apiKey,
        request: step.request,
        stepIndex,
        totalSteps: steps.length,
        copyTradeWallet,
        seenSellSignatures
      });

      if (result.ok && result.signature) {
        await waitForSignatureConfirmation(result.signature);
      }
    }
  } finally {
    activeTrailingSellSchedules.delete(scheduleKey);
  }
}

async function buildAndNotifyTrailingSell({
  subscriber,
  trade,
  apiKey,
  request,
  stepIndex,
  totalSteps,
  copyTradeWallet,
  seenSellSignatures
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  apiKey: string;
  request: PumpPortalLightningTradeRequest;
  stepIndex: number;
  totalSteps: number;
  copyTradeWallet: WatchedWallet;
  seenSellSignatures: Set<string>;
}): Promise<PumpPortalLightningTradeResult> {
  const latencyTracker = createCopyTradeLatencyTracker({
    chatId: subscriber.chatId,
    sourceWallet: copyTradeWallet.address,
    tradingWallet: subscriber.tradingWallet?.publicKey || null,
    observedSignature: trade.signature,
    mint: trade.mint,
    mode: copyTradeLatencyMode()
  });
  latencyTracker.mark("request_built", { status: "ok" });

  const requestRiskBlockedReason = copyTradeRequestRiskBlockedReason({ config, request });
  latencyTracker.mark("risk_checked", {
    status: requestRiskBlockedReason ? "blocked" : "ok",
    reason: requestRiskBlockedReason
  });

  const liveBlockedReason = copyTradeLiveExecutionBlockedReason(copyTradeExecutionModeConfig());
  latencyTracker.mark("live_gate_checked", {
    status: liveBlockedReason ? "blocked" : "ok",
    reason: liveBlockedReason
  });

  const blockedReason = liveBlockedReason || requestRiskBlockedReason;
  if (blockedReason) {
    const result: PumpPortalLightningTradeResult = {
      ok: false,
      status: null,
      signature: null,
      errorText: `Trailing sell skipped: ${blockedReason}`,
      raw: null
    };

    latencyTracker.skip(blockedReason, { status: "skipped" });
    logCopyTradeLatency(latencyTracker);
    console.warn(`Skipping trailing sell for ${subscriber.chatId}: ${blockedReason}`);
    await sendTelegramMessage({
      token: config.telegramToken,
      chatId: subscriber.chatId,
      text: formatCopyTradeTrailingSellResultMessage({
        trade,
        stepIndex,
        totalSteps,
        request,
        result
      }),
      replyMarkup: buildWalletTradeReplyMarkup(trade)
    });
    return result;
  }

  latencyTracker.mark("submit_started");
  const { result, duplicateSignature } = await executeTrailingSellWithDuplicateRetry({
    apiKey,
    request,
    seenSellSignatures
  });
  const trailingSellSkipped = Boolean(result.errorText?.startsWith("Trailing sell skipped:"));
  latencyTracker.mark("submit_finished", {
    status: result.ok ? "submitted" : trailingSellSkipped ? "skipped" : "failed",
    reason: result.ok ? null : result.errorText,
    signature: result.signature
  });
  logCopyTradeLatency(latencyTracker);

  await recordCopyTradeExecution({
    subscriber,
    trade,
    copyTradeWallet,
    tradingWalletPublicKey: subscriber.tradingWallet?.publicKey || "",
    request,
    result,
    trailingSellStepIndex: stepIndex,
    trailingSellTotalSteps: totalSteps
  });

  await sendTelegramMessage({
    token: config.telegramToken,
    chatId: subscriber.chatId,
    text: formatCopyTradeTrailingSellResultMessage({
      trade,
      stepIndex,
      totalSteps,
      request,
      result,
      duplicateSignature
    }),
    replyMarkup: buildWalletTradeReplyMarkup(trade)
  });

  if (result.ok && result.signature && !duplicateSignature) {
    seenSellSignatures.add(result.signature);
  }

  return result;
}

async function executeTrailingSellWithDuplicateRetry({
  apiKey,
  request,
  seenSellSignatures
}: {
  apiKey: string;
  request: PumpPortalLightningTradeRequest;
  seenSellSignatures: Set<string>;
}): Promise<{ result: PumpPortalLightningTradeResult; duplicateSignature: boolean }> {
  let duplicateSignature = false;
  let result: PumpPortalLightningTradeResult | null = null;

  for (let attempt = 0; attempt < 12; attempt += 1) {
    const blockedReason = copyTradeLiveExecutionBlockedReason(copyTradeExecutionModeConfig());
    if (blockedReason) {
      result = {
        ok: false,
        status: null,
        signature: null,
        errorText: `Trailing sell skipped: ${blockedReason}`,
        raw: null
      };
      break;
    }

    result = await executePumpPortalLightningTrade({
      url: config.pumpPortalLightningTradeUrl,
      apiKey,
      request
    });
    duplicateSignature = Boolean(result.ok && result.signature && seenSellSignatures.has(result.signature));

    if (isTokenAccountNotReadyError(result) && attempt < 11) {
      console.warn(`PumpPortal could not find token account for trailing sell; retrying after account indexing delay`);
      await trackedDelay(3000);
      continue;
    }

    if (!duplicateSignature || !result.signature || attempt === 2) {
      break;
    }

    console.warn(`PumpPortal returned duplicate trailing sell signature ${result.signature}; retrying after confirmation delay`);
    await waitForSignatureConfirmation(result.signature);
    await trackedDelay(5000);
  }

  return {
    result: result || {
      ok: false,
      status: null,
      signature: null,
      errorText: "Trailing sell request was not submitted",
      raw: null
    },
    duplicateSignature
  };
}

function isTokenAccountNotReadyError(result: PumpPortalLightningTradeResult): boolean {
  return !result.ok && /could not find account|token account balance/i.test(result.errorText || "");
}

function trackedDelay(ms: number): Promise<void> {
  if (ms <= 0) {
    return Promise.resolve();
  }

  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      trailingSellTimers.delete(timer);
      resolve();
    }, ms);
    trailingSellTimers.add(timer);
  });
}

async function waitForSignatureConfirmation(signature: string, timeoutMs = 30000, pollMs = 2000): Promise<boolean> {
  const startedAt = Date.now();

  while (!isShuttingDown && Date.now() - startedAt < timeoutMs) {
    try {
      const response = await fetch(config.solanaRpcUrl, {
        method: "POST",
        headers: {
          "Content-Type": "application/json"
        },
        body: JSON.stringify({
          jsonrpc: "2.0",
          id: signature,
          method: "getSignatureStatuses",
          params: [[signature], { searchTransactionHistory: true }]
        })
      });
      const body = await response.json() as { result?: { value?: Array<{ confirmationStatus?: string; err?: unknown } | null> } };
      const status = body.result?.value?.[0] || null;

      if (status?.err) {
        console.warn(`Signature ${signature} landed with error: ${JSON.stringify(status.err)}`);
        return false;
      }

      if (status?.confirmationStatus === "confirmed" || status?.confirmationStatus === "finalized") {
        return true;
      }
    } catch (error) {
      console.warn(`Could not check trailing sell confirmation for ${signature}: ${errorMessage(error)}`);
      return false;
    }

    await trackedDelay(pollMs);
  }

  console.warn(`Timed out waiting for trailing sell confirmation: ${signature}`);
  return false;
}

async function recordCopyTradeExecution({
  subscriber,
  trade,
  copyTradeWallet,
  tradingWalletPublicKey,
  request,
  result,
  trailingSellStepIndex = null,
  trailingSellTotalSteps = null
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  copyTradeWallet: WatchedWallet;
  tradingWalletPublicKey: string;
  request: PumpPortalLightningTradeRequest;
  result: PumpPortalLightningTradeResult;
  trailingSellStepIndex?: number | null;
  trailingSellTotalSteps?: number | null;
}): Promise<void> {
  if (!copyTradeRecorder || !trade.mint || !tradingWalletPublicKey) {
    return;
  }

  const record: CopyTradeExecutionRecord = {
    chatId: subscriber.chatId,
    sourceWalletAddress: copyTradeWallet.address,
    sourceWalletLabel: copyTradeWallet.label,
    tradingWalletPublicKey,
    mint: trade.mint,
    action: request.action,
    amount: request.amount,
    denominatedInSol: request.denominatedInSol,
    status: result.ok ? "submitted" : "failed",
    signature: result.signature,
    errorText: result.errorText,
    httpStatus: result.status,
    observedTrade: trade,
    request,
    response: result.raw,
    trailingSellStepIndex,
    trailingSellTotalSteps
  };

  try {
    await copyTradeRecorder.recordCopyTradeExecution(record);
  } catch (error) {
    console.warn(`Could not record copy trade execution for ${subscriber.chatId}: ${errorMessage(error)}`);
  }
}

function classifyEventMode(event: LooseRecord): Exclude<AlertModeValue, "both"> | null {
  const eventType = String(event?.txType ?? event?.type ?? event?.eventType ?? "").toLowerCase();

  if (eventType === "create") {
    return "newtokens";
  }

  if (eventType === "migration" || eventType === "migrate") {
    return "migrations";
  }

  if (eventType === "buy" || eventType === "sell") {
    return null;
  }

  if (hasMigrationStyleFields(event)) {
    return "migrations";
  }

  return null;
}

function hasMigrationStyleFields(event: LooseRecord): boolean {
  return Boolean(
    stringValue(event.signature || event.tx || event.txHash || event.transaction || event.transactionHash) &&
      stringValue(event.mint || event.ca || event.token || event.tokenAddress || event.address) &&
      stringValue(event.pool || event.poolAddress || event.raydiumPool || event.poolCandidate)
  );
}

function shouldSendEventToSubscriber(eventMode: Exclude<AlertModeValue, "both">, subscriber: SubscriberRecord): boolean {
  return subscriber.mode === "both" || subscriber.mode === eventMode;
}

async function sendAlertToSubscribers({
  subscribers: recipients,
  migration,
  text,
  replyMarkup
}: {
  subscribers: SubscriberRecord[];
  migration: MigrationData;
  text: string;
  replyMarkup: ReturnType<typeof buildMigrationReplyMarkup>;
}): Promise<void> {
  for (const subscriber of recipients) {
    const chatId = subscriber.chatId;

    if (migration.imageUrl) {
      try {
        await sendTelegramPhoto({
          token: config.telegramToken,
          chatId,
          photoUrl: migration.imageUrl,
          caption: text,
          replyMarkup
        });
        continue;
      } catch (error) {
        console.warn(`Could not send token photo to ${chatId}; sending text alert instead: ${errorMessage(error)}`);
      }
    }

    try {
      await sendTelegramMessage({ token: config.telegramToken, chatId, text, replyMarkup });
    } catch (error) {
      console.warn(`Could not send Telegram alert to ${chatId}: ${errorMessage(error)}`);
    }
  }
}

async function notifySubscribers(text: string): Promise<void> {
  for (const subscriber of subscribers.list()) {
    try {
      await sendTelegramMessage({
        token: config.telegramToken,
        chatId: subscriber.chatId,
        text
      });
    } catch (error) {
      console.warn(`Could not notify Telegram subscriber ${subscriber.chatId}: ${errorMessage(error)}`);
    }
  }
}

function shutdownMessage(): string {
  if (config.shutdownReason === "deploy") {
    return "<b>Bot stopped for an update, please restart the bot.</b>";
  }

  return "<b>Bot stopped.</b>";
}

async function getTransactionAnalysis(event: LooseRecord): Promise<TransactionAnalysis | null> {
  if (!config.transactionFlowEnabled) {
    return null;
  }

  if (!isMigrationEvent(event)) {
    return null;
  }

  const signature = getEventId(event);

  try {
    return await analyzeSolanaTransaction({
      signature,
      rpcUrl: config.solanaRpcUrl,
      event,
      accountLabels: config.transactionAccountLabels
    });
  } catch (error) {
    console.warn(`Could not analyze transaction SOL flow: ${errorMessage(error)}`);
    return null;
  }
}

function isMigrationEvent(event: LooseRecord): boolean {
  return classifyEventMode(event) === "migrations";
}

async function getSolUsdPrice(): Promise<number | undefined> {
  const cacheMs = 60000;

  if (cachedSolUsdPrice && Date.now() - cachedSolUsdPriceAt < cacheMs) {
    return cachedSolUsdPrice;
  }

  try {
    const response = await fetch(config.solUsdPriceUrl);
    const body = asRecord(await response.json());
    const data = asRecord(body.data);
    const solana = asRecord(body.solana);
    const price = Number(data.amount ?? body.price ?? solana.usd);

    if (!response.ok || !Number.isFinite(price)) {
      throw new Error(`Unexpected SOL price response: ${response.status}`);
    }

    cachedSolUsdPrice = price;
    cachedSolUsdPriceAt = Date.now();
    return price;
  } catch (error) {
    console.warn(`Could not fetch SOL/USD price; falling back to SOL market cap: ${errorMessage(error)}`);
    return cachedSolUsdPrice;
  }
}

function normalizeIpfsUrl(value: unknown): unknown {
  if (!value || typeof value !== "string") {
    return value;
  }

  if (value.startsWith("ipfs://ipfs/")) {
    return `https://ipfs.io/ipfs/${value.slice("ipfs://ipfs/".length)}`;
  }

  if (value.startsWith("ipfs://")) {
    return `https://ipfs.io/ipfs/${value.slice("ipfs://".length)}`;
  }

  return value;
}

function pickEventMint(event: LooseRecord): string | null {
  return stringValue(event.mint || event.ca || event.token || event.tokenAddress || event.address);
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function getPumpFunCoinInfo(event: LooseRecord): Promise<LooseRecord | null> {
  const mint = pickEventMint(event);

  if (!mint) {
    return null;
  }

  if (tokenInfoCache.has(mint)) {
    return tokenInfoCache.get(mint) ?? null;
  }

  let lastError: unknown;

  for (const [attempt, delayMs] of pumpFunCoinInfoRetryDelaysMs.entries()) {
    if (delayMs > 0) {
      await sleep(delayMs);
    }

    try {
      const response = await fetch(`${config.pumpFunCoinApiBaseUrl}/${mint}`, {
        headers: {
          accept: "application/json"
        }
      });
      const body = await response.text();
      const tokenInfo = JSON.parse(body) as unknown;

      if (!response.ok || !isRecord(tokenInfo)) {
        throw new Error(`Unexpected Pump.fun coin response: ${response.status}`);
      }

      const normalizedTokenInfo = {
        ...tokenInfo,
        image_uri: normalizeIpfsUrl(tokenInfo.image_uri),
        metadata_uri: normalizeIpfsUrl(tokenInfo.metadata_uri)
      };

      tokenInfoCache.set(mint, normalizedTokenInfo);

      if (tokenInfoCache.size > 500) {
        const oldest = tokenInfoCache.keys().next().value;
        if (oldest) {
          tokenInfoCache.delete(oldest);
        }
      }

      return normalizedTokenInfo;
    } catch (error) {
      lastError = error;
      console.warn(
        `Could not fetch Pump.fun coin info for ${mint} on attempt ${attempt + 1}/${pumpFunCoinInfoRetryDelaysMs.length}: ${errorMessage(error)}`
      );
    }
  }

  console.warn(`Giving up on Pump.fun coin info for ${mint}: ${errorMessage(lastError)}`);
  return null;
}

async function getTokenMetadata(event: LooseRecord, tokenInfo: LooseRecord | null): Promise<LooseRecord | null> {
  const uri = normalizeIpfsUrl(event.uri || event.metadataUri || event.metadata || tokenInfo?.metadata_uri);

  if (!uri || typeof uri !== "string" || !uri.startsWith("http")) {
    return tokenInfo ? metadataFromTokenInfo(tokenInfo) : null;
  }

  if (metadataCache.has(uri)) {
    return metadataCache.get(uri) ?? null;
  }

  try {
    const response = await fetch(uri);
    const metadata = await response.json();

    if (!response.ok || !isRecord(metadata)) {
      throw new Error(`Unexpected metadata response: ${response.status}`);
    }

    const normalizedMetadata = {
      ...metadata,
      image: normalizeIpfsUrl(metadata.image || metadata.image_url || metadata.imageUrl)
    };

    metadataCache.set(uri, normalizedMetadata);

    if (metadataCache.size > 500) {
      const oldest = metadataCache.keys().next().value;
      if (oldest) {
        metadataCache.delete(oldest);
      }
    }

    return {
      ...(metadataFromTokenInfo(tokenInfo) || {}),
      ...normalizedMetadata
    };
  } catch (error) {
    console.warn(`Could not fetch token metadata: ${errorMessage(error)}`);
    metadataCache.set(uri, null);
    return tokenInfo ? metadataFromTokenInfo(tokenInfo) : null;
  }
}

function metadataFromTokenInfo(tokenInfo: LooseRecord | null): LooseRecord | null {
  if (!tokenInfo) {
    return null;
  }

  return {
    name: tokenInfo.name,
    symbol: tokenInfo.symbol,
    description: tokenInfo.description,
    image: normalizeIpfsUrl(tokenInfo.image_uri),
    createdOn: "https://pump.fun"
  };
}

async function writeMigrationLog(migration: MigrationData): Promise<void> {
  await mkdir(dirname(config.migrationLogPath), { recursive: true });
  await appendFile(config.migrationLogPath, `${JSON.stringify(migration)}\n`);
}

async function writeWalletTradeLog(trade: WalletTradeData): Promise<void> {
  await mkdir(dirname(config.walletTradeLogPath), { recursive: true });
  await appendFile(config.walletTradeLogPath, `${JSON.stringify(trade)}\n`);
}

function testMigrationMessage(): string {
  return formatMigrationMessage(
    {
      name: "Test Token",
      symbol: "TEST",
      mint: "So11111111111111111111111111111111111111112",
      signature: "5VfUXexampleTxSignature111111111111111111111111111111111",
      destination: "PumpSwap",
      pool: "PumpSwapPool111111111111111111111111111111111",
      marketCapSol: 28.887,
      uri: "https://ipfs.io/ipfs/QmQKPfCeUFBi2LacnTQJoxmQ2P1dSw6o7cHZeaoVps6JYh",
      txType: "migration"
    },
    {
      ...config,
      solUsdPrice: 180,
      metadata: {
        name: "Test Token",
        symbol: "TEST",
        image: "https://ipfs.io/ipfs/QmTyuok2MLsRxwy2uJWjMgHZuATetVzLk7HfxYC4X9yohw"
      },
      tokenInfo: {
        creator: "CreatorWallet111111111111111111111111111111111",
        is_cashback_enabled: true,
        tokenized_agent: true
      }
    }
  );
}

function assertConfig(): void {
  const missing: string[] = [];

  if (!config.telegramToken) {
    missing.push("TELEGRAM_BOT_TOKEN");
  }

  if (missing.length > 0) {
    throw new Error(`Missing required env vars: ${missing.join(", ")}`);
  }

  if (config.supabaseUrl && !config.supabaseServiceRoleKey) {
    console.warn("SUPABASE_URL is set but no service role key was found; using JSON subscriber storage.");
  }

  if (!config.supabaseUrl && config.supabaseServiceRoleKey) {
    console.warn("A Supabase service role key is set but SUPABASE_URL is missing; using JSON subscriber storage.");
  }
}

const commandPoller = createTelegramCommandPoller({
  config,
  testMessage: testMigrationMessage,
  subscribers,
  onWalletWatchlistChange: () => {
    migrationListener.setSubscriptionMethods(activePumpPortalSubscriptions());
    return syncHeliusWalletWebhook();
  },
  onCopyTradeEmergencyStop: (chatId) => {
    return activateCopyTradeEmergencyStop(chatId);
  }
});
const migrationListener = createPumpPortalMigrationListener({
  pumpPortalWsUrl: config.pumpPortalWsUrl,
  pumpPortalApiKey: config.pumpPortalApiKey,
  subscriptionMethods: activePumpPortalSubscriptions(),
  onMigration: (event) => {
    handlePumpPortalEvent(event).catch((error) => {
      console.error("Failed to process PumpPortal event:", error);
    });
  },
  onStatus: (message: string) => console.log(message),
  onError: (error: Error) => console.error("PumpPortal websocket error:", error.message)
});

const heliusWebhookServer = createHeliusWebhookServer({
  authHeader: config.heliusWebhookAuthHeader,
  port: config.webhookPort,
  onEvents: handleHeliusWebhookEvents
});

async function syncHeliusWalletWebhook(): Promise<string | undefined> {
  let result: Awaited<ReturnType<typeof syncHeliusWebhook>>;

  try {
    result = await syncHeliusWebhook({
      apiKey: config.heliusApiKey,
      apiBaseUrl: config.heliusApiBaseUrl,
      authHeader: config.heliusWebhookAuthHeader,
      publicUrl: config.heliusWebhookPublicUrl,
      webhookId: config.heliusWebhookId,
      statePath: config.heliusWebhookStatePath,
      accountAddresses: watchedWalletAddresses()
    });
  } catch (error) {
    const warning = `Helius webhook sync failed: ${errorMessage(error)}`;
    console.warn(warning);
    return warning;
  }

  if (result.warning) {
    console.warn(result.warning);
    return result.warning;
  }

  if (result.webhookId) {
    console.log(`Helius webhook synced: ${result.webhookId}`);
  }

  return undefined;
}

async function shutdown(): Promise<void> {
  if (isShuttingDown) {
    return;
  }

  isShuttingDown = true;
  console.log("Shutting down");
  commandPoller.stop();
  migrationListener.stop();
  for (const timer of trailingSellTimers) {
    clearTimeout(timer);
  }
  trailingSellTimers.clear();
  await heliusWebhookServer.stop();

  if (config.telegramToken && subscribers.count() > 0) {
    await notifySubscribers(shutdownMessage());
  }

  process.exit(0);
}

process.on("SIGINT", () => {
  shutdown().catch((error) => {
    console.error("Shutdown failed:", error);
    process.exit(1);
  });
});
process.on("SIGTERM", () => {
  shutdown().catch((error) => {
    console.error("Shutdown failed:", error);
    process.exit(1);
  });
});

assertConfig();
await subscribers.init();
await loadCopyTradeEmergencyStop();
console.log(`Using ${subscriberStoreLabel} subscriber storage`);
console.log(`Loaded ${subscribers.count()} verified Telegram subscriber(s)`);
logCopyTradeExecutionState();
// Supabase-backed subscribers load at startup, so compute account-trade
// subscriptions after init rather than from the empty pre-init store.
migrationListener.setSubscriptionMethods(activePumpPortalSubscriptions());
if (watchedWalletAddresses().length > 0) {
  await syncHeliusWalletWebhook();
}
if (config.heliusWebhookAuthHeader) {
  await heliusWebhookServer.start();
} else {
  console.warn(missingHeliusConfigWarning());
}
migrationListener.start();
await commandPoller.start();
