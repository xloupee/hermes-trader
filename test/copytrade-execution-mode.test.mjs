import assert from "node:assert/strict";
import test from "node:test";
import {
  copyTradeExecutionStateLabel,
  copyTradeLiveExecutionBlockedReason,
  copyTradeLiveExecutionEnabled,
  formatCopyTradeExecutionStateLog
} from "../dist/copytrade-execution-mode.js";

test("copy trade execution mode blocks live PumpPortal submissions by default", () => {
  const config = { copyTradeEnabled: false, copyTradeDryRun: true };

  assert.equal(copyTradeLiveExecutionEnabled(config), false);
  assert.equal(copyTradeLiveExecutionBlockedReason(config), "COPY_TRADE_ENABLED is not true");
  assert.equal(copyTradeExecutionStateLabel(config), "DISABLED + DRY RUN");
  assert.match(formatCopyTradeExecutionStateLog(config), /livePumpPortalSubmissions=blocked/);
  assert.match(formatCopyTradeExecutionStateLog(config), /emergencyStop=inactive/);
});

test("copy trade execution mode requires enabled and not dry-run for live submissions", () => {
  assert.equal(copyTradeLiveExecutionEnabled({ copyTradeEnabled: true, copyTradeDryRun: true }), false);
  assert.equal(
    copyTradeLiveExecutionBlockedReason({ copyTradeEnabled: true, copyTradeDryRun: true }),
    "COPY_TRADE_DRY_RUN is enabled"
  );
  assert.equal(copyTradeExecutionStateLabel({ copyTradeEnabled: true, copyTradeDryRun: true }), "DRY RUN");

  const liveConfig = { copyTradeEnabled: true, copyTradeDryRun: false };

  assert.equal(copyTradeLiveExecutionEnabled(liveConfig), true);
  assert.equal(copyTradeLiveExecutionBlockedReason(liveConfig), null);
  assert.equal(copyTradeExecutionStateLabel(liveConfig), "LIVE");
  assert.match(formatCopyTradeExecutionStateLog(liveConfig), /livePumpPortalSubmissions=allowed/);
});

test("copy trade execution mode emergency stop overrides live env settings", () => {
  const config = {
    copyTradeEnabled: true,
    copyTradeDryRun: false,
    copyTradeEmergencyStopped: true
  };

  assert.equal(copyTradeLiveExecutionEnabled(config), false);
  assert.equal(copyTradeLiveExecutionBlockedReason(config), "copy trade emergency stop is active");
  assert.equal(copyTradeExecutionStateLabel(config), "EMERGENCY STOPPED");
  assert.match(formatCopyTradeExecutionStateLog(config), /emergencyStop=active/);
  assert.match(formatCopyTradeExecutionStateLog(config), /livePumpPortalSubmissions=blocked/);
});
