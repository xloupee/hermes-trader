import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import {
  COPY_TRADE_HOT_PATH_SNAPSHOT_VERSION,
  createCopyTradeHotPathSnapshot,
  validateCopyTradeHotPathSnapshot,
  writeCopyTradeHotPathSnapshotFile
} from "../dist/copytrade-hot-snapshot.js";

const routing = {
  executionProvider: "direct-auto",
  pool: "auto",
  defaultSlippage: 10,
  defaultPriorityFee: 0.00005,
  maxBuySol: 0.2,
  dailySolCap: 1,
  maxSignalAgeMs: 750,
  minWalletReserveSol: 0,
  allowedSources: ["shredstream"],
  maxSlippage: 20,
  maxPriorityFee: 0.001,
  maxCopyWalletsPerChat: 10,
  liveTradingEnabled: true,
  emergencyStopped: false
};

function subscriber(overrides = {}) {
  return {
    chatId: overrides.chatId || "chat-1",
    mode: "both",
    watchedWallets: [],
    copyTradeWallets: overrides.copyTradeWallets || [
      {
        address: "WalletB1111111111111111111111111111111111",
        label: "Beta",
        addedAt: "now",
        updatedAt: "now"
      },
      {
        address: "WalletA1111111111111111111111111111111111",
        label: "Alpha",
        addedAt: "now",
        updatedAt: "now"
      }
    ],
    tradingWallet: Object.hasOwn(overrides, "tradingWallet")
      ? overrides.tradingWallet
      : {
          publicKey: overrides.tradingWalletPublicKey || "TradingWallet111111111111111111111111111111",
          provider: overrides.tradingWalletProvider || "local-solana",
          kind: overrides.tradingWalletProvider || "local-solana",
          encryptedApiKey: "encrypted",
          apiKeyLast4: "abcd",
          encryptedSecretKey: overrides.encryptedSecretKey ?? "encrypted-secret",
          createdAt: "now",
          updatedAt: "now"
        },
    tradingWallets: [],
    copyWalletAddress: null,
    copyWalletAddresses: [],
    copyAmountSol: overrides.copyAmountSol ?? 0.01,
    copyTradeBuySlippagePercent: overrides.copyTradeBuySlippagePercent ?? null,
    copyTradeBuyPriorityFeeSol: overrides.copyTradeBuyPriorityFeeSol ?? null,
    copyTradeSellSlippagePercent: null,
    copyTradeSellPriorityFeeSol: null,
    copyTradeRetryFailedBuys: overrides.copyTradeRetryFailedBuys ?? false,
    copyTradeBuyPressureSellEnabled: overrides.copyTradeBuyPressureSellEnabled ?? false,
    copyTradeBuyPressureSellTimeoutMs: overrides.copyTradeBuyPressureSellTimeoutMs ?? null,
    copyTargetWalletAddress: null,
    cashbackPayoutWalletAddress: null,
    notificationsPaused: overrides.notificationsPaused ?? false,
    verifiedAt: "now",
    updatedAt: "now"
  };
}

test("copy trade hot-path snapshot is deterministic and checksummed", () => {
  const snapshot = createCopyTradeHotPathSnapshot({
    subscribers: [
      subscriber({ chatId: "chat-b" }),
      subscriber({ chatId: "chat-a", copyTradeRetryFailedBuys: true })
    ],
    routing,
    dailySpentSolByChatId: {
      "chat-a": 0.1,
      "chat-b": 0
    },
    sequence: 7,
    generatedAtMs: 1_780_000_000_000
  });

  assert.equal(snapshot.version, COPY_TRADE_HOT_PATH_SNAPSHOT_VERSION);
  assert.equal(snapshot.sequence, 7);
  assert.match(snapshot.checksum, /^sha256:[a-f0-9]{64}$/);
  assert.equal(validateCopyTradeHotPathSnapshot(snapshot), true);
  assert.deepEqual(snapshot.subscribers.map((entry) => entry.chatId), ["chat-a", "chat-b"]);
  assert.deepEqual(snapshot.subscribers[0].wallets.map((wallet) => wallet.address), [
    "WalletA1111111111111111111111111111111111",
    "WalletB1111111111111111111111111111111111"
  ]);
  assert.doesNotMatch(JSON.stringify(snapshot), /encrypted|encryptedSecretKey|apiKeyLast4|abcd/);
  assert.equal(snapshot.subscribers[0].dailySpentSol, 0.1);
});

