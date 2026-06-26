#!/usr/bin/env node
import { existsSync } from "node:fs";
import { pathToFileURL } from "node:url";
import {
  chainReport,
  dedupeRows,
  displayTxDelta,
  readJsonl
} from "./sync-local-copy-executions-to-supabase.mjs";

const DEFAULT_EXECUTIONS_PATH = "/tmp/jito-copy-executions-local-send.jsonl";
const LIVE_EXECUTIONS_PATH = "/var/log/jito-copy-executions-vps.jsonl";
const DEFAULT_LIMIT = 20;
const DEFAULT_COMPUTE_UNIT_LIMIT = 400_000;
const DEFAULT_POSITION_ENRICH_LIMIT = 100;
const DEFAULT_MIN_TX_DELTA_COVERAGE = 0.9;
const DEFAULT_MIN_POSITION_ELIGIBLE = 1;
const DEFAULT_MIN_CANARY_SENT = 10;
const DEFAULT_TARGET_TX_DELTA = 10;
const DEFAULT_PROMOTION_TX_DELTA_TARGET = 50;
const LAMPORTS_PER_SOL = 1_000_000_000;
const SECRET_PARAM_RE = /([?&](?:api[-_]?key|token|auth|signature|access_token|key|c)=)[^&\s]+/gi;

function argValue(name, fallback = null) {
  const prefix = `--${name}=`;
  const match = process.argv.find((arg) => arg.startsWith(prefix));
  return match ? match.slice(prefix.length) : fallback;
}

function hasFlag(name) {
  return process.argv.includes(`--${name}`);
}

function numberValue(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function positiveInteger(value, fallback) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? Math.floor(number) : fallback;
}

function nonnegativeInteger(value, fallback = null) {
  if (value === null || value === undefined || value === "") {
    return fallback;
  }
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? Math.floor(number) : fallback;
}

function nonnegativeNumber(value, fallback = null) {
  if (value === null || value === undefined || value === "") {
    return fallback;
  }
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? number : fallback;
}

function stringValue(value) {
  return typeof value === "string" && value.trim() ? value : null;
}

function defaultExecutionsPath() {
  return process.env.JITO_COPY_EXECUTIONS_PATH ||
    process.env.COPY_TRADE_RUST_ASYNC_PLATFORM_FEE_LOCAL_EXECUTIONS_PATH ||
    process.env.COPY_TRADE_RUST_EXECUTION_ALERTS_LOCAL_EXECUTIONS_PATH ||
    (existsSync(LIVE_EXECUTIONS_PATH) ? LIVE_EXECUTIONS_PATH : DEFAULT_EXECUTIONS_PATH);
}

function sanitizeLaneLabel(value) {
  const raw = String(value || "unknown").trim();
  if (!raw) {
    return "unknown";
  }

  let sanitized = raw.replace(SECRET_PARAM_RE, "$1<redacted>");
  sanitized = sanitized.replace(/https?:\/\/[^\s,|]+/gi, (match) => {
    try {
      const url = new URL(match);
      return `${url.protocol}//${url.host}/<redacted>`;
    } catch {
      return "<redacted-url>";
    }
  });
  sanitized = sanitized.replace(
    /(api[-_]?key|token|auth|signature|access_token|key|c)=([^&\s]+)/gi,
    "$1=<redacted>"
  );
  return sanitized;
}

function objectValue(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : null;
}

function arrayValue(value) {
  return Array.isArray(value) ? value : [];
}

function firstNumber(...values) {
  for (const value of values) {
    const number = numberValue(value);
    if (number !== null) {
      return number;
    }
  }
  return null;
}

function byteBucket(value) {
  const bytes = numberValue(value);
  if (bytes === null) {
    return "bytes=n/a";
  }
  const lower = Math.floor(bytes / 100) * 100;
  const upper = lower + 99;
  return `bytes=${lower}-${upper}`;
}

function lamportsToSol(lamports) {
  return numberValue(lamports) === null ? null : lamports / LAMPORTS_PER_SOL;
}

function solToLamports(sol) {
  const number = numberValue(sol);
  return number === null ? null : Math.round(number * LAMPORTS_PER_SOL);
}

function effectiveTipLamports(row) {
  const tips = [
    {
      provider: "jito",
      lamports: numberValue(row.jitoTipLamports) ?? 0,
      account: stringValue(row.jitoTipAccount)
    },
    {
      provider: "helius_sender",
      lamports: numberValue(row.heliusSenderTipLamports) ?? 0,
      account: stringValue(row.heliusSenderTipAccount)
    },
    {
      provider: "nozomi",
      lamports: numberValue(row.nozomiTipLamports) ?? 0,
      account: stringValue(row.nozomiTipAccount)
    },
    {
      provider: "bloxroute",
      lamports: numberValue(row.bloxrouteTipLamports) ?? 0,
      account: stringValue(row.bloxrouteTipAccount)
    }
  ];
  const byAccount = new Map();
  let accountlessTipLamports = 0;
  for (const tip of tips) {
    if (tip.lamports <= 0) {
      continue;
    }
    if (tip.account === null) {
      accountlessTipLamports += tip.lamports;
      continue;
    }
    byAccount.set(tip.account, Math.max(byAccount.get(tip.account) ?? 0, tip.lamports));
  }
  const configuredTipLamports =
    accountlessTipLamports + [...byAccount.values()].reduce((total, lamports) => total + lamports, 0);

  return {
    jitoTipLamports: tips[0].lamports,
    heliusSenderTipLamports: tips[1].lamports,
    nozomiTipLamports: tips[2].lamports,
    bloxrouteTipLamports: tips[3].lamports,
    configuredTipLamports,
    mergedTipAccountCount: byAccount.size,
    duplicateTipAccountMerged: configuredTipLamports < tips.reduce((total, tip) => total + tip.lamports, 0)
  };
}

function estimatedPriorityFeeLamports(row) {
  const microLamports = numberValue(row.priorityFeeMicroLamports) ?? 0;
  if (microLamports <= 0) {
    return 0;
  }
  const computeUnitLimit = numberValue(row.computeUnitLimit) ?? DEFAULT_COMPUTE_UNIT_LIMIT;
  return Math.ceil((computeUnitLimit * microLamports) / 1_000_000);
}

