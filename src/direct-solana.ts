import BN from "bn.js";
import { createRequire } from "node:module";
import {
  ComputeBudgetProgram,
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  TransactionInstruction,
  TransactionMessage,
  VersionedTransaction
} from "@solana/web3.js";
import { getAssociatedTokenAddressSync, NATIVE_MINT, TOKEN_2022_PROGRAM_ID, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { buildDirectAutoTransactionPayload } from "./direct-auto.js";
import { maxQuoteLamportsForSlippageCap } from "./direct-budget.js";
import type { DirectTransactionPayload } from "./direct-pump.js";
import type { PlatformFeeSplit } from "./platform-fee.js";
import type {
  DirectExecutionTimingMetadata,
  DirectExecutionGateConfig,
  DirectTradeExecutionProvider,
  TradeAmountBasis,
  TradeExecutionResult
} from "./trade-execution.js";
import {
  buildDirectRouteMetadata,
  directExecutionLiveBlockedReason,
  tradeExecutionFailedResult,
  tradeExecutionSkippedResult
} from "./trade-execution.js";

const DEFAULT_COMPUTE_UNIT_LIMIT = 250_000;
const LAMPORTS_PER_SOL = 1_000_000_000n;
const require = createRequire(import.meta.url);
const DIRECT_SIMULATION_CONFIG = {
  replaceRecentBlockhash: true,
  sigVerify: false
} as const;

type PumpSdkModule = typeof import("@pump-fun/pump-sdk");
type PumpSwapSdkModule = typeof import("@pump-fun/pump-swap-sdk");
type OnlinePumpSdkInstance = InstanceType<PumpSdkModule["OnlinePumpSdk"]>;
type OnlinePumpAmmSdkInstance = InstanceType<PumpSwapSdkModule["OnlinePumpAmmSdk"]>;
type PumpGlobal = Awaited<ReturnType<OnlinePumpSdkInstance["fetchGlobal"]>>;
type PumpFeeConfig = Awaited<ReturnType<OnlinePumpSdkInstance["fetchFeeConfig"]>>;
type LatestBlockhash = Awaited<ReturnType<Connection["getLatestBlockhash"]>>;

let pumpSdkModule: PumpSdkModule | null = null;
let pumpSwapSdkModule: PumpSwapSdkModule | null = null;
const pumpSdkByConnection = new WeakMap<Connection, OnlinePumpSdkInstance>();
const pumpAmmSdkByConnection = new WeakMap<Connection, OnlinePumpAmmSdkInstance>();
const PUMP_CONFIG_CACHE_MS = 60_000;
const MINT_TOKEN_PROGRAM_CACHE_MS = 10 * 60_000;
const PUMP_FAST_BUY_STATE_CACHE_MS = 2 * 60_000;
const OBSERVED_BUY_FAST_STATE_MAX_AGE_MS = 1_000;
const PUMP_TOKEN_2022_VERIFIED_CREATOR_CACHE_MS = 60_000;
const MAX_BLOCKHASH_CACHE_MS = 30_000;
const latestBlockhashByConnection = new WeakMap<Connection, {
  value: LatestBlockhash | null;
  expiresAtMs: number;
  inflight: Promise<LatestBlockhash> | null;
}>();
const mintTokenProgramByConnection = new WeakMap<Connection, Map<string, {
  value: PublicKey;
  expiresAtMs: number;
}>>();
let pumpGlobalCache: { value: PumpGlobal; expiresAtMs: number } | null = null;
let pumpGlobalInflight: Promise<PumpGlobal> | null = null;
let pumpFeeConfigCache: { value: PumpFeeConfig | null; expiresAtMs: number } | null = null;
let pumpFeeConfigInflight: Promise<PumpFeeConfig | null> | null = null;
const directPumpFastBuyStateByMint = new Map<string, DirectPumpFastBuyStateCacheEntry>();
const directPumpFastBuyStateInflightByMint = new Map<string, Promise<boolean>>();

export interface DirectPumpFastBuyStateInput {
  mint: string;
  creator: string;
  creatorVerified?: boolean;
  tokenProgram: string;
  virtualTokenReserves: string | bigint;
  virtualQuoteReserves: string | bigint;
  realTokenReserves: string | bigint;
  realQuoteReserves: string | bigint;
  tokenTotalSupply: string | bigint;
  complete?: boolean | null;
  isMayhemMode?: boolean | null;
  isCashbackCoin?: boolean | null;
  quoteMint?: string | null;
  source?: string | null;
  observedAtMs?: number;
  cacheMs?: number;
}

export interface DirectPumpFastBuyStateReserveUpdate {
  mint: string;
  virtualTokenReserves: string | bigint;
  virtualQuoteReserves: string | bigint;
  source?: string | null;
  observedAtMs?: number;
  cacheMs?: number;
}

export interface DirectPumpFastBuyStateChainSnapshot extends DirectPumpFastBuyStateInput {
  source: string;
}

interface DirectPumpFastBuyStateCacheEntry {
  mint: PublicKey;
  creator: PublicKey;
  creatorVault: PublicKey;
  creatorVerified: boolean;
  creatorVerifiedAtMs: number | null;
  creatorSource: string | null;
  tokenProgram: PublicKey;
  virtualTokenReserves: BN;
  virtualQuoteReserves: BN;
  realTokenReserves: BN;
  realQuoteReserves: BN;
  tokenTotalSupply: BN;
  complete: boolean;
  isMayhemMode: boolean;
  isCashbackCoin: boolean;
  quoteMint: PublicKey;
  source: string | null;
  observedAtMs: number;
  expiresAtMs: number;
}

function blockhashCacheMsFromConfig(value: number | undefined): number {
  return Math.min(Math.max(0, Math.floor(Number.isFinite(value) ? value || 0 : 0)), MAX_BLOCKHASH_CACHE_MS);
}

function loadPumpSdk(): PumpSdkModule {
  pumpSdkModule ||= require("@pump-fun/pump-sdk") as PumpSdkModule;
  return pumpSdkModule;
}

function loadPumpSwapSdk(): PumpSwapSdkModule {
  pumpSwapSdkModule ||= require("@pump-fun/pump-swap-sdk") as PumpSwapSdkModule;
  return pumpSwapSdkModule;
}

function getOnlinePumpSdk(connection: Connection, module = loadPumpSdk()): OnlinePumpSdkInstance {
  const cached = pumpSdkByConnection.get(connection);
  if (cached) {
    return cached;
  }

  const sdk = new module.OnlinePumpSdk(connection);
  pumpSdkByConnection.set(connection, sdk);
  return sdk;
}

function getOnlinePumpAmmSdk(connection: Connection, module = loadPumpSwapSdk()): OnlinePumpAmmSdkInstance {
  const cached = pumpAmmSdkByConnection.get(connection);
  if (cached) {
    return cached;
  }

  const sdk = new module.OnlinePumpAmmSdk(connection);
  pumpAmmSdkByConnection.set(connection, sdk);
  return sdk;
}

async function fetchCachedPumpGlobal(sdk: OnlinePumpSdkInstance, { forceRefresh = false }: { forceRefresh?: boolean } = {}): Promise<PumpGlobal> {
  const now = Date.now();
  if (!forceRefresh && pumpGlobalCache && pumpGlobalCache.expiresAtMs > now) {
    return pumpGlobalCache.value;
  }

  pumpGlobalInflight ||= sdk.fetchGlobal().then((value) => {
    pumpGlobalCache = { value, expiresAtMs: Date.now() + PUMP_CONFIG_CACHE_MS };
    return value;
  }).finally(() => {
    pumpGlobalInflight = null;
  });

  return pumpGlobalInflight;
}

async function fetchCachedPumpFeeConfig(sdk: OnlinePumpSdkInstance, { forceRefresh = false }: { forceRefresh?: boolean } = {}): Promise<PumpFeeConfig | null> {
  const now = Date.now();
  if (!forceRefresh && pumpFeeConfigCache && pumpFeeConfigCache.expiresAtMs > now) {
    return pumpFeeConfigCache.value;
  }

  pumpFeeConfigInflight ||= sdk.fetchFeeConfig().catch(() => null).then((value) => {
    pumpFeeConfigCache = { value, expiresAtMs: Date.now() + PUMP_CONFIG_CACHE_MS };
    return value;
  }).finally(() => {
    pumpFeeConfigInflight = null;
  });

  return pumpFeeConfigInflight;
}

export async function warmDirectSolanaSdk({
  connection,
  provider,
  forceRefresh = false
}: {
  connection: Connection;
  provider: DirectTradeExecutionProvider;
  forceRefresh?: boolean;
}): Promise<void> {
  const warmups: Promise<unknown>[] = [];

  if (provider === "direct-pump" || provider === "direct-auto") {
    const pumpModule = loadPumpSdk();
    const pumpSdk = getOnlinePumpSdk(connection, pumpModule);
    warmups.push(
      fetchCachedPumpGlobal(pumpSdk, { forceRefresh }),
      fetchCachedPumpFeeConfig(pumpSdk, { forceRefresh })
    );
  }

  if (provider === "direct-pumpswap" || provider === "direct-auto") {
    const pumpSwapModule = loadPumpSwapSdk();
    getOnlinePumpAmmSdk(connection, pumpSwapModule);
  }

  const results = await Promise.allSettled(warmups);
  const rejected = results.find((result): result is PromiseRejectedResult => result.status === "rejected");
  if (rejected) {
    throw rejected.reason;
  }
}

async function getLatestBlockhashForDirectSend({
  connection,
  cacheMs = 0,
  forceRefresh = false
}: {
  connection: Connection;
  cacheMs?: number;
  forceRefresh?: boolean;
}): Promise<{ blockhash: LatestBlockhash; cacheHit: boolean }> {
  const clampedCacheMs = blockhashCacheMsFromConfig(cacheMs);
  if (clampedCacheMs <= 0) {
    return {
      blockhash: await connection.getLatestBlockhash("confirmed"),
      cacheHit: false
    };
  }

  const now = Date.now();
  let entry = latestBlockhashByConnection.get(connection);
  if (!forceRefresh && entry?.value && entry.expiresAtMs > now) {
    return {
      blockhash: entry.value,
      cacheHit: true
    };
  }

  if (!entry) {
    entry = {
      value: null,
      expiresAtMs: 0,
      inflight: null
    };
    latestBlockhashByConnection.set(connection, entry);
  }

  entry.inflight ||= Promise.resolve(connection.getLatestBlockhash("confirmed")).then((value) => {
    entry.value = value;
    entry.expiresAtMs = Date.now() + clampedCacheMs;
    return value;
  }).finally(() => {
    entry.inflight = null;
  });

  return {
    blockhash: await entry.inflight,
    cacheHit: false
  };
}

export async function warmDirectSolanaBlockhash({
  connection,
  cacheMs,
  forceRefresh = false
}: {
  connection: Connection;
  cacheMs: number;
  forceRefresh?: boolean;
}): Promise<void> {
  await getLatestBlockhashForDirectSend({ connection, cacheMs, forceRefresh });
}

export interface DirectSolanaBuildRequest {
  provider: DirectTradeExecutionProvider;
  action: "buy" | "sell";
  mint: string;
  amountLamports: bigint;
  amountBasis: TradeAmountBasis;
  slippagePercent: number;
  priorityFeeSol: number;
  walletPublicKey: string;
  platformFee?: PlatformFeeSplit | null;
  metadata?: Record<string, unknown>;
  forceFreshBuyState?: boolean;
  readConnections?: DirectSolanaReadConnection[];
}

export function directAutoProviderOrderForRequest(
  request: Pick<DirectSolanaBuildRequest, "metadata">
): Array<"direct-pumpswap" | "direct-pump"> {
  const hints = [
    request.metadata?.observedPool,
    request.metadata?.pool,
    request.metadata?.observedSource,
    request.metadata?.source,
    request.metadata?.tradeSource
  ]
    .filter((value): value is string | number | boolean => ["string", "number", "boolean"].includes(typeof value))
    .map((value) => String(value).trim().toLowerCase())
    .filter(Boolean);

  if (hints.some((hint) => hint === "pump-amm" || hint === "pumpswap" || hint.includes("pump-amm") || hint.includes("pumpswap"))) {
    return ["direct-pumpswap", "direct-pump"];
  }

  if (hints.some((hint) => hint === "pump" || hint === "pump_fun" || hint === "pump-fun" || hint.includes("bonding"))) {
    return ["direct-pump", "direct-pumpswap"];
  }

  return ["direct-pumpswap", "direct-pump"];
}

export interface SolanaDirectSendConfig {
  gate: DirectExecutionGateConfig;
  simulateBeforeSend?: boolean;
  allowSendAfterSimulationFailure?: boolean;
  skipPreflight?: boolean;
  confirmationMode?: "inline" | "background";
  maxRetries?: number;
  blockhashCacheMs?: number;
  sendConnections?: DirectSolanaSendConnection[];
  nowMs?: () => number;
  onStage?: (stage: DirectSolanaSendStage, details: DirectSolanaSendStageDetails) => void;
}

export interface DirectSolanaSendConnection {
  label: string;
  url?: string | null;
  connection: Pick<Connection, "sendRawTransaction"> & Partial<Pick<Connection, "getLatestBlockhash" | "getMultipleAccountsInfo">>;
}

export interface DirectSolanaReadConnection {
  label: string;
  connection: Pick<Connection, "getMultipleAccountsInfo">;
}

export interface DirectBuildTimingRecord {
  stage: string;
  atMs: number;
  durationMs?: number;
  status?: string;
  [key: string]: unknown;
}

export type DirectSolanaSendStage =
  | "transaction_build_started"
  | "blockhash_started"
  | "blockhash_received"
  | "signing_started"
  | "signing_finished"
  | "transaction_built"
  | "simulation_started"
  | "simulation_finished"
  | "raw_send_started"
  | "signature_returned"
  | "raw_send_failed"
  | "confirmation_started"
  | "confirmation_finished";

export interface DirectSolanaSendStageDetails {
  atMs: number;
  blockhash?: string;
  lastValidBlockHeight?: number;
  unitsConsumed?: number | null;
  signature?: string;
  txBytes?: number;
  instructionCount?: number;
  status?: string;
  errorText?: string;
  rpcCount?: number;
  sendRpcLabel?: string;
  sendRpcUrl?: string | null;
  sendRpcErrors?: Array<{ label: string; errorText: string }>;
}

interface DirectSolanaSendStageRecord extends DirectSolanaSendStageDetails {
  stage: DirectSolanaSendStage;
}

function uniqueSendRpcUrls(primaryUrl: string, urls: string[]): string[] {
  const seen = new Set([primaryUrl.trim()]);
  return urls
    .map((url) => url.trim())
    .filter((url) => {
      if (!url || seen.has(url)) {
        return false;
      }
      seen.add(url);
      return true;
    });
}

export function buildDirectSolanaSendConnections({
  primaryConnection,
  primaryUrl,
  urls,
  jitoUrls = [],
  jitoAuthUuid
}: {
  primaryConnection: Connection;
  primaryUrl: string;
  urls: string[];
  jitoUrls?: string[];
  jitoAuthUuid?: string;
}): DirectSolanaSendConnection[] {
  return [
    {
      label: "primary",
      url: primaryUrl,
      connection: primaryConnection
    },
    ...uniqueSendRpcUrls(primaryUrl, urls).map((url, index) => ({
      label: `fanout-${index + 1}`,
      url,
      connection: new Connection(url, "confirmed")
    })),
    ...uniqueSendRpcUrls(primaryUrl, jitoUrls).map((url, index) => ({
      label: `jito-${index + 1}`,
      url: jitoTransactionUrl(url),
      connection: createJitoBlockEngineSendConnection({
        url,
        authUuid: jitoAuthUuid
      })
    }))
  ];
}

function jitoTransactionUrl(url: string): string {
  const trimmed = url.trim().replace(/\/+$/, "");
  return trimmed.endsWith("/api/v1/transactions") ? trimmed : `${trimmed}/api/v1/transactions`;
}

function createJitoBlockEngineSendConnection({
  url,
  authUuid,
  timeoutMs = 1_500
}: {
  url: string;
  authUuid?: string;
  timeoutMs?: number;
}): Pick<Connection, "sendRawTransaction"> {
  const transactionUrl = jitoTransactionUrl(url);

  return {
    async sendRawTransaction(serializedTransaction, options = {}) {
      const encodedTransaction = Buffer.from(serializedTransaction).toString("base64");
      const headers: Record<string, string> = {
        "Content-Type": "application/json"
      };
      if (authUuid) {
        headers["x-jito-auth"] = authUuid;
      }

      const response = await fetch(transactionUrl, {
        method: "POST",
        headers,
        body: JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          method: "sendTransaction",
          params: [
            encodedTransaction,
            {
              encoding: "base64",
              ...(options.skipPreflight !== undefined ? { skipPreflight: options.skipPreflight } : {}),
              ...(options.maxRetries !== undefined ? { maxRetries: options.maxRetries } : {})
            }
          ]
        }),
        signal: AbortSignal.timeout(timeoutMs)
      });
      const text = await response.text();
      let body: unknown = null;
      try {
        body = text ? JSON.parse(text) : null;
      } catch {
        // Keep the raw text for the error below.
      }

      if (!response.ok) {
        throw new Error(`Jito sendTransaction HTTP ${response.status}: ${text.slice(0, 500)}`);
      }

      const record = body && typeof body === "object" ? body as { result?: unknown; error?: unknown } : null;
      if (record?.error) {
        throw new Error(`Jito sendTransaction error: ${JSON.stringify(record.error).slice(0, 500)}`);
      }

      if (typeof record?.result !== "string" || !record.result) {
        throw new Error(`Jito sendTransaction did not return a signature: ${text.slice(0, 500)}`);
      }

      return record.result;
    }
  };
}

