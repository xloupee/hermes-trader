#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { writeFile } from "node:fs/promises";

const RPC_URL = "https://rpc.mainnet.chain.robinhood.com";
const CAST = "/Users/kennethjiang/.foundry/bin/cast";
const BLOCKS = [10_794_005, 11_735_771];
const OUTPUT = new URL("./pins.json", import.meta.url);
const IDENTITIES = [
  ["factory", "0x5fcc1df0dc020cf454e742e9a8ae2554c37a452c", 20518, "0x4aa0ce56b5b67d27f2fab59dcb796fa552d10ceafdecb06e088cdd254c92c0fc"],
  ["migrator", "0x5790ef23be2e1543442c12f4550fae147ba8edbe", 3577, "0x88b7c4f6dfb99df8493cf7b7905a538212fc1c7eb176ffbbcaade5a6988c83d6"],
  ["locker", "0xad69d8a00564f4a2365cc74594925f95281706aa", 3583, "0xee4522db997a71e396e90ef14a123f3a4a857268040b17a618ff2f47e204eb4a"],
  ["position_manager", "0x73991a25c818bf1f1128deaab1492d45638de0d3", 24384, "0x0a493d1af3d0f25fed8efa205244ebee14114267a08647fc38c515c7cd6ead4f"],
  ["v3_factory", "0x1f7d7550b1b028f7571e69a784071f0205fd2efa", 24535, "0xec72b1abd1f2faee020cfea9c646bd8994f9fb389054f6e574f103a895091739"],
  ["swap_router", "0xcaf681a66d020601342297493863e78c959e5cb2", 24497, "0x6f36c378e272c6324c48f045182bcb54bd8ad654cf9ebd42e8893d52c4cb25dc"],
  ["weth", "0x0bd7d308f8e1639fab988df18a8011f41eacad73", 2202, "0x5706be52f64875fee65a2cec0d80e47a23d8793cbe85d214b48445e2d05f5353"],
  ["fallback_factory", "0x8bceaa40b9acdfaedf85adf4ff01f5ad6517937f", 13859, "0xbab145d02e7005f0d84c6c1639d39b799b0ea16df99ebbdaf5a14d9da820b4e0"],
  ["owner_safe_proxy", "0xb3f3b54e11217f4f73e7a766b7caa187390d700d", 171, "0xd7d408ebcd99b2b70be43e20253d6d92a8ea8fab29bd3be7f55b10032331fb4c"],
  ["owner_safe_singleton", "0x29fcb43b46531bca003ddc8fcb67ffe91900c762", 24421, "0xb1f926978a0f44a2c0ec8fe822418ae969bd8c3f18d61e5103100339894f81ff"],
];

let id = 0;
async function rpc(method, params) {
  for (let attempt = 0; attempt < 6; attempt += 1) {
    const response = await fetch(RPC_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: ++id, method, params }),
    });
    if (response.status === 429 && attempt < 5) {
      await new Promise((resolve) => setTimeout(resolve, 500 * 2 ** attempt));
      continue;
    }
    if (!response.ok) throw new Error(`${method}: HTTP ${response.status}`);
    const body = await response.json();
    if (body.error) throw new Error(`${method}: ${JSON.stringify(body.error)}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
    return body.result;
  }
  throw new Error(`${method}: retry budget exhausted`);
}

const observations = [];
for (const blockNumber of BLOCKS) {
  const blockTag = `0x${blockNumber.toString(16)}`;
  for (const [role, address, expectedBytes, expectedHash] of IDENTITIES) {
    const code = await rpc("eth_getCode", [address, blockTag]);
    const codeBytes = (code.length - 2) / 2;
    const runtimeHash = execFileSync(CAST, ["keccak", code], { encoding: "utf8" }).trim();
    const matched = codeBytes === expectedBytes && runtimeHash.toLowerCase() === expectedHash;
    observations.push({
      block_number: blockNumber,
      role,
      address,
      expected_code_bytes: expectedBytes,
      observed_code_bytes: codeBytes,
      expected_runtime_hash: expectedHash,
      observed_runtime_hash: runtimeHash,
      matched,
    });
    if (!matched) throw new Error(`${role} pin drift at ${blockNumber}`);
  }
}

const output = {
  schema_version: 1,
  source_commit: "3829a7b2dccb2c651c85a920e19c2f705607ab6d",
  rpc_url: RPC_URL,
  chain_id: Number.parseInt(await rpc("eth_chainId", []), 16),
  concurrency: 1,
  blocks: BLOCKS,
  checks: observations.length,
  mismatches: observations.filter((row) => !row.matched).length,
  all_matched: observations.every((row) => row.matched),
  observations,
};
if (output.chain_id !== 4663) throw new Error(`chain id ${output.chain_id} != 4663`);
await writeFile(OUTPUT, `${JSON.stringify(output, null, 2)}\n`, { flag: "wx", mode: 0o644 });
console.log(JSON.stringify({ output: OUTPUT.pathname, checks: output.checks, mismatches: output.mismatches }));
