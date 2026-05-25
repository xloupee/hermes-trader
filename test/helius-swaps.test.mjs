import assert from "node:assert/strict";
import test from "node:test";
import { normalizeHeliusSwapData } from "../dist/helius-swaps.js";
import { formatCopyTradeSimulationMessage, isCopyableSolToTokenBuy } from "../dist/wallet-monitor.js";

const SOL_MINT = "So11111111111111111111111111111111111111112";
const wallet = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
const copyWallet = "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV";
const bonkMint = "DezXAZ8z7PnrnRJjz3n26VsZdrmRwNny4nBj9JkzjW7B";
const usdcMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const jupMint = "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN";
const config = {
  pumpFunBaseUrl: "https://pump.fun",
  solscanBaseUrl: "https://solscan.io"
};

function normalize(event) {
  return normalizeHeliusSwapData({
    event: {
      type: "SWAP",
      source: "JUPITER",
      feePayer: wallet,
      signature: "5VfUXexampleTxSignature111111111111111111111111111111111",
      timestamp: 1770000000,
      nativeTransfers: [],
      tokenTransfers: [],
      accountData: [],
      ...event
    },
    targetWallet: wallet,
    config
  });
}

function assertNotCopyable(trade) {
  assert.notEqual(trade.action, "buy");
  assert.equal(isCopyableSolToTokenBuy(trade), false);
  assert.equal(
    formatCopyTradeSimulationMessage(trade, {
      copyWalletAddress: copyWallet,
      copyAmountSol: 0.1
    }),
    null
  );
}

test("Helius swap parser accepts a simple native SOL to single token buy", () => {
  const trade = normalize({
    events: {
      swap: {
        nativeInput: {
          account: wallet,
          amount: "125000000"
        },
        nativeOutput: null,
        tokenInputs: [],
        tokenOutputs: [
          {
            userAccount: wallet,
            mint: bonkMint,
            rawTokenAmount: {
              tokenAmount: "250000000000",
              decimals: 6
            },
            symbol: "BONK"
          }
        ]
      }
    },
    nativeTransfers: [
      {
        fromUserAccount: wallet,
        toUserAccount: "Pool111111111111111111111111111111111111111",
        amount: 125000000
      }
    ],
    tokenTransfers: [
      {
        fromUserAccount: "Pool111111111111111111111111111111111111111",
        toUserAccount: wallet,
        mint: bonkMint,
        tokenAmount: 250000,
        symbol: "BONK"
      }
    ]
  });

  assert.equal(trade.action, "buy");
  assert.equal(trade.mint, bonkMint);
  assert.deepEqual(trade.input, {
    mint: SOL_MINT,
    symbol: "SOL",
    amount: 0.125
  });
  assert.deepEqual(trade.output, {
    mint: bonkMint,
    symbol: "BONK",
    amount: 250000
  });
  assert.equal(trade.raw.heliusSwapParser.reason, null);
  assert.equal(isCopyableSolToTokenBuy(trade), true);
});

test("Helius swap parser rejects token-to-token routes with incidental native SOL movement", () => {
  const trade = normalize({
    nativeTransfers: [
      {
        fromUserAccount: wallet,
        toUserAccount: "Fee111111111111111111111111111111111111111",
        amount: 5000
      }
    ],
    tokenTransfers: [
      {
        fromUserAccount: "Route1111111111111111111111111111111111111",
        toUserAccount: wallet,
        mint: jupMint,
        tokenAmount: 12.5,
        symbol: "JUP"
      }
    ],
    accountData: [
      {
        account: "TokenAccount1111111111111111111111111111111111",
        tokenBalanceChanges: [
          {
            userAccount: wallet,
            mint: usdcMint,
            symbol: "USDC",
            rawTokenAmount: {
              tokenAmount: "-1000000",
              decimals: 6
            }
          }
        ]
      }
    ]
  });

  assert.equal(trade.action, "swap");
  assert.equal(trade.mint, jupMint);
  assert.deepEqual(trade.input, {
    mint: usdcMint,
    symbol: "USDC",
    amount: 1
  });
  assert.deepEqual(trade.output, {
    mint: jupMint,
    symbol: "JUP",
    amount: 12.5
  });
  assert.match(trade.raw.heliusSwapParser.reason, /token for another token/);
  assertNotCopyable(trade);
});

