import { executionLocalDetectUs, firstNumber, subtractMs } from "@/lib/benchmark-position";
import { listLocalExecutions, summarizeLocalExecutions, type LocalExecutionReport } from "@/lib/local-executions";
import { listSignals, type SignalFilters, type SignalObservation } from "@/lib/signals";
import { createAdminClient } from "@/lib/supabase/admin";

export interface BenchmarkLatencyCells {
  block: string;
  local: string;
  decode: string;
  scan: string;
  txParse: string;
}

export interface BenchmarkRow {
  id: string;
  createdAt: string;
  observedAtMs: number;
  provider: string;
  source: string;
  endpoint: string | null;
  targetWallet: string;
  signature: string;
  slot: number;
  action: string;
  mint: string;
  route: string;
  copyWallet: string | null;
  telegramSubscriber: string | null;
  copyable: boolean;
  signalObservationId: number | null;
  localExecutionId: number | null;
  signal: SignalObservation | null;
  execution: LocalExecutionReport | null;
}

export function buildBenchmarkRows(signals: SignalObservation[], executions: LocalExecutionReport[]): BenchmarkRow[] {
  return buildBenchmarkRowsWithSubscribers(signals, executions, new Map());
}

export function buildBenchmarkRowsWithSubscribers(
  signals: SignalObservation[],
  executions: LocalExecutionReport[],
  subscriberByCopyWallet: Map<string, string>
): BenchmarkRow[] {
  const signalBySignature = new Map(signals.map((signal) => [signal.signature, signal]));
  const executionSignatures = new Set(executions.map((execution) => execution.observedSignature));
  const executionRows = executions.map((execution) =>
    benchmarkRowFromExecution(
      execution,
      signalBySignature.get(execution.observedSignature),
      execution.copyWallet ? subscriberByCopyWallet.get(execution.copyWallet) ?? null : null
    )
  );
  const signalOnlyRows = signals
    .filter((signal) => !executionSignatures.has(signal.signature))
    .map(benchmarkRowFromSignal);

  return [...executionRows, ...signalOnlyRows].sort((left, right) => right.observedAtMs - left.observedAtMs);
}

export async function listBenchmarkRows(filters: SignalFilters) {
  const [signals, executions] = await Promise.all([
    listSignals(filters),
    listLocalExecutions(filters)
  ]);
  const subscriberByCopyWallet = await listTelegramSubscribersByCopyWallet(executions);

  return {
    rows: buildBenchmarkRowsWithSubscribers(signals, executions, subscriberByCopyWallet),
    summary: summarizeLocalExecutions(executions),
    filters
  };
}

async function listTelegramSubscribersByCopyWallet(executions: LocalExecutionReport[]): Promise<Map<string, string>> {
  const copyWallets = [...new Set(executions.map((execution) => execution.copyWallet).filter((wallet): wallet is string => Boolean(wallet)))];
  if (copyWallets.length === 0) {
    return new Map();
  }

  const { data, error } = await createAdminClient()
    .from("telegram_trading_wallets")
    .select("chat_id,public_key")
    .in("public_key", copyWallets);

  if (error) {
    return new Map();
  }

  return new Map(
    ((data as Array<{ chat_id: string | null; public_key: string | null }> | null) || [])
      .filter((row): row is { chat_id: string; public_key: string } => Boolean(row.chat_id && row.public_key))
      .map((row) => [row.public_key, row.chat_id])
  );
}

function benchmarkRowFromSignal(signal: SignalObservation): BenchmarkRow {
  return {
    id: `signal:${signal.id}`,
    createdAt: signal.createdAt,
    observedAtMs: signal.observedAtMs,
    provider: signal.provider,
    source: signal.source,
    endpoint: signal.endpoint,
    targetWallet: signal.targetWallet,
    signature: signal.signature,
    slot: signal.slot,
    action: signal.action,
    mint: signal.mint,
    route: signal.route,
    copyWallet: null,
    telegramSubscriber: null,
    copyable: signal.copyable,
    signalObservationId: signal.id,
    localExecutionId: null,
    signal,
    execution: null
  };
}

function benchmarkRowFromExecution(
  execution: LocalExecutionReport,
  signal?: SignalObservation,
  telegramSubscriber: string | null = null
): BenchmarkRow {
  const fallbackSignal = signal ? signalWithExecutionFallback(signal, execution) : signalFromExecution(execution);
  return {
    id: `execution:${execution.id}`,
    createdAt: execution.createdAt,
    observedAtMs: execution.observedAtMs,
    provider: fallbackSignal.provider,
    source: fallbackSignal.source,
    endpoint: fallbackSignal.endpoint,
    targetWallet: execution.observedWallet,
    signature: execution.observedSignature,
    slot: execution.slot,
    action: execution.observedAction,
    mint: execution.mint,
    route: execution.selectedRoute,
    copyWallet: execution.copyWallet,
    telegramSubscriber,
    copyable: execution.observedAction === "buy" && execution.buyStatus === "buyLanded",
    signalObservationId: signal?.id ?? null,
    localExecutionId: execution.id,
    signal: fallbackSignal,
    execution
  };
}

