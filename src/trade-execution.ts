export const TRADE_EXECUTION_PROVIDERS = [
  "pumpportal-lightning",
  "direct-pump",
  "direct-pumpswap",
  "direct-auto"
] as const;

export type TradeExecutionProvider = (typeof TRADE_EXECUTION_PROVIDERS)[number];
export type DirectTradeExecutionProvider = "direct-pump" | "direct-pumpswap" | "direct-auto";
export type TradeAction = "buy" | "sell";
export type TradeAmountBasis = "sol" | "token" | "percent";
export type TradeExecutionStatus = "skipped" | "failed" | "simulated" | "submitted" | "confirmed" | "expired";

export interface TradeExecutionPlatformFee {
  enabled: boolean;
  bps: number;
  treasury: string | null;
  budgetLamports: bigint;
  feeLamports: bigint;
  tradeLamports: bigint;
}

export interface TradeExecutionRequest {
  provider: TradeExecutionProvider;
  action: TradeAction;
  mint: string;
  amount: number | string;
  amountBasis: TradeAmountBasis;
  slippagePercent: number;
  priorityFeeSol: number;
  walletPublicKey: string;
  pool?: string | null;
  source?: string | null;
  signalSignature?: string | null;
  platformFee?: TradeExecutionPlatformFee | null;
  metadata?: Record<string, unknown>;
}

export interface DirectRouteMetadata {
  provider: DirectTradeExecutionProvider;
  route: "pump-bonding-curve" | "pumpswap-amm" | "auto";
  mint: string;
  walletPublicKey: string;
  poolAddress: string | null;
  priorityFeeSol: number | null;
  slippagePercent: number | null;
  amount: number | string | null;
  amountBasis: TradeAmountBasis | null;
}

export interface TradeExecutionResult {
  ok: boolean;
  status: TradeExecutionStatus;
  provider: TradeExecutionProvider;
  route: DirectRouteMetadata["route"] | "pumpportal-lightning" | null;
  signature: string | null;
  errorText: string | null;
  raw: unknown;
  submittedAtMs: number | null;
  confirmedAtMs: number | null;
  slot: number | null;
  metadata: Record<string, unknown>;
  platformFee?: TradeExecutionPlatformFee | null;
}

export interface DirectExecutionTimingMetadata {
  stages: Array<{ stage: string; atMs: number; [key: string]: unknown }>;
  atMs: Record<string, number>;
  startedAtMs: number | null;
  finishedAtMs: number | null;
  blockhashStartedAtMs: number | null;
  blockhashFinishedAtMs: number | null;
  signStartedAtMs: number | null;
  signFinishedAtMs: number | null;
  simulationStartedAtMs: number | null;
  simulationFinishedAtMs: number | null;
  rawSendStartedAtMs: number | null;
  rawSendFinishedAtMs: number | null;
  signatureReturnedAtMs: number | null;
  confirmationStartedAtMs: number | null;
  confirmationFinishedAtMs: number | null;
  totalMs: number | null;
  timeToSignatureMs: number | null;
  signatureToConfirmationMs: number | null;
  blockhashMs: number | null;
  signingMs: number | null;
  simulationMs: number | null;
  rawSendMs: number | null;
  confirmationMs: number | null;
  timeToConfirmationMs: number | null;
  simulateBeforeSend: boolean | null;
  skipPreflight: boolean | null;
  maxRetries: number | null;
  blockhashCacheMs: number | null;
  rawSendRpcCount: number | null;
  rawSendWinner: string | null;
  rawSendErrors: Array<{ label: string; errorText: string }> | null;
  instructionCount: number | null;
  txBytes: number | null;
  unitsConsumed: number | null;
  blockhash: string | null;
  lastValidBlockHeight: number | null;
}

export interface DirectExecutionGateConfig {
  provider: TradeExecutionProvider;
  copyTradeEnabled: boolean;
  copyTradeDryRun: boolean;
  directExecutionEnabled?: boolean;
  directExecutionLiveEnabled?: boolean;
  directExecutionBuildOnly?: boolean;
  directExecutionSimulateOnly?: boolean;
  emergencyStopped?: boolean;
}

export function normalizeTradeExecutionProvider(value: string | null | undefined): TradeExecutionProvider | null {
  const normalized = value?.trim().toLowerCase();
  if (!normalized) {
    return null;
  }

  return TRADE_EXECUTION_PROVIDERS.includes(normalized as TradeExecutionProvider)
    ? (normalized as TradeExecutionProvider)
    : null;
}

