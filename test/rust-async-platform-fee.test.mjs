import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import {
  buildRustAsyncPlatformFeeCashbackAccrual,
  plannedCopySpendLamportsFromRustExecution
} from "../dist/rust-async-platform-fee.js";

function cashbackConfig(overrides = {}) {
  return {
    enabled: true,
    feeShareBps: 2000,
    minClaimLamports: 1_000_000n,
    maxPayoutLamportsPerDay: 0n,
    payoutWalletPublicKey: null,
    payoutWalletSecretKey: "secret",
    ...overrides
  };
}

test("Rust async platform fee accrual uses planned copy spend and starts pending", () => {
  const entry = buildRustAsyncPlatformFeeCashbackAccrual({
    chatId: "chat-1",
    tradingWalletPublicKey: "CopyWallet111",
    sourceSignature: "observed-sig",
    executionSignature: "rust-buy-sig",
    plannedCopySpendLamports: 1_000_000n,
    platformFeeEnabled: true,
    platformFeeBps: 100,
    platformFeeTreasury: "Treasury111111111111111111111111111111111",
    cashbackConfig: cashbackConfig()
  });

  assert.equal(entry.status, "pending");
  assert.equal(entry.platformFeeCollectionStatus, "pending");
  assert.equal(entry.platformFeeLamports, 10_000n);
  assert.equal(entry.cashbackLamports, 2_000n);
  assert.equal(entry.platformFeeBps, 100);
  assert.equal(entry.platformFeeTreasury, "Treasury111111111111111111111111111111111");
  assert.equal(
    entry.executionKey,
    "chat-1:CopyWallet111:observed-sig:rust-buy-sig:buy:-1:-1"
  );
});

