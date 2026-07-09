import { createClient } from "@supabase/supabase-js";
import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  sendAndConfirmTransaction
} from "@solana/web3.js";
import bs58 from "bs58";
import type { CopyTradeExecutionAction, CopyTradeExecutionStatus, TelegramChatId } from "./types.js";
import type { TradeExecutionPlatformFee, TradeExecutionProvider } from "./trade-execution.js";

const BPS_DENOMINATOR = 10_000n;
const LAMPORTS_PER_SOL = 1_000_000_000n;
const DEFAULT_CASHBACK_FEE_SHARE_BPS = 4_000;
const DEFAULT_CASHBACK_MIN_CLAIM_SOL = 0.001;

export type CashbackLedgerStatus = "pending" | "claimable" | "paid" | "voided";
export type CashbackLedgerAction = CopyTradeExecutionAction | "adjustment";
export type CashbackLedgerEntryType = "trade" | "manual_adjustment";
export type CashbackPayoutStatus = "pending" | "submitted" | "confirmed" | "failed";
export type CashbackConfigValueSource = "global" | "subscriber_override";
export type CashbackPlatformFeeCollectionStatus = "not_required" | "pending" | "submitted" | "confirmed" | "failed";

export interface CashbackConfig {
  enabled: boolean;
  feeShareBps: number;
  minClaimLamports: bigint;
  maxPayoutLamportsPerDay: bigint;
  payoutWalletPublicKey?: string | null;
  payoutWalletSecretKey?: string | null;
}

export interface CashbackAccrualInput {
  chatId: string;
  tradingWalletPublicKey: string;
  executionKey: string;
  sourceSignature: string | null;
  executionSignature: string | null;
  action: CopyTradeExecutionAction;
  status: CopyTradeExecutionStatus;
  provider: TradeExecutionProvider;
  platformFee: TradeExecutionPlatformFee | null | undefined;
  trailingSellStepIndex?: number | null;
  trailingSellTotalSteps?: number | null;
  config: CashbackConfig;
}

export interface CashbackLedgerEntry {
  id?: string | number;
  chatId: string;
  tradingWalletPublicKey: string;
  executionKey: string;
  sourceSignature: string | null;
  executionSignature: string | null;
  action: CashbackLedgerAction;
  platformFeeLamports: bigint;
  cashbackLamports: bigint;
  cashbackFeeShareBps?: number | null;
  platformFeeBps?: number | null;
  platformFeeTreasury?: string | null;
  platformFeeCollectionStatus?: CashbackPlatformFeeCollectionStatus | null;
  platformFeeTransferSignature?: string | null;
  platformFeeCollectionError?: string | null;
  platformFeeCollectionAttempts?: number | null;
  platformFeeCollectionUpdatedAt?: string | null;
  platformFeeLeaseToken?: string | null;
  platformFeeLeaseExpiresAt?: string | null;
  platformFeeTransactionBase64?: string | null;
  platformFeeRecentBlockhash?: string | null;
  platformFeeLastValidBlockHeight?: number | null;
  entryType?: CashbackLedgerEntryType;
  adjustmentReason?: string | null;
  adjustedBy?: string | null;
  status: CashbackLedgerStatus;
  createdAt?: string | null;
  updatedAt?: string | null;
}

export interface CashbackSubscriberOverride {
  chatId: string;
  enabledOverride: boolean | null;
  feeShareBpsOverride: number | null;
  note: string | null;
  updatedBy: string | null;
  updatedAt: string | null;
}

export interface ResolvedCashbackConfig {
  chatId: string;
  config: CashbackConfig;
  enabledSource: CashbackConfigValueSource;
  feeShareBpsSource: CashbackConfigValueSource;
  override: CashbackSubscriberOverride | null;
}

export interface CashbackPayoutRecord {
  id?: string | number;
  chatId: string;
  tradingWalletPublicKey: string;
  amountLamports: bigint;
  status: CashbackPayoutStatus;
  signature: string | null;
  errorText: string | null;
  createdAt?: string | null;
  updatedAt?: string | null;
}

export interface CashbackSummary {
  enabled: boolean;
  tradingWalletPublicKey: string | null;
  payoutWalletPublicKey: string | null;
  accruedLamports: bigint;
  claimableLamports: bigint;
  pendingLamports: bigint;
  lifetimePaidLamports: bigint;
  minClaimLamports: bigint;
  payoutUnavailableReason: string | null;
}

export interface CashbackClaimResult {
  ok: boolean;
  status: "submitted" | "failed" | "below_threshold" | "unavailable";
  summary: CashbackSummary;
  signature: string | null;
  errorText: string | null;
}

export interface CashbackReconciliationReport {
  platformFeeLamports: bigint;
  cashbackAccruedLamports: bigint;
  cashbackPaidLamports: bigint;
  outstandingLiabilityLamports: bigint;
  pendingPayoutLamports: bigint;
}

