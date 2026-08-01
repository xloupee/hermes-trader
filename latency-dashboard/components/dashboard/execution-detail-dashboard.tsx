"use client";

import Link from "next/link";
import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import type { DashboardExecution } from "@/lib/dashboard-client";
import { formatCount, formatMs, formatSlot, formatSol, landingSummary } from "@/lib/dashboard-client";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import { CopyChip } from "@/components/dashboard/copy-chip";
import styles from "@/components/dashboard/dashboard-shared.module.css";

function DetailList({ rows }: { rows: Array<{ label: string; value: string }> }) {
  return <dl className={styles.detailList}>{rows.map((item) => <div key={item.label}><dt>{item.label}</dt><dd>{item.value}</dd></div>)}</dl>;
}

function GroupedDiagnostics({ row }: { row: DashboardExecution }) {
  const timingRows = [
    { label: "Observed", value: new Date(row.observedAtMs).toLocaleString() },
    { label: "Outcome", value: landingSummary(row) },
    { label: "Observed to signature", value: formatMs(row.observedToSignatureReturnedMs) },
    { label: "Observed to send", value: formatMs(row.observedToSendSubmittedMs) }
  ];
  const positionRows = [
    { label: "Target slot", value: formatCount(row.targetSlot) },
    { label: "Copy slot", value: formatCount(row.copySlot) },
    { label: "Slot delta", value: formatSlot(row.slotDelta) },
    { label: "Tx delta", value: formatCount(row.txDelta) },
    { label: "Landing comparison", value: row.landingComparison }
  ];
  const feeRows = [
    { label: "Gross spend", value: formatSol(row.grossCopySpendSol) },
    { label: "Network fee", value: formatSol(row.networkFeeSol) },
    { label: "Observed SOL", value: formatSol(row.observedSolAmount) },
    { label: "Copy limit", value: formatSol(row.maxCopySol) }
  ];
  const executionRows = [
    { label: "Action", value: row.observedAction || "n/a" },
    { label: "Mint", value: row.mint || "n/a" },
    { label: "Provider", value: row.provider || "n/a" },
    { label: "Source", value: row.source || "n/a" },
    { label: "Route", value: row.selectedRoute || "n/a" },
    { label: "Observed wallet", value: row.observedWallet || "n/a" }
  ];

  return <section className={styles.detailGrid}>
    <article className={styles.detailGroup}><h3>Timing diagnostics</h3><DetailList rows={timingRows} /></article>
    <article className={styles.detailGroup}><h3>Execution details</h3><DetailList rows={executionRows} /></article>
    <article className={styles.detailGroup}><h3>Position diagnostics</h3><DetailList rows={positionRows} /></article>
    <article className={styles.detailGroup}><h3>Fee diagnostics</h3><DetailList rows={feeRows} /></article>
  </section>;
}

export function ExecutionDetailDashboard({ id }: { id: string }) {
  const query = useAutoRefreshQuery<{ execution: DashboardExecution }>(async () => {
    const response = await fetch(`/api/dashboard/executions/${encodeURIComponent(id)}`);
    if (!response.ok) throw new Error(response.status === 404 ? "Execution not found" : "Could not load execution detail");
    return response.json() as Promise<{ execution: DashboardExecution }>;
  }, { intervalMs: 15000 });
  const row = query.data?.execution;

  return <section>
    <div className={styles.toolbar}><p className={styles.paneTitle}>Execution detail · {id}</p><Link href="/dashboard/executions">Back to executions</Link></div>
    <DashboardRefreshToolbar loading={query.loading} error={query.error} paused={query.paused} autoPaused={query.autoPaused} lastUpdated={query.lastUpdated} onRefresh={query.refresh} onTogglePause={query.setPaused} />
    {row ? <>
      <header className={styles.detailTop}><div><h1>{row.observedAction.toUpperCase()} {row.mint}</h1><p>Route: {row.selectedRoute || "n/a"}</p><p>Observed: {new Date(row.observedAtMs).toLocaleString()}</p><p>Observed wallet: <CopyChip value={row.observedWallet} label="observed wallet" /></p><p>Copy wallet: <CopyChip value={row.copyWallet} label="copy wallet" /></p></div><div className={styles.detailTopPill}><span>Execution ID: </span><CopyChip value={String(row.id)} label="execution id" /></div></header>
      <GroupedDiagnostics row={row} />
      <section className={styles.rawSection}><h3>Sanitized normalized diagnostics</h3><details><summary>Normalized execution JSON</summary><pre>{JSON.stringify(row, null, 2)}</pre></details></section>
    </> : <div className={styles.emptyState}>{query.error || "No detail row loaded yet."}</div>}
  </section>;
}
