import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  applyCopyTradeBuyPressureTrade,
  claimCopyTradeBuyPressureSellTrigger,
  copyTradeBuyPressureTimeoutTrigger,
  createCopyTradeBuyPressureSellWatcher,
  createJsonCopyTradeBuyPressureSellStore,
  isOwnCopyTradeBuyPressureTrade
} from "../dist/copytrade-buy-pressure.js";
import {
  formatCopyTradeBuyPressureSellResultMessage,
  formatCopyTradeBuyPressureSellScheduledMessage
} from "../dist/wallet-monitor.js";

const sourceWallet = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
const tradingWallet = "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV";
const mint = "DezXAZ8z7PnrnRJjz3n26VsZdrmRwNny4nBj9JkzjW7B";

function trade(overrides = {}) {
  return {
    observedAt: "2026-05-29T12:00:00.000Z",
    provider: "pumpportal",
    targetWallet: overrides.targetWallet || sourceWallet,
    label: overrides.label ?? "Copy Alpha",
    action: overrides.action || "buy",
    mint: overrides.mint ?? mint,
    signature: overrides.signature ?? "observed-buy-tx",
    timestamp: overrides.timestamp ?? 1_779_999_900,
    feePayer: overrides.feePayer ?? overrides.targetWallet ?? sourceWallet,
    source: overrides.source ?? "PUMP_FUN",
    input: { mint: "So11111111111111111111111111111111111111112", symbol: "SOL", amount: overrides.solAmount ?? 0.1 },
    output: { mint: overrides.mint ?? mint, symbol: null, amount: overrides.tokenAmount ?? 1000 },
    solAmount: overrides.solAmount ?? 0.1,
    tokenAmount: overrides.tokenAmount ?? 1000,
    pool: overrides.pool ?? "pump",
    marketCapSol: null,
    pumpFunUrl: `https://pump.fun/${overrides.mint ?? mint}`,
    solscanTokenUrl: `https://solscan.io/token/${overrides.mint ?? mint}`,
    solscanTxUrl: `https://solscan.io/tx/${overrides.signature ?? "observed-buy-tx"}`,
    raw: {}
  };
}

function subscriber(overrides = {}) {
  return {
    chatId: "chat-1",
    mode: "both",
    watchedWallets: [],
    copyTradeWallets: [copyTradeWallet()],
    tradingWallet: {
      publicKey: overrides.tradingWalletPublicKey || tradingWallet,
      encryptedApiKey: "encrypted",
      apiKeyLast4: "abcd",
      createdAt: "now",
      updatedAt: "now"
    },
    tradingWallets: [],
    copyWalletAddress: null,
    copyWalletAddresses: [],
    copyAmountSol: 0.01,
    copyTradeBuySlippagePercent: null,
    copyTradeBuyPriorityFeeSol: null,
    copyTradeSellSlippagePercent: null,
    copyTradeSellPriorityFeeSol: null,
    copyTradeRetryFailedBuys: false,
    copyTargetWalletAddress: null,
    verifiedAt: "now",
    updatedAt: "now"
  };
}

function copyTradeWallet(overrides = {}) {
  return {
    address: overrides.address || sourceWallet,
    label: overrides.label ?? "Copy Alpha",
    addedAt: "now",
    updatedAt: "now"
  };
}

function watcher(overrides = {}) {
  const created = createCopyTradeBuyPressureSellWatcher({
    config: {
      enabled: true,
      sellPercent: overrides.sellPercent ?? 100,
      timeoutMs: overrides.timeoutMs ?? 30_000,
      minBuys: overrides.minBuys ?? 1,
      minTotalSol: overrides.minTotalSol ?? 0
    },
    subscriber: subscriber(),
    trade: trade({ signature: "source-buy-tx" }),
    copyTradeWallet: copyTradeWallet(),
    buySignature: overrides.copyBuySignature ?? "our-copy-buy-tx",
    executionProvider: "pumpportal-lightning",
    nowMs: overrides.nowMs ?? 1_000
  });

  assert.ok(created);
  return created;
}

