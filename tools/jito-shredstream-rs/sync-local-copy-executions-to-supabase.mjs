#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const DEFAULT_EXECUTIONS_PATH = "/tmp/jito-copy-executions-local-send.jsonl";
const DEFAULT_SUPABASE_CWD = `${process.env.HOME || ""}/Documents/pumpfunnoti`;
const DEFAULT_WATCH_INTERVAL_MS = 1000;
const DEFAULT_REFRESH_INTERVAL_MS = 5000;
const DEFAULT_REFRESH_RECENT_LIMIT = 1;
const DEFAULT_REFRESH_PENDING_LIMIT = 25;
const DEFAULT_NEW_ROW_BACKFILL = 20;
const DEFAULT_RPC_TIMEOUT_MS = 5000;
const DEFAULT_BLOCK_POSITION_RETRY_ATTEMPTS = 3;
const DEFAULT_BLOCK_POSITION_RETRY_MS = 500;
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

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function readJsonl(path, { recentLimit = 0, pendingPositionLimit = 0 } = {}) {
  if (!existsSync(path)) {
    return [];
  }
  const rows = readFileSync(path, "utf8")
    .split(/\r?\n/)
    .filter((line) => line.trim().length > 0);
  const parsedRows = rows
    .map((line, index) => {
      try {
        return JSON.parse(line);
      } catch (error) {
        throw new Error(`invalid JSONL at ${path}:${index + 1}: ${error.message}`);
      }
    })
    .filter((row) =>
      row.schema === "copytrade.localExecution.v1" ||
      row.schema === "copytrade.sendLaneAttribution.v1" ||
      row.schema === "copytrade.transactionConfirmation.v1"
    );
  const scopedRows = scopeRowsForRecentLocalExecutions(parsedRows, recentLimit);
  const recentRows = mergeSidecarRows(scopedRows);
  if (pendingPositionLimit <= 0) {
    return recentRows;
  }

  const pendingRows = pendingPositionRefreshRows(parsedRows, pendingPositionLimit);
  if (recentLimit <= 0) {
    return pendingRows;
  }

  return dedupeRows([...recentRows, ...pendingRows]);
}

function scopeRowsForRecentLocalExecutions(rows, recentLimit) {
  if (recentLimit <= 0) {
    return rows;
  }
  const localRows = rows.filter((row) => row.schema === "copytrade.localExecution.v1");
  const selectedLocalRows = localRows.slice(-recentLimit);
  const firstSelected = selectedLocalRows[0];
  if (!firstSelected) {
    return [];
  }
  const firstSelectedIndex = rows.findIndex((row) => row === firstSelected);
  return firstSelectedIndex >= 0 ? rows.slice(firstSelectedIndex) : [];
}

function sendLaneAttributionKey(row) {
  return [
    row.provider,
    row.observedSignature,
    row.copyWallet,
    row.mint,
    row.sendSignature
  ].join("\u0000");
}

function transactionConfirmationKey(row) {
  return [
    row.provider,
    row.observedSignature,
    row.copyWallet,
    row.mint,
    row.signature || row.sendSignature
  ].join("\u0000");
}

function mergeSidecarRows(rows) {
  const attributionsByKey = new Map();
  const confirmationsByKey = new Map();
  for (const row of rows) {
    if (row.schema === "copytrade.sendLaneAttribution.v1") {
      attributionsByKey.set(sendLaneAttributionKey(row), row);
    } else if (
      row.schema === "copytrade.transactionConfirmation.v1" &&
      row.transactionRole === "copy_buy"
    ) {
      confirmationsByKey.set(transactionConfirmationKey(row), row);
    }
  }

  return rows
    .filter((row) => row.schema === "copytrade.localExecution.v1")
    .map((row) => {
      const attribution = attributionsByKey.get(sendLaneAttributionKey(row));
      const confirmation = confirmationsByKey.get(transactionConfirmationKey(row));
      return {
        ...row,
        ...(attribution ? { sendLaneAttribution: attribution } : {}),
        ...(confirmation ? { rustTransactionConfirmation: confirmation } : {})
      };
    });
}

