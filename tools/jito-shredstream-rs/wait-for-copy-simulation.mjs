#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { spawnSync } from "node:child_process";

const DEFAULT_EXECUTIONS_PATH = "/tmp/jito-copy-executions-local.jsonl";
const DEFAULT_TIMEOUT_MS = 120_000;
const DEFAULT_INTERVAL_MS = 500;

function argValue(name, fallback = null) {
  const prefix = `--${name}=`;
  const match = process.argv.find((arg) => arg.startsWith(prefix));
  return match ? match.slice(prefix.length) : fallback;
}

function booleanArg(name, fallback = false) {
  const value = argValue(name);
  if (value === null) {
    return fallback;
  }
  return ["1", "true", "yes", "on"].includes(String(value).toLowerCase());
}

function numberValue(value, fallback) {
  if (value === null || value === undefined || value === "") {
    return fallback;
  }

  const parsed = Number(value);
  return Number.isFinite(parsed) && parsed >= 0 ? parsed : fallback;
}

function readRows(path) {
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
    });
}

function relevantRow(rows, observedSignature) {
  const candidates = rows.filter((row) => {
    if (observedSignature) {
      return row.observedSignature === observedSignature;
    }
    return row.signed || row.simulated || row.decision === "simulated";
  });

  return candidates.at(-1) ?? null;
}

function runVerifier(path, observedSignature) {
  const args = ["tools/jito-shredstream-rs/verify-copy-simulation.mjs", `--executions=${path}`];
  if (observedSignature) {
    args.push(`--observed-signature=${observedSignature}`);
  }

  const result = spawnSync(process.execPath, args, {
    cwd: process.cwd(),
    stdio: "inherit"
  });

  process.exit(result.status ?? 1);
}

async function main() {
  const path = argValue("executions", process.env.JITO_COPY_EXECUTIONS_PATH || DEFAULT_EXECUTIONS_PATH);
  const observedSignature = argValue("observed-signature");
  const includeExisting = booleanArg("include-existing", false);
  const timeoutMs = numberValue(argValue("timeout-ms"), DEFAULT_TIMEOUT_MS);
  const intervalMs = numberValue(argValue("interval-ms"), DEFAULT_INTERVAL_MS);
  const baselineCount = includeExisting ? 0 : readRows(path).length;
  const startedAt = Date.now();
  let lastCount = -1;

  console.error(`waiting for copy simulation row in ${path}`);
  if (baselineCount > 0) {
    console.error(`ignoring ${baselineCount} existing execution row(s)`);
  }

  while (Date.now() - startedAt <= timeoutMs) {
    const rows = readRows(path);
    if (rows.length !== lastCount) {
      console.error(`execution rows: ${rows.length}`);
      lastCount = rows.length;
    }

    const row = relevantRow(rows.slice(baselineCount), observedSignature);
    if (row) {
      runVerifier(path, observedSignature);
      return;
    }

    await sleep(intervalMs);
  }

  console.error(`timed out after ${timeoutMs}ms waiting for copy simulation row`);
  process.exit(1);
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
