"use client";

import {
  FILTER_OUTCOME_OPTIONS,
  TIME_RANGE_OPTIONS,
  type DashboardFilterState,
  type DashboardTimeRange,
  type LandingPreset
} from "@/lib/dashboard-client";

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
      {showOutcomePresets ? <div className={styles.presetWrap} role="radiogroup" aria-label="Execution preset">
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
            {preset.value === "all" ? "All tape" : preset.label}
          </button>
        ))}
      </div> : null}
      <label className={styles.rangeControl}>
        History
        <select
          value={filters.since || "custom"}
          onChange={(event) => onFiltersChange({
            since: event.target.value === "custom" ? "" : event.target.value as DashboardTimeRange,
            from: "",
            to: ""
          })}
          disabled={disabled}
          aria-label="History range"
        >
          {TIME_RANGE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
          <option value="custom">Custom dates</option>
        </select>
      </label>
      <details className={styles.advancedFilters} open={!showOutcomePresets}>
        <summary>Advanced filters</summary>
        <div className={styles.filterRow}>
        {showField("from") ? <label className={styles.filterItem}>
          From
          <input
            value={filters.from}
            onChange={(event) => onFiltersChange({ from: event.target.value })}
            placeholder="RFC3339 or epoch ms"
            type="text"
            disabled={disabled}
            aria-label="From"
          />
        </label> : null}
        {showField("to") ? <label className={styles.filterItem}>
          To
          <input
            value={filters.to}
            onChange={(event) => onFiltersChange({ to: event.target.value })}
            placeholder="RFC3339 or epoch ms"
            type="text"
            disabled={disabled}
            aria-label="To"
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
        {showField("side") ? <label className={styles.filterItem}>
          Side
          <select
            value={filters.side}
            onChange={(event) => onFiltersChange({ side: event.target.value })}
            disabled={disabled}
            aria-label="Side filter"
          >
            <option value="">all</option>
            <option value="buy">buy</option>
            <option value="sell">sell</option>
          </select>
        </label> : null}
        {showField("wallet") ? <label className={styles.filterItem}>
          Wallet
          <input
            value={filters.wallet}
            onChange={(event) => onFiltersChange({ wallet: event.target.value })}
            placeholder="wallet"
            type="text"
            disabled={disabled}
            aria-label="Wallet"
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
      </details>
    </section>
  );
}
