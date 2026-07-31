import type {
  DashboardExecutionCursor,
  DashboardExecutionFilters,
  LocalExecutionReport
} from "./local-executions";

export type ExecutionOutcome =
  | "landed"
  | "failed_on_chain"
  | "ack_not_landed"
  | "send_failed"
  | "skipped"
  | "unknown";

export type LandingComparison =
  | "same_slot"
  | "cross_slot"
  | "no_target"
  | "unavailable";

export type DashboardExecution = Omit<LocalExecutionReport, "observedWallet" | "copyWallet"> & {
  observedWallet: string | null;
  copyWallet: string | null;
  outcome: ExecutionOutcome;
  landingComparison: LandingComparison;
};

export interface ExecutionSummary {
  total: number;
  landed: number;
  outcome: Record<ExecutionOutcome, number>;
  landingComparison: Record<LandingComparison, number>;
  totalGrossCopySpendSol: number;
  totalExtraSpendSol: number;
}

export interface DashboardExecutionsResponse {
  executions: DashboardExecution[];
  summary: ExecutionSummary;
  pagination: {
    limit: number;
    hasMore: boolean;
    nextCursor: string | null;
  };
  filters: DashboardExecutionFilters;
}

export interface DashboardOverviewResponse {
  summary: ExecutionSummary;
  latestObservedAtMs: number | null;
  sourcesObserved: number;
  executions: DashboardExecution[];
  filters: DashboardExecutionFilters;
}

export interface DashboardSource {
  source: string;
  provider: string;
  count: number;
  latestObservedAtMs: number;
}

export interface DashboardSourcesResponse {
  sources: DashboardSource[];
  filters: DashboardExecutionFilters;
}

export interface DashboardSystemResponse {
  time: string;
  tables: {
    copytradeLocalExecutions: number | null;
    copytradeSignalObservations: number | null;
  };
  environment: {
    supabaseUrl: boolean;
    hasServiceRole: boolean;
  };
}

export function sanitizeWallet(address: unknown): string | null;
export function parseExecutionFilters(searchParams: URLSearchParams): DashboardExecutionFilters;
export function encodeExecutionCursor(cursor: DashboardExecutionCursor): string | null;
export function decodeExecutionCursor(cursor: string | null | undefined): DashboardExecutionCursor | null;
export function executionOutcomeForRow(row: Partial<LocalExecutionReport> | null | undefined): ExecutionOutcome;
export function landingComparisonForRow(row: Partial<LocalExecutionReport> | null | undefined): LandingComparison;
export function toDashboardExecution(row: LocalExecutionReport): DashboardExecution;
export function summarizeExecutions(
  rows: Array<Partial<LocalExecutionReport> | DashboardExecution>
): ExecutionSummary;

export const dashboardContractSchema: {
  outcomes: ExecutionOutcome[];
  landingComparisons: LandingComparison[];
  defaultLimit: 50;
  maxLimit: 100;
};
