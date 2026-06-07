import type { WalletTradeData } from "./types.js";

export type CopyTradeSignalProvider = "pumpportal" | "parallel" | "shredstream" | "all";
export type CopyTradeSignalSource = Extract<WalletTradeData["provider"], "pumpportal" | "geyser" | "shredstream">;
export type CopyTradeSignalRaceOutcome = "won" | "duplicate" | "skipped";

export interface CopyTradeSignalRaceRecord {
  key: string;
  provider: CopyTradeSignalSource;
  signature: string;
  targetWallet: string;
  mint: string;
  claimedAtMs: number;
}

export interface CopyTradeSignalRaceClaim {
  outcome: "won" | "duplicate";
  key: string | null;
  record: CopyTradeSignalRaceRecord | null;
}

export interface CopyTradeSignalRaceLogInput {
  mode: CopyTradeSignalProvider;
  trade: WalletTradeData;
  outcome: CopyTradeSignalRaceOutcome;
  reason?: string | null;
  winner?: CopyTradeSignalRaceRecord | null;
  key?: string | null;
  receivedAtMs?: number | null;
  normalizedAtMs?: number | null;
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(9)));
}

function formatSeconds(ms: number): string {
  return formatNumber(ms / 1000);
}

function tradeTimestampMs(trade: WalletTradeData): number | null {
  if (!Number.isFinite(trade.timestamp)) {
    return null;
  }

  const timestamp = Number(trade.timestamp);
  return timestamp > 10_000_000_000 ? timestamp : timestamp * 1000;
}

export function parseCopyTradeSignalProvider(value: string | undefined): CopyTradeSignalProvider {
  const normalized = value?.trim().toLowerCase();

  if (normalized === "parallel" || normalized === "geyser") {
    return "parallel";
  }

  if (normalized === "shredstream" || normalized === "shred") {
    return "shredstream";
  }

  if (normalized === "all") {
    return "all";
  }

  return "pumpportal";
}

export function copyTradeSignalProviderAllows(
  provider: CopyTradeSignalProvider,
  source: CopyTradeSignalSource
): boolean {
  if (provider === "all") {
    return true;
  }

  if (source === "pumpportal") {
    return provider === "pumpportal" || provider === "parallel";
  }

  if (provider === "parallel") {
    return source === "geyser";
  }

  if (provider === "shredstream") {
    return source === "shredstream";
  }

  return false;
}

export function copyTradeSignalSource(value: WalletTradeData["provider"]): CopyTradeSignalSource | null {
  return value === "pumpportal" || value === "geyser" || value === "shredstream" ? value : null;
}

export function copyTradeSignalProviderRaces(provider: CopyTradeSignalProvider): boolean {
  return provider !== "pumpportal";
}

function normalizeSource(value: string | null | undefined): string | null {
  const normalized = value?.trim().toUpperCase();
  return normalized ? normalized : null;
}

export function copyTradeSignalSourceBlockedReason({
  trade,
  allowedSources
}: {
  trade: WalletTradeData;
  allowedSources: string[];
}): string | null {
  if (allowedSources.length === 0) {
    return null;
  }

  const source = normalizeSource(trade.source || trade.pool);
  const normalizedAllowedSources = allowedSources
    .map((value) => normalizeSource(value))
    .filter((value): value is string => Boolean(value));

  if (!source || !normalizedAllowedSources.includes(source)) {
    return `copy trade source ${source || "unknown"} is not in COPY_TRADE_ALLOWED_SOURCES=${normalizedAllowedSources.join(",")}`;
  }

  return null;
}

export function copyTradeSignalRaceKey(trade: WalletTradeData): string | null {
  const signature = trade.signature?.trim();
  const targetWallet = trade.targetWallet?.trim();
  const mint = trade.mint?.trim();

  if (!signature || !targetWallet || !mint) {
    return null;
  }

  return [signature, targetWallet, mint].join(":");
}

export function copyTradeSignalAgeBlockedReason({
  trade,
  maxSignalAgeMs,
  nowMs
}: {
  trade: WalletTradeData;
  maxSignalAgeMs: number;
  nowMs: number;
}): string | null {
  if (!Number.isFinite(maxSignalAgeMs) || maxSignalAgeMs <= 0) {
    return null;
  }

  const timestampMs = tradeTimestampMs(trade);
  if (timestampMs === null) {
    return `observed trade timestamp is missing; cannot enforce COPY_TRADE_MAX_SIGNAL_AGE_MS=${formatNumber(maxSignalAgeMs)}`;
  }

  const ageMs = nowMs - timestampMs;
  if (ageMs > maxSignalAgeMs) {
    return `observed trade signal is ${formatSeconds(ageMs)}s old, exceeding COPY_TRADE_MAX_SIGNAL_AGE_MS=${formatNumber(maxSignalAgeMs)}`;
  }

  return null;
}

export function createCopyTradeSignalRaceTracker({ maxEntries = 1000 }: { maxEntries?: number } = {}) {
  const records = new Map<string, CopyTradeSignalRaceRecord>();

  function prune(): void {
    while (records.size > maxEntries) {
      const oldest = records.keys().next().value;
      if (!oldest) {
        return;
      }

      records.delete(oldest);
    }
  }

  return {
    claim(trade: WalletTradeData, nowMs = Date.now()): CopyTradeSignalRaceClaim {
      const key = copyTradeSignalRaceKey(trade);
      const provider = copyTradeSignalSource(trade.provider);

      if (!key || !provider || !trade.signature || !trade.mint) {
        return {
          outcome: "won",
          key,
          record: null
        };
      }

      const existing = records.get(key);
      if (existing) {
        return {
          outcome: "duplicate",
          key,
          record: existing
        };
      }

      const record = {
        key,
        provider,
        signature: trade.signature,
        targetWallet: trade.targetWallet,
        mint: trade.mint,
        claimedAtMs: nowMs
      };
      records.set(key, record);
      prune();

      return {
        outcome: "won",
        key,
        record
      };
    },
    size(): number {
      return records.size;
    }
  };
}

export function copyTradeSignalRaceLogPayload({
  mode,
  trade,
  outcome,
  reason = null,
  winner = null,
  key = copyTradeSignalRaceKey(trade),
  receivedAtMs = null,
  normalizedAtMs = null
}: CopyTradeSignalRaceLogInput): Record<string, unknown> {
  const timestampMs = tradeTimestampMs(trade);
  const signalAgeMs = typeof receivedAtMs === "number" && timestampMs !== null
    ? Math.max(0, receivedAtMs - timestampMs)
    : null;
  const normalizedLagMs = typeof receivedAtMs === "number" && typeof normalizedAtMs === "number"
    ? Math.max(0, normalizedAtMs - receivedAtMs)
    : null;
  const winnerDeltaMs = typeof receivedAtMs === "number" && winner?.claimedAtMs
    ? receivedAtMs - winner.claimedAtMs
    : null;

  return {
    event: "copy_trade_signal_race",
    mode,
    provider: trade.provider,
    observedSignature: trade.signature,
    targetWallet: trade.targetWallet,
    mint: trade.mint,
    outcome,
    reason,
    raceKey: key,
    winnerProvider: winner?.provider || null,
    winnerClaimedAtMs: winner?.claimedAtMs || null,
    receivedAtMs,
    normalizedAtMs,
    signalAgeMs,
    normalizedLagMs,
    winnerDeltaMs
  };
}
