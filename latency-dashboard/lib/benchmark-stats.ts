export interface MetricStats {
  count: number;
  p50: number | null;
  p90: number | null;
  min: number | null;
  max: number | null;
  avg: number | null;
}

export function metricStats(values: Array<number | null | undefined>): MetricStats {
  const numeric = values
    .filter((value): value is number => typeof value === "number" && Number.isFinite(value))
    .sort((left, right) => left - right);

  if (numeric.length === 0) {
    return { count: 0, p50: null, p90: null, min: null, max: null, avg: null };
  }

  const percentile = (percent: number) => numeric[Math.min(numeric.length - 1, Math.floor((numeric.length - 1) * percent))];
  const total = numeric.reduce((sum, value) => sum + value, 0);

  return {
    count: numeric.length,
    p50: percentile(0.5),
    p90: percentile(0.9),
    min: numeric[0],
    max: numeric[numeric.length - 1],
    avg: Math.round(total / numeric.length)
  };
}
