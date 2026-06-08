import { createAdminClient } from "@/lib/supabase/admin";

export type TradeStatus = "submitted" | "failed" | "skipped" | "simulated" | "confirmed" | "expired" | string;

export interface TradeRow {
  id: number;
  createdAt: string;
  chatId: string;
  sourceWallet: string;
  sourceWalletLabel: string | null;
  tradingWallet: string;
  mint: string;
  action: string;
  amount: string;
  status: TradeStatus;
  signature: string | null;
  observedSignature: string | null;
  route: string | null;
  provider: string | null;
  targetObservedToSubmitMs: number | null;
  targetBlocktimeToSubmitMs: number | null;
  buildMs: number | null;
  sendMs: number | null;
  winnerProvider: string | null;
  sendRpcWinner: string | null;
  sendRpcCount: number | null;
  targetSlot: number | null;
  copySlot: number | null;
  slotDelta: number | null;
  errorText: string | null;
  observedTrade: unknown;
  request: unknown;
  response: unknown;
  latencySummary: unknown;
}

export interface TradeFilters {
  since: string;
  limit: number;
  status?: string | null;
  route?: string | null;
  provider?: string | null;
  chatId?: string | null;
  sourceWallet?: string | null;
  tradingWallet?: string | null;
  mint?: string | null;
  minLatencyMs?: number | null;
  maxLatencyMs?: number | null;
}

interface RawExecutionRow {
  id: number;
  created_at: string;
  chat_id: string;
  source_wallet_address: string;
  source_wallet_label: string | null;
  trading_wallet_public_key: string;
  mint: string;
  action: string;
  amount: string;
  status: string;
  signature: string | null;
  error_text: string | null;
  observed_trade: unknown;
  request: unknown;
  response: unknown;
  observed_signature?: string | null;
  target_observed_to_submit_ms?: number | null;
  target_blocktime_to_submit_ms?: number | null;
  build_ms?: number | null;
  send_ms?: number | null;
  winner_provider?: string | null;
  send_rpc_winner?: string | null;
  send_rpc_count?: number | null;
  target_slot?: number | null;
  copy_slot?: number | null;
  slot_delta?: number | null;
  latency_summary?: unknown;
}

const FULL_EXECUTION_SELECT = "id,created_at,chat_id,source_wallet_address,source_wallet_label,trading_wallet_public_key,mint,action,amount,status,signature,error_text,observed_trade,request,response,observed_signature,target_observed_to_submit_ms,target_blocktime_to_submit_ms,build_ms,send_ms,winner_provider,send_rpc_winner,send_rpc_count,target_slot,copy_slot,slot_delta,latency_summary";
const LEGACY_EXECUTION_SELECT = "id,created_at,chat_id,source_wallet_address,source_wallet_label,trading_wallet_public_key,mint,action,amount,status,signature,error_text,observed_trade,request,response";

function recordValue(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {};
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value : null;
}

