#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { createClient } from "@supabase/supabase-js";

const DEFAULT_EXECUTIONS_PATH = "/tmp/jito-copy-executions-local-send.jsonl";

loadEnvFile(".env.local");
loadEnvFile(".env");

function argValue(name, fallback = null) {
  const prefix = `--${name}=`;
  const match = process.argv.find((arg) => arg.startsWith(prefix));
  return match ? match.slice(prefix.length) : fallback;
}

function readJsonl(path) {
  if (!existsSync(path)) {
    throw new Error(`execution log not found: ${path}`);
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
    });
}

function loadEnvFile(path) {
  if (!existsSync(path)) {
    return;
  }
  for (const line of readFileSync(path, "utf8").split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) {
      continue;
    }
    const index = trimmed.indexOf("=");
    if (index <= 0) {
      continue;
    }
    const key = trimmed.slice(0, index);
    let value = trimmed.slice(index + 1);
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1);
    }
    if (!process.env[key]) {
      process.env[key] = value;
    }
  }
}

async function rpc(method, params) {
  const rpcUrl = process.env.SOLANA_RPC_URL;
  if (!rpcUrl) {
    return null;
  }

  const response = await fetch(rpcUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params })
  });
  if (!response.ok) {
    throw new Error(`${method} HTTP ${response.status}: ${await response.text()}`);
  }
  const body = await response.json();
  if (body.error) {
    throw new Error(`${method} RPC error: ${JSON.stringify(body.error)}`);
  }
  return body.result;
}

function uiAmount(balance) {
  const amount = Number(balance?.uiTokenAmount?.uiAmountString ?? balance?.uiTokenAmount?.uiAmount);
  return Number.isFinite(amount) ? amount : 0;
}

