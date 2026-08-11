"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import {
  type DashboardExecutionsResponse,
  toQueryParams
} from "@/lib/dashboard-client";
import { useDashboardFilters } from "./use-dashboard-filters";
import { DashboardFiltersPanel } from "@/components/dashboard/dashboard-filters";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import { ExecutionTable } from "@/components/dashboard/execution-table";
import { FeedLeaderboard } from "@/components/dashboard/feed-leaderboard";
import styles from "@/components/dashboard/dashboard-shared.module.css";

export function OverviewDashboard() {
  const { filters, setFilters, setOutcome } = useDashboardFilters();
  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery<DashboardExecutionsResponse>(
    async (): Promise<DashboardExecutionsResponse> => {
      const tableQuery = toQueryParams(filters, true);
      const response = await fetch(`/api/dashboard/executions?${tableQuery}`);
      if (!response.ok) throw new Error("failed executions query");
      return response.json() as Promise<DashboardExecutionsResponse>;
    },
    { intervalMs: 15000 }
  );

  return (
    <section className={`${styles.tapePage} ${styles.compactTapePage}`}>
      <FeedLeaderboard rows={data?.executions ?? []} />
      <DashboardFiltersPanel filters={filters} onFiltersChange={setFilters} onOutcomeChange={setOutcome} />
      <DashboardRefreshToolbar
        loading={loading}
        error={error}
        paused={paused}
        autoPaused={autoPaused}
        lastUpdated={lastUpdated}
        freshness={data?.freshness}
        onRefresh={refresh}
        onTogglePause={setPaused}
      />
      <ExecutionTable
        rows={data?.executions ?? []}
        gatewayRows={data?.gatewayConfirmations ?? []}
        includeRowLinks
        emptyMessage="No executions match these filters. Clear a filter or choose All tape."
      />
    </section>
  );
}
