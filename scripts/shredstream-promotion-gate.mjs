#!/usr/bin/env node
import dotenv from "dotenv";
import { pathToFileURL } from "node:url";
import {
  loadActiveWallets,
  readJsonl,
  summarizeReadiness
} from "./wallet-feed-readiness-report.mjs";

dotenv.config();

const DEFAULT_WALLET_PATH = "logs/wallet-trades.jsonl";

function argValue(name, fallback = null) {
  const prefix = `--${name}=`;
  const match = process.argv.find((arg) => arg.startsWith(prefix));
  return match ? match.slice(prefix.length) : fallback;
}

function numberValue(value, fallback) {
  if (value === null || value === undefined || value === "") {
    return fallback;
  }

  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function totalMapValue(entries, key) {
  return entries.reduce((total, entry) => total + (entry[key] || 0), 0);
}

function totalNestedMapValue(entries, key, nestedKey) {
  return entries.reduce((total, entry) => total + (entry[key].get(nestedKey) || 0), 0);
}

function evaluatePromotionGate(summary, {
  minActiveWallets = 1,
  minCopyableBuys = 1,
  minShredstreamCopyableBuys = 1,
  minMatchedCopyableGroups = 1
} = {}) {
  const activeWallets = summary.selectedWallets.length;
  const copyableBuys = summary.copyableRows.length;
  const shredstreamCopyableBuys = totalNestedMapValue(summary.wallets, "copyableProviderCounts", "shredstream");
  const matchedCopyableGroups = totalMapValue(summary.wallets, "matchedCopyableGroups");
  const failures = [];

  if (activeWallets < minActiveWallets) {
    failures.push(`active copytrade wallets ${activeWallets} < ${minActiveWallets}`);
  }

  if (copyableBuys < minCopyableBuys) {
    failures.push(`copyable buys ${copyableBuys} < ${minCopyableBuys}`);
  }

  if (shredstreamCopyableBuys < minShredstreamCopyableBuys) {
    failures.push(`ShredStream copyable buys ${shredstreamCopyableBuys} < ${minShredstreamCopyableBuys}`);
  }

  if (matchedCopyableGroups < minMatchedCopyableGroups) {
    failures.push(`matched copyable groups ${matchedCopyableGroups} < ${minMatchedCopyableGroups}`);
  }

  return {
    ok: failures.length === 0,
    failures,
    metrics: {
      activeWallets,
      copyableBuys,
      shredstreamCopyableBuys,
      matchedCopyableGroups
    }
  };
}

async function main() {
  const walletPath = argValue("path", process.env.WALLET_TRADE_LOG_PATH || DEFAULT_WALLET_PATH);
  const since = argValue("since", process.env.REPORT_SINCE);
  const sinceMs = since ? Date.parse(since) : null;
  const thresholds = {
    minActiveWallets: numberValue(argValue("min-active-wallets"), 1),
    minCopyableBuys: numberValue(argValue("min-copyable-buys"), 1),
    minShredstreamCopyableBuys: numberValue(argValue("min-shredstream-copyable-buys"), 1),
    minMatchedCopyableGroups: numberValue(argValue("min-matched-copyable-groups"), 1)
  };

  if (since && !Number.isFinite(sinceMs)) {
    throw new Error(`invalid --since date: ${since}`);
  }

  const activeWallets = await loadActiveWallets({ includeDiagnostic: false });
  const walletLog = await readJsonl(walletPath);
  const summary = summarizeReadiness({
    activeWallets,
    rows: walletLog.rows,
    sinceMs,
    role: "copytrade",
    includeDiagnostic: false
  });
  const gate = evaluatePromotionGate(summary, thresholds);

  console.log(`ShredStream promotion gate | path=${walletPath}${since ? ` | since=${since}` : ""}`);
  console.log(
    `Metrics: activeWallets=${gate.metrics.activeWallets} copyableBuys=${gate.metrics.copyableBuys} ` +
      `shredstreamCopyableBuys=${gate.metrics.shredstreamCopyableBuys} matchedCopyableGroups=${gate.metrics.matchedCopyableGroups}`
  );

  if (!gate.ok) {
    console.log(`Result=FAIL | ${gate.failures.join("; ")}`);
    process.exit(1);
  }

  console.log("Result=PASS");
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  });
}

export { evaluatePromotionGate };