function createBuildTimingTracker(nowMs = Date.now) {
  const startedAtMs = nowMs();
  let previousAtMs = startedAtMs;
  const records: DirectBuildTimingRecord[] = [{ stage: "build_started", atMs: startedAtMs }];

  return {
    mark(stage: string, details: Record<string, unknown> = {}) {
      const atMs = nowMs();
      records.push({
        stage,
        atMs,
        durationMs: Math.max(0, Math.round(atMs - previousAtMs)),
        ...details
      });
      previousAtMs = atMs;
    },
    metadata() {
      const finishedAtMs = records[records.length - 1]?.atMs ?? nowMs();
      return {
        stages: records.map((record) => ({ ...record })),
        totalMs: Math.max(0, Math.round(finishedAtMs - startedAtMs))
      };
    }
  };
}

function publicKey(value: string, label: string): PublicKey {
  try {
    return new PublicKey(value);
  } catch {
    throw new Error(`invalid ${label}: ${value}`);
  }
}

function solToMicroLamportsPerComputeUnit(priorityFeeSol: number): number | null {
  if (!Number.isFinite(priorityFeeSol) || priorityFeeSol <= 0) {
    return null;
  }

  const priorityLamports = Math.max(1, Math.floor(priorityFeeSol * Number(LAMPORTS_PER_SOL)));
  return Math.max(1, Math.floor((priorityLamports * 1_000_000) / DEFAULT_COMPUTE_UNIT_LIMIT));
}

