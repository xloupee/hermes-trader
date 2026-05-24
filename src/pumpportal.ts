import WebSocket, { type RawData } from "ws";
import { isRecord } from "./types.js";
import type {
  CopyTradeSettings,
  LooseRecord,
  PumpPortalLightningTradeRequest,
  PumpPortalLightningTradeResult,
  PumpPortalLightningWallet,
  PumpPortalLocalTradeBuildResult,
  PumpPortalLocalTradeRequest,
  PumpPortalTradePool,
  WalletTradeData
} from "./types.js";

export const PUMPPORTAL_TRADE_LOCAL_URL = "https://pumpportal.fun/api/trade-local";
export const PUMPPORTAL_CREATE_WALLET_URL = "https://pumpportal.fun/api/create-wallet";
export const PUMPPORTAL_LIGHTNING_TRADE_URL = "https://pumpportal.fun/api/trade";

export type PumpPortalSubscription =
  | string
  | {
      method: string;
      keys?: string[];
    };

interface PumpPortalUrlOptions {
  pumpPortalWsUrl: string;
  pumpPortalApiKey?: string;
}

interface PumpPortalMigrationListenerOptions extends PumpPortalUrlOptions {
  subscriptionMethods?: PumpPortalSubscription[];
  onMigration: (event: LooseRecord) => void | Promise<void>;
  onStatus?: (message: string) => void;
  onError?: (error: Error) => void;
}

interface PumpPortalMigrationListener {
  start: () => void;
  stop: () => void;
  setSubscriptionMethods: (nextSubscriptionMethods: PumpPortalSubscription[] | PumpPortalSubscription) => void;
}

export function buildPumpPortalUrl({ pumpPortalWsUrl, pumpPortalApiKey }: PumpPortalUrlOptions): string {
  const url = new URL(pumpPortalWsUrl);

  if (pumpPortalApiKey) {
    url.searchParams.set("api-key", pumpPortalApiKey);
  }

  return url.toString();
}

export function buildPumpPortalLocalTradeRequest({
  trade,
  copySettings,
  slippage,
  priorityFee,
  pool
}: {
  trade: WalletTradeData;
  copySettings: CopyTradeSettings;
  slippage: number;
  priorityFee: number;
  pool: PumpPortalTradePool;
}): PumpPortalLocalTradeRequest | null {
  const copyWalletAddress = copySettings.copyWalletAddresses?.[0] || copySettings.copyWalletAddress;

  if (!copyWalletAddress || !copySettings.copyAmountSol || !trade.mint) {
    return null;
  }

  return {
    publicKey: copyWalletAddress,
    action: "buy",
    mint: trade.mint,
    amount: copySettings.copyAmountSol,
    denominatedInSol: "true",
    slippage,
    priorityFee,
    pool
  };
}

export function buildPumpPortalLocalSellRequest({
  publicKey,
  mint,
  amountPercent,
  slippage,
  priorityFee,
  pool
}: {
  publicKey: string;
  mint: string;
  amountPercent: number;
  slippage: number;
  priorityFee: number;
  pool: PumpPortalTradePool;
}): PumpPortalLocalTradeRequest | null {
  if (!publicKey || !mint || !Number.isFinite(amountPercent) || amountPercent <= 0) {
    return null;
  }

  const boundedPercent = Math.min(100, amountPercent);

  return {
    publicKey,
    action: "sell",
    mint,
    amount: `${boundedPercent}%`,
    denominatedInSol: "false",
    slippage,
    priorityFee,
    pool
  };
}

