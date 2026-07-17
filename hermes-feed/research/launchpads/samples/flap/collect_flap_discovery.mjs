#!/usr/bin/env node

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

const RPC_URL = process.env.FLAP_RPC_URL ?? "https://rpc.mainnet.chain.robinhood.com";
const SOURCE_SHA = process.env.FLAP_SOURCE_SHA;
const OUTPUT = resolve(process.argv[2] ?? "flap_discovery_evidence.json");
const BLOCK_CAP = 250_000;
const CHUNK_SIZE = 10_000;
const PORTAL = "0x26605f322f7ff986f381bb9a6e3f5dab0beaeb09";
const VAULT_PORTAL = "0xe9f7ab7de8fb8756acbb6a1cd13316a43308197b";
const IMPLEMENTATION_SLOT =
  "0x360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc";
const TOPICS = {
  TokenCreated: "0x504e7f360b2e5fe33cbaaae4c593bc55305328341bf79009e43e0e3b7f699603",
  TokenBought: "0xa800a2038683844fac66747f771bfdfae862eb28b16bcfa387afa9fbacce8ff7",
  TokenSold: "0x03a4693e592f5e75dc7c136acb39b146d2b4966c0e509c34f362dee02b3b861a",
};
const EXPECTED = {
  chain_id: 4663,
  portal: {
    proxy_runtime_keccak256: "0xcecb292d9c022858199c9348abf0d5836f9ea4dab5cf03710e1dcf41fd9a4c35",
    implementation: "0xd9c9981d784a3765d8264d6104650b901c4e36b1",
    implementation_runtime_keccak256:
      "0x85facd83c203c88ea8f37c4f00c328f983e90c5045b06ec20ef18639c818186b",
    version: "v5.14.16",
  },
  vault_portal: {
    proxy_runtime_keccak256: "0xe7109718479fd7c6d05b829ffc6a1469e4c949ae282497c15d179b2af4e5e3a9",
    implementation: "0xe5789d9d5616dd8ec66de95bb31a29ac1c847769",
    implementation_runtime_keccak256:
      "0x8b4bcf2d4a81f646f500da41a331b01bed39065046a5058a333fb942c81c0464",
    version: "1.12.1",
  },
};

if (!SOURCE_SHA?.match(/^[0-9a-f]{40}$/)) {
  throw new Error("FLAP_SOURCE_SHA must be the exact 40-character repository SHA");
}

let requestId = 0;
let requestCount = 0;
const delay = (milliseconds) => new Promise((resolveDelay) => setTimeout(resolveDelay, milliseconds));
async function rpc(method, params) {
  for (let attempt = 0; attempt < 8; attempt++) {
    if (requestCount > 0) await delay(125);
    const response = await fetch(RPC_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: ++requestId, method, params }),
    });
    requestCount++;
    if (response.status === 429 || response.status >= 500) {
      if (attempt === 7) throw new Error(`${method}: HTTP ${response.status} after bounded retries`);
      await delay(500 * 2 ** attempt);
      continue;
    }
    if (!response.ok) throw new Error(`${method}: HTTP ${response.status}`);
    const body = await response.json();
    if (body.error) throw new Error(`${method}: ${JSON.stringify(body.error)}`);
    return body.result;
  }
  throw new Error(`${method}: unreachable retry state`);
}

const hex = (value) => `0x${value.toString(16)}`;
const integer = (value) => Number.parseInt(value, 16);
const lower = (value) => value?.toLowerCase() ?? null;
const sha256Hex = (value) => `0x${createHash("sha256").update(value).digest("hex")}`;
const wordAddress = (word) => `0x${word.slice(-40)}`.toLowerCase();
const wordDecimal = (word) => BigInt(`0x${word}`).toString();

function keccakBytecode(bytecode) {
  return execFileSync("cast", ["keccak", bytecode], { encoding: "utf8" }).trim().toLowerCase();
}

function decodeAbiString(result) {
  const data = result.slice(2);
  const offset = Number(BigInt(`0x${data.slice(0, 64)}`)) * 2;
  const length = Number(BigInt(`0x${data.slice(offset, offset + 64)}`));
  return Buffer.from(data.slice(offset + 64, offset + 64 + length * 2), "hex").toString("utf8");
}

