#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const DEFAULT_EXECUTIONS_PATH = "/tmp/jito-copy-executions-local-send.jsonl";
const DEFAULT_SUPABASE_CWD = `${process.env.HOME || ""}/Documents/pumpfunnoti`;
const DEFAULT_WATCH_INTERVAL_MS = 1000;
const DEFAULT_REFRESH_INTERVAL_MS = 5000;
const DEFAULT_REFRESH_RECENT_LIMIT = 1;
const DEFAULT_NEW_ROW_BACKFILL = 0;
const confirmedTransactionCache = new Map();
const blockSignatureCache = new Map();

function argValue(name, fallback = null) {
  const prefix = `--${name}=`;
  const match = process.argv.find((arg) => arg.startsWith(prefix));
  return match ? match.slice(prefix.length) : fallback;
}

function hasFlag(name) {
  return process.argv.includes(`--${name}`);
}

function supabaseCwd() {
  const configured = process.env.JITO_SUPABASE_CWD || argValue("supabase-cwd");
  if (configured) {
    return configured;
  }
  if (existsSync("supabase/.temp/project-ref")) {
    return process.cwd();
  }
  if (DEFAULT_SUPABASE_CWD && existsSync(`${DEFAULT_SUPABASE_CWD}/supabase/.temp/project-ref`)) {
    return DEFAULT_SUPABASE_CWD;
  }
  return process.cwd();
}

function positiveInteger(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? Math.floor(number) : fallback;
}

function nonNegativeInteger(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? Math.floor(number) : fallback;
}

function boolish(value, fallback = false) {
  if (value === undefined || value === null || value === "") {
    return fallback;
  }
  return ["1", "true", "yes", "y", "on"].includes(String(value).trim().toLowerCase());
}

function readJsonl(path, { recentLimit = 0 } = {}) {
  if (!existsSync(path)) {
    return [];
  }
  const lines = readFileSync(path, "utf8")
    .split(/\r?\n/)
    .filter((line) => line.trim().length > 0);
  const selectedLines = recentLimit > 0 ? lines.slice(-recentLimit) : lines;
  return selectedLines
    .map((line, index) => {
      try {
        return JSON.parse(line);
      } catch (error) {
        throw new Error(`invalid JSONL at ${path}:${index + 1}: ${error.message}`);
      }
    })
    .filter((row) => row.schema === "copytrade.localExecution.v1");
}

function sqlString(value) {
  if (value === null || value === undefined) {
    return "null";
  }
  return `'${String(value).replace(/'/g, "''")}'`;
}

function sqlNumber(value) {
  return Number.isFinite(value) ? String(value) : "null";
}

function sqlBoolean(value) {
  return value ? "true" : "false";
}

function sqlJson(value) {
  return `${sqlString(JSON.stringify(value ?? null))}::jsonb`;
}

async function rpc(method, params) {
  if (!process.env.SOLANA_RPC_URL) {
    return null;
  }
  const response = await fetch(process.env.SOLANA_RPC_URL, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params })
  });
  const body = await response.json();
  if (body.error) {
    throw new Error(`${method} RPC error: ${JSON.stringify(body.error)}`);
  }
  return body.result;
}

async function confirmedTransaction(signature) {
  if (!signature) {
    return null;
  }
  if (confirmedTransactionCache.has(signature)) {
    return confirmedTransactionCache.get(signature);
  }
  const transaction = await rpc("getTransaction", [
    signature,
    {
      encoding: "jsonParsed",
      commitment: "confirmed",
      maxSupportedTransactionVersion: 0
    }
  ]);
  if (transaction) {
    confirmedTransactionCache.set(signature, transaction);
  }
  return transaction;
}

function signatureFromBlockTransaction(transaction) {
  if (typeof transaction === "string") {
    return transaction;
  }
  const firstSignature = transaction?.transaction?.signatures?.[0] ?? transaction?.signatures?.[0];
  return typeof firstSignature === "string" ? firstSignature : null;
}

function blockSignatures(block) {
  if (Array.isArray(block?.signatures)) {
    return block.signatures.filter((signature) => typeof signature === "string");
  }
  if (Array.isArray(block?.transactions)) {
    return block.transactions.map(signatureFromBlockTransaction).filter((signature) => typeof signature === "string");
  }
  return null;
}

