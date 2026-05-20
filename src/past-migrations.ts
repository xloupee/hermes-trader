import "dotenv/config";
import { asRecord, stringValue } from "./types.js";
import type { LooseRecord } from "./types.js";

const MIGRATION_ACCOUNT = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
const PUMPFUN_PROGRAM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMPSWAP_PROGRAM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const RAYDIUM_V4_PROGRAM = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";

const rpcUrl = process.env.SOLANA_RPC_URL || "https://api.mainnet-beta.solana.com";
const requestedLimit = Number.parseInt(process.argv[2] || "10", 10);
const limit = Number.isFinite(requestedLimit) ? Math.min(Math.max(requestedLimit, 1), 100) : 10;

interface SignatureInfo {
  signature: string;
  slot: unknown;
  blockTime: number | null;
  err?: unknown;
}

interface MigrationInstruction {
  programId?: unknown;
  accounts?: unknown;
}

async function rpc(method: string, params: unknown[]): Promise<unknown> {
  const response = await fetch(rpcUrl, {
    method: "POST",
    headers: {
      "content-type": "application/json"
    },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: Date.now(),
      method,
      params
    })
  });

  const body = asRecord(await response.json());

  if (body.error) {
    throw new Error(`${method} failed: ${stringValue(asRecord(body.error).message)}`);
  }

  return body.result;
}

function findMigrationInstruction(transaction: unknown): MigrationInstruction | undefined {
  const transactionBody = asRecord(asRecord(transaction).transaction);
  const message = asRecord(transactionBody.message);
  const instructions = Array.isArray(message.instructions) ? message.instructions : [];

  return instructions.find((instructionValue): instructionValue is MigrationInstruction => {
    const instruction = asRecord(instructionValue);
    const accounts = Array.isArray(instruction.accounts) ? instruction.accounts : [];
    return instruction.programId === PUMPFUN_PROGRAM && accounts.includes(MIGRATION_ACCOUNT);
  });
}

function detectDestination(accounts: string[], logs: string[]): string {
  if (accounts.includes(PUMPSWAP_PROGRAM) || logs.some((log) => log.includes("Instruction: CreatePool"))) {
    return "PumpSwap";
  }

  if (accounts.includes(RAYDIUM_V4_PROGRAM) || logs.some((log) => log.includes("initialize2"))) {
    return "Raydium";
  }

  return "Unknown";
}

function extractMigration(signatureInfo: SignatureInfo, transaction: unknown): LooseRecord | null {
  const instruction = findMigrationInstruction(transaction);
  const meta = asRecord(asRecord(transaction).meta);
  const logs = Array.isArray(meta.logMessages) ? meta.logMessages.map(String) : [];
  const accounts = Array.isArray(instruction?.accounts) ? instruction.accounts.map(String) : [];

  if (!instruction || !logs.some((log) => log.includes("Instruction: Migrate"))) {
    return null;
  }

  const mint = accounts[2] || null;
  const poolCandidate = accounts[9] || null;

  return {
    signature: signatureInfo.signature,
    slot: signatureInfo.slot,
    blockTime: signatureInfo.blockTime,
    time: signatureInfo.blockTime ? new Date(signatureInfo.blockTime * 1000).toISOString() : null,
    destination: detectDestination(accounts, logs),
    mint,
    poolCandidate,
    pumpFunUrl: mint ? `https://pump.fun/${mint}` : null,
    solscanTxUrl: `https://solscan.io/tx/${signatureInfo.signature}`,
    solscanTokenUrl: mint ? `https://solscan.io/token/${mint}` : null
  };
}

const signatures = await rpc("getSignaturesForAddress", [
  MIGRATION_ACCOUNT,
  { limit: Math.max(limit * 3, 20) }
]);

const migrations: LooseRecord[] = [];

for (const signatureInfoValue of Array.isArray(signatures) ? signatures : []) {
  const signatureRecord = asRecord(signatureInfoValue);
  const signature = stringValue(signatureRecord.signature);

  if (!signature) {
    continue;
  }

  const signatureInfo: SignatureInfo = {
    signature,
    slot: signatureRecord.slot,
    blockTime: typeof signatureRecord.blockTime === "number" ? signatureRecord.blockTime : null,
    err: signatureRecord.err
  };

  if (signatureInfo.err) {
    continue;
  }

  const transaction = await rpc("getTransaction", [
    signatureInfo.signature,
    {
      encoding: "jsonParsed",
      maxSupportedTransactionVersion: 0
    }
  ]);

  const migration = extractMigration(signatureInfo, transaction);

  if (migration) {
    migrations.push(migration);
  }

  if (migrations.length >= limit) {
    break;
  }
}

console.log(JSON.stringify(migrations, null, 2));
