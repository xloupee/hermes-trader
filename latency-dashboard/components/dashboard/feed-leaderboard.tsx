import type { DashboardExecution } from "@/lib/dashboard-client";
import type { GatewayConfirmation } from "@/lib/gateway-confirmations";
import { executionEvidenceCounts, feedLeaderboard, isLandedBuy, type FeedKey, type FeedStanding } from "@/lib/feed-winners";
import styles from "@/components/dashboard/dashboard-shared.module.css";

const FEED_TONES: Record<FeedKey, string> = {
  "vortex-fra": styles.feedVortex,
  "jito-primary": styles.feedJito,
  "doublezero-leader": styles.feedDoublezero,
  "doublezero-retransmit-eu": styles.feedDoublezero,
  unknown: styles.feedUnknown
};

function isGatewayLandedBuy(row: GatewayConfirmation): boolean {
  return row.observedAction.toLowerCase() === "buy" && row.ok && row.status === "landed";
}

export function FeedLeaderboard({
  rows,
  gatewayRows = []
}: {
  rows: DashboardExecution[];
  gatewayRows?: GatewayConfirmation[];
}) {
  const canonicalSignatures = new Set(rows.map((row) => row.sendSignature).filter(Boolean));
  const uniqueGatewayRows = gatewayRows.filter((row) => !canonicalSignatures.has(row.signature));
  const landedBuySources = [
    ...rows.filter(isLandedBuy).map((row) => row.inboundSource),
    ...uniqueGatewayRows.filter(isGatewayLandedBuy).map((row) => row.inboundSource)
  ];
  const evidenceSources = [
    ...rows.map((row) => row.inboundSource),
    ...uniqueGatewayRows.map((row) => row.inboundSource)
  ];
  const winnerStandings = feedLeaderboard(landedBuySources);
  const evidence = executionEvidenceCounts(evidenceSources);
  const standingByKey = new Map(winnerStandings.map((standing) => [standing.key, standing]));
  const trackedFeeds: FeedStanding[] = (["jito-primary", "doublezero-leader", "vortex-fra"] as const).map((key) => (
    standingByKey.get(key) || {
      key,
      label: key === "jito-primary" ? "Jito" : key === "vortex-fra" ? "Vortex" : "DoubleZero",
      wins: 0,
      share: 0
    }
  ));
  const standings = [
    ...winnerStandings,
    ...trackedFeeds.filter((standing) => !standingByKey.has(standing.key))
  ];

  return (
    <section className={styles.feedLeaderboard} aria-label="Feed winner leaderboard">
      <header className={styles.feedLeaderboardHeading}>
        <div>
          <span>Landed buy race</span>
          <h2>Feed leaderboard</h2>
        </div>
        <small>{landedBuySources.length} landed buy{landedBuySources.length === 1 ? "" : "s"} · execution evidence only</small>
      </header>
      <div className={styles.feedStandings}>
        {standings.length > 0 ? standings.map((standing, index) => (
          <div className={styles.feedStanding} key={standing.key}>
            <span className={styles.feedRank}>{String(index + 1).padStart(2, "0")}</span>
            <div className={styles.feedIdentity}>
              <strong className={FEED_TONES[standing.key]}>{standing.label}</strong>
              <small>{evidence.get(standing.key) || 0} row{(evidence.get(standing.key) || 0) === 1 ? "" : "s"} in view</small>
            </div>
            <div
              className={styles.feedShareTrack}
              aria-label={`${standing.label}: ${standing.wins} wins, ${standing.share.toFixed(1)} percent`}
              role="img"
            >
              <span className={FEED_TONES[standing.key]} style={{ inlineSize: `${standing.share}%` }} />
            </div>
            <b>{standing.wins}</b>
            <span className={styles.feedShare}>{standing.share.toFixed(1)}%</span>
          </div>
        )) : <p className={styles.feedEmpty}>No feed evidence in the current view.</p>}
      </div>
    </section>
  );
}
