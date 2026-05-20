import "dotenv/config";
import { appendFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import { buildMigrationReplyMarkup, extractMigrationData, formatMigrationMessage, getEventId } from "./format.js";
import { createTelegramCommandPoller } from "./commands.js";
import { createPumpPortalMigrationListener } from "./pumpportal.js";
import { analyzeSolanaTransaction } from "./solana.js";
import { sendTelegramMessage, sendTelegramPhoto } from "./telegram.js";

const config = {
  telegramToken: process.env.TELEGRAM_BOT_TOKEN,
  telegramChatId: process.env.TELEGRAM_CHAT_ID,
  pumpPortalApiKey: process.env.PUMPPORTAL_API_KEY,
  pumpPortalWsUrl: process.env.PUMPPORTAL_WS_URL || "wss://pumpportal.fun/api/data",
  alertMode: process.env.ALERT_MODE || process.env.PUMPPORTAL_ALERT_MODE || "migrations",
  solscanBaseUrl: process.env.SOLSCAN_BASE_URL || "https://solscan.io",
  pumpFunBaseUrl: process.env.PUMPFUN_BASE_URL || "https://pump.fun",
  migrationLogPath: process.env.MIGRATION_LOG_PATH || "logs/migrations.jsonl",
  pumpFunCoinApiBaseUrl: process.env.PUMPFUN_COIN_API_BASE_URL || "https://frontend-api-v3.pump.fun/coins",
  solUsdPriceUrl: process.env.SOL_USD_PRICE_URL || "https://api.coinbase.com/v2/prices/SOL-USD/spot",
  solanaRpcUrl: process.env.SOLANA_RPC_URL || "https://api.mainnet-beta.solana.com",
  transactionFlowEnabled: process.env.TRANSACTION_FLOW_ENABLED === "true",
  transactionAccountLabels: process.env.TRANSACTION_ACCOUNT_LABELS
};

const seenEvents = new Set();
let cachedSolUsdPrice;
let cachedSolUsdPriceAt = 0;
const metadataCache = new Map();
const tokenInfoCache = new Map();
let isShuttingDown = false;

const alertModes = {
  migrations: {
    label: "Migrated coins only",
    methods: ["subscribeMigration"]
  },
  migration: {
    label: "Migrated coins only",
    methods: ["subscribeMigration"]
  },
  newtokens: {
    label: "New tokens only",
    methods: ["subscribeNewToken"]
  },
  newtoken: {
    label: "New tokens only",
    methods: ["subscribeNewToken"]
  },
  tokens: {
    label: "New tokens only",
    methods: ["subscribeNewToken"]
  },
  both: {
    label: "New tokens and migrated coins",
    methods: ["subscribeNewToken", "subscribeMigration"]
  },
  all: {
    label: "New tokens and migrated coins",
    methods: ["subscribeNewToken", "subscribeMigration"]
  }
};

let currentAlertMode = normalizeAlertMode(config.alertMode);

function rememberEvent(id) {
  if (!id) {
    return false;
  }

  if (seenEvents.has(id)) {
    return true;
  }

  seenEvents.add(id);

  if (seenEvents.size > 1000) {
    const oldest = seenEvents.values().next().value;
    seenEvents.delete(oldest);
  }

  return false;
}

function getModeLabel() {
  return currentAlertMode.label;
}

function getSubscriptionMethods() {
  return currentAlertMode.methods;
}

async function setAlertMode(requestedMode) {
  const nextMode = alertModes[requestedMode];

  if (!nextMode) {
    return { ok: false };
  }

  if (sameMethods(currentAlertMode.methods, nextMode.methods)) {
    return { ok: true, label: nextMode.label };
  }

  currentAlertMode = nextMode;
  config.alertMode = requestedMode;
  seenEvents.clear();
  migrationListener.setSubscriptionMethods(nextMode.methods);
  console.log(`Alert mode changed to ${nextMode.label} (${nextMode.methods.join(", ")})`);

  return { ok: true, label: nextMode.label };
}

function normalizeAlertMode(value) {
  return alertModes[String(value || "").toLowerCase()] || alertModes.migrations;
}

function sameMethods(left, right) {
  return left.length === right.length && left.every((method) => right.includes(method));
}

async function handleMigration(event) {
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
    alertModeLabel: getModeLabel(),
    activeSubscriptionMethods: getSubscriptionMethods(),
    solUsdPrice,
    tokenInfo,
    metadata,
    transactionAnalysis
  };
  const migration = extractMigrationData(event, eventConfig);
  await writeMigrationLog(migration);
  console.log(`Migration: ${JSON.stringify(migration)}`);

  if (!config.telegramChatId) {
    console.warn("Skipping migration alert because TELEGRAM_CHAT_ID is not configured");
    return;
  }

  const text = formatMigrationMessage(event, eventConfig);
  const replyMarkup = buildMigrationReplyMarkup(event, eventConfig);

  if (migration.imageUrl) {
    try {
      await sendTelegramPhoto({
        token: config.telegramToken,
        chatId: config.telegramChatId,
        photoUrl: migration.imageUrl,
        caption: text,
        replyMarkup
      });
      return;
    } catch (error) {
      console.warn(`Could not send token photo; sending text alert instead: ${error.message}`);
    }
  }

  await sendTelegramMessage({ token: config.telegramToken, chatId: config.telegramChatId, text, replyMarkup });
}

