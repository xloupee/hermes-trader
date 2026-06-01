#!/usr/bin/env node
import { readFileSync } from "node:fs";

function argValue(name, fallback = null) {
  const index = process.argv.indexOf(name);
  return index >= 0 && process.argv[index + 1] ? process.argv[index + 1] : fallback;
}

function providerLabel(provider) {
  return provider === "yellowstone" ? "geyser" : String(provider || "unknown");
}

function parseJsonFromLine(line) {
  const trimmed = line.trim();
  if (!trimmed) {
    return null;
  }

  const jsonStart = trimmed.indexOf("{");
  if (jsonStart < 0) {
    return null;
  }

  try {
    return JSON.parse(trimmed.slice(jsonStart));
  } catch {
    return null;
  }
}

function readJsonLines(path) {
  try {
    return readFileSync(path, "utf8")
      .split(/\r?\n/)
      .map(parseJsonFromLine)
      .filter(Boolean);
  } catch (error) {
    if (error && error.code === "ENOENT") {
      return [];
    }
    throw error;
  }
}

function percentile(values, p) {
  if (values.length === 0) {
    return null;
  }

  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[index];
}

function ms(value) {
  return value === null || value === undefined ? "n/a" : `${Math.round(value)}ms`;
}

function tradeKey(trade) {
  return [
    trade.signature || "no-signature",
    trade.targetWallet || "no-wallet",
    trade.mint || "no-mint"
  ].join("|");
}

function tradeObservedMs(trade) {
  const observed = Date.parse(trade.observedAt || "");
  return Number.isFinite(observed) ? observed : null;
}

function tradeSourceMs(trade) {
  const timestamp = Number(trade.timestamp);
  return Number.isFinite(timestamp) && timestamp > 0 ? timestamp * 1000 : null;
}

function summarizeRejects(path) {
  if (!path) {
    return { total: null, reasons: new Map() };
  }

  const reasons = new Map();
  let total = 0;

  for (const line of readFileSync(path, "utf8").split(/\r?\n/)) {
    if (!line.includes("Yellowstone wallet trade rejected")) {
      continue;
    }

    const parsed = parseJsonFromLine(line);
    const reason = parsed?.reason || "unknown";
    reasons.set(reason, (reasons.get(reason) || 0) + 1);
    total += 1;
  }

  return { total, reasons };
}

const logPath = argValue("--log", "logs/wallet-trades.jsonl");
const appLogPath = argValue("--app-log");
const trades = readJsonLines(logPath)
  .filter((trade) => trade && trade.signature && trade.targetWallet && trade.mint)
  .map((trade) => ({
    ...trade,
    reportProvider: providerLabel(trade.provider),
    observedMs: tradeObservedMs(trade),
    sourceMs: tradeSourceMs(trade)
  }))
  .filter((trade) => trade.observedMs !== null);

const groups = new Map();
for (const trade of trades) {
  const key = tradeKey(trade);
  const entries = groups.get(key) || [];
  entries.push(trade);
  groups.set(key, entries);
}

const providerCounts = new Map();
const firstProviderCounts = new Map();
const duplicateCounts = new Map();
const observedLagByProvider = new Map();
const relativeLagByProvider = new Map();
const parserCounts = new Map();
const providersSeen = new Set();

