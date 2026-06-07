export const copyTradeLatencyMilestones = [
  "received",
  "normalized",
  "request_built",
  "risk_checked",
  "live_gate_checked",
  "balance_checked",
  "direct_buy_state_refreshed",
  "direct_wallet_checked",
  "direct_signer_ready",
  "direct_warmup_started",
  "direct_build_started",
  "direct_build_finished",
  "direct_blockhash_started",
  "direct_blockhash_received",
  "direct_signing_started",
  "direct_signing_finished",
  "direct_simulate_started",
  "direct_simulate_finished",
  "direct_raw_send_started",
  "direct_raw_signature_returned",
  "direct_raw_send_failed",
  "direct_confirmation_started",
  "direct_confirmation_finished",
  "submit_started",
  "submit_finished",
  "skipped"
] as const;

export type CopyTradeLatencyMilestone = (typeof copyTradeLatencyMilestones)[number];
export type CopyTradeLatencyMode = "dry" | "live";
export type CopyTradeLatencyClock = () => number;

export interface CopyTradeLatencyContext {
  chatId: string | number | null;
  sourceWallet: string | null;
  tradingWallet: string | null;
  observedSignature: string | null;
  mint: string | null;
  mode: CopyTradeLatencyMode;
}

export interface CopyTradeLatencyMilestoneDetails {
  status?: string | null;
  reason?: string | null;
  signature?: string | null;
}

export interface CopyTradeLatencyMilestoneRecord extends CopyTradeLatencyMilestoneDetails {
  milestone: CopyTradeLatencyMilestone;
  atMs: number;
}

export interface CopyTradeLatencyTrace {
  context: CopyTradeLatencyContext;
  startedAtMs: number;
  milestones: CopyTradeLatencyMilestoneRecord[];
}

export interface CopyTradeLatencyLogMetadata {
  event: "copy_trade_latency";
  chatId: string | null;
  sourceWallet: string | null;
  tradingWallet: string | null;
  observedSignature: string | null;
  mint: string | null;
  mode: CopyTradeLatencyMode;
  status: string;
  reason: string | null;
  signature: string | null;
  totalMs: number;
  stagesMs: Record<string, number>;
}

export interface CopyTradeLatencySummaryUpdate {
  targetTimestamp?: number | null;
  targetSlot?: number | null;
  copySlot?: number | null;
  winnerProvider?: string | null;
  sendRpcWinner?: string | null;
  sendRpcCount?: number | null;
}

export interface CopyTradeLatencySummaryMetadata {
  event: "copy_trade_latency_summary";
  chatId: string | null;
  sourceWallet: string | null;
  tradingWallet: string | null;
  observedSignature: string | null;
  mint: string | null;
  mode: CopyTradeLatencyMode;
  status: string;
  reason: string | null;
  signature: string | null;
  targetObservedToSubmitMs: number;
  targetBlockTimeToSubmitMs: number | null;
  targetSlot: number | null;
  copySlot: number | null;
  slotDelta: number | null;
  buildMs: number | null;
  sendMs: number | null;
  winnerProvider: string | null;
  sendRpcWinner: string | null;
  sendRpcCount: number | null;
}

export interface CopyTradeLatencyTracker {
  mark(milestone: CopyTradeLatencyMilestone, details?: CopyTradeLatencyMilestoneDetails): CopyTradeLatencyTrace;
  skip(reason: string, details?: Omit<CopyTradeLatencyMilestoneDetails, "reason">): CopyTradeLatencyTrace;
  snapshot(): CopyTradeLatencyTrace;
  format(details?: CopyTradeLatencyMilestoneDetails): CopyTradeLatencyLogMetadata;
}

export function createCopyTradeLatencyClock({
  receivedAtMs,
  normalizedAtMs,
  fallbackClock = Date.now
}: {
  receivedAtMs?: number;
  normalizedAtMs?: number;
  fallbackClock?: CopyTradeLatencyClock;
}): CopyTradeLatencyClock {
  const fixedTimes = [receivedAtMs, normalizedAtMs].filter((value): value is number => typeof value === "number");

  return () => fixedTimes.shift() ?? fallbackClock();
}

