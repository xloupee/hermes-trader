const LAMPORTS_PER_SOL = 1_000_000_000;
const PUMPFUN_MIGRATION_ACCOUNT = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
const PUMPSWAP_PROGRAM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const SPL_TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const TOKEN_2022_PROGRAM = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
const WSOL_MINT = "So11111111111111111111111111111111111111112";

const OFFICIAL_ACCOUNT_LABELS = {
  [PUMPFUN_MIGRATION_ACCOUNT]: "Pump.fun migration fee",
  "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV": "Pump.fun protocol fee",
  "7VtfL8fvgNfhz17qKRMjzQEXgbdpnHHHQRh54R9jP2RJ": "Pump.fun protocol fee",
  "7hTckgnGnLQR6sdH7YkqFTAA7VwTfYFaZ6EhEsU3saCX": "Pump.fun protocol fee",
  "9rPYyANsfQZw3DnDmKE3YCQF5E8oD89UXoHn9JFEhJUz": "Pump.fun protocol fee",
  AVmoTthdrX6tKt4nDjco2D775W2YK3sDhxPcMmzUAmTY: "Pump.fun protocol fee",
  CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM: "Pump.fun protocol fee",
  FWsW1xNtWscwNmKv6wVsU1iTzRN6wmmk3MjxRP5tT7hz: "Pump.fun protocol fee",
  G5UZAVbAf46s7cKWoyKu8kYTip9DGTpbLZ2qa9Aq69dP: "Pump.fun protocol fee",
  JCRGumoE9Qi5BBgULTgdgTLjSgkCMSbF62ZZfGs84JeU: "Pump.fun protocol fee",
  GesfTA3X2arioaHp8bbKdjG9vJtskViWACZoYvxp4twS: "Pump.fun reserved fee",
  "4budycTjhs9fD6xw62VBducVTNgMgJJ5BgtKq7mAZwn6": "Pump.fun reserved fee",
  "8SBKzEQU4nLSzcwF4a74F2iaUDQyTfjGndn6qUWBnrpR": "Pump.fun reserved fee",
  "4UQeTP1T39KZ9Sfxzo3WR5skgsaP6NZa87BAkuazLEKH": "Pump.fun reserved fee",
  "8sNeir4QsLsJdYpc9RZacohhK1Y5FLU3nC5LXgYB4aa6": "Pump.fun reserved fee",
  Fh9HmeLNUMVCvejxCtCL2DbYaRyBFVJ5xrWkLnMH6fdk: "Pump.fun reserved fee",
  "463MEnMeGyJekNZFQSTUABBEbLnvMTALbT6ZmsxAbAdq": "Pump.fun reserved fee",
  "6AUH3WEHucYZyC61hqpqYUWVto5qA5hjHuNQ32GNnNxA": "Pump.fun reserved fee",
  "5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD": "Pump.fun buyback fee",
  "9M4giFFMxmFGXtc3feFzRai56WbBqehoSeRE5GK7gf7": "Pump.fun buyback fee",
  GXPFM2caqTtQYC2cJ5yJRi9VDkpsYZXzYdwYpGnLmtDL: "Pump.fun buyback fee",
  "3BpXnfJaUTiwXnJNe7Ej1rcbzqTTQUvLShZaWazebsVR": "Pump.fun buyback fee",
  "5cjcW9wExnJJiqgLjq7DEG75Pm6JBgE1hNv4B2vHXUW6": "Pump.fun buyback fee",
  EHAAiTxcdDwQ3U4bU6YcMsQGaekdzLS3B5SmYo46kJtL: "Pump.fun buyback fee",
  "5eHhjP8JaYkz83CWwvGU2uMUXefd3AazWGx4gpcuEEYD": "Pump.fun buyback fee",
  A7hAgCzFw14fejgCp387JUJRMNyz4j89JKnhtKU8piqW: "Pump.fun buyback fee"
};

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

function pickEventMint(event) {
  return event?.mint || event?.ca || event?.token || event?.tokenAddress || event?.address;
}

function getParsedAccount(accountInfo) {
  return accountInfo?.data && typeof accountInfo.data === "object" ? accountInfo.data.parsed : null;
}

function labelAccount({ address, event, labels, accountInfo, deltaSol }) {
  if (!address) {
    return "Unknown";
  }

  if (labels[address]) {
    return labels[address];
  }

  if (OFFICIAL_ACCOUNT_LABELS[address]) {
    return OFFICIAL_ACCOUNT_LABELS[address];
  }

  if (address === event?.traderPublicKey) {
    return "Creator";
  }

  if (address === event?.bondingCurveKey || address === event?.bondingCurve) {
    return "Bonding curve";
  }

  if (address === pickEventMint(event)) {
    return "Token mint";
  }

  const parsedAccount = getParsedAccount(accountInfo);
  const parsedType = parsedAccount?.type;
  const parsedInfo = parsedAccount?.info || {};

  if (accountInfo?.owner === PUMPSWAP_PROGRAM && deltaSol > 0) {
    return "PumpSwap pool state rent";
  }

  if (parsedType === "account" && parsedInfo.mint === WSOL_MINT && deltaSol > 1) {
    return "Pool WSOL liquidity";
  }

  if (parsedType === "account" && parsedInfo.mint === pickEventMint(event) && deltaSol > 0) {
    return "Pool token vault rent";
  }

  if (parsedType === "mint" && accountInfo?.owner === TOKEN_2022_PROGRAM && deltaSol > 0) {
    return "LP mint rent";
  }

  if ((accountInfo?.owner === SPL_TOKEN_PROGRAM || accountInfo?.owner === TOKEN_2022_PROGRAM) && deltaSol > 0) {
    return "Token account rent";
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

async function getAccountInfoMap({ addresses, rpcUrl }) {
  if (addresses.length === 0) {
    return new Map();
  }

  const result = await rpc({
    rpcUrl,
    method: "getMultipleAccounts",
    params: [
      addresses,
      {
        encoding: "jsonParsed"
      }
    ]
  });

  return new Map(addresses.map((address, index) => [address, result?.value?.[index] || null]));
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
  const rawChanges = [];

  for (let index = 0; index < Math.min(preBalances.length, postBalances.length, accountKeys.length); index += 1) {
    const address = accountKeyToAddress(accountKeys[index]);
    const deltaLamports = postBalances[index] - preBalances[index];

    if (!address || deltaLamports === 0) {
      continue;
    }

    rawChanges.push({
      address,
      deltaSol: lamportsToSol(deltaLamports)
    });
  }

  const accountInfoByAddress = await getAccountInfoMap({
    rpcUrl,
    addresses: rawChanges.map((change) => change.address)
  });
  const changes = rawChanges.map((change) => ({
    ...change,
    label: labelAccount({
      ...change,
      event,
      labels,
      accountInfo: accountInfoByAddress.get(change.address)
    })
  }));

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
