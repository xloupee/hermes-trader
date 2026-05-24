import type { PumpPortalLightningTradeRequest, WalletTradeData } from "./types.js";

export interface CopyTradeRiskControlConfig {
  copyTradeMaxBuySol: number;
  copyTradeDailySolCap: number;
  copyTradeMaxSignalAgeMs: number;
  copyTradeMaxSlippage: number;
  copyTradeMaxPriorityFee: number;
  copyTradeMinWalletReserveSol: number;
  copyTradeMaxCopyWalletsPerChat: number;
  copyTradeAllowedSources: string[];
}

export interface CopyTradeBuyRiskContext {
  config: CopyTradeRiskControlConfig;
  request: PumpPortalLightningTradeRequest;
  trade: WalletTradeData;
  copyTradeWalletCount: number;
  dailySpentSol: number;
  nowMs: number;
}

export interface CopyTradeDailyBudgetReservation {
  ok: boolean;
  reason: string | null;
  spentSol: number;
}

function capEnabled(value: number): boolean {
  return Number.isFinite(value) && value > 0;
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(9)));
}

function formatSeconds(ms: number): string {
  return formatNumber(ms / 1000);
}

function requestAmountSol(request: PumpPortalLightningTradeRequest): number | null {
  if (request.action !== "buy" || request.denominatedInSol !== "true" || typeof request.amount !== "number") {
    return null;
  }

  return request.amount;
}

function tradeTimestampMs(trade: WalletTradeData): number | null {
  if (!Number.isFinite(trade.timestamp)) {
    return null;
  }

  const timestamp = Number(trade.timestamp);
  return timestamp > 10_000_000_000 ? timestamp : timestamp * 1000;
}

function normalizeSource(value: string | null | undefined): string | null {
  const normalized = value?.trim().toUpperCase();
  return normalized ? normalized : null;
}

function copyTradeDailyCapBlockedReason({
  amountSol,
  dailySpentSol,
  dailyCapSol
}: {
  amountSol: number;
  dailySpentSol: number;
  dailyCapSol: number;
}): string | null {
  if (!capEnabled(dailyCapSol)) {
    return null;
  }

  const nextSpentSol = dailySpentSol + amountSol;
  if (nextSpentSol <= dailyCapSol) {
    return null;
  }

  return `daily copy buy budget would reach ${formatNumber(nextSpentSol)} SOL, exceeding COPY_TRADE_DAILY_SOL_CAP=${formatNumber(dailyCapSol)} SOL`;
}

export function copyTradeRequestRiskBlockedReason({
  config,
  request
}: {
  config: CopyTradeRiskControlConfig;
  request: PumpPortalLightningTradeRequest;
}): string | null {
  if (capEnabled(config.copyTradeMaxSlippage) && request.slippage > config.copyTradeMaxSlippage) {
    return `${request.action} slippage ${formatNumber(request.slippage)}% exceeds COPY_TRADE_MAX_SLIPPAGE=${formatNumber(config.copyTradeMaxSlippage)}%`;
  }

  if (capEnabled(config.copyTradeMaxPriorityFee) && request.priorityFee > config.copyTradeMaxPriorityFee) {
    return `${request.action} priority fee ${formatNumber(request.priorityFee)} SOL exceeds COPY_TRADE_MAX_PRIORITY_FEE=${formatNumber(config.copyTradeMaxPriorityFee)} SOL`;
  }

  return null;
}

export function copyTradeWalletReserveBlockedReason({
  config,
  request,
  tradingWalletBalanceSol
}: {
  config: CopyTradeRiskControlConfig;
  request: PumpPortalLightningTradeRequest;
  tradingWalletBalanceSol: number | null;
}): string | null {
  if (!capEnabled(config.copyTradeMinWalletReserveSol)) {
    return null;
  }

  const amountSol = requestAmountSol(request);
  if (amountSol === null) {
    return "copy buy amount is not a fixed SOL amount";
  }

  if (tradingWalletBalanceSol === null) {
    return `could not verify trading wallet balance for COPY_TRADE_MIN_WALLET_RESERVE_SOL=${formatNumber(config.copyTradeMinWalletReserveSol)} SOL`;
  }

  const requiredSol = amountSol + config.copyTradeMinWalletReserveSol;
  if (tradingWalletBalanceSol < requiredSol) {
    return `trading wallet balance ${formatNumber(tradingWalletBalanceSol)} SOL cannot cover ${formatNumber(amountSol)} SOL buy plus COPY_TRADE_MIN_WALLET_RESERVE_SOL=${formatNumber(config.copyTradeMinWalletReserveSol)} SOL`;
  }

  return null;
}