export interface CashbackStore {
  accrue: (entry: CashbackLedgerEntry) => Promise<void>;
  getSubscriberConfig: (input: {
    chatId: TelegramChatId;
    config: CashbackConfig;
  }) => Promise<ResolvedCashbackConfig>;
  setSubscriberConfigOverride: (input: {
    chatId: TelegramChatId;
    enabledOverride?: boolean | null;
    feeShareBpsOverride?: number | null;
    note?: string | null;
    updatedBy: string;
    config: CashbackConfig;
  }) => Promise<ResolvedCashbackConfig>;
  clearSubscriberConfigOverride: (input: {
    chatId: TelegramChatId;
    field: "enabled" | "feeShareBps" | "all";
    note?: string | null;
    updatedBy: string;
    config: CashbackConfig;
  }) => Promise<ResolvedCashbackConfig>;
  createManualAdjustment: (input: {
    chatId: TelegramChatId;
    tradingWalletPublicKey: string;
    cashbackLamports: bigint;
    reason: string;
    adjustedBy: string;
    executionKey?: string | null;
  }) => Promise<CashbackLedgerEntry>;
  getLedgerEntryByExecutionKey: (executionKey: string) => Promise<CashbackLedgerEntry | null>;
  listPlatformFeeCollections: (input: {
    statuses: CashbackPlatformFeeCollectionStatus[];
    limit?: number;
  }) => Promise<CashbackLedgerEntry[]>;
  claimPlatformFeeCollections: (input: {
    statuses: CashbackPlatformFeeCollectionStatus[];
    leaseToken: string;
    leaseDurationMs: number;
    limit?: number;
  }) => Promise<CashbackLedgerEntry[]>;
  updatePlatformFeeCollection: (input: {
    executionKey: string;
    collectionStatus: CashbackPlatformFeeCollectionStatus;
    ledgerStatus?: CashbackLedgerStatus;
    transferSignature?: string | null;
    errorText?: string | null;
    attempts?: number | null;
    transactionBase64?: string | null;
    recentBlockhash?: string | null;
    lastValidBlockHeight?: number | null;
    expectedLeaseToken?: string | null;
    releaseLease?: boolean;
  }) => Promise<boolean>;
  getSummary: (input: {
    chatId: TelegramChatId;
    tradingWalletPublicKey: string | null;
    payoutWalletPublicKey?: string | null;
    config: CashbackConfig;
  }) => Promise<CashbackSummary>;
  listClaimableEntries: (input: {
    chatId: TelegramChatId;
    tradingWalletPublicKey: string;
  }) => Promise<CashbackLedgerEntry[]>;
  createPayout: (payout: CashbackPayoutRecord) => Promise<CashbackPayoutRecord>;
  updatePayout: (input: {
    id: string | number;
    status: CashbackPayoutStatus;
    signature?: string | null;
    errorText?: string | null;
  }) => Promise<void>;
  markLedgerPaid: (input: {
    ids: Array<string | number>;
  }) => Promise<void>;
  getReconciliationReport: () => Promise<CashbackReconciliationReport>;
}

interface SupabaseCashbackLedgerRow {
  id?: string | number;
  chat_id: string;
  trading_wallet_public_key: string;
  execution_key: string;
  source_signature: string | null;
  execution_signature: string | null;
  action: CashbackLedgerAction;
  platform_fee_lamports: string | number;
  cashback_lamports: string | number;
  cashback_fee_share_bps?: number | null;
  platform_fee_bps?: number | null;
  platform_fee_treasury?: string | null;
  platform_fee_collection_status?: CashbackPlatformFeeCollectionStatus | null;
  platform_fee_transfer_signature?: string | null;
  platform_fee_collection_error?: string | null;
  platform_fee_collection_attempts?: number | null;
  platform_fee_collection_updated_at?: string | null;
  platform_fee_lease_token?: string | null;
  platform_fee_lease_expires_at?: string | null;
  platform_fee_transaction_base64?: string | null;
  platform_fee_recent_blockhash?: string | null;
  platform_fee_last_valid_block_height?: number | null;
  entry_type?: CashbackLedgerEntryType | null;
  adjustment_reason?: string | null;
  adjusted_by?: string | null;
  status: CashbackLedgerStatus;
  created_at?: string | null;
  updated_at?: string | null;
}

interface SupabaseCashbackSubscriberRow {
  chat_id: string;
  cashback_enabled_override?: boolean | null;
  cashback_fee_share_bps_override?: number | null;
  cashback_override_note?: string | null;
  cashback_override_updated_by?: string | null;
  cashback_override_updated_at?: string | null;
}

interface SupabaseCashbackPayoutRow {
  id?: string | number;
  chat_id: string;
  trading_wallet_public_key: string;
  amount_lamports: string | number;
  status: CashbackPayoutStatus;
  signature: string | null;
  error_text: string | null;
  created_at?: string | null;
  updated_at?: string | null;
}

interface SupabaseErrorLike {
  message?: string;
}

function formatSupabaseError(error: SupabaseErrorLike | null): Error | null {
  return error ? new Error(error.message || "Supabase cashback request failed") : null;
}

function bigintValue(value: string | number | bigint | null | undefined): bigint {
  if (value === null || value === undefined || value === "") {
    return 0n;
  }

  return BigInt(value);
}

function ledgerEntryFromRow(row: SupabaseCashbackLedgerRow): CashbackLedgerEntry {
  return {
    id: row.id,
    chatId: row.chat_id,
    tradingWalletPublicKey: row.trading_wallet_public_key,
    executionKey: row.execution_key,
    sourceSignature: row.source_signature,
    executionSignature: row.execution_signature,
    action: row.action,
    platformFeeLamports: bigintValue(row.platform_fee_lamports),
    cashbackLamports: bigintValue(row.cashback_lamports),
    cashbackFeeShareBps: row.cashback_fee_share_bps ?? null,
    platformFeeBps: row.platform_fee_bps ?? null,
    platformFeeTreasury: row.platform_fee_treasury || null,
    platformFeeCollectionStatus: row.platform_fee_collection_status || "not_required",
    platformFeeTransferSignature: row.platform_fee_transfer_signature || null,
    platformFeeCollectionError: row.platform_fee_collection_error || null,
    platformFeeCollectionAttempts: row.platform_fee_collection_attempts ?? 0,
    platformFeeCollectionUpdatedAt: row.platform_fee_collection_updated_at || null,
    platformFeeLeaseToken: row.platform_fee_lease_token || null,
    platformFeeLeaseExpiresAt: row.platform_fee_lease_expires_at || null,
    platformFeeTransactionBase64: row.platform_fee_transaction_base64 || null,
    platformFeeRecentBlockhash: row.platform_fee_recent_blockhash || null,
    platformFeeLastValidBlockHeight: row.platform_fee_last_valid_block_height ?? null,
    entryType: row.entry_type || "trade",
    adjustmentReason: row.adjustment_reason || null,
    adjustedBy: row.adjusted_by || null,
    status: row.status,
    createdAt: row.created_at,
    updatedAt: row.updated_at
  };
}