for (const entries of groups.values()) {
  entries.sort((a, b) => a.observedMs - b.observedMs);
  const first = entries[0];
  firstProviderCounts.set(first.reportProvider, (firstProviderCounts.get(first.reportProvider) || 0) + 1);

  const perProviderFirst = new Map();
  const perProviderSeen = new Map();
  for (const entry of entries) {
    providersSeen.add(entry.reportProvider);
    providerCounts.set(entry.reportProvider, (providerCounts.get(entry.reportProvider) || 0) + 1);
    perProviderSeen.set(entry.reportProvider, (perProviderSeen.get(entry.reportProvider) || 0) + 1);

    if (!perProviderFirst.has(entry.reportProvider)) {
      perProviderFirst.set(entry.reportProvider, entry);
    }

    const parserKey = [entry.reportProvider, entry.raw?.parser || entry.raw?.source || entry.source || "unknown"].join(":");
    parserCounts.set(parserKey, (parserCounts.get(parserKey) || 0) + 1);
  }

  for (const [provider, count] of perProviderSeen) {
    if (count > 1) {
      duplicateCounts.set(provider, (duplicateCounts.get(provider) || 0) + count - 1);
    }
  }

  for (const [provider, entry] of perProviderFirst) {
    const relative = entry.observedMs - first.observedMs;
    const relativeValues = relativeLagByProvider.get(provider) || [];
    relativeValues.push(relative);
    relativeLagByProvider.set(provider, relativeValues);

    if (entry.sourceMs) {
      const observedValues = observedLagByProvider.get(provider) || [];
      observedValues.push(entry.observedMs - entry.sourceMs);
      observedLagByProvider.set(provider, observedValues);
    }
  }
}

const providers = [...providersSeen].sort();
const missingCounts = new Map(providers.map((provider) => [provider, 0]));
for (const entries of groups.values()) {
  const present = new Set(entries.map((entry) => entry.reportProvider));
  for (const provider of providers) {
    if (!present.has(provider)) {
      missingCounts.set(provider, (missingCounts.get(provider) || 0) + 1);
    }
  }
}

const rejects = summarizeRejects(appLogPath);
const matchedExamples = [...groups.values()]
  .filter((entries) => new Set(entries.map((entry) => entry.reportProvider)).size > 1)
  .slice(0, 3);
const fallbackExamples = matchedExamples.length >= 3
  ? matchedExamples
  : [...matchedExamples, ...[...groups.values()].filter((entries) => !matchedExamples.includes(entries)).slice(0, 3 - matchedExamples.length)];

console.log(`Wallet feed latency report`);
console.log(`log=${logPath}`);
console.log(`trades=${trades.length} groups=${groups.size} providers=${providers.join(",") || "none"}`);
console.log("");

console.log("Provider counts");
for (const provider of providers) {
  console.log([
    `- ${provider}`,
    `events=${providerCounts.get(provider) || 0}`,
    `first=${firstProviderCounts.get(provider) || 0}`,
    `duplicates=${duplicateCounts.get(provider) || 0}`,
    `missing=${missingCounts.get(provider) || 0}`
  ].join(" "));
}

console.log("");
console.log("Lag by provider");
for (const provider of providers) {
  const observed = observedLagByProvider.get(provider) || [];
  const relative = relativeLagByProvider.get(provider) || [];
  console.log([
    `- ${provider}`,
    `sourceLagP50=${ms(percentile(observed, 50))}`,
    `sourceLagP90=${ms(percentile(observed, 90))}`,
    `relativeP50=${ms(percentile(relative, 50))}`,
    `relativeP90=${ms(percentile(relative, 90))}`
  ].join(" "));
}

console.log("");
console.log("Parser/source counts");
for (const [key, count] of [...parserCounts.entries()].sort()) {
  console.log(`- ${key} ${count}`);
}

console.log("");
if (rejects.total === null) {
  console.log("Geyser rejects: n/a (pass --app-log with captured service logs to count Yellowstone parser rejections)");
} else {
  console.log(`Geyser rejects: ${rejects.total}`);
  for (const [reason, count] of [...rejects.reasons.entries()].sort()) {
    console.log(`- ${reason} ${count}`);
  }
}

console.log("");
console.log("Examples");
for (const entries of fallbackExamples) {
  const first = entries[0];
  const providerSummary = entries.map((entry) => `${entry.reportProvider}@${new Date(entry.observedMs).toISOString()}`).join(", ");
  console.log(`- signature=${first.signature} wallet=${first.targetWallet} mint=${first.mint} providers=${providerSummary}`);
}

