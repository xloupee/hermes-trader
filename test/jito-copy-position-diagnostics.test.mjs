import assert from "node:assert/strict";
import test from "node:test";

import {
  autoSellStatus,
  blockPositionDiagnostics,
  blockPositionDiagnosticsWithRetry,
  buyStatus,
  chainReportFromRustConfirmation,
  dedupeRows,
  displayTxDelta,
  executionKey,
  pendingPositionRefreshRows,
  syncLimitForCycle,
  unknownChainReport
} from "../tools/jito-shredstream-rs/sync-local-copy-executions-to-supabase.mjs";
import {
  buildLandingScoreboard,
  buildPromotionComparison,
  evaluateCanarySample,
  evaluatePromotionCandidate,
  evaluateTargetTxDelta,
  evaluateTxDeltaCoverage,
} from "../tools/jito-shredstream-rs/landing-scoreboard-report.mjs";

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

test("rust confirmation report carries block position indexes into dashboard fields", async () => {
  const report = await chainReportFromRustConfirmation(
    {
      ...baseRow,
      observedAction: "buy",
      observedSignature: "target-sig",
      sendSignature: "copy-sig",
      sent: true,
      decision: "sent"
    },
    {
      status: "landed",
      ok: true,
      confirmationSlot: 100,
      targetTxIndex: 11,
      copyTxIndex: 15,
      sameSlotTxDelta: 4,
      txDelta: 4,
      blockPositionError: null
    }
  );

  assert.equal(report.status, "landed");
  assert.equal(report.buyStatus, "buyLanded");
  assert.equal(report.targetTxIndex, 11);
  assert.equal(report.copyTxIndex, 15);
  assert.equal(report.sameSlotTxDelta, 4);
  assert.equal(report.txDelta, 4);
  assert.equal(report.positionUnavailableReason, null);
  assert.equal(report.blockPositionDiagnostics.status, "found");
  assert.equal(report.blockPositionDiagnostics.targetTxIndex, 11);
  assert.equal(report.blockPositionDiagnostics.copyTxIndex, 15);
  assert.equal(report.blockPositionDiagnostics.txDelta, 4);
});

test("rust confirmation report can hydrate missing block position from sync RPC", async () => {
  const report = await chainReportFromRustConfirmation(
    {
      ...baseRow,
      observedAction: "buy",
      observedSignature: "target-sig",
      sendSignature: "copy-sig",
      sent: true,
      decision: "sent"
    },
    {
      status: "landed",
      ok: true,
      confirmationSlot: 100,
      blockPositionError: "rust getBlock failed"
    },
    rpcWithBlocks(new Map([
      [100, { signatures: ["before", "target-sig", "middle", "copy-sig", "after"] }]
    ]))
  );

  assert.equal(report.status, "landed");
  assert.equal(report.targetTxIndex, 1);
  assert.equal(report.copyTxIndex, 3);
  assert.equal(report.sameSlotTxDelta, 2);
  assert.equal(report.txDelta, 2);
  assert.equal(report.positionUnavailableReason, null);
  assert.equal(report.blockPositionDiagnostics.status, "found");
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

test("block position diagnostics retry temporary block unavailability", async () => {
  let calls = 0;
  const diagnostics = await blockPositionDiagnosticsWithRetry(
    baseRow,
    { slot: 100 },
    async (method, params) => {
      assert.equal(method, "getBlock");
      assert.equal(params[0], 100);
      calls += 1;
      if (calls === 1) {
        throw new Error("Block not available for slot 100");
      }
      return { signatures: ["before", "target-sig", "copy-sig"] };
    },
    { attempts: 2, retryDelayMs: 0 }
  );

  assert.equal(calls, 2);
  assert.equal(diagnostics.status, "found");
  assert.equal(diagnostics.txDelta, 1);
});

test("block position diagnostics retry temporary RPC rate limits", async () => {
  let calls = 0;
  const diagnostics = await blockPositionDiagnosticsWithRetry(
    baseRow,
    { slot: 100 },
    async (method, params) => {
      assert.equal(method, "getBlock");
      assert.equal(params[0], 100);
      calls += 1;
      if (calls === 1) {
        throw new Error("getBlock HTTP status: HTTP status client error (429 Too Many Requests)");
      }
      return { signatures: ["before", "target-sig", "copy-sig"] };
    },
    { attempts: 2, retryDelayMs: 0 }
  );

  assert.equal(calls, 2);
  assert.equal(diagnostics.status, "found");
  assert.equal(diagnostics.txDelta, 1);
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

test("pending position refresh selects submitted copy buys missing tx delta", () => {
  const completeExecution = {
    schema: "copytrade.localExecution.v1",
    provider: "shredstream",
    observedSignature: "observed-complete",
    observedWallet: "wallet",
    copyWallet: "copy-wallet",
    observedAction: "buy",
    mint: "mint-complete",
    sendSignature: "copy-complete",
    sent: true,
    decision: "sent"
  };
  const missingExecution = {
    ...completeExecution,
    observedSignature: "observed-missing",
    mint: "mint-missing",
    sendSignature: "copy-missing"
  };
  const skippedExecution = {
    ...completeExecution,
    observedSignature: "observed-skipped",
    mint: "mint-skipped",
    sendSignature: null,
    sent: false,
    decision: "skipped"
  };

  const rows = pendingPositionRefreshRows([
    completeExecution,
    missingExecution,
    skippedExecution,
    {
      schema: "copytrade.transactionConfirmation.v1",
      provider: "shredstream",
      observedSignature: "observed-complete",
      copyWallet: "copy-wallet",
      mint: "mint-complete",
      transactionRole: "copy_buy",
      signature: "copy-complete",
      targetTxIndex: 1,
      copyTxIndex: 3,
      txDelta: 2
    }
  ], 10);

  assert.deepEqual(rows.map((row) => row.observedSignature), ["observed-missing"]);
});

test("landing scoreboard fails when tx delta coverage is below gate", () => {
  const scoreboard = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-base-a",
      observedAction: "buy",
      sendSignature: "copy-a",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 0,
        txDelta: 2
      }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-base-b",
      observedAction: "buy",
      sendSignature: "copy-b",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 1
      }
    }
  ], { includeUnsent: false, minCoverage: 0.9, minPositionEligible: 1 });
  const gate = evaluateTxDeltaCoverage(scoreboard.summary, { minCoverage: 0.9, minPositionEligible: 1 });

  assert.equal(scoreboard.summary.positionEligible, 2);
  assert.equal(scoreboard.summary.txDeltaPresent, 1);
  assert.equal(gate.ok, false);
  assert.equal(scoreboard.txDeltaGate.ok, false);
  assert.match(gate.reason, /below 90\.0%/);
});

