import type { Route } from "next";
import type {
  DashboardExecution,
  DashboardExecutionsResponse,
  DashboardOverviewResponse,
  DashboardSourcesResponse,
  DashboardSystemResponse
} from "@/lib/dashboard-contract.mjs";

export type {
  DashboardExecution,
  DashboardExecutionsResponse,
  DashboardOverviewResponse,
  DashboardSourcesResponse,
  DashboardSystemResponse
};

export type LandingPreset = "all" | "landed-buys" | "landed-sells";

export interface DashboardFilterState {
  from: string;
  to: string;
  provider: string;
  wallet: string;
  mint: string;
  side: string;
  route: string;
  source: string;
  outcome: LandingPreset;
}

export interface MeResponse {
  id: string;
  user: { id: string; email: string | null };
  isAdmin: true;
}

export const DASHBOARD_NAV: Array<{ href: Route; label: string }> = [
  { href: "/dashboard", label: "Overview" },
  { href: "/dashboard/executions", label: "Executions" },
  { href: "/dashboard/sources", label: "Sources" },
  { href: "/dashboard/system", label: "System" }
];

export const DEFAULT_FILTERS: DashboardFilterState = {
  from: "",
  to: "",
  provider: "",
  wallet: "",
  mint: "",
  side: "",
  route: "",
  source: "",
  outcome: "all"
};

export const FILTER_OUTCOME_OPTIONS: Array<{ value: LandingPreset; label: string }> = [
  { value: "all", label: "All" },
  { value: "landed-buys", label: "Landed Buys" },
  { value: "landed-sells", label: "Landed Sells" }
];

function normalizeOutcome(side: string | null, outcome: string | null): LandingPreset {
  if (outcome === "landed" && side === "buy") return "landed-buys";
  if (outcome === "landed" && side === "sell") return "landed-sells";
  return "all";
}

export function parseDashboardFilters(searchParams: URLSearchParams | null): DashboardFilterState {
  return {
    from: searchParams?.get("from")?.trim() || "",
    to: searchParams?.get("to")?.trim() || "",
    provider: searchParams?.get("provider")?.trim() || "",
    wallet: searchParams?.get("wallet")?.trim() || "",
    mint: searchParams?.get("mint")?.trim() || "",
    side: searchParams?.get("side")?.trim() || "",
    route: searchParams?.get("route")?.trim() || "",
    source: searchParams?.get("source")?.trim() || "",
    outcome: normalizeOutcome(
      searchParams?.get("side") ?? null,
      searchParams?.get("outcome") ?? null
    )
  };
}

export function toQueryParams(filters: Partial<DashboardFilterState>, includeOutcome = false): string {
  const normalized = { ...DEFAULT_FILTERS, ...filters };
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries({
    from: normalized.from,
    to: normalized.to,
    provider: normalized.provider,
    wallet: normalized.wallet,
    mint: normalized.mint,
    side: normalized.side,
    route: normalized.route,
    source: normalized.source
  })) {
    if (value.trim()) params.set(key, value.trim());
  }
  if (includeOutcome && normalized.outcome === "landed-buys") {
    params.set("side", "buy");
    params.set("outcome", "landed");
  }
  if (includeOutcome && normalized.outcome === "landed-sells") {
    params.set("side", "sell");
    params.set("outcome", "landed");
  }
  return params.toString();
}

export function isLandedBuy(row: DashboardExecution): boolean {
  return row.observedAction === "buy" && row.outcome === "landed";
}

export function isLandedSell(row: DashboardExecution): boolean {
  return row.observedAction === "sell" && row.outcome === "landed";
}

export function hasNonLandedAttempt(row: DashboardExecution): boolean {
  return row.outcome === "ack_not_landed" || row.outcome === "failed_on_chain" || row.outcome === "send_failed";
}

export function applyLandingPreset(rows: DashboardExecution[], preset: LandingPreset): DashboardExecution[] {
  if (preset === "landed-buys") return rows.filter(isLandedBuy);
  if (preset === "landed-sells") return rows.filter(isLandedSell);
  return rows;
}

export function landingSummary(row: DashboardExecution): string {
  if (row.outcome === "landed") {
    const copySlot = row.copySlot ?? "n/a";
    if (row.landingComparison === "no_target") return `Landed · slot ${copySlot} · no target comparison`;
    if (row.landingComparison === "same_slot") {
      return `Landed · same slot · ${sameSlotTransactionPosition(row.sameSlotTxDelta)}`;
    }
    if (row.landingComparison === "cross_slot") {
      const slotDelta = typeof row.slotDelta === "number" && Number.isFinite(row.slotDelta)
        ? formatSlot(row.slotDelta)
        : "slot delta unavailable";
      return `Landed · ${slotDelta} · copy slot ${copySlot}`;
    }
    return `Landed · slot ${copySlot} · comparison unavailable`;
  }
  const labels = {
    failed_on_chain: "Failed on chain",
    ack_not_landed: "ACKed · not landed",
    send_failed: "Send failed",
    skipped: "Skipped",
    unknown: "Unknown"
  } as const;
  return labels[row.outcome];
}

function sameSlotTransactionPosition(txDelta: number | null): string {
  if (typeof txDelta !== "number" || !Number.isFinite(txDelta)) return "tx delta unavailable";
  if (txDelta > 0) return `${txDelta} tx after target`;
  if (txDelta < 0) return `${Math.abs(txDelta)} tx before target`;
  return "at target transaction";
}

export function landingComparisonSummary(row: DashboardExecution): string {
  if (row.landingComparison === "no_target") return "no target comparison";
  if (row.landingComparison === "unavailable") return "comparison unavailable";
  if (row.landingComparison === "same_slot") {
    return `same-slot · slot ${row.copySlot ?? "n/a"} · ${row.sameSlotTxDelta ?? "n/a"} tx`;
  }
  return `cross-slot · ${formatSlot(row.slotDelta)} · ${row.txDelta ?? "n/a"} tx`;
}

export interface OverviewMetricValues {
  landedBuys: number;
  landedSells: number;
  landingRate: string;
  nonLandedAttempts: number;
}

export function overviewMetricValues(
  summary: DashboardOverviewResponse["summary"],
  landedBuySummary: DashboardOverviewResponse["summary"],
  landedSellSummary: DashboardOverviewResponse["summary"]
): OverviewMetricValues {
  const nonLandedAttempts = summary.outcome.failed_on_chain
    + summary.outcome.ack_not_landed
    + summary.outcome.send_failed;
  return {
    landedBuys: landedBuySummary.outcome.landed,
    landedSells: landedSellSummary.outcome.landed,
    landingRate: formatPercent(summary.outcome.landed, summary.outcome.landed + nonLandedAttempts),
    nonLandedAttempts
  };
}

export function formatCount(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value) ? String(value) : "n/a";
}

export function formatPercent(numerator: number, denominator: number): string {
  return denominator > 0 ? `${Math.round((numerator / denominator) * 100)}%` : "n/a";
}

export function formatSol(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value) ? `${value.toFixed(6)} SOL` : "n/a";
}

export function formatMs(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "n/a";
  return value >= 1000 ? `${Math.round(value / 10) / 100}s` : `${value}ms`;
}

export function shortText(value: string | null | undefined, size = 6): string {
  if (!value) return "n/a";
  return value.length <= size * 2 + 2 ? value : `${value.slice(0, size)}...${value.slice(-size)}`;
}

export function formatSlot(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) return "n/a";
  if (value === 0) return "same slot";
  return `${value > 0 ? "+" : ""}${value} slot${Math.abs(value) === 1 ? "" : "s"}`;
}
