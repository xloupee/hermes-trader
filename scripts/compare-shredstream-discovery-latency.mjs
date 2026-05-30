#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";
import {
  compareDiscoveryLatency,
  summarizeDiscoveryLatency
} from "../dist/shredstream-latency.js";

function usage() {
  return [
    "Usage:",
    "  node scripts/compare-shredstream-discovery-latency.mjs --pumpportal <path> --shredstream <path> [--comparisons-out <path>] [--create-window-ms <ms>]",
    "",
    "Defaults:",
    "  --pumpportal logs/pumpportal-discovery-events.jsonl",
    "  --shredstream logs/shred-pump-events.jsonl"
  ].join("\n");
}

function parseArgs(argv) {
  const args = {
    pumpportal: "logs/pumpportal-discovery-events.jsonl",
    shredstream: "logs/shred-pump-events.jsonl",
    comparisonsOut: null,
    createWindowMs: 2500
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];

    if (arg === "--help" || arg === "-h") {
      console.log(usage());
      process.exit(0);
    }

    if (arg === "--pumpportal" && next) {
      args.pumpportal = next;
      index += 1;
      continue;
    }

    if (arg === "--shredstream" && next) {
      args.shredstream = next;
      index += 1;
      continue;
    }

    if (arg === "--comparisons-out" && next) {
      args.comparisonsOut = next;
      index += 1;
      continue;
    }

    if (arg === "--create-window-ms" && next) {
      const parsed = Number(next);
      if (!Number.isFinite(parsed) || parsed < 0) {
        throw new Error(`Invalid --create-window-ms value: ${next}`);
      }
      args.createWindowMs = parsed;
      index += 1;
      continue;
    }

    throw new Error(`Unknown or incomplete argument: ${arg}\n${usage()}`);
  }

  return args;
}

async function readJsonl(path) {
  const text = await readFile(path, "utf8").catch((error) => {
    if (error?.code === "ENOENT") {
      return "";
    }
    throw error;
  });

  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line));
}

const args = parseArgs(process.argv.slice(2));
const [pumpPortalEvents, shredstreamEvents] = await Promise.all([
  readJsonl(args.pumpportal),
  readJsonl(args.shredstream)
]);
const comparisons = compareDiscoveryLatency({
  pumpPortalEvents,
  shredstreamEvents,
  createWindowMs: args.createWindowMs
});
const summary = summarizeDiscoveryLatency({ pumpPortalEvents, shredstreamEvents, comparisons });

if (args.comparisonsOut) {
  await writeFile(args.comparisonsOut, comparisons.map((comparison) => JSON.stringify(comparison)).join("\n") + "\n");
}

console.log(JSON.stringify({
  event: "shredstream_discovery_latency_summary",
  pumpPortalPath: args.pumpportal,
  shredstreamPath: args.shredstream,
  createWindowMs: args.createWindowMs,
  pumpPortalCount: pumpPortalEvents.length,
  shredstreamCount: shredstreamEvents.length,
  ...summary
}, null, 2));
