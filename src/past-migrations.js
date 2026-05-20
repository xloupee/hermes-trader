import "dotenv/config";

const MIGRATION_ACCOUNT = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
const PUMPFUN_PROGRAM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMPSWAP_PROGRAM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const RAYDIUM_V4_PROGRAM = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";

const rpcUrl = process.env.SOLANA_RPC_URL || "https://api.mainnet-beta.solana.com";
const requestedLimit = Number.parseInt(process.argv[2] || "10", 10);
const limit = Number.isFinite(requestedLimit) ? Math.min(Math.max(requestedLimit, 1), 100) : 10;

async function rpc(method, params) {
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

  const body = await response.json();

  if (body.error) {
    throw new Error(`${method} failed: ${body.error.message}`);
  }

  return body.result;
}

function findMigrationInstruction(transaction) {
  return transaction?.transaction?.message?.instructions?.find((instruction) => {
    return instruction.programId === PUMPFUN_PROGRAM && instruction.accounts?.includes(MIGRATION_ACCOUNT);
  });
}

function detectDestination(accounts, logs) {
  if (accounts.includes(PUMPSWAP_PROGRAM) || logs.some((log) => log.includes("Instruction: CreatePool"))) {
    return "PumpSwap";
  }

  if (accounts.includes(RAYDIUM_V4_PROGRAM) || logs.some((log) => log.includes("initialize2"))) {
    return "Raydium";
  }

  return "Unknown";
}

function extractMigration(signatureInfo, transaction) {
  const instruction = findMigrationInstruction(transaction);
  const logs = transaction?.meta?.logMessages || [];
  const accounts = instruction?.accounts || [];

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

const migrations = [];

for (const signatureInfo of signatures) {
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