async function fetchBlockSignatures(slot, rpcFn = rpc) {
  if (!Number.isFinite(slot)) {
    return { signatures: null, unavailableReason: "missing slot" };
  }
  if (rpcFn === rpc && blockSignatureCache.has(slot)) {
    return { signatures: blockSignatureCache.get(slot), unavailableReason: null };
  }

  try {
    const block = await rpcFn("getBlock", [
      slot,
      {
        commitment: "confirmed",
        transactionDetails: "signatures",
        rewards: false,
        maxSupportedTransactionVersion: 0
      }
    ]);
    const signatures = blockSignatures(block);
    if (!signatures) {
      return { signatures: null, unavailableReason: "block signatures unavailable" };
    }
    if (rpcFn === rpc) {
      blockSignatureCache.set(slot, signatures);
    }
    return { signatures, unavailableReason: null };
  } catch (error) {
    return { signatures: null, unavailableReason: `getBlock failed: ${error.message}` };
  }
}

function baseBlockPositionDiagnostics(row, copyTransaction) {
  const targetSlot = Number.isFinite(row.slot) ? row.slot : null;
  const copySlot = Number.isFinite(copyTransaction?.slot) ? copyTransaction.slot : null;
  const slotDelta =
    targetSlot !== null && copySlot !== null ? copySlot - targetSlot : null;

  return {
    schema: "copytrade.blockPositionDiagnostics.v1",
    status: "unknown",
    targetSignature: row.observedSignature ?? null,
    copySignature: row.sendSignature ?? null,
    targetSlot,
    copySlot,
    slotDelta,
    targetTxIndex: null,
    copyTxIndex: null,
    sameSlotTxDelta: null,
    crossSlotPositionSummary: null,
    unavailableReason: null
  };
}

async function blockPositionDiagnostics(row, copyTransaction, rpcFn = rpc) {
  const diagnostics = baseBlockPositionDiagnostics(row, copyTransaction);

  if (!diagnostics.targetSignature || !diagnostics.copySignature) {
    diagnostics.unavailableReason = "missing target or copy signature";
    return diagnostics;
  }
  if (!Number.isFinite(diagnostics.targetSlot) || !Number.isFinite(diagnostics.copySlot)) {
    diagnostics.unavailableReason = "missing target or copy slot";
    return diagnostics;
  }

  const targetBlock = await fetchBlockSignatures(diagnostics.targetSlot, rpcFn);
  if (!targetBlock.signatures) {
    diagnostics.unavailableReason = `target block unavailable: ${targetBlock.unavailableReason}`;
    return diagnostics;
  }

  diagnostics.targetTxIndex = targetBlock.signatures.indexOf(diagnostics.targetSignature);
  if (diagnostics.targetTxIndex < 0) {
    diagnostics.targetTxIndex = null;
    diagnostics.unavailableReason = "target signature not found in confirmed block";
    return diagnostics;
  }

  const copyBlock =
    diagnostics.copySlot === diagnostics.targetSlot
      ? targetBlock
      : await fetchBlockSignatures(diagnostics.copySlot, rpcFn);
  if (!copyBlock.signatures) {
    diagnostics.unavailableReason = `copy block unavailable: ${copyBlock.unavailableReason}`;
    return diagnostics;
  }

  diagnostics.copyTxIndex = copyBlock.signatures.indexOf(diagnostics.copySignature);
  if (diagnostics.copyTxIndex < 0) {
    diagnostics.copyTxIndex = null;
    diagnostics.unavailableReason = "copy signature not found in confirmed block";
    return diagnostics;
  }

  diagnostics.status = "found";
  if (diagnostics.slotDelta === 0) {
    diagnostics.sameSlotTxDelta = diagnostics.copyTxIndex - diagnostics.targetTxIndex;
  } else if (diagnostics.slotDelta > 0) {
    let intermediateSlotTransactionCount = 0;
    const intermediateSlots = [];
    for (let slot = diagnostics.targetSlot + 1; slot < diagnostics.copySlot; slot += 1) {
      const intermediateBlock = await fetchBlockSignatures(slot, rpcFn);
      if (!intermediateBlock.signatures) {
        diagnostics.unavailableReason = `intermediate block ${slot} unavailable: ${intermediateBlock.unavailableReason}`;
        return diagnostics;
      }
      intermediateSlots.push({
        slot,
        transactionCount: intermediateBlock.signatures.length
      });
      intermediateSlotTransactionCount += intermediateBlock.signatures.length;
    }
    const targetSlotTransactionsAfterTarget =
      targetBlock.signatures.length - diagnostics.targetTxIndex - 1;
    const copySlotTransactionsThroughCopy = diagnostics.copyTxIndex + 1;
    const crossSlotTxDelta =
      targetSlotTransactionsAfterTarget +
      intermediateSlotTransactionCount +
      copySlotTransactionsThroughCopy;
    diagnostics.crossSlotPositionSummary = {
      targetSlotTransactionCount: targetBlock.signatures.length,
      copySlotTransactionCount: copyBlock.signatures.length,
      targetTxIndex: diagnostics.targetTxIndex,
      copyTxIndex: diagnostics.copyTxIndex,
      targetSlotTransactionsAfterTarget,
      intermediateSlotCount: intermediateSlots.length,
      intermediateSlotTransactionCount,
      intermediateSlots,
      copySlotTransactionsThroughCopy,
      crossSlotTxDelta
    };
  }

  return diagnostics;
}

