"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import {
  type DashboardExecutionsResponse,
  type DashboardFilterState,
  type DashboardOverviewResponse,
  formatCount,
  overviewMetricValues,
  toQueryParams
} from "@/lib/dashboard-client";
import { useDashboardFilters } from "./use-dashboard-filters";
import { DashboardFiltersPanel } from "@/components/dashboard/dashboard-filters";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import { ExecutionTable } from "@/components/dashboard/execution-table";
import styles from "@/components/dashboard/dashboard-shared.module.css";

interface OverviewPayload {
  overview: DashboardOverviewResponse;
  landedBuys: DashboardOverviewResponse;
  landedSells: DashboardOverviewResponse;
  executions: DashboardExecutionsResponse;
}

function fetchOverview(query: string): Promise<DashboardOverviewResponse> {
  return fetch(`/api/dashboard/overview?${query}`).then((response) => {
    if (!response.ok) throw new Error("failed overview summary query");
    return response.json() as Promise<DashboardOverviewResponse>;
  });
}

export function OverviewDashboard() {
  const { filters, setFilters, setOutcome } = useDashboardFilters();
  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery<OverviewPayload>(
    async (): Promise<OverviewPayload> => {
      const tableQuery = toQueryParams(filters, true);
      const summaryFilters: DashboardFilterState = filters.outcome === "all"
        ? filters
        : { ...filters, action: "", outcome: "all" };
      const overviewQuery = toQueryParams(summaryFilters, true);
      const landedBuyQuery = toQueryParams({ ...summaryFilters, action: "buy", outcome: "landed-buys" }, true);
      const landedSellQuery = toQueryParams({ ...summaryFilters, action: "sell", outcome: "landed-sells" }, true);
      const [overview, landedBuys, landedSells, executions] = await Promise.all([
        fetchOverview(overviewQuery),
        fetchOverview(landedBuyQuery),
        fetchOverview(landedSellQuery),
        fetch(`/api/dashboard/executions?${tableQuery}`).then((response) => {
          if (!response.ok) throw new Error("failed executions query");
          return response.json() as Promise<DashboardExecutionsResponse>;
        })
      ]);
      return { overview, landedBuys, landedSells, executions };
    },
    { intervalMs: 15000 }
  );

  const metrics = data
    ? overviewMetricValues(data.overview.summary, data.landedBuys.summary, data.landedSells.summary)
    : null;

  return (
    <section>
      <div className={styles.dualLatency}>
        <div className={styles.metric}><span>landed buys</span><strong>{formatCount(metrics?.landedBuys)}</strong></div>
        <div className={styles.metric}><span>landed sells</span><strong>{formatCount(metrics?.landedSells)}</strong></div>
        <div className={styles.metric}><span>landing rate</span><strong>{metrics?.landingRate ?? "n/a"}</strong></div>
        <div className={styles.metric}><span>non-landed attempts</span><strong>{formatCount(metrics?.nonLandedAttempts)}</strong></div>
      </div>
      <DashboardFiltersPanel filters={filters} onFiltersChange={setFilters} onOutcomeChange={setOutcome} />
      <DashboardRefreshToolbar
        loading={loading}
        error={error}
        paused={paused}
        autoPaused={autoPaused}
        lastUpdated={lastUpdated}
        onRefresh={refresh}
        onTogglePause={setPaused}
      />
      <ExecutionTable rows={data?.executions.executions ?? []} includeRowLinks emptyMessage="No executions match this filter set." />
    </section>
  );
}
