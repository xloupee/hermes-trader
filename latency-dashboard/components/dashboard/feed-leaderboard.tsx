import type { DashboardExecution } from "@/lib/dashboard-client";
import { executionEvidenceCounts, feedLeaderboard, isLandedBuy, type FeedKey, type FeedStanding } from "@/lib/feed-winners";
import styles from "@/components/dashboard/dashboard-shared.module.css";

const FEED_TONES: Record<FeedKey, string> = {
  "vortex-fra": styles.feedVortex,
  "jito-primary": styles.feedJito,
  "doublezero-leader": styles.feedDoublezero,
  "doublezero-retransmit-eu": styles.feedDoublezero,
  unknown: styles.feedUnknown
};

export function FeedLeaderboard({ rows }: { rows: DashboardExecution[] }) {
  const landedBuys = rows.filter(isLandedBuy);
  const winnerStandings = feedLeaderboard(landedBuys.map((row) => row.inboundSource));
  const evidence = executionEvidenceCounts(rows.map((row) => row.inboundSource));
  const standingByKey = new Map(winnerStandings.map((standing) => [standing.key, standing]));
  const trackedFeeds: FeedStanding[] = (["jito-primary", "doublezero-leader", "doublezero-retransmit-eu", "vortex-fra"] as const).map((key) => (
    standingByKey.get(key) || {
      key,
      label: key === "jito-primary" ? "Jito primary" : key === "vortex-fra" ? "Vortex FRA" : key === "doublezero-leader" ? "DoubleZero leader" : "DoubleZero retransmit EU",
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
        <small>{landedBuys.length} landed buy{landedBuys.length === 1 ? "" : "s"} · execution evidence only</small>
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