function computeBudgetInstructions(priorityFeeSol: number): TransactionInstruction[] {
  const microLamports = solToMicroLamportsPerComputeUnit(priorityFeeSol);

  return [
    ComputeBudgetProgram.setComputeUnitLimit({ units: DEFAULT_COMPUTE_UNIT_LIMIT }),
    ...(microLamports ? [ComputeBudgetProgram.setComputeUnitPrice({ microLamports })] : [])
  ];
}

function platformFeeInstruction({
  split,
  fromPubkey
}: {
  split?: PlatformFeeSplit | null;
  fromPubkey: PublicKey;
}): TransactionInstruction[] {
  if (!split?.enabled || split.feeLamports <= 0n || !split.treasury || split.blockedReason) {
    return [];
  }

  return [
    SystemProgram.transfer({
      fromPubkey,
      toPubkey: publicKey(split.treasury, "PLATFORM_FEE_TREASURY"),
      lamports: Number(split.feeLamports)
    })
  ];
}

function bn(value: bigint): BN {
  return new BN(value.toString());
}

function bnFromInteger(value: string | bigint): BN {
  return new BN(value.toString());
}

function booleanValue(value: boolean | null | undefined, fallback = false): boolean {
  return typeof value === "boolean" ? value : fallback;
}

export function primeDirectPumpFastBuyState(input: DirectPumpFastBuyStateInput): boolean {
  try {
    const mint = publicKey(input.mint, "mint");
    const creator = publicKey(input.creator, "creator");
    const tokenProgram = publicKey(input.tokenProgram, "tokenProgram");
    const quoteMint = input.quoteMint
      ? publicKey(input.quoteMint, "quoteMint")
      : NATIVE_MINT;

    if (!tokenProgram.equals(TOKEN_PROGRAM_ID) && !tokenProgram.equals(TOKEN_2022_PROGRAM_ID)) {
      return false;
    }

    const now = input.observedAtMs ?? Date.now();
    const cacheMs = Math.max(0, input.cacheMs ?? PUMP_FAST_BUY_STATE_CACHE_MS);
    const existing = directPumpFastBuyStateByMint.get(mint.toBase58());
    const verifiedByInput = input.creatorVerified === true;
    const verifiedByExisting = existing?.creatorVerified === true;
    const cachedCreator = !verifiedByInput && verifiedByExisting ? existing.creator : creator;
    const cachedCreatorVault = loadPumpSdk().creatorVaultPda(cachedCreator);
    const cachedTokenProgram = !verifiedByInput && verifiedByExisting ? existing.tokenProgram : tokenProgram;
    directPumpFastBuyStateByMint.set(mint.toBase58(), {
      mint,
      creator: cachedCreator,
      creatorVault: cachedCreatorVault,
      creatorVerified: verifiedByInput || verifiedByExisting,
      creatorVerifiedAtMs: verifiedByInput ? now : existing?.creatorVerifiedAtMs ?? null,
      creatorSource: verifiedByInput ? input.source || "chain-verified" : existing?.creatorSource ?? null,
      tokenProgram: cachedTokenProgram,
      virtualTokenReserves: bnFromInteger(input.virtualTokenReserves),
      virtualQuoteReserves: bnFromInteger(input.virtualQuoteReserves),
      realTokenReserves: bnFromInteger(input.realTokenReserves),
      realQuoteReserves: bnFromInteger(input.realQuoteReserves),
      tokenTotalSupply: bnFromInteger(input.tokenTotalSupply),
      complete: booleanValue(input.complete),
      isMayhemMode: booleanValue(input.isMayhemMode),
      isCashbackCoin: booleanValue(input.isCashbackCoin),
      quoteMint,
      source: input.source || null,
      observedAtMs: now,
      expiresAtMs: now + cacheMs
    });
    return true;
  } catch {
    return false;
  }
}

export function refreshDirectPumpFastBuyStateReserves(input: DirectPumpFastBuyStateReserveUpdate): boolean {
  const cached = directPumpFastBuyStateByMint.get(input.mint);
  if (!cached) {
    return false;
  }

  try {
    const virtualTokenReserves = bnFromInteger(input.virtualTokenReserves);
    const virtualQuoteReserves = bnFromInteger(input.virtualQuoteReserves);
    const realTokenOffset = cached.virtualTokenReserves.sub(cached.realTokenReserves);
    const realQuoteOffset = cached.virtualQuoteReserves.sub(cached.realQuoteReserves);
    const now = input.observedAtMs ?? Date.now();
    const cacheMs = Math.max(0, input.cacheMs ?? PUMP_FAST_BUY_STATE_CACHE_MS);
    directPumpFastBuyStateByMint.set(input.mint, {
      ...cached,
      virtualTokenReserves,
      virtualQuoteReserves,
      realTokenReserves: BN.max(new BN(0), virtualTokenReserves.sub(realTokenOffset)),
      realQuoteReserves: BN.max(new BN(0), virtualQuoteReserves.sub(realQuoteOffset)),
      source: input.source || cached.source,
      observedAtMs: now,
      expiresAtMs: now + cacheMs
    });
    return true;
  } catch {
    return false;
  }
}

function cachedDirectPumpFastBuyState(mint: PublicKey): DirectPumpFastBuyStateCacheEntry | null {
  const cached = directPumpFastBuyStateByMint.get(mint.toBase58());
  if (!cached || !cached.creatorVerified || cached.expiresAtMs <= Date.now()) {
    return null;
  }

  if (!cached.creatorVault.equals(loadPumpSdk().creatorVaultPda(cached.creator))) {
    return null;
  }

  if (cached.tokenProgram.equals(TOKEN_PROGRAM_ID)) {
    return cached;
  }

  const creatorVerifiedAgeMs = cached.creatorVerifiedAtMs === null
    ? Number.POSITIVE_INFINITY
    : Math.max(0, Date.now() - cached.creatorVerifiedAtMs);
  return creatorVerifiedAgeMs <= PUMP_TOKEN_2022_VERIFIED_CREATOR_CACHE_MS ? cached : null;
}

function cachedMintTokenProgram(connection: Connection, mint: PublicKey): PublicKey | null {
  const cached = mintTokenProgramByConnection.get(connection)?.get(mint.toBase58());
  return cached && cached.expiresAtMs > Date.now() ? cached.value : null;
}

function cacheMintTokenProgram({
  connection,
  mint,
  tokenProgram,
  cacheMs = MINT_TOKEN_PROGRAM_CACHE_MS
}: {
  connection: Connection;
  mint: PublicKey;
  tokenProgram: PublicKey;
  cacheMs?: number;
}): void {
  if (cacheMs <= 0) {
    return;
  }

  let cache = mintTokenProgramByConnection.get(connection);
  cache ||= new Map();
  cache.set(mint.toBase58(), {
    value: tokenProgram,
    expiresAtMs: Date.now() + cacheMs
  });
  mintTokenProgramByConnection.set(connection, cache);
}

