#!/usr/bin/env node

import { writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";

const RPC_URL = "https://rpc.mainnet.chain.robinhood.com";
const CHAIN_ID = 4663;
const RANGES = [
  { label: "migration_span", from_block: 10_079_778, to_block: 10_794_005 },
  { label: "current_curve_span", from_block: 11_700_000, to_block: 11_735_771 },
];
const CHUNK_BLOCKS = 50_000;
const FACTORY = "0x5fcc1df0dc020cf454e742e9a8ae2554c37a452c";
const MIGRATOR = "0x5790ef23be2e1543442c12f4550fae147ba8edbe";
const TOPICS = {
  token_created: "0x91de26bc430b3a4f1d6cfb11d72f2e5ca75d7622d37b2a88a8998ec28e747a11",
  trade: "0x2c76e7a47fd53e2854856ac3f0a5f3ee40d15cfaa82266357ea9779c486ab9c3",
  graduated: "0x3a11b9c0ca38b86101cb9e6e1dd2f752c31467c6eaa353f931b801a338406de6",
  migrated: "0x57aa04076c8e8e00f17b6f082eb7c65ec1aa90f07da036638ccfcb07dcae6cc8",
  v3_migrated: "0x992845b53354dcd9aaa6bac6563775e438b83ec3765539e9c09b0ffb3e92421b",
};
const OUTPUT = new URL("./scan.json", import.meta.url);

let rpcId = 0;
let httpAttempts = 0;
let retries = 0;
let rateLimited = 0;
const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
async function rpc(method, params) {
  for (let attempt = 0; attempt < 6; attempt += 1) {
    httpAttempts += 1;
    const response = await fetch(RPC_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: ++rpcId, method, params }),
    });
    if (response.status === 429 && attempt < 5) {
      retries += 1;
      rateLimited += 1;
      await sleep(500 * 2 ** attempt);
      continue;
    }
    if (!response.ok) throw new Error(`${method}: HTTP ${response.status}`);
    const body = await response.json();
    if (body.error) throw new Error(`${method}: ${JSON.stringify(body.error)}`);
    await sleep(100);
    return body.result;
  }
  throw new Error(`${method}: retry budget exhausted`);
}

const hex = (value) => `0x${value.toString(16)}`;
const number = (value) => Number.parseInt(value, 16);
const topicAddress = (value) => `0x${value.slice(-40)}`.toLowerCase();
const topic0Name = new Map(Object.entries(TOPICS).map(([name, value]) => [value, name]));
const normalizeLog = (log) => ({
  address: log.address.toLowerCase(),
  block_number: number(log.blockNumber),
  block_hash: log.blockHash,
  transaction_hash: log.transactionHash,
  transaction_index: number(log.transactionIndex),
  log_index: number(log.logIndex),
  removed: Boolean(log.removed),
  event: topic0Name.get(log.topics[0]) ?? "unknown",
  topics: log.topics,
  data: log.data,
});
const logKey = (log) => [log.block_hash, log.transaction_hash, log.log_index, log.address, log.topics[0]].join(":");

async function blockAnchor(blockNumber) {
  const block = await rpc("eth_getBlockByNumber", [hex(blockNumber), false]);
  if (!block || number(block.number) !== blockNumber) throw new Error(`missing block ${blockNumber}`);
  return {
    l2_block_number: blockNumber,
    block_hash: block.hash,
    parent_hash: block.parentHash,
    timestamp: number(block.timestamp),
  };
}

async function logsFor(address, topics, fromBlock, toBlock) {
  return rpc("eth_getLogs", [{
    address,
    fromBlock: hex(fromBlock),
    toBlock: hex(toBlock),
    topics,
  }]);
}

const chainId = number(await rpc("eth_chainId", []));
if (chainId !== CHAIN_ID) throw new Error(`chain id ${chainId} != ${CHAIN_ID}`);
const anchorsBefore = [];
for (const range of RANGES) {
  anchorsBefore.push({
    label: range.label,
    from: await blockAnchor(range.from_block),
    to: await blockAnchor(range.to_block),
  });
}

const chunks = [];
const allLogs = [];
for (const range of RANGES) {
  for (let from = range.from_block; from <= range.to_block; from += CHUNK_BLOCKS) {
    const to = Math.min(range.to_block, from + CHUNK_BLOCKS - 1);
    const factoryLogs = await logsFor(
      FACTORY,
      [[TOPICS.token_created, TOPICS.trade, TOPICS.graduated, TOPICS.migrated]],
      from,
      to,
    );
    const migrationLogs = await logsFor(MIGRATOR, [TOPICS.v3_migrated], from, to);
    const normalized = [...factoryLogs, ...migrationLogs].map(normalizeLog);
    allLogs.push(...normalized);
    chunks.push({
      range: range.label,
      from_block: from,
      to_block: to,
      scanned_blocks: to - from + 1,
      factory_event_logs: factoryLogs.length,
      v3_migrated_logs: migrationLogs.length,
    });
  }
}

