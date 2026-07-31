import { createAdminClient } from "@/lib/supabase/admin";
import type { SignalFilters } from "@/lib/signals";

export interface BlockPositionDiagnostics {
  schema: string;
  status: string;
  targetSignature: string | null;
  copySignature: string | null;
  targetSlot: number | null;
  copySlot: number | null;
  slotDelta: number | null;
  targetTxIndex: number | null;
  copyTxIndex: number | null;
  sameSlotTxDelta: number | null;
  txDelta: number | null;
  crossSlotPositionSummary: CrossSlotPositionSummary | null;
  unavailableReason: string | null;
}

export interface CrossSlotPositionSummary {
  targetSlotTransactionCount?: number | null;
  copySlotTransactionCount?: number | null;
  targetTxIndex?: number | null;
  copyTxIndex?: number | null;
  targetSlotTransactionsAfterTarget?: number | null;
  intermediateSlotCount?: number | null;
  intermediateSlotTransactionCount?: number | null;
  copySlotTransactionsThroughCopy?: number | null;
  crossSlotTxDelta?: number | null;
}

export interface LocalExecutionReport {
  id: number;
  createdAt: string;
  observedAtMs: number;
  provider: string;
  source: string;
  endpoint: string | null;
  observedWallet: string;
  copyWallet: string | null;
  observedSignature: string;
  sendSignature: string | null;
  slot: number;
  copySlot: number | null;
  slotDeltaFromObserved: number | null;
  targetSlot: number | null;
  targetTxIndex: number | null;
  copyTxIndex: number | null;
  sameSlotTxDelta: number | null;
  slotDelta: number | null;
  txDelta: number | null;
  positionUnavailableReason: string | null;
  selectedRoute: string;
  routeLayout: string | null;
  mint: string;
  observedAction: string;
  observedSolAmount: number | null;
  maxCopySol: number | null;
  decision: string;
  buyStatus: string | null;
  buyChainError: unknown;
  reason: string | null;
  signed: boolean;
  simulated: boolean;
  sent: boolean;
  dryRun: boolean;
  sendEnabled: boolean;
  simulationRequested: boolean;
  instructionCount: number;
  simulationUnitsConsumed: number | null;
  fillTokenDelta: number | null;
  copyWalletSolDelta: number | null;
  grossCopySpendSol: number | null;
  networkFeeSol: number | null;
  extraSpendBeyondObservedSol: number | null;
  extraSpendBeyondObservedAndNetworkFeeSol: number | null;
  observedToSignedMs: number | null;
  observedToSimulationCompletedMs: number | null;
  observedToSendSubmittedMs: number | null;
  observedToSignatureReturnedMs: number | null;
  feedReceivedAtMs: number | null;
  decodedAtMs: number | null;
  matchedAtMs: number | null;
  plannedAtMs: number | null;
  builtAtMs: number | null;
  feedReceivedToDecodedUs: number | null;
  decodedToMatchedUs: number | null;
  matchedToPlannedMs: number | null;
  plannedToBuiltMs: number | null;
  executorQueueUs: number | null;
  guardsUs: number | null;
  unsignedBuildUs: number | null;
  signUs: number | null;
  serializeUs: number | null;
  batchTransactionCount: number | null;
  matchedTransactionIndex: number | null;
  batchScanUs: number | null;
  txParseUs: number | null;
  accountExpandUs: number | null;
  walletMatchUs: number | null;
  routeParseUs: number | null;
  sendLaneMs: number | null;
  sendLaneMode: string | null;
  firstAckLane: string | null;
  sendLaneAttempts: SendLaneAttempt[];
  feeProfileName: string | null;
  selectedPriorityFeeMicroLamports: number | null;
  selectedHeliusTipLamports: number | null;
  sourcePositionBucket: string | null;
  feeReason: string | null;
  feeCapHit: boolean;
  targetBlockTimeMs: number | null;
  autoSellEnabled: boolean;
  autoSellDelayMs: number | null;
  autoSellAttempted: boolean;
  autoSellSigned: boolean;
  autoSellSimulated: boolean;
  autoSellSent: boolean;
  autoSellDecision: string | null;
  autoSellStatus: string | null;
  autoSellChainError: unknown;
  autoSellSlot: number | null;
  autoSellReason: string | null;
  autoSellTokenAmountRaw: number | null;
  autoSellSendSignature: string | null;
  buySignatureToAutoSellSubmittedMs: number | null;
  buySignatureToAutoSellSignatureReturnedMs: number | null;
  blockPositionDiagnostics: BlockPositionDiagnostics | null;
  rawExecution: unknown;
  chainReport: unknown;
}