function mintTokenProgramFromAccountInfo({
  mint,
  mintInfo
}: {
  mint: PublicKey;
  mintInfo: Awaited<ReturnType<Connection["getAccountInfo"]>>;
}): PublicKey {
  const mintAddress = mint.toBase58();
  if (!mintInfo) {
    throw new Error(`mint account not found: ${mintAddress}`);
  }

  if (mintInfo.owner.equals(TOKEN_PROGRAM_ID) || mintInfo.owner.equals(TOKEN_2022_PROGRAM_ID)) {
    return mintInfo.owner;
  }

  throw new Error(`mint account is not owned by SPL Token or Token-2022: ${mintAddress}`);
}

export async function resolveMintTokenProgram({
  connection,
  mint,
  cacheMs = MINT_TOKEN_PROGRAM_CACHE_MS,
  commitment = "confirmed"
}: {
  connection: Connection;
  mint: PublicKey;
  cacheMs?: number;
  commitment?: "processed" | "confirmed" | "finalized";
}): Promise<PublicKey> {
  const cached = cachedMintTokenProgram(connection, mint);

  if (cached) {
    return cached;
  }
  const mintInfo = await connection.getAccountInfo(mint, commitment);
  const tokenProgram = mintTokenProgramFromAccountInfo({ mint, mintInfo });

  cacheMintTokenProgram({ connection, mint, tokenProgram, cacheMs });
  return tokenProgram;
}

async function tokenBalanceRaw({
  connection,
  mint,
  user,
  tokenProgram = TOKEN_PROGRAM_ID
}: {
  connection: Connection;
  mint: PublicKey;
  user: PublicKey;
  tokenProgram?: PublicKey;
}): Promise<BN> {
  const ata = getAssociatedTokenAddressSync(mint, user, true, tokenProgram);
  const balance = await connection.getTokenAccountBalance(ata);
  return new BN(balance.value.amount);
}

function percentOfRawBalance(balance: BN, percentBasis: bigint): BN {
  return balance.mul(new BN(percentBasis.toString())).div(new BN(10_000));
}

function amountBasisLabel(amountBasis: TradeAmountBasis): string {
  return amountBasis === "percent" ? "percent" : amountBasis === "token" ? "token" : "sol";
}

function platformFeeResult(split?: PlatformFeeSplit | null) {
  return split ? {
    enabled: split.enabled,
    bps: split.bps,
    treasury: split.treasury,
    budgetLamports: split.budgetLamports,
    feeLamports: split.feeLamports,
    tradeLamports: split.tradeLamports
  } : null;
}

function platformFeeForProceeds(split: PlatformFeeSplit | null | undefined, proceedsLamports: bigint): PlatformFeeSplit | null | undefined {
  if (!split?.enabled || split.blockedReason || !split.treasury) {
    return split;
  }

  const budgetLamports = proceedsLamports < 0n ? 0n : proceedsLamports;
  const feeLamports = (budgetLamports * BigInt(split.bps)) / 10_000n;
  return {
    ...split,
    budgetLamports,
    feeLamports,
    tradeLamports: budgetLamports - feeLamports
  };
}

export async function fetchDirectPumpBuyState({
  connection,
  pumpModule,
  mint,
  user
}: {
  connection: Connection;
  pumpModule: PumpSdkModule;
  mint: PublicKey;
  user: PublicKey;
}) {
  const legacyAssociatedUser = getAssociatedTokenAddressSync(mint, user, true, TOKEN_PROGRAM_ID);
  const token2022AssociatedUser = getAssociatedTokenAddressSync(mint, user, true, TOKEN_2022_PROGRAM_ID);
  const [mintInfo, bondingCurveAccountInfo, legacyAssociatedUserAccountInfo, token2022AssociatedUserAccountInfo] = await connection.getMultipleAccountsInfo([
    mint,
    pumpModule.bondingCurvePda(mint),
    legacyAssociatedUser,
    token2022AssociatedUser
  ]);
  const tokenProgram = mintTokenProgramFromAccountInfo({ mint, mintInfo });

  if (!bondingCurveAccountInfo) {
    throw new Error(`Bonding curve account not found for mint: ${mint.toBase58()}`);
  }
  cacheMintTokenProgram({ connection, mint, tokenProgram });

  return {
    tokenProgram,
    bondingCurveAccountInfo,
    bondingCurve: pumpModule.PUMP_SDK.decodeBondingCurve(bondingCurveAccountInfo),
    associatedUserAccountInfo: tokenProgram.equals(TOKEN_2022_PROGRAM_ID)
      ? token2022AssociatedUserAccountInfo
    : legacyAssociatedUserAccountInfo
  };
}

async function fetchDirectPumpBuyStateFromFastestConnection({
  connection,
  readConnections = [],
  pumpModule,
  mint,
  user
}: {
  connection: Connection;
  readConnections?: DirectSolanaReadConnection[];
  pumpModule: PumpSdkModule;
  mint: PublicKey;
  user: PublicKey;
}) {
  const candidates = readConnections.length > 0
    ? readConnections
    : [{ label: "primary", connection }];
  let pending = candidates.length;
  const errors: string[] = [];

  return new Promise<Awaited<ReturnType<typeof fetchDirectPumpBuyState>> & {
    source: string;
    observedAtMs: number;
  }>((resolve, reject) => {
    for (const candidate of candidates) {
      fetchDirectPumpBuyState({
        connection: candidate.connection as Connection,
        pumpModule,
        mint,
        user
      }).then((state) => {
        resolve({
          ...state,
          source: `rpc:${candidate.label}`,
          observedAtMs: Date.now()
        });
      }).catch((error) => {
        errors.push(`${candidate.label}: ${error instanceof Error ? error.message : String(error)}`);
        pending -= 1;
        if (pending <= 0) {
          reject(new Error(`all direct Pump buy-state RPCs failed: ${errors.join("; ")}`));
        }
      });
    }
  });
}

export async function fetchDirectPumpFastBuyStateFromChain({
  connection,
  mint,
  commitment = "processed"
}: {
  connection: Pick<Connection, "getMultipleAccountsInfo">;
  mint: PublicKey;
  commitment?: "processed" | "confirmed" | "finalized";
}): Promise<DirectPumpFastBuyStateChainSnapshot> {
  const pumpModule = loadPumpSdk();
  const [mintInfo, bondingCurveAccountInfo] = await connection.getMultipleAccountsInfo([
    mint,
    pumpModule.bondingCurvePda(mint)
  ], commitment);
  const tokenProgram = mintTokenProgramFromAccountInfo({ mint, mintInfo });

  if (!bondingCurveAccountInfo) {
    throw new Error(`Bonding curve account not found for mint: ${mint.toBase58()}`);
  }

  const bondingCurve = pumpModule.PUMP_SDK.decodeBondingCurve(bondingCurveAccountInfo);
  return {
    mint: mint.toBase58(),
    creator: bondingCurve.creator.toBase58(),
    creatorVerified: true,
    tokenProgram: tokenProgram.toBase58(),
    virtualTokenReserves: bondingCurve.virtualTokenReserves.toString(),
    virtualQuoteReserves: bondingCurve.virtualQuoteReserves.toString(),
    realTokenReserves: bondingCurve.realTokenReserves.toString(),
    realQuoteReserves: bondingCurve.realQuoteReserves.toString(),
    tokenTotalSupply: bondingCurve.tokenTotalSupply.toString(),
    complete: bondingCurve.complete,
    isMayhemMode: bondingCurve.isMayhemMode,
    isCashbackCoin: bondingCurve.isCashbackCoin,
    quoteMint: bondingCurve.quoteMint.toBase58(),
    source: "rpc-bonding-curve-prefetch"
  };
}

export function prefetchDirectPumpFastBuyStateFromChain({
  connection,
  mint,
  commitment = "processed",
  source = "rpc-bonding-curve-prefetch",
  cacheMs
}: {
  connection: Pick<Connection, "getMultipleAccountsInfo">;
  mint: PublicKey;
  commitment?: "processed" | "confirmed" | "finalized";
  source?: string;
  cacheMs?: number;
}): Promise<boolean> {
  const key = mint.toBase58();
  const existing = directPumpFastBuyStateInflightByMint.get(key);
  if (existing) {
    return existing;
  }

  const inflight = fetchDirectPumpFastBuyStateFromChain({
    connection,
    mint,
    commitment
  }).then((snapshot) => primeDirectPumpFastBuyState({
    ...snapshot,
    source,
    ...(cacheMs !== undefined ? { cacheMs } : {})
  })).finally(() => {
    directPumpFastBuyStateInflightByMint.delete(key);
  });

  directPumpFastBuyStateInflightByMint.set(key, inflight);
  return inflight;
}

async function awaitDirectPumpFastBuyStateInflight(mint: PublicKey): Promise<boolean> {
  const inflight = directPumpFastBuyStateInflightByMint.get(mint.toBase58());
  return inflight ? inflight.catch(() => false) : false;
}