test("landing scoreboard groups copy buys by transaction shape", () => {
  const scoreboard = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-canary-a",
      observedAction: "buy",
      routeLayout: "direct-pump",
      instructionCount: 5,
      signedTxBytes: 612,
      sendSignature: "copy-a",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 0,
        txDelta: 8
      }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-canary-b",
      observedAction: "buy",
      routeLayout: "direct-pump",
      instructionCount: 5,
      signedTxBytes: 645,
      sendSignature: "copy-b",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 0,
        txDelta: 12
      }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-base-a",
      observedAction: "buy",
      routeLayout: "migrated-amm",
      instructionCount: 6,
      signedTxBytes: 934,
      sendSignature: "copy-c",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 1,
        txDelta: 300
      }
    }
  ], { includeUnsent: false, minCoverage: 0.9, minPositionEligible: 1 });

  assert.deepEqual(scoreboard.byTransactionShape.map((row) => row.shape), [
    "direct-pump | ix=5 | bytes=600-699",
    "migrated-amm | ix=6 | bytes=900-999"
  ]);
  assert.equal(scoreboard.byTransactionShape[0].sent, 2);
  assert.equal(scoreboard.byTransactionShape[0].p50TxDelta, 8);
  assert.equal(scoreboard.byTransactionShape[0].targetTxDeltaHits, 1);
  assert.equal(scoreboard.byTransactionShape[0].targetTxDeltaRate, 0.5);
  assert.equal(scoreboard.byTransactionShape[0].signedTxBytesPresent, 2);
  assert.equal(scoreboard.byTransactionShape[0].signedTxBytesCoverage, 1);
  assert.equal(scoreboard.byTransactionShape[0].p50SignedTxBytes, 612);
  assert.equal(scoreboard.byTransactionShape[1].p50TxDelta, 300);
});

test("landing scoreboard reports target tx delta gate without changing sample gate", () => {
  const passing = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-base-b",
      observedAction: "buy",
      sendSignature: "copy-a",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 0,
        txDelta: 8
      }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-canary-a",
      observedAction: "buy",
      sendSignature: "copy-b",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 0,
        txDelta: 10
      }
    }
  ], { includeUnsent: false, minSent: 20, targetTxDelta: 10 });
  const failing = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-canary-b",
      observedAction: "buy",
      sendSignature: "copy-c",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 0,
        txDelta: 27
      }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-base-a",
      observedAction: "buy",
      sendSignature: "copy-d",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 1,
        txDelta: 80
      }
    }
  ], { includeUnsent: false, minSent: 20, targetTxDelta: 10 });

  assert.equal(passing.targetGate.ok, true);
  assert.equal(passing.sampleGate.ok, false);
  assert.equal(failing.targetGate.ok, false);
  assert.match(failing.targetGate.reason, /above target 10/);
  assert.equal(evaluateTargetTxDelta(failing.summary).ok, false);
});

