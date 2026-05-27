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
import { PUMP_BONDING_CURVE_PROGRAM_ID, PUMPSWAP_AMM_PROGRAM_ID } from "../dist/shredstream-decoder.js";

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
`,
    "utf8"
  );

  const source = createShredstreamSource({
    mode: "grpc",
    grpcUrl: "127.0.0.1:9999",
    decoderCommand: `node ${decoderPath}`
  });
  const stats = await runShredListener({ source, eventLogPath });
  const lines = (await readFile(eventLogPath, "utf8")).trim().split("\n").map((line) => JSON.parse(line));

  assert.deepEqual(stats, {
    linesRead: 1,
    recordsAccepted: 1,
    eventsWritten: 1,
    parseErrors: 0
  });
  assert.equal(lines[0].signature, "grpc-pump-sig");
  assert.equal(lines[0].eventType, "create");
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
