import { createAdminClient } from "@/lib/supabase/admin";

export interface SignalObservation {
  id: number;
  createdAt: string;
  provider: string;
  source: string;
  endpoint: string | null;
  targetWallet: string;
  signature: string;
  slot: number;
  action: string;
  mint: string;
  route: string;
  observedAtMs: number;
  grpcMessageReceivedAtMs: number | null;
  entriesDeserializedAtMs: number | null;
  tradeParsedAtMs: number | null;
  blockTimeMs: number | null;
  observedMinusBlockTimeMs: number | null;
  grpcReceivedMinusBlockTimeMs: number | null;
  deserializeMs: number | null;
  parseMs: number | null;
  localDetectMs: number | null;
  deserializeUs: number | null;
  parseUs: number | null;
  localDetectUs: number | null;
  batchTransactionCount: number | null;
  matchedTransactionIndex: number | null;
  batchScanUs: number | null;
  txParseUs: number | null;
  accountExpandUs: number | null;
  walletMatchUs: number | null;
  routeParseUs: number | null;
  solAmount: number | null;
  tokenAmount: number | null;
  copyable: boolean;
  rawEvent: unknown;
}

export interface SignalFilters {
  since: string;
  limit: number;
  provider?: string | null;
  targetWallet?: string | null;
  mint?: string | null;
  action?: string | null;
  route?: string | null;
  maxLagMs?: number | null;
}

interface RawSignalObservation {
  id: number;
  created_at: string;
  provider: string;
  source: string;
  endpoint: string | null;
  target_wallet: string;
  signature: string;
  slot: number;
  action: string;
  mint: string;
  route: string;
  observed_at_ms: number;
  grpc_message_received_at_ms?: number | null;
  entries_deserialized_at_ms?: number | null;
  trade_parsed_at_ms?: number | null;
  block_time_ms: number | null;
  observed_minus_block_time_ms: number | null;
  grpc_received_minus_block_time_ms?: number | null;
  deserialize_ms?: number | null;
  parse_ms?: number | null;
  local_detect_ms?: number | null;
  deserialize_us?: number | null;
  parse_us?: number | null;
  local_detect_us?: number | null;
  batch_transaction_count?: number | null;
  matched_transaction_index?: number | null;
  batch_scan_us?: number | null;
  tx_parse_us?: number | null;
  account_expand_us?: number | null;
  wallet_match_us?: number | null;
  route_parse_us?: number | null;
  sol_amount: number | null;
  token_amount: number | null;
  copyable: boolean;
  raw_event: unknown;
}

const SIGNAL_BASE_COLUMNS = [
  "id",
  "created_at",
  "provider",
  "source",
  "endpoint",
  "target_wallet",
  "signature",
  "slot",
  "action",
  "mint",
  "route",
  "observed_at_ms",
  "grpc_message_received_at_ms",
  "entries_deserialized_at_ms",
  "trade_parsed_at_ms",
  "block_time_ms",
  "observed_minus_block_time_ms",
  "grpc_received_minus_block_time_ms",
  "deserialize_ms",
  "parse_ms",
  "local_detect_ms",
  "deserialize_us",
  "parse_us",
  "local_detect_us",
  "batch_transaction_count",
  "matched_transaction_index",
  "batch_scan_us",
  "tx_parse_us",
  "account_expand_us",
  "wallet_match_us",
  "route_parse_us",
  "sol_amount",
  "token_amount",
  "copyable"
];

const SIGNAL_SELECT = SIGNAL_BASE_COLUMNS.join(",");
const SIGNAL_DETAIL_SELECT = [...SIGNAL_BASE_COLUMNS, "raw_event"].join(",");

function coerceDate(value: string): string {
  const match = value.match(/^(\d+)(h|d|m)$/);
  if (match) {
    const amount = Number(match[1]);
    const unit = match[2];
    const ms = unit === "h" ? amount * 60 * 60 * 1000 : unit === "d" ? amount * 24 * 60 * 60 * 1000 : amount * 60 * 1000;
    return new Date(Date.now() - ms).toISOString();
  }

  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString() : date.toISOString();
}

function optionalString(value: string | null): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function optionalNumber(value: string | null): number | null {
  const trimmed = value?.trim();
  if (!trimmed) {
    return null;
  }
  const numeric = Number(trimmed);
  return Number.isFinite(numeric) ? numeric : null;
}

export function parseSignalFilters(searchParams: URLSearchParams): SignalFilters {
  const limit = Number(searchParams.get("limit") || 150);
  return {
    since: coerceDate(searchParams.get("since") || "24h"),
    limit: Number.isFinite(limit) ? Math.min(Math.max(limit, 1), 500) : 150,
    provider: optionalString(searchParams.get("provider")),
    targetWallet: optionalString(searchParams.get("targetWallet")),
    mint: optionalString(searchParams.get("mint")),
    action: optionalString(searchParams.get("action")),
    route: optionalString(searchParams.get("route")),
    maxLagMs: optionalNumber(searchParams.get("maxLagMs"))
  };
}

