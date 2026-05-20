import "dotenv/config";
import { createTelegramCommandPoller } from "./commands.js";
import { formatMigrationMessage } from "./format.js";

const config = {
  telegramToken: process.env.TELEGRAM_BOT_TOKEN,
  telegramChatId: process.env.TELEGRAM_CHAT_ID,
  pumpPortalApiKey: process.env.PUMPPORTAL_API_KEY,
  pumpPortalWsUrl: process.env.PUMPPORTAL_WS_URL || "wss://pumpportal.fun/api/data",
  solscanBaseUrl: process.env.SOLSCAN_BASE_URL || "https://solscan.io",
  pumpFunBaseUrl: process.env.PUMPFUN_BASE_URL || "https://pump.fun"
};

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
  if (!config.telegramToken) {
    throw new Error("Missing required env var: TELEGRAM_BOT_TOKEN");
  }
}

const commandPoller = createTelegramCommandPoller({
  config,
  testMessage: testMigrationMessage
});

function shutdown() {
  commandPoller.stop();
  console.log("Shutting down Telegram bot");
  process.exit(0);
}

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);

assertConfig();
await commandPoller.start();
