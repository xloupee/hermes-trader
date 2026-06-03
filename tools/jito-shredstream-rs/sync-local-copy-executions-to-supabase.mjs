#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";

const DEFAULT_EXECUTIONS_PATH = "/tmp/jito-copy-executions-local-send.jsonl";

function argValue(name, fallback = null) {
  const prefix = `--${name}=`;
  const match = process.argv.find((arg) => arg.startsWith(prefix));
  return match ? match.slice(prefix.length) : fallback;
}

function hasFlag(name) {
  return process.argv.includes(`--${name}`);
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
    return null;
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
    return null;
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
    slot: transaction.slot,
    slotDeltaFromObserved: Number.isFinite(transaction.slot) && Number.isFinite(row.slot) ? transaction.slot - row.slot : null,
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
    cwd: process.cwd(),
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

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
