"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import type { DashboardSystemResponse, MeResponse } from "@/lib/dashboard-client";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import styles from "@/components/dashboard/dashboard-shared.module.css";

interface SystemPayload { me: MeResponse; system: DashboardSystemResponse }

export function SystemDashboard() {
  const query = useAutoRefreshQuery<SystemPayload>(async () => {
    const [meResponse, systemResponse] = await Promise.all([fetch("/api/me"), fetch("/api/dashboard/system")]);
    if (!meResponse.ok || !systemResponse.ok) throw new Error("One or more dashboard system endpoints are unavailable");
    const [me, system] = await Promise.all([meResponse.json() as Promise<MeResponse>, systemResponse.json() as Promise<DashboardSystemResponse>]);
    return { me, system };
  }, { intervalMs: 15000 });

  return <section>
    <div className={styles.systemGrid}>
      <article className={styles.statCard}><h3>Operator identity</h3><p>Admin id: {query.data?.me.user.id || "n/a"}</p><p>Email: {query.data?.me.user.email || "n/a"}</p></article>
      <article className={styles.statCard}><h3>Execution table</h3><p>Rows: {query.data?.system.tables.copytradeLocalExecutions ?? "unavailable"}</p></article>
      <article className={styles.statCard}><h3>Signal observations</h3><p>Rows: {query.data?.system.tables.copytradeSignalObservations ?? "unavailable"}</p></article>
      <article className={styles.statCard}><h3>Server contract</h3><p>Supabase URL: {query.data?.system.environment.supabaseUrl ? "configured" : "missing"}</p><p>Service role: {query.data?.system.environment.hasServiceRole ? "configured" : "missing"}</p><p>Checked: {query.data?.system.time ? new Date(query.data.system.time).toLocaleString() : "n/a"}</p></article>
    </div>
    <DashboardRefreshToolbar loading={query.loading} error={query.error} paused={query.paused} autoPaused={query.autoPaused} lastUpdated={query.lastUpdated} onRefresh={query.refresh} onTogglePause={query.setPaused} />
    <h2 className={styles.paneTitle}>Authenticated endpoint health</h2>
    <ul className={styles.healthList}>
      <li><span>/api/me</span><strong>{query.data ? "ok" : "pending"}</strong><em>SSR session</em></li>
      <li><span>/api/dashboard/system</span><strong>{query.data ? "ok" : "pending"}</strong><em>read only</em></li>
    </ul>
  </section>;
}