function payoutFromRow(row: SupabaseCashbackPayoutRow): CashbackPayoutRecord {
  return {
    id: row.id,
    chatId: row.chat_id,
    tradingWalletPublicKey: row.trading_wallet_public_key,
    amountLamports: bigintValue(row.amount_lamports),
    status: row.status,
    signature: row.signature,
    errorText: row.error_text,
    createdAt: row.created_at,
    updatedAt: row.updated_at
  };
}

function ledgerRow(entry: CashbackLedgerEntry): SupabaseCashbackLedgerRow {
  return {
    chat_id: entry.chatId,
    trading_wallet_public_key: entry.tradingWalletPublicKey,
    execution_key: entry.executionKey,
    source_signature: entry.sourceSignature,
    execution_signature: entry.executionSignature,
    action: entry.action,
    platform_fee_lamports: entry.platformFeeLamports.toString(),
    cashback_lamports: entry.cashbackLamports.toString(),
    cashback_fee_share_bps: entry.cashbackFeeShareBps ?? null,
    platform_fee_bps: entry.platformFeeBps ?? null,
    platform_fee_treasury: entry.platformFeeTreasury || null,
    platform_fee_collection_status: entry.platformFeeCollectionStatus || "not_required",
    platform_fee_transfer_signature: entry.platformFeeTransferSignature || null,
    platform_fee_collection_error: entry.platformFeeCollectionError || null,
    platform_fee_collection_attempts: entry.platformFeeCollectionAttempts ?? 0,
    platform_fee_collection_updated_at: entry.platformFeeCollectionUpdatedAt || null,
    platform_fee_lease_token: entry.platformFeeLeaseToken || null,
    platform_fee_lease_expires_at: entry.platformFeeLeaseExpiresAt || null,
    platform_fee_transaction_base64: entry.platformFeeTransactionBase64 || null,
    platform_fee_recent_blockhash: entry.platformFeeRecentBlockhash || null,
    platform_fee_last_valid_block_height: entry.platformFeeLastValidBlockHeight ?? null,
    entry_type: entry.entryType || "trade",
    adjustment_reason: entry.adjustmentReason || null,
    adjusted_by: entry.adjustedBy || null,
    status: entry.status
  };
}

function payoutRow(payout: CashbackPayoutRecord): SupabaseCashbackPayoutRow {
  return {
    chat_id: payout.chatId,
    trading_wallet_public_key: payout.tradingWalletPublicKey,
    amount_lamports: payout.amountLamports.toString(),
    status: payout.status,
    signature: payout.signature,
    error_text: payout.errorText
  };
}

export function solToCashbackLamports(amountSol: number): bigint {
  return BigInt(Math.max(0, Math.round(amountSol * 1_000_000_000)));
}

export function parseCashbackConfig(env: NodeJS.ProcessEnv): CashbackConfig {
  const feeShare = Number(env.CASHBACK_FEE_SHARE_BPS ?? DEFAULT_CASHBACK_FEE_SHARE_BPS);
  const minClaim = Number(env.CASHBACK_MIN_CLAIM_SOL ?? DEFAULT_CASHBACK_MIN_CLAIM_SOL);
  const maxPerDay = Number(env.CASHBACK_MAX_PAYOUT_SOL_PER_DAY ?? 0);

  return {
    enabled: env.CASHBACK_ENABLED === "true",
    feeShareBps: Number.isFinite(feeShare) ? Math.floor(feeShare) : DEFAULT_CASHBACK_FEE_SHARE_BPS,
    minClaimLamports: solToCashbackLamports(Number.isFinite(minClaim) ? minClaim : DEFAULT_CASHBACK_MIN_CLAIM_SOL),
    maxPayoutLamportsPerDay: solToCashbackLamports(Number.isFinite(maxPerDay) ? maxPerDay : 0),
    payoutWalletPublicKey: env.CASHBACK_PAYOUT_WALLET_PUBLIC_KEY || null,
    payoutWalletSecretKey: env.CASHBACK_PAYOUT_WALLET_SECRET_KEY || null
  };
}

export function cashbackConfigBlockedReason(config: CashbackConfig): string | null {
  if (!config.enabled) {
    return null;
  }

  if (!Number.isInteger(config.feeShareBps) || config.feeShareBps < 0 || config.feeShareBps > 10_000) {
    return `CASHBACK_FEE_SHARE_BPS must be an integer from 0 to 10000; got ${String(config.feeShareBps)}`;
  }

  if (config.minClaimLamports < 0n) {
    return "CASHBACK_MIN_CLAIM_SOL must be non-negative";
  }

  if (config.maxPayoutLamportsPerDay < 0n) {
    return "CASHBACK_MAX_PAYOUT_SOL_PER_DAY must be non-negative";
  }

  if (config.payoutWalletPublicKey) {
    try {
      new PublicKey(config.payoutWalletPublicKey);
    } catch {
      return `invalid CASHBACK_PAYOUT_WALLET_PUBLIC_KEY: ${config.payoutWalletPublicKey}`;
    }
  }

  return null;
}

export function normalizeCashbackChatId(chatId: TelegramChatId): string {
  const normalized = String(chatId).trim();
  if (!/^-?\d{3,20}$/.test(normalized)) {
    throw new Error("Telegram chat id must be a numeric id");
  }

  return normalized;
}

export function validateCashbackFeeShareBps(feeShareBps: number): number {
  if (!Number.isInteger(feeShareBps) || feeShareBps < 0 || feeShareBps > 10_000) {
    throw new Error("cashback fee-share override must be an integer from 0 to 10000");
  }

  return feeShareBps;
}

function subscriberOverrideFromRow(row: SupabaseCashbackSubscriberRow | null): CashbackSubscriberOverride | null {
  if (!row) {
    return null;
  }

  return {
    chatId: row.chat_id,
    enabledOverride: row.cashback_enabled_override ?? null,
    feeShareBpsOverride: row.cashback_fee_share_bps_override ?? null,
    note: row.cashback_override_note || null,
    updatedBy: row.cashback_override_updated_by || null,
    updatedAt: row.cashback_override_updated_at || null
  };
}

