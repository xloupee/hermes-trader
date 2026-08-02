import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { describe, test } from "node:test";

import {
  landingComparisonSummary,
  landingSummary,
  isDelayedLanding,
  leaderContext,
  leaderSummary,
  overviewMetricValues,
  parseDashboardFilters,
  shortText,
  toQueryParams
} from "../dashboard-client.ts";
import { dashboardInboundSourcePredicate, executionEvidenceCounts, executionFeed, feedIdentity, feedLeaderboard, feedTransportLabel, isLandedBuy, normalizeInboundFeedAttribution } from "../feed-winners.ts";
import { sendLaneIdentity } from "../send-lanes.ts";
import { formatUserDate, formatUserDateTime, formatUserTime, userTimeZoneLabel } from "../user-time.ts";

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
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "no_target", copySlot: 1355697770 }), "Landed · no target comparison");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "same_slot", copySlot: 1355697770, sameSlotTxDelta: 42 }), "Landed · same slot · 42 tx after target");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "same_slot", copySlot: 1355697770, sameSlotTxDelta: -3 }), "Landed · same slot · 3 tx before target");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "same_slot", copySlot: 1355697770, sameSlotTxDelta: 0 }), "Landed · same slot · at target transaction");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "same_slot", copySlot: 1355697770, sameSlotTxDelta: null }), "Landed · same slot · tx delta unavailable");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "cross_slot", copySlot: 1355697770, slotDelta: 1, txDelta: 730 }), "Landed · +1 slot · 730 tx after target");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "cross_slot", copySlot: 1355697768, slotDelta: -2, blockPositionDiagnostics: { crossSlotPositionSummary: { crossSlotTxDelta: -19 } } }), "Landed · -2 slots · 19 tx before target");
    assert.equal(landingSummary({ outcome: "landed", landingComparison: "cross_slot", copySlot: 1355697768, slotDelta: 2 }), "Landed · +2 slots · tx delta unavailable");
    assert.doesNotMatch(landingSummary({ outcome: "landed", landingComparison: "unavailable", copySlot: 1355697768 }), /copy slot/i);
    assert.equal(landingComparisonSummary({ landingComparison: "same_slot", copySlot: 44, sameSlotTxDelta: 2 }), "same-slot · slot 44 · 2 tx");
    assert.equal(isDelayedLanding({ outcome: "landed", slotDelta: 1 }), true);
    assert.equal(isDelayedLanding({ outcome: "landed", slotDelta: 2 }), true);
    assert.equal(isDelayedLanding({ outcome: "landed", slotDelta: 0 }), false);
    assert.equal(isDelayedLanding({ outcome: "failed_on_chain", slotDelta: 2 }), false);
  });

  test("validator leader summaries prefer copy location and mark leader changes", () => {
    assert.equal(leaderSummary({ leaderDiagnostics: null }), "n/a");
    assert.equal(leaderSummary({ leaderDiagnostics: {
      copyLeader: { broadRegion: "Europe", location: "DE:Hesse:Frankfurt", shortIdentity: "copy" },
      targetLeader: { broadRegion: "North America", location: "US:Virginia", shortIdentity: "target" },
      leaderChanged: true
    } }), "Europe changed");
    assert.equal(leaderContext({ leaderDiagnostics: {
      copyLeader: { shortIdentity: "copy...leader", broadRegion: "Europe" },
      targetLeader: { shortIdentity: "copy...leader", broadRegion: "Europe" },
      leaderChanged: false,
      regionPath: "Europe"
    } }), "copy...leader");
    assert.equal(leaderContext({ leaderDiagnostics: {
      copyLeader: { shortIdentity: "copy", broadRegion: "North America" },
      targetLeader: { shortIdentity: "target", broadRegion: "Europe" },
      leaderChanged: true,
      regionPath: "Europe -> North America"
    } }), "Europe -> North America");
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
    assert.match(detail, /Latency breakdown/);
    assert.match(detail, /localDetectUs\(row\)/);
    assert.match(detail, /row\.entryDecodeUs, row\.feedReceivedToEntriesReadyUs, row\.feedReceivedToDecodedUs/);
    assert.match(detail, /row\.txParseUs\).*row\.routeParseUs/s);
    assert.match(detail, /row\.unsignedBuildUs\).*row\.signUs/s);
    assert.match(detail, /ackDurationMs\(row\)/);
    assert.match(detail, /executionFeed\(row\.inboundSource\)/);
    assert.match(detail, /row\.inboundContributors/);
    assert.match(detail, /row\.inboundSelectionGeneration/);
    assert.doesNotMatch(detail, /rawExecution|chainReport|privateKey|keypair|mnemonic|seed|custody/);
  });

  test("execution table remains accessible with eleven columns", () => {
    const table = readFileSync(new URL("../../components/dashboard/execution-table.tsx", import.meta.url), "utf8");
    const styles = readFileSync(new URL("../../components/dashboard/dashboard-shared.module.css", import.meta.url), "utf8");
    assert.equal((table.match(/<th(?:\s|>)/g) || []).length, 11);
    assert.match(table, /aria-label="Execution results"/);
    assert.ok(table.indexOf("<th>Act</th>") < table.indexOf("<th>Result / placement</th>"));
    assert.ok(table.indexOf("<th>Result / placement</th>") < table.indexOf("<th>TX after</th>"));
    assert.ok(table.indexOf("<th>TX after</th>") < table.indexOf("<th>Leader</th>"));
    assert.ok(table.indexOf("<th>Leader</th>") < table.indexOf("<th>Feed</th>"));
    assert.ok(table.indexOf("<th>Feed</th>") < table.indexOf("<th>Lane / ACK</th>"));
    assert.ok(table.indexOf("<th>Lane / ACK</th>") < table.indexOf("<th>CA</th>"));
    assert.ok(table.indexOf("<th>Wallet</th>") < table.indexOf("<th>Telegram ID</th>"));
    assert.ok(table.indexOf("<th>Telegram ID</th>") < table.indexOf("<th>Transaction</th>"));
    assert.doesNotMatch(table, /feedTransportLabel|row\.selectedRoute/);
    assert.match(table, /executionFeed\(row\.inboundSource\)/);
    assert.match(table, /sendLaneIdentity\(row\.firstAckLane\)/);
    assert.match(table, /title=\{lane\.raw \|\| undefined\}/);
    assert.match(table, /placementClass\(row\)/);
    assert.match(table, /transactionDistance\(row\)/);
    assert.match(table, /row\.sameSlotTxDelta/);
    assert.match(table, /crossSlotPositionSummary\?\.crossSlotTxDelta/);
    assert.doesNotMatch(table, /return landing\.secondary/);
    assert.match(table, /className=\{styles\.txDistance\}/);
    assert.match(table, /row\.telegramSubscriberId/);
    assert.match(table, /label="Telegram subscriber ID"/);
    assert.match(table, /leaderSummary\(row\)/);
    assert.match(table, /className=\{styles\.ackCell\}[\s\S]*lane\.label[\s\S]*formatMs\(row\.observedToSignatureReturnedMs\)/);
    assert.match(table, /useUserTimeZone\(\)/);
    assert.match(table, /Time · \{timeZoneLabel\}/);
    assert.match(styles, /\.sideBuy\s*\{\s*color:\s*var\(--green\);\s*\}/);
    assert.match(styles, /\.sideSell\s*\{\s*color:\s*var\(--red\);\s*\}/);
  });

  test("timestamps use an explicit user timezone and 12-hour clock", () => {
    const timestamp = Date.UTC(2026, 6, 31, 12, 5, 9);
    assert.equal(formatUserTime(timestamp, "America/Los_Angeles", "en-US"), "5:05:09 AM");
    assert.equal(formatUserDate(timestamp, "America/Los_Angeles", "en-US"), "Jul 31");
    assert.match(formatUserDateTime(timestamp, "America/Los_Angeles", "en-US"), /5:05:09 AM/);
    assert.equal(userTimeZoneLabel("America/Los_Angeles", timestamp, "en-US"), "PDT");
  });

  test("feed identity accepts only typed inbound winners and keeps transport separate", () => {
    assert.deepEqual(feedIdentity("vortex-fra"), { key: "vortex-fra", label: "Vortex FRA" });
    assert.deepEqual(feedIdentity("jito-primary"), { key: "jito-primary", label: "Jito primary" });
    assert.deepEqual(feedIdentity("doublezero-leader"), { key: "doublezero-leader", label: "DoubleZero leader" });
    assert.deepEqual(feedIdentity("doublezero-retransmit-eu"), { key: "doublezero-retransmit-eu", label: "DoubleZero retransmit EU" });
    assert.deepEqual(feedIdentity("shredstream"), { key: "unknown", label: "Unknown" });
    assert.deepEqual(feedIdentity("stable-ingress-active"), { key: "unknown", label: "Unknown" });
    assert.deepEqual(feedIdentity("confirmed_rpc"), { key: "unknown", label: "Unknown" });
    assert.deepEqual(feedIdentity("JITO-PRIMARY"), { key: "unknown", label: "Unknown" });
    assert.deepEqual(feedIdentity(" jito-primary "), { key: "unknown", label: "Unknown" });
    assert.deepEqual(executionFeed(null, "shredstream"), { key: "unknown", label: "Unknown" });
    assert.equal(feedTransportLabel("jito-primary", null), "ShredStream");
    assert.equal(feedTransportLabel("stable-ingress-active", "shredstream"), "ShredStream");
  });

  test("typed inbound normalization follows durable path priority and fail-closed legacy rules", () => {
    const inbound = (selectedSource, overrides = {}) => ({
      schemaVersion: 1,
      selectedSource,
      contributors: [selectedSource],
      selectionGeneration: 1,
      ...overrides
    });
    const unknown = { inboundSource: null, inboundContributors: [], inboundSelectionGeneration: null };

    for (const selectedSource of ["jito-primary", "doublezero-leader", "doublezero-retransmit-eu", "vortex-fra"]) {
      assert.deepEqual(normalizeInboundFeedAttribution({ executionTelemetry: { inbound: inbound(selectedSource) } }, null, "shredstream"), {
        inboundSource: selectedSource,
        inboundContributors: [selectedSource],
        inboundSelectionGeneration: 1
      });
    }

    assert.deepEqual(normalizeInboundFeedAttribution({
      executionTelemetry: { inbound: inbound("vortex-fra", { contributors: ["vortex-fra", "jito-primary"], selectionGeneration: 17 }) },
      rustTransactionConfirmation: { executionTelemetry: { inbound: inbound("jito-primary", { selectionGeneration: 16 }) } }
    }, { executionTelemetry: { inbound: inbound("doublezero-leader") } }, "jito-primary"), {
      inboundSource: "vortex-fra",
      inboundContributors: ["vortex-fra", "jito-primary"],
      inboundSelectionGeneration: 17
    });

    assert.equal(normalizeInboundFeedAttribution({ rustTransactionConfirmation: { executionTelemetry: { inbound: inbound("doublezero-retransmit-eu") } } }, null, null).inboundSource, "doublezero-retransmit-eu");
    assert.equal(normalizeInboundFeedAttribution(null, { executionTelemetry: { inbound: inbound("doublezero-leader") } }, null).inboundSource, "doublezero-leader");
    assert.equal(normalizeInboundFeedAttribution(null, null, "jito-primary").inboundSource, "jito-primary");
    assert.equal(normalizeInboundFeedAttribution(null, null, "shredstream").inboundSource, null);
    assert.equal(normalizeInboundFeedAttribution(null, null, "stable-ingress-active").inboundSource, null);
    assert.equal(normalizeInboundFeedAttribution(null, null, "JITO-PRIMARY").inboundSource, null);
    assert.equal(normalizeInboundFeedAttribution(null, null, " jito-primary ").inboundSource, null);
    assert.equal(normalizeInboundFeedAttribution({ executionTelemetry: { inbound: inbound("jito-primary", { selectionGeneration: Number.MAX_SAFE_INTEGER }) } }, null, null).inboundSelectionGeneration, Number.MAX_SAFE_INTEGER);

    const malformedInbound = [
      inbound("jito-primary", { schemaVersion: 2 }),
      { selectedSource: "jito-primary", contributors: ["jito-primary"], selectionGeneration: 1 },
      inbound("JITO-PRIMARY"),
      inbound(" jito-primary "),
      inbound("jito-primary", { contributors: [] }),
      inbound("jito-primary", { contributors: undefined }),
      inbound("jito-primary", { contributors: "jito-primary" }),
      inbound("jito-primary", { contributors: ["raw-union-eu", "jito-primary"] }),
      inbound("jito-primary", { contributors: ["vortex-fra"] }),
      inbound("jito-primary", { selectionGeneration: 0 }),
      inbound("jito-primary", { selectionGeneration: -1 }),
      inbound("jito-primary", { selectionGeneration: 1.5 }),
      inbound("jito-primary", { selectionGeneration: undefined }),
      inbound("jito-primary", { selectionGeneration: Number.MAX_SAFE_INTEGER + 1 })
    ];
    for (const value of malformedInbound) {
      assert.deepEqual(normalizeInboundFeedAttribution({
        executionTelemetry: { inbound: value },
        rustTransactionConfirmation: { executionTelemetry: { inbound: inbound("vortex-fra") } }
      }, { executionTelemetry: { inbound: inbound("doublezero-leader") } }, "jito-primary"), unknown);
    }
    assert.deepEqual(normalizeInboundFeedAttribution({
      executionTelemetry: { inbound: "shredstream" },
      rustTransactionConfirmation: { executionTelemetry: { inbound: inbound("jito-primary") } }
    }, null, "jito-primary"), unknown);

    const jitoFilter = dashboardInboundSourcePredicate("jito-primary");
    assert.match(jitoFilter, /raw_execution->executionTelemetry->inbound->schemaVersion\.eq\.1/);
    assert.match(jitoFilter, /raw_execution->executionTelemetry->inbound->>selectedSource\.eq\.jito-primary/);
    assert.match(jitoFilter, /raw_execution->executionTelemetry->inbound->contributors\.cs\.\["jito-primary"\]/);
    assert.match(jitoFilter, /raw_execution->executionTelemetry->inbound->contributors\.cd\.\["jito-primary","doublezero-leader","doublezero-retransmit-eu","vortex-fra"\]/);
    assert.match(jitoFilter, /raw_execution->executionTelemetry->inbound->selectionGeneration\.gt\.0/);
    assert.match(jitoFilter, /raw_execution->executionTelemetry->inbound->selectionGeneration\.lt\.9007199254740992/);
    assert.match(jitoFilter, /raw_execution->executionTelemetry->inbound->>selectionGeneration\.not\.like\.\*\.\*/);
    assert.match(jitoFilter, /raw_execution->rustTransactionConfirmation->executionTelemetry->inbound->schemaVersion\.eq\.1/);
    assert.match(jitoFilter, /chain_report->executionTelemetry->inbound->schemaVersion\.eq\.1/);
    assert.equal((jitoFilter.match(/->schemaVersion\.eq\.1/g) || []).length, 3);
    assert.equal((jitoFilter.match(/->contributors\.cs\.\["jito-primary"\]/g) || []).length, 3);
    assert.equal((jitoFilter.match(/->contributors\.cd\./g) || []).length, 3);
    assert.equal((jitoFilter.match(/->selectionGeneration\.gt\.0/g) || []).length, 3);
    assert.equal((jitoFilter.match(/->selectionGeneration\.lt\.9007199254740992/g) || []).length, 3);
    assert.equal((jitoFilter.match(/->>selectionGeneration\.not\.like\.\*\.\*/g) || []).length, 3);
    assert.match(jitoFilter, /source\.eq\.jito-primary/);
    assert.doesNotMatch(jitoFilter, /selectedSource\.ilike|source\.ilike|provider|shredstream/);
    assert.match(dashboardInboundSourcePredicate("unknown"), /source\.not\.in\.\(jito-primary,doublezero-leader,doublezero-retransmit-eu,vortex-fra\)/);
    assert.equal(dashboardInboundSourcePredicate("shredstream"), "source.eq.__no_typed_feed_match__");
  });

  test("outbound ACK lanes use stable operator labels without losing raw attribution", () => {
    assert.deepEqual(sendLaneIdentity("helius-sender-fast:sender.helius-rpc.com"), {
      key: "helius-sender",
      label: "Helius Sender",
      raw: "helius-sender-fast:sender.helius-rpc.com"
    });
    assert.equal(sendLaneIdentity("nozomi-fra-1:nozomi.example").label, "Nozomi");
    assert.equal(sendLaneIdentity("jito-1:frankfurt.mainnet.block-engine.jito.wtf").label, "Jito");
    assert.equal(sendLaneIdentity("rpc-primary:mainnet.helius-rpc.com").label, "RPC");
    assert.equal(sendLaneIdentity(null).label, "Lane n/a");
  });

  test("feed leaderboard ranks visible buy winners with stable shares and colors", () => {
    assert.equal(isLandedBuy({ observedAction: "buy", outcome: "landed" }), true);
    assert.equal(isLandedBuy({ observedAction: "buy", outcome: "skipped" }), false);
    assert.equal(isLandedBuy({ observedAction: "buy", outcome: "failed_on_chain" }), false);
    assert.equal(isLandedBuy({ observedAction: "sell", outcome: "landed" }), false);
    assert.deepEqual(feedLeaderboard(["jito-primary", "vortex-fra", "doublezero-leader", "doublezero-retransmit-eu"]), [
      { key: "jito-primary", label: "Jito primary", wins: 1, share: 25 },
      { key: "doublezero-leader", label: "DoubleZero leader", wins: 1, share: 25 },
      { key: "doublezero-retransmit-eu", label: "DoubleZero retransmit EU", wins: 1, share: 25 },
      { key: "vortex-fra", label: "Vortex FRA", wins: 1, share: 25 }
    ]);
    assert.deepEqual(feedLeaderboard(["jito-primary", "shredstream", "vortex-fra"]), [
      { key: "jito-primary", label: "Jito primary", wins: 1, share: (1 / 3) * 100 },
      { key: "vortex-fra", label: "Vortex FRA", wins: 1, share: (1 / 3) * 100 },
      { key: "unknown", label: "Unknown", wins: 1, share: (1 / 3) * 100 }
    ]);
    assert.deepEqual([...executionEvidenceCounts(["jito-primary", "vortex-fra", "jito-backup"])], [
      ["jito-primary", 1],
      ["vortex-fra", 1],
      ["unknown", 1]
    ]);
    const overview = readFileSync(new URL("../../components/dashboard/overview-dashboard.tsx", import.meta.url), "utf8");
    const leaderboard = readFileSync(new URL("../../components/dashboard/feed-leaderboard.tsx", import.meta.url), "utf8");
    const styles = readFileSync(new URL("../../components/dashboard/dashboard-shared.module.css", import.meta.url), "utf8");
    assert.match(overview, /<FeedLeaderboard rows=\{data\?\.executions \?\? \[\]\} \/>/);
    assert.match(leaderboard, /rows\.filter\(isLandedBuy\)/);
    assert.match(leaderboard, /Landed buy race/);
    assert.match(leaderboard, /landed buy/);
    assert.match(leaderboard, /execution evidence only/);
    assert.match(leaderboard, /\["jito-primary", "doublezero-leader", "doublezero-retransmit-eu", "vortex-fra"\]/);
    assert.match(leaderboard, /Feed leaderboard/);
    assert.doesNotMatch(readFileSync(new URL("../../components/dashboard/execution-table.tsx", import.meta.url), "utf8"), /feedTransportLabel/);
    assert.match(styles, /\.feedLeaderboard\s*\{[^}]*block-size:\s*auto;/s);
    assert.match(styles, /\.feedStandings\s*\{[^}]*max-block-size:\s*132px;/s);
  });
});
