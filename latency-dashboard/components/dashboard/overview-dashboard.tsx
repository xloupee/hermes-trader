"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import {
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

export function OverviewDashboard() {
  const { filters, setFilters, setOutcome } = useDashboardFilters();
  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery<DashboardOverviewResponse>(
    async (): Promise<DashboardOverviewResponse> => {
      const query = buildQuery(filters);
      const response = await fetch(`/api/dashboard/overview?${query}`);
      if (!response.ok) throw new Error("Could not load dashboard overview");
      return response.json() as Promise<DashboardOverviewResponse>;
    },
    { intervalMs: 15000 }
  );

  const rows = applyLandingPreset(data?.executions ?? [], filters.outcome);
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
        Executions: {data?.summary.total ?? 0} | sources observed: {data?.sourcesObserved ?? 0} | latest: {data?.latestObservedAtMs ? new Date(data.latestObservedAtMs).toLocaleString() : "n/a"}
      </div>
    </section>
  );
}
