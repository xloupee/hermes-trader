"use client";

import { Pause, Play, RefreshCcw } from "lucide-react";
import type { DashboardExecutionFreshness } from "@/lib/local-executions";
import { formatUserDateTime, formatUserTime, userTimeZoneLabel } from "@/lib/user-time";
import { useUserTimeZone } from "@/lib/use-user-time-zone";

import styles from "@/components/dashboard/dashboard-shared.module.css";

interface DashboardRefreshToolbarProps {
  loading: boolean;
  error: string | null;
  paused: boolean;
  autoPaused: boolean;
  lastUpdated: Date | null;
  freshness?: DashboardExecutionFreshness | null;
  onRefresh: () => void;
  onTogglePause: (next: boolean) => void;
}
export function DashboardRefreshToolbar({
  loading,
  error,
  paused,
  autoPaused,
  lastUpdated,
  freshness,
  onRefresh,
  onTogglePause
}: DashboardRefreshToolbarProps) {
  const timeZone = useUserTimeZone();
  return (
    <div className={styles.toolbar}>
      <div className={styles.toolbarStatus}>
        <span className={error ? styles.syncError : loading ? styles.syncLoading : styles.syncReady}>
          <i aria-hidden="true" />
          {error ? "Feed interrupted" : loading && !lastUpdated ? "Connecting to execution feed" : paused || autoPaused ? "Refresh paused" : "Live feed"}
        </span>
        <span className={styles.muted}>{error || (lastUpdated ? `Synced ${formatUserTime(lastUpdated, timeZone)} ${userTimeZoneLabel(timeZone, lastUpdated)}` : "Waiting for first response")}</span>
        {freshness ? <span className={styles.muted}>
          {freshness.latestObservedAtMs === null
            ? "Execution store has no rows"
            : `Data through ${formatUserDateTime(freshness.latestObservedAtMs, timeZone)}`}
        </span> : null}
      </div>
      <div className={styles.toolbarButtons}>
        <button
          className="icon-button"
          onClick={onRefresh}
          type="button"
          title="Refresh now"
          aria-label="Refresh now"
        >
          <RefreshCcw size={15} aria-hidden="true" />
        </button>
        <button
          className="icon-button"
          onClick={() => onTogglePause(!paused)}
          type="button"
          title={paused || autoPaused ? "Resume 15-second refresh" : "Pause refresh"}
          aria-label={paused || autoPaused ? "Resume refresh" : "Pause refresh"}
        >
          {paused || autoPaused ? <Play size={15} aria-hidden="true" /> : <Pause size={15} aria-hidden="true" />}
        </button>
      </div>
    </div>
  );
}
