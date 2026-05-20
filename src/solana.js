const LAMPORTS_PER_SOL = 1_000_000_000;

function lamportsToSol(value) {
  return value / LAMPORTS_PER_SOL;
}

function accountKeyToAddress(accountKey) {
  if (!accountKey) {
    return null;
  }

  if (typeof accountKey === "string") {
    return accountKey;
  }

  return accountKey.pubkey || accountKey.toString?.() || null;
}

function parseAccountLabels(value) {
  if (!value) {
    return {};
  }

  try {
    const labels = JSON.parse(value);
    return labels && typeof labels === "object" && !Array.isArray(labels) ? labels : {};
  } catch {
    return {};
  }
}

function labelAccount(address, event, labels) {
  if (!address) {
    return "Unknown";
  }

  if (labels[address]) {
    return labels[address];
  }

  if (address === event?.traderPublicKey) {
    return "Creator";
  }

  if (address === event?.bondingCurveKey || address === event?.bondingCurve) {
    return "Bonding curve";
  }

  if (address === event?.mint) {
    return "Token mint";
  }

  return "Unlabeled wallet";
}

async function rpc({ rpcUrl, method, params }) {
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

  if (!response.ok || body.error) {
    throw new Error(body.error?.message || `RPC ${method} failed with ${response.status}`);
  }

  return body.result;
}

async function wait(ms) {
  await new Promise((resolve) => setTimeout(resolve, ms));
}

async function getTransactionWithRetry({ signature, rpcUrl }) {
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const transaction = await rpc({
      rpcUrl,
      method: "getTransaction",
      params: [
        signature,
        {
          encoding: "jsonParsed",
          maxSupportedTransactionVersion: 0
        }
      ]
    });

    if (transaction) {
      return transaction;
    }

    await wait(1000);
  }

  throw new Error("Transaction not available from RPC yet");
}

export async function analyzeSolanaTransaction({ signature, rpcUrl, event, accountLabels }) {
  if (!signature || !rpcUrl) {
    return null;
  }

  const transaction = await getTransactionWithRetry({ signature, rpcUrl });

  const accountKeys = transaction?.transaction?.message?.accountKeys || [];
  const preBalances = transaction?.meta?.preBalances || [];
  const postBalances = transaction?.meta?.postBalances || [];
  const labels = parseAccountLabels(accountLabels);
  const feePayer = accountKeyToAddress(accountKeys[0]);
  const changes = [];

  for (let index = 0; index < Math.min(preBalances.length, postBalances.length, accountKeys.length); index += 1) {
    const address = accountKeyToAddress(accountKeys[index]);
    const deltaLamports = postBalances[index] - preBalances[index];

    if (!address || deltaLamports === 0) {
      continue;
    }

    changes.push({
      address,
      label: labelAccount(address, event, labels),
      deltaSol: lamportsToSol(deltaLamports)
    });
  }

  const recipients = changes
    .filter((change) => change.deltaSol > 0.000001)
    .sort((a, b) => b.deltaSol - a.deltaSol)
    .slice(0, 6);

  const senders = changes
    .filter((change) => change.deltaSol < -0.000001)
    .sort((a, b) => a.deltaSol - b.deltaSol)
    .slice(0, 4);

  return {
    feePayer,
    networkFeeSol: lamportsToSol(transaction?.meta?.fee || 0),
    recipients,
    senders
  };
}
