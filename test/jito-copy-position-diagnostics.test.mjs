import assert from "node:assert/strict";
import test from "node:test";

import { blockPositionDiagnostics } from "../tools/jito-shredstream-rs/sync-local-copy-executions-to-supabase.mjs";

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
