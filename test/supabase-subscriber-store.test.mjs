import assert from "node:assert/strict";
import test from "node:test";
import { createSupabaseSubscriberStore, importSubscribersToSupabase, subscriberFromRow } from "../dist/subscribers-supabase.js";

const wallet = "Wallet111111111111111111111111111111111111111";
const otherWallet = "Other111111111111111111111111111111111111111";

class MemorySubscriberRepository {
  constructor(records = []) {
    this.subscribers = new Map();
    this.wallets = new Map();
    this.copyTradeWallets = new Map();
    this.tradingWallets = new Map();

    for (const record of records) {
      this.subscribers.set(record.chatId, {
        ...record,
        watchedWallets: [],
        copyTradeWallets: [],
        tradingWallet: record.tradingWallet || null,
        tradingWallets: []
      });

      for (const walletRecord of record.watchedWallets || []) {
        this.wallets.set(`${record.chatId}:${walletRecord.address}`, {
          chatId: record.chatId,
          ...walletRecord
        });
      }

      for (const walletRecord of record.copyTradeWallets || []) {
        this.copyTradeWallets.set(`${record.chatId}:${walletRecord.address}`, {
          chatId: record.chatId,
          ...walletRecord
        });
      }

      const tradingWallets = record.tradingWallets?.length
        ? record.tradingWallets
        : record.tradingWallet
          ? [record.tradingWallet]
          : [];

      for (const tradingWallet of tradingWallets) {
        this.tradingWallets.set(`${record.chatId}:${tradingWallet.publicKey}`, {
          chatId: record.chatId,
          ...tradingWallet
        });
      }
    }
  }

  async listSubscribers() {
    return [...this.subscribers.values()]
      .map((subscriber) => {
        const tradingWallets = [...this.tradingWallets.values()]
          .filter((walletRecord) => walletRecord.chatId === subscriber.chatId)
          .map(({ chatId: _chatId, ...walletRecord }) => ({ ...walletRecord }));
        const activePublicKey = subscriber.tradingWallet?.publicKey || null;
        const tradingWallet = activePublicKey
          ? tradingWallets.find((walletRecord) => walletRecord.publicKey === activePublicKey) || tradingWallets[0] || null
          : tradingWallets[0] || null;

        return {
          ...subscriber,
          watchedWallets: [...this.wallets.values()]
          .filter((walletRecord) => walletRecord.chatId === subscriber.chatId)
          .map(({ chatId: _chatId, ...walletRecord }) => ({ ...walletRecord })),
          copyTradeWallets: [...this.copyTradeWallets.values()]
          .filter((walletRecord) => walletRecord.chatId === subscriber.chatId)
          .map(({ chatId: _chatId, ...walletRecord }) => ({ ...walletRecord })),
          tradingWallet,
          tradingWallets
        };
      })
      .sort((left, right) => left.chatId.localeCompare(right.chatId));
  }

  async upsertSubscriber(subscriber) {
    const existing = this.subscribers.get(subscriber.chatId);
    this.subscribers.set(subscriber.chatId, {
      ...subscriber,
      watchedWallets: existing?.watchedWallets || [],
      copyTradeWallets: existing?.copyTradeWallets || [],
      tradingWallet: subscriber.tradingWallet || existing?.tradingWallet || null,
      tradingWallets: [],
      copyWalletAddress: null,
      copyWalletAddresses: []
    });
  }

  async deleteSubscriber(chatId) {
    this.subscribers.delete(chatId);

    for (const key of this.wallets.keys()) {
      if (key.startsWith(`${chatId}:`)) {
        this.wallets.delete(key);
      }
    }

    for (const key of this.copyTradeWallets.keys()) {
      if (key.startsWith(`${chatId}:`)) {
        this.copyTradeWallets.delete(key);
      }
    }

    for (const key of this.tradingWallets.keys()) {
      if (key.startsWith(`${chatId}:`)) {
        this.tradingWallets.delete(key);
      }
    }
  }

  async upsertWatchedWallet(chatId, walletRecord) {
    this.wallets.set(`${chatId}:${walletRecord.address}`, {
      chatId,
      ...walletRecord
    });
  }

  async deleteWatchedWallet(chatId, address) {
    this.wallets.delete(`${chatId}:${address}`);
  }

