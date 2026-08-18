import { createAdminClient } from "@/lib/supabase/admin";
import type { DashboardExecutionFilters } from "@/lib/local-executions";
import type { LeaderDiagnostics } from "@/lib/leader-diagnostics";

export interface GatewayConfirmation {
  id: number;
  createdAt: string;
  observedAtMs: number;
  confirmationAtMs: number | null;
  provider: string;
  source: string;
  inboundSource: string | null;
  observedWallet: string;
  copyWallet: string;
  observedSignature: string;
  signature: string;
  slot: number;
  confirmationSlot: number | null;
  slotDelta: number | null;
  targetTxIndex: number | null;
  copyTxIndex: number | null;
  sameSlotTxDelta: number | null;
  txDelta: number | null;
  targetSlotLeader: string | null;
  copySlotLeader: string | null;
  telegramId: string | null;
  firstAckAtMs: number | null;
  dispatchAttempts: number | null;
  observedToSendSubmittedMs: number | null;
  observedToSignatureReturnedMs: number | null;
  sendLaneAttribution: unknown;
  rawConfirmation: unknown;
  selectedRoute: string | null;
  routeLayout: string | null;
  mint: string;
  observedAction: string;
  transactionRole: string;
  firstAckLane: string | null;
  dispatchToAckMs: number | null;
  status: string | null;
  ok: boolean;
  confirmationStatus: string | null;
  reason: string | null;
  gatewayIntentKey: string | null;
  gatewayState: string | null;
  reconciliationApplied: boolean | null;
  leaderDiagnostics: LeaderDiagnostics | null;
}

export interface GatewayConfirmationFreshness {
  latestObservedAtMs: number | null;
  latestConfirmationAtMs: number | null;
  latestCreatedAt: string | null;
}

const GATEWAY_CONFIRMATION_SELECT = [
  "id",
  "created_at",
  "observed_at_ms",
  "confirmation_at_ms",
  "provider",
  "source",
  "execution_telemetry",
  "observed_wallet",
  "copy_wallet",
  "observed_signature",
  "signature",
  "slot",
  "confirmation_slot",
  "slot_delta",
  "target_tx_index",
  "copy_tx_index",
  "same_slot_tx_delta",
  "tx_delta",
  "target_slot_leader",
  "copy_slot_leader",
  "telegram_id",
  "first_ack_lane",
  "first_ack_at_ms",
  "dispatch_attempts",
  "observed_to_send_submitted_ms",
  "observed_to_signature_returned_ms",
  "send_lane_attribution",
  "raw_confirmation",
  "selected_route",
  "route_layout",
  "mint",
  "observed_action",
  "transaction_role",
  "status",
  "ok",
  "confirmation_status",
  "reason",
  "gateway_intent_key",
  "gateway_state",
  "reconciliation_applied"
].join(",");

function missingTableError(error: { message?: string; code?: string } | null): boolean {
  const message = error?.message?.toLowerCase() || "";
  return error?.code === "42P01" || message.includes("does not exist") || message.includes("schema cache");
}

