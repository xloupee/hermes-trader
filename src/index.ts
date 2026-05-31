import "dotenv/config";
import { appendFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { Connection, PublicKey } from "@solana/web3.js";
import { buildMigrationReplyMarkup, escapeHtml, extractMigrationData, formatMigrationMessage, getEventId } from "./format.js";
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
  copyTradeBuyIdempotencyKey,
  safelyCompleteCopyTradeBuyIdempotency,
  safelyFailCopyTradeBuyIdempotency
} from "./copytrade-idempotency.js";
import { createCopyTradeLatencyClock, createCopyTradeLatencyTracker } from "./copytrade-latency.js";
import {
  copyTradeSignalAgeBlockedReason,
  copyTradeSignalProviderAllows,
  copyTradeSignalProviderRaces,
  copyTradeSignalRaceLogPayload,
  copyTradeSignalSource,
  copyTradeSignalSourceBlockedReason,
  createCopyTradeSignalRaceTracker,
  parseCopyTradeSignalProvider
} from "./copytrade-signal-race.js";
import type { CopyTradeSignalRaceRecord } from "./copytrade-signal-race.js";
import {
  applyCopyTradeBuyPressureTrade,
  claimCopyTradeBuyPressureSellTrigger,
  copyTradeBuyPressureTimeoutTrigger,
  createCopyTradeBuyPressureSellWatcher,
  createJsonCopyTradeBuyPressureSellStore
} from "./copytrade-buy-pressure.js";
import type {
  CopyTradeBuyPressureSellTrigger,
  CopyTradeBuyPressureSellWatcher
} from "./copytrade-buy-pressure.js";
import {
  buildDirectSolanaSendConnections,
  buildDirectSolanaPayload,
  fetchDirectPumpFastBuyStateFromChain,
  prefetchDirectPumpFastBuyStateFromChain,
  primeDirectPumpFastBuyState,
  refreshDirectPumpFastBuyStateReserves,
  resolveMintTokenProgram,
  sendSolanaDirectTransaction,
  simulateSolanaDirectTransaction,
  warmDirectSolanaBlockhash,
  warmDirectSolanaSdk
} from "./direct-solana.js";
import type { DirectSolanaSendStage } from "./direct-solana.js";
import type { DirectTransactionPayload } from "./direct-pump.js";
import { createGeyserWalletTradeListener } from "./geyser.js";
import type { GeyserWalletTradeReject } from "./geyser.js";
import { createHeliusWebhookServer, missingHeliusConfigWarning, syncHeliusWebhook } from "./helius.js";
import { heliusEventMentionsWatchedWallet, isHeliusSwapEvent, normalizeHeliusSwapData } from "./helius-swaps.js";
import { createYellowstoneWalletMonitor } from "./yellowstone.js";
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
import { decryptLocalSolanaKeypair, decryptSecret, encryptionSecretReady } from "./secrets.js";
import { calculatePlatformFeeSplit, platformFeeConfigBlockedReason } from "./platform-fee.js";
import {
  buildCashbackAccrual,
  buildCashbackExecutionKey,
  cashbackConfigBlockedReason,
  claimCashback,
  createSupabaseCashbackStore,
  parseCashbackConfig
} from "./cashback.js";
import type { CashbackConfig, CashbackStore } from "./cashback.js";
import {
  formatTradeExecutionResultLog,
  isDirectTradeExecutionProvider,
  parseTradeExecutionProvider,
  routeForDirectProvider,
  tradeExecutionProviderConfigError,
  tradeExecutionSkippedResult
} from "./trade-execution.js";
import type {
  DirectTradeExecutionProvider,
  TradeExecutionPlatformFee,
  TradeExecutionProvider,
  TradeExecutionResult
} from "./trade-execution.js";
import { analyzeSolanaTransaction, getSolanaBalanceSol } from "./solana.js";
import { normalizePumpPortalDiscoveryLatencyEvent } from "./shredstream-latency.js";
import { createShredstreamWalletObserver } from "./shredstream-wallet-observer.js";
import { createSubscriberStore } from "./subscribers.js";
import { createSupabaseCopyTradeRecorderFromEnv, createSupabaseSubscriberStoreFromEnv } from "./subscribers-supabase.js";
import { sendTelegramMessage, sendTelegramPhoto } from "./telegram.js";
import { asRecord, errorMessage, isRecord, stringValue } from "./types.js";
import {
  buildWalletTradeReplyMarkup,
  formatAutoCopyBuyMessage,
  formatCopyTradeBuyPressureSellResultMessage,
  formatCopyTradeBuyPressureSellScheduledMessage,
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
  CopyTradeExecutionStatus,
  LooseRecord,
  MigrationData,
  PumpPortalLightningTradeRequest,
  PumpPortalLightningTradeResult,
  PumpPortalTradePool,
  SubscriberRecord,
  TradingWallet,
  TrailingSellConfig,
  TrailingSellStep,
  TransactionAnalysis,
  WalletTradeData,
  WatchedWallet
} from "./types.js";
import type { CopyTradeLatencyMilestone, CopyTradeLatencyMilestoneDetails, CopyTradeLatencyTracker } from "./copytrade-latency.js";

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

function rawListFromEnv(value: string | undefined): string[] {
  return value
    ? [...new Set(value.split(",").map((entry) => entry.trim()).filter(Boolean))]
    : [];
}

function diagnosticWalletsFromEnv(value: string | undefined): WatchedWallet[] {
  const now = new Date().toISOString();

  return rawListFromEnv(value)
    .map((entry) => {
      const [address, ...labelParts] = entry.split(":");
      const label = labelParts.join(":").trim();

      return {
        address: address.trim(),
        label: label || "diagnostic",
        addedAt: now,
        updatedAt: now
      };
    })
    .filter((wallet) => wallet.address);
}

function finiteNumberValue(value: unknown): number | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const number = Number(value);
  return Number.isFinite(number) ? number : null;
}

const PUMP_DEFAULT_TOKEN_TOTAL_SUPPLY = 1_000_000_000_000_000n;
const PUMP_DEFAULT_VIRTUAL_TO_REAL_TOKEN_OFFSET = 280_000_000_000_000n;
const PUMP_DEFAULT_VIRTUAL_TO_REAL_QUOTE_OFFSET = 30_000_000_000n;
const SOL_DECIMALS = 9;
const PUMP_TOKEN_DECIMALS = 6;
const SYSTEM_PROGRAM_ADDRESS = "11111111111111111111111111111111";
const TOKEN_PROGRAM_ADDRESS = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM_ADDRESS = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const DIRECT_PUMP_FAST_STATE_PREFETCH_MAX_IN_FLIGHT = 4;
const DIRECT_PUMP_FAST_STATE_PREFETCH_MAX_QUEUED = 400;
const DIRECT_PUMP_FAST_STATE_PREFETCH_RETRY_DELAYS_MS = [0, 100, 400, 1200, 2500];
const directPumpFastStatePrefetchQueue: Array<{ mint: string; event: LooseRecord }> = [];
const directPumpFastStatePrefetchPending = new Set<string>();
const directPumpFastStatePrefetchStats = {
  scheduled: 0,
  primed: 0,
  missed: 0,
  failed: 0,
  invalidMint: 0,
  dropped: 0,
  directPrimed: 0,
  lastLogAtMs: 0
};
let directPumpFastStatePrefetchInFlight = 0;

function firstString(records: Array<LooseRecord | null | undefined>, keys: string[]): string | null {
  for (const record of records) {
    if (!record) {
      continue;
    }
    for (const key of keys) {
      const value = stringValue(record[key]);
      if (value) {
        return value;
      }
    }
  }

  return null;
}

function firstValue(records: Array<LooseRecord | null | undefined>, keys: string[]): unknown {
  for (const record of records) {
    if (!record) {
      continue;
    }
    for (const key of keys) {
      if (record[key] !== undefined && record[key] !== null && record[key] !== "") {
        return record[key];
      }
    }
  }

  return null;
}

function decimalValueToBigInt(value: unknown, decimals: number): bigint | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const raw = String(value).trim();
  if (!raw) {
    return null;
  }

  if (/^-?\d+$/.test(raw)) {
    return BigInt(raw) * 10n ** BigInt(decimals);
  }

  const match = raw.match(/^(-?)(\d+)(?:\.(\d+))?$/);
  if (!match) {
    const number = Number(value);
    return Number.isFinite(number) ? BigInt(Math.round(number * 10 ** decimals)) : null;
  }

  const sign = match[1] === "-" ? -1n : 1n;
  const whole = BigInt(match[2] || "0") * 10n ** BigInt(decimals);
  const fraction = BigInt((match[3] || "").slice(0, decimals).padEnd(decimals, "0") || "0");
  return sign * (whole + fraction);
}

function integerValueToBigInt(value: unknown): bigint | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const raw = String(value).trim();
  if (!raw) {
    return null;
  }

  if (/^-?\d+$/.test(raw)) {
    return BigInt(raw);
  }

  const number = Number(value);
  return Number.isFinite(number) ? BigInt(Math.round(number)) : null;
}

function positiveBigInt(value: bigint | null): bigint | null {
  return value !== null && value > 0n ? value : null;
}

function pickRawOrUiBigInt({
  records,
  rawKeys,
  uiKeys,
  decimals
}: {
  records: Array<LooseRecord | null | undefined>;
  rawKeys: string[];
  uiKeys: string[];
  decimals: number;
}): bigint | null {
  return positiveBigInt(integerValueToBigInt(firstValue(records, rawKeys)))
    ?? positiveBigInt(decimalValueToBigInt(firstValue(records, uiKeys), decimals));
}

function booleanFromRecords(records: Array<LooseRecord | null | undefined>, keys: string[]): boolean | null {
  const value = firstValue(records, keys);
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value === "string") {
    const normalized = value.trim().toLowerCase();
    if (normalized === "true") {
      return true;
    }
    if (normalized === "false") {
      return false;
    }
  }
  return null;
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

function directExecutionConfirmationModeFromEnv(value: string | undefined): BotConfig["directExecutionConfirmationMode"] {
  const normalized = value?.trim().toLowerCase();
  return normalized === "background" || normalized === "async" ? "background" : "inline";
}

function yellowstoneCommitmentFromEnv(value: string | undefined): BotConfig["yellowstoneCommitment"] {
  const normalized = value?.trim().toLowerCase();
  return normalized === "confirmed" || normalized === "finalized" ? normalized : "processed";
}

function copyTradeLatencyMilestoneForDirectStage(stage: DirectSolanaSendStage): CopyTradeLatencyMilestone | null {
  if (stage === "transaction_build_started") {
    return null;
  }

  if (stage === "transaction_built") {
    return null;
  }

  if (stage === "blockhash_started") {
    return "direct_blockhash_started";
  }

  if (stage === "blockhash_received") {
    return "direct_blockhash_received";
  }

  if (stage === "signing_started") {
    return "direct_signing_started";
  }

  if (stage === "signing_finished") {
    return "direct_signing_finished";
  }

  if (stage === "simulation_started") {
    return "direct_simulate_started";
  }

  if (stage === "simulation_finished") {
    return "direct_simulate_finished";
  }

  if (stage === "raw_send_started") {
    return "direct_raw_send_started";
  }

  if (stage === "signature_returned") {
    return "direct_raw_signature_returned";
  }

  if (stage === "raw_send_failed") {
    return "direct_raw_send_failed";
  }

  if (stage === "confirmation_started") {
    return "direct_confirmation_started";
  }

  if (stage === "confirmation_finished") {
    return "direct_confirmation_finished";
  }

  return null;
}

