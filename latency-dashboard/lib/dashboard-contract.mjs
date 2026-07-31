const EXECUTION_OUTCOMES = ["landed", "failed_on_chain", "ack_not_landed", "send_failed", "skipped", "unknown"];
const LANDING_COMPARISONS = ["same_slot", "cross_slot", "no_target", "unavailable"];

/** @typedef {"landed" | "failed_on_chain" | "ack_not_landed" | "send_failed" | "skipped" | "unknown"} ExecutionOutcome */
/** @typedef {"same_slot" | "cross_slot" | "no_target" | "unavailable"} LandingComparison */
/** @typedef {{ observedAtMs:number, id:number }} ExecutionCursor */

function hasOwn(obj, key) {
  return Object.prototype.hasOwnProperty.call(obj, key);
}

const DEFAULT_LIMIT = 50;
const MAX_LIMIT = 100;

function isFiniteNumber(value) {
  return Number.isFinite(value);
}

function optionalString(value) {
  if (typeof value !== "string") {
    return null;
  }
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function coerceDate(value) {
  const trimmed = optionalString(value) ?? "24h";
  const match = trimmed.match(/^(\d+)(h|d|m)$/);
  if (match) {
    const amount = Number(match[1]);
    const unit = match[2];
    const ms = unit === "h" ? amount * 60 * 60 * 1000 : unit === "d" ? amount * 24 * 60 * 60 * 1000 : amount * 60 * 1000;
    return new Date(Date.now() - ms).toISOString();
  }

  const date = new Date(trimmed);
  return Number.isNaN(date.getTime()) ? new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString() : date.toISOString();
}

export function sanitizeWallet(address) {
  if (typeof address !== "string") {
    return null;
  }
  const trimmed = address.trim();
  if (!trimmed) {
    return null;
  }
  if (trimmed.length <= 8) {
    return `${"*".repeat(trimmed.length)}`;
  }
  return `${trimmed.slice(0, 6)}...${trimmed.slice(-4)}`;
}

export function parseExecutionFilters(searchParams) {
  const cursorRaw = optionalString(searchParams.get("cursor"));
  const cursor = decodeExecutionCursor(cursorRaw);
  const limit = optionalString(searchParams.get("limit"));
  const requestedLimit = Number(limit);
  const finalLimit = isFiniteNumber(requestedLimit) ? Math.max(1, Math.min(requestedLimit, MAX_LIMIT)) : DEFAULT_LIMIT;

  return {
    since: coerceDate(searchParams.get("since")),
    sinceObservedAtMs: new Date(coerceDate(searchParams.get("since"))).getTime(),
    limit: finalLimit,
    cursor,
    provider: optionalString(searchParams.get("provider")),
    source: optionalString(searchParams.get("source")),
    observedWallet: optionalString(searchParams.get("observedWallet")),
    copyWallet: optionalString(searchParams.get("copyWallet")),
    mint: optionalString(searchParams.get("mint")),
    route: optionalString(searchParams.get("route")),
    action: optionalString(searchParams.get("action"))
  };
}

export function encodeExecutionCursor({ observedAtMs, id }) {
  if (!isFiniteNumber(observedAtMs) || !isFiniteNumber(id)) {
    return null;
  }
  return Buffer.from(JSON.stringify({ observedAtMs, id }), "utf8").toString("base64url");
}

export function decodeExecutionCursor(cursor) {
  if (!cursor) {
    return null;
  }
  try {
    const value = Buffer.from(cursor, "base64url").toString("utf8");
    const parsed = JSON.parse(value);
    const observedAtMs = Number(parsed?.observedAtMs);
    const id = Number(parsed?.id);
    if (!isFiniteNumber(observedAtMs) || !isFiniteNumber(id)) {
      return null;
    }
    return { observedAtMs, id };
  } catch {
    return null;
  }
}

export function executionOutcomeForRow(row) {
  if (!row) {
    return "unknown";
  }

  if (row.observedAction === "sell") {
    return row.decision === "skip" || row.decision === "skipped" ? "skipped" : "landed";
  }

  if (row.decision === "skip" || row.decision === "skipped") {
    return "skipped";
  }

  if (row.buyChainError || row.autoSellStatus === "autoSellFailedOnChain") {
    return "failed_on_chain";
  }

  if (row.buyStatus === "buyLanded") {
    return "landed";
  }

  if (row.buyStatus === "buyFailedOnChain") {
    return "failed_on_chain";
  }

  if (row.buyStatus === "buySubmitted" || row.sendSignature || row.sent) {
    return "ack_not_landed";
  }

  if (row.decision === "error" || row.autoSellStatus === "autoSellSubmitted" || row.decision === "send_failed") {
    return "send_failed";
  }

  return "unknown";
}

export function landingComparisonForRow(row) {
  const diagnostics = row?.blockPositionDiagnostics;
  if (diagnostics && diagnostics.unavailableReason) {
    return "unavailable";
  }

  const targetSlot = diagnostics?.targetSlot ?? row?.targetSlot ?? null;
  const copySlot = diagnostics?.copySlot ?? row?.copySlot ?? null;

  if (targetSlot == null && copySlot == null) {
    return "no_target";
  }

  if (targetSlot != null && copySlot != null) {
    return targetSlot === copySlot ? "same_slot" : "cross_slot";
  }

  return "no_target";
}

export function toDashboardExecution(row) {
  const outcome = executionOutcomeForRow(row);
  const landingComparison = landingComparisonForRow(row);
  if (!row || !outcome || !landingComparison) {
    return {
      outcome: "unknown",
      landingComparison: "unavailable",
      observedWallet: sanitizeWallet(row?.observedWallet),
      copyWallet: sanitizeWallet(row?.copyWallet)
    };
  }

  return {
    ...row,
    observedWallet: sanitizeWallet(row.observedWallet),
    copyWallet: sanitizeWallet(row.copyWallet),
    outcome,
    landingComparison
  };
}

export function summarizeExecutions(rows) {
  const outcome = Object.fromEntries(EXECUTION_OUTCOMES.map((item) => [item, 0]));
  const landingComparison = Object.fromEntries(LANDING_COMPARISONS.map((item) => [item, 0]));
  let totalGrossCopySpendSol = 0;
  let totalExtraSpendSol = 0;
  let landedCount = 0;

  for (const row of rows ?? []) {
    const rowOutcome = executionOutcomeForRow(row);
    const rowComparison = landingComparisonForRow(row);
    if (hasOwn(outcome, rowOutcome)) {
      outcome[rowOutcome] += 1;
    }
    if (hasOwn(landingComparison, rowComparison)) {
      landingComparison[rowComparison] += 1;
    }
    if (rowOutcome === "landed") {
      landedCount += 1;
    }
    if (typeof row?.grossCopySpendSol === "number" && Number.isFinite(row.grossCopySpendSol)) {
      totalGrossCopySpendSol += row.grossCopySpendSol;
    }
    if (typeof row?.extraSpendBeyondObservedAndNetworkFeeSol === "number" && Number.isFinite(row.extraSpendBeyondObservedAndNetworkFeeSol)) {
      totalExtraSpendSol += row.extraSpendBeyondObservedAndNetworkFeeSol;
    }
  }

  return {
    total: rows.length,
    landed: landedCount,
    outcome,
    landingComparison,
    totalGrossCopySpendSol,
    totalExtraSpendSol
  };
}

export const dashboardContractSchema = {
  outcomes: EXECUTION_OUTCOMES,
  landingComparisons: LANDING_COMPARISONS,
  defaultLimit: DEFAULT_LIMIT,
  maxLimit: MAX_LIMIT
};