  async upsertCopyTradeWallet(chatId, walletRecord) {
    this.copyTradeWallets.set(`${chatId}:${walletRecord.address}`, {
      chatId,
      ...walletRecord
    });
  }

  async deleteCopyTradeWallet(chatId, address) {
    this.copyTradeWallets.delete(`${chatId}:${address}`);
  }

  async deleteAllCopyTradeWallets(chatId) {
    for (const key of this.copyTradeWallets.keys()) {
      if (key.startsWith(`${chatId}:`)) {
        this.copyTradeWallets.delete(key);
      }
    }
  }

  async upsertTradingWallet(chatId, walletRecord) {
    this.tradingWallets.set(`${chatId}:${walletRecord.publicKey}`, {
      chatId,
      ...walletRecord
    });
  }

  async deleteTradingWallet(chatId, publicKey) {
    this.tradingWallets.delete(`${chatId}:${publicKey}`);
  }

  async deleteAllTradingWallets(chatId) {
    for (const key of this.tradingWallets.keys()) {
      if (key.startsWith(`${chatId}:`)) {
        this.tradingWallets.delete(key);
      }
    }
  }
}

function subscriber(overrides = {}) {
  return {
    chatId: "chat-1",
    mode: "both",
    watchedWallets: [
      {
        address: wallet,
        label: "Alpha",
        addedAt: "2026-05-22T00:00:00.000Z",
        updatedAt: "2026-05-22T00:00:00.000Z"
      }
    ],
    copyTradeWallets: [],
    tradingWallet: null,
    copyWalletAddress: null,
    copyWalletAddresses: [],
    copyAmountSol: null,
    copyTradeBuySlippagePercent: null,
    copyTradeBuyPriorityFeeSol: null,
    copyTradeSellSlippagePercent: null,
    copyTradeSellPriorityFeeSol: null,
    copyTradeRetryFailedBuys: false,
    copyTargetWalletAddress: null,
    verifiedAt: "2026-05-22T00:00:00.000Z",
    updatedAt: "2026-05-22T00:00:00.000Z",
    ...overrides
  };
}

test("Supabase subscriber store can load stored rows plus explicit seeded chat ids", async () => {
  const repository = new MemorySubscriberRepository([subscriber()]);
  const store = createSupabaseSubscriberStore({
    repository,
    initialChatIds: ["seed-chat"]
  });

  await store.init();

  assert.equal(store.count(), 2);
  assert.equal(store.get("chat-1")?.mode, "both");
  assert.deepEqual(
    store.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [[wallet, "Alpha"]]
  );
  assert.equal(store.get("seed-chat")?.mode, "migrations");
  const stored = await repository.listSubscribers();
  assert.equal(stored.some((entry) => entry.chatId === "seed-chat"), true);
});

