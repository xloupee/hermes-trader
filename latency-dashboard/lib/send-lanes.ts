export type SendLaneKey =
  | "helius-sender"
  | "nozomi"
  | "jito"
  | "erpc"
  | "astralane"
  | "lunar"
  | "circular"
  | "bloxroute"
  | "zero-slot"
  | "tpu"
  | "rpc"
  | "unknown";

export interface SendLaneIdentity {
  key: SendLaneKey;
  label: string;
  raw: string | null;
}

const LABELS: Record<SendLaneKey, string> = {
  "helius-sender": "Helius Sender",
  nozomi: "Nozomi",
  jito: "Jito",
  erpc: "eRPC",
  astralane: "Astralane",
  lunar: "Lunar",
  circular: "Circular",
  bloxroute: "Bloxroute",
  "zero-slot": "0slot",
  tpu: "TPU",
  rpc: "RPC",
  unknown: "Lane n/a"
};

export function sendLaneIdentity(value: string | null | undefined): SendLaneIdentity {
  const raw = value?.trim() || null;
  const normalized = raw?.toLowerCase() || "";
  let key: SendLaneKey = "unknown";

  if (normalized.includes("helius-sender") || normalized.includes("sender-max")) key = "helius-sender";
  else if (normalized.includes("nozomi")) key = "nozomi";
  else if (normalized.includes("jito")) key = "jito";
  else if (normalized.includes("erpc")) key = "erpc";
  else if (normalized.includes("astralane")) key = "astralane";
  else if (normalized.includes("lunar")) key = "lunar";
  else if (normalized.includes("circular")) key = "circular";
  else if (normalized.includes("bloxroute") || normalized.includes("beam-")) key = "bloxroute";
  else if (normalized.includes("zero-slot") || normalized.includes("0slot")) key = "zero-slot";
  else if (normalized.includes("tpu-")) key = "tpu";
  else if (normalized.includes("rpc")) key = "rpc";

  return { key, label: LABELS[key], raw };
}
