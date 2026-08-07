"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import { type DashboardExecutionsResponse, toQueryParams } from "@/lib/dashboard-client";
import { useDashboardFilters } from "./use-dashboard-filters";
import { DashboardFiltersPanel } from "@/components/dashboard/dashboard-filters";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import { ExecutionTable } from "@/components/dashboard/execution-table";

export function ExecutionsDashboard() {
  const { filters, setFilters, setOutcome } = useDashboardFilters();
  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery<DashboardExecutionsResponse>(
    async (): Promise<DashboardExecutionsResponse> => {
      const query = toQueryParams(filters, true);
      const response = await fetch(`/api/dashboard/executions?${query}`);
      if (!response.ok) throw new Error("Could not load executions");
      return response.json() as Promise<DashboardExecutionsResponse>;
    },
    { intervalMs: 15000 }
  );

  return (
    <section>
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
      <ExecutionTable rows={data?.executions || []} includeRowLinks emptyMessage="No matching executions." />
    </section>
  );
}
