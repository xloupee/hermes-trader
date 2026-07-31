"use client";

import Link from "next/link";
import {
  type DashboardExecution,
  formatMs,
  formatSlot,
  formatSol,
  landingSummary,
  shortText
} from "@/lib/dashboard-client";
import { CopyChip } from "@/components/dashboard/copy-chip";

import styles from "@/components/dashboard/dashboard-shared.module.css";

interface ExecutionTableProps {
  rows: DashboardExecution[];
  emptyMessage: string;
  includeRowLinks?: boolean;
}

function statusClass(status: string | null) {
  const lowered = (status || "").toLowerCase();
  if (lowered.includes("landed") || lowered.includes("submitted")) {
    return styles.statusGood;
  }
  if (lowered.includes("failed") || lowered.includes("error") || lowered.includes("forbidden")) {
    return styles.statusBad;
  }
  return styles.statusMuted;
}

function crossSlotText(row: DashboardExecution): string {
  if (row.slotDelta === null || !Number.isFinite(row.slotDelta)) {
    return "cross-slot n/a";
  }
  if (row.slotDelta === 0) {
    return `same-slot (${row.sameSlotTxDelta ?? "n/a"})`;
  }
  return `cross-slot ${formatSlot(row.slotDelta)} (${row.txDelta ?? "n/a"} tx)`;
}

export function ExecutionTable({
  rows,
  emptyMessage,
  includeRowLinks = false
}: ExecutionTableProps) {
  if (rows.length === 0) {
    return <div className={styles.emptyState} role="status">{emptyMessage}</div>;
  }

  return (
    <section className={styles.dataSection}>
      <div className={styles.desktopTableWrap}>
        <table className={styles.dataTable}>
          <thead>
            <tr>
              <th>Seen</th>
              <th>Action</th>
              <th>Route</th>
              <th>Provider</th>
              <th>Target wallet</th>
              <th>Landing status</th>
              <th>Cross-slot</th>
              <th>Transaction</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.id}>
                <td>{new Date(row.observedAtMs).toLocaleTimeString()}</td>
                <td>{row.observedAction}</td>
                <td>{row.selectedRoute || "n/a"}</td>
                <td>{row.provider || "n/a"}</td>
                <td><CopyChip value={row.observedWallet} label="target wallet" /></td>
                <td>
                  <span className={statusClass(landingSummary(row))}>{landingSummary(row)}</span>
                  <div className={styles.meta}>{formatMs(row.observedToSignatureReturnedMs)}</div>
                </td>
                <td>{crossSlotText(row)}</td>
                <td className={styles.signCell}>
                  <CopyChip value={row.observedSignature} label="signature" />
                  {includeRowLinks ? (
                    <div className={styles.meta}>
                      <Link href={`/dashboard/executions/${row.id}`}>Open detail</Link>
                    </div>
                  ) : (
                    <div className={styles.meta}>copy {shortText(row.sendSignature || row.observedSignature, 7)}</div>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className={styles.mobileCards}>
        {rows.map((row) => (
          <article key={row.id} className={styles.card}>
            <header>
              <h3>{row.observedAction} · {row.selectedRoute || "n/a"}</h3>
              <span className={statusClass(landingSummary(row))}>{landingSummary(row)}</span>
            </header>
            <p>Provider: {row.provider || "n/a"}</p>
            <p>Seen: {new Date(row.observedAtMs).toLocaleString()}</p>
            <p>Target wallet: <CopyChip value={row.observedWallet} label="target wallet" /></p>
            <p>Target Tx: <CopyChip value={row.observedSignature} label="target signature" /></p>
            <p>Send slot: {formatSlot(row.slotDelta)}</p>
            <p>Copy metrics: {row.sent ? formatSol(row.grossCopySpendSol) : "not sent"}</p>
            <p>Copy slot delta: {formatSlot(row.slotDeltaFromObserved)}</p>
            <p>Cross-slot: {crossSlotText(row)}</p>
            {includeRowLinks ? <Link href={`/dashboard/executions/${row.id}`}>Open execution detail</Link> : null}
          </article>
        ))}
      </div>
    </section>
  );
}
