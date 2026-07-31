import assert from "node:assert/strict";
import { describe, test } from "node:test";
import {
  decodeExecutionCursor,
  encodeExecutionCursor,
  executionOutcomeForRow,
  landingComparisonForRow,
  parseExecutionFilters,
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

  test("outcome mapping handles sell rows as landed", () => {
    assert.equal(executionOutcomeForRow({ observedAction: "sell", decision: "sent" }), "landed");
    assert.equal(executionOutcomeForRow({ observedAction: "buy", decision: "skip" }), "skipped");
    assert.equal(executionOutcomeForRow({ observedAction: "buy", decision: "sent", buyStatus: "buyLanded", buyChainError: null }), "landed");
  });

  test("landing comparison and summary counts", () => {
    const rows = [
      toDashboardExecution({ observedAtMs: 1, id: 1, observedAction: "buy", sendSignature: "abc", observedWallet: "obs1", copyWallet: null, buyStatus: "buySubmitted" }),
      toDashboardExecution({ observedAtMs: 2, id: 2, observedAction: "sell", observedWallet: "obs2", copyWallet: null, buyStatus: null, targetSlot: 20, copySlot: 20 }),
      toDashboardExecution({ observedAtMs: 3, id: 3, observedAction: "buy", observedWallet: "obs3", copyWallet: null, buyStatus: "buyLanded" }),
    ];
    const summary = summarizeExecutions(rows);
    assert.equal(summary.outcome.landed, 2);
    assert.equal(summary.outcome.ack_not_landed, 1);
    assert.equal(landingComparisonForRow({ targetSlot: 11, copySlot: 12 }), "cross_slot");
    assert.equal(landingComparisonForRow({}), "no_target");
  });
});