export async function buildPumpPortalLocalTrade({
  url,
  request
}: {
  url: string;
  request: PumpPortalLocalTradeRequest;
}): Promise<PumpPortalLocalTradeBuildResult> {
  try {
    const response = await fetch(url, {
      method: "POST",
      headers: {
        "Content-Type": "application/json"
      },
      body: JSON.stringify(request)
    });
    const body = Buffer.from(await response.arrayBuffer());

    return {
      ok: response.ok,
      status: response.status,
      bodyLength: body.length,
      errorText: response.ok ? null : body.toString("utf8").slice(0, 500)
    };
  } catch (error) {
    return {
      ok: false,
      status: null,
      bodyLength: null,
      errorText: error instanceof Error ? error.message : String(error)
    };
  }
}

function responseString(record: LooseRecord, keys: string[]): string | null {
  for (const key of keys) {
    const value = record[key];

    if (typeof value === "string" && value.trim()) {
      return value.trim();
    }
  }

  return null;
}

function normalizePumpPortalLightningWallet(body: unknown): PumpPortalLightningWallet | null {
  const record = isRecord(body) ? body : {};
  const nested = isRecord(record.wallet) ? record.wallet : {};
  const merged = {
    ...record,
    ...nested
  };
  const publicKey = responseString(merged, ["publicKey", "public_key", "walletPublicKey", "wallet_public_key", "wallet", "address"]);
  const privateKey = responseString(merged, ["privateKey", "private_key", "walletPrivateKey", "wallet_private_key", "secretKey", "secret_key"]);
  const apiKey = responseString(merged, ["apiKey", "api_key", "key"]);

  return publicKey && privateKey && apiKey
    ? {
        publicKey,
        privateKey,
        apiKey
      }
    : null;
}

export async function createPumpPortalLightningWallet({
  url
}: {
  url: string;
}): Promise<{ ok: true; wallet: PumpPortalLightningWallet } | { ok: false; status: number | null; errorText: string }> {
  try {
    const response = await fetch(url, {
      method: "GET"
    });
    const text = await response.text();
    let body: unknown = null;

    try {
      body = text ? JSON.parse(text) : null;
    } catch {
      body = null;
    }

    const wallet = response.ok ? normalizePumpPortalLightningWallet(body) : null;

    if (wallet) {
      return {
        ok: true,
        wallet
      };
    }

    return {
      ok: false,
      status: response.status,
      errorText: text.slice(0, 500) || "PumpPortal did not return wallet keys"
    };
  } catch (error) {
    return {
      ok: false,
      status: null,
      errorText: error instanceof Error ? error.message : String(error)
    };
  }
}

export function buildPumpPortalLightningBuyRequest({
  trade,
  amountSol,
  slippage,
  priorityFee,
  pool,
  skipPreflight
}: {
  trade: WalletTradeData;
  amountSol: number;
  slippage: number;
  priorityFee: number;
  pool: PumpPortalTradePool;
  skipPreflight?: boolean;
}): PumpPortalLightningTradeRequest | null {
  if (!trade.mint || !Number.isFinite(amountSol) || amountSol <= 0) {
    return null;
  }

  const request: PumpPortalLightningTradeRequest = {
    action: "buy",
    mint: trade.mint,
    amount: amountSol,
    denominatedInSol: "true",
    slippage,
    priorityFee,
    pool
  };

  if (typeof skipPreflight === "boolean") {
    request.skipPreflight = skipPreflight;
  }

  return request;
}

export function buildPumpPortalLightningSellRequest({
  mint,
  amountPercent,
  slippage,
  priorityFee,
  pool,
  skipPreflight
}: {
  mint: string;
  amountPercent: number;
  slippage: number;
  priorityFee: number;
  pool: PumpPortalTradePool;
  skipPreflight?: boolean;
}): PumpPortalLightningTradeRequest | null {
  if (!mint || !Number.isFinite(amountPercent) || amountPercent <= 0) {
    return null;
  }

  const request: PumpPortalLightningTradeRequest = {
    action: "sell",
    mint,
    amount: `${Math.min(100, amountPercent)}%`,
    denominatedInSol: "false",
    slippage,
    priorityFee,
    pool
  };

  if (typeof skipPreflight === "boolean") {
    request.skipPreflight = skipPreflight;
  }

  return request;
}

