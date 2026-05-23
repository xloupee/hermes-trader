import assert from "node:assert/strict";
import test from "node:test";
import { createSupabaseSubscriberStore, importSubscribersToSupabase } from "../dist/subscribers-supabase.js";

const wallet = "Wallet111111111111111111111111111111111111111";
const otherWallet = "Other111111111111111111111111111111111111111";

class MemorySubscriberRepository {
  constructor(records = []) {
    this.subscribers = new Map();
    this.wallets = new Map();

    for (const record of records) {
      this.subscribers.set(record.chatId, {
        ...record,
        watchedWallets: []
      });

      for (const walletRecord of record.watchedWallets || []) {
        this.wallets.set(`${record.chatId}:${walletRecord.address}`, {
          chatId: record.chatId,
          ...walletRecord
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
          .map(({ chatId: _chatId, ...walletRecord }) => ({ ...walletRecord }))
      }))
      .sort((left, right) => left.chatId.localeCompare(right.chatId));
  }

  async upsertSubscriber(subscriber) {
    const existing = this.subscribers.get(subscriber.chatId);
    this.subscribers.set(subscriber.chatId, {
      ...subscriber,
      watchedWallets: existing?.watchedWallets || []
    });
  }

  async deleteSubscriber(chatId) {
    this.subscribers.delete(chatId);

    for (const key of this.wallets.keys()) {
      if (key.startsWith(`${chatId}:`)) {
        this.wallets.delete(key);
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
    copyWalletAddress: null,
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
  assert.equal(await store.setCopyAmountSol("chat-1", 0.25), true);
  assert.equal(await store.setCopyTargetWallet("chat-1", otherWallet), false);
  assert.equal(await store.setCopyTargetWallet("chat-1", wallet), true);
  assert.equal(await store.setCopyWallet("chat-2", otherWallet), false);
  assert.equal(await store.setCopyAmountSol("chat-2", 0.25), false);
  assert.equal(await store.setCopyTargetWallet("chat-2", wallet), false);
  assert.equal(store.get("chat-1")?.mode, "newtokens");
  assert.equal(store.get("chat-1")?.copyWalletAddress, otherWallet);
  assert.equal(store.get("chat-1")?.copyAmountSol, 0.25);
  assert.equal(store.get("chat-1")?.copyTargetWalletAddress, wallet);

  const reloaded = createSupabaseSubscriberStore({ repository });
  await reloaded.init();
  assert.deepEqual(
    reloaded.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
    [[wallet, null]]
  );
  assert.equal(reloaded.get("chat-1")?.copyWalletAddress, otherWallet);
  assert.equal(reloaded.get("chat-1")?.copyAmountSol, 0.25);
  assert.equal(reloaded.get("chat-1")?.copyTargetWalletAddress, wallet);

  assert.equal(await reloaded.unwatchWallet("chat-1", wallet), true);
  assert.deepEqual(reloaded.listWatchedWallets("chat-1"), []);
  assert.equal(reloaded.get("chat-1")?.copyTargetWalletAddress, null);
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
