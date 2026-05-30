import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { copyTradeLiveExecutionBlockedReason } from "../dist/copytrade-execution-mode.js";
import {
  copyTradeSignalAgeBlockedReason,
  copyTradeSignalProviderAllows,
  copyTradeSignalRaceKey,
  copyTradeSignalRaceLogPayload,
  createCopyTradeSignalRaceTracker,
  parseCopyTradeSignalProvider
} from "../dist/copytrade-signal-race.js";

const nowMs = Date.parse("2026-05-30T00:00:00.000Z");

function trade(overrides = {}) {
  return {
    observedAt: new Date(nowMs).toISOString(),
    provider: "pumpportal",
    targetWallet: "TargetWallet111111111111111111111111111111",
    label: null,
    action: "buy",
    mint: "Mint111111111111111111111111111111111111111",
    signature: "ObservedSignature11111111111111111111111111",
    timestamp: nowMs / 1000,
    feePayer: "TargetWallet111111111111111111111111111111",
    source: "PUMP_FUN",
    input: {
      mint: "So11111111111111111111111111111111111111112",
      symbol: "SOL",
      amount: 0.25
    },
    output: {
      mint: "Mint111111111111111111111111111111111111111",
      symbol: null,
      amount: 1000
    },
    solAmount: 0.25,
    tokenAmount: 1000,
    pool: "pump",
    marketCapSol: null,
    pumpFunUrl: null,
    solscanTokenUrl: null,
    solscanTxUrl: null,
    raw: {},
    ...overrides
  };
}

test("copy trade signal provider defaults to PumpPortal and parallel enables Geyser", () => {
  assert.equal(parseCopyTradeSignalProvider(undefined), "pumpportal");
  assert.equal(parseCopyTradeSignalProvider(""), "pumpportal");
  assert.equal(parseCopyTradeSignalProvider("pumpportal"), "pumpportal");
  assert.equal(parseCopyTradeSignalProvider("parallel"), "parallel");
  assert.equal(parseCopyTradeSignalProvider("geyser"), "parallel");
  assert.equal(parseCopyTradeSignalProvider("unknown"), "pumpportal");

  assert.equal(copyTradeSignalProviderAllows("pumpportal", "pumpportal"), true);
  assert.equal(copyTradeSignalProviderAllows("pumpportal", "geyser"), false);
  assert.equal(copyTradeSignalProviderAllows("parallel", "pumpportal"), true);
  assert.equal(copyTradeSignalProviderAllows("parallel", "geyser"), true);
});

test("copy trade signal race lets PumpPortal first win and Geyser duplicate lose", () => {
  const tracker = createCopyTradeSignalRaceTracker();
  const pumpPortalTrade = trade({ provider: "pumpportal" });
  const geyserTrade = trade({ provider: "geyser", source: "GEYSER_PUMP_BONDING_CURVE" });

  const first = tracker.claim(pumpPortalTrade, nowMs);
  const duplicate = tracker.claim(geyserTrade, nowMs + 4);

  assert.equal(first.outcome, "won");
  assert.equal(first.record.provider, "pumpportal");
  assert.equal(duplicate.outcome, "duplicate");
  assert.equal(duplicate.record.provider, "pumpportal");
  assert.equal(tracker.size(), 1);
  assert.deepEqual(copyTradeSignalRaceLogPayload({
    mode: "parallel",
    trade: geyserTrade,
    outcome: "duplicate",
    winner: duplicate.record,
    key: duplicate.key
  }), {
    event: "copy_trade_signal_race",
    mode: "parallel",
    provider: "geyser",
    observedSignature: "ObservedSignature11111111111111111111111111",
    targetWallet: "TargetWallet111111111111111111111111111111",
    mint: "Mint111111111111111111111111111111111111111",
    outcome: "duplicate",
    reason: null,
    raceKey: copyTradeSignalRaceKey(geyserTrade),
    winnerProvider: "pumpportal",
    winnerClaimedAtMs: nowMs
  });
});

test("copy trade signal race lets Geyser first win and PumpPortal duplicate lose", () => {
  const tracker = createCopyTradeSignalRaceTracker();
  const geyserTrade = trade({ provider: "geyser", source: "GEYSER_PUMP_BONDING_CURVE" });
  const pumpPortalTrade = trade({ provider: "pumpportal" });

  const first = tracker.claim(geyserTrade, nowMs);
  const duplicate = tracker.claim(pumpPortalTrade, nowMs + 6);
  const duplicateLater = tracker.claim(geyserTrade, nowMs + 8);

  assert.equal(first.outcome, "won");
  assert.equal(first.record.provider, "geyser");
  assert.equal(duplicate.outcome, "duplicate");
  assert.equal(duplicate.record.provider, "geyser");
  assert.equal(duplicateLater.outcome, "duplicate");
  assert.equal(duplicateLater.record.provider, "geyser");
});

test("stale signals are skipped before claiming the race", () => {
  const staleTrade = trade({ timestamp: (nowMs - 61_000) / 1000 });
  const missingTimestampTrade = trade({ timestamp: null });

  assert.match(
    copyTradeSignalAgeBlockedReason({
      trade: staleTrade,
      maxSignalAgeMs: 60_000,
      nowMs
    }),
    /observed trade signal is 61s old/
  );
  assert.match(
    copyTradeSignalAgeBlockedReason({
      trade: missingTimestampTrade,
      maxSignalAgeMs: 60_000,
      nowMs
    }),
    /timestamp is missing/
  );
});

test("emergency stop remains a provider-neutral live gate", () => {
  assert.equal(
    copyTradeLiveExecutionBlockedReason({
      copyTradeEnabled: true,
      copyTradeDryRun: false,
      copyTradeEmergencyStopped: true
    }),
    "copy trade emergency stop is active"
  );
});

test("index wires PumpPortal and Geyser through the shared signal race path", () => {
  const indexSource = readFileSync(new URL("../src/index.ts", import.meta.url), "utf8");

  assert.match(indexSource, /copyTradeSignalProvider: parseCopyTradeSignalProvider\(process\.env\.COPY_TRADE_SIGNAL_PROVIDER\)/);
  assert.match(indexSource, /if \(!copyTradeSignalProviderAllows\(config\.copyTradeSignalProvider, "geyser"\)\)/);
  assert.match(indexSource, /return handleWalletTradeSignal\(trade, \{/);
  assert.match(indexSource, /copyTradeSignalAgeBlockedReason\(\{[\s\S]*maxSignalAgeMs: config\.copyTradeMaxSignalAgeMs/);
  assert.match(indexSource, /copyTradeSignalRaceTracker\.claim\(trade, receivedAtMs\)/);
  assert.match(indexSource, /enabled: config\.geyserEnabled \|\| config\.copyTradeSignalProvider === "parallel"/);
});
