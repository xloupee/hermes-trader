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

export type LandingPreset = "all" | "landed-buys" | "landed-sells" | "non-landed";

export interface DashboardFilterState {
  since: string;
  provider: string;
  observedWallet: string;
  mint: string;
  action: string;
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
  since: "24h",
  provider: "",
  observedWallet: "",
  mint: "",
  action: "",
  route: "",
  source: "",
  outcome: "all"
};

export const FILTER_OUTCOME_OPTIONS: Array<{ value: LandingPreset; label: string }> = [
  { value: "all", label: "All" },
  { value: "landed-buys", label: "Landed Buys" },
  { value: "landed-sells", label: "Landed Sells" },
  { value: "non-landed", label: "Non-landed Attempts" }
];

function normalizeOutcome(value: string | null): LandingPreset {
  return value === "landed-buys" || value === "landed-sells" || value === "non-landed" ? value : "all";
}

export function parseDashboardFilters(searchParams: URLSearchParams | null): DashboardFilterState {
  return {
    since: searchParams?.get("since")?.trim() || DEFAULT_FILTERS.since,
    provider: searchParams?.get("provider")?.trim() || "",
    observedWallet: searchParams?.get("observedWallet")?.trim() || "",
    mint: searchParams?.get("mint")?.trim() || "",
    action: searchParams?.get("action")?.trim() || "",
    route: searchParams?.get("route")?.trim() || "",
    source: searchParams?.get("source")?.trim() || "",
    outcome: normalizeOutcome(searchParams?.get("outcome") ?? null)
  };
}

export function toQueryParams(filters: Partial<DashboardFilterState>): string {
  const normalized = { ...DEFAULT_FILTERS, ...filters };
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries({
    since: normalized.since,
    provider: normalized.provider,
    observedWallet: normalized.observedWallet,
    mint: normalized.mint,
    action: normalized.action,
    route: normalized.route,
    source: normalized.source
  })) {
    if (value.trim()) params.set(key, value.trim());
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
  if (preset === "non-landed") return rows.filter(hasNonLandedAttempt);
  return rows;
}

export function landingSummary(row: DashboardExecution): string {
  if (isLandedBuy(row)) return "Buy landed";
  if (isLandedSell(row)) return "Sell landed";
  return row.outcome.replaceAll("_", " ");
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
