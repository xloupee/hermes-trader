import assert from "node:assert/strict";
import test from "node:test";
import bs58 from "bs58";
import {
  buildGeyserWalletSubscribeRequest,
  geyserUpdateMentionsWallet,
  normalizeGeyserWalletTrade
} from "../dist/geyser.js";

const SOL_MINT = "So11111111111111111111111111111111111111112";
const PUMP_PROGRAM = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMPSWAP_PROGRAM = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const wallet = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
const otherWallet = "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV";
const pool = "8opHzTAnfzRpPEx21XtnrVTX28YQuCpAjcn1PczScKh";
const bonkMint = "DezXAZ8z7PnrnRJjz3n26VsZdrmRwNny4nBj9JkzjW7B";
const jupMint = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN";
const signature = "1111111111111111111111111111111111111111111111111111111111111111";
const config = {
  pumpFunBaseUrl: "https://pump.fun",
  solscanBaseUrl: "https://solscan.io"
};

function key(value) {
  return bs58.decode(value);
}

function tokenBalance({ accountIndex, mint, owner, amount, decimals = 6 }) {
  return {
    accountIndex,
    mint,
    owner,
    programId: "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
    uiTokenAmount: {
      amount: String(amount),
      decimals,
      uiAmount: Number(amount) / 10 ** decimals,
      uiAmountString: String(Number(amount) / 10 ** decimals)
    }
  };
}

function update({
  accountKeys = [wallet, pool, PUMP_PROGRAM],
  preBalances = ["2000000000", "0", "0"],
  postBalances = ["1874995000", "0", "0"],
  preTokenBalances = [],
  postTokenBalances = [],
  isVote = false,
  err,
  loadedWritableAddresses = [],
  loadedReadonlyAddresses = []
} = {}) {
  return {
    filters: ["walletTrades"],
    createdAt: new Date("2026-05-29T12:00:00.000Z"),
    transaction: {
      slot: "423023220",
      transaction: {
        signature: key(signature),
        isVote,
        index: "7",
        transaction: {
          signatures: [key(signature)],
          message: {
            header: {
              numRequiredSignatures: 1,
              numReadonlySignedAccounts: 0,
              numReadonlyUnsignedAccounts: 1
            },
            accountKeys: accountKeys.map(key),
            recentBlockhash: key("11111111111111111111111111111111"),
            instructions: [],
            versioned: true,
            addressTableLookups: []
          }
        },
        meta: {
          err,
          fee: "5000",
          preBalances,
          postBalances,
          innerInstructions: [],
          innerInstructionsNone: false,
          logMessages: [],
          logMessagesNone: false,
          preTokenBalances,
          postTokenBalances,
          rewards: [],
          loadedWritableAddresses: loadedWritableAddresses.map(key),
          loadedReadonlyAddresses: loadedReadonlyAddresses.map(key),
          returnData: undefined,
          returnDataNone: true
        }
      }
    }
  };
}

function normalize(nextUpdate, targetWallet = wallet) {
  return normalizeGeyserWalletTrade({
    update: nextUpdate,
    targetWallet,
    config
  });
}

test("Geyser subscription request is empty without wallets and dedupes wallet filters", () => {
  assert.deepEqual(buildGeyserWalletSubscribeRequest([]).transactions, {});

  const request = buildGeyserWalletSubscribeRequest([
    { address: wallet },
    { address: otherWallet },
    { address: wallet }
  ]);

  assert.deepEqual(request.transactions.walletTrades.accountInclude, [wallet, otherWallet].sort());
  assert.equal(request.transactions.walletTrades.vote, false);
  assert.equal(request.transactions.walletTrades.failed, false);
});

test("Geyser parser accepts a Pump bonding-curve SOL to token buy", () => {
  const result = normalize(update({
    accountKeys: [wallet, pool, PUMP_PROGRAM],
    preBalances: ["2000000000", "0", "0"],
    postBalances: ["1874995000", "0", "0"],
    preTokenBalances: [],
    postTokenBalances: [
      tokenBalance({
        accountIndex: 3,
        mint: bonkMint,
        owner: wallet,
        amount: "250000000000"
      })
    ]
  }));

  assert.equal(result.ok, true);
  assert.equal(result.trade.provider, "geyser");
  assert.equal(result.trade.action, "buy");
  assert.equal(result.trade.mint, bonkMint);
  assert.equal(result.trade.source, "GEYSER_PUMP_BONDING_CURVE");
  assert.equal(result.trade.pool, "pump");
  assert.equal(result.trade.solAmount, 0.125);
  assert.equal(result.trade.tokenAmount, 250000);
  assert.deepEqual(result.trade.input, {
    mint: SOL_MINT,
    symbol: "SOL",
    amount: 0.125
  });
  assert.deepEqual(result.trade.output, {
    mint: bonkMint,
    symbol: null,
    amount: 250000
  });
  assert.equal(result.trade.timestamp, 1780056000);
  assert.equal(result.trade.raw.geyserParser.accepted, true);
});

test("Geyser parser accepts a PumpSwap token to SOL sell", () => {
  const result = normalize(update({
    accountKeys: [wallet, pool, PUMPSWAP_PROGRAM],
    preBalances: ["1000000000", "0", "0"],
    postBalances: ["1199995000", "0", "0"],
    preTokenBalances: [
      tokenBalance({
        accountIndex: 3,
        mint: bonkMint,
        owner: wallet,
        amount: "300000000000"
      })
    ],
    postTokenBalances: [
      tokenBalance({
        accountIndex: 3,
        mint: bonkMint,
        owner: wallet,
        amount: "50000000000"
      })
    ]
  }));

  assert.equal(result.ok, true);
  assert.equal(result.trade.action, "sell");
  assert.equal(result.trade.mint, bonkMint);
  assert.equal(result.trade.source, "GEYSER_PUMPSWAP");
  assert.equal(result.trade.pool, "pump-amm");
  assert.equal(result.trade.solAmount, 0.2);
  assert.equal(result.trade.tokenAmount, 250000);
  assert.deepEqual(result.trade.input, {
    mint: bonkMint,
    symbol: null,
    amount: 250000
  });
  assert.deepEqual(result.trade.output, {
    mint: SOL_MINT,
    symbol: "SOL",
    amount: 0.2
  });
});

test("Geyser parser rejects failed, vote, unrelated, and ambiguous transactions", () => {
  assert.match(normalize(update({ err: { err: new Uint8Array([1]) } })).reason, /failed transaction/);
  assert.match(normalize(update({ isVote: true })).reason, /vote transaction/);
  assert.match(normalize(update({ accountKeys: [wallet, pool, otherWallet] })).reason, /does not include Pump/);

  const ambiguous = normalize(update({
    postTokenBalances: [
      tokenBalance({ accountIndex: 3, mint: bonkMint, owner: wallet, amount: "100000000" }),
      tokenBalance({ accountIndex: 4, mint: jupMint, owner: wallet, amount: "200000000" })
    ]
  }));
  assert.equal(ambiguous.ok, false);
  assert.match(ambiguous.reason, /ambiguous/);
});

test("Geyser parser checks loaded address table keys for wallet mentions", () => {
  const nextUpdate = update({
    accountKeys: [otherWallet, pool, PUMP_PROGRAM],
    loadedWritableAddresses: [wallet]
  });

  assert.equal(geyserUpdateMentionsWallet(nextUpdate, wallet), true);
});