function tokenDelta(transaction, owner, mint) {
  const pre = transaction?.meta?.preTokenBalances ?? [];
  const post = transaction?.meta?.postTokenBalances ?? [];
  const byIndex = new Map();

  for (const balance of pre) {
    if (balance.owner === owner && balance.mint === mint) {
      byIndex.set(balance.accountIndex, { pre: uiAmount(balance), post: 0 });
    }
  }
  for (const balance of post) {
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

async function chainReport(row) {
  if (!row.sendSignature) {
    return {
      buyStatus: row.sent || row.decision === "sent" ? "buySubmitted" : null,
      autoSellStatus: row.autoSellSent || row.autoSellDecision === "sent" ? "autoSellSubmitted" : null,
      autoSell: null
    };
  }
  const transaction = await rpc("getTransaction", [
    row.sendSignature,
    {
      encoding: "jsonParsed",
      commitment: "confirmed",
      maxSupportedTransactionVersion: 0
    }
  ]);
  if (!transaction) {
    return {
      buyStatus: "buySubmitted",
      autoSellStatus: row.autoSellSent || row.autoSellDecision === "sent" ? "autoSellSubmitted" : null,
      sendSignature: row.sendSignature,
      autoSell: null
    };
  }

  let autoSell = null;
  if (row.autoSellSendSignature) {
    const autoSellTransaction = await rpc("getTransaction", [
      row.autoSellSendSignature,
      {
        encoding: "jsonParsed",
        commitment: "confirmed",
        maxSupportedTransactionVersion: 0
      }
    ]);
    autoSell = autoSellTransaction
      ? {
          status: autoSellTransaction.meta?.err ? "failedOnChain" : "landed",
          signature: row.autoSellSendSignature,
          slot: autoSellTransaction.slot,
          blockTime: autoSellTransaction.blockTime,
          err: autoSellTransaction.meta?.err ?? null,
          unavailableReason: null
        }
      : {
          status: "submitted",
          signature: row.autoSellSendSignature,
          slot: null,
          blockTime: null,
          err: null,
          unavailableReason: "auto-sell transaction not found at confirmed commitment"
        };
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

  return {
    sendSignature: row.sendSignature,
    status: transaction.meta?.err ? "failedOnChain" : "landed",
    buyStatus: transaction.meta?.err ? "buyFailedOnChain" : "buyLanded",
    autoSellStatus: autoSell?.err
      ? "autoSellFailedOnChain"
      : autoSell?.slot
        ? "autoSellLanded"
        : row.autoSellSendSignature || row.autoSellSent || row.autoSellDecision === "sent"
          ? "autoSellSubmitted"
          : null,
    autoSell,
    observedSignature: row.observedSignature,
    confirmed: true,
    slot: transaction.slot,
    observedSlot: row.slot,
    slotDeltaFromObserved: Number.isFinite(transaction.slot) && Number.isFinite(row.slot) ? transaction.slot - row.slot : null,
    blockTime: transaction.blockTime,
    err: transaction.meta?.err ?? null,
    copyWallet: row.copyWallet,
    mint: row.mint,
    intendedCopySpendSol,
    maxCopySol: row.maxCopySol ?? null,
    fillTokenDelta: tokenDelta(transaction, row.copyWallet, row.mint),
    copyWalletSolDelta,
    grossCopySpendSol,
    networkFeeSol,
    extraSpendBeyondObservedSol,
    extraSpendBeyondObservedAndNetworkFeeSol
  };
}

function dbRow(row, report) {
  return {
    observed_at_ms: row.observedAtMs,
    execution_at_ms: row.executionAtMs ?? null,
    provider: row.provider,
    source: row.source,
    endpoint: row.endpoint ?? null,
    observed_wallet: row.observedWallet,
    copy_wallet: row.copyWallet ?? null,
    observed_signature: row.observedSignature,
    send_signature: row.sendSignature ?? null,
    slot: row.slot,
    copy_slot: report?.slot ?? null,
    slot_delta_from_observed: report?.slotDeltaFromObserved ?? null,
    selected_route: row.selectedRoute,
    route_layout: row.routeLayout ?? null,
    mint: row.mint,
    observed_action: row.observedAction,
    observed_sol_amount: row.observedSolAmount ?? null,
    max_copy_sol: row.maxCopySol ?? null,
    decision: row.decision,
    reason: row.reason ?? null,
    signed: Boolean(row.signed),
    simulated: Boolean(row.simulated),
    sent: Boolean(row.sent),
    dry_run: Boolean(row.dryRun),
    send_enabled: Boolean(row.sendEnabled),
    simulation_requested: Boolean(row.simulationRequested),
    instruction_count: row.instructionCount ?? 0,
    simulation_units_consumed: row.simulationUnitsConsumed ?? null,
    fill_token_delta: report?.fillTokenDelta ?? null,
    copy_wallet_sol_delta: report?.copyWalletSolDelta ?? null,
    gross_copy_spend_sol: report?.grossCopySpendSol ?? null,
    network_fee_sol: report?.networkFeeSol ?? null,
    extra_spend_beyond_observed_sol: report?.extraSpendBeyondObservedSol ?? null,
    extra_spend_beyond_observed_and_network_fee_sol: report?.extraSpendBeyondObservedAndNetworkFeeSol ?? null,
    observed_to_signed_ms: row.observedToSignedMs ?? null,
    observed_to_simulation_completed_ms: row.observedToSimulationCompletedMs ?? null,
    observed_to_send_submitted_ms: row.observedToSendSubmittedMs ?? null,
    observed_to_signature_returned_ms: row.observedToSignatureReturnedMs ?? null,
    raw_execution: row,
    chain_report: report ?? {}
  };
}

async function main() {
  const supabaseUrl = process.env.NEXT_PUBLIC_SUPABASE_URL || process.env.SUPABASE_URL;
  const serviceRoleKey = process.env.SUPABASE_SERVICE_ROLE_KEY;
  if (!supabaseUrl || !serviceRoleKey) {
    throw new Error("NEXT_PUBLIC_SUPABASE_URL/SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY are required");
  }

  const path = argValue("executions", process.env.JITO_COPY_EXECUTIONS_PATH || DEFAULT_EXECUTIONS_PATH);
  const rows = readJsonl(path);
  const supabase = createClient(supabaseUrl, serviceRoleKey, {
    auth: { persistSession: false, autoRefreshToken: false }
  });

  const dbRows = [];
  for (const row of rows) {
    if (row.schema !== "copytrade.localExecution.v1") {
      continue;
    }
    const report = await chainReport(row);
    dbRows.push(dbRow(row, report));
  }

  if (dbRows.length === 0) {
    console.log("no execution rows to sync");
    return;
  }

  const { data, error } = await supabase
    .from("copytrade_local_executions")
    .upsert(dbRows, {
      onConflict: "provider,observed_signature,observed_wallet,observed_action,mint"
    })
    .select("id,observed_signature,decision,send_signature");

  if (error) {
    throw error;
  }

  console.log(JSON.stringify({ synced: data?.length ?? 0, executions: data }, null, 2));
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