const rawCopyTradeExecutionProvider = process.env.COPY_TRADE_EXECUTION_PROVIDER || process.env.TRADE_EXECUTION_PROVIDER;
const copyTradeExecutionProviderError = tradeExecutionProviderConfigError(rawCopyTradeExecutionProvider);

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
  pumpPortalDiscoveryLogPath: process.env.PUMPPORTAL_DISCOVERY_LOG_PATH || "logs/pumpportal-discovery-events.jsonl",
  shredstreamCompareEnabled: process.env.SHREDSTREAM_COMPARE_ENABLED === "true",
  shredstreamWalletObserverEnabled: process.env.SHREDSTREAM_WALLET_OBSERVER_ENABLED === "true",
  shredstreamWalletObserverStatsIntervalMs: positiveIntegerFromEnv(process.env.SHREDSTREAM_WALLET_OBSERVER_STATS_INTERVAL_MS, 60_000),
  walletFeedDiagnosticWallets: diagnosticWalletsFromEnv(process.env.WALLET_FEED_DIAGNOSTIC_WALLETS),
  copyTradeEmergencyStopPath: process.env.COPY_TRADE_EMERGENCY_STOP_PATH || "data/copytrade-emergency-stop.json",
  heliusApiKey: process.env.HELIUS_API_KEY,
  heliusApiBaseUrl: process.env.HELIUS_API_BASE_URL || "https://api-mainnet.helius-rpc.com",
  heliusWebhookAuthHeader: process.env.HELIUS_WEBHOOK_AUTH_HEADER,
  heliusWebhookId: process.env.HELIUS_WEBHOOK_ID,
  heliusWebhookPublicUrl: process.env.HELIUS_WEBHOOK_PUBLIC_URL,
  heliusWebhookStatePath: process.env.HELIUS_WEBHOOK_STATE_PATH || "data/helius-webhook.json",
  yellowstoneEnabled: process.env.YELLOWSTONE_ENABLED === "true",
  yellowstoneEndpoint: process.env.YELLOWSTONE_ENDPOINT,
  yellowstoneToken: process.env.YELLOWSTONE_TOKEN || process.env.QUICKNODE_YELLOWSTONE_TOKEN,
  yellowstoneCommitment: yellowstoneCommitmentFromEnv(process.env.YELLOWSTONE_COMMITMENT),
  yellowstoneShadowOnly: process.env.YELLOWSTONE_SHADOW_ONLY !== "false",
  yellowstoneReconnectMs: positiveIntegerFromEnv(process.env.YELLOWSTONE_RECONNECT_MS, 2000),
  webhookPort: Number(process.env.WEBHOOK_PORT || 3000),
  pumpFunCoinApiBaseUrl: process.env.PUMPFUN_COIN_API_BASE_URL || "https://frontend-api-v3.pump.fun/coins",
  solUsdPriceUrl: process.env.SOL_USD_PRICE_URL || "https://api.coinbase.com/v2/prices/SOL-USD/spot",
  solanaRpcUrl: process.env.SOLANA_RPC_URL || "https://api.mainnet-beta.solana.com",
  geyserEnabled: process.env.GEYSER_ENABLED === "true",
  geyserGrpcUrl: process.env.GEYSER_GRPC_URL,
  geyserXToken: process.env.GEYSER_X_TOKEN,
  transactionFlowEnabled: process.env.TRANSACTION_FLOW_ENABLED === "true",
  transactionAccountLabels: process.env.TRANSACTION_ACCOUNT_LABELS,
  shutdownReason: process.env.BOT_SHUTDOWN_REASON,
  notifyOnShutdown: process.env.BOT_NOTIFY_ON_SHUTDOWN === "true",
  copyTradeEnabled: process.env.COPY_TRADE_ENABLED === "true",
  copyTradeDryRun: process.env.COPY_TRADE_DRY_RUN !== "false",
  copyTradeSignalProvider: parseCopyTradeSignalProvider(process.env.COPY_TRADE_SIGNAL_PROVIDER),
  copyTradeExecutionProvider: parseTradeExecutionProvider(rawCopyTradeExecutionProvider),
  copyTradeSlippage: numberFromEnv(process.env.COPY_TRADE_SLIPPAGE, 10),
  copyTradePriorityFee: numberFromEnv(process.env.COPY_TRADE_PRIORITY_FEE, 0.00005),
  copyTradePool: pumpPortalPoolFromEnv(process.env.COPY_TRADE_POOL),
  copyTradeMaxBuySol: positiveNumberFromEnv(process.env.COPY_TRADE_MAX_BUY_SOL, 0.005),
  copyTradeDailySolCap: nonNegativeNumberFromEnv(process.env.COPY_TRADE_DAILY_SOL_CAP, 0.02),
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
  copyTradeTrailingSellMaxBuilds: positiveIntegerFromEnv(process.env.COPY_TRADE_TRAILING_SELL_MAX_BUILDS, 5),
  copyTradeBuyPressureSellEnabled: process.env.COPY_TRADE_BUY_PRESSURE_SELL_ENABLED === "true",
  copyTradeBuyPressureSellPercent: percentFromEnv(process.env.COPY_TRADE_BUY_PRESSURE_SELL_PERCENT, 100),
  copyTradeBuyPressureSellTimeoutMs: positiveIntegerFromEnv(process.env.COPY_TRADE_BUY_PRESSURE_SELL_TIMEOUT_MS, 120_000),
  copyTradeBuyPressureSellMinBuys: positiveIntegerFromEnv(process.env.COPY_TRADE_BUY_PRESSURE_SELL_MIN_BUYS, 1),
  copyTradeBuyPressureSellMinTotalSol: nonNegativeNumberFromEnv(process.env.COPY_TRADE_BUY_PRESSURE_SELL_MIN_TOTAL_SOL, 0),
  copyTradeBuyPressureSellStatePath: process.env.COPY_TRADE_BUY_PRESSURE_SELL_STATE_PATH || "data/copytrade-buy-pressure-sells.json",
  directExecutionEnabled: process.env.DIRECT_EXECUTION_ENABLED === "true",
  directExecutionLiveEnabled: process.env.DIRECT_EXECUTION_LIVE_ENABLED === "true",
  directExecutionBuildOnly: process.env.DIRECT_EXECUTION_BUILD_ONLY === "true",
  directExecutionSimulateOnly: process.env.DIRECT_EXECUTION_SIMULATE_ONLY === "true",
  directExecutionSimulateBeforeSend: process.env.DIRECT_EXECUTION_SIMULATE_BEFORE_SEND !== "false",
  directExecutionSkipPreflight: process.env.DIRECT_EXECUTION_SKIP_PREFLIGHT === "true",
  directExecutionConfirmationMode: directExecutionConfirmationModeFromEnv(process.env.DIRECT_EXECUTION_CONFIRMATION_MODE),
  directExecutionMaxRetries: Math.floor(nonNegativeNumberFromEnv(process.env.DIRECT_EXECUTION_MAX_RETRIES, 3)),
  directExecutionBlockhashCacheMs: Math.floor(nonNegativeNumberFromEnv(process.env.DIRECT_EXECUTION_BLOCKHASH_CACHE_MS, 15_000)),
  directExecutionBlockhashWarmIntervalMs: Math.floor(nonNegativeNumberFromEnv(process.env.DIRECT_EXECUTION_BLOCKHASH_WARM_INTERVAL_MS, 5_000)),
  directExecutionSdkWarmIntervalMs: Math.floor(nonNegativeNumberFromEnv(process.env.DIRECT_EXECUTION_SDK_WARM_INTERVAL_MS, 30_000)),
  directExecutionSendRpcUrls: rawListFromEnv(process.env.DIRECT_EXECUTION_SEND_RPC_URLS),
  directExecutionJitoSendUrls: rawListFromEnv(process.env.DIRECT_EXECUTION_JITO_SEND_URLS),
  directExecutionJitoAuthUuid: process.env.DIRECT_EXECUTION_JITO_AUTH_UUID,
  directExecutionCanaryChatIds: rawListFromEnv(process.env.DIRECT_EXECUTION_CANARY_CHAT_IDS),
  directExecutionCanaryWallets: rawListFromEnv(process.env.DIRECT_EXECUTION_CANARY_WALLETS),
  platformFeeEnabled: process.env.PLATFORM_FEE_ENABLED === "true",
  platformFeeBps: Math.floor(nonNegativeNumberFromEnv(process.env.PLATFORM_FEE_BPS, 100)),
  platformFeeTreasury: process.env.PLATFORM_FEE_TREASURY,
  cashbackEnabled: process.env.CASHBACK_ENABLED === "true",
  cashbackFeeShareBps: Math.floor(nonNegativeNumberFromEnv(process.env.CASHBACK_FEE_SHARE_BPS, 0)),
  cashbackMinClaimSol: nonNegativeNumberFromEnv(process.env.CASHBACK_MIN_CLAIM_SOL, 0.005),
  cashbackPayoutWalletPublicKey: process.env.CASHBACK_PAYOUT_WALLET_PUBLIC_KEY,
  cashbackPayoutWalletSecretKey: process.env.CASHBACK_PAYOUT_WALLET_SECRET_KEY,
  cashbackMaxPayoutSolPerDay: nonNegativeNumberFromEnv(process.env.CASHBACK_MAX_PAYOUT_SOL_PER_DAY, 0)
};
const cashbackConfig: CashbackConfig = parseCashbackConfig(process.env);

const seenEvents = new Set<string>();
const seenGeyserDiagnostics = new Set<string>();
let cachedSolUsdPrice: number | undefined;
let cachedSolUsdPriceAt = 0;
const metadataCache = new Map<string, LooseRecord | null>();
const tokenInfoCache = new Map<string, LooseRecord | null>();
const trailingSellTimers = new Set<NodeJS.Timeout>();
const activeTrailingSellSchedules = new Set<string>();
const buyPressureSellTimers = new Map<string, NodeJS.Timeout>();
const activeBuyPressureSellWatchers = new Map<string, CopyTradeBuyPressureSellWatcher>();
const activeBuyPressureSellTriggers = new Set<string>();
const copyBuySubmissionGuard = createCopyBuySubmissionGuard();
const copyBuySemanticSubmissionGuard = createCopyBuySubmissionGuard();
const copyTradeSignalRaceTracker = createCopyTradeSignalRaceTracker();
const copyTradeDailySolBudget = createInMemoryCopyTradeDailySolBudget();
const directSolanaConnection = new Connection(config.solanaRpcUrl, "confirmed");
const directSolanaSendConnections = buildDirectSolanaSendConnections({
  primaryConnection: directSolanaConnection,
  primaryUrl: config.solanaRpcUrl,
  urls: config.directExecutionSendRpcUrls,
  jitoUrls: config.directExecutionJitoSendUrls,
  jitoAuthUuid: config.directExecutionJitoAuthUuid
});
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
const cashbackStore: CashbackStore | null =
  config.supabaseUrl && config.supabaseServiceRoleKey
    ? createSupabaseCashbackStore({
        url: config.supabaseUrl,
        serviceRoleKey: config.supabaseServiceRoleKey
      })
    : null;
const copyTradeBuyIdempotencyPath = process.env.COPY_TRADE_BUY_IDEMPOTENCY_PATH || "data/copytrade-buy-idempotency.json";
const copyTradeBuyIdempotency =
  config.supabaseUrl && config.supabaseServiceRoleKey
    ? createSupabaseCopyTradeBuyIdempotencyStore({
        url: config.supabaseUrl,
        serviceRoleKey: config.supabaseServiceRoleKey,
        fallback: createJsonCopyTradeBuyIdempotencyStore({
          path: copyTradeBuyIdempotencyPath
        })
      })
    : createJsonCopyTradeBuyIdempotencyStore({
        path: copyTradeBuyIdempotencyPath
      });
const copyTradeBuyPressureSellStore = createJsonCopyTradeBuyPressureSellStore({
  path: config.copyTradeBuyPressureSellStatePath
});
let platformFeeTreasuryWarmup: Promise<string | null> | null = null;
let platformFeeTreasuryVerified: string | null = null;
let platformFeeTreasuryBlockedReason: string | null = null;
const localSolanaSignerCache = new Map<string, ReturnType<typeof decryptLocalSolanaKeypair>>();
let directBlockhashWarmTimer: NodeJS.Timeout | null = null;
let directSdkWarmTimer: NodeJS.Timeout | null = null;

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

type CopyTradeExecutionResult = PumpPortalLightningTradeResult | TradeExecutionResult;

function isProviderNeutralResult(result: CopyTradeExecutionResult): result is TradeExecutionResult {
  return typeof result.status === "string";
}

function resultSignature(result: CopyTradeExecutionResult): string | null {
  return result.signature;
}

function resultOk(result: CopyTradeExecutionResult): boolean {
  return result.ok;
}

function legacyResultFromExecution(result: CopyTradeExecutionResult): PumpPortalLightningTradeResult {
  if (!isProviderNeutralResult(result)) {
    return result;
  }

  return {
    ok: result.ok,
    status: null,
    signature: result.signature,
    errorText: result.errorText,
    raw: {
      status: result.status,
      provider: result.provider,
      route: result.route,
      submittedAtMs: result.submittedAtMs,
      confirmedAtMs: result.confirmedAtMs,
      slot: result.slot,
      metadata: result.metadata,
      raw: result.raw
    }
  };
}

function resolveExecutionProviderForTrade(trade: WalletTradeData): TradeExecutionProvider {
  void trade;
  return config.copyTradeExecutionProvider;
}

function directExecutionGate(provider: TradeExecutionProvider) {
  return {
    provider,
    copyTradeEnabled: config.copyTradeEnabled,
    copyTradeDryRun: config.copyTradeDryRun,
    copyTradeEmergencyStopped,
    emergencyStopped: copyTradeEmergencyStopped,
    directExecutionEnabled: config.directExecutionEnabled,
    directExecutionLiveEnabled: config.directExecutionLiveEnabled,
    directExecutionBuildOnly: config.directExecutionBuildOnly,
    directExecutionSimulateOnly: config.directExecutionSimulateOnly
  };
}

function directBuildOrSimulateMode(provider: TradeExecutionProvider): boolean {
  return isDirectTradeExecutionProvider(provider) && config.directExecutionEnabled && (config.directExecutionBuildOnly || config.directExecutionSimulateOnly);
}

function copyTradeSubmissionBlockedReason(provider: TradeExecutionProvider): string | null {
  if (copyTradeExecutionProviderError) {
    return copyTradeExecutionProviderError;
  }

  if (directBuildOrSimulateMode(provider)) {
    if (copyTradeEmergencyStopped) {
      return "copy trade emergency stop is active";
    }

    if (!config.copyTradeEnabled) {
      return "COPY_TRADE_ENABLED is not true";
    }

    return null;
  }

  if (isDirectTradeExecutionProvider(provider)) {
    if (copyTradeEmergencyStopped) {
      return "copy trade emergency stop is active";
    }

    if (!config.copyTradeEnabled) {
      return "COPY_TRADE_ENABLED is not true";
    }

    if (config.copyTradeDryRun) {
      return "COPY_TRADE_DRY_RUN is enabled";
    }

    if (!config.directExecutionEnabled) {
      return "DIRECT_EXECUTION_ENABLED is not true";
    }

    if (!config.directExecutionLiveEnabled) {
      return "DIRECT_EXECUTION_LIVE_ENABLED is not true";
    }

    return null;
  }

  return copyTradeLiveExecutionBlockedReason(copyTradeExecutionModeConfig());
}

function shouldReserveDailyBudget(provider: TradeExecutionProvider): boolean {
  return !directBuildOrSimulateMode(provider);
}

function directCanaryBlockedReason({
  provider,
  chatId,
  tradingWalletPublicKey
}: {
  provider: TradeExecutionProvider;
  chatId: string;
  tradingWalletPublicKey: string;
}): string | null {
  if (!isDirectTradeExecutionProvider(provider)) {
    return null;
  }

  const directExecutionModeEnabled =
    config.directExecutionEnabled &&
    (config.directExecutionLiveEnabled || config.directExecutionBuildOnly || config.directExecutionSimulateOnly);

  if (!directExecutionModeEnabled) {
    return null;
  }

  if (config.directExecutionCanaryChatIds.length === 0 && config.directExecutionCanaryWallets.length === 0) {
    return "direct execution requires DIRECT_EXECUTION_CANARY_CHAT_IDS or DIRECT_EXECUTION_CANARY_WALLETS";
  }

  if (config.directExecutionCanaryChatIds.length > 0 && !config.directExecutionCanaryChatIds.includes(chatId)) {
    return `chat ${chatId} is not in DIRECT_EXECUTION_CANARY_CHAT_IDS`;
  }

  if (
    config.directExecutionCanaryWallets.length > 0 &&
    !config.directExecutionCanaryWallets.includes(tradingWalletPublicKey)
  ) {
    return `trading wallet ${tradingWalletPublicKey} is not in DIRECT_EXECUTION_CANARY_WALLETS`;
  }

  return null;
}

function validateSolanaPublicKey(value: string): string | null {
  try {
    new PublicKey(value);
    return null;
  } catch {
    return `invalid Solana public key: ${value}`;
  }
}

function solToLamports(amountSol: number): bigint {
  return BigInt(Math.max(0, Math.round(amountSol * 1_000_000_000)));
}

function lamportsToSol(lamports: bigint): number {
  return Number(lamports) / 1_000_000_000;
}

function platformFeeResultFields(split: ReturnType<typeof calculatePlatformFeeSplit>): TradeExecutionPlatformFee | null {
  return split.enabled
    ? {
        enabled: split.enabled,
        bps: split.bps,
        treasury: split.treasury,
        budgetLamports: split.budgetLamports,
        feeLamports: split.feeLamports,
        tradeLamports: split.tradeLamports
      }
    : null;
}

function calculateCopyTradePlatformFee(action: "buy" | "sell", budgetLamports: bigint | number = 0n): ReturnType<typeof calculatePlatformFeeSplit> {
  return calculatePlatformFeeSplit({
    action,
    budgetLamports,
    config: {
      enabled: config.platformFeeEnabled,
      bps: config.platformFeeBps,
      treasury: config.platformFeeTreasury,
      validateTreasury: validateSolanaPublicKey
    }
  });
}

function copyTradePlatformFeeBlockedReason({
  action,
  platformFee,
  provider
}: {
  action: "buy" | "sell";
  platformFee: ReturnType<typeof calculatePlatformFeeSplit>;
  provider: TradeExecutionProvider;
}): string | null {
  return platformFee.blockedReason ||
    (platformFee.enabled && platformFee.tradeLamports <= 0n && action === "buy" ? "copy buy amount is too small after platform fee" : null) ||
    (platformFee.enabled && !isDirectTradeExecutionProvider(provider)
      ? "PLATFORM_FEE_ENABLED requires a direct execution provider"
      : null);
}

function withPlatformFeeResult(
  result: TradeExecutionResult,
  split: ReturnType<typeof calculatePlatformFeeSplit> | null
): TradeExecutionResult {
  return split?.enabled ? { ...result, platformFee: platformFeeResultFields(split) } : result;
}

type DeferredCopyBuyIdempotencyClaim = {
  key: string;
  input: Parameters<typeof copyTradeBuyIdempotency.claimBuy>[0];
};

async function persistDeferredCopyTradeBuyIdempotency({
  claim,
  result,
  errorText
}: {
  claim: DeferredCopyBuyIdempotencyClaim;
  result?: PumpPortalLightningTradeResult | null;
  errorText?: string | null;
}): Promise<void> {
  try {
    const idempotencyClaim = await copyTradeBuyIdempotency.claimBuy(claim.input);

    if (!idempotencyClaim.claimed) {
      console.warn(
        `Fast direct copy buy submitted before durable duplicate check; existing durable record for ${claim.key} is ${idempotencyClaim.existing?.status || "unknown"}`
      );
      return;
    }

    if (result) {
      await safelyCompleteCopyTradeBuyIdempotency(copyTradeBuyIdempotency, claim.key, result);
      return;
    }

    await safelyFailCopyTradeBuyIdempotency(copyTradeBuyIdempotency, claim.key, errorText || "fast direct copy buy failed before result");
  } catch (error) {
    console.warn(`Could not persist fast direct copy buy idempotency key ${claim.key}: ${errorMessage(error)}`);
  }
}