function implementationFromSlot(value) {
  return `0x${value.slice(-40)}`.toLowerCase();
}

function decodeTokenCreated(data) {
  const body = data.slice(2);
  if (body.length < 7 * 64 || body.length % 64 !== 0) return null;
  const words = Array.from({ length: 7 }, (_, index) => body.slice(index * 64, (index + 1) * 64));
  const decoded = {
    id: wordDecimal(words[0]),
    creator: wordAddress(words[1]),
    nonce: wordDecimal(words[2]),
    token: wordAddress(words[3]),
  };
  if (decoded.creator === "0x0000000000000000000000000000000000000000") return null;
  if (decoded.token === "0x0000000000000000000000000000000000000000") return null;
  return decoded;
}

async function observePin(address, expected, blockTag) {
  const proxyCode = await rpc("eth_getCode", [address, blockTag]);
  const slot = await rpc("eth_getStorageAt", [address, IMPLEMENTATION_SLOT, blockTag]);
  const implementation = implementationFromSlot(slot);
  const implementationCode = await rpc("eth_getCode", [implementation, blockTag]);
  const versionResult = await rpc("eth_call", [{ to: address, data: "0x54fd4d50" }, blockTag]);
  const observed = {
    proxy_runtime_bytes: (proxyCode.length - 2) / 2,
    proxy_runtime_keccak256: keccakBytecode(proxyCode),
    implementation_slot: IMPLEMENTATION_SLOT,
    implementation,
    implementation_runtime_bytes: (implementationCode.length - 2) / 2,
    implementation_runtime_keccak256: keccakBytecode(implementationCode),
    version: decodeAbiString(versionResult),
  };
  const matches =
    observed.proxy_runtime_keccak256 === expected.proxy_runtime_keccak256 &&
    observed.implementation === expected.implementation &&
    observed.implementation_runtime_keccak256 === expected.implementation_runtime_keccak256 &&
    observed.version === expected.version;
  if (!matches) throw new Error(`pin drift at ${address}: ${JSON.stringify(observed)}`);
  return { expected, observed, matches };
}

async function logsForTopic(fromBlock, toBlock, topic) {
  const logs = [];
  for (let start = fromBlock; start <= toBlock; start += CHUNK_SIZE) {
    const end = Math.min(toBlock, start + CHUNK_SIZE - 1);
    const chunk = await rpc("eth_getLogs", [
      { fromBlock: hex(start), toBlock: hex(end), address: PORTAL, topics: [topic] },
    ]);
    logs.push(...chunk);
  }
  return logs;
}

function selector(input) {
  return input && input.length >= 10 ? input.slice(0, 10).toLowerCase() : null;
}

const startedAt = new Date().toISOString();
const chainId = integer(await rpc("eth_chainId", []));
if (chainId !== EXPECTED.chain_id) throw new Error(`chain mismatch: ${chainId}`);
const boundary = await rpc("eth_getBlockByNumber", ["finalized", false]);
if (!boundary?.number || !boundary?.hash) throw new Error("finalized boundary unavailable");
const toBlock = integer(boundary.number);
const fromBlock = toBlock - BLOCK_CAP + 1;
if (fromBlock < 0) throw new Error("bounded range underflow");
const fromBoundary = await rpc("eth_getBlockByNumber", [hex(fromBlock), false]);
if (!fromBoundary?.number || !fromBoundary?.hash) throw new Error("range start boundary unavailable");
const blockTag = boundary.number;
const pins = {
  portal: await observePin(PORTAL, EXPECTED.portal, blockTag),
  vault_portal: await observePin(VAULT_PORTAL, EXPECTED.vault_portal, blockTag),
};