export function resolveCashbackConfig({
  chatId,
  config,
  override = null
}: {
  chatId: TelegramChatId;
  config: CashbackConfig;
  override?: CashbackSubscriberOverride | null;
}): ResolvedCashbackConfig {
  const normalizedChatId = normalizeCashbackChatId(chatId);
  const enabledFromOverride = override?.enabledOverride;
  const feeShareBpsFromOverride = override?.feeShareBpsOverride;
  const feeShareBps = feeShareBpsFromOverride === null || feeShareBpsFromOverride === undefined
    ? config.feeShareBps
    : validateCashbackFeeShareBps(feeShareBpsFromOverride);

  return {
    chatId: normalizedChatId,
    config: {
      ...config,
      enabled: enabledFromOverride === null || enabledFromOverride === undefined ? config.enabled : enabledFromOverride,
      feeShareBps
    },
    enabledSource: enabledFromOverride === null || enabledFromOverride === undefined ? "global" : "subscriber_override",
    feeShareBpsSource: feeShareBpsFromOverride === null || feeShareBpsFromOverride === undefined ? "global" : "subscriber_override",
    override
  };
}

export function requireKnownCashbackSubscriber(
  override: CashbackSubscriberOverride | null,
  chatId: TelegramChatId
): CashbackSubscriberOverride {
  if (!override) {
    throw new Error(`unknown Telegram subscriber: ${String(chatId)}`);
  }

  return override;
}

export function calculateCashbackLamports(platformFeeLamports: bigint | number | string, feeShareBps: number): bigint {
  if (!Number.isInteger(feeShareBps) || feeShareBps < 0 || feeShareBps > 10_000) {
    throw new Error("feeShareBps must be an integer from 0 to 10000");
  }

  const platformFee = bigintValue(platformFeeLamports);
  if (platformFee <= 0n || feeShareBps === 0) {
    return 0n;
  }

  return (platformFee * BigInt(feeShareBps)) / BPS_DENOMINATOR;
}

export function buildCashbackExecutionKey({
  chatId,
  tradingWalletPublicKey,
  sourceSignature,
  executionSignature,
  action,
  trailingSellStepIndex = null,
  trailingSellTotalSteps = null
}: {
  chatId: string;
  tradingWalletPublicKey: string;
  sourceSignature: string | null;
  executionSignature: string | null;
  action: CopyTradeExecutionAction;
  trailingSellStepIndex?: number | null;
  trailingSellTotalSteps?: number | null;
}): string {
  return [
    chatId,
    tradingWalletPublicKey,
    sourceSignature || "no-source-signature",
    executionSignature || "no-execution-signature",
    action,
    trailingSellStepIndex ?? -1,
    trailingSellTotalSteps ?? -1
  ].join(":");
}

export function buildCashbackAccrual(input: CashbackAccrualInput): CashbackLedgerEntry | null {
  if (!input.config.enabled || cashbackConfigBlockedReason(input.config)) {
    return null;
  }

  if (input.provider === "pumpportal-lightning") {
    return null;
  }

  if (input.status !== "submitted" && input.status !== "confirmed") {
    return null;
  }

  const platformFeeLamports = input.platformFee?.enabled ? input.platformFee.feeLamports : 0n;
  const cashbackLamports = calculateCashbackLamports(platformFeeLamports, input.config.feeShareBps);

  if (platformFeeLamports <= 0n || cashbackLamports <= 0n) {
    return null;
  }

  return {
    chatId: input.chatId,
    tradingWalletPublicKey: input.tradingWalletPublicKey,
    executionKey: input.executionKey,
    sourceSignature: input.sourceSignature,
    executionSignature: input.executionSignature,
    action: input.action,
    platformFeeLamports,
    cashbackLamports,
    cashbackFeeShareBps: input.config.feeShareBps,
    entryType: "trade",
    adjustmentReason: null,
    adjustedBy: null,
    status: "claimable"
  };
}

export function buildPendingPlatformFeeCashbackAccrual(input: CashbackAccrualInput): CashbackLedgerEntry | null {
  const entry = buildCashbackAccrual(input);
  if (!entry || !input.platformFee?.enabled) {
    return null;
  }

  return {
    ...entry,
    status: "pending",
    platformFeeBps: input.platformFee.bps,
    platformFeeTreasury: input.platformFee.treasury,
    platformFeeCollectionStatus: "pending",
    platformFeeTransferSignature: null,
    platformFeeCollectionError: null,
    platformFeeCollectionAttempts: 0,
    platformFeeCollectionUpdatedAt: new Date().toISOString()
  };
}

export function buildCashbackManualAdjustment({
  chatId,
  tradingWalletPublicKey,
  cashbackLamports,
  reason,
  adjustedBy,
  executionKey = null
}: {
  chatId: TelegramChatId;
  tradingWalletPublicKey: string;
  cashbackLamports: bigint;
  reason: string;
  adjustedBy: string;
  executionKey?: string | null;
}): CashbackLedgerEntry {
  const normalizedChatId = normalizeCashbackChatId(chatId);
  const normalizedTradingWallet = tradingWalletPublicKey.trim();
  const normalizedReason = reason.trim();
  const normalizedAdjustedBy = adjustedBy.trim();

  if (!normalizedTradingWallet) {
    throw new Error("trading wallet public key is required for cashback adjustment");
  }

  if (cashbackLamports === 0n) {
    throw new Error("cashback adjustment lamports must be non-zero");
  }

  if (!normalizedReason) {
    throw new Error("cashback adjustment reason is required");
  }

  if (!normalizedAdjustedBy) {
    throw new Error("cashback adjustment updated_by is required");
  }

  const key = executionKey?.trim() ||
    `manual:${normalizedChatId}:${normalizedTradingWallet}:${Date.now()}:${Math.random().toString(36).slice(2)}`;

  return {
    chatId: normalizedChatId,
    tradingWalletPublicKey: normalizedTradingWallet,
    executionKey: key,
    sourceSignature: null,
    executionSignature: null,
    action: "adjustment",
    platformFeeLamports: 0n,
    cashbackLamports,
    cashbackFeeShareBps: null,
    entryType: "manual_adjustment",
    adjustmentReason: normalizedReason,
    adjustedBy: normalizedAdjustedBy,
    status: "claimable"
  };
}