test("Rust async platform fee bridge is default-off and post-submit only", async () => {
  const [indexSource, typesSource, envExample, readme, cashbackSource, safetyMigration] = await Promise.all([
    readFile("src/index.ts", "utf8"),
    readFile("src/types.ts", "utf8"),
    readFile(".env.example", "utf8"),
    readFile("README.md", "utf8"),
    readFile("src/cashback.ts", "utf8"),
    readFile("supabase/migrations/20260709191607_rust_async_platform_fee_delivery_safety.sql", "utf8")
  ]);

  assert.match(typesSource, /copyTradeRustAsyncPlatformFeeEnabled: boolean;/);
  assert.match(typesSource, /copyTradeRustAsyncPlatformFeeCanaryWallets: string\[\];/);
  assert.match(indexSource, /COPY_TRADE_RUST_ASYNC_PLATFORM_FEE_ENABLED/);
  assert.match(indexSource, /RUST_ASYNC_PLATFORM_FEE_ENABLED/);
  assert.match(indexSource, /COPY_TRADE_RUST_ASYNC_PLATFORM_FEE_CANARY_WALLETS/);
  assert.match(envExample, /COPY_TRADE_RUST_ASYNC_PLATFORM_FEE_ENABLED=false/);
  assert.match(envExample, /COPY_TRADE_RUST_ASYNC_PLATFORM_FEE_CANARY_WALLETS=/);
  assert.match(envExample, /COPY_TRADE_RUST_ASYNC_PLATFORM_FEE_SOURCE=local-jsonl/);
  assert.match(envExample, /COPY_TRADE_RUST_ASYNC_PLATFORM_FEE_LOCAL_EXECUTIONS_PATH=\/var\/log\/jito-copy-executions-vps\.jsonl/);
  assert.match(readme, /Rust still sends the buy unchanged/);
  assert.match(readme, /tiny-wallet rollout/);
  assert.match(readme, /COPY_TRADE_RUST_ASYNC_PLATFORM_FEE_SOURCE=local-jsonl/);
  assert.match(readme, /does not change the Rust hot path/);

  assert.match(
    indexSource,
    /fetchRustCopyBuyExecutionRowsFromSupabase[\s\S]*provider", "eq\.shredstream"[\s\S]*observed_action", "eq\.buy"[\s\S]*decision", "eq\.sent"[\s\S]*sent", "eq\.true"[\s\S]*send_signature", "not\.is\.null"[\s\S]*copy_wallet", "not\.is\.null"/
  );
  assert.match(
    indexSource,
    /seedRustAsyncPlatformFeeCollection[\s\S]*plannedCopySpendLamportsFromRustExecution\(row\.raw_execution\)[\s\S]*activeCopyTradeEntriesForTarget\(row\.observed_wallet\)[\s\S]*entry\.subscriber\.tradingWallet\?\.publicKey === row\.copy_wallet/
  );
  assert.match(
    indexSource,
    /function rustCopyBuyExecutionLanded[\s\S]*hasOwnProperty\.call\(row\.chain_report, "err"\)[\s\S]*row\.chain_report\.err === null[\s\S]*buyLanded/
  );
  assert.match(
    indexSource,
    /seedRustAsyncPlatformFeeCollection[\s\S]*!rustCopyBuyExecutionLanded\(row\)/
  );
  assert.match(
    indexSource,
    /prepareRustAsyncPlatformFeeTransfer[\s\S]*new Transaction\(\)\.add\(SystemProgram\.transfer\([\s\S]*fromPubkey: signer\.publicKey[\s\S]*toPubkey: treasury/
  );
  assert.match(
    indexSource,
    /confirmRustAsyncPlatformFeeTransfer[\s\S]*collectionStatus: "confirmed"[\s\S]*ledgerStatus: "claimable"[\s\S]*collectionStatus: "failed"[\s\S]*ledgerStatus: "voided"/
  );
  assert.match(indexSource, /claimPlatformFeeCollections\([\s\S]*leaseToken[\s\S]*leaseDurationMs/);
  assert.match(
    indexSource,
    /prepareRustAsyncPlatformFeeTransfer[\s\S]*transaction\.sign\(signer\)[\s\S]*transaction\.serialize\(\)\.toString\("base64"\)/
  );
  assert.match(
    indexSource,
    /transactionBase64: signedTransfer\.transactionBase64[\s\S]*broadcastRustAsyncPlatformFeeTransfer\(signedTransfer\)/
  );
  assert.match(
    indexSource,
    /entry\.platformFeeTransactionBase64 && entry\.platformFeeTransferSignature[\s\S]*transactionBase64: entry\.platformFeeTransactionBase64/
  );
  assert.match(
    indexSource,
    /collectionStatus === "submitted"[\s\S]*entry\.platformFeeTransactionBase64[\s\S]*broadcastRustAsyncPlatformFeeTransfer\(persistedTransfer\)/
  );
  assert.match(cashbackSource, /expectedLeaseToken[\s\S]*platform_fee_lease_token/);
  assert.match(safetyMigration, /platform_fee_lease_token text null/);
  assert.match(safetyMigration, /platform_fee_transaction_base64 text null/);
  assert.match(safetyMigration, /revoke all on table public\.telegram_cashback_ledger from anon, authenticated/);
});

test("Rust async platform fee accrual accepts snake_case raw execution field", () => {
  assert.equal(
    plannedCopySpendLamportsFromRustExecution({ planned_copy_spend_lamports: "2500000" }),
    2_500_000n
  );
});

test("Rust async platform fee accrual skips flag-off and zero-value cases", () => {
  const base = {
    chatId: "chat-1",
    tradingWalletPublicKey: "CopyWallet111",
    sourceSignature: "observed-sig",
    executionSignature: "rust-buy-sig",
    plannedCopySpendLamports: 1_000_000n,
    platformFeeBps: 100,
    platformFeeTreasury: "Treasury111111111111111111111111111111111",
    cashbackConfig: cashbackConfig()
  };

  assert.equal(buildRustAsyncPlatformFeeCashbackAccrual({
    ...base,
    platformFeeEnabled: false
  }), null);
  assert.equal(buildRustAsyncPlatformFeeCashbackAccrual({
    ...base,
    platformFeeEnabled: true,
    platformFeeBps: 0
  }), null);
  assert.equal(buildRustAsyncPlatformFeeCashbackAccrual({
    ...base,
    platformFeeEnabled: true,
    cashbackConfig: cashbackConfig({ feeShareBps: 0 })
  }), null);
});

test("duplicate Rust async platform fee execution keys do not double accrue", async () => {
  const entries = [];
  const accrue = async (entry) => {
    if (!entries.some((existing) => existing.executionKey === entry.executionKey)) {
      entries.push(entry);
    }
  };
  const input = {
    chatId: "chat-1",
    tradingWalletPublicKey: "CopyWallet111",
    sourceSignature: "observed-sig",
    executionSignature: "rust-buy-sig",
    plannedCopySpendLamports: 1_000_000n,
    platformFeeEnabled: true,
    platformFeeBps: 100,
    platformFeeTreasury: "Treasury111111111111111111111111111111111",
    cashbackConfig: cashbackConfig()
  };

  await accrue(buildRustAsyncPlatformFeeCashbackAccrual(input));
  await accrue(buildRustAsyncPlatformFeeCashbackAccrual(input));

  assert.equal(entries.length, 1);
  assert.equal(entries[0].cashbackLamports, 2_000n);
});
