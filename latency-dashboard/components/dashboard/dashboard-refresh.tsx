"use client";

import { CirclePause, CirclePlay, RefreshCcw } from "lucide-react";

import styles from "@/components/dashboard/dashboard-shared.module.css";

interface DashboardRefreshToolbarProps {
  loading: boolean;
  error: string | null;
  paused: boolean;
  autoPaused: boolean;
  lastUpdated: Date | null;
  onRefresh: () => void;
  onTogglePause: (next: boolean) => void;
}

export function DashboardRefreshToolbar({
  loading,
  error,
  paused,
  autoPaused,
  lastUpdated,
  onRefresh,
  onTogglePause
}: DashboardRefreshToolbarProps) {
  return (
    <div className={styles.toolbar}>
      <div className={styles.toolbarStatus}>
        <span className={styles.badge}>{loading ? "loading..." : "ready"}</span>
        <span className={styles.muted}>
          {!error ? `updated ${lastUpdated ? lastUpdated.toLocaleTimeString() : "never"}` : `error: ${error}`}
        </span>
      </div>
      <div className={styles.toolbarButtons}>
        <button
          className="icon-button"
          onClick={onRefresh}
          type="button"
          title="Refresh now"
          aria-label="Refresh now"
        >
          <RefreshCcw size={16} />
        </button>
        <button
          className="icon-button"
          onClick={() => onTogglePause(!paused)}
          type="button"
          title={paused || autoPaused ? "Resume 15-second refresh" : "Pause refresh"}
          aria-label={paused || autoPaused ? "Resume refresh" : "Pause refresh"}
        >
          {paused || autoPaused ? <CirclePlay size={16} /> : <CirclePause size={16} />}
        </button>
      </div>
    </div>
  );
}

