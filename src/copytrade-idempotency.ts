import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname } from "node:path";
import { createClient } from "@supabase/supabase-js";
import { asRecord, errorMessage } from "./types.js";
import type {
  PumpPortalLightningTradeRequest,
  PumpPortalLightningTradeResult,
  WalletTradeData
} from "./types.js";

export type CopyTradeBuyIdempotencyStatus = "claimed" | "submitted" | "failed";

export interface CopyTradeBuyIdempotencyRecord {
  key: string;
  chatId: string;
  sourceWalletAddress: string;
  tradingWalletPublicKey: string;
  observedSignature: string;
  mint: string;
  action: "buy";
  amountSol: number;
  provider: WalletTradeData["provider"];
  request: PumpPortalLightningTradeRequest;
  status: CopyTradeBuyIdempotencyStatus;
  resultSignature: string | null;
  errorText: string | null;
  httpStatus: number | null;
  response: unknown;
  claimedAt: string;
  updatedAt: string;
  completedAt: string | null;
}

export interface CopyTradeBuyIdempotencyClaimInput {
  key: string;
  chatId: string;
  sourceWalletAddress: string;
  tradingWalletPublicKey: string;
  observedSignature: string;
  mint: string;
  action?: "buy";
  amountSol: number;
  provider: WalletTradeData["provider"];
  request: PumpPortalLightningTradeRequest;
  retryFailed?: boolean;
  now?: string;
}

export interface CopyTradeBuyIdempotencyClaimResult {
  claimed: boolean;
  existing: CopyTradeBuyIdempotencyRecord | null;
}

export interface CopyTradeBuyIdempotencyStore {
  claimBuy: (input: CopyTradeBuyIdempotencyClaimInput) => Promise<CopyTradeBuyIdempotencyClaimResult>;
  completeBuy: (key: string, result: PumpPortalLightningTradeResult) => Promise<void>;
  failBuy: (key: string, errorText: string) => Promise<void>;
}

export function copyTradeBuyIdempotencyKey({
  chatId,
  mint,
  action = "buy"
}: {
  chatId: string | null | undefined;
  mint: string | null | undefined;
  action?: "buy";
}): string | null {
  const normalizedChatId = chatId?.trim();
  const normalizedMint = mint?.trim();

  if (!normalizedChatId || !normalizedMint) {
    return null;
  }

  return [normalizedChatId, normalizedMint, action].join(":");
}

interface StoredCopyTradeBuyIdempotencyFile {
  version: 1;
  records: CopyTradeBuyIdempotencyRecord[];
}

interface SupabaseErrorLike {
  code?: string;
  message?: string;
}

interface SupabaseCopyTradeBuyIdempotencyRow {
  idempotency_key: string;
  chat_id: string;
  source_wallet_address: string;
  trading_wallet_public_key: string;
  observed_signature: string;
  mint: string;
  action: "buy";
  amount_sol: string | number;
  provider: string;
  request: unknown;
  status: CopyTradeBuyIdempotencyStatus;
  result_signature: string | null;
  error_text: string | null;
  http_status: number | null;
  response: unknown;
  claimed_at: string;
  updated_at: string;
  completed_at: string | null;
}

type SupabaseClientLike = {
  // Supabase's generated table types are not available in this repo, so this stays deliberately loose.
  from: (table: string) => any;
};

function baseRecord(input: CopyTradeBuyIdempotencyClaimInput): CopyTradeBuyIdempotencyRecord {
  const now = input.now || new Date().toISOString();

  return {
    key: input.key,
    chatId: input.chatId,
    sourceWalletAddress: input.sourceWalletAddress,
    tradingWalletPublicKey: input.tradingWalletPublicKey,
    observedSignature: input.observedSignature,
    mint: input.mint,
    action: "buy",
    amountSol: input.amountSol,
    provider: input.provider,
    request: input.request,
    status: "claimed",
    resultSignature: null,
    errorText: null,
    httpStatus: null,
    response: null,
    claimedAt: now,
    updatedAt: now,
    completedAt: null
  };
}

