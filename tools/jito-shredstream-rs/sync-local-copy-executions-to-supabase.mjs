#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const DEFAULT_EXECUTIONS_PATH = "/tmp/jito-copy-executions-local-send.jsonl";
const DEFAULT_SUPABASE_CWD = `${process.env.HOME || ""}/Documents/pumpfunnoti`;

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

function readJsonl(path) {
  if (!existsSync(path)) {
    return [];
  }
  return readFileSync(path, "utf8")
    .split(/\r?\n/)
    .filter((line) => line.trim().length > 0)
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
    diagnostics.crossSlotPositionSummary = {
      targetSlotTransactionCount: targetBlock.signatures.length,
      copySlotTransactionCount: copyBlock.signatures.length,
      targetTxIndex: diagnostics.targetTxIndex,
      copyTxIndex: diagnostics.copyTxIndex
    };
  }

  return diagnostics;
}

function unknownChainReport(row, unavailableReason) {
  const diagnostics = baseBlockPositionDiagnostics(row, null);
  diagnostics.unavailableReason = unavailableReason;
  return {
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
    return unknownChainReport(row, "missing copy send signature");
  }
  let transaction;
  try {
    transaction = await rpc("getTransaction", [
      row.sendSignature,
      {
        encoding: "jsonParsed",
        commitment: "confirmed",
        maxSupportedTransactionVersion: 0
      }
    ]);
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

  return {
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
    blockTime: transaction.blockTime
  };
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

async function syncOnce(path) {
  const rawRows = readJsonl(path);
  const rows = dedupeRows(rawRows);
  if (rows.length === 0) {
    return 0;
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

async function main() {
  const path = argValue("executions", process.env.JITO_COPY_EXECUTIONS_PATH || DEFAULT_EXECUTIONS_PATH);
  const watch = hasFlag("watch");
  const intervalMs = Number(argValue("interval-ms", "5000"));

  let lastSyncedCount = -1;
  do {
    const rowCount = readJsonl(path).length;
    if (rowCount !== lastSyncedCount) {
      const synced = await syncOnce(path);
      lastSyncedCount = rowCount;
      console.error(`synced ${synced} unique local copy executions to Supabase`);
    }
    if (watch) {
      await new Promise((resolve) => setTimeout(resolve, Number.isFinite(intervalMs) ? intervalMs : 5000));
    }
  } while (watch);
}

export {
  blockPositionDiagnostics,
  blockSignatures,
  dedupeRows,
  executionKey,
  fetchBlockSignatures
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error.message);
    process.exit(1);
  });
}
