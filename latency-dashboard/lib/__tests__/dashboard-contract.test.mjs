import assert from "node:assert/strict";
import { describe, test } from "node:test";
import {
  decodeExecutionCursor,
  dashboardOutcomePredicate,
  encodeExecutionCursor,
  executionOutcomeForRow,
  landingComparisonForRow,
  parseExecutionFilters,
  parseSourceFilters,
  pageExecutionRows,
  isExecutionBeforeCursor,
  sanitizeWallet,
  summarizeExecutions,
  toDashboardExecution
} from "../dashboard-contract.mjs";

describe("dashboard contract", () => {
  test("parseExecutionFilters applies default and max bounds", () => {
    const defaults = parseExecutionFilters(new URLSearchParams());
    assert.equal(defaults.limit, 50);
    assert.equal(defaults.cursor, null);
    assert.equal(defaults.sinceObservedAtMs > 0, true);

    const empty = parseExecutionFilters(new URLSearchParams("limit="));
    assert.equal(empty.limit, 50);

    const capped = parseExecutionFilters(new URLSearchParams("limit=500"));
    assert.equal(capped.limit, 100);

    const bounded = parseExecutionFilters(new URLSearchParams("limit=0"));
    assert.equal(bounded.limit, 1);
  });

  test("cursor encoding is reversible", () => {
    const cursor = { observedAtMs: 1710000000000, id: 7 };
    const encoded = encodeExecutionCursor(cursor);
    const decoded = decodeExecutionCursor(encoded);
    assert.deepEqual(decoded, cursor);
  });

  test("wallets are sanitized before return", () => {
    assert.equal(sanitizeWallet("abcdefghijklmnopqrstuvwxyz"), "abcdef...wxyz");
    assert.equal(sanitizeWallet("short"), "*****");
    assert.equal(sanitizeWallet(""), null);
    assert.equal(sanitizeWallet("  abcdefghijklmnopqrstuvwxyz  "), "abcdef...wxyz");
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
    const sourceFilters = parseSourceFilters(new URLSearchParams("source=shred&observedWallet=abc&copyWallet=ignored&outcome=landed"));
    assert.equal(sourceFilters.source, "shred");
    assert.equal(sourceFilters.observedWallet, "abc");
    assert.equal("copyWallet" in sourceFilters, false);
    assert.equal("outcome" in sourceFilters, false);

    const sideFilters = parseExecutionFilters(new URLSearchParams("side=buy&outcome=landed"));
    assert.equal(sideFilters.action, "buy");
    assert.equal(sideFilters.outcome, "landed");
    assert.equal(parseSourceFilters(new URLSearchParams("side=sell")).action, "sell");
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
});
