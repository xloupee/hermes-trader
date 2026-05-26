import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  PUMP_BONDING_CURVE_PROGRAM_ID,
  PUMPSWAP_AMM_PROGRAM_ID,
  normalizeShredstreamTransaction
} from "../dist/shredstream-decoder.js";
import { runShredListener, shredDiscoveryEnabled } from "../dist/shred-listener.js";

const receivedAtMs = 1770000000123;
const signature = "5VfUXexampleTxSignature111111111111111111111111111111111";
const systemProgram = "11111111111111111111111111111111";
const mint = "DezXAZ8z7PnrnRJjz3n26VsZdrmRwNny4nBj9JkzjW7B";
const trader = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
const bondingCurve = "BndgCurve1111111111111111111111111111111111";
const pool = "Pool111111111111111111111111111111111111111";

function dataBase64(discriminatorHex, u64Args = []) {
  const buffer = Buffer.alloc(8 + u64Args.length * 8);
  Buffer.from(discriminatorHex, "hex").copy(buffer, 0);

  for (const [index, value] of u64Args.entries()) {
    buffer.writeBigUInt64LE(BigInt(value), 8 + index * 8);
  }

  return buffer.toString("base64");
}

test("ignores non-Pump transactions", () => {
  const events = normalizeShredstreamTransaction(
    {
      slot: 123,
      signature,
      accountKeys: [systemProgram],
      instructions: [
        {
          programIdIndex: 0,
          accounts: [],
          dataBase64: dataBase64("0102030405060708")
        }
      ]
    },
    { receivedAtMs }
  );

  assert.deepEqual(events, []);
});

test("emits unknown Pump instructions with discriminator and best-effort accounts", () => {
  const events = normalizeShredstreamTransaction(
    {
      slot: 124,
      signature,
      accountKeys: [
        PUMP_BONDING_CURVE_PROGRAM_ID,
        "Global1111111111111111111111111111111111111",
        mint,
        bondingCurve,
        "AssociatedBondingCurve111111111111111111111",
        "AssociatedUser11111111111111111111111111111",
        trader
      ],
      instructions: [
        {
          programIdIndex: 0,
          accounts: [1, 1, 2, 3, 4, 5, 6],
          dataBase64: dataBase64("0102030405060708")
        }
      ]
    },
    { receivedAtMs }
  );

  assert.equal(events.length, 1);
  assert.deepEqual(events[0], {
    source: "shredstream",
    slot: 124,
    signature,
    receivedAtMs,
    programId: PUMP_BONDING_CURVE_PROGRAM_ID,
    eventType: "unknown-pump",
    mint,
    trader,
    bondingCurve,
    baseMint: mint,
    pool: "pump",
    rawInstructionDiscriminator: "0102030405060708",
    instructionIndex: 0,
    decodeStatus: "unknown-discriminator"
  });
});

test("decodes Pump buy instructions and extracts mint trader and amounts", () => {
  const events = normalizeShredstreamTransaction(
    {
      slot: 125,
      signature,
      accountKeys: [
        PUMP_BONDING_CURVE_PROGRAM_ID,
        "Global1111111111111111111111111111111111111",
        "FeeRecipient1111111111111111111111111111111",
        mint,
        bondingCurve,
        "AssociatedBondingCurve111111111111111111111",
        "AssociatedUser11111111111111111111111111111",
        trader
      ],
      instructions: [
        {
          programIdIndex: 0,
          accounts: [1, 2, 3, 4, 5, 6, 7],
          dataBase64: dataBase64("66063d1201daebea", ["123456789", "250000000"])
        }
      ]
    },
    { receivedAtMs }
  );

  assert.equal(events.length, 1);
  assert.equal(events[0].eventType, "buy");
  assert.equal(events[0].decodeStatus, "decoded");
  assert.equal(events[0].mint, mint);
  assert.equal(events[0].trader, trader);
  assert.equal(events[0].bondingCurve, bondingCurve);
  assert.equal(events[0].baseMint, mint);
  assert.equal(events[0].pool, "pump");
  assert.equal(events[0].tokenAmountRaw, "123456789");
  assert.equal(events[0].maxSolCostLamports, "250000000");
  assert.equal(events[0].solAmountLamports, undefined);
  assert.equal(events[0].amountSemantics, "token_amount_out_with_max_sol_cost");
});

test("decodes Pump v2 buy instructions with quote amount first", () => {
  const events = normalizeShredstreamTransaction(
    {
      slot: 125,
      signature,
      accountKeys: [
        PUMP_BONDING_CURVE_PROGRAM_ID,
        mint,
        "So11111111111111111111111111111111111111112",
        "TokenProgram1111111111111111111111111111111",
        "QuoteTokenProgram1111111111111111111111111",
        "AssociatedToken1111111111111111111111111111",
        "FeeRecipient1111111111111111111111111111111",
        "QuoteFeeRecipient111111111111111111111111",
        "BuybackFeeRecipient11111111111111111111111",
        "QuoteBuybackFee111111111111111111111111111",
        bondingCurve,
        "AssociatedBaseBonding111111111111111111111",
        "AssociatedQuoteBonding11111111111111111111",
        trader
      ],
      instructions: [
        {
          programIdIndex: 0,
          accounts: [1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13],
          dataBase64: dataBase64("c2ab1c46684d5b2f", ["250000000", "123456789"])
        }
      ]
    },
    { receivedAtMs }
  );

  assert.equal(events.length, 1);
  assert.equal(events[0].eventType, "buy");
  assert.equal(events[0].mint, mint);
  assert.equal(events[0].trader, trader);
  assert.equal(events[0].bondingCurve, bondingCurve);
  assert.equal(events[0].baseMint, mint);
  assert.equal(events[0].quoteMint, "So11111111111111111111111111111111111111112");
  assert.equal(events[0].solAmountLamports, "250000000");
  assert.equal(events[0].spendableQuoteAmountIn, "250000000");
  assert.equal(events[0].minTokenAmountOut, "123456789");
  assert.equal(events[0].tokenAmountRaw, undefined);
  assert.equal(events[0].amountSemantics, "spendable_quote_in_with_min_tokens_out");
});

