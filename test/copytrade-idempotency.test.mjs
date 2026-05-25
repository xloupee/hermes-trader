import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  copyTradeBuyIdempotencyKey,
  createJsonCopyTradeBuyIdempotencyStore
} from "../dist/copytrade-idempotency.js";

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
  const input = {
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

  return {
    ...input,
    key: overrides.key || copyTradeBuyIdempotencyKey(input)
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

test("JSON copy buy idempotency blocks same mint after failed submission", async (t) => {
  const path = await tempStorePath(t);
  const firstStore = createJsonCopyTradeBuyIdempotencyStore({ path });

  assert.equal((await firstStore.claimBuy(claimInput())).claimed, true);
  await firstStore.failBuy(claimInput().key, "HTTP 500");

  const restartedStore = createJsonCopyTradeBuyIdempotencyStore({ path });
  const duplicate = await restartedStore.claimBuy(claimInput({
    observedSignature: "target-buy-signature-2"
  }));

  assert.equal(duplicate.claimed, false);
  assert.equal(duplicate.existing?.status, "failed");
  assert.equal(duplicate.existing?.errorText, "HTTP 500");
});

test("JSON copy buy idempotency blocks same chat and mint across observed signatures", async (t) => {
  const path = await tempStorePath(t);
  const store = createJsonCopyTradeBuyIdempotencyStore({ path });

  const first = await store.claimBuy(claimInput());
  const second = await store.claimBuy(claimInput({
    sourceWalletAddress: "AnotherSourceWallet1111111111111111111111111",
    tradingWalletPublicKey: "AnotherTradingWallet11111111111111111111111",
    observedSignature: "target-buy-signature-2"
  }));

  assert.equal(first.claimed, true);
  assert.equal(second.claimed, false);
  assert.equal(second.existing?.observedSignature, "target-buy-signature-1");
});

test("JSON copy buy idempotency allows different chats and different mints", async (t) => {
  const path = await tempStorePath(t);
  const store = createJsonCopyTradeBuyIdempotencyStore({ path });

  const first = await store.claimBuy(claimInput());
  const differentMint = await store.claimBuy(claimInput({
    mint: "OtherMint11111111111111111111111111111111111",
    request: {
      ...request,
      mint: "OtherMint11111111111111111111111111111111111"
    },
    observedSignature: "target-buy-signature-2"
  }));
  const differentChat = await store.claimBuy(claimInput({
    chatId: "chat-2",
    observedSignature: "target-buy-signature-3"
  }));

  assert.equal(first.claimed, true);
  assert.equal(differentMint.claimed, true);
  assert.equal(differentChat.claimed, true);
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
