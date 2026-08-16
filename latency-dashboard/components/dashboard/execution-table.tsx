"use client";

import Link from "next/link";
import {
  type DashboardExecution,
  formatMs,
  isDelayedLanding,
  landingSummary,
  leaderContext,
  leaderSummary,
  leaderTitle,
  shortText
} from "@/lib/dashboard-client";
import type { GatewayConfirmation } from "@/lib/gateway-confirmations";
import { CopyChip } from "@/components/dashboard/copy-chip";
import { executionFeed, type FeedKey } from "@/lib/feed-winners";
import { sendLaneIdentity, type SendLaneKey } from "@/lib/send-lanes";
import { formatUserDate, formatUserTime, userTimeZoneLabel } from "@/lib/user-time";
import { useUserTimeZone } from "@/lib/use-user-time-zone";
import styles from "@/components/dashboard/dashboard-shared.module.css";

interface ExecutionTableProps {
  rows: DashboardExecution[];
  gatewayRows?: GatewayConfirmation[];
  emptyMessage: string;
  includeRowLinks?: boolean;
}

type ExecutionTableRow =
  | { kind: "canonical"; row: DashboardExecution }
  | { kind: "gateway"; row: GatewayConfirmation };

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

function transactionDistance(row: DashboardExecution): string {
  if (
    row.outcome !== "landed"
    || (row.landingComparison !== "same_slot" && row.landingComparison !== "cross_slot")
  ) {
    return "n/a";
  }

  const candidates = row.landingComparison === "same_slot"
    ? [row.sameSlotTxDelta, row.blockPositionDiagnostics?.sameSlotTxDelta]
    : [
        row.txDelta,
        row.blockPositionDiagnostics?.txDelta,
        row.blockPositionDiagnostics?.crossSlotPositionSummary?.crossSlotTxDelta
      ];
  const distance = candidates.find((value) => typeof value === "number" && Number.isFinite(value));
  return typeof distance === "number" ? String(distance) : "n/a";
}

function sideClass(side: string) {
  return side.toLowerCase() === "sell" ? styles.sideSell : styles.sideBuy;
}

function gatewayStatusClass(row: GatewayConfirmation) {
  if (row.ok && row.status === "landed") return styles.statusGood;
  if (!row.ok) return styles.statusBad;
  return styles.statusMuted;
}

function gatewayStatusLabel(row: GatewayConfirmation) {
  if (row.ok && row.status === "landed") return "Landed";
  if (row.status) return row.status.replaceAll("_", " ");
  return row.ok ? "Confirmed" : "Failed";
}

function gatewayResultLabel(row: GatewayConfirmation) {
  const status = gatewayStatusLabel(row);
  if (!(row.ok && row.status === "landed")) return status;
  if (typeof row.slotDelta !== "number" || !Number.isFinite(row.slotDelta)) return status;
  if (row.slotDelta === 0) return "Landed · same slot";
  return `Landed · ${row.slotDelta > 0 ? "+" : ""}${row.slotDelta} slot${Math.abs(row.slotDelta) === 1 ? "" : "s"}`;
}

function gatewayPlacement(row: GatewayConfirmation): string {
  const txDelta = row.txDelta ?? row.sameSlotTxDelta;
  if (typeof txDelta === "number" && Number.isFinite(txDelta)) {
    return String(txDelta);
  }
  return "n/a";
}

function gatewayLeader(row: GatewayConfirmation): string {
  const leader = row.copySlotLeader || row.targetSlotLeader;
  return leader ? shortText(leader, 7) : "n/a";
}

