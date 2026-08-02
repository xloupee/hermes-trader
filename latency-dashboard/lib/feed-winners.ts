export const RECOGNIZED_FEED_SOURCES = [
  "jito-primary",
  "doublezero-leader",
  "doublezero-retransmit-eu",
  "vortex-fra"
] as const;

export type RecognizedFeedSource = (typeof RECOGNIZED_FEED_SOURCES)[number];
export type FeedKey = RecognizedFeedSource | "unknown";

export interface FeedIdentity {
  key: FeedKey;
  label: string;
}

export interface FeedStanding extends FeedIdentity {
  wins: number;
  share: number;
}

export interface InboundFeedAttribution {
  inboundSource: string | null;
  inboundContributors: string[];
  inboundSelectionGeneration: number | null;
}

export function isLandedBuy(row: { observedAction?: string | null; outcome?: string | null }): boolean {
  return row.observedAction?.toLowerCase() === "buy" && row.outcome === "landed";
}

const RECOGNIZED_FEED_SOURCE_SET = new Set<string>(RECOGNIZED_FEED_SOURCES);

const FEED_LABELS: Record<FeedKey, string> = {
  "jito-primary": "Jito primary",
  "doublezero-leader": "DoubleZero leader",
  "doublezero-retransmit-eu": "DoubleZero retransmit EU",
  "vortex-fra": "Vortex FRA",
  unknown: "Unknown"
};

const FEED_ORDER: FeedKey[] = [...RECOGNIZED_FEED_SOURCES, "unknown"];

export function recognizedFeedSource(value: unknown): RecognizedFeedSource | null {
  return typeof value === "string" && RECOGNIZED_FEED_SOURCE_SET.has(value)
    ? value as RecognizedFeedSource
    : null;
}

export function recognizedFeedSourcesMatchingFilter(value: string): RecognizedFeedSource[] {
  const normalized = value.trim().toLowerCase();
  if (!normalized) return [...RECOGNIZED_FEED_SOURCES];
  return RECOGNIZED_FEED_SOURCES.filter((source) => source.includes(normalized));
}

export function dashboardInboundSourcePredicate(sourceFilter: string): string {
  const candidates = recognizedFeedSourcesMatchingFilter(sourceFilter);
  const rawInbound = "raw_execution->executionTelemetry->inbound";
  const confirmationInbound = "raw_execution->rustTransactionConfirmation->executionTelemetry->inbound";
  const chainInbound = "chain_report->executionTelemetry->inbound";

  if (candidates.length === 0) {
    if (sourceFilter.trim().toLowerCase() !== "unknown") return "source.eq.__no_typed_feed_match__";
    const recognized = `(${RECOGNIZED_FEED_SOURCES.join(",")})`;
    return [
      `and(${rawInbound}.not.is.null,or(${rawInbound}->>selectedSource.is.null,${rawInbound}->>selectedSource.not.in.${recognized}))`,
      `and(${rawInbound}.is.null,${confirmationInbound}.not.is.null,or(${confirmationInbound}->>selectedSource.is.null,${confirmationInbound}->>selectedSource.not.in.${recognized}))`,
      `and(${rawInbound}.is.null,${confirmationInbound}.is.null,${chainInbound}.not.is.null,or(${chainInbound}->>selectedSource.is.null,${chainInbound}->>selectedSource.not.in.${recognized}))`,
      `and(${rawInbound}.is.null,${confirmationInbound}.is.null,${chainInbound}.is.null,source.not.in.${recognized})`
    ].join(",");
  }

  const predicates: string[] = [];
  const canonicalContributors = `[${RECOGNIZED_FEED_SOURCES.map((source) => `"${source}"`).join(",")}]`;
  const validInbound = (path: string, source: RecognizedFeedSource) => [
    `${path}->schemaVersion.eq.1`,
    `${path}->>selectedSource.eq.${source}`,
    `${path}->contributors.cs.["${source}"]`,
    `${path}->contributors.cd.${canonicalContributors}`,
    `${path}->selectionGeneration.gt.0`,
    `${path}->selectionGeneration.lt.${Number.MAX_SAFE_INTEGER + 1}`,
    `${path}->>selectionGeneration.not.like.*.*`
  ].join(",");
  for (const source of candidates) {
    predicates.push(`and(${validInbound(rawInbound, source)})`);
    predicates.push(`and(${rawInbound}.is.null,${validInbound(confirmationInbound, source)})`);
    predicates.push(`and(${rawInbound}.is.null,${confirmationInbound}.is.null,${validInbound(chainInbound, source)})`);
    predicates.push(`and(${rawInbound}.is.null,${confirmationInbound}.is.null,${chainInbound}.is.null,source.eq.${source})`);
  }
  return predicates.join(",");
}

