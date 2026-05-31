#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { createClient } from "@supabase/supabase-js";

function loadEnv(path = ".env") {
  if (!existsSync(path)) {
    return {};
  }

  const env = {};
  for (const raw of readFileSync(path, "utf8").split(/\r?\n/)) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) {
      continue;
    }

    const index = line.indexOf("=");
    if (index === -1) {
      continue;
    }

    const key = line.slice(0, index).trim();
    let value = line.slice(index + 1).trim();
    if ((value.startsWith("\"") && value.endsWith("\"")) || (value.startsWith("'") && value.endsWith("'"))) {
      value = value.slice(1, -1);
    }
    env[key] = value;
  }

  return env;
}

function serviceRoleKey(env) {
  return env.SUPABASE_SERVICE_ROLE_KEY || env.SUPABASE_SERVICE_KEY || env.SUPABASE_SERVICE_ROLE || "";
}

function bigintValue(value) {
  if (value === null || value === undefined || value === "") {
    return 0n;
  }

  return BigInt(value);
}

function formatSol(lamports) {
  const whole = lamports / 1_000_000_000n;
  const fractional = lamports % 1_000_000_000n;
  const fraction = fractional.toString().padStart(9, "0").replace(/0+$/, "");
  return `${fraction ? `${whole}.${fraction}` : whole} SOL`;
}

function platformFeeLamports(row) {
  return bigintValue(row.response?.platformFee?.feeLamports);
}

async function selectAll(client, table, columns) {
  const { data, error } = await client.from(table).select(columns);
  if (error) {
    throw new Error(error.message || `Could not query ${table}`);
  }

  return data || [];
}

const args = new Set(process.argv.slice(2));
const envPath = process.argv.includes("--env")
  ? process.argv[process.argv.indexOf("--env") + 1] || ".env"
  : ".env";
const env = { ...loadEnv(envPath), ...process.env };
const url = env.SUPABASE_URL;
const key = serviceRoleKey(env);

if (!url || !key) {
  console.error("SUPABASE_URL and a Supabase service role key are required.");
  process.exit(1);
}

const client = createClient(url, key, {
  auth: {
    persistSession: false,
    autoRefreshToken: false
  }
});

const [executions, ledger, payouts] = await Promise.all([
  selectAll(client, "telegram_copytrade_executions", "id,status,response,created_at"),
  selectAll(client, "telegram_cashback_ledger", "id,status,platform_fee_lamports,cashback_lamports,created_at"),
  selectAll(client, "telegram_cashback_payouts", "id,status,amount_lamports,signature,error_text,created_at")
]);

const collectedPlatformFees = executions.reduce((sum, row) => sum + platformFeeLamports(row), 0n);
const ledgerPlatformFees = ledger
  .filter((row) => row.status !== "voided")
  .reduce((sum, row) => sum + bigintValue(row.platform_fee_lamports), 0n);
const cashbackAccrued = ledger
  .filter((row) => row.status !== "voided")
  .reduce((sum, row) => sum + bigintValue(row.cashback_lamports), 0n);
const cashbackPaid = payouts
  .filter((row) => row.status === "submitted" || row.status === "confirmed")
  .reduce((sum, row) => sum + bigintValue(row.amount_lamports), 0n);
const failedPayouts = payouts
  .filter((row) => row.status === "failed")
  .reduce((sum, row) => sum + bigintValue(row.amount_lamports), 0n);
const openPayouts = payouts
  .filter((row) => row.status === "pending")
  .reduce((sum, row) => sum + bigintValue(row.amount_lamports), 0n);
const outstandingLiability = cashbackAccrued - cashbackPaid;

console.log("Cashback reconciliation");
console.log(`executions=${executions.length}`);
console.log(`ledgerEntries=${ledger.length}`);
console.log(`payouts=${payouts.length}`);
console.log("");
console.log(`platformFeesFromExecutions=${formatSol(collectedPlatformFees)} (${collectedPlatformFees} lamports)`);
console.log(`platformFeesInLedger=${formatSol(ledgerPlatformFees)} (${ledgerPlatformFees} lamports)`);
console.log(`cashbackAccrued=${formatSol(cashbackAccrued)} (${cashbackAccrued} lamports)`);
console.log(`cashbackPaid=${formatSol(cashbackPaid)} (${cashbackPaid} lamports)`);
console.log(`cashbackFailedPayouts=${formatSol(failedPayouts)} (${failedPayouts} lamports)`);
console.log(`cashbackOpenPayouts=${formatSol(openPayouts)} (${openPayouts} lamports)`);
console.log(`outstandingLiability=${formatSol(outstandingLiability)} (${outstandingLiability} lamports)`);

if (args.has("--json")) {
  console.log(JSON.stringify({
    executions: executions.length,
    ledgerEntries: ledger.length,
    payouts: payouts.length,
    platformFeesFromExecutions: collectedPlatformFees.toString(),
    platformFeesInLedger: ledgerPlatformFees.toString(),
    cashbackAccrued: cashbackAccrued.toString(),
    cashbackPaid: cashbackPaid.toString(),
    cashbackFailedPayouts: failedPayouts.toString(),
    cashbackOpenPayouts: openPayouts.toString(),
    outstandingLiability: outstandingLiability.toString()
  }, null, 2));
}