const createdLogs = await logsForTopic(fromBlock, toBlock, TOPICS.TokenCreated);
const boughtLogs = await logsForTopic(fromBlock, toBlock, TOPICS.TokenBought);
const soldLogs = await logsForTopic(fromBlock, toBlock, TOPICS.TokenSold);
const uniqueCreatedTxs = [...new Set(createdLogs.map((log) => log.transactionHash.toLowerCase()))];
const envelopes = new Map();
for (const [index, txHash] of uniqueCreatedTxs.entries()) {
  const transaction = await rpc("eth_getTransactionByHash", [txHash]);
  const receipt = await rpc("eth_getTransactionReceipt", [txHash]);
  if (!transaction || !receipt) throw new Error(`ambiguous or missing transaction envelope: ${txHash}`);
  envelopes.set(txHash, { transaction, receipt });
  if ((index + 1) % 250 === 0) process.stderr.write(`resolved ${index + 1}/${uniqueCreatedTxs.length}\n`);
}

const selectorCounts = new Map();
const claims = [];
let directCount = 0;
let vaultCount = 0;
let falsePositives = 0;
let actionMismatches = 0;
let decodeMisses = 0;
for (const log of createdLogs) {
  const txHash = log.transactionHash.toLowerCase();
  const { transaction: tx, receipt } = envelopes.get(txHash);
  const matchingReceiptLogs = receipt.logs.filter(
    (receiptLog) =>
      lower(receiptLog.address) === PORTAL &&
      lower(receiptLog.topics?.[0]) === TOPICS.TokenCreated &&
      lower(receiptLog.logIndex) === lower(log.logIndex),
  );
  const decoded = decodeTokenCreated(log.data);
  const destination = lower(tx.to);
  const origin = destination === PORTAL ? "direct_portal" : destination === VAULT_PORTAL ? "vault_portal" : "other";
  const callSelector = selector(tx.input);
  selectorCounts.set(`${origin}:${callSelector}`, (selectorCounts.get(`${origin}:${callSelector}`) ?? 0) + 1);
  const receiptConfirmed =
    receipt.status === "0x1" &&
    lower(receipt.blockHash) === lower(log.blockHash) &&
    matchingReceiptLogs.length === 1 &&
    lower(log.address) === PORTAL &&
    lower(log.topics?.[0]) === TOPICS.TokenCreated &&
    log.removed === false;
  const actionMismatch = origin === "other";
  if (origin === "direct_portal") directCount++;
  else if (origin === "vault_portal") vaultCount++;
  else actionMismatches++;
  if (!decoded) decodeMisses++;
  if (!receiptConfirmed || !decoded) falsePositives++;
  claims.push({
    classification: origin,
    profile_classification:
      origin === "direct_portal"
        ? "canonical_portal_event_direct_portal_origin_discovery_only"
        : origin === "vault_portal"
          ? "canonical_portal_event_vault_portal_origin_discovery_only"
          : "canonical_portal_event_unrecognized_origin_fail_closed",
    transaction_hash: txHash,
    transaction_from: lower(tx.from),
    transaction_to: destination,
    transaction_selector_observed_not_admitted: callSelector,
    transaction_value_wei: BigInt(tx.value).toString(),
    receipt_status: integer(receipt.status),
    block_number: integer(log.blockNumber),
    block_hash: lower(log.blockHash),
    transaction_index: integer(log.transactionIndex),
    log_index: integer(log.logIndex),
    event_emitter: lower(log.address),
    event_topic0: lower(log.topics[0]),
    event_data_sha256: sha256Hex(Buffer.from(log.data.slice(2), "hex")),
    event_identity_confirmed_in_receipt: receiptConfirmed,
    decoded,
    quote_eligible: false,
    entry_outcome: null,
    exit_outcome: null,
    slippage_outcome: null,
    action_mismatch: actionMismatch,
  });
}

const boundaryAgain = await rpc("eth_getBlockByNumber", [boundary.number, false]);
if (!boundaryAgain || lower(boundaryAgain.hash) !== lower(boundary.hash)) {
  throw new Error("finalized boundary hash drift or RPC ambiguity");
}
const endPins = {
  portal: await observePin(PORTAL, EXPECTED.portal, blockTag),
  vault_portal: await observePin(VAULT_PORTAL, EXPECTED.vault_portal, blockTag),
};