async function verifyPlatformFeeTreasuryAccount({
  connection,
  treasury
}: {
  connection: Connection;
  treasury: string;
}): Promise<string | null> {
  if (platformFeeTreasuryVerified === treasury) {
    return null;
  }

  if (!platformFeeTreasuryWarmup) {
    platformFeeTreasuryWarmup = (async () => {
      try {
        const treasuryInfo = await connection.getAccountInfo(new PublicKey(treasury), "confirmed");
        if (!treasuryInfo) {
          platformFeeTreasuryBlockedReason =
            "PLATFORM_FEE_TREASURY account is not initialized on-chain; fund it once before collecting tiny platform fees";
          return platformFeeTreasuryBlockedReason;
        }

        platformFeeTreasuryVerified = treasury;
        platformFeeTreasuryBlockedReason = null;
        return null;
      } catch (error) {
        const reason = `could not verify PLATFORM_FEE_TREASURY account: ${errorMessage(error)}`;
        platformFeeTreasuryBlockedReason = reason;
        return reason;
      } finally {
        platformFeeTreasuryWarmup = null;
      }
    })();
  }

  return platformFeeTreasuryWarmup;
}

function warmPlatformFeeTreasuryAccount(): void {
  if (!config.platformFeeEnabled || !config.platformFeeTreasury) {
    return;
  }

  verifyPlatformFeeTreasuryAccount({
    connection: directSolanaConnection,
    treasury: config.platformFeeTreasury
  }).then((reason) => {
    if (reason) {
      console.warn(`Platform fee treasury warmup failed: ${reason}`);
    }
  }).catch((error) => {
    console.warn(`Platform fee treasury warmup failed: ${errorMessage(error)}`);
  });
}

function warmDirectExecutionHotPath(): void {
  if (!isDirectTradeExecutionProvider(config.copyTradeExecutionProvider)) {
    return;
  }
  const provider = config.copyTradeExecutionProvider;

  const warm = (forceRefresh = false) => {
    warmDirectSolanaSdk({
      connection: directSolanaConnection,
      provider,
      forceRefresh
    }).then(() => {
      console.log(`Direct execution hot path warmed: provider=${provider}${forceRefresh ? " refresh=true" : ""}`);
    }).catch((error) => {
      console.warn(`Direct execution hot path warmup failed: ${errorMessage(error)}`);
    });
  };

  warm();

  if (config.directExecutionSdkWarmIntervalMs > 0) {
    directSdkWarmTimer = setInterval(() => warm(true), config.directExecutionSdkWarmIntervalMs);
    directSdkWarmTimer.unref?.();
  }
}

function warmDirectExecutionBlockhashCache(): void {
  if (!isDirectTradeExecutionProvider(config.copyTradeExecutionProvider) || config.directExecutionBlockhashCacheMs <= 0) {
    return;
  }

  const warm = () => {
    const primaryWarmup = warmDirectSolanaBlockhash({
      connection: directSolanaConnection,
      cacheMs: config.directExecutionBlockhashCacheMs,
      forceRefresh: true
    });
    const fanoutWarmups = directSolanaSendConnections
      .filter((candidate) => candidate.connection !== directSolanaConnection)
      .map((candidate) => candidate.connection.getLatestBlockhash?.("processed"));

    Promise.allSettled([primaryWarmup, ...fanoutWarmups]).then((results) => {
      const rejected = results.find((result): result is PromiseRejectedResult => result.status === "rejected");
      if (rejected) {
        console.warn(`Direct execution blockhash/send-RPC warmup failed: ${errorMessage(rejected.reason)}`);
      }
    });
  };

  warm();

  if (config.directExecutionBlockhashWarmIntervalMs > 0) {
    directBlockhashWarmTimer = setInterval(warm, config.directExecutionBlockhashWarmIntervalMs);
    directBlockhashWarmTimer.unref?.();
  }
}

function cachedLocalSolanaSigner(tradingWallet: TradingWallet): ReturnType<typeof decryptLocalSolanaKeypair> {
  const cacheKey = [
    tradingWallet.publicKey,
    tradingWallet.secretKeyFormat || "base58",
    tradingWallet.encryptedSecretKey || ""
  ].join(":");
  const cached = localSolanaSignerCache.get(cacheKey);

  if (cached) {
    return cached;
  }

  const signer = decryptLocalSolanaKeypair({
    encryptedSecretKey: tradingWallet.encryptedSecretKey || "",
    encryptionSecret: config.pumpPortalWalletKeyEncryptionSecret || "",
    secretKeyFormat: tradingWallet.secretKeyFormat
  });
  localSolanaSignerCache.set(cacheKey, signer);
  return signer;
}

function warmLocalSolanaSignerCache(): void {
  if (!encryptionSecretReady(config.pumpPortalWalletKeyEncryptionSecret)) {
    return;
  }

  const walletsByPublicKey = new Map<string, TradingWallet>();

  for (const subscriber of subscribers.list()) {
    const wallets = [subscriber.tradingWallet, ...(subscriber.tradingWallets || [])].filter(
      (wallet): wallet is TradingWallet =>
        Boolean(wallet?.publicKey) &&
        wallet?.provider === "local-solana" &&
        Boolean(wallet?.encryptedSecretKey)
    );

    for (const wallet of wallets) {
      walletsByPublicKey.set(wallet.publicKey, wallet);
    }
  }

  if (walletsByPublicKey.size === 0) {
    return;
  }

  let warmed = 0;
  let failed = 0;

  for (const wallet of walletsByPublicKey.values()) {
    try {
      cachedLocalSolanaSigner(wallet);
      warmed += 1;
    } catch (error) {
      failed += 1;
      console.warn(`Local Solana signer warmup failed for ${wallet.publicKey}: ${errorMessage(error)}`);
    }
  }

  console.log(`Local Solana signer cache warmed: warmed=${warmed} failed=${failed}`);
}