function directPumpFastBuyState({
  connection,
  mint,
  pumpModule,
  maxAgeMs = null,
  sourceIncludes = null
}: {
  connection: Connection;
  mint: PublicKey;
  pumpModule: PumpSdkModule;
  maxAgeMs?: number | null;
  sourceIncludes?: string | null;
}) {
  const cached = cachedDirectPumpFastBuyState(mint);
  if (!cached) {
    return null;
  }

  const cacheAgeMs = Math.max(0, Date.now() - cached.observedAtMs);
  if (typeof maxAgeMs === "number" && cacheAgeMs > maxAgeMs) {
    return null;
  }

  if (sourceIncludes && !cached.source?.includes(sourceIncludes)) {
    return null;
  }

  cacheMintTokenProgram({
    connection,
    mint,
    tokenProgram: cached.tokenProgram
  });

  return {
    tokenProgram: cached.tokenProgram,
    bondingCurveAccountInfo: {
      data: Buffer.alloc(0),
      executable: false,
      lamports: 0,
      owner: pumpModule.PUMP_PROGRAM_ID,
      rentEpoch: 0
    },
    bondingCurve: {
      virtualTokenReserves: cached.virtualTokenReserves,
      virtualQuoteReserves: cached.virtualQuoteReserves,
      realTokenReserves: cached.realTokenReserves,
      realQuoteReserves: cached.realQuoteReserves,
      tokenTotalSupply: cached.tokenTotalSupply,
      complete: cached.complete,
      creator: cached.creator,
      isMayhemMode: cached.isMayhemMode,
      isCashbackCoin: cached.isCashbackCoin,
      quoteMint: cached.quoteMint
    },
    associatedUserAccountInfo: null,
    creatorVerified: cached.creatorVerified,
    creatorVerifiedAtMs: cached.creatorVerifiedAtMs,
    creatorVerifiedAgeMs: cached.creatorVerifiedAtMs === null ? null : Math.max(0, Date.now() - cached.creatorVerifiedAtMs),
    creatorSource: cached.creatorSource,
    source: cached.source,
    observedAtMs: cached.observedAtMs
  };
}

function directSolanaTimingMetadata(stages: DirectSolanaSendStageRecord[]): DirectExecutionTimingMetadata {
  const atMs = Object.fromEntries(stages.map((stage) => [stage.stage, stage.atMs])) as Record<string, number>;
  const duration = (from: DirectSolanaSendStage, to: DirectSolanaSendStage): number | null => {
    const start = atMs[from];
    const end = atMs[to];
    return typeof start === "number" && typeof end === "number" ? Math.max(0, Math.round(end - start)) : null;
  };
  const stageDetail = <T extends keyof DirectSolanaSendStageDetails>(
    stage: DirectSolanaSendStage,
    key: T
  ): DirectSolanaSendStageDetails[T] | null => {
    const record = [...stages].reverse().find((candidate) => candidate.stage === stage);
    return record && record[key] !== undefined ? record[key] : null;
  };

  return {
    stages: stages.map((stage) => ({ ...stage })),
    atMs,
    startedAtMs: atMs.transaction_build_started ?? null,
    finishedAtMs: atMs.confirmation_finished ?? atMs.raw_send_failed ?? atMs.signature_returned ?? null,
    blockhashStartedAtMs: atMs.blockhash_started ?? null,
    blockhashFinishedAtMs: atMs.blockhash_received ?? null,
    signStartedAtMs: atMs.signing_started ?? null,
    signFinishedAtMs: atMs.signing_finished ?? null,
    simulationStartedAtMs: atMs.simulation_started ?? null,
    simulationFinishedAtMs: atMs.simulation_finished ?? null,
    rawSendStartedAtMs: atMs.raw_send_started ?? null,
    rawSendFinishedAtMs: atMs.signature_returned ?? atMs.raw_send_failed ?? null,
    signatureReturnedAtMs: atMs.signature_returned ?? null,
    confirmationStartedAtMs: atMs.confirmation_started ?? null,
    confirmationFinishedAtMs: atMs.confirmation_finished ?? null,
    totalMs: duration("transaction_build_started", "confirmation_finished")
      ?? duration("transaction_build_started", "raw_send_failed")
      ?? duration("transaction_build_started", "signature_returned"),
    timeToSignatureMs: duration("transaction_build_started", "signature_returned"),
    signatureToConfirmationMs: duration("signature_returned", "confirmation_finished"),
    blockhashMs: duration("blockhash_started", "blockhash_received"),
    signingMs: duration("signing_started", "signing_finished"),
    simulationMs: duration("simulation_started", "simulation_finished"),
    rawSendMs: duration("raw_send_started", "signature_returned") ?? duration("raw_send_started", "raw_send_failed"),
    confirmationMs: duration("confirmation_started", "confirmation_finished"),
    timeToConfirmationMs: duration("transaction_build_started", "confirmation_finished"),
    simulateBeforeSend: null,
    skipPreflight: null,
    maxRetries: null,
    blockhashCacheMs: null,
    rawSendRpcCount: stageDetail("raw_send_started", "rpcCount") ?? null,
    rawSendWinner: stageDetail("signature_returned", "sendRpcLabel") ?? null,
    rawSendErrors: stageDetail("signature_returned", "sendRpcErrors") ?? stageDetail("raw_send_failed", "sendRpcErrors") ?? null,
    instructionCount: stageDetail("transaction_build_started", "instructionCount") ?? null,
    txBytes: stageDetail("signature_returned", "txBytes") ?? stageDetail("raw_send_failed", "txBytes") ?? null,
    unitsConsumed: stageDetail("simulation_finished", "unitsConsumed") ?? null,
    blockhash: stageDetail("blockhash_received", "blockhash") ?? null,
    lastValidBlockHeight: stageDetail("blockhash_received", "lastValidBlockHeight") ?? null
  };
}

export async function buildDirectSolanaPayload({
  connection,
  request
}: {
  connection: Connection;
  request: DirectSolanaBuildRequest;
}): Promise<TradeExecutionResult | DirectTransactionPayload> {
  if (request.provider === "direct-auto") {
    const providerOrder = directAutoProviderOrderForRequest(request);
    const attemptBuilders = {
      "direct-pumpswap": () => buildDirectPumpSwapSolanaPayload({
        connection,
        request: {
          ...request,
          provider: "direct-pumpswap" as const,
          metadata: {
            ...(request.metadata || {}),
            requestedProvider: "direct-auto",
            autoRouteAttempt: "direct-pumpswap",
            autoRoutePreference: providerOrder
          }
        }
      }),
      "direct-pump": () => buildDirectPumpSolanaPayload({
        connection,
        request: {
          ...request,
          provider: "direct-pump" as const,
          metadata: {
            ...(request.metadata || {}),
            requestedProvider: "direct-auto",
            autoRouteAttempt: "direct-pump",
            autoRoutePreference: providerOrder
          }
        }
      })
    };

    return buildDirectAutoTransactionPayload({
      metadata: request.metadata,
      platformFee: platformFeeResult(request.platformFee),
      attempts: providerOrder.map((provider) => ({
        provider,
        build: attemptBuilders[provider]
      }))
    });
  }

  if (request.provider === "direct-pumpswap") {
    return buildDirectPumpSwapSolanaPayload({
      connection,
      request: { ...request, provider: "direct-pumpswap" }
    });
  }

  return buildDirectPumpSolanaPayload({
    connection,
    request: { ...request, provider: "direct-pump" }
  });
}