test("Supabase subscriber store mirrors JSON store mutations", async () => {
  const repository = new MemorySubscriberRepository();
  const store = createSupabaseSubscriberStore({ repository });

  await store.init();
  assert.equal(await store.watchWallet("chat-1", wallet, "Before verify"), false);

  await store.add("chat-1");
  assert.equal(await store.watchWallet("chat-1", wallet, "Alpha <Wallet>"), true);
  assert.deepEqual(
    store.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [[wallet, "Alpha <Wallet>"]]
  );
  assert.equal(await store.watchWallet("chat-1", wallet), true);
  assert.deepEqual(
    store.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [[wallet, "Alpha <Wallet>"]]
  );
  assert.equal(await store.renameWallet("chat-1", wallet, "Beta Wallet"), true);
  assert.deepEqual(
    store.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [[wallet, "Beta Wallet"]]
  );
  assert.equal(await store.renameWallet("chat-1", wallet, null), true);
  assert.deepEqual(
    store.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [[wallet, null]]
  );
  assert.equal(await store.renameWallet("chat-1", otherWallet, "Missing"), false);
  assert.equal(await store.renameWallet("chat-2", wallet, "Unverified"), false);
  assert.equal(await store.setMode("chat-1", "newtokens"), true);
  assert.equal(await store.setCopyWallet("chat-1", otherWallet), true);
  assert.equal(await store.setCopyWallet("chat-1", wallet), true);
  assert.deepEqual(store.listCopyWallets("chat-1"), [otherWallet, wallet]);
  assert.equal(await store.setCopyAmountSol("chat-1", 0.25), true);
  assert.equal(
    await store.setTradingWallet("chat-1", {
      publicKey: otherWallet,
      encryptedApiKey: "encrypted-api-key",
      apiKeyLast4: "ikey",
      label: null,
      createdAt: "2026-05-23T00:00:00.000Z",
      updatedAt: "2026-05-23T00:00:00.000Z"
    }),
    true
  );
  assert.equal(await store.setTradingWallet("chat-2", store.getTradingWallet("chat-1")), false);
  assert.equal(await store.renameTradingWallet("chat-1", "Main Wallet"), true);
  assert.equal(await store.renameTradingWallet("chat-2", "Missing"), false);
  assert.equal(await store.watchCopyTradeWallet("chat-1", otherWallet, "Copy Alpha"), true);
  assert.equal(await store.watchCopyTradeWallet("chat-1", wallet, "Copy Beta"), true);
  assert.equal(
    await store.setCopyTradeWalletTrailingSellConfig("chat-1", otherWallet, {
      enabled: true,
      mode: "custom_steps",
      percentBasis: "original_position",
      steps: [
        { percent: 25, delayMs: 15000 },
        { percent: 100, delayMs: 60000 }
      ],
      updatedAt: "2026-05-23T01:00:00.000Z"
    }),
    true
  );
  assert.equal(await store.setCopyTradeWalletTrailingSellConfig("chat-1", "missing", null), false);
  assert.equal(await store.renameCopyTradeWallet("chat-1", wallet, null), true);
  assert.equal(await store.renameCopyTradeWallet("chat-1", "missing", "Nope"), false);
  assert.equal(await store.setCopyWallet("chat-2", otherWallet), false);
  assert.equal(await store.setCopyAmountSol("chat-2", 0.25), false);
  assert.equal(await store.setCopyTradeBuySlippage("chat-1", 12.5), true);
  assert.equal(await store.setCopyTradeBuyPriorityFee("chat-1", 0.00012), true);
  assert.equal(await store.setCopyTradeSellSlippage("chat-1", 20), true);
  assert.equal(await store.setCopyTradeSellPriorityFee("chat-1", 0.0002), true);
  assert.equal(await store.setCopyTradeRetryFailedBuys("chat-1", true), true);
  assert.equal(await store.setCopyTradeBuySlippage("chat-2", 12.5), false);
  assert.equal(await store.setCopyTradeSellPriorityFee("chat-2", 0.0002), false);
  assert.equal(await store.setCopyTradeRetryFailedBuys("chat-2", true), false);
  assert.equal(await store.watchCopyTradeWallet("chat-2", wallet, "Unverified"), false);
  assert.equal(store.get("chat-1")?.mode, "newtokens");
  assert.equal(store.get("chat-1")?.copyWalletAddress, otherWallet);
  assert.deepEqual(store.get("chat-1")?.copyWalletAddresses, [otherWallet, wallet]);
  assert.deepEqual(store.listCopyWallets("chat-1"), [otherWallet, wallet]);
  assert.equal(store.get("chat-1")?.copyAmountSol, 0.25);
  assert.equal(store.get("chat-1")?.copyTradeBuySlippagePercent, 12.5);
  assert.equal(store.get("chat-1")?.copyTradeBuyPriorityFeeSol, 0.00012);
  assert.equal(store.get("chat-1")?.copyTradeSellSlippagePercent, 20);
  assert.equal(store.get("chat-1")?.copyTradeSellPriorityFeeSol, 0.0002);
  assert.equal(store.get("chat-1")?.copyTradeRetryFailedBuys, true);
  assert.equal(store.getTradingWallet("chat-1")?.publicKey, otherWallet);
  assert.equal(store.getTradingWallet("chat-1")?.apiKeyLast4, "ikey");
  assert.equal(store.getTradingWallet("chat-1")?.label, "Main Wallet");
  assert.deepEqual(
    store.listCopyTradeWallets("chat-1").map((entry) => [entry.address, entry.label, entry.trailingSellConfig?.percentBasis || null]),
    [
      [otherWallet, "Copy Alpha", "original_position"],
      [wallet, null, null]
    ]
  );

  const reloaded = createSupabaseSubscriberStore({ repository });
  await reloaded.init();
  assert.deepEqual(
    reloaded.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [[wallet, null]]
  );
  assert.equal(reloaded.get("chat-1")?.copyWalletAddress, null);
  assert.deepEqual(reloaded.get("chat-1")?.copyWalletAddresses, []);
  assert.equal(reloaded.get("chat-1")?.copyAmountSol, 0.25);
  assert.equal(reloaded.get("chat-1")?.copyTradeBuySlippagePercent, 12.5);
  assert.equal(reloaded.get("chat-1")?.copyTradeBuyPriorityFeeSol, 0.00012);
  assert.equal(reloaded.get("chat-1")?.copyTradeSellSlippagePercent, 20);
  assert.equal(reloaded.get("chat-1")?.copyTradeSellPriorityFeeSol, 0.0002);
  assert.equal(reloaded.get("chat-1")?.copyTradeRetryFailedBuys, true);
  assert.equal(reloaded.getTradingWallet("chat-1")?.publicKey, otherWallet);
  assert.equal(reloaded.getTradingWallet("chat-1")?.label, "Main Wallet");
  assert.equal(await reloaded.renameTradingWallet("chat-1", null), true);
  assert.equal(reloaded.getTradingWallet("chat-1")?.label, null);
  assert.deepEqual(
    reloaded.listCopyTradeWallets("chat-1").map((entry) => [entry.address, entry.label, entry.trailingSellConfig?.steps.length || 0]),
    [
      [otherWallet, "Copy Alpha", 2],
      [wallet, null, 0]
    ]
  );
  assert.equal(await reloaded.resetCopyTradeExecutionSettings("chat-1"), true);
  assert.equal(reloaded.get("chat-1")?.copyTradeBuySlippagePercent, null);
  assert.equal(reloaded.get("chat-1")?.copyTradeBuyPriorityFeeSol, null);
  assert.equal(reloaded.get("chat-1")?.copyTradeSellSlippagePercent, null);
  assert.equal(reloaded.get("chat-1")?.copyTradeSellPriorityFeeSol, null);
  assert.equal(reloaded.get("chat-1")?.copyTradeRetryFailedBuys, false);
  assert.equal(await reloaded.resetCopyTradeExecutionSettings("chat-2"), false);

  assert.equal(await reloaded.unwatchWallet("chat-1", wallet), true);
  assert.deepEqual(reloaded.listWatchedWallets("chat-1"), []);
  assert.deepEqual(reloaded.listCopyTradeWallets("chat-1").map((entry) => entry.address), [otherWallet, wallet]);
  assert.equal(await reloaded.unwatchCopyTradeWallet("chat-1", wallet), true);
  assert.deepEqual(reloaded.listCopyTradeWallets("chat-1").map((entry) => entry.address), [otherWallet]);
  assert.equal(await reloaded.unwatchAllCopyTradeWallets("chat-1"), 1);
  assert.deepEqual(reloaded.listCopyTradeWallets("chat-1"), []);
  assert.deepEqual(reloaded.listWatchedWallets("chat-1"), []);
  assert.equal(reloaded.getTradingWallet("chat-1")?.publicKey, otherWallet);
  assert.equal(await reloaded.unwatchAllCopyTradeWallets("chat-1"), 0);
  await reloaded.remove("chat-1");
  assert.equal(reloaded.has("chat-1"), false);
});

