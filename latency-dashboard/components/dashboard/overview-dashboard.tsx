"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import {
  type ExecutionResponse,
  type SignalSummary,
  applyLandingPreset,
  formatCount,
  formatPercent,
  type LandingPreset,
  hasNonLandedAttempt,
  isLandedBuy,
  isLandedSell,
  toQueryParams
} from "./dashboard-contract";
import { useDashboardFilters } from "./use-dashboard-filters";
import { DashboardFiltersPanel } from "@/components/dashboard/dashboard-filters";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import { ExecutionTable } from "@/components/dashboard/execution-table";
import styles from "@/components/dashboard/dashboard-shared.module.css";

interface OverviewPayload {
  signals: { summary: SignalSummary };
  executions: ExecutionResponse;
}

function buildQuery(filters: { since: string; provider: string; targetWallet: string; mint: string; action: string; route: string; source: string }) {
  return toQueryParams({
    since: filters.since,
    provider: filters.provider,
    targetWallet: filters.targetWallet,
    mint: filters.mint,
    action: filters.action,
    route: filters.route,
    source: filters.source,
    outcome: "all" as LandingPreset
  });
}

export function OverviewDashboard() {
  const { filters, setFilters, setOutcome } = useDashboardFilters();
  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery<OverviewPayload>(
    async (): Promise<OverviewPayload> => {
      const query = buildQuery(filters);
      const [signalSummary, executions] = await Promise.all([
        fetch(`/api/signals/summary?${query}`).then((response) => {
          if (!response.ok) {
            throw new Error("failed signals summary");
          }
          return response.json() as Promise<{ summary: SignalSummary }>;
        }),
        fetch(`/api/signals/executions?${query}`).then((response) => {
          if (!response.ok) {
            throw new Error("failed executions query");
          }
          return response.json() as Promise<ExecutionResponse>;
        })
      ]);
      return { signals: signalSummary, executions };
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
      <h2 className={styles.paneTitle}>Source feed signal summary</h2>
      <div className={styles.detailList}>
        Total signals: {data?.signals.summary.total ?? 0} | buys / sells: {data?.signals.summary.buys ?? 0} / {data?.signals.summary.sells ?? 0} | copyable: {data?.signals.summary.copyable ?? 0}
      </div>
    </section>
  );
}
