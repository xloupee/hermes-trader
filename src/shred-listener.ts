import "dotenv/config";
import { mkdirSync } from "node:fs";
import { appendFile } from "node:fs/promises";
import { dirname } from "node:path";
import { pathToFileURL } from "node:url";
import { normalizeShredstreamTransaction } from "./shredstream-decoder.js";
import { createJsonlShredstreamSource, createShredstreamSourceFromEnv, type ShredstreamSource } from "./shredstream-source.js";

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
  eventLogPath,
  source
}: {
  inputPath?: string;
  eventLogPath: string;
  source?: ShredstreamSource;
}): Promise<ShredListenerStats> {
  const selectedSource = source || createJsonlShredstreamSource(inputPath || "-");
  const stats: ShredListenerStats = {
    linesRead: 0,
    recordsAccepted: 0,
    eventsWritten: 0,
    parseErrors: 0
  };

  mkdirSync(dirname(eventLogPath), { recursive: true });

  for await (const record of selectedSource.readRecords()) {
    stats.linesRead += 1;

    if (!record.transaction) {
      if (record.parseError) {
        stats.parseErrors += 1;
      }
      continue;
    }

    const transaction = record.transaction;

    stats.recordsAccepted += 1;

    for (const event of normalizeShredstreamTransaction(transaction)) {
      await appendFile(eventLogPath, `${JSON.stringify(event)}\n`, "utf8");
      stats.eventsWritten += 1;
    }
  }

  return stats;
}

async function main(): Promise<void> {
  if (!shredDiscoveryEnabled()) {
    console.log("ShredStream discovery listener disabled. Set SHREDSTREAM_DISCOVERY_ENABLED=true to run this prototype.");
    return;
  }

  const eventLogPath = process.env.SHREDSTREAM_EVENT_LOG_PATH || DEFAULT_SHREDSTREAM_EVENT_LOG_PATH;
  const source = createShredstreamSourceFromEnv();

  console.log(`Starting ShredStream prototype listener: source=${source.describe()}, eventLog=${eventLogPath}`);
  console.log("Prototype mode only: no Telegram bot wiring and no live trades.");

  const stats = await runShredListener({ source, eventLogPath });

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