function percentAmountToBasisPoints(amount: number | string): bigint | null {
  if (typeof amount === "number") {
    return null;
  }

  const percent = Number(amount.replace("%", ""));
  if (!Number.isFinite(percent) || percent <= 0) {
    return null;
  }

  return BigInt(Math.floor(percent * 100));
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
  console.log(
    [
      "Copy trade signal provider",
      `mode=${config.copyTradeSignalProvider}`,
      `pumpPortal=trigger`,
      `geyser=${copyTradeSignalProviderAllows(config.copyTradeSignalProvider, "geyser") ? "trigger" : "diagnostic"}`,
      `shredstream=${copyTradeSignalProviderAllows(config.copyTradeSignalProvider, "shredstream") ? "trigger" : config.shredstreamWalletObserverEnabled ? "observe" : "off"}`,
      `diagnosticWallets=${config.walletFeedDiagnosticWallets.length}`
    ].join(" | ")
  );
  console.log(
    [
      "Direct execution controls",
      `provider=${config.copyTradeExecutionProvider}`,
      `enabled=${config.directExecutionEnabled ? "true" : "false"}`,
      `live=${config.directExecutionLiveEnabled ? "true" : "false"}`,
      `buildOnly=${config.directExecutionBuildOnly ? "true" : "false"}`,
      `simulateOnly=${config.directExecutionSimulateOnly ? "true" : "false"}`,
      `simulateBeforeSend=${config.directExecutionSimulateBeforeSend ? "true" : "false"}`,
      `skipPreflight=${config.directExecutionSkipPreflight ? "true" : "false"}`,
      `maxRetries=${config.directExecutionMaxRetries}`,
      `confirmationMode=${config.directExecutionConfirmationMode}`,
      `blockhashCacheMs=${config.directExecutionBlockhashCacheMs}`,
      `blockhashWarmMs=${config.directExecutionBlockhashWarmIntervalMs}`,
      `sdkWarmMs=${config.directExecutionSdkWarmIntervalMs}`,
      `sendRpcFanout=${directSolanaSendConnections.length}`,
      `canaryChats=${config.directExecutionCanaryChatIds.length || "none"}`,
      `canaryWallets=${config.directExecutionCanaryWallets.length || "none"}`,
      `platformFee=${config.platformFeeEnabled ? `${config.platformFeeBps}bps` : "disabled"}`,
      `cashback=${cashbackConfig.enabled ? `${cashbackConfig.feeShareBps}bps` : "disabled"}`
    ].join(" | ")
  );

  const platformFeeBlockedReason = platformFeeConfigBlockedReason({
    enabled: config.platformFeeEnabled,
    bps: config.platformFeeBps,
    treasury: config.platformFeeTreasury,
    validateTreasury: validateSolanaPublicKey
  });
  if (platformFeeBlockedReason) {
    console.warn(`Platform fee config blocks fee-enabled direct execution: ${platformFeeBlockedReason}`);
  }

  const cashbackBlockedReason = cashbackConfigBlockedReason(cashbackConfig);
  if (cashbackBlockedReason) {
    console.warn(`Cashback config blocks accrual and claims: ${cashbackBlockedReason}`);
  }

  if (cashbackConfig.enabled && !cashbackStore) {
    console.warn("CASHBACK_ENABLED is true but Supabase is not configured; cashback accrual and claims are disabled.");
  }

  if (copyTradeExecutionProviderError) {
    console.warn(`Copy trade execution provider config blocks live submissions: ${copyTradeExecutionProviderError}`);
  }
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

async function persistCopyTradeEmergencyStopCleared(chatId: string | number): Promise<void> {
  const state = {
    active: false,
    clearedByChatId: String(chatId),
    clearedAt: new Date().toISOString()
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

async function clearCopyTradeEmergencyStop(chatId: string | number): Promise<string> {
  copyTradeEmergencyStopped = false;
  await persistCopyTradeEmergencyStopCleared(chatId);
  const message = formatCopyTradeExecutionStateLog(copyTradeExecutionModeConfig());

  console.warn(`Copy trade emergency stop cleared by ${chatId}: ${message}`);
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
      [
        ...subscribers
          .list()
          .filter(isSubscriberLive)
          .flatMap((subscriber) => [...(subscriber.watchedWallets || []), ...(subscriber.copyTradeWallets || [])]),
        ...config.walletFeedDiagnosticWallets
      ].map((wallet) => wallet.address)
        .filter(Boolean)
    )
  ].sort();
}

function copyTradeWalletAddresses(): string[] {
  return [
    ...new Set(
      subscribers
        .list()
        .filter(isSubscriberLive)
        .flatMap((subscriber) => subscriber.copyTradeWallets || [])
        .map((wallet) => wallet.address)
        .filter(Boolean)
    )
  ].sort();
}

function activeGeyserWallets(): WatchedWallet[] {
  return [
    ...new Map(
      [
        ...subscribers
          .list()
          .filter(isSubscriberLive)
          .flatMap((subscriber) => [...(subscriber.watchedWallets || []), ...(subscriber.copyTradeWallets || [])]),
        ...config.walletFeedDiagnosticWallets
      ]
        .filter((wallet) => wallet.address)
        .map((wallet) => [wallet.address, wallet])
    ).values()
  ].sort((a, b) => a.address.localeCompare(b.address));
}

function diagnosticWallet(address: string | null | undefined): WatchedWallet | null {
  const normalized = address?.trim();
  if (!normalized) {
    return null;
  }

  return config.walletFeedDiagnosticWallets.find((wallet) => wallet.address === normalized) || null;
}

function activeBuyPressureSellMints(): string[] {
  return [...new Set([...activeBuyPressureSellWatchers.values()].map((watcher) => watcher.mint).filter(Boolean))].sort();
}

function activePumpPortalSubscriptions(): PumpPortalSubscription[] {
  const copyTradeWallets = [...new Set([...copyTradeWalletAddresses(), ...config.walletFeedDiagnosticWallets.map((wallet) => wallet.address)])].sort();
  const buyPressureMints = activeBuyPressureSellMints();

  return [
    ...baseSubscriptionMethods,
    ...(copyTradeWallets.length > 0
      ? [
          {
            method: "subscribeAccountTrade",
            keys: copyTradeWallets
          }
        ]
      : []),
    ...(buyPressureMints.length > 0
      ? [
          {
            method: "subscribeTokenTrade",
            keys: buyPressureMints
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

function runAfterHotPath(label: string, work: Promise<unknown>): void {
  work.catch((error) => {
    console.warn(`${label} failed: ${errorMessage(error)}`);
  });
}

function rememberGeyserDiagnostic(id: string | null): boolean {
  if (!id) {
    return false;
  }

  if (seenGeyserDiagnostics.has(id)) {
    return true;
  }

  seenGeyserDiagnostics.add(id);

  if (seenGeyserDiagnostics.size > 1000) {
    const oldest = seenGeyserDiagnostics.values().next().value;
    if (oldest) {
      seenGeyserDiagnostics.delete(oldest);
    }
  }

  return false;
}

function geyserDiagnosticEventId(trade: WalletTradeData): string | null {
  return trade.signature
    ? ["geyser-wallet-trade", trade.signature, trade.targetWallet].join(":")
    : getWalletTradeEventId(trade);
}

function logCopyTradeSignalRace(
  trade: WalletTradeData,
  outcome: "won" | "duplicate" | "skipped",
  options: {
    reason?: string | null;
    winner?: CopyTradeSignalRaceRecord | null;
    key?: string | null;
    receivedAtMs?: number | null;
    normalizedAtMs?: number | null;
  } = {}
): void {
  if (!copyTradeSignalProviderRaces(config.copyTradeSignalProvider) || !copyTradeSignalSource(trade.provider)) {
    return;
  }

  console.log(`Copy trade signal race: ${JSON.stringify(copyTradeSignalRaceLogPayload({
    mode: config.copyTradeSignalProvider,
    trade,
    outcome,
    reason: options.reason,
    winner: options.winner,
    key: options.key,
    receivedAtMs: options.receivedAtMs,
    normalizedAtMs: options.normalizedAtMs
  }))}`);
}

async function handleGeyserWalletTradeDiagnostic(trade: WalletTradeData): Promise<void> {
  const eventId = geyserDiagnosticEventId(trade);

  if (rememberGeyserDiagnostic(eventId)) {
    return;
  }

  await writeWalletTradeLog(trade);
  console.log(`Geyser wallet trade diagnostic: ${JSON.stringify(trade)}`);
}

async function handleWalletTradeSignal(
  trade: WalletTradeData,
  { receivedAtMs = Date.now(), normalizedAtMs = receivedAtMs }: { receivedAtMs?: number; normalizedAtMs?: number } = {}
): Promise<boolean> {
  const signalSource = copyTradeSignalSource(trade.provider);

  if (signalSource && !copyTradeSignalProviderAllows(config.copyTradeSignalProvider, signalSource)) {
    return false;
  }

  const entries = subscribers
    .list()
    .filter(isSubscriberLive)
    .flatMap((subscriber) =>
      (subscriber.watchedWallets || [])
        .filter((wallet) => wallet.address === trade.targetWallet)
        .map((wallet) => ({ subscriber, wallet, label: wallet.label }))
    );
  const copyTradeEntries = activeCopyTradeEntriesForTarget(trade.targetWallet);
  const hasBuyPressureWatchers = Boolean(
    trade.mint && [...activeBuyPressureSellWatchers.values()].some((watcher) => watcher.mint === trade.mint)
  );

  if (entries.length === 0 && copyTradeEntries.length === 0 && !hasBuyPressureWatchers) {
    return false;
  }

  const canRaceCopyTradeSignal = copyTradeSignalProviderRaces(config.copyTradeSignalProvider) &&
    signalSource !== null &&
    copyTradeSignalProviderAllows(config.copyTradeSignalProvider, signalSource) &&
    (copyTradeEntries.length > 0 || hasBuyPressureWatchers);
  const raceAgeBlockedReason = canRaceCopyTradeSignal
    ? copyTradeSignalAgeBlockedReason({
        trade,
        maxSignalAgeMs: config.copyTradeMaxSignalAgeMs,
        nowMs: receivedAtMs
      })
    : null;
  const raceSourceBlockedReason = canRaceCopyTradeSignal && !raceAgeBlockedReason
    ? copyTradeSignalSourceBlockedReason({
        trade,
        allowedSources: config.copyTradeAllowedSources
      })
    : null;
  const raceBlockedReason = raceAgeBlockedReason || raceSourceBlockedReason;
  const raceCopyableBlockedReason = canRaceCopyTradeSignal && !raceBlockedReason && !isCopyableSolToTokenBuy(trade)
    ? "trade is not a copyable SOL-to-token buy"
    : null;
  const racesCopyTradeSignal = canRaceCopyTradeSignal && !raceCopyableBlockedReason;

  if (raceBlockedReason) {
    logCopyTradeSignalRace(trade, "skipped", { reason: raceBlockedReason, receivedAtMs, normalizedAtMs });
    return true;
  }

  if (raceCopyableBlockedReason) {
    logCopyTradeSignalRace(trade, "skipped", { reason: raceCopyableBlockedReason, receivedAtMs, normalizedAtMs });
  } else if (racesCopyTradeSignal) {
    const claim = copyTradeSignalRaceTracker.claim(trade, receivedAtMs);

    if (claim.outcome === "duplicate") {
      logCopyTradeSignalRace(trade, "duplicate", {
        winner: claim.record,
        key: claim.key,
        receivedAtMs,
        normalizedAtMs
      });
      return true;
    }

    logCopyTradeSignalRace(trade, "won", {
      winner: claim.record,
      key: claim.key,
      receivedAtMs,
      normalizedAtMs
    });
    prefetchDirectPumpFastBuyStateForTrade(trade);
  }

  const eventId = walletTradeSeenEventId(trade);

  if (rememberEvent(eventId)) {
    return true;
  }

  const walletTradeLog = writeWalletTradeLog(trade)
    .then(() => {
      console.log(`Wallet trade event: ${JSON.stringify(trade)}`);
    })
    .catch((error) => {
      console.warn(`Wallet trade log write failed: ${errorMessage(error)}`);
    });

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
        normalizedAtMs
      }
    )
  ));

  await handleCopyTradeBuyPressureTrade(trade);
  await Promise.all(entries.map((entry) =>
    sendWalletTradeAlert(entry.subscriber, {
      ...trade,
      label: entry.label
    })
  ));
  await walletTradeLog;

  return true;
}

async function handleGeyserWalletTrade(
  trade: WalletTradeData,
  timing: { receivedAtMs?: number; normalizedAtMs?: number } = {}
): Promise<void> {
  if (!copyTradeSignalProviderAllows(config.copyTradeSignalProvider, "geyser")) {
    await handleGeyserWalletTradeDiagnostic(trade);
    return;
  }

  const handled = await handleWalletTradeSignal(trade, timing);
  if (handled) {
    return;
  }

  const diagnostic = diagnosticWallet(trade.targetWallet);
  if (diagnostic) {
    const diagnosticTrade = {
      ...trade,
      label: diagnostic.label,
      raw: {
        ...trade.raw,
        diagnosticWallet: true
      }
    };
    await writeWalletTradeLog(diagnosticTrade);
    console.log(`Geyser diagnostic wallet trade event: ${JSON.stringify(diagnosticTrade)}`);
  }
}

function handleGeyserWalletTradeReject(reject: GeyserWalletTradeReject): void {
  const eventId = reject.signature
    ? ["geyser-wallet-trade-reject", reject.signature, reject.targetWallet, reject.reason].join(":")
    : ["geyser-wallet-trade-reject", reject.slot, reject.targetWallet, reject.reason].filter(Boolean).join(":");

  if (rememberGeyserDiagnostic(eventId)) {
    return;
  }

  console.log(`Geyser wallet trade reject: ${JSON.stringify(reject)}`);
}

async function handleMigration(event: LooseRecord): Promise<void> {
  const eventMode = classifyEventMode(event);

  if (!eventMode) {
    return;
  }

  if (eventMode === "newtokens") {
    scheduleDirectPumpFastBuyStatePrime(event);
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
  if (eventMode === "newtokens") {
    primeDirectPumpFastBuyStateFromEvent(event, tokenInfo);
  }
  await writeMigrationLog(migration);
  console.log(`PumpPortal event: ${JSON.stringify(migration)}`);

  const text = formatMigrationMessage(event, eventConfig);
  const replyMarkup = buildMigrationReplyMarkup(event, eventConfig);

  await sendAlertToSubscribers({ subscribers: recipients, migration, text, replyMarkup });
}

async function handlePumpPortalEvent(event: LooseRecord): Promise<void> {
  const receivedAtMs = Date.now();
  await observePumpPortalDiscoveryEvent(event, receivedAtMs);

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

async function observePumpPortalDiscoveryEvent(event: LooseRecord, receivedAtMs: number): Promise<void> {
  if (!config.shredstreamCompareEnabled) {
    return;
  }

  const discoveryEvent = normalizePumpPortalDiscoveryLatencyEvent(event, receivedAtMs);

  if (!discoveryEvent) {
    return;
  }

  await writePumpPortalDiscoveryLog(discoveryEvent);
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

  const trade = normalizePumpPortalAccountTradeData({
    event,
    targetWallet: trader,
    label: null
  });

  if (!trade) {
    return false;
  }

  const receivedAtMs = Date.now();
  const handled = await handleWalletTradeSignal(trade, {
    receivedAtMs,
    normalizedAtMs: receivedAtMs
  });

  if (handled) {
    return true;
  }

  const diagnostic = diagnosticWallet(trader);
  if (diagnostic) {
    const diagnosticTrade = {
      ...trade,
      label: diagnostic.label,
      raw: {
        ...trade.raw,
        diagnosticWallet: true
      }
    };
    await writeWalletTradeLog(diagnosticTrade);
    console.log(`PumpPortal diagnostic wallet trade event: ${JSON.stringify(diagnosticTrade)}`);
    return true;
  }

  return false;
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
    if (!isSubscriberLive(subscriber)) {
      continue;
    }

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

    await handleCopyTradeBuyPressureTrade(loggedTrade);

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

async function handleYellowstoneWalletTrade(
  trade: WalletTradeData,
  { receivedAtMs = Date.now(), normalizedAtMs = Date.now() }: { receivedAtMs?: number; normalizedAtMs?: number } = {}
): Promise<void> {
  if (config.yellowstoneShadowOnly) {
    await writeWalletTradeLog(trade);
    console.log(`Yellowstone shadow wallet trade event: ${JSON.stringify(trade)}`);
    return;
  }

  const eventId = walletTradeSeenEventId(trade);
  if (rememberEvent(eventId)) {
    return;
  }

  await writeWalletTradeLog(trade);
  console.log(`Wallet trade event: ${JSON.stringify(trade)}`);

  await handleCopyTradeBuyPressureTrade(trade);

  const copyTradeEntries = subscribers
    .list()
    .filter(isSubscriberLive)
    .flatMap((subscriber) =>
      (subscriber.copyTradeWallets || [])
        .filter((wallet) => wallet.address === trade.targetWallet)
        .map((wallet) => ({ subscriber, wallet, label: wallet.label }))
    );
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
        normalizedAtMs
      }
    )
  ));

  const entries = subscribers
    .list()
    .filter(isSubscriberLive)
    .flatMap((subscriber) =>
      (subscriber.watchedWallets || [])
        .filter((wallet) => wallet.address === trade.targetWallet)
        .map((wallet) => ({ subscriber, label: wallet.label }))
    );
  await Promise.all(entries.map((entry) =>
    sendWalletTradeAlert(entry.subscriber, {
      ...trade,
      label: entry.label
    })
  ));
}

async function handleShredstreamWalletTrade(
  trade: WalletTradeData,
  { receivedAtMs = Date.now(), normalizedAtMs = Date.now() }: { receivedAtMs?: number; normalizedAtMs?: number } = {}
): Promise<void> {
  const diagnostic = diagnosticWallet(trade.targetWallet);
  const observedTrade = {
    ...trade,
    label: diagnostic?.label || trade.label,
    raw: {
      ...trade.raw,
      ...(diagnostic ? { diagnosticWallet: true } : {}),
      feedTiming: {
        receivedAtMs,
        normalizedAtMs,
        normalizedLagMs: Math.max(0, normalizedAtMs - receivedAtMs)
      }
    }
  };

  if (!diagnostic && copyTradeSignalProviderAllows(config.copyTradeSignalProvider, "shredstream")) {
    if (activeCopyTradeEntriesForTarget(observedTrade.targetWallet).length > 0) {
      prefetchDirectPumpFastBuyStateForTrade(observedTrade);
    }
    await handleWalletTradeSignal(observedTrade, { receivedAtMs, normalizedAtMs });
    return;
  }

  await writeWalletTradeLog(observedTrade);
  console.log(`ShredStream shadow wallet trade event: ${JSON.stringify(observedTrade)}`);
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

function isDirectPayload(value: TradeExecutionResult | DirectTransactionPayload): value is DirectTransactionPayload {
  return Array.isArray((value as DirectTransactionPayload).instructions);
}

async function executeDirectCopyTrade({
  subscriber,
  provider,
  request,
  amountLamports,
  amountBasis,
  platformFee = null,
  metadata = {},
  forceFreshBuyState = false,
  onLatencyMilestone
}: {
  subscriber: SubscriberRecord;
  provider: DirectTradeExecutionProvider;
  request: PumpPortalLightningTradeRequest;
  amountLamports: bigint;
  amountBasis: "sol" | "percent" | "token";
  platformFee?: ReturnType<typeof calculatePlatformFeeSplit> | null;
  metadata?: Record<string, unknown>;
  forceFreshBuyState?: boolean;
  onLatencyMilestone?: (milestone: CopyTradeLatencyMilestone, details?: CopyTradeLatencyMilestoneDetails) => void;
}): Promise<TradeExecutionResult> {
  const tradingWallet = subscriber.tradingWallet;

  if (!tradingWallet || tradingWallet.provider !== "local-solana" || !tradingWallet.encryptedSecretKey) {
    return tradeExecutionSkippedResult({
      provider,
      route: provider === "direct-pumpswap" ? "pumpswap-amm" : provider === "direct-pump" ? "pump-bonding-curve" : "auto",
      reason: "direct execution requires an active local-signing trading wallet",
      metadata,
      platformFee: platformFee ? platformFeeResultFields(platformFee) : null
    });
  }

  const canaryBlockedReason = directCanaryBlockedReason({
    provider,
    chatId: subscriber.chatId,
    tradingWalletPublicKey: tradingWallet.publicKey
  });
  if (canaryBlockedReason) {
    return tradeExecutionSkippedResult({
      provider,
      route: provider === "direct-pumpswap" ? "pumpswap-amm" : provider === "direct-pump" ? "pump-bonding-curve" : "auto",
      reason: canaryBlockedReason,
      metadata,
      platformFee: platformFee ? platformFeeResultFields(platformFee) : null
    });
  }
  onLatencyMilestone?.("direct_wallet_checked", { status: "ok" });

  if (!encryptionSecretReady(config.pumpPortalWalletKeyEncryptionSecret)) {
    return tradeExecutionSkippedResult({
      provider,
      route: provider === "direct-pumpswap" ? "pumpswap-amm" : provider === "direct-pump" ? "pump-bonding-curve" : "auto",
      reason: "missing PUMPPORTAL_WALLET_KEY_ENCRYPTION_SECRET",
      metadata,
      platformFee: platformFee ? platformFeeResultFields(platformFee) : null
    });
  }

  let signer: ReturnType<typeof decryptLocalSolanaKeypair>;
  try {
    signer = cachedLocalSolanaSigner(tradingWallet);
  } catch (error) {
    return tradeExecutionSkippedResult({
      provider,
      route: provider === "direct-pumpswap" ? "pumpswap-amm" : provider === "direct-pump" ? "pump-bonding-curve" : "auto",
      reason: `could not decrypt local-signing trading wallet: ${errorMessage(error)}`,
      metadata,
      platformFee: platformFee ? platformFeeResultFields(platformFee) : null
    });
  }
  onLatencyMilestone?.("direct_signer_ready", { status: "ok" });
  const connection = directSolanaConnection;

  if (config.directExecutionBlockhashCacheMs > 0) {
    onLatencyMilestone?.("direct_warmup_started", { status: "blockhash" });
    warmDirectSolanaBlockhash({
      connection,
      cacheMs: config.directExecutionBlockhashCacheMs
    }).catch((error) => {
      console.warn(`Direct execution pre-build blockhash warmup failed: ${errorMessage(error)}`);
    });
  } else {
    onLatencyMilestone?.("direct_warmup_started", { status: "disabled" });
  }

  onLatencyMilestone?.("direct_build_started");
  const built = await buildDirectSolanaPayload({
    connection,
    request: {
      provider,
      action: request.action,
      mint: request.mint,
      amountLamports,
      amountBasis,
      slippagePercent: request.slippage,
      priorityFeeSol: request.priorityFee,
      walletPublicKey: tradingWallet.publicKey,
      platformFee,
      metadata,
      forceFreshBuyState
    }
  });
  onLatencyMilestone?.("direct_build_finished", {
    status: isDirectPayload(built) ? "built" : built.status,
    reason: isDirectPayload(built) ? null : built.errorText
  });

  if (!isDirectPayload(built)) {
    return withPlatformFeeResult(built, platformFee);
  }
  const resultPlatformFee = built.platformFee || platformFee;

  if (config.directExecutionBuildOnly) {
    return withPlatformFeeResult({
      ok: false,
      status: "skipped",
      provider: built.provider,
      route: built.route.route,
      signature: null,
      errorText: "direct execution build-only mode is enabled; transaction was built but not submitted",
      raw: null,
      submittedAtMs: null,
      confirmedAtMs: null,
      slot: null,
      metadata: {
        ...built.metadata,
        instructionCount: built.instructions.length
      }
    }, resultPlatformFee);
  }

  if (config.directExecutionSimulateOnly) {
    return withPlatformFeeResult(await simulateSolanaDirectTransaction({
      connection,
      signer,
      payload: built
    }), resultPlatformFee);
  }

  if (config.directExecutionConfirmationMode === "background" && request.action === "buy") {
    return withPlatformFeeResult(await sendSolanaDirectTransaction({
      connection,
      signer,
      payload: built,
      config: {
        gate: directExecutionGate(provider),
        simulateBeforeSend: config.directExecutionSimulateBeforeSend,
        skipPreflight: config.directExecutionSkipPreflight,
        confirmationMode: "background",
        maxRetries: config.directExecutionMaxRetries,
        blockhashCacheMs: config.directExecutionBlockhashCacheMs,
        sendConnections: directSolanaSendConnections,
        onStage: (stage, details) => {
          const milestone = copyTradeLatencyMilestoneForDirectStage(stage);
          if (milestone) {
            onLatencyMilestone?.(milestone, {
              status: details.status,
              reason: details.errorText,
              signature: details.signature || null
            });
          }
        }
      }
    }), resultPlatformFee);
  }

  return withPlatformFeeResult(await sendSolanaDirectTransaction({
    connection,
    signer,
    payload: built,
    config: {
      gate: directExecutionGate(provider),
      simulateBeforeSend: config.directExecutionSimulateBeforeSend,
      skipPreflight: config.directExecutionSkipPreflight,
      confirmationMode: "inline",
      maxRetries: config.directExecutionMaxRetries,
      blockhashCacheMs: config.directExecutionBlockhashCacheMs,
      sendConnections: directSolanaSendConnections,
      onStage: (stage, details) => {
        const milestone = copyTradeLatencyMilestoneForDirectStage(stage);
        if (milestone) {
          onLatencyMilestone?.(milestone, {
            status: details.status,
            reason: details.errorText,
            signature: details.signature || null
          });
        }
      }
    }
  }), resultPlatformFee);
}

async function executeCopyTradeBuy({
  subscriber,
  trade,
  request,
  provider,
  platformFee,
  onLatencyMilestone
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  request: PumpPortalLightningTradeRequest;
  provider: TradeExecutionProvider;
  platformFee: ReturnType<typeof calculatePlatformFeeSplit>;
  onLatencyMilestone?: (milestone: CopyTradeLatencyMilestone, details?: CopyTradeLatencyMilestoneDetails) => void;
}): Promise<CopyTradeExecutionResult> {
  if (!isDirectTradeExecutionProvider(provider)) {
    if (platformFee.enabled) {
      return tradeExecutionSkippedResult({
        provider: "pumpportal-lightning",
        route: "pumpportal-lightning",
        reason: "PLATFORM_FEE_ENABLED requires a direct execution provider",
        platformFee: platformFeeResultFields(platformFee),
        metadata: { observedSignature: trade.signature }
      });
    }

    if (subscriber.tradingWallet?.provider === "local-solana") {
      return tradeExecutionSkippedResult({
        provider: "pumpportal-lightning",
        route: "pumpportal-lightning",
        reason: "PumpPortal Lightning execution requires a PumpPortal-backed trading wallet",
        metadata: { observedSignature: trade.signature }
      });
    }

    const apiKey = decryptSecret(subscriber.tradingWallet?.encryptedApiKey || "", config.pumpPortalWalletKeyEncryptionSecret || "");
    return executePumpPortalLightningTrade({
      url: config.pumpPortalLightningTradeUrl,
      apiKey,
      request
    });
  }

  const refreshedFastState = refreshDirectPumpFastBuyStateFromTrade(trade);
  onLatencyMilestone?.("direct_buy_state_refreshed", {
    status: refreshedFastState ? "refreshed" : "skipped"
  });
  const forceFreshBuyState = request.action === "buy" &&
    trade.pool === "pump" &&
    !refreshedFastState &&
    trade.provider !== "pumpportal";

  return executeDirectCopyTrade({
    subscriber,
    provider,
    request,
    amountLamports: platformFee.tradeLamports,
    amountBasis: "sol",
    platformFee,
    forceFreshBuyState,
    onLatencyMilestone,
    metadata: {
      observedSignature: trade.signature,
      sourceWallet: trade.targetWallet,
      observedPool: trade.pool || request.pool || null,
      observedSource: trade.source || null,
      forceFreshBuyState,
      budgetLamports: platformFee.budgetLamports.toString()
    }
  });
}

async function executeCopyTradeSell({
  subscriber,
  trade,
  request,
  provider,
  seenSellSignatures,
  platformFee
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  request: PumpPortalLightningTradeRequest;
  provider: TradeExecutionProvider;
  seenSellSignatures: Set<string>;
  platformFee: ReturnType<typeof calculatePlatformFeeSplit>;
}): Promise<{ result: CopyTradeExecutionResult; duplicateSignature: boolean }> {
  if (!isDirectTradeExecutionProvider(provider)) {
    if (platformFee.enabled) {
      return {
        result: tradeExecutionSkippedResult({
          provider: "pumpportal-lightning",
          route: "pumpportal-lightning",
          reason: "PLATFORM_FEE_ENABLED requires a direct execution provider",
          platformFee: platformFeeResultFields(platformFee),
          metadata: { observedSignature: trade.signature }
        }),
        duplicateSignature: false
      };
    }

    if (subscriber.tradingWallet?.provider === "local-solana") {
      return {
        result: tradeExecutionSkippedResult({
          provider: "pumpportal-lightning",
          route: "pumpportal-lightning",
          reason: "PumpPortal Lightning execution requires a PumpPortal-backed trading wallet",
          metadata: { observedSignature: trade.signature }
        }),
        duplicateSignature: false
      };
    }

    const apiKey = decryptSecret(subscriber.tradingWallet?.encryptedApiKey || "", config.pumpPortalWalletKeyEncryptionSecret || "");
    return executeTrailingSellWithDuplicateRetry({
      apiKey,
      request,
      seenSellSignatures
    });
  }

  const percentBasisPoints = percentAmountToBasisPoints(request.amount);
  const result = percentBasisPoints === null
    ? tradeExecutionSkippedResult({
        provider,
        route: provider === "direct-pumpswap" ? "pumpswap-amm" : provider === "direct-pump" ? "pump-bonding-curve" : "auto",
        reason: "direct trailing sell requires a percent amount",
        platformFee: platformFeeResultFields(platformFee),
        metadata: { observedSignature: trade.signature }
      })
    : await executeDirectCopyTrade({
        subscriber,
        provider,
        request,
        amountLamports: percentBasisPoints,
        amountBasis: "percent",
        platformFee,
        metadata: {
          observedSignature: trade.signature,
          sourceWallet: trade.targetWallet,
          observedPool: trade.pool || request.pool || null,
          observedSource: trade.source || null
        }
      });

  return {
    result,
    duplicateSignature: Boolean(result.ok && result.signature && seenSellSignatures.has(result.signature))
  };
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
    logCopyTradeSignalRace(trade, "skipped", { reason });
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
  let copyBuySemanticReserved = false;
  let copyBuySemanticKey: string | null = null;
  let durableCopyBuyClaimKey: string | null = null;
  let deferredDurableCopyBuyClaim: DeferredCopyBuyIdempotencyClaim | null = null;
  let preBuyTokenBalance: number | null = null;
  let result: CopyTradeExecutionResult | null = null;

  try {
    const executionProvider = resolveExecutionProviderForTrade(trade);
    const fastDirectCopyBuyPath = isDirectTradeExecutionProvider(executionProvider);
    const executionSettings = copyTradeBuyExecutionSettings(subscriber);
    const request = buildPumpPortalLightningBuyRequest({
      trade,
      amountSol: subscriber.copyAmountSol,
      slippage: executionSettings.slippage,
      priorityFee: executionSettings.priorityFee,
      pool: config.copyTradePool
    });

    if (!request) {
      skipWithLatencyLog("could not build copy buy request");
      return;
    }
    latencyTracker.mark("request_built");

    const amountSol = typeof request.amount === "number" ? request.amount : null;
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

    const platformFee = calculateCopyTradePlatformFee("buy", solToLamports(amountSol));
    const platformFeeBlockedReason = copyTradePlatformFeeBlockedReason({
      action: "buy",
      platformFee,
      provider: executionProvider
    });

    if (platformFeeBlockedReason) {
      skipWithLatencyLog(platformFeeBlockedReason);
      await notifySkippedAutoCopyBuy({
        subscriber,
        trade,
        copyTradeWallet,
        request,
        reason: platformFeeBlockedReason
      });
      return;
    }

    const canaryBlockedReason = directCanaryBlockedReason({
      provider: executionProvider,
      chatId: subscriber.chatId,
      tradingWalletPublicKey: subscriber.tradingWallet.publicKey
    });
    if (canaryBlockedReason) {
      skipWithLatencyLog(canaryBlockedReason);
      await notifySkippedAutoCopyBuy({
        subscriber,
        trade,
        copyTradeWallet,
        request,
        reason: canaryBlockedReason
      });
      return;
    }

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

    if (shouldReserveDailyBudget(executionProvider)) {
      const durableCopyBuyKey = copyTradeBuyIdempotencyKey({
        chatId: subscriber.chatId,
        mint: trade.mint,
        action: "buy"
      });

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

      if (fastDirectCopyBuyPath) {
        copyBuySemanticKey = durableCopyBuyKey;

        if (!copyBuySemanticSubmissionGuard.reserve(copyBuySemanticKey)) {
          const reason = "copy buy coin is already in flight or was handled in this bot process";
          skipWithLatencyLog(reason);
          console.warn(
            `Skipping duplicate fast-path auto copy buy for ${subscriber.chatId}:${trade.mint}: coin already handled in memory`
          );
          return;
        }

        copyBuySemanticReserved = true;
      }

      const claimInput = {
        key: durableCopyBuyKey,
        chatId: subscriber.chatId,
        sourceWalletAddress: copyTradeWallet.address,
        tradingWalletPublicKey: subscriber.tradingWallet.publicKey,
        observedSignature: trade.signature,
        mint: trade.mint,
        amountSol,
        provider: trade.provider,
        request,
        retryFailed: subscriber.copyTradeRetryFailedBuys
      };

      if (fastDirectCopyBuyPath) {
        deferredDurableCopyBuyClaim = {
          key: durableCopyBuyKey,
          input: claimInput
        };
      } else {
        let idempotencyClaim;
        try {
          idempotencyClaim = await copyTradeBuyIdempotency.claimBuy(claimInput);
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
          const reason = existing?.status === "failed" && !subscriber.copyTradeRetryFailedBuys
            ? "copy buy coin was already handled (failed); enable Copy Repeat Buys in /copytrade settings to copy repeat same-coin target buys"
            : existing
              ? `copy buy coin was already handled (${existing.status})`
              : "copy buy coin was already handled";
          skipWithLatencyLog(reason);
          await notifySkippedAutoCopyBuy({
            subscriber,
            trade,
            copyTradeWallet,
            request,
            reason
          });
          console.warn(
            `Skipping duplicate auto copy buy for ${subscriber.chatId}:${trade.mint}: coin already handled`
          );
          return;
        }
        durableCopyBuyClaimKey = durableCopyBuyKey;
      }
    }

    const blockedReason = copyTradeSubmissionBlockedReason(executionProvider);
    latencyTracker.mark("live_gate_checked", {
      status: blockedReason ? "blocked" : "ok",
      reason: blockedReason
    });
    if (blockedReason) {
      if (durableCopyBuyClaimKey) {
        await safelyFailCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, blockedReason);
      }
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
        if (durableCopyBuyClaimKey) {
          await safelyFailCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, reserveBlockedReason);
        }
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
      if (durableCopyBuyClaimKey) {
        await safelyFailCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, reason);
      }
      skipWithLatencyLog(reason);
      console.warn(`Skipping auto copy buy for ${subscriber.chatId}: ${reason}`);
      return;
    }

    if (!copyBuySubmissionGuard.reserve(copyBuyKey)) {
      const reason = "copy buy is already in flight";
      if (durableCopyBuyClaimKey) {
        await safelyFailCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, reason);
      }
      skipWithLatencyLog(reason);
      console.warn(
        `Skipping duplicate auto copy buy for ${subscriber.chatId}:${copyTradeWallet.address}:${trade.signature}: already in flight`
      );
      return;
    }
    copyBuyReserved = true;

    const finalBlockedReason = copyTradeSubmissionBlockedReason(executionProvider);
    if (finalBlockedReason) {
      if (durableCopyBuyClaimKey) {
        await safelyFailCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, finalBlockedReason);
      }
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

    if (shouldReserveDailyBudget(executionProvider)) {
      const budgetReservation = copyTradeDailySolBudget.reserve({
        key: dailyBudgetKey,
        amountSol,
        capSol: config.copyTradeDailySolCap,
        nowMs: Date.now()
      });
      if (!budgetReservation.ok) {
        const reason = budgetReservation.reason || "daily copy buy budget exceeded";
        if (durableCopyBuyClaimKey) {
          await safelyFailCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, reason);
        }
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
    }

    if (copyTradeBuyPressureSellEnabledForSubscriber(subscriber) && trade.mint) {
      preBuyTokenBalance = await getTokenBalanceForWalletMint({
        owner: subscriber.tradingWallet.publicKey,
        mint: trade.mint
      });

      if (preBuyTokenBalance === null) {
        console.warn(`Could not snapshot pre-buy token balance for ${subscriber.chatId}:${trade.mint}`);
      }
    }

    latencyTracker.mark("submit_started");
    result = await executeCopyTradeBuy({
      subscriber,
      trade,
      request,
      provider: executionProvider,
      platformFee,
      onLatencyMilestone: (milestone, details) => latencyTracker.mark(milestone, details)
    });
    latencyTracker.mark("submit_finished", {
      status: resultOk(result) ? (isProviderNeutralResult(result) ? result.status : "submitted") : "failed",
      reason: resultOk(result) ? null : result.errorText,
      signature: resultSignature(result)
    });
    logLatencyOnce();
    console.log(`Copy trade execution result: ${isProviderNeutralResult(result) ? formatTradeExecutionResultLog(result) : JSON.stringify(result)}`);

    const legacyResult = legacyResultFromExecution(result);
    if (durableCopyBuyClaimKey) {
      safelyCompleteCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, legacyResult);
    } else if (deferredDurableCopyBuyClaim) {
      persistDeferredCopyTradeBuyIdempotency({
        claim: deferredDurableCopyBuyClaim,
        result: legacyResult
      });
    }

    await recordCopyTradeExecution({
      subscriber,
      trade,
      copyTradeWallet,
      tradingWalletPublicKey: subscriber.tradingWallet.publicKey,
      request,
      result
    }).catch((error) => {
      console.warn(`Could not record copy buy execution for ${subscriber.chatId}: ${errorMessage(error)}`);
    });

    const message = formatAutoCopyBuyMessage({
      trade,
      tradingWalletPublicKey: subscriber.tradingWallet.publicKey,
      copyAmountSol: subscriber.copyAmountSol,
      result
    });
    let copyBuyMessageId: number | null = null;

    if (message) {
      const telegramResult = await sendTelegramMessage({
        token: config.telegramToken,
        chatId: subscriber.chatId,
        text: message,
        replyMarkup: buildWalletTradeReplyMarkup(trade)
      }).catch((error) => {
        console.warn(`Could not send auto copy buy alert to ${subscriber.chatId}: ${errorMessage(error)}`);
        return null;
      });
      copyBuyMessageId = telegramResult?.result?.message_id ?? null;
    }

    if (resultOk(result) && resultSignature(result)) {
      const submittedFirst = isProviderNeutralResult(result) &&
        result.status === "submitted" &&
        config.directExecutionConfirmationMode === "background";
      const postSubmission = submittedFirst
        ? watchSubmittedCopyTradeBuy({
            subscriber,
            trade,
            copyTradeWallet,
            buySignature: resultSignature(result),
            executionProvider,
            preBuyTokenBalance,
            replyToMessageId: copyBuyMessageId
          })
        : scheduleCopyTradeTrailingSellsAfterConfirmation({
            subscriber,
            trade,
            copyTradeWallet,
            buySignature: resultSignature(result),
            executionProvider,
            preBuyTokenBalance,
            replyToMessageId: copyBuyMessageId
          });

      postSubmission.catch((error) => {
        console.warn(`Could not prepare copy buy post-confirmation work: ${errorMessage(error)}`);
      });
    }

  } catch (error) {
    if (durableCopyBuyClaimKey && !result) {
      await safelyFailCopyTradeBuyIdempotency(copyTradeBuyIdempotency, durableCopyBuyClaimKey, errorMessage(error));
    } else if (deferredDurableCopyBuyClaim && !result) {
      persistDeferredCopyTradeBuyIdempotency({
        claim: deferredDurableCopyBuyClaim,
        errorText: errorMessage(error)
      });
    }
    latencyTracker.skip(errorMessage(error), { status: "failed" });
    logLatencyOnce();
    console.warn(`Could not execute auto copy buy for ${subscriber.chatId}: ${errorMessage(error)}`);
  } finally {
    if (copyBuySemanticReserved && !(result && resultOk(result))) {
      copyBuySemanticSubmissionGuard.release(copyBuySemanticKey);
    }
    if (copyBuyReserved) {
      copyBuySubmissionGuard.release(copyBuyKey);
    }
  }
}

