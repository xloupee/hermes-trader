import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  createCopyTradeLatencyClock,
  createCopyTradeLatencyTrace,
  createCopyTradeLatencyTracker,
  formatCopyTradeLatencyLog,
  recordCopyTradeLatencyMilestone
} from "../dist/copytrade-latency.js";

const context = {
  chatId: "-1001234567890",
  sourceWallet: "SourceWallet111111111111111111111111111111",
  tradingWallet: "TradingWallet111111111111111111111111111",
  observedSignature: "ObservedSignature11111111111111111111111111",
  mint: "Mint111111111111111111111111111111111111111",
  mode: "live"
};

test("index runtime wires Helius receipt and normalization timestamps into copy buy latency", () => {
  const indexSource = readFileSync(new URL("../src/index.ts", import.meta.url), "utf8");

  assert.match(
    indexSource,
    /subscribeAccountTrade[\s\S]*keys: copyTradeWallets/
  );
  assert.match(
    indexSource,
    /handlePumpPortalAccountTrade[\s\S]*sendCopyTradeSimulationAlert\([\s\S]*receivedAtMs,\s*[\r\n\s]*normalizedAtMs: receivedAtMs/
  );
  assert.match(
    indexSource,
    /const signature = stringValue\(event\.signature[\s\S]*if \(!action \|\| trader !== targetWallet \|\| !mint \|\| !signature\)/
  );
  assert.match(
    indexSource,
    /await loadCopyTradeEmergencyStop\(\);[\s\S]*logCopyTradeExecutionState\(\);/
  );
  assert.match(
    indexSource,
    /async function handleHeliusWebhookEvents[\s\S]*const receivedAtMs = Date\.now\(\);[\s\S]*await handleHeliusSwap\(event, \{ receivedAtMs \}\);/
  );
  assert.match(
    indexSource,
    /async function handleHeliusSwap[\s\S]*const normalizedAtMs = Date\.now\(\);[\s\S]*sendCopyTradeSimulationAlert\([\s\S]*receivedAtMs,\s*[\r\n\s]*normalizedAtMs[\s\S]*\);/
  );
  assert.match(
    indexSource,
    /createCopyTradeLatencyTracker\([\s\S]*clock: createCopyTradeLatencyClock\(timing\)/
  );
  assert.match(
    indexSource,
    /copyTradeBuyIdempotency\.claimBuy\([\s\S]*copyTradeBuyRiskBlockedReason/
  );
  assert.match(
    indexSource,
    /if \(!idempotencyClaim\.claimed\) \{[\s\S]*coin already handled[\s\S]*return;[\s\S]*\}\s*durableCopyBuyClaimKey = durableCopyBuyKey;/
  );
  assert.match(
    indexSource,
    /await Promise\.all\(copyTradeEntries\.map\(\(entry\) =>[\s\S]*sendCopyTradeSimulationAlert/
  );
  assert.match(
    indexSource,
    /function scheduleCopyTradeTrailingSellsAfterConfirmation[\s\S]*await waitForSignatureConfirmation\(buySignature\)[\s\S]*await scheduleCopyTradeTrailingSells/
  );
  assert.match(
    indexSource,
    /if \(resultOk\(result\) && resultSignature\(result\)\) \{[\s\S]*scheduleCopyTradeTrailingSellsAfterConfirmation\([\s\S]*buySignature: resultSignature\(result\)/
  );
  assert.doesNotMatch(
    indexSource,
    /if \(result\.ok\) \{[\s\S]{0,400}await scheduleCopyTradeTrailingSells\(/
  );
});

test("copy trade latency trace records deterministic total and stage timings", () => {
  let trace = createCopyTradeLatencyTrace({ context, nowMs: 1_000 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "normalized", nowMs: 1_012 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "request_built", nowMs: 1_020 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "risk_checked", nowMs: 1_033 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "live_gate_checked", nowMs: 1_040 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "balance_checked", nowMs: 1_075 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "submit_started", nowMs: 1_090 });
  trace = recordCopyTradeLatencyMilestone({
    trace,
    milestone: "submit_finished",
    nowMs: 1_155,
    details: {
      status: "submitted",
      signature: "CopyBuySignature111111111111111111111111111"
    }
  });

  assert.deepEqual(formatCopyTradeLatencyLog(trace), {
    event: "copy_trade_latency",
    chatId: "-1001234567890",
    sourceWallet: "SourceWallet111111111111111111111111111111",
    tradingWallet: "TradingWallet111111111111111111111111111",
    observedSignature: "ObservedSignature11111111111111111111111111",
    mint: "Mint111111111111111111111111111111111111111",
    mode: "live",
    status: "submitted",
    reason: null,
    signature: "CopyBuySignature111111111111111111111111111",
    totalMs: 155,
    stagesMs: {
      received_to_normalized: 12,
      normalized_to_request_built: 8,
      request_built_to_risk_checked: 13,
      risk_checked_to_live_gate_checked: 7,
      live_gate_checked_to_balance_checked: 35,
      balance_checked_to_submit_started: 15,
      submit_started_to_submit_finished: 65
    }
  });
});

test("copy trade latency tracker uses an injectable clock and formats skipped copy buys", () => {
  const times = [10_000, 10_005, 10_014, 10_016];
  const tracker = createCopyTradeLatencyTracker(
    {
      ...context,
      mode: "dry"
    },
    {
      clock: () => times.shift() ?? 10_016
    }
  );

  tracker.mark("normalized");
  tracker.mark("risk_checked", { status: "blocked" });
  tracker.skip("COPY_TRADE_DRY_RUN=true", { status: "skipped" });

  assert.deepEqual(tracker.format(), {
    event: "copy_trade_latency",
    chatId: "-1001234567890",
    sourceWallet: "SourceWallet111111111111111111111111111111",
    tradingWallet: "TradingWallet111111111111111111111111111",
    observedSignature: "ObservedSignature11111111111111111111111111",
    mint: "Mint111111111111111111111111111111111111111",
    mode: "dry",
    status: "skipped",
    reason: "COPY_TRADE_DRY_RUN=true",
    signature: null,
    totalMs: 16,
    stagesMs: {
      received_to_normalized: 5,
      normalized_to_risk_checked: 9,
      risk_checked_to_skipped: 2
    }
  });
});

test("copy trade latency clock carries webhook receipt and normalization timestamps first", () => {
  let fallbackNow = 20_000;
  const tracker = createCopyTradeLatencyTracker(
    {
      ...context,
      mode: "live"
    },
    {
      clock: createCopyTradeLatencyClock({
        receivedAtMs: 10_000,
        normalizedAtMs: 10_018,
        fallbackClock: () => {
          fallbackNow += 7;
          return fallbackNow;
        }
      })
    }
  );

  tracker.mark("normalized");
  tracker.mark("request_built");
  tracker.mark("risk_checked");
  tracker.skip("copy trade emergency stop is active", { status: "skipped" });

  assert.deepEqual(tracker.format().stagesMs, {
    received_to_normalized: 18,
    normalized_to_request_built: 9989,
    request_built_to_risk_checked: 7,
    risk_checked_to_skipped: 7
  });
});

test("copy trade latency skip after balance check preserves chronological stage order", () => {
  let trace = createCopyTradeLatencyTrace({ context, nowMs: 1_000 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "normalized", nowMs: 1_010 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "request_built", nowMs: 1_020 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "risk_checked", nowMs: 1_030 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "live_gate_checked", nowMs: 1_040 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "balance_checked", nowMs: 1_080 });
  trace = recordCopyTradeLatencyMilestone({
    trace,
    milestone: "skipped",
    nowMs: 1_090,
    details: {
      status: "skipped",
      reason: "copy trade emergency stop is active"
    }
  });

  assert.deepEqual(formatCopyTradeLatencyLog(trace).stagesMs, {
    received_to_normalized: 10,
    normalized_to_request_built: 10,
    request_built_to_risk_checked: 10,
    risk_checked_to_live_gate_checked: 10,
    live_gate_checked_to_balance_checked: 40,
    balance_checked_to_skipped: 10
  });
});

test("copy trade latency log omits unset context and clamps backwards clocks", () => {
  let trace = createCopyTradeLatencyTrace({
    context: {
      chatId: null,
      sourceWallet: "",
      tradingWallet: null,
      observedSignature: null,
      mint: "   ",
      mode: "live"
    },
    nowMs: 200
  });

  trace = recordCopyTradeLatencyMilestone({
    trace,
    milestone: "skipped",
    nowMs: 150,
    details: {
      reason: "signal is stale"
    }
  });

  assert.deepEqual(formatCopyTradeLatencyLog(trace), {
    event: "copy_trade_latency",
    chatId: null,
    sourceWallet: null,
    tradingWallet: null,
    observedSignature: null,
    mint: null,
    mode: "live",
    status: "skipped",
    reason: "signal is stale",
    signature: null,
    totalMs: 0,
    stagesMs: {
      received_to_skipped: 0
    }
  });
});

test("copy trade latency milestones are upserted to avoid duplicate stage keys", () => {
  let trace = createCopyTradeLatencyTrace({ context, nowMs: 1_000 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "normalized", nowMs: 1_010 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "normalized", nowMs: 1_030 });
  trace = recordCopyTradeLatencyMilestone({ trace, milestone: "request_built", nowMs: 1_045 });

  assert.deepEqual(
    trace.milestones.map(({ milestone, atMs }) => ({ milestone, atMs })),
    [
      { milestone: "received", atMs: 1_000 },
      { milestone: "normalized", atMs: 1_030 },
      { milestone: "request_built", atMs: 1_045 }
    ]
  );
  assert.deepEqual(formatCopyTradeLatencyLog(trace).stagesMs, {
    received_to_normalized: 30,
    normalized_to_request_built: 15
  });
});
