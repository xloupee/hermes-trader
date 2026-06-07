import { createReadStream } from "node:fs";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { Connection, PublicKey } from "@solana/web3.js";
import {
  isKnownPumpRouterProgram,
  isPumpProgram,
  normalizeShredstreamTransaction,
  type RawPumpDiscoveryEvent,
  type ShredstreamAddressTableLookupInput,
  type ShredstreamSourceTiming,
  type ShredstreamTransactionInput
} from "./shredstream-decoder.js";

export type ShredstreamSourceMode = "jsonl" | "grpc";

export interface ShredstreamSourceConfig {
  mode: ShredstreamSourceMode;
  inputPath?: string;
  grpcUrl?: string;
  decoderCommand?: string;
  solanaRpcUrl?: string;
  addressLookupTableTimeoutMs?: number;
  addressLookupTableResolver?: ShredstreamAddressLookupTableResolver;
}

export type ShredstreamAddressLookupTableResolver = (
  lookup: ShredstreamAddressTableLookupInput
) => Promise<string[]> | string[];

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
  readRecords: (options?: { signal?: AbortSignal }) => AsyncIterable<ShredstreamRecord>;
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
  config.solanaRpcUrl = env.SOLANA_RPC_URL || "https://api.mainnet-beta.solana.com";
  config.addressLookupTableTimeoutMs = parseAddressLookupTableTimeoutMs(env.SHREDSTREAM_ALT_LOOKUP_TIMEOUT_MS);
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

  return createGrpcShredstreamSource(config.grpcUrl, config.decoderCommand, {
    solanaRpcUrl: config.solanaRpcUrl,
    addressLookupTableTimeoutMs: config.addressLookupTableTimeoutMs,
    addressLookupTableResolver: config.addressLookupTableResolver
  });
}

export function createJsonlShredstreamSource(inputPath: string): ShredstreamSource {
  return {
    describe: () => `jsonl:${inputPath}`,
    readRecords: (options) => readJsonlTransactions(inputPath, options)
  };
}

export function createGrpcShredstreamSource(
  grpcUrl: string,
  decoderCommand?: string,
  options: {
    solanaRpcUrl?: string;
    addressLookupTableTimeoutMs?: number;
    addressLookupTableResolver?: ShredstreamAddressLookupTableResolver;
  } = {}
): ShredstreamSource {
  const command = resolveGrpcDecoderCommand(grpcUrl, decoderCommand);
  const addressLookupTableResolver =
    options.addressLookupTableResolver ||
    createRpcAddressLookupTableResolver(options.solanaRpcUrl || process.env.SOLANA_RPC_URL || "https://api.mainnet-beta.solana.com");

  return {
    describe: () => `grpc:${grpcUrl}`,
    readRecords: (readOptions) =>
      readCommandJsonlTransactions(command, {
        ...readOptions,
        addressLookupTableResolver,
        addressLookupTableTimeoutMs: options.addressLookupTableTimeoutMs
      })
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

async function* readJsonlTransactions(
  inputPath: string,
  { signal }: { signal?: AbortSignal } = {}
): AsyncIterable<ShredstreamRecord> {
  for await (const line of readJsonlLines(inputPath)) {
    if (signal?.aborted) {
      return;
    }

    const trimmed = line.trim();

    if (!trimmed) {
      continue;
    }

    yield parseShredstreamTransactionLine(trimmed);
  }
}

async function* readCommandJsonlTransactions(
  command: string,
  {
    signal,
    addressLookupTableResolver,
    addressLookupTableTimeoutMs
  }: {
    signal?: AbortSignal;
    addressLookupTableResolver?: ShredstreamAddressLookupTableResolver;
    addressLookupTableTimeoutMs?: number;
  } = {}
): AsyncIterable<ShredstreamRecord> {
  const child = spawn(command, {
    shell: true,
    stdio: ["ignore", "pipe", "pipe"]
  });
  const exit = waitForChild(child);
  const stderr: string[] = [];
  const abort = () => {
    child.kill("SIGTERM");
  };

  signal?.addEventListener("abort", abort, { once: true });

  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk: string) => {
    stderr.push(chunk);
    while (stderr.join("").length > 8192 && stderr.length > 1) {
      stderr.shift();
    }
  });

  const reader = createInterface({ input: child.stdout, crlfDelay: Infinity });

  try {
    for await (const line of reader) {
      if (signal?.aborted) {
        child.kill("SIGTERM");
        return;
      }

      const trimmed = line.trim();

      if (!trimmed) {
        continue;
      }

      const sourceReadAtMs = Date.now();
      const parsedRecord = parseShredstreamTransactionLine(trimmed, sourceReadAtMs);
      yield await resolveAddressTableLookups(parsedRecord, {
        resolver: addressLookupTableResolver,
        timeoutMs: addressLookupTableTimeoutMs
      });
    }

    const exitCode = await exit;

    if (exitCode !== 0 && !signal?.aborted) {
      const details = stderr.join("").trim();
      throw new Error(`ShredStream decoder command exited with code ${exitCode}${details ? `: ${details}` : ""}`);
    }
  } finally {
    signal?.removeEventListener("abort", abort);
  }
}

