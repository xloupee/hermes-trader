import assert from "node:assert/strict";
import test from "node:test";
import bs58 from "bs58";
import {
  buildYellowstoneSubscribeRequest,
  normalizeYellowstoneTrade
} from "../dist/yellowstone.js";

const wallet = "Gved9Awp3ntzPNiEVMmR5HgKMGAfjPAiZdhMwrjcTHUG";
const otherWallet = "A8myhNPHpPsq7e4gkPntbiQCgK7GL4M4smkyFzbHtvdS";
const mint = "CmGsBfi3Zbsygpt1FcvFcQa3mD2drYGX6w5WdsWZpump";
const pumpProgram = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const pumpAmmProgram = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const solMint = "So11111111111111111111111111111111111111112";
const buyDiscriminator = [102, 6, 61, 18, 1, 218, 235, 234];
const sellDiscriminator = [51, 230, 133, 164, 1, 127, 131, 173];
const signature = "oZpvKND2Q3ibmPaZa5Bja3YoCfVukdgByxVLPf9KuinQMA8rdJzh2rhLj3SA9QPisPWxW4bpy2oDVL3ELrYX45s";
const config = {
  pumpFunBaseUrl: "https://pump.fun",
  solscanBaseUrl: "https://solscan.io"
};

function key(address) {
  return bs58.decode(address);
}

function update({
  accountKeys,
  programIdIndex,
  preBalances,
  postBalances,
  preTokenBalances = [],
  postTokenBalances = [],
  instructionAccounts = [0, 1, 2],
  instructionData = buyDiscriminator,
  innerInstructions = [],
  isVote = false,
  err = undefined
}) {
  return {
    filters: ["watchedWallets"],
    createdAt: new Date("2026-05-29T23:29:16.000Z"),
    transaction: {
      slot: "423038340",
      transaction: {
        signature: bs58.decode(signature),
        isVote,
        index: "1",
        transaction: {
          message: {
            accountKeys: accountKeys.map(key),
            instructions: [
              {
                programIdIndex,
                accounts: Uint8Array.from(instructionAccounts),
                data: Uint8Array.from([...instructionData, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1])
              }
            ]
          }
        },
        meta: {
          err,
          preBalances: preBalances.map(String),
          postBalances: postBalances.map(String),
          preTokenBalances,
          postTokenBalances,
          loadedWritableAddresses: [],
          loadedReadonlyAddresses: [],
          innerInstructions,
          logMessages: []
        }
      }
    }
  };
}

function pumpAmmUpdate(overrides = {}) {
  return update({
    accountKeys: [
      otherWallet,
      wallet,
      otherWallet,
      mint,
      solMint,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      pumpAmmProgram
    ],
    programIdIndex: 16,
    instructionAccounts: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    ...overrides
  });
}

function pumpUpdate(overrides = {}) {
  return update({
    accountKeys: [wallet, otherWallet, mint, otherWallet, otherWallet, otherWallet, wallet, pumpProgram],
    programIdIndex: 7,
    instructionAccounts: [0, 1, 2, 3, 4, 5, 6],
    ...overrides
  });
}

function tokenBalance({ owner, amount, decimals = 6 }) {
  return {
    accountIndex: 3,
    mint,
    owner,
    programId: "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
    uiTokenAmount: {
      amount: String(amount),
      decimals,
      uiAmount: Number(amount) / 10 ** decimals,
      uiAmountString: String(Number(amount) / 10 ** decimals)
    }
  };
}

test("Yellowstone subscription request keeps wallet filters sorted and shared", () => {
  const empty = buildYellowstoneSubscribeRequest([], "processed");
  assert.deepEqual(empty.transactions.pumpWallets.accountInclude, []);
  assert.deepEqual(empty.transactions.pumpAmmWallets.accountInclude, []);
  assert.deepEqual(empty.transactions.pumpWallets.accountRequired, [pumpProgram]);
  assert.deepEqual(empty.transactions.pumpAmmWallets.accountRequired, [pumpAmmProgram]);

  const request = buildYellowstoneSubscribeRequest([otherWallet, wallet, wallet], "confirmed");
  assert.deepEqual(request.transactions.pumpWallets.accountInclude, [otherWallet, wallet].sort());
  assert.deepEqual(request.transactions.pumpAmmWallets.accountInclude, [otherWallet, wallet].sort());
  assert.equal(request.transactions.pumpWallets.vote, false);
  assert.equal(request.transactions.pumpAmmWallets.failed, false);
});