export function formatCashbackSol(lamports: bigint): string {
  const whole = lamports / LAMPORTS_PER_SOL;
  const fractional = lamports % LAMPORTS_PER_SOL;
  const fraction = fractional.toString().padStart(9, "0").replace(/0+$/, "");

  return fraction ? `${whole.toString()}.${fraction}` : whole.toString();
}

export function formatCashbackSummaryText(summary: CashbackSummary): string {
  const tradingWalletLine = summary.tradingWalletPublicKey
    ? `<code>${summary.tradingWalletPublicKey}</code>`
    : "No active trading wallet";
  const payoutWalletLine = summary.payoutWalletPublicKey
    ? `<code>${summary.payoutWalletPublicKey}</code>`
    : "Not set";
  const claimable = formatCashbackSol(summary.claimableLamports);
  const claimLine = summary.claimableLamports >= summary.minClaimLamports
    ? `🎉 Ready to claim: <b>${claimable} SOL</b>`
    : null;

  return [
    "<b>💎 Cashback Vault</b>",
    "",
    "<b>👛 Trading Wallet</b>",
    `└ ${tradingWalletLine}`,
    "",
    "<b>💸 Payout Wallet</b>",
    `└ ${payoutWalletLine}`,
    "",
    "<b>🎁 Rewards</b>",
    `├ Claimable: <b>${formatCashbackSol(summary.claimableLamports)} SOL</b>`,
    `└ Lifetime Paid: ${formatCashbackSol(summary.lifetimePaidLamports)} SOL`,
    "",
    claimLine,
    summary.payoutUnavailableReason ? `<b>Payout:</b> ${summary.payoutUnavailableReason}` : null
  ].filter((line): line is string => line !== null).join("\n");
}

export function cashbackSummaryReplyMarkup(summary: CashbackSummary): { inline_keyboard: Array<Array<{ text: string; callback_data: string }>> } {
  const keyboard: Array<Array<{ text: string; callback_data: string }>> = [];

  if (
    summary.enabled &&
    summary.tradingWalletPublicKey &&
    summary.payoutWalletPublicKey &&
    !summary.payoutUnavailableReason &&
    summary.claimableLamports > 0n
  ) {
    keyboard.push([{ text: "Claim Cashback", callback_data: "cashback:claim" }]);
  }

  keyboard.push([
    { text: summary.payoutWalletPublicKey ? "Change Payout Wallet" : "Add Payout Wallet", callback_data: "cashback:set_payout_wallet" },
    { text: "Refresh", callback_data: "cashback:dashboard" }
  ]);

  return {
    inline_keyboard: keyboard
  };
}

