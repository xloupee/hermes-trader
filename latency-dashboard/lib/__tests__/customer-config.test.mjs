import assert from "node:assert/strict";
import test from "node:test";
import { createConfigDiff, plannedExposure } from "../customer-config/diff.mjs";

function config() {
  return {
    copyTradingEnabled: true,
    amountPerBuySol: 0.15,
    maxDailyBuys: 20,
    maxPositionSol: 0.6,
    buySlippagePercent: 12,
    sellSlippagePercent: 15,
    priorityFeeSol: 0.0025,
    stopLossEnabled: true,
    stopLossPercent: 18,
    trailingStopEnabled: true,
    trailingStopPercent: 14,
    devSnipingEnabled: true,
    blockDevSelling: true,
    targets: [
      { id: "one", label: "Wallet one", enabled: true, amountOverrideSol: null },
      { id: "two", label: "Wallet two", enabled: true, amountOverrideSol: 0.1 }
    ],
    alerts: {
      copiedBuy: true,
      copiedSell: true,
      positionWarning: true,
      runtimeFailure: true,
      dailySummary: false
    }
  };
}

test("createConfigDiff returns no changes for identical settings", () => {
  const before = config();
  assert.deepEqual(createConfigDiff(before, structuredClone(before)), []);
});

test("createConfigDiff labels risk-increasing changes", () => {
  const before = config();
  const after = structuredClone(before);
  after.amountPerBuySol = 0.25;
  after.buySlippagePercent = 18;
  after.targets.push({ id: "three", label: "Wallet three", enabled: true, amountOverrideSol: null });
  const changes = createConfigDiff(before, after);

  assert.deepEqual(changes.map((change) => change.id), ["amount", "buy-slippage", "target-three"]);
  assert.ok(changes.every((change) => change.warning));
});

test("plannedExposure respects wallet amount overrides", () => {
  const fixture = config();
  assert.equal(plannedExposure(fixture), 0.25);
  fixture.targets[1].enabled = false;
  assert.equal(plannedExposure(fixture), 0.15);
});
