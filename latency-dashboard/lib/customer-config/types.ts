export type FixtureScenario =
  | "active"
  | "pending"
  | "failed"
  | "conflict"
  | "empty"
  | "unlinked"
  | "missing-wallet";

export type ApplyLifecycle = "active" | "editing" | "applying" | "pending" | "failed" | "conflict";

export interface TargetWallet {
  id: string;
  label: string;
  address: string;
  enabled: boolean;
  copiedTrades: number;
  amountOverrideSol: number | null;
}

export interface ExitLevel {
  id: string;
  triggerPercent: number;
  sellPercent: number;
}

export interface TradingWallet {
  address: string;
  balanceSol: number;
  ready: boolean;
}

export interface CustomerConfig {
  revision: number;
  telegramLinked: boolean;
  telegramHandle: string;
  copyTradingEnabled: boolean;
  amountPerBuySol: number;
  maxDailyBuys: number;
  maxPositionSol: number;
  buySlippagePercent: number;
  sellSlippagePercent: number;
  priorityFeeSol: number;
  stopLossEnabled: boolean;
  stopLossPercent: number;
  trailingStopEnabled: boolean;
  trailingStopPercent: number;
  exitLevels: ExitLevel[];
  targets: TargetWallet[];
  devSnipingEnabled: boolean;
  blockDevSelling: boolean;
  alerts: {
    copiedBuy: boolean;
    copiedSell: boolean;
    positionWarning: boolean;
    runtimeFailure: boolean;
    dailySummary: boolean;
  };
  tradingWallet: TradingWallet | null;
  cashback: {
    referralCode: string;
    earnedSol: number;
    invitedUsers: number;
  };
}

export interface ConfigDiff {
  id: string;
  group: string;
  label: string;
  before: string;
  after: string;
  warning?: string;
}

export interface ConfigActivity {
  id: string;
  type: "configuration" | "trade";
  title: string;
  detail: string;
  status: "active" | "pending" | "landed" | "failed";
  occurredAt: string;
}

export interface StoredFixtureState {
  scenario: FixtureScenario;
  config: CustomerConfig;
  activity: ConfigActivity[];
}

export interface ApplyRequest {
  scenario: FixtureScenario;
  expectedRevision: number;
  config: CustomerConfig;
}

export interface ApplyResult {
  operationId: string;
  status: "active" | "pending" | "failed" | "conflict";
  config?: CustomerConfig;
  message?: string;
}

export interface OperationResult {
  operationId: string;
  status: "active" | "pending" | "failed";
}

export interface HermesConfigClient {
  load(): Promise<StoredFixtureState>;
  selectScenario(scenario: FixtureScenario): Promise<StoredFixtureState>;
  apply(request: ApplyRequest): Promise<ApplyResult>;
  status(operationId: string): Promise<OperationResult>;
}