function unknownChainReport(row, unavailableReason) {
  const diagnostics = baseBlockPositionDiagnostics(row, null);
  diagnostics.unavailableReason = unavailableReason;
  return {
    status: "unknown",
    slot: null,
    slotDeltaFromObserved: null,
    blockPositionDiagnostics: diagnostics,
    targetSlot: diagnostics.targetSlot,
    copySlot: diagnostics.copySlot,
    slotDelta: diagnostics.slotDelta,
    targetTxIndex: diagnostics.targetTxIndex,
    copyTxIndex: diagnostics.copyTxIndex,
    sameSlotTxDelta: diagnostics.sameSlotTxDelta,
    crossSlotPositionSummary: diagnostics.crossSlotPositionSummary,
    positionUnavailableReason: diagnostics.unavailableReason,
    fillTokenDelta: null,
    copyWalletSolDelta: null,
    grossCopySpendSol: null,
    networkFeeSol: null,
    extraSpendBeyondObservedSol: null,
    extraSpendBeyondObservedAndNetworkFeeSol: null,
    err: null,
    blockTime: null
  };
}

function submittedChainReport(signature, unavailableReason) {
  return {
    status: "submitted",
    signature: signature ?? null,
    slot: null,
    err: null,
    blockTime: null,
    unavailableReason
  };
}

async function transactionChainReport(signature) {
  if (!signature) {
    return submittedChainReport(null, "missing transaction signature");
  }
  let transaction;
  try {
    transaction = await confirmedTransaction(signature);
  } catch (error) {
    return submittedChainReport(signature, `getTransaction failed: ${error.message}`);
  }
  if (!transaction) {
    return submittedChainReport(signature, "transaction not found at confirmed commitment");
  }

  return {
    status: transaction.meta?.err ? "failedOnChain" : "landed",
    signature,
    slot: transaction.slot,
    err: transaction.meta?.err ?? null,
    blockTime: transaction.blockTime,
    unavailableReason: null,
    transaction
  };
}

function buyStatus(row, report) {
  if (report?.err) {
    return "buyFailedOnChain";
  }
  if (Number.isFinite(report?.slot)) {
    return "buyLanded";
  }
  if (row.sendSignature || row.sent || row.decision === "sent") {
    return "buySubmitted";
  }
  return null;
}

function autoSellStatus(row, report) {
  if (report?.err) {
    return "autoSellFailedOnChain";
  }
  if (Number.isFinite(report?.slot)) {
    return "autoSellLanded";
  }
  if (row.autoSellSendSignature || row.autoSellSent || row.autoSellDecision === "sent") {
    return "autoSellSubmitted";
  }
  return null;
}

function uiAmount(balance) {
  const amount = Number(balance?.uiTokenAmount?.uiAmountString ?? balance?.uiTokenAmount?.uiAmount);
  return Number.isFinite(amount) ? amount : 0;
}