async function buildDirectPumpSolanaPayload({
  connection,
  request
}: {
  connection: Connection;
  request: DirectSolanaBuildRequest & { provider: "direct-pump" };
}): Promise<TradeExecutionResult | DirectTransactionPayload> {
  const buildTiming = createBuildTimingTracker();
  const mint = publicKey(request.mint, "mint");
  const user = publicKey(request.walletPublicKey, "walletPublicKey");
  buildTiming.mark("keys_ready");
  const pumpModule = loadPumpSdk();
  const {
    getBuyTokenAmountFromSolAmount,
    getSellSolAmountFromTokenAmount,
    PUMP_SDK
  } = pumpModule;
  const sdk = getOnlinePumpSdk(connection, pumpModule);
  buildTiming.mark("sdk_ready");
  const route = buildDirectRouteMetadata({
    provider: "direct-pump",
    mint: request.mint,
    walletPublicKey: request.walletPublicKey,
    priorityFeeSol: request.priorityFeeSol,
    slippagePercent: request.slippagePercent,
    amount: request.amountLamports.toString(),
    amountBasis: request.amountBasis
  });
  const metadataWithBuildTiming = (extra: Record<string, unknown> = {}) => ({
    ...(request.metadata || {}),
    ...extra,
    directBuildTiming: buildTiming.metadata()
  });

  try {
    const instructions = [...computeBudgetInstructions(request.priorityFeeSol)];
    let appliedPlatformFee = request.platformFee;
    let tokenProgram: PublicKey;
    let global: PumpGlobal;
    let feeConfig: PumpFeeConfig | null;
    buildTiming.mark("compute_budget_ready", { instructionCount: instructions.length });

    if (request.action === "buy") {
      instructions.push(...platformFeeInstruction({
        split: request.platformFee,
        fromPubkey: user
      }));
      buildTiming.mark("platform_fee_ready", { instructionCount: instructions.length });

      if (request.amountBasis !== "sol") {
        buildTiming.mark("skipped", { status: "wrong_amount_basis" });
        return tradeExecutionSkippedResult({
          provider: "direct-pump",
          route: route.route,
          reason: `direct Pump buy requires SOL amount basis; got ${amountBasisLabel(request.amountBasis)}`,
          platformFee: platformFeeResult(request.platformFee),
          metadata: metadataWithBuildTiming({ route })
        });
      }

      let fastBuyState = request.forceFreshBuyState
        ? directPumpFastBuyState({
            connection,
            mint,
            pumpModule,
            maxAgeMs: OBSERVED_BUY_FAST_STATE_MAX_AGE_MS,
            sourceIncludes: "observed-buy-prefetch"
          })
        : directPumpFastBuyState({ connection, mint, pumpModule });
      if (!fastBuyState && request.forceFreshBuyState && await awaitDirectPumpFastBuyStateInflight(mint)) {
        fastBuyState = directPumpFastBuyState({
          connection,
          mint,
          pumpModule,
          maxAgeMs: OBSERVED_BUY_FAST_STATE_MAX_AGE_MS,
          sourceIncludes: "observed-buy-prefetch"
        });
      }
      const [buyState, cachedGlobal, cachedFeeConfig] = await Promise.all([
        fastBuyState ?? fetchDirectPumpBuyStateFromFastestConnection({
          connection,
          readConnections: request.readConnections,
          pumpModule,
          mint,
          user
        }),
        fetchCachedPumpGlobal(sdk),
        fetchCachedPumpFeeConfig(sdk)
      ]);
      tokenProgram = buyState.tokenProgram;
      global = cachedGlobal;
      feeConfig = cachedFeeConfig;
      buildTiming.mark("buy_accounts_ready", {
        tokenProgram: tokenProgram.toBase58(),
        source: fastBuyState ? "cache" : "rpc",
        forceFreshBuyState: request.forceFreshBuyState === true,
        cachedStateSource: "source" in buyState ? buyState.source : null,
        cachedStateAgeMs: "observedAtMs" in buyState ? Math.max(0, Date.now() - buyState.observedAtMs) : null,
        creatorSource: "creatorSource" in buyState ? buyState.creatorSource : null,
        creatorVerifiedAgeMs: "creatorVerifiedAgeMs" in buyState ? buyState.creatorVerifiedAgeMs : null,
        creatorVerified: "creatorVerified" in buyState ? buyState.creatorVerified : true,
        feeConfigLoaded: Boolean(feeConfig),
        associatedUserAccountExists: Boolean(buyState.associatedUserAccountInfo),
        bondingCurveComplete: Boolean(buyState.bondingCurve.complete)
      });
      if (buyState.bondingCurve.complete) {
        buildTiming.mark("skipped", { status: "bonding_curve_complete" });
        return tradeExecutionSkippedResult({
          provider: "direct-pump",
          route: route.route,
          reason: "direct Pump bonding curve is complete/migrated; use direct-pumpswap",
          platformFee: platformFeeResult(request.platformFee),
          metadata: metadataWithBuildTiming({ route })
        });
      }

      const sdkQuoteLamports = maxQuoteLamportsForSlippageCap(request.amountLamports, request.slippagePercent);
      if (sdkQuoteLamports <= 0n) {
        buildTiming.mark("skipped", { status: "amount_too_small" });
        return tradeExecutionSkippedResult({
          provider: "direct-pump",
          route: route.route,
          reason: "direct Pump buy amount is too small after slippage cap",
          platformFee: platformFeeResult(request.platformFee),
          metadata: metadataWithBuildTiming({ route })
        });
      }

      const solAmount = bn(sdkQuoteLamports);
      const amount = getBuyTokenAmountFromSolAmount({
        global,
        feeConfig,
        mintSupply: buyState.bondingCurve.tokenTotalSupply,
        bondingCurve: buyState.bondingCurve,
        amount: solAmount,
        quoteMint: NATIVE_MINT
      });
      buildTiming.mark("quote_ready", {
        sdkQuoteLamports: sdkQuoteLamports.toString(),
        tokenAmount: amount.toString()
      });
      const buyInstructions = tokenProgram.equals(TOKEN_2022_PROGRAM_ID)
        ? PUMP_SDK.buyV2Instructions({
            global,
            bondingCurveAccountInfo: buyState.bondingCurveAccountInfo,
            bondingCurve: buyState.bondingCurve,
            associatedUserAccountInfo: buyState.associatedUserAccountInfo,
            mint,
            user,
            amount,
            quoteAmount: solAmount,
            slippage: request.slippagePercent,
            tokenProgram,
            quoteTokenProgram: TOKEN_PROGRAM_ID
          })
        : PUMP_SDK.buyInstructions({
            global,
            bondingCurveAccountInfo: buyState.bondingCurveAccountInfo,
            bondingCurve: buyState.bondingCurve,
            associatedUserAccountInfo: buyState.associatedUserAccountInfo,
            mint,
            user,
            amount,
            solAmount,
            slippage: request.slippagePercent,
            tokenProgram
      });
      instructions.push(...await buyInstructions);
      buildTiming.mark("instructions_ready", { instructionCount: instructions.length });
    } else {
      const [resolvedTokenProgram, cachedGlobal, cachedFeeConfig] = await Promise.all([
        resolveMintTokenProgram({ connection, mint }),
        fetchCachedPumpGlobal(sdk),
        fetchCachedPumpFeeConfig(sdk)
      ]);
      tokenProgram = resolvedTokenProgram;
      global = cachedGlobal;
      feeConfig = cachedFeeConfig;
      buildTiming.mark("config_ready", {
        tokenProgram: tokenProgram.toBase58(),
        feeConfigLoaded: Boolean(feeConfig)
      });
      const sellState = await sdk.fetchSellState(mint, user, tokenProgram);
      buildTiming.mark("sell_state_ready", {
        bondingCurveComplete: Boolean(sellState.bondingCurve.complete)
      });
      if (sellState.bondingCurve.complete) {
        buildTiming.mark("skipped", { status: "bonding_curve_complete" });
        return tradeExecutionSkippedResult({
          provider: "direct-pump",
          route: route.route,
          reason: "direct Pump bonding curve is complete/migrated; use direct-pumpswap",
          platformFee: platformFeeResult(request.platformFee),
          metadata: metadataWithBuildTiming({ route })
        });
      }

      const amount = request.amountBasis === "percent"
        ? percentOfRawBalance(
            await tokenBalanceRaw({ connection, mint, user, tokenProgram }),
            request.amountLamports
          )
        : bn(request.amountLamports);
      const solAmount = getSellSolAmountFromTokenAmount({
        global,
        feeConfig,
        mintSupply: sellState.bondingCurve.tokenTotalSupply,
        bondingCurve: sellState.bondingCurve,
        amount
      });
      appliedPlatformFee = platformFeeForProceeds(request.platformFee, BigInt(solAmount.toString()));

      const sellInstructions = tokenProgram.equals(TOKEN_2022_PROGRAM_ID)
        ? PUMP_SDK.sellV2Instructions({
            global,
            bondingCurveAccountInfo: sellState.bondingCurveAccountInfo,
            bondingCurve: sellState.bondingCurve,
            mint,
            user,
            amount,
            quoteAmount: solAmount,
            slippage: request.slippagePercent,
            tokenProgram,
            quoteTokenProgram: TOKEN_PROGRAM_ID
          })
        : PUMP_SDK.sellInstructions({
            global,
            bondingCurveAccountInfo: sellState.bondingCurveAccountInfo,
            bondingCurve: sellState.bondingCurve,
            mint,
            user,
            amount,
            solAmount,
            slippage: request.slippagePercent,
            tokenProgram,
            mayhemMode: sellState.bondingCurve.isMayhemMode,
            cashback: sellState.bondingCurve.isCashbackCoin
          });
      instructions.push(...await sellInstructions);
      instructions.push(...platformFeeInstruction({
        split: appliedPlatformFee,
        fromPubkey: user
      }));
      buildTiming.mark("instructions_ready", { instructionCount: instructions.length });
    }
    buildTiming.mark("build_finished", { instructionCount: instructions.length });

    return {
      provider: "direct-pump",
      route,
      instructions,
      signers: [],
      metadata: metadataWithBuildTiming({
        route,
        ...(request.action === "buy"
          ? {
              maxSpendLamports: request.amountLamports.toString(),
              sdkQuoteLamports: maxQuoteLamportsForSlippageCap(request.amountLamports, request.slippagePercent).toString()
            }
          : {}),
        tokenProgram: tokenProgram.toBase58(),
        ...(request.action === "sell" && appliedPlatformFee?.enabled
          ? {
              platformFeeBasis: "sell_expected_output_lamports"
            }
          : {}),
        instructionCount: instructions.length
      }),
      platformFee: appliedPlatformFee
    };
  } catch (error) {
    buildTiming.mark("build_failed", {
      status: "failed",
      errorText: error instanceof Error ? error.message : String(error)
    });
    return tradeExecutionFailedResult({
      provider: "direct-pump",
      route: route.route,
      errorText: error instanceof Error ? error.message : String(error),
      metadata: metadataWithBuildTiming({ route }),
      platformFee: platformFeeResult(request.platformFee)
    });
  }
}