export function createSupabaseCashbackStore({
  url,
  serviceRoleKey
}: {
  url: string;
  serviceRoleKey: string;
}): CashbackStore {
  const client = createClient(url, serviceRoleKey, {
    auth: {
      persistSession: false,
      autoRefreshToken: false
    }
  });

  async function selectLedger(chatId?: string, tradingWalletPublicKey?: string | null): Promise<CashbackLedgerEntry[]> {
    let query = client.from("telegram_cashback_ledger").select("*");
    if (chatId) {
      query = query.eq("chat_id", chatId);
    }
    if (tradingWalletPublicKey) {
      query = query.eq("trading_wallet_public_key", tradingWalletPublicKey);
    }

    const { data, error } = await query;
    const formattedError = formatSupabaseError(error);
    if (formattedError) {
      throw formattedError;
    }

    return ((data || []) as SupabaseCashbackLedgerRow[]).map(ledgerEntryFromRow);
  }

  async function selectPayouts(chatId?: string, tradingWalletPublicKey?: string | null): Promise<CashbackPayoutRecord[]> {
    let query = client.from("telegram_cashback_payouts").select("*");
    if (chatId) {
      query = query.eq("chat_id", chatId);
    }
    if (tradingWalletPublicKey) {
      query = query.eq("trading_wallet_public_key", tradingWalletPublicKey);
    }

    const { data, error } = await query;
    const formattedError = formatSupabaseError(error);
    if (formattedError) {
      throw formattedError;
    }

    return ((data || []) as SupabaseCashbackPayoutRow[]).map(payoutFromRow);
  }

  async function selectSubscriberOverride(chatId: TelegramChatId): Promise<CashbackSubscriberOverride | null> {
    const normalizedChatId = normalizeCashbackChatId(chatId);
    const { data, error } = await client
      .from("telegram_subscribers")
      .select("chat_id,cashback_enabled_override,cashback_fee_share_bps_override,cashback_override_note,cashback_override_updated_by,cashback_override_updated_at")
      .eq("chat_id", normalizedChatId)
      .maybeSingle();
    const formattedError = formatSupabaseError(error);
    if (formattedError) {
      throw formattedError;
    }

    return subscriberOverrideFromRow(data as SupabaseCashbackSubscriberRow | null);
  }

  async function requireSubscriberOverride(chatId: TelegramChatId): Promise<CashbackSubscriberOverride> {
    return requireKnownCashbackSubscriber(await selectSubscriberOverride(chatId), chatId);
  }

  return {
    async accrue(entry) {
      const { error } = await client
        .from("telegram_cashback_ledger")
        .upsert(ledgerRow(entry), { onConflict: "execution_key", ignoreDuplicates: true });
      const formattedError = formatSupabaseError(error);
      if (formattedError) {
        throw formattedError;
      }
    },
    async getSubscriberConfig({ chatId, config }) {
      const override = requireKnownCashbackSubscriber(await selectSubscriberOverride(chatId), chatId);

      return resolveCashbackConfig({ chatId, config, override });
    },
    async setSubscriberConfigOverride({
      chatId,
      enabledOverride,
      feeShareBpsOverride,
      note = null,
      updatedBy,
      config
    }) {
      const subscriber = await requireSubscriberOverride(chatId);
      const values: Record<string, unknown> = {
        cashback_override_note: note,
        cashback_override_updated_by: updatedBy.trim(),
        cashback_override_updated_at: new Date().toISOString(),
        updated_at: new Date().toISOString()
      };

      if (!values.cashback_override_updated_by) {
        throw new Error("cashback override updated_by is required");
      }

      if (enabledOverride !== undefined) {
        values.cashback_enabled_override = enabledOverride;
      }

      if (feeShareBpsOverride !== undefined) {
        values.cashback_fee_share_bps_override = feeShareBpsOverride === null
          ? null
          : validateCashbackFeeShareBps(feeShareBpsOverride);
      }

      const { data, error } = await client
        .from("telegram_subscribers")
        .update(values)
        .eq("chat_id", subscriber.chatId)
        .select("chat_id,cashback_enabled_override,cashback_fee_share_bps_override,cashback_override_note,cashback_override_updated_by,cashback_override_updated_at")
        .single();
      const formattedError = formatSupabaseError(error);
      if (formattedError) {
        throw formattedError;
      }

      return resolveCashbackConfig({
        chatId: subscriber.chatId,
        config,
        override: subscriberOverrideFromRow(data as SupabaseCashbackSubscriberRow)
      });
    },
    async clearSubscriberConfigOverride({ chatId, field, note = null, updatedBy, config }) {
      await requireSubscriberOverride(chatId);
      const values: Record<string, unknown> = {
        cashback_override_note: note,
        cashback_override_updated_by: updatedBy.trim(),
        cashback_override_updated_at: new Date().toISOString(),
        updated_at: new Date().toISOString()
      };

      if (!values.cashback_override_updated_by) {
        throw new Error("cashback override updated_by is required");
      }

      if (field === "enabled" || field === "all") {
        values.cashback_enabled_override = null;
      }

      if (field === "feeShareBps" || field === "all") {
        values.cashback_fee_share_bps_override = null;
      }

      const { data, error } = await client
        .from("telegram_subscribers")
        .update(values)
        .eq("chat_id", normalizeCashbackChatId(chatId))
        .select("chat_id,cashback_enabled_override,cashback_fee_share_bps_override,cashback_override_note,cashback_override_updated_by,cashback_override_updated_at")
        .single();
      const formattedError = formatSupabaseError(error);
      if (formattedError) {
        throw formattedError;
      }

      return resolveCashbackConfig({
        chatId,
        config,
        override: subscriberOverrideFromRow(data as SupabaseCashbackSubscriberRow)
      });
    },
    async createManualAdjustment(input) {
      await requireSubscriberOverride(input.chatId);
      const entry = buildCashbackManualAdjustment(input);
      const { data, error } = await client
        .from("telegram_cashback_ledger")
        .insert(ledgerRow(entry))
        .select("*")
        .single();
      const formattedError = formatSupabaseError(error);
      if (formattedError) {
        throw formattedError;
      }

      return ledgerEntryFromRow(data as SupabaseCashbackLedgerRow);
    },
    async getLedgerEntryByExecutionKey(executionKey) {
      const { data, error } = await client
        .from("telegram_cashback_ledger")
        .select("*")
        .eq("execution_key", executionKey)
        .maybeSingle();
      const formattedError = formatSupabaseError(error);
      if (formattedError) {
        throw formattedError;
      }

      return data ? ledgerEntryFromRow(data as SupabaseCashbackLedgerRow) : null;
    },
    async listPlatformFeeCollections({ statuses, limit = 50 }) {
      if (statuses.length === 0) {
        return [];
      }

      const { data, error } = await client
        .from("telegram_cashback_ledger")
        .select("*")
        .in("platform_fee_collection_status", statuses)
        .order("platform_fee_collection_updated_at", { ascending: true, nullsFirst: true })
        .limit(limit);
      const formattedError = formatSupabaseError(error);
      if (formattedError) {
        throw formattedError;
      }

      return ((data || []) as SupabaseCashbackLedgerRow[]).map(ledgerEntryFromRow);
    },
    async claimPlatformFeeCollections({ statuses, leaseToken, leaseDurationMs, limit = 50 }) {
      if (statuses.length === 0 || limit <= 0) {
        return [];
      }
      if (!leaseToken.trim()) {
        throw new Error("platform fee collection lease token is required");
      }
      if (!Number.isFinite(leaseDurationMs) || leaseDurationMs <= 0) {
        throw new Error("platform fee collection lease duration must be positive");
      }

      const now = new Date();
      const nowIso = now.toISOString();
      const leaseExpiresAt = new Date(now.getTime() + leaseDurationMs).toISOString();
      const { data: candidates, error: candidateError } = await client
        .from("telegram_cashback_ledger")
        .select("execution_key")
        .in("platform_fee_collection_status", statuses)
        .or(`platform_fee_lease_expires_at.is.null,platform_fee_lease_expires_at.lt.${nowIso}`)
        .order("platform_fee_collection_updated_at", { ascending: true, nullsFirst: true })
        .limit(limit);
      const formattedCandidateError = formatSupabaseError(candidateError);
      if (formattedCandidateError) {
        throw formattedCandidateError;
      }

      const claimed: CashbackLedgerEntry[] = [];
      for (const candidate of (candidates || []) as Array<{ execution_key: string }>) {
        const { data, error } = await client
          .from("telegram_cashback_ledger")
          .update({
            platform_fee_lease_token: leaseToken,
            platform_fee_lease_expires_at: leaseExpiresAt,
            platform_fee_collection_updated_at: nowIso,
            updated_at: nowIso
          })
          .eq("execution_key", candidate.execution_key)
          .in("platform_fee_collection_status", statuses)
          .or(`platform_fee_lease_expires_at.is.null,platform_fee_lease_expires_at.lt.${nowIso}`)
          .select("*")
          .maybeSingle();
        const formattedError = formatSupabaseError(error);
        if (formattedError) {
          throw formattedError;
        }
        if (data) {
          claimed.push(ledgerEntryFromRow(data as SupabaseCashbackLedgerRow));
        }
      }

      return claimed;
    },
    async updatePlatformFeeCollection({
      executionKey,
      collectionStatus,
      ledgerStatus,
      transferSignature,
      errorText,
      attempts,
      transactionBase64,
      recentBlockhash,
      lastValidBlockHeight,
      expectedLeaseToken,
      releaseLease = false
    }) {
      const values: Record<string, unknown> = {
        platform_fee_collection_status: collectionStatus,
        platform_fee_collection_updated_at: new Date().toISOString(),
        updated_at: new Date().toISOString()
      };

      if (ledgerStatus) {
        values.status = ledgerStatus;
      }

      if (transferSignature !== undefined) {
        values.platform_fee_transfer_signature = transferSignature;
      }

      if (errorText !== undefined) {
        values.platform_fee_collection_error = errorText;
      }

      if (attempts !== undefined) {
        values.platform_fee_collection_attempts = attempts;
      }

      if (transactionBase64 !== undefined) {
        values.platform_fee_transaction_base64 = transactionBase64;
      }

      if (recentBlockhash !== undefined) {
        values.platform_fee_recent_blockhash = recentBlockhash;
      }

      if (lastValidBlockHeight !== undefined) {
        values.platform_fee_last_valid_block_height = lastValidBlockHeight;
      }

      if (releaseLease) {
        values.platform_fee_lease_token = null;
        values.platform_fee_lease_expires_at = null;
      }

      let query = client
        .from("telegram_cashback_ledger")
        .update(values)
        .eq("execution_key", executionKey);
      if (expectedLeaseToken !== undefined) {
        query = expectedLeaseToken === null
          ? query.is("platform_fee_lease_token", null)
          : query.eq("platform_fee_lease_token", expectedLeaseToken);
      }
      const { data, error } = await query
        .select("execution_key")
        .maybeSingle();
      const formattedError = formatSupabaseError(error);
      if (formattedError) {
        throw formattedError;
      }
      return Boolean(data);
    },
    async getSummary({ chatId, tradingWalletPublicKey, payoutWalletPublicKey = null, config }) {
      const normalizedChatId = String(chatId);
      const [ledger, payouts] = await Promise.all([
        selectLedger(normalizedChatId, tradingWalletPublicKey),
        selectPayouts(normalizedChatId, tradingWalletPublicKey)
      ]);
      const accruedLamports = ledger
        .filter((entry) => entry.status !== "voided")
        .reduce((sum, entry) => sum + entry.cashbackLamports, 0n);
      const claimableLamports = ledger
        .filter((entry) => entry.status === "claimable")
        .reduce((sum, entry) => sum + entry.cashbackLamports, 0n);
      const pendingLamports = ledger
        .filter((entry) => entry.status === "pending")
        .reduce((sum, entry) => sum + entry.cashbackLamports, 0n) +
        payouts
          .filter((payout) => payout.status === "pending" || payout.status === "submitted")
          .reduce((sum, payout) => sum + payout.amountLamports, 0n);
      const lifetimePaidLamports = payouts
        .filter((payout) => payout.status === "submitted" || payout.status === "confirmed")
        .reduce((sum, payout) => sum + payout.amountLamports, 0n);
      const hasOpenPayout = payouts.some((payout) => payout.status === "pending" || payout.status === "submitted");

      return {
        enabled: config.enabled,
        tradingWalletPublicKey,
        payoutWalletPublicKey,
        accruedLamports,
        claimableLamports,
        pendingLamports,
        lifetimePaidLamports,
        minClaimLamports: config.minClaimLamports,
        payoutUnavailableReason: (!config.enabled ? "cashback is disabled" : null) ||
          cashbackConfigBlockedReason(config) ||
          (!payoutWalletPublicKey ? "add a payout wallet" : null) ||
          (!config.payoutWalletSecretKey ? "payout sender is not configured" : null) ||
          (hasOpenPayout ? "payout is already pending" : null)
      };
    },
    async listClaimableEntries({ chatId, tradingWalletPublicKey }) {
      return (await selectLedger(String(chatId), tradingWalletPublicKey))
        .filter((entry) => entry.status === "claimable")
        .sort((left, right) => String(left.id || "").localeCompare(String(right.id || "")));
    },
    async createPayout(payout) {
      const { data, error } = await client
        .from("telegram_cashback_payouts")
        .insert(payoutRow(payout))
        .select("*")
        .single();
      const formattedError = formatSupabaseError(error);
      if (formattedError) {
        throw formattedError;
      }

      return payoutFromRow(data as SupabaseCashbackPayoutRow);
    },
    async updatePayout({ id, status, signature = null, errorText = null }) {
      const { error } = await client
        .from("telegram_cashback_payouts")
        .update({
          status,
          signature,
          error_text: errorText,
          updated_at: new Date().toISOString()
        })
        .eq("id", id);
      const formattedError = formatSupabaseError(error);
      if (formattedError) {
        throw formattedError;
      }
    },
    async markLedgerPaid({ ids }) {
      if (ids.length === 0) {
        return;
      }

      const { error } = await client
        .from("telegram_cashback_ledger")
        .update({
          status: "paid",
          updated_at: new Date().toISOString()
        })
        .in("id", ids);
      const formattedError = formatSupabaseError(error);
      if (formattedError) {
        throw formattedError;
      }
    },
    async getReconciliationReport() {
      const [ledger, payouts] = await Promise.all([selectLedger(), selectPayouts()]);
      const platformFeeLamports = ledger
        .filter((entry) => entry.status !== "voided")
        .reduce((sum, entry) => sum + entry.platformFeeLamports, 0n);
      const cashbackAccruedLamports = ledger
        .filter((entry) => entry.status !== "voided")
        .reduce((sum, entry) => sum + entry.cashbackLamports, 0n);
      const cashbackPaidLamports = payouts
        .filter((payout) => payout.status === "submitted" || payout.status === "confirmed")
        .reduce((sum, payout) => sum + payout.amountLamports, 0n);
      const pendingPayoutLamports = payouts
        .filter((payout) => payout.status === "pending")
        .reduce((sum, payout) => sum + payout.amountLamports, 0n);

      return {
        platformFeeLamports,
        cashbackAccruedLamports,
        cashbackPaidLamports,
        outstandingLiabilityLamports: cashbackAccruedLamports - cashbackPaidLamports,
        pendingPayoutLamports
      };
    }
  };
}

