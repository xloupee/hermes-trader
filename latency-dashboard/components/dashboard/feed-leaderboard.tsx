import type { DashboardExecution } from "@/lib/dashboard-client";
import { feedLeaderboard, type FeedKey } from "@/lib/feed-winners";
import styles from "@/components/dashboard/dashboard-shared.module.css";

const FEED_TONES: Record<FeedKey, string> = {
  vortex: styles.feedVortex,
  jito: styles.feedJito,
  erpc: styles.feedErpc,
  shredstream: styles.feedShredstream,
  "shred-union": styles.feedUnion,
  everstake: styles.feedEverstake,
  doublezero: styles.feedDoublezero,
  "on-chain": styles.feedOnChain,
  unknown: styles.feedUnknown
};

export function FeedLeaderboard({ rows }: { rows: DashboardExecution[] }) {
  const buys = rows.filter((row) => row.observedAction.toLowerCase() === "buy");
  const standings = feedLeaderboard(buys.map((row) => row.source));

  return (
    <section className={styles.feedLeaderboard} aria-label="Feed winner leaderboard">
      <header className={styles.feedLeaderboardHeading}>
        <div>
          <span>Inbound buy race</span>
          <h2>Feed leaderboard</h2>
        </div>
        <small>{buys.length} visible buy{buys.length === 1 ? "" : "s"}</small>
      </header>
      <div className={styles.feedStandings}>
        {standings.length > 0 ? standings.map((standing, index) => (
          <div className={styles.feedStanding} key={standing.key}>
            <span className={styles.feedRank}>{String(index + 1).padStart(2, "0")}</span>
            <strong className={FEED_TONES[standing.key]}>{standing.label}</strong>
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
        )) : <p className={styles.feedEmpty}>No buy execution winners in the current view.</p>}
      </div>
    </section>
  );
}