test("Helius swap parser rejects native SOL spends with multiple output mints", () => {
  const trade = normalize({
    nativeTransfers: [
      {
        fromUserAccount: wallet,
        toUserAccount: "Pool111111111111111111111111111111111111111",
        amount: 200000000
      }
    ],
    tokenTransfers: [
      {
        fromUserAccount: "Pool111111111111111111111111111111111111111",
        toUserAccount: wallet,
        mint: bonkMint,
        tokenAmount: 100,
        symbol: "BONK"
      },
      {
        fromUserAccount: "Pool222222222222222222222222222222222222222",
        toUserAccount: wallet,
        mint: jupMint,
        tokenAmount: 5,
        symbol: "JUP"
      }
    ]
  });

  assert.equal(trade.action, "unknown");
  assert.equal(trade.mint, null);
  assert.deepEqual(trade.input, {
    mint: SOL_MINT,
    symbol: "SOL",
    amount: 0.2
  });
  assert.equal(trade.output, null);
  assert.match(trade.raw.heliusSwapParser.reason, /multiple output token mints/);
  assertNotCopyable(trade);
});

test("Helius swap parser rejects wrapped SOL token-transfer ambiguity", () => {
  const trade = normalize({
    nativeTransfers: [
      {
        fromUserAccount: wallet,
        toUserAccount: "Pool111111111111111111111111111111111111111",
        amount: 200000000
      }
    ],
    tokenTransfers: [
      {
        fromUserAccount: wallet,
        toUserAccount: "Pool111111111111111111111111111111111111111",
        mint: SOL_MINT,
        tokenAmount: 0.2,
        symbol: "WSOL"
      },
      {
        fromUserAccount: "Pool111111111111111111111111111111111111111",
        toUserAccount: wallet,
        mint: bonkMint,
        tokenAmount: 500,
        symbol: "BONK"
      }
    ]
  });

  assert.equal(trade.action, "unknown");
  assert.equal(trade.mint, null);
  assert.deepEqual(trade.input, {
    mint: SOL_MINT,
    symbol: "SOL",
    amount: 0.2
  });
  assert.deepEqual(trade.output, {
    mint: bonkMint,
    symbol: "BONK",
    amount: 500
  });
  assert.match(trade.raw.heliusSwapParser.reason, /wrapped SOL/);
  assertNotCopyable(trade);
});

test("Helius swap parser rejects fee-heavy missing-token-leg ambiguity", () => {
  const trade = normalize({
    nativeTransfers: [
      {
        fromUserAccount: wallet,
        toUserAccount: "Fee111111111111111111111111111111111111111",
        amount: 5000
      }
    ],
    tokenTransfers: [
      {
        fromUserAccount: "Pool111111111111111111111111111111111111111",
        toUserAccount: wallet,
        mint: bonkMint,
        tokenAmount: 500,
        symbol: "BONK"
      }
    ]
  });

  assert.equal(trade.action, "unknown");
  assert.equal(trade.mint, null);
  assert.match(trade.raw.heliusSwapParser.reason, /too small to prove/);
  assertNotCopyable(trade);
});

test("Helius swap parser rejects top-level native transfer plus token-in without structured swap proof", () => {
  for (const amount of [1_000_000, 2_039_280, 20_000_000]) {
    const trade = normalize({
      nativeTransfers: [
        {
          fromUserAccount: wallet,
          toUserAccount: "Fee111111111111111111111111111111111111111",
          amount
        }
      ],
      tokenTransfers: [
        {
          fromUserAccount: "Pool111111111111111111111111111111111111111",
          toUserAccount: wallet,
          mint: bonkMint,
          tokenAmount: 500,
          symbol: "BONK"
        }
      ]
    });

    assert.equal(trade.action, "unknown");
    assert.equal(trade.mint, null);
    assert.match(trade.raw.heliusSwapParser.reason, /lacks structured Helius swap proof/);
    assertNotCopyable(trade);
  }
});

