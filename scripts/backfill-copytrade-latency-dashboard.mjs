#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { createClient } from "@supabase/supabase-js";

const DEFAULT_SERVICE = "pumpfun-migration-bot.service";
const DEFAULT_SINCE = "7 days ago";

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

function argValue(name, fallback = null) {
  const prefix = `--${name}=`;
  const inline = process.argv.find((arg) => arg.startsWith(prefix));
  if (inline) {
    return inline.slice(prefix.length);
  }

  const index = process.argv.indexOf(`--${name}`);
  if (index >= 0 && process.argv[index + 1]) {
    return process.argv[index + 1];
  }

  return fallback;
}

function hasFlag(name) {
  return process.argv.includes(`--${name}`);
}

function serviceRoleKey(env) {
  return env.SUPABASE_SERVICE_ROLE_KEY || env.SUPABASE_SERVICE_KEY || env.SUPABASE_SERVICE_ROLE || "";
}

function parseLatencySummaries({ service, since }) {
  const output = execFileSync("journalctl", ["-u", service, "--since", since, "--no-pager"], {
    encoding: "utf8",
    maxBuffer: 100 * 1024 * 1024
  });
  const summaries = [];

  for (const line of output.split(/\r?\n/)) {
    const marker = "Copy trade latency summary: ";
    const markerIndex = line.indexOf(marker);
    if (markerIndex === -1) {
      continue;
    }

    try {
      const summary = JSON.parse(line.slice(markerIndex + marker.length));
      if (summary?.event === "copy_trade_latency_summary") {
        summaries.push(summary);
      }
    } catch {
      // Ignore malformed historical rows.
    }
  }

  return summaries;
}

function latestBySignature(summaries) {
  const bySignature = new Map();
  for (const summary of summaries) {
    if (!summary.signature) {
      continue;
    }
    bySignature.set(summary.signature, summary);
  }
  return [...bySignature.values()];
}

function latencyColumns(summary) {
  return {
    observed_signature: summary.observedSignature ?? null,
    target_observed_to_submit_ms: summary.targetObservedToSubmitMs ?? null,
    target_blocktime_to_submit_ms: summary.targetBlockTimeToSubmitMs ?? null,
    build_ms: summary.buildMs ?? null,
    send_ms: summary.sendMs ?? null,
    winner_provider: summary.winnerProvider ?? null,
    send_rpc_winner: summary.sendRpcWinner ?? null,
    send_rpc_count: summary.sendRpcCount ?? null,
    target_slot: summary.targetSlot ?? null,
    copy_slot: summary.copySlot ?? null,
    slot_delta: summary.slotDelta ?? null,
    latency_status: summary.status ?? null,
    latency_summary: summary
  };
}

async function main() {
  const env = { ...loadEnv(process.env.ENV_FILE || ".env"), ...process.env };
  const url = env.SUPABASE_URL;
  const key = serviceRoleKey(env);
  if (!url || !key) {
    throw new Error("SUPABASE_URL and a Supabase service role key are required");
  }

  const service = argValue("service", process.env.REPORT_SERVICE || DEFAULT_SERVICE);
  const since = argValue("since", process.env.REPORT_SINCE || DEFAULT_SINCE);
  const dryRun = hasFlag("dry-run");
  const summaries = latestBySignature(parseLatencySummaries({ service, since }));

  if (dryRun) {
    console.log(JSON.stringify({
      dryRun: true,
      service,
      since,
      summaries: summaries.length,
      signatures: summaries.slice(0, 20).map((summary) => summary.signature)
    }, null, 2));
    return;
  }

  const supabase = createClient(url, key, {
    auth: {
      persistSession: false,
      autoRefreshToken: false
    }
  });
  let updated = 0;
  let failed = 0;

  for (const summary of summaries) {
    const { error } = await supabase
      .from("telegram_copytrade_executions")
      .update(latencyColumns(summary))
      .eq("signature", summary.signature);

    if (error) {
      failed += 1;
      console.warn(`Failed to backfill ${summary.signature}: ${error.message}`);
      continue;
    }

    updated += 1;
  }

  console.log(JSON.stringify({
    service,
    since,
    summaries: summaries.length,
    updated,
    failed
  }, null, 2));
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