async function scheduleCopyTradeTrailingSellsAfterConfirmation({
  subscriber,
  trade,
  copyTradeWallet,
  buySignature,
  executionProvider,
  preBuyTokenBalance,
  replyToMessageId = null
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  copyTradeWallet: WatchedWallet;
  buySignature: string | null;
  executionProvider: TradeExecutionProvider;
  preBuyTokenBalance: number | null;
  replyToMessageId?: number | null;
}): Promise<void> {
  if (!buySignature) {
    await notifySkippedTrailingSellSchedule({
      subscriber,
      trade,
      reason: "copy buy did not return a transaction signature",
      replyToMessageId
    });
    return;
  }

  const confirmation = await waitForSignatureConfirmationResult(buySignature);
  if (!confirmation.confirmed) {
    await notifySkippedTrailingSellSchedule({
      subscriber,
      trade,
      reason: confirmation.errorText
        ? `copy buy was not confirmed: ${confirmation.errorText}`
        : "copy buy was not confirmed before trailing sell scheduling timeout",
      replyToMessageId
    });
    return;
  }

  await scheduleCopyTradePostConfirmationExits({
    subscriber,
    trade,
    copyTradeWallet,
    buySignature,
    preBuyTokenBalance,
    executionProvider,
    replyToMessageId
  });
}