async function getTransactionAnalysis(event) {
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
    console.warn(`Could not analyze transaction SOL flow: ${error.message}`);
    return null;
  }
}

function isMigrationEvent(event) {
  const eventType = String(event?.txType ?? event?.type ?? event?.eventType ?? "").toLowerCase();

  if (eventType === "migration" || eventType === "migrate") {
    return true;
  }

  if (eventType === "create") {
    return false;
  }

  return getSubscriptionMethods().includes("subscribeMigration");
}

async function getSolUsdPrice() {
  const cacheMs = 60000;

  if (cachedSolUsdPrice && Date.now() - cachedSolUsdPriceAt < cacheMs) {
    return cachedSolUsdPrice;
  }

  try {
    const response = await fetch(config.solUsdPriceUrl);
    const body = await response.json();
    const price = Number(body?.data?.amount ?? body?.price ?? body?.solana?.usd);

    if (!response.ok || !Number.isFinite(price)) {
      throw new Error(`Unexpected SOL price response: ${response.status}`);
    }

    cachedSolUsdPrice = price;
    cachedSolUsdPriceAt = Date.now();
    return price;
  } catch (error) {
    console.warn(`Could not fetch SOL/USD price; falling back to SOL market cap: ${error.message}`);
    return cachedSolUsdPrice;
  }
}

function normalizeIpfsUrl(value) {
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

async function getPumpFunCoinInfo(event) {
  const mint = event?.mint || event?.ca || event?.token || event?.tokenAddress || event?.address;

  if (!mint) {
    return null;
  }

  if (tokenInfoCache.has(mint)) {
    return tokenInfoCache.get(mint);
  }

  try {
    const response = await fetch(`${config.pumpFunCoinApiBaseUrl}/${mint}`, {
      headers: {
        accept: "application/json"
      }
    });
    const tokenInfo = await response.json();

    if (!response.ok || !tokenInfo || typeof tokenInfo !== "object") {
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
      tokenInfoCache.delete(oldest);
    }

    return normalizedTokenInfo;
  } catch (error) {
    console.warn(`Could not fetch Pump.fun coin info: ${error.message}`);
    tokenInfoCache.set(mint, null);
    return null;
  }
}

async function getTokenMetadata(event, tokenInfo) {
  const uri = normalizeIpfsUrl(event?.uri || event?.metadataUri || event?.metadata || tokenInfo?.metadata_uri);

  if (!uri || typeof uri !== "string" || !uri.startsWith("http")) {
    return tokenInfo ? metadataFromTokenInfo(tokenInfo) : null;
  }

  if (metadataCache.has(uri)) {
    return metadataCache.get(uri);
  }

  try {
    const response = await fetch(uri);
    const metadata = await response.json();

    if (!response.ok || !metadata || typeof metadata !== "object") {
      throw new Error(`Unexpected metadata response: ${response.status}`);
    }

    const normalizedMetadata = {
      ...metadata,
      image: normalizeIpfsUrl(metadata.image || metadata.image_url || metadata.imageUrl)
    };

    metadataCache.set(uri, normalizedMetadata);

    if (metadataCache.size > 500) {
      const oldest = metadataCache.keys().next().value;
      metadataCache.delete(oldest);
    }

    return {
      ...metadataFromTokenInfo(tokenInfo),
      ...normalizedMetadata
    };
  } catch (error) {
    console.warn(`Could not fetch token metadata: ${error.message}`);
    metadataCache.set(uri, null);
    return tokenInfo ? metadataFromTokenInfo(tokenInfo) : null;
  }
}

function metadataFromTokenInfo(tokenInfo) {
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

async function writeMigrationLog(migration) {
  await mkdir(dirname(config.migrationLogPath), { recursive: true });
  await appendFile(config.migrationLogPath, `${JSON.stringify(migration)}\n`);
}

function testMigrationMessage() {
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
        is_cashback_enabled: true,
        tokenized_agent: true
      }
    }
  );
}

function assertConfig() {
  const missing = [];

  if (!config.telegramToken) {
    missing.push("TELEGRAM_BOT_TOKEN");
  }

  if (missing.length > 0) {
    throw new Error(`Missing required env vars: ${missing.join(", ")}`);
  }
}

const commandPoller = createTelegramCommandPoller({
  config,
  testMessage: testMigrationMessage,
  getModeLabel,
  setAlertMode
});
const migrationListener = createPumpPortalMigrationListener({
  pumpPortalWsUrl: config.pumpPortalWsUrl,
  pumpPortalApiKey: config.pumpPortalApiKey,
  subscriptionMethods: getSubscriptionMethods(),
  onMigration: (event) => {
    handleMigration(event).catch((error) => {
      console.error("Failed to process migration event:", error);
    });
  },
  onStatus: (message) => console.log(message),
  onError: (error) => console.error("PumpPortal websocket error:", error.message)
});

async function shutdown() {
  if (isShuttingDown) {
    return;
  }

  isShuttingDown = true;
  console.log("Shutting down");
  commandPoller.stop();
  migrationListener.stop();

  if (config.telegramToken && config.telegramChatId) {
    try {
      await sendTelegramMessage({
        token: config.telegramToken,
        chatId: config.telegramChatId,
        text: "<b>Bot stopped.</b>"
      });
    } catch (error) {
      console.warn(`Could not send shutdown notification: ${error.message}`);
    }
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
migrationListener.start();
await commandPoller.start();