function normalizeRecord(value: unknown): CopyTradeBuyIdempotencyRecord | null {
  const record = asRecord(value);
  const amountSol = Number(record.amountSol ?? record.amount_sol);
  const status = record.status === "submitted" || record.status === "failed" ? record.status : "claimed";
  const provider = record.provider === "pumpportal" ? "pumpportal" : "helius";
  const request = asRecord(record.request) as unknown as PumpPortalLightningTradeRequest;

  if (
    typeof record.key !== "string" ||
    typeof record.chatId !== "string" ||
    typeof record.sourceWalletAddress !== "string" ||
    typeof record.tradingWalletPublicKey !== "string" ||
    typeof record.observedSignature !== "string" ||
    typeof record.mint !== "string" ||
    !Number.isFinite(amountSol)
  ) {
    return null;
  }

  return {
    key: record.key,
    chatId: record.chatId,
    sourceWalletAddress: record.sourceWalletAddress,
    tradingWalletPublicKey: record.tradingWalletPublicKey,
    observedSignature: record.observedSignature,
    mint: record.mint,
    action: "buy",
    amountSol,
    provider,
    request,
    status,
    resultSignature: typeof record.resultSignature === "string" ? record.resultSignature : null,
    errorText: typeof record.errorText === "string" ? record.errorText : null,
    httpStatus: Number.isFinite(Number(record.httpStatus)) ? Number(record.httpStatus) : null,
    response: record.response ?? null,
    claimedAt: typeof record.claimedAt === "string" ? record.claimedAt : new Date().toISOString(),
    updatedAt: typeof record.updatedAt === "string" ? record.updatedAt : new Date().toISOString(),
    completedAt: typeof record.completedAt === "string" ? record.completedAt : null
  };
}

function recordFromSupabaseRow(row: SupabaseCopyTradeBuyIdempotencyRow): CopyTradeBuyIdempotencyRecord {
  return {
    key: row.idempotency_key,
    chatId: row.chat_id,
    sourceWalletAddress: row.source_wallet_address,
    tradingWalletPublicKey: row.trading_wallet_public_key,
    observedSignature: row.observed_signature,
    mint: row.mint,
    action: "buy",
    amountSol: Number(row.amount_sol),
    provider: row.provider === "pumpportal" ? "pumpportal" : "helius",
    request: asRecord(row.request) as unknown as PumpPortalLightningTradeRequest,
    status: row.status,
    resultSignature: row.result_signature,
    errorText: row.error_text,
    httpStatus: row.http_status,
    response: row.response,
    claimedAt: row.claimed_at,
    updatedAt: row.updated_at,
    completedAt: row.completed_at
  };
}

function supabaseRowFromRecord(record: CopyTradeBuyIdempotencyRecord): SupabaseCopyTradeBuyIdempotencyRow {
  return {
    idempotency_key: record.key,
    chat_id: record.chatId,
    source_wallet_address: record.sourceWalletAddress,
    trading_wallet_public_key: record.tradingWalletPublicKey,
    observed_signature: record.observedSignature,
    mint: record.mint,
    action: record.action,
    amount_sol: record.amountSol,
    provider: record.provider,
    request: record.request,
    status: record.status,
    result_signature: record.resultSignature,
    error_text: record.errorText,
    http_status: record.httpStatus,
    response: record.response,
    claimed_at: record.claimedAt,
    updated_at: record.updatedAt,
    completed_at: record.completedAt
  };
}

function completedRecord(
  record: CopyTradeBuyIdempotencyRecord,
  result: PumpPortalLightningTradeResult,
  now = new Date().toISOString()
): CopyTradeBuyIdempotencyRecord {
  return {
    ...record,
    status: result.ok ? "submitted" : "failed",
    resultSignature: result.signature,
    errorText: result.errorText,
    httpStatus: result.status,
    response: result.raw,
    updatedAt: now,
    completedAt: now
  };
}

function failedRecord(
  record: CopyTradeBuyIdempotencyRecord,
  errorText: string,
  now = new Date().toISOString()
): CopyTradeBuyIdempotencyRecord {
  return {
    ...record,
    status: "failed",
    errorText,
    updatedAt: now,
    completedAt: now
  };
}

function sameSemanticBuy(record: CopyTradeBuyIdempotencyRecord, input: CopyTradeBuyIdempotencyClaimInput): boolean {
  return record.chatId === input.chatId && record.mint === input.mint && record.action === (input.action || "buy");
}

function retryClaimedRecord(
  existing: CopyTradeBuyIdempotencyRecord,
  input: CopyTradeBuyIdempotencyClaimInput
): CopyTradeBuyIdempotencyRecord {
  const now = input.now || new Date().toISOString();

  return {
    ...baseRecord({ ...input, now }),
    key: existing.key,
    claimedAt: now,
    updatedAt: now
  };
}

async function readLocalFile(path: string): Promise<StoredCopyTradeBuyIdempotencyFile> {
  try {
    const parsed = JSON.parse(await readFile(path, "utf8")) as unknown;
    const record = asRecord(parsed);
    const records = Array.isArray(record.records)
      ? record.records.map(normalizeRecord).filter((entry): entry is CopyTradeBuyIdempotencyRecord => Boolean(entry))
      : [];

    return {
      version: 1,
      records
    };
  } catch (error) {
    if (typeof error === "object" && error !== null && "code" in error && error.code === "ENOENT") {
      return {
        version: 1,
        records: []
      };
    }

    throw error;
  }
}