export function parseTradeExecutionProvider(
  value: string | null | undefined,
  fallback: TradeExecutionProvider = "pumpportal-lightning"
): TradeExecutionProvider {
  return normalizeTradeExecutionProvider(value) || fallback;
}

export function tradeExecutionProviderConfigError(value: string | null | undefined): string | null {
  if (!value?.trim()) {
    return null;
  }

  return normalizeTradeExecutionProvider(value)
    ? null
    : `unsupported trade execution provider "${value}"; expected ${TRADE_EXECUTION_PROVIDERS.join(", ")}`;
}

export function isDirectTradeExecutionProvider(provider: TradeExecutionProvider): provider is DirectTradeExecutionProvider {
  return provider === "direct-pump" || provider === "direct-pumpswap" || provider === "direct-auto";
}

export function isSpecificDirectProvider(provider: TradeExecutionProvider): provider is Exclude<DirectTradeExecutionProvider, "direct-auto"> {
  return provider === "direct-pump" || provider === "direct-pumpswap";
}

export function directExecutionLiveBlockedReason(config: DirectExecutionGateConfig): string | null {
  if (config.emergencyStopped) {
    return "copy trade emergency stop is active";
  }

  if (!isDirectTradeExecutionProvider(config.provider)) {
    return `trade execution provider ${config.provider} is not a direct provider`;
  }

  if (!config.copyTradeEnabled) {
    return "COPY_TRADE_ENABLED is not true";
  }

  if (config.copyTradeDryRun) {
    return "COPY_TRADE_DRY_RUN is enabled";
  }

  if (!config.directExecutionEnabled) {
    return "DIRECT_EXECUTION_ENABLED is not true";
  }

  if (!config.directExecutionLiveEnabled) {
    return "DIRECT_EXECUTION_LIVE_ENABLED is not true";
  }

  if (config.directExecutionBuildOnly) {
    return "direct execution build-only mode is enabled";
  }

  if (config.directExecutionSimulateOnly) {
    return "direct execution simulate-only mode is enabled";
  }

  return null;
}

export function directExecutionLiveEnabled(config: DirectExecutionGateConfig): boolean {
  return directExecutionLiveBlockedReason(config) === null;
}

export function routeForDirectProvider(provider: DirectTradeExecutionProvider): DirectRouteMetadata["route"] {
  if (provider === "direct-pump") {
    return "pump-bonding-curve";
  }

  if (provider === "direct-pumpswap") {
    return "pumpswap-amm";
  }

  return "auto";
}

export function buildDirectRouteMetadata({
  provider,
  mint,
  walletPublicKey,
  poolAddress = null,
  priorityFeeSol = null,
  slippagePercent = null,
  amount = null,
  amountBasis = null
}: {
  provider: DirectTradeExecutionProvider;
  mint: string;
  walletPublicKey: string;
  poolAddress?: string | null;
  priorityFeeSol?: number | null;
  slippagePercent?: number | null;
  amount?: number | string | null;
  amountBasis?: TradeAmountBasis | null;
}): DirectRouteMetadata {
  return {
    provider,
    route: routeForDirectProvider(provider),
    mint,
    walletPublicKey,
    poolAddress,
    priorityFeeSol,
    slippagePercent,
    amount,
    amountBasis
  };
}

export function tradeExecutionSkippedResult({
  provider,
  route = null,
  reason,
  raw = null,
  metadata = {},
  platformFee = null
}: {
  provider: TradeExecutionProvider;
  route?: TradeExecutionResult["route"];
  reason: string;
  raw?: unknown;
  metadata?: Record<string, unknown>;
  platformFee?: TradeExecutionPlatformFee | null;
}): TradeExecutionResult {
  return {
    ok: false,
    status: "skipped",
    provider,
    route,
    signature: null,
    errorText: reason,
    raw,
    submittedAtMs: null,
    confirmedAtMs: null,
    slot: null,
    metadata,
    platformFee
  };
}

export function tradeExecutionFailedResult({
  provider,
  route = null,
  errorText,
  raw = null,
  metadata = {},
  platformFee = null
}: {
  provider: TradeExecutionProvider;
  route?: TradeExecutionResult["route"];
  errorText: string;
  raw?: unknown;
  metadata?: Record<string, unknown>;
  platformFee?: TradeExecutionPlatformFee | null;
}): TradeExecutionResult {
  return {
    ok: false,
    status: "failed",
    provider,
    route,
    signature: null,
    errorText,
    raw,
    submittedAtMs: null,
    confirmedAtMs: null,
    slot: null,
    metadata,
    platformFee
  };
}