test("buy-pressure watcher triggers on the first qualifying post-entry buy by default", () => {
  const active = watcher();
  const result = applyCopyTradeBuyPressureTrade({
    watcher: active,
    trade: trade({ signature: "follow-on-buy-tx", targetWallet: "Buyer111111111111111111111111111111111111111", solAmount: 0.2 })
  });

  assert.equal(result.changed, true);
  assert.equal(result.trigger?.kind, "buy_pressure");
  assert.equal(result.trigger?.buyCount, 1);
  assert.equal(result.trigger?.buySol, 0.2);
  assert.match(result.trigger?.reason || "", /buy-pressure trigger/);
});

test("buy-pressure watcher supports count and total SOL thresholds", () => {
  let active = watcher({ minBuys: 2, minTotalSol: 0.5 });
  const first = applyCopyTradeBuyPressureTrade({
    watcher: active,
    trade: trade({ signature: "follow-on-buy-a", targetWallet: "BuyerA11111111111111111111111111111111111111", solAmount: 0.2 })
  });

  assert.equal(first.trigger, null);
  active = first.watcher;

  const second = applyCopyTradeBuyPressureTrade({
    watcher: active,
    trade: trade({ signature: "follow-on-buy-b", targetWallet: "BuyerB11111111111111111111111111111111111111", solAmount: 0.3 })
  });

  assert.equal(second.trigger?.kind, "buy_pressure");
  assert.equal(second.trigger?.buyCount, 2);
  assert.equal(second.trigger?.buySol, 0.5);
});

test("buy-pressure watcher excludes own copy buy, source buy replay, duplicates, wrong mints, and sells", () => {
  const active = watcher();

  assert.equal(isOwnCopyTradeBuyPressureTrade({ watcher: active, trade: trade({ signature: "our-copy-buy-tx" }) }), true);
  assert.equal(isOwnCopyTradeBuyPressureTrade({ watcher: active, trade: trade({ targetWallet: tradingWallet, signature: "own-wallet-buy" }) }), true);

  const ignoredTrades = [
    trade({ signature: "source-buy-tx" }),
    trade({ targetWallet: tradingWallet, signature: "own-wallet-buy" }),
    trade({ signature: "wrong-mint", mint: "WrongMint1111111111111111111111111111111111111" }),
    trade({ signature: "sell-tx", action: "sell" })
  ];

  for (const ignored of ignoredTrades) {
    const result = applyCopyTradeBuyPressureTrade({ watcher: active, trade: ignored });
    assert.equal(result.changed, false);
    assert.equal(result.trigger, null);
  }

  const first = applyCopyTradeBuyPressureTrade({
    watcher: active,
    trade: trade({ signature: "follow-on-buy-dup", targetWallet: "BuyerD11111111111111111111111111111111111111" })
  });
  assert.equal(first.trigger?.kind, "buy_pressure");

  const duplicate = applyCopyTradeBuyPressureTrade({
    watcher: first.watcher,
    trade: trade({ signature: "follow-on-buy-dup", targetWallet: "BuyerD11111111111111111111111111111111111111" })
  });
  assert.equal(duplicate.changed, false);
});

test("buy-pressure watcher only counts post-confirmation trade timestamps", () => {
  const active = watcher({ nowMs: 10_000 });

  const delayedPreEntry = applyCopyTradeBuyPressureTrade({
    watcher: active,
    trade: trade({
      signature: "delayed-pre-entry-buy",
      targetWallet: "BuyerP11111111111111111111111111111111111111",
      timestamp: 9
    })
  });

  assert.equal(delayedPreEntry.changed, false);
  assert.equal(delayedPreEntry.trigger, null);

  const postEntry = applyCopyTradeBuyPressureTrade({
    watcher: active,
    trade: trade({
      signature: "post-entry-buy",
      targetWallet: "BuyerP11111111111111111111111111111111111111",
      timestamp: 10
    })
  });

  assert.equal(postEntry.trigger?.kind, "buy_pressure");
});

