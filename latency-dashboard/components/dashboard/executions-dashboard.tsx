"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import {
  type ExecutionResponse,
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

function buildQuery(filters: {
  since: string;
  provider: string;
  targetWallet: string;
  mint: string;
  action: string;
  route: string;
  source: string;
}) {
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

export function ExecutionsDashboard() {
  const { filters, setFilters, setOutcome } = useDashboardFilters();
  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery<ExecutionResponse>(
    async (): Promise<ExecutionResponse> => {
      const query = buildQuery(filters);
      const response = await fetch(`/api/signals/executions?${query}`);
      if (!response.ok) {
        throw new Error("Could not load executions");
      }
      return response.json() as Promise<ExecutionResponse>;
    },
    { intervalMs: 15000 }
  );

  const rows = applyLandingPreset(data?.executions || [], filters.outcome);
  const landedBuys = rows.filter((row) => isLandedBuy(row)).length;
  const landedSells = rows.filter((row) => isLandedSell(row)).length;
  const totalAttempts = rows.filter((row) => row.sent).length;
  const nonLanded = rows.filter((row) => hasNonLandedAttempt(row)).length;
  const landingRate = formatPercent(landedBuys, totalAttempts);

  return (
    <section>
      <div className="dual-latency">
        <div className="metric">
          <span>landed buys</span>
          <strong>{formatCount(landedBuys)}</strong>
        </div>
        <div className="metric">
          <span>landed sells</span>
          <strong>{formatCount(landedSells)}</strong>
        </div>
        <div className="metric">
          <span>landing rate</span>
          <strong>{landingRate}</strong>
        </div>
        <div className="metric">
          <span>non-landed attempts</span>
          <strong>{formatCount(nonLanded)}</strong>
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
      <ExecutionTable rows={rows} includeRowLinks emptyMessage="No matching executions." />
    </section>
  );
}
