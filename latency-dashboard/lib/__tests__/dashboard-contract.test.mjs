import assert from "node:assert/strict";
import { describe, test } from "node:test";
import {
  DashboardFilterError,
  decodeExecutionCursor,
  dashboardOutcomePredicate,
  encodeExecutionCursor,
  executionOutcomeForRow,
  executionMatchesWallet,
  landingComparisonForRow,
  parseExecutionFilters,
  parseSourceFilters,
  pageExecutionRows,
  isExecutionBeforeCursor,
  isObservedAtWithinRange,
  sanitizeWallet,
  summarizeExecutions,
  toDashboardExecution
} from "../dashboard-contract.mjs";

describe("dashboard contract", () => {
  test("parseExecutionFilters applies default and max bounds", () => {
    const now = Date.parse("2026-07-31T12:00:00.000Z");
    const defaults = parseExecutionFilters(new URLSearchParams(), now);
    assert.equal(defaults.limit, 50);
    assert.equal(defaults.cursor, null);
    assert.equal(defaults.from, "2026-07-30T12:00:00.000Z");
    assert.equal(defaults.to, "2026-07-31T12:00:00.000Z");

    const empty = parseExecutionFilters(new URLSearchParams("limit="));
    assert.equal(empty.limit, 50);

    const capped = parseExecutionFilters(new URLSearchParams("limit=500"));
    assert.equal(capped.limit, 100);

    assert.throws(() => parseExecutionFilters(new URLSearchParams("limit=0")), DashboardFilterError);
  });

  test("cursor encoding is reversible", () => {
    const cursor = { observedAtMs: 1710000000000, id: 7 };
    const encoded = encodeExecutionCursor(cursor);
    const decoded = decodeExecutionCursor(encoded);
    assert.deepEqual(decoded, cursor);
  });

  test("authenticated execution DTOs preserve full wallets and omit raw custody material", () => {
    assert.equal(sanitizeWallet("abcdefghijklmnopqrstuvwxyz"), "abcdefghijklmnopqrstuvwxyz");
    assert.equal(sanitizeWallet("short"), "short");
    assert.equal(sanitizeWallet(""), null);
    assert.equal(sanitizeWallet("  abcdefghijklmnopqrstuvwxyz  "), "abcdefghijklmnopqrstuvwxyz");
    const dto = toDashboardExecution({
      observedWallet: "ObservedWalletFullAddress",
      copyWallet: "CopyWalletFullAddress",
      rawExecution: { privateKey: "never-return" },
      chainReport: { seed: "never-return" },
      privateKey: "never-return",
      observedAction: "buy"
    });
    assert.equal(dto.observedWallet, "ObservedWalletFullAddress");
    assert.equal(dto.copyWallet, "CopyWalletFullAddress");
    assert.equal("rawExecution" in dto, false);
    assert.equal("chainReport" in dto, false);
    assert.equal("privateKey" in dto, false);
    assert.equal(JSON.stringify(dto).includes("never-return"), false);
  });

  test("fixed from/to are inclusive, primary, and invalid dates fail with 400", () => {
    const filters = parseExecutionFilters(new URLSearchParams(
      "from=2026-07-01T00%3A00%3A00Z&to=1782950400000&since=1h&side=sell&action=buy"
    ));
    assert.equal(filters.fromObservedAtMs, Date.parse("2026-07-01T00:00:00Z"));
    assert.equal(filters.toObservedAtMs, 1782950400000);
    assert.equal(filters.side, "sell");
    assert.equal(isObservedAtWithinRange(filters.fromObservedAtMs, filters), true);
    assert.equal(isObservedAtWithinRange(filters.toObservedAtMs, filters), true);
    assert.equal(isObservedAtWithinRange(filters.toObservedAtMs + 1, filters), false);
    for (const query of ["from=yesterday", "to=2026-07-31", "from=2026-02-30T00%3A00%3A00Z", "from=2026-08-01T00%3A00%3A00Z&to=2026-07-01T00%3A00%3A00Z"]) {
      assert.throws(
        () => parseExecutionFilters(new URLSearchParams(query)),
        (error) => error instanceof DashboardFilterError && error.status === 400
      );
    }
  });

  test("wallet matching covers observed and copy wallets exactly", () => {
    const row = { observedWallet: "watched123", copyWallet: "copy456" };
    assert.equal(executionMatchesWallet(row, "watched123"), true);
    assert.equal(executionMatchesWallet(row, "copy456"), true);
    assert.equal(executionMatchesWallet(row, "watched"), false);
    const filters = parseExecutionFilters(new URLSearchParams("wallet=copy456"));
    assert.equal(filters.wallet, "copy456");
  });

  test("outcome mapping requires confirmed landing evidence", () => {
    assert.equal(executionOutcomeForRow({ observedAction: "sell", decision: "sent" }), "ack_not_landed");
    assert.equal(executionOutcomeForRow({ observedAction: "sell" }), "unknown");
    assert.equal(executionOutcomeForRow({ observedAction: "sell", decision: "sent", sendSignature: "sell", copySlot: 21, buyStatus: "buyLanded" }), "landed");
    assert.equal(executionOutcomeForRow({ observedAction: "buy", decision: "skip" }), "skipped");
    assert.equal(executionOutcomeForRow({ observedAction: "buy", decision: "error" }), "send_failed");
    assert.equal(executionOutcomeForRow({ observedAction: "buy", decision: "sent", buyStatus: "buyFailedOnChain", buyChainError: {} }), "failed_on_chain");
    assert.equal(executionOutcomeForRow({ observedAction: "buy", decision: "sent", sendSignature: "buy", copySlot: 22, buyStatus: "buyLanded", buyChainError: null }), "landed");
  });

  test("landing comparison and summary counts", () => {
    const rows = [
      toDashboardExecution({ observedAtMs: 1, id: 1, observedAction: "buy", sendSignature: "abc", observedWallet: "obs1", copyWallet: null, buyStatus: "buySubmitted" }),
      toDashboardExecution({ observedAtMs: 2, id: 2, observedAction: "sell", observedWallet: "obs2", copyWallet: null, sendSignature: "sell", buyStatus: "buyLanded", targetSlot: 20, copySlot: 20 }),
      toDashboardExecution({ observedAtMs: 3, id: 3, observedAction: "buy", observedWallet: "obs3", copyWallet: null, sendSignature: "buy", copySlot: 21, buyStatus: "buyLanded" }),
    ];
    const summary = summarizeExecutions(rows);
    assert.equal(summary.outcome.landed, 2);
    assert.equal(summary.outcome.ack_not_landed, 1);
    assert.equal(landingComparisonForRow({ targetSlot: 11, copySlot: 12 }), "cross_slot");
    assert.equal(landingComparisonForRow({}), "no_target");
  });

  test("outcome predicates are database-side and source filters omit execution-only fields", () => {
    assert.match(dashboardOutcomePredicate("landed"), /copy_slot\.not\.is\.null/);
    assert.match(dashboardOutcomePredicate("failed_on_chain"), /buyFailedOnChain/);
    const filters = parseExecutionFilters(new URLSearchParams("outcome=ack_not_landed"));
    assert.equal(filters.outcome, "ack_not_landed");
    const sourceFilters = parseSourceFilters(new URLSearchParams("source=shred&wallet=abc&side=buy"));
    assert.equal(sourceFilters.source, "shred");
    assert.equal(sourceFilters.wallet, "abc");
    assert.equal(sourceFilters.side, "buy");
    assert.equal("copyWallet" in sourceFilters, false);
    assert.equal("outcome" in sourceFilters, false);

    const sideFilters = parseExecutionFilters(new URLSearchParams("side=buy&outcome=landed"));
    assert.equal(sideFilters.side, "buy");
    assert.equal(sideFilters.outcome, "landed");
    assert.equal(parseSourceFilters(new URLSearchParams("side=sell")).side, "sell");
  });

  test("sentinel pagination preserves tied timestamp keysets across pages", () => {
    const rows = [
      { observedAtMs: 100, id: 3 },
      { observedAtMs: 100, id: 2 },
      { observedAtMs: 100, id: 1 },
      { observedAtMs: 99, id: 9 }
    ];
    const first = pageExecutionRows(rows.slice(0, 3), 2);
    assert.deepEqual(first.items.map((row) => row.id), [3, 2]);
    assert.equal(first.hasMore, true);
    const cursor = first.items.at(-1);
    const secondCandidates = rows.filter((row) => isExecutionBeforeCursor(row, cursor));
    const second = pageExecutionRows(secondCandidates, 2);
    assert.deepEqual(second.items.map((row) => row.id), [1, 9]);
    assert.equal(second.hasMore, false);
  });

  test("database filters precede limit and overview reuses the identical filter query", async () => {
    const source = await import("node:fs/promises").then((fs) => fs.readFile(new URL("../local-executions.ts", import.meta.url), "utf8"));
    const filterBuilder = source.indexOf("function filteredDashboardExecutionQuery");
    const listQuery = source.indexOf("export async function listDashboardExecutions");
    const limit = source.indexOf(".limit(filters.limit + 1)", listQuery);
    assert.equal(filterBuilder >= 0 && filterBuilder < listQuery && listQuery < limit, true);
    assert.match(source, /\.gte\("observed_at_ms", filters\.fromObservedAtMs\)\s*\.lte\("observed_at_ms", filters\.toObservedAtMs\)/);
    assert.match(source, /observed_wallet\.eq\.\$\{filters\.wallet\},copy_wallet\.eq\.\$\{filters\.wallet\}/);
    assert.match(source, /exactDashboardExecutionCount[\s\S]*filteredDashboardExecutionQuery\(filters, "id", true\)/);
  });
});