function isSubmittedCopyBuy(row) {
  const action = String(row.observedAction ?? "").toLowerCase();
  return action === "buy" && Boolean(row.sendSignature || row.sent || row.decision === "sent");
}

function confirmationTxDelta(confirmation) {
  return finiteNumberOrNull(
    confirmation?.txDelta ?? confirmation?.sameSlotTxDelta ?? confirmation?.txsAfterObserved
  );
}

function needsBlockPositionRefresh(row) {
  if (!isSubmittedCopyBuy(row) || !row.sendSignature) {
    return false;
  }

  const confirmation = row.rustTransactionConfirmation;
  if (!confirmation) {
    return true;
  }

  const txDelta = confirmationTxDelta(confirmation);
  const targetTxIndex = finiteNumberOrNull(confirmation.targetTxIndex);
  const copyTxIndex = finiteNumberOrNull(confirmation.copyTxIndex);
  return (
    txDelta === null ||
    targetTxIndex === null ||
    copyTxIndex === null ||
    Boolean(confirmation.blockPositionError)
  );
}

function pendingPositionRefreshRows(rows, limit) {
  return mergeSidecarRows(rows).filter(needsBlockPositionRefresh).slice(-limit);
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

function timestampFromMs(value) {
  if (!Number.isFinite(value)) {
    return null;
  }
  return new Date(value).toISOString();
}

async function rpc(method, params) {
  const rpcUrl =
    process.env.JITO_SYNC_RPC_URL ||
    process.env.JITO_BLOCK_POSITION_RPC_URL ||
    process.env.SOLANA_RPC_URL;
  if (!rpcUrl) {
    return null;
  }
  const timeoutMs = positiveInteger(process.env.JITO_SYNC_RPC_TIMEOUT_MS, DEFAULT_RPC_TIMEOUT_MS);
  const abort = new AbortController();
  const timeout = setTimeout(() => abort.abort(), timeoutMs);
  try {
    const response = await fetch(rpcUrl, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
      signal: abort.signal
    });
    const body = await response.json();
    if (body.error) {
      throw new Error(`${method} RPC error: ${JSON.stringify(body.error)}`);
    }
    return body.result;
  } catch (error) {
    if (error?.name === "AbortError") {
      throw new Error(`${method} RPC timed out after ${timeoutMs}ms`);
    }
    throw error;
  } finally {
    clearTimeout(timeout);
  }
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
    txDelta: null,
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
    diagnostics.txDelta = diagnostics.sameSlotTxDelta;
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
    diagnostics.txDelta = crossSlotTxDelta;
  }

  return diagnostics;
}

function retryableBlockPositionReason(reason) {
  if (!reason) {
    return false;
  }
  return /block unavailable|getBlock failed|getBlock RPC error|Block not available|timeout|429|Too Many Requests/i.test(reason);
}

async function blockPositionDiagnosticsWithRetry(row, copyTransaction, rpcFn = rpc, options = {}) {
  const attempts = Math.max(
    1,
    positiveInteger(
      options.attempts ??
        argValue("block-position-retry-attempts", process.env.JITO_SYNC_BLOCK_POSITION_RETRY_ATTEMPTS),
      DEFAULT_BLOCK_POSITION_RETRY_ATTEMPTS
    )
  );
  const retryDelayMs = nonNegativeInteger(
    options.retryDelayMs ??
      argValue("block-position-retry-ms", process.env.JITO_SYNC_BLOCK_POSITION_RETRY_MS),
    DEFAULT_BLOCK_POSITION_RETRY_MS
  );

  let diagnostics = null;
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    diagnostics = await blockPositionDiagnostics(row, copyTransaction, rpcFn);
    if (
      diagnostics.status === "found" ||
      attempt >= attempts ||
      !retryableBlockPositionReason(diagnostics.unavailableReason)
    ) {
      return diagnostics;
    }
    if (retryDelayMs > 0) {
      await sleep(retryDelayMs * attempt);
    }
  }
  return diagnostics;
}

