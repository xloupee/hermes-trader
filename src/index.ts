import "dotenv/config";
import { appendFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import { buildMigrationReplyMarkup, extractMigrationData, formatMigrationMessage, getEventId } from "./format.js";
import { createTelegramCommandPoller } from "./commands.js";
import { createHeliusWebhookServer, missingHeliusConfigWarning, syncHeliusWebhook } from "./helius.js";
import { heliusEventMentionsWatchedWallet, isHeliusSwapEvent, normalizeHeliusSwapData } from "./helius-swaps.js";
import {
  buildPumpPortalLocalTrade,
  buildPumpPortalLocalTradeRequest,
  createPumpPortalMigrationListener,
  PUMPPORTAL_TRADE_LOCAL_URL
} from "./pumpportal.js";
import { analyzeSolanaTransaction } from "./solana.js";
import { createSubscriberStore } from "./subscribers.js";
import { createSupabaseSubscriberStoreFromEnv } from "./subscribers-supabase.js";
import { sendTelegramMessage, sendTelegramPhoto } from "./telegram.js";
import { asRecord, errorMessage, isRecord, stringValue } from "./types.js";
import {
  buildWalletTradeReplyMarkup,
  formatCopyTradeSimulationMessage,
  formatWalletTradeMessageWithCopySettings,
  getWalletTradeEventId,
  isCopyableSolToTokenBuy
} from "./wallet-monitor.js";
import type { AlertModeValue, BotConfig, LooseRecord, MigrationData, PumpPortalTradePool, SubscriberRecord, TransactionAnalysis, WalletTradeData } from "./types.js";

function numberFromEnv(value: string | undefined, fallback: number): number {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
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
  alertMode: process.env.ALERT_MODE || process.env.PUMPPORTAL_ALERT_MODE || "migrations",
  solscanBaseUrl: process.env.SOLSCAN_BASE_URL || "https://solscan.io",
  pumpFunBaseUrl: process.env.PUMPFUN_BASE_URL || "https://pump.fun",
  migrationLogPath: process.env.MIGRATION_LOG_PATH || "logs/migrations.jsonl",
  walletTradeLogPath: process.env.WALLET_TRADE_LOG_PATH || "logs/wallet-trades.jsonl",
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
  copyTradeSlippage: numberFromEnv(process.env.COPY_TRADE_SLIPPAGE, 10),
  copyTradePriorityFee: numberFromEnv(process.env.COPY_TRADE_PRIORITY_FEE, 0.00005),
  copyTradePool: pumpPortalPoolFromEnv(process.env.COPY_TRADE_POOL)
};

const seenEvents = new Set<string>();
let cachedSolUsdPrice: number | undefined;
let cachedSolUsdPriceAt = 0;
const metadataCache = new Map<string, LooseRecord | null>();
const tokenInfoCache = new Map<string, LooseRecord | null>();
let isShuttingDown = false;
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

const baseSubscriptionMethods = ["subscribeNewToken", "subscribeMigration"];

function watchedWalletAddresses(): string[] {
  return [
    ...new Set(
      subscribers
        .list()
        .flatMap((subscriber) => subscriber.watchedWallets || [])
        .map((wallet) => wallet.address)
        .filter(Boolean)
    )
  ].sort();
}

function activePumpPortalSubscriptions(): string[] {
  return [...baseSubscriptionMethods];
}

function activeSubscriptionMethodNames(): string[] {
  return activePumpPortalSubscriptions();
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
  const eventMode = classifyEventMode(event);

  if (eventMode) {
    await handleMigration(event);
    return;
  }

  console.warn(`Skipping unknown PumpPortal event type: ${JSON.stringify(event)}`);
}

async function handleHeliusWebhookEvents(events: LooseRecord[]): Promise<void> {
  for (const event of events) {
    if (!isHeliusSwapEvent(event)) {
      continue;
    }

    await handleHeliusSwap(event);
  }
}

async function handleHeliusSwap(event: LooseRecord): Promise<boolean> {
  const subscribersByWallet = new Map<string, Array<{ subscriber: SubscriberRecord; label: string | null }>>();

  for (const subscriber of subscribers.list()) {
    for (const wallet of subscriber.watchedWallets || []) {
      if (!heliusEventMentionsWatchedWallet(event, wallet.address)) {
        continue;
      }

      const entries = subscribersByWallet.get(wallet.address) || [];
      entries.push({ subscriber, label: wallet.label });
      subscribersByWallet.set(wallet.address, entries);
    }
  }

  if (subscribersByWallet.size === 0) {
    return false;
  }

  for (const [targetWallet, entries] of subscribersByWallet) {
    const loggedTrade = normalizeHeliusSwapData({
      event,
      targetWallet,
      label: null,
      config
    });
    const eventId = getWalletTradeEventId(loggedTrade);

    if (rememberEvent(eventId)) {
      continue;
    }

    await writeWalletTradeLog(loggedTrade);
    console.log(`Wallet trade event: ${JSON.stringify(loggedTrade)}`);

    for (const entry of entries) {
      await sendWalletTradeAlert(entry.subscriber, {
        ...loggedTrade,
        label: entry.label
      });
    }
  }

  return true;
}

async function sendWalletTradeAlert(subscriber: SubscriberRecord, trade: WalletTradeData): Promise<void> {
  const copyWalletAddresses = subscriber.copyWalletAddresses.length > 0
    ? subscriber.copyWalletAddresses
    : subscriber.copyWalletAddress
      ? [subscriber.copyWalletAddress]
      : [];
  const copySettings =
    subscriber.copyTargetWalletAddress && subscriber.copyTargetWalletAddress === trade.targetWallet
      ? {
          copyWalletAddress: copyWalletAddresses[0] || null,
          copyWalletAddresses,
          copyAmountSol: subscriber.copyAmountSol,
          copyTargetWalletAddress: subscriber.copyTargetWalletAddress
        }
      : null;

  try {
    await sendTelegramMessage({
      token: config.telegramToken,
      chatId: subscriber.chatId,
      text: formatWalletTradeMessageWithCopySettings(trade, copySettings),
      replyMarkup: buildWalletTradeReplyMarkup(trade)
    });

    if (copySettings && isCopyableSolToTokenBuy(trade)) {
      for (const copyWalletAddress of copyWalletAddresses) {
        const perWalletCopySettings = {
          ...copySettings,
          copyWalletAddress,
          copyWalletAddresses: [copyWalletAddress]
        };
        const pumpPortalRequest = buildPumpPortalLocalTradeRequest({
          trade,
          copySettings: perWalletCopySettings,
          slippage: config.copyTradeSlippage,
          priorityFee: config.copyTradePriorityFee,
          pool: config.copyTradePool
        });
        const pumpPortalBuild = pumpPortalRequest
          ? await buildPumpPortalLocalTrade({
              url: config.pumpPortalTradeLocalUrl,
              request: pumpPortalRequest
            })
          : null;
        const copyTradeSimulationMessage = formatCopyTradeSimulationMessage(trade, perWalletCopySettings, pumpPortalBuild);

        if (copyTradeSimulationMessage) {
          await sendTelegramMessage({
            token: config.telegramToken,
            chatId: subscriber.chatId,
            text: copyTradeSimulationMessage,
            replyMarkup: buildWalletTradeReplyMarkup(trade)
          });
        }
      }
    }
  } catch (error) {
    console.warn(`Could not send wallet trade alert to ${subscriber.chatId}: ${errorMessage(error)}`);
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
    return syncHeliusWalletWebhook();
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
console.log(`Using ${subscriberStoreLabel} subscriber storage`);
console.log(`Loaded ${subscribers.count()} verified Telegram subscriber(s)`);
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
