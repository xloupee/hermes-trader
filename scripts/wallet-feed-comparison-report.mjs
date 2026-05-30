#!/usr/bin/env node
import { createReadStream, existsSync } from "node:fs";
import { createInterface } from "node:readline";

const DEFAULT_PATH = "logs/wallet-trades.jsonl";
const DEFAULT_LIMIT = 20;
const WSOL_MINT = "So11111111111111111111111111111111111111112";

function argValue(name, fallback = null) {
  const prefix = `--${name}=`;
  const match = process.argv.find((arg) => arg.startsWith(prefix));
  return match ? match.slice(prefix.length) : fallback;
}

function numberValue(value, fallback = null) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function booleanValue(value, fallback = false) {
  if (value === null || value === undefined || value === "") {
    return fallback;
  }

  return ["1", "true", "yes", "on"].includes(String(value).toLowerCase());
}

function observedMs(trade) {
  const parsed = Date.parse(trade.observedAt || "");
  return Number.isFinite(parsed) ? parsed : null;
}

function tradeKey(trade) {
  const signature = trade.signature || "";
  const wallet = trade.targetWallet || "";
  const mint = trade.mint || "";

  if (signature && wallet && mint) {
    return `${signature}|${wallet}|${mint}`;
  }

  if (signature && wallet) {
    return `${signature}|${wallet}|`;
  }

  return [wallet, trade.action || "", mint, trade.timestamp ?? "", trade.solAmount ?? "", trade.tokenAmount ?? ""].join("|");
}

async function readJsonl(path) {
  if (!existsSync(path)) {
    throw new Error(`wallet trade log not found: ${path}`);
  }

  const rows = [];
  const errors = [];

  let index = 0;
  const reader = createInterface({
    input: createReadStream(path, { encoding: "utf8" }),
    crlfDelay: Infinity
  });

  for await (const line of reader) {
    index += 1;
    if (!line.trim()) {
      continue;
    }

    try {
      rows.push(JSON.parse(line));
    } catch (error) {
      errors.push({ line: index, error: error.message });
    }
  }

  return { rows, errors };
}

function filterRows(rows, { sinceMs, includeDiagnostic }) {
  return rows.filter((trade) => {
    if (!trade || typeof trade !== "object") {
      return false;
    }

    if (!trade.provider || !trade.targetWallet) {
      return false;
    }

    if (!includeDiagnostic && trade.raw?.diagnosticWallet) {
      return false;
    }

    if (sinceMs === null) {
      return true;
    }

    const ms = observedMs(trade);
    return ms !== null && ms >= sinceMs;
  });
}

function isSolAsset(asset) {
  return asset?.symbol === "SOL" || asset?.mint === WSOL_MINT;
}

function isCopyableSolToTokenBuy(trade) {
  return trade?.action === "buy" && isSolAsset(trade.input) && Boolean(trade.output?.mint) && !isSolAsset(trade.output);
}

function groupedRows(rows) {
  const groups = new Map();

  for (const trade of rows) {
    const key = tradeKey(trade);
    const group = groups.get(key) || [];
    group.push(trade);
    groups.set(key, group);
  }

  return [...groups.values()]
    .map((group) => group.slice().sort((a, b) => (observedMs(a) ?? 0) - (observedMs(b) ?? 0)))
    .sort((a, b) => (observedMs(b[0]) ?? 0) - (observedMs(a[0]) ?? 0));
}

function providerCounts(rows) {
  const counts = new Map();

  for (const trade of rows) {
    counts.set(trade.provider, (counts.get(trade.provider) || 0) + 1);
  }

  return [...counts.entries()].sort(([a], [b]) => a.localeCompare(b));
}

function matchedGroups(groups) {
  return groups.filter((group) => new Set(group.map((trade) => trade.provider)).size > 1);
}

function winnerCounts(groups) {
  const counts = new Map();

  for (const group of groups) {
    const first = group[0];
    if (!first?.provider) {
      continue;
    }

    counts.set(first.provider, (counts.get(first.provider) || 0) + 1);
  }

  return [...counts.entries()].sort(([a], [b]) => a.localeCompare(b));
}

function providerIsolatedCounts(groups) {
  const counts = new Map();

  for (const group of groups) {
    const providers = new Set(group.map((trade) => trade.provider).filter(Boolean));
    if (providers.size !== 1) {
      continue;
    }

    const provider = [...providers][0];
    counts.set(provider, (counts.get(provider) || 0) + 1);
  }

  return [...counts.entries()].sort(([a], [b]) => a.localeCompare(b));
}

