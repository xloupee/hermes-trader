"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import {
  type LatencyResponse,
  type ExecutionResponse,
  type SignalResponse,
  type MeResponse,
  type SignalSummary
} from "./dashboard-contract";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import styles from "@/components/dashboard/dashboard-shared.module.css";

interface ContractHealth {
  label: string;
  status: "ok" | "error";
  latency: string;
}

interface SystemPayload {
  me: MeResponse;
  signalSummary: SignalResponse["summary"];
  executionSummary: ExecutionResponse["summary"];
  latencySummary: LatencyResponse["summary"];
}

export function SystemDashboard() {
  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery<SystemPayload>(
    async (): Promise<SystemPayload> => {
      const [meResponse, signalSummaryResponse, executionSummaryResponse, latencySummaryResponse] = await Promise.all([
        fetch("/api/me"),
        fetch("/api/signals/summary"),
        fetch("/api/signals/executions"),
        fetch("/api/latency/summary")
      ]);

      if (!meResponse.ok || !signalSummaryResponse.ok || !executionSummaryResponse.ok || !latencySummaryResponse.ok) {
        throw new Error("one or more system endpoints unavailable");
      }

      const [me, signalSummary, executionSummary, latencySummary] = await Promise.all([
        meResponse.json() as Promise<MeResponse>,
        signalSummaryResponse.json() as Promise<{ summary: SignalSummary; filters: Record<string, string> }>,
        executionSummaryResponse.json() as Promise<ExecutionResponse>,
        latencySummaryResponse.json() as Promise<LatencyResponse>
      ]);

      return {
        me,
        signalSummary: signalSummary.summary,
        executionSummary: executionSummary.summary,
        latencySummary: latencySummary.summary
      };
    },
    { intervalMs: 15000 }
  );

  const endpointHealth: Array<ContractHealth> = [
    { label: "auth", status: data ? "ok" : "error", latency: "live" },
    { label: "signals", status: data ? "ok" : "error", latency: "live" },
    { label: "executions", status: data ? "ok" : "error", latency: "live" },
    { label: "latency", status: data ? "ok" : "error", latency: "live" }
  ];
  const landedSells = data ? data.executionSummary.autoSellLanded : 0;

  return (
    <section>
      <div className={styles.systemGrid}>
        <article className={styles.statCard}>
          <h3>Operator identity</h3>
          <p>Admin id: {data?.me.user.id || "n/a"}</p>
          <p>Email: {data?.me.user.email || "n/a"}</p>
        </article>
        <article className={styles.statCard}>
          <h3>Signals</h3>
          <p>Total signals: {data?.signalSummary.total ?? "n/a"}</p>
          <p>Bids / asks: {data?.signalSummary.buys ?? "n/a"} / {data?.signalSummary.sells ?? "n/a"}</p>
          <p>Copyable: {data?.signalSummary.copyable ?? "n/a"}</p>
        </article>
        <article className={styles.statCard}>
          <h3>Executions</h3>
          <p>Total: {data?.executionSummary.total ?? "n/a"}</p>
          <p>Sent: {data?.executionSummary.sent ?? "n/a"}</p>
          <p>Landed buys: {data?.executionSummary.landed ?? "n/a"}</p>
          <p>Landed sells: {landedSells ?? "n/a"}</p>
        </article>
        <article className={styles.statCard}>
          <h3>Latency</h3>
          <p>Total: {data?.latencySummary.total ?? "n/a"}</p>
          <p>Submitted: {data?.latencySummary.submitted ?? "n/a"} | Failed: {data?.latencySummary.failed ?? "n/a"}</p>
          <p>Target p50/p90: {data?.latencySummary.targetObserved?.p50 ?? "n/a"} / {data?.latencySummary.targetObserved?.p90 ?? "n/a"} ms</p>
        </article>
      </div>

      <DashboardRefreshToolbar
        loading={loading}
        error={error}
        paused={paused}
        autoPaused={autoPaused}
        lastUpdated={lastUpdated}
        onRefresh={refresh}
        onTogglePause={setPaused}
      />

      <h2 className="pane-title">Endpoint health</h2>
      <ul className={styles.healthList}>
        {endpointHealth.map((entry, index) => (
          <li key={`${entry.label}-${index}`}>
            <span>{entry.label}</span>
            <strong>{entry.status}</strong>
            <em>{entry.latency}</em>
          </li>
        ))}
      </ul>
    </section>
  );
}
