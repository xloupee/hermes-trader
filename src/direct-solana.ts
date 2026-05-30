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
const MAX_BLOCKHASH_CACHE_MS = 30_000;
const latestBlockhashByConnection = new WeakMap<Connection, {
  value: LatestBlockhash | null;
  expiresAtMs: number;
  inflight: Promise<LatestBlockhash> | null;
}>();
let pumpGlobalCache: { value: PumpGlobal; expiresAtMs: number } | null = null;
let pumpGlobalInflight: Promise<PumpGlobal> | null = null;
let pumpFeeConfigCache: { value: PumpFeeConfig | null; expiresAtMs: number } | null = null;
let pumpFeeConfigInflight: Promise<PumpFeeConfig | null> | null = null;

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

async function fetchCachedPumpGlobal(sdk: OnlinePumpSdkInstance): Promise<PumpGlobal> {
  const now = Date.now();
  if (pumpGlobalCache && pumpGlobalCache.expiresAtMs > now) {
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

async function fetchCachedPumpFeeConfig(sdk: OnlinePumpSdkInstance): Promise<PumpFeeConfig | null> {
  const now = Date.now();
  if (pumpFeeConfigCache && pumpFeeConfigCache.expiresAtMs > now) {
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
  provider
}: {
  connection: Connection;
  provider: DirectTradeExecutionProvider;
}): Promise<void> {
  const warmups: Promise<unknown>[] = [];

  if (provider === "direct-pump" || provider === "direct-auto") {
    const pumpModule = loadPumpSdk();
    const pumpSdk = getOnlinePumpSdk(connection, pumpModule);
    warmups.push(fetchCachedPumpGlobal(pumpSdk), fetchCachedPumpFeeConfig(pumpSdk));
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
  cacheMs = 0
}: {
  connection: Connection;
  cacheMs?: number;
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
  if (entry?.value && entry.expiresAtMs > now) {
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
  cacheMs
}: {
  connection: Connection;
  cacheMs: number;
}): Promise<void> {
  await getLatestBlockhashForDirectSend({ connection, cacheMs });
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
}

export interface SolanaDirectSendConfig {
  gate: DirectExecutionGateConfig;
  simulateBeforeSend?: boolean;
  allowSendAfterSimulationFailure?: boolean;
  skipPreflight?: boolean;
  confirmationMode?: "inline" | "background";
  maxRetries?: number;
  blockhashCacheMs?: number;
  nowMs?: () => number;
  onStage?: (stage: DirectSolanaSendStage, details: DirectSolanaSendStageDetails) => void;
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
}

interface DirectSolanaSendStageRecord extends DirectSolanaSendStageDetails {
  stage: DirectSolanaSendStage;
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

export async function resolveMintTokenProgram({
  connection,
  mint
}: {
  connection: Connection;
  mint: PublicKey;
}): Promise<PublicKey> {
  const mintInfo = await connection.getAccountInfo(mint, "confirmed");

  if (!mintInfo) {
    throw new Error(`mint account not found: ${mint.toBase58()}`);
  }

  if (mintInfo.owner.equals(TOKEN_PROGRAM_ID) || mintInfo.owner.equals(TOKEN_2022_PROGRAM_ID)) {
    return mintInfo.owner;
  }

  throw new Error(`mint account is not owned by SPL Token or Token-2022: ${mint.toBase58()}`);
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
    return buildDirectAutoTransactionPayload({
      metadata: request.metadata,
      platformFee: platformFeeResult(request.platformFee),
      attempts: [
        {
          provider: "direct-pumpswap",
          build: () => buildDirectPumpSwapSolanaPayload({
            connection,
            request: {
              ...request,
              provider: "direct-pumpswap",
              metadata: {
                ...(request.metadata || {}),
                requestedProvider: "direct-auto",
                autoRouteAttempt: "direct-pumpswap"
              }
            }
          })
        },
        {
          provider: "direct-pump",
          build: () => buildDirectPumpSolanaPayload({
            connection,
            request: {
              ...request,
              provider: "direct-pump",
              metadata: {
                ...(request.metadata || {}),
                requestedProvider: "direct-auto",
                autoRouteAttempt: "direct-pump"
              }
            }
          })
        }
      ]
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
  const mint = publicKey(request.mint, "mint");
  const user = publicKey(request.walletPublicKey, "walletPublicKey");
  const pumpModule = loadPumpSdk();
  const {
    getBuyTokenAmountFromSolAmount,
    getSellSolAmountFromTokenAmount,
    PUMP_SDK
  } = pumpModule;
  const sdk = getOnlinePumpSdk(connection, pumpModule);
  const route = buildDirectRouteMetadata({
    provider: "direct-pump",
    mint: request.mint,
    walletPublicKey: request.walletPublicKey,
    priorityFeeSol: request.priorityFeeSol,
    slippagePercent: request.slippagePercent,
    amount: request.amountLamports.toString(),
    amountBasis: request.amountBasis
  });

  try {
    const [tokenProgram, global, feeConfig] = await Promise.all([
      resolveMintTokenProgram({ connection, mint }),
      fetchCachedPumpGlobal(sdk),
      fetchCachedPumpFeeConfig(sdk)
    ]);
    const instructions = [...computeBudgetInstructions(request.priorityFeeSol)];
    let appliedPlatformFee = request.platformFee;

    if (request.action === "buy") {
      instructions.push(...platformFeeInstruction({
        split: request.platformFee,
        fromPubkey: user
      }));

      if (request.amountBasis !== "sol") {
        return tradeExecutionSkippedResult({
          provider: "direct-pump",
          route: route.route,
          reason: `direct Pump buy requires SOL amount basis; got ${amountBasisLabel(request.amountBasis)}`,
          platformFee: platformFeeResult(request.platformFee),
          metadata: { route }
        });
      }

      const buyState = await sdk.fetchBuyState(mint, user, tokenProgram);
      if (buyState.bondingCurve.complete) {
        return tradeExecutionSkippedResult({
          provider: "direct-pump",
          route: route.route,
          reason: "direct Pump bonding curve is complete/migrated; use direct-pumpswap",
          platformFee: platformFeeResult(request.platformFee),
          metadata: { ...(request.metadata || {}), route }
        });
      }

      const sdkQuoteLamports = maxQuoteLamportsForSlippageCap(request.amountLamports, request.slippagePercent);
      if (sdkQuoteLamports <= 0n) {
        return tradeExecutionSkippedResult({
          provider: "direct-pump",
          route: route.route,
          reason: "direct Pump buy amount is too small after slippage cap",
          platformFee: platformFeeResult(request.platformFee),
          metadata: { ...(request.metadata || {}), route }
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
    } else {
      const sellState = await sdk.fetchSellState(mint, user, tokenProgram);
      if (sellState.bondingCurve.complete) {
        return tradeExecutionSkippedResult({
          provider: "direct-pump",
          route: route.route,
          reason: "direct Pump bonding curve is complete/migrated; use direct-pumpswap",
          platformFee: platformFeeResult(request.platformFee),
          metadata: { ...(request.metadata || {}), route }
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
    }

    return {
      provider: "direct-pump",
      route,
      instructions,
      signers: [],
      metadata: {
        ...(request.metadata || {}),
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
      },
      platformFee: appliedPlatformFee
    };
  } catch (error) {
    return tradeExecutionFailedResult({
      provider: "direct-pump",
      route: route.route,
      errorText: error instanceof Error ? error.message : String(error),
      metadata: { ...(request.metadata || {}), route },
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
        txBytes: serializedTransaction.length
      });
      signature = await connection.sendRawTransaction(serializedTransaction, {
        skipPreflight: config.skipPreflight,
        maxRetries: config.maxRetries
      });
      markStage("signature_returned", {
        signature,
        blockhash,
        lastValidBlockHeight,
        txBytes: serializedTransaction.length
      });
    } catch (error) {
      markStage("raw_send_failed", {
        blockhash,
        lastValidBlockHeight,
        txBytes: serializedTransaction.length,
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
