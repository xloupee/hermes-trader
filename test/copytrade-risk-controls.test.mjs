import assert from "node:assert/strict";
import test from "node:test";
import {
  copyTradeBuyRiskBlockedReason,
  copyTradeDailyBudgetKey,
  copyTradeRequestRiskBlockedReason,
  copyTradeWalletReserveBlockedReason,
  createInMemoryCopyTradeDailySolBudget,
  formatCopyTradeRiskControlLog
} from "../dist/copytrade-risk-controls.js";

const nowMs = Date.parse("2026-05-24T12:00:00.000Z");

const baseConfig = {
  copyTradeMaxBuySol: 0.005,
  copyTradeDailySolCap: 0.02,
  copyTradeMaxSignalAgeMs: 60_000,
  copyTradeMaxSlippage: 15,
  copyTradeMaxPriorityFee: 0.0002,
  copyTradeMinWalletReserveSol: 0,
  copyTradeMaxCopyWalletsPerChat: 1,
  copyTradeAllowedSources: []
};

function buyRequest(overrides = {}) {
  return {
    action: "buy",
    mint: "Mint111111111111111111111111111111111111111",
    amount: 0.003,
    denominatedInSol: "true",
    slippage: 10,
    priorityFee: 0.00005,
    pool: "auto",
    ...overrides
  };
}

function sellRequest(overrides = {}) {
  return {
    action: "sell",
    mint: "Mint111111111111111111111111111111111111111",
    amount: "20%",
    denominatedInSol: "false",
    slippage: 10,
    priorityFee: 0.00005,
    pool: "auto",
    ...overrides
  };
}

function trade(overrides = {}) {
  return {
    observedAt: new Date(nowMs).toISOString(),
    provider: "helius",
    targetWallet: "TargetWallet111111111111111111111111111111",
    label: null,
    action: "buy",
    mint: "Mint111111111111111111111111111111111111111",
    signature: "TargetBuySignature111111111111111111111111111",
    timestamp: nowMs / 1000,
    feePayer: "TargetWallet111111111111111111111111111111",
    source: "JUPITER",
    input: {
      mint: "So11111111111111111111111111111111111111112",
      symbol: "SOL",
      amount: 0.25
    },
    output: {
      mint: "Mint111111111111111111111111111111111111111",
      symbol: "PUMP",
      amount: 1000
    },
    solAmount: 0.25,
    tokenAmount: 1000,
    pool: "JUPITER",
    marketCapSol: null,
    pumpFunUrl: null,
    solscanTokenUrl: null,
    solscanTxUrl: null,
    raw: {},
    ...overrides
  };
}

function buyRiskReason(overrides = {}) {
  return copyTradeBuyRiskBlockedReason({
    config: {
      ...baseConfig,
      ...(overrides.config || {})
    },
    request: overrides.request || buyRequest(),
    trade: overrides.trade || trade(),
    copyTradeWalletCount: overrides.copyTradeWalletCount ?? 1,
    dailySpentSol: overrides.dailySpentSol ?? 0,
    nowMs: overrides.nowMs ?? nowMs
  });
}

test("copy trade buy risk controls allow a small fresh signal inside all caps", () => {
  assert.equal(buyRiskReason(), null);
});

test("copy trade buy risk controls block amount above max buy SOL", () => {
  assert.match(
    buyRiskReason({ request: buyRequest({ amount: 0.006 }) }),
    /COPY_TRADE_MAX_BUY_SOL=0\.005 SOL/
  );
});

test("copy trade buy risk controls block daily SOL cap breaches", () => {
  assert.match(
    buyRiskReason({ dailySpentSol: 0.018, request: buyRequest({ amount: 0.003 }) }),
    /COPY_TRADE_DAILY_SOL_CAP=0\.02 SOL/
  );
});

