import type { DirectInstruction, DirectTransactionPayload } from "./direct-pump.js";
import type { DirectExecutionGateConfig, TradeExecutionResult } from "./trade-execution.js";
import {
  directExecutionLiveBlockedReason,
  tradeExecutionFailedResult,
  tradeExecutionSkippedResult
} from "./trade-execution.js";

export interface DirectConnectionLike {
  getLatestBlockhash?: () => Promise<{ blockhash: string; lastValidBlockHeight?: number }> | { blockhash: string; lastValidBlockHeight?: number };
  simulateTransaction?: (transaction: DirectUnsignedTransaction) => Promise<DirectSimulationResult> | DirectSimulationResult;
  sendRawTransaction?: (serializedTransaction: Uint8Array, options?: DirectSendOptions) => Promise<string> | string;
  confirmTransaction?: (signature: string, blockhashContext?: { blockhash?: string; lastValidBlockHeight?: number }) => Promise<DirectConfirmationResult> | DirectConfirmationResult;
}

export interface DirectSignerLike {
  publicKey: string;
  signTransaction: (transaction: DirectUnsignedTransaction) => Promise<DirectSignedTransaction> | DirectSignedTransaction;
}

export interface DirectUnsignedTransaction {
  feePayer: string;
  recentBlockhash: string;
  instructions: DirectInstruction[];
  payload: DirectTransactionPayload;
}

export interface DirectSignedTransaction extends DirectUnsignedTransaction {
  serialize: () => Uint8Array;
}

export interface DirectSimulationResult {
  err: unknown;
  logs?: string[];
  unitsConsumed?: number;
}

export interface DirectConfirmationResult {
  err: unknown;
  slot?: number;
}

export interface DirectSendOptions {
  skipPreflight?: boolean;
  maxRetries?: number;
}

export interface SendDirectTransactionConfig {
  gate: DirectExecutionGateConfig;
  simulateBeforeSend?: boolean;
  allowSendAfterSimulationFailure?: boolean;
  skipPreflight?: boolean;
  maxRetries?: number;
  nowMs?: () => number;
}

export async function sendDirectTransaction({
  connection,
  signer,
  payload,
  config
}: {
  connection: DirectConnectionLike;
  signer: DirectSignerLike;
  payload: DirectTransactionPayload;
  config: SendDirectTransactionConfig;
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
    const blockhashContext = connection.getLatestBlockhash
      ? await connection.getLatestBlockhash()
      : { blockhash: "direct-exec-test-blockhash" };

    const transaction: DirectUnsignedTransaction = {
      feePayer: signer.publicKey,
      recentBlockhash: blockhashContext.blockhash,
      instructions: payload.instructions,
      payload
    };

    if (config.simulateBeforeSend !== false && connection.simulateTransaction) {
      const simulation = await connection.simulateTransaction(transaction);
      if (simulation.err && !config.allowSendAfterSimulationFailure) {
        return tradeExecutionFailedResult({
          provider: payload.provider,
          route: payload.route.route,
          errorText: `direct transaction simulation failed: ${JSON.stringify(simulation.err)}`,
          raw: simulation,
          metadata: {
            ...payload.metadata,
            simulation
          }
        });
      }
    }

    const signedTransaction = await signer.signTransaction(transaction);
    let signature: string | undefined;
    try {
      signature = await connection.sendRawTransaction?.(signedTransaction.serialize(), {
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
          blockhash: blockhashContext.blockhash,
          lastValidBlockHeight: blockhashContext.lastValidBlockHeight ?? null
        }
      });
    }

    if (!signature) {
      return tradeExecutionFailedResult({
        provider: payload.provider,
        route: payload.route.route,
        errorText: "direct connection did not return a transaction signature",
        metadata: payload.metadata
      });
    }

    if (!connection.confirmTransaction) {
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
          blockhash: blockhashContext.blockhash,
          lastValidBlockHeight: blockhashContext.lastValidBlockHeight ?? null
        }
      };
    }

    let confirmation: DirectConfirmationResult;
    try {
      confirmation = await connection.confirmTransaction(signature, blockhashContext);
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
          blockhash: blockhashContext.blockhash,
          lastValidBlockHeight: blockhashContext.lastValidBlockHeight ?? null,
          confirmationError: error instanceof Error ? error.message : String(error)
        }
      };
    }
    if (confirmation.err) {
      return {
        ok: false,
        status: "failed",
        provider: payload.provider,
        route: payload.route.route,
        signature,
        errorText: `direct transaction confirmation failed: ${JSON.stringify(confirmation.err)}`,
        raw: confirmation,
        submittedAtMs,
        confirmedAtMs: nowMs(),
        slot: confirmation.slot ?? null,
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
      raw: confirmation,
      submittedAtMs,
      confirmedAtMs: nowMs(),
      slot: confirmation.slot ?? null,
      metadata: {
        ...payload.metadata,
        blockhash: blockhashContext.blockhash,
        lastValidBlockHeight: blockhashContext.lastValidBlockHeight ?? null
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
