import assert from "node:assert/strict";
import test from "node:test";

import {
  autoSellStatus,
  blockPositionDiagnostics,
  buyStatus,
  dedupeRows,
  displayTxDelta,
  executionKey,
  syncLimitForCycle,
  unknownChainReport
} from "../tools/jito-shredstream-rs/sync-local-copy-executions-to-supabase.mjs";

const baseRow = {
  observedSignature: "target-sig",
  sendSignature: "copy-sig",
  slot: 100
};

function rpcWithBlocks(blocks) {
  return async (method, params) => {
    assert.equal(method, "getBlock");
    const slot = params[0];
    if (!blocks.has(slot)) {
      throw new Error(`missing block ${slot}`);
    }
    return blocks.get(slot);
  };
}

test("block position diagnostics report same-slot transaction delta", async () => {
  const diagnostics = await blockPositionDiagnostics(
    baseRow,
    { slot: 100 },
    rpcWithBlocks(new Map([
      [100, { signatures: ["before", "target-sig", "middle", "copy-sig", "after"] }]
    ]))
  );

  assert.equal(diagnostics.status, "found");
  assert.equal(diagnostics.targetSlot, 100);
  assert.equal(diagnostics.copySlot, 100);
  assert.equal(diagnostics.slotDelta, 0);
  assert.equal(diagnostics.targetTxIndex, 1);
  assert.equal(diagnostics.copyTxIndex, 3);
  assert.equal(diagnostics.sameSlotTxDelta, 2);
  assert.equal(diagnostics.txDelta, 2);
  assert.equal(diagnostics.crossSlotPositionSummary, null);
  assert.equal(diagnostics.unavailableReason, null);
});

test("block position diagnostics report later-slot position summary", async () => {
  const diagnostics = await blockPositionDiagnostics(
    baseRow,
    { slot: 102 },
    rpcWithBlocks(new Map([
      [100, { signatures: ["before", "target-sig", "after"] }],
      [101, { signatures: ["intermediate-1", "intermediate-2"] }],
      [102, { signatures: ["copy-before", "copy-sig", "copy-after", "copy-tail"] }]
    ]))
  );

  assert.equal(diagnostics.status, "found");
  assert.equal(diagnostics.slotDelta, 2);
  assert.equal(diagnostics.targetTxIndex, 1);
  assert.equal(diagnostics.copyTxIndex, 1);
  assert.equal(diagnostics.sameSlotTxDelta, null);
  assert.equal(diagnostics.txDelta, 5);
  assert.deepEqual(diagnostics.crossSlotPositionSummary, {
    targetSlotTransactionCount: 3,
    copySlotTransactionCount: 4,
    targetTxIndex: 1,
    copyTxIndex: 1,
    targetSlotTransactionsAfterTarget: 1,
    intermediateSlotCount: 1,
    intermediateSlotTransactionCount: 2,
    intermediateSlots: [
      {
        slot: 101,
        transactionCount: 2
      }
    ],
    copySlotTransactionsThroughCopy: 2,
    crossSlotTxDelta: 5
  });
});

test("dashboard tx delta falls back to cross-slot transaction distance", () => {
  assert.equal(displayTxDelta({ txDelta: 3, sameSlotTxDelta: 2 }), 3);
  assert.equal(displayTxDelta({ sameSlotTxDelta: 2 }), 2);
  assert.equal(
    displayTxDelta({
      sameSlotTxDelta: null,
      crossSlotPositionSummary: { crossSlotTxDelta: 5620 }
    }),
    5620
  );
  assert.equal(displayTxDelta({ sameSlotTxDelta: null }, 7), 7);
});

test("local execution dedupe keeps separate copy wallets for same observed trade", () => {
  const baseExecution = {
    provider: "shredstream",
    observedSignature: "observed-sig",
    observedWallet: "target-wallet",
    observedAction: "buy",
    mint: "mint"
  };
  const walletA = { ...baseExecution, copyWallet: "copy-wallet-a", sendSignature: "copy-a" };
  const walletB = { ...baseExecution, copyWallet: "copy-wallet-b", sendSignature: "copy-b" };

  assert.notEqual(executionKey(walletA), executionKey(walletB));
  const deduped = dedupeRows([walletA, walletB, { ...walletA, sendSignature: "copy-a-newer" }]);

  assert.equal(deduped.length, 2);
  assert.deepEqual(
    deduped.map((row) => row.copyWallet).sort(),
    ["copy-wallet-a", "copy-wallet-b"]
  );
  assert.equal(
    deduped.find((row) => row.copyWallet === "copy-wallet-a")?.sendSignature,
    "copy-a-newer"
  );
});