async function resolveAddressTableLookups(
  record: ShredstreamRecord,
  {
    resolver,
    timeoutMs = 25
  }: {
    resolver?: ShredstreamAddressLookupTableResolver;
    timeoutMs?: number;
  } = {}
): Promise<ShredstreamRecord> {
  if (!record.transaction || !resolver || !record.transaction.addressTableLookups?.length) {
    return withSourceTiming(record, { altLookupStatus: "not_needed", altLookupCount: 0 });
  }

  if (!transactionMayNeedAddressTableLookups(record.transaction)) {
    return withSourceTiming(record, {
      altLookupStatus: "not_needed",
      altLookupCount: record.transaction.addressTableLookups.length
    });
  }

  if (transactionDecodesWithoutAddressTableLookups(record.transaction)) {
    return withSourceTiming(record, {
      altLookupStatus: "static_decoded",
      altLookupCount: record.transaction.addressTableLookups.length
    });
  }

  const altLookupStartedAtMs = Date.now();
  try {
    const loadedAddresses = (
      await withTimeout(Promise.all(record.transaction.addressTableLookups.map((lookup) => resolver(lookup))), timeoutMs)
    ).flat();
    const altLookupFinishedAtMs = Date.now();
    return {
      transaction: {
        ...record.transaction,
        accountKeys: [...record.transaction.accountKeys, ...loadedAddresses],
        sourceTiming: mergeSourceTiming(record.transaction.sourceTiming, {
          altLookupStatus: "hydrated",
          altLookupCount: record.transaction.addressTableLookups.length,
          altLookupStartedAtMs,
          altLookupFinishedAtMs,
          altLookupDurationMs: altLookupFinishedAtMs - altLookupStartedAtMs,
          altLookupTimeoutMs: timeoutMs
        })
      }
    };
  } catch {
    const altLookupFinishedAtMs = Date.now();
    return withSourceTiming(record, {
      altLookupStatus: "timeout_or_error",
      altLookupCount: record.transaction.addressTableLookups.length,
      altLookupStartedAtMs,
      altLookupFinishedAtMs,
      altLookupDurationMs: altLookupFinishedAtMs - altLookupStartedAtMs,
      altLookupTimeoutMs: timeoutMs
    });
  }
}

function withSourceTiming(record: ShredstreamRecord, timing: ShredstreamSourceTiming): ShredstreamRecord {
  if (!record.transaction) {
    return record;
  }

  return {
    transaction: {
      ...record.transaction,
      sourceTiming: mergeSourceTiming(record.transaction.sourceTiming, timing)
    }
  };
}

function mergeSourceTiming(
  current: ShredstreamSourceTiming | undefined,
  update: ShredstreamSourceTiming
): ShredstreamSourceTiming {
  return {
    ...current,
    ...update
  };
}

function parseAddressLookupTableTimeoutMs(value: string | undefined): number | undefined {
  if (!value) {
    return undefined;
  }

  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`Invalid SHREDSTREAM_ALT_LOOKUP_TIMEOUT_MS="${value}". Expected a non-negative number.`);
  }

  return parsed;
}

function withTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  if (timeoutMs <= 0) {
    return promise;
  }

  return new Promise<T>((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("ShredStream ALT lookup timed out")), timeoutMs);
    promise.then(
      (value) => {
        clearTimeout(timeout);
        resolve(value);
      },
      (error: unknown) => {
        clearTimeout(timeout);
        reject(error);
      }
    );
  });
}

function transactionMayNeedAddressTableLookups(transaction: ShredstreamTransactionInput): boolean {
  if (transaction.accountKeys.some((accountKey) => isPumpProgram(accountKey) || isKnownPumpRouterProgram(accountKey))) {
    return true;
  }

  return transaction.instructions.some((instruction) => {
    if (instruction.programId && (isPumpProgram(instruction.programId) || isKnownPumpRouterProgram(instruction.programId))) {
      return true;
    }

    if (typeof instruction.programIdIndex === "number") {
      const programId = transaction.accountKeys[instruction.programIdIndex] || null;
      return isPumpProgram(programId) || isKnownPumpRouterProgram(programId);
    }

    return false;
  });
}

function transactionDecodesWithoutAddressTableLookups(transaction: ShredstreamTransactionInput): boolean {
  const events = normalizeShredstreamTransaction(transaction);
  return events.length > 0 && events.every(eventHasFastPathAccounts);
}

function eventHasFastPathAccounts(event: RawPumpDiscoveryEvent): boolean {
  if (event.decodeStatus !== "decoded") {
    return event.eventType === "unknown-pump";
  }

  if (event.eventType === "buy" || event.eventType === "sell") {
    if (event.mint && event.trader && event.mint === event.trader) {
      return false;
    }

    return Boolean(event.mint && event.trader);
  }

  if (event.eventType === "create" || event.eventType === "migrate") {
    return Boolean(event.mint);
  }

  return true;
}

function createRpcAddressLookupTableResolver(rpcUrl: string): ShredstreamAddressLookupTableResolver {
  const connection = new Connection(rpcUrl, "confirmed");
  const cache = new Map<string, Promise<string[]>>();

  return async (lookup) => {
    let addresses = cache.get(lookup.accountKey);

    if (!addresses) {
      addresses = connection
        .getAddressLookupTable(new PublicKey(lookup.accountKey))
        .then((result) => result.value?.state.addresses.map((address) => address.toBase58()) || []);
      cache.set(lookup.accountKey, addresses);
    }

    const tableAddresses = await addresses;
    return [
      ...lookup.writableIndexes.map((index) => tableAddresses[index]).filter((address): address is string => Boolean(address)),
      ...lookup.readonlyIndexes.map((index) => tableAddresses[index]).filter((address): address is string => Boolean(address))
    ];
  };
}

function parseShredstreamTransactionLine(line: string, sourceReadAtMs?: number): ShredstreamRecord {
  try {
    const value: unknown = JSON.parse(line);
    const parsedAtMs = Date.now();

    if (!isShredstreamTransaction(value)) {
      return { parseError: "JSONL record does not match ShredstreamTransactionInput" };
    }

    return {
      transaction: {
        ...value,
        sourceTiming: mergeSourceTiming(value.sourceTiming, {
          ...(sourceReadAtMs ? { sourceReadAtMs } : {}),
          parsedAtMs
        })
      }
    };
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
    (record.addressTableLookups === undefined ||
      (Array.isArray(record.addressTableLookups) && record.addressTableLookups.every(isShredstreamAddressTableLookup))) &&
    Array.isArray(record.instructions) &&
    record.instructions.every(isShredstreamInstruction)
  );
}

function isShredstreamAddressTableLookup(value: unknown): boolean {
  if (!value || typeof value !== "object") {
    return false;
  }

  const record = value as Record<string, unknown>;
  return (
    typeof record.accountKey === "string" &&
    Array.isArray(record.writableIndexes) &&
    record.writableIndexes.every((index) => Number.isInteger(index) && Number(index) >= 0) &&
    Array.isArray(record.readonlyIndexes) &&
    record.readonlyIndexes.every((index) => Number.isInteger(index) && Number(index) >= 0)
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