const selectorSummary = [...selectorCounts.entries()]
  .map(([key, count]) => {
    const separator = key.indexOf(":");
    return { origin: key.slice(0, separator), selector_observed_not_admitted: key.slice(separator + 1), count };
  })
  .sort((a, b) => a.origin.localeCompare(b.origin) || b.count - a.count || a.selector_observed_not_admitted.localeCompare(b.selector_observed_not_admitted));
const tokenKeys = claims.map((claim) => claim.decoded?.token).filter(Boolean);
const uniqueTokens = new Set(tokenKeys);
const duplicateEventIdentities = claims.length - new Set(claims.map((claim) => `${claim.transaction_hash}:${claim.log_index}`)).size;
const duplicateTokenClaims = tokenKeys.length - uniqueTokens.size;
const evidence = {
  record_type: "flap_canonical_token_created_discovery_scan",
  schema_version: 1,
  captured_at: new Date().toISOString(),
  started_at: startedAt,
  source: {
    repository_sha: SOURCE_SHA,
    branch: "codex/samples-flap-discovery",
    collector: "hermes-feed/research/launchpads/samples/flap/collect_flap_discovery.mjs",
  },
  scope: {
    chain: "Robinhood Chain mainnet",
    chain_id: chainId,
    public_rpc_endpoint: RPC_URL,
    concurrency: 1,
    block_cap: BLOCK_CAP,
    actual_block_count: toBlock - fromBlock + 1,
    ground_truth: "canonical Portal TokenCreated receipt log only",
    wallet_filter: null,
    admits_vault_selectors: false,
    admits_predictions: false,
    admits_quotes: false,
    admits_readiness: false,
    admits_execution: false,
    admits_promotion: false,
  },
  finalized_window: {
    from_block: fromBlock,
    from_block_hash: lower(fromBoundary.hash),
    from_block_timestamp: new Date(integer(fromBoundary.timestamp) * 1000).toISOString(),
    to_block: toBlock,
    to_block_hash: lower(boundary.hash),
    to_block_timestamp: new Date(integer(boundary.timestamp) * 1000).toISOString(),
    boundary_hash_recheck: lower(boundaryAgain.hash),
  },
  pins: { start: pins, end: endPins, stable: true },
  topics: TOPICS,
  results: {
    canonical_token_created_claims: claims.length,
    confirmed: claims.length - falsePositives,
    false_positives: falsePositives,
    decode_misses: decodeMisses,
    ground_truth_misses: 0,
    action_mismatches: actionMismatches,
    direct_portal_origin: directCount,
    vault_portal_origin: vaultCount,
    unique_tokens: uniqueTokens.size,
    duplicate_event_identities: duplicateEventIdentities,
    duplicate_token_claims: duplicateTokenClaims,
    token_bought_control_logs_not_substituted: boughtLogs.length,
    token_sold_control_logs_not_substituted: soldLogs.length,
    trade_logs_substituted_for_launch_ground_truth: 0,
    quote_eligible: 0,
    entries_attempted: 0,
    exits_attempted: 0,
    slippage_outcomes_available: 0,
  },
  observed_action_selectors: selectorSummary,
  methodology: {
    canonical_claim_rule: "address == Portal and topic0 == TokenCreated and exact receipt log identity is present in a successful receipt",
    origin_rule: "transaction.to == Portal => direct; transaction.to == VaultPortal => vault origin; otherwise fail-closed action mismatch",
    miss_definition: "canonical TokenCreated logs returned inside the exact bounded eth_getLogs range but not represented as claims",
    false_positive_definition: "claimed log that fails canonical emitter/topic, successful receipt identity, non-removed, or strict nonzero creator/token decoding",
    action_mismatch_definition: "canonical TokenCreated receipt whose outer transaction destination is neither pinned Portal nor pinned VaultPortal",
    control_rule: "TokenBought and TokenSold are counted separately and never create launch claims",
    limitation: "single public RPC endpoint; provider completeness is not independently cross-checked",
  },
  rpc_request_count: requestCount,
  claims,
};

await mkdir(dirname(OUTPUT), { recursive: true });
await writeFile(OUTPUT, `${JSON.stringify(evidence, null, 2)}\n`);
process.stdout.write(`${JSON.stringify({ output: OUTPUT, results: evidence.results, rpc_request_count: requestCount })}\n`);