export function parseCashbackPayoutKeypair(secretKey: string): Keypair {
  const trimmed = secretKey.trim();

  try {
    const parsed = JSON.parse(trimmed) as unknown;
    if (Array.isArray(parsed)) {
      return Keypair.fromSecretKey(Uint8Array.from(parsed.map((value) => Number(value))));
    }
  } catch {
    // Continue to base58/base64 parsing.
  }

  try {
    return Keypair.fromSecretKey(bs58.decode(trimmed));
  } catch {
    return Keypair.fromSecretKey(Uint8Array.from(Buffer.from(trimmed, "base64")));
  }
}

export async function sendCashbackPayout({
  connection,
  secretKey,
  expectedPayoutWalletPublicKey,
  recipientPublicKey,
  amountLamports
}: {
  connection: Connection;
  secretKey: string;
  expectedPayoutWalletPublicKey?: string | null;
  recipientPublicKey: string;
  amountLamports: bigint;
}): Promise<string> {
  if (amountLamports <= 0n) {
    throw new Error("cashback payout amount must be positive");
  }

  const payer = parseCashbackPayoutKeypair(secretKey);
  if (expectedPayoutWalletPublicKey && payer.publicKey.toBase58() !== expectedPayoutWalletPublicKey) {
    throw new Error("CASHBACK_PAYOUT_WALLET_SECRET_KEY does not match CASHBACK_PAYOUT_WALLET_PUBLIC_KEY");
  }

  const recipient = new PublicKey(recipientPublicKey);
  const lamports = Number(amountLamports);
  if (!Number.isSafeInteger(lamports)) {
    throw new Error("cashback payout amount exceeds safe integer lamports");
  }

  const transaction = new Transaction().add(
    SystemProgram.transfer({
      fromPubkey: payer.publicKey,
      toPubkey: recipient,
      lamports
    })
  );

  return sendAndConfirmTransaction(connection, transaction, [payer], {
    commitment: "confirmed"
  });
}