test("copy trade buy risk controls block stale signals and missing timestamps", () => {
  assert.match(
    buyRiskReason({ trade: trade({ timestamp: (nowMs - 61_000) / 1000 }) }),
    /observed trade signal is 61s old/
  );

  assert.match(
    buyRiskReason({ trade: trade({ timestamp: null }) }),
    /timestamp is missing/
  );
});

test("copy trade request risk controls block risky slippage and priority fees", () => {
  assert.match(
    buyRiskReason({ request: buyRequest({ slippage: 20 }) }),
    /COPY_TRADE_MAX_SLIPPAGE=15%/
  );

  assert.match(
    buyRiskReason({ request: buyRequest({ priorityFee: 0.0003 }) }),
    /COPY_TRADE_MAX_PRIORITY_FEE=0\.0002 SOL/
  );

  assert.match(
    copyTradeRequestRiskBlockedReason({
      config: baseConfig,
      request: sellRequest({ priorityFee: 0.0003 })
    }),
    /sell priority fee 0\.0003 SOL/
  );
});

test("copy trade buy risk controls block chats copying too many wallets", () => {
  assert.match(
    buyRiskReason({ copyTradeWalletCount: 2 }),
    /COPY_TRADE_MAX_COPY_WALLETS_PER_CHAT=1/
  );
});

test("copy trade buy risk controls can enforce a Helius source allowlist", () => {
  assert.equal(
    buyRiskReason({
      config: {
        copyTradeAllowedSources: ["JUPITER"]
      }
    }),
    null
  );

  assert.match(
    buyRiskReason({
      config: {
        copyTradeAllowedSources: ["PUMP_FUN"]
      }
    }),
    /COPY_TRADE_ALLOWED_SOURCES=PUMP_FUN/
  );
});

test("copy trade wallet reserve control blocks insufficient or unverifiable balances", () => {
  const config = {
    ...baseConfig,
    copyTradeMinWalletReserveSol: 0.01
  };

  assert.equal(
    copyTradeWalletReserveBlockedReason({
      config,
      request: buyRequest({ amount: 0.003 }),
      tradingWalletBalanceSol: 0.02
    }),
    null
  );

  assert.match(
    copyTradeWalletReserveBlockedReason({
      config,
      request: buyRequest({ amount: 0.003 }),
      tradingWalletBalanceSol: 0.012
    }),
    /cannot cover 0\.003 SOL buy plus COPY_TRADE_MIN_WALLET_RESERVE_SOL=0\.01 SOL/
  );

  assert.match(
    copyTradeWalletReserveBlockedReason({
      config,
      request: buyRequest({ amount: 0.003 }),
      tradingWalletBalanceSol: null
    }),
    /could not verify trading wallet balance/
  );
});

test("copy trade daily SOL budget reserves per UTC day and wallet key", () => {
  const budget = createInMemoryCopyTradeDailySolBudget();
  const key = copyTradeDailyBudgetKey({
    chatId: "chat-1",
    tradingWalletPublicKey: "TradingWallet111111111111111111111111111111"
  });

  assert.equal(budget.reserve({ key, amountSol: 0.012, capSol: 0.02, nowMs }).ok, true);
  assert.equal(budget.spentSol({ key, nowMs }), 0.012);

  const blocked = budget.reserve({ key, amountSol: 0.009, capSol: 0.02, nowMs });
  assert.equal(blocked.ok, false);
  assert.match(blocked.reason, /COPY_TRADE_DAILY_SOL_CAP=0\.02 SOL/);
  assert.equal(budget.spentSol({ key, nowMs }), 0.012);

  const tomorrowMs = Date.parse("2026-05-25T00:00:01.000Z");
  assert.equal(budget.spentSol({ key, nowMs: tomorrowMs }), 0);
});

test("copy trade risk control log includes the wallet reserve", () => {
  assert.match(
    formatCopyTradeRiskControlLog({
      ...baseConfig,
      copyTradeMinWalletReserveSol: 0.01
    }),
    /minWalletReserveSol=0\.01/
  );
});