function tokenDelta(transaction, owner, mint) {
  const byIndex = new Map();
  for (const balance of transaction?.meta?.preTokenBalances ?? []) {
    if (balance.owner === owner && balance.mint === mint) {
      byIndex.set(balance.accountIndex, { pre: uiAmount(balance), post: 0 });
    }
  }
  for (const balance of transaction?.meta?.postTokenBalances ?? []) {
    if (balance.owner === owner && balance.mint === mint) {
      const current = byIndex.get(balance.accountIndex) ?? { pre: 0, post: 0 };
      current.post = uiAmount(balance);
      byIndex.set(balance.accountIndex, current);
    }
  }

  let delta = 0;
  for (const value of byIndex.values()) {
    delta += value.post - value.pre;
  }
  return delta;
}

function solDelta(transaction, account) {
  const keys = transaction?.transaction?.message?.accountKeys ?? [];
  const index = keys.findIndex((key) => (typeof key === "string" ? key : key.pubkey) === account);
  if (index < 0) {
    return null;
  }
  const pre = transaction?.meta?.preBalances?.[index];
  const post = transaction?.meta?.postBalances?.[index];
  if (!Number.isFinite(pre) || !Number.isFinite(post)) {
    return null;
  }
  return (post - pre) / 1_000_000_000;
}

function positiveOrNull(value) {
  return Number.isFinite(value) ? Math.max(0, value) : null;
}

function executionKey(row) {
  return [
    row.provider,
    row.observedSignature,
    row.observedWallet,
    row.observedAction,
    row.mint
  ].join("\u0000");
}

function dedupeRows(rows) {
  const byKey = new Map();
  for (const row of rows) {
    byKey.set(executionKey(row), row);
  }
  return [...byKey.values()];
}

async function chainReport(row) {
  if (!row.sendSignature) {
    const report = unknownChainReport(row, "missing copy send signature");
    report.buyStatus = buyStatus(row, report);
    report.autoSellStatus = autoSellStatus(row, null);
    report.autoSell = row.autoSellSendSignature
      ? submittedChainReport(row.autoSellSendSignature, "copy transaction missing; auto-sell not checked")
      : null;
    return report;
  }
  let transaction;
  try {
    transaction = await confirmedTransaction(row.sendSignature);
  } catch (error) {
    return unknownChainReport(row, `getTransaction failed: ${error.message}`);
  }
  if (!transaction) {
    return unknownChainReport(row, "copy transaction not found at confirmed commitment");
  }

  const copyWalletSolDelta = solDelta(transaction, row.copyWallet);
  const grossCopySpendSol = Number.isFinite(copyWalletSolDelta)
    ? Math.abs(Math.min(copyWalletSolDelta, 0))
    : null;
  const intendedCopySpendSol = Number.isFinite(row.observedSolAmount) ? row.observedSolAmount : null;
  const networkFeeSol = Number.isFinite(transaction.meta?.fee) ? transaction.meta.fee / 1_000_000_000 : null;
  const extraSpendBeyondObservedSol =
    grossCopySpendSol !== null && intendedCopySpendSol !== null
      ? positiveOrNull(grossCopySpendSol - intendedCopySpendSol)
      : null;
  const extraSpendBeyondObservedAndNetworkFeeSol =
    extraSpendBeyondObservedSol !== null && networkFeeSol !== null
      ? positiveOrNull(extraSpendBeyondObservedSol - networkFeeSol)
      : null;
  const positionDiagnostics = await blockPositionDiagnostics(row, transaction);
  const autoSellReport = row.autoSellSendSignature
    ? await transactionChainReport(row.autoSellSendSignature)
    : null;

  const report = {
    status: transaction.meta?.err ? "failedOnChain" : "landed",
    slot: transaction.slot,
    slotDeltaFromObserved: Number.isFinite(transaction.slot) && Number.isFinite(row.slot) ? transaction.slot - row.slot : null,
    blockPositionDiagnostics: positionDiagnostics,
    targetSlot: positionDiagnostics.targetSlot,
    copySlot: positionDiagnostics.copySlot,
    slotDelta: positionDiagnostics.slotDelta,
    targetTxIndex: positionDiagnostics.targetTxIndex,
    copyTxIndex: positionDiagnostics.copyTxIndex,
    sameSlotTxDelta: positionDiagnostics.sameSlotTxDelta,
    crossSlotPositionSummary: positionDiagnostics.crossSlotPositionSummary,
    positionUnavailableReason: positionDiagnostics.unavailableReason,
    fillTokenDelta: tokenDelta(transaction, row.copyWallet, row.mint),
    copyWalletSolDelta,
    grossCopySpendSol,
    networkFeeSol,
    extraSpendBeyondObservedSol,
    extraSpendBeyondObservedAndNetworkFeeSol,
    err: transaction.meta?.err ?? null,
    blockTime: transaction.blockTime,
    autoSell: autoSellReport
      ? {
          status: autoSellReport.status,
          signature: autoSellReport.signature,
          slot: autoSellReport.slot,
          err: autoSellReport.err,
          blockTime: autoSellReport.blockTime,
          unavailableReason: autoSellReport.unavailableReason
        }
      : null
  };
  report.buyStatus = buyStatus(row, report);
  report.autoSellStatus = autoSellStatus(row, autoSellReport);
  return report;
}