function normalizeTimestampMs(value: number): number {
  return Number.isFinite(value) ? value : 0;
}

function normalizeText(value: string | number | null | undefined): string | null {
  if (value === null || value === undefined) {
    return null;
  }

  const normalized = String(value).trim();
  return normalized ? normalized : null;
}

function normalizeContext(context: CopyTradeLatencyContext): CopyTradeLatencyContext {
  return {
    chatId: normalizeText(context.chatId),
    sourceWallet: normalizeText(context.sourceWallet),
    tradingWallet: normalizeText(context.tradingWallet),
    observedSignature: normalizeText(context.observedSignature),
    mint: normalizeText(context.mint),
    mode: context.mode
  };
}

function normalizeDetails(details: CopyTradeLatencyMilestoneDetails | undefined): CopyTradeLatencyMilestoneDetails {
  return {
    status: normalizeText(details?.status),
    reason: normalizeText(details?.reason),
    signature: normalizeText(details?.signature)
  };
}

function durationMs(startMs: number, endMs: number): number {
  return Math.max(0, Math.round(endMs - startMs));
}

function upsertMilestone(
  milestones: CopyTradeLatencyMilestoneRecord[],
  nextMilestone: CopyTradeLatencyMilestoneRecord
): CopyTradeLatencyMilestoneRecord[] {
  const existingIndex = milestones.findIndex(({ milestone }) => milestone === nextMilestone.milestone);
  if (existingIndex === -1) {
    return [...milestones, nextMilestone];
  }

  return milestones.map((milestone, index) => (index === existingIndex ? nextMilestone : milestone));
}

function copyTrace(trace: CopyTradeLatencyTrace): CopyTradeLatencyTrace {
  return {
    context: { ...trace.context },
    startedAtMs: trace.startedAtMs,
    milestones: trace.milestones.map((milestone) => ({ ...milestone }))
  };
}

export function createCopyTradeLatencyTrace({
  context,
  nowMs
}: {
  context: CopyTradeLatencyContext;
  nowMs: number;
}): CopyTradeLatencyTrace {
  const startedAtMs = normalizeTimestampMs(nowMs);

  return {
    context: normalizeContext(context),
    startedAtMs,
    milestones: [
      {
        milestone: "received",
        atMs: startedAtMs
      }
    ]
  };
}

export function recordCopyTradeLatencyMilestone({
  trace,
  milestone,
  nowMs,
  details
}: {
  trace: CopyTradeLatencyTrace;
  milestone: CopyTradeLatencyMilestone;
  nowMs: number;
  details?: CopyTradeLatencyMilestoneDetails;
}): CopyTradeLatencyTrace {
  const normalizedDetails = normalizeDetails(details);

  return {
    context: { ...trace.context },
    startedAtMs: trace.startedAtMs,
    milestones: upsertMilestone(trace.milestones, {
      milestone,
      atMs: normalizeTimestampMs(nowMs),
      ...normalizedDetails
    })
  };
}

function stageDurations(milestones: CopyTradeLatencyMilestoneRecord[]): Record<string, number> {
  const stagesMs: Record<string, number> = {};

  for (let index = 1; index < milestones.length; index += 1) {
    const previous = milestones[index - 1];
    const current = milestones[index];
    stagesMs[`${previous.milestone}_to_${current.milestone}`] = durationMs(previous.atMs, current.atMs);
  }

  return stagesMs;
}

function lastMilestone(trace: CopyTradeLatencyTrace): CopyTradeLatencyMilestoneRecord {
  return trace.milestones[trace.milestones.length - 1] || {
    milestone: "received",
    atMs: trace.startedAtMs
  };
}

