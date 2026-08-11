import type { DashboardSourceFilters } from "@/lib/dashboard-contract.mjs";
import { createAdminClient } from "@/lib/supabase/admin";
import { dashboardInboundSourcePredicate, normalizeInboundFeedAttribution } from "@/lib/feed-winners";

interface RawSourceRow {
  id: number;
  observed_at_ms: number;
  source: string;
  provider: string;
  observed_wallet: string;
  copy_wallet: string | null;
  selected_route: string;
  observed_action: string;
  raw_execution: unknown;
  chain_report: unknown;
}

export interface DashboardSourceRow {
  source: string;
  provider: string;
  count: number;
  latestObservedAtMs: number;
}

const PAGE_SIZE = 1000;

function sourceCursorWhere(row: RawSourceRow) {
  return `observed_at_ms.lt.${row.observed_at_ms},and(observed_at_ms.eq.${row.observed_at_ms},id.lt.${row.id})`;
}

export async function listDashboardSources(filters: DashboardSourceFilters): Promise<DashboardSourceRow[]> {
  const grouped = new Map<string, DashboardSourceRow>();
  let cursor: RawSourceRow | null = null;

  while (true) {
    let query = createAdminClient()
      .from("copytrade_local_executions")
      .select("id,observed_at_ms,source,provider,observed_wallet,copy_wallet,selected_route,observed_action,raw_execution,chain_report", { count: "exact" })
      .gte("observed_at_ms", filters.fromObservedAtMs)
      .lte("observed_at_ms", filters.toObservedAtMs)
      .order("observed_at_ms", { ascending: false })
      .order("id", { ascending: false })
      .limit(PAGE_SIZE);

    if (filters.provider) query = query.eq("provider", filters.provider);
    if (filters.source) query = query.or(dashboardInboundSourcePredicate(filters.source));
    if (filters.wallet) query = query.or(`observed_wallet.eq.${filters.wallet},copy_wallet.eq.${filters.wallet}`);
    if (filters.mint) query = query.ilike("mint", `%${filters.mint}%`);
    if (filters.route) query = query.eq("selected_route", filters.route);
    if (filters.side) query = query.eq("observed_action", filters.side);
    if (cursor) query = query.or(sourceCursorWhere(cursor));

    const { data, error, count } = await query;
    if (error) throw error;
    const rows = (((data as unknown) as RawSourceRow[] | null) || []);

    for (const row of rows) {
      const inboundSource = normalizeInboundFeedAttribution(row.raw_execution, row.chain_report, row.source).inboundSource ?? "unknown";
      const key = `${inboundSource}:${row.provider}`;
      const current = grouped.get(key);
      if (!current) {
        grouped.set(key, { source: inboundSource, provider: row.provider, count: 1, latestObservedAtMs: row.observed_at_ms });
      } else {
        current.count += 1;
        current.latestObservedAtMs = Math.max(current.latestObservedAtMs, row.observed_at_ms);
      }
    }

    if (rows.length === 0 || rows.length >= (count ?? rows.length)) break;
    cursor = rows[rows.length - 1];
  }

  return [...grouped.values()].sort((left, right) => right.latestObservedAtMs - left.latestObservedAtMs || left.source.localeCompare(right.source));
}
