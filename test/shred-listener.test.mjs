import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { runShredListener, shredDiscoveryEnabled } from "../dist/shred-listener.js";
import {
  createShredstreamSource,
  parseShredstreamSourceMode,
  resolveShredstreamSourceConfig
} from "../dist/shredstream-source.js";
import { FLASHX_ROUTER_PROGRAM_ID, PUMP_BONDING_CURVE_PROGRAM_ID, PUMPSWAP_AMM_PROGRAM_ID } from "../dist/shredstream-decoder.js";

function dataBase64(discriminatorHex) {
  const buffer = Buffer.alloc(8);
  Buffer.from(discriminatorHex, "hex").copy(buffer, 0);
  return buffer.toString("base64");
}

test("ShredStream listener kill switch is off unless explicitly enabled", () => {
  assert.equal(shredDiscoveryEnabled({}), false);
  assert.equal(shredDiscoveryEnabled({ SHREDSTREAM_DISCOVERY_ENABLED: "false" }), false);
  assert.equal(shredDiscoveryEnabled({ SHREDSTREAM_DISCOVERY_ENABLED: "true" }), true);
});

test("ShredStream source mode defaults to JSONL stdin", () => {
  assert.equal(parseShredstreamSourceMode({}), "jsonl");
  assert.deepEqual(resolveShredstreamSourceConfig({}), {
    mode: "jsonl",
    inputPath: "-"
  });
});

test("ShredStream source mode rejects invalid values", () => {
  assert.throws(
    () => resolveShredstreamSourceConfig({ SHREDSTREAM_SOURCE: "websocket" }),
    /Invalid SHREDSTREAM_SOURCE="websocket".*Expected "jsonl" or "grpc"/
  );
  assert.throws(
    () =>
      resolveShredstreamSourceConfig({
        SHREDSTREAM_SOURCE: "grpc",
        SHREDSTREAM_GRPC_URL: "127.0.0.1:9999",
        SHREDSTREAM_ALT_LOOKUP_TIMEOUT_MS: "soon"
      }),
    /Invalid SHREDSTREAM_ALT_LOOKUP_TIMEOUT_MS="soon".*Expected a non-negative number/
  );
});

test("ShredStream gRPC source mode requires a URL before adapter creation", () => {
  assert.throws(
    () => resolveShredstreamSourceConfig({ SHREDSTREAM_SOURCE: "grpc" }),
    /SHREDSTREAM_GRPC_URL is required when SHREDSTREAM_SOURCE=grpc/
  );
});