function numberValue(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function missingColumnError(error: { message?: string } | null): boolean {
  const message = error?.message?.toLowerCase() || "";
  return message.includes("column") && (message.includes("does not exist") || message.includes("schema cache"));
}

function routeFromResponse(response: unknown): string | null {
  const responseRecord = recordValue(response);
  const metadata = recordValue(responseRecord.metadata);
  const route = recordValue(metadata.route);
  return stringValue(responseRecord.route) || stringValue(route.route) || stringValue(metadata.route);
}

function providerFromResponse(response: unknown): string | null {
  const responseRecord = recordValue(response);
  const metadata = recordValue(responseRecord.metadata);
  return stringValue(responseRecord.provider) || stringValue(metadata.requestedProvider);
}

function coerceDate(value: string): string {
  const match = value.match(/^(\d+)(h|d|m)$/);
  if (match) {
    const amount = Number(match[1]);
    const unit = match[2];
    const ms = unit === "h" ? amount * 60 * 60 * 1000 : unit === "d" ? amount * 24 * 60 * 60 * 1000 : amount * 60 * 1000;
    return new Date(Date.now() - ms).toISOString();
  }

  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? new Date(Date.now() - 7 * 24 * 60 * 60 * 1000).toISOString() : date.toISOString();
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

export function parseFilters(searchParams: URLSearchParams): TradeFilters {
  const limit = Number(searchParams.get("limit") || 100);
  return {
    since: coerceDate(searchParams.get("since") || "7d"),
    limit: Number.isFinite(limit) ? Math.min(Math.max(limit, 1), 500) : 100,
    status: optionalString(searchParams.get("status")),
    route: optionalString(searchParams.get("route")),
    provider: optionalString(searchParams.get("provider")),
    chatId: optionalString(searchParams.get("chatId")),
    sourceWallet: optionalString(searchParams.get("sourceWallet")),
    tradingWallet: optionalString(searchParams.get("tradingWallet")),
    mint: optionalString(searchParams.get("mint")),
    minLatencyMs: optionalNumber(searchParams.get("minLatencyMs")),
    maxLatencyMs: optionalNumber(searchParams.get("maxLatencyMs"))
  };
}

export function normalizeTrade(row: RawExecutionRow): TradeRow {
  const responseRecord = recordValue(row.response);
  const latencySummary = row.latency_summary ?? responseRecord.latencySummary ?? null;
  const latencyRecord = recordValue(latencySummary);
  const observedTrade = recordValue(row.observed_trade);

  return {
    id: row.id,
    createdAt: row.created_at,
    chatId: row.chat_id,
    sourceWallet: row.source_wallet_address,
    sourceWalletLabel: row.source_wallet_label,
    tradingWallet: row.trading_wallet_public_key,
    mint: row.mint,
    action: row.action,
    amount: row.amount,
    status: row.status,
    signature: row.signature,
    observedSignature: row.observed_signature ?? stringValue(latencyRecord.observedSignature) ?? stringValue(observedTrade.signature),
    route: routeFromResponse(row.response),
    provider: providerFromResponse(row.response),
    targetObservedToSubmitMs: row.target_observed_to_submit_ms ?? numberValue(latencyRecord.targetObservedToSubmitMs),
    targetBlocktimeToSubmitMs: row.target_blocktime_to_submit_ms ?? numberValue(latencyRecord.targetBlockTimeToSubmitMs),
    buildMs: row.build_ms ?? numberValue(latencyRecord.buildMs),
    sendMs: row.send_ms ?? numberValue(latencyRecord.sendMs),
    winnerProvider: row.winner_provider ?? stringValue(latencyRecord.winnerProvider),
    sendRpcWinner: row.send_rpc_winner ?? stringValue(latencyRecord.sendRpcWinner),
    sendRpcCount: row.send_rpc_count ?? numberValue(latencyRecord.sendRpcCount),
    targetSlot: row.target_slot ?? numberValue(latencyRecord.targetSlot),
    copySlot: row.copy_slot ?? numberValue(latencyRecord.copySlot),
    slotDelta: row.slot_delta ?? numberValue(latencyRecord.slotDelta),
    errorText: row.error_text,
    observedTrade: row.observed_trade,
    request: row.request,
    response: row.response,
    latencySummary
  };
}

function matchesPostFilters(row: TradeRow, filters: TradeFilters): boolean {
  if (filters.route && row.route !== filters.route) {
    return false;
  }
  if (filters.provider && row.provider !== filters.provider && row.winnerProvider !== filters.provider) {
    return false;
  }
  if (typeof filters.minLatencyMs === "number" && (row.targetObservedToSubmitMs ?? -1) < filters.minLatencyMs) {
    return false;
  }
  if (typeof filters.maxLatencyMs === "number" && (row.targetObservedToSubmitMs ?? Number.POSITIVE_INFINITY) > filters.maxLatencyMs) {
    return false;
  }
  return true;
}

export async function listTrades(filters: TradeFilters): Promise<TradeRow[]> {
  const supabase = createAdminClient();
  let query = supabase
    .from("telegram_copytrade_executions")
    .select(FULL_EXECUTION_SELECT)
    .gte("created_at", filters.since)
    .order("created_at", { ascending: false })
    .limit(filters.limit);

  if (filters.status) {
    query = query.eq("status", filters.status);
  }
  if (filters.chatId) {
    query = query.eq("chat_id", filters.chatId);
  }
  if (filters.sourceWallet) {
    query = query.ilike("source_wallet_address", `%${filters.sourceWallet}%`);
  }
  if (filters.tradingWallet) {
    query = query.ilike("trading_wallet_public_key", `%${filters.tradingWallet}%`);
  }
  if (filters.mint) {
    query = query.ilike("mint", `%${filters.mint}%`);
  }

  const result = await query;
  let rows = result.data as RawExecutionRow[] | null;
  let error = result.error;
  if (missingColumnError(error)) {
    let legacyQuery = supabase
      .from("telegram_copytrade_executions")
      .select(LEGACY_EXECUTION_SELECT)
      .gte("created_at", filters.since)
      .order("created_at", { ascending: false })
      .limit(filters.limit);

    if (filters.status) {
      legacyQuery = legacyQuery.eq("status", filters.status);
    }
    if (filters.chatId) {
      legacyQuery = legacyQuery.eq("chat_id", filters.chatId);
    }
    if (filters.sourceWallet) {
      legacyQuery = legacyQuery.ilike("source_wallet_address", `%${filters.sourceWallet}%`);
    }
    if (filters.tradingWallet) {
      legacyQuery = legacyQuery.ilike("trading_wallet_public_key", `%${filters.tradingWallet}%`);
    }
    if (filters.mint) {
      legacyQuery = legacyQuery.ilike("mint", `%${filters.mint}%`);
    }

    const legacyResult = await legacyQuery;
    rows = legacyResult.data as RawExecutionRow[] | null;
    error = legacyResult.error;
  }
  if (error) {
    throw error;
  }

  return (rows || [])
    .map(normalizeTrade)
    .filter((row) => matchesPostFilters(row, filters));
}

export async function getTrade(id: string): Promise<TradeRow | null> {
  const supabase = createAdminClient();
  const result = await supabase
    .from("telegram_copytrade_executions")
    .select(FULL_EXECUTION_SELECT)
    .eq("id", id)
    .maybeSingle();
  let row = result.data as RawExecutionRow | null;
  let error = result.error;

  if (missingColumnError(error)) {
    const legacyResult = await supabase
      .from("telegram_copytrade_executions")
      .select(LEGACY_EXECUTION_SELECT)
      .eq("id", id)
      .maybeSingle();
    row = legacyResult.data as RawExecutionRow | null;
    error = legacyResult.error;
  }

  if (error) {
    throw error;
  }

  return row ? normalizeTrade(row) : null;
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

export function summarizeTrades(rows: TradeRow[]) {
  const grouped = new Map<string, TradeRow[]>();
  for (const row of rows) {
    const key = `${row.route || "unknown"}:${row.winnerProvider || row.provider || "unknown"}:${row.status}`;
    grouped.set(key, [...(grouped.get(key) || []), row]);
  }

  return {
    total: rows.length,
    submitted: rows.filter((row) => ["submitted", "confirmed"].includes(row.status)).length,
    failed: rows.filter((row) => ["failed", "expired"].includes(row.status)).length,
    targetObserved: stats(rows.map((row) => row.targetObservedToSubmitMs)),
    blocktime: stats(rows.map((row) => row.targetBlocktimeToSubmitMs)),
    build: stats(rows.map((row) => row.buildMs)),
    send: stats(rows.map((row) => row.sendMs)),
    groups: [...grouped.entries()].map(([key, group]) => {
      const [route, provider, status] = key.split(":");
      return {
        route,
        provider,
        status,
        count: group.length,
        targetObserved: stats(group.map((row) => row.targetObservedToSubmitMs)),
        blocktime: stats(group.map((row) => row.targetBlocktimeToSubmitMs))
      };
    }).sort((left, right) => right.count - left.count)
  };
}
