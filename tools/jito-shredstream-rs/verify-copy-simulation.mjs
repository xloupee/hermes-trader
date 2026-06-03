#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";

const DEFAULT_EXECUTIONS_PATH = "/tmp/jito-copy-executions-local.jsonl";

function argValue(name, fallback = null) {
  const prefix = `--${name}=`;
  const match = process.argv.find((arg) => arg.startsWith(prefix));
  return match ? match.slice(prefix.length) : fallback;
}

function readJsonl(path) {
  if (!existsSync(path)) {
    throw new Error(`execution log not found: ${path}`);
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

function latestRelevant(rows, observedSignature) {
  const candidates = rows.filter((row) => {
    if (observedSignature) {
      return row.observedSignature === observedSignature;
    }
    return row.signed || row.simulated || row.decision === "simulated";
  });

  if (candidates.length === 0) {
    throw new Error(
      observedSignature
        ? `no simulation row for observed signature ${observedSignature}`
        : "no signed simulation row found"
    );
  }

  return candidates.at(-1);
}

function requireTrue(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function main() {
  const path = argValue("executions", process.env.JITO_COPY_EXECUTIONS_PATH || DEFAULT_EXECUTIONS_PATH);
  const observedSignature = argValue("observed-signature");
  const rows = readJsonl(path);
  const row = latestRelevant(rows, observedSignature);

  requireTrue(row.schema === "copytrade.localExecution.v1", "unexpected execution schema");
  requireTrue(row.simulationRequested === true, "simulation was not requested");
  requireTrue(row.sendEnabled === false, "send was enabled during simulation gate");
  requireTrue(row.dryRun === true, "dry run was not enabled during simulation gate");
  requireTrue(row.signed === true, "copy transaction was not signed");
  requireTrue(row.simulated === true, "copy transaction was not simulated");
  requireTrue(row.sent === false, "simulation gate unexpectedly sent a transaction");
  requireTrue(row.decision === "simulated", `simulation decision was ${row.decision}`);
  requireTrue(!row.simulationError, `simulation error: ${JSON.stringify(row.simulationError)}`);
  requireTrue(row.routeLayout === "direct-pump", `unsupported route layout ${row.routeLayout}`);
  requireTrue(row.instructionCount === 3, `unexpected instruction count ${row.instructionCount}`);
  requireTrue(Number(row.observedSolAmount) > 0, "missing observed SOL amount");
  requireTrue(Number(row.maxCopySol) <= 0.001, `max copy SOL guard too high: ${row.maxCopySol}`);
  requireTrue(row.copySignature, "missing local copy signature");
  requireTrue(Number.isFinite(Number(row.observedToSignedMs)), "missing observedToSignedMs");
  requireTrue(
    Number.isFinite(Number(row.observedToSimulationCompletedMs)),
    "missing observedToSimulationCompletedMs"
  );

  console.log(
    JSON.stringify(
      {
        ok: true,
        observedSignature: row.observedSignature,
        copySignature: row.copySignature,
        copyWallet: row.copyWallet,
        mint: row.mint,
        observedSolAmount: row.observedSolAmount,
        maxCopySol: row.maxCopySol,
        routeLayout: row.routeLayout,
        instructionCount: row.instructionCount,
        simulationUnitsConsumed: row.simulationUnitsConsumed ?? null,
        observedToSignedMs: row.observedToSignedMs,
        observedToSimulationCompletedMs: row.observedToSimulationCompletedMs,
        executionLog: path
      },
      null,
      2
    )
  );
}

try {
  main();
} catch (error) {
  console.error(error.message);
  process.exit(1);
}