test("landing scoreboard reports same-slot rate and waits for canary sample size", () => {
  const scoreboard = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-base-b",
      observedAction: "buy",
      sendSignature: "copy-a",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 0,
        txDelta: 8
      }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-canary-a",
      observedAction: "buy",
      sendSignature: "copy-b",
      sent: true,
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 1,
        txDelta: 80
      }
    }
  ], { includeUnsent: false, minSent: 20, targetTxDelta: 10 });

  assert.equal(scoreboard.summary.landedRate, 1);
  assert.equal(scoreboard.summary.sameSlotLanded, 1);
  assert.equal(scoreboard.summary.sameSlotRate, 0.5);
  assert.equal(scoreboard.summary.targetTxDelta, 10);
  assert.equal(scoreboard.summary.targetTxDeltaHits, 1);
  assert.equal(scoreboard.summary.targetTxDeltaRate, 0.5);
  assert.equal(scoreboard.summary.signedTxBytesPresent, 0);
  assert.equal(scoreboard.summary.signedTxBytesCoverage, 0);
  assert.equal(scoreboard.sampleGate.ok, false);
  assert.match(scoreboard.sampleGate.reason, /only 2 sent rows/);
  assert.equal(evaluateCanarySample(scoreboard.summary, { minSent: 2 }).ok, true);
});

test("landing scoreboard includes TPU dispatch attempts in report-only lane scores", () => {
  const scoreboard = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-canary-b",
      observedAction: "buy",
      sendSignature: "copy-a",
      sent: true,
      sendLaneAttribution: {
        firstAckLane: "helius-sender-1-fast:sender.helius-rpc.com",
        firstAckAtMs: 123,
        allAttempts: [
          {
            label: "helius-sender-1-fast:sender.helius-rpc.com",
            kind: "helius_sender",
            status: "submitted",
            durationMs: 4,
            ackAt: 123
          },
          {
            label: "tpu-quic",
            kind: "tpu_quic",
            mode: "fanout_slots",
            status: "dispatched",
            durationMs: 2,
            fanoutSlots: 12
          }
        ]
      },
      rustTransactionConfirmation: {
        status: "landed",
        ok: true,
        slotDelta: 0,
        txDelta: 9
      }
    }
  ], { includeUnsent: false, minSent: 1, targetTxDelta: 10 });

  const tpuLane = scoreboard.adaptiveLaneScores.find((row) => row.lane === "tpu-quic/fanout_slots");
  assert.ok(tpuLane);
  assert.equal(tpuLane.sent, 1);
  assert.equal(tpuLane.dispatchedAttempts, 1);
  assert.equal(tpuLane.submittedAttempts, 0);
  assert.equal(tpuLane.targetTxDeltaHits, 1);
  assert.equal(tpuLane.errorClasses.length, 0);
});

test("promotion candidate requires landing and tx delta improvement", () => {
  const baseline = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "base-a",
      sent: true,
      observedToSignedMs: 8,
      observedToSendSubmittedMs: 10,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 80 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "base-b",
      sent: true,
      observedToSignedMs: 9,
      observedToSendSubmittedMs: 12,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 1, txDelta: 140 }
    }
  ], { includeUnsent: false, minSent: 1, targetTxDelta: 50 });
  const canary = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "canary-a",
      sent: true,
      observedToSignedMs: 7,
      observedToSendSubmittedMs: 9,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 20 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "canary-b",
      sent: true,
      observedToSignedMs: 8,
      observedToSendSubmittedMs: 10,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 40 }
    }
  ], { includeUnsent: false, minSent: 1, targetTxDelta: 50 });

  const promotion = evaluatePromotionCandidate(baseline.summary, canary.summary, {
    txDeltaTarget: 50
  });

  assert.equal(promotion.ok, true);
  assert.deepEqual(promotion.failed, []);
  assert.deepEqual(promotion.unknown, []);
});

test("promotion candidate fails on landed-rate regression even with faster tx delta", () => {
  const baseline = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "base-a",
      sent: true,
      observedToSignedMs: 8,
      observedToSendSubmittedMs: 10,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 80 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "base-b",
      sent: true,
      observedToSignedMs: 9,
      observedToSendSubmittedMs: 12,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 1, txDelta: 120 }
    }
  ], { includeUnsent: false, minSent: 1, targetTxDelta: 50 });
  const canary = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "canary-a",
      sent: true,
      observedToSignedMs: 7,
      observedToSendSubmittedMs: 9,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 20 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "canary-b",
      sent: true,
      observedToSignedMs: 8,
      observedToSendSubmittedMs: 10
    }
  ], { includeUnsent: false, minSent: 1, targetTxDelta: 50 });

  const promotion = evaluatePromotionCandidate(baseline.summary, canary.summary, {
    txDeltaTarget: 50
  });

  assert.equal(promotion.ok, false);
  assert.ok(promotion.failed.includes("landed_rate_no_regression"));
});