function normalizeSignal(row: RawSignalObservation): SignalObservation {
  return {
    id: row.id,
    createdAt: row.created_at,
    provider: row.provider,
    source: row.source,
    endpoint: row.endpoint,
    targetWallet: row.target_wallet,
    signature: row.signature,
    slot: row.slot,
    action: row.action,
    mint: row.mint,
    route: row.route,
    observedAtMs: row.observed_at_ms,
    grpcMessageReceivedAtMs: row.grpc_message_received_at_ms ?? null,
    entriesDeserializedAtMs: row.entries_deserialized_at_ms ?? null,
    tradeParsedAtMs: row.trade_parsed_at_ms ?? null,
    blockTimeMs: row.block_time_ms,
    observedMinusBlockTimeMs: row.observed_minus_block_time_ms,
    grpcReceivedMinusBlockTimeMs: row.grpc_received_minus_block_time_ms ?? null,
    deserializeMs: row.deserialize_ms ?? null,
    parseMs: row.parse_ms ?? null,
    localDetectMs: row.local_detect_ms ?? null,
    deserializeUs: row.deserialize_us ?? null,
    parseUs: row.parse_us ?? null,
    localDetectUs: row.local_detect_us ?? null,
    batchTransactionCount: row.batch_transaction_count ?? null,
    matchedTransactionIndex: row.matched_transaction_index ?? null,
    batchScanUs: row.batch_scan_us ?? null,
    txParseUs: row.tx_parse_us ?? null,
    accountExpandUs: row.account_expand_us ?? null,
    walletMatchUs: row.wallet_match_us ?? null,
    routeParseUs: row.route_parse_us ?? null,
    solAmount: row.sol_amount,
    tokenAmount: row.token_amount,
    copyable: row.copyable,
    rawEvent: row.raw_event
  };
}

function matchesPostFilters(row: SignalObservation, filters: SignalFilters): boolean {
  if (typeof filters.maxLagMs === "number" && (row.observedMinusBlockTimeMs ?? Number.POSITIVE_INFINITY) > filters.maxLagMs) {
    return false;
  }
  return true;
}

export async function listSignals(filters: SignalFilters): Promise<SignalObservation[]> {
  const supabase = createAdminClient();
  let query = supabase
    .from("copytrade_signal_observations")
    .select(SIGNAL_SELECT)
    .gte("created_at", filters.since)
    .order("created_at", { ascending: false })
    .limit(filters.limit);

  if (filters.provider) {
    query = query.eq("provider", filters.provider);
  }
  if (filters.targetWallet) {
    query = query.ilike("target_wallet", `%${filters.targetWallet}%`);
  }
  if (filters.mint) {
    query = query.ilike("mint", `%${filters.mint}%`);
  }
  if (filters.action) {
    query = query.eq("action", filters.action);
  }
  if (filters.route) {
    query = query.eq("route", filters.route);
  }

  const { data, error } = await query;
  if (error) {
    throw error;
  }

  return (((data as unknown) as RawSignalObservation[] | null) || [])
    .map(normalizeSignal)
    .filter((row) => matchesPostFilters(row, filters));
}

export async function getSignalObservation(id: number): Promise<SignalObservation | null> {
  const { data, error } = await createAdminClient()
    .from("copytrade_signal_observations")
    .select(SIGNAL_DETAIL_SELECT)
    .eq("id", id)
    .maybeSingle();

  if (error) {
    throw error;
  }

  return data ? normalizeSignal(data as unknown as RawSignalObservation) : null;
}

function percentile(values: number[], p: number): number | null {
  if (values.length === 0) {
    return null;
  }
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[index] ?? null;
}

function stats(values: Array<number | null>) {
  const numeric = values.filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  if (numeric.length === 0) {
    return { count: 0, p50: null, p90: null, min: null, max: null, avg: null };
  }
  const total = numeric.reduce((sum, value) => sum + value, 0);
  return {
    count: numeric.length,
    p50: percentile(numeric, 50),
    p90: percentile(numeric, 90),
    min: Math.min(...numeric),
    max: Math.max(...numeric),
    avg: Math.round(total / numeric.length)
  };
}

export function summarizeSignals(rows: SignalObservation[]) {
  const grouped = new Map<string, SignalObservation[]>();
  for (const row of rows) {
    const key = `${row.provider}:${row.route}:${row.action}`;
    grouped.set(key, [...(grouped.get(key) || []), row]);
  }

  return {
    total: rows.length,
    buys: rows.filter((row) => row.action === "buy").length,
    sells: rows.filter((row) => row.action === "sell").length,
    copyable: rows.filter((row) => row.copyable).length,
    lag: stats(rows.map((row) => row.observedMinusBlockTimeMs)),
    grpcLag: stats(rows.map((row) => row.grpcReceivedMinusBlockTimeMs)),
    localDetect: stats(rows.map((row) => row.localDetectMs)),
    deserialize: stats(rows.map((row) => row.deserializeMs)),
    parse: stats(rows.map((row) => row.parseMs)),
    batchScan: stats(rows.map((row) => row.batchScanUs)),
    txParse: stats(rows.map((row) => row.txParseUs)),
    accountExpand: stats(rows.map((row) => row.accountExpandUs)),
    walletMatch: stats(rows.map((row) => row.walletMatchUs)),
    routeParse: stats(rows.map((row) => row.routeParseUs)),
    groups: [...grouped.entries()].map(([key, group]) => {
      const [provider, route, action] = key.split(":");
      return {
        provider,
        route,
        action,
        count: group.length,
        lag: stats(group.map((row) => row.observedMinusBlockTimeMs)),
        localDetect: stats(group.map((row) => row.localDetectMs))
      };
    }).sort((left, right) => right.count - left.count)
  };
}
