import type {
  DirectRouteMetadata,
  TradeAction,
  TradeAmountBasis,
  TradeExecutionProvider,
  TradeExecutionResult
} from "./trade-execution.js";
import {
  buildDirectRouteMetadata,
  tradeExecutionFailedResult,
  tradeExecutionSkippedResult
} from "./trade-execution.js";

export type DirectInstruction = Record<string, unknown> | {
  programId?: unknown;
  keys?: unknown[];
  data?: unknown;
};

export interface DirectBuilderRequest {
  action: TradeAction;
  mint: string;
  amount: number | string | bigint;
  amountBasis: TradeAmountBasis;
  slippagePercent: number;
  priorityFeeSol?: number | null;
  walletPublicKey: string;
  platformFeeInstruction?: DirectInstruction | null;
  metadata?: Record<string, unknown>;
}

export interface DirectTransactionPayload {
  provider: TradeExecutionProvider;
  route: DirectRouteMetadata;
  instructions: DirectInstruction[];
  signers: unknown[];
  metadata: Record<string, unknown>;
}

export interface PumpSdkLike {
  buyInstructions?: (input: Record<string, unknown>) => DirectInstruction[] | Promise<DirectInstruction[]>;
  sellInstructions?: (input: Record<string, unknown>) => DirectInstruction[] | Promise<DirectInstruction[]>;
  buildBuyInstructions?: (input: Record<string, unknown>) => DirectInstruction[] | Promise<DirectInstruction[]>;
  buildSellInstructions?: (input: Record<string, unknown>) => DirectInstruction[] | Promise<DirectInstruction[]>;
}

export interface DirectPumpStateLoader {
  (request: DirectBuilderRequest): Promise<{ ok: boolean; reason?: string; state?: Record<string, unknown> }> | { ok: boolean; reason?: string; state?: Record<string, unknown> };
}

export async function buildDirectPumpTransaction({
  sdk,
  request,
  loadState
}: {
  sdk: PumpSdkLike;
  request: DirectBuilderRequest;
  loadState?: DirectPumpStateLoader;
}): Promise<TradeExecutionResult | DirectTransactionPayload> {
  const provider = "direct-pump";
  const route = buildDirectRouteMetadata({
    provider,
    mint: request.mint,
    walletPublicKey: request.walletPublicKey,
    priorityFeeSol: request.priorityFeeSol ?? null,
    slippagePercent: request.slippagePercent,
    amount: String(request.amount),
    amountBasis: request.amountBasis
  });

  const stateResult = loadState ? await loadState(request) : { ok: true, state: {} };
  if (!stateResult.ok) {
    return tradeExecutionSkippedResult({
      provider,
      route: route.route,
      reason: stateResult.reason || "direct Pump bonding-curve state is unavailable",
      metadata: { route }
    });
  }

  try {
    const instructions = await buildPumpInstructions({ sdk, request, state: stateResult.state || {} });
    if (instructions.length === 0) {
      return tradeExecutionSkippedResult({
        provider,
        route: route.route,
        reason: "direct Pump SDK returned no instructions",
        metadata: { route }
      });
    }

    const prefixed = request.platformFeeInstruction ? [request.platformFeeInstruction, ...instructions] : instructions;
    return {
      provider,
      route,
      instructions: prefixed,
      signers: [],
      metadata: {
        ...(request.metadata || {}),
        route,
        state: stateResult.state || {}
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

async function buildPumpInstructions({
  sdk,
  request,
  state
}: {
  sdk: PumpSdkLike;
  request: DirectBuilderRequest;
  state: Record<string, unknown>;
}): Promise<DirectInstruction[]> {
  const input = {
    mint: request.mint,
    amount: request.amount,
    amountBasis: request.amountBasis,
    slippagePercent: request.slippagePercent,
    priorityFeeSol: request.priorityFeeSol ?? null,
    walletPublicKey: request.walletPublicKey,
    state
  };

  const method =
    request.action === "buy"
      ? sdk.buyInstructions || sdk.buildBuyInstructions
      : sdk.sellInstructions || sdk.buildSellInstructions;

  if (!method) {
    throw new Error(`direct Pump SDK ${request.action} instruction builder is not configured`);
  }

  return method(input);
}
