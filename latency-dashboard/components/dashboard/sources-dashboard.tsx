"use client";

import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";
import {
  type ExecutionResponse,
  type ObservationsResponse,
  applyLandingPreset,
  isLandedBuy,
  isLandedSell,
  toQueryParams,
  type LandingPreset
} from "./dashboard-contract";
import { useDashboardFilters } from "./use-dashboard-filters";
import { DashboardFiltersPanel } from "@/components/dashboard/dashboard-filters";
import { DashboardRefreshToolbar } from "@/components/dashboard/dashboard-refresh";
import styles from "@/components/dashboard/dashboard-shared.module.css";

interface SourceStatRow {
  source: string;
  signals: number;
  buys: number;
  sells: number;
  landedBuys: number;
  landedSells: number;
  nonLandedAttempts: number;
  lastSeen: string;
}

interface SourcesPayload {
  signals: ObservationsResponse;
  executions: ExecutionResponse;
}

function buildQuery(filters: {
  since: string;
  provider: string;
  targetWallet: string;
  mint: string;
  action: string;
  route: string;
  source: string;
}) {
  return toQueryParams({
    since: filters.since,
    provider: filters.provider,
    targetWallet: filters.targetWallet,
    mint: filters.mint,
    action: filters.action,
    route: filters.route,
    source: filters.source,
    outcome: "all" as LandingPreset
  });
}

export function SourcesDashboard() {
  const { filters, setFilters, setOutcome } = useDashboardFilters();
  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery<SourcesPayload>(
    async (): Promise<SourcesPayload> => {
      const query = buildQuery(filters);
      const [signals, executions] = await Promise.all([
        fetch(`/api/signals/observations?${query}`).then((response) => {
          if (!response.ok) {
            throw new Error("Could not load source signals");
          }
          return response.json() as Promise<ObservationsResponse>;
        }),
        fetch(`/api/signals/executions?${query}`).then((response) => {
          if (!response.ok) {
            throw new Error("Could not load source executions");
          }
          return response.json() as Promise<ExecutionResponse>;
        })
      ]);
      return { signals, executions };
    },
    { intervalMs: 15000 }
  );

  const rows = data
    ? applyLandingPreset((data.executions.executions || []), filters.outcome)
    : [];
  const sourceMap = new Map<string, SourceStatRow>();

  for (const row of data?.signals.signals || []) {
    const source = row.source || "n/a";
    const entry = sourceMap.get(source) ?? {
      source,
      signals: 0,
      buys: 0,
      sells: 0,
      landedBuys: 0,
      landedSells: 0,
      nonLandedAttempts: 0,
      lastSeen: row.createdAt
    };
    entry.signals += 1;
    if (row.action === "buy") {
      entry.buys += 1;
    }
    if (row.action === "sell") {
      entry.sells += 1;
    }
    if (new Date(row.createdAt).getTime() > new Date(entry.lastSeen).getTime()) {
      entry.lastSeen = row.createdAt;
    }
    sourceMap.set(source, entry);
  }

  for (const row of rows) {
    const source = row.source || "n/a";
    const entry = sourceMap.get(source) ?? {
      source,
      signals: 0,
      buys: 0,
      sells: 0,
      landedBuys: 0,
      landedSells: 0,
      nonLandedAttempts: 0,
      lastSeen: row.createdAt
    };
    if (isLandedBuy(row)) {
      entry.landedBuys += 1;
    }
    if (isLandedSell(row)) {
      entry.landedSells += 1;
    }
    if (row.sent && !isLandedBuy(row) && !isLandedSell(row)) {
      entry.nonLandedAttempts += 1;
    }
    if (new Date(row.createdAt).getTime() > new Date(entry.lastSeen).getTime()) {
      entry.lastSeen = row.createdAt;
    }
    sourceMap.set(source, entry);
  }

  const sourceRows = [...sourceMap.values()].sort((left, right) => right.signals - left.signals);

  return (
    <section>
      <div className={styles.metricStrip}>
        <div className={styles.metric}>
          <span>sources</span>
          <strong>{sourceRows.length}</strong>
        </div>
        <div className={styles.metric}>
          <span>signals</span>
          <strong>{data?.signals.signals.length ?? 0}</strong>
        </div>
        <div className={styles.metric}>
          <span>executions</span>
          <strong>{data?.executions.summary.total ?? 0}</strong>
        </div>
        <div className={styles.metric}>
          <span>buy / sell</span>
          <strong>{(data?.signals.signals.filter((signal) => signal.action === "buy").length ?? 0)} / {(data?.signals.signals.filter((signal) => signal.action === "sell").length ?? 0)}</strong>
        </div>
      </div>
      <DashboardFiltersPanel
        filters={filters}
        onFiltersChange={setFilters}
        onOutcomeChange={setOutcome}
      />
      <DashboardRefreshToolbar
        loading={loading}
        error={error}
        paused={paused}
        autoPaused={autoPaused}
        lastUpdated={lastUpdated}
        onRefresh={refresh}
        onTogglePause={setPaused}
      />
      <div className={styles.dataSection}>
        <div className={styles.desktopTableWrap}>
          <table className={styles.dataTable}>
            <thead>
              <tr>
                <th>Source</th>
                <th>Signals</th>
                <th>Buy / Sell</th>
                <th>Landed buys</th>
                <th>Landed sells</th>
                <th>Non-landed attempts</th>
                <th>Latest</th>
                <th>Copy ratio</th>
              </tr>
            </thead>
            <tbody>
              {sourceRows.length === 0 ? (
                <tr>
                  <td colSpan={8}>
                    <div className={styles.emptyState}>No source rows match the current filter set.</div>
                  </td>
                </tr>
              ) : (
                sourceRows.map((row) => {
                  const denominator = Math.max(1, row.signals);
                  const ratio = `${Math.round((row.landedBuys / denominator) * 100)}%`;
                  return (
                    <tr key={row.source}>
                      <td>{row.source}</td>
                      <td>{row.signals}</td>
                      <td>{row.buys} / {row.sells}</td>
                      <td>{row.landedBuys}</td>
                      <td>{row.landedSells}</td>
                      <td>{row.nonLandedAttempts}</td>
                      <td>{new Date(row.lastSeen).toLocaleTimeString()}</td>
                      <td>{ratio}</td>
                    </tr>
                  );
                })
              )}
            </tbody>
          </table>
        </div>
        <div className={styles.mobileCards}>
          {sourceRows.length === 0 ? (
            <div className={styles.emptyState}>No source rows match the current filter set.</div>
          ) : sourceRows.map((row) => (
            <article className={styles.card} key={row.source}>
              <header>
                <h3>{row.source}</h3>
                <span>{row.signals} signals</span>
              </header>
              <p>Buy/Sell: {row.buys}/{row.sells}</p>
              <p>Landed buys: {row.landedBuys}</p>
              <p>Landed sells: {row.landedSells}</p>
              <p>Non-landed attempts: {row.nonLandedAttempts}</p>
              <p>Latest seen: {new Date(row.lastSeen).toLocaleString()}</p>
            </article>
          ))}
        </div>
      </div>
    </section>
  );
}
