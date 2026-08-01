import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, test } from "node:test";

import {
  landingComparisonSummary,
  landingSummary,
  overviewMetricValues,
  parseDashboardFilters,
  shortText,
  toQueryParams
} from "../dashboard-client.ts";
import { executionFeed, feedIdentity } from "../feed-winners.ts";

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
  test("fixed filters serialize without legacy keys and landed presets remain exact", () => {
    const fixed = new URLSearchParams(toQueryParams({
      from: "2026-07-30T00:00:00Z",
      to: "2026-07-31T00:00:00Z",
      wallet: "Wallet123",
      side: "buy",
      outcome: "all",
      mint: "Mint123",
      route: "pump",
      provider: "jito"
    }, true));
    assert.deepEqual([...fixed.keys()].sort(), ["from", "mint", "provider", "route", "side", "to", "wallet"]);
    assert.equal(fixed.has("since"), false);
    assert.equal(fixed.has("action"), false);

    const buyQuery = new URLSearchParams(toQueryParams({ outcome: "landed-buys" }, true));
    assert.equal(buyQuery.get("side"), "buy");
    assert.equal(buyQuery.get("outcome"), "landed");
    assert.equal(parseDashboardFilters(buyQuery).outcome, "landed-buys");
    const sellQuery = new URLSearchParams(toQueryParams({ outcome: "landed-sells" }, true));
    assert.equal(sellQuery.get("side"), "sell");
    assert.equal(sellQuery.get("outcome"), "landed");
  });

  test("overview cards map exact summaries and denominator", () => {
    const all = summary(outcomeCounts({ landed: 5, failed_on_chain: 2, ack_not_landed: 2, send_failed: 1, skipped: 20 }));
    assert.deepEqual(overviewMetricValues(all, summary(outcomeCounts({ landed: 3 })), summary(outcomeCounts({ landed: 2 }))), {
      landedBuys: 3,
      landedSells: 2,
      landingRate: "50%",
      nonLandedAttempts: 5
    });
  });

  test("primary landing labels preserve canonical no-target, same-slot, and cross-slot wording", () => {
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "no_target", copySlot: 1355697770 }), "Landed · slot 1355697770 · no target comparison");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "same_slot", copySlot: 1355697770, sameSlotTxDelta: 42 }), "Landed · same slot · 42 tx after target");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "same_slot", copySlot: 1355697770, sameSlotTxDelta: -3 }), "Landed · same slot · 3 tx before target");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "same_slot", copySlot: 1355697770, sameSlotTxDelta: 0 }), "Landed · same slot · at target transaction");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "same_slot", copySlot: 1355697770, sameSlotTxDelta: null }), "Landed · same slot · tx delta unavailable");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "cross_slot", copySlot: 1355697770, slotDelta: 1 }), "Landed · +1 slot · copy slot 1355697770");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "cross_slot", copySlot: 1355697768, slotDelta: -2 }), "Landed · -2 slots · copy slot 1355697768");
    assert.equal(landingComparisonSummary({ landingComparison: "same_slot", copySlot: 44, sameSlotTxDelta: 2 }), "same-slot · slot 44 · 2 tx");
  });

  test("CopyChip abbreviates display text and copies the full API value", () => {
    const fullValue = "5WvP4QJ8M7JQkM4YE9x6JfJxAqQx1xA4cXqG9FvP6kLm";
    assert.notEqual(shortText(fullValue, 7), fullValue);
    const source = readFileSync(new URL("../../components/dashboard/copy-chip.tsx", import.meta.url), "utf8");
    assert.match(source, /const display = shortText\(value, 7\)/);
    assert.match(source, /navigator\.clipboard\.writeText\(value\)/);
    assert.doesNotMatch(source, /writeText\((?:display|shortText\()/);
  });

  test("execution pages use server filtering and detail renders only the sanitized DTO", () => {
    const overview = readFileSync(new URL("../../components/dashboard/overview-dashboard.tsx", import.meta.url), "utf8");
    const executions = readFileSync(new URL("../../components/dashboard/executions-dashboard.tsx", import.meta.url), "utf8");
    const detail = readFileSync(new URL("../../components/dashboard/execution-detail-dashboard.tsx", import.meta.url), "utf8");
    const detailRoute = readFileSync(new URL("../../app/api/dashboard/executions/[id]/route.ts", import.meta.url), "utf8");
    assert.match(overview, /\/api\/dashboard\/executions/);
    assert.doesNotMatch(overview, /\/api\/dashboard\/overview/);
    assert.match(executions, /\/api\/dashboard\/executions/);
    assert.doesNotMatch(overview, /applyLandingPreset|\.filter\(/);
    assert.doesNotMatch(executions, /applyLandingPreset|\.filter\(/);
    assert.match(detailRoute, /toDashboardExecution\(execution\)/);
    assert.match(detail, /<details><summary>Normalized execution JSON<\/summary>/);
    assert.match(detail, /JSON\.stringify\(row, null, 2\)/);
    assert.doesNotMatch(detail, /rawExecution|chainReport|privateKey|keypair|mnemonic|seed|custody/);
  });

  test("execution table remains accessible with eight columns", () => {
    const table = readFileSync(new URL("../../components/dashboard/execution-table.tsx", import.meta.url), "utf8");
    assert.equal((table.match(/<th>/g) || []).length, 8);
    assert.match(table, /aria-label="Execution results"/);
    assert.ok(table.indexOf("<th>Result / placement</th>") < table.indexOf("<th>Feed / route</th>"));
    assert.ok(table.indexOf("<th>Feed / route</th>") < table.indexOf("<th>Act</th>"));
    assert.match(table, /row\.selectedRoute \|\| "route unavailable"/);
  });

  test("feed identity preserves inbound source attribution and transport fallback", () => {
    assert.deepEqual(feedIdentity("vortex-fra"), { key: "vortex", label: "Vortex" });
    assert.deepEqual(feedIdentity("jito-primary"), { key: "jito", label: "Jito" });
    assert.deepEqual(feedIdentity("erpc-direct-fra"), { key: "erpc", label: "eRPC" });
    assert.deepEqual(executionFeed("profit-target-monitor", "shredstream"), { key: "shredstream", label: "ShredStream" });
  });
});
