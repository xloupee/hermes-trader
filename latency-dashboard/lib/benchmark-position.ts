import type { LocalExecutionReport } from "@/lib/local-executions";

export type Tone = "good" | "bad" | "muted";

export function firstNumber(...values: Array<number | null | undefined>): number | null {
  return values.find((value): value is number => typeof value === "number" && Number.isFinite(value)) ?? null;
}

export function allNumbers(...values: Array<number | null | undefined>): number[] | null {
  const numeric = values.filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  return numeric.length === values.length ? numeric : null;
}

export function deltaMs(end: number | null | undefined, start: number | null | undefined): number | null {
  if (typeof end !== "number" || typeof start !== "number") {
    return null;
  }
  return Number.isFinite(end) && Number.isFinite(start) ? Math.max(0, end - start) : null;
}

export function subtractMs(left: number | null | undefined, right: number | null | undefined): number | null {
  if (
    typeof left !== "number" ||
    !Number.isFinite(left) ||
    typeof right !== "number" ||
    !Number.isFinite(right)
  ) {
    return null;
  }
  return Math.round(left - right);
}

export function executionLocalDetectUs(report: LocalExecutionReport | null | undefined): number | null {
  const summed = allNumbers(report?.feedReceivedToDecodedUs, report?.batchScanUs, report?.txParseUs);
  if (summed) {
    return summed.reduce((total, value) => total + value, 0);
  }
  return firstNumber(report?.decodedToMatchedUs, report?.txParseUs);
}

export function copyAckMs(report: LocalExecutionReport | null | undefined): number | null {
  const returned = report?.observedToSignatureReturnedMs;
  const submitted = report?.observedToSendSubmittedMs;
  if (typeof returned !== "number" || !Number.isFinite(returned)) {
    return null;
  }
  if (typeof submitted !== "number" || !Number.isFinite(submitted)) {
    return returned;
  }
  return Math.max(0, returned - submitted);
}

export function autoSellAckMs(row: LocalExecutionReport): number | null {
  return deltaMs(row.buySignatureToAutoSellSignatureReturnedMs, row.buySignatureToAutoSellSubmittedMs);
}

export function slots(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return "n/a";
  }
  return value === 0 ? "0" : `+${value}`;
}

export function slotDelta(report: LocalExecutionReport | null | undefined): string {
  const diagnosticsDelta = report?.blockPositionDiagnostics?.slotDelta;
  if (typeof diagnosticsDelta === "number" && Number.isFinite(diagnosticsDelta)) {
    return slots(diagnosticsDelta);
  }
  return slots(report?.slotDeltaFromObserved);
}

export function txsAfterWallet(report: LocalExecutionReport | null | undefined): string {
  const diagnostics = report?.blockPositionDiagnostics;
  if (!diagnostics) {
    return "n/a";
  }
  if (diagnostics.status !== "found") {
    return diagnostics.unavailableReason ? "unknown" : "n/a";
  }
  if (typeof diagnostics.txDelta === "number" && Number.isFinite(diagnostics.txDelta)) {
    return `+${diagnostics.txDelta}`;
  }
  if (diagnostics.slotDelta !== 0) {
    const crossSlotTxDelta = diagnostics.crossSlotPositionSummary?.crossSlotTxDelta;
    if (typeof crossSlotTxDelta === "number" && Number.isFinite(crossSlotTxDelta)) {
      return `+${crossSlotTxDelta}`;
    }
    return typeof diagnostics.slotDelta === "number" ? `+${diagnostics.slotDelta} slot` : "cross-slot";
  }
  return typeof diagnostics.sameSlotTxDelta === "number" ? `+${diagnostics.sameSlotTxDelta}` : "same slot";
}