async function buildDirectPumpSwapSolanaPayload({
  connection,
  request
}: {
  connection: Connection;
  request: DirectSolanaBuildRequest & { provider: "direct-pumpswap" };
}): Promise<TradeExecutionResult | DirectTransactionPayload> {
  const mint = publicKey(request.mint, "mint");
  const user = publicKey(request.walletPublicKey, "walletPublicKey");
  const pumpSwapModule = loadPumpSwapSdk();
  const {
    canonicalPumpPoolPda,
    PUMP_AMM_SDK
  } = pumpSwapModule;
  const pool = canonicalPumpPoolPda(mint, NATIVE_MINT);
  const sdk = getOnlinePumpAmmSdk(connection, pumpSwapModule);
  const route = buildDirectRouteMetadata({
    provider: "direct-pumpswap",
    mint: request.mint,
    walletPublicKey: request.walletPublicKey,
    poolAddress: pool.toBase58(),
    priorityFeeSol: request.priorityFeeSol,
    slippagePercent: request.slippagePercent,
    amount: request.amountLamports.toString(),
    amountBasis: request.amountBasis
  });

  try {
    const state = await sdk.swapSolanaState(pool, user);
    const instructions = [...computeBudgetInstructions(request.priorityFeeSol)];
    let appliedPlatformFee = request.platformFee;

    if (request.action === "buy") {
      instructions.push(...platformFeeInstruction({
        split: request.platformFee,
        fromPubkey: user
      }));

      if (request.amountBasis !== "sol") {
        return tradeExecutionSkippedResult({
          provider: "direct-pumpswap",
          route: route.route,
          reason: `direct PumpSwap buy requires SOL amount basis; got ${amountBasisLabel(request.amountBasis)}`,
          metadata: { route },
          platformFee: platformFeeResult(request.platformFee)
        });
      }

      const sdkQuoteLamports = maxQuoteLamportsForSlippageCap(request.amountLamports, request.slippagePercent);
      if (sdkQuoteLamports <= 0n) {
        return tradeExecutionSkippedResult({
          provider: "direct-pumpswap",
          route: route.route,
          reason: "direct PumpSwap buy amount is too small after slippage cap",
          metadata: { ...(request.metadata || {}), route },
          platformFee: platformFeeResult(request.platformFee)
        });
      }

      instructions.push(...await PUMP_AMM_SDK.buyQuoteInput(state, bn(sdkQuoteLamports), request.slippagePercent));
    } else {
      const amount = request.amountBasis === "percent"
        ? percentOfRawBalance(
            await tokenBalanceRaw({
              connection,
              mint,
              user,
              tokenProgram: state.baseTokenProgram
            }),
            request.amountLamports
        )
        : bn(request.amountLamports);
      const quote = pumpSwapModule.sellBaseInput({
        base: amount,
        slippage: request.slippagePercent,
        baseReserve: state.poolBaseAmount,
        quoteReserve: state.poolQuoteAmount,
        baseMintAccount: state.baseMintAccount,
        baseMint: state.baseMint,
        coinCreator: state.pool.coinCreator,
        creator: state.pool.creator,
        feeConfig: state.feeConfig,
        globalConfig: state.globalConfig
      });
      appliedPlatformFee = platformFeeForProceeds(request.platformFee, BigInt(quote.minQuote.toString()));

      instructions.push(...await PUMP_AMM_SDK.sellBaseInput(state, amount, request.slippagePercent));
      instructions.push(...platformFeeInstruction({
        split: appliedPlatformFee,
        fromPubkey: user
      }));
    }

    return {
      provider: "direct-pumpswap",
      route,
      instructions,
      signers: [],
      metadata: {
        ...(request.metadata || {}),
        route,
        poolAddress: pool.toBase58(),
        ...(request.action === "buy"
          ? {
              maxSpendLamports: request.amountLamports.toString(),
              sdkQuoteLamports: maxQuoteLamportsForSlippageCap(request.amountLamports, request.slippagePercent).toString()
            }
          : {}),
        ...(request.action === "sell" && appliedPlatformFee?.enabled
          ? {
              platformFeeBasis: "sell_min_output_lamports"
            }
          : {}),
        instructionCount: instructions.length
      },
      platformFee: appliedPlatformFee
    };
  } catch (error) {
    return tradeExecutionFailedResult({
      provider: "direct-pumpswap",
      route: route.route,
      errorText: error instanceof Error ? error.message : String(error),
      metadata: { ...(request.metadata || {}), route, poolAddress: pool.toBase58() },
      platformFee: platformFeeResult(request.platformFee)
    });
  }
}

async function transactionForPayload({
  connection,
  payer,
  payload,
  blockhashCacheMs = 0
}: {
  connection: Connection;
  payer: PublicKey;
  payload: DirectTransactionPayload;
  blockhashCacheMs?: number;
}): Promise<{ transaction: VersionedTransaction; blockhash: string; lastValidBlockHeight: number; blockhashCacheHit: boolean }> {
  const { blockhash, cacheHit } = await getLatestBlockhashForDirectSend({
    connection,
    cacheMs: blockhashCacheMs
  });
  const message = new TransactionMessage({
    payerKey: payer,
    recentBlockhash: blockhash.blockhash,
    instructions: payload.instructions as TransactionInstruction[]
  }).compileToV0Message();
  return {
    transaction: new VersionedTransaction(message),
    blockhash: blockhash.blockhash,
    lastValidBlockHeight: blockhash.lastValidBlockHeight,
    blockhashCacheHit: cacheHit
  };
}

async function sendRawTransactionFanout({
  primaryConnection,
  sendConnections = [],
  serializedTransaction,
  options
}: {
  primaryConnection: Connection;
  sendConnections?: DirectSolanaSendConnection[];
  serializedTransaction: Uint8Array;
  options: { skipPreflight?: boolean; maxRetries?: number };
}): Promise<{
  signature: string;
  winner: DirectSolanaSendConnection;
  errors: Array<{ label: string; errorText: string }>;
  rpcCount: number;
}> {
  const primary = sendConnections[0] ?? {
    label: "primary",
    url: null,
    connection: primaryConnection
  };
  const attempts = sendConnections.length > 0 ? sendConnections : [primary];
  const errors: Array<{ label: string; errorText: string }> = [];
  const promises = attempts.map(async (candidate) => {
    try {
      return {
        signature: await candidate.connection.sendRawTransaction(serializedTransaction, options),
        winner: candidate
      };
    } catch (error) {
      errors.push({
        label: candidate.label,
        errorText: error instanceof Error ? error.message : String(error)
      });
      throw error;
    }
  });

  try {
    const result = await Promise.any(promises);
    return {
      ...result,
      errors: [...errors],
      rpcCount: attempts.length
    };
  } catch {
    const error = new Error(errors.map((entry) => `${entry.label}: ${entry.errorText}`).join("; ") || "all direct send RPCs failed");
    (error as Error & { sendRpcErrors?: Array<{ label: string; errorText: string }> }).sendRpcErrors = [...errors];
    throw error;
  }
}

export async function simulateSolanaDirectTransaction({
  connection,
  signer,
  payload
}: {
  connection: Connection;
  signer: Keypair;
  payload: DirectTransactionPayload;
}): Promise<TradeExecutionResult> {
  try {
    const { transaction, blockhash, lastValidBlockHeight } = await transactionForPayload({
      connection,
      payer: signer.publicKey,
      payload
    });
    transaction.sign([signer]);
    const simulation = await connection.simulateTransaction(transaction, DIRECT_SIMULATION_CONFIG);

    if (simulation.value.err) {
      return tradeExecutionFailedResult({
        provider: payload.provider,
        route: payload.route.route,
        errorText: `direct transaction simulation failed: ${JSON.stringify(simulation.value.err)}`,
        raw: simulation.value,
        metadata: {
          ...payload.metadata,
          blockhash,
          lastValidBlockHeight
        }
      });
    }

    return {
      ok: true,
      status: "simulated",
      provider: payload.provider,
      route: payload.route.route,
      signature: null,
      errorText: null,
      raw: simulation.value,
      submittedAtMs: null,
      confirmedAtMs: null,
      slot: null,
      metadata: {
        ...payload.metadata,
        blockhash,
        lastValidBlockHeight,
        unitsConsumed: simulation.value.unitsConsumed ?? null
      }
    };
  } catch (error) {
    return tradeExecutionFailedResult({
      provider: payload.provider,
      route: payload.route.route,
      errorText: error instanceof Error ? error.message : String(error),
      metadata: payload.metadata
    });
  }
}

