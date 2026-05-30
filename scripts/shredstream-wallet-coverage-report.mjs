#!/usr/bin/env node
import { createReadStream, existsSync } from "node:fs";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";

const DEFAULT_WALLET_PATH = "logs/wallet-trades.jsonl";
const DEFAULT_SHREDSTREAM_PATH = "logs/shred-pump-events.jsonl";
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

async function readJsonl(path, label) {
  if (!existsSync(path)) {
    throw new Error(`${label} log not found: ${path}`);
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

function rowMs(row) {
  if (Number.isFinite(row.receivedAtMs)) {
    return Number(row.receivedAtMs);
  }

  const observedMs = Date.parse(row.observedAt || "");
  if (Number.isFinite(observedMs)) {
    return observedMs;
  }

  if (Number.isFinite(row.timestamp)) {
    const timestamp = Number(row.timestamp);
    return timestamp > 10_000_000_000 ? timestamp : timestamp * 1000;
  }

  return null;
}

function inWindow(row, { sinceMs, untilMs }) {
  const ms = rowMs(row);

  if (sinceMs !== null && (ms === null || ms < sinceMs)) {
    return false;
  }

  if (untilMs !== null && (ms === null || ms > untilMs)) {
    return false;
  }

  return true;
}

function walletKey(row) {
  const signature = row.signature || "";
  const wallet = row.targetWallet || "";
  const mint = row.mint || "";
  const action = normalizeAction(row.action) || "";

  return signature && wallet && mint && action ? `${signature}|${wallet}|${mint}|${action}` : null;
}

function shredKey(row) {
  const signature = row.signature || "";
  const wallet = row.trader || "";
  const mint = row.mint || "";
  const action = normalizeAction(row.eventType) || "";

  return signature && wallet && mint && action ? `${signature}|${wallet}|${mint}|${action}` : null;
}

function signatureKey(row) {
  return row.signature || null;
}

function normalizeAction(action) {
  return action === "buy" || action === "sell" ? action : null;
}

function isSolAsset(asset) {
  return asset?.symbol === "SOL" || asset?.mint === WSOL_MINT;
}

function isCopyableWalletBuy(row) {
  return row?.action === "buy" && isSolAsset(row.input) && Boolean(row.output?.mint) && !isSolAsset(row.output);
}

function isCopyableShredBuy(row) {
  return row?.eventType === "buy" && (!row.quoteMint || row.quoteMint === WSOL_MINT);
}

function providerSet(value) {
  return new Set(
    String(value || "pumpportal,geyser")
      .split(",")
      .map((provider) => provider.trim())
      .filter(Boolean)
  );
}

function formatCounts(counts) {
  const entries = [...counts.entries()].sort(([a], [b]) => a.localeCompare(b));
  return entries.length === 0 ? "none" : entries.map(([key, count]) => `${key}=${count}`).join(" ");
}

function short(value) {
  if (!value || value.length <= 14) {
    return value || "n/a";
  }

  return `${value.slice(0, 6)}...${value.slice(-6)}`;
}

function summarizeCoverage({ walletRows, shredRows, sinceMs = null, untilMs = null, providers = providerSet(null), copyableOnly = false }) {
  const windowedWalletRows = walletRows.filter((row) => {
    if (!row || typeof row !== "object") {
      return false;
    }

    if (!providers.has(row.provider)) {
      return false;
    }

    if (!walletKey(row)) {
      return false;
    }

    return inWindow(row, { sinceMs, untilMs });
  });
  const windowedShredRows = shredRows.filter((row) => {
    if (!row || typeof row !== "object") {
      return false;
    }

    if (row.source !== "shredstream" || row.decodeStatus !== "decoded") {
      return false;
    }

    if (normalizeAction(row.eventType) === null || !shredKey(row)) {
      return false;
    }

    return inWindow(row, { sinceMs, untilMs });
  });
  const copyableWalletRows = windowedWalletRows.filter(isCopyableWalletBuy);
  const copyableShredRows = windowedShredRows.filter(isCopyableShredBuy);
  const filteredWalletRows = copyableOnly ? copyableWalletRows : windowedWalletRows;
  const filteredShredRows = copyableOnly ? copyableShredRows : windowedShredRows;
  const shredByKey = new Map();
  const shredBySignature = new Map();
  const providerCounts = new Map();
  const actionCounts = new Map();
  const missingReasonCounts = new Map();

  for (const row of shredRows) {
    if (!row || typeof row !== "object" || row.source !== "shredstream") {
      continue;
    }

    const signature = signatureKey(row);
    if (!signature) {
      continue;
    }

    const rows = shredBySignature.get(signature) || [];
    rows.push(row);
    shredBySignature.set(signature, rows);
  }

  for (const row of filteredShredRows) {
    const key = shredKey(row);
    const current = shredByKey.get(key);
    if (!current || (rowMs(row) ?? 0) < (rowMs(current) ?? 0)) {
      shredByKey.set(key, row);
    }
  }

  const matched = [];
  const missing = [];
  const walletKeys = new Set(filteredWalletRows.map(walletKey).filter(Boolean));
  const uncorroboratedShredRows = filteredShredRows.filter((row) => {
    const key = shredKey(row);
    return key && !walletKeys.has(key);
  });

  for (const row of filteredWalletRows) {
    providerCounts.set(row.provider, (providerCounts.get(row.provider) || 0) + 1);
    actionCounts.set(row.action || "unknown", (actionCounts.get(row.action || "unknown") || 0) + 1);

    const key = walletKey(row);
    const shred = shredByKey.get(key);
    if (!shred) {
      const classification = classifyMissingWalletRow(row, shredBySignature.get(row.signature) || []);
      missingReasonCounts.set(classification.reason, (missingReasonCounts.get(classification.reason) || 0) + 1);
      missing.push({ row, ...classification });
      continue;
    }

    const walletMs = rowMs(row);
    const shredMs = rowMs(shred);
    matched.push({
      key,
      wallet: row,
      shred,
      deltaMs: walletMs !== null && shredMs !== null ? shredMs - walletMs : null
    });
  }

  return {
    walletRows: filteredWalletRows,
    shredRows: filteredShredRows,
    windowedWalletRows,
    windowedShredRows,
    copyableWalletRows,
    copyableShredRows,
    providerCounts,
    actionCounts,
    missingReasonCounts,
    matched,
    missing,
    uncorroboratedShredRows
  };
}

function classifyMissingWalletRow(walletRow, shredRowsForSignature) {
  if (shredRowsForSignature.length === 0) {
    return {
      reason: "signature_absent",
      candidates: []
    };
  }

  const candidates = shredRowsForSignature.map((row) => ({
    decodeStatus: row.decodeStatus || "unknown",
    eventType: row.eventType || "unknown",
    programId: row.programId || "unknown",
    trader: row.trader || null,
    mint: row.mint || null,
    actionMatches: normalizeAction(row.eventType) === normalizeAction(walletRow.action),
    walletMatches: row.trader === walletRow.targetWallet,
    mintMatches: row.mint === walletRow.mint
  }));
  const decodedTradeCandidates = candidates.filter(
    (candidate) => candidate.decodeStatus === "decoded" && normalizeAction(candidate.eventType) !== null
  );

  if (decodedTradeCandidates.length === 0) {
    return {
      reason: "signature_present_no_decoded_trade",
      candidates
    };
  }

  if (decodedTradeCandidates.some((candidate) => candidate.walletMatches && candidate.mintMatches && !candidate.actionMatches)) {
    return {
      reason: "action_mismatch",
      candidates
    };
  }

  if (decodedTradeCandidates.some((candidate) => candidate.walletMatches && !candidate.mintMatches)) {
    return {
      reason: "mint_mismatch",
      candidates
    };
  }

  if (decodedTradeCandidates.some((candidate) => !candidate.walletMatches && candidate.mintMatches)) {
    return {
      reason: "wallet_mismatch",
      candidates
    };
  }

  return {
    reason: "wallet_and_mint_mismatch",
    candidates
  };
}

function percentile(values, p) {
  const numeric = values.filter((value) => Number.isFinite(value)).sort((a, b) => a - b);
  if (numeric.length === 0) {
    return null;
  }

  const index = Math.min(numeric.length - 1, Math.max(0, Math.ceil((p / 100) * numeric.length) - 1));
  return numeric[index];
}

async function main() {
  const walletPath = argValue("wallet", process.env.WALLET_TRADE_LOG_PATH || DEFAULT_WALLET_PATH);
  const shredstreamPath = argValue("shredstream", process.env.SHREDSTREAM_EVENT_LOG_PATH || DEFAULT_SHREDSTREAM_PATH);
  const since = argValue("since", process.env.REPORT_SINCE);
  const until = argValue("until", process.env.REPORT_UNTIL);
  const limit = numberValue(argValue("limit", process.env.REPORT_LIMIT), DEFAULT_LIMIT);
  const providers = providerSet(argValue("providers", process.env.REPORT_PROVIDERS));
  const copyableOnly = booleanValue(argValue("copyable-only", process.env.REPORT_COPYABLE_ONLY), false);
  const sinceMs = since ? Date.parse(since) : null;
  const untilMs = until ? Date.parse(until) : null;

  if (since && !Number.isFinite(sinceMs)) {
    throw new Error(`invalid --since date: ${since}`);
  }

  if (until && !Number.isFinite(untilMs)) {
    throw new Error(`invalid --until date: ${until}`);
  }

  const wallet = await readJsonl(walletPath, "wallet trade");
  const shredstream = await readJsonl(shredstreamPath, "ShredStream event");
  const summary = summarizeCoverage({
    walletRows: wallet.rows,
    shredRows: shredstream.rows,
    sinceMs,
    untilMs,
    providers,
    copyableOnly
  });
  const deltas = summary.matched.map((row) => row.deltaMs).filter((value) => Number.isFinite(value));

  console.log(
    `ShredStream wallet coverage | wallet=${walletPath} | shredstream=${shredstreamPath}` +
      `${since ? ` | since=${since}` : ""}${until ? ` | until=${until}` : ""}${copyableOnly ? " | copyableOnly=true" : ""}`
  );
  console.log(
    `WalletRows=${summary.walletRows.length} ShredstreamCandidates=${summary.shredRows.length} ` +
      `matched=${summary.matched.length} missing=${summary.missing.length} ` +
      `walletParseErrors=${wallet.errors.length} shredParseErrors=${shredstream.errors.length}`
  );
  console.log(`Unmatched global ShredStream candidates=${summary.uncorroboratedShredRows.length}`);
  console.log(
    `Copyable SOL-to-token buys: wallet=${summary.copyableWalletRows.length}/${summary.windowedWalletRows.length} ` +
      `shredstream=${summary.copyableShredRows.length}/${summary.windowedShredRows.length}`
  );
  console.log(`Wallet providers: ${formatCounts(summary.providerCounts)}`);
  console.log(`Wallet actions: ${formatCounts(summary.actionCounts)}`);
  console.log(`Missing reasons: ${formatCounts(summary.missingReasonCounts)}`);

  if (deltas.length > 0) {
    console.log(
      `Shred minus wallet observedAt: count=${deltas.length} p50=${percentile(deltas, 50)}ms ` +
        `p90=${percentile(deltas, 90)}ms min=${Math.min(...deltas)}ms max=${Math.max(...deltas)}ms`
    );
  }

  console.log("\nRecent matched wallet rows:");
  for (const row of summary.matched.slice(-limit).reverse()) {
    console.log(
      `  ${short(row.wallet.signature)} wallet=${short(row.wallet.targetWallet)} mint=${short(row.wallet.mint)} ` +
        `provider=${row.wallet.provider} action=${row.wallet.action} deltaMs=${row.deltaMs ?? "n/a"}`
    );
  }
  if (summary.matched.length === 0) {
    console.log("  none");
  }

  console.log("\nRecent missing wallet rows:");
  for (const missing of summary.missing.slice(-limit).reverse()) {
    const row = missing.row;
    const candidateSummary = missing.candidates
      .slice(0, 3)
      .map(
        (candidate) =>
          `${candidate.eventType}/${candidate.decodeStatus}` +
          ` wallet=${candidate.walletMatches ? "yes" : short(candidate.trader)}` +
          ` mint=${candidate.mintMatches ? "yes" : short(candidate.mint)}`
      )
      .join("; ");
    console.log(
      `  ${short(row.signature)} wallet=${short(row.targetWallet)} mint=${short(row.mint)} ` +
        `provider=${row.provider} action=${row.action} reason=${missing.reason} observedAt=${row.observedAt || "n/a"}` +
        `${candidateSummary ? ` candidates=${candidateSummary}` : ""}`
    );
  }
  if (summary.missing.length === 0) {
    console.log("  none");
  }

  console.log("\nRecent unmatched global ShredStream rows:");
  for (const row of summary.uncorroboratedShredRows.slice(-limit).reverse()) {
    console.log(
      `  ${short(row.signature)} wallet=${short(row.trader)} mint=${short(row.mint)} ` +
        `event=${row.eventType || "n/a"} pool=${row.pool || "n/a"} receivedAtMs=${row.receivedAtMs ?? "n/a"}`
    );
  }
  if (summary.uncorroboratedShredRows.length === 0) {
    console.log("  none");
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  });
}

export { providerSet, summarizeCoverage };