export function sameSlotTxsAfterWallet(report: LocalExecutionReport | null | undefined): string {
  const diagnostics = report?.blockPositionDiagnostics;
  if (!diagnostics) {
    return "n/a";
  }
  if (diagnostics.status !== "found") {
    return diagnostics.unavailableReason ? "unknown" : "n/a";
  }
  if (diagnostics.slotDelta !== 0) {
    return "cross-slot";
  }
  return typeof diagnostics.sameSlotTxDelta === "number" && Number.isFinite(diagnostics.sameSlotTxDelta)
    ? `+${diagnostics.sameSlotTxDelta}`
    : "same slot";
}

export function crossSlotLanding(report: LocalExecutionReport | null | undefined): string {
  const diagnostics = report?.blockPositionDiagnostics;
  if (!diagnostics) {
    return "n/a";
  }
  if (diagnostics.status !== "found") {
    return diagnostics.unavailableReason ? "unknown" : "n/a";
  }
  if (diagnostics.slotDelta === 0) {
    return "same slot";
  }
  if (typeof diagnostics.slotDelta !== "number" || !Number.isFinite(diagnostics.slotDelta)) {
    return "cross-slot";
  }
  const crossSlotTxDelta = diagnostics.crossSlotPositionSummary?.crossSlotTxDelta;
  const txs =
    typeof crossSlotTxDelta === "number" && Number.isFinite(crossSlotTxDelta)
      ? `, +${crossSlotTxDelta} tx`
      : "";
  return `+${diagnostics.slotDelta} slot${diagnostics.slotDelta === 1 ? "" : "s"}${txs}`;
}

export function positionSummary(report: LocalExecutionReport | null | undefined): string {
  const diagnostics = report?.blockPositionDiagnostics;
  if (!diagnostics) {
    return "n/a";
  }
  if (diagnostics.status !== "found") {
    return diagnostics.unavailableReason ? "unknown" : "n/a";
  }
  if (diagnostics.slotDelta === 0) {
    const sameSlotDelta = diagnostics.txDelta ?? diagnostics.sameSlotTxDelta;
    return typeof sameSlotDelta === "number" ? `+${sameSlotDelta} tx` : "same slot";
  }
  return typeof diagnostics.slotDelta === "number" ? `+${diagnostics.slotDelta} slot` : "n/a";
}

export function actionTone(action: string): Tone {
  if (action === "buy") {
    return "good";
  }
  if (action === "sell") {
    return "bad";
  }
  return "muted";
}

export function statusTone(status: string | null | undefined): Tone {
  if (status === "confirmed" || status === "submitted" || status === "buyLanded" || status === "autoSellLanded") {
    return "good";
  }
  if (status === "failed" || status === "expired" || status === "buyFailedOnChain" || status === "autoSellFailedOnChain") {
    return "bad";
  }
  return "muted";
}

export function decisionTone(report: LocalExecutionReport | null | undefined): Tone {
  if (!report) {
    return "muted";
  }
  if (report.buyStatus === "buyLanded") {
    return "good";
  }
  if (report.buyStatus === "buyFailedOnChain" || report.decision === "error") {
    return "bad";
  }
  return "muted";
}

export function positionTone(report: LocalExecutionReport | null | undefined): Tone {
  const diagnostics = report?.blockPositionDiagnostics;
  if (!diagnostics || diagnostics.status !== "found") {
    return "muted";
  }
  const txDelta = diagnostics.txDelta ?? diagnostics.sameSlotTxDelta;
  if (diagnostics.slotDelta === 0 && typeof txDelta === "number" && txDelta >= 0) {
    return "good";
  }
  return diagnostics.slotDelta !== null && diagnostics.slotDelta > 0 ? "bad" : "muted";
}

export function copyStatus(report: LocalExecutionReport | null | undefined): string {
  if (!report) {
    return "none";
  }
  return report.buyStatus || report.decision;
}

export function autoSellStatus(report: LocalExecutionReport | null | undefined): string {
  if (!report) {
    return "none";
  }
  return report.autoSellStatus || report.autoSellDecision || (report.autoSellEnabled ? "armed" : "off");
}

export function chainError(value: unknown): string {
  return value ? JSON.stringify(value) : "none";
}
