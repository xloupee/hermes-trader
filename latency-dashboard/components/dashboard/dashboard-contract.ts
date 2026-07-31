import type { Route } from "next";

export type LandingPreset = "all" | "landed-buys" | "landed-sells" | "non-landed";

export interface DashboardFilterState {
  since: string;
  provider: string;
  targetWallet: string;
  mint: string;
  action: string;
  route: string;
  source: string;
  outcome: LandingPreset;
}

export interface ExecutionSummary {
  total: number;
  sent: number;
  landed: number;
  failedOnChain: number;
  autoSellLanded: number;
  autoSellFailedOnChain: number;
  skipped: number;
  errors: number;
  avgSignatureMs: number | null;
  avgSlotDelta: number | null;
  totalGrossSpendSol: number | null;
  totalExtraSpendSol: number | null;
}

export interface SignalSummary {
  total: number;
  buys: number;
  sells: number;
  copyable: number;
}

export interface ExecutionListRow {
  id: number;
  createdAt: string;
  observedAtMs: number;
  provider: string;
  source: string;
  endpoint: string | null;
  observedWallet: string;
  copyWallet: string | null;
  observedSignature: string;
  sendSignature: string | null;
  slot: number;
  copySlot: number | null;
  slotDeltaFromObserved: number | null;
  targetSlot: number | null;
  targetTxIndex: number | null;
  copyTxIndex: number | null;
  sameSlotTxDelta: number | null;
  slotDelta: number | null;
  txDelta: number | null;
  positionUnavailableReason: string | null;
  selectedRoute: string;
  routeLayout: string | null;
  mint: string;
  observedAction: string;
  observedSolAmount: number | null;
  maxCopySol: number | null;
  decision: string;
  buyStatus: string | null;
  buyChainError: unknown;
  autoSellEnabled: boolean;
  autoSellStatus: string | null;
  autoSellChainError: unknown;
  autoSellReason: string | null;
  observedToSignedMs: number | null;
  observedToSendSubmittedMs: number | null;
  observedToSignatureReturnedMs: number | null;
  sendLaneAttempts: Array<{
    label: string | null;
    kind: string | null;
    mode: string | null;
    status: string | null;
    durationMs: number | null;
  }>;
  sent: boolean;
  grossCopySpendSol: number | null;
  networkFeeSol: number | null;
  extraSpendBeyondObservedAndNetworkFeeSol: number | null;
  sourcePositionBucket: string | null;
  rawExecution: unknown;
  chainReport: unknown;
}

export interface ExecutionResponse {
  executions: ExecutionListRow[];
  summary: ExecutionSummary;
  filters: Record<string, string>;
}

export interface SignalResponseRow {
  id: number;
  createdAt: string;
  observedAtMs: number;
  provider: string;
  source: string;
  endpoint: string | null;
  targetWallet: string;
  observedWallet?: string;
  signature: string;
  slot: number;
  action: string;
  mint: string;
  route: string;
  observedMinusBlockTimeMs: number | null;
  localDetectMs: number | null;
}

export interface SignalResponse {
  signals: SignalResponseRow[];
  summary: SignalSummary;
  filters: Record<string, string>;
}

export interface ObservationsResponse {
  signals: SignalResponseRow[];
  filters: Record<string, string>;
}

export interface LatencySummary {
  total: number;
  submitted: number;
  failed: number;
  targetObserved: {
    p50: number | null;
    p90: number | null;
  };
  blocktime: {
    p50: number | null;
    p90: number | null;
  };
}

export interface LatencyResponse {
  summary: LatencySummary;
  filters: Record<string, string>;
}

export interface BenchmarkDetailRow {
  id: string;
  createdAt: string;
  observedAtMs: number;
  provider: string;
  source: string;
  endpoint: string | null;
  targetWallet: string;
  signature: string;
  slot: number;
  action: string;
  mint: string;
  route: string;
  copyWallet: string | null;
  telegramSubscriber: string | null;
  copyable: boolean;
  signal: SignalResponseRow | null;
  execution: ExecutionListRow | null;
  signalObservationId: number | null;
}

export interface BenchmarkResponse {
  row: BenchmarkDetailRow;
}

export interface MeResponse {
  user: {
    id: string;
    email: string | null;
  };
}

export interface RouteDefinition {
  href: Route;
  label: string;
}

export const DASHBOARD_NAV: RouteDefinition[] = [
  { href: "/dashboard/overview", label: "Overview" },
  { href: "/dashboard/executions", label: "Executions" },
  { href: "/dashboard/sources", label: "Sources" },
  { href: "/dashboard/system", label: "System" }
];

export const DEFAULT_FILTERS: DashboardFilterState = {
  since: "24h",
  provider: "",
  targetWallet: "",
  mint: "",
  action: "",
  route: "",
  source: "",
  outcome: "all"
};

export const FILTER_OUTCOME_OPTIONS: { value: LandingPreset; label: string }[] = [
  { value: "all", label: "All" },
  { value: "landed-buys", label: "Landed Buys" },
  { value: "landed-sells", label: "Landed Sells" },
  { value: "non-landed", label: "Non-landed Attempts" }
];

