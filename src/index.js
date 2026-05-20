import "dotenv/config";
import WebSocket from "ws";
import { formatMigrationMessage, getEventId } from "./format.js";
import { sendTelegramMessage } from "./telegram.js";

const config = {
  telegramToken: process.env.TELEGRAM_BOT_TOKEN,
  telegramChatId: process.env.TELEGRAM_CHAT_ID,
  pumpPortalApiKey: process.env.PUMPPORTAL_API_KEY,
  pumpPortalWsUrl: process.env.PUMPPORTAL_WS_URL || "wss://pumpportal.fun/api/data",
  solscanBaseUrl: process.env.SOLSCAN_BASE_URL || "https://solscan.io",
  pumpFunBaseUrl: process.env.PUMPFUN_BASE_URL || "https://pump.fun"
};

const seenEvents = new Set();
let reconnectAttempt = 0;
let shouldReconnect = true;

function buildPumpPortalUrl() {
  const url = new URL(config.pumpPortalWsUrl);

  if (config.pumpPortalApiKey) {
    url.searchParams.set("api-key", config.pumpPortalApiKey);
  }

  return url.toString();
}

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

async function handleMessage(data) {
  let event;

  try {
    event = JSON.parse(data.toString());
  } catch (error) {
    console.warn("Skipping non-JSON websocket message:", data.toString());
    return;
  }

  if (event?.message || event?.error) {
    console.log("PumpPortal:", event);
    return;
  }

  const eventId = getEventId(event);
  if (rememberEvent(eventId)) {
    return;
  }

  const text = formatMigrationMessage(event, config);
  await sendTelegramMessage({
    token: config.telegramToken,
    chatId: config.telegramChatId,
    text
  });
}

function connect() {
  const wsUrl = buildPumpPortalUrl();
  const ws = new WebSocket(wsUrl);

  ws.on("open", () => {
    reconnectAttempt = 0;
    console.log("Connected to PumpPortal websocket");
    ws.send(JSON.stringify({ method: "subscribeMigration" }));
  });

  ws.on("message", (data) => {
    handleMessage(data).catch((error) => {
      console.error("Failed to process migration event:", error);
    });
  });

  ws.on("error", (error) => {
    console.error("PumpPortal websocket error:", error.message);
  });

  ws.on("close", (code, reason) => {
    console.warn(`PumpPortal websocket closed: ${code} ${reason}`);

    if (!shouldReconnect) {
      return;
    }

    reconnectAttempt += 1;
    const delayMs = Math.min(30000, 1000 * 2 ** reconnectAttempt);
    console.log(`Reconnecting in ${delayMs}ms`);
    setTimeout(connect, delayMs);
  });

  return ws;
}

function assertConfig() {
  const missing = [];

  if (!config.telegramToken) {
    missing.push("TELEGRAM_BOT_TOKEN");
  }

  if (!config.telegramChatId) {
    missing.push("TELEGRAM_CHAT_ID");
  }

  if (missing.length > 0) {
    throw new Error(`Missing required env vars: ${missing.join(", ")}`);
  }
}

process.on("SIGINT", () => {
  shouldReconnect = false;
  console.log("Shutting down");
  process.exit(0);
});

assertConfig();
connect();