const seen = new Set();
const logs = allLogs
  .filter((log) => {
    if (log.removed) throw new Error(`removed log ${logKey(log)}`);
    const key = logKey(log);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  })
  .sort((a, b) => a.block_number - b.block_number || a.transaction_index - b.transaction_index || a.log_index - b.log_index);

const byTransaction = new Map();
for (const log of logs) {
  const grouped = byTransaction.get(log.transaction_hash) ?? [];
  grouped.push(log);
  byTransaction.set(log.transaction_hash, grouped);
}

const currentCandidates = [];
for (const [transactionHash, transactionLogs] of byTransaction) {
  const created = transactionLogs.filter((log) => log.event === "token_created");
  const trades = transactionLogs.filter((log) => log.event === "trade");
  const migrated = transactionLogs.some((log) => log.event === "migrated" || log.event === "v3_migrated");
  if (migrated || (created.length === 0 && trades.length === 0)) continue;
  const primary = created[0] ?? trades[0];
  const action_hint = created.length > 0
    ? "launch"
    : BigInt(`0x${trades[0].data.slice(2, 66)}`) === 0n ? "sell" : "buy";
  currentCandidates.push({
    transaction_hash: transactionHash,
    token: topicAddress(primary.topics[1]),
    block_number: primary.block_number,
    block_hash: primary.block_hash,
    transaction_index: primary.transaction_index,
    action_hint,
    primary_event_log_indices: transactionLogs
      .filter((log) => log.event === "token_created" || log.event === "trade")
      .map((log) => log.log_index),
  });
}

const migrationCandidates = logs
  .filter((log) => log.event === "v3_migrated")
  .map((log) => ({
    transaction_hash: log.transaction_hash,
    token: topicAddress(log.topics[1]),
    pool: topicAddress(log.topics[2]),
    token_id: BigInt(`0x${log.data.slice(2, 66)}`).toString(),
    block_number: log.block_number,
    block_hash: log.block_hash,
    transaction_index: log.transaction_index,
    log_index: log.log_index,
  }));

const anchorsAfter = [];
for (const range of RANGES) {
  anchorsAfter.push({
    label: range.label,
    from: await blockAnchor(range.from_block),
    to: await blockAnchor(range.to_block),
  });
}
if (JSON.stringify(anchorsBefore) !== JSON.stringify(anchorsAfter)) {
  throw new Error("scan endpoint anchors drifted during collection");
}

const countsByEvent = Object.fromEntries(Object.keys(TOPICS).map((name) => [name, 0]));
for (const log of logs) countsByEvent[log.event] = (countsByEvent[log.event] ?? 0) + 1;
const deduplicatedLogsSha256 = createHash("sha256")
  .update(logs.map((log) => JSON.stringify(log)).join("\n"))
  .digest("hex");
const output = {
  schema_version: 1,
  collected_at: new Date().toISOString(),
  source_commit: "3829a7b2dccb2c651c85a920e19c2f705607ab6d",
  rpc_url: RPC_URL,
  chain_id: chainId,
  concurrency: 1,
  rpc_metrics: { logical_requests: rpcId - retries, http_attempts: httpAttempts, retries, rate_limited: rateLimited },
  scan: {
    range_semantics: "two_disjoint_inclusive_ranges_aggregate_bounded",
    requested_cap_blocks: 750_000,
    ranges: RANGES.map((range) => ({
      ...range,
      scanned_blocks: range.to_block - range.from_block + 1,
    })),
    scanned_blocks: RANGES.reduce((sum, range) => sum + range.to_block - range.from_block + 1, 0),
    chunk_blocks: CHUNK_BLOCKS,
    chunks,
    anchors_before: anchorsBefore,
    anchors_after: anchorsAfter,
  },
  identity: { factory: FACTORY, migrator: MIGRATOR, topics: TOPICS },
  counts: {
    raw_logs: allLogs.length,
    deduplicated_logs: logs.length,
    deduplicated_logs_sha256: deduplicatedLogsSha256,
    duplicate_logs: allLogs.length - logs.length,
    by_event: countsByEvent,
    current_curve_candidates: currentCandidates.length,
    migrated_v3_boundary_scopes: migrationCandidates.length,
  },
  current_curve_candidates: currentCandidates,
  migrated_v3_boundary_candidates: migrationCandidates,
};

await writeFile(OUTPUT, `${JSON.stringify(output, null, 2)}\n`, { flag: "wx", mode: 0o644 });
console.log(JSON.stringify({
  output: OUTPUT.pathname,
  scanned_blocks: output.scan.scanned_blocks,
  counts: output.counts,
}));