function unknownChainReport(row, unavailableReason) {
  const diagnostics = baseBlockPositionDiagnostics(row, null);
  diagnostics.unavailableReason = unavailableReason;
  const status = row.sendSignature || row.sent || row.decision === "sent" ? "submitted" : "unknown";
  const report = {
    status,
    slot: null,
    slotDeltaFromObserved: null,
    blockPositionDiagnostics: diagnostics,
    targetSlot: diagnostics.targetSlot,
    copySlot: diagnostics.copySlot,
    slotDelta: diagnostics.slotDelta,
    targetTxIndex: diagnostics.targetTxIndex,
    copyTxIndex: diagnostics.copyTxIndex,
    sameSlotTxDelta: diagnostics.sameSlotTxDelta,
    txDelta: diagnostics.txDelta,
    crossSlotPositionSummary: diagnostics.crossSlotPositionSummary,
    positionUnavailableReason: diagnostics.unavailableReason,
    fillTokenDelta: null,
    copyWalletSolDelta: null,
    grossCopySpendSol: null,
    networkFeeSol: null,
    extraSpendBeyondObservedSol: null,
    extraSpendBeyondObservedAndNetworkFeeSol: null,
    err: null,
    targetBlockTime: null,
    blockTime: null
  };
  report.buyStatus = buyStatus(row, report);
  report.autoSellStatus = autoSellStatus(row, null);
  return report;
}

function displayTxDelta(report, fallback = null) {
  const txDelta = report?.txDelta ?? report?.blockPositionDiagnostics?.txDelta;
  if (Number.isFinite(txDelta)) {
    return txDelta;
  }

  const sameSlotTxDelta = report?.sameSlotTxDelta;
  if (Number.isFinite(sameSlotTxDelta)) {
    return sameSlotTxDelta;
  }

  const crossSlotTxDelta = report?.crossSlotPositionSummary?.crossSlotTxDelta;
  if (Number.isFinite(crossSlotTxDelta)) {
    return crossSlotTxDelta;
  }

  return fallback ?? null;
}

