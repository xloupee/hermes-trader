import assert from "node:assert/strict";
import test from "node:test";
import {
  buildCashbackAccrual,
  buildCashbackExecutionKey,
  cashbackSummaryReplyMarkup,
  calculateCashbackLamports,
  claimCashback,
  formatCashbackSol,
  formatCashbackSummaryText,
  parseCashbackConfig
} from "../dist/cashback.js";

const payoutWallet = "11111111111111111111111111111111";

function config(overrides = {}) {
  return {
    enabled: true,
    feeShareBps: 2000,
    minClaimLamports: 5_000_000n,
    maxPayoutLamportsPerDay: 0n,
    payoutWalletPublicKey: null,
    payoutWalletSecretKey: "secret",
    ...overrides
  };
}

function platformFee(overrides = {}) {
  return {
    enabled: true,
    bps: 100,
    treasury: "Treasury111111111111111111111111111111111",
    budgetLamports: 1_000_000n,
    feeLamports: 10_000n,
    tradeLamports: 990_000n,
    ...overrides
  };
}

class MemoryCashbackStore {
  constructor(entries = []) {
    this.entries = entries.map((entry, index) => ({ id: index + 1, ...entry }));
    this.payouts = [];
  }

  async accrue(entry) {
    if (this.entries.some((existing) => existing.executionKey === entry.executionKey)) {
      return;
    }
    this.entries.push({ id: this.entries.length + 1, ...entry });
  }

  async getSummary({ chatId, tradingWalletPublicKey, payoutWalletPublicKey = null, config: cashbackConfig }) {
    const entries = this.entries.filter((entry) =>
      entry.chatId === String(chatId) &&
      (!tradingWalletPublicKey || entry.tradingWalletPublicKey === tradingWalletPublicKey)
    );
    const payouts = this.payouts.filter((payout) =>
      payout.chatId === String(chatId) &&
      (!tradingWalletPublicKey || payout.tradingWalletPublicKey === tradingWalletPublicKey)
    );

    return {
      enabled: cashbackConfig.enabled,
      tradingWalletPublicKey,
      payoutWalletPublicKey,
      accruedLamports: entries.filter((entry) => entry.status !== "voided").reduce((sum, entry) => sum + entry.cashbackLamports, 0n),
      claimableLamports: entries.filter((entry) => entry.status === "claimable").reduce((sum, entry) => sum + entry.cashbackLamports, 0n),
      pendingLamports: entries.filter((entry) => entry.status === "pending").reduce((sum, entry) => sum + entry.cashbackLamports, 0n),
      lifetimePaidLamports: payouts.filter((payout) => payout.status === "submitted" || payout.status === "confirmed").reduce((sum, payout) => sum + payout.amountLamports, 0n),
      minClaimLamports: cashbackConfig.minClaimLamports,
      payoutUnavailableReason: (!payoutWalletPublicKey ? "add a payout wallet" : null) ||
        (cashbackConfig.payoutWalletSecretKey ? null : "payout sender is not configured")
    };
  }

  async listClaimableEntries({ chatId, tradingWalletPublicKey }) {
    return this.entries.filter((entry) =>
      entry.chatId === String(chatId) &&
      entry.tradingWalletPublicKey === tradingWalletPublicKey &&
      entry.status === "claimable"
    );
  }

  async createPayout(payout) {
    const record = { id: this.payouts.length + 1, ...payout };
    this.payouts.push(record);
    return record;
  }

  async updatePayout({ id, status, signature = null, errorText = null }) {
    const payout = this.payouts.find((entry) => entry.id === id);
    if (payout) {
      payout.status = status;
      payout.signature = signature;
      payout.errorText = errorText;
    }
  }

  async markLedgerPaid({ ids }) {
    for (const entry of this.entries) {
      if (ids.includes(entry.id)) {
        entry.status = "paid";
      }
    }
  }

  async getReconciliationReport() {
    throw new Error("not used");
  }
}

test("cashback fee share is computed from collected platform fee only", () => {
  assert.equal(calculateCashbackLamports(10_000n, 2000), 2_000n);
  assert.equal(calculateCashbackLamports(99n, 5000), 49n);
  assert.equal(formatCashbackSol(5_000_001n), "0.005000001");
});