function feeTipCost(row) {
  const {
    jitoTipLamports,
    heliusSenderTipLamports,
    nozomiTipLamports,
    bloxrouteTipLamports,
    configuredTipLamports,
    mergedTipAccountCount,
    duplicateTipAccountMerged
  } = effectiveTipLamports(row);
  const estimatedPriorityFeeLamportsValue = estimatedPriorityFeeLamports(row);
  const configuredFeeTipLamports = estimatedPriorityFeeLamportsValue + configuredTipLamports;
  const chainReport = objectValue(row.chainReport);
  const observedNetworkFeeSol = firstNumber(row.networkFeeSol, chainReport?.networkFeeSol);
  const observedFeeTipSol =
    observedNetworkFeeSol === null
      ? null
      : observedNetworkFeeSol + (configuredTipLamports / LAMPORTS_PER_SOL);
  const estimatedTotalCopySpendLamports = numberValue(row.estimatedTotalCopySpendLamports);
  const plannedCopySpendLamports = firstNumber(
    row.plannedCopySpendLamports,
    solToLamports(row.plannedCopySolAmount),
    solToLamports(row.observedSolAmount)
  );
  const estimatedSpendOverPlannedLamports =
    estimatedTotalCopySpendLamports !== null && plannedCopySpendLamports !== null
      ? Math.max(0, estimatedTotalCopySpendLamports - plannedCopySpendLamports)
      : null;

  return {
    priorityFeeMicroLamports: numberValue(row.priorityFeeMicroLamports),
    estimatedPriorityFeeLamports: estimatedPriorityFeeLamportsValue,
    jitoTipLamports,
    heliusSenderTipLamports,
    nozomiTipLamports,
    bloxrouteTipLamports,
    configuredTipLamports,
    mergedTipAccountCount,
    duplicateTipAccountMerged,
    configuredFeeTipLamports,
    configuredFeeTipSol: lamportsToSol(configuredFeeTipLamports),
    observedNetworkFeeSol,
    observedFeeTipSol,
    estimatedSpendOverPlannedLamports,
    estimatedSpendOverPlannedSol: lamportsToSol(estimatedSpendOverPlannedLamports)
  };
}

function laneAttribution(row) {
  const attribution = objectValue(row.sendLaneAttribution);
  const rawAttempts = arrayValue(attribution?.allAttempts).length > 0
    ? arrayValue(attribution?.allAttempts)
    : arrayValue(row.sendRpcAttempts);
  const attempts = rawAttempts.map((attempt) => ({
    label: sanitizeLaneLabel(stringValue(attempt?.label) ?? "unknown"),
    kind: stringValue(attempt?.kind) ?? "unknown",
    mode: stringValue(attempt?.mode),
    status: stringValue(attempt?.status) ?? "unknown",
    durationMs: numberValue(attempt?.durationMs),
    fanoutSlots: numberValue(attempt?.fanoutSlots),
    ackAtMs: firstNumber(attempt?.ackAt, attempt?.ackAtMs),
    errorClass: stringValue(attempt?.errorClass),
    error: stringValue(attempt?.error)
  }));

  const firstAckLane = stringValue(attribution?.firstAckLane) ?? stringValue(row.sendRpcWinner);

  return {
    firstAckLane: firstAckLane === null ? null : sanitizeLaneLabel(firstAckLane),
    firstAckAtMs: firstNumber(attribution?.firstAckAtMs, attribution?.firstAckAt),
    allAttempts: attempts,
    attributionComplete: Boolean(attribution)
  };
}

function landingStatus(row) {
  const confirmation = objectValue(row.rustTransactionConfirmation);
  if (confirmation) {
    const status = stringValue(confirmation.status) ?? (confirmation.ok === true ? "landed" : "unknown");
    return {
      status,
      landed: status === "landed" && confirmation.ok !== false,
      failedOnChain: status === "failed" || Boolean(confirmation.err),
      checked: confirmation.checked === true,
      reason: stringValue(confirmation.reason)
    };
  }
  if (row.sendSignature || row.sent || row.decision === "sent") {
    return {
      status: "submitted_no_confirmation",
      landed: false,
      failedOnChain: false,
      checked: false,
      reason: "missing copy buy confirmation sidecar"
    };
  }
  return {
    status: stringValue(row.decision) ?? "unknown",
    landed: false,
    failedOnChain: false,
    checked: false,
    reason: stringValue(row.reason)
  };
}

function positionFields(row) {
  const confirmation = objectValue(row.rustTransactionConfirmation);
  const chainReport = objectValue(row.chainReport);
  const diagnostics = objectValue(chainReport?.blockPositionDiagnostics);
  const sameSlotTxDelta = firstNumber(
    confirmation?.sameSlotTxDelta,
    confirmation?.txsAfterObserved,
    diagnostics?.sameSlotTxDelta
  );
  const txDelta = displayTxDelta(
    {
      txDelta: firstNumber(confirmation?.txDelta, diagnostics?.txDelta),
      sameSlotTxDelta,
      crossSlotPositionSummary: objectValue(diagnostics?.crossSlotPositionSummary)
    },
    numberValue(row.txDelta)
  );

  return {
    slotDelta: firstNumber(confirmation?.slotDelta, diagnostics?.slotDelta, row.slotDelta),
    targetTxIndex: firstNumber(confirmation?.targetTxIndex, diagnostics?.targetTxIndex),
    copyTxIndex: firstNumber(confirmation?.copyTxIndex, diagnostics?.copyTxIndex),
    sameSlotTxDelta,
    txDelta
  };
}

function isCopyBuy(row) {
  return row?.schema === "copytrade.localExecution.v1" && row.observedAction === "buy";
}

function copyBuyLandingRows(rows, { includeUnsent = true } = {}) {
  return rows
    .filter(isCopyBuy)
    .filter((row) => includeUnsent || row.sent || row.sendSignature || row.decision === "sent")
    .map((row) => {
      const landing = landingStatus(row);
      const lane = laneAttribution(row);
      const position = positionFields(row);
      const cost = feeTipCost(row);
      return {
        observedAtMs: numberValue(row.observedAtMs),
        observedSignature: stringValue(row.observedSignature),
        sendSignature: stringValue(row.sendSignature),
        copyWallet: stringValue(row.copyWallet),
        mint: stringValue(row.mint),
        route: stringValue(row.routeLayout) ?? stringValue(row.selectedRoute),
        instructionCount: numberValue(row.instructionCount),
        signedTxBytes: firstNumber(row.signedTxBytes, row.serializedBytes, row.txBytes),
        writableAccountCount: numberValue(row.writableAccountCount),
        computeUnitLimit: numberValue(row.computeUnitLimit),
        selectedTipAccount: stringValue(row.selectedTipAccount),
        sourceComputeUnitLimit: numberValue(row.sourceComputeUnitLimit),
        sourceComputeUnitPriceMicroLamports: numberValue(row.sourceComputeUnitPriceMicroLamports),
        computeUnitsConsumed: numberValue(row.computeUnitsConsumed),
        costUnits: numberValue(row.costUnits),
        blockhashSourceRpc: stringValue(row.blockhashSourceRpc),
        blockhashCommitment: stringValue(row.blockhashCommitment),
        blockhashContextSlot: numberValue(row.blockhashContextSlot),
        blockhashAgeMs: numberValue(row.blockhashAgeMs),
        observedToSignedMs: numberValue(row.observedToSignedMs),
        observedToSendSubmittedMs: numberValue(row.observedToSendSubmittedMs),
        observedToSignatureReturnedMs: numberValue(row.observedToSignatureReturnedMs),
        matchedToPlannedMs: numberValue(row.matchedToPlannedMs),
        plannedToBuiltMs: numberValue(row.plannedToBuiltMs),
        unsignedBuildUs: numberValue(row.unsignedBuildUs),
        signUs: numberValue(row.signUs),
        serializeUs: numberValue(row.serializeUs),
        decision: stringValue(row.decision),
        sent: Boolean(row.sent || row.sendSignature),
        status: landing.status,
        landed: landing.landed,
        notLanded: !landing.landed,
        failedOnChain: landing.failedOnChain,
        confirmationChecked: landing.checked,
        reason: landing.reason,
        firstAckLane: lane.firstAckLane,
        firstAckAtMs: lane.firstAckAtMs,
        allLaneAttempts: lane.allAttempts,
        laneAttributionComplete: lane.attributionComplete,
        feeProfileName: stringValue(row.feeProfileName) ?? "unknown",
        selectedPriorityFeeMicroLamports: numberValue(row.selectedPriorityFeeMicroLamports),
        selectedHeliusTipLamports: numberValue(row.selectedHeliusTipLamports),
        sourcePositionBucket: stringValue(row.sourcePositionBucket) ?? "unknown",
        feeReason: stringValue(row.feeReason),
        feeCapHit: Boolean(row.feeCapHit),
        accountPriorityFeeEnabled: Boolean(row.accountPriorityFeeEnabled),
        accountPriorityFeeMicroLamports: numberValue(row.accountPriorityFeeMicroLamports),
        accountPriorityFeeAgeMs: numberValue(row.accountPriorityFeeAgeMs),
        accountPriorityFeeSampleCount: numberValue(row.accountPriorityFeeSampleCount),
        accountPriorityFeeAccountCount: numberValue(row.accountPriorityFeeAccountCount),
        accountPriorityFeeApplied: Boolean(row.accountPriorityFeeApplied),
        accountPriorityFeeReason: stringValue(row.accountPriorityFeeReason),
        ...position,
        feeTipCost: cost
      };
    });
}