function finiteNumber(value: number | null | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function stageDuration(stagesMs: Record<string, number>, key: string): number | null {
  return finiteNumber(stagesMs[key]);
}

export function formatCopyTradeLatencyLog(
  trace: CopyTradeLatencyTrace,
  details?: CopyTradeLatencyMilestoneDetails
): CopyTradeLatencyLogMetadata {
  const latest = lastMilestone(trace);
  const normalizedDetails = normalizeDetails(details);
  const status = normalizedDetails.status || latest.status || latest.milestone;
  const reason = normalizedDetails.reason || latest.reason || null;
  const signature = normalizedDetails.signature || latest.signature || null;

  return {
    event: "copy_trade_latency",
    chatId: normalizeText(trace.context.chatId),
    sourceWallet: normalizeText(trace.context.sourceWallet),
    tradingWallet: normalizeText(trace.context.tradingWallet),
    observedSignature: normalizeText(trace.context.observedSignature),
    mint: normalizeText(trace.context.mint),
    mode: trace.context.mode,
    status,
    reason,
    signature,
    totalMs: durationMs(trace.startedAtMs, latest.atMs),
    stagesMs: stageDurations(trace.milestones)
  };
}

export function formatCopyTradeLatencySummary(
  trace: CopyTradeLatencyTrace,
  details?: CopyTradeLatencyMilestoneDetails,
  update: CopyTradeLatencySummaryUpdate = {}
): CopyTradeLatencySummaryMetadata {
  const log = formatCopyTradeLatencyLog(trace, details);
  const latest = lastMilestone(trace);
  const targetTimestamp = finiteNumber(update.targetTimestamp);
  const targetSlot = finiteNumber(update.targetSlot);
  const copySlot = finiteNumber(update.copySlot);
  const slotDelta = targetSlot !== null && copySlot !== null ? copySlot - targetSlot : null;
  const targetBlockTimeToSubmitMs = targetTimestamp === null
    ? null
    : durationMs(targetTimestamp * 1000, latest.atMs);
  const buildMs = stageDuration(log.stagesMs, "direct_build_started_to_direct_build_finished");
  const sendMs = stageDuration(log.stagesMs, "direct_raw_send_started_to_direct_raw_signature_returned") ??
    stageDuration(log.stagesMs, "submit_started_to_submit_finished");

  return {
    event: "copy_trade_latency_summary",
    chatId: log.chatId,
    sourceWallet: log.sourceWallet,
    tradingWallet: log.tradingWallet,
    observedSignature: log.observedSignature,
    mint: log.mint,
    mode: log.mode,
    status: log.status,
    reason: log.reason,
    signature: log.signature,
    targetObservedToSubmitMs: log.totalMs,
    targetBlockTimeToSubmitMs,
    targetSlot,
    copySlot,
    slotDelta,
    buildMs,
    sendMs,
    winnerProvider: normalizeText(update.winnerProvider),
    sendRpcWinner: normalizeText(update.sendRpcWinner),
    sendRpcCount: finiteNumber(update.sendRpcCount)
  };
}

export function createCopyTradeLatencyTracker(
  context: CopyTradeLatencyContext,
  options: { clock?: CopyTradeLatencyClock } = {}
): CopyTradeLatencyTracker {
  const clock = options.clock || Date.now;
  let trace = createCopyTradeLatencyTrace({
    context,
    nowMs: clock()
  });

  return {
    mark(milestone: CopyTradeLatencyMilestone, details?: CopyTradeLatencyMilestoneDetails): CopyTradeLatencyTrace {
      trace = recordCopyTradeLatencyMilestone({
        trace,
        milestone,
        nowMs: clock(),
        details
      });

      return this.snapshot();
    },
    skip(reason: string, details?: Omit<CopyTradeLatencyMilestoneDetails, "reason">): CopyTradeLatencyTrace {
      return this.mark("skipped", {
        ...details,
        reason
      });
    },
    snapshot(): CopyTradeLatencyTrace {
      return copyTrace(trace);
    },
    format(details?: CopyTradeLatencyMilestoneDetails): CopyTradeLatencyLogMetadata {
      return formatCopyTradeLatencyLog(trace, details);
    }
  };
}
