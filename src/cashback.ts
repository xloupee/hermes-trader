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

export type CashbackLedgerStatus = "pending" | "claimable" | "paid" | "voided";
export type CashbackPayoutStatus = "pending" | "submitted" | "confirmed" | "failed";

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
  action: CopyTradeExecutionAction;
  platformFeeLamports: bigint;
  cashbackLamports: bigint;
  status: CashbackLedgerStatus;
  createdAt?: string | null;
  updatedAt?: string | null;
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
  action: CopyTradeExecutionAction;
  platform_fee_lamports: string | number;
  cashback_lamports: string | number;
  status: CashbackLedgerStatus;
  created_at?: string | null;
  updated_at?: string | null;
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
  const feeShare = Number(env.CASHBACK_FEE_SHARE_BPS ?? 0);
  const minClaim = Number(env.CASHBACK_MIN_CLAIM_SOL ?? 0.005);
  const maxPerDay = Number(env.CASHBACK_MAX_PAYOUT_SOL_PER_DAY ?? 0);

  return {
    enabled: env.CASHBACK_ENABLED === "true",
    feeShareBps: Number.isFinite(feeShare) ? Math.floor(feeShare) : 0,
    minClaimLamports: solToCashbackLamports(Number.isFinite(minClaim) ? minClaim : 0.005),
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
    summary.claimableLamports >= summary.minClaimLamports
  ) {
    keyboard.push([{ text: "Claim Cashback", callback_data: "cashback:claim" }]);
  }

  keyboard.push([{ text: summary.payoutWalletPublicKey ? "Change Payout Wallet" : "Add Payout Wallet", callback_data: "cashback:set_payout_wallet" }]);
  keyboard.push([{ text: "Refresh", callback_data: "cashback:dashboard" }]);

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
