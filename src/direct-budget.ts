export function maxQuoteLamportsForSlippageCap(maxSpendLamports: bigint, slippagePercent: number): bigint {
  if (maxSpendLamports <= 0n) {
    return 0n;
  }

  if (!Number.isFinite(slippagePercent) || slippagePercent <= 0) {
    return maxSpendLamports;
  }

  const precision = 1_000_000_000n;
  const slippageFactor = BigInt(Math.floor(slippagePercent * 10_000_000));
  const denominator = precision + slippageFactor;

  return denominator <= 0n ? maxSpendLamports : (maxSpendLamports * precision) / denominator;
}