test("cashback default claim minimum is 0.001 SOL", () => {
  assert.equal(parseCashbackConfig({ CASHBACK_ENABLED: "true" }).minClaimLamports, 1_000_000n);
});

test("cashback dashboard does not mention minimum until claim attempt", () => {
  const summary = {
    enabled: true,
    tradingWalletPublicKey: "Wallet111",
    payoutWalletPublicKey: payoutWallet,
    accruedLamports: 500_000n,
    claimableLamports: 500_000n,
    pendingLamports: 0n,
    lifetimePaidLamports: 0n,
    minClaimLamports: 1_000_000n,
    payoutUnavailableReason: null
  };

  const text = formatCashbackSummaryText(summary);
  assert.doesNotMatch(text, /minimum|cash out|cashout/i);
  assert.equal(
    cashbackSummaryReplyMarkup(summary).inline_keyboard.some((row) =>
      row.some((button) => button.text === "Claim Cashback" && button.callback_data === "cashback:claim")
    ),
    true
  );
  assert.deepEqual(cashbackSummaryReplyMarkup(summary).inline_keyboard.at(-1), [
    { text: "Change Payout Wallet", callback_data: "cashback:set_payout_wallet" },
    { text: "Refresh", callback_data: "cashback:dashboard" }
  ]);
});

test("cashback accrual is direct-only, successful, and idempotent by execution key", async () => {
  const executionKey = buildCashbackExecutionKey({
    chatId: "chat-1",
    tradingWalletPublicKey: "Wallet111",
    sourceSignature: "source-sig",
    executionSignature: "copy-sig",
    action: "buy"
  });
  const entry = buildCashbackAccrual({
    chatId: "chat-1",
    tradingWalletPublicKey: "Wallet111",
    executionKey,
    sourceSignature: "source-sig",
    executionSignature: "copy-sig",
    action: "buy",
    status: "submitted",
    provider: "direct-pump",
    platformFee: platformFee(),
    config: config()
  });

  assert.equal(entry.cashbackLamports, 2_000n);
  assert.equal(
    buildCashbackAccrual({
      chatId: "chat-1",
      tradingWalletPublicKey: "Wallet111",
      executionKey: "portal",
      sourceSignature: "source-sig",
      executionSignature: "copy-sig",
      action: "buy",
      status: "submitted",
      provider: "pumpportal-lightning",
      platformFee: platformFee(),
      config: config()
    }),
    null
  );

  const store = new MemoryCashbackStore();
  await store.accrue(entry);
  await store.accrue(entry);
  assert.equal(store.entries.length, 1);
});

test("claim threshold blocks small balances", async () => {
  const store = new MemoryCashbackStore([
    {
      chatId: "chat-1",
      tradingWalletPublicKey: "Wallet111",
      executionKey: "one",
      sourceSignature: "source",
      executionSignature: "copy",
      action: "buy",
      platformFeeLamports: 10_000n,
      cashbackLamports: 2_000n,
      status: "claimable"
    }
  ]);

  const result = await claimCashback({
    store,
    config: config(),
    connection: {},
    chatId: "chat-1",
    tradingWalletPublicKey: "Wallet111",
    payoutWalletPublicKey: payoutWallet
  });

  assert.equal(result.ok, false);
  assert.equal(result.status, "below_threshold");
  assert.equal(store.payouts.length, 0);
});

test("failed payout keeps claimable ledger balance recoverable", async () => {
  const store = new MemoryCashbackStore([
    {
      chatId: "chat-1",
      tradingWalletPublicKey: "Wallet111111111111111111111111111111111111111",
      executionKey: "one",
      sourceSignature: "source",
      executionSignature: "copy",
      action: "buy",
      platformFeeLamports: 100_000_000n,
      cashbackLamports: 20_000_000n,
      status: "claimable"
    }
  ]);

  const result = await claimCashback({
    store,
    config: config(),
    connection: {},
    chatId: "chat-1",
    tradingWalletPublicKey: "Wallet111111111111111111111111111111111111111",
    payoutWalletPublicKey: payoutWallet
  });

  assert.equal(result.ok, false);
  assert.equal(result.status, "failed");
  assert.equal(store.payouts[0].status, "failed");
  assert.equal(store.entries[0].status, "claimable");
  assert.equal(result.summary.claimableLamports, 20_000_000n);
});