test("ShredStream gRPC source mode reads normalized JSONL from decoder command", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "shred-listener-grpc-"));
  t.after(() => rm(dir, { recursive: true, force: true }));

  const decoderPath = join(dir, "decoder.mjs");
  const eventLogPath = join(dir, "events.jsonl");

  await writeFile(
    decoderPath,
    `
const discriminator = Buffer.alloc(8);
Buffer.from("181ec828051c0777", "hex").copy(discriminator, 0);
console.log(JSON.stringify({
  slot: 4,
  signature: "grpc-pump-sig",
  receivedAtMs: 300,
  accountKeys: ["${PUMP_BONDING_CURVE_PROGRAM_ID}", "mint", "bonding", "trader"],
  instructions: [{ programIdIndex: 0, accounts: [1, 1, 2, 1, 1, 1, 1, 3], dataBase64: discriminator.toString("base64") }]
}));
const sellDiscriminator = Buffer.alloc(8);
Buffer.from("33e685a4017f83ad", "hex").copy(sellDiscriminator, 0);
const buyDiscriminator = Buffer.alloc(24);
Buffer.from("c62e1552b4d9e870", "hex").copy(buyDiscriminator, 0);
buyDiscriminator.writeBigUInt64LE(100000n, 8);
buyDiscriminator.writeBigUInt64LE(147967581n, 16);
console.log(JSON.stringify({
  slot: 5,
  signature: "grpc-amm-alt-sig",
  receivedAtMs: 301,
  accountKeys: ["${PUMPSWAP_AMM_PROGRAM_ID}", "pool", "trader", "global", "So11111111111111111111111111111111111111112"],
  addressTableLookups: [{ accountKey: "lookup-table", writableIndexes: [0], readonlyIndexes: [1] }],
  instructions: [{ programIdIndex: 0, accounts: [1, 2, 3, 6, 4], dataBase64: sellDiscriminator.toString("base64") }]
}));
console.log(JSON.stringify({
  slot: 6,
  signature: "grpc-amm-static-alt-sig",
  receivedAtMs: 301,
  accountKeys: ["${PUMPSWAP_AMM_PROGRAM_ID}", "pool", "trader", "static-amm-mint-pump", "So11111111111111111111111111111111111111112"],
  addressTableLookups: [{ accountKey: "static-amm-lookup-table", writableIndexes: [0], readonlyIndexes: [1] }],
  instructions: [{ programIdIndex: 0, accounts: [1, 2, 0, 3, 4], dataBase64: buyDiscriminator.toString("base64") }]
}));
const flashxData = Buffer.alloc(22);
flashxData[0] = 0;
flashxData.writeBigUInt64LE(990000n, 1);
flashxData.writeBigUInt64LE(29142236873n, 9);
console.log(JSON.stringify({
  slot: 7,
  signature: "grpc-flashx-alt-sig",
  receivedAtMs: 302,
  accountKeys: ["trader", "ata", "${FLASHX_ROUTER_PROGRAM_ID}", "router-state", "bonding", "associated-bonding", "creator-vault", "local", "ComputeBudget111111111111111111111111111111", "AssociatedToken1111111111111111111111111111", "flashx-mint-pump", "11111111111111111111111111111111"],
  addressTableLookups: [{ accountKey: "flashx-lookup-table", writableIndexes: [0], readonlyIndexes: [1, 2] }],
  instructions: [{ programIdIndex: 2, accounts: [0, 0, 12, 11, 2, 13, 3, 3, 14, 12, 10, 4, 5, 1, 0, 11, 14, 6, 3, 13, 3, 7, 14, 3], dataBase64: flashxData.toString("base64") }]
}));
`,
    "utf8"
  );

  let flashxLookupCalls = 0;
  let staticAmmLookupCalls = 0;
  const source = createShredstreamSource({
    mode: "grpc",
    grpcUrl: "127.0.0.1:9999",
    decoderCommand: `node ${decoderPath}`,
    addressLookupTableResolver: (lookup) => {
      if (lookup.accountKey === "lookup-table") {
        return ["loaded-ata", "loaded-mint"];
      }

      if (lookup.accountKey === "static-amm-lookup-table") {
        staticAmmLookupCalls += 1;
        return ["unused-static-amm-ata", "unused-static-amm-mint"];
      }

      assert.equal(lookup.accountKey, "flashx-lookup-table");
      flashxLookupCalls += 1;
      return ["loaded-router-ata", PUMP_BONDING_CURVE_PROGRAM_ID, "loaded-token-program"];
    }
  });
  const stats = await runShredListener({ source, eventLogPath });
  const lines = (await readFile(eventLogPath, "utf8")).trim().split("\n").map((line) => JSON.parse(line));

  assert.deepEqual(stats, {
    linesRead: 4,
    recordsAccepted: 4,
    eventsWritten: 4,
    parseErrors: 0
  });
  assert.equal(lines[0].signature, "grpc-pump-sig");
  assert.equal(lines[0].eventType, "create");
  assert.equal(lines[1].signature, "grpc-amm-alt-sig");
  assert.equal(lines[1].eventType, "sell");
  assert.equal(lines[1].mint, "loaded-mint");
  assert.equal(lines[1].sourceTiming.altLookupStatus, "hydrated");
  assert.equal(lines[2].signature, "grpc-amm-static-alt-sig");
  assert.equal(lines[2].eventType, "buy");
  assert.equal(lines[2].mint, "static-amm-mint-pump");
  assert.equal(lines[2].sourceTiming.altLookupStatus, "static_decoded");
  assert.equal(lines[3].signature, "grpc-flashx-alt-sig");
  assert.equal(lines[3].eventType, "buy");
  assert.equal(lines[3].routerProgramId, FLASHX_ROUTER_PROGRAM_ID);
  assert.equal(lines[3].mint, "flashx-mint-pump");
  assert.equal(lines[3].trader, "trader");
  assert.equal(lines[3].sourceTiming.altLookupStatus, "not_needed");
  assert.equal(flashxLookupCalls, 0);
  assert.equal(staticAmmLookupCalls, 0);
});