export async function claimCashback({
  store,
  config,
  connection,
  chatId,
  tradingWalletPublicKey,
  payoutWalletPublicKey
}: {
  store: CashbackStore;
  config: CashbackConfig;
  connection: Connection;
  chatId: TelegramChatId;
  tradingWalletPublicKey: string | null;
  payoutWalletPublicKey?: string | null;
}): Promise<CashbackClaimResult> {
  const summary = await store.getSummary({ chatId, tradingWalletPublicKey, payoutWalletPublicKey, config });
  if (!tradingWalletPublicKey) {
    return { ok: false, status: "unavailable", summary, signature: null, errorText: "No active trading wallet." };
  }

  if (!payoutWalletPublicKey) {
    return { ok: false, status: "unavailable", summary, signature: null, errorText: "Add a payout wallet first." };
  }

  if (summary.payoutUnavailableReason || !config.payoutWalletSecretKey) {
    return { ok: false, status: "unavailable", summary, signature: null, errorText: summary.payoutUnavailableReason };
  }

  if (summary.claimableLamports < config.minClaimLamports) {
    return { ok: false, status: "below_threshold", summary, signature: null, errorText: null };
  }

  if (config.maxPayoutLamportsPerDay > 0n && summary.claimableLamports > config.maxPayoutLamportsPerDay) {
    return {
      ok: false,
      status: "unavailable",
      summary,
      signature: null,
      errorText: "Claimable balance is above CASHBACK_MAX_PAYOUT_SOL_PER_DAY."
    };
  }

  const claimableEntries = await store.listClaimableEntries({ chatId, tradingWalletPublicKey });
  const amountLamports = claimableEntries.reduce((sum, entry) => sum + entry.cashbackLamports, 0n);
  const payout = await store.createPayout({
    chatId: String(chatId),
    tradingWalletPublicKey,
    amountLamports,
    status: "pending",
    signature: null,
    errorText: null
  });

  let sentSignature: string | null = null;

  try {
    sentSignature = await sendCashbackPayout({
      connection,
      secretKey: config.payoutWalletSecretKey,
      expectedPayoutWalletPublicKey: config.payoutWalletPublicKey,
      recipientPublicKey: payoutWalletPublicKey,
      amountLamports
    });
    await store.updatePayout({ id: payout.id || "", status: "submitted", signature: sentSignature, errorText: null });
    await store.markLedgerPaid({ ids: claimableEntries.map((entry) => entry.id).filter((id): id is string | number => id !== undefined) });
    await store.updatePayout({ id: payout.id || "", status: "confirmed", signature: sentSignature, errorText: null });
    const nextSummary = await store.getSummary({ chatId, tradingWalletPublicKey, payoutWalletPublicKey, config });
    return { ok: true, status: "submitted", summary: nextSummary, signature: sentSignature, errorText: null };
  } catch (error) {
    const errorText = error instanceof Error ? error.message : String(error);
    await store.updatePayout({
      id: payout.id || "",
      status: sentSignature ? "submitted" : "failed",
      signature: sentSignature,
      errorText
    });
    const nextSummary = await store.getSummary({ chatId, tradingWalletPublicKey, payoutWalletPublicKey, config });
    return { ok: false, status: "failed", summary: nextSummary, signature: null, errorText };
  }
}