async function buildRestRows(rows) {
  const records = [];
  for (const row of rows) {
    const report = await chainReport(row);
    const chain = report
      ? {
          ...report,
          sendSignature: row.sendSignature,
          observedSignature: row.observedSignature,
          copyWallet: row.copyWallet,
          mint: row.mint,
          intendedCopySpendSol: row.observedSolAmount ?? null,
          maxCopySol: row.maxCopySol ?? null
        }
      : {};

    records.push({
      observed_at_ms: Number.isFinite(row.observedAtMs) ? row.observedAtMs : null,
      execution_at_ms: Number.isFinite(row.executionAtMs) ? row.executionAtMs : null,
      provider: row.provider ?? null,
      source: row.source ?? null,
      endpoint: row.endpoint ?? null,
      observed_wallet: row.observedWallet ?? null,
      copy_wallet: row.copyWallet ?? null,
      observed_signature: row.observedSignature ?? null,
      send_signature: row.sendSignature ?? null,
      slot: Number.isFinite(row.slot) ? row.slot : null,
      copy_slot: report?.slot ?? null,
      slot_delta_from_observed: report?.slotDeltaFromObserved ?? null,
      target_slot: report?.targetSlot ?? null,
      target_tx_index: report?.targetTxIndex ?? null,
      copy_tx_index: report?.copyTxIndex ?? null,
      same_slot_tx_delta: report?.sameSlotTxDelta ?? null,
      position_unavailable_reason: report?.positionUnavailableReason ?? null,
      selected_route: row.selectedRoute ?? null,
      route_layout: row.routeLayout ?? null,
      mint: row.mint ?? null,
      observed_action: row.observedAction ?? null,
      observed_sol_amount: Number.isFinite(row.observedSolAmount) ? row.observedSolAmount : null,
      max_copy_sol: Number.isFinite(row.maxCopySol) ? row.maxCopySol : null,
      decision: row.decision ?? null,
      reason: row.reason ?? null,
      signed: Boolean(row.signed),
      simulated: Boolean(row.simulated),
      sent: Boolean(row.sent),
      dry_run: Boolean(row.dryRun),
      send_enabled: Boolean(row.sendEnabled),
      send_rpc_winner: row.sendRpcWinner ?? null,
      send_rpc_url_count: Number.isFinite(row.sendRpcUrlCount) ? row.sendRpcUrlCount : null,
      send_rpc_errors: row.sendRpcErrors ?? [],
      simulation_requested: Boolean(row.simulationRequested),
      instruction_count: Number.isFinite(row.instructionCount) ? row.instructionCount : 0,
      simulation_units_consumed: Number.isFinite(row.simulationUnitsConsumed)
        ? row.simulationUnitsConsumed
        : null,
      fill_token_delta: report?.fillTokenDelta ?? null,
      copy_wallet_sol_delta: report?.copyWalletSolDelta ?? null,
      gross_copy_spend_sol: report?.grossCopySpendSol ?? null,
      network_fee_sol: report?.networkFeeSol ?? null,
      extra_spend_beyond_observed_sol: report?.extraSpendBeyondObservedSol ?? null,
      extra_spend_beyond_observed_and_network_fee_sol:
        report?.extraSpendBeyondObservedAndNetworkFeeSol ?? null,
      observed_to_signed_ms: Number.isFinite(row.observedToSignedMs) ? row.observedToSignedMs : null,
      observed_to_simulation_completed_ms: Number.isFinite(row.observedToSimulationCompletedMs)
        ? row.observedToSimulationCompletedMs
        : null,
      observed_to_send_submitted_ms: Number.isFinite(row.observedToSendSubmittedMs)
        ? row.observedToSendSubmittedMs
        : null,
      observed_to_signature_returned_ms: Number.isFinite(row.observedToSignatureReturnedMs)
        ? row.observedToSignatureReturnedMs
        : null,
      auto_sell_enabled: Boolean(row.autoSellEnabled),
      auto_sell_delay_ms: Number.isFinite(row.autoSellDelayMs) ? row.autoSellDelayMs : null,
      auto_sell_attempted: Boolean(row.autoSellAttempted),
      auto_sell_signed: Boolean(row.autoSellSigned),
      auto_sell_simulated: Boolean(row.autoSellSimulated),
      auto_sell_sent: Boolean(row.autoSellSent),
      auto_sell_decision: row.autoSellDecision ?? null,
      auto_sell_reason: row.autoSellReason ?? null,
      auto_sell_token_amount_raw: Number.isFinite(row.autoSellTokenAmountRaw)
        ? row.autoSellTokenAmountRaw
        : null,
      auto_sell_send_signature: row.autoSellSendSignature ?? null,
      auto_sell_send_rpc_winner: row.autoSellSendRpcWinner ?? null,
      auto_sell_send_rpc_url_count: Number.isFinite(row.autoSellSendRpcUrlCount)
        ? row.autoSellSendRpcUrlCount
        : null,
      auto_sell_send_rpc_errors: row.autoSellSendRpcErrors ?? [],
      buy_signature_to_auto_sell_submitted_ms: Number.isFinite(row.buySignatureToAutoSellSubmittedMs)
        ? row.buySignatureToAutoSellSubmittedMs
        : null,
      buy_signature_to_auto_sell_signature_returned_ms: Number.isFinite(
        row.buySignatureToAutoSellSignatureReturnedMs
      )
        ? row.buySignatureToAutoSellSignatureReturnedMs
        : null,
      raw_execution: row,
      chain_report: chain
    });
  }

  return records;
}