function filterLandingRows(rows, { sinceMs = null, untilMs = null, lastSent = null } = {}) {
  let filtered = rows;
  if (Number.isFinite(sinceMs)) {
    filtered = filtered.filter((row) => Number.isFinite(row.observedAtMs) && row.observedAtMs >= sinceMs);
  }
  if (Number.isFinite(untilMs)) {
    filtered = filtered.filter((row) => Number.isFinite(row.observedAtMs) && row.observedAtMs < untilMs);
  }
  if (Number.isFinite(lastSent) && lastSent > 0) {
    const sentRows = filtered.filter((row) => row.sent);
    const keep = new Set(sentRows.slice(-lastSent));
    filtered = filtered.filter((row) => keep.has(row) || !row.sent);
  }
  return filtered;
}

function needsPositionEnrichment(row) {
  if (!isCopyBuy(row) || !(row.sent || row.sendSignature || row.decision === "sent")) {
    return false;
  }
  return !Number.isFinite(positionFields(row).txDelta);
}

async function enrichMissingPositions(rows, { positionEnrichLimit = DEFAULT_POSITION_ENRICH_LIMIT } = {}) {
  let remaining = positionEnrichLimit;
  for (const row of [...rows].reverse()) {
    if (remaining <= 0) {
      break;
    }
    if (!needsPositionEnrichment(row)) {
      continue;
    }
    try {
      row.chainReport = await chainReport(row);
      remaining -= 1;
    } catch (error) {
      row.chainReport = {
        status: "unknown",
        positionUnavailableReason: `scoreboard chain enrichment failed: ${error.message}`,
        blockPositionDiagnostics: {
          schema: "copytrade.blockPositionDiagnostics.v1",
          status: "unknown",
          unavailableReason: `scoreboard chain enrichment failed: ${error.message}`
        }
      };
      remaining -= 1;
    }
  }
  return rows;
}

function percentile(values, p) {
  const numeric = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  if (numeric.length === 0) {
    return null;
  }
  const index = Math.min(numeric.length - 1, Math.max(0, Math.ceil((p / 100) * numeric.length) - 1));
  return numeric[index];
}

function sum(values) {
  const numeric = values.filter((value) => Number.isFinite(value));
  return numeric.length === 0 ? null : numeric.reduce((total, value) => total + value, 0);
}

function duplicateObservedSendGroups(rows) {
  const groups = new Map();
  for (const row of rows) {
    if (!row.observedSignature || !row.sendSignature) {
      continue;
    }
    if (!groups.has(row.observedSignature)) {
      groups.set(row.observedSignature, new Set());
    }
    groups.get(row.observedSignature).add(row.sendSignature);
  }
  return [...groups.values()].filter((sendSignatures) => sendSignatures.size > 1).length;
}

function summarizeRows(rows, { targetTxDelta = DEFAULT_TARGET_TX_DELTA } = {}) {
  const sent = rows.filter((row) => row.sent);
  const landed = sent.filter((row) => row.landed);
  const sameSlotLanded = landed.filter((row) => row.slotDelta === 0);
  const positionEligible = sent.filter(
    (row) => row.landed || row.failedOnChain || Number.isFinite(row.slotDelta)
  );
  const txDeltaPresent = positionEligible.filter((row) => Number.isFinite(row.txDelta));
  const targetTxDeltaRows = txDeltaPresent.filter((row) => row.txDelta <= targetTxDelta);
  const signedTxBytesPresent = sent.filter((row) => Number.isFinite(row.signedTxBytes));
  return {
    copyBuys: rows.length,
    sent: sent.length,
    landed: landed.length,
    notLanded: sent.length - landed.length,
    failedOnChain: sent.filter((row) => row.failedOnChain).length,
    landedRate: sent.length > 0 ? landed.length / sent.length : null,
    sameSlotLanded: sameSlotLanded.length,
    sameSlotRate: landed.length > 0 ? sameSlotLanded.length / landed.length : null,
    targetTxDelta,
    targetTxDeltaHits: targetTxDeltaRows.length,
    targetTxDeltaRate: txDeltaPresent.length > 0 ? targetTxDeltaRows.length / txDeltaPresent.length : null,
    positionEligible: positionEligible.length,
    txDeltaPresent: txDeltaPresent.length,
    txDeltaCoverage: positionEligible.length > 0 ? txDeltaPresent.length / positionEligible.length : null,
    signedTxBytesPresent: signedTxBytesPresent.length,
    signedTxBytesCoverage: sent.length > 0 ? signedTxBytesPresent.length / sent.length : null,
    missingLaneAttribution: sent.filter((row) => !row.laneAttributionComplete).length,
    duplicateObservedSendGroups: duplicateObservedSendGroups(sent),
    p50SlotDelta: percentile(landed.map((row) => row.slotDelta), 50),
    p90SlotDelta: percentile(landed.map((row) => row.slotDelta), 90),
    p50TxDelta: percentile(landed.map((row) => row.txDelta), 50),
    p90TxDelta: percentile(landed.map((row) => row.txDelta), 90),
    p50ObservedToSignedMs: percentile(sent.map((row) => row.observedToSignedMs), 50),
    p90ObservedToSignedMs: percentile(sent.map((row) => row.observedToSignedMs), 90),
    p50ObservedToSendSubmittedMs: percentile(sent.map((row) => row.observedToSendSubmittedMs), 50),
    p90ObservedToSendSubmittedMs: percentile(sent.map((row) => row.observedToSendSubmittedMs), 90),
    p50ObservedToSignatureReturnedMs: percentile(sent.map((row) => row.observedToSignatureReturnedMs), 50),
    p90ObservedToSignatureReturnedMs: percentile(sent.map((row) => row.observedToSignatureReturnedMs), 90),
    p50MatchedToPlannedMs: percentile(sent.map((row) => row.matchedToPlannedMs), 50),
    p90MatchedToPlannedMs: percentile(sent.map((row) => row.matchedToPlannedMs), 90),
    p50PlannedToBuiltMs: percentile(sent.map((row) => row.plannedToBuiltMs), 50),
    p90PlannedToBuiltMs: percentile(sent.map((row) => row.plannedToBuiltMs), 90),
    p50UnsignedBuildUs: percentile(sent.map((row) => row.unsignedBuildUs), 50),
    p90UnsignedBuildUs: percentile(sent.map((row) => row.unsignedBuildUs), 90),
    p50SignUs: percentile(sent.map((row) => row.signUs), 50),
    p90SignUs: percentile(sent.map((row) => row.signUs), 90),
    p50SerializeUs: percentile(sent.map((row) => row.serializeUs), 50),
    p90SerializeUs: percentile(sent.map((row) => row.serializeUs), 90),
    p50SignedTxBytes: percentile(sent.map((row) => row.signedTxBytes), 50),
    p90SignedTxBytes: percentile(sent.map((row) => row.signedTxBytes), 90),
    totalObservedFeeTipSol: sum(sent.map((row) => row.feeTipCost.observedFeeTipSol)),
    totalConfiguredFeeTipSol: sum(sent.map((row) => row.feeTipCost.configuredFeeTipSol))
  };
}

