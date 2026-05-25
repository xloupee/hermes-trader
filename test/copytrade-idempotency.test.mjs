import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { createJsonCopyTradeBuyIdempotencyStore } from "../dist/copytrade-idempotency.js";

const request = {
  action: "buy",
  mint: "Mint111111111111111111111111111111111111111",
  amount: 0.1,
  denominatedInSol: "true",
  slippage: 10,
  priorityFee: 0.00005,
  pool: "pump"
};

function claimInput(overrides = {}) {
  return {
    key: "chat-1:TradingWallet1111111111111111111111111111111:SourceWallet11111111111111111111111111111111:target-buy-signature-1:Mint111111111111111111111111111111111111111:buy",
    chatId: "chat-1",
    sourceWalletAddress: "SourceWallet11111111111111111111111111111111",
    tradingWalletPublicKey: "TradingWallet1111111111111111111111111111111",
    observedSignature: "target-buy-signature-1",
    mint: request.mint,
    action: "buy",
    amountSol: 0.1,
    provider: "helius",
    request,
    now: "2026-05-24T12:00:00.000Z",
    ...overrides
  };
}

async function tempStorePath(t) {
  const dir = await mkdtemp(join(tmpdir(), "copytrade-idempotency-"));
  t.after(() => rm(dir, { recursive: true, force: true }));
  return join(dir, "copytrade-buy-idempotency.json");
}

test("JSON copy buy idempotency blocks duplicate claims before submit", async (t) => {
  const path = await tempStorePath(t);
  const store = createJsonCopyTradeBuyIdempotencyStore({ path });

  const first = await store.claimBuy(claimInput());
  const second = await store.claimBuy(claimInput());

  assert.equal(first.claimed, true);
  assert.equal(second.claimed, false);
  assert.equal(second.existing?.status, "claimed");
  assert.equal(second.existing?.observedSignature, "target-buy-signature-1");
});

test("JSON copy buy idempotency survives restart after completion", async (t) => {
  const path = await tempStorePath(t);
  const firstStore = createJsonCopyTradeBuyIdempotencyStore({ path });

  assert.equal((await firstStore.claimBuy(claimInput())).claimed, true);
  await firstStore.completeBuy(claimInput().key, {
    ok: true,
    status: 200,
    signature: "copy-buy-signature-1",
    errorText: null,
    raw: { signature: "copy-buy-signature-1" }
  });

  const restartedStore = createJsonCopyTradeBuyIdempotencyStore({ path });
  const duplicate = await restartedStore.claimBuy(claimInput());
  const stored = JSON.parse(await readFile(path, "utf8"));

  assert.equal(duplicate.claimed, false);
  assert.equal(duplicate.existing?.status, "submitted");
  assert.equal(duplicate.existing?.resultSignature, "copy-buy-signature-1");
  assert.equal(stored.records.length, 1);
});

test("JSON copy buy idempotency serializes concurrent claims", async (t) => {
  const path = await tempStorePath(t);
  const store = createJsonCopyTradeBuyIdempotencyStore({ path });

  const attempts = await Promise.all(Array.from({ length: 12 }, () => store.claimBuy(claimInput())));
  const claimed = attempts.filter((attempt) => attempt.claimed);
  const duplicates = attempts.filter((attempt) => !attempt.claimed);

  assert.equal(claimed.length, 1);
  assert.equal(duplicates.length, 11);
  assert.equal(duplicates.every((attempt) => attempt.existing?.status === "claimed"), true);
});
