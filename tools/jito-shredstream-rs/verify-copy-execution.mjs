#!/usr/bin/env node
import { existsSync, readFileSync } from "node:fs";
import { config as loadEnv } from "dotenv";

loadEnv();

const DEFAULT_EXECUTIONS_PATH = "/tmp/jito-copy-executions-local-send.jsonl";

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

async function rpc(method, params) {
  const rpcUrl = process.env.SOLANA_RPC_URL;
  if (!rpcUrl) {
    throw new Error("SOLANA_RPC_URL must be set in the environment or .env");
  }

  const response = await fetch(rpcUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method,
      params
    })
  });

  if (!response.ok) {
    throw new Error(`${method} HTTP ${response.status}: ${await response.text()}`);
  }

  const body = await response.json();
  if (body.error) {
    throw new Error(`${method} RPC error: ${JSON.stringify(body.error)}`);
  }

  return body.result;
}

function latestRelevant(rows, signature) {
  const candidates = rows.filter((row) => {
    if (signature) {
      return row.sendSignature === signature || row.copySignature === signature;
    }
    return row.sendSignature || row.sent || row.decision === "sent";
  });

  if (candidates.length === 0) {
    throw new Error(signature ? `no execution row for ${signature}` : "no sent execution row found");
  }

  return candidates.at(-1);
}

function uiAmount(balance) {
  const amount = Number(balance?.uiTokenAmount?.uiAmountString ?? balance?.uiTokenAmount?.uiAmount);
  return Number.isFinite(amount) ? amount : 0;
}

function tokenDelta(transaction, owner, mint) {
  const pre = transaction?.meta?.preTokenBalances ?? [];
  const post = transaction?.meta?.postTokenBalances ?? [];
  const byIndex = new Map();

  for (const balance of pre) {
    if (balance.owner === owner && balance.mint === mint) {
      byIndex.set(balance.accountIndex, { pre: uiAmount(balance), post: 0 });
    }
  }

  for (const balance of post) {
    if (balance.owner === owner && balance.mint === mint) {
      const current = byIndex.get(balance.accountIndex) ?? { pre: 0, post: 0 };
      current.post = uiAmount(balance);
      byIndex.set(balance.accountIndex, current);
    }
  }

  let delta = 0;
  for (const value of byIndex.values()) {
    delta += value.post - value.pre;
  }

  return delta;
}

function solDelta(transaction, account) {
  const keys = transaction?.transaction?.message?.accountKeys ?? [];
  const index = keys.findIndex((key) => (typeof key === "string" ? key : key.pubkey) === account);
  if (index < 0) {
    return null;
  }

  const pre = transaction?.meta?.preBalances?.[index];
  const post = transaction?.meta?.postBalances?.[index];
  if (!Number.isFinite(pre) || !Number.isFinite(post)) {
    return null;
  }

  return (post - pre) / 1_000_000_000;
}

async function main() {
  const path = argValue("executions", process.env.JITO_COPY_EXECUTIONS_PATH || DEFAULT_EXECUTIONS_PATH);
  const signature = argValue("signature");
  const rows = readJsonl(path);
  const row = latestRelevant(rows, signature);
  const sendSignature = row.sendSignature;

  if (!sendSignature) {
    throw new Error("latest execution row has no sendSignature");
  }

  const transaction = await rpc("getTransaction", [
    sendSignature,
    {
      encoding: "jsonParsed",
      commitment: "confirmed",
      maxSupportedTransactionVersion: 0
    }
  ]);

  if (!transaction) {
    throw new Error(`transaction not found at confirmed commitment: ${sendSignature}`);
  }

  const copyWallet = row.copyWallet;
  const mint = row.mint;
  const fillTokenDelta = tokenDelta(transaction, copyWallet, mint);
  const copyWalletSolDelta = solDelta(transaction, copyWallet);

  const summary = {
    sendSignature,
    observedSignature: row.observedSignature,
    confirmed: true,
    slot: transaction.slot,
    blockTime: transaction.blockTime,
    err: transaction.meta?.err ?? null,
    copyWallet,
    mint,
    fillTokenDelta,
    copyWalletSolDelta,
    decision: row.decision,
    observedToSignedMs: row.observedToSignedMs ?? null,
    observedToSimulationCompletedMs: row.observedToSimulationCompletedMs ?? null,
    observedToSendSubmittedMs: row.observedToSendSubmittedMs ?? null,
    observedToSignatureReturnedMs: row.observedToSignatureReturnedMs ?? null,
    simulationUnitsConsumed: row.simulationUnitsConsumed ?? null,
    executionLog: path
  };

  console.log(JSON.stringify(summary, null, 2));

  if (transaction.meta?.err) {
    process.exitCode = 2;
  } else if (!(fillTokenDelta > 0)) {
    process.exitCode = 3;
  }
}

main().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