function hasSupabaseRestEnv() {
  return Boolean(process.env.SUPABASE_URL && process.env.SUPABASE_SERVICE_ROLE_KEY);
}

async function syncViaSupabaseRest(records) {
  if (records.length === 0) {
    return;
  }

  const base = process.env.SUPABASE_URL.trim().replace(/\/+$/, "");
  const response = await fetch(
    `${base}/rest/v1/copytrade_local_executions?on_conflict=provider,observed_signature,observed_wallet,observed_action,mint`,
    {
      method: "POST",
      headers: {
        apikey: process.env.SUPABASE_SERVICE_ROLE_KEY,
        authorization: `Bearer ${process.env.SUPABASE_SERVICE_ROLE_KEY}`,
        "content-type": "application/json",
        prefer: "resolution=merge-duplicates,return=minimal"
      },
      body: JSON.stringify(records)
    }
  );

  if (!response.ok) {
    throw new Error(`Supabase REST upsert failed: ${response.status} ${await response.text()}`);
  }
}

async function buildSql(rows) {
  const columns = [
    "observed_at_ms",
    "execution_at_ms",
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
    "send_rpc_winner",
    "send_rpc_url_count",
    "send_rpc_errors",
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
    "auto_sell_send_rpc_winner",
    "auto_sell_send_rpc_url_count",
    "auto_sell_send_rpc_errors",
    "buy_signature_to_auto_sell_submitted_ms",
    "buy_signature_to_auto_sell_signature_returned_ms",
    "raw_execution",
    "chain_report"
  ];

  const values = [];
  for (const row of rows) {
    const report = await chainReport(row);
    const chain = report
      ? {
          ...report,
          sendSignature: row.sendSignature,
          observedSignature: row.observedSignature,
          copyWallet: row.copyWallet,
          mint: row.mint,
          intendedCopySpendSol: row.observedSolAmount ?? null,
          maxCopySol: row.maxCopySol ?? null
        }
      : {};

    values.push(`(${[
      sqlNumber(row.observedAtMs),
      sqlNumber(row.executionAtMs),
      sqlString(row.provider),
      sqlString(row.source),
      sqlString(row.endpoint),
      sqlString(row.observedWallet),
      sqlString(row.copyWallet),
      sqlString(row.observedSignature),
      sqlString(row.sendSignature),
      sqlNumber(row.slot),
      sqlNumber(report?.slot),
      sqlNumber(report?.slotDeltaFromObserved),
      sqlNumber(report?.targetSlot),
      sqlNumber(report?.targetTxIndex),
      sqlNumber(report?.copyTxIndex),
      sqlNumber(report?.sameSlotTxDelta),
      sqlString(report?.positionUnavailableReason),
      sqlString(row.selectedRoute),
      sqlString(row.routeLayout),
      sqlString(row.mint),
      sqlString(row.observedAction),
      sqlNumber(row.observedSolAmount),
      sqlNumber(row.maxCopySol),
      sqlString(row.decision),
      sqlString(row.reason),
      sqlBoolean(row.signed),
      sqlBoolean(row.simulated),
      sqlBoolean(row.sent),
      sqlBoolean(row.dryRun),
      sqlBoolean(row.sendEnabled),
      sqlString(row.sendRpcWinner),
      sqlNumber(row.sendRpcUrlCount),
      sqlJson(row.sendRpcErrors ?? []),
      sqlBoolean(row.simulationRequested),
      sqlNumber(row.instructionCount ?? 0),
      sqlNumber(row.simulationUnitsConsumed),
      sqlNumber(report?.fillTokenDelta),
      sqlNumber(report?.copyWalletSolDelta),
      sqlNumber(report?.grossCopySpendSol),
      sqlNumber(report?.networkFeeSol),
      sqlNumber(report?.extraSpendBeyondObservedSol),
      sqlNumber(report?.extraSpendBeyondObservedAndNetworkFeeSol),
      sqlNumber(row.observedToSignedMs),
      sqlNumber(row.observedToSimulationCompletedMs),
      sqlNumber(row.observedToSendSubmittedMs),
      sqlNumber(row.observedToSignatureReturnedMs),
      sqlBoolean(row.autoSellEnabled),
      sqlNumber(row.autoSellDelayMs),
      sqlBoolean(row.autoSellAttempted),
      sqlBoolean(row.autoSellSigned),
      sqlBoolean(row.autoSellSimulated),
      sqlBoolean(row.autoSellSent),
      sqlString(row.autoSellDecision),
      sqlString(row.autoSellReason),
      sqlNumber(row.autoSellTokenAmountRaw),
      sqlString(row.autoSellSendSignature),
      sqlString(row.autoSellSendRpcWinner),
      sqlNumber(row.autoSellSendRpcUrlCount),
      sqlJson(row.autoSellSendRpcErrors ?? []),
      sqlNumber(row.buySignatureToAutoSellSubmittedMs),
      sqlNumber(row.buySignatureToAutoSellSignatureReturnedMs),
      `${sqlString(JSON.stringify(row))}::jsonb`,
      `${sqlString(JSON.stringify(chain))}::jsonb`
    ].join(",")})`);
  }

  const updates = columns
    .filter((column) => !["provider", "observed_signature", "observed_wallet", "observed_action", "mint"].includes(column))
    .map((column) => `${column}=excluded.${column}`)
    .join(",");

  return `insert into public.copytrade_local_executions (${columns.join(",")}) values ${values.join(",")} on conflict (provider, observed_signature, observed_wallet, observed_action, mint) do update set ${updates};`;
}

