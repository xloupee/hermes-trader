"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import type { DashboardSourcesResponse } from "@/lib/dashboard-client";
import { toQueryParams } from "@/lib/dashboard-client";
import { useDashboardFilters } from "./use-dashboard-filters";
import { DashboardFiltersPanel } from "@/components/dashboard/dashboard-filters";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import styles from "@/components/dashboard/dashboard-shared.module.css";

export function SourcesDashboard() {
  const { filters, setFilters, setOutcome } = useDashboardFilters();
  const query = useAutoRefreshQuery<DashboardSourcesResponse>(async () => {
    const response = await fetch(`/api/dashboard/sources?${toQueryParams(filters)}`);
    if (!response.ok) throw new Error("Could not load dashboard sources");
    return response.json() as Promise<DashboardSourcesResponse>;
  }, { intervalMs: 15000 });
  const rows = query.data?.sources ?? [];
  const observations = rows.reduce((total, row) => total + row.count, 0);

  return <section>
    <div className={styles.metricStrip}>
      <div className={styles.metric}><span>source lanes</span><strong>{rows.length}</strong></div>
      <div className={styles.metric}><span>observations</span><strong>{observations}</strong></div>
      <div className={styles.metric}><span>providers</span><strong>{new Set(rows.map((row) => row.provider)).size}</strong></div>
    </div>
    <DashboardFiltersPanel filters={filters} onFiltersChange={setFilters} onOutcomeChange={setOutcome} />
    <DashboardRefreshToolbar loading={query.loading} error={query.error} paused={query.paused} autoPaused={query.autoPaused} lastUpdated={query.lastUpdated} onRefresh={query.refresh} onTogglePause={query.setPaused} />
    <div className={styles.dataSection}><div className={styles.desktopTableWrap}><table className={styles.dataTable}>
      <thead><tr><th>Source</th><th>Provider</th><th>Observations</th><th>Latest</th></tr></thead>
      <tbody>{rows.map((row) => <tr key={`${row.source}:${row.provider}`}><td>{row.source}</td><td>{row.provider}</td><td>{row.count}</td><td>{new Date(row.latestObservedAtMs).toLocaleString()}</td></tr>)}</tbody>
    </table></div>
    <div className={styles.mobileCards}>{rows.map((row) => <article className={styles.card} key={`${row.source}:${row.provider}`}><header><h3>{row.source}</h3><span>{row.provider}</span></header><p>{row.count} observations</p><p>Latest: {new Date(row.latestObservedAtMs).toLocaleString()}</p></article>)}</div>
    {rows.length === 0 ? <div className={styles.emptyState}>No source rows match the current filter set.</div> : null}</div>
  </section>;
}
