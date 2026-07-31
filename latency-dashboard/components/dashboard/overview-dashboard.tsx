"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import {
  type DashboardExecutionsResponse,
  type DashboardOverviewResponse,
  applyLandingPreset,
  formatCount,
  formatPercent,
  hasNonLandedAttempt,
  isLandedBuy,
  isLandedSell,
  toQueryParams
} from "@/lib/dashboard-client";
import { useDashboardFilters } from "./use-dashboard-filters";
import { DashboardFiltersPanel } from "@/components/dashboard/dashboard-filters";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import { ExecutionTable } from "@/components/dashboard/execution-table";
import styles from "@/components/dashboard/dashboard-shared.module.css";

function buildQuery(filters: { since: string; provider: string; observedWallet: string; mint: string; action: string; route: string; source: string }) {
  return toQueryParams({
    since: filters.since,
    provider: filters.provider,
    observedWallet: filters.observedWallet,
    mint: filters.mint,
    action: filters.action,
    route: filters.route,
    source: filters.source
  });
}

interface OverviewPayload {
  overview: DashboardOverviewResponse;
  executions: DashboardExecutionsResponse;
}

export function OverviewDashboard() {
  const { filters, setFilters, setOutcome } = useDashboardFilters();
  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery<OverviewPayload>(
    async (): Promise<OverviewPayload> => {
      const query = buildQuery(filters);
      const [overviewResponse, executionsResponse] = await Promise.all([
        fetch(`/api/dashboard/overview?${query}`),
        fetch(`/api/dashboard/executions?${query}`)
      ]);
      if (!overviewResponse.ok || !executionsResponse.ok) throw new Error("Could not load dashboard overview");
      const [overview, executions] = await Promise.all([
        overviewResponse.json() as Promise<DashboardOverviewResponse>,
        executionsResponse.json() as Promise<DashboardExecutionsResponse>
      ]);
      return { overview, executions };
    },
    { intervalMs: 15000 }
  );

  const rows = applyLandingPreset(data?.executions.executions ?? [], filters.outcome);
  const totalAttempts = rows.filter((row) => row.sent).length;
  const landedBuys = rows.filter((row) => isLandedBuy(row)).length;
  const landedSells = rows.filter((row) => isLandedSell(row)).length;
  const nonLandedAttempts = rows.filter((row) => hasNonLandedAttempt(row)).length;
  const landingRate = formatPercent(landedBuys, totalAttempts);

  return (
    <section>
      <div className={styles.dualLatency}>
        <div className={styles.metric}>
          <span>landed buys</span>
          <strong>{formatCount(landedBuys)}</strong>
        </div>
        <div className={styles.metric}>
          <span>landed sells</span>
          <strong>{formatCount(landedSells)}</strong>
        </div>
        <div className={styles.metric}>
          <span>landing rate</span>
          <strong>{landingRate}</strong>
        </div>
        <div className={styles.metric}>
          <span>non-landed attempts</span>
          <strong>{formatCount(nonLandedAttempts)}</strong>
        </div>
      </div>

      <DashboardFiltersPanel
        filters={filters}
        onFiltersChange={setFilters}
        onOutcomeChange={setOutcome}
      />
      <DashboardRefreshToolbar
        loading={loading}
        error={error}
        paused={paused}
        autoPaused={autoPaused}
        lastUpdated={lastUpdated}
        onRefresh={refresh}
        onTogglePause={setPaused}
      />
      <ExecutionTable rows={rows} includeRowLinks emptyMessage="No executions match this filter set." />
      <h2 className={styles.paneTitle}>Dashboard window</h2>
      <div className={styles.detailList}>
        Executions: {data?.overview.summary.total ?? 0} | buys / sells: {data?.overview.summary.side.buy ?? 0} / {data?.overview.summary.side.sell ?? 0} | landed: {data?.overview.summary.landed ?? 0}
      </div>
    </section>
  );
}