test("timeout fallback and buy-pressure trigger race can only claim once", () => {
  const active = watcher({ timeoutMs: 1_000, nowMs: 10_000 });
  const pressure = applyCopyTradeBuyPressureTrade({
    watcher: active,
    trade: trade({ signature: "follow-on-buy-race", targetWallet: "BuyerR11111111111111111111111111111111111111" })
  });

  assert.ok(pressure.trigger);
  const claimed = claimCopyTradeBuyPressureSellTrigger({
    watcher: pressure.watcher,
    trigger: pressure.trigger,
    nowMs: 10_500
  });

  assert.equal(copyTradeBuyPressureTimeoutTrigger({ watcher: claimed, nowMs: 11_500 }), null);

  const timedOut = copyTradeBuyPressureTimeoutTrigger({ watcher: active, nowMs: 11_001 });
  assert.equal(timedOut?.kind, "timeout");
  assert.match(timedOut?.reason || "", /timeout fallback/);
});

test("json store persists active watchers for restart resume", async () => {
  const dir = await mkdtemp(join(tmpdir(), "copytrade-buy-pressure-"));
  const path = join(dir, "watchers.json");
  const store = createJsonCopyTradeBuyPressureSellStore({ path });
  const active = watcher({ minBuys: 2, minTotalSol: 0.25 });

  try {
    await store.save([active]);
    const loaded = await store.load();

    assert.equal(loaded.length, 1);
    assert.equal(loaded[0].id, active.id);
    assert.equal(loaded[0].mint, mint);
    assert.equal(loaded[0].minBuys, 2);
    assert.equal(loaded[0].minTotalSol, 0.25);
    assert.equal(loaded[0].copyBuySignature, "our-copy-buy-tx");
    assert.equal(loaded[0].preBuyTokenBalance, null);
    assert.equal(loaded[0].postBuyTokenBalance, null);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("telegram messages explain buy-pressure and timeout reasons", () => {
  const active = watcher({ minBuys: 2, minTotalSol: 0.5 });
  const scheduled = formatCopyTradeBuyPressureSellScheduledMessage({
    trade: active.trade,
    watcher: active
  });

  assert.match(scheduled || "", /Buy-Pressure Sell/);
  assert.match(scheduled || "", /Exit watcher armed/);
  assert.match(scheduled || "", /2 buys and 0\.5 SOL/);

  const result = formatCopyTradeBuyPressureSellResultMessage({
    trade: active.trade,
    trigger: {
      kind: "timeout",
      reason: "timeout fallback after 30s without qualifying buy pressure",
      buyCount: 1,
      buySol: 0.2,
      signature: null
    },
    request: {
      mint,
      amount: "100%"
    },
    result: {
      ok: false,
      status: "skipped",
      provider: "pumpportal-lightning",
      route: "pumpportal-lightning",
      signature: null,
      errorText: "Buy-pressure sell skipped: COPY_TRADE_DRY_RUN is enabled",
      raw: null,
      submittedAtMs: null,
      confirmedAtMs: null,
      slot: null,
      metadata: {}
    }
  });

  assert.match(result, /Timeout fallback/);
  assert.match(result, /Sell skipped/);
  assert.match(result, /COPY_TRADE_DRY_RUN is enabled/);
});

test("index integration keeps buy-pressure sells confirmation-gated and live-gated", async () => {
  const source = await readFile("src/index.ts", "utf8");

  assert.match(
    source,
    /const confirmed = await waitForSignatureConfirmation\(buySignature\);[\s\S]*await scheduleCopyTradePostConfirmationExits/
  );
  assert.match(source, /preBuyTokenBalance = await getTokenBalanceForWalletMint/);
  assert.match(source, /await copyTradePositionBalanceBlockedReason\(watcher\)/);
  assert.match(source, /activeBuyPressureSellWatchers\.set\(watcher\.id, claimedWatcher\);[\s\S]*await persistActiveBuyPressureSellWatchers\(\);/);
  assert.match(source, /copyTradeSubmissionBlockedReason\(watcher\.executionProvider\)/);
  assert.match(source, /Buy-pressure sell skipped:/);
});