test("Supabase subscriber store does not pretend retry toggle persisted when column is missing", async () => {
  class MissingRetryColumnRepository extends MemorySubscriberRepository {
    async upsertSubscriber(record) {
      if (record.copyTradeRetryFailedBuys) {
        throw new Error("Could not find the 'copy_trade_retry_failed_buys' column in the schema cache");
      }

      return super.upsertSubscriber(record);
    }
  }

  const repository = new MissingRetryColumnRepository();
  const store = createSupabaseSubscriberStore({ repository });

  await store.init();
  await store.add("chat-1");
  await assert.rejects(
    () => store.setCopyTradeRetryFailedBuys("chat-1", true),
    /copy_trade_retry_failed_buys/
  );
  assert.equal(store.get("chat-1")?.copyTradeRetryFailedBuys, false);
});

test("importSubscribersToSupabase upserts subscribers and watched wallets", async () => {
  const repository = new MemorySubscriberRepository();

  await importSubscribersToSupabase({
    repository,
    subscribers: [subscriber()]
  });

  const imported = await repository.listSubscribers();
  assert.equal(imported.length, 1);
  assert.equal(imported[0].chatId, "chat-1");
  assert.deepEqual(
    imported[0].watchedWallets.map((entry) => [entry.address, entry.label]),
    [[wallet, "Alpha"]]
  );
});

