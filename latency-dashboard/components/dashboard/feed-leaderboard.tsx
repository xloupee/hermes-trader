import type { DashboardExecution } from "@/lib/dashboard-client";
import type { GatewayConfirmation } from "@/lib/gateway-confirmations";
import { executionEvidenceCounts, feedLeaderboard, isLandedBuy, type FeedKey, type FeedStanding } from "@/lib/feed-winners";
import styles from "@/components/dashboard/dashboard-shared.module.css";

const FEED_TONES: Record<FeedKey, string> = {
  "vortex-fra": styles.feedVortex,
  "jito-primary": styles.feedJito,
  "doublezero-leader": styles.feedDoublezero,
  "doublezero-retransmit-eu": styles.feedDoublezero,
  "helius-raw-auto-ax1": styles.feedHelius,
  "helius-preconf": styles.feedHelius,
  "helius-raw": styles.feedHelius,
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
  const landedBuyFeeds = [
    ...rows.filter(isLandedBuy).map((row) => ({ inboundSource: row.inboundSource, heliusFeedStage: row.heliusFeedStage })),
    ...uniqueGatewayRows.filter(isGatewayLandedBuy).map((row) => ({ inboundSource: row.inboundSource, heliusFeedStage: row.heliusFeedStage }))
  ];
  const evidenceFeeds = [
    ...rows.map((row) => ({ inboundSource: row.inboundSource, heliusFeedStage: row.heliusFeedStage })),
    ...uniqueGatewayRows.map((row) => ({ inboundSource: row.inboundSource, heliusFeedStage: row.heliusFeedStage }))
  ];
  const winnerStandings = feedLeaderboard(landedBuyFeeds);
  const evidence = executionEvidenceCounts(evidenceFeeds);
  const standingByKey = new Map(winnerStandings.map((standing) => [standing.key, standing]));
  const trackedFeeds: FeedStanding[] = (["jito-primary", "doublezero-leader", "vortex-fra", "helius-preconf", "helius-raw"] as const).map((key) => (
    standingByKey.get(key) || {
      key,
      label: key === "jito-primary"
        ? "Jito"
        : key === "vortex-fra"
          ? "Vortex"
          : key === "helius-preconf"
            ? "Helius Preconf"
            : key === "helius-raw"
              ? "Helius Raw Shreds"
              : "DoubleZero",
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
        <small>{landedBuyFeeds.length} landed buy{landedBuyFeeds.length === 1 ? "" : "s"} · execution evidence only</small>
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
