"use client";

import type { GatewayConfirmation, GatewayConfirmationFreshness } from "@/lib/gateway-confirmations";
import { formatSlot, shortText } from "@/lib/dashboard-client";
import { CopyChip } from "@/components/dashboard/copy-chip";
import { formatUserDate, formatUserDateTime, formatUserTime, userTimeZoneLabel } from "@/lib/user-time";
import { useUserTimeZone } from "@/lib/use-user-time-zone";
import styles from "@/components/dashboard/dashboard-shared.module.css";

interface GatewayConfirmationTableProps {
  rows: GatewayConfirmation[];
  freshness?: GatewayConfirmationFreshness | null;
}

function sideClass(side: string) {
  return side.toLowerCase() === "sell" ? styles.sideSell : styles.sideBuy;
}

function statusClass(row: GatewayConfirmation) {
  if (row.ok && row.status === "landed") return styles.statusGood;
  if (!row.ok) return styles.statusBad;
  return styles.statusMuted;
}

function statusLabel(row: GatewayConfirmation) {
  if (row.ok && row.status === "landed") return "Landed";
  if (row.status) return row.status.replaceAll("_", " ");
  return row.ok ? "Confirmed" : "Failed";
}

function placement(row: GatewayConfirmation) {
  const txDelta = row.txDelta ?? row.sameSlotTxDelta;
  if (typeof txDelta === "number" && Number.isFinite(txDelta)) {
    return txDelta === 0 ? "same transaction" : `${txDelta > 0 ? "+" : ""}${txDelta} tx`;
  }
  return formatSlot(row.slotDelta);
}

function latestLabel(freshness: GatewayConfirmationFreshness | null | undefined, timeZone: string) {
  if (!freshness?.latestObservedAtMs) return "No confirmation evidence yet";
  return `Latest ${formatUserDateTime(freshness.latestObservedAtMs, timeZone)}`;
}

export function GatewayConfirmationTable({ rows, freshness }: GatewayConfirmationTableProps) {
  const timeZone = useUserTimeZone();
  const timeZoneLabel = userTimeZoneLabel(timeZone);

  return (
    <section className={styles.evidenceBlock} aria-label="Gateway confirmation evidence">
      <header className={styles.evidenceHeader}>
        <div>
          <p>Read-only chain evidence</p>
          <h2>Gateway confirmations</h2>
          <span>Separate from the canonical execution ledger · {rows.length} in window</span>
        </div>
        <span title={timeZone}>{latestLabel(freshness, timeZone)}</span>
      </header>
      {rows.length === 0 ? (
        <div className={styles.emptyState} role="status">No gateway confirmations match these filters.</div>
      ) : (
        <>
          <div className={styles.desktopTableWrap}>
            <table className={styles.dataTable} aria-label="Gateway confirmation results">
              <thead>
                <tr>
                  <th title={timeZone}>Time · {timeZoneLabel}</th>
                  <th>Result</th>
                  <th>Act</th>
                  <th>Route</th>
                  <th>Slot</th>
                  <th>TX Δ</th>
                  <th>CA</th>
                  <th>Wallet</th>
                  <th>Transaction</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => (
                  <tr key={row.id}>
                    <td className={styles.timeCell}>
                      <strong>{formatUserTime(row.observedAtMs, timeZone)}</strong>
                      <span>{formatUserDate(row.observedAtMs, timeZone)}</span>
                    </td>
                    <td>
                      <strong className={statusClass(row)}>{statusLabel(row)}</strong>
                      <span className={styles.meta}>{row.confirmationStatus || row.gatewayState || row.transactionRole}</span>
                    </td>
                    <td><span className={sideClass(row.observedAction)}>{row.observedAction}</span></td>
                    <td>
                      <strong>{row.selectedRoute || "n/a"}</strong>
                      <span className={styles.meta}>{row.routeLayout || "route layout n/a"}</span>
                    </td>
                    <td className={styles.txDistance}>{row.confirmationSlot ?? row.slot}</td>
                    <td className={styles.txDistance}>{placement(row)}</td>
                    <td className={styles.assetCell}><CopyChip value={row.mint} label="mint address" /></td>
                    <td><CopyChip value={row.copyWallet} label="copy wallet" /></td>
                    <td className={styles.signCell}>
                      <CopyChip value={row.signature} label="confirmed transaction signature" />
                      <span className={styles.meta}>observed {shortText(row.observedSignature, 7)}</span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className={styles.mobileCards}>
            {rows.map((row) => (
              <article key={row.id} className={styles.card}>
                <header className={styles.cardHeader}>
                  <div><span className={sideClass(row.observedAction)}>{row.observedAction}</span><strong>{shortText(row.mint, 5)}</strong></div>
                  <time title={timeZone}>{formatUserTime(row.observedAtMs, timeZone)} {timeZoneLabel}</time>
                </header>
                <div className={styles.cardOutcome}>
                  <span className={statusClass(row)}>{statusLabel(row)}</span>
                  <p>{row.confirmationStatus || row.gatewayState || row.transactionRole}</p>
                </div>
                <div className={styles.cardMeta}>
                  <span>Route<strong>{row.selectedRoute || "n/a"}</strong></span>
                  <span>Slot<strong>{row.confirmationSlot ?? row.slot}</strong></span>
                  <span>TX delta<strong>{placement(row)}</strong></span>
                  <span>Source<strong>{row.source}</strong></span>
                </div>
                <div className={styles.cardCopies}>
                  <span>Wallet <CopyChip value={row.copyWallet} label="copy wallet" /></span>
                  <span>Mint <CopyChip value={row.mint} label="mint address" /></span>
                  <span>Tx <CopyChip value={row.signature} label="confirmed transaction signature" /></span>
                </div>
              </article>
            ))}
          </div>
        </>
      )}
    </section>
  );
}
