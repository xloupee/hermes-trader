"use client";

import { FILTER_OUTCOME_OPTIONS, type DashboardFilterState, type LandingPreset } from "@/lib/dashboard-client";

import styles from "@/components/dashboard/dashboard-shared.module.css";

type DashboardFilterField = Exclude<keyof DashboardFilterState, "outcome">;

interface DashboardFiltersProps {
  filters: DashboardFilterState;
  onFiltersChange: (filters: Partial<DashboardFilterState>) => void;
  onOutcomeChange: (outcome: LandingPreset) => void;
  disabled?: boolean;
  visibleFields?: readonly DashboardFilterField[];
  showOutcomePresets?: boolean;
}

export function DashboardFiltersPanel({
  filters,
  onFiltersChange,
  onOutcomeChange,
  disabled,
  visibleFields,
  showOutcomePresets = true
}: DashboardFiltersProps) {
  const showField = (field: DashboardFilterField) => !visibleFields || visibleFields.includes(field);

  return (
    <section className={styles.filterWrap} aria-label="Dashboard filters">
      <div className={styles.filterRow}>
        {showField("since") ? <label className={styles.filterItem}>
          Since
          <input
            value={filters.since}
            onChange={(event) => onFiltersChange({ since: event.target.value })}
            placeholder="24h, 7d, 2026-07-31"
            type="text"
            disabled={disabled}
            aria-label="Since"
          />
        </label> : null}
        {showField("provider") ? <label className={styles.filterItem}>
          Provider
          <input
            value={filters.provider}
            onChange={(event) => onFiltersChange({ provider: event.target.value })}
            placeholder="provider"
            type="text"
            disabled={disabled}
            aria-label="Provider"
          />
        </label> : null}
        {showField("route") ? <label className={styles.filterItem}>
          Route
          <input
            value={filters.route}
            onChange={(event) => onFiltersChange({ route: event.target.value })}
            placeholder="route"
            type="text"
            disabled={disabled}
            aria-label="Route"
          />
        </label> : null}
        {showField("action") ? <label className={styles.filterItem}>
          Action
          <select
            value={filters.action}
            onChange={(event) => onFiltersChange({ action: event.target.value })}
            disabled={disabled}
            aria-label="Action filter"
          >
            <option value="">all</option>
            <option value="buy">buy</option>
            <option value="sell">sell</option>
          </select>
        </label> : null}
        {showField("observedWallet") ? <label className={styles.filterItem}>
          Observed wallet
          <input
            value={filters.observedWallet}
            onChange={(event) => onFiltersChange({ observedWallet: event.target.value })}
            placeholder="wallet"
            type="text"
            disabled={disabled}
            aria-label="Observed wallet"
          />
        </label> : null}
        {showField("mint") ? <label className={styles.filterItem}>
          CA
          <input
            value={filters.mint}
            onChange={(event) => onFiltersChange({ mint: event.target.value })}
            placeholder="mint"
            type="text"
            disabled={disabled}
            aria-label="Mint filter"
          />
        </label> : null}
        {showField("source") ? <label className={styles.filterItem}>
          Source
          <input
            value={filters.source}
            onChange={(event) => onFiltersChange({ source: event.target.value })}
            placeholder="signal source"
            type="text"
            disabled={disabled}
            aria-label="Source filter"
          />
        </label> : null}
      </div>
      {showOutcomePresets ? <div className={styles.presetWrap} role="radiogroup" aria-label="Landed preset">
        {FILTER_OUTCOME_OPTIONS.map((preset) => (
          <button
            className={filters.outcome === preset.value ? styles.presetActive : styles.presetButton}
            key={preset.value}
            onClick={() => onOutcomeChange(preset.value)}
            type="button"
            role="radio"
            aria-checked={filters.outcome === preset.value}
            disabled={disabled}
          >
            {preset.label}
          </button>
        ))}
      </div> : null}
    </section>
  );
}