test("promotion candidate fails on hot-path timing regression", () => {
  const baseline = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "base-a",
      sent: true,
      observedToSignedMs: 8,
      observedToSendSubmittedMs: 10,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 80 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "base-b",
      sent: true,
      observedToSignedMs: 9,
      observedToSendSubmittedMs: 12,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 1, txDelta: 120 }
    }
  ], { includeUnsent: false, minSent: 1, targetTxDelta: 50 });
  const canary = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "canary-a",
      sent: true,
      observedToSignedMs: 18,
      observedToSendSubmittedMs: 25,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 20 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedAction: "buy",
      sendSignature: "canary-b",
      sent: true,
      observedToSignedMs: 19,
      observedToSendSubmittedMs: 26,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 40 }
    }
  ], { includeUnsent: false, minSent: 1, targetTxDelta: 50 });

  const promotion = evaluatePromotionCandidate(baseline.summary, canary.summary, {
    txDeltaTarget: 50
  });

  assert.equal(promotion.ok, false);
  assert.ok(promotion.failed.includes("p90_observed_to_signed_no_regression"));
  assert.ok(promotion.failed.includes("p90_observed_to_submitted_no_regression"));
});

test("promotion candidate fails on duplicate observed trade send signatures", () => {
  const baseline = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-base-a",
      observedAction: "buy",
      sendSignature: "base-a",
      sent: true,
      observedToSignedMs: 8,
      observedToSendSubmittedMs: 10,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 80 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-base-b",
      observedAction: "buy",
      sendSignature: "base-b",
      sent: true,
      observedToSignedMs: 9,
      observedToSendSubmittedMs: 12,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 1, txDelta: 120 }
    }
  ], { includeUnsent: false, minSent: 1, targetTxDelta: 50 });
  const canary = buildLandingScoreboard([
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-canary-dup",
      observedAction: "buy",
      sendSignature: "canary-a",
      sent: true,
      observedToSignedMs: 7,
      observedToSendSubmittedMs: 9,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 20 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedSignature: "obs-canary-dup",
      observedAction: "buy",
      sendSignature: "canary-b",
      sent: true,
      observedToSignedMs: 8,
      observedToSendSubmittedMs: 10,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 40 }
    }
  ], { includeUnsent: false, minSent: 1, targetTxDelta: 50 });

  const promotion = evaluatePromotionCandidate(baseline.summary, canary.summary, {
    txDeltaTarget: 50
  });

  assert.equal(canary.summary.duplicateObservedSendGroups, 1);
  assert.equal(promotion.ok, false);
  assert.ok(promotion.failed.includes("no_duplicate_observed_send_signatures"));
});

test("promotion comparison uses non-overlapping observedAt windows", () => {
  const rows = [
    {
      schema: "copytrade.localExecution.v1",
      observedAtMs: 1000,
      observedAction: "buy",
      sendSignature: "base-a",
      sent: true,
      observedToSignedMs: 8,
      observedToSendSubmittedMs: 10,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 90 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedAtMs: 2000,
      observedAction: "buy",
      sendSignature: "base-b",
      sent: true,
      observedToSignedMs: 9,
      observedToSendSubmittedMs: 12,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 1, txDelta: 120 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedAtMs: 3000,
      observedAction: "buy",
      sendSignature: "canary-a",
      sent: true,
      observedToSignedMs: 7,
      observedToSendSubmittedMs: 9,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 20 }
    },
    {
      schema: "copytrade.localExecution.v1",
      observedAtMs: 4000,
      observedAction: "buy",
      sendSignature: "canary-b",
      sent: true,
      observedToSignedMs: 8,
      observedToSendSubmittedMs: 10,
      rustTransactionConfirmation: { status: "landed", ok: true, slotDelta: 0, txDelta: 40 }
    }
  ];

  const comparison = buildPromotionComparison(rows, {
    includeUnsent: false,
    minSent: 1,
    targetTxDelta: 50,
    baselineSinceMs: 1000,
    baselineUntilMs: 3000,
    canarySinceMs: 3000,
    promotionOptions: { txDeltaTarget: 50 }
  });

  assert.deepEqual(comparison.baseline.rows.map((row) => row.sendSignature), ["base-a", "base-b"]);
  assert.deepEqual(comparison.canary.rows.map((row) => row.sendSignature), ["canary-a", "canary-b"]);
  assert.equal(comparison.baseline.summary.sent, 2);
  assert.equal(comparison.canary.summary.sent, 2);
  assert.equal(comparison.baseline.summary.targetTxDeltaRate, 0);
  assert.equal(comparison.canary.summary.targetTxDeltaRate, 1);
  assert.equal(comparison.promotion.ok, true);
  assert.deepEqual(comparison.promotion.failed, []);
});