export async function sendSolanaDirectTransaction({
  connection,
  signer,
  payload,
  config
}: {
  connection: Connection;
  signer: Keypair;
  payload: DirectTransactionPayload;
  config: SolanaDirectSendConfig;
}): Promise<TradeExecutionResult> {
  const blockedReason = directExecutionLiveBlockedReason(config.gate);
  if (blockedReason) {
    return tradeExecutionSkippedResult({
      provider: payload.provider,
      route: payload.route.route,
      reason: blockedReason,
      metadata: payload.metadata
    });
  }

  const nowMs = config.nowMs || Date.now;
  const stageRecords: DirectSolanaSendStageRecord[] = [];
  const markStage = (stage: DirectSolanaSendStage, details: Omit<DirectSolanaSendStageDetails, "atMs"> = {}) => {
    const record = { stage, atMs: nowMs(), ...details };
    stageRecords.push(record);
    config.onStage?.(stage, record);
    return record;
  };
  const metadataWithTiming = (metadata: Record<string, unknown>, extra: Record<string, unknown> = {}) => {
    const timing = directSolanaTimingMetadata(stageRecords);
    timing.simulateBeforeSend = config.simulateBeforeSend !== false;
    timing.skipPreflight = config.skipPreflight ?? null;
    timing.maxRetries = config.maxRetries ?? null;
    timing.blockhashCacheMs = blockhashCacheMsFromConfig(config.blockhashCacheMs);

    return {
      ...metadata,
      ...extra,
      directSolanaTiming: timing
    };
  };

  try {
    markStage("transaction_build_started", {
      instructionCount: payload.instructions.length
    });
    markStage("blockhash_started", {
      instructionCount: payload.instructions.length
    });
    const { transaction, blockhash, lastValidBlockHeight, blockhashCacheHit } = await transactionForPayload({
      connection,
      payer: signer.publicKey,
      payload,
      blockhashCacheMs: config.blockhashCacheMs
    });
    markStage("blockhash_received", {
      blockhash,
      lastValidBlockHeight,
      status: blockhashCacheHit ? "cached" : "fresh"
    });
    markStage("signing_started", {
      blockhash,
      lastValidBlockHeight
    });
    try {
      transaction.sign([signer]);
      markStage("signing_finished", {
        blockhash,
        lastValidBlockHeight,
        status: "ok"
      });
    } catch (error) {
      markStage("signing_finished", {
        blockhash,
        lastValidBlockHeight,
        status: "failed",
        errorText: error instanceof Error ? error.message : String(error)
      });
      throw error;
    }
    markStage("transaction_built", {
      blockhash,
      lastValidBlockHeight,
      instructionCount: payload.instructions.length
    });

    if (config.simulateBeforeSend !== false) {
      markStage("simulation_started", { blockhash, lastValidBlockHeight });
      let simulation;
      try {
        simulation = await connection.simulateTransaction(transaction, DIRECT_SIMULATION_CONFIG);
      } catch (error) {
        markStage("simulation_finished", {
          blockhash,
          lastValidBlockHeight,
          status: "failed",
          errorText: error instanceof Error ? error.message : String(error)
        });
        throw error;
      }
      markStage("simulation_finished", {
        blockhash,
        lastValidBlockHeight,
        unitsConsumed: simulation.value.unitsConsumed ?? null,
        status: simulation.value.err ? "failed" : "ok"
      });

      if (simulation.value.err && !config.allowSendAfterSimulationFailure) {
        return tradeExecutionFailedResult({
          provider: payload.provider,
          route: payload.route.route,
          errorText: `direct transaction simulation failed: ${JSON.stringify(simulation.value.err)}`,
          raw: simulation.value,
          metadata: metadataWithTiming(payload.metadata, {
            blockhash,
            lastValidBlockHeight
          })
        });
      }
    }

    let signature: string;
    const serializedTransaction = transaction.serialize();
    try {
      markStage("raw_send_started", {
        blockhash,
        lastValidBlockHeight,
        txBytes: serializedTransaction.length,
        rpcCount: config.sendConnections?.length || 1
      });
      const sendResult = await sendRawTransactionFanout({
        primaryConnection: connection,
        sendConnections: config.sendConnections,
        serializedTransaction,
        options: {
          skipPreflight: config.skipPreflight,
          maxRetries: config.maxRetries
        }
      });
      signature = sendResult.signature;
      markStage("signature_returned", {
        signature,
        blockhash,
        lastValidBlockHeight,
        txBytes: serializedTransaction.length,
        rpcCount: sendResult.rpcCount,
        sendRpcLabel: sendResult.winner.label,
        sendRpcUrl: sendResult.winner.url ?? null,
        sendRpcErrors: sendResult.errors
      });
    } catch (error) {
      const sendRpcErrors = (error as Error & { sendRpcErrors?: Array<{ label: string; errorText: string }> }).sendRpcErrors;
      markStage("raw_send_failed", {
        blockhash,
        lastValidBlockHeight,
        txBytes: serializedTransaction.length,
        rpcCount: config.sendConnections?.length || 1,
        sendRpcErrors,
        status: "failed",
        errorText: error instanceof Error ? error.message : String(error)
      });
      return tradeExecutionFailedResult({
        provider: payload.provider,
        route: payload.route.route,
        errorText: error instanceof Error ? error.message : String(error),
        metadata: metadataWithTiming(payload.metadata, {
          blockhash,
          lastValidBlockHeight
        })
      });
    }
    const submittedAtMs = stageRecords.find((stage) => stage.stage === "signature_returned")?.atMs ?? nowMs();

    if (config.confirmationMode === "background") {
      return {
        ok: true,
        status: "submitted",
        provider: payload.provider,
        route: payload.route.route,
        signature,
        errorText: null,
        raw: null,
        submittedAtMs,
        confirmedAtMs: null,
        slot: null,
        metadata: metadataWithTiming(payload.metadata, {
          blockhash,
          lastValidBlockHeight,
          confirmationMode: "background"
        })
      };
    }

    let confirmation;
    try {
      markStage("confirmation_started", {
        signature,
        blockhash,
        lastValidBlockHeight
      });
      confirmation = await connection.confirmTransaction({
        signature,
        blockhash,
        lastValidBlockHeight
      }, "confirmed");
      markStage("confirmation_finished", {
        signature,
        blockhash,
        lastValidBlockHeight,
        status: confirmation.value.err ? "failed" : "confirmed"
      });
    } catch (error) {
      markStage("confirmation_finished", {
        signature,
        blockhash,
        lastValidBlockHeight,
        status: "submitted",
        errorText: error instanceof Error ? error.message : String(error)
      });
      return {
        ok: true,
        status: "submitted",
        provider: payload.provider,
        route: payload.route.route,
        signature,
        errorText: null,
        raw: null,
        submittedAtMs,
        confirmedAtMs: null,
        slot: null,
        metadata: metadataWithTiming(payload.metadata, {
          blockhash,
          lastValidBlockHeight,
          confirmationError: error instanceof Error ? error.message : String(error)
        })
      };
    }

    if (confirmation.value.err) {
      const confirmationFinishedAtMs = stageRecords.find((stage) => stage.stage === "confirmation_finished")?.atMs ?? nowMs();
      return {
        ok: false,
        status: "failed",
        provider: payload.provider,
        route: payload.route.route,
        signature,
        errorText: `direct transaction confirmation failed: ${JSON.stringify(confirmation.value.err)}`,
        raw: confirmation.value,
        submittedAtMs,
        confirmedAtMs: confirmationFinishedAtMs,
        slot: confirmation.context?.slot ?? null,
        metadata: metadataWithTiming(payload.metadata, {
          blockhash,
          lastValidBlockHeight
        })
      };
    }

    const confirmationFinishedAtMs = stageRecords.find((stage) => stage.stage === "confirmation_finished")?.atMs ?? nowMs();
    return {
      ok: true,
      status: "confirmed",
      provider: payload.provider,
      route: payload.route.route,
      signature,
      errorText: null,
      raw: confirmation.value,
      submittedAtMs,
      confirmedAtMs: confirmationFinishedAtMs,
      slot: confirmation.context?.slot ?? null,
      metadata: metadataWithTiming(payload.metadata, {
        blockhash,
        lastValidBlockHeight
      })
    };
  } catch (error) {
    return tradeExecutionFailedResult({
      provider: payload.provider,
      route: payload.route.route,
      errorText: error instanceof Error ? error.message : String(error),
      metadata: metadataWithTiming(payload.metadata)
    });
  }
}
