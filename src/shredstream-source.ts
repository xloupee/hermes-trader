import { createReadStream } from "node:fs";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import type { ShredstreamTransactionInput } from "./shredstream-decoder.js";

export type ShredstreamSourceMode = "jsonl" | "grpc";

export interface ShredstreamSourceConfig {
  mode: ShredstreamSourceMode;
  inputPath?: string;
  grpcUrl?: string;
  decoderCommand?: string;
}

export interface ShredstreamSourceRecord {
  transaction: ShredstreamTransactionInput;
  parseError?: never;
}

export interface ShredstreamInvalidSourceRecord {
  transaction?: never;
  parseError: string;
}

export type ShredstreamRecord = ShredstreamSourceRecord | ShredstreamInvalidSourceRecord;

export interface ShredstreamSource {
  readRecords: () => AsyncIterable<ShredstreamRecord>;
  describe: () => string;
}

export function parseShredstreamSourceMode(env: NodeJS.ProcessEnv = process.env): ShredstreamSourceMode {
  const value = env.SHREDSTREAM_SOURCE;

  if (!value || value.toLowerCase() === "jsonl") {
    return "jsonl";
  }

  if (value.toLowerCase() === "grpc") {
    return "grpc";
  }

  throw new Error(`Invalid SHREDSTREAM_SOURCE="${value}". Expected "jsonl" or "grpc".`);
}

export function resolveShredstreamSourceConfig(env: NodeJS.ProcessEnv = process.env): ShredstreamSourceConfig {
  const mode = parseShredstreamSourceMode(env);
  const config: ShredstreamSourceConfig = { mode };

  if (mode === "jsonl") {
    config.inputPath = env.SHREDSTREAM_INPUT_PATH || "-";
    return config;
  }

  if (!env.SHREDSTREAM_GRPC_URL) {
    throw new Error("SHREDSTREAM_GRPC_URL is required when SHREDSTREAM_SOURCE=grpc.");
  }

  config.grpcUrl = env.SHREDSTREAM_GRPC_URL;
  config.decoderCommand = env.SHREDSTREAM_DECODER_CMD;
  return config;
}

export function createShredstreamSourceFromEnv(env: NodeJS.ProcessEnv = process.env): ShredstreamSource {
  return createShredstreamSource(resolveShredstreamSourceConfig(env));
}

export function createShredstreamSource(config: ShredstreamSourceConfig): ShredstreamSource {
  if (config.mode === "jsonl") {
    return createJsonlShredstreamSource(config.inputPath || "-");
  }

  if (!config.grpcUrl) {
    throw new Error("SHREDSTREAM_GRPC_URL is required when SHREDSTREAM_SOURCE=grpc.");
  }

  return createGrpcShredstreamSource(config.grpcUrl, config.decoderCommand);
}

export function createJsonlShredstreamSource(inputPath: string): ShredstreamSource {
  return {
    describe: () => `jsonl:${inputPath}`,
    readRecords: () => readJsonlTransactions(inputPath)
  };
}

export function createGrpcShredstreamSource(grpcUrl: string, decoderCommand?: string): ShredstreamSource {
  const command = resolveGrpcDecoderCommand(grpcUrl, decoderCommand);

  return {
    describe: () => `grpc:${grpcUrl}`,
    readRecords: () => readCommandJsonlTransactions(command)
  };
}

function resolveGrpcDecoderCommand(grpcUrl: string, decoderCommand?: string): string {
  if (decoderCommand) {
    return decoderCommand.includes("{grpcUrl}")
      ? decoderCommand.replaceAll("{grpcUrl}", shellQuote(grpcUrl))
      : decoderCommand;
  }

  return `cargo run --manifest-path tools/shredstream-rs/Cargo.toml --quiet -- watch --grpc-url ${shellQuote(grpcUrl)}`;
}

async function* readJsonlTransactions(inputPath: string): AsyncIterable<ShredstreamRecord> {
  for await (const line of readJsonlLines(inputPath)) {
    const trimmed = line.trim();

    if (!trimmed) {
      continue;
    }

    yield parseShredstreamTransactionLine(trimmed);
  }
}

async function* readCommandJsonlTransactions(command: string): AsyncIterable<ShredstreamRecord> {
  const child = spawn(command, {
    shell: true,
    stdio: ["ignore", "pipe", "pipe"]
  });
  const stderr: string[] = [];

  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string) => stderr.push(chunk));

  const reader = createInterface({ input: child.stdout, crlfDelay: Infinity });

  for await (const line of reader) {
    const trimmed = line.trim();

    if (!trimmed) {
      continue;
    }

    yield parseShredstreamTransactionLine(trimmed);
  }

  const exitCode = await waitForChild(child);

  if (exitCode !== 0) {
    const details = stderr.join("").trim();
    throw new Error(`ShredStream decoder command exited with code ${exitCode}${details ? `: ${details}` : ""}`);
  }
}

function parseShredstreamTransactionLine(line: string): ShredstreamRecord {
  try {
    const value: unknown = JSON.parse(line);

    if (!isShredstreamTransaction(value)) {
      return { parseError: "JSONL record does not match ShredstreamTransactionInput" };
    }

    return { transaction: value };
  } catch {
    return { parseError: "JSONL record is not valid JSON" };
  }
}

function waitForChild(child: ReturnType<typeof spawn>): Promise<number | null> {
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (code) => resolve(code));
  });
}

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

async function* readJsonlLines(inputPath: string): AsyncIterable<string> {
  const input = inputPath === "-" ? process.stdin : createReadStream(inputPath, { encoding: "utf8" });
  const reader = createInterface({ input, crlfDelay: Infinity });

  for await (const line of reader) {
    yield line;
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
