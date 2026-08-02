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

const RAW_INBOUND_PATH = "raw_execution->executionTelemetry->inbound";
const CONFIRMATION_INBOUND_PATH = "raw_execution->rustTransactionConfirmation->executionTelemetry->inbound";
const CHAIN_INBOUND_PATH = "chain_report->executionTelemetry->inbound";
const CANONICAL_CONTRIBUTORS_FILTER = `[${RECOGNIZED_FEED_SOURCES.map((source) => `"${source}"`).join(",")}]`;

function validInboundFilter(path: string, source: RecognizedFeedSource): string {
  return [
    `${path}->>schemaVersion.not.is.null`,
    `${path}->>selectedSource.not.is.null`,
    `${path}->>contributors.not.is.null`,
    `${path}->>selectionGeneration.not.is.null`,
    `${path}->schemaVersion.eq.1`,
    `${path}->>selectedSource.eq.${source}`,
    `${path}->contributors.cs.["${source}"]`,
    `${path}->contributors.cd.${CANONICAL_CONTRIBUTORS_FILTER}`,
    `${path}->selectionGeneration.gt.0`,
    `${path}->selectionGeneration.lt.${Number.MAX_SAFE_INTEGER + 1}`,
    `${path}->>selectionGeneration.not.like.*.*`
  ].join(",");
}

function invalidInboundFilter(path: string): string {
  const anyValidAttribution = RECOGNIZED_FEED_SOURCES
    .map((source) => `and(${validInboundFilter(path, source)})`)
    .join(",");
  return `not.or(${anyValidAttribution})`;
}

export function dashboardInboundSourcePredicate(sourceFilter: string): string {
  const candidates = recognizedFeedSourcesMatchingFilter(sourceFilter);

  if (candidates.length === 0) {
    if (sourceFilter.trim().toLowerCase() !== "unknown") return "source.eq.__no_typed_feed_match__";
    const recognized = `(${RECOGNIZED_FEED_SOURCES.join(",")})`;
    return [
      `and(${RAW_INBOUND_PATH}.not.is.null,${invalidInboundFilter(RAW_INBOUND_PATH)})`,
      `and(${RAW_INBOUND_PATH}.is.null,${CONFIRMATION_INBOUND_PATH}.not.is.null,${invalidInboundFilter(CONFIRMATION_INBOUND_PATH)})`,
      `and(${RAW_INBOUND_PATH}.is.null,${CONFIRMATION_INBOUND_PATH}.is.null,${CHAIN_INBOUND_PATH}.not.is.null,${invalidInboundFilter(CHAIN_INBOUND_PATH)})`,
      `and(${RAW_INBOUND_PATH}.is.null,${CONFIRMATION_INBOUND_PATH}.is.null,${CHAIN_INBOUND_PATH}.is.null,or(source.is.null,source.not.in.${recognized}))`
    ].join(",");
  }

  const predicates: string[] = [];
  for (const source of candidates) {
    predicates.push(`and(${validInboundFilter(RAW_INBOUND_PATH, source)})`);
    predicates.push(`and(${RAW_INBOUND_PATH}.is.null,${validInboundFilter(CONFIRMATION_INBOUND_PATH, source)})`);
    predicates.push(`and(${RAW_INBOUND_PATH}.is.null,${CONFIRMATION_INBOUND_PATH}.is.null,${validInboundFilter(CHAIN_INBOUND_PATH, source)})`);
    predicates.push(`and(${RAW_INBOUND_PATH}.is.null,${CONFIRMATION_INBOUND_PATH}.is.null,${CHAIN_INBOUND_PATH}.is.null,source.eq.${source})`);
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

export function dashboardInboundSourceFilterMatches(
  sourceFilter: string,
  rawExecutionValue: unknown,
  chainReportValue: unknown,
  legacySource: unknown
): boolean {
  const attribution = normalizeInboundFeedAttribution(rawExecutionValue, chainReportValue, legacySource);
  if (sourceFilter.trim().toLowerCase() === "unknown") return attribution.inboundSource === null;
  return attribution.inboundSource !== null
    && recognizedFeedSourcesMatchingFilter(sourceFilter).includes(attribution.inboundSource as RecognizedFeedSource);
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
