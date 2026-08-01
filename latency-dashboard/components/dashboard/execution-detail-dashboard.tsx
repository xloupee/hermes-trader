"use client";

import Link from "next/link";
import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import type { DashboardExecution } from "@/lib/dashboard-client";
import { formatCount, formatMs, formatSlot, formatSol, landingSummary } from "@/lib/dashboard-client";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import { CopyChip } from "@/components/dashboard/copy-chip";
import { ms, us } from "@/lib/benchmark-format";
import { firstNumber } from "@/lib/benchmark-position";
import styles from "@/components/dashboard/dashboard-shared.module.css";

function DetailList({ rows }: { rows: Array<{ label: string; value: string }> }) {
  return <dl className={styles.detailList}>{rows.map((item) => <div key={item.label}><dt>{item.label}</dt><dd>{item.value}</dd></div>)}</dl>;
}

function localDetectUs(row: DashboardExecution): number | null {
  const stages = [row.feedReceivedToDecodedUs, row.batchScanUs, row.txParseUs];
  if (stages.every((value): value is number => typeof value === "number" && Number.isFinite(value))) {
    return stages.reduce((total, value) => total + value, 0);
  }
  return firstNumber(row.decodedToMatchedUs, row.txParseUs);
}

function ackDurationMs(row: DashboardExecution): number | null {
  const returned = row.observedToSignatureReturnedMs;
  const submitted = row.observedToSendSubmittedMs;
  if (typeof returned !== "number" || !Number.isFinite(returned)) return null;
  return typeof submitted === "number" && Number.isFinite(submitted) ? Math.max(0, returned - submitted) : returned;
}

function LatencyBreakdown({ row }: { row: DashboardExecution }) {
  const stages = [
    { label: "Local", value: us(localDetectUs(row)), context: "detect total" },
    { label: "Entry decode", value: us(firstNumber(row.entryDecodeUs, row.feedReceivedToEntriesReadyUs, row.feedReceivedToDecodedUs)), context: "entries ready" },
    { label: "Scan", value: us(row.batchScanUs), context: "batch scan" },
    { label: "Tx / route parse", value: `${us(row.txParseUs)} / ${us(row.routeParseUs)}`, context: "tx / route" },
    { label: "Build / sign", value: `${us(row.unsignedBuildUs)} / ${us(row.signUs)}`, context: "build / sign" },
    { label: "Submit / ACK", value: `${ms(row.observedToSendSubmittedMs)} / ${ms(ackDurationMs(row))}`, context: "submit / first ACK" }
  ];

  return <article className={styles.latencyBreakdown} aria-labelledby="latency-breakdown-title">
    <header><div><h2 id="latency-breakdown-title">Latency breakdown</h2><p>Detection through first transport acknowledgement</p></div><span>µs unless marked ms</span></header>
    <dl className={styles.latencyStages}>{stages.map((stage) => <div key={stage.label}><dt>{stage.label}</dt><dd>{stage.value}</dd><small>{stage.context}</small></div>)}</dl>
  </article>;
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
      <LatencyBreakdown row={row} />
      <GroupedDiagnostics row={row} />
      <section className={styles.rawSection}><h3>Sanitized normalized diagnostics</h3><details><summary>Normalized execution JSON</summary><pre>{JSON.stringify(row, null, 2)}</pre></details></section>
    </> : <div className={styles.emptyState}>{query.error || "No detail row loaded yet."}</div>}
  </section>;
}