test("decodes PumpSwap instructions as pump-amm events", () => {
  const events = normalizeShredstreamTransaction(
    {
      slot: 126,
      signature,
      accountKeys: [
        pool,
        trader,
        "GlobalConfig11111111111111111111111111111111",
        mint,
        "So11111111111111111111111111111111111111112"
      ],
      instructions: [
        {
          programId: PUMPSWAP_AMM_PROGRAM_ID,
          accounts: [0, 1, 2, 3, 4],
          dataBase64: dataBase64("66063d1201daebea", ["987654321", "100000000"])
        }
      ]
    },
    { receivedAtMs }
  );

  assert.equal(events.length, 1);
  assert.equal(events[0].programId, PUMPSWAP_AMM_PROGRAM_ID);
  assert.equal(events[0].eventType, "buy");
  assert.equal(events[0].decodeStatus, "decoded");
  assert.equal(events[0].pool, "pump-amm");
  assert.equal(events[0].mint, mint);
  assert.equal(events[0].trader, trader);
  assert.equal(events[0].baseMint, mint);
  assert.equal(events[0].quoteMint, "So11111111111111111111111111111111111111112");
  assert.equal(events[0].tokenAmountRaw, "987654321");
  assert.equal(events[0].maxQuoteAmountIn, "100000000");
  assert.equal(events[0].solAmountLamports, "100000000");
});

test("decodes non-WSOL PumpSwap quote amounts without labeling them as SOL", () => {
  const quoteMint = "USDC111111111111111111111111111111111111111";
  const [event] = normalizeShredstreamTransaction(
    {
      slot: 126,
      signature,
      accountKeys: [pool, trader, "GlobalConfig11111111111111111111111111111111", mint, quoteMint],
      instructions: [
        {
          programId: PUMPSWAP_AMM_PROGRAM_ID,
          accounts: [0, 1, 2, 3, 4],
          dataBase64: dataBase64("66063d1201daebea", ["987654321", "100000000"])
        }
      ]
    },
    { receivedAtMs }
  );

  assert.equal(event.quoteMint, quoteMint);
  assert.equal(event.maxQuoteAmountIn, "100000000");
  assert.equal(event.solAmountLamports, undefined);
});

test("handles invalid base64 without throwing", () => {
  const events = normalizeShredstreamTransaction(
    {
      slot: 127,
      signature,
      accountKeys: [PUMP_BONDING_CURVE_PROGRAM_ID],
      instructions: [
        {
          programIdIndex: 0,
          accounts: [],
          dataBase64: "not base64!!!"
        }
      ]
    },
    { receivedAtMs }
  );

  assert.equal(events.length, 1);
  assert.equal(events[0].eventType, "unknown-pump");
  assert.equal(events[0].decodeStatus, "invalid-data");
  assert.equal(events[0].rawInstructionDiscriminator, undefined);
});

test("shred listener requires explicit enable env helper", () => {
  assert.equal(shredDiscoveryEnabled({ SHREDSTREAM_DISCOVERY_ENABLED: "true" }), true);
  assert.equal(shredDiscoveryEnabled({ SHREDSTREAM_DISCOVERY_ENABLED: "false" }), false);
  assert.equal(shredDiscoveryEnabled({}), false);
});

test("shred listener reads JSONL and writes normalized Pump events", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pumpfunnoti-shredstream-"));
  const inputPath = join(dir, "input.jsonl");
  const outputPath = join(dir, "events.jsonl");
  await mkdir(dir, { recursive: true });
  await writeFile(
    inputPath,
    [
      JSON.stringify({
        slot: 128,
        signature,
        receivedAtMs,
        accountKeys: [PUMP_BONDING_CURVE_PROGRAM_ID, mint],
        instructions: [
          {
            programIdIndex: 0,
            accounts: [1],
            dataBase64: dataBase64("181ec828051c0777")
          }
        ]
      }),
      "not json"
    ].join("\n"),
    "utf8"
  );

  const stats = await runShredListener({ inputPath, eventLogPath: outputPath });
  const [line] = (await readFile(outputPath, "utf8")).trim().split("\n");
  const event = JSON.parse(line);

  assert.deepEqual(stats, {
    linesRead: 2,
    recordsAccepted: 1,
    eventsWritten: 1,
    parseErrors: 1
  });
  assert.equal(event.source, "shredstream");
  assert.equal(event.signature, signature);
  assert.equal(event.eventType, "create");
});