function normalizeOutcome(value: string | null): LandingPreset {
  if (value === "landed-buys" || value === "landed-sells" || value === "non-landed") {
    return value;
  }
  return "all";
}

export function parseDashboardFilters(searchParams: URLSearchParams | null): DashboardFilterState {
  return {
    since: searchParams?.get("since")?.trim() || DEFAULT_FILTERS.since,
    provider: searchParams?.get("provider")?.trim() || DEFAULT_FILTERS.provider,
    targetWallet: searchParams?.get("targetWallet")?.trim() || DEFAULT_FILTERS.targetWallet,
    mint: searchParams?.get("mint")?.trim() || DEFAULT_FILTERS.mint,
    action: searchParams?.get("action")?.trim() || DEFAULT_FILTERS.action,
    route: searchParams?.get("route")?.trim() || DEFAULT_FILTERS.route,
    source: searchParams?.get("source")?.trim() || DEFAULT_FILTERS.source,
    outcome: normalizeOutcome(searchParams?.get("outcome") ?? null),
  };
}

export function toQueryParams(filters: Partial<DashboardFilterState>, includeOutcome = false): string {
  const normalized = {
    ...DEFAULT_FILTERS,
    ...filters
  };
  const params = new URLSearchParams();
  if (normalized.since.trim()) {
    params.set("since", normalized.since.trim());
  }
  if (normalized.provider.trim()) {
    params.set("provider", normalized.provider.trim());
  }
  if (normalized.targetWallet.trim()) {
    params.set("targetWallet", normalized.targetWallet.trim());
  }
  if (normalized.mint.trim()) {
    params.set("mint", normalized.mint.trim());
  }
  if (normalized.action.trim()) {
    params.set("action", normalized.action.trim());
  }
  if (normalized.route.trim()) {
    params.set("route", normalized.route.trim());
  }
  if (normalized.source.trim()) {
    params.set("source", normalized.source.trim());
  }
  if (includeOutcome && normalized.outcome && normalized.outcome !== "all") {
    params.set("outcome", normalized.outcome);
  }
  return params.toString();
}

export function isLandedBuy(row: Pick<ExecutionListRow, "buyStatus" | "autoSellStatus">): boolean {
  const buyStatus = (row.buyStatus || "").toLowerCase();
  return buyStatus.includes("landed") || buyStatus === "buyno-target" || buyStatus === "no-target";
}

export function isLandedSell(row: Pick<ExecutionListRow, "autoSellStatus" | "buyStatus">): boolean {
  const autoSellStatus = (row.autoSellStatus || "").toLowerCase();
  if (autoSellStatus === "autoselllanded" || autoSellStatus.includes("landed")) {
    return true;
  }
  return false;
}

export function hasNoTarget(row: Pick<ExecutionListRow, "buyStatus">): boolean {
  return (row.buyStatus || "").toLowerCase() === "no-target" || (row.buyStatus || "").toLowerCase() === "buyno-target";
}

export function landingSummary(row: Pick<ExecutionListRow, "buyStatus" | "autoSellStatus" | "decision" | "observedAction">): string {
  if (isLandedBuy(row)) {
    return row.observedAction === "buy" ? "Buy landed" : "Landed";
  }
  if (isLandedSell(row)) {
    return "Sell landed";
  }
  if (hasNoTarget(row)) {
    return "No target";
  }
  return row.decision || "Pending";
}

export function hasNonLandedAttempt(row: Pick<ExecutionListRow, "sent" | "buyStatus" | "autoSellStatus">): boolean {
  return row.sent === true && !isLandedBuy(row) && !isLandedSell(row);
}

export function applyLandingPreset(rows: ExecutionListRow[], outcome: LandingPreset): ExecutionListRow[] {
  if (outcome === "all") {
    return rows;
  }
  if (outcome === "landed-buys") {
    return rows.filter((row) => isLandedBuy(row) && row.observedAction === "buy");
  }
  if (outcome === "landed-sells") {
    return rows.filter((row) => isLandedSell(row) && row.observedAction === "sell");
  }
  return rows.filter((row) => hasNonLandedAttempt(row));
}

export function formatCount(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return "n/a";
  }
  return String(value);
}

export function formatPercent(numerator: number, denominator: number): string {
  if (!Number.isFinite(numerator) || !Number.isFinite(denominator) || denominator <= 0) {
    return "n/a";
  }
  return `${Math.round((numerator / denominator) * 100)}%`;
}

export function formatSol(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return "n/a";
  }
  return `${value.toFixed(6)} SOL`;
}

export function formatMs(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return "n/a";
  }
  if (value >= 1000) {
    return `${Math.round(value / 10) / 100}s`;
  }
  return `${value}ms`;
}

export function shortText(value: string | null | undefined, size = 6): string {
  if (!value) {
    return "n/a";
  }
  if (value.length <= size * 2 + 2) {
    return value;
  }
  return `${value.slice(0, size)}…${value.slice(-size)}`;
}

export function formatSlot(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return "n/a";
  }
  if (value === 0) {
    return "same slot";
  }
  return `${value > 0 ? "+" : ""}${value} slot${Math.abs(value) === 1 ? "" : "s"}`;
}

export function formatLastSeen(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return "n/a";
  }
  return date.toLocaleTimeString();
}