export function copyTradeBuyRiskBlockedReason({
  config,
  request,
  trade,
  copyTradeWalletCount,
  dailySpentSol,
  nowMs
}: CopyTradeBuyRiskContext): string | null {
  const amountSol = requestAmountSol(request);
  if (amountSol === null) {
    return "copy buy amount is not a fixed SOL amount";
  }

  if (capEnabled(config.copyTradeMaxCopyWalletsPerChat) && copyTradeWalletCount > config.copyTradeMaxCopyWalletsPerChat) {
    return `chat copies ${copyTradeWalletCount} wallets, exceeding COPY_TRADE_MAX_COPY_WALLETS_PER_CHAT=${formatNumber(config.copyTradeMaxCopyWalletsPerChat)}`;
  }

  if (capEnabled(config.copyTradeMaxBuySol) && amountSol > config.copyTradeMaxBuySol) {
    return `copy buy amount ${formatNumber(amountSol)} SOL exceeds COPY_TRADE_MAX_BUY_SOL=${formatNumber(config.copyTradeMaxBuySol)} SOL`;
  }

  const dailyCapReason = copyTradeDailyCapBlockedReason({
    amountSol,
    dailySpentSol,
    dailyCapSol: config.copyTradeDailySolCap
  });
  if (dailyCapReason) {
    return dailyCapReason;
  }

  if (capEnabled(config.copyTradeMaxSignalAgeMs)) {
    const timestampMs = tradeTimestampMs(trade);

    if (timestampMs === null) {
      return `observed trade timestamp is missing; cannot enforce COPY_TRADE_MAX_SIGNAL_AGE_MS=${formatNumber(config.copyTradeMaxSignalAgeMs)}`;
    }

    const ageMs = nowMs - timestampMs;
    if (ageMs > config.copyTradeMaxSignalAgeMs) {
      return `observed trade signal is ${formatSeconds(ageMs)}s old, exceeding COPY_TRADE_MAX_SIGNAL_AGE_MS=${formatNumber(config.copyTradeMaxSignalAgeMs)}`;
    }
  }

  if (config.copyTradeAllowedSources.length > 0) {
    const source = normalizeSource(trade.source || trade.pool);
    const allowedSources = config.copyTradeAllowedSources.map((value) => normalizeSource(value)).filter((value): value is string => Boolean(value));

    if (!source || !allowedSources.includes(source)) {
      return `copy trade source ${source || "unknown"} is not in COPY_TRADE_ALLOWED_SOURCES=${allowedSources.join(",")}`;
    }
  }

  return copyTradeRequestRiskBlockedReason({ config, request });
}

function utcDay(nowMs: number): string {
  return new Date(nowMs).toISOString().slice(0, 10);
}

export function copyTradeDailyBudgetKey({
  chatId,
  tradingWalletPublicKey
}: {
  chatId: string;
  tradingWalletPublicKey: string;
}): string {
  return [chatId, tradingWalletPublicKey].join(":");
}

export function createInMemoryCopyTradeDailySolBudget() {
  let activeDay = "";
  const spentByKey = new Map<string, number>();

  function resetIfNewDay(nowMs: number): void {
    const day = utcDay(nowMs);
    if (day === activeDay) {
      return;
    }

    activeDay = day;
    spentByKey.clear();
  }

  return {
    spentSol({ key, nowMs }: { key: string; nowMs: number }): number {
      resetIfNewDay(nowMs);
      return spentByKey.get(key) || 0;
    },
    reserve({
      key,
      amountSol,
      capSol,
      nowMs
    }: {
      key: string;
      amountSol: number;
      capSol: number;
      nowMs: number;
    }): CopyTradeDailyBudgetReservation {
      resetIfNewDay(nowMs);

      const spentSol = spentByKey.get(key) || 0;
      const reason = copyTradeDailyCapBlockedReason({
        amountSol,
        dailySpentSol: spentSol,
        dailyCapSol: capSol
      });

      if (reason) {
        return {
          ok: false,
          reason,
          spentSol
        };
      }

      const nextSpentSol = spentSol + amountSol;
      spentByKey.set(key, nextSpentSol);

      return {
        ok: true,
        reason: null,
        spentSol: nextSpentSol
      };
    }
  };
}

export function formatCopyTradeRiskControlLog(config: CopyTradeRiskControlConfig): string {
  return [
    "Copy trade risk controls",
    `maxBuySol=${formatNumber(config.copyTradeMaxBuySol)}`,
    `dailySolCap=${formatNumber(config.copyTradeDailySolCap)}`,
    `maxSignalAgeMs=${formatNumber(config.copyTradeMaxSignalAgeMs)}`,
    `maxSlippage=${formatNumber(config.copyTradeMaxSlippage)}`,
    `maxPriorityFee=${formatNumber(config.copyTradeMaxPriorityFee)}`,
    `maxCopyWalletsPerChat=${formatNumber(config.copyTradeMaxCopyWalletsPerChat)}`,
    `allowedSources=${config.copyTradeAllowedSources.length > 0 ? config.copyTradeAllowedSources.join(",") : "any"}`,
    `minWalletReserveSol=${formatNumber(config.copyTradeMinWalletReserveSol)}`
  ].join(" | ");
}
