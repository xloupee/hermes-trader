"use client";

import Link from "next/link";
import {
  type DashboardExecution,
  formatMs,
  isDelayedLanding,
  landingSummary,
  leaderSummary,
  leaderTitle,
  shortText
} from "@/lib/dashboard-client";
import { CopyChip } from "@/components/dashboard/copy-chip";
import { executionFeed, feedTransportLabel, type FeedKey } from "@/lib/feed-winners";
import { formatUserDate, formatUserTime, userTimeZoneLabel } from "@/lib/user-time";
import { useUserTimeZone } from "@/lib/use-user-time-zone";
import styles from "@/components/dashboard/dashboard-shared.module.css";

interface ExecutionTableProps {
  rows: DashboardExecution[];
  emptyMessage: string;
  includeRowLinks?: boolean;
}

function statusClass(outcome: DashboardExecution["outcome"]) {
  if (outcome === "landed") return styles.statusGood;
  if (outcome === "failed_on_chain" || outcome === "send_failed") return styles.statusBad;
  return styles.statusMuted;
}

function placementClass(row: DashboardExecution) {
  return isDelayedLanding(row) ? styles.statusBad : statusClass(row.outcome);
}

function landingParts(row: DashboardExecution): { primary: string; secondary: string } {
  const summary = landingSummary(row);
  const pieces = summary.split(" · ");
  if (row.outcome !== "landed") {
    return { primary: pieces[0], secondary: row.reason || "No additional landing context" };
  }
  if (row.landingComparison === "same_slot") {
    return { primary: pieces.slice(0, 2).join(" · "), secondary: pieces.slice(2).join(" · ") };
  }
  if (row.landingComparison === "cross_slot") {
    return { primary: pieces.slice(0, 2).join(" · "), secondary: pieces.slice(2).join(" · ") };
  }
  return { primary: pieces.slice(0, 2).join(" · "), secondary: pieces.slice(2).join(" · ") };
}

function sideClass(side: string) {
  return side.toLowerCase() === "sell" ? styles.sideSell : styles.sideBuy;
}

function ackLaneLabel(lane: string | null): string {
  if (!lane) return "lane n/a";
  return lane.split(":", 1)[0] || "lane n/a";
}

const FEED_CLASSES: Record<FeedKey, string> = {
  vortex: styles.feedVortex,
  jito: styles.feedJito,
  erpc: styles.feedErpc,
  "shred-union": styles.feedUnion,
  everstake: styles.feedEverstake,
  doublezero: styles.feedDoublezero,
  "on-chain": styles.feedOnChain,
  unknown: styles.feedUnknown
};

export function ExecutionTable({ rows, emptyMessage, includeRowLinks = false }: ExecutionTableProps) {
  const timeZone = useUserTimeZone();
  const timeZoneLabel = userTimeZoneLabel(timeZone);

  if (rows.length === 0) {
    return <div className={styles.emptyState} role="status">{emptyMessage}</div>;
  }

  return (
    <section className={styles.dataSection} aria-label="Live execution tape">
      <div className={styles.desktopTableWrap}>
        <table className={styles.dataTable} aria-label="Execution results">
          <thead>
            <tr>
              <th title={timeZone}>Time · {timeZoneLabel}</th>
              <th>Act</th>
              <th>Result / placement</th>
              <th>Leader</th>
              <th>Feed / route</th>
              <th>Lane / ACK</th>
              <th>Asset</th>
              <th>Wallet</th>
              <th>Transaction</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => {
              const landing = landingParts(row);
              const feed = executionFeed(row.source, row.provider);
              const transport = feedTransportLabel(row.source, row.provider);
              return <tr key={row.id}>
                <td className={styles.timeCell}>
                  <strong>{formatUserTime(row.observedAtMs, timeZone)}</strong>
                  <span>{formatUserDate(row.observedAtMs, timeZone)}</span>
                </td>
                <td><span className={sideClass(row.observedAction)}>{row.observedAction}</span></td>
                <td>
                  <span className={placementClass(row)}>{landing.primary}</span>
                  <div className={styles.meta}>{landing.secondary}</div>
                </td>
                <td className={styles.leaderCell} title={leaderTitle(row)}>
                  <strong>{leaderSummary(row)}</strong>
                  <span className={styles.meta}>{row.leaderDiagnostics?.regionPath || "validator location"}</span>
                </td>
                <td><strong className={FEED_CLASSES[feed.key]}>{feed.label}</strong><div className={styles.meta}>{transport ? `${transport} · ` : ""}{row.selectedRoute || "route unavailable"}</div></td>
                <td className={styles.ackCell}>
                  <strong className={styles.ackLane} title={row.firstAckLane || undefined}>{ackLaneLabel(row.firstAckLane)}</strong>
                  <span className={styles.meta}>{formatMs(row.observedToSignatureReturnedMs)} ACK</span>
                </td>
                <td className={styles.assetCell}><CopyChip value={row.mint} label="mint address" /></td>
                <td><CopyChip value={row.observedWallet} label="watched wallet" /></td>
                <td className={styles.signCell}>
                  <CopyChip value={row.sendSignature || row.observedSignature} label="transaction signature" />
                  {includeRowLinks ? (
                    <div className={styles.meta}><Link className={styles.detailLink} href={`/dashboard/executions/${row.id}`}>Inspect →</Link></div>
                  ) : (
                    <div className={styles.meta}>copy {shortText(row.sendSignature || row.observedSignature, 7)}</div>
                  )}
                </td>
              </tr>;
            })}
          </tbody>
        </table>
      </div>
      <div className={styles.mobileCards}>
        {rows.map((row) => {
          const landing = landingParts(row);
          const feed = executionFeed(row.source, row.provider);
          const transport = feedTransportLabel(row.source, row.provider);
          return <article key={row.id} className={styles.card}>
            <header className={styles.cardHeader}>
              <div><span className={sideClass(row.observedAction)}>{row.observedAction}</span><strong>{shortText(row.mint, 5)}</strong></div>
              <time title={timeZone}>{formatUserTime(row.observedAtMs, timeZone)} {timeZoneLabel}</time>
            </header>
            <div className={styles.cardOutcome}>
              <span className={placementClass(row)}>{landing.primary}</span>
              <p>{landing.secondary}</p>
            </div>
            <div className={styles.cardMeta}>
              <span>Leader<strong title={leaderTitle(row)}>{leaderSummary(row)}</strong><small className={styles.meta}>{row.leaderDiagnostics?.regionPath || "validator location"}</small></span>
              <span>Feed<strong className={FEED_CLASSES[feed.key]}>{feed.label}</strong>{transport ? <small className={styles.meta}>{transport}</small> : null}</span>
              <span>Route<strong>{row.selectedRoute || "n/a"}</strong></span>
              <span>Lane / ACK<strong className={styles.ackLane} title={row.firstAckLane || undefined}>{ackLaneLabel(row.firstAckLane)}</strong><small className={styles.meta}>{formatMs(row.observedToSignatureReturnedMs)} ACK</small></span>
            </div>
            <div className={styles.cardCopies}>
              <span>Wallet <CopyChip value={row.observedWallet} label="watched wallet" /></span>
              <span>Tx <CopyChip value={row.sendSignature || row.observedSignature} label="transaction signature" /></span>
            </div>
            {includeRowLinks ? <Link className={styles.mobileDetailLink} href={`/dashboard/executions/${row.id}`}>Inspect execution <span aria-hidden="true">→</span></Link> : null}
          </article>;
        })}
      </div>
    </section>
  );
}
