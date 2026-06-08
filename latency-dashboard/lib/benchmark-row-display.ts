import { duration, ms, msToUs, us } from "@/lib/benchmark-format";
import { copyAckMs, executionLocalDetectUs, firstNumber, subtractMs } from "@/lib/benchmark-position";
import type { BenchmarkRow } from "@/lib/benchmark-rows";

export function durationWithFallback(
  msValue: number | null | undefined,
  usValue: number | null | undefined,
  fallbackUs: number | null | undefined
): string {
  return duration(msValue, firstNumber(usValue, fallbackUs));
}

export function benchmarkLatencyCells(row: BenchmarkRow) {
  const execution = row.execution;
  const signal = row.signal;

  if (execution && !row.signalObservationId) {
    return {
      block: "exec-only",
      local: durationWithFallback(null, executionLocalDetectUs(execution), msToUs(execution.observedToSignatureReturnedMs)),
      decode: durationWithFallback(null, execution.feedReceivedToDecodedUs, execution.unsignedBuildUs),
      scan: durationWithFallback(null, execution.batchScanUs, msToUs(execution.sendLaneMs)),
      txParse: durationWithFallback(null, execution.txParseUs, msToUs(copyAckMs(execution)))
    };
  }

  return {
    block: ms(firstNumber(signal?.observedMinusBlockTimeMs, subtractMs(execution?.matchedAtMs, execution?.targetBlockTimeMs))),
    local: durationWithFallback(signal?.localDetectMs, signal?.localDetectUs, executionLocalDetectUs(execution)),
    decode: durationWithFallback(signal?.deserializeMs, signal?.deserializeUs, execution?.feedReceivedToDecodedUs),
    scan: us(firstNumber(signal?.batchScanUs, execution?.batchScanUs)),
    txParse: us(firstNumber(signal?.txParseUs, execution?.txParseUs))
  };
}
