import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  FLASHX_ROUTER_PROGRAM_ID,
  PUMP_BONDING_CURVE_PROGRAM_ID,
  PUMPSWAP_AMM_PROGRAM_ID,
  normalizeShredstreamTransaction,
  rawPumpDiscoveryEventToWalletTrade
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

function flashxBuyDataBase64(solLamports, minTokenAmountOut) {
  const buffer = Buffer.alloc(22);
  buffer[0] = 0;
  buffer.writeBigUInt64LE(BigInt(solLamports), 1);
  buffer.writeBigUInt64LE(BigInt(minTokenAmountOut), 9);
  buffer[17] = 0;
  buffer[18] = 1;
  buffer[19] = 0x21;
  buffer[20] = 0x32;
  buffer[21] = 0;
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

test("converts decoded ShredStream buys into shadow wallet trade rows", () => {
  const [event] = normalizeShredstreamTransaction(
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

  const trade = rawPumpDiscoveryEventToWalletTrade({
    event,
    wallet: { address: trader, label: "fast wallet" },
    explorer: {
      pumpFunBaseUrl: "https://pump.fun/coin",
      solscanBaseUrl: "https://solscan.io"
    }
  });

  assert.equal(trade?.provider, "shredstream");
  assert.equal(trade?.action, "buy");
  assert.equal(trade?.targetWallet, trader);
  assert.equal(trade?.label, "fast wallet");
  assert.equal(trade?.mint, mint);
  assert.equal(trade?.signature, signature);
  assert.equal(trade?.source, "SHREDSTREAM_PUMP");
  assert.equal(trade?.input?.mint, "So11111111111111111111111111111111111111112");
  assert.equal(trade?.input?.symbol, "SOL");
  assert.equal(trade?.output?.mint, mint);
  assert.equal(trade?.tokenAmount, 123.456789);
  assert.equal(trade?.solAmount, 0.25);
  assert.equal(trade?.pumpFunUrl, `https://pump.fun/coin/${mint}`);
  assert.equal(trade?.solscanTxUrl, `https://solscan.io/tx/${signature}`);
  assert.equal(trade?.raw.parser, "shredstream-pump-instruction");
});

test("decodes FLASHX router Pump buys without hydrated ALT accounts", () => {
  const events = normalizeShredstreamTransaction(
    {
      slot: 126,
      signature: "flashx-router-buy-sig",
      accountKeys: [
        trader,
        "UserMintAta11111111111111111111111111111111",
        FLASHX_ROUTER_PROGRAM_ID,
        "RouterState11111111111111111111111111111111",
        bondingCurve,
        "AssociatedBondingCurve111111111111111111111",
        "CreatorVault1111111111111111111111111111111",
        "Local11111111111111111111111111111111111111",
        "ComputeBudget111111111111111111111111111111",
        "AssociatedToken1111111111111111111111111111",
        mint,
        systemProgram,
        "FeeRecipient1111111111111111111111111111111",
        "RouterAux111111111111111111111111111111111",
        "EventAuthority11111111111111111111111111111"
      ],
      instructions: [
        {
          programIdIndex: 2,
          accounts: [0, 0, 15, 11, 2, 16, 12, 3, 17, 15, 10, 4, 5, 1, 0, 11, 18, 6, 17, 16, 13, 7, 18, 19],
          dataBase64: flashxBuyDataBase64("990000", "29142236873")
        }
      ]
    },
    { receivedAtMs }
  );

  assert.equal(events.length, 1);
  assert.equal(events[0].eventType, "buy");
  assert.equal(events[0].decodeStatus, "decoded");
  assert.equal(events[0].programId, PUMP_BONDING_CURVE_PROGRAM_ID);
  assert.equal(events[0].routerProgramId, FLASHX_ROUTER_PROGRAM_ID);
  assert.equal(events[0].mint, mint);
  assert.equal(events[0].trader, trader);
  assert.equal(events[0].bondingCurve, bondingCurve);
  assert.equal(events[0].quoteMint, "So11111111111111111111111111111111111111112");
  assert.equal(events[0].spendableSolLamports, "990000");
  assert.equal(events[0].solAmountLamports, "990000");
  assert.equal(events[0].minTokenAmountOut, "29142236873");
});

test("decodes FLASHX router Pump buys from target token balance deltas instead of temp WSOL account indexes", () => {
  const wrongTempWsolAccount = "83DqVhmHb3RmZa8ieYC7VtHB5upyC5GHAr6g4WYfMjg4";
  const actualMint = "5QmKEjUKCPpusjDycze5J875edpqtcHy8GUvdmB1pump";
  const actualTokenAccount = "8Bdtcgjc263mAwEALzPw3kwpzNZqRhgnugjhENu1vXZp";
  const actualBondingCurve = "8fwrauNJMqDGqduDsDFTTShceFGx61Yn94wgPihhpYsd";
  const targetWallet = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
  const events = normalizeShredstreamTransaction(
    {
      slot: 423222176,
      signature: "3i6fp3KzBmKTja7kYM1WruhBfAPVZknBXBbNxj98E1413p9RGixYHcVPGNgQUkfuHk7eNmxwZXGGR4Ag15TbY4dx",
      accountKeys: [
        targetWallet,
        actualTokenAccount,
        "BAzr8WMC3TDX7xi1bi1fb1kV9kugFUFf83T4Ypp5CSiy",
        FLASHX_ROUTER_PROGRAM_ID,
        wrongTempWsolAccount,
        "5dQZShePk85ztskSABT2nTo5iYgAykiPvu1PKsFnFhT9",
        "HXNZP8xNU6EqUNVzACYwQ5PJWJ95wzjK7kwnVpumhT93",
        actualBondingCurve,
        "72ayp8X7L5fCTmEtXBBYMKuvFt1p2mj5vxcV2KfJNBpa",
        "H7LM5X8oVPR5uiTjHGBUXjtkutDvMvyFjrBxe87kSLB5",
        "8aHZJSt6frgjRTTfg4foDXjZHMyZ2ZQQjpwcWzzCvAGp",
        "GmTjpGuvGsCrBFUAdV3RnBYqFC3Vmcr2Z9XkDWtu46dd",
        "ComputeBudget111111111111111111111111111111",
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
        actualMint,
        systemProgram,
        "FSqbigodv6rL8ABcJkZDF1Lps8HQnpuK2Df1ffH8ScWf",
        "Hq2wp8uJ9jCPsYgNHex8RtqdvMPfVGoYwjvF1ATiwn2Y",
        "846ah7iBSu9ApuCyEhA5xpnjHHX7d4QJKetWLbwzmJZ8",
        "EWGcY44T5cBH61LwX3oGRd9cPfg55auELFwKSAorbFQx"
      ],
      preTokenBalances: [
        {
          accountIndex: 1,
          mint: actualMint,
          owner: targetWallet,
          uiTokenAmount: { amount: "0", decimals: 6 }
        }
      ],
      postTokenBalances: [
        {
          accountIndex: 1,
          mint: actualMint,
          owner: targetWallet,
          uiTokenAmount: { amount: "16880200732", decimals: 6 }
        }
      ],
      instructions: [
        {
          programIdIndex: 3,
          accounts: [0, 0, 15, 11, 3, 16, 12, 4, 17, 15, 4, 7, 5, 1, 0, 15, 18, 6, 17, 16, 13, 8, 18, 19],
          dataBase64: flashxBuyDataBase64("990000", "14106483363")
        }
      ]
    },
    { receivedAtMs }
  );

  assert.equal(events.length, 1);
  assert.equal(events[0].eventType, "buy");
  assert.equal(events[0].decodeStatus, "decoded");
  assert.equal(events[0].routerProgramId, FLASHX_ROUTER_PROGRAM_ID);
  assert.equal(events[0].trader, targetWallet);
  assert.equal(events[0].mint, actualMint);
  assert.notEqual(events[0].mint, wrongTempWsolAccount);
  assert.equal(events[0].baseMint, actualMint);
  assert.equal(events[0].tokenAmountRaw, "16880200732");
  assert.equal(events[0].spendableSolLamports, "990000");
  assert.equal(events[0].minTokenAmountOut, "14106483363");
});

test("decodes FLASHX router Pump buys from Pump-looking accounts before fixed temp-account fallback", () => {
  const wrongTempWsolAccount = "83DqVhmHb3RmZa8ieYC7VtHB5upyC5GHAr6g4WYfMjg4";
  const actualMint = "5QmKEjUKCPpusjDycze5J875edpqtcHy8GUvdmB1pump";
  const targetWallet = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
  const events = normalizeShredstreamTransaction(
    {
      slot: 423222176,
      signature: "flashx-no-balance-meta-sig",
      accountKeys: [
        targetWallet,
        "8Bdtcgjc263mAwEALzPw3kwpzNZqRhgnugjhENu1vXZp",
        "BAzr8WMC3TDX7xi1bi1fb1kV9kugFUFf83T4Ypp5CSiy",
        FLASHX_ROUTER_PROGRAM_ID,
        wrongTempWsolAccount,
        "5dQZShePk85ztskSABT2nTo5iYgAykiPvu1PKsFnFhT9",
        "HXNZP8xNU6EqUNVzACYwQ5PJWJ95wzjK7kwnVpumhT93",
        "8fwrauNJMqDGqduDsDFTTShceFGx61Yn94wgPihhpYsd",
        "72ayp8X7L5fCTmEtXBBYMKuvFt1p2mj5vxcV2KfJNBpa",
        "H7LM5X8oVPR5uiTjHGBUXjtkutDvMvyFjrBxe87kSLB5",
        "8aHZJSt6frgjRTTfg4foDXjZHMyZ2ZQQjpwcWzzCvAGp",
        "GmTjpGuvGsCrBFUAdV3RnBYqFC3Vmcr2Z9XkDWtu46dd",
        "ComputeBudget111111111111111111111111111111",
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
        actualMint,
        systemProgram
      ],
      instructions: [
        {
          programIdIndex: 3,
          accounts: [0, 0, 15, 11, 3, 15, 12, 4, 7, 15, 4, 7, 5, 1, 0, 15, 14],
          dataBase64: flashxBuyDataBase64("990000", "14106483363")
        }
      ]
    },
    { receivedAtMs }
  );

  assert.equal(events.length, 1);
  assert.equal(events[0].mint, actualMint);
  assert.notEqual(events[0].mint, wrongTempWsolAccount);
  assert.equal(events[0].tokenAmountRaw, undefined);
});

test("guards FLASHX router sells from being decoded as copyable buys with balance deltas", () => {
  const actualMint = "FRjGWFQEw7qZQAhvsafzqoV7SvAepSL1Dpgx5giRpump";
  const targetWallet = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
  const events = normalizeShredstreamTransaction(
    {
      slot: 423240263,
      signature: "3ghnnzj5WhCfkN7KZsa2BZi5ZK9MweAMdW9wbFa1L3HYUr1NWbfB3kcWSNsZ1JJa2mSAZJnUDA8oTMihf84qQxEg",
      accountKeys: [
        targetWallet,
        FLASHX_ROUTER_PROGRAM_ID,
        "89Car9WTSLkbjEceFH2Fdr52iQ1MDXtrc5zgafjUAYEt",
        "DEqMQpG1oq7gqA8JeCRsbcJhMWwcU8tQvBLyUhsD7RCm",
        "AwcQLVAP7mZE9jGHB4fAReHvSfp8SnPC1e9Rh4r1X23q",
        "GjxanYEdq6FMhstLNQZ2k1kpTbiCc7Pnrzbtfg4eRwDD",
        "A88fHmAEcHqHeTbX55hce2ttGfEHACqGQBV2nw27S5zL",
        "8aHZJSt6frgjRTTfg4foDXjZHMyZ2ZQQjpwcWzzCvAGp",
        "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
        "ComputeBudget111111111111111111111111111111",
        systemProgram,
        "3j9vH5XjckQw1qjiUqpw1aKLgYY1W3r9BT5WPovSwP4M",
        actualMint
      ],
      preTokenBalances: [
        {
          accountIndex: 2,
          mint: actualMint,
          owner: targetWallet,
          uiTokenAmount: { amount: "34943728744", decimals: 6 }
        }
      ],
      postTokenBalances: [
        {
          accountIndex: 2,
          mint: actualMint,
          owner: targetWallet,
          uiTokenAmount: { amount: "0", decimals: 6 }
        }
      ],
      instructions: [
        {
          programIdIndex: 1,
          accounts: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
          dataBase64: flashxBuyDataBase64("34943728744", "579367")
        }
      ]
    },
    { receivedAtMs }
  );

  assert.deepEqual(events, []);
});

test("guards raw FLASHX sell-shaped payloads from being decoded as copyable buys", () => {
  const actualMint = "FRjGWFQEw7qZQAhvsafzqoV7SvAepSL1Dpgx5giRpump";
  const targetWallet = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
  const events = normalizeShredstreamTransaction(
    {
      slot: 423240263,
      signature: "flashx-raw-sell-shaped-sig",
      accountKeys: [
        targetWallet,
        FLASHX_ROUTER_PROGRAM_ID,
        "89Car9WTSLkbjEceFH2Fdr52iQ1MDXtrc5zgafjUAYEt",
        "DEqMQpG1oq7gqA8JeCRsbcJhMWwcU8tQvBLyUhsD7RCm",
        "AwcQLVAP7mZE9jGHB4fAReHvSfp8SnPC1e9Rh4r1X23q",
        "GjxanYEdq6FMhstLNQZ2k1kpTbiCc7Pnrzbtfg4eRwDD",
        "A88fHmAEcHqHeTbX55hce2ttGfEHACqGQBV2nw27S5zL",
        "8aHZJSt6frgjRTTfg4foDXjZHMyZ2ZQQjpwcWzzCvAGp",
        "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
        "ComputeBudget111111111111111111111111111111",
        systemProgram,
        "3j9vH5XjckQw1qjiUqpw1aKLgYY1W3r9BT5WPovSwP4M",
        actualMint
      ],
      instructions: [
        {
          programIdIndex: 1,
          accounts: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
          dataBase64: flashxBuyDataBase64("34943728744", "579367")
        }
      ]
    },
    { receivedAtMs }
  );

  assert.deepEqual(events, []);
});

test("does not convert ShredStream wallet trade rows for non-matching or undecoded events", () => {
  const [event] = normalizeShredstreamTransaction(
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

  const explorer = {
    pumpFunBaseUrl: "https://pump.fun/coin",
    solscanBaseUrl: "https://solscan.io"
  };

  assert.equal(
    rawPumpDiscoveryEventToWalletTrade({
      event,
      wallet: { address: "DifferentWallet111111111111111111111111111111", label: null },
      explorer
    }),
    null
  );
  assert.equal(
    rawPumpDiscoveryEventToWalletTrade({
      event: { ...event, decodeStatus: "unknown-discriminator" },
      wallet: { address: trader, label: null },
      explorer
    }),
    null
  );
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
