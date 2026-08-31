export interface GatewayDispatchLatency {
  detectToFirstPossibleWriteUs: number | null;
  detectToFirstAcknowledgedWriteUs: number | null;
}

export function gatewayDispatchLatency(rawExecution: unknown): GatewayDispatchLatency;
