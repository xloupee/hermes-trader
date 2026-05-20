import "dotenv/config";
import { appendFile, mkdir } from "node:fs/promises";
import { dirname } from "node:path";
import { buildMigrationReplyMarkup, extractMigrationData, formatMigrationMessage, getEventId } from "./format.js";
import { createTelegramCommandPoller } from "./commands.js";
import { createPumpPortalMigrationListener } from "./pumpportal.js";
import { sendTelegramMessage } from "./telegram.js";

const config = {
  telegramToken: process.env.TELEGRAM_BOT_TOKEN,
  telegramChatId: process.env.TELEGRAM_CHAT_ID,
  pumpPortalApiKey: process.env.PUMPPORTAL_API_KEY,
  pumpPortalWsUrl: process.env.PUMPPORTAL_WS_URL || "wss://pumpportal.fun/api/data",
  pumpPortalSubscriptionMethod: process.env.PUMPPORTAL_SUBSCRIPTION_METHOD || "subscribeMigration",
  solscanBaseUrl: process.env.SOLSCAN_BASE_URL || "https://solscan.io",
  pumpFunBaseUrl: process.env.PUMPFUN_BASE_URL || "https://pump.fun",
  migrationLogPath: process.env.MIGRATION_LOG_PATH || "logs/migrations.jsonl"
};

const seenEvents = new Set();

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

async function handleMigration(event) {
  const eventId = getEventId(event);
  if (rememberEvent(eventId)) {
    return;
  }

  const migration = extractMigrationData(event, config);
  await writeMigrationLog(migration);
  console.log(`Migration: ${JSON.stringify(migration)}`);

  if (!config.telegramChatId) {
    console.warn("Skipping migration alert because TELEGRAM_CHAT_ID is not configured");
    return;
  }

  const text = formatMigrationMessage(event, config);
  const replyMarkup = buildMigrationReplyMarkup(event, config);
  await sendTelegramMessage({
    token: config.telegramToken,
    chatId: config.telegramChatId,
    text,
    replyMarkup
  });
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
      marketCapSol: "test message",
      txType: "migration"
    },
    config
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
  testMessage: testMigrationMessage
});
const migrationListener = createPumpPortalMigrationListener({
  pumpPortalWsUrl: config.pumpPortalWsUrl,
  pumpPortalApiKey: config.pumpPortalApiKey,
  subscriptionMethod: config.pumpPortalSubscriptionMethod,
  onMigration: (event) => {
    handleMigration(event).catch((error) => {
      console.error("Failed to process migration event:", error);
    });
  },
  onStatus: (message) => console.log(message),
  onError: (error) => console.error("PumpPortal websocket error:", error.message)
});

function shutdown() {
  console.log("Shutting down");
  commandPoller.stop();
  migrationListener.stop();
  process.exit(0);
}

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);

assertConfig();
migrationListener.start();
await commandPoller.start();