async function writeLocalFile(path: string, data: StoredCopyTradeBuyIdempotencyFile): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  const tempPath = `${path}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(tempPath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
  await rename(tempPath, path);
}

export function createJsonCopyTradeBuyIdempotencyStore({
  path
}: {
  path: string;
}): CopyTradeBuyIdempotencyStore {
  let queue = Promise.resolve();

  function withLock<T>(fn: () => Promise<T>): Promise<T> {
    const run = queue.then(fn, fn);
    queue = run.then(
      () => undefined,
      () => undefined
    );
    return run;
  }

  return {
    claimBuy(input) {
      return withLock(async () => {
        const data = await readLocalFile(path);
        const existing = data.records.find((record) => record.key === input.key || sameSemanticBuy(record, input)) || null;

        if (existing) {
          if (input.retryFailed && existing.status === "failed") {
            data.records = data.records.map((record) =>
              record.key === existing.key ? retryClaimedRecord(existing, input) : record
            );
            await writeLocalFile(path, data);

            return {
              claimed: true,
              existing: null
            };
          }

          return {
            claimed: false,
            existing
          };
        }

        const record = baseRecord(input);
        data.records.push(record);
        await writeLocalFile(path, data);

        return {
          claimed: true,
          existing: null
        };
      });
    },
    completeBuy(key, result) {
      return withLock(async () => {
        const data = await readLocalFile(path);
        data.records = data.records.map((record) => (record.key === key ? completedRecord(record, result) : record));
        await writeLocalFile(path, data);
      });
    },
    failBuy(key, errorText) {
      return withLock(async () => {
        const data = await readLocalFile(path);
        data.records = data.records.map((record) => (record.key === key ? failedRecord(record, errorText) : record));
        await writeLocalFile(path, data);
      });
    }
  };
}

function isUniqueViolation(error: SupabaseErrorLike | null): boolean {
  return Boolean(
    error &&
      (error.code === "23505" || /duplicate key|already exists|violates unique/i.test(error.message || ""))
  );
}

function isMissingSupabaseRelation(error: SupabaseErrorLike | null): boolean {
  return Boolean(
    error &&
      (
        error.code === "42P01" ||
        error.code === "PGRST205" ||
        /Could not find the table|schema cache|relation .* does not exist/i.test(error.message || "")
      )
  );
}

function formatSupabaseError(error: SupabaseErrorLike | null): Error | null {
  return error ? new Error(error.message || "Supabase copy trade idempotency request failed") : null;
}

export function createSupabaseCopyTradeBuyIdempotencyStore({
  url,
  serviceRoleKey,
  fallback,
  client: providedClient
}: {
  url?: string;
  serviceRoleKey?: string;
  fallback?: CopyTradeBuyIdempotencyStore;
  client?: SupabaseClientLike;
}): CopyTradeBuyIdempotencyStore {
  if (!providedClient && (!url || !serviceRoleKey)) {
    throw new Error("Supabase copy trade idempotency store requires url and serviceRoleKey");
  }

  const client: SupabaseClientLike = providedClient || createClient(url || "", serviceRoleKey || "", {
    auth: {
      persistSession: false,
      autoRefreshToken: false
    }
  });
  let useFallback = false;
  let warnedAboutFallback = false;

  function activateFallback(error: SupabaseErrorLike | null): boolean {
    if (!fallback || !isMissingSupabaseRelation(error)) {
      return false;
    }

    useFallback = true;

    if (!warnedAboutFallback) {
      warnedAboutFallback = true;
      console.warn(
        `Supabase copy trade idempotency table is unavailable; using local JSON idempotency fallback: ${
          error?.message || "missing relation"
        }`
      );
    }

    return true;
  }

  async function readExisting(input: CopyTradeBuyIdempotencyClaimInput): Promise<CopyTradeBuyIdempotencyRecord | null> {
    const byKey = await client
      .from("telegram_copytrade_buy_idempotency")
      .select("*")
      .eq("idempotency_key", input.key)
      .maybeSingle();
    const keyError = formatSupabaseError(byKey.error);

    if (keyError) {
      throw keyError;
    }

    if (byKey.data) {
      return recordFromSupabaseRow(byKey.data as SupabaseCopyTradeBuyIdempotencyRow);
    }

    const bySemantic = await client
      .from("telegram_copytrade_buy_idempotency")
      .select("*")
      .eq("chat_id", input.chatId)
      .eq("mint", input.mint)
      .eq("action", input.action || "buy")
      .order("claimed_at", { ascending: true })
      .limit(1)
      .maybeSingle();
    const semanticError = formatSupabaseError(bySemantic.error);

    if (semanticError) {
      throw semanticError;
    }

    return bySemantic.data ? recordFromSupabaseRow(bySemantic.data as SupabaseCopyTradeBuyIdempotencyRow) : null;
  }

  async function reclaimFailed(input: CopyTradeBuyIdempotencyClaimInput, existing: CopyTradeBuyIdempotencyRecord) {
    if (!input.retryFailed || existing.status !== "failed") {
      return null;
    }

    const retryRecord = retryClaimedRecord(existing, input);
    const { data, error } = await client
      .from("telegram_copytrade_buy_idempotency")
      .update({
        source_wallet_address: retryRecord.sourceWalletAddress,
        trading_wallet_public_key: retryRecord.tradingWalletPublicKey,
        observed_signature: retryRecord.observedSignature,
        amount_sol: retryRecord.amountSol,
        provider: retryRecord.provider,
        request: retryRecord.request,
        status: retryRecord.status,
        result_signature: retryRecord.resultSignature,
        error_text: retryRecord.errorText,
        http_status: retryRecord.httpStatus,
        response: retryRecord.response,
        claimed_at: retryRecord.claimedAt,
        updated_at: retryRecord.updatedAt,
        completed_at: retryRecord.completedAt
      })
      .eq("idempotency_key", existing.key)
      .eq("status", "failed")
      .select("*")
      .maybeSingle();
    const formattedError = formatSupabaseError(error);

    if (formattedError) {
      throw formattedError;
    }

    return data ? recordFromSupabaseRow(data as SupabaseCopyTradeBuyIdempotencyRow) : null;
  }

  return {
    async claimBuy(input) {
      if (useFallback && fallback) {
        return fallback.claimBuy(input);
      }

      const record = baseRecord(input);
      const { error } = await client
        .from("telegram_copytrade_buy_idempotency")
        .insert(supabaseRowFromRecord(record));

      if (!error) {
        return {
          claimed: true,
          existing: null
        };
      }

      if (isUniqueViolation(error)) {
        const existing = await readExisting(input);
        const retried = existing ? await reclaimFailed(input, existing) : null;

        if (retried) {
          return {
            claimed: true,
            existing: null
          };
        }

        return {
          claimed: false,
          existing: existing || await readExisting(input)
        };
      }

      if (activateFallback(error) && fallback) {
        return fallback.claimBuy(input);
      }

      const formattedError = formatSupabaseError(error);
      throw formattedError || new Error("Supabase copy trade idempotency request failed");
    },
    async completeBuy(key, result) {
      if (useFallback && fallback) {
        return fallback.completeBuy(key, result);
      }

      const now = new Date().toISOString();
      const { error } = await client
        .from("telegram_copytrade_buy_idempotency")
        .update({
          status: result.ok ? "submitted" : "failed",
          result_signature: result.signature,
          error_text: result.errorText,
          http_status: result.status,
          response: result.raw,
          updated_at: now,
          completed_at: now
        })
        .eq("idempotency_key", key);

      if (activateFallback(error) && fallback) {
        return fallback.completeBuy(key, result);
      }

      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    },
    async failBuy(key, errorText) {
      if (useFallback && fallback) {
        return fallback.failBuy(key, errorText);
      }

      const now = new Date().toISOString();
      const { error } = await client
        .from("telegram_copytrade_buy_idempotency")
        .update({
          status: "failed",
          error_text: errorText,
          updated_at: now,
          completed_at: now
        })
        .eq("idempotency_key", key);

      if (activateFallback(error) && fallback) {
        return fallback.failBuy(key, errorText);
      }

      const formattedError = formatSupabaseError(error);

      if (formattedError) {
        throw formattedError;
      }
    }
  };
}

export async function safelyCompleteCopyTradeBuyIdempotency(
  store: CopyTradeBuyIdempotencyStore,
  key: string,
  result: PumpPortalLightningTradeResult
): Promise<void> {
  try {
    await store.completeBuy(key, result);
  } catch (error) {
    console.warn(`Could not mark copy buy idempotency key ${key} complete: ${errorMessage(error)}`);
  }
}

export async function safelyFailCopyTradeBuyIdempotency(
  store: CopyTradeBuyIdempotencyStore,
  key: string,
  reason: string
): Promise<void> {
  try {
    await store.failBuy(key, reason);
  } catch (error) {
    console.warn(`Could not mark copy buy idempotency key ${key} failed: ${errorMessage(error)}`);
  }
}