test("Yellowstone parser normalizes PumpSwap buy from balance deltas", () => {
  const trade = normalizeYellowstoneTrade(pumpAmmUpdate({
    preBalances: [0, 200000000, 0],
    postBalances: [0, 198900000, 0],
    preTokenBalances: [],
    postTokenBalances: [tokenBalance({ owner: wallet, amount: 2175779266 })]
  }), wallet, config);

  assert.equal(trade.provider, "yellowstone");
  assert.equal(trade.action, "buy");
  assert.equal(trade.mint, mint);
  assert.equal(trade.source, "GEYSER_PUMPSWAP");
  assert.equal(trade.pool, "pump-amm");
  assert.equal(trade.solAmount, 0.0011);
  assert.equal(trade.tokenAmount, 2175.779266);
  assert.equal(trade.raw.slot, 423038340);
});

test("Yellowstone parser normalizes PumpSwap sell from balance deltas", () => {
  const trade = normalizeYellowstoneTrade(pumpAmmUpdate({
    preBalances: [0, 198900000, 0],
    postBalances: [0, 199300000, 0],
    instructionData: sellDiscriminator,
    preTokenBalances: [tokenBalance({ owner: wallet, amount: 2175779266 })],
    postTokenBalances: [tokenBalance({ owner: wallet, amount: 1087889633 })]
  }), wallet, config);

  assert.equal(trade.action, "sell");
  assert.equal(trade.source, "GEYSER_PUMPSWAP");
  assert.deepEqual(trade.input, { mint, symbol: null, amount: 1087.889633 });
  assert.deepEqual(trade.output, {
    mint: "So11111111111111111111111111111111111111112",
    symbol: "SOL",
    amount: 0.0004
  });
});

test("Yellowstone parser normalizes Pump bonding-curve buys", () => {
  const trade = normalizeYellowstoneTrade(pumpUpdate({
    preBalances: [200000000, 0, 0],
    postBalances: [198900000, 0, 0],
    preTokenBalances: [],
    postTokenBalances: [tokenBalance({ owner: wallet, amount: 500000000 })]
  }), wallet, config);

  assert.equal(trade.action, "buy");
  assert.equal(trade.source, "GEYSER_PUMP_BONDING_CURVE");
  assert.equal(trade.pool, "pump");
});

test("Yellowstone parser rejects failed, vote, unrelated, and ambiguous transactions", () => {
  assert.equal(normalizeYellowstoneTrade(pumpAmmUpdate({
    preBalances: [1, 0, 0],
    postBalances: [1, 0, 0],
    isVote: true
  }), wallet, config), null);

  assert.equal(normalizeYellowstoneTrade(pumpAmmUpdate({
    preBalances: [1, 0, 0],
    postBalances: [1, 0, 0],
    err: { err: Uint8Array.from([1]) }
  }), wallet, config), null);

  assert.equal(normalizeYellowstoneTrade(update({
    accountKeys: [wallet, otherWallet, otherWallet],
    programIdIndex: 2,
    preBalances: [1, 0, 0],
    postBalances: [1, 0, 0]
  }), wallet, config), null);

  assert.equal(normalizeYellowstoneTrade(pumpAmmUpdate({
    preBalances: [1, 0, 0],
    postBalances: [1, 0, 0],
    preTokenBalances: [],
    postTokenBalances: [
      tokenBalance({ owner: wallet, amount: 1 }),
      { ...tokenBalance({ owner: wallet, amount: 1 }), mint: otherWallet }
    ]
  }), wallet, config), null);
});

test("Yellowstone parser rejects PumpSwap non-SOL quote mints", () => {
  assert.equal(normalizeYellowstoneTrade(pumpAmmUpdate({
    accountKeys: [
      otherWallet,
      wallet,
      otherWallet,
      mint,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      otherWallet,
      pumpAmmProgram
    ],
    preBalances: [1, 0, 0],
    postBalances: [1, 0, 0],
    postTokenBalances: [tokenBalance({ owner: wallet, amount: 1 })]
  }), wallet, config), null);
});
