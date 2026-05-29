#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { createClient } from "@supabase/supabase-js";

const DEFAULT_SERVICE = "pumpfun-migration-bot.service";
const DEFAULT_LIMIT = 500;

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

function numberValue(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function serviceRoleKey(env) {
  return env.SUPABASE_SERVICE_ROLE_KEY || env.SUPABASE_SERVICE_KEY || env.SUPABASE_SERVICE_ROLE || "";
}

function percentile(values, p) {
  if (values.length === 0) {
    return null;
  }

  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[index];
}

function median(values) {
  return percentile(values, 50);
}

function stats(values) {
  const numeric = values.filter((value) => Number.isFinite(value));
  if (numeric.length === 0) {
    return null;
  }

  const sum = numeric.reduce((total, value) => total + value, 0);
  return {
    count: numeric.length,
    avgMs: Math.round(sum / numeric.length),
    p50Ms: median(numeric),
    p90Ms: percentile(numeric, 90),
    minMs: Math.min(...numeric),
    maxMs: Math.max(...numeric)
  };
}

function executionProvider(row) {
  const provider = row.response?.provider || row.response?.metadata?.requestedProvider;
  if (provider) {
    return provider;
  }

  return row.signature || row.status === "submitted" ? "pumpportal-lightning" : "unknown";
}

function joinKey({ chatId, observedSignature, mint }) {
  return [chatId || "", observedSignature || "", mint || ""].join("|");
}

function observedSignature(row) {
  return row.observed_trade?.signature || row.response?.metadata?.observedSignature || null;
}

function parseLatencyLogs({ service, since }) {
  let output = "";
  try {
    output = execFileSync("journalctl", ["-u", service, "--since", since, "--no-pager"], {
      encoding: "utf8",
      maxBuffer: 100 * 1024 * 1024
    });
  } catch (error) {
    const stderr = error?.stderr ? String(error.stderr) : "";
    throw new Error(`journalctl failed: ${error.message}${stderr ? `\n${stderr}` : ""}`);
  }

  const latencies = [];
  for (const line of output.split(/\r?\n/)) {
    const marker = "Copy trade latency: ";
    const markerIndex = line.indexOf(marker);
    if (markerIndex === -1) {
      continue;
    }

    try {
      const latency = JSON.parse(line.slice(markerIndex + marker.length));
      latencies.push(latency);
    } catch {
      // Ignore malformed historical lines; the summary should be best effort.
    }
  }

  return latencies;
}

function formatMaybeStats(value) {
  if (!value) {
    return "n/a";
  }

  return `count=${value.count} avg=${value.avgMs}ms p50=${value.p50Ms}ms p90=${value.p90Ms}ms min=${value.minMs}ms max=${value.maxMs}ms`;
}

function groupKey(row) {
  return `${row.provider}:${row.action}:${row.status}`;
}

function summarize(rows) {
  const groups = new Map();
  for (const row of rows) {
    const key = groupKey(row);
    const group = groups.get(key) || [];
    group.push(row);
    groups.set(key, group);
  }

  return [...groups.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, group]) => {
      const [provider, action, status] = key.split(":");
      const signatures = group.filter((row) => row.signature).length;
      return {
        provider,
        action,
        status,
        rows: group.length,
        signatures,
        total: stats(group.map((row) => row.latency?.totalMs)),
        submit: stats(group.map((row) => row.latency?.stagesMs?.submit_started_to_submit_finished)),
        receiptToBuild: stats(group.map((row) => {
          if (!row.latency) {
            return null;
          }

          return (row.latency.stagesMs?.received_to_normalized || 0) +
            (row.latency.stagesMs?.normalized_to_request_built || 0);
        }))
      };
    });
}

function latestRows(rows, count = 12) {
  return rows
    .slice()
    .sort((a, b) => String(b.created_at).localeCompare(String(a.created_at)))
    .slice(0, count)
    .map((row) => ({
      id: row.id,
      created_at: row.created_at,
      provider: row.provider,
      route: row.response?.route || null,
      status: row.status,
      amount: row.amount,
      signature: row.signature || null,
      totalMs: row.latency?.totalMs ?? null,
      submitMs: row.latency?.stagesMs?.submit_started_to_submit_finished ?? null,
      observedSignature: row.observedSignature,
      error: row.error_text || null
    }));
}

