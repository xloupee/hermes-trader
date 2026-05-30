#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { createReadStream } from "node:fs";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";
import { createClient } from "@supabase/supabase-js";
import dotenv from "dotenv";

dotenv.config();

const DEFAULT_WALLET_PATH = "logs/wallet-trades.jsonl";
const DEFAULT_JSON_SUBSCRIBERS_PATH = "data/telegram-subscribers.json";
const WSOL_MINT = "So11111111111111111111111111111111111111112";

function argValue(name, fallback = null) {
  const prefix = `--${name}=`;
  const match = process.argv.find((arg) => arg.startsWith(prefix));
  return match ? match.slice(prefix.length) : fallback;
}

function observedMs(trade) {
  const parsed = Date.parse(trade.observedAt || "");
  return Number.isFinite(parsed) ? parsed : null;
}

function inWindow(row, { sinceMs }) {
  const ms = observedMs(row);
  return sinceMs === null || (ms !== null && ms >= sinceMs);
}

function isSolAsset(asset) {
  return asset?.symbol === "SOL" || asset?.mint === WSOL_MINT;
}

function isCopyableSolToTokenBuy(trade) {
  return trade?.action === "buy" && isSolAsset(trade.input) && Boolean(trade.output?.mint) && !isSolAsset(trade.output);
}

function tradeKey(trade) {
  const signature = trade.signature || "";
  const wallet = trade.targetWallet || "";
  const mint = trade.mint || "";

  return signature && wallet && mint ? `${signature}|${wallet}|${mint}` : null;
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

function normalizeWallet(value, role) {
  const address = typeof value?.address === "string" ? value.address.trim() : "";
  if (!address) {
    return null;
  }

  return {
    address,
    role,
    label: typeof value.label === "string" && value.label.trim() ? value.label.trim() : null,
    chatCount: 1
  };
}

function diagnosticWalletsFromEnv(value) {
  return String(value || "")
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const [address, label] = entry.split(":");
      return normalizeWallet({ address, label }, "diagnostic");
    })
    .filter(Boolean);
}

function mergeWallets(wallets) {
  const byAddress = new Map();

  for (const wallet of wallets) {
    if (!wallet?.address) {
      continue;
    }

    const roles = Array.isArray(wallet.roles)
      ? wallet.roles.filter(Boolean)
      : wallet.role
        ? [wallet.role]
        : [];
    const existing = byAddress.get(wallet.address);
    if (!existing) {
      byAddress.set(wallet.address, {
        ...wallet,
        roles: new Set(roles),
        labels: new Set(wallet.label ? [wallet.label] : []),
        chatCount: wallet.chatCount || 1
      });
      continue;
    }

    for (const role of roles) {
      existing.roles.add(role);
    }
    if (wallet.label) {
      existing.labels.add(wallet.label);
    }
    existing.chatCount += wallet.chatCount || 1;
  }

  return [...byAddress.values()]
    .map((wallet) => ({
      address: wallet.address,
      roles: [...wallet.roles].sort(),
      label: [...wallet.labels].sort()[0] || null,
      chatCount: wallet.chatCount
    }))
    .sort((a, b) => a.address.localeCompare(b.address));
}

function walletsFromSubscribers(subscribers) {
  const wallets = [];

  for (const subscriber of subscribers) {
    for (const wallet of subscriber.watchedWallets || []) {
      const normalized = normalizeWallet(wallet, "watched");
      if (normalized) {
        wallets.push(normalized);
      }
    }

    for (const wallet of subscriber.copyTradeWallets || []) {
      const normalized = normalizeWallet(wallet, "copytrade");
      if (normalized) {
        wallets.push(normalized);
      }
    }
  }

  return mergeWallets(wallets);
}

async function loadSupabaseWallets() {
  const url = process.env.SUPABASE_URL;
  const serviceRoleKey = process.env.SUPABASE_SERVICE_ROLE_KEY || process.env.SUPABASE_SERVICE_KEY || process.env.SUPABASE_SERVICE_ROLE;

  if (!url || !serviceRoleKey) {
    return null;
  }

  const client = createClient(url, serviceRoleKey, {
    auth: {
      persistSession: false,
      autoRefreshToken: false
    }
  });
  const [{ data: watchedRows, error: watchedError }, { data: copyTradeRows, error: copyTradeError }] = await Promise.all([
    client.from("telegram_watched_wallets").select("chat_id,address,label"),
    client.from("telegram_copytrade_wallets").select("chat_id,address,label")
  ]);

  if (watchedError || copyTradeError) {
    throw new Error(watchedError?.message || copyTradeError?.message || "Supabase wallet lookup failed");
  }

  return mergeWallets([
    ...(watchedRows || []).map((row) => normalizeWallet(row, "watched")),
    ...(copyTradeRows || []).map((row) => normalizeWallet(row, "copytrade"))
  ]);
}

function loadJsonSubscriberWallets(path) {
  if (!existsSync(path)) {
    return [];
  }

  const parsed = JSON.parse(readFileSync(path, "utf8"));
  const subscribers = Array.isArray(parsed) ? parsed : Object.values(parsed);
  return walletsFromSubscribers(subscribers);
}

async function loadActiveWallets({ includeDiagnostic }) {
  const liveWallets = await loadSupabaseWallets();
  const subscriberWallets = liveWallets || loadJsonSubscriberWallets(process.env.TELEGRAM_SUBSCRIBERS_PATH || DEFAULT_JSON_SUBSCRIBERS_PATH);
  const diagnosticWallets = includeDiagnostic ? diagnosticWalletsFromEnv(process.env.WALLET_FEED_DIAGNOSTIC_WALLETS) : [];

  return mergeWallets([...subscriberWallets, ...diagnosticWallets]);
}

