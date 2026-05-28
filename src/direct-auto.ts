import type { DirectTransactionPayload } from "./direct-pump.js";
import type {
  DirectTradeExecutionProvider,
  TradeExecutionPlatformFee,
  TradeExecutionResult
} from "./trade-execution.js";
import { tradeExecutionSkippedResult } from "./trade-execution.js";

export interface DirectAutoRouteAttempt {
  provider: Exclude<DirectTradeExecutionProvider, "direct-auto">;
  build: () => Promise<TradeExecutionResult | DirectTransactionPayload>;
}

export function isDirectTransactionPayload(
  value: TradeExecutionResult | DirectTransactionPayload
): value is DirectTransactionPayload {
  return Array.isArray((value as DirectTransactionPayload).instructions);
}

export async function buildDirectAutoTransactionPayload({
  attempts,
  metadata = {},
  platformFee = null
}: {
  attempts: DirectAutoRouteAttempt[];
  metadata?: Record<string, unknown>;
  platformFee?: TradeExecutionPlatformFee | null;
}): Promise<TradeExecutionResult | DirectTransactionPayload> {
  const results: Array<{
    provider: DirectAutoRouteAttempt["provider"];
    status: TradeExecutionResult["status"];
    errorText: string | null;
  }> = [];

  for (const attempt of attempts) {
    let result: TradeExecutionResult | DirectTransactionPayload;
    try {
      result = await attempt.build();
    } catch (error) {
      results.push({
        provider: attempt.provider,
        status: "failed",
        errorText: error instanceof Error ? error.message : String(error)
      });
      continue;
    }

    if (isDirectTransactionPayload(result)) {
      return result;
    }

    results.push({
      provider: attempt.provider,
      status: result.status,
      errorText: result.errorText
    });
  }

  return tradeExecutionSkippedResult({
    provider: "direct-auto",
    route: "auto",
    reason: [
      "direct-auto could not build a PumpSwap or Pump route",
      ...results.map((result) => `${result.provider}: ${result.errorText || result.status}`)
    ].join("; "),
    metadata: {
      ...metadata,
      autoRouteAttempts: results
    },
    platformFee
  });
}