async function syncOnce(path, { recentLimit = 0 } = {}) {
  const rawRows = readJsonl(path, { recentLimit });
  const rows = dedupeRows(rawRows);
  if (rows.length === 0) {
    return 0;
  }

  if (hasSupabaseRestEnv()) {
    const records = await buildRestRows(rows);
    await syncViaSupabaseRest(records);
    return rows.length;
  }

  const sql = await buildSql(rows);
  const result = spawnSync("supabase", ["db", "query", "--linked", sql], {
    cwd: supabaseCwd(),
    env: { ...process.env, SUPABASE_TELEMETRY_DISABLED: "1" },
    encoding: "utf8"
  });

  if (result.status !== 0) {
    throw new Error((result.stderr || result.stdout || "supabase db query failed").trim());
  }
  return rows.length;
}

function syncLimitForCycle({
  hasNewRows,
  rowCount,
  lastSyncedCount,
  recentLimit,
  refreshRecentLimit,
  newRowBackfill
}) {
  if (!hasNewRows) {
    return refreshRecentLimit;
  }
  if (lastSyncedCount < 0 || rowCount <= lastSyncedCount) {
    return recentLimit;
  }
  const newRows = rowCount - lastSyncedCount;
  return Math.min(recentLimit, Math.max(1, newRows + newRowBackfill));
}