export interface SendLaneAttempt {
  label: string | null;
  kind: string | null;
  mode: string | null;
  beamProvider: string | null;
  status: string | null;
  durationMs: number | null;
  errorClass: string | null;
  providerTipLamports: number | null;
  fanoutSlots: number | null;
  timeoutMs: number | null;
}

export interface SendLaneAttribution {
  sendLaneMode: string | null;
  firstAckLane: string | null;
  firstAckAtMs: number | null;
  allAttempts: SendLaneAttempt[];
}

export interface DashboardExecutionCursor {
  observedAtMs: number;
  id: number;
}

export interface DashboardExecutionFilters {
  since: string;
  sinceObservedAtMs: number;
  limit: number;
  cursor: DashboardExecutionCursor | null;
  provider?: string | null;
  source?: string | null;
  observedWallet?: string | null;
  copyWallet?: string | null;
  mint?: string | null;
  route?: string | null;
  action?: string | null;
}

interface RawLocalExecutionReport {
  id: number;
  created_at: string;
  observed_at_ms: number;
  provider: string;
  source: string;
  endpoint: string | null;
  observed_wallet: string;
  copy_wallet: string | null;
  observed_signature: string;
  send_signature: string | null;
  slot: number;
  copy_slot: number | null;
  slot_delta_from_observed: number | null;
  target_slot: number | null;
  target_tx_index: number | null;
  copy_tx_index: number | null;
  same_slot_tx_delta: number | null;
  slot_delta: number | null;
  tx_delta: number | null;
  position_unavailable_reason: string | null;
  selected_route: string;
  route_layout: string | null;
  mint: string;
  observed_action: string;
  observed_sol_amount: number | null;
  max_copy_sol: number | null;
  decision: string;
  reason: string | null;
  signed: boolean;
  simulated: boolean;
  sent: boolean;
  dry_run: boolean;
  send_enabled: boolean;
  simulation_requested: boolean;
  instruction_count: number;
  simulation_units_consumed: number | null;
  fill_token_delta: number | null;
  copy_wallet_sol_delta: number | null;
  gross_copy_spend_sol: number | null;
  network_fee_sol: number | null;
  extra_spend_beyond_observed_sol: number | null;
  extra_spend_beyond_observed_and_network_fee_sol: number | null;
  observed_to_signed_ms: number | null;
  observed_to_simulation_completed_ms: number | null;
  observed_to_send_submitted_ms: number | null;
  observed_to_signature_returned_ms: number | null;
  feed_received_at_ms: number | null;
  decoded_at_ms: number | null;
  matched_at_ms: number | null;
  planned_at_ms: number | null;
  built_at_ms: number | null;
  feed_received_to_decoded_us: number | null;
  decoded_to_matched_us: number | null;
  matched_to_planned_ms: number | null;
  planned_to_built_ms: number | null;
  executor_queue_us: number | null;
  guards_us: number | null;
  unsigned_build_us: number | null;
  sign_us: number | null;
  serialize_us: number | null;
  batch_transaction_count: number | null;
  matched_transaction_index: number | null;
  batch_scan_us: number | null;
  tx_parse_us: number | null;
  account_expand_us: number | null;
  wallet_match_us: number | null;
  route_parse_us: number | null;
  send_lane_ms: number | null;
  fee_profile_name: string | null;
  selected_priority_fee_micro_lamports: number | null;
  selected_helius_tip_lamports: number | null;
  source_position_bucket: string | null;
  fee_reason: string | null;
  fee_cap_hit: boolean | null;
  auto_sell_enabled: boolean;
  auto_sell_delay_ms: number | null;
  auto_sell_attempted: boolean;
  auto_sell_signed: boolean;
  auto_sell_simulated: boolean;
  auto_sell_sent: boolean;
  auto_sell_decision: string | null;
  auto_sell_reason: string | null;
  auto_sell_token_amount_raw: number | null;
  auto_sell_send_signature: string | null;
  buy_signature_to_auto_sell_submitted_ms: number | null;
  buy_signature_to_auto_sell_signature_returned_ms: number | null;
  raw_execution: unknown;
  chain_report: unknown;
}