async function scheduleCopyTradePostConfirmationExits({
  subscriber,
  trade,
  copyTradeWallet,
  buySignature,
  preBuyTokenBalance,
  executionProvider,
  replyToMessageId = null
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  copyTradeWallet: WatchedWallet;
  buySignature: string;
  preBuyTokenBalance: number | null;
  executionProvider: TradeExecutionProvider;
  replyToMessageId?: number | null;
}): Promise<void> {
  const postBuyTokenBalance = copyTradeBuyPressureSellEnabledForSubscriber(subscriber) && trade.mint
    ? await getTokenBalanceForWalletMint({
        owner: subscriber.tradingWallet?.publicKey || "",
        mint: trade.mint
      })
    : null;
  const buyPressureWatcherStarted = await startCopyTradeBuyPressureSellWatcher({
    subscriber,
    trade,
    copyTradeWallet,
    buySignature,
    preBuyTokenBalance,
    postBuyTokenBalance,
    executionProvider
  });

  if (buyPressureWatcherStarted) {
    return;
  }

  await scheduleCopyTradeTrailingSells({
    subscriber,
    trade,
    copyTradeWallet,
    executionProvider,
    replyToMessageId
  });
}

async function updateRecordedCopyTradeBuyStatus({
  subscriber,
  trade,
  buySignature,
  status,
  errorText = null
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  buySignature: string;
  status: Extract<CopyTradeExecutionStatus, "confirmed" | "expired" | "failed">;
  errorText?: string | null;
}): Promise<void> {
  if (!copyTradeRecorder?.updateCopyTradeExecutionStatus) {
    return;
  }

  try {
    await copyTradeRecorder.updateCopyTradeExecutionStatus({
      chatId: subscriber.chatId,
      action: "buy",
      signature: buySignature,
      status,
      errorText
    });
  } catch (error) {
    console.warn(`Could not update copy buy confirmation status for ${subscriber.chatId}:${buySignature}: ${errorMessage(error)}`);
  }
}

async function watchSubmittedCopyTradeBuy({
  subscriber,
  trade,
  copyTradeWallet,
  buySignature,
  executionProvider,
  preBuyTokenBalance,
  replyToMessageId = null
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  copyTradeWallet: WatchedWallet;
  buySignature: string | null;
  executionProvider: TradeExecutionProvider;
  preBuyTokenBalance: number | null;
  replyToMessageId?: number | null;
}): Promise<void> {
  if (!buySignature) {
    await notifySkippedTrailingSellSchedule({
      subscriber,
      trade,
      reason: "copy buy did not return a transaction signature",
      replyToMessageId
    });
    return;
  }

  const confirmation = await waitForSignatureConfirmationResult(buySignature);
  if (!confirmation.confirmed) {
    const errorText = confirmation.errorText || "copy buy was not confirmed before trailing sell scheduling timeout";
    console.warn(`Submitted copy buy did not confirm: ${buySignature}: ${errorText}`);
    await updateRecordedCopyTradeBuyStatus({
      subscriber,
      trade,
      buySignature,
      status: confirmation.timedOut ? "expired" : "failed",
      errorText
    });
    await sendCopyTradeBuyConfirmationUpdate({
      subscriber,
      trade,
      buySignature,
      confirmed: false,
      timedOut: confirmation.timedOut,
      errorText,
      replyToMessageId
    });
    return;
  }

  await updateRecordedCopyTradeBuyStatus({
    subscriber,
    trade,
    buySignature,
    status: "confirmed"
  });

  await sendCopyTradeBuyConfirmationUpdate({
    subscriber,
    trade,
    buySignature,
    confirmed: true,
    replyToMessageId
  });

  await scheduleCopyTradePostConfirmationExits({
    subscriber,
    trade,
    copyTradeWallet,
    buySignature,
    preBuyTokenBalance,
    executionProvider,
    replyToMessageId
  });
}

async function sendCopyTradeBuyConfirmationUpdate({
  subscriber,
  trade,
  buySignature,
  confirmed,
  timedOut = false,
  errorText = null,
  replyToMessageId = null
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  buySignature: string;
  confirmed: boolean;
  timedOut?: boolean;
  errorText?: string | null;
  replyToMessageId?: number | null;
}): Promise<void> {
  if (!trade.mint) {
    return;
  }

  const statusLine = confirmed ? "🟢 Buy confirmed" : timedOut ? "🟡 Confirmation timed out" : "🔴 Buy failed";
  const lines = [
    "<b>⚡ Auto Copy Buy</b>",
    statusLine,
    "",
    "<b>🪙 Contract Address</b>",
    `<code>${trade.mint}</code>`,
    "",
    `<b>Tx:</b> <code>${buySignature}</code>`
  ];
  if (!confirmed && errorText && !timedOut) {
    lines.push("", `<b>Reason:</b> ${escapeHtml(errorText)}`);
  }

  try {
    await sendTelegramMessage({
      token: config.telegramToken,
      chatId: subscriber.chatId,
      text: lines.join("\n"),
      replyToMessageId,
      replyMarkup: buildWalletTradeReplyMarkup(trade)
    });
  } catch (error) {
    console.warn(`Could not send copy buy confirmation update to ${subscriber.chatId}: ${errorMessage(error)}`);
  }
}

async function notifySkippedTrailingSellSchedule({
  subscriber,
  trade,
  reason,
  replyToMessageId = null
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  reason: string;
  replyToMessageId?: number | null;
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
      replyToMessageId,
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
      `intended copy buy ${request.amount} SOL of ${request.mint}`,
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
    `🟡 ${mode}; no copy order was submitted.`,
    "<b>Reason</b>",
    `└ <code>${reason}</code>`
  ].join("\n");
}

function copyTradeBuyPressureSellConfig() {
  return {
    enabled: config.copyTradeBuyPressureSellEnabled,
    sellPercent: config.copyTradeBuyPressureSellPercent,
    timeoutMs: config.copyTradeBuyPressureSellTimeoutMs,
    minBuys: config.copyTradeBuyPressureSellMinBuys,
    minTotalSol: config.copyTradeBuyPressureSellMinTotalSol
  };
}

function copyTradeBuyPressureSellEnabledForSubscriber(subscriber: SubscriberRecord): boolean {
  return isSubscriberLive(subscriber) && config.copyTradeBuyPressureSellEnabled && subscriber.copyTradeBuyPressureSellEnabled === true;
}

function copyTradeBuyPressureSellConfigForSubscriber(subscriber: SubscriberRecord) {
  return {
    ...copyTradeBuyPressureSellConfig(),
    enabled: copyTradeBuyPressureSellEnabledForSubscriber(subscriber),
    timeoutMs: subscriber.copyTradeBuyPressureSellTimeoutMs ?? config.copyTradeBuyPressureSellTimeoutMs
  };
}

function copyTradeRouteForProvider(provider: TradeExecutionProvider): TradeExecutionResult["route"] {
  return isDirectTradeExecutionProvider(provider) ? routeForDirectProvider(provider) : "pumpportal-lightning";
}

function parsedTokenAccountUiAmount(value: unknown): number {
  const record = asRecord(value);
  const parsed = asRecord(record.parsed);
  const info = asRecord(parsed.info);
  const tokenAmount = asRecord(info.tokenAmount);
  return finiteNumberValue(tokenAmount.uiAmountString) ?? finiteNumberValue(tokenAmount.uiAmount) ?? 0;
}

async function getTokenBalanceForWalletMint({
  owner,
  mint
}: {
  owner: string;
  mint: string;
}): Promise<number | null> {
  try {
    const response = await directSolanaConnection.getParsedTokenAccountsByOwner(
      new PublicKey(owner),
      {
        mint: new PublicKey(mint)
      },
      "confirmed"
    );

    return response.value.reduce((sum, account) => sum + parsedTokenAccountUiAmount(account.account.data), 0);
  } catch (error) {
    console.warn(`Could not fetch token balance for ${owner}:${mint}: ${errorMessage(error)}`);
    return null;
  }
}

async function copyTradePositionBalanceBlockedReason(watcher: CopyTradeBuyPressureSellWatcher): Promise<string | null> {
  const preBuyBalance = watcher.preBuyTokenBalance;
  const postBuyBalance = watcher.postBuyTokenBalance;

  if (preBuyBalance === null || postBuyBalance === null) {
    return "copy buy token balance snapshot is unavailable";
  }

  const positionTokenAmount = postBuyBalance - preBuyBalance;
  const tolerance = Math.max(Math.abs(positionTokenAmount) * 0.005, 0.000000001);

  if (preBuyBalance > tolerance) {
    return "trading wallet already held this mint before the copied buy";
  }

  if (positionTokenAmount <= tolerance) {
    return "copy buy did not increase the trading wallet token balance";
  }

  const currentBalance = await getTokenBalanceForWalletMint({
    owner: watcher.tradingWalletPublicKey,
    mint: watcher.mint
  });

  if (currentBalance === null) {
    return "could not verify current copied-position token balance";
  }

  if (currentBalance > positionTokenAmount + tolerance) {
    return "trading wallet token balance changed after the copied buy";
  }

  if (currentBalance <= 0) {
    return "copied-position token balance is already zero";
  }

  return null;
}

function refreshPumpPortalSubscriptions(): void {
  migrationListener.setSubscriptionMethods(activePumpPortalSubscriptions());
}

function refreshGeyserSubscriptions(): void {
  geyserWalletTradeListener.setWallets(activeGeyserWallets());
}

function clearBuyPressureSellTimer(watcherId: string): void {
  const timer = buyPressureSellTimers.get(watcherId);

  if (timer) {
    clearTimeout(timer);
    buyPressureSellTimers.delete(watcherId);
  }
}

async function persistActiveBuyPressureSellWatchers(): Promise<void> {
  await copyTradeBuyPressureSellStore.save([...activeBuyPressureSellWatchers.values()]);
}

function scheduleBuyPressureSellTimeout(watcher: CopyTradeBuyPressureSellWatcher): void {
  clearBuyPressureSellTimer(watcher.id);
  const delayMs = Math.min(Math.max(0, watcher.expiresAtMs - Date.now()), 2_147_483_647);
  const timer = setTimeout(() => {
    buyPressureSellTimers.delete(watcher.id);
    const activeWatcher = activeBuyPressureSellWatchers.get(watcher.id);

    if (!activeWatcher) {
      return;
    }

    const trigger = copyTradeBuyPressureTimeoutTrigger({ watcher: activeWatcher });
    if (!trigger) {
      return;
    }

    triggerCopyTradeBuyPressureSell({ watcher: activeWatcher, trigger }).catch((error) => {
      console.warn(`Could not run buy-pressure timeout sell for ${watcher.id}: ${errorMessage(error)}`);
    });
  }, delayMs);
  buyPressureSellTimers.set(watcher.id, timer);
}

async function loadCopyTradeBuyPressureSellWatchers(): Promise<void> {
  const storedWatchers = await copyTradeBuyPressureSellStore.load();

  if (!config.copyTradeBuyPressureSellEnabled) {
    if (storedWatchers.length > 0) {
      console.warn(
        `COPY_TRADE_BUY_PRESSURE_SELL_ENABLED is not true; ${storedWatchers.length} persisted buy-pressure sell watcher(s) will not resume`
      );
    }
    return;
  }

  for (const watcher of storedWatchers) {
    if (watcher.triggeredAtMs) {
      continue;
    }

    activeBuyPressureSellWatchers.set(watcher.id, watcher);
    scheduleBuyPressureSellTimeout(watcher);
  }

  if (activeBuyPressureSellWatchers.size > 0) {
    console.log(`Loaded ${activeBuyPressureSellWatchers.size} buy-pressure sell watcher(s)`);
  }

  await persistActiveBuyPressureSellWatchers();
}

async function startCopyTradeBuyPressureSellWatcher({
  subscriber,
  trade,
  copyTradeWallet,
  buySignature,
  preBuyTokenBalance,
  postBuyTokenBalance,
  executionProvider
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  copyTradeWallet: WatchedWallet;
  buySignature: string;
  preBuyTokenBalance: number | null;
  postBuyTokenBalance: number | null;
  executionProvider: TradeExecutionProvider;
}): Promise<boolean> {
  const watcher = createCopyTradeBuyPressureSellWatcher({
    config: copyTradeBuyPressureSellConfigForSubscriber(subscriber),
    subscriber,
    trade,
    copyTradeWallet,
    buySignature,
    executionProvider,
    preBuyTokenBalance,
    postBuyTokenBalance
  });

  if (!watcher) {
    return false;
  }

  if (activeBuyPressureSellWatchers.has(watcher.id)) {
    return true;
  }

  activeBuyPressureSellWatchers.set(watcher.id, watcher);
  scheduleBuyPressureSellTimeout(watcher);

  try {
    await persistActiveBuyPressureSellWatchers();
  } catch (error) {
    clearBuyPressureSellTimer(watcher.id);
    activeBuyPressureSellWatchers.delete(watcher.id);
    console.warn(`Could not persist buy-pressure sell watcher for ${subscriber.chatId}:${trade.mint}: ${errorMessage(error)}`);
    return false;
  }

  refreshPumpPortalSubscriptions();
  const message = formatCopyTradeBuyPressureSellScheduledMessage({ trade, watcher });

  if (message) {
    await sendTelegramMessage({
      token: config.telegramToken,
      chatId: subscriber.chatId,
      text: message,
      replyMarkup: buildWalletTradeReplyMarkup(trade)
    }).catch((error) => {
      console.warn(`Could not send buy-pressure sell watcher alert to ${subscriber.chatId}: ${errorMessage(error)}`);
    });
  }

  return true;
}

async function handleCopyTradeBuyPressureTrade(trade: WalletTradeData): Promise<void> {
  if (!config.copyTradeBuyPressureSellEnabled || activeBuyPressureSellWatchers.size === 0) {
    return;
  }

  const triggers: Array<{ watcher: CopyTradeBuyPressureSellWatcher; trigger: CopyTradeBuyPressureSellTrigger }> = [];
  let changed = false;

  for (const watcher of activeBuyPressureSellWatchers.values()) {
    const subscriber = subscribers.get(watcher.chatId);

    if (!subscriber || !isSubscriberLive(subscriber)) {
      activeBuyPressureSellWatchers.delete(watcher.id);
      clearBuyPressureSellTimer(watcher.id);
      changed = true;
      continue;
    }

    const result = applyCopyTradeBuyPressureTrade({ watcher, trade });

    if (!result.changed) {
      continue;
    }

    changed = true;
    activeBuyPressureSellWatchers.set(result.watcher.id, result.watcher);

    if (result.trigger) {
      triggers.push({ watcher: result.watcher, trigger: result.trigger });
    }
  }

  if (changed && triggers.length === 0) {
    await persistActiveBuyPressureSellWatchers();
  }

  for (const entry of triggers) {
    await triggerCopyTradeBuyPressureSell(entry);
  }
}

