import assert from "node:assert/strict";
import test from "node:test";
import {
  buildPumpPortalLightningBuyRequest,
  buildPumpPortalLightningSellRequest,
  executePumpPortalLightningTrade
} from "../dist/pumpportal.js";

const mint = "DezXAZ8z7PnrnRJjz3n26VsZdrmRwNny4nBj9JkzjW7B";
const signature = "5".repeat(88);

async function withFetchResponse(response, callback) {
  const originalFetch = globalThis.fetch;
  const calls = [];

  globalThis.fetch = async (url, init) => {
    calls.push({ url: String(url), init });
    return typeof response === "function" ? response(url, init) : response;
  };

  try {
    return await callback(calls);
  } finally {
    globalThis.fetch = originalFetch;
  }
}

function jsonResponse(body, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "Content-Type": "application/json"
    }
  });
}

function lightningRequest(extra = {}) {
  return {
    action: "buy",
    mint,
    amount: 0.25,
    denominatedInSol: "true",
    slippage: 15,
    priorityFee: 0.00009,
    pool: "auto",
    ...extra
  };
}

function execute(request = lightningRequest()) {
  return executePumpPortalLightningTrade({
    url: "https://pumpportal.fun/api/trade",
    apiKey: "api-key-alpha",
    request
  });
}

test("PumpPortal Lightning builders carry skipPreflight when callers set it", () => {
  assert.deepEqual(
    buildPumpPortalLightningBuyRequest({
      trade: { mint },
      amountSol: 0.25,
      slippage: 15,
      priorityFee: 0.00009,
      pool: "auto",
      skipPreflight: false
    }),
    lightningRequest({ skipPreflight: false })
  );

  assert.deepEqual(
    buildPumpPortalLightningSellRequest({
      mint,
      amountPercent: 20,
      slippage: 12,
      priorityFee: 0.00007,
      pool: "pump",
      skipPreflight: true
    }),
    {
      action: "sell",
      mint,
      amount: "20%",
      denominatedInSol: "false",
      slippage: 12,
      priorityFee: 0.00007,
      pool: "pump",
      skipPreflight: true
    }
  );
});

test("executePumpPortalLightningTrade succeeds for 200 JSON with a signature", async () => {
  const result = await withFetchResponse(jsonResponse({ signature }), async (calls) => {
    const tradeResult = await execute();

    assert.match(calls[0].url, /api-key=api-key-alpha/);
    assert.deepEqual(JSON.parse(calls[0].init.body), lightningRequest());

    return tradeResult;
  });

  assert.equal(result.ok, true);
  assert.equal(result.status, 200);
  assert.equal(result.signature, signature);
  assert.equal(result.errorText, null);
  assert.deepEqual(result.raw, { signature });
});

test("executePumpPortalLightningTrade fails for 200 JSON with an error field", async () => {
  const result = await withFetchResponse(jsonResponse({ error: "insufficient funds", signature }), () => execute());

  assert.equal(result.ok, false);
  assert.equal(result.status, 200);
  assert.equal(result.signature, signature);
  assert.match(result.errorText, /error: insufficient funds/);
  assert.deepEqual(result.raw, { error: "insufficient funds", signature });
});

test("executePumpPortalLightningTrade fails for 200 JSON without a signature", async () => {
  const result = await withFetchResponse(jsonResponse({ status: "submitted" }), () => execute());

  assert.equal(result.ok, false);
  assert.equal(result.status, 200);
  assert.equal(result.signature, null);
  assert.match(result.errorText, /did not return a transaction signature/);
  assert.match(result.errorText, /"status":"submitted"/);
  assert.deepEqual(result.raw, { status: "submitted" });
});

test("executePumpPortalLightningTrade fails for non-JSON 2xx without a signature", async () => {
  const result = await withFetchResponse(new Response("submitted", { status: 200 }), () => execute());

  assert.equal(result.ok, false);
  assert.equal(result.status, 200);
  assert.equal(result.signature, null);
  assert.match(result.errorText, /did not return a transaction signature/);
  assert.match(result.errorText, /submitted/);
  assert.equal(result.raw, "submitted");
});

test("executePumpPortalLightningTrade still fails for non-2xx errors", async () => {
  const result = await withFetchResponse(jsonResponse({ error: "rate limited" }, 429), () => execute());

  assert.equal(result.ok, false);
  assert.equal(result.status, 429);
  assert.equal(result.signature, null);
  assert.match(result.errorText, /error: rate limited/);
  assert.deepEqual(result.raw, { error: "rate limited" });
});

test("executePumpPortalLightningTrade keeps HTTP status for empty non-2xx errors", async () => {
  const result = await withFetchResponse(new Response("", { status: 500 }), () => execute());

  assert.equal(result.ok, false);
  assert.equal(result.status, 500);
  assert.equal(result.signature, null);
  assert.equal(result.errorText, "HTTP 500");
  assert.equal(result.raw, null);
});
