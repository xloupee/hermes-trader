import WebSocket, { type RawData } from "ws";
import { isRecord } from "./types.js";
import type { LooseRecord } from "./types.js";

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