const LOCAL_EXECUTION_BASE_COLUMNS = [
  "id",
  "created_at",
  "observed_at_ms",
  "provider",
  "source",
  "endpoint",
  "observed_wallet",
  "copy_wallet",
  "observed_signature",
  "send_signature",
  "slot",
  "copy_slot",
  "slot_delta_from_observed",
  "target_slot",
  "target_tx_index",
  "copy_tx_index",
  "same_slot_tx_delta",
  "slot_delta",
  "tx_delta",
  "position_unavailable_reason",
  "selected_route",
  "route_layout",
  "mint",
  "observed_action",
  "observed_sol_amount",
  "max_copy_sol",
  "decision",
  "reason",
  "signed",
  "simulated",
  "sent",
  "dry_run",
  "send_enabled",
  "simulation_requested",
  "instruction_count",
  "simulation_units_consumed",
  "fill_token_delta",
  "copy_wallet_sol_delta",
  "gross_copy_spend_sol",
  "network_fee_sol",
  "extra_spend_beyond_observed_sol",
  "extra_spend_beyond_observed_and_network_fee_sol",
  "observed_to_signed_ms",
  "observed_to_simulation_completed_ms",
  "observed_to_send_submitted_ms",
  "observed_to_signature_returned_ms",
  "feed_received_at_ms",
  "decoded_at_ms",
  "matched_at_ms",
  "planned_at_ms",
  "built_at_ms",
  "feed_received_to_decoded_us",
  "decoded_to_matched_us",
  "matched_to_planned_ms",
  "planned_to_built_ms",
  "executor_queue_us",
  "guards_us",
  "unsigned_build_us",
  "sign_us",
  "serialize_us",
  "batch_transaction_count",
  "matched_transaction_index",
  "batch_scan_us",
  "tx_parse_us",
  "account_expand_us",
  "wallet_match_us",
  "route_parse_us",
  "send_lane_ms",
  "fee_profile_name",
  "selected_priority_fee_micro_lamports",
  "selected_helius_tip_lamports",
  "source_position_bucket",
  "fee_reason",
  "fee_cap_hit",
  "auto_sell_enabled",
  "auto_sell_delay_ms",
  "auto_sell_attempted",
  "auto_sell_signed",
  "auto_sell_simulated",
  "auto_sell_sent",
  "auto_sell_decision",
  "auto_sell_reason",
  "auto_sell_token_amount_raw",
  "auto_sell_send_signature",
  "buy_signature_to_auto_sell_submitted_ms",
  "buy_signature_to_auto_sell_signature_returned_ms"
];

const LOCAL_EXECUTION_SELECT = LOCAL_EXECUTION_BASE_COLUMNS.join(",");
const LOCAL_EXECUTION_DETAIL_SELECT = [
  ...LOCAL_EXECUTION_BASE_COLUMNS,
  "raw_execution",
  "chain_report"
].join(",");