function gatewayLeaderTitle(row: GatewayConfirmation): string | undefined {
  const parts = [
    row.targetSlotLeader ? `Target: ${row.targetSlotLeader}` : null,
    row.copySlotLeader ? `Copy: ${row.copySlotLeader}` : null
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : undefined;
}

function gatewayLeaderContext(row: GatewayConfirmation): string {
  if (row.targetSlotLeader && row.copySlotLeader) {
    return row.targetSlotLeader === row.copySlotLeader ? "target + copy" : "copy leader";
  }
  return row.copySlotLeader ? "copy leader" : row.targetSlotLeader ? "target leader" : "gateway evidence";
}

const FEED_CLASSES: Record<FeedKey, string> = {
  "vortex-fra": styles.feedVortex,
  "jito-primary": styles.feedJito,
  "doublezero-leader": styles.feedDoublezero,
  "doublezero-retransmit-eu": styles.feedDoublezero,
  unknown: styles.feedUnknown
};

const LANE_CLASSES: Record<SendLaneKey, string> = {
  "helius-sender": styles.laneHelius,
  nozomi: styles.laneNozomi,
  jito: styles.laneJito,
  erpc: styles.laneErpc,
  astralane: styles.laneAstralane,
  lunar: styles.laneLunar,
  circular: styles.laneCircular,
  bloxroute: styles.laneBloxroute,
  "zero-slot": styles.laneZeroSlot,
  tpu: styles.laneTpu,
  rpc: styles.laneRpc,
  unknown: styles.laneUnknown
};

export function ExecutionTable({ rows, gatewayRows = [], emptyMessage, includeRowLinks = false }: ExecutionTableProps) {
  const timeZone = useUserTimeZone();
  const timeZoneLabel = userTimeZoneLabel(timeZone);
  const tableRows: ExecutionTableRow[] = [
    ...rows.map((row) => ({ kind: "canonical" as const, row })),
    ...gatewayRows.map((row) => ({ kind: "gateway" as const, row }))
  ].sort((left, right) => right.row.observedAtMs - left.row.observedAtMs);

  if (tableRows.length === 0) {
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
              <th>TX after</th>
              <th>Leader</th>
              <th>Feed</th>
              <th>Lane / ACK</th>
              <th>CA</th>
              <th>Wallet</th>
              <th>Telegram ID</th>
              <th>Transaction</th>
            </tr>
          </thead>
          <tbody>
            {tableRows.map((entry) => {
              const canonical = entry.kind === "canonical" ? entry.row : null;
              const gateway = entry.kind === "gateway" ? entry.row : null;
              const row = entry.row;
              const landing = canonical ? landingParts(canonical) : null;
              const feed = canonical
                ? executionFeed(canonical.inboundSource)
                : executionFeed(gateway?.inboundSource);
              const lane = sendLaneIdentity(canonical?.firstAckLane ?? gateway?.firstAckLane ?? null);
              return <tr key={`${entry.kind}-${row.id}`}>
                <td className={styles.timeCell}>
                  <strong>{formatUserTime(row.observedAtMs, timeZone)}</strong>
                  <span>{formatUserDate(row.observedAtMs, timeZone)}</span>
                </td>
                <td><span className={sideClass(row.observedAction)}>{row.observedAction}</span></td>
                <td>
                  {canonical ? <>
                    <span className={placementClass(canonical)}>{landing?.primary}</span>
                    {canonical.outcome !== "landed" ? <div className={styles.meta}>{landing?.secondary}</div> : null}
                  </> : <>
                    <span className={gatewayStatusClass(gateway!)}>{gatewayResultLabel(gateway!)}</span>
                    {gateway?.ok && gateway.status === "landed" ? null : <div className={styles.meta}>{gateway?.confirmationStatus || gateway?.gatewayState || gateway?.transactionRole}</div>}
                  </>}
                </td>
                <td className={styles.txDistance}>{canonical ? transactionDistance(canonical) : gatewayPlacement(gateway!)}</td>
                <td className={styles.leaderCell} title={canonical ? leaderTitle(canonical) : gatewayLeaderTitle(gateway!)}>
                  <strong>{canonical ? leaderSummary(canonical) : gatewayLeader(gateway!)}</strong>
                  <span className={styles.meta}>{canonical ? leaderContext(canonical) : gatewayLeaderContext(gateway!)}</span>
                </td>
                <td><strong className={FEED_CLASSES[feed.key]}>{feed.label}</strong></td>
                <td className={styles.ackCell}>
                  <strong className={`${styles.ackLane} ${LANE_CLASSES[lane.key]}`} title={lane.raw || undefined}>{lane.label}</strong>
                  <span className={styles.meta}>{`${formatMs(canonical?.observedToSignatureReturnedMs ?? gateway?.observedToSignatureReturnedMs ?? null)} ACK`}</span>
                </td>
                <td className={styles.assetCell}><CopyChip value={row.mint} label="mint address" /></td>
                <td><CopyChip value={row.observedWallet} label="watched wallet" /></td>
                <td className={styles.subscriberCell}><CopyChip value={canonical ? canonical.telegramSubscriberId : gateway?.telegramId ?? null} label="Telegram subscriber ID" /></td>
                <td className={styles.signCell}>
                  <CopyChip value={canonical ? (canonical.sendSignature || canonical.observedSignature) : gateway?.signature} label="transaction signature" />
                  {canonical && includeRowLinks ? (
                    <div className={styles.meta}><Link className={styles.detailLink} href={`/dashboard/executions/${row.id}`}>Inspect →</Link></div>
                  ) : <div className={styles.meta}>copy {shortText(canonical ? (canonical.sendSignature || canonical.observedSignature) : gateway?.signature, 7)}</div>}
                </td>
              </tr>;
            })}
          </tbody>
        </table>
      </div>
      <div className={styles.mobileCards}>
        {tableRows.map((entry) => {
          const canonical = entry.kind === "canonical" ? entry.row : null;
          const gateway = entry.kind === "gateway" ? entry.row : null;
          const row = entry.row;
          const landing = canonical ? landingParts(canonical) : null;
          const feed = canonical
            ? executionFeed(canonical.inboundSource)
            : executionFeed(gateway?.inboundSource);
          const lane = sendLaneIdentity(canonical?.firstAckLane ?? gateway?.firstAckLane ?? null);
          return <article key={`${entry.kind}-${row.id}`} className={styles.card}>
            <header className={styles.cardHeader}>
              <div><span className={sideClass(row.observedAction)}>{row.observedAction}</span><strong>{shortText(row.mint, 5)}</strong></div>
              <time title={timeZone}>{formatUserTime(row.observedAtMs, timeZone)} {timeZoneLabel}</time>
            </header>
            <div className={styles.cardOutcome}>
              {canonical ? <>
                <span className={placementClass(canonical)}>{landing?.primary}</span>
                {canonical.outcome !== "landed" ? <p>{landing?.secondary}</p> : null}
              </> : <>
                <span className={gatewayStatusClass(gateway!)}>{gatewayResultLabel(gateway!)}</span>
                {gateway?.ok && gateway.status === "landed" ? null : <p>{gateway?.confirmationStatus || gateway?.gatewayState || gateway?.transactionRole}</p>}
              </>}
            </div>
            <div className={styles.cardMeta}>
              <span>TX after<strong className={styles.txDistance}>{canonical ? transactionDistance(canonical) : gatewayPlacement(gateway!)}</strong></span>
              <span>Leader<strong title={canonical ? leaderTitle(canonical) : gatewayLeaderTitle(gateway!)}>{canonical ? leaderSummary(canonical) : gatewayLeader(gateway!)}</strong><small className={styles.meta}>{canonical ? leaderContext(canonical) : gatewayLeaderContext(gateway!)}</small></span>
              <span>Feed<strong className={FEED_CLASSES[feed.key]}>{feed.label}</strong></span>
              <span>Lane / ACK<strong className={`${styles.ackLane} ${LANE_CLASSES[lane.key]}`} title={lane.raw || undefined}>{lane.label}</strong><small className={styles.meta}>{`${formatMs(canonical?.observedToSignatureReturnedMs ?? gateway?.observedToSignatureReturnedMs ?? null)} ACK`}</small></span>
            </div>
            <div className={styles.cardCopies}>
              <span>Wallet <CopyChip value={row.observedWallet} label="watched wallet" /></span>
              <span>Telegram <CopyChip value={canonical ? canonical.telegramSubscriberId : gateway?.telegramId ?? null} label="Telegram subscriber ID" /></span>
              <span>Tx <CopyChip value={canonical ? (canonical.sendSignature || canonical.observedSignature) : gateway?.signature} label="transaction signature" /></span>
            </div>
            {canonical && includeRowLinks ? <Link className={styles.mobileDetailLink} href={`/dashboard/executions/${row.id}`}>Inspect execution <span aria-hidden="true">→</span></Link> : null}
          </article>;
        })}
      </div>
    </section>
  );
}
