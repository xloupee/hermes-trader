import assert from "node:assert/strict";
import test from "node:test";
import { PUMP_BONDING_CURVE_PROGRAM_ID } from "../dist/shredstream-decoder.js";
import { createShredstreamWalletObserver } from "../dist/shredstream-wallet-observer.js";

const receivedAtMs = Date.parse("2026-05-30T16:00:00.000Z");
const signature = "ObservedSignature11111111111111111111111111";
const mint = "Mint111111111111111111111111111111111111111";
const trader = "Trader111111111111111111111111111111111111";
const bondingCurve = "BondingCurve111111111111111111111111111111";

function dataBase64(discriminatorHex, u64Args = []) {
  const buffer = Buffer.alloc(8 + u64Args.length * 8);
  Buffer.from(discriminatorHex, "hex").copy(buffer, 0);

  for (const [index, value] of u64Args.entries()) {
    buffer.writeBigUInt64LE(BigInt(value), 8 + index * 8);
  }

  return buffer.toString("base64");
}

test("ShredStream wallet observer emits matching trades and processing stats", async () => {
  const statuses = [];
  const trades = [];
  let resolveStats;
  const statsSeen = new Promise((resolve) => {
    resolveStats = resolve;
  });
  const source = {
    describe: () => "fixture",
    async *readRecords() {
      yield { parseError: "bad json" };
      yield {
        transaction: {
          slot: 1,
          signature: "unmatched",
          receivedAtMs,
          accountKeys: [
            PUMP_BONDING_CURVE_PROGRAM_ID,
            "Global1111111111111111111111111111111111111",
            "FeeRecipient1111111111111111111111111111111",
            mint,
            bondingCurve,
            "AssociatedBondingCurve111111111111111111111",
            "AssociatedUser11111111111111111111111111111",
            "SomeoneElse111111111111111111111111111111"
          ],
          instructions: [
            {
              programIdIndex: 0,
              accounts: [1, 2, 3, 4, 5, 6, 7],
              dataBase64: dataBase64("66063d1201daebea", ["123456789", "250000000"])
            }
          ]
        }
      };
      yield {
        transaction: {
          slot: 2,
          signature,
          receivedAtMs,
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
            },
            {
              programIdIndex: 0,
              accounts: [1, 2, 3, 4, 5, 6, 7],
              dataBase64: dataBase64("66063d1201daebea", ["123456789", "250000000"])
            }
          ]
        }
      };
    }
  };
  const observer = createShredstreamWalletObserver({
    enabled: true,
    source,
    wallets: [{ address: trader, label: "watched", addedAt: new Date(receivedAtMs).toISOString(), updatedAt: new Date(receivedAtMs).toISOString() }],
    explorer: {
      pumpFunBaseUrl: "https://pump.fun",
      solscanBaseUrl: "https://solscan.io"
    },
    statsIntervalMs: 60_000,
    isDiagnosticWallet: (wallet) => wallet.label === "diagnostic",
    onTrade: (trade) => {
      trades.push(trade);
    },
    onStatus: (message) => {
      statuses.push(message);
      if (message.startsWith("ShredStream wallet observer stats: ")) {
        resolveStats(message);
      }
    }
  });

  observer.start();
  const statsMessage = await statsSeen;
  observer.stop();

  assert.equal(trades.length, 1);
  assert.equal(trades[0].provider, "shredstream");
  assert.equal(trades[0].targetWallet, trader);
  assert.equal(trades[0].signature, signature);

  const stats = JSON.parse(statsMessage.replace("ShredStream wallet observer stats: ", ""));
  assert.equal(stats.recordsRead, 3);
  assert.equal(stats.parseErrors, 1);
  assert.equal(stats.pumpEvents, 3);
  assert.equal(stats.decodedWalletCandidates, 3);
  assert.equal(stats.watchedWalletMatches, 2);
  assert.equal(stats.diagnosticWalletMatches, 0);
  assert.equal(stats.realWalletMatches, 2);
  assert.equal(stats.ambiguousWalletCandidates, 0);
  assert.equal(stats.duplicateWalletCandidates, 1);
  assert.equal(stats.tradesEmitted, 1);
  assert.equal(stats.diagnosticTradesEmitted, 0);
  assert.equal(stats.realTradesEmitted, 1);
  assert.equal(statuses.some((message) => message.includes("ShredStream wallet observer started")), true);
});

