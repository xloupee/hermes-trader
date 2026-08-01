export type FeedKey =
  | "vortex"
  | "jito"
  | "erpc"
  | "shred-union"
  | "everstake"
  | "doublezero"
  | "on-chain"
  | "unknown";

export interface FeedIdentity {
  key: FeedKey;
  label: string;
}

export interface FeedStanding extends FeedIdentity {
  wins: number;
  share: number;
}

const FEED_LABELS: Record<FeedKey, string> = {
  vortex: "Vortex",
  jito: "Jito",
  erpc: "eRPC",
  "shred-union": "Shred union",
  everstake: "Everstake",
  doublezero: "DoubleZero",
  "on-chain": "On-chain",
  unknown: "Unknown"
};

const FEED_ORDER: FeedKey[] = [
  "vortex",
  "jito",
  "erpc",
  "shred-union",
  "everstake",
  "doublezero",
  "on-chain",
  "unknown"
];

export function feedIdentity(value: string | null | undefined): FeedIdentity {
  const normalized = value?.trim().toLowerCase() || "";
  let key: FeedKey = "unknown";

  if (normalized.includes("vortex")) key = "vortex";
  else if (normalized.includes("erpc")) key = "erpc";
  else if (normalized.includes("shred-union") || normalized.includes("raw-union")) key = "shred-union";
  else if (normalized.includes("jito") || normalized.includes("shredstream")) key = "jito";
  else if (normalized.includes("everstake")) key = "everstake";
  else if (normalized.includes("doublezero")) key = "doublezero";
  else if (normalized.includes("on_chain") || normalized.includes("confirmed_rpc")) key = "on-chain";

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
  source: string | null | undefined,
  provider: string | null | undefined
): FeedIdentity {
  const sourceFeed = feedIdentity(source);
  return sourceFeed.key !== "unknown" ? sourceFeed : feedIdentity(provider);
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
