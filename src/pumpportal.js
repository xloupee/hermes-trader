import WebSocket from "ws";

export function buildPumpPortalUrl({ pumpPortalWsUrl, pumpPortalApiKey }) {
  const url = new URL(pumpPortalWsUrl);

  if (pumpPortalApiKey) {
    url.searchParams.set("api-key", pumpPortalApiKey);
  }

  return url.toString();
}

export function createPumpPortalMigrationListener({
  pumpPortalWsUrl,
  pumpPortalApiKey,
  subscriptionMethod = "subscribeMigration",
  onMigration,
  onStatus = () => {},
  onError = () => {}
}) {
  let reconnectAttempt = 0;
  let reconnectTimer;
  let shouldReconnect = true;
  let ws;

  function connect() {
    const wsUrl = buildPumpPortalUrl({ pumpPortalWsUrl, pumpPortalApiKey });
    ws = new WebSocket(wsUrl);

    ws.on("open", () => {
      reconnectAttempt = 0;
      onStatus("Connected to PumpPortal websocket");
      ws.send(JSON.stringify({ method: subscriptionMethod }));
      onStatus(`Sent PumpPortal subscription: ${subscriptionMethod}`);
    });

    ws.on("message", (data) => {
      let event;

      try {
        event = JSON.parse(data.toString());
      } catch (error) {
        onError(new Error(`Skipping non-JSON websocket message: ${data.toString()}`));
        return;
      }

      if (event?.message || event?.error) {
        onStatus(`PumpPortal: ${JSON.stringify(event)}`);
        return;
      }

      onMigration(event);
    });

    ws.on("error", (error) => {
      onError(error);
    });

    ws.on("close", (code, reason) => {
      onStatus(`PumpPortal websocket closed: ${code} ${reason}`);

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
    }
  };
}
