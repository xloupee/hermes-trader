import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, test } from "node:test";

import {
  landingComparisonSummary,
  landingSummary,
  overviewMetricValues,
  parseDashboardFilters,
  toQueryParams
} from "../dashboard-client.ts";

const outcomeCounts = (values = {}) => ({
  landed: 0,
  failed_on_chain: 0,
  ack_not_landed: 0,
  send_failed: 0,
  skipped: 0,
  unknown: 0,
  ...values
});

const summary = (outcome) => ({
  total: Object.values(outcome).reduce((total, value) => total + value, 0),
  landed: outcome.landed,
  outcome,
  side: { buy: 0, sell: 0, other: 0 }
});

describe("dashboard UI contract", () => {
  test("landed presets persist exact server filters", () => {
    const buyQuery = new URLSearchParams(toQueryParams({ outcome: "landed-buys" }, true));
    assert.equal(buyQuery.get("side"), "buy");
    assert.equal(buyQuery.get("outcome"), "landed");
    assert.equal(parseDashboardFilters(buyQuery).outcome, "landed-buys");

    const sellQuery = new URLSearchParams(toQueryParams({ outcome: "landed-sells" }, true));
    assert.equal(sellQuery.get("side"), "sell");
    assert.equal(sellQuery.get("outcome"), "landed");
    assert.equal(parseDashboardFilters(sellQuery).outcome, "landed-sells");
  });

  test("overview cards map exact summaries and denominator", () => {
    const all = summary(outcomeCounts({ landed: 5, failed_on_chain: 2, ack_not_landed: 2, send_failed: 1, skipped: 20 }));
    const buys = summary(outcomeCounts({ landed: 3 }));
    const sells = summary(outcomeCounts({ landed: 2 }));
    assert.deepEqual(overviewMetricValues(all, buys, sells), {
      landedBuys: 3,
      landedSells: 2,
      landingRate: "50%",
      nonLandedAttempts: 5
    });
  });

  test("landing labels preserve no-target, same-slot, and cross-slot semantics", () => {
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "no_target", copySlot: 44 }), "Landed · slot 44 · no target comparison");
    assert.equal(landingComparisonSummary({ landingComparison: "same_slot", copySlot: 44, sameSlotTxDelta: 2 }), "same-slot · slot 44 · 2 tx");
    assert.equal(landingComparisonSummary({ landingComparison: "cross_slot", slotDelta: 1, txDelta: 3 }), "cross-slot · +1 slot · 3 tx");
  });

  test("execution pages use canonical server filtering and an accessible eight-column table", () => {
    const overviewSource = readFileSync(new URL("../../components/dashboard/overview-dashboard.tsx", import.meta.url), "utf8");
    const executionsSource = readFileSync(new URL("../../components/dashboard/executions-dashboard.tsx", import.meta.url), "utf8");
    const tableSource = readFileSync(new URL("../../components/dashboard/execution-table.tsx", import.meta.url), "utf8");
    assert.match(overviewSource, /\/api\/dashboard\/overview/);
    assert.match(overviewSource, /\/api\/dashboard\/executions/);
    assert.match(executionsSource, /\/api\/dashboard\/executions/);
    assert.doesNotMatch(overviewSource, /applyLandingPreset|\.filter\(/);
    assert.doesNotMatch(executionsSource, /applyLandingPreset|\.filter\(/);
    assert.equal((tableSource.match(/<th>/g) || []).length, 8);
    assert.match(tableSource, /aria-label="Execution results"/);
  });
});