test("ShredStream gRPC source mode caps slow ALT lookup waits", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "shred-listener-grpc-timeout-"));
  t.after(() => rm(dir, { recursive: true, force: true }));

  const decoderPath = join(dir, "decoder.mjs");
  await writeFile(
    decoderPath,
    `
const sellDiscriminator = Buffer.alloc(8);
Buffer.from("33e685a4017f83ad", "hex").copy(sellDiscriminator, 0);
console.log(JSON.stringify({
  slot: 8,
  signature: "grpc-amm-slow-alt-sig",
  receivedAtMs: 400,
  accountKeys: ["${PUMPSWAP_AMM_PROGRAM_ID}", "pool", "trader", "global", "So11111111111111111111111111111111111111112"],
  addressTableLookups: [{ accountKey: "slow-lookup-table", writableIndexes: [0], readonlyIndexes: [1] }],
  instructions: [{ programIdIndex: 0, accounts: [1, 2, 3, 6, 4], dataBase64: sellDiscriminator.toString("base64") }]
}));
`,
    "utf8"
  );

  const source = createShredstreamSource({
    mode: "grpc",
    grpcUrl: "127.0.0.1:9999",
    decoderCommand: `node ${decoderPath}`,
    addressLookupTableTimeoutMs: 1,
    addressLookupTableResolver: () => new Promise((resolve) => setTimeout(() => resolve(["loaded-ata", "loaded-mint"]), 50))
  });

  const records = [];
  for await (const record of source.readRecords()) {
    records.push(record);
  }

  assert.equal(records.length, 1);
  assert.equal(records[0].transaction.signature, "grpc-amm-slow-alt-sig");
  assert.equal(records[0].transaction.sourceTiming.altLookupStatus, "timeout_or_error");
  assert.equal(records[0].transaction.sourceTiming.altLookupCount, 1);
  assert.equal(records[0].transaction.sourceTiming.altLookupTimeoutMs, 1);
  assert.deepEqual(records[0].transaction.accountKeys, [
    PUMPSWAP_AMM_PROGRAM_ID,
    "pool",
    "trader",
    "global",
    "So11111111111111111111111111111111111111112"
  ]);
});

test("JSONL harness writes only decoded Pump and PumpSwap events", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "shred-listener-"));
  t.after(() => rm(dir, { recursive: true, force: true }));

  const inputPath = join(dir, "input.jsonl");
  const eventLogPath = join(dir, "events.jsonl");

  await writeFile(
    inputPath,
    [
      JSON.stringify({
        slot: 1,
        signature: "pump-sig",
        receivedAtMs: 100,
        accountKeys: [PUMP_BONDING_CURVE_PROGRAM_ID, "mint", "bonding", "trader"],
        instructions: [
          {
            programIdIndex: 0,
            accounts: [1, 1, 2, 1, 1, 1, 1, 3],
            dataBase64: dataBase64("181ec828051c0777")
          }
        ]
      }),
      JSON.stringify({
        slot: 2,
        signature: "other-sig",
        accountKeys: ["11111111111111111111111111111111"],
        instructions: [{ programIdIndex: 0, accounts: [], dataBase64: dataBase64("181ec828051c0777") }]
      }),
      "{invalid-json",
      JSON.stringify({
        slot: 3,
        signature: "amm-sig",
        receivedAtMs: 200,
        accountKeys: [PUMPSWAP_AMM_PROGRAM_ID, "pool", "trader", "global", "mint"],
        instructions: [
          {
            programId: PUMPSWAP_AMM_PROGRAM_ID,
            accounts: [1, 2, 3, 4],
            dataBase64: dataBase64("33e685a4017f83ad")
          }
        ]
      })
    ].join("\n"),
    "utf8"
  );

  const stats = await runShredListener({ inputPath, eventLogPath });
  const lines = (await readFile(eventLogPath, "utf8")).trim().split("\n").map((line) => JSON.parse(line));

  assert.deepEqual(stats, {
    linesRead: 4,
    recordsAccepted: 3,
    eventsWritten: 2,
    parseErrors: 1
  });
  assert.deepEqual(
    lines.map((line) => [
      line.signature,
      line.eventType,
      line.decodeStatus,
      line.pool,
      line.mint,
      line.trader,
      line.bondingCurve,
      line.instructionIndex
    ]),
    [
      ["pump-sig", "create", "decoded", "pump", "mint", "trader", "bonding", 0],
      ["amm-sig", "sell", "decoded", "pump-amm", "mint", "trader", undefined, 0]
    ]
  );
});

test("JSONL harness rejects malformed instruction records without crashing", async (t) => {
  const dir = await mkdtemp(join(tmpdir(), "shred-listener-malformed-"));
  t.after(() => rm(dir, { recursive: true, force: true }));

  const inputPath = join(dir, "input.jsonl");
  const eventLogPath = join(dir, "events.jsonl");

  await writeFile(
    inputPath,
    [
      JSON.stringify({
        slot: 1,
        signature: "bad-instruction",
        accountKeys: [PUMP_BONDING_CURVE_PROGRAM_ID],
        instructions: [null]
      }),
      JSON.stringify({
        slot: 2,
        signature: "bad-accounts",
        accountKeys: [PUMP_BONDING_CURVE_PROGRAM_ID],
        instructions: [{ programIdIndex: 0, accounts: "not-array", dataBase64: dataBase64("181ec828051c0777") }]
      })
    ].join("\n"),
    "utf8"
  );

  const stats = await runShredListener({ inputPath, eventLogPath });

  assert.deepEqual(stats, {
    linesRead: 2,
    recordsAccepted: 0,
    eventsWritten: 0,
    parseErrors: 2
  });
});