async function triggerCopyTradeBuyPressureSell({
  watcher,
  trigger
}: {
  watcher: CopyTradeBuyPressureSellWatcher;
  trigger: CopyTradeBuyPressureSellTrigger;
}): Promise<void> {
  if (!activeBuyPressureSellWatchers.has(watcher.id) || activeBuyPressureSellTriggers.has(watcher.id)) {
    return;
  }

  const subscriber = subscribers.get(watcher.chatId);

  if (!subscriber || !isSubscriberLive(subscriber)) {
    activeBuyPressureSellWatchers.delete(watcher.id);
    clearBuyPressureSellTimer(watcher.id);
    await persistActiveBuyPressureSellWatchers();
    return;
  }

  const claimedWatcher = claimCopyTradeBuyPressureSellTrigger({ watcher, trigger });
  activeBuyPressureSellTriggers.add(watcher.id);
  clearBuyPressureSellTimer(watcher.id);
  activeBuyPressureSellWatchers.set(watcher.id, claimedWatcher);

  try {
    await persistActiveBuyPressureSellWatchers();
  } catch (error) {
    activeBuyPressureSellWatchers.set(watcher.id, watcher);
    scheduleBuyPressureSellTimeout({ ...watcher, expiresAtMs: Date.now() + 5000 });
    activeBuyPressureSellTriggers.delete(watcher.id);
    console.warn(`Could not persist claimed buy-pressure sell watcher ${watcher.id}: ${errorMessage(error)}`);
    return;
  }

  try {
    await buildAndNotifyBuyPressureSell({
      watcher: claimedWatcher,
      trigger
    });
  } finally {
    activeBuyPressureSellWatchers.delete(watcher.id);
    try {
      await persistActiveBuyPressureSellWatchers();
    } catch (error) {
      console.warn(`Could not persist completed buy-pressure sell watcher ${watcher.id}: ${errorMessage(error)}`);
    }
    refreshPumpPortalSubscriptions();
    activeBuyPressureSellTriggers.delete(watcher.id);
  }
}

async function buildAndNotifyBuyPressureSell({
  watcher,
  trigger
}: {
  watcher: CopyTradeBuyPressureSellWatcher;
  trigger: CopyTradeBuyPressureSellTrigger;
}): Promise<CopyTradeExecutionResult> {
  const subscriber = subscribers.get(watcher.chatId);
  const copyTradeWallet = subscriber?.copyTradeWallets.find((wallet) => wallet.address === watcher.sourceWalletAddress) || watcher.copyTradeWallet;
  const executionSettings = subscriber
    ? copyTradeSellExecutionSettings(subscriber)
    : { slippage: config.copyTradeSlippage, priorityFee: config.copyTradePriorityFee };
  const request = buildPumpPortalLightningSellRequest({
    mint: watcher.mint,
    amountPercent: watcher.sellPercent,
    slippage: executionSettings.slippage,
    priorityFee: executionSettings.priorityFee,
    pool: config.copyTradePool
  });
  const fallbackRequest = request || {
    action: "sell" as const,
    mint: watcher.mint,
    amount: `${watcher.sellPercent}%` as `${number}%`,
    denominatedInSol: "false" as const,
    slippage: executionSettings.slippage,
    priorityFee: executionSettings.priorityFee,
    pool: config.copyTradePool
  };
  const latencyTracker = createCopyTradeLatencyTracker({
    chatId: watcher.chatId,
    sourceWallet: watcher.sourceWalletAddress,
    tradingWallet: watcher.tradingWalletPublicKey,
    observedSignature: watcher.observedSignature,
    mint: watcher.mint,
    mode: copyTradeLatencyMode()
  });
  latencyTracker.mark("request_built", { status: request ? "ok" : "failed" });

  const setupBlockedReason = !request
    ? "could not build buy-pressure sell request"
    : !subscriber
      ? "subscriber no longer exists"
      : subscriber.tradingWallet?.publicKey !== watcher.tradingWalletPublicKey
        ? "active trading wallet changed before buy-pressure sell"
        : null;
  const requestRiskBlockedReason = request ? copyTradeRequestRiskBlockedReason({ config, request }) : null;
  latencyTracker.mark("risk_checked", {
    status: setupBlockedReason || requestRiskBlockedReason ? "blocked" : "ok",
    reason: setupBlockedReason || requestRiskBlockedReason
  });

  const liveBlockedReason = request && !setupBlockedReason ? copyTradeSubmissionBlockedReason(watcher.executionProvider) : null;
  latencyTracker.mark("live_gate_checked", {
    status: liveBlockedReason ? "blocked" : "ok",
    reason: liveBlockedReason
  });
  const platformFee = calculateCopyTradePlatformFee("sell");
  const platformFeeBlockedReason = request && !setupBlockedReason
    ? copyTradePlatformFeeBlockedReason({
        action: "sell",
        platformFee,
        provider: watcher.executionProvider
      })
    : null;
  const positionBlockedReason = request && !setupBlockedReason && !requestRiskBlockedReason && !liveBlockedReason && !platformFeeBlockedReason
    ? await copyTradePositionBalanceBlockedReason(watcher)
    : null;

  const blockedReason = setupBlockedReason || liveBlockedReason || requestRiskBlockedReason || platformFeeBlockedReason || positionBlockedReason;
  let result: CopyTradeExecutionResult;
  let duplicateSignature = false;

  if (blockedReason || !subscriber || !request) {
    result = tradeExecutionSkippedResult({
      provider: watcher.executionProvider,
      route: copyTradeRouteForProvider(watcher.executionProvider),
      reason: `Buy-pressure sell skipped: ${blockedReason || "sell request unavailable"}`,
      metadata: {
        triggerKind: trigger.kind,
        triggerReason: trigger.reason,
        observedSignature: watcher.observedSignature,
        copyBuySignature: watcher.copyBuySignature
      }
    });
    latencyTracker.skip(blockedReason || "sell request unavailable", { status: "skipped" });
  } else {
    latencyTracker.mark("submit_started");
    const execution = await executeCopyTradeSell({
      subscriber,
      trade: watcher.trade,
      request,
      provider: watcher.executionProvider,
      seenSellSignatures: new Set<string>(),
      platformFee
    });
    result = execution.result;
    duplicateSignature = execution.duplicateSignature;
    latencyTracker.mark("submit_finished", {
      status: resultOk(result) ? (isProviderNeutralResult(result) ? result.status : "submitted") : "failed",
      reason: resultOk(result) ? null : result.errorText,
      signature: resultSignature(result)
    });
  }

  logCopyTradeLatency(latencyTracker);

  if (subscriber && request) {
    await recordCopyTradeExecution({
      subscriber,
      trade: watcher.trade,
      copyTradeWallet,
      tradingWalletPublicKey: watcher.tradingWalletPublicKey,
      request,
      result
    });
  }

  await sendTelegramMessage({
    token: config.telegramToken,
    chatId: watcher.chatId,
    text: formatCopyTradeBuyPressureSellResultMessage({
      trade: watcher.trade,
      trigger,
      request: request || fallbackRequest,
      result
    }),
    replyMarkup: buildWalletTradeReplyMarkup(watcher.trade)
  }).catch((error) => {
    console.warn(`Could not send buy-pressure sell result alert to ${watcher.chatId}: ${errorMessage(error)}`);
  });

  if (duplicateSignature) {
    console.warn(`Buy-pressure sell returned duplicate signature for ${watcher.id}`);
  }

  return result;
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
  copyTradeWallet,
  executionProvider,
  replyToMessageId = null
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  copyTradeWallet: WatchedWallet;
  executionProvider: TradeExecutionProvider;
  replyToMessageId?: number | null;
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
        replyToMessageId,
        replyMarkup: buildWalletTradeReplyMarkup(trade)
      });
    } catch (error) {
      console.warn(`Could not send trailing sell schedule alert to ${subscriber.chatId}: ${errorMessage(error)}`);
    }
  }

  runTrailingSellSchedule({
    subscriber,
    trade,
    steps,
    copyTradeWallet,
    scheduleKey,
    executionProvider,
    replyToMessageId
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
  steps,
  copyTradeWallet,
  scheduleKey,
  executionProvider,
  replyToMessageId = null
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  steps: Array<{ delayMs: number; request: PumpPortalLightningTradeRequest }>;
  copyTradeWallet: WatchedWallet;
  scheduleKey: string;
  executionProvider: TradeExecutionProvider;
  replyToMessageId?: number | null;
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
        request: step.request,
        stepIndex,
        totalSteps: steps.length,
        copyTradeWallet,
        seenSellSignatures,
        executionProvider,
        replyToMessageId
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
  request,
  stepIndex,
  totalSteps,
  copyTradeWallet,
  seenSellSignatures,
  executionProvider,
  replyToMessageId = null
}: {
  subscriber: SubscriberRecord;
  trade: WalletTradeData;
  request: PumpPortalLightningTradeRequest;
  stepIndex: number;
  totalSteps: number;
  copyTradeWallet: WatchedWallet;
  seenSellSignatures: Set<string>;
  executionProvider: TradeExecutionProvider;
  replyToMessageId?: number | null;
}): Promise<CopyTradeExecutionResult> {
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

  const liveBlockedReason = copyTradeSubmissionBlockedReason(executionProvider);
  latencyTracker.mark("live_gate_checked", {
    status: liveBlockedReason ? "blocked" : "ok",
    reason: liveBlockedReason
  });
  const platformFee = calculateCopyTradePlatformFee("sell");
  const platformFeeBlockedReason = copyTradePlatformFeeBlockedReason({
    action: "sell",
    platformFee,
    provider: executionProvider
  });

  const blockedReason = liveBlockedReason || requestRiskBlockedReason || platformFeeBlockedReason;
  if (blockedReason) {
    const result = tradeExecutionSkippedResult({
      provider: executionProvider,
      route: isDirectTradeExecutionProvider(executionProvider)
        ? executionProvider === "direct-pumpswap"
          ? "pumpswap-amm"
          : executionProvider === "direct-pump"
            ? "pump-bonding-curve"
            : "auto"
        : "pumpportal-lightning",
      reason: `Trailing sell skipped: ${blockedReason}`
    });

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
      replyToMessageId,
      replyMarkup: buildWalletTradeReplyMarkup(trade)
    });
    return result;
  }

  latencyTracker.mark("submit_started");
  const { result, duplicateSignature } = await executeCopyTradeSell({
    subscriber,
    trade,
    request,
    provider: executionProvider,
    seenSellSignatures,
    platformFee
  });
  const trailingSellSkipped = Boolean(result.errorText?.startsWith("Trailing sell skipped:"));
  latencyTracker.mark("submit_finished", {
    status: resultOk(result) ? (isProviderNeutralResult(result) ? result.status : "submitted") : trailingSellSkipped ? "skipped" : "failed",
    reason: resultOk(result) ? null : result.errorText,
    signature: resultSignature(result)
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
    replyToMessageId,
    replyMarkup: buildWalletTradeReplyMarkup(trade)
  });

  if (resultOk(result) && resultSignature(result) && !duplicateSignature) {
    seenSellSignatures.add(resultSignature(result) || "");
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

type SignatureConfirmationResult = {
  confirmed: boolean;
  timedOut: boolean;
  errorText: string | null;
};

async function waitForSignatureConfirmationResult(
  signature: string,
  timeoutMs = 30000,
  pollMs = 2000
): Promise<SignatureConfirmationResult> {
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
        const errorText = `transaction landed with error: ${JSON.stringify(status.err)}`;
        console.warn(`Signature ${signature} ${errorText}`);
        return { confirmed: false, timedOut: false, errorText };
      }

      if (status?.confirmationStatus === "confirmed" || status?.confirmationStatus === "finalized") {
        return { confirmed: true, timedOut: false, errorText: null };
      }
    } catch (error) {
      console.warn(`Could not check signature confirmation for ${signature}: ${errorMessage(error)}`);
    }

    await trackedDelay(pollMs);
  }

  console.warn(`Timed out waiting for signature confirmation: ${signature}`);
  return { confirmed: false, timedOut: true, errorText: "confirmation timed out" };
}

async function waitForSignatureConfirmation(signature: string, timeoutMs = 30000, pollMs = 2000): Promise<boolean> {
  return (await waitForSignatureConfirmationResult(signature, timeoutMs, pollMs)).confirmed;
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
  result: CopyTradeExecutionResult;
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
    status: isProviderNeutralResult(result) ? result.status : result.ok ? "submitted" : "failed",
    signature: resultSignature(result),
    errorText: result.errorText,
    httpStatus: isProviderNeutralResult(result) ? null : result.status,
    observedTrade: trade,
    request,
    response: isProviderNeutralResult(result)
      ? {
          status: result.status,
          provider: result.provider,
          route: result.route,
          signature: result.signature,
          submittedAtMs: result.submittedAtMs,
          confirmedAtMs: result.confirmedAtMs,
          slot: result.slot,
          metadata: result.metadata,
          platformFee: result.platformFee || null,
          raw: result.raw
        }
      : result.raw,
    trailingSellStepIndex,
    trailingSellTotalSteps
  };

  try {
    await copyTradeRecorder.recordCopyTradeExecution(record);
    await accrueCashbackFromCopyTradeExecution({
      record,
      trade,
      result,
      trailingSellStepIndex,
      trailingSellTotalSteps
    });
  } catch (error) {
    console.warn(`Could not record copy trade execution for ${subscriber.chatId}: ${errorMessage(error)}`);
  }
}

async function accrueCashbackFromCopyTradeExecution({
  record,
  trade,
  result,
  trailingSellStepIndex = null,
  trailingSellTotalSteps = null
}: {
  record: CopyTradeExecutionRecord;
  trade: WalletTradeData;
  result: CopyTradeExecutionResult;
  trailingSellStepIndex?: number | null;
  trailingSellTotalSteps?: number | null;
}): Promise<void> {
  if (!cashbackStore || !isProviderNeutralResult(result) || !isDirectTradeExecutionProvider(result.provider)) {
    return;
  }

  const executionKey = buildCashbackExecutionKey({
    chatId: record.chatId,
    tradingWalletPublicKey: record.tradingWalletPublicKey,
    sourceSignature: trade.signature,
    executionSignature: result.signature,
    action: record.action,
    trailingSellStepIndex,
    trailingSellTotalSteps
  });
  const accrual = buildCashbackAccrual({
    chatId: record.chatId,
    tradingWalletPublicKey: record.tradingWalletPublicKey,
    executionKey,
    sourceSignature: trade.signature,
    executionSignature: result.signature,
    action: record.action,
    status: record.status,
    provider: result.provider,
    platformFee: result.platformFee,
    trailingSellStepIndex,
    trailingSellTotalSteps,
    config: cashbackConfig
  });

  if (!accrual) {
    return;
  }

  try {
    await cashbackStore.accrue(accrual);
  } catch (error) {
    console.warn(`Could not accrue cashback for ${record.chatId}:${executionKey}: ${errorMessage(error)}`);
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
  return isSubscriberLive(subscriber) && (subscriber.mode === "both" || subscriber.mode === eventMode);
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
    return "<b>Bot is restarting for an update.</b>";
  }

  return "<b>Bot stopped.</b>";
}

