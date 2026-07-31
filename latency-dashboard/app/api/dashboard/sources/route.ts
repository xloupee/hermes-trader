import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { createAdminClient } from "@/lib/supabase/admin";
import { parseExecutionFilters } from "@/lib/dashboard-contract.mjs";

interface RawSourceRow {
  id: number;
  created_at: string;
  observed_at_ms: number;
  source: string;
  provider: string;
}

interface DashboardSourceRow {
  source: string;
  provider: string;
  count: number;
  latestObservedAtMs: number;
}

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const filters = parseExecutionFilters(new URL(request.url).searchParams);
    const sinceObservedAtMs = new Date(filters.since).getTime();
    const queryBase = createAdminClient()
      .from("copytrade_signal_observations")
      .select("id,created_at,observed_at_ms,source,provider")
      .gte("observed_at_ms", sinceObservedAtMs);

    const query = filters.provider
      ? queryBase.eq("provider", filters.provider)
      : queryBase;

    const { data, error } = await query;
    if (error) {
      throw error;
    }

    const rows = (((data as unknown) as RawSourceRow[] | null) || []);
    const map = new Map<string, DashboardSourceRow>();

    for (const row of rows) {
      if (filters.source && !row.source.toLowerCase().includes(filters.source.toLowerCase())) {
        continue;
      }

      const key = `${row.source}:${row.provider}`;
      const current = map.get(key);
      if (!current) {
        map.set(key, {
          source: row.source,
          provider: row.provider,
          count: 1,
          latestObservedAtMs: row.observed_at_ms
        });
        continue;
      }

      current.count += 1;
      if (row.observed_at_ms > current.latestObservedAtMs) {
        current.latestObservedAtMs = row.observed_at_ms;
      }
    }

    return Response.json({
      sources: [...map.values()].sort((left, right) => right.latestObservedAtMs - left.latestObservedAtMs),
      filters: {
        ...filters,
        cursor: null
      }
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