async function main() {
  const path = argValue("executions", process.env.JITO_COPY_EXECUTIONS_PATH || DEFAULT_EXECUTIONS_PATH);
  const watch = hasFlag("watch");
  const intervalMs = positiveInteger(
    argValue("interval-ms", String(DEFAULT_WATCH_INTERVAL_MS)),
    DEFAULT_WATCH_INTERVAL_MS
  );
  const refreshIntervalMs = positiveInteger(
    argValue(
      "refresh-interval-ms",
      process.env.JITO_SYNC_REFRESH_INTERVAL_MS || String(DEFAULT_REFRESH_INTERVAL_MS)
    ),
    DEFAULT_REFRESH_INTERVAL_MS
  );
  const recentLimit = positiveInteger(
    argValue("recent-limit", process.env.JITO_SYNC_RECENT_LIMIT || (watch ? "100" : "0")),
    watch ? 100 : 0
  );
  const refreshRecentLimit = positiveInteger(
    argValue(
      "refresh-recent-limit",
      process.env.JITO_SYNC_REFRESH_RECENT_LIMIT || String(Math.min(recentLimit, DEFAULT_REFRESH_RECENT_LIMIT))
    ),
    Math.min(recentLimit, DEFAULT_REFRESH_RECENT_LIMIT)
  );
  const newRowBackfill = nonNegativeInteger(
    argValue("new-row-backfill", process.env.JITO_SYNC_NEW_ROW_BACKFILL || String(DEFAULT_NEW_ROW_BACKFILL)),
    DEFAULT_NEW_ROW_BACKFILL
  );
  const refreshSentRows = boolish(
    argValue("refresh-sent-rows", process.env.JITO_SYNC_REFRESH_SENT_ROWS),
    true
  );

  let lastSyncedCount = -1;
  let lastRefreshAtMs = 0;
  do {
    const rowCount = readJsonl(path).length;
    const nowMs = Date.now();
    const hasNewRows = rowCount !== lastSyncedCount;
    const shouldRefreshRows =
      watch && refreshSentRows && rowCount > 0 && nowMs - lastRefreshAtMs >= refreshIntervalMs;
    if (hasNewRows || shouldRefreshRows) {
      const syncRecentLimit = syncLimitForCycle({
        hasNewRows,
        rowCount,
        lastSyncedCount,
        recentLimit,
        refreshRecentLimit,
        newRowBackfill
      });
      const synced = await syncOnce(path, { recentLimit: syncRecentLimit });
      lastSyncedCount = rowCount;
      lastRefreshAtMs = Date.now();
      const scope = syncRecentLimit > 0 ? `last ${syncRecentLimit} rows` : "all rows";
      const reason = hasNewRows ? "new rows" : "refresh";
      console.error(`synced ${synced} unique local copy executions to Supabase (${scope}, ${reason})`);
    }
    if (watch) {
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
  } while (watch);
}

export {
  blockPositionDiagnostics,
  blockSignatures,
  buyStatus,
  dedupeRows,
  executionKey,
  fetchBlockSignatures,
  syncLimitForCycle,
  autoSellStatus
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
