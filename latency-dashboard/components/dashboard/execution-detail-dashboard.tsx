"use client";

import Link from "next/link";
import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import {
  type BenchmarkDetailRow,
  type BenchmarkResponse,
  formatMs,
  formatSlot,
  formatSol,
  isLandedBuy,
  isLandedSell,
  hasNoTarget,
  formatCount
} from "./dashboard-contract";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import { CopyChip } from "@/components/dashboard/copy-chip";
import styles from "@/components/dashboard/dashboard-shared.module.css";

interface GroupedDiagnosticsProps {
  row: BenchmarkDetailRow;
}

function DetailList({ rows }: { rows: Array<{ label: string; value: string }> }) {
  return (
    <dl className={styles.detailList}>
      {rows.map((item) => (
        <div key={item.label}>
          <dt>{item.label}</dt>
          <dd>{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}

function GroupedDiagnostics({ row }: GroupedDiagnosticsProps) {
  const execution = row.execution;
  const signal = row.signal;
  const timingRows = [
    { label: "Observed", value: new Date(row.observedAtMs).toLocaleString() },
    { label: "Slot", value: String(row.slot) },
    { label: "Route", value: row.route || "n/a" },
    {
      label: "Copy status",
      value: execution
        ? (isLandedSell(execution) ? "sell landed" : isLandedBuy(execution) ? "buy landed" : hasNoTarget(execution) ? "no target" : String(execution.buyStatus || "n/a"))
        : "n/a"
    },
    { label: "Signal action", value: String(signal?.action || row.action) },
    { label: "Copy signature", value: String(execution?.sendSignature || "n/a") },
    { label: "Signal signature", value: String(signal?.signature || row.signature) }
  ];
  const positionRows = [
    { label: "Target slot", value: String((execution?.targetSlot as number | null) ?? "n/a") },
    { label: "Copy slot", value: String((execution?.copySlot as number | null) ?? "n/a") },
    { label: "Slot delta", value: formatSlot((execution?.slotDelta as number | null) ?? null) },
    { label: "Tx delta", value: String((execution?.txDelta as number | null) ?? "n/a") },
    { label: "Position", value: String((execution?.positionUnavailableReason as string | null) || "n/a") }
  ];
  const feeRows = [
    { label: "Gross spend", value: formatSol((execution?.grossCopySpendSol as number | null) ?? null) },
    { label: "Network fee", value: formatSol((execution?.networkFeeSol as number | null) ?? null) },
    { label: "Observed SOL", value: formatSol((execution?.observedSolAmount as number | null) ?? null) },
    { label: "Copy amount", value: formatSol((execution?.maxCopySol as number | null) ?? null) },
    { label: "Copy age", value: formatMs((execution?.observedToSignatureReturnedMs as number | null) ?? null) }
  ];
  const signalRows = [
    { label: "Mint", value: row.mint || "n/a" },
    { label: "Provider", value: row.provider || "n/a" },
    { label: "Source", value: row.source || "n/a" },
    { label: "Target wallet", value: row.targetWallet || "n/a" },
    { label: "Signal id", value: formatCount(row.signalObservationId) },
    { label: "Copy wallet", value: row.copyWallet || "n/a" },
    { label: "Signal action", value: String(signal?.action || row.action) }
  ];

  return (
    <section className={styles.detailGrid}>
      <article className={styles.detailGroup}>
        <h3>Timing diagnostics</h3>
        <DetailList rows={timingRows} />
      </article>
      <article className={styles.detailGroup}>
        <h3>Signal details</h3>
        <DetailList rows={signalRows} />
      </article>
      <article className={styles.detailGroup}>
        <h3>Position diagnostics</h3>
        <DetailList rows={positionRows} />
      </article>
      <article className={styles.detailGroup}>
        <h3>Fee diagnostics</h3>
        <DetailList rows={feeRows} />
      </article>
    </section>
  );
}

export function ExecutionDetailDashboard({ id }: { id: string }) {
  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery<BenchmarkResponse>(
    async (): Promise<BenchmarkResponse> => {
      const response = await fetch(`/api/signals/benchmark-rows/detail?rowId=${encodeURIComponent(`execution:${id}`)}`);
      if (!response.ok) {
        throw new Error("Could not load execution detail");
      }
      return response.json() as Promise<BenchmarkResponse>;
    },
    { intervalMs: 15000 }
  );

  const row = data?.row;
  return (
    <section>
      <div className={styles.toolbar}>
        <p className="pane-title">Execution detail · {id}</p>
        <Link href="/dashboard/executions">Back to executions</Link>
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
        {row ? (
        <>
          <header className={styles.detailTop}>
            <div>
              <h1>{row.action.toUpperCase()} {row.mint}</h1>
              <p>Route: {row.route || "n/a"}</p>
              <p>Observed: {new Date(row.observedAtMs).toLocaleString()}</p>
            </div>
            <div className={styles.detailTopPill}>
              <span>Signal ID: </span>
              <CopyChip value={String(row.id)} label="signal row id" />
            </div>
          </header>
          <GroupedDiagnostics row={row} />
          <section className={styles.rawSection}>
            <h3>Raw JSON snapshots</h3>
            <details>
              <summary>Row JSON</summary>
              <pre>{JSON.stringify(row, null, 2)}</pre>
            </details>
            <details>
              <summary>Signal JSON</summary>
              <pre>{JSON.stringify(row.signal, null, 2)}</pre>
            </details>
            <details>
              <summary>Execution JSON</summary>
              <pre>{JSON.stringify(row.execution, null, 2)}</pre>
            </details>
          </section>
        </>
      ) : (
        <div className={styles.emptyState}>{error ? error : "No detail row loaded yet."}</div>
      )}
    </section>
  );
}
