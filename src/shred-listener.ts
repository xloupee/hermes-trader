import "dotenv/config";
import { createReadStream, mkdirSync } from "node:fs";
import { appendFile } from "node:fs/promises";
import { dirname } from "node:path";
import { createInterface } from "node:readline";
import { pathToFileURL } from "node:url";
import { normalizeShredstreamTransaction, type ShredstreamTransactionInput } from "./shredstream-decoder.js";

export const DEFAULT_SHREDSTREAM_EVENT_LOG_PATH = "logs/shred-pump-events.jsonl";

export interface ShredListenerStats {
  linesRead: number;
  recordsAccepted: number;
  eventsWritten: number;
  parseErrors: number;
}

export function shredDiscoveryEnabled(env: NodeJS.ProcessEnv = process.env): boolean {
  return env.SHREDSTREAM_DISCOVERY_ENABLED === "true";
}

export async function runShredListener({
  inputPath,
  eventLogPath
}: {
  inputPath: string;
  eventLogPath: string;
}): Promise<ShredListenerStats> {
  const stats: ShredListenerStats = {
    linesRead: 0,
    recordsAccepted: 0,
    eventsWritten: 0,
    parseErrors: 0
  };

  mkdirSync(dirname(eventLogPath), { recursive: true });

  for await (const line of readJsonlLines(inputPath)) {
    stats.linesRead += 1;

    const trimmed = line.trim();

    if (!trimmed) {
      continue;
    }

    const transaction = parseShredstreamTransaction(trimmed);

    if (!transaction) {
      stats.parseErrors += 1;
      continue;
    }

    stats.recordsAccepted += 1;

    for (const event of normalizeShredstreamTransaction(transaction)) {
      await appendFile(eventLogPath, `${JSON.stringify(event)}\n`, "utf8");
      stats.eventsWritten += 1;
    }
  }

  return stats;
}

async function* readJsonlLines(inputPath: string): AsyncIterable<string> {
  const input = inputPath === "-" ? process.stdin : createReadStream(inputPath, { encoding: "utf8" });
  const reader = createInterface({ input, crlfDelay: Infinity });

  for await (const line of reader) {
    yield line;
  }
}

function parseShredstreamTransaction(line: string): ShredstreamTransactionInput | null {
  try {
    const value: unknown = JSON.parse(line);
    return isShredstreamTransaction(value) ? value : null;
  } catch {
    return null;
  }
}

function isShredstreamTransaction(value: unknown): value is ShredstreamTransactionInput {
  if (!value || typeof value !== "object") {
    return false;
  }

  const record = value as Record<string, unknown>;

  return (
    typeof record.slot === "number" &&
    typeof record.signature === "string" &&
    Array.isArray(record.accountKeys) &&
    record.accountKeys.every((accountKey) => typeof accountKey === "string") &&
    Array.isArray(record.instructions) &&
    record.instructions.every(isShredstreamInstruction)
  );
}

function isShredstreamInstruction(value: unknown): boolean {
  if (!value || typeof value !== "object") {
    return false;
  }

  const record = value as Record<string, unknown>;
  const programIdValid = record.programId === undefined || typeof record.programId === "string";
  const programIdIndexValid =
    record.programIdIndex === undefined || (Number.isInteger(record.programIdIndex) && Number(record.programIdIndex) >= 0);
  const dataValid = record.dataBase64 === undefined || typeof record.dataBase64 === "string";
  const accountsValid =
    record.accounts === undefined ||
    (Array.isArray(record.accounts) &&
      record.accounts.every(
        (account) => typeof account === "string" || (Number.isInteger(account) && Number(account) >= 0)
      ));

  return programIdValid && programIdIndexValid && dataValid && accountsValid;
}

async function main(): Promise<void> {
  if (!shredDiscoveryEnabled()) {
    console.log("ShredStream discovery listener disabled. Set SHREDSTREAM_DISCOVERY_ENABLED=true to run this prototype.");
    return;
  }

  const inputPath = process.env.SHREDSTREAM_INPUT_PATH || "-";
  const eventLogPath = process.env.SHREDSTREAM_EVENT_LOG_PATH || DEFAULT_SHREDSTREAM_EVENT_LOG_PATH;

  console.log(`Starting ShredStream prototype listener: input=${inputPath}, eventLog=${eventLogPath}`);
  console.log("Prototype mode only: no Telegram bot wiring and no live trades.");

  const stats = await runShredListener({ inputPath, eventLogPath });

  console.log(
    `ShredStream listener finished: linesRead=${stats.linesRead}, recordsAccepted=${stats.recordsAccepted}, eventsWritten=${stats.eventsWritten}, parseErrors=${stats.parseErrors}`
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error: unknown) => {
    console.error(`ShredStream listener failed: ${error instanceof Error ? error.message : String(error)}`);
    process.exitCode = 1;
  });
}