const PUMPPORTAL_LIGHTNING_SIGNATURE_KEYS = [
  "signature",
  "tx",
  "txid",
  "txId",
  "txHash",
  "transaction",
  "transactionHash",
  "transactionSignature"
];

const PUMPPORTAL_LIGHTNING_ERROR_KEYS = ["error", "errors", "errorText", "error_text", "errorMessage", "error_message"];

function hasResponseField(record: LooseRecord, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(record, key);
}

function summarizeResponseValue(value: unknown): string | null {
  if (typeof value === "string") {
    return value.trim() ? value.trim().slice(0, 500) : null;
  }

  if (typeof value === "number" || typeof value === "boolean" || typeof value === "bigint") {
    return String(value);
  }

  if (value === null || typeof value === "undefined") {
    return null;
  }

  try {
    return JSON.stringify(value).slice(0, 500);
  } catch {
    return String(value).slice(0, 500);
  }
}

function summarizeResponseBody(body: unknown, text: string): string | null {
  const trimmedText = text.trim();

  if (trimmedText) {
    return trimmedText.slice(0, 500);
  }

  return summarizeResponseValue(body);
}

function fieldErrorText(record: LooseRecord, key: string): string {
  const value = record[key];
  const detail = summarizeResponseValue(value);

  return detail ? `${key}: ${detail}` : `${key} field present`;
}

function pumpPortalLightningErrorText(record: LooseRecord): string | null {
  for (const key of PUMPPORTAL_LIGHTNING_ERROR_KEYS) {
    if (hasResponseField(record, key)) {
      return fieldErrorText(record, key);
    }
  }

  const status = responseString(record, ["status"]);

  if (status && /^(error|failed|failure)$/i.test(status)) {
    const detail = responseString(record, ["message", "reason", "description"]);
    return detail || `status: ${status}`;
  }

  if (record.ok === false || record.success === false) {
    const detail = responseString(record, ["message", "reason", "description"]);
    return detail || "PumpPortal response marked the trade as failed";
  }

  return null;
}

function missingPumpPortalLightningSignatureText(body: unknown, text: string): string {
  const bodySummary = summarizeResponseBody(body, text);

  return bodySummary
    ? `PumpPortal did not return a transaction signature: ${bodySummary}`
    : "PumpPortal did not return a transaction signature";
}

export async function executePumpPortalLightningTrade({
  url,
  apiKey,
  request
}: {
  url: string;
  apiKey: string;
  request: PumpPortalLightningTradeRequest;
}): Promise<PumpPortalLightningTradeResult> {
  try {
    const endpoint = new URL(url);
    endpoint.searchParams.set("api-key", apiKey);

    const response = await fetch(endpoint.toString(), {
      method: "POST",
      headers: {
        "Content-Type": "application/json"
      },
      body: JSON.stringify(request)
    });
    const text = await response.text();
    let body: unknown = text;

    try {
      body = text ? JSON.parse(text) : null;
    } catch {
      body = text;
    }

    const record = isRecord(body) ? body : {};
    const signature = responseString(record, PUMPPORTAL_LIGHTNING_SIGNATURE_KEYS);
    const responseErrorText = pumpPortalLightningErrorText(record);

    if (!response.ok) {
      return {
        ok: false,
        status: response.status,
        signature,
        errorText: responseErrorText || summarizeResponseBody(body, text) || `HTTP ${response.status}`,
        raw: body
      };
    }

    if (responseErrorText) {
      return {
        ok: false,
        status: response.status,
        signature,
        errorText: responseErrorText,
        raw: body
      };
    }

    if (!signature) {
      return {
        ok: false,
        status: response.status,
        signature: null,
        errorText: missingPumpPortalLightningSignatureText(body, text),
        raw: body
      };
    }

    return {
      ok: true,
      status: response.status,
      signature,
      errorText: null,
      raw: body
    };
  } catch (error) {
    return {
      ok: false,
      status: null,
      signature: null,
      errorText: error instanceof Error ? error.message : String(error),
      raw: null
    };
  }
}