export function normalizeFeedContributors(value: unknown): string[] {
  return Array.isArray(value) && value.every((item) => recognizedFeedSource(item) !== null)
    ? [...value] as string[]
    : [];
}

function recordValue(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function hasOwn(value: Record<string, unknown> | null, key: string): boolean {
  return value !== null && Object.prototype.hasOwnProperty.call(value, key);
}

function validatedInboundAttribution(value: unknown): InboundFeedAttribution | null {
  const inbound = recordValue(value);
  if (!inbound || inbound.schemaVersion !== 1) return null;

  const selectedSource = recognizedFeedSource(inbound.selectedSource);
  if (!selectedSource) return null;

  if (!Array.isArray(inbound.contributors) || inbound.contributors.length === 0) return null;
  const contributors = normalizeFeedContributors(inbound.contributors);
  if (contributors.length !== inbound.contributors.length || !contributors.includes(selectedSource)) return null;

  const selectionGeneration = inbound.selectionGeneration;
  if (!Number.isSafeInteger(selectionGeneration) || (selectionGeneration as number) <= 0) return null;

  return {
    inboundSource: selectedSource,
    inboundContributors: contributors,
    inboundSelectionGeneration: selectionGeneration as number
  };
}

export function normalizeInboundFeedAttribution(
  rawExecutionValue: unknown,
  chainReportValue: unknown,
  legacySource: unknown
): InboundFeedAttribution {
  const rawExecution = recordValue(rawExecutionValue);
  const confirmation = recordValue(rawExecution?.rustTransactionConfirmation);
  const telemetryCandidates = [
    recordValue(rawExecution?.executionTelemetry),
    recordValue(confirmation?.executionTelemetry),
    recordValue(recordValue(chainReportValue)?.executionTelemetry)
  ];

  for (const telemetry of telemetryCandidates) {
    if (!hasOwn(telemetry, "inbound")) continue;
    return validatedInboundAttribution(telemetry?.inbound) ?? {
      inboundSource: null,
      inboundContributors: [],
      inboundSelectionGeneration: null
    };
  }

  return {
    inboundSource: recognizedFeedSource(legacySource),
    inboundContributors: [],
    inboundSelectionGeneration: null
  };
}

export function feedIdentity(value: string | null | undefined): FeedIdentity {
  const key = recognizedFeedSource(value) ?? "unknown";
  return { key, label: FEED_LABELS[key] };
}

export function feedTransportLabel(
  source: string | null | undefined,
  provider: string | null | undefined
): string | null {
  const normalized = `${source || ""} ${provider || ""}`.toLowerCase();
  return normalized.includes("jito") || normalized.includes("shredstream") ? "ShredStream" : null;
}

export function executionFeed(
  inboundSource: string | null | undefined,
  _provider?: string | null | undefined
): FeedIdentity {
  return feedIdentity(inboundSource);
}

export function feedLeaderboard(sources: Array<string | null | undefined>): FeedStanding[] {
  const counts = new Map<FeedKey, number>();
  for (const source of sources) {
    const feed = feedIdentity(source);
    counts.set(feed.key, (counts.get(feed.key) || 0) + 1);
  }

  const total = sources.length;
  return [...counts.entries()]
    .map(([key, wins]) => ({
      key,
      label: FEED_LABELS[key],
      wins,
      share: total === 0 ? 0 : (wins / total) * 100
    }))
    .sort((left, right) => right.wins - left.wins || FEED_ORDER.indexOf(left.key) - FEED_ORDER.indexOf(right.key));
}

export function executionEvidenceCounts(
  sources: Array<string | null | undefined>
): Map<FeedKey, number> {
  const counts = new Map<FeedKey, number>();
  for (const source of sources) {
    const feed = feedIdentity(source);
    counts.set(feed.key, (counts.get(feed.key) || 0) + 1);
  }
  return counts;
}