export function formatTradeExecutionResultLog(result: TradeExecutionResult): string {
  const parts = [
    `provider=${result.provider}`,
    `status=${result.status}`,
    `route=${result.route || "unknown"}`,
    `signature=${result.signature || "none"}`
  ];

  if (result.platformFee) {
    parts.push(
      `platformFeeBps=${result.platformFee.bps}`,
      `platformFeeLamports=${result.platformFee.feeLamports.toString()}`,
      `tradeLamports=${result.platformFee.tradeLamports.toString()}`,
      `budgetLamports=${result.platformFee.budgetLamports.toString()}`,
      `platformFeeTreasury=${result.platformFee.treasury || "none"}`
    );
  }

  const timing = result.metadata.directSolanaTiming;
  if (timing && typeof timing === "object" && !Array.isArray(timing)) {
    const durations = (timing as {
      timeToSignatureMs?: unknown;
      confirmationMs?: unknown;
      timeToConfirmationMs?: unknown;
      rawSendMs?: unknown;
      rawSendWinner?: unknown;
      rawSendRpcCount?: unknown;
    });
    if (typeof durations.timeToSignatureMs === "number") {
      parts.push(`timeToSignatureMs=${durations.timeToSignatureMs}`);
    }
    if (typeof durations.rawSendMs === "number") {
      parts.push(`rawSendMs=${durations.rawSendMs}`);
    }
    if (typeof durations.rawSendWinner === "string") {
      parts.push(`rawSendWinner=${durations.rawSendWinner}`);
    }
    if (typeof durations.rawSendRpcCount === "number") {
      parts.push(`rawSendRpcCount=${durations.rawSendRpcCount}`);
    }
    if (typeof durations.confirmationMs === "number") {
      parts.push(`confirmationMs=${durations.confirmationMs}`);
    }
    if (typeof durations.timeToConfirmationMs === "number") {
      parts.push(`timeToConfirmationMs=${durations.timeToConfirmationMs}`);
    }
  }

  const buildTiming = result.metadata.directBuildTiming;
  if (buildTiming && typeof buildTiming === "object" && !Array.isArray(buildTiming)) {
    const totalMs = (buildTiming as { totalMs?: unknown }).totalMs;
    const stages = (buildTiming as { stages?: unknown }).stages;
    const buyAccounts = Array.isArray(stages)
      ? stages.find((record) => {
        return record &&
          typeof record === "object" &&
          (record as { stage?: unknown }).stage === "buy_accounts_ready";
      }) as Record<string, unknown> | undefined
      : undefined;
    const instructionsReady = Array.isArray(stages)
      ? stages.find((record) => {
        return record &&
          typeof record === "object" &&
          (record as { stage?: unknown }).stage === "instructions_ready";
      }) as Record<string, unknown> | undefined
      : undefined;

    if (typeof totalMs === "number") {
      parts.push(`directBuildMs=${totalMs}`);
    }
    if (typeof instructionsReady?.buyInstructionBuilder === "string") {
      parts.push(`directBuyBuilder=${instructionsReady.buyInstructionBuilder}`);
    }
    if (buyAccounts) {
      if (typeof buyAccounts.source === "string") {
        parts.push(`directBuyState=${buyAccounts.source}`);
      }
      if (typeof buyAccounts.cachedStateSource === "string") {
        parts.push(`directBuyStateSource=${buyAccounts.cachedStateSource}`);
      }
      if (typeof buyAccounts.cachedStateAgeMs === "number") {
        parts.push(`directBuyStateAgeMs=${buyAccounts.cachedStateAgeMs}`);
      }
      if (typeof buyAccounts.creatorSource === "string") {
        parts.push(`directCreatorSource=${buyAccounts.creatorSource}`);
      }
      if (typeof buyAccounts.creatorVerifiedAgeMs === "number") {
        parts.push(`directCreatorAgeMs=${buyAccounts.creatorVerifiedAgeMs}`);
      }
      if (typeof buyAccounts.tokenProgram === "string") {
        parts.push(`tokenProgram=${buyAccounts.tokenProgram}`);
      }
      if (buyAccounts.forceFreshBuyState === true) {
        parts.push("forceFreshBuyState=true");
      }
    }
  }

  if (result.errorText) {
    parts.push(`error=${result.errorText}`);
  }

  return parts.join(" | ");
}