async function main() {
  const env = { ...loadEnv(process.env.ENV_FILE || ".env"), ...process.env };
  const url = env.SUPABASE_URL;
  const key = serviceRoleKey(env);
  if (!url || !key) {
    throw new Error("SUPABASE_URL and service-role key are required");
  }

  const chatId = env.REPORT_CHAT_ID || "1355697770";
  const actionFilter = env.REPORT_ACTION || "buy";
  const since = env.REPORT_SINCE || "7 days ago";
  const limit = numberValue(env.REPORT_LIMIT) || DEFAULT_LIMIT;
  const service = env.REPORT_SERVICE || DEFAULT_SERVICE;
  const supabase = createClient(url, key, {
    auth: {
      persistSession: false,
      autoRefreshToken: false
    }
  });

  let query = supabase
    .from("telegram_copytrade_executions")
    .select("id,chat_id,source_wallet_address,trading_wallet_public_key,mint,action,amount,status,signature,error_text,observed_trade,request,response,created_at")
    .eq("chat_id", chatId)
    .order("created_at", { ascending: false })
    .limit(limit);

  if (actionFilter !== "all") {
    query = query.eq("action", actionFilter);
  }

  const { data, error } = await query;

  if (error) {
    throw error;
  }

  const latencies = parseLatencyLogs({ service, since });
  const latencyBySignature = new Map();
  const latencyByKey = new Map();
  for (const latency of latencies) {
    if (latency.signature) {
      latencyBySignature.set(latency.signature, latency);
    }

    const key = joinKey({
      chatId: latency.chatId,
      observedSignature: latency.observedSignature,
      mint: latency.mint
    });
    const group = latencyByKey.get(key) || [];
    group.push(latency);
    latencyByKey.set(key, group);
  }

  const rows = (data || []).map((row) => {
    const observed = observedSignature(row);
    const fallbackLatencies = latencyByKey.get(joinKey({
      chatId: row.chat_id,
      observedSignature: observed,
      mint: row.mint
    })) || [];
    const latency = row.signature
      ? latencyBySignature.get(row.signature) || null
      : fallbackLatencies.length === 1 ? fallbackLatencies[0] : null;

    return {
      ...row,
      observedSignature: observed,
      provider: executionProvider(row),
      latency
    };
  });

  const summary = summarize(rows);
  const output = {
    generatedAt: new Date().toISOString(),
    chatId,
    action: actionFilter,
    since,
    executionRows: rows.length,
    latencyLogs: latencies.length,
    joinedRows: rows.filter((row) => row.latency).length,
    summary,
    latest: latestRows(rows)
  };

  if (env.REPORT_JSON === "true") {
    console.log(JSON.stringify(output, null, 2));
    return;
  }

  console.log(`Copytrade performance report | chat=${chatId} | action=${actionFilter} | since=${since}`);
  console.log(`Execution rows=${output.executionRows} | latency logs=${output.latencyLogs} | joined=${output.joinedRows}`);
  console.log("");
  for (const row of summary) {
    console.log(`${row.provider} / ${row.action} / ${row.status}`);
    console.log(`  rows=${row.rows} signatures=${row.signatures}`);
    console.log(`  total: ${formatMaybeStats(row.total)}`);
    console.log(`  submit: ${formatMaybeStats(row.submit)}`);
    console.log(`  receipt->build: ${formatMaybeStats(row.receiptToBuild)}`);
  }
  console.log("");
  console.log("Latest rows:");
  for (const row of output.latest) {
    const total = row.totalMs === null ? "n/a" : `${row.totalMs}ms`;
    const submit = row.submitMs === null ? "n/a" : `${row.submitMs}ms`;
    console.log(
      `  #${row.id} ${row.created_at} ${row.provider}/${row.status} amount=${row.amount} ` +
      `total=${total} submit=${submit} sig=${row.signature || "none"}`
    );
    if (row.error) {
      console.log(`    error=${row.error}`);
    }
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
