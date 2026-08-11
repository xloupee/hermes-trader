"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import type { DashboardSystemResponse } from "@/lib/dashboard-client";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import { formatUserDateTime } from "@/lib/user-time";
import { useUserTimeZone } from "@/lib/use-user-time-zone";
import styles from "@/components/dashboard/dashboard-shared.module.css";

export function SystemDashboard() {
  const timeZone = useUserTimeZone();
  const query = useAutoRefreshQuery<DashboardSystemResponse>(async () => {
    const response = await fetch("/api/dashboard/system");
    if (!response.ok) throw new Error("Dashboard system endpoint is unavailable");
    return response.json() as Promise<DashboardSystemResponse>;
  }, { intervalMs: 15000 });

  return <section>
    <div className={styles.systemGrid}>
      <article className={styles.statCard}><h3>Access mode</h3><p>Public read-only observer</p><p>No sign-in required</p></article>
      <article className={styles.statCard}><h3>Execution table</h3><p>Rows: {query.data?.tables.copytradeLocalExecutions ?? "unavailable"}</p></article>
      <article className={styles.statCard}><h3>Signal observations</h3><p>Rows: {query.data?.tables.copytradeSignalObservations ?? "unavailable"}</p></article>
      <article className={styles.statCard}><h3>Server contract</h3><p>Supabase URL: {query.data?.environment.supabaseUrl ? "configured" : "missing"}</p><p>Service role: {query.data?.environment.hasServiceRole ? "configured" : "missing"}</p><p title={timeZone}>Checked: {query.data?.time ? formatUserDateTime(query.data.time, timeZone) : "n/a"}</p></article>
    </div>
    <DashboardRefreshToolbar loading={query.loading} error={query.error} paused={query.paused} autoPaused={query.autoPaused} lastUpdated={query.lastUpdated} onRefresh={query.refresh} onTogglePause={query.setPaused} />
    <h2 className={styles.paneTitle}>Public endpoint health</h2>
    <ul className={styles.healthList}>
      <li><span>/api/dashboard/system</span><strong>{query.data ? "ok" : "pending"}</strong><em>read only</em></li>
    </ul>
  </section>;
}