function signalFromExecution(execution: LocalExecutionReport): SignalObservation {
  const tradeParsedAtMs = execution.matchedAtMs ?? execution.observedAtMs;
  return {
    id: 0,
    createdAt: execution.createdAt,
    provider: execution.provider,
    source: execution.source,
    endpoint: execution.endpoint,
    targetWallet: execution.observedWallet,
    signature: execution.observedSignature,
    slot: execution.slot,
    action: execution.observedAction,
    mint: execution.mint,
    route: execution.selectedRoute,
    observedAtMs: execution.observedAtMs,
    grpcMessageReceivedAtMs: execution.feedReceivedAtMs,
    entriesDeserializedAtMs: execution.decodedAtMs,
    tradeParsedAtMs,
    blockTimeMs: execution.targetBlockTimeMs,
    observedMinusBlockTimeMs: subtractMs(tradeParsedAtMs, execution.targetBlockTimeMs),
    grpcReceivedMinusBlockTimeMs: subtractMs(execution.feedReceivedAtMs, execution.targetBlockTimeMs),
    deserializeMs: null,
    parseMs: null,
    localDetectMs: null,
    deserializeUs: execution.feedReceivedToDecodedUs,
    parseUs: firstNumber(execution.routeParseUs, execution.txParseUs),
    localDetectUs: executionLocalDetectUs(execution),
    batchTransactionCount: execution.batchTransactionCount,
    matchedTransactionIndex: execution.matchedTransactionIndex,
    batchScanUs: execution.batchScanUs,
    txParseUs: execution.txParseUs,
    accountExpandUs: execution.accountExpandUs,
    walletMatchUs: execution.walletMatchUs,
    routeParseUs: execution.routeParseUs,
    solAmount: execution.observedSolAmount,
    tokenAmount: null,
    copyable: execution.observedAction === "buy" && execution.buyStatus === "buyLanded",
    rawEvent: execution.rawExecution
  };
}

function signalWithExecutionFallback(signal: SignalObservation, execution: LocalExecutionReport): SignalObservation {
  const tradeParsedAtMs = firstNumber(signal.tradeParsedAtMs, execution.matchedAtMs, execution.observedAtMs);
  const grpcMessageReceivedAtMs = firstNumber(signal.grpcMessageReceivedAtMs, execution.feedReceivedAtMs);
  const blockTimeMs = firstNumber(signal.blockTimeMs, execution.targetBlockTimeMs);

  return {
    ...signal,
    endpoint: signal.endpoint ?? execution.endpoint,
    route: signal.route || execution.selectedRoute,
    observedAtMs: firstNumber(signal.observedAtMs, execution.observedAtMs) ?? signal.observedAtMs,
    grpcMessageReceivedAtMs,
    entriesDeserializedAtMs: firstNumber(signal.entriesDeserializedAtMs, execution.decodedAtMs),
    tradeParsedAtMs,
    blockTimeMs,
    observedMinusBlockTimeMs: firstNumber(
      signal.observedMinusBlockTimeMs,
      subtractMs(tradeParsedAtMs, blockTimeMs)
    ),
    grpcReceivedMinusBlockTimeMs: firstNumber(
      signal.grpcReceivedMinusBlockTimeMs,
      subtractMs(grpcMessageReceivedAtMs, blockTimeMs)
    ),
    deserializeUs: firstNumber(signal.deserializeUs, execution.feedReceivedToDecodedUs),
    parseUs: firstNumber(signal.parseUs, execution.routeParseUs, execution.txParseUs),
    localDetectUs: firstNumber(signal.localDetectUs, executionLocalDetectUs(execution)),
    batchTransactionCount: firstNumber(signal.batchTransactionCount, execution.batchTransactionCount),
    matchedTransactionIndex: firstNumber(signal.matchedTransactionIndex, execution.matchedTransactionIndex),
    batchScanUs: firstNumber(signal.batchScanUs, execution.batchScanUs),
    txParseUs: firstNumber(signal.txParseUs, execution.txParseUs),
    accountExpandUs: firstNumber(signal.accountExpandUs, execution.accountExpandUs),
    walletMatchUs: firstNumber(signal.walletMatchUs, execution.walletMatchUs),
    routeParseUs: firstNumber(signal.routeParseUs, execution.routeParseUs),
    solAmount: firstNumber(signal.solAmount, execution.observedSolAmount),
    copyable: signal.copyable || (execution.observedAction === "buy" && execution.buyStatus === "buyLanded")
  };
}