function metricNotRegressed(canary, baseline, { tolerance = 0 } = {}) {
  if (!Number.isFinite(canary) || !Number.isFinite(baseline)) {
    return null;
  }
  return canary + tolerance >= baseline;
}

function metricImprovedLower(canary, baseline, { minImprovement = 0 } = {}) {
  if (!Number.isFinite(canary) || !Number.isFinite(baseline)) {
    return null;
  }
  return baseline - canary >= minImprovement;
}

function metricImprovedHigher(canary, baseline, { minImprovement = 0 } = {}) {
  if (!Number.isFinite(canary) || !Number.isFinite(baseline)) {
    return null;
  }
  return canary - baseline >= minImprovement;
}

function gateResult(name, ok, details) {
  return { name, ok, details };
}

function evaluatePromotionCandidate(
  baselineSummary,
  canarySummary,
  {
    txDeltaTarget = DEFAULT_PROMOTION_TX_DELTA_TARGET,
    minSameSlotImprovement = 0,
    minTargetTxDeltaRateImprovement = 0,
    minP50TxDeltaImprovement = 1,
    minP90TxDeltaImprovement = 1,
    allowP90ObservedToSignedRegressionMs = 0,
    allowP90ObservedToSendSubmittedRegressionMs = 0,
    allowLandedRateRegression = 0
  } = {}
) {
  const gates = [
    gateResult(
      "landed_rate_no_regression",
      metricNotRegressed(canarySummary.landedRate, baselineSummary.landedRate, {
        tolerance: allowLandedRateRegression
      }),
      `baseline=${formatPercent(baselineSummary.landedRate)} canary=${formatPercent(canarySummary.landedRate)}`
    ),
    gateResult(
      "same_slot_rate_improves",
      metricImprovedHigher(canarySummary.sameSlotRate, baselineSummary.sameSlotRate, {
        minImprovement: minSameSlotImprovement
      }),
      `baseline=${formatPercent(baselineSummary.sameSlotRate)} canary=${formatPercent(canarySummary.sameSlotRate)}`
    ),
    gateResult(
      "p50_tx_delta_improves",
      metricImprovedLower(canarySummary.p50TxDelta, baselineSummary.p50TxDelta, {
        minImprovement: minP50TxDeltaImprovement
      }),
      `baseline=${formatNumber(baselineSummary.p50TxDelta)} canary=${formatNumber(canarySummary.p50TxDelta)}`
    ),
    gateResult(
      "p90_tx_delta_improves",
      metricImprovedLower(canarySummary.p90TxDelta, baselineSummary.p90TxDelta, {
        minImprovement: minP90TxDeltaImprovement
      }),
      `baseline=${formatNumber(baselineSummary.p90TxDelta)} canary=${formatNumber(canarySummary.p90TxDelta)}`
    ),
    gateResult(
      `tx_delta_lte_${txDeltaTarget}_improves`,
      metricImprovedHigher(canarySummary.targetTxDeltaRate, baselineSummary.targetTxDeltaRate, {
        minImprovement: minTargetTxDeltaRateImprovement
      }),
      `baseline=${formatPercent(baselineSummary.targetTxDeltaRate)} canary=${formatPercent(canarySummary.targetTxDeltaRate)}`
    ),
    gateResult(
      "p90_observed_to_signed_no_regression",
      metricNotRegressed(
        baselineSummary.p90ObservedToSignedMs,
        canarySummary.p90ObservedToSignedMs,
        { tolerance: allowP90ObservedToSignedRegressionMs }
      ),
      `baseline=${formatNumber(baselineSummary.p90ObservedToSignedMs)}ms canary=${formatNumber(canarySummary.p90ObservedToSignedMs)}ms`
    ),
    gateResult(
      "p90_observed_to_submitted_no_regression",
      metricNotRegressed(
        baselineSummary.p90ObservedToSendSubmittedMs,
        canarySummary.p90ObservedToSendSubmittedMs,
        { tolerance: allowP90ObservedToSendSubmittedRegressionMs }
      ),
      `baseline=${formatNumber(baselineSummary.p90ObservedToSendSubmittedMs)}ms canary=${formatNumber(canarySummary.p90ObservedToSendSubmittedMs)}ms`
    ),
    gateResult(
      "no_duplicate_observed_send_signatures",
      Number.isFinite(canarySummary.duplicateObservedSendGroups)
        ? canarySummary.duplicateObservedSendGroups === 0
        : null,
      `canaryDuplicateGroups=${formatNumber(canarySummary.duplicateObservedSendGroups)}`
    )
  ];
  const unknown = gates.filter((gate) => gate.ok === null);
  const failed = gates.filter((gate) => gate.ok === false);
  return {
    ok: unknown.length === 0 && failed.length === 0,
    gates,
    unknown: unknown.map((gate) => gate.name),
    failed: failed.map((gate) => gate.name)
  };
}

