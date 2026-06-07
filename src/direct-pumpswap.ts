import type {
  DirectBuilderRequest,
  DirectInstruction,
  DirectTransactionPayload
} from "./direct-pump.js";
import type { DirectRouteMetadata, TradeExecutionResult } from "./trade-execution.js";
import {
  buildDirectRouteMetadata,
  tradeExecutionFailedResult,
  tradeExecutionSkippedResult
} from "./trade-execution.js";

export interface PumpSwapSdkLike {
  buyQuoteInput?: (input: Record<string, unknown>) => DirectInstruction[] | Promise<DirectInstruction[]>;
  sellBaseInput?: (input: Record<string, unknown>) => DirectInstruction[] | Promise<DirectInstruction[]>;
  buildBuyInstructions?: (input: Record<string, unknown>) => DirectInstruction[] | Promise<DirectInstruction[]>;
  buildSellInstructions?: (input: Record<string, unknown>) => DirectInstruction[] | Promise<DirectInstruction[]>;
}

export interface PumpSwapPoolState {
  poolAddress: string | null;
  state?: Record<string, unknown>;
  needsWrappedSolAccount?: boolean;
}

export interface PumpSwapPoolLoader {
  (request: DirectBuilderRequest): Promise<{ ok: boolean; reason?: string; pool?: PumpSwapPoolState }> | { ok: boolean; reason?: string; pool?: PumpSwapPoolState };
}

export async function buildDirectPumpSwapTransaction({
  sdk,
  request,
  loadPool
}: {
  sdk: PumpSwapSdkLike;
  request: DirectBuilderRequest;
  loadPool: PumpSwapPoolLoader;
}): Promise<TradeExecutionResult | DirectTransactionPayload> {
  const provider = "direct-pumpswap";
  const poolResult = await loadPool(request);
  const pool = poolResult.pool || { poolAddress: null };
  const route: DirectRouteMetadata = buildDirectRouteMetadata({
    provider,
    mint: request.mint,
    walletPublicKey: request.walletPublicKey,
    poolAddress: pool.poolAddress,
    priorityFeeSol: request.priorityFeeSol ?? null,
    slippagePercent: request.slippagePercent,
    amount: String(request.amount),
    amountBasis: request.amountBasis
  });

  if (!poolResult.ok || !pool.poolAddress) {
    return tradeExecutionSkippedResult({
      provider,
      route: route.route,
      reason: poolResult.reason || "canonical PumpSwap pool/state is unavailable",
      metadata: { route }
    });
  }

  try {
    const instructions = await buildPumpSwapInstructions({ sdk, request, pool });
    if (instructions.length === 0) {
      return tradeExecutionSkippedResult({
        provider,
        route: route.route,
        reason: "direct PumpSwap SDK returned no instructions",
        metadata: { route }
      });
    }

    const wrappedSolInstructions = pool.needsWrappedSolAccount
      ? [
          { kind: "create-wsol-account", owner: request.walletPublicKey },
          { kind: "sync-wsol-account", owner: request.walletPublicKey },
          ...instructions,
          { kind: "close-wsol-account", owner: request.walletPublicKey }
        ]
      : instructions;

    const withPlatformFee = request.platformFeeInstruction
      ? request.action === "sell"
        ? [...wrappedSolInstructions, request.platformFeeInstruction]
        : [request.platformFeeInstruction, ...wrappedSolInstructions]
      : wrappedSolInstructions;

    return {
      provider,
      route,
      instructions: withPlatformFee,
      signers: [],
      metadata: {
        ...(request.metadata || {}),
        route,
        pool
      }
    };
  } catch (error) {
    return tradeExecutionFailedResult({
      provider,
      route: route.route,
      errorText: error instanceof Error ? error.message : String(error),
      metadata: { route }
    });
  }
}

async function buildPumpSwapInstructions({
  sdk,
  request,
  pool
}: {
  sdk: PumpSwapSdkLike;
  request: DirectBuilderRequest;
  pool: PumpSwapPoolState;
}): Promise<DirectInstruction[]> {
  const input = {
    mint: request.mint,
    poolAddress: pool.poolAddress,
    amount: request.amount,
    amountBasis: request.amountBasis,
    slippagePercent: request.slippagePercent,
    priorityFeeSol: request.priorityFeeSol ?? null,
    walletPublicKey: request.walletPublicKey,
    state: pool.state || {}
  };

  const method =
    request.action === "buy"
      ? sdk.buyQuoteInput || sdk.buildBuyInstructions
      : sdk.sellBaseInput || sdk.buildSellInstructions;

  if (!method) {
    throw new Error(`direct PumpSwap SDK ${request.action} instruction builder is not configured`);
  }

  return method(input);
}
