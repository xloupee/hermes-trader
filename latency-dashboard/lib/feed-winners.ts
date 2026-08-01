export type FeedKey =
  | "vortex"
  | "jito"
  | "erpc"
  | "shredstream"
  | "shred-union"
  | "everstake"
  | "doublezero"
  | "on-chain"
  | "unknown";

export interface FeedIdentity {
  key: FeedKey;
  label: string;
}

const FEED_LABELS: Record<FeedKey, string> = {
  vortex: "Vortex",
  jito: "Jito",
  erpc: "eRPC",
  shredstream: "ShredStream",
  "shred-union": "Shred union",
  everstake: "Everstake",
  doublezero: "DoubleZero",
  "on-chain": "On-chain",
  unknown: "Unknown"
};

export function feedIdentity(value: string | null | undefined): FeedIdentity {
  const normalized = value?.trim().toLowerCase() || "";
  let key: FeedKey = "unknown";

  if (normalized.includes("vortex")) key = "vortex";
  else if (normalized.includes("erpc")) key = "erpc";
  else if (normalized.includes("jito")) key = "jito";
  else if (normalized.includes("shred-union") || normalized.includes("raw-union")) key = "shred-union";
  else if (normalized.includes("shredstream")) key = "shredstream";
  else if (normalized.includes("everstake")) key = "everstake";
  else if (normalized.includes("doublezero")) key = "doublezero";
  else if (normalized.includes("on_chain") || normalized.includes("confirmed_rpc")) key = "on-chain";

  return { key, label: FEED_LABELS[key] };
}

export function executionFeed(
  source: string | null | undefined,
  provider: string | null | undefined
): FeedIdentity {
  const sourceFeed = feedIdentity(source);
  return sourceFeed.key !== "unknown" ? sourceFeed : feedIdentity(provider);
}