function evaluateCanarySample(summary, { minSent = DEFAULT_MIN_CANARY_SENT } = {}) {
  if (summary.sent < minSent) {
    return {
      ok: false,
      reason: `only ${summary.sent} sent rows; need ${minSent} before deciding canary`
    };
  }
  return {
    ok: true,
    reason: `${summary.sent} sent rows meets canary sample gate`
  };
}

function evaluateTxDeltaCoverage(
  summary,
  {
    minCoverage = DEFAULT_MIN_TX_DELTA_COVERAGE,
    minPositionEligible = DEFAULT_MIN_POSITION_ELIGIBLE
  } = {}
) {
  if (summary.positionEligible < minPositionEligible) {
    return {
      ok: false,
      reason: `only ${summary.positionEligible} position-eligible rows; need ${minPositionEligible}`
    };
  }
  if (!Number.isFinite(summary.txDeltaCoverage) || summary.txDeltaCoverage < minCoverage) {
    const coverage = Number.isFinite(summary.txDeltaCoverage)
      ? `${(summary.txDeltaCoverage * 100).toFixed(1)}%`
      : "n/a";
    return {
      ok: false,
      reason: `txDelta coverage ${coverage} below ${(minCoverage * 100).toFixed(1)}%`
    };
  }
  return {
    ok: true,
    reason: `txDelta coverage ${(summary.txDeltaCoverage * 100).toFixed(1)}%`
  };
}

function evaluateTargetTxDelta(summary) {
  if (summary.txDeltaPresent <= 0) {
    return {
      ok: false,
      reason: "no txDelta rows available for target gate"
    };
  }
  if (!Number.isFinite(summary.p50TxDelta) || summary.p50TxDelta > summary.targetTxDelta) {
    return {
      ok: false,
      reason: `p50 txDelta ${formatNumber(summary.p50TxDelta)} above target ${summary.targetTxDelta}`
    };
  }
  return {
    ok: true,
    reason: `p50 txDelta ${summary.p50TxDelta} within target ${summary.targetTxDelta}`
  };
}

function groupByFirstAckLane(rows, options = {}) {
  const groups = new Map();
  for (const row of rows.filter((candidate) => candidate.sent)) {
    const lane = row.firstAckLane ?? "missing-first-ack";
    const group = groups.get(lane) ?? [];
    group.push(row);
    groups.set(lane, group);
  }
  return [...groups.entries()]
    .map(([lane, groupRows]) => ({
      lane,
      ...summarizeRows(groupRows, options),
      attemptLabels: [...new Set(groupRows.flatMap((row) => row.allLaneAttempts.map((attempt) => attempt.label)))].sort()
    }))
    .sort((a, b) => b.sent - a.sent || a.lane.localeCompare(b.lane));
}

function landedRate(summary) {
  return summary.landedRate;
}

function shapeKey(row) {
  const route = row.route ?? "route=n/a";
  const instructionCount = Number.isFinite(row.instructionCount) ? row.instructionCount : "n/a";
  return `${route} | ix=${instructionCount} | ${byteBucket(row.signedTxBytes)}`;
}

function groupByTransactionShape(rows, options = {}) {
  const groups = new Map();
  for (const row of rows.filter((candidate) => candidate.sent)) {
    const shape = shapeKey(row);
    const group = groups.get(shape) ?? [];
    group.push(row);
    groups.set(shape, group);
  }
  return [...groups.entries()]
    .map(([shape, groupRows]) => ({
      shape,
      ...summarizeRows(groupRows, options)
    }))
    .sort((a, b) => b.sent - a.sent || a.shape.localeCompare(b.shape));
}

function feeProfileKey(row) {
  return [
    `profile=${row.feeProfileName ?? "unknown"}`,
    `bucket=${row.sourcePositionBucket ?? "unknown"}`,
    `priority=${Number.isFinite(row.selectedPriorityFeeMicroLamports) ? row.selectedPriorityFeeMicroLamports : "n/a"}`,
    `heliusTip=${Number.isFinite(row.selectedHeliusTipLamports) ? row.selectedHeliusTipLamports : "n/a"}`,
    `capHit=${row.feeCapHit ? "yes" : "no"}`
  ].join(" | ");
}

function groupByFeeProfile(rows, options = {}) {
  const groups = new Map();
  for (const row of rows.filter((candidate) => candidate.sent)) {
    const key = feeProfileKey(row);
    const group = groups.get(key) ?? [];
    group.push(row);
    groups.set(key, group);
  }
  return [...groups.entries()]
    .map(([feeProfile, groupRows]) => ({
      feeProfile,
      ...summarizeRows(groupRows, options),
      reasons: [...new Set(groupRows.map((row) => row.feeReason).filter(Boolean))].sort()
    }))
    .sort((a, b) => b.sent - a.sent || a.feeProfile.localeCompare(b.feeProfile));
}

function scoreLaneSummary(summary) {
  if (summary.sent <= 0) {
    return null;
  }
  const landingRate = landedRate(summary) ?? 0;
  const failedRate = summary.failedOnChain / summary.sent;
  const notLandedRate = summary.notLanded / summary.sent;
  const slotPenalty = Number.isFinite(summary.p90SlotDelta) ? Math.max(0, summary.p90SlotDelta) * 4 : 12;
  const txPenalty = Number.isFinite(summary.p90TxDelta) ? Math.max(0, summary.p90TxDelta) * 0.25 : 3;
  const configuredCostPerSent =
    Number.isFinite(summary.totalConfiguredFeeTipSol) && summary.sent > 0
      ? summary.totalConfiguredFeeTipSol / summary.sent
      : 0;
  const costPenalty = configuredCostPerSent * 1_000;
  return Number((landingRate * 100 - failedRate * 25 - notLandedRate * 35 - slotPenalty - txPenalty - costPenalty).toFixed(3));
}

function laneAttemptKey(attempt) {
  const mode = attempt.mode ? `/${attempt.mode}` : "";
  return `${attempt.label}${mode}`;
}

function isSubmittedOrDispatchedAttempt(attempt) {
  return attempt.status === "submitted" || attempt.status === "dispatched";
}