test("ShredStream wallet observer suppresses same-signature buy and sell pairs", async () => {
  const statuses = [];
  const trades = [];
  let resolveStats;
  const statsSeen = new Promise((resolve) => {
    resolveStats = resolve;
  });
  const source = {
    describe: () => "fixture",
    async *readRecords() {
      yield {
        transaction: {
          slot: 3,
          signature: "ambiguous-signature",
          receivedAtMs,
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
            },
            {
              programIdIndex: 0,
              accounts: [1, 2, 3, 4, 5, 6, 7],
              dataBase64: dataBase64("33e685a4017f83ad", ["123456789", "250000000"])
            }
          ]
        }
      };
    }
  };
  const observer = createShredstreamWalletObserver({
    enabled: true,
    source,
    wallets: [{ address: trader, label: "watched", addedAt: new Date(receivedAtMs).toISOString(), updatedAt: new Date(receivedAtMs).toISOString() }],
    explorer: {
      pumpFunBaseUrl: "https://pump.fun",
      solscanBaseUrl: "https://solscan.io"
    },
    statsIntervalMs: 60_000,
    onTrade: (trade) => {
      trades.push(trade);
    },
    onStatus: (message) => {
      statuses.push(message);
      if (message.startsWith("ShredStream wallet observer stats: ")) {
        resolveStats(message);
      }
    }
  });

  observer.start();
  const statsMessage = await statsSeen;
  observer.stop();

  assert.equal(trades.length, 0);
  const stats = JSON.parse(statsMessage.replace("ShredStream wallet observer stats: ", ""));
  assert.equal(stats.decodedWalletCandidates, 2);
  assert.equal(stats.watchedWalletMatches, 2);
  assert.equal(stats.realWalletMatches, 2);
  assert.equal(stats.ambiguousWalletCandidates, 2);
  assert.equal(stats.tradesEmitted, 0);
});

test("ShredStream wallet observer stats split diagnostic and real wallets", async () => {
  const statuses = [];
  let resolveStats;
  const statsSeen = new Promise((resolve) => {
    resolveStats = resolve;
  });
  const source = {
    describe: () => "fixture",
    async *readRecords() {
      yield {
        transaction: {
          slot: 4,
          signature,
          receivedAtMs,
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
        }
      };
    }
  };
  const observer = createShredstreamWalletObserver({
    enabled: true,
    source,
    wallets: [{ address: trader, label: "diagnostic", addedAt: new Date(receivedAtMs).toISOString(), updatedAt: new Date(receivedAtMs).toISOString() }],
    explorer: {
      pumpFunBaseUrl: "https://pump.fun",
      solscanBaseUrl: "https://solscan.io"
    },
    statsIntervalMs: 60_000,
    isDiagnosticWallet: (wallet) => wallet.label === "diagnostic",
    onTrade: () => {},
    onStatus: (message) => {
      statuses.push(message);
      if (message.startsWith("ShredStream wallet observer stats: ")) {
        resolveStats(message);
      }
    }
  });

  observer.start();
  const statsMessage = await statsSeen;
  observer.stop();

  assert.equal(statuses.some((message) => message.includes("realWallets=0 diagnosticWallets=1")), true);
  const stats = JSON.parse(statsMessage.replace("ShredStream wallet observer stats: ", ""));
  assert.equal(stats.wallets, 1);
  assert.equal(stats.realWallets, 0);
  assert.equal(stats.diagnosticWallets, 1);
  assert.equal(stats.watchedWalletMatches, 1);
  assert.equal(stats.diagnosticWalletMatches, 1);
  assert.equal(stats.realWalletMatches, 0);
  assert.equal(stats.tradesEmitted, 1);
  assert.equal(stats.diagnosticTradesEmitted, 1);
  assert.equal(stats.realTradesEmitted, 0);
});