test("Supabase subscriber store persists local Solana custody metadata", async () => {
  const repository = new MemorySubscriberRepository();
  const store = createSupabaseSubscriberStore({ repository });

  await store.init();
  await store.add("chat-1");

  assert.equal(
    await store.setTradingWallet("chat-1", {
      publicKey: otherWallet,
      provider: "local-solana",
      kind: "local-solana",
      encryptedApiKey: "",
      apiKeyLast4: "3cr3",
      encryptedSecretKey: "encrypted-secret-key",
      secretKeyFormat: "base58",
      keyLast4: "3cr3",
      label: "Local One",
      createdAt: "2026-05-28T00:00:00.000Z",
      updatedAt: "2026-05-28T00:00:00.000Z"
    }),
    true
  );

  const reloaded = createSupabaseSubscriberStore({ repository });
  await reloaded.init();
  const walletRecord = reloaded.getTradingWallet("chat-1");

  assert.equal(walletRecord?.provider, "local-solana");
  assert.equal(walletRecord?.kind, "local-solana");
  assert.equal(walletRecord?.encryptedSecretKey, "encrypted-secret-key");
  assert.equal(walletRecord?.secretKeyFormat, "base58");
  assert.equal(walletRecord?.keyLast4, "3cr3");
  assert.equal(walletRecord?.label, "Local One");
});

test("Supabase subscriber store persists multiple trading wallets and active selection", async () => {
  const repository = new MemorySubscriberRepository();
  const store = createSupabaseSubscriberStore({ repository });

  await store.init();
  await store.add("chat-1");

  const firstWallet = {
    publicKey: wallet,
    provider: "local-solana",
    kind: "local-solana",
    encryptedApiKey: "",
    apiKeyLast4: "1111",
    encryptedSecretKey: "encrypted-secret-key-1",
    secretKeyFormat: "base58",
    keyLast4: "1111",
    label: "Local One",
    createdAt: "2026-05-28T00:00:00.000Z",
    updatedAt: "2026-05-28T00:00:00.000Z"
  };
  const secondWallet = {
    publicKey: otherWallet,
    provider: "local-solana",
    kind: "local-solana",
    encryptedApiKey: "",
    apiKeyLast4: "2222",
    encryptedSecretKey: "encrypted-secret-key-2",
    secretKeyFormat: "base58",
    keyLast4: "2222",
    label: "Local Two",
    createdAt: "2026-05-28T00:01:00.000Z",
    updatedAt: "2026-05-28T00:01:00.000Z"
  };

  assert.equal(await store.setTradingWallet("chat-1", firstWallet), true);
  assert.equal(await store.setTradingWallet("chat-1", secondWallet), true);
  assert.equal(store.getTradingWallet("chat-1")?.publicKey, otherWallet);
  assert.deepEqual(store.listTradingWallets("chat-1").map((entry) => entry.publicKey), [wallet, otherWallet]);
  assert.equal(await store.setActiveTradingWallet("chat-1", wallet), true);
  assert.equal(store.getTradingWallet("chat-1")?.publicKey, wallet);

  const reloaded = createSupabaseSubscriberStore({ repository });
  await reloaded.init();

  assert.deepEqual(reloaded.listTradingWallets("chat-1").map((entry) => entry.publicKey), [wallet, otherWallet]);
  assert.equal(reloaded.getTradingWallet("chat-1")?.publicKey, wallet);

  assert.equal(await reloaded.removeTradingWallet("chat-1", "missing-wallet"), false);
  assert.equal(await reloaded.removeTradingWallet("chat-1", otherWallet), true);
  assert.deepEqual(reloaded.listTradingWallets("chat-1").map((entry) => entry.publicKey), [wallet]);
  assert.equal(reloaded.getTradingWallet("chat-1")?.publicKey, wallet);

  assert.equal(await reloaded.setTradingWallet("chat-1", secondWallet), true);
  assert.equal(reloaded.getTradingWallet("chat-1")?.publicKey, otherWallet);
  assert.equal(await reloaded.removeTradingWallet("chat-1", otherWallet), true);
  assert.deepEqual(reloaded.listTradingWallets("chat-1").map((entry) => entry.publicKey), [wallet]);
  assert.equal(reloaded.getTradingWallet("chat-1")?.publicKey, wallet);

  assert.equal(await reloaded.removeAllTradingWallets("chat-1"), 1);
  assert.deepEqual(reloaded.listTradingWallets("chat-1"), []);
  assert.equal(reloaded.getTradingWallet("chat-1"), null);
  assert.equal(await reloaded.removeAllTradingWallets("chat-1"), 0);

  const stored = await repository.listSubscribers();
  assert.equal(stored[0].copyTargetWalletAddress, null);
  assert.deepEqual(stored[0].tradingWallets, []);
  assert.equal(stored[0].tradingWallet, null);
});