test("Helius swap parser accepts high-confidence Pump.fun top-level SOL to token buys", () => {
  const pumpMint = "7sNaUnzXkXZXUah8Mv7xcxJW85ttFupNEafYHsuGpump";
  const trade = normalize({
    source: "PUMP_FUN",
    events: {},
    nativeTransfers: [
      {
        fromUserAccount: wallet,
        toUserAccount: "Pool111111111111111111111111111111111111111",
        amount: 977777777
      },
      {
        fromUserAccount: wallet,
        toUserAccount: "Tip1111111111111111111111111111111111111111",
        amount: 4644444
      }
    ],
    tokenTransfers: [
      {
        fromUserAccount: "Pool111111111111111111111111111111111111111",
        toUserAccount: wallet,
        mint: pumpMint,
        tokenAmount: 33393724.317703
      }
    ],
    accountData: [
      {
        account: wallet,
        nativeBalanceChange: -983235021
      }
    ]
  });

  assert.equal(trade.action, "buy");
  assert.equal(trade.mint, pumpMint);
  assert.deepEqual(trade.input, {
    mint: SOL_MINT,
    symbol: "SOL",
    amount: 0.982422221
  });
  assert.deepEqual(trade.output, {
    mint: pumpMint,
    symbol: null,
    amount: 33393724.317703
  });
  assert.equal(trade.raw.heliusSwapParser.reason, null);
  assert.equal(isCopyableSolToTokenBuy(trade), true);
});

test("Helius swap parser rejects Pump.fun top-level buys without fee payer and balance confirmation", () => {
  for (const partialEvent of [
    {
      source: "PUMP_FUN",
      feePayer: "Other1111111111111111111111111111111111111111",
      accountData: [
        {
          account: wallet,
          nativeBalanceChange: -200000000
        }
      ]
    },
    {
      source: "PUMP_FUN",
      feePayer: null,
      accountData: [
        {
          account: wallet,
          nativeBalanceChange: -200000000
        }
      ]
    },
    {
      source: "PUMP_FUN",
      accountData: []
    }
  ]) {
    const trade = normalize({
      ...partialEvent,
      nativeTransfers: [
        {
          fromUserAccount: wallet,
          toUserAccount: "Pool111111111111111111111111111111111111111",
          amount: 200000000
        }
      ],
      tokenTransfers: [
        {
          fromUserAccount: "Pool111111111111111111111111111111111111111",
          toUserAccount: wallet,
          mint: bonkMint,
          tokenAmount: 500,
          symbol: "BONK"
        }
      ]
    });

    assert.equal(trade.action, "unknown");
    assert.equal(trade.mint, null);
    assert.match(trade.raw.heliusSwapParser.reason, /lacks structured Helius swap proof/);
    assertNotCopyable(trade);
  }
});

test("Helius swap parser rejects native balance inflow ambiguity", () => {
  const trade = normalize({
    nativeTransfers: [
      {
        fromUserAccount: wallet,
        toUserAccount: "Pool111111111111111111111111111111111111111",
        amount: 200000000
      }
    ],
    tokenTransfers: [
      {
        fromUserAccount: "Pool111111111111111111111111111111111111111",
        toUserAccount: wallet,
        mint: bonkMint,
        tokenAmount: 500,
        symbol: "BONK"
      }
    ],
    accountData: [
      {
        account: wallet,
        nativeBalanceChange: 1000
      }
    ]
  });

  assert.equal(trade.action, "unknown");
  assert.equal(trade.mint, null);
  assert.match(trade.raw.heliusSwapParser.reason, /native SOL movement on both sides/);
  assertNotCopyable(trade);
});
