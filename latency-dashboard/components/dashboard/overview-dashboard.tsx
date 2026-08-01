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
import { FeedLeaderboard } from "@/components/dashboard/feed-leaderboard";
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
        : { ...filters, side: "", outcome: "all" };
      const overviewQuery = toQueryParams(summaryFilters, true);
      const landedBuyQuery = toQueryParams({ ...summaryFilters, side: "buy", outcome: "landed-buys" }, true);
      const landedSellQuery = toQueryParams({ ...summaryFilters, side: "sell", outcome: "landed-sells" }, true);
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
    <section className={styles.tapePage}>
      <header className={styles.pageHeader}>
        <div><p>Realtime operator view</p><h1>Execution tape</h1></div>
        <span>{data?.executions.summary.total ?? 0} results in window</span>
      </header>
      <div className={styles.dualLatency}>
        <div className={styles.metric}><span>Landed buys</span><strong>{formatCount(metrics?.landedBuys)}</strong><i>BUY</i></div>
        <div className={styles.metric}><span>Landed sells</span><strong>{formatCount(metrics?.landedSells)}</strong><i>SELL</i></div>
        <div className={styles.metric}><span>Landing rate</span><strong>{metrics?.landingRate ?? "n/a"}</strong><i>24H</i></div>
        <div className={styles.metric}><span>Non-landed</span><strong>{formatCount(metrics?.nonLandedAttempts)}</strong><i>ATTEMPTS</i></div>
      </div>
      <FeedLeaderboard rows={data?.executions.executions ?? []} />
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
      <ExecutionTable rows={data?.executions.executions ?? []} includeRowLinks emptyMessage="No executions match these filters. Clear a filter or choose All tape." />
    </section>
  );
}