function summarizeReadiness({ activeWallets, rows, sinceMs = null, role = "copytrade", includeDiagnostic = false }) {
  const selectedWallets = activeWallets.filter((wallet) => {
    if (!includeDiagnostic && wallet.roles.includes("diagnostic")) {
      return false;
    }

    return role === "all" || wallet.roles.includes(role);
  });
  const selectedAddresses = new Set(selectedWallets.map((wallet) => wallet.address));
  const selectedRows = rows.filter((row) => {
    if (!row || typeof row !== "object" || !selectedAddresses.has(row.targetWallet)) {
      return false;
    }

    if (!includeDiagnostic && row.raw?.diagnosticWallet) {
      return false;
    }

    return inWindow(row, { sinceMs });
  });
  const copyableRows = selectedRows.filter(isCopyableSolToTokenBuy);
  const byWallet = new Map(selectedWallets.map((wallet) => [
    wallet.address,
    {
      wallet,
      rows: 0,
      copyableRows: 0,
      providerCounts: new Map(),
      copyableProviderCounts: new Map(),
      matchedCopyableGroups: 0,
      isolatedCopyableGroups: new Map()
    }
  ]));
  const copyableGroups = new Map();

  for (const row of selectedRows) {
    const entry = byWallet.get(row.targetWallet);
    if (!entry) {
      continue;
    }

    entry.rows += 1;
    entry.providerCounts.set(row.provider, (entry.providerCounts.get(row.provider) || 0) + 1);

    if (!isCopyableSolToTokenBuy(row)) {
      continue;
    }

    entry.copyableRows += 1;
    entry.copyableProviderCounts.set(row.provider, (entry.copyableProviderCounts.get(row.provider) || 0) + 1);

    const key = tradeKey(row);
    if (key) {
      const group = copyableGroups.get(key) || [];
      group.push(row);
      copyableGroups.set(key, group);
    }
  }

  for (const group of copyableGroups.values()) {
    const providers = new Set(group.map((row) => row.provider).filter(Boolean));
    const wallet = group[0]?.targetWallet;
    const entry = wallet ? byWallet.get(wallet) : null;
    if (!entry) {
      continue;
    }

    if (providers.size > 1) {
      entry.matchedCopyableGroups += 1;
    } else if (providers.size === 1) {
      const provider = [...providers][0];
      entry.isolatedCopyableGroups.set(provider, (entry.isolatedCopyableGroups.get(provider) || 0) + 1);
    }
  }

  return {
    selectedWallets,
    selectedRows,
    copyableRows,
    wallets: [...byWallet.values()]
  };
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

async function main() {
  const walletPath = argValue("path", process.env.WALLET_TRADE_LOG_PATH || DEFAULT_WALLET_PATH);
  const since = argValue("since", process.env.REPORT_SINCE);
  const role = argValue("role", process.env.REPORT_ROLE || "copytrade");
  const includeDiagnostic = ["1", "true", "yes", "on"].includes(String(argValue("include-diagnostic", process.env.REPORT_INCLUDE_DIAGNOSTIC || "false")).toLowerCase());
  const sinceMs = since ? Date.parse(since) : null;

  if (role !== "copytrade" && role !== "watched" && role !== "all" && role !== "diagnostic") {
    throw new Error(`invalid --role: ${role}`);
  }

  if (since && !Number.isFinite(sinceMs)) {
    throw new Error(`invalid --since date: ${since}`);
  }

  const activeWallets = await loadActiveWallets({ includeDiagnostic: includeDiagnostic || role === "diagnostic" });
  const walletLog = await readJsonl(walletPath);
  const summary = summarizeReadiness({
    activeWallets,
    rows: walletLog.rows,
    sinceMs,
    role,
    includeDiagnostic: includeDiagnostic || role === "diagnostic"
  });

  console.log(
    `Wallet feed readiness | path=${walletPath} | role=${role}` +
      `${since ? ` | since=${since}` : ""}${includeDiagnostic ? " | includeDiagnostic=true" : ""}`
  );
  console.log(
    `ActiveWallets=${summary.selectedWallets.length} Rows=${summary.selectedRows.length} ` +
      `CopyableBuys=${summary.copyableRows.length} parseErrors=${walletLog.errors.length}`
  );
  console.log("\nWallet readiness:");
  for (const entry of summary.wallets) {
    console.log(
      `  ${short(entry.wallet.address)} roles=${entry.wallet.roles.join(",")} label=${entry.wallet.label || "n/a"} ` +
        `chats=${entry.wallet.chatCount} rows=${entry.rows} copyable=${entry.copyableRows} ` +
        `providers=${formatCounts(entry.providerCounts)} copyableProviders=${formatCounts(entry.copyableProviderCounts)} ` +
        `matchedCopyableGroups=${entry.matchedCopyableGroups} isolatedCopyableGroups=${formatCounts(entry.isolatedCopyableGroups)}`
    );
  }
  if (summary.wallets.length === 0) {
    console.log("  none");
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : String(error));
    process.exit(1);
  });
}

export {
  diagnosticWalletsFromEnv,
  isCopyableSolToTokenBuy,
  loadActiveWallets,
  readJsonl,
  mergeWallets,
  summarizeReadiness,
  walletsFromSubscribers
};
