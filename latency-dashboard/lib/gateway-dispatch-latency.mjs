function objectValue(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : null;
}

function finiteNonNegative(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
}

function minimum(values) {
  return values.length > 0 ? Math.min(...values) : null;
}

export function gatewayDispatchLatency(rawExecution) {
  const execution = objectValue(rawExecution);
  const receipt = objectValue(execution?.gatewayReceipt);
  const attempts = Array.isArray(receipt?.dispatchEvidence) ? receipt.dispatchEvidence : [];
  const lanes = attempts.flatMap((attempt) => {
    const value = objectValue(attempt);
    return Array.isArray(value?.lanes) ? value.lanes : [];
  }).map(objectValue).filter(Boolean);
  const possibleWritesNs = lanes
    .map((lane) => finiteNonNegative(lane.detectToPossibleWriteNs))
    .filter((value) => value !== null);
  const acknowledgedWritesNs = lanes
    .filter((lane) => lane.outcome === "acknowledged")
    .map((lane) => finiteNonNegative(lane.detectToPossibleWriteNs))
    .filter((value) => value !== null);
  const firstPossibleWriteNs = minimum(possibleWritesNs);
  const firstAcknowledgedWriteNs = minimum(acknowledgedWritesNs);

  return {
    detectToFirstPossibleWriteUs: firstPossibleWriteNs === null ? null : firstPossibleWriteNs / 1_000,
    detectToFirstAcknowledgedWriteUs: firstAcknowledgedWriteNs === null ? null : firstAcknowledgedWriteNs / 1_000
  };
}