function groupByAttemptLane(rows, options = {}) {
  const groups = new Map();
  for (const row of rows.filter((candidate) => candidate.sent)) {
    for (const attempt of row.allLaneAttempts) {
      if (!isSubmittedOrDispatchedAttempt(attempt)) {
        continue;
      }
      const lane = laneAttemptKey(attempt);
      const group = groups.get(lane) ?? [];
      group.push(row);
      groups.set(lane, group);
    }
  }
  return [...groups.entries()]
    .map(([lane, groupRows]) => {
      const summary = summarizeRows(groupRows, options);
      return {
        lane,
        ...summary,
        landedRate: landedRate(summary),
        configuredFeeTipPerSentSol:
          Number.isFinite(summary.totalConfiguredFeeTipSol) && summary.sent > 0
            ? summary.totalConfiguredFeeTipSol / summary.sent
            : null,
        dispatchedAttempts: groupRows.flatMap((row) => row.allLaneAttempts).filter((attempt) =>
          laneAttemptKey(attempt) === lane && attempt.status === "dispatched"
        ).length,
        submittedAttempts: groupRows.flatMap((row) => row.allLaneAttempts).filter((attempt) =>
          laneAttemptKey(attempt) === lane && attempt.status === "submitted"
        ).length,
        errorClasses: [
          ...new Set(
            groupRows
              .flatMap((row) => row.allLaneAttempts)
              .filter((attempt) => laneAttemptKey(attempt) === lane && attempt.errorClass)
              .map((attempt) => attempt.errorClass)
          )
        ].sort(),
        adaptiveScore: scoreLaneSummary(summary),
        reportOnly: true
      };
    })
    .sort((a, b) => (b.adaptiveScore ?? -Infinity) - (a.adaptiveScore ?? -Infinity) || b.sent - a.sent || a.lane.localeCompare(b.lane));
}

function buildLandingScoreboard(rows, options = {}) {
  const landingRows = filterLandingRows(copyBuyLandingRows(rows, options), options);
  const summary = summarizeRows(landingRows, options);
  return {
    generatedAt: new Date().toISOString(),
    summary,
    sampleGate: evaluateCanarySample(summary, options),
    txDeltaGate: evaluateTxDeltaCoverage(summary, options),
    targetGate: evaluateTargetTxDelta(summary),
    byFirstAckLane: groupByFirstAckLane(landingRows, options),
    byFeeProfile: groupByFeeProfile(landingRows, options),
    byTransactionShape: groupByTransactionShape(landingRows, options),
    adaptiveLaneScores: groupByAttemptLane(landingRows, options),
    rows: landingRows
  };
}

function buildPromotionComparison(rows, options = {}) {
  const {
    baselineSinceMs = null,
    baselineUntilMs = null,
    canarySinceMs = null,
    canaryUntilMs = null,
    baselineOptions = {},
    canaryOptions = {},
    promotionOptions = {},
    ...sharedOptions
  } = options;
  const baseline = buildLandingScoreboard(rows, {
    ...sharedOptions,
    ...baselineOptions,
    sinceMs: baselineSinceMs,
    untilMs: baselineUntilMs
  });
  const canary = buildLandingScoreboard(rows, {
    ...sharedOptions,
    ...canaryOptions,
    sinceMs: canarySinceMs,
    untilMs: canaryUntilMs
  });
  return {
    baseline,
    canary,
    promotion: evaluatePromotionCandidate(baseline.summary, canary.summary, promotionOptions)
  };
}

function short(value) {
  if (!value) {
    return "n/a";
  }
  return value.length <= 14 ? value : `${value.slice(0, 6)}...${value.slice(-6)}`;
}

function formatNumber(value) {
  return Number.isFinite(value) ? String(value) : "n/a";
}

function formatSol(value) {
  if (!Number.isFinite(value)) {
    return "n/a";
  }
  return value.toFixed(9).replace(/0+$/, "").replace(/\.$/, "");
}

function formatPercent(value) {
  return Number.isFinite(value) ? `${(value * 100).toFixed(1)}%` : "n/a";
}

function formatAttempts(attempts) {
  if (!attempts.length) {
    return "n/a";
  }
  return attempts
    .map((attempt) => {
      const mode = attempt.mode ? `/${attempt.mode}` : "";
      const duration = Number.isFinite(attempt.durationMs) ? ` ${attempt.durationMs}ms` : "";
      return `${attempt.label}${mode}:${attempt.status}${duration}`;
    })
    .join(" | ");
}

