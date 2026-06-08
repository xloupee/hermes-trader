import {
  buildCashbackExecutionKey,
  buildPendingPlatformFeeCashbackAccrual
} from "./cashback.js";
import type { CashbackConfig, CashbackLedgerEntry } from "./cashback.js";
import { calculatePlatformFeeSplit } from "./platform-fee.js";
import type { TradeExecutionPlatformFee } from "./trade-execution.js";

const RUST_PLANNED_COPY_SPEND_KEYS = [
  "plannedCopySpendLamports",
  "planned_copy_spend_lamports"
];

function integerValueToBigInt(value: unknown): bigint | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  const raw = String(value).trim();
  if (!raw) {
    return null;
  }

  if (/^\d+$/.test(raw)) {
    return BigInt(raw);
  }

  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? BigInt(Math.floor(number)) : null;
}

function platformFeeResult(split: ReturnType<typeof calculatePlatformFeeSplit>): TradeExecutionPlatformFee | null {
  if (!split.enabled || split.feeLamports <= 0n || split.blockedReason) {
    return null;
  }

  return {
    enabled: true,
    bps: split.bps,
    treasury: split.treasury,
    budgetLamports: split.budgetLamports,
    feeLamports: split.feeLamports,
    tradeLamports: split.tradeLamports
  };
}

export function plannedCopySpendLamportsFromRustExecution(rawExecution: Record<string, unknown>): bigint | null {
  for (const key of RUST_PLANNED_COPY_SPEND_KEYS) {
    const lamports = integerValueToBigInt(rawExecution[key]);
    if (lamports !== null && lamports > 0n) {
      return lamports;
    }
  }

  return null;
}

export function buildRustAsyncPlatformFeeCashbackAccrual({
  chatId,
  tradingWalletPublicKey,
  sourceSignature,
  executionSignature,
  plannedCopySpendLamports,
  platformFeeEnabled,
  platformFeeBps,
  platformFeeTreasury,
  cashbackConfig
}: {
  chatId: string;
  tradingWalletPublicKey: string;
  sourceSignature: string;
  executionSignature: string;
  plannedCopySpendLamports: bigint;
  platformFeeEnabled: boolean;
  platformFeeBps: number;
  platformFeeTreasury: string | null | undefined;
  cashbackConfig: CashbackConfig;
}): CashbackLedgerEntry | null {
  const split = calculatePlatformFeeSplit({
    action: "buy",
    budgetLamports: plannedCopySpendLamports,
    config: {
      enabled: platformFeeEnabled,
      bps: platformFeeBps,
      treasury: platformFeeTreasury
    }
  });
  const platformFee = platformFeeResult(split);

  if (!platformFee) {
    return null;
  }

  return buildPendingPlatformFeeCashbackAccrual({
    chatId,
    tradingWalletPublicKey,
    executionKey: buildCashbackExecutionKey({
      chatId,
      tradingWalletPublicKey,
      sourceSignature,
      executionSignature,
      action: "buy"
    }),
    sourceSignature,
    executionSignature,
    action: "buy",
    status: "submitted",
    provider: "direct-auto",
    platformFee,
    config: cashbackConfig
  });
}