export function createPumpPortalMigrationListener({
  pumpPortalWsUrl,
  pumpPortalApiKey,
  subscriptionMethods = ["subscribeMigration"],
  onMigration,
  onStatus = () => {},
  onError = () => {}
}: PumpPortalMigrationListenerOptions): PumpPortalMigrationListener {
  let reconnectAttempt = 0;
  let reconnectTimer: NodeJS.Timeout | undefined;
  let currentSubscriptionMethods = normalizeSubscriptionMethods(subscriptionMethods);
  let intentionalRestart = false;
  let shouldReconnect = false;
  let ws: WebSocket | undefined;

  function connect(): void {
    const wsUrl = buildPumpPortalUrl({ pumpPortalWsUrl, pumpPortalApiKey });
    const socket = new WebSocket(wsUrl);
    ws = socket;

    socket.on("open", () => {
      reconnectAttempt = 0;
      onStatus("Connected to PumpPortal websocket");

      for (const subscription of currentSubscriptionMethods) {
        socket.send(JSON.stringify(subscription));
        onStatus(`Sent PumpPortal subscription: ${subscription.method}`);
      }
    });

    socket.on("message", (data: RawData) => {
      let event: unknown;

      try {
        event = JSON.parse(data.toString());
      } catch (error) {
        onError(new Error(`Skipping non-JSON websocket message: ${data.toString()}`));
        return;
      }

      const eventRecord = isRecord(event) ? event : {};

      if (eventRecord.message || eventRecord.error) {
        onStatus(`PumpPortal: ${JSON.stringify(eventRecord)}`);
        return;
      }

      onMigration(eventRecord);
    });

    socket.on("error", (error) => {
      onError(error);
    });

    socket.on("close", (code, reason) => {
      onStatus(`PumpPortal websocket closed: ${code} ${reason}`);

      if (intentionalRestart) {
        intentionalRestart = false;
        return;
      }

      if (!shouldReconnect) {
        return;
      }

      reconnectAttempt += 1;
      const delayMs = Math.min(30000, 1000 * 2 ** reconnectAttempt);
      onStatus(`Reconnecting to PumpPortal in ${delayMs}ms`);
      reconnectTimer = setTimeout(connect, delayMs);
    });
  }

  return {
    start() {
      shouldReconnect = true;
      connect();
    },
    stop() {
      shouldReconnect = false;
      clearTimeout(reconnectTimer);
      ws?.close();
    },
    setSubscriptionMethods(nextSubscriptionMethods: PumpPortalSubscription[] | PumpPortalSubscription) {
      currentSubscriptionMethods = normalizeSubscriptionMethods(nextSubscriptionMethods);
      reconnectAttempt = 0;
      clearTimeout(reconnectTimer);

      if (ws) {
        intentionalRestart = true;
        ws.close();
      }

      if (shouldReconnect) {
        connect();
      }
    }
  };
}

function normalizeSubscriptionMethods(value: PumpPortalSubscription[] | PumpPortalSubscription): Array<{ method: string; keys?: string[] }> {
  const methods = Array.isArray(value) ? value : [value];
  const byPayload = new Map<string, { method: string; keys?: string[] }>();

  for (const method of methods) {
    const subscription = typeof method === "string" ? { method } : method;
    const keys = Array.isArray(subscription.keys) ? [...new Set(subscription.keys.filter(Boolean))].sort() : undefined;

    if (!subscription.method || (subscription.keys && (!keys || keys.length === 0))) {
      continue;
    }

    const normalized = keys ? { method: subscription.method, keys } : { method: subscription.method };
    byPayload.set(JSON.stringify(normalized), normalized);
  }

  return [...byPayload.values()];
}