function printTextReport(scoreboard, { path, limit }) {
  const { summary } = scoreboard;
  console.log(`Landing scoreboard | path=${path}`);
  console.log(
    `copyBuys=${summary.copyBuys} sent=${summary.sent} landed=${summary.landed} notLanded=${summary.notLanded} failedOnChain=${summary.failedOnChain} landedRate=${formatPercent(summary.landedRate)} sameSlotRate=${formatPercent(summary.sameSlotRate)}`
  );
  console.log(
    `p50 slotDelta=${formatNumber(summary.p50SlotDelta)} p90 slotDelta=${formatNumber(summary.p90SlotDelta)} p50 txDelta=${formatNumber(summary.p50TxDelta)} p90 txDelta=${formatNumber(summary.p90TxDelta)}`
  );
  console.log(
    `target txDelta<=${summary.targetTxDelta} hits=${summary.targetTxDeltaHits}/${summary.txDeltaPresent} rate=${formatPercent(summary.targetTxDeltaRate)}`
  );
  console.log(
    `target gate=${scoreboard.targetGate.ok ? "pass" : "fail"} ${scoreboard.targetGate.reason}`
  );
  console.log(
    `signedBytes coverage=${formatPercent(summary.signedTxBytesCoverage)} (${summary.signedTxBytesPresent}/${summary.sent}) p50=${formatNumber(summary.p50SignedTxBytes)} p90=${formatNumber(summary.p90SignedTxBytes)}`
  );
  console.log(
    `sample gate=${scoreboard.sampleGate.ok ? "pass" : "wait"} ${scoreboard.sampleGate.reason}`
  );
  console.log(
    `txDelta coverage=${Number.isFinite(summary.txDeltaCoverage) ? `${(summary.txDeltaCoverage * 100).toFixed(1)}%` : "n/a"} (${summary.txDeltaPresent}/${summary.positionEligible}) gate=${scoreboard.txDeltaGate.ok ? "pass" : "fail"} ${scoreboard.txDeltaGate.reason}`
  );
  console.log(
    `feeTip observed=${formatSol(summary.totalObservedFeeTipSol)} SOL configured=${formatSol(summary.totalConfiguredFeeTipSol)} SOL missingLaneAttribution=${summary.missingLaneAttribution}`
  );
  console.log("");
  console.log("Adaptive lane scores (report-only, no auto-selection)");
  for (const row of scoreboard.adaptiveLaneScores) {
    console.log(
      [
        row.lane,
        `score=${formatNumber(row.adaptiveScore)}`,
        `sent=${row.sent}`,
        `submitted=${row.submittedAttempts}`,
        `dispatched=${row.dispatchedAttempts}`,
        `landedRate=${formatPercent(row.landedRate)}`,
        `sameSlotRate=${formatPercent(row.sameSlotRate)}`,
        `targetTxRate=${formatPercent(row.targetTxDeltaRate)}`,
        `byteCoverage=${formatPercent(row.signedTxBytesCoverage)}`,
        `p90Slot=${formatNumber(row.p90SlotDelta)}`,
        `p90Tx=${formatNumber(row.p90TxDelta)}`,
        `feeTipPerSent=${formatSol(row.configuredFeeTipPerSentSol)} SOL`,
        `errors=${row.errorClasses.length ? row.errorClasses.join(",") : "none"}`
      ].join(" | ")
    );
  }
  console.log("");
  console.log("By fee profile");
  for (const row of scoreboard.byFeeProfile) {
    console.log(
      [
        row.feeProfile,
        `sent=${row.sent}`,
        `landed=${row.landed}`,
        `notLanded=${row.notLanded}`,
        `failed=${row.failedOnChain}`,
        `sameSlotRate=${formatPercent(row.sameSlotRate)}`,
        `targetTxRate=${formatPercent(row.targetTxDeltaRate)}`,
        `p50Tx=${formatNumber(row.p50TxDelta)}`,
        `p90Tx=${formatNumber(row.p90TxDelta)}`,
        `configuredFeeTip=${formatSol(row.totalConfiguredFeeTipSol)} SOL`,
        `reasons=${row.reasons.length ? row.reasons.join(",") : "n/a"}`
      ].join(" | ")
    );
  }
  console.log("");
  console.log("By transaction shape");
  for (const row of scoreboard.byTransactionShape) {
    console.log(
      [
        row.shape,
        `sent=${row.sent}`,
        `landed=${row.landed}`,
        `notLanded=${row.notLanded}`,
        `failed=${row.failedOnChain}`,
        `sameSlotRate=${formatPercent(row.sameSlotRate)}`,
        `targetTxRate=${formatPercent(row.targetTxDeltaRate)}`,
        `byteCoverage=${formatPercent(row.signedTxBytesCoverage)}`,
        `p50Slot=${formatNumber(row.p50SlotDelta)}`,
        `p50Tx=${formatNumber(row.p50TxDelta)}`,
        `p50Bytes=${formatNumber(row.p50SignedTxBytes)}`
      ].join(" | ")
    );
  }
  console.log("");
  console.log("By first ACK lane (landing scoreboard, not ACK scoreboard)");
  for (const row of scoreboard.byFirstAckLane) {
    console.log(
      [
        row.lane,
        `sent=${row.sent}`,
        `landed=${row.landed}`,
        `notLanded=${row.notLanded}`,
        `failed=${row.failedOnChain}`,
        `sameSlotRate=${formatPercent(row.sameSlotRate)}`,
        `targetTxRate=${formatPercent(row.targetTxDeltaRate)}`,
        `byteCoverage=${formatPercent(row.signedTxBytesCoverage)}`,
        `p50Slot=${formatNumber(row.p50SlotDelta)}`,
        `p50Tx=${formatNumber(row.p50TxDelta)}`,
        `configuredFeeTip=${formatSol(row.totalConfiguredFeeTipSol)} SOL`
      ].join(" | ")
    );
  }
  console.log("");
  console.log(`Recent copy buys (limit ${limit})`);
  for (const row of scoreboard.rows.slice(-limit).reverse()) {
    console.log(
      [
        new Date(row.observedAtMs ?? 0).toISOString(),
        `ack=${row.firstAckLane ?? "n/a"}`,
        `status=${row.status}`,
        `slotDelta=${formatNumber(row.slotDelta)}`,
        `txDelta=${formatNumber(row.txDelta)}`,
        `idx=${formatNumber(row.targetTxIndex)}->${formatNumber(row.copyTxIndex)}`,
        `feeProfile=${row.feeProfileName}/${row.sourcePositionBucket}`,
        `acctFee=${formatNumber(row.accountPriorityFeeMicroLamports)}`,
        `acctApplied=${row.accountPriorityFeeApplied ? "yes" : "no"}`,
        `feeTip=${formatSol(row.feeTipCost.observedFeeTipSol ?? row.feeTipCost.configuredFeeTipSol)} SOL`,
        `route=${row.route ?? "n/a"}`,
        `ix=${formatNumber(row.instructionCount)}`,
        `bytes=${formatNumber(row.signedTxBytes)}`,
        `writable=${formatNumber(row.writableAccountCount)}`,
        `tipAcct=${short(row.selectedTipAccount)}`,
        `srcCU=${formatNumber(row.sourceComputeUnitPriceMicroLamports)}`,
        `cost=${formatNumber(row.costUnits)}`,
        `bh=${row.blockhashCommitment ?? "n/a"}/${formatNumber(row.blockhashAgeMs)}ms`,
        `copy=${short(row.sendSignature)}`,
        `attempts=${formatAttempts(row.allLaneAttempts)}`
      ].join(" | ")
    );
  }
}

function printPromotionComparison(comparison, { path }) {
  const { baseline, canary, promotion } = comparison;
  console.log(`Landing promotion comparison | path=${path}`);
  console.log(
    [
      "baseline",
      `sent=${baseline.summary.sent}`,
      `landedRate=${formatPercent(baseline.summary.landedRate)}`,
      `sameSlotRate=${formatPercent(baseline.summary.sameSlotRate)}`,
      `p50Tx=${formatNumber(baseline.summary.p50TxDelta)}`,
      `p90Tx=${formatNumber(baseline.summary.p90TxDelta)}`,
      `targetTxRate=${formatPercent(baseline.summary.targetTxDeltaRate)}`,
      `p90ObsToSigned=${formatNumber(baseline.summary.p90ObservedToSignedMs)}ms`,
      `p90ObsToSubmitted=${formatNumber(baseline.summary.p90ObservedToSendSubmittedMs)}ms`,
      `duplicateGroups=${formatNumber(baseline.summary.duplicateObservedSendGroups)}`
    ].join(" | ")
  );
  console.log(
    [
      "canary",
      `sent=${canary.summary.sent}`,
      `landedRate=${formatPercent(canary.summary.landedRate)}`,
      `sameSlotRate=${formatPercent(canary.summary.sameSlotRate)}`,
      `p50Tx=${formatNumber(canary.summary.p50TxDelta)}`,
      `p90Tx=${formatNumber(canary.summary.p90TxDelta)}`,
      `targetTxRate=${formatPercent(canary.summary.targetTxDeltaRate)}`,
      `p90ObsToSigned=${formatNumber(canary.summary.p90ObservedToSignedMs)}ms`,
      `p90ObsToSubmitted=${formatNumber(canary.summary.p90ObservedToSendSubmittedMs)}ms`,
      `duplicateGroups=${formatNumber(canary.summary.duplicateObservedSendGroups)}`
    ].join(" | ")
  );
  console.log(
    `baseline sample=${baseline.sampleGate.ok ? "pass" : "fail"} ${baseline.sampleGate.reason}`
  );
  console.log(
    `baseline txDelta=${baseline.txDeltaGate.ok ? "pass" : "fail"} ${baseline.txDeltaGate.reason}`
  );
  console.log(`canary sample=${canary.sampleGate.ok ? "pass" : "fail"} ${canary.sampleGate.reason}`);
  console.log(`canary txDelta=${canary.txDeltaGate.ok ? "pass" : "fail"} ${canary.txDeltaGate.reason}`);
  console.log(`promotion=${promotion.ok ? "pass" : "fail"}`);
  for (const gate of promotion.gates) {
    console.log(`${gate.ok === true ? "pass" : gate.ok === false ? "fail" : "unknown"} ${gate.name} ${gate.details}`);
  }
}