function numberValue(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function booleanValue(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function mapGatewayConfirmation(row: Record<string, unknown>): GatewayConfirmation {
  const telemetry = row.execution_telemetry && typeof row.execution_telemetry === "object" && !Array.isArray(row.execution_telemetry)
    ? row.execution_telemetry as Record<string, unknown>
    : null;
  const inbound = telemetry?.inbound && typeof telemetry.inbound === "object" && !Array.isArray(telemetry.inbound)
    ? telemetry.inbound as Record<string, unknown>
    : null;
  const rawConfirmation = objectValue(row.raw_confirmation);
  const rawTelemetry = objectValue(rawConfirmation?.executionTelemetry);
  const rawTimeline = objectValue(rawTelemetry?.timeline);
  const rawFirstAck = objectValue(rawTimeline?.firstAck);
  const durableAckEvidence = telemetry?.retryAckEvidenceDurable !== false;
  const ackLane = durableAckEvidence
    ? stringValue(telemetry?.ackLane)
      ?? stringValue(row.first_ack_lane)
      ?? stringValue(rawConfirmation?.firstAckLane)
      ?? stringValue(rawConfirmation?.sendRpcWinner)
      ?? stringValue(rawTelemetry?.ackLane)
      ?? stringValue(rawFirstAck?.lane)
    : null;
  return {
    id: numberValue(row.id) ?? 0,
    createdAt: stringValue(row.created_at) ?? new Date(0).toISOString(),
    observedAtMs: numberValue(row.observed_at_ms) ?? 0,
    confirmationAtMs: numberValue(row.confirmation_at_ms),
    provider: stringValue(row.provider) ?? "unknown",
    source: stringValue(row.source) ?? "unknown",
    inboundSource: stringValue(inbound?.selectedSource),
    observedWallet: stringValue(row.observed_wallet) ?? "",
    copyWallet: stringValue(row.copy_wallet) ?? "",
    observedSignature: stringValue(row.observed_signature) ?? "",
    signature: stringValue(row.signature) ?? "",
    slot: numberValue(row.slot) ?? 0,
    confirmationSlot: numberValue(row.confirmation_slot),
    slotDelta: numberValue(row.slot_delta),
    targetTxIndex: numberValue(row.target_tx_index),
    copyTxIndex: numberValue(row.copy_tx_index),
    sameSlotTxDelta: numberValue(row.same_slot_tx_delta),
    txDelta: numberValue(row.tx_delta),
    targetSlotLeader: stringValue(row.target_slot_leader) ?? stringValue(rawConfirmation?.targetSlotLeader),
    copySlotLeader: stringValue(row.copy_slot_leader) ?? stringValue(rawConfirmation?.copySlotLeader),
    telegramId: stringValue(row.telegram_id) ?? stringValue(rawConfirmation?.telegramId),
    firstAckLane: ackLane,
    firstAckAtMs: numberValue(row.first_ack_at_ms)
      ?? numberValue(rawConfirmation?.firstAckAtMs)
      ?? numberValue(rawFirstAck?.observedAtUnixMs),
    dispatchAttempts: numberValue(row.dispatch_attempts),
    observedToSendSubmittedMs: numberValue(row.observed_to_send_submitted_ms)
      ?? numberValue(rawConfirmation?.observedToSendSubmittedMs),
    observedToSignatureReturnedMs: numberValue(row.observed_to_signature_returned_ms)
      ?? numberValue(rawConfirmation?.observedToSignatureReturnedMs),
    sendLaneAttribution: row.send_lane_attribution ?? null,
    rawConfirmation,
    selectedRoute: stringValue(row.selected_route),
    routeLayout: stringValue(row.route_layout),
    mint: stringValue(row.mint) ?? "",
    observedAction: stringValue(row.observed_action) ?? "unknown",
    transactionRole: stringValue(row.transaction_role) ?? "unknown",
    dispatchToAckMs: !durableAckEvidence
      ? null
      : numberValue(telemetry?.dispatchPersistenceStartedToAckMs),
    status: stringValue(row.status),
    ok: row.ok === true,
    confirmationStatus: stringValue(row.confirmation_status),
    reason: stringValue(row.reason),
    gatewayIntentKey: stringValue(row.gateway_intent_key),
    gatewayState: stringValue(row.gateway_state),
    reconciliationApplied: booleanValue(row.reconciliation_applied),
    leaderDiagnostics: null
  };
}

function filteredGatewayConfirmationQuery(filters: DashboardExecutionFilters) {
  let query = createAdminClient()
    .from("copytrade_gateway_confirmations")
    .select(GATEWAY_CONFIRMATION_SELECT)
    .gte("observed_at_ms", filters.fromObservedAtMs)
    .lte("observed_at_ms", filters.toObservedAtMs);

  if (filters.provider) query = query.eq("provider", filters.provider);
  if (filters.source) query = query.eq("source", filters.source);
  if (filters.wallet) query = query.or(`observed_wallet.eq.${filters.wallet},copy_wallet.eq.${filters.wallet}`);
  if (filters.mint) query = query.ilike("mint", `%${filters.mint}%`);
  if (filters.route) query = query.eq("selected_route", filters.route);
  if (filters.side) query = query.eq("observed_action", filters.side);
  if (filters.outcome === "landed") {
    query = query.eq("status", "landed").eq("ok", true);
  } else if (filters.outcome === "failed_on_chain") {
    query = query.eq("ok", false);
  } else if (filters.outcome) {
    query = query.eq("status", filters.outcome);
  }
  return query;
}

export async function listGatewayConfirmations(filters: DashboardExecutionFilters): Promise<GatewayConfirmation[]> {
  const { data, error } = await filteredGatewayConfirmationQuery(filters)
    .order("observed_at_ms", { ascending: false })
    .order("id", { ascending: false })
    .limit(filters.limit);

  if (missingTableError(error)) return [];
  if (error) throw error;
  return ((((data as unknown) as Array<Record<string, unknown>> | null) ?? [])).map(mapGatewayConfirmation);
}

export async function getGatewayConfirmationFreshness(): Promise<GatewayConfirmationFreshness> {
  const { data, error } = await createAdminClient()
    .from("copytrade_gateway_confirmations")
    .select("observed_at_ms,confirmation_at_ms,created_at")
    .order("observed_at_ms", { ascending: false })
    .order("id", { ascending: false })
    .limit(1);

  if (missingTableError(error)) {
    return { latestObservedAtMs: null, latestConfirmationAtMs: null, latestCreatedAt: null };
  }
  if (error) throw error;

  const latest = ((data as unknown) as Array<Record<string, unknown>> | null)?.[0];
  return {
    latestObservedAtMs: numberValue(latest?.observed_at_ms),
    latestConfirmationAtMs: numberValue(latest?.confirmation_at_ms),
    latestCreatedAt: stringValue(latest?.created_at)
  };
}
