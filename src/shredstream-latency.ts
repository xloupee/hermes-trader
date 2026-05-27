export type DiscoverySource = "pumpportal" | "shredstream";
export type DiscoverySourceWinner = DiscoverySource | "tie";

export interface DiscoveryLatencyEvent {
  source: DiscoverySource;
  signature?: string | null;
  instructionIndex?: number | null;
  mint?: string | null;
  receivedAtMs: number;
  slot?: number | null;
  programId?: string | null;
  eventType?: string | null;
  decodeStatus?: string | null;
}

export interface DiscoveryLatencyComparison {
  mint: string | null;
  signature: string | null;
  slot: number | null;
  pumpportal_received_at_ms: number;
  shredstream_received_at_ms: number;
  shred_minus_pumpportal_ms: number;
  source_winner: DiscoverySourceWinner;
  decode_status: string | null;
  program_id: string | null;
  event_type: string | null;
}

export interface DiscoveryLatencySummary {
  matchedCount: number;
  missingPumpPortalCount: number;
  missingShredstreamCount: number;
  shredWins: number;
  pumpPortalWins: number;
  ties: number;
  p50DeltaMs: number | null;
  p90DeltaMs: number | null;
  p99DeltaMs: number | null;
}

export function compareDiscoveryLatency({
  pumpPortalEvents,
  shredstreamEvents,
  createWindowMs = 2500
}: {
  pumpPortalEvents: DiscoveryLatencyEvent[];
  shredstreamEvents: DiscoveryLatencyEvent[];
  createWindowMs?: number;
}): DiscoveryLatencyComparison[] {
  const unusedPumpPortal = new Set(pumpPortalEvents.map((_, index) => index));
  const comparisons: DiscoveryLatencyComparison[] = [];

  for (const shredstreamEvent of shredstreamEvents) {
    const matchIndex = findPumpPortalMatch(shredstreamEvent, pumpPortalEvents, unusedPumpPortal, createWindowMs);

    if (matchIndex === null) {
      continue;
    }

    unusedPumpPortal.delete(matchIndex);
    comparisons.push(formatComparison(pumpPortalEvents[matchIndex], shredstreamEvent));
  }

  return comparisons;
}

export function summarizeDiscoveryLatency({
  pumpPortalEvents,
  shredstreamEvents,
  comparisons
}: {
  pumpPortalEvents: DiscoveryLatencyEvent[];
  shredstreamEvents: DiscoveryLatencyEvent[];
  comparisons: DiscoveryLatencyComparison[];
}): DiscoveryLatencySummary {
  const deltas = comparisons.map((comparison) => comparison.shred_minus_pumpportal_ms).sort((a, b) => a - b);
  const matchedCount = comparisons.length;

  return {
    matchedCount,
    missingPumpPortalCount: Math.max(0, shredstreamEvents.length - matchedCount),
    missingShredstreamCount: Math.max(0, pumpPortalEvents.length - matchedCount),
    shredWins: comparisons.filter((comparison) => comparison.source_winner === "shredstream").length,
    pumpPortalWins: comparisons.filter((comparison) => comparison.source_winner === "pumpportal").length,
    ties: comparisons.filter((comparison) => comparison.source_winner === "tie").length,
    p50DeltaMs: percentile(deltas, 0.5),
    p90DeltaMs: percentile(deltas, 0.9),
    p99DeltaMs: percentile(deltas, 0.99)
  };
}

function findPumpPortalMatch(
  shredstreamEvent: DiscoveryLatencyEvent,
  pumpPortalEvents: DiscoveryLatencyEvent[],
  unusedPumpPortal: Set<number>,
  createWindowMs: number
): number | null {
  return (
    findMatchBy(unusedPumpPortal, pumpPortalEvents, (pumpPortalEvent) =>
      Boolean(
        shredstreamEvent.signature &&
          pumpPortalEvent.signature === shredstreamEvent.signature &&
          pumpPortalEvent.instructionIndex === shredstreamEvent.instructionIndex
      )
    ) ??
    findMatchBy(unusedPumpPortal, pumpPortalEvents, (pumpPortalEvent) =>
      Boolean(
        shredstreamEvent.signature &&
          shredstreamEvent.mint &&
          pumpPortalEvent.signature === shredstreamEvent.signature &&
          pumpPortalEvent.mint === shredstreamEvent.mint
      )
    ) ??
    findMatchBy(unusedPumpPortal, pumpPortalEvents, (pumpPortalEvent) =>
      Boolean(
        shredstreamEvent.eventType === "create" &&
          pumpPortalEvent.eventType === "create" &&
          shredstreamEvent.mint &&
          pumpPortalEvent.mint === shredstreamEvent.mint &&
          Math.abs(pumpPortalEvent.receivedAtMs - shredstreamEvent.receivedAtMs) <= createWindowMs
      )
    )
  );
}

function findMatchBy(
  candidates: Set<number>,
  events: DiscoveryLatencyEvent[],
  predicate: (event: DiscoveryLatencyEvent) => boolean
): number | null {
  for (const index of candidates) {
    if (predicate(events[index])) {
      return index;
    }
  }

  return null;
}

function formatComparison(
  pumpPortalEvent: DiscoveryLatencyEvent,
  shredstreamEvent: DiscoveryLatencyEvent
): DiscoveryLatencyComparison {
  const delta = shredstreamEvent.receivedAtMs - pumpPortalEvent.receivedAtMs;

  return {
    mint: shredstreamEvent.mint || pumpPortalEvent.mint || null,
    signature: shredstreamEvent.signature || pumpPortalEvent.signature || null,
    slot: shredstreamEvent.slot ?? pumpPortalEvent.slot ?? null,
    pumpportal_received_at_ms: pumpPortalEvent.receivedAtMs,
    shredstream_received_at_ms: shredstreamEvent.receivedAtMs,
    shred_minus_pumpportal_ms: delta,
    source_winner: delta < 0 ? "shredstream" : delta > 0 ? "pumpportal" : "tie",
    decode_status: shredstreamEvent.decodeStatus || null,
    program_id: shredstreamEvent.programId || pumpPortalEvent.programId || null,
    event_type: shredstreamEvent.eventType || pumpPortalEvent.eventType || null
  };
}

function percentile(sortedValues: number[], fraction: number): number | null {
  if (sortedValues.length === 0) {
    return null;
  }

  const index = Math.min(sortedValues.length - 1, Math.ceil(sortedValues.length * fraction) - 1);
  return sortedValues[index];
}
