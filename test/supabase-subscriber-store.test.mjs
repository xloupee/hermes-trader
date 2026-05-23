import assert from "node:assert/strict";
import test from "node:test";
import { createSupabaseSubscriberStore, importSubscribersToSupabase } from "../dist/subscribers-supabase.js";

const wallet = "Wallet111111111111111111111111111111111111111";
const otherWallet = "Other111111111111111111111111111111111111111";

class MemorySubscriberRepository {
  constructor(records = []) {
    this.subscribers = new Map();
    this.wallets = new Map();
    this.copyTradeWallets = new Map();
    this.myWallets = new Map();
    this.tradingWallets = new Map();

    for (const record of records) {
      this.subscribers.set(record.chatId, {
        ...record,
        watchedWallets: [],
        copyTradeWallets: [],
        myWallets: [],
        tradingWallet: null
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

      for (const walletRecord of record.myWallets || []) {
        this.myWallets.set(`${record.chatId}:${walletRecord.address}`, {
          chatId: record.chatId,
          ...walletRecord
        });
      }

      if (record.tradingWallet) {
        this.tradingWallets.set(record.chatId, {
          chatId: record.chatId,
          ...record.tradingWallet
        });
      }
    }
  }

  async listSubscribers() {
    return [...this.subscribers.values()]
      .map((subscriber) => ({
        ...subscriber,
        watchedWallets: [...this.wallets.values()]
          .filter((walletRecord) => walletRecord.chatId === subscriber.chatId)
          .map(({ chatId: _chatId, ...walletRecord }) => ({ ...walletRecord })),
        copyTradeWallets: [...this.copyTradeWallets.values()]
          .filter((walletRecord) => walletRecord.chatId === subscriber.chatId)
          .map(({ chatId: _chatId, ...walletRecord }) => ({ ...walletRecord })),
        myWallets: [...this.myWallets.values()]
          .filter((walletRecord) => walletRecord.chatId === subscriber.chatId)
          .map(({ chatId: _chatId, ...walletRecord }) => ({ ...walletRecord })),
        tradingWallet: this.tradingWallets.has(subscriber.chatId)
          ? (({ chatId: _chatId, ...walletRecord }) => ({ ...walletRecord }))(this.tradingWallets.get(subscriber.chatId))
          : null
      }))
      .sort((left, right) => left.chatId.localeCompare(right.chatId));
  }

  async upsertSubscriber(subscriber) {
    const existing = this.subscribers.get(subscriber.chatId);
    this.subscribers.set(subscriber.chatId, {
      ...subscriber,
      watchedWallets: existing?.watchedWallets || [],
      copyTradeWallets: existing?.copyTradeWallets || [],
      myWallets: existing?.myWallets || [],
      tradingWallet: existing?.tradingWallet || null
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

    for (const key of this.myWallets.keys()) {
      if (key.startsWith(`${chatId}:`)) {
        this.myWallets.delete(key);
      }
    }

    this.tradingWallets.delete(chatId);
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

  async upsertMyWallet(chatId, walletRecord) {
    this.myWallets.set(`${chatId}:${walletRecord.address}`, {
      chatId,
      ...walletRecord
    });
  }

  async deleteMyWallet(chatId, address) {
    this.myWallets.delete(`${chatId}:${address}`);
  }

  async upsertTradingWallet(chatId, walletRecord) {
    this.tradingWallets.set(chatId, {
      chatId,
      ...walletRecord
    });
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
    myWallets: [],
    tradingWallet: null,
    copyWalletAddress: null,
    copyWalletAddresses: [],
    copyAmountSol: null,
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
  assert.equal(await store.addMyWallet("chat-1", otherWallet, "My Alpha"), true);
  assert.equal(await store.addMyWallet("chat-1", wallet, "My Beta"), true);
  assert.equal(await store.renameMyWallet("chat-1", wallet, null), true);
  assert.equal(await store.renameMyWallet("chat-1", "missing", "Nope"), false);
  assert.equal(
    await store.setTradingWallet("chat-1", {
      publicKey: otherWallet,
      encryptedApiKey: "encrypted-api-key",
      apiKeyLast4: "ikey",
      createdAt: "2026-05-23T00:00:00.000Z",
      updatedAt: "2026-05-23T00:00:00.000Z"
    }),
    true
  );
  assert.equal(await store.setTradingWallet("chat-2", store.getTradingWallet("chat-1")), false);
  assert.equal(await store.watchCopyTradeWallet("chat-1", otherWallet, "Copy Alpha"), true);
  assert.equal(await store.watchCopyTradeWallet("chat-1", wallet, "Copy Beta"), true);
  assert.equal(await store.renameCopyTradeWallet("chat-1", wallet, null), true);
  assert.equal(await store.renameCopyTradeWallet("chat-1", "missing", "Nope"), false);
  assert.equal(await store.setCopyWallet("chat-2", otherWallet), false);
  assert.equal(await store.setCopyAmountSol("chat-2", 0.25), false);
  assert.equal(await store.addMyWallet("chat-2", wallet, "Unverified"), false);
  assert.equal(await store.watchCopyTradeWallet("chat-2", wallet, "Unverified"), false);
  assert.equal(store.get("chat-1")?.mode, "newtokens");
  assert.equal(store.get("chat-1")?.copyWalletAddress, null);
  assert.deepEqual(store.get("chat-1")?.copyWalletAddresses, []);
  assert.deepEqual(store.listCopyWallets("chat-1"), []);
  assert.deepEqual(
    store.listMyWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [
      [otherWallet, "My Alpha"],
      [wallet, null]
    ]
  );
  assert.equal(store.get("chat-1")?.copyAmountSol, 0.25);
  assert.equal(store.getTradingWallet("chat-1")?.publicKey, otherWallet);
  assert.equal(store.getTradingWallet("chat-1")?.apiKeyLast4, "ikey");
  assert.deepEqual(
    store.listCopyTradeWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [
      [otherWallet, "Copy Alpha"],
      [wallet, null]
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
  assert.deepEqual(
    reloaded.listMyWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [
      [otherWallet, "My Alpha"],
      [wallet, null]
    ]
  );
  assert.equal(reloaded.get("chat-1")?.copyAmountSol, 0.25);
  assert.equal(reloaded.getTradingWallet("chat-1")?.publicKey, otherWallet);
  assert.deepEqual(
    reloaded.listCopyTradeWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [
      [otherWallet, "Copy Alpha"],
      [wallet, null]
    ]
  );

  assert.equal(await reloaded.unwatchWallet("chat-1", wallet), true);
  assert.deepEqual(reloaded.listWatchedWallets("chat-1"), []);
  assert.deepEqual(reloaded.listCopyTradeWallets("chat-1").map((entry) => entry.address), [otherWallet, wallet]);
  assert.equal(await reloaded.unwatchCopyTradeWallet("chat-1", wallet), true);
  assert.deepEqual(reloaded.listCopyTradeWallets("chat-1").map((entry) => entry.address), [otherWallet]);
  assert.equal(await reloaded.removeMyWallet("chat-1", wallet), true);
  assert.deepEqual(reloaded.listMyWallets("chat-1").map((entry) => entry.address), [otherWallet]);
  await reloaded.remove("chat-1");
  assert.equal(reloaded.has("chat-1"), false);
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