test("copy trade hot-path snapshot skips incomplete or disabled copy state", () => {
  const snapshot = createCopyTradeHotPathSnapshot({
    subscribers: [
      subscriber({ chatId: "complete" }),
      subscriber({ chatId: "paused", notificationsPaused: true }),
      subscriber({ chatId: "missing-wallet", tradingWallet: null }),
      subscriber({ chatId: "pumpportal-wallet", tradingWalletProvider: "pumpportal-lightning" }),
      subscriber({ chatId: "missing-local-secret", encryptedSecretKey: "" }),
      subscriber({ chatId: "missing-amount", copyAmountSol: 0 }),
      subscriber({
        chatId: "disabled-copy-wallet",
        copyTradeWallets: [
          {
            address: "Disabled111111111111111111111111111111111",
            label: "Disabled",
            copyTradeEnabled: false,
            addedAt: "now",
            updatedAt: "now"
          }
        ]
      })
    ],
    routing,
    dailySpentSolByChatId: {
      complete: 0,
      paused: 0,
      "missing-wallet": 0,
      "pumpportal-wallet": 0,
      "missing-local-secret": 0,
      "missing-amount": 0,
      "disabled-copy-wallet": 0
    },
    sequence: 1,
    generatedAtMs: 1
  });

  assert.deepEqual(snapshot.subscribers.map((entry) => entry.chatId), ["complete"]);
});

test("copy trade hot-path snapshot exports warm config when Node live trading is disabled", () => {
  const snapshot = createCopyTradeHotPathSnapshot({
    subscribers: [subscriber({ chatId: "complete" })],
    routing: {
      ...routing,
      liveTradingEnabled: false
    },
    dailySpentSolByChatId: {
      complete: 0
    },
    sequence: 1,
    generatedAtMs: 1
  });

  assert.deepEqual(snapshot.subscribers.map((entry) => entry.chatId), ["complete"]);
});

test("copy trade hot-path snapshot still fails closed during emergency stop", () => {
  const snapshot = createCopyTradeHotPathSnapshot({
    subscribers: [subscriber({ chatId: "complete" })],
    routing: {
      ...routing,
      liveTradingEnabled: false,
      emergencyStopped: true
    },
    dailySpentSolByChatId: {
      complete: 0
    },
    sequence: 1,
    generatedAtMs: 1
  });

  assert.deepEqual(snapshot.subscribers, []);
});

test("copy trade hot-path snapshot skips settings that would violate warm risk caps", () => {
  const snapshot = createCopyTradeHotPathSnapshot({
    subscribers: [
      subscriber({ chatId: "complete" }),
      subscriber({ chatId: "above-buy-cap", copyAmountSol: 0.3 }),
      subscriber({ chatId: "above-daily-cap", copyAmountSol: 0.2 }),
      subscriber({ chatId: "above-slippage-cap", copyTradeBuySlippagePercent: 25 }),
      subscriber({ chatId: "above-priority-cap", copyTradeBuyPriorityFeeSol: 0.01 })
    ],
    routing,
    dailySpentSolByChatId: {
      complete: 0,
      "above-buy-cap": 0,
      "above-daily-cap": 0.9,
      "above-slippage-cap": 0,
      "above-priority-cap": 0
    },
    sequence: 2,
    generatedAtMs: 1
  });

  assert.deepEqual(snapshot.subscribers.map((entry) => entry.chatId), ["complete"]);
});

test("copy trade hot-path snapshot validation rejects partial or mutated state", () => {
  const snapshot = createCopyTradeHotPathSnapshot({
    subscribers: [subscriber()],
    routing,
    dailySpentSolByChatId: {
      "chat-1": 0
    },
    sequence: 1,
    generatedAtMs: 1
  });
  const mutated = structuredClone(snapshot);
  mutated.subscribers[0].copyAmountSol = 0.02;

  assert.equal(validateCopyTradeHotPathSnapshot(mutated), false);
  assert.equal(validateCopyTradeHotPathSnapshot({ ...snapshot, version: 2 }), false);
});

test("copy trade hot-path snapshot writes atomically", async () => {
  const dir = await mkdtemp(join(tmpdir(), "copytrade-hot-snapshot-"));
  const path = join(dir, "snapshot.json");
  const snapshot = createCopyTradeHotPathSnapshot({
    subscribers: [subscriber()],
    routing,
    dailySpentSolByChatId: {
      "chat-1": 0
    },
    sequence: 1,
    generatedAtMs: 1
  });

  try {
    await writeCopyTradeHotPathSnapshotFile(path, snapshot);
    assert.deepEqual(JSON.parse(await readFile(path, "utf8")), snapshot);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});
