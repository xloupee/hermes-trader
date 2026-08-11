"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import { type DashboardExecutionsResponse, toQueryParams } from "@/lib/dashboard-client";
import { useDashboardFilters } from "./use-dashboard-filters";
import { DashboardFiltersPanel } from "@/components/dashboard/dashboard-filters";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import { ExecutionTable } from "@/components/dashboard/execution-table";
import { GatewayConfirmationTable } from "@/components/dashboard/gateway-confirmation-table";
import styles from "@/components/dashboard/dashboard-shared.module.css";

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
    <section className={styles.tapePage}>
      <header className={styles.pageHeader}>
        <div><p>Historical and live records</p><h1>All executions</h1></div>
        <span>{data?.summary.total ?? 0} results in window</span>
      </header>
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
      <GatewayConfirmationTable
        rows={data?.gatewayConfirmations ?? []}
        freshness={data?.gatewayConfirmationFreshness}
      />
      <ExecutionTable rows={data?.executions || []} includeRowLinks emptyMessage="No executions match these filters. Clear a filter or choose All tape." />
    </section>
  );
}