test("block position diagnostics fail quietly when confirmed block is unavailable", async () => {
  const diagnostics = await blockPositionDiagnostics(
    baseRow,
    { slot: 100 },
    rpcWithBlocks(new Map())
  );

  assert.equal(diagnostics.status, "unknown");
  assert.equal(diagnostics.targetTxIndex, null);
  assert.equal(diagnostics.copyTxIndex, null);
  assert.equal(diagnostics.sameSlotTxDelta, null);
  assert.match(diagnostics.unavailableReason, /target block unavailable/);
});

test("block position diagnostics fail quietly when target signature is missing", async () => {
  const diagnostics = await blockPositionDiagnostics(
    baseRow,
    { slot: 100 },
    rpcWithBlocks(new Map([
      [100, { signatures: ["before", "copy-sig", "after"] }]
    ]))
  );

  assert.equal(diagnostics.status, "unknown");
  assert.equal(diagnostics.targetTxIndex, null);
  assert.equal(diagnostics.copyTxIndex, null);
  assert.equal(diagnostics.sameSlotTxDelta, null);
  assert.equal(diagnostics.unavailableReason, "target signature not found in confirmed block");
});

test("block position diagnostics fail quietly when copy signature is missing", async () => {
  const diagnostics = await blockPositionDiagnostics(
    baseRow,
    { slot: 100 },
    rpcWithBlocks(new Map([
      [100, { signatures: ["before", "target-sig", "after"] }]
    ]))
  );

  assert.equal(diagnostics.status, "unknown");
  assert.equal(diagnostics.targetTxIndex, 1);
  assert.equal(diagnostics.copyTxIndex, null);
  assert.equal(diagnostics.sameSlotTxDelta, null);
  assert.equal(diagnostics.unavailableReason, "copy signature not found in confirmed block");
});

test("copy buy status distinguishes submitted, landed, and failed-on-chain", () => {
  assert.equal(
    buyStatus({ sendSignature: "copy-sig", sent: true, decision: "sent" }, { slot: null, err: null }),
    "buySubmitted"
  );
  assert.equal(
    buyStatus({ sendSignature: "copy-sig", sent: true, decision: "sent" }, { slot: 123, err: null }),
    "buyLanded"
  );
  assert.equal(
    buyStatus({ sendSignature: "copy-sig", sent: true, decision: "sent" }, { slot: 123, err: { InstructionError: [1, { Custom: 6024 }] } }),
    "buyFailedOnChain"
  );
});

test("missing landed copy transaction keeps submitted buy status", () => {
  const report = unknownChainReport(
    { ...baseRow, sent: true, decision: "sent" },
    "copy transaction not found at confirmed commitment"
  );

  assert.equal(report.status, "submitted");
  assert.equal(report.buyStatus, "buySubmitted");
  assert.equal(report.positionUnavailableReason, "copy transaction not found at confirmed commitment");
  assert.equal(report.slotDelta, null);
});

test("auto-sell status distinguishes submitted, landed, and failed-on-chain", () => {
  assert.equal(
    autoSellStatus({ autoSellSendSignature: "sell-sig", autoSellSent: true, autoSellDecision: "sent" }, { slot: null, err: null }),
    "autoSellSubmitted"
  );
  assert.equal(
    autoSellStatus({ autoSellSendSignature: "sell-sig", autoSellSent: true, autoSellDecision: "sent" }, { slot: 123, err: null }),
    "autoSellLanded"
  );
  assert.equal(
    autoSellStatus({ autoSellSendSignature: "sell-sig", autoSellSent: true, autoSellDecision: "sent" }, { slot: 123, err: { InstructionError: [1, { Custom: 6024 }] } }),
    "autoSellFailedOnChain"
  );
});

test("sync watch loop limits new-row and refresh batches", () => {
  assert.equal(syncLimitForCycle({
    hasNewRows: true,
    rowCount: 92,
    lastSyncedCount: -1,
    recentLimit: 100,
    refreshRecentLimit: 3,
    newRowBackfill: 2
  }), 100);

  assert.equal(syncLimitForCycle({
    hasNewRows: true,
    rowCount: 93,
    lastSyncedCount: 92,
    recentLimit: 100,
    refreshRecentLimit: 3,
    newRowBackfill: 0
  }), 1);

  assert.equal(syncLimitForCycle({
    hasNewRows: false,
    rowCount: 93,
    lastSyncedCount: 93,
    recentLimit: 100,
    refreshRecentLimit: 1,
    newRowBackfill: 0
  }), 1);
});