function finiteNumberOrNull(value) {
  return Number.isFinite(value) ? value : null;
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
    row.copyWallet,
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
  let targetTransaction = null;
  try {
    targetTransaction = await confirmedTransaction(row.observedSignature);
  } catch {
    targetTransaction = null;
  }

  if (!row.sendSignature) {
    const report = unknownChainReport(row, "missing copy send signature");
    report.targetBlockTime = targetTransaction?.blockTime ?? null;
    report.buyStatus = buyStatus(row, report);
    report.autoSellStatus = autoSellStatus(row, null);
    report.autoSell = row.autoSellSendSignature
      ? submittedChainReport(row.autoSellSendSignature, "copy transaction missing; auto-sell not checked")
      : null;
    return report;
  }
  if (row.rustTransactionConfirmation) {
    const report = await chainReportFromRustConfirmation(row, row.rustTransactionConfirmation);
    report.targetBlockTime = targetTransaction?.blockTime ?? null;
    return report;
  }
  let transaction;
  try {
    transaction = await confirmedTransaction(row.sendSignature);
  } catch (error) {
    return unknownChainReport(row, `getTransaction failed: ${error.message}`);
  }
  if (!transaction) {
    const report = unknownChainReport(row, "copy transaction not found at confirmed commitment");
    report.targetBlockTime = targetTransaction?.blockTime ?? null;
    return report;
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
  const positionDiagnostics = await blockPositionDiagnosticsWithRetry(row, transaction);
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
    txDelta: positionDiagnostics.txDelta,
    crossSlotPositionSummary: positionDiagnostics.crossSlotPositionSummary,
    positionUnavailableReason: positionDiagnostics.unavailableReason,
    fillTokenDelta: tokenDelta(transaction, row.copyWallet, row.mint),
    copyWalletSolDelta,
    grossCopySpendSol,
    networkFeeSol,
    extraSpendBeyondObservedSol,
    extraSpendBeyondObservedAndNetworkFeeSol,
    err: transaction.meta?.err ?? null,
    targetBlockTime: targetTransaction?.blockTime ?? null,
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

function applyPositionDiagnostics(report, positionDiagnostics) {
  report.blockPositionDiagnostics = positionDiagnostics;
  report.targetSlot = positionDiagnostics.targetSlot;
  report.copySlot = positionDiagnostics.copySlot;
  report.slotDelta = positionDiagnostics.slotDelta;
  report.targetTxIndex = positionDiagnostics.targetTxIndex;
  report.copyTxIndex = positionDiagnostics.copyTxIndex;
  report.sameSlotTxDelta = positionDiagnostics.sameSlotTxDelta;
  report.txDelta = positionDiagnostics.txDelta;
  report.crossSlotPositionSummary = positionDiagnostics.crossSlotPositionSummary;
  report.positionUnavailableReason = positionDiagnostics.unavailableReason;
}

async function chainReportFromRustConfirmation(row, confirmation, rpcFn = rpc) {
  const slot = Number.isFinite(confirmation.confirmationSlot)
    ? confirmation.confirmationSlot
    : null;
  const slotDeltaFromObserved =
    Number.isFinite(slot) && Number.isFinite(row.slot) ? slot - row.slot : null;
  const positionDiagnostics = baseBlockPositionDiagnostics(
    row,
    slot === null ? null : { slot }
  );
  positionDiagnostics.targetTxIndex = finiteNumberOrNull(confirmation.targetTxIndex);
  positionDiagnostics.copyTxIndex = finiteNumberOrNull(confirmation.copyTxIndex);
  positionDiagnostics.sameSlotTxDelta = finiteNumberOrNull(
    confirmation.sameSlotTxDelta ?? confirmation.txsAfterObserved
  );
  positionDiagnostics.txDelta = finiteNumberOrNull(
    confirmation.txDelta ?? confirmation.sameSlotTxDelta ?? confirmation.txsAfterObserved
  );
  positionDiagnostics.status =
    Number.isFinite(positionDiagnostics.targetTxIndex) &&
    Number.isFinite(positionDiagnostics.copyTxIndex) &&
    Number.isFinite(positionDiagnostics.txDelta)
      ? "found"
      : "unknown";
  positionDiagnostics.unavailableReason =
    positionDiagnostics.status === "found"
      ? null
      : confirmation.blockPositionError ||
        "block position not fetched by dashboard sync";

  const report = {
    status: confirmation.ok === false ? "failedOnChain" : confirmation.status || "submitted",
    slot,
    slotDeltaFromObserved,
    blockPositionDiagnostics: positionDiagnostics,
    targetSlot: positionDiagnostics.targetSlot,
    copySlot: positionDiagnostics.copySlot,
    slotDelta: positionDiagnostics.slotDelta,
    targetTxIndex: positionDiagnostics.targetTxIndex,
    copyTxIndex: positionDiagnostics.copyTxIndex,
    sameSlotTxDelta: positionDiagnostics.sameSlotTxDelta,
    txDelta: positionDiagnostics.txDelta,
    crossSlotPositionSummary: positionDiagnostics.crossSlotPositionSummary,
    positionUnavailableReason: positionDiagnostics.unavailableReason,
    fillTokenDelta: null,
    copyWalletSolDelta: null,
    grossCopySpendSol: null,
    networkFeeSol: null,
    extraSpendBeyondObservedSol: null,
    extraSpendBeyondObservedAndNetworkFeeSol: null,
    err: confirmation.err ?? null,
    targetBlockTime: null,
    blockTime: null,
    autoSell: null
  };
  if (positionDiagnostics.status !== "found" && slot !== null) {
    const refreshedPositionDiagnostics = await blockPositionDiagnosticsWithRetry(
      row,
      { slot },
      rpcFn
    );
    if (refreshedPositionDiagnostics.status === "found") {
      applyPositionDiagnostics(report, refreshedPositionDiagnostics);
    }
  }
  report.buyStatus = buyStatus(row, report);
  report.autoSellStatus = autoSellStatus(row, null);
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
      created_at: timestampFromMs(row.observedAtMs),
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
      slot_delta: report?.slotDelta ?? row.slotDelta ?? null,
      tx_delta: displayTxDelta(report, row.txDelta),
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
	      signed_tx_bytes: Number.isFinite(row.signedTxBytes) ? row.signedTxBytes : null,
	      writable_account_count: Number.isFinite(row.writableAccountCount) ? row.writableAccountCount : null,
	      compute_unit_limit: Number.isFinite(row.computeUnitLimit) ? row.computeUnitLimit : null,
	      selected_tip_account: row.selectedTipAccount ?? null,
	      source_compute_unit_limit: Number.isFinite(row.sourceComputeUnitLimit)
	        ? row.sourceComputeUnitLimit
	        : null,
	      source_compute_unit_price_micro_lamports: Number.isFinite(row.sourceComputeUnitPriceMicroLamports)
	        ? row.sourceComputeUnitPriceMicroLamports
	        : null,
	      compute_units_consumed: Number.isFinite(row.computeUnitsConsumed) ? row.computeUnitsConsumed : null,
	      cost_units: Number.isFinite(row.costUnits) ? row.costUnits : null,
	      transaction_meta_error: row.transactionMetaError ?? null,
	      blockhash: row.blockhash ?? null,
	      blockhash_source_rpc: row.blockhashSourceRpc ?? null,
	      blockhash_commitment: row.blockhashCommitment ?? null,
	      blockhash_context_slot: Number.isFinite(row.blockhashContextSlot) ? row.blockhashContextSlot : null,
	      blockhash_age_ms: Number.isFinite(row.blockhashAgeMs) ? row.blockhashAgeMs : null,
	      blockhash_selection_strategy: row.blockhashSelectionStrategy ?? null,
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
      feed_received_at_ms: Number.isFinite(row.feedReceivedAtMs) ? row.feedReceivedAtMs : null,
      decoded_at_ms: Number.isFinite(row.decodedAtMs) ? row.decodedAtMs : null,
      matched_at_ms: Number.isFinite(row.matchedAtMs) ? row.matchedAtMs : null,
      planned_at_ms: Number.isFinite(row.plannedAtMs) ? row.plannedAtMs : null,
      built_at_ms: Number.isFinite(row.builtAtMs) ? row.builtAtMs : null,
      feed_received_to_decoded_us: Number.isFinite(row.feedReceivedToDecodedUs)
        ? row.feedReceivedToDecodedUs
        : null,
      decoded_to_matched_us: Number.isFinite(row.decodedToMatchedUs)
        ? row.decodedToMatchedUs
        : null,
      matched_to_planned_ms: Number.isFinite(row.matchedToPlannedMs)
        ? row.matchedToPlannedMs
        : null,
      planned_to_built_ms: Number.isFinite(row.plannedToBuiltMs) ? row.plannedToBuiltMs : null,
      executor_queue_us: Number.isFinite(row.executorQueueUs) ? row.executorQueueUs : null,
      guards_us: Number.isFinite(row.guardsUs) ? row.guardsUs : null,
      unsigned_build_us: Number.isFinite(row.unsignedBuildUs) ? row.unsignedBuildUs : null,
      sign_us: Number.isFinite(row.signUs) ? row.signUs : null,
      serialize_us: Number.isFinite(row.serializeUs) ? row.serializeUs : null,
      batch_transaction_count: Number.isFinite(row.batchTransactionCount)
        ? row.batchTransactionCount
        : null,
      matched_transaction_index: Number.isFinite(row.matchedTransactionIndex)
        ? row.matchedTransactionIndex
        : null,
      batch_scan_us: Number.isFinite(row.batchScanUs) ? row.batchScanUs : null,
      tx_parse_us: Number.isFinite(row.txParseUs) ? row.txParseUs : null,
      account_expand_us: Number.isFinite(row.accountExpandUs) ? row.accountExpandUs : null,
      wallet_match_us: Number.isFinite(row.walletMatchUs) ? row.walletMatchUs : null,
      route_parse_us: Number.isFinite(row.routeParseUs) ? row.routeParseUs : null,
      send_lane_ms: Number.isFinite(row.sendLaneMs) ? row.sendLaneMs : null,
      fee_profile_name: row.feeProfileName ?? null,
      selected_priority_fee_micro_lamports: Number.isFinite(row.selectedPriorityFeeMicroLamports)
        ? row.selectedPriorityFeeMicroLamports
        : null,
      selected_helius_tip_lamports: Number.isFinite(row.selectedHeliusTipLamports)
        ? row.selectedHeliusTipLamports
        : null,
      source_position_bucket: row.sourcePositionBucket ?? null,
      fee_reason: row.feeReason ?? null,
      fee_cap_hit: Boolean(row.feeCapHit),
      account_priority_fee_enabled: Boolean(row.accountPriorityFeeEnabled),
      account_priority_fee_micro_lamports: Number.isFinite(row.accountPriorityFeeMicroLamports)
        ? row.accountPriorityFeeMicroLamports
        : null,
      account_priority_fee_age_ms: Number.isFinite(row.accountPriorityFeeAgeMs)
        ? row.accountPriorityFeeAgeMs
        : null,
      account_priority_fee_sample_count: Number.isFinite(row.accountPriorityFeeSampleCount)
        ? row.accountPriorityFeeSampleCount
        : null,
      account_priority_fee_account_count: Number.isFinite(row.accountPriorityFeeAccountCount)
        ? row.accountPriorityFeeAccountCount
        : null,
      account_priority_fee_applied: Boolean(row.accountPriorityFeeApplied),
      account_priority_fee_reason: row.accountPriorityFeeReason ?? null,
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

const OPTIONAL_REST_COLUMNS = new Set([
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
  "account_priority_fee_enabled",
  "account_priority_fee_micro_lamports",
  "account_priority_fee_age_ms",
  "account_priority_fee_sample_count",
  "account_priority_fee_account_count",
  "account_priority_fee_applied",
  "account_priority_fee_reason",
  "signed_tx_bytes",
  "writable_account_count",
  "compute_unit_limit",
  "selected_tip_account",
  "source_compute_unit_limit",
  "source_compute_unit_price_micro_lamports",
  "compute_units_consumed",
  "cost_units",
  "transaction_meta_error",
  "blockhash",
  "blockhash_source_rpc",
  "blockhash_commitment",
  "blockhash_context_slot",
  "blockhash_age_ms",
  "blockhash_selection_strategy",
  "slot_delta",
  "tx_delta"
]);

function missingOptionalColumn(text) {
  let parsed = null;
  try {
    parsed = JSON.parse(text);
  } catch {
    parsed = null;
  }

  const message = `${parsed?.message ?? ""} ${parsed?.details ?? ""} ${text}`.toLowerCase();
  const candidates = [
    /'([a-z0-9_]+)'\s+column/,
    /column\s+"?([a-z0-9_]+)"?\s+does not exist/,
    /could not find the\s+'([a-z0-9_]+)'/
  ];

  for (const candidate of candidates) {
    const match = candidate.exec(message);
    if (match && OPTIONAL_REST_COLUMNS.has(match[1])) {
      return match[1];
    }
  }

  return null;
}

function stripOptionalRestColumns(records) {
  return records.map((record) => Object.fromEntries(
    Object.entries(record).filter(([column]) => !OPTIONAL_REST_COLUMNS.has(column))
  ));
}

async function postExecutionRecords(base, records) {
  return fetch(
    `${base}/rest/v1/copytrade_local_executions?on_conflict=provider,observed_signature,observed_wallet,copy_wallet,observed_action,mint`,
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
}

async function syncViaSupabaseRest(records) {
  if (records.length === 0) {
    return;
  }

  const base = process.env.SUPABASE_URL.trim().replace(/\/+$/, "");
  const response = await postExecutionRecords(base, records);

  if (!response.ok) {
    const text = await response.text();
    const missingColumn = missingOptionalColumn(text);
    if (missingColumn) {
      console.warn(
        `Supabase REST schema is missing optional timing column ${missingColumn}; retrying with timing columns in raw_execution only`
      );
      const retry = await postExecutionRecords(base, stripOptionalRestColumns(records));
      if (retry.ok) {
        return;
      }
      throw new Error(`Supabase REST fallback upsert failed: ${retry.status} ${await retry.text()}`);
    }
    throw new Error(`Supabase REST upsert failed: ${response.status} ${text}`);
  }
}

async function buildSql(rows) {
  const columns = [
    "created_at",
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
    "slot_delta",
    "tx_delta",
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
    "signed_tx_bytes",
    "writable_account_count",
    "compute_unit_limit",
    "selected_tip_account",
    "source_compute_unit_limit",
    "source_compute_unit_price_micro_lamports",
    "compute_units_consumed",
    "cost_units",
    "transaction_meta_error",
    "blockhash",
    "blockhash_source_rpc",
    "blockhash_commitment",
    "blockhash_context_slot",
    "blockhash_age_ms",
    "blockhash_selection_strategy",
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
    "account_priority_fee_enabled",
    "account_priority_fee_micro_lamports",
    "account_priority_fee_age_ms",
    "account_priority_fee_sample_count",
    "account_priority_fee_account_count",
    "account_priority_fee_applied",
    "account_priority_fee_reason",
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
      sqlString(timestampFromMs(row.observedAtMs)),
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
      sqlNumber(report?.slotDelta ?? row.slotDelta),
      sqlNumber(displayTxDelta(report, row.txDelta)),
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
      sqlNumber(row.signedTxBytes),
      sqlNumber(row.writableAccountCount),
      sqlNumber(row.computeUnitLimit),
      sqlString(row.selectedTipAccount),
      sqlNumber(row.sourceComputeUnitLimit),
      sqlNumber(row.sourceComputeUnitPriceMicroLamports),
      sqlNumber(row.computeUnitsConsumed),
      sqlNumber(row.costUnits),
      sqlString(row.transactionMetaError),
      sqlString(row.blockhash),
      sqlString(row.blockhashSourceRpc),
      sqlString(row.blockhashCommitment),
      sqlNumber(row.blockhashContextSlot),
      sqlNumber(row.blockhashAgeMs),
      sqlString(row.blockhashSelectionStrategy),
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
      sqlNumber(row.feedReceivedAtMs),
      sqlNumber(row.decodedAtMs),
      sqlNumber(row.matchedAtMs),
      sqlNumber(row.plannedAtMs),
      sqlNumber(row.builtAtMs),
      sqlNumber(row.feedReceivedToDecodedUs),
      sqlNumber(row.decodedToMatchedUs),
      sqlNumber(row.matchedToPlannedMs),
      sqlNumber(row.plannedToBuiltMs),
      sqlNumber(row.executorQueueUs),
      sqlNumber(row.guardsUs),
      sqlNumber(row.unsignedBuildUs),
      sqlNumber(row.signUs),
      sqlNumber(row.serializeUs),
      sqlNumber(row.batchTransactionCount),
      sqlNumber(row.matchedTransactionIndex),
      sqlNumber(row.batchScanUs),
      sqlNumber(row.txParseUs),
      sqlNumber(row.accountExpandUs),
      sqlNumber(row.walletMatchUs),
      sqlNumber(row.routeParseUs),
      sqlNumber(row.sendLaneMs),
      sqlString(row.feeProfileName),
      sqlNumber(row.selectedPriorityFeeMicroLamports),
      sqlNumber(row.selectedHeliusTipLamports),
      sqlString(row.sourcePositionBucket),
      sqlString(row.feeReason),
      sqlBoolean(row.feeCapHit),
      sqlBoolean(row.accountPriorityFeeEnabled),
      sqlNumber(row.accountPriorityFeeMicroLamports),
      sqlNumber(row.accountPriorityFeeAgeMs),
      sqlNumber(row.accountPriorityFeeSampleCount),
      sqlNumber(row.accountPriorityFeeAccountCount),
      sqlBoolean(row.accountPriorityFeeApplied),
      sqlString(row.accountPriorityFeeReason),
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
    .filter((column) => !["provider", "observed_signature", "observed_wallet", "copy_wallet", "observed_action", "mint"].includes(column))
    .map((column) => `${column}=excluded.${column}`)
    .join(",");

  return `insert into public.copytrade_local_executions (${columns.join(",")}) values ${values.join(",")} on conflict (provider, observed_signature, observed_wallet, copy_wallet, observed_action, mint) do update set ${updates};`;
}

async function syncOnce(path, { recentLimit = 0, pendingPositionLimit = 0 } = {}) {
  const rawRows = readJsonl(path, { recentLimit, pendingPositionLimit });
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
  const refreshPendingLimit = positiveInteger(
    argValue(
      "refresh-pending-limit",
      process.env.JITO_SYNC_REFRESH_PENDING_LIMIT || String(DEFAULT_REFRESH_PENDING_LIMIT)
    ),
    DEFAULT_REFRESH_PENDING_LIMIT
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
      const syncRecentLimit = hasNewRows ? syncLimitForCycle({
        hasNewRows,
        rowCount,
        lastSyncedCount,
        recentLimit,
        refreshRecentLimit,
        newRowBackfill
      }) : 0;
      const pendingPositionLimit = shouldRefreshRows ? refreshPendingLimit : 0;
      const synced = await syncOnce(path, { recentLimit: syncRecentLimit, pendingPositionLimit });
      lastSyncedCount = rowCount;
      if (shouldRefreshRows) {
        lastRefreshAtMs = Date.now();
      }
      const scopes = [];
      if (syncRecentLimit > 0) {
        scopes.push(`last ${syncRecentLimit} rows`);
      } else if (pendingPositionLimit <= 0) {
        scopes.push("all rows");
      }
      if (pendingPositionLimit > 0) {
        scopes.push(`pending position rows up to ${pendingPositionLimit}`);
      }
      const reason = hasNewRows && shouldRefreshRows
        ? "new rows + refresh"
        : hasNewRows
          ? "new rows"
          : "refresh";
      console.error(`synced ${synced} unique local copy executions to Supabase (${scopes.join(" + ")}, ${reason})`);
    }
    if (watch) {
      await new Promise((resolve) => setTimeout(resolve, intervalMs));
    }
  } while (watch);
}

export {
  blockPositionDiagnostics,
  blockPositionDiagnosticsWithRetry,
  blockSignatures,
  buildSql,
  buyStatus,
  chainReport,
  chainReportFromRustConfirmation,
  dedupeRows,
  executionKey,
  fetchBlockSignatures,
  displayTxDelta,
  mergeSidecarRows,
  needsBlockPositionRefresh,
  pendingPositionRefreshRows,
  readJsonl,
  syncLimitForCycle,
  unknownChainReport,
  autoSellStatus
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