function shouldNotifySubscribersOnShutdown(): boolean {
  return config.notifyOnShutdown;
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

function validTokenProgram(value: string | null): string | null {
  return value === TOKEN_PROGRAM_ADDRESS || value === TOKEN_2022_PROGRAM_ADDRESS ? value : null;
}

function directPumpFastBuyStateInputFromEvent(
  event: LooseRecord,
  tokenInfo: LooseRecord | null = null,
  tokenProgramOverride: string | null = null
) {
  const records = [event, tokenInfo];
  const mint = pickEventMint(event) || firstString([tokenInfo], ["mint"]);
  const creator = firstString(records, ["creatorAddress", "creator", "creatorPublicKey"]);
  const tokenProgram = validTokenProgram(tokenProgramOverride) || validTokenProgram(firstString(records, ["tokenProgram", "token_program"]));
  const virtualQuoteReserves = pickRawOrUiBigInt({
    records,
    rawKeys: ["virtual_quote_reserves", "virtualQuoteReserves", "virtual_sol_reserves", "virtualSolReserves"],
    uiKeys: ["vSolInBondingCurve", "virtualSolInBondingCurve"],
    decimals: SOL_DECIMALS
  });
  const virtualTokenReserves = pickRawOrUiBigInt({
    records,
    rawKeys: ["virtual_token_reserves", "virtualTokenReserves"],
    uiKeys: ["vTokensInBondingCurve", "virtualTokensInBondingCurve"],
    decimals: PUMP_TOKEN_DECIMALS
  });
  const realQuoteReserves = pickRawOrUiBigInt({
    records,
    rawKeys: ["real_quote_reserves", "realQuoteReserves", "real_sol_reserves", "realSolReserves"],
    uiKeys: ["realSolInBondingCurve"],
    decimals: SOL_DECIMALS
  }) ?? (virtualQuoteReserves ? virtualQuoteReserves - PUMP_DEFAULT_VIRTUAL_TO_REAL_QUOTE_OFFSET : null);
  const realTokenReserves = pickRawOrUiBigInt({
    records,
    rawKeys: ["real_token_reserves", "realTokenReserves"],
    uiKeys: ["realTokensInBondingCurve"],
    decimals: PUMP_TOKEN_DECIMALS
  }) ?? (virtualTokenReserves ? virtualTokenReserves - PUMP_DEFAULT_VIRTUAL_TO_REAL_TOKEN_OFFSET : null);
  const tokenTotalSupply = positiveBigInt(integerValueToBigInt(firstValue(records, ["total_supply", "totalSupply", "total_supply_str"])))
    ?? PUMP_DEFAULT_TOKEN_TOTAL_SUPPLY;

  if (!mint || !creator || !tokenProgram || !virtualQuoteReserves || !virtualTokenReserves || !realQuoteReserves || !realTokenReserves) {
    return null;
  }

  return {
    mint,
    creator,
    tokenProgram,
    virtualTokenReserves,
    virtualQuoteReserves,
    realTokenReserves,
    realQuoteReserves,
    tokenTotalSupply,
    complete: booleanFromRecords(records, ["complete"]),
    isMayhemMode: booleanFromRecords(records, ["isMayhemMode", "is_mayhem_mode"]),
    isCashbackCoin: booleanFromRecords(records, ["isCashbackCoin", "is_cashback_enabled", "cashbackEnabled"]),
    quoteMint: firstString(records, ["quoteMint", "quote_mint"]) || SYSTEM_PROGRAM_ADDRESS,
    source: "pumpportal-event"
  };
}

function primeDirectPumpFastBuyStateFromEvent(event: LooseRecord, tokenInfo: LooseRecord | null = null): boolean {
  const input = directPumpFastBuyStateInputFromEvent(event, tokenInfo);
  return input ? primeDirectPumpFastBuyState(input) : false;
}

function logDirectPumpFastStatePrefetchStats({ force = false }: { force?: boolean } = {}): void {
  const nowMs = Date.now();
  const shouldLog = force ||
    directPumpFastStatePrefetchStats.primed === 1 ||
    nowMs - directPumpFastStatePrefetchStats.lastLogAtMs > 60_000;

  if (!shouldLog) {
    return;
  }

  directPumpFastStatePrefetchStats.lastLogAtMs = nowMs;
  console.log(`Direct Pump fast state prefetch stats | scheduled=${directPumpFastStatePrefetchStats.scheduled} | primed=${directPumpFastStatePrefetchStats.primed} | directPrimed=${directPumpFastStatePrefetchStats.directPrimed} | missed=${directPumpFastStatePrefetchStats.missed} | failed=${directPumpFastStatePrefetchStats.failed} | invalidMint=${directPumpFastStatePrefetchStats.invalidMint} | dropped=${directPumpFastStatePrefetchStats.dropped} | inFlight=${directPumpFastStatePrefetchInFlight} | queued=${directPumpFastStatePrefetchQueue.length}`);
}

async function prefetchDirectPumpFastStateForEvent({
  mint,
  event
}: {
  mint: string;
  event: LooseRecord;
}): Promise<void> {
  const mintPublicKey = new PublicKey(mint);
  let lastError: unknown = null;

  for (const delayMs of DIRECT_PUMP_FAST_STATE_PREFETCH_RETRY_DELAYS_MS) {
    if (delayMs > 0) {
      await sleep(delayMs);
    }

    try {
      const chainSnapshot = await fetchDirectPumpFastBuyStateFromChain({
        connection: directSolanaConnection,
        mint: mintPublicKey,
        commitment: "processed"
      });
      const input = {
        ...chainSnapshot,
        source: "pumpportal-create-prefetch"
      };
      if (input) {
        const primed = primeDirectPumpFastBuyState(input);
        if (primed) {
          directPumpFastStatePrefetchStats.primed += 1;
        } else {
          directPumpFastStatePrefetchStats.failed += 1;
        }
        logDirectPumpFastStatePrefetchStats();
      }
      return;
    } catch (error) {
      lastError = error;
    }
  }

  const message = errorMessage(lastError);
  if (/mint account not found/i.test(message)) {
    directPumpFastStatePrefetchStats.missed += 1;
  } else {
    directPumpFastStatePrefetchStats.failed += 1;
    console.warn(`Direct Pump fast state prefetch failed for ${mint}: ${message}`);
  }
  logDirectPumpFastStatePrefetchStats();
}

function runDirectPumpFastStatePrefetchQueue(): void {
  while (
    directPumpFastStatePrefetchInFlight < DIRECT_PUMP_FAST_STATE_PREFETCH_MAX_IN_FLIGHT &&
    directPumpFastStatePrefetchQueue.length > 0
  ) {
    const next = directPumpFastStatePrefetchQueue.shift();
    if (!next) {
      return;
    }

    let mintPublicKey: PublicKey;
    try {
      mintPublicKey = new PublicKey(next.mint);
    } catch {
      directPumpFastStatePrefetchStats.invalidMint += 1;
      directPumpFastStatePrefetchPending.delete(next.mint);
      logDirectPumpFastStatePrefetchStats();
      continue;
    }

    directPumpFastStatePrefetchInFlight += 1;
    void mintPublicKey;
    prefetchDirectPumpFastStateForEvent(next).finally(() => {
      directPumpFastStatePrefetchInFlight = Math.max(0, directPumpFastStatePrefetchInFlight - 1);
      directPumpFastStatePrefetchPending.delete(next.mint);
      runDirectPumpFastStatePrefetchQueue();
    });
  }
}

function scheduleDirectPumpFastBuyStatePrime(event: LooseRecord): void {
  if (primeDirectPumpFastBuyStateFromEvent(event)) {
    directPumpFastStatePrefetchStats.directPrimed += 1;
    logDirectPumpFastStatePrefetchStats();
  }

  const mint = pickEventMint(event);
  if (!mint || directPumpFastStatePrefetchPending.has(mint)) {
    return;
  }

  if (directPumpFastStatePrefetchQueue.length >= DIRECT_PUMP_FAST_STATE_PREFETCH_MAX_QUEUED) {
    directPumpFastStatePrefetchStats.dropped += 1;
    logDirectPumpFastStatePrefetchStats();
    return;
  }

  directPumpFastStatePrefetchStats.scheduled += 1;
  directPumpFastStatePrefetchPending.add(mint);
  directPumpFastStatePrefetchQueue.push({ mint, event });
  runDirectPumpFastStatePrefetchQueue();
}

function refreshDirectPumpFastBuyStateFromTrade(trade: WalletTradeData): boolean {
  if (trade.provider !== "pumpportal" || trade.pool !== "pump" || !trade.mint) {
    return false;
  }

  const raw = asRecord(trade.raw);
  const virtualQuoteReserves = pickRawOrUiBigInt({
    records: [raw],
    rawKeys: ["virtual_quote_reserves", "virtualQuoteReserves", "virtual_sol_reserves", "virtualSolReserves"],
    uiKeys: ["vSolInBondingCurve", "virtualSolInBondingCurve"],
    decimals: SOL_DECIMALS
  });
  const virtualTokenReserves = pickRawOrUiBigInt({
    records: [raw],
    rawKeys: ["virtual_token_reserves", "virtualTokenReserves"],
    uiKeys: ["vTokensInBondingCurve", "virtualTokensInBondingCurve"],
    decimals: PUMP_TOKEN_DECIMALS
  });

  if (!virtualQuoteReserves || !virtualTokenReserves) {
    return false;
  }

  return refreshDirectPumpFastBuyStateReserves({
    mint: trade.mint,
    virtualQuoteReserves,
    virtualTokenReserves,
    source: "pumpportal-account-trade",
    observedAtMs: Date.parse(trade.observedAt) || Date.now()
  });
}

function prefetchDirectPumpFastBuyStateForTrade(trade: WalletTradeData): boolean {
  if (
    !isDirectTradeExecutionProvider(config.copyTradeExecutionProvider) ||
    trade.action !== "buy" ||
    trade.pool !== "pump" ||
    !trade.mint
  ) {
    return false;
  }

  let mint: PublicKey;
  try {
    mint = new PublicKey(trade.mint);
  } catch {
    return false;
  }

  prefetchDirectPumpFastBuyStateFromChain({
    connection: directSolanaConnection,
    mint,
    commitment: "processed",
    source: `${trade.provider}-observed-buy-prefetch`
  }).catch((error) => {
    console.warn(`Direct Pump observed-buy prefetch failed for ${trade.mint}: ${errorMessage(error)}`);
  });
  return true;
}

function activeCopyTradeEntriesForTarget(targetWallet: string): Array<{
  subscriber: SubscriberRecord;
  wallet: WatchedWallet;
  label: string | null;
}> {
  return subscribers
    .list()
    .filter(isSubscriberLive)
    .flatMap((subscriber) =>
      (subscriber.copyTradeWallets || [])
        .filter((wallet) => wallet.address === targetWallet)
        .map((wallet) => ({ subscriber, wallet, label: wallet.label }))
    );
}

function isSubscriberLive(subscriber: SubscriberRecord): boolean {
  return subscriber.notificationsPaused !== true;
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

async function writePumpPortalDiscoveryLog(event: unknown): Promise<void> {
  await mkdir(dirname(config.pumpPortalDiscoveryLogPath), { recursive: true });
  await appendFile(config.pumpPortalDiscoveryLogPath, `${JSON.stringify(event)}\n`);
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
    refreshPumpPortalSubscriptions();
    yellowstoneWalletMonitor.setWallets(watchedWalletAddresses());
    shredstreamWalletObserver.setWallets(activeGeyserWallets());
    refreshGeyserSubscriptions();
    return syncHeliusWalletWebhook();
  },
  onCopyTradeEmergencyStop: (chatId) => {
    return activateCopyTradeEmergencyStop(chatId);
  },
  onCopyTradeEmergencyResume: (chatId) => {
    return clearCopyTradeEmergencyStop(chatId);
  },
  cashback: cashbackConfig.enabled && cashbackStore
    ? {
        getSummary: ({ chatId, tradingWalletPublicKey, payoutWalletPublicKey }) =>
          cashbackStore.getSummary({ chatId, tradingWalletPublicKey, payoutWalletPublicKey, config: cashbackConfig }),
        claim: ({ chatId, tradingWalletPublicKey, payoutWalletPublicKey }) =>
          claimCashback({
            store: cashbackStore,
            config: cashbackConfig,
            connection: directSolanaConnection,
            chatId,
            tradingWalletPublicKey,
            payoutWalletPublicKey
          })
      }
    : undefined
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

const geyserWalletTradeListener = createGeyserWalletTradeListener({
  enabled: config.geyserEnabled || copyTradeSignalProviderAllows(config.copyTradeSignalProvider, "geyser"),
  endpoint: config.geyserGrpcUrl,
  xToken: config.geyserXToken,
  wallets: activeGeyserWallets(),
  config,
  onTrade: handleGeyserWalletTrade,
  onReject: handleGeyserWalletTradeReject,
  onStatus: (message: string) => console.log(message),
  onError: (error: Error) => console.error("Geyser stream error:", error.message)
});

const heliusWebhookServer = createHeliusWebhookServer({
  authHeader: config.heliusWebhookAuthHeader,
  port: config.webhookPort,
  onEvents: handleHeliusWebhookEvents
});
const yellowstoneWalletMonitor = createYellowstoneWalletMonitor({
  enabled: config.yellowstoneEnabled,
  endpoint: config.yellowstoneEndpoint,
  token: config.yellowstoneToken,
  commitment: config.yellowstoneCommitment,
  reconnectMs: config.yellowstoneReconnectMs,
  shadowOnly: config.yellowstoneShadowOnly,
  wallets: watchedWalletAddresses(),
  explorer: {
    pumpFunBaseUrl: config.pumpFunBaseUrl,
    solscanBaseUrl: config.solscanBaseUrl
  },
  onTrade: handleYellowstoneWalletTrade,
  onStatus: (message) => console.log(message),
  onError: (error) => console.error("Yellowstone gRPC error:", error.message)
});

const shredstreamWalletObserver = createShredstreamWalletObserver({
  enabled: config.shredstreamWalletObserverEnabled || copyTradeSignalProviderAllows(config.copyTradeSignalProvider, "shredstream"),
  wallets: activeGeyserWallets(),
  explorer: {
    pumpFunBaseUrl: config.pumpFunBaseUrl,
    solscanBaseUrl: config.solscanBaseUrl
  },
  statsIntervalMs: config.shredstreamWalletObserverStatsIntervalMs,
  isDiagnosticWallet: (wallet) => Boolean(diagnosticWallet(wallet.address)),
  onTrade: handleShredstreamWalletTrade,
  onStatus: (message) => console.log(message),
  onError: (error) => console.error("ShredStream wallet observer error:", error.message)
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
  yellowstoneWalletMonitor.stop();
  shredstreamWalletObserver.stop();
  migrationListener.stop();
  if (directBlockhashWarmTimer) {
    clearInterval(directBlockhashWarmTimer);
    directBlockhashWarmTimer = null;
  }
  if (directSdkWarmTimer) {
    clearInterval(directSdkWarmTimer);
    directSdkWarmTimer = null;
  }
  geyserWalletTradeListener.stop();
  for (const timer of trailingSellTimers) {
    clearTimeout(timer);
  }
  trailingSellTimers.clear();
  for (const timer of buyPressureSellTimers.values()) {
    clearTimeout(timer);
  }
  buyPressureSellTimers.clear();
  await heliusWebhookServer.stop();

  if (shouldNotifySubscribersOnShutdown() && config.telegramToken && subscribers.count() > 0) {
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
await loadCopyTradeBuyPressureSellWatchers();
console.log(`Using ${subscriberStoreLabel} subscriber storage`);
console.log(`Loaded ${subscribers.count()} verified Telegram subscriber(s)`);
logCopyTradeExecutionState();
warmPlatformFeeTreasuryAccount();
warmLocalSolanaSignerCache();
warmDirectExecutionHotPath();
warmDirectExecutionBlockhashCache();
  // Supabase-backed subscribers load at startup, so compute account-trade
  // subscriptions after init rather than from the empty pre-init store.
refreshPumpPortalSubscriptions();
yellowstoneWalletMonitor.setWallets(watchedWalletAddresses());
shredstreamWalletObserver.setWallets(activeGeyserWallets());
refreshGeyserSubscriptions();
if (watchedWalletAddresses().length > 0) {
  await syncHeliusWalletWebhook();
}
if (config.heliusWebhookAuthHeader) {
  await heliusWebhookServer.start();
} else {
  console.warn(missingHeliusConfigWarning());
}
yellowstoneWalletMonitor.start();
shredstreamWalletObserver.start();
migrationListener.start();
geyserWalletTradeListener.start();
await commandPoller.start();