function lagRows(groups) {
  const rows = [];

  for (const group of groups) {
    const first = group[0];
    const firstMs = observedMs(first);

    if (firstMs === null) {
      continue;
    }

    for (const trade of group.slice(1)) {
      const ms = observedMs(trade);

      if (ms === null) {
        continue;
      }

      rows.push({
        signature: first.signature || null,
        targetWallet: first.targetWallet || null,
        mint: first.mint || null,
        winner: first.provider,
        provider: trade.provider,
        lagMs: ms - firstMs
      });
    }
  }

  return rows;
}

function percentile(values, p) {
  if (values.length === 0) {
    return null;
  }

  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[index];
}

function stats(values) {
  const numeric = values.filter((value) => Number.isFinite(value));

  if (numeric.length === 0) {
    return null;
  }

  return {
    count: numeric.length,
    p50Ms: percentile(numeric, 50),
    p90Ms: percentile(numeric, 90),
    minMs: Math.min(...numeric),
    maxMs: Math.max(...numeric)
  };
}

function formatCounts(entries) {
  return entries.length === 0 ? "none" : entries.map(([key, count]) => `${key}=${count}`).join(" ");
}

function short(value) {
  if (!value || value.length <= 14) {
    return value || "n/a";
  }

  return `${value.slice(0, 6)}...${value.slice(-6)}`;
}

async function main() {
  const path = argValue("path", process.env.WALLET_TRADE_LOG_PATH || DEFAULT_PATH);
  const limit = numberValue(argValue("limit", process.env.REPORT_LIMIT), DEFAULT_LIMIT);
  const since = argValue("since", process.env.REPORT_SINCE);
  const copyableOnly = booleanValue(argValue("copyable-only", process.env.REPORT_COPYABLE_ONLY), false);
  const includeDiagnostic = booleanValue(argValue("include-diagnostic", process.env.REPORT_INCLUDE_DIAGNOSTIC), true);
  const sinceMs = since ? Date.parse(since) : null;

  if (since && !Number.isFinite(sinceMs)) {
    throw new Error(`invalid --since date: ${since}`);
  }

  const { rows, errors } = await readJsonl(path);
  const windowed = filterRows(rows, { sinceMs, includeDiagnostic });
  const copyableRows = windowed.filter(isCopyableSolToTokenBuy);
  const filtered = copyableOnly ? copyableRows : windowed;
  const groups = groupedRows(filtered);
  const matched = matchedGroups(groups);
  const lags = lagRows(matched);
  const lagsByProvider = new Map();

  for (const row of lags) {
    const key = `${row.winner}->${row.provider}`;
    const values = lagsByProvider.get(key) || [];
    values.push(row.lagMs);
    lagsByProvider.set(key, values);
  }

  console.log(
    `Wallet feed comparison | path=${path}${since ? ` | since=${since}` : ""}` +
      `${copyableOnly ? " | copyableOnly=true" : ""}${includeDiagnostic ? "" : " | includeDiagnostic=false"}`
  );
  console.log(`Rows=${filtered.length} groups=${groups.length} matchedGroups=${matched.length} parseErrors=${errors.length}`);
  console.log(`Copyable SOL-to-token buys=${copyableRows.length}/${windowed.length}`);
  console.log(`Provider rows: ${formatCounts(providerCounts(filtered))}`);
  console.log(`Copyable provider rows: ${formatCounts(providerCounts(copyableRows))}`);
  console.log(`Matched winners: ${formatCounts(winnerCounts(matched))}`);
  console.log(`Provider-isolated groups: ${formatCounts(providerIsolatedCounts(groups))}`);

  if (lagsByProvider.size > 0) {
    console.log("\nLag after winning provider:");
    for (const [key, values] of [...lagsByProvider.entries()].sort(([a], [b]) => a.localeCompare(b))) {
      const summary = stats(values);
      console.log(`  ${key}: count=${summary.count} p50=${summary.p50Ms}ms p90=${summary.p90Ms}ms min=${summary.minMs}ms max=${summary.maxMs}ms`);
    }
  }

  console.log("\nRecent matched signatures:");
  for (const group of matched.slice(0, limit)) {
    const first = group[0];
    const firstMs = observedMs(first);
    const providers = group.map((trade) => {
      const ms = observedMs(trade);
      const delta = firstMs !== null && ms !== null ? `+${ms - firstMs}ms` : "+n/a";
      return `${trade.provider}(${delta})`;
    });
    console.log(
      `  ${short(first.signature)} wallet=${short(first.targetWallet)} mint=${short(first.mint)} ` +
      `action=${first.action || "n/a"} winner=${first.provider} providers=${providers.join(",")}`
    );
  }

  if (matched.length === 0) {
    console.log("  none yet");
  }

  if (errors.length > 0) {
    console.log("\nMalformed JSONL lines:");
    for (const error of errors.slice(0, 5)) {
      console.log(`  line ${error.line}: ${error.error}`);
    }
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
