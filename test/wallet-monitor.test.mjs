import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { createSubscriberStore } from "../dist/subscribers.js";
import {
  buildWalletTradeReplyMarkup,
  eventMentionsWatchedWallet,
  formatWalletTradeMessage,
  getWalletTradeEventId,
  isValidSolanaAddress,
  normalizeWalletTradeData
} from "../dist/wallet-monitor.js";

const wallet = "39azUYFWPz3VHgKCf3VChUwbpURdCHRxjWVowf5jUJjg";
const otherWallet = "62qc2CNXwrYqQScmEdiZFFAnJR262PxWEuNQtxfafNgV";
const mint = "So11111111111111111111111111111111111111112";
const config = {
  pumpFunBaseUrl: "https://pump.fun",
  solscanBaseUrl: "https://solscan.io"
};

test("subscriber store persists per-chat watched wallets with labels", async () => {
  const dir = await mkdtemp(join(tmpdir(), "pumpfunnoti-"));
  const path = join(dir, "subscribers.json");

  try {
    const store = createSubscriberStore({ path });
    await store.init();

    assert.equal(await store.watchWallet("chat-1", wallet, "Before verify"), false);

    await store.add("chat-1");
    assert.equal(await store.watchWallet("chat-1", wallet, "Alpha <Wallet>"), true);
    assert.deepEqual(
      store.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
      [[wallet, "Alpha <Wallet>"]]
    );

    const reloaded = createSubscriberStore({ path });
    await reloaded.init();
    assert.deepEqual(
      reloaded.listWatchedWallets("chat-1").map((entry) => [entry.address, entry.label]),
      [[wallet, "Alpha <Wallet>"]]
    );

    assert.equal(await reloaded.unwatchWallet("chat-1", wallet), true);
    assert.deepEqual(reloaded.listWatchedWallets("chat-1"), []);

    const body = JSON.parse(await readFile(path, "utf8"));
    assert.equal(body.subscribers[0].chatId, "chat-1");
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test("wallet monitor normalizes and formats matching PumpPortal account trades", () => {
  const event = {
    txType: "buy",
    traderPublicKey: wallet,
    mint,
    signature: "5VfUXexampleTxSignature111111111111111111111111111111111",
    solAmount: 0.125,
    tokenAmount: 250000,
    marketCapSol: 42,
    pool: "pump"
  };

  assert.equal(isValidSolanaAddress(wallet), true);
  assert.equal(eventMentionsWatchedWallet(event, wallet), true);
  assert.equal(eventMentionsWatchedWallet(event, otherWallet), false);

  const trade = normalizeWalletTradeData({
    event,
    targetWallet: wallet,
    label: "Alpha <Wallet>",
    config
  });

  assert.equal(trade.action, "buy");
  assert.equal(trade.mint, mint);
  assert.equal(trade.solAmount, 0.125);
  assert.equal(trade.tokenAmount, 250000);
  assert.equal(getWalletTradeEventId(trade), `wallet-trade:${trade.signature}:${wallet}:buy:${mint}`);
  assert.deepEqual(buildWalletTradeReplyMarkup(trade)?.inline_keyboard[0][0].copy_text, { text: mint });

  const message = formatWalletTradeMessage(trade);
  assert.match(message, /Wallet trade detected/);
  assert.match(message, /Alpha &lt;Wallet&gt;/);
  assert.match(message, /0.125 SOL/);
  assert.match(message, /Pool:<\/b> pump/);
});
