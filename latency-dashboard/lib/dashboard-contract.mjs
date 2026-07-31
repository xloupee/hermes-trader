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
const SKIPPED_DECISIONS = new Set(["skip", "skipped", "simulated", "wouldCopy", "wouldBuy"]);
const SEND_FAILED_DECISIONS = new Set(["error", "send_failed"]);

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
  const limitRaw = optionalString(searchParams.get("limit"));
  const requestedLimit = limitRaw === null ? null : Number(limitRaw);
  const finalLimit = requestedLimit !== null && isFiniteNumber(requestedLimit)
    ? Math.max(1, Math.min(Math.trunc(requestedLimit), MAX_LIMIT))
    : DEFAULT_LIMIT;

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
    action: optionalString(searchParams.get("side")) ?? optionalString(searchParams.get("action")),
    outcome: optionalEnum(searchParams.get("outcome"), EXECUTION_OUTCOMES)
  };
}

function optionalEnum(value, allowed) {
  const candidate = optionalString(value);
  return candidate && allowed.includes(candidate) ? candidate : null;
}

export function parseSourceFilters(searchParams) {
  const since = coerceDate(searchParams.get("since"));
  return {
    since,
    sinceObservedAtMs: new Date(since).getTime(),
    provider: optionalString(searchParams.get("provider")),
    source: optionalString(searchParams.get("source")),
    observedWallet: optionalString(searchParams.get("observedWallet")),
    mint: optionalString(searchParams.get("mint")),
    route: optionalString(searchParams.get("route")),
    action: optionalString(searchParams.get("side")) ?? optionalString(searchParams.get("action"))
  };
}

export function unsupportedSourceFilters(searchParams) {
  return ["copyWallet", "cursor", "limit", "outcome"].filter((name) => optionalString(searchParams.get(name)) !== null);
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

  if (SKIPPED_DECISIONS.has(row.decision) || row.dryRun === true || row.sendEnabled === false && row.decision === "simulated") {
    return "skipped";
  }

  if (SEND_FAILED_DECISIONS.has(row.decision)) {
    return "send_failed";
  }

  if (row.buyChainError || row.buyStatus === "buyFailedOnChain") {
    return "failed_on_chain";
  }

  const confirmedCopySlot = row.copySlot ?? row.blockPositionDiagnostics?.copySlot ?? null;
  if (row.buyStatus === "buyLanded" && row.sendSignature && confirmedCopySlot !== null) {
    return "landed";
  }

  if (row.buyStatus === "buySubmitted" || row.sendSignature || row.sent || row.decision === "sent") {
    return "ack_not_landed";
  }

  return "unknown";
}

export function landingComparisonForRow(row) {
  const diagnostics = row?.blockPositionDiagnostics;
  if (diagnostics && diagnostics.unavailableReason) {
    return "unavailable";
  }

  if (row?.observedAction === "sell" && row?.targetSlot == null && diagnostics?.status !== "found") {
    return "no_target";
  }

  const targetSlot = row?.targetSlot ?? diagnostics?.targetSlot ?? null;
  const copySlot = diagnostics?.copySlot ?? row?.copySlot ?? null;

  if (targetSlot == null && copySlot == null) {
    return "no_target";
  }

  if (targetSlot != null && copySlot != null) {
    return targetSlot === copySlot ? "same_slot" : "cross_slot";
  }

  return "no_target";
}

const SKIPPED_DB = "or(decision.in.(skip,skipped,simulated,wouldCopy,wouldBuy),dry_run.eq.true)";
const NOT_SKIPPED_DB = "and(decision.not.in.(skip,skipped,simulated,wouldCopy,wouldBuy),dry_run.eq.false)";
const SEND_FAILED_DB = "decision.in.(error,send_failed)";
const NOT_SEND_FAILED_DB = "decision.not.in.(error,send_failed)";
const CHAIN_FAILED_DB = "or(chain_report->>buyStatus.eq.buyFailedOnChain,chain_report->>status.eq.failedOnChain,chain_report->err.not.is.null)";
const NOT_CHAIN_FAILED_DB = "and(or(chain_report->>buyStatus.is.null,chain_report->>buyStatus.neq.buyFailedOnChain),or(chain_report->>status.is.null,chain_report->>status.neq.failedOnChain),chain_report->err.is.null)";
const LANDED_DB = "and(send_signature.not.is.null,copy_slot.not.is.null)";
const NOT_LANDED_DB = "or(send_signature.is.null,copy_slot.is.null)";
const ACK_DB = "or(send_signature.not.is.null,sent.eq.true,decision.eq.sent)";
const NOT_ACK_DB = "and(send_signature.is.null,sent.eq.false,decision.neq.sent)";

export function dashboardOutcomePredicate(outcome) {
  switch (outcome) {
    case "skipped":
      return SKIPPED_DB;
    case "send_failed":
      return `and(${NOT_SKIPPED_DB},${SEND_FAILED_DB})`;
    case "failed_on_chain":
      return `and(${NOT_SKIPPED_DB},${NOT_SEND_FAILED_DB},${CHAIN_FAILED_DB})`;
    case "landed":
      return `and(${NOT_SKIPPED_DB},${NOT_SEND_FAILED_DB},${NOT_CHAIN_FAILED_DB},${LANDED_DB})`;
    case "ack_not_landed":
      return `and(${NOT_SKIPPED_DB},${NOT_SEND_FAILED_DB},${NOT_CHAIN_FAILED_DB},${NOT_LANDED_DB},${ACK_DB})`;
    case "unknown":
      return `and(${NOT_SKIPPED_DB},${NOT_SEND_FAILED_DB},${NOT_CHAIN_FAILED_DB},${NOT_LANDED_DB},${NOT_ACK_DB})`;
    default:
      return null;
  }
}

export function pageExecutionRows(rows, limit) {
  return {
    items: rows.slice(0, limit),
    hasMore: rows.length > limit
  };
}

export function isExecutionBeforeCursor(row, cursor) {
  return row.observedAtMs < cursor.observedAtMs || row.observedAtMs === cursor.observedAtMs && row.id < cursor.id;
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