test("subscriberFromRow maps legacy PumpPortal and local Solana trading wallet rows", () => {
  const legacy = subscriberFromRow({
    chat_id: "chat-1",
    mode: "both",
    copy_wallet_address: null,
    copy_wallet_addresses: [],
    copy_amount_sol: null,
    copy_target_wallet_address: null,
    verified_at: "2026-05-28T00:00:00.000Z",
    updated_at: "2026-05-28T00:00:00.000Z",
    telegram_watched_wallets: [],
    telegram_copytrade_wallets: [],
    telegram_trading_wallets: [
      {
        public_key: wallet,
        encrypted_api_key: "encrypted-api-key",
        api_key_last4: "ikey",
        created_at: "2026-05-28T00:00:00.000Z",
        updated_at: "2026-05-28T00:00:00.000Z"
      }
    ]
  });
  const local = subscriberFromRow({
    chat_id: "chat-2",
    mode: "both",
    copy_wallet_address: null,
    copy_wallet_addresses: [],
    copy_amount_sol: null,
    copy_target_wallet_address: null,
    verified_at: "2026-05-28T00:00:00.000Z",
    updated_at: "2026-05-28T00:00:00.000Z",
    telegram_watched_wallets: [],
    telegram_copytrade_wallets: [],
    telegram_trading_wallets: [
      {
        public_key: otherWallet,
        encrypted_api_key: "",
        api_key_last4: "3cr3",
        provider: "local-solana",
        kind: "local-solana",
        encrypted_secret_key: "encrypted-secret-key",
        secret_key_format: "base58",
        key_last4: "3cr3",
        created_at: "2026-05-28T00:00:00.000Z",
        updated_at: "2026-05-28T00:00:00.000Z"
      }
    ]
  });

  assert.equal(legacy.tradingWallet?.provider, "pumpportal-lightning");
  assert.equal(legacy.tradingWallet?.encryptedApiKey, "encrypted-api-key");
  assert.equal(local.tradingWallet?.provider, "local-solana");
  assert.equal(local.tradingWallet?.encryptedSecretKey, "encrypted-secret-key");
  assert.equal(local.tradingWallet?.secretKeyFormat, "base58");
});

test("Supabase subscriber store migrates legacy copy target out of watched wallets", async () => {
  const repository = new MemorySubscriberRepository([
    subscriber({
      watchedWallets: [
        {
          address: wallet,
          label: "Legacy Copy",
          addedAt: "2026-05-22T00:00:00.000Z",
          updatedAt: "2026-05-22T00:00:00.000Z"
        },
        {
          address: otherWallet,
          label: "Normal Watch",
          addedAt: "2026-05-22T00:00:00.000Z",
          updatedAt: "2026-05-22T00:00:00.000Z"
        }
      ],
      copyTargetWalletAddress: wallet
    })
  ]);
  const store = createSupabaseSubscriberStore({ repository });

  await store.init();

  assert.deepEqual(store.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]), [[otherWallet, "Normal Watch"]]);
  assert.deepEqual(store.listCopyTradeWallets("chat-1").map((entry) => [entry.address, entry.label]), [[wallet, "Legacy Copy"]]);
});