function missingTableError(error: { message?: string; code?: string } | null): boolean {
  const message = error?.message?.toLowerCase() || "";
  return error?.code === "42P01" || message.includes("does not exist") || message.includes("schema cache");
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function numberValue(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function secondsToMs(value: unknown): number | null {
  const seconds = numberValue(value);
  return seconds === null ? null : seconds * 1000;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function arrayValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function chainReportValue(row: RawLocalExecutionReport): Record<string, unknown> | null {
  return objectValue(row.chain_report);
}

function submittedBuyStatus(row: RawLocalExecutionReport): string | null {
  return row.send_signature || row.sent || row.decision === "sent" ? "buySubmitted" : null;
}

function submittedAutoSellStatus(row: RawLocalExecutionReport): string | null {
  return row.auto_sell_send_signature || row.auto_sell_sent || row.auto_sell_decision === "sent"
    ? "autoSellSubmitted"
    : null;
}

function buyStatus(row: RawLocalExecutionReport): string | null {
  const report = chainReportValue(row);
  const status = stringValue(report?.buyStatus);
  if (status) {
    return status;
  }
  if (report?.err) {
    return "buyFailedOnChain";
  }
  if (numberValue(report?.slot) !== null || Number.isFinite(row.copy_slot)) {
    return "buyLanded";
  }
  return submittedBuyStatus(row);
}

function autoSellReport(row: RawLocalExecutionReport): Record<string, unknown> | null {
  return objectValue(chainReportValue(row)?.autoSell);
}

function autoSellStatus(row: RawLocalExecutionReport): string | null {
  const report = chainReportValue(row);
  const status = stringValue(report?.autoSellStatus);
  if (status) {
    return status;
  }
  const sellReport = autoSellReport(row);
  if (sellReport?.err) {
    return "autoSellFailedOnChain";
  }
  if (numberValue(sellReport?.slot) !== null) {
    return "autoSellLanded";
  }
  return submittedAutoSellStatus(row);
}

function firstNumberValue(...values: Array<unknown>): number | null {
  for (const value of values) {
    const number = numberValue(value);
    if (number !== null) {
      return number;
    }
  }
  return null;
}

function normalizeBlockPositionDiagnostics(row: RawLocalExecutionReport): BlockPositionDiagnostics | null {
  const report = objectValue(row.chain_report);
  const diagnostic = objectValue(report?.blockPositionDiagnostics);
  const hasFlatPosition =
    numberValue(row.target_slot) !== null ||
    numberValue(row.copy_slot) !== null ||
    numberValue(row.slot_delta) !== null ||
    numberValue(row.target_tx_index) !== null ||
    numberValue(row.copy_tx_index) !== null ||
    numberValue(row.same_slot_tx_delta) !== null ||
    numberValue(row.tx_delta) !== null ||
    Boolean(row.position_unavailable_reason);

  if (!diagnostic && !hasFlatPosition) {
    return null;
  }

  const targetTxIndex = firstNumberValue(diagnostic?.targetTxIndex, row.target_tx_index);
  const copyTxIndex = firstNumberValue(diagnostic?.copyTxIndex, row.copy_tx_index);
  const txDelta = firstNumberValue(diagnostic?.txDelta, row.tx_delta);
  const status =
    stringValue(diagnostic?.status) ||
    (targetTxIndex !== null && copyTxIndex !== null && txDelta !== null ? "found" : "unknown");

  return {
    schema: stringValue(diagnostic?.schema) || "copytrade.blockPositionDiagnostics.v1",
    status,
    targetSignature: stringValue(diagnostic?.targetSignature) || row.observed_signature,
    copySignature: stringValue(diagnostic?.copySignature) || row.send_signature,
    targetSlot: firstNumberValue(diagnostic?.targetSlot, row.target_slot, row.slot),
    copySlot: firstNumberValue(diagnostic?.copySlot, row.copy_slot),
    slotDelta: firstNumberValue(diagnostic?.slotDelta, row.slot_delta),
    targetTxIndex,
    copyTxIndex,
    sameSlotTxDelta: firstNumberValue(diagnostic?.sameSlotTxDelta, row.same_slot_tx_delta),
    txDelta,
    crossSlotPositionSummary: objectValue(diagnostic?.crossSlotPositionSummary),
    unavailableReason: stringValue(diagnostic?.unavailableReason) || row.position_unavailable_reason
  };
}

function normalizeSendLaneAttempt(value: unknown): SendLaneAttempt {
  const attempt = objectValue(value);
  return {
    label: stringValue(attempt?.label),
    kind: stringValue(attempt?.kind),
    mode: stringValue(attempt?.mode),
    beamProvider: stringValue(attempt?.beamProvider),
    status: stringValue(attempt?.status),
    durationMs: numberValue(attempt?.durationMs),
    errorClass: stringValue(attempt?.errorClass),
    providerTipLamports: numberValue(attempt?.providerTipLamports),
    fanoutSlots: numberValue(attempt?.fanoutSlots),
    timeoutMs: numberValue(attempt?.timeoutMs)
  };
}

function normalizeSendLaneAttribution(rawExecution: Record<string, unknown> | null): SendLaneAttribution | null {
  const attribution = objectValue(rawExecution?.sendLaneAttribution);
  if (!attribution) {
    return null;
  }

  return {
    sendLaneMode: stringValue(attribution.sendLaneMode),
    firstAckLane: stringValue(attribution.firstAckLane),
    firstAckAtMs: numberValue(attribution.firstAckAtMs),
    allAttempts: arrayValue(attribution.allAttempts).map(normalizeSendLaneAttempt)
  };
}

function normalizeReport(row: RawLocalExecutionReport): LocalExecutionReport {
  const rawExecution = objectValue(row.raw_execution);
  const chainReport = chainReportValue(row);
  const sendLaneAttribution = normalizeSendLaneAttribution(rawExecution);
  const rawNumber = (key: string) => numberValue(rawExecution?.[key]);
  const firstNumber = (...values: Array<unknown>): number | null => {
    for (const value of values) {
      const number = numberValue(value);
      if (number !== null) {
        return number;
      }
    }
    return null;
  };

  return {
    id: row.id,
    createdAt: row.created_at,
    observedAtMs: row.observed_at_ms,
    provider: row.provider,
    source: row.source,
    endpoint: row.endpoint,
    observedWallet: row.observed_wallet,
    copyWallet: row.copy_wallet,
    observedSignature: row.observed_signature,
    sendSignature: row.send_signature,
    slot: row.slot,
    copySlot: row.copy_slot,
    slotDeltaFromObserved: row.slot_delta_from_observed,
    targetSlot: row.target_slot,
    targetTxIndex: row.target_tx_index,
    copyTxIndex: row.copy_tx_index,
    sameSlotTxDelta: row.same_slot_tx_delta,
    slotDelta: row.slot_delta,
    txDelta: row.tx_delta,
    positionUnavailableReason: row.position_unavailable_reason,
    selectedRoute: row.selected_route,
    routeLayout: row.route_layout,
    mint: row.mint,
    observedAction: row.observed_action,
    observedSolAmount: row.observed_sol_amount,
    maxCopySol: row.max_copy_sol,
    decision: row.decision,
    buyStatus: buyStatus(row),
    buyChainError: chainReportValue(row)?.err ?? null,
    reason: row.reason,
    signed: row.signed,
    simulated: row.simulated,
    sent: row.sent,
    dryRun: row.dry_run,
    sendEnabled: row.send_enabled,
    simulationRequested: row.simulation_requested,
    instructionCount: row.instruction_count,
    simulationUnitsConsumed: row.simulation_units_consumed,
    fillTokenDelta: row.fill_token_delta,
    copyWalletSolDelta: row.copy_wallet_sol_delta,
    grossCopySpendSol: row.gross_copy_spend_sol,
    networkFeeSol: row.network_fee_sol,
    extraSpendBeyondObservedSol: row.extra_spend_beyond_observed_sol,
    extraSpendBeyondObservedAndNetworkFeeSol: row.extra_spend_beyond_observed_and_network_fee_sol,
    observedToSignedMs: row.observed_to_signed_ms,
    observedToSimulationCompletedMs: row.observed_to_simulation_completed_ms,
    observedToSendSubmittedMs: row.observed_to_send_submitted_ms,
    observedToSignatureReturnedMs: row.observed_to_signature_returned_ms,
    feedReceivedAtMs: firstNumber(row.feed_received_at_ms, rawNumber("feedReceivedAtMs")),
    decodedAtMs: firstNumber(row.decoded_at_ms, rawNumber("decodedAtMs")),
    matchedAtMs: firstNumber(row.matched_at_ms, rawNumber("matchedAtMs")),
    plannedAtMs: firstNumber(row.planned_at_ms, rawNumber("plannedAtMs")),
    builtAtMs: firstNumber(row.built_at_ms, rawNumber("builtAtMs")),
    feedReceivedToDecodedUs: firstNumber(row.feed_received_to_decoded_us, rawNumber("feedReceivedToDecodedUs")),
    decodedToMatchedUs: firstNumber(row.decoded_to_matched_us, rawNumber("decodedToMatchedUs")),
    matchedToPlannedMs: firstNumber(row.matched_to_planned_ms, rawNumber("matchedToPlannedMs")),
    plannedToBuiltMs: firstNumber(row.planned_to_built_ms, rawNumber("plannedToBuiltMs")),
    executorQueueUs: firstNumber(row.executor_queue_us, rawNumber("executorQueueUs")),
    guardsUs: firstNumber(row.guards_us, rawNumber("guardsUs")),
    unsignedBuildUs: firstNumber(row.unsigned_build_us, rawNumber("unsignedBuildUs")),
    signUs: firstNumber(row.sign_us, rawNumber("signUs")),
    serializeUs: firstNumber(row.serialize_us, rawNumber("serializeUs")),
    batchTransactionCount: firstNumber(row.batch_transaction_count, rawNumber("batchTransactionCount")),
    matchedTransactionIndex: firstNumber(row.matched_transaction_index, rawNumber("matchedTransactionIndex")),
    batchScanUs: firstNumber(row.batch_scan_us, rawNumber("batchScanUs")),
    txParseUs: firstNumber(row.tx_parse_us, rawNumber("txParseUs")),
    accountExpandUs: firstNumber(row.account_expand_us, rawNumber("accountExpandUs")),
    walletMatchUs: firstNumber(row.wallet_match_us, rawNumber("walletMatchUs")),
    routeParseUs: firstNumber(row.route_parse_us, rawNumber("routeParseUs")),
    sendLaneMs: firstNumber(row.send_lane_ms, rawNumber("sendLaneMs")),
    sendLaneMode: sendLaneAttribution?.sendLaneMode ?? stringValue(rawExecution?.sendLaneMode),
    firstAckLane: sendLaneAttribution?.firstAckLane ?? stringValue(rawExecution?.sendRpcWinner),
    sendLaneAttempts: sendLaneAttribution?.allAttempts ?? [],
    feeProfileName: row.fee_profile_name ?? stringValue(rawExecution?.feeProfileName),
    selectedPriorityFeeMicroLamports: firstNumber(
      row.selected_priority_fee_micro_lamports,
      rawNumber("selectedPriorityFeeMicroLamports")
    ),
    selectedHeliusTipLamports: firstNumber(
      row.selected_helius_tip_lamports,
      rawNumber("selectedHeliusTipLamports")
    ),
    sourcePositionBucket: row.source_position_bucket ?? stringValue(rawExecution?.sourcePositionBucket),
    feeReason: row.fee_reason ?? stringValue(rawExecution?.feeReason),
    feeCapHit: Boolean(row.fee_cap_hit ?? rawExecution?.feeCapHit),
    targetBlockTimeMs: firstNumber(secondsToMs(chainReport?.targetBlockTime)),
    autoSellEnabled: row.auto_sell_enabled,
    autoSellDelayMs: row.auto_sell_delay_ms,
    autoSellAttempted: row.auto_sell_attempted,
    autoSellSigned: row.auto_sell_signed,
    autoSellSimulated: row.auto_sell_simulated,
    autoSellSent: row.auto_sell_sent,
    autoSellDecision: row.auto_sell_decision,
    autoSellStatus: autoSellStatus(row),
    autoSellChainError: autoSellReport(row)?.err ?? null,
    autoSellSlot: numberValue(autoSellReport(row)?.slot),
    autoSellReason: row.auto_sell_reason,
    autoSellTokenAmountRaw: row.auto_sell_token_amount_raw,
    autoSellSendSignature: row.auto_sell_send_signature,
    buySignatureToAutoSellSubmittedMs: row.buy_signature_to_auto_sell_submitted_ms,
    buySignatureToAutoSellSignatureReturnedMs: row.buy_signature_to_auto_sell_signature_returned_ms,
    blockPositionDiagnostics: normalizeBlockPositionDiagnostics(row),
    rawExecution: row.raw_execution,
    chainReport: row.chain_report
  };
}

export async function listLocalExecutions(filters: SignalFilters): Promise<LocalExecutionReport[]> {
  const supabase = createAdminClient();
  let query = supabase
    .from("copytrade_local_executions")
    .select(LOCAL_EXECUTION_SELECT)
    .gte("created_at", filters.since)
    .order("observed_at_ms", { ascending: false })
    .limit(filters.limit);

  if (filters.provider) {
    query = query.eq("provider", filters.provider);
  }
  if (filters.targetWallet) {
    query = query.ilike("observed_wallet", `%${filters.targetWallet}%`);
  }
  if (filters.mint) {
    query = query.ilike("mint", `%${filters.mint}%`);
  }
  if (filters.action) {
    query = query.eq("observed_action", filters.action);
  }
  if (filters.route) {
    query = query.eq("selected_route", filters.route);
  }

  const { data, error } = await query;
  if (missingTableError(error)) {
    return [];
  }
  if (error) {
    throw error;
  }

  return (((data as unknown) as RawLocalExecutionReport[] | null) || []).map(normalizeReport);
}

function buildDashboardCursorWhere(cursor: DashboardExecutionCursor) {
  return `observed_at_ms.lt.${cursor.observedAtMs},and(observed_at_ms.eq.${cursor.observedAtMs},id.lt.${cursor.id})`;
}

export async function listDashboardExecutions(filters: DashboardExecutionFilters): Promise<LocalExecutionReport[]> {
  const supabase = createAdminClient();
  let query = supabase
    .from("copytrade_local_executions")
    .select(LOCAL_EXECUTION_SELECT)
    .gte("observed_at_ms", filters.sinceObservedAtMs)
    .order("observed_at_ms", { ascending: false })
    .order("id", { ascending: false })
    .limit(filters.limit);

  if (filters.provider) {
    query = query.eq("provider", filters.provider);
  }
  if (filters.source) {
    query = query.ilike("source", `%${filters.source}%`);
  }
  if (filters.observedWallet) {
    query = query.ilike("observed_wallet", `%${filters.observedWallet}%`);
  }
  if (filters.copyWallet) {
    query = query.ilike("copy_wallet", `%${filters.copyWallet}%`);
  }
  if (filters.mint) {
    query = query.ilike("mint", `%${filters.mint}%`);
  }
  if (filters.route) {
    query = query.eq("selected_route", filters.route);
  }
  if (filters.action) {
    query = query.eq("observed_action", filters.action);
  }

  if (filters.cursor) {
    query = query.or(buildDashboardCursorWhere(filters.cursor));
  }

  const { data, error } = await query;
  if (missingTableError(error)) {
    return [];
  }
  if (error) {
    throw error;
  }

  return (((data as unknown) as RawLocalExecutionReport[] | null) || []).map(normalizeReport);
}

export async function getLocalExecution(id: number): Promise<LocalExecutionReport | null> {
  const { data, error } = await createAdminClient()
    .from("copytrade_local_executions")
    .select(LOCAL_EXECUTION_DETAIL_SELECT)
    .eq("id", id)
    .maybeSingle();

  if (missingTableError(error)) {
    return null;
  }
  if (error) {
    throw error;
  }

  return data ? normalizeReport(data as unknown as RawLocalExecutionReport) : null;
}

export function summarizeLocalExecutions(rows: LocalExecutionReport[]) {
  const sentRows = rows.filter((row) => row.sent);
  const landedRows = rows.filter((row) => row.buyStatus === "buyLanded");
  return {
    total: rows.length,
    sent: sentRows.length,
    landed: landedRows.length,
    failedOnChain: rows.filter((row) => row.buyStatus === "buyFailedOnChain").length,
    autoSellLanded: rows.filter((row) => row.autoSellStatus === "autoSellLanded").length,
    autoSellFailedOnChain: rows.filter((row) => row.autoSellStatus === "autoSellFailedOnChain").length,
    skipped: rows.filter((row) => row.decision === "skip").length,
    errors: rows.filter((row) => row.decision === "error").length,
    avgSignatureMs: average(landedRows.map((row) => row.observedToSignatureReturnedMs)),
    avgSlotDelta: average(landedRows.map((row) => row.slotDeltaFromObserved)),
    totalGrossSpendSol: sum(landedRows.map((row) => row.grossCopySpendSol)),
    totalExtraSpendSol: sum(landedRows.map((row) => row.extraSpendBeyondObservedAndNetworkFeeSol))
  };
}

function average(values: Array<number | null>): number | null {
  const numeric = values.filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  if (numeric.length === 0) {
    return null;
  }
  return Math.round(numeric.reduce((total, value) => total + value, 0) / numeric.length);
}

function sum(values: Array<number | null>): number | null {
  const numeric = values.filter((value): value is number => typeof value === "number" && Number.isFinite(value));
  if (numeric.length === 0) {
    return null;
  }
  return numeric.reduce((total, value) => total + value, 0);
}