async function main() {
  const path = argValue("executions", argValue("path", defaultExecutionsPath()));
  const limit = positiveInteger(argValue("limit", process.env.JITO_LANDING_REPORT_LIMIT), DEFAULT_LIMIT);
  const includeUnsent = !hasFlag("sent-only");
  const sinceMs =
    nonnegativeInteger(argValue("since-ms"), null) ??
    (stringValue(argValue("since-iso")) === null ? null : Date.parse(argValue("since-iso")));
  const untilMs =
    nonnegativeInteger(argValue("until-ms"), null) ??
    (stringValue(argValue("until-iso")) === null ? null : Date.parse(argValue("until-iso")));
  const lastSent = positiveInteger(argValue("last-sent"), null);
  const positionEnrichLimit = positiveInteger(
    argValue("position-enrich-limit", process.env.JITO_LANDING_POSITION_ENRICH_LIMIT),
    DEFAULT_POSITION_ENRICH_LIMIT
  );
  const minTxDeltaCoverage = nonnegativeNumber(
    argValue("min-tx-delta-coverage", process.env.JITO_LANDING_MIN_TX_DELTA_COVERAGE),
    DEFAULT_MIN_TX_DELTA_COVERAGE
  );
  const minPositionEligible = positiveInteger(
    argValue("min-position-eligible", process.env.JITO_LANDING_MIN_POSITION_ELIGIBLE),
    DEFAULT_MIN_POSITION_ELIGIBLE
  );
  const minCanarySent = positiveInteger(
    argValue("min-canary-sent", process.env.JITO_LANDING_MIN_CANARY_SENT),
    DEFAULT_MIN_CANARY_SENT
  );
  const targetTxDelta = nonnegativeInteger(
    argValue("target-tx-delta", process.env.JITO_LANDING_TARGET_TX_DELTA),
    DEFAULT_TARGET_TX_DELTA
  );
  const promotionTxDeltaTarget = nonnegativeInteger(
    argValue("promotion-tx-delta-target", process.env.JITO_LANDING_PROMOTION_TX_DELTA_TARGET),
    DEFAULT_PROMOTION_TX_DELTA_TARGET
  );
  const allowP90ObservedToSignedRegressionMs = nonnegativeNumber(
    argValue(
      "allow-p90-observed-to-signed-regression-ms",
      process.env.JITO_LANDING_ALLOW_P90_OBSERVED_TO_SIGNED_REGRESSION_MS
    ),
    0
  );
  const allowP90ObservedToSendSubmittedRegressionMs = nonnegativeNumber(
    argValue(
      "allow-p90-observed-to-submitted-regression-ms",
      process.env.JITO_LANDING_ALLOW_P90_OBSERVED_TO_SUBMITTED_REGRESSION_MS
    ),
    0
  );
  const rawRows = readJsonl(path);
  const rows = dedupeRows(rawRows);
  const enrichedRows = await enrichMissingPositions(rows, { positionEnrichLimit });
  if (hasFlag("promotion-compare")) {
    const baselineSinceMs =
      nonnegativeInteger(argValue("baseline-since-ms"), null) ??
      (stringValue(argValue("baseline-since-iso")) === null ? null : Date.parse(argValue("baseline-since-iso")));
    const baselineUntilMs =
      nonnegativeInteger(argValue("baseline-until-ms"), null) ??
      (stringValue(argValue("baseline-until-iso")) === null ? null : Date.parse(argValue("baseline-until-iso")));
    const canarySinceMs =
      nonnegativeInteger(argValue("canary-since-ms"), null) ??
      (stringValue(argValue("canary-since-iso")) === null ? null : Date.parse(argValue("canary-since-iso")));
    const canaryUntilMs =
      nonnegativeInteger(argValue("canary-until-ms"), null) ??
      (stringValue(argValue("canary-until-iso")) === null ? null : Date.parse(argValue("canary-until-iso")));
    const comparison = buildPromotionComparison(enrichedRows, {
      includeUnsent,
      minCoverage: minTxDeltaCoverage,
      minPositionEligible,
      minSent: minCanarySent,
      targetTxDelta: promotionTxDeltaTarget,
      baselineSinceMs,
      baselineUntilMs,
      canarySinceMs,
      canaryUntilMs,
      promotionOptions: {
        txDeltaTarget: promotionTxDeltaTarget,
        allowP90ObservedToSignedRegressionMs,
        allowP90ObservedToSendSubmittedRegressionMs
      }
    });
    const rawCanaryRows = filterLandingRows(copyBuyLandingRows(rawRows, { includeUnsent }), {
      sinceMs: canarySinceMs,
      untilMs: canaryUntilMs
    });
    comparison.canary.summary.duplicateObservedSendGroups = Math.max(
      comparison.canary.summary.duplicateObservedSendGroups,
      duplicateObservedSendGroups(rawCanaryRows)
    );
    comparison.promotion = evaluatePromotionCandidate(
      comparison.baseline.summary,
      comparison.canary.summary,
      {
        txDeltaTarget: promotionTxDeltaTarget,
        allowP90ObservedToSignedRegressionMs,
        allowP90ObservedToSendSubmittedRegressionMs
      }
    );
    if (hasFlag("json")) {
      console.log(JSON.stringify({ path, ...comparison }, null, 2));
    } else {
      printPromotionComparison(comparison, { path });
    }
    if (
      !comparison.baseline.sampleGate.ok ||
      !comparison.baseline.txDeltaGate.ok ||
      !comparison.canary.sampleGate.ok ||
      !comparison.canary.txDeltaGate.ok ||
      !comparison.promotion.ok
    ) {
      process.exitCode = 1;
    }
    return;
  }
  const scoreboard = buildLandingScoreboard(enrichedRows, {
    includeUnsent,
    sinceMs,
    untilMs,
    lastSent,
    minCoverage: minTxDeltaCoverage,
    minPositionEligible,
    minSent: minCanarySent,
    targetTxDelta
  });

  if (hasFlag("json")) {
    console.log(JSON.stringify({ path, ...scoreboard }, null, 2));
  } else {
    printTextReport(scoreboard, { path, limit });
  }

  if (!scoreboard.sampleGate.ok || !scoreboard.txDeltaGate.ok) {
    process.exitCode = 1;
  }
}

export {
  buildLandingScoreboard,
  buildPromotionComparison,
  copyBuyLandingRows,
  effectiveTipLamports,
  evaluateCanarySample,
  evaluatePromotionCandidate,
  evaluateTargetTxDelta,
  evaluateTxDeltaCoverage,
  estimatedPriorityFeeLamports,
  feeTipCost,
  filterLandingRows,
  enrichMissingPositions,
  groupByTransactionShape,
  groupByFeeProfile,
  groupByAttemptLane,
  laneAttribution,
  needsPositionEnrichment,
  sanitizeLaneLabel
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
