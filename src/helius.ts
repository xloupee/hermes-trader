import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { asRecord, isRecord, stringValue } from "./types.js";
import type { LooseRecord } from "./types.js";

interface HeliusWebhookSyncOptions {
  apiKey?: string;
  apiBaseUrl: string;
  authHeader?: string;
  publicUrl?: string;
  webhookId?: string;
  statePath: string;
  accountAddresses: string[];
}

interface HeliusWebhookState {
  webhookID?: string;
  updatedAt?: string;
}

interface HeliusWebhookServerOptions {
  authHeader?: string;
  port: number;
  onEvents: (events: LooseRecord[]) => void | Promise<void>;
}

interface HeliusWebhookServer {
  start: () => Promise<void>;
  stop: () => Promise<void>;
  port: () => number | null;
}

export interface HeliusWebhookSyncResult {
  ok: boolean;
  webhookId: string | null;
  skipped?: boolean;
  message?: string;
  warning?: string;
}

export function missingHeliusConfigWarning(): string {
  return "Helius wallet swap monitoring is not fully configured. Set HELIUS_API_KEY, HELIUS_WEBHOOK_PUBLIC_URL, and HELIUS_WEBHOOK_AUTH_HEADER, then retry from /trackwallets, /copytrade, or restart the bot.";
}

export async function syncHeliusWebhook(options: HeliusWebhookSyncOptions): Promise<HeliusWebhookSyncResult> {
  const accountAddresses = [...new Set(options.accountAddresses.filter(Boolean))].sort();

  if (!options.apiKey || !options.publicUrl || !options.authHeader) {
    return {
      ok: false,
      webhookId: null,
      warning: missingHeliusConfigWarning()
    };
  }

  const webhookId = options.webhookId || (await readHeliusWebhookState(options.statePath)).webhookID || null;

  if (accountAddresses.length === 0) {
    return {
      ok: true,
      webhookId,
      skipped: true,
      message: webhookId
        ? "No watched wallets; leaving existing Helius webhook unchanged."
        : "No watched wallets; Helius webhook not created."
    };
  }

  const payload = buildHeliusWebhookPayload({
    accountAddresses,
    authHeader: options.authHeader,
    publicUrl: options.publicUrl
  });

  const url = webhookId
    ? `${options.apiBaseUrl.replace(/\/$/, "")}/v0/webhooks/${encodeURIComponent(webhookId)}?api-key=${encodeURIComponent(options.apiKey)}`
    : `${options.apiBaseUrl.replace(/\/$/, "")}/v0/webhooks?api-key=${encodeURIComponent(options.apiKey)}`;
  const response = await fetch(url, {
    method: webhookId ? "PUT" : "POST",
    headers: {
      "content-type": "application/json"
    },
    body: JSON.stringify(payload)
  });
  const body = asRecord(await response.json().catch(() => null));

  if (!response.ok) {
    throw new Error(stringValue(body.message || body.error) || `Helius webhook sync failed with ${response.status}`);
  }

  const nextWebhookId = stringValue(body.webhookID) || webhookId;

  if (nextWebhookId) {
    await writeHeliusWebhookState(options.statePath, {
      webhookID: nextWebhookId,
      updatedAt: new Date().toISOString()
    });
  }

  return {
    ok: true,
    webhookId: nextWebhookId
  };
}

export function buildHeliusWebhookPayload({
  accountAddresses,
  authHeader,
  publicUrl
}: {
  accountAddresses: string[];
  authHeader: string;
  publicUrl: string;
}): LooseRecord {
  return {
    webhookURL: publicUrl,
    transactionTypes: ["SWAP"],
    accountAddresses,
    webhookType: "enhanced",
    authHeader,
    txnStatus: "success"
  };
}

async function readHeliusWebhookState(path: string): Promise<HeliusWebhookState> {
  try {
    const body = await readFile(path, "utf8");
    const parsed = asRecord(JSON.parse(body) as unknown);
    return {
      webhookID: stringValue(parsed.webhookID) || undefined,
      updatedAt: stringValue(parsed.updatedAt) || undefined
    };
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") {
      return {};
    }

    throw error;
  }
}

async function writeHeliusWebhookState(path: string, state: HeliusWebhookState): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, `${JSON.stringify(state, null, 2)}\n`);
}

export function createHeliusWebhookServer({ authHeader, port, onEvents }: HeliusWebhookServerOptions): HeliusWebhookServer {
  let server: Server | null = null;

  async function handleRequest(request: IncomingMessage, response: ServerResponse): Promise<void> {
    if (request.method !== "POST" || request.url?.split("?")[0] !== "/webhooks/helius") {
      sendJson(response, 404, { ok: false, error: "not found" });
      return;
    }

    if (!authHeader || request.headers.authorization !== authHeader) {
      sendJson(response, 401, { ok: false, error: "unauthorized" });
      return;
    }

    let events: LooseRecord[];

    try {
      events = normalizeHeliusWebhookBody(JSON.parse(await readRequestBody(request)) as unknown);
    } catch (error) {
      sendJson(response, 400, { ok: false, error: error instanceof Error ? error.message : String(error) });
      return;
    }

    sendJson(response, 200, { ok: true, received: events.length });

    Promise.resolve(onEvents(events)).catch((error) => {
      console.error("Failed to process Helius webhook events:", error);
    });
  }

  return {
    async start() {
      if (server) {
        return;
      }

      server = createServer((request, response) => {
        handleRequest(request, response).catch((error) => {
          console.error("Helius webhook request failed:", error);
          if (!response.headersSent) {
            sendJson(response, 500, { ok: false, error: "internal error" });
          }
        });
      });

      await new Promise<void>((resolve) => {
        server?.listen(port, resolve);
      });
      const address = server?.address();
      const listeningPort = typeof address === "object" && address ? address.port : port;
      console.log(`Helius webhook receiver listening on port ${listeningPort}`);
    },
    async stop() {
      if (!server) {
        return;
      }

      const closingServer = server;
      server = null;
      await new Promise<void>((resolve, reject) => {
        closingServer.close((error) => (error ? reject(error) : resolve()));
      });
    },
    port() {
      const address = server?.address();
      return typeof address === "object" && address ? address.port : null;
    }
  };
}

function normalizeHeliusWebhookBody(body: unknown): LooseRecord[] {
  if (Array.isArray(body)) {
    return body.filter(isRecord);
  }

  if (isRecord(body)) {
    return [body];
  }

  return [];
}

async function readRequestBody(request: IncomingMessage): Promise<string> {
  const chunks: Buffer[] = [];
  let size = 0;
  const maxBytes = 1_000_000;

  for await (const chunk of request) {
    const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    size += buffer.length;

    if (size > maxBytes) {
      throw new Error("request body too large");
    }

    chunks.push(buffer);
  }

  return Buffer.concat(chunks).toString("utf8");
}

function sendJson(response: ServerResponse, statusCode: number, body: LooseRecord): void {
  response.writeHead(statusCode, {
    "content-type": "application/json"
  });
  response.end(JSON.stringify(body));
}
