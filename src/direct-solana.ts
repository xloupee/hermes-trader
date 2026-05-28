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
import { getAssociatedTokenAddressSync, NATIVE_MINT, TOKEN_PROGRAM_ID } from "@solana/spl-token";
import { buildDirectAutoTransactionPayload } from "./direct-auto.js";
import { maxQuoteLamportsForSlippageCap } from "./direct-budget.js";
import type { DirectTransactionPayload } from "./direct-pump.js";
import type { PlatformFeeSplit } from "./platform-fee.js";
import type {
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

type PumpSdkModule = typeof import("@pump-fun/pump-sdk");
type PumpSwapSdkModule = typeof import("@pump-fun/pump-swap-sdk");

let pumpSdkModule: PumpSdkModule | null = null;
let pumpSwapSdkModule: PumpSwapSdkModule | null = null;

function loadPumpSdk(): PumpSdkModule {
  pumpSdkModule ||= require("@pump-fun/pump-sdk") as PumpSdkModule;
  return pumpSdkModule;
}

function loadPumpSwapSdk(): PumpSwapSdkModule {
  pumpSwapSdkModule ||= require("@pump-fun/pump-swap-sdk") as PumpSwapSdkModule;
  return pumpSwapSdkModule;
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
  maxRetries?: number;
  nowMs?: () => number;
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
  const {
    getBuyTokenAmountFromSolAmount,
    getSellSolAmountFromTokenAmount,
    OnlinePumpSdk,
    PUMP_SDK
  } = loadPumpSdk();
  const sdk = new OnlinePumpSdk(connection);
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
    const [global, feeConfig] = await Promise.all([
      sdk.fetchGlobal(),
      sdk.fetchFeeConfig().catch(() => null)
    ]);
    const instructions = [...computeBudgetInstructions(request.priorityFeeSol), ...platformFeeInstruction({
      split: request.platformFee,
      fromPubkey: user
    })];

    if (request.action === "buy") {
      if (request.amountBasis !== "sol") {
        return tradeExecutionSkippedResult({
          provider: "direct-pump",
          route: route.route,
          reason: `direct Pump buy requires SOL amount basis; got ${amountBasisLabel(request.amountBasis)}`,
          platformFee: platformFeeResult(request.platformFee),
          metadata: { route }
        });
      }

      const buyState = await sdk.fetchBuyState(mint, user, TOKEN_PROGRAM_ID);
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
      instructions.push(...await PUMP_SDK.buyInstructions({
        global,
        bondingCurveAccountInfo: buyState.bondingCurveAccountInfo,
        bondingCurve: buyState.bondingCurve,
        associatedUserAccountInfo: buyState.associatedUserAccountInfo,
        mint,
        user,
        amount,
        solAmount,
        slippage: request.slippagePercent,
        tokenProgram: TOKEN_PROGRAM_ID
      }));
    } else {
      const sellState = await sdk.fetchSellState(mint, user, TOKEN_PROGRAM_ID);
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
            await tokenBalanceRaw({ connection, mint, user, tokenProgram: TOKEN_PROGRAM_ID }),
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

      instructions.push(...await PUMP_SDK.sellInstructions({
        global,
        bondingCurveAccountInfo: sellState.bondingCurveAccountInfo,
        bondingCurve: sellState.bondingCurve,
        mint,
        user,
        amount,
        solAmount,
        slippage: request.slippagePercent,
        tokenProgram: TOKEN_PROGRAM_ID,
        mayhemMode: sellState.bondingCurve.isMayhemMode,
        cashback: sellState.bondingCurve.isCashbackCoin
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
        instructionCount: instructions.length
      }
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
  const {
    canonicalPumpPoolPda,
    OnlinePumpAmmSdk,
    PUMP_AMM_SDK
  } = loadPumpSwapSdk();
  const pool = canonicalPumpPoolPda(mint, NATIVE_MINT);
  const sdk = new OnlinePumpAmmSdk(connection);
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
    const instructions = [...computeBudgetInstructions(request.priorityFeeSol), ...platformFeeInstruction({
      split: request.platformFee,
      fromPubkey: user
    })];

    if (request.action === "buy") {
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

      instructions.push(...await PUMP_AMM_SDK.sellBaseInput(state, amount, request.slippagePercent));
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
        instructionCount: instructions.length
      }
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
  payload
}: {
  connection: Connection;
  payer: PublicKey;
  payload: DirectTransactionPayload;
}): Promise<{ transaction: VersionedTransaction; blockhash: string; lastValidBlockHeight: number }> {
  const blockhash = await connection.getLatestBlockhash("confirmed");
  const message = new TransactionMessage({
    payerKey: payer,
    recentBlockhash: blockhash.blockhash,
    instructions: payload.instructions as TransactionInstruction[]
  }).compileToV0Message();
  return {
    transaction: new VersionedTransaction(message),
    blockhash: blockhash.blockhash,
    lastValidBlockHeight: blockhash.lastValidBlockHeight
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
    const simulation = await connection.simulateTransaction(transaction, {
      replaceRecentBlockhash: false,
      sigVerify: true
    });

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
  const submittedAtMs = nowMs();

  try {
    const { transaction, blockhash, lastValidBlockHeight } = await transactionForPayload({
      connection,
      payer: signer.publicKey,
      payload
    });
    transaction.sign([signer]);

    if (config.simulateBeforeSend !== false) {
      const simulation = await connection.simulateTransaction(transaction, {
        replaceRecentBlockhash: false,
        sigVerify: true
      });

      if (simulation.value.err && !config.allowSendAfterSimulationFailure) {
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
    }

    let signature: string;
    try {
      signature = await connection.sendRawTransaction(transaction.serialize(), {
        skipPreflight: config.skipPreflight,
        maxRetries: config.maxRetries
      });
    } catch (error) {
      return tradeExecutionFailedResult({
        provider: payload.provider,
        route: payload.route.route,
        errorText: error instanceof Error ? error.message : String(error),
        metadata: {
          ...payload.metadata,
          blockhash,
          lastValidBlockHeight
        }
      });
    }

    let confirmation;
    try {
      confirmation = await connection.confirmTransaction({
        signature,
        blockhash,
        lastValidBlockHeight
      }, "confirmed");
    } catch (error) {
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
        metadata: {
          ...payload.metadata,
          blockhash,
          lastValidBlockHeight,
          confirmationError: error instanceof Error ? error.message : String(error)
        }
      };
    }

    if (confirmation.value.err) {
      return {
        ok: false,
        status: "failed",
        provider: payload.provider,
        route: payload.route.route,
        signature,
        errorText: `direct transaction confirmation failed: ${JSON.stringify(confirmation.value.err)}`,
        raw: confirmation.value,
        submittedAtMs,
        confirmedAtMs: nowMs(),
        slot: null,
        metadata: payload.metadata
      };
    }

    return {
      ok: true,
      status: "confirmed",
      provider: payload.provider,
      route: payload.route.route,
      signature,
      errorText: null,
      raw: confirmation.value,
      submittedAtMs,
      confirmedAtMs: nowMs(),
      slot: null,
      metadata: {
        ...payload.metadata,
        blockhash,
        lastValidBlockHeight
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
